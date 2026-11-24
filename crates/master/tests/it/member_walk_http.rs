//! FIX_PLAN 4.1: what each kind of signed-in caller may do, walked over
//! every route the route walk reads (`route_walk_http`), and the proof
//! that the caller layer (`http::caller::identify`) decides as each
//! handler's own `require*` did.
//!
//! **What a route demands is read from the code, not from its URL.** Each
//! registration's handler is resolved to its function (the same lexer as
//! the route walk), and that function, with the functions of the same file
//! it calls, is searched for the checks: `require_admin` or `Admin` makes
//! it admin-only, `require_any` or `Caller` lets a login with a pending
//! password change in, `require_bytes`, `ByteCaller` or `link_principal`
//! take a link, `require_agent_editor` needs an admin for `?scope=server`.
//! A handler whose function the test cannot find, or that has none of
//! these checks, fails the test.
//!
//! **Known and pending (decision 2 of the design note):** a handler that
//! still calls `require*` by hand after its body or query extractor answers
//! a signed-in caller without the right (a member on an admin route, a
//! login that must change its password) with 400, 413, 415 or 422 before
//! the 403. Those handlers are found from their signatures, listed by the
//! walk, and turn into 403 as they convert to the extractors.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use harness_core::Secret;
use harness_core::policy::ExecLevel;
use harness_master::auth::{Auth, cookie_name, sha256_hex};
use harness_master::{Master, router};
use harness_store::{NewUser, Role, Store, UserPatch};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

use crate::common;
use crate::route_walk_http::{
    PUBLIC, Table, Tok, Toks, close_of, fill, lex, module_file, read_table, rs_files, src_dir,
};

// ------------------------------------------------------ reading the code

/// What a handler, with what it calls in its own file, checks.
#[derive(Clone, Debug, Default, PartialEq)]
struct Checks {
    /// `require` or `User`.
    user: bool,
    /// `require_admin` or `Admin`.
    admin: bool,
    /// `require_any` or `Caller`.
    any: bool,
    /// `require_bytes`, `ByteCaller` or `link_principal`.
    bytes: bool,
    /// `require_agent_editor`.
    editor: bool,
    /// Takes one of the extractors of `http::caller`.
    converted: bool,
    /// Takes a body or a query extractor (the handler itself only).
    input: bool,
}

impl Checks {
    fn or(&mut self, o: &Checks) {
        self.user |= o.user;
        self.admin |= o.admin;
        self.any |= o.any;
        self.bytes |= o.bytes;
        self.editor |= o.editor;
    }
    fn any_check(&self) -> bool {
        self.user || self.admin || self.any || self.bytes || self.editor
    }
}

struct FnDef {
    params: Vec<(Tok, usize)>,
    body: Vec<(Tok, usize)>,
}

/// Every free `fn` of a file by name: those at the top level, not the
/// methods of an `impl` or the functions of a nested module.
fn fns_of(toks: &Toks) -> HashMap<String, FnDef> {
    let mut out = HashMap::new();
    let mut i = 0;
    let mut depth = 0i32;
    while i + 1 < toks.len() {
        match toks[i].0 {
            Tok::P('{') => depth += 1,
            Tok::P('}') => depth -= 1,
            _ => {}
        }
        if depth != 0 {
            i += 1;
            continue;
        }
        if let (Tok::Id(f), Tok::Id(name)) = (&toks[i].0, &toks[i + 1].0)
            && f == "fn"
            && let Some(open) = (i + 2..toks.len()).find(|&j| toks[j].0 == Tok::P('('))
            && let Some(close) = close_of(toks, open)
        {
            let after =
                (close + 1..toks.len()).find(|&j| matches!(toks[j].0, Tok::P('{') | Tok::P(';')));
            let body = match after {
                Some(b) if toks[b].0 == Tok::P('{') => {
                    let end = close_of(toks, b).unwrap_or(b);
                    toks[b..=end].to_vec()
                }
                _ => Vec::new(),
            };
            out.entry(name.clone()).or_insert(FnDef {
                params: toks[open + 1..close].to_vec(),
                body,
            });
        }
        i += 1;
    }
    out
}

/// The last path segment of each parameter's type: `State(m): State<M>`
/// is `State`, `b: axum::body::Bytes` is `Bytes`.
fn param_types(params: &Toks) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut in_type = false;
    let mut cur: Option<String> = None;
    let mut i = 0;
    while i < params.len() {
        match &params[i].0 {
            Tok::P('(' | '<' | '[') => {
                depth += 1;
                if in_type {
                    in_type = false;
                    out.extend(cur.take());
                }
            }
            Tok::P(')' | '>' | ']') => depth -= 1,
            Tok::P(',') if depth == 0 => {
                if in_type {
                    out.extend(cur.take());
                }
                in_type = false;
            }
            Tok::P(':') if depth == 0 && !in_type => {
                if params.get(i + 1).map(|t| &t.0) == Some(&Tok::P(':')) {
                    i += 1;
                } else {
                    in_type = true;
                    cur = None;
                }
            }
            Tok::Id(s) if in_type && depth == 0 && s != "mut" => cur = Some(s.clone()),
            _ => {}
        }
        i += 1;
    }
    if in_type {
        out.extend(cur);
    }
    out
}

const BODY_OR_QUERY: &[&str] = &[
    "Json",
    "Bytes",
    "Multipart",
    "Form",
    "Body",
    "Request",
    "String",
    "Query",
    "Result",
    "Option",
];

/// Calls `name(` that are not method calls (`a.require(..)` is not ours)
/// and lie on the function's straight-line path, so a dead check does not
/// count. A lexer's reading of "straight-line": a call inside a block
/// opened by `if`, `else`, `while`, `for`, `loop` or a closure (`|..|`)
/// does not count, nor one after a `return` at the top of the body. A
/// `match` arm counts (the helpers choose there: `require_agent_editor`).
/// What a lexer cannot see (a branch that never runs because of a value,
/// a check inside a `match` arm that is never taken) is left to the golden
/// file and the member walk, which ask the server.
fn calls(body: &Toks) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    // One entry per open block: whether the code in it always runs.
    let mut live: Vec<bool> = Vec::new();
    let mut returned = false;
    for i in 0..body.len() {
        match &body[i].0 {
            Tok::P('{') => {
                let parent = live.last().copied().unwrap_or(true);
                let gate = !live.is_empty() && opens_a_branch(body, i);
                live.push(parent && !gate);
            }
            Tok::P('}') => {
                live.pop();
            }
            Tok::Id(r) if r == "return" && live.len() == 1 => returned = true,
            Tok::Id(n)
                if !returned
                    && live.iter().all(|l| *l)
                    && body.get(i + 1).map(|t| &t.0) == Some(&Tok::P('('))
                    && (i == 0 || body[i - 1].0 != Tok::P('.')) =>
            {
                out.insert(n.clone());
            }
            _ => {}
        }
    }
    out
}

