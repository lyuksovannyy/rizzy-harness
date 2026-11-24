//! FIX_PLAN 4.1, first half: every route the master serves answers 401 to a
//! caller with no credentials, bar a short written public list.
//!
//! **How the routes are found.** axum's `Router` cannot list its routes, and
//! a recording wrapper would only see the registrations that remember to use
//! it. So the test reads the source: it lexes every `.rs` file under
//! `crates/master/src` (comments and strings handled, not a regex over text)
//! and reads each `.route("path", get(a).post(b)...)` registration. The
//! parse is strict. A registration shape it does not know (`.nest`,
//! `.route_service`, `any(..)`, a path that is not a literal, an unknown
//! method helper, a `.merge` of anything but `module::routes()`, a second
//! `.fallback`) fails the test and names the file and line. A file with
//! registrations that `http.rs` does not reach through `.merge(m::routes())`
//! fails too (dead code, or a route this test cannot see).
//! `route_table_is_read_strictly` proves the parser on shapes written out
//! in the test, including the ones it must refuse.
//!
//! **What is asserted** for every non-public `(method, path)`: 401 with no
//! credentials, with an unknown bearer token, and with a well-formed unknown
//! session cookie and login key; 401 or 403 from a foreign Origin; 401 (not
//! 400, 413, 422 or 500) with an empty body, a 1 MB body of zeros and
//! malformed JSON, so the check of who asks comes before any body is read;
//! and 401, 404 or 405 for every one of GET, POST, PUT, PATCH and DELETE the
//! route does not register.
//!
//! Not covered here: the node link (its own listener and pinned keys:
//! `link_hardening`, `hostile_node`), what the answers contain once logged
//! in (each area's own test file), the WebSocket after the upgrade (`/api/ws`
//! is walked on its own below because it authenticates by its first
//! message).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use harness_core::Secret;
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, router};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::json;

use crate::common;
use common::SameOrigin as _;

// ---------------------------------------------------------------- lexer

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Id(String),
    Str(String),
    P(char),
}

type Toks = [(Tok, usize)];