/// Whether the `{` at `open` starts a block that may not run: the
/// statement it ends has `if`, `else`, `while`, `for`, `loop` or a
/// closure's `|` outside any parentheses.
fn opens_a_branch(t: &Toks, open: usize) -> bool {
    let mut depth = 0i32;
    for j in (0..open).rev() {
        match &t[j].0 {
            Tok::P(')' | ']') => depth += 1,
            Tok::P('(' | '[') => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            Tok::P(';' | '{' | '}') if depth == 0 => return false,
            Tok::P('|') if depth == 0 => return true,
            Tok::Id(k)
                if depth == 0 && ["if", "else", "while", "for", "loop"].contains(&k.as_str()) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

/// The names a file brings in with `use a::b::name;` or
/// `use a::b::{x, y};` (one level of braces), as their full paths.
fn imports_of(toks: &Toks) -> HashMap<String, Vec<String>> {
    let mut out = HashMap::new();
    let mut i = 0;
    while i < toks.len() {
        if toks[i].0 != Tok::Id("use".into()) {
            i += 1;
            continue;
        }
        let mut prefix = Vec::new();
        i += 1;
        while let Some((t, _)) = toks.get(i) {
            match t {
                Tok::Id(n) => {
                    prefix.push(n.clone());
                    i += 1;
                }
                Tok::P(':') => i += 1,
                Tok::P('{') => {
                    let end = close_of(toks, i).unwrap_or(i);
                    for (t, _) in &toks[i + 1..end] {
                        if let Tok::Id(n) = t
                            && n != "self"
                        {
                            let mut full = prefix.clone();
                            full.push(n.clone());
                            out.insert(n.clone(), full);
                        }
                    }
                    i = end + 1;
                    break;
                }
                _ => {
                    if let Some(last) = prefix.last() {
                        out.insert(last.clone(), prefix.clone());
                    }
                    break;
                }
            }
        }
    }
    out
}

const LEAVES: &[&str] = &["require", "require_any", "require_admin", "require_bytes"];

struct Code {
    files: HashMap<PathBuf, HashMap<String, FnDef>>,
    imports: HashMap<PathBuf, HashMap<String, Vec<String>>>,
    memo: HashMap<(PathBuf, String), Checks>,
}

impl Code {
    fn read() -> Self {
        let mut list = Vec::new();
        rs_files(&src_dir(), &mut list);
        let mut files = HashMap::new();
        let mut imports = HashMap::new();
        for f in list {
            let toks = lex(&std::fs::read_to_string(&f).unwrap());
            files.insert(f.clone(), fns_of(&toks));
            imports.insert(f, imports_of(&toks));
        }
        Code {
            files,
            imports,
            memo: HashMap::new(),
        }
    }

    /// The file and function a handler path names, seen from `from`.
    fn resolve(&self, from: &Path, segs: &[String]) -> Option<(PathBuf, String)> {
        let (name, module) = segs.split_last()?;
        let file = match module.first().map(String::as_str) {
            None => from.to_path_buf(),
            Some("crate") => module_file(&src_dir().with_extension("rs"), &module[1..])?,
            Some("super") => {
                let up = from.parent()?.with_extension("rs");
                module_file(&up, &module[1..])?
            }
            Some(_) => module_file(from, module)?,
        };
        self.files.get(&file)?.get(name)?;
        Some((file, name.clone()))
    }

    fn checks(&mut self, file: &Path, name: &str, seen: &mut BTreeSet<String>) -> Checks {
        let key = (file.to_path_buf(), name.to_string());
        if let Some(c) = self.memo.get(&key) {
            return c.clone();
        }
        if !seen.insert(name.to_string()) {
            return Checks::default();
        }
        let Some(def) = self.files.get(file).and_then(|f| f.get(name)) else {
            return Checks::default();
        };
        let types = param_types(&def.params);
        let has = |t: &str| types.iter().any(|x| x == t);
        let called = calls(&def.body);
        let mut c = Checks {
            user: has("User") || called.contains("require"),
            admin: has("Admin") || called.contains("require_admin"),
            any: has("Caller") || called.contains("require_any"),
            bytes: has("ByteCaller")
                || called.contains("require_bytes")
                || called.contains("link_principal"),
            editor: called.contains("require_agent_editor"),
            converted: ["Caller", "User", "Admin", "ByteCaller"]
                .iter()
                .any(|t| has(t)),
            input: types.iter().any(|t| BODY_OR_QUERY.contains(&t.as_str())),
        };
        // What it calls in its own file, or brings in with `use` from
        // another (`use super::connections::person`): the helpers that
        // check for it.
        let local: Vec<(PathBuf, String)> = called
            .into_iter()
            // The checks themselves are leaves: `require_admin` calls
            // `require`, which calls `require_any`.
            .filter(|n| n != name && !LEAVES.contains(&n.as_str()))
            .filter_map(|n| {
                if self.files[file].contains_key(&n) {
                    Some((file.to_path_buf(), n))
                } else {
                    let segs = self.imports.get(file)?.get(&n)?.clone();
                    self.resolve(file, &segs)
                }
            })
            .collect();
        for (f, n) in local {
            let sub = self.checks(&f, &n, seen);
            c.or(&sub);
        }
        self.memo.insert(key, c.clone());
        c
    }
}

/// Each non-public route's checks, or what could not be read.
fn classify(t: &Table) -> (BTreeMap<(String, String), Checks>, Vec<String>) {
    let mut code = Code::read();
    let mut out = BTreeMap::new();
    let mut problems = Vec::new();
    for ((m, p), (file, names)) in &t.handlers {
        if PUBLIC.iter().any(|(pm, pp, _)| pm == m && pp == p) {
            continue;
        }
        let mut c = Checks::default();
        let mut found = false;
        for segs in names {
            if let Some((f, n)) = code.resolve(file, segs) {
                found = true;
                let sub = code.checks(&f, &n, &mut BTreeSet::new());
                c.or(&sub);
                c.converted |= sub.converted;
                c.input |= sub.input;
            }
        }
        if !found {
            problems.push(format!("{m} {p}: no handler function found for {names:?}"));
        } else if !c.any_check() {
            problems.push(format!("{m} {p}: the handler checks no caller"));
        }
        out.insert((m.clone(), p.clone()), c);
    }
    (out, problems)
}

/// The code reads as expected on the shapes in use.
#[test]
fn handlers_are_read_from_the_code() {
    assert_eq!(
        param_types(&lex(
            "State(m): State<Master>, headers: HeaderMap, Path((a, b)): Path<(String, i64)>, b: axum::body::Bytes, Json(x): Json<Value>"
        )),
        ["State", "HeaderMap", "Path", "Bytes", "Json"]
    );
    // A dead check does not count.
    let live = |src: &str| calls(&lex(src));
    assert!(live("{ require_admin(m, h)?; }").contains("require_admin"));
    assert!(
        live("{ let w = match a { X => require_admin(m, h), _ => y }; }").contains("require_admin")
    );
    for dead in [
        "{ if false { require_admin(m, h)?; } }",
        "{ if x { a } else { require_admin(m, h)?; } }",
        "{ let f = |m| { require_admin(m, h) }; }",
        "{ return ok(); require_admin(m, h)?; }",
        "{ for x in y { require_admin(m, h)?; } }",
    ] {
        assert!(!live(dead).contains("require_admin"), "{dead}");
    }
    let t = read_table(&src_dir()).unwrap_or_else(|p| panic!("{p:?}"));
    let (c, problems) = classify(&t);
    assert!(problems.is_empty(), "\n{}\n", problems.join("\n"));
    let at = |m: &str, p: &str| c[&(m.to_string(), p.to_string())].clone();
    // Direct, delegated (`put_test` calls `put_provider`), converted, and
    // a closure's callee (`image(..)` in profile.rs).
    assert!(at("GET", "/api/admin/storage").admin);
    assert!(at("GET", "/api/admin/storage").converted);
    assert!(at("PUT", "/api/admin/providers/test").admin);
    assert!(at("PUT", "/api/admin/providers/test").input);
    assert!(at("GET", "/api/me").any && !at("GET", "/api/me").admin);
    assert!(at("PUT", "/api/me/avatar").user);
    assert!(at("GET", "/api/users/{id}/avatar").bytes);
    assert!(at("GET", "/api/favicon").bytes);
    assert!(at("PATCH", "/api/agents/{id}").editor);
    // An editor route takes `User`, but its admin check is the helper's:
    // read as converted, not as admin-first.
    let e = at("PATCH", "/api/agents/{id}");
    assert!(e.converted && e.user && e.editor && !e.admin);
}

// ---------------------------------------------------------------- rigs

const TOKEN: &str = "api-secret";

#[derive(Clone)]
struct Login {
    cookie: String,
    key: String,
}

struct Rig {
    base: String,
    store: Store,
    _dir: tempfile::TempDir,
}

/// A user and a login for them, made in the store; `seed` makes the
/// cookie, so two rigs can hold the same values.
fn user(store: &Store, name: &str, role: Role, must_change: bool) -> String {
    let id = harness_core::new_id();
    store
        .create_user(&NewUser {
            user_id: id.clone(),
            username: name.into(),
            password_hash: "not a hash: these logins are made in the store".into(),
            role,
            exec_ceiling: ExecLevel::None,
            must_change_password: must_change,
        })
        .unwrap();
    id
}

fn login(store: &Store, uid: &str, seed: u8, expires_in_ms: i64) -> Login {
    let cookie = format!("{seed:02x}").repeat(32);
    let key = format!("{:02x}", seed.wrapping_add(1)).repeat(32);
    let now = harness_core::now_ms();
    store
        .ui_session_create(
            &sha256_hex(&cookie),
            &sha256_hex(&key),
            uid,
            now,
            now + expires_in_ms,
        )
        .unwrap();
    Login { cookie, key }
}

struct People {
    admin: Login,
    member: Login,
    must_change: Login,
    disabled: Login,
    expired: Login,
}

async fn start(app: fn(Master) -> axum::Router) -> (Rig, People) {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let auth = Auth::new(Secret::new(TOKEN.into()), vec![addr.to_string()], false);
    let master = Master::new(Store::open_in_memory().unwrap(), auth);
    let store = master.inner_mirror();
    let day = 24 * 3600 * 1000;
    let a = user(&store, "alex", Role::Admin, false);
    let m = user(&store, "mo", Role::Member, false);
    let c = user(&store, "chang", Role::Member, true);
    let d = user(&store, "dis", Role::Member, false);
    let people = People {
        admin: login(&store, &a, 0x10, day),
        member: login(&store, &m, 0x20, day),
        must_change: login(&store, &c, 0x30, day),
        disabled: login(&store, &d, 0x40, day),
        expired: login(&store, &m, 0x50, -1000),
    };
    // Disabled after the login was made: the row is still there.
    store
        .update_user(
            &d,
            &UserPatch {
                role: None,
                disabled: Some(true),
            },
        )
        .unwrap();
    let app = app(master);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (
        Rig {
            base: format!("http://{addr}"),
            store,
            _dir: dir,
        },
        people,
    )
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

#[derive(Clone)]
enum As {
    Nobody,
    Bearer(&'static str),
    Cookie(Login),
    /// A cookie and no key (the link is in the URL).
    CookieOnly(Login),
    Foreign,
}

// The class lists below (ADMIN_ONLY, ANY_LOGIN, BYTE_ROUTES) and VALID_BODIES
// are written by hand on purpose. An edit to one needs a line of reason in
// the pull request: a route that moves between classes changes who may call
// it. Nothing checks that a list and a handler were not changed together;
// for a route that takes a body whose valid body is missing from
// VALID_BODIES, the lists are the only guard (the golden file's bodies are
// empty, `{}` or a small object, so they stop at a body error before the
// role check; `the_written_lists_hold_over_http` sends the valid ones).
/// A body each admin-only route parses (its required fields, read from
/// its body type), so a member's request gets past the body extractor to
/// the handler's own admin check, which then refuses it before acting.
const VALID_BODIES: &[(&str, &str, &str)] = &[
    ("PATCH", "/api/admin/workspaces/{id}", r#"{"owner":"x"}"#),
    (
        "POST",
        "/api/admin/providers/models/test",
        r#"{"model":"x"}"#,
    ),
    ("POST", "/api/admin/providers/test", r#"{"preset":"x"}"#),
    ("POST", "/api/agent-summary", r#"{"prompt":"x"}"#),
    ("POST", "/api/users", r#"{"username":"x"}"#),
    ("PUT", "/api/admin/memory", r#"{"recall":"keywords"}"#),
    ("PUT", "/api/admin/pricing", r#"{"currency":"x"}"#),
    ("PUT", "/api/admin/providers/demo", r#"{"preset":"x"}"#),
    ("PUT", "/api/admin/providers/test", r#"{"preset":"x"}"#),
    ("PUT", "/api/admin/providers/{id}", r#"{"preset":"x"}"#),
    (
        "PUT",
        "/api/admin/workspaces/policy",
        r#"{"members_can_create":true}"#,
    ),
    ("PUT", "/api/users/{id}/exec-ceiling", r#"{"level":"none"}"#),
    ("POST", "/api/agents", NEW_AGENT),
    ("PUT", "/api/admin/agent-models", "{}"),
    ("PUT", "/api/admin/agents/{id}/avatar", "x"),
    ("PUT", "/api/admin/savings", "{}"),
    ("PUT", "/api/admin/storage", "{}"),
    ("PATCH", "/api/users/{id}", "{}"),
    ("PATCH", "/api/agents/{id}", r#"{"name":"x"}"#),
    ("POST", "/api/me/password", r#"{"current":"x","new":"y"}"#),
    (
        "POST",
        "/api/sessions/{id}/approval-mode",
        r#"{"mode":"ask"}"#,
    ),
    (
        "POST",
        "/api/sessions/{id}/approvals/{approval_id}",
        r#"{"decision":"deny"}"#,
    ),
    (
        "POST",
        "/api/sessions/{id}/step-limit",
        r#"{"max_steps":5}"#,
    ),
];

const NEW_AGENT: &str = "{\"id\":\"x\",\"name\":\"x\",\"prompt\":\"x\",\"features\":{\"memory\":false,\"project_memory\":false,\"personalisation\":false,\"history\":false,\"recall\":false,\"attached_files\":false,\"office\":false,\"board\":false,\"push\":false,\"pull_requests\":false,\"web\":false,\"read_files\":false,\"edit_files\":false,\"commands\":false,\"read_only_commands\":false,\"mcp\":false,\"skills\":false,\"delegate\":false,\"background_subagents\":false,\"as_agent\":false,\"as_subagent\":false}}";

const SMALL_JSON: &[u8] = br#"{"name":"x"}"#;

/// What the written lists promise, asked over HTTP with the valid bodies,
/// without reading any handler for the expectation (so a handler that
/// changes its role fails here even if its list line is changed with it):
/// a member gets 403 on every admin-only route, a login that must change
/// its password gets 403 "change your password first" everywhere but on
/// the any-login routes, and the any-login routes let it in.
#[tokio::test]
async fn the_written_lists_hold_over_http() {
    let t = read_table(&src_dir()).unwrap_or_else(|p| panic!("{p:?}"));
    let (class, _) = classify(&t);
    let all = pairs(&t);
    // Every admin-only or any-login route that takes a body has one.
    let mut bodiless: Vec<String> = Vec::new();
    for (m, p) in &all {
        let listed = ADMIN_ONLY.iter().any(|(lm, lp)| lm == m && lp == p)
            || ANY_LOGIN.iter().any(|(lm, lp)| lm == m && lp == p);
        let writes = matches!(m.as_str(), "POST" | "PUT" | "PATCH");
        let has = VALID_BODIES.iter().any(|(vm, vp, _)| vm == m && vp == p);
        if listed && writes && class[&(m.clone(), p.clone())].input && !has {
            bodiless.push(format!("{m} {p}"));
        }
    }
    assert!(
        bodiless.is_empty(),
        "admin-only and any-login routes with a body and no VALID_BODIES entry:\n{}",
        bodiless.join("\n")
    );
    let (rig, people) = start(router).await;
    let mut findings = Vec::new();
    let editor = "only the server's admins change the server's agents";
    for (m, p) in &all {
        let (m, p) = (m.as_str(), p.as_str());
        let url = fill(p);
        let body: Option<&[u8]> = match VALID_BODIES.iter().find(|(vm, vp, _)| *vm == m && *vp == p)
        {
            Some((_, _, b)) => Some(b.as_bytes()),
            None if m == "GET" => None,
            None => Some(b""),
        };
        let admin = ADMIN_ONLY.iter().any(|(lm, lp)| *lm == m && *lp == p);
        let any = ANY_LOGIN.iter().any(|(lm, lp)| *lm == m && *lp == p);
        // A member, on the agent editor's routes asking for the server's.
        let target = if admin && p.contains("/agent") && !p.starts_with("/api/admin/") {
            format!("{url}?scope=server")
        } else {
            url.clone()
        };
        if admin {
            let (code, err) =
                send(&rig, &As::Cookie(people.member.clone()), m, &target, body).await;
            if code != 403 || (err != ADMINS_ONLY && err != editor) {
                findings.push(format!("member {m} {p}: {code} {err}"));
            }
        }
        let (code, err) = send(
            &rig,
            &As::Cookie(people.must_change.clone()),
            m,
            &target,
            body,
        )
        .await;
        if any {
            if code == 401 || err == CHANGE_FIRST {
                findings.push(format!("must-change {m} {p}: {code} {err}: meant to work"));
            }
        } else if code != 403 || err != CHANGE_FIRST {
            findings.push(format!("must-change {m} {p}: {code} {err}"));
        }
    }
    assert!(findings.is_empty(), "\n{}\n", findings.join("\n"));
}

async fn send(rig: &Rig, who: &As, m: &str, url: &str, body: Option<&[u8]>) -> (u16, String) {
    let mut rb = client().request(
        Method::from_bytes(m.as_bytes()).unwrap(),
        format!("{}{url}", rig.base),
    );
    rb = match who {
        As::Nobody => rb,
        As::Bearer(t) => rb.bearer_auth(t),
        As::Cookie(l) => rb
            .header("cookie", format!("{}={}", cookie_name(), l.cookie))
            .header(common::LOGIN_HEADER, &l.key)
            .header("origin", &rig.base),
        As::CookieOnly(l) => rb
            .header("cookie", format!("{}={}", cookie_name(), l.cookie))
            .header("origin", &rig.base),
        As::Foreign => rb.header("origin", "https://evil.example"),
    };
    if let Some(b) = body {
        rb = rb
            .header("content-type", "application/json")
            .body(b.to_vec());
    }
    match rb.send().await {
        Ok(r) => {
            let code = r.status().as_u16();
            let text = r.text().await.unwrap_or_default();
            let error = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v["error"].as_str().map(String::from))
                .unwrap_or_default();
            (code, error)
        }
        Err(_) => (0, String::new()),
    }
}

/// Links for `urls`, minted for `who`'s login.
async fn mint(rig: &Rig, who: &Login, urls: &[String]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for chunk in urls.chunks(100) {
        let r: Value = client()
            .post(format!("{}/api/links", rig.base))
            .header("cookie", format!("{}={}", cookie_name(), who.cookie))
            .header(common::LOGIN_HEADER, &who.key)
            .header("origin", &rig.base)
            .json(&json!({ "urls": chunk }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        for (u, l) in r["links"].as_object().unwrap() {
            out.insert(u.clone(), l.as_str().unwrap().to_string());
        }
    }
    out
}

/// A URL with a link: its own when one was minted for it, else a link
/// minted for another URL, carried over.
fn linked(links: &BTreeMap<String, String>, url: &str, other: &str) -> String {
    match links.get(url) {
        Some(l) => l.clone(),
        None => {
            let q = &other[other.find('?').unwrap() + 1..];
            let sep = if url.contains('?') { '&' } else { '?' };
            format!("{url}{sep}{q}")
        }
    }
}

const BODY_CODES: &[u16] = &[400, 413, 415, 422];
const CHANGE_FIRST: &str = "change your password first";
const ADMINS_ONLY: &str = "admins only";

fn pairs(t: &Table) -> Vec<(String, String)> {
    t.routes
        .iter()
        .flat_map(|(p, ms)| ms.iter().map(move |m| (m.clone(), p.clone())))
        .filter(|(m, p)| !PUBLIC.iter().any(|(pm, pp, _)| pm == m && pp == p))
        .collect()
}

// ---------------------------------------------------------- the walks

/// Routes under `/api/admin/` that a member may call, each with the
/// reason read in the handler: none. Anything there that the code does not
/// reserve for admins is a finding.
const ADMIN_LOOKING_BUT_OPEN: &[(&str, &str, &str)] = &[];

/// Every route the code reserves for server admins (`require_admin`,
/// `Admin`, or `require_agent_editor`'s `?scope=server`), as read today.
/// A route that loses its admin check when its handler converts, or a new
/// admin route, fails until this list is changed on purpose.
/// Every route that lets in a login whose password must still change
/// (`require_any` or `Caller`), as read today.
const ANY_LOGIN: &[(&str, &str)] = &[
    ("DELETE", "/api/sessions/{id}/allows/{grant}"),
    ("GET", "/api/me"),
    ("POST", "/api/me/password"),
    ("POST", "/api/sessions/{id}/approval-mode"),
    ("POST", "/api/sessions/{id}/approvals/{approval_id}"),
    ("POST", "/api/sessions/{id}/background/stop"),
    ("POST", "/api/sessions/{id}/background/{bg}/stop"),
    ("POST", "/api/sessions/{id}/cancel"),
    ("POST", "/api/sessions/{id}/step-limit"),
];

/// Every route that takes a link (`require_bytes`, `ByteCaller` or
/// `link_principal`), as read today.
const BYTE_ROUTES: &[(&str, &str)] = &[
    ("GET", "/api/artifacts/{id}/latest/download"),
    ("GET", "/api/artifacts/{id}/versions/{vid}/download"),
    ("GET", "/api/artifacts/{id}/versions/{vid}/preview"),
    ("GET", "/api/artifacts/{id}/versions/{vid}/thumb"),
    ("GET", "/api/favicon"),
    ("GET", "/api/media/{id}/content"),
    ("GET", "/api/media/{id}/thumb"),
    ("GET", "/api/sessions/{id}/made/{file_id}"),
    ("GET", "/api/users/{id}/avatar"),
    ("GET", "/api/users/{id}/banner"),
];

const ADMIN_ONLY: &[(&str, &str)] = &[
    ("DELETE", "/api/admin/nodes/{id}"),
    ("DELETE", "/api/admin/providers/demo"),
    ("DELETE", "/api/admin/providers/test"),
    ("DELETE", "/api/admin/providers/{id}"),
    ("DELETE", "/api/admin/providers/{id}/models/refresh"),
    ("DELETE", "/api/admin/providers/{id}/models/{raw}"),
    ("DELETE", "/api/admin/workspaces/{id}"),
    ("DELETE", "/api/agents/{id}"),
    ("DELETE", "/api/users/{id}"),
    ("GET", "/api/admin/graphify"),
    ("GET", "/api/admin/memory"),
    ("GET", "/api/admin/nodes"),
    ("GET", "/api/admin/providers"),
    ("GET", "/api/admin/providers/{id}/models"),
    ("GET", "/api/admin/savings"),
    ("GET", "/api/admin/storage"),
    ("GET", "/api/admin/usage"),
    ("GET", "/api/admin/workspaces"),
    ("GET", "/api/admin/workspaces/{id}/members"),
    ("GET", "/api/users"),
    ("PATCH", "/api/admin/workspaces/{id}"),
    ("PATCH", "/api/agents/{id}"),
    ("PATCH", "/api/users/{id}"),
    ("POST", "/api/admin/nodes/join"),
    ("POST", "/api/admin/providers/demo"),
    ("POST", "/api/admin/providers/models/test"),
    ("POST", "/api/admin/providers/test"),
    ("POST", "/api/admin/providers/{id}/models/refresh"),
    ("POST", "/api/agent-summary"),
    ("POST", "/api/agents"),
    ("POST", "/api/users"),
    ("POST", "/api/users/{id}/password"),
    ("PUT", "/api/admin/agent-models"),
    ("PUT", "/api/admin/agents/{id}/avatar"),
    ("PUT", "/api/admin/memory"),
    ("PUT", "/api/admin/pricing"),
    ("PUT", "/api/admin/providers/demo"),
    ("PUT", "/api/admin/providers/test"),
    ("PUT", "/api/admin/providers/{id}"),
    ("PUT", "/api/admin/savings"),
    ("PUT", "/api/admin/storage"),
    ("PUT", "/api/admin/workspaces/policy"),
    ("PUT", "/api/users/{id}/exec-ceiling"),
];

/// Findings of the member walk left open, each reported: none yet.
const KNOWN: &[(&str, &str, &str)] = &[];

#[tokio::test]
async fn each_kind_of_caller_gets_what_the_code_allows() {
    let t = read_table(&src_dir()).unwrap_or_else(|p| panic!("{p:?}"));
    let (class, problems) = classify(&t);
    assert!(problems.is_empty(), "\n{}\n", problems.join("\n"));
    let (rig, people) = start(router).await;
    let mut findings: Vec<String> = Vec::new();
    let mut pending: BTreeSet<String> = BTreeSet::new();
    // Admin-only routes where no body sent got past the body extractor to
    // the admin check (1c of the review): the check is not asked over HTTP.
    let mut unreached: BTreeSet<String> = BTreeSet::new();
    let mut note = |kind: &str, m: &str, p: &str, what: String| {
        if !KNOWN
            .iter()
            .any(|(k, km, kp)| *k == kind && *km == m && *kp == p)
        {
            findings.push(format!("[{kind}] {m} {p}: {what}"));
        }
    };

    // Module docs that speak of admins.
    let mut admin_doc: BTreeSet<PathBuf> = BTreeSet::new();
    let mut files = Vec::new();
    rs_files(&src_dir(), &mut files);
    for f in files {
        let src = std::fs::read_to_string(&f).unwrap();
        let doc: String = src
            .lines()
            .take_while(|l| l.starts_with("//!") || l.trim().is_empty())
            .collect();
        if doc.to_lowercase().contains("admin") {
            admin_doc.insert(f);
        }
    }

    // Each class read from the code must equal its written list.
    let mut moved = Vec::new();
    for (name, listed, pick) in [
        (
            "ADMIN_ONLY",
            ADMIN_ONLY,
            (|c: &Checks| c.admin || c.editor) as fn(&Checks) -> bool,
        ),
        ("ANY_LOGIN", ANY_LOGIN, |c: &Checks| c.any),
        ("BYTE_ROUTES", BYTE_ROUTES, |c: &Checks| c.bytes),
    ] {
        let derived: BTreeSet<(String, String)> = class
            .iter()
            .filter(|(_, c)| pick(c))
            .map(|(k, _)| k.clone())
            .collect();
        let listed: BTreeSet<(String, String)> = listed
            .iter()
            .map(|(m, p)| (m.to_string(), p.to_string()))
            .collect();
        if derived != listed {
            moved.push(format!(
                "{name} changed; read from the code now:\n{}",
                derived
                    .iter()
                    .map(|(m, p)| format!("    (\"{m}\", \"{p}\"),"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
    }
    assert!(moved.is_empty(), "\n{}\n", moved.join("\n"));

    let all = pairs(&t);
    let urls: Vec<String> = t.routes.keys().map(|p| fill(p)).collect();
    let links = mint(&rig, &people.member, &urls).await;
    let made = links
        .iter()
        .find(|(u, _)| u.contains("/made/"))
        .map(|(_, l)| l.clone())
        .expect("a made file's download is linkable");

    for (m, p) in &all {
        let (m, p) = (m.as_str(), p.as_str());
        let c = &class[&(m.to_string(), p.to_string())];
        let url = fill(p);
        let get = m == "GET";
        let empty: Option<&[u8]> = if get { None } else { Some(b"") };

        // 1. A link never passes a write.
        if !get {
            let (code, _) = send(
                &rig,
                &As::CookieOnly(people.member.clone()),
                m,
                &linked(&links, &url, &made),
                empty,
            )
            .await;
            if code != 401 {
                note("link-write", m, p, format!("{code}"));
            }
        }

        // 2. A login whose password must change first.
        let (code, err) = send(
            &rig,
            &As::Cookie(people.must_change.clone()),
            m,
            &url,
            empty,
        )
        .await;
        if c.any {
            if code == 401 || err == CHANGE_FIRST {
                note("must-change", m, p, format!("{code} {err}: meant to work"));
            }
        } else if c.input && !c.converted && BODY_CODES.contains(&code) {
            pending.insert(format!("{m} {p}"));
        } else if !(code == 403 && err == CHANGE_FIRST) {
            note("must-change", m, p, format!("{code} {err}"));
        }

        // 3. A disabled account's login, and an expired one: nobody.
        for (kind, l) in [("disabled", &people.disabled), ("expired", &people.expired)] {
            let (code, _) = send(&rig, &As::Cookie(l.clone()), m, &url, empty).await;
            if code != 401 {
                note(kind, m, p, format!("{code}"));
            }
        }

        // 4. A member, with an empty, a small and a 1 MB body.
        let big = vec![b'{'; 1 << 20];
        let bodies: Vec<Option<&[u8]>> = if get {
            vec![None]
        } else {
            let mut v: Vec<Option<&[u8]>> =
                vec![Some(b""), Some(b"{}"), Some(SMALL_JSON), Some(&big)];
            if let Some((_, _, body)) = VALID_BODIES.iter().find(|(vm, vp, _)| *vm == m && *vp == p)
            {
                v.push(Some(body.as_bytes()));
            }
            v
        };
        let admin_only = c.admin || c.editor;
        let target = if c.editor && !c.admin {
            format!("{url}?scope=server")
        } else {
            url.clone()
        };
        let mut reached = false;
        for b in bodies {
            let (code, err) = send(&rig, &As::Cookie(people.member.clone()), m, &target, b).await;
            if admin_only {
                if code == 403 || code == 404 {
                    reached = true;
                    continue;
                }
                // An agent editor's admin check is the helper's, made after
                // the body is read, so the `User` extractor does not move it.
                let admin_first = c.converted && !(c.editor && !c.admin);
                if c.input && !admin_first && BODY_CODES.contains(&code) {
                    pending.insert(format!("{m} {p}"));
                } else {
                    note("member-on-admin", m, p, format!("{code} {err}"));
                }
            } else {
                if code == 401 || code >= 500 && code != 503 && code != 504 {
                    note("member", m, p, format!("{code} {err}"));
                }
                if err == ADMINS_ONLY {
                    note("admin-by-answer-only", m, p, format!("{code}"));
                }
            }
        }
        if admin_only && !reached {
            unreached.insert(format!("{m} {p}"));
        }
        if p.starts_with("/api/admin/")
            && !admin_only
            && !ADMIN_LOOKING_BUT_OPEN
                .iter()
                .any(|(am, ap, _)| *am == m && *ap == p)
        {
            note(
                "admin-path-not-admin",
                m,
                p,
                "no admin check in its code".into(),
            );
        }
    }

    // 5. The API token, last (it may change settings): the machine owner's
    // root key, admin rights and every session (`auth.rs`): never 401,
    // never "admins only", never "change your password first".
    for (m, p) in &all {
        let (m, p) = (m.as_str(), p.as_str());
        let empty: Option<&[u8]> = if m == "GET" { None } else { Some(b"") };
        let (code, err) = send(&rig, &As::Bearer(TOKEN), m, &fill(p), empty).await;
        if code == 401 || err == ADMINS_ONLY || err == CHANGE_FIRST || code == 500 {
            note("api-token", m, p, format!("{code} {err}"));
        }
    }

    let mut admin_doc_routes: Vec<String> = t
        .handlers
        .iter()
        .filter(|((m, p), (f, _))| {
            admin_doc.contains(f)
                && !PUBLIC.iter().any(|(pm, pp, _)| pm == m && pp == p)
                && !class[&(m.clone(), p.clone())].admin
                && !class[&(m.clone(), p.clone())].editor
        })
        .map(|((m, p), (f, _))| format!("{m} {p} ({})", f.file_name().unwrap().to_string_lossy()))
        .collect();
    admin_doc_routes.sort();
    eprintln!(
        "member walk: admin-only routes whose admin check no body reached: {}\n{}",
        unreached.len(),
        unreached.iter().cloned().collect::<Vec<_>>().join("\n")
    );
    eprintln!(
        "member walk: {} routes; role answered after the body (pending conversion, decision 2): {}\n{}\nopen routes in files whose doc speaks of admins: {}\n{}",
        all.len(),
        pending.len(),
        pending.iter().cloned().collect::<Vec<_>>().join("\n"),
        admin_doc_routes.len(),
        admin_doc_routes.join("\n"),
    );
    let _ = &rig.store;
    assert!(
        unreached.is_empty(),
        "admin-only routes whose admin check no body reached (add a body to VALID_BODIES):\n{}",
        unreached.iter().cloned().collect::<Vec<_>>().join("\n")
    );
    assert!(
        findings.is_empty(),
        "\nFINDINGS:\n{}\n",
        findings.join("\n")
    );
}

// ------------------------------------------------------------- golden

/// Today's answers, as `require*` gave them before the caller layer and
/// before any handler converted to the extractors: one line per request,
/// `kind  METHOD  path  body  status  error`, in the walk's order.
const GOLDEN: &str = "tests/it/auth_golden.txt";

/// The kinds of caller the golden walk sends, in order.
const KINDS: &[&str] = &[
    "none",
    "bearer",
    "made-up-cookie",
    "cookie-only",
    "foreign-origin",
    "member",
    "admin",
    "must-change",
    "disabled",
    "expired",
    "api-token",
    "link",
];

/// Kinds that are nobody: the layer answers them 401 where the old order
/// may have parsed the body first.
const NOBODY: &[&str] = &[
    "none",
    "bearer",
    "made-up-cookie",
    "cookie-only",
    "disabled",
    "expired",
    "link",
];

/// Every request of the golden walk with its answer, in order. Admins and
/// the token act on the server (an empty object can be a valid body), so
/// the walk runs on a fresh rig and in one fixed order.
async fn golden_walk(rig: &Rig, people: &People, t: &Table) -> Vec<String> {
    let urls: Vec<String> = t.routes.keys().map(|p| fill(p)).collect();
    let links = mint(rig, &people.member, &urls).await;
    let made = links
        .iter()
        .find(|(u, _)| u.contains("/made/"))
        .map(|(_, l)| l.clone())
        .expect("a made file's download is linkable");
    let mut out = Vec::new();
    for (m, p) in pairs(t) {
        let url = fill(&p);
        let mut methods = vec![m.clone()];
        if m == "GET" && links.contains_key(&url) {
            methods.push("HEAD".into());
        }
        for m in methods {
            let bodies: &[(&str, Option<&[u8]>)] = if matches!(m.as_str(), "GET" | "HEAD") {
                &[("none", None)]
            } else {
                &[("empty", Some(b"")), ("object", Some(b"{}"))]
            };
            for (body_kind, body) in bodies {
                for kind in KINDS {
                    let (who, u) = match *kind {
                        "none" => (As::Nobody, url.clone()),
                        "bearer" => (As::Bearer("nope"), url.clone()),
                        "made-up-cookie" => (
                            As::Cookie(Login {
                                cookie: "ab".repeat(32),
                                key: "cd".repeat(32),
                            }),
                            url.clone(),
                        ),
                        "cookie-only" => (As::CookieOnly(people.member.clone()), url.clone()),
                        "foreign-origin" => (As::Foreign, url.clone()),
                        "member" => (As::Cookie(people.member.clone()), url.clone()),
                        "admin" => (As::Cookie(people.admin.clone()), url.clone()),
                        "must-change" => (As::Cookie(people.must_change.clone()), url.clone()),
                        "disabled" => (As::Cookie(people.disabled.clone()), url.clone()),
                        "expired" => (As::Cookie(people.expired.clone()), url.clone()),
                        "api-token" => (As::Bearer(TOKEN), url.clone()),
                        _ => (
                            As::CookieOnly(people.member.clone()),
                            linked(&links, &url, &made),
                        ),
                    };
                    let (code, err) = send(rig, &who, &m, &u, *body).await;
                    let err = err.replace(['\t', '\n'], " ");
                    out.push(format!("{kind}\t{m}\t{p}\t{body_kind}\t{code}\t{err}"));
                }
            }
        }
    }
    out
}

/// Write the golden file from the router without the caller layer. Only
/// on purpose: `HARNESS_WRITE_GOLDEN=1 cargo test -p harness-master
/// --features testing --test it write_the_auth_golden`, and only on code
/// whose handlers all still call `require*` (see the design note: once
/// handlers convert, this router refuses them, and the file is frozen).
#[cfg(feature = "testing")]
#[tokio::test]
async fn write_the_auth_golden() {
    if std::env::var("HARNESS_WRITE_GOLDEN").as_deref() != Ok("1") {
        return;
    }
    let t = read_table(&src_dir()).unwrap_or_else(|p| panic!("{p:?}"));
    let (rig, people) = start(harness_master::router_before_identify).await;
    let lines = golden_walk(&rig, &people, &t).await;
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(GOLDEN);
    let head = "# What require* answered before the caller layer (FIX_PLAN 4.1). One line per request:\n# kind, method, path, body, status, error. Written by write_the_auth_golden; never edit by hand.\n";
    std::fs::write(&path, format!("{head}{}\n", lines.join("\n"))).unwrap();
}

/// The caller layer answers every request of the golden walk as `require*`
/// did, except the two written classes of difference, matched by rule:
/// a caller who is nobody gets 401 where a body error came first, and
/// (decision 2) a member or a login that must change its password gets the
/// 403 "admins only" or "change your password first" where a body error
/// came first. A route the golden does not know is listed, not compared (a
/// route added after it); a golden line whose route is gone fails.
#[tokio::test]
async fn the_layer_answers_as_require_did() {
    let t = read_table(&src_dir()).unwrap_or_else(|p| panic!("{p:?}"));
    let golden = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(GOLDEN))
        .expect("the golden file");
    let want: Vec<&str> = golden.lines().filter(|l| !l.starts_with('#')).collect();
    let (rig, people) = start(router).await;
    let got = golden_walk(&rig, &people, &t).await;
    let key = |l: &str| l.splitn(5, '\t').take(4).collect::<Vec<_>>().join("\t");
    let mut by_key: BTreeMap<String, &str> = BTreeMap::new();
    for l in &want {
        by_key.insert(key(l), l);
    }
    let mut diffs = Vec::new();
    let mut unknown = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let (mut nobody_401, mut decision_2) = (0, 0);
    for l in &got {
        let k = key(l);
        let Some(w) = by_key.get(&k) else {
            let f: Vec<&str> = l.split('\t').collect();
            unknown.insert(format!("{} {}", f[1], f[2]));
            continue;
        };
        seen.insert(k);
        if l == w {
            continue;
        }
        let g: Vec<&str> = l.split('\t').collect();
        let o: Vec<&str> = w.split('\t').collect();
        let (kind, new, old) = (
            g[0],
            g[4].parse::<u16>().unwrap(),
            o[4].parse::<u16>().unwrap(),
        );
        let body_first = BODY_CODES.contains(&old);
        if body_first && new == 401 && NOBODY.contains(&kind) {
            nobody_401 += 1;
        } else if body_first
            && new == 403
            && matches!(kind, "member" | "must-change")
            && [ADMINS_ONLY, CHANGE_FIRST].contains(&g[5])
        {
            decision_2 += 1;
        } else {
            diffs.push(format!("now   {l}\nwas   {w}"));
        }
    }
    let gone: Vec<&str> = by_key
        .iter()
        .filter(|(k, _)| !seen.contains(*k))
        .map(|(_, l)| *l)
        .collect();
    eprintln!(
        "golden walk: {} requests, {} as before, {nobody_401} nobody now 401, {decision_2} decision-2 403s; routes not in the golden: {unknown:?}",
        got.len(),
        got.len() - nobody_401 - decision_2 - diffs.len() - unknown.len()
    );
    assert!(
        gone.is_empty(),
        "golden lines for routes that are gone:\n{}",
        gone.join("\n")
    );
    assert!(
        diffs.is_empty(),
        "\nANSWERS CHANGED:\n{}\n",
        diffs.join("\n")
    );
}

/// The Origin guard answers before the layer, as it answered before the
/// handlers: a page of another site gets 403 whatever it sends, a cookie
/// write without `Origin` gets 403 "origin required", and the page's own
/// flows pass.
#[tokio::test]
async fn the_origin_guard_answers_first() {
    let (rig, people) = start(router).await;
    let c = client();
    let cookie = format!("{}={}", cookie_name(), people.member.cookie);
    // Another site, no credentials, a write: the guard's 403, not 401.
    let r = c
        .post(format!("{}/api/sessions", rig.base))
        .header("origin", "https://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    assert_eq!(r.text().await.unwrap(), "origin not allowed");
    // A cookie write with no Origin: refused before the layer.
    let r = c
        .put(format!("{}/api/me/settings", rig.base))
        .header("cookie", &cookie)
        .header(common::LOGIN_HEADER, &people.member.key)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    assert_eq!(r.text().await.unwrap(), "origin required");
    // A cookie read with no Origin is the page's own GET: it passes.
    let r = c
        .get(format!("{}/api/me", rig.base))
        .header("cookie", &cookie)
        .header(common::LOGIN_HEADER, &people.member.key)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    // A login from the page's own origin (public: no layer).
    let r = c
        .post(format!("{}/api/login", rig.base))
        .header("origin", &rig.base)
        .json(&json!({"username": "mo", "password": "wrong password!"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        r.json::<Value>().await.unwrap()["error"],
        "wrong username or password"
    );
}