/// Rust source as tokens with their lines: comments dropped, string
/// contents kept apart from code, lifetimes told from char literals.
fn lex(src: &str) -> Vec<(Tok, usize)> {
    let c: Vec<char> = src.chars().collect();
    let (mut i, mut line) = (0, 1);
    let mut out = Vec::new();
    while i < c.len() {
        let ch = c[i];
        if ch == '\n' {
            line += 1;
            i += 1;
        } else if ch.is_whitespace() {
            i += 1;
        } else if ch == '/' && c.get(i + 1) == Some(&'/') {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
        } else if ch == '/' && c.get(i + 1) == Some(&'*') {
            let mut depth = 0;
            while i < c.len() {
                if c[i] == '/' && c.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if c[i] == '*' && c.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    if c[i] == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
        } else if ch == '"' {
            let start = line;
            i += 1;
            let mut s = String::new();
            while i < c.len() && c[i] != '"' {
                if c[i] == '\\' {
                    i += 1;
                }
                if c[i] == '\n' {
                    line += 1;
                }
                s.push(c[i]);
                i += 1;
            }
            i += 1;
            out.push((Tok::Str(s), start));
        } else if ch == 'r'
            && matches!(c.get(i + 1), Some('"' | '#'))
            && raw_string(&c, i).is_some()
        {
            let (s, next) = raw_string(&c, i).unwrap();
            let start = line;
            line += s.matches('\n').count();
            i = next;
            out.push((Tok::Str(s), start));
        } else if ch == '\'' {
            // A char literal ('x', '\n') or a lifetime ('a).
            if c.get(i + 1) == Some(&'\\') {
                i += 2;
                while i < c.len() && c[i] != '\'' {
                    i += 1;
                }
                i += 1;
            } else if c.get(i + 2) == Some(&'\'') {
                i += 3;
            } else {
                i += 1;
            }
        } else if ch.is_alphanumeric() || ch == '_' {
            let st = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            out.push((Tok::Id(c[st..i].iter().collect()), line));
        } else {
            out.push((Tok::P(ch), line));
            i += 1;
        }
    }
    out
}

/// `r"..."` or `r#"..."#` at `i`: the contents and the index after it.
fn raw_string(c: &[char], i: usize) -> Option<(String, usize)> {
    let mut j = i + 1;
    let mut hashes = 0;
    while c.get(j) == Some(&'#') {
        hashes += 1;
        j += 1;
    }
    if c.get(j) != Some(&'"') {
        return None;
    }
    j += 1;
    let st = j;
    while j < c.len() {
        if c[j] == '"' && (0..hashes).all(|k| c.get(j + 1 + k) == Some(&'#')) {
            return Some((c[st..j].iter().collect(), j + 1 + hashes));
        }
        j += 1;
    }
    None
}

// -------------------------------------------------------- route reading

/// The index of the `)`, `]` or `}` that closes the group opened at `open`.
fn close_of(t: &Toks, open: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, (tok, _)) in t.iter().enumerate().skip(open) {
        match tok {
            Tok::P('(' | '[' | '{') => depth += 1,
            Tok::P(')' | ']' | '}') => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Helper functions in a `routes()` that build a method router, and what
/// they register. A new helper must be written here on purpose.
const HELPERS: &[(&str, &[&str])] = &[("image", &["PUT", "DELETE"])];

/// The methods of the handler expression `get(a).post(b).layer(c)`.
fn methods_of(args: &Toks) -> Result<Vec<String>, String> {
    let mut i = 0;
    let mut out = Vec::new();
    loop {
        let Some((Tok::Id(first), _)) = args.get(i) else {
            return Err("handler is not a call chain".into());
        };
        let mut name = first.clone();
        i += 1;
        // `axum::routing::put(..)`: the last segment names the method.
        while matches!(args.get(i), Some((Tok::P(':'), _))) {
            let (Some((Tok::P(':'), _)), Some((Tok::Id(n), _))) =
                (args.get(i + 1), args.get(i + 2))
            else {
                return Err("odd path in handler".into());
            };
            name = n.clone();
            i += 3;
        }
        if !matches!(args.get(i), Some((Tok::P('('), _))) {
            return Err(format!("`{name}` is not called"));
        }
        i = close_of(args, i).ok_or("unbalanced handler")? + 1;
        match name.as_str() {
            "get" | "post" | "put" | "patch" | "delete" | "head" | "options" | "trace" => {
                out.push(name.to_uppercase())
            }
            "layer" | "route_layer" => {}
            n => match HELPERS.iter().find(|(h, _)| *h == n) {
                Some((_, ms)) => out.extend(ms.iter().map(|m| m.to_string())),
                None => return Err(format!("unknown method helper `{n}` (any/on/closure?)")),
            },
        }
        match args.get(i) {
            None => return Ok(out),
            Some((Tok::P(','), _)) if i + 1 == args.len() => return Ok(out),
            Some((Tok::P('.'), _)) => i += 1,
            Some(_) => return Err("unexpected tokens after a handler".into()),
        }
    }
}

#[derive(Debug, Default)]
struct Table {
    /// path template -> methods
    routes: BTreeMap<String, BTreeSet<String>>,
}

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Read the registrations of one file's tokens into `t`; return the modules
/// it merges (`.merge(a::b::routes())`) as paths of segments.
fn read_file(
    file: &Path,
    toks: &Toks,
    t: &mut Table,
    problems: &mut Vec<String>,
) -> Vec<Vec<String>> {
    let mut merges = Vec::new();
    let mentions_router = toks.iter().any(|(k, _)| *k == Tok::Id("Router".into()));
    for i in 0..toks.len().saturating_sub(2) {
        let (Tok::P('.'), _) = &toks[i] else { continue };
        let (Tok::Id(name), line) = &toks[i + 1] else {
            continue;
        };
        if !matches!(toks[i + 2].0, Tok::P('(')) {
            continue;
        }
        let here = format!("{}:{line}", file.display());
        let Some(close) = close_of(toks, i + 2) else {
            problems.push(format!("{here}: unbalanced `.{name}(`"));
            continue;
        };
        let args = &toks[i + 3..close];
        match name.as_str() {
            "route" => {
                let (Some((Tok::Str(path), _)), Some((Tok::P(','), _))) =
                    (args.first(), args.get(1))
                else {
                    problems.push(format!("{here}: `.route` whose path is not a literal"));
                    continue;
                };
                match methods_of(&args[2..]) {
                    Ok(ms) if !ms.is_empty() => {
                        t.routes.entry(path.clone()).or_default().extend(ms)
                    }
                    Ok(_) => problems.push(format!("{here}: `{path}` registers no method")),
                    Err(e) => problems.push(format!("{here}: `{path}`: {e}")),
                }
            }
            "route_service" | "nest" | "nest_service" | "fallback_service" if mentions_router => {
                problems.push(format!(
                    "{here}: `.{name}(` is a shape this test cannot read"
                ))
            }
            "fallback" if mentions_router => {
                let ids: Vec<_> = args
                    .iter()
                    .filter_map(|(k, _)| match k {
                        Tok::Id(s) => Some(s.as_str()),
                        _ => None,
                    })
                    .collect();
                if ids != ["crate", "assets", "serve"] || args.len() != 7 {
                    problems.push(format!(
                        "{here}: a fallback other than crate::assets::serve (the SPA)"
                    ));
                }
            }
            "merge" if mentions_router => {
                // `a::b::routes()`
                let mut segs = Vec::new();
                let mut j = 0;
                let ok = loop {
                    match (args.get(j).map(|x| &x.0), args.get(j + 1).map(|x| &x.0)) {
                        (Some(Tok::Id(s)), Some(Tok::P(':'))) => {
                            segs.push(s.clone());
                            j += 3;
                        }
                        (Some(Tok::Id(s)), Some(Tok::P('('))) if s == "routes" => {
                            break j + 3 == args.len();
                        }
                        _ => break false,
                    }
                };
                // `local(..)`: a function of this same file, whose routes
                // this loop reads anyway (`http.rs`'s `public` and
                // `private`).
                let local = match args.first().map(|x| &x.0) {
                    Some(Tok::Id(f)) if matches!(args.get(1), Some((Tok::P('('), _))) => {
                        close_of(args, 1) == Some(args.len() - 1)
                            && toks.windows(2).any(|w| {
                                w[0].0 == Tok::Id("fn".into()) && w[1].0 == Tok::Id(f.clone())
                            })
                    }
                    _ => false,
                };
                if ok && !segs.is_empty() {
                    merges.push(segs);
                } else if !local {
                    problems.push(format!(
                        "{here}: `.merge` of something but `module::routes()`"
                    ));
                }
            }
            _ => {}
        }
    }
    merges
}

/// The module `segs` below file `from` (`http.rs` + `users` = `http/users.rs`).
fn module_file(from: &Path, segs: &[String]) -> Option<PathBuf> {
    let mut cur = from.to_path_buf();
    for s in segs {
        let base = cur.with_extension("");
        let a = base.join(format!("{s}.rs"));
        let b = base.join(s).join("mod.rs");
        cur = if a.exists() {
            a
        } else if b.exists() {
            b
        } else {
            return None;
        };
    }
    Some(cur)
}

/// Every registration the master's `router()` serves, or the problems.
fn read_table(src_dir: &Path) -> Result<Table, Vec<String>> {
    let mut files = Vec::new();
    rs_files(src_dir, &mut files);
    files.sort();
    let mut problems = Vec::new();
    let mut per_file: BTreeMap<PathBuf, (Table, Vec<Vec<String>>)> = BTreeMap::new();
    for f in &files {
        let toks = lex(&std::fs::read_to_string(f).unwrap());
        let mut t = Table::default();
        let merges = read_file(f, &toks, &mut t, &mut problems);
        per_file.insert(f.clone(), (t, merges));
    }
    // Follow the merges from http.rs; every file with routes must be reached.
    let root = src_dir.join("http.rs");
    let mut reached = BTreeSet::from([root.clone()]);
    let mut todo = vec![root];
    let mut all = Table::default();
    while let Some(f) = todo.pop() {
        let Some((t, merges)) = per_file.get(&f) else {
            problems.push(format!("{} is not a source file", f.display()));
            continue;
        };
        for (p, ms) in &t.routes {
            all.routes.entry(p.clone()).or_default().extend(ms.clone());
        }
        for m in merges {
            match module_file(&f, m) {
                Some(next) if reached.insert(next.clone()) => todo.push(next),
                Some(_) => {}
                None => problems.push(format!("{}: merged module {m:?} not found", f.display())),
            }
        }
    }
    for (f, (t, _)) in &per_file {
        if !t.routes.is_empty() && !reached.contains(f) {
            problems.push(format!(
                "{} registers routes but `http.rs` does not merge it: the walk cannot see them",
                f.display()
            ));
        }
    }
    if problems.is_empty() {
        Ok(all)
    } else {
        Err(problems)
    }
}

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

#[test]
fn route_table_is_read_strictly() {
    let m = |s: &str| {
        let toks = lex(s);
        let mut t = Table::default();
        let mut p = Vec::new();
        let merges = read_file(Path::new("t.rs"), &toks, &mut t, &mut p);
        (t, p, merges)
    };
    // Shapes in use: chains, a helper, layers, and comments and strings that
    // mention routes.
    let (t, p, _) = m(r#"use axum::Router;
        fn r() { Router::new()
            // .route("/no", get(x))
            .route("/a/{id}", get(h).post(axum::routing::post(h)).layer(L::new(1)))
            .route("/b", axum::routing::put(move |a: A| f(a, '\'', "x")).delete(d))
            .route("/c", image(Slot::A))
            .route("/d", get(|| async { "}.route(\"/z\", get(q))" })); }"#);
    assert!(p.is_empty(), "{p:?}");
    assert_eq!(
        t.routes.keys().map(String::as_str).collect::<Vec<_>>(),
        ["/a/{id}", "/b", "/c", "/d"]
    );
    assert_eq!(
        t.routes["/a/{id}"],
        BTreeSet::from(["GET".into(), "POST".into()])
    );
    assert_eq!(
        t.routes["/b"],
        BTreeSet::from(["PUT".into(), "DELETE".into()])
    );
    assert_eq!(
        t.routes["/c"],
        BTreeSet::from(["PUT".into(), "DELETE".into()])
    );
    // Shapes it must refuse, each with its reason.
    for (src, why) in [
        (r#".route("/a", any(h))"#, "any"),
        (r#".route(P, get(h))"#, "not a literal"),
        (r#".nest("/b", x)"#, "nest"),
        (r#".route("/a", handler_fn())"#, "unknown helper"),
        (r#".route("/a", my_methods)"#, "not called"),
        (r#".route_service("/a", s)"#, "route_service"),
        (r#".fallback(other)"#, "fallback"),
        (r#".merge(other())"#, "merge"),
    ] {
        let (_, p, _) = m(&format!(
            "use axum::Router; fn f() {{ Router::new(){src}; }}"
        ));
        assert_eq!(p.len(), 1, "{why}: {p:?}");
    }
    let (_, p, merges) = m("use axum::Router; fn f() { Router::new().merge(a::b::routes()) }");
    assert!(p.is_empty(), "{p:?}");
    assert_eq!(merges, [vec!["a".to_string(), "b".to_string()]]);
    // A function of the same file is read with the file; one from
    // elsewhere is not.
    let (_, p, merges) =
        m("use axum::Router; fn g(m: &M) -> Router { x } fn f() { Router::new().merge(g(&m)) }");
    assert!(p.is_empty() && merges.is_empty(), "{p:?}");
    let (_, p, _) = m("use axum::Router; fn f() { Router::new().merge(h(&m)) }");
    assert_eq!(p.len(), 1, "{p:?}");
}

/// The real tree reads cleanly and the walk sees a plausible table.
#[test]
fn the_real_route_table_is_readable() {
    let t = read_table(&src_dir()).unwrap_or_else(|p| panic!("\n{}\n", p.join("\n")));
    let pairs: usize = t.routes.values().map(BTreeSet::len).sum();
    assert!(
        t.routes.len() > 100 && pairs > 150,
        "{} {pairs}",
        t.routes.len()
    );
}

// ---------------------------------------------------------- the walk

/// Public on purpose. Every line says why, and each reason was read in the
/// handler. Anything else answering something but 401 is a finding.
const PUBLIC: &[(&str, &str, &str)] = &[
    (
        "GET",
        "/api/setup",
        "tells the page whether first-run setup is open (a bool); nothing else",
    ),
    (
        "POST",
        "/api/setup",
        "first-run claim: refused unless the claim code from the server's disk is sent, and shut once an admin exists",
    ),
    (
        "POST",
        "/api/login",
        "the login itself: username and password, throttled per name and address",
    ),
    (
        "POST",
        "/api/logout",
        "ends the session named by the caller's own cookie; without one it only clears the cookie",
    ),
    (
        "POST",
        "/api/hooks/{trigger_id}",
        "an incoming webhook: its own per-trigger bearer secret (hooks_http), never the API token; the module's checks run in a fixed order and the body limit comes first by design",
    ),
    (
        "GET",
        "/api/ws",
        "authenticates itself: a request that is not an upgrade is refused, one with no cookie and no token is 401, a cookie-only one waits for the login key as its first message (walked in `websocket_authenticates_itself`)",
    ),
];

/// FINDING 1 and 2 of the first half, fixed: routes that read the body
/// (`Json<T>`, `Bytes`, a limit) or a required query before the handler
/// asked who was calling. Every non-public route now sits behind
/// `http::caller::identify`, which runs before any of its handler's
/// extractors. A route that answers 400, 413, 415 or 422 without
/// credentials again is a finding like any other. The lists stay as the
/// place a known exception would be pinned, and must stay empty.
const BODY_FIRST: &[(&str, &str)] = &[];
const BODY_FIRST_CODES: &[u16] = &[400, 413, 415, 422];
const QUERY_FIRST: &[(&str, &str)] = &[];
const QUERY_FIRST_CODES: &[u16] = &[400];

fn fill(path: &str) -> String {
    let mut out = String::new();
    let mut rest = path;
    while let Some(a) = rest.find('{') {
        out.push_str(&rest[..a]);
        let b = rest[a..].find('}').unwrap() + a;
        let name = rest[a + 1..b].trim_start_matches('*');
        out.push_str(match name {
            "id" => "6f1c1e0a-3c1b-4b7e-9a55-0d2f4a7c9e11",
            "turn" | "rewind_seq" | "vid" => "1",
            _ => "x",
        });
        rest = &rest[b + 1..];
    }
    out.push_str(rest);
    out
}

struct Rig {
    base: String,
    _dir: tempfile::TempDir,
}

async fn start() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, "CLAIM").unwrap();
    let auth = Auth::new(
        Secret::new("api-secret".into()),
        vec![addr.to_string()],
        false,
    )
    .with_claim(Claim {
        code: Secret::new("CLAIM".into()),
        path: claim_path,
    });
    let master = Master::new(Store::open_in_memory().unwrap(), auth);
    tokio::spawn(async move { axum::serve(listener, router(master)).await.unwrap() });
    let base = format!("http://{addr}");
    // An admin exists, so the walk does not stand in front of an open setup.
    let r = reqwest::Client::new()
        .post(format!("{base}/api/setup"))
        .json(&json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    Rig { base, _dir: dir }
}

const NO_BODY: [&str; 5] = ["GET", "POST", "PUT", "PATCH", "DELETE"];

#[tokio::test]
async fn every_route_answers_401_without_credentials() {
    let table = read_table(&src_dir()).unwrap_or_else(|p| panic!("\n{}\n", p.join("\n")));
    let rig = start().await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let hex = "ab".repeat(32);
    let cookie = format!("{}={hex}", harness_master::auth::cookie_name());
    let zeros = vec![0u8; 1 << 20];

    // Every public entry names a registered route (no stale line).
    for (m, p, _) in PUBLIC {
        assert!(
            table.routes.get(*p).is_some_and(|ms| ms.contains(*m)),
            "PUBLIC lists {m} {p}, which is not registered"
        );
    }

    let mut findings: Vec<(String, String, String, u16)> = Vec::new();
    let mut leaking: BTreeSet<(String, String)> = BTreeSet::new();
    let mut walked = 0;
    // `body_ok`: the variant is one a route that reads its input first may
    // answer for (not the foreign Origin, not an unregistered method).
    let mut check = |m: &str, p: &str, variant: &str, got: Option<StatusCode>, ok: &[u16]| {
        walked += 1;
        let code = got.map_or(0, |s| s.as_u16());
        if ok.contains(&code) {
            return;
        }
        let input_variant = !matches!(variant, "foreign-origin" | "unregistered-method");
        let class: &[u16] = if input_variant && BODY_FIRST.contains(&(m, p)) {
            BODY_FIRST_CODES
        } else if input_variant && QUERY_FIRST.contains(&(m, p)) {
            QUERY_FIRST_CODES
        } else {
            &[]
        };
        if class.contains(&code) {
            leaking.insert((m.into(), p.into()));
        } else {
            findings.push((m.into(), p.into(), variant.into(), code));
        }
    };
    let send = |m: &str, path: &str| {
        client.request(
            Method::from_bytes(m.as_bytes()).unwrap(),
            format!("{}{path}", rig.base),
        )
    };
    let st = |r: Result<reqwest::Response, reqwest::Error>| r.ok().map(|r| r.status());

    for (path, methods) in &table.routes {
        let url = fill(path);
        for m in methods {
            if PUBLIC.iter().any(|(pm, pp, _)| pm == m && pp == path) {
                continue;
            }
            let (m, path) = (m.as_str(), path.as_str());
            // No credentials at all.
            check(m, path, "none", st(send(m, &url).send().await), &[401]);
            // An unknown bearer token.
            check(
                m,
                path,
                "bearer",
                st(send(m, &url).bearer_auth("not-the-token").send().await),
                &[401],
            );
            // A well-formed cookie and login key nobody issued.
            check(
                m,
                path,
                "cookie",
                st(send(m, &url)
                    .header("cookie", &cookie)
                    .header(common::LOGIN_HEADER, &hex)
                    .same_origin()
                    .send()
                    .await),
                &[401],
            );
            // A page of another site: the Origin guard or the login check.
            check(
                m,
                path,
                "foreign-origin",
                st(send(m, &url)
                    .header("origin", "https://evil.example")
                    .send()
                    .await),
                &[401, 403],
            );
            if m != "GET" {
                for (name, body) in [
                    ("empty-body", Vec::new()),
                    ("1mb-zeros", zeros.clone()),
                    ("bad-json", b"{\"a\":[1,".to_vec()),
                ] {
                    check(
                        m,
                        path,
                        name,
                        st(send(m, &url)
                            .header("content-type", "application/json")
                            .body(body)
                            .send()
                            .await),
                        &[401],
                    );
                }
            }
        }
        for m in NO_BODY.iter().filter(|m| !methods.contains(**m)) {
            let r = st(send(m, &url).send().await);
            check(m, path, "unregistered-method", r, &[401, 404, 405]);
        }
    }

    // Unknown API paths are 404, not the SPA.
    for p in ["/api/no-such-route", "/api/"] {
        let r = st(send("GET", p).send().await);
        check("GET", p, "unknown-api-path", r, &[404]);
    }

    let unexpected: Vec<_> = findings
        .iter()
        .map(|(m, p, v, code)| format!("{m} {p} [{v}] answered {code}"))
        .collect();
    let stale: Vec<_> = BODY_FIRST
        .iter()
        .chain(QUERY_FIRST)
        .filter(|(m, p)| !leaking.contains(&(m.to_string(), p.to_string())))
        .map(|(m, p)| format!("{m} {p}"))
        .collect();
    eprintln!(
        "route walk: {} paths, {walked} requests",
        table.routes.len()
    );
    assert!(
        unexpected.is_empty() && stale.is_empty(),
        "\nFINDINGS (not 401, not public, not a pinned known finding):\n{}\nSTALE known entries (answer 401 now: delete them):\n{}\n",
        unexpected.join("\n"),
        stale.join("\n")
    );
}

/// `/api/ws` is the one route that authenticates after the upgrade for a
/// browser: no cookie and no token is a plain 401; a cookie nobody issued
/// is let in to wait for its key, and a wrong key gets it nothing and a
/// close (the answer for a live cookie is the same: R2-1).
#[tokio::test]
async fn websocket_authenticates_itself() {
    use futures::{SinkExt as _, StreamExt as _};
    use tokio_tungstenite::tungstenite::{Error, Message, client::IntoClientRequest as _};
    let rig = start().await;
    let url = rig.base.replace("http://", "ws://") + "/api/ws";
    let origin = rig.base.clone();

    // A plain GET is not an upgrade.
    let r = reqwest::get(format!("{}/api/ws", rig.base)).await.unwrap();
    assert!(
        r.status().is_client_error(),
        "plain GET /api/ws: {}",
        r.status()
    );

    // No cookie, no token.
    let mut req = url.as_str().into_client_request().unwrap();
    req.headers_mut().insert("origin", origin.parse().unwrap());
    match tokio_tungstenite::connect_async(req).await {
        Err(Error::Http(resp)) => assert_eq!(resp.status(), 401),
        other => panic!("no credentials must be refused with 401: {other:?}"),
    }

    // An unknown bearer token.
    let mut req = url.as_str().into_client_request().unwrap();
    req.headers_mut().insert("origin", origin.parse().unwrap());
    req.headers_mut()
        .insert("authorization", "Bearer nope".parse().unwrap());
    match tokio_tungstenite::connect_async(req).await {
        Err(Error::Http(resp)) => assert_eq!(resp.status(), 401),
        other => panic!("a wrong token must be refused with 401: {other:?}"),
    }

    // An unknown cookie waits for its key; a wrong key is signed out, and
    // the socket never carries an event.
    let hex = "cd".repeat(32);
    let mut req = url.as_str().into_client_request().unwrap();
    req.headers_mut().insert("origin", origin.parse().unwrap());
    req.headers_mut().insert(
        "cookie",
        format!("{}={hex}", harness_master::auth::cookie_name())
            .parse()
            .unwrap(),
    );
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    ws.send(Message::text(
        json!({"type": "login", "key": hex}).to_string(),
    ))
    .await
    .unwrap();
    while let Some(m) = ws.next().await {
        match m {
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(Message::Ping(_) | Message::Pong(_)) => {}
            Ok(other) => panic!("an unauthenticated socket was sent {other:?}"),
        }
    }
}

/// What an unauthenticated upload costs: nothing of its body is read. A
/// 1 GB `Content-Length` with `Expect: 100-continue` is answered 401 with
/// no `100 Continue` first (so a well-behaved client never sends the body),
/// and the server closes the connection instead of draining the rest; a
/// client that sends anyway gets the same 401 and the same close.
#[tokio::test]
async fn an_unauthenticated_upload_is_refused_unread() {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let rig = start().await;
    let addr = rig.base.trim_start_matches("http://").to_string();
    for (expect, sent) in [(true, 0usize), (false, 64 * 1024)] {
        let mut s = tokio::net::TcpStream::connect(&addr).await.unwrap();
        let mut head = format!(
            "POST /api/sessions HTTP/1.1\r\nHost: {addr}\r\ncontent-type: application/json\r\ncontent-length: 1000000000\r\n"
        );
        if expect {
            head.push_str("expect: 100-continue\r\n");
        }
        head.push_str("\r\n");
        s.write_all(head.as_bytes()).await.unwrap();
        s.write_all(&vec![b'{'; sent]).await.unwrap();
        // Read until the server closes; a server that waited for the rest
        // of the gigabyte would time out here.
        let mut got = Vec::new();
        let read = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let mut buf = [0u8; 4096];
            loop {
                match s.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => got.extend_from_slice(&buf[..n]),
                }
            }
        })
        .await;
        let text = String::from_utf8_lossy(&got);
        assert!(
            read.is_ok(),
            "expect={expect}: the server kept the connection open: {text}"
        );
        assert!(
            text.starts_with("HTTP/1.1 401"),
            "expect={expect}: not a bare 401: {text}"
        );
        assert!(!text.contains("100 Continue"), "expect={expect}: {text}");
    }
}

/// The layer takes a link (a live login's cookie and a URL minted for it,
/// no login key) only where the byte handlers take one today: a GET of a
/// linkable URL. A made file's download gets past it (and answers 404 for
/// a chat that does not exist, as before); a PUT to a linkable URL with
/// the same kind of link is 401 before its body is read.
#[tokio::test]
async fn a_link_counts_only_for_the_byte_routes() {
    let rig = start().await;
    let c = reqwest::Client::new();
    let r = c
        .post(format!("{}/api/login", rig.base))
        .json(&json!({"username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let cookie = r
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let key = r.json::<serde_json::Value>().await.unwrap()["login_key"]
        .as_str()
        .unwrap()
        .to_string();
    let made = format!("/api/sessions/{}/made/f1", fill("{id}"));
    let thumb = "/api/media/m1/thumb";
    let links: serde_json::Value = c
        .post(format!("{}/api/links", rig.base))
        .header("cookie", &cookie)
        .header(common::LOGIN_HEADER, &key)
        .same_origin()
        .json(&json!({"urls": [made, thumb]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let link = |u: &str| links["links"][u].as_str().unwrap().to_string();
    let r = c
        .get(format!("{}{}", rig.base, link(&made)))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(
        r.status(),
        StatusCode::NOT_FOUND,
        "the link got past the layer"
    );
    // The same link on another URL is nobody.
    let other = link(&made).replace("/f1?", "/f2?");
    let r = c
        .get(format!("{}{other}", rig.base))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    let r = c
        .put(format!("{}{}", rig.base, link(thumb)))
        .header("cookie", &cookie)
        .same_origin()
        .body(vec![0u8; 1 << 20])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "a link never writes");
}
