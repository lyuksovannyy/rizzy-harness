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
use harness_master::{Master, router, router_before_identify};
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

/// Calls `name(` that are not method calls (`a.require(..)` is not ours).
fn calls(body: &Toks) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for i in 0..body.len().saturating_sub(1) {
        if let Tok::Id(n) = &body[i].0
            && body[i + 1].0 == Tok::P('(')
            && (i == 0 || body[i - 1].0 != Tok::P('.'))
        {
            out.insert(n.clone());
        }
    }
    out
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

async fn start(before_identify: bool) -> (Rig, People) {
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
    let app = if before_identify {
        router_before_identify(master)
    } else {
        router(master)
    };
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

const SMALL_JSON: &[u8] = br#"{"name":"x"}"#;

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

/// A converted route's answer per kind of caller.
type Pinned = (&'static str, &'static str, &'static [(&'static str, u16)]);

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
    let (rig, people) = start(false).await;
    let mut findings: Vec<String> = Vec::new();
    let mut pending: BTreeSet<String> = BTreeSet::new();
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

    let derived: BTreeSet<(String, String)> = class
        .iter()
        .filter(|(_, c)| c.admin || c.editor)
        .map(|(k, _)| k.clone())
        .collect();
    let listed: BTreeSet<(String, String)> = ADMIN_ONLY
        .iter()
        .map(|(m, p)| (m.to_string(), p.to_string()))
        .collect();
    assert_eq!(
        derived,
        listed,
        "\nthe admin-only routes changed; read from the code now:\n{}\n",
        derived
            .iter()
            .map(|(m, p)| format!("    (\"{m}\", \"{p}\"),"))
            .collect::<Vec<_>>()
            .join("\n")
    );

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
            vec![Some(b""), Some(SMALL_JSON), Some(&big)]
        };
        let admin_only = c.admin || c.editor;
        let target = if c.editor && !c.admin {
            format!("{url}?scope=server")
        } else {
            url.clone()
        };
        for b in bodies {
            let (code, err) = send(&rig, &As::Cookie(people.member.clone()), m, &target, b).await;
            if admin_only {
                if code == 403 || code == 404 {
                    continue;
                }
                if c.input && !c.converted && BODY_CODES.contains(&code) {
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
        "member walk: {} routes; role answered after the body (pending conversion, decision 2): {}\n{}\nopen routes in files whose doc speaks of admins: {}\n{}",
        all.len(),
        pending.len(),
        pending.iter().cloned().collect::<Vec<_>>().join("\n"),
        admin_doc_routes.len(),
        admin_doc_routes.join("\n"),
    );
    let _ = &rig.store;
    assert!(
        findings.is_empty(),
        "\nFINDINGS:\n{}\n",
        findings.join("\n")
    );
}

/// The layer and each handler's own `require*` decide alike: the same
/// requests, in the same order, to a server with the layer and one built
/// without it (`router_before_identify`), from every kind of caller. The
/// only difference allowed is the one the layer exists for: a caller who
/// is nobody gets 401 where the old order answered the body first. The
/// handlers already converted to the extractors cannot run without the
/// layer, so theirs are pinned against what `require*` would answer.
#[tokio::test]
async fn the_layer_and_the_handlers_checks_agree() {
    let t = read_table(&src_dir()).unwrap_or_else(|p| panic!("{p:?}"));
    let (class, problems) = classify(&t);
    assert!(problems.is_empty(), "{problems:?}");
    let (new, pn) = start(false).await;
    let (old, po) = start(true).await;
    let urls: Vec<String> = t.routes.keys().map(|p| fill(p)).collect();
    let links_new = mint(&new, &pn.member, &urls).await;
    let links_old = mint(&old, &po.member, &urls).await;
    let made = |l: &BTreeMap<String, String>| {
        l.iter()
            .find(|(u, _)| u.contains("/made/"))
            .map(|(_, v)| v.clone())
            .unwrap()
    };
    let (made_new, made_old) = (made(&links_new), made(&links_old));

    let mut diffs = Vec::new();
    let mut converted = BTreeMap::new();
    let mut n = 0;
    for (m, p) in pairs(&t) {
        let c = &class[&(m.clone(), p.clone())];
        let url = fill(&p);
        let mut methods = vec![m.clone()];
        if m == "GET" && links_new.contains_key(&url) {
            methods.push("HEAD".into());
        }
        for m in methods {
            let body: Option<&[u8]> = if matches!(m.as_str(), "GET" | "HEAD") {
                None
            } else {
                Some(b"")
            };
            let read = matches!(m.as_str(), "GET" | "HEAD");
            let byte_url = links_new.contains_key(&url);
            // (name, caller on new, caller on old, url on new, url on old, nobody?)
            let kinds: Vec<(&str, As, As, String, String, bool)> = vec![
                (
                    "none",
                    As::Nobody,
                    As::Nobody,
                    url.clone(),
                    url.clone(),
                    true,
                ),
                (
                    "bearer",
                    As::Bearer("nope"),
                    As::Bearer("nope"),
                    url.clone(),
                    url.clone(),
                    true,
                ),
                (
                    "made-up-cookie",
                    As::Cookie(Login {
                        cookie: "ab".repeat(32),
                        key: "cd".repeat(32),
                    }),
                    As::Cookie(Login {
                        cookie: "ab".repeat(32),
                        key: "cd".repeat(32),
                    }),
                    url.clone(),
                    url.clone(),
                    true,
                ),
                (
                    "foreign-origin",
                    As::Foreign,
                    As::Foreign,
                    url.clone(),
                    url.clone(),
                    true,
                ),
                (
                    "member",
                    As::Cookie(pn.member.clone()),
                    As::Cookie(po.member.clone()),
                    url.clone(),
                    url.clone(),
                    false,
                ),
                (
                    "admin",
                    As::Cookie(pn.admin.clone()),
                    As::Cookie(po.admin.clone()),
                    url.clone(),
                    url.clone(),
                    false,
                ),
                (
                    "must-change",
                    As::Cookie(pn.must_change.clone()),
                    As::Cookie(po.must_change.clone()),
                    url.clone(),
                    url.clone(),
                    false,
                ),
                (
                    "api-token",
                    As::Bearer(TOKEN),
                    As::Bearer(TOKEN),
                    url.clone(),
                    url.clone(),
                    false,
                ),
                (
                    if byte_url { "link" } else { "link-elsewhere" },
                    As::CookieOnly(pn.member.clone()),
                    As::CookieOnly(po.member.clone()),
                    linked(&links_new, &url, &made_new),
                    linked(&links_old, &url, &made_old),
                    !(byte_url && read && c.bytes),
                ),
            ];
            for (kind, wn, wo, un, uo, nobody) in kinds {
                n += 1;
                let (a, ea) = send(&new, &wn, &m, &un, body).await;
                if c.converted {
                    converted.insert(format!("{m} {p} [{kind}]"), (a, ea));
                    continue;
                }
                let (b, eb) = send(&old, &wo, &m, &uo, body).await;
                let ok = a == b || nobody && a == 401 && BODY_CODES.contains(&b);
                if !ok {
                    diffs.push(format!(
                        "{m} {p} [{kind}]: layer {a} {ea} / before {b} {eb}"
                    ));
                }
                if kind == "foreign-origin" && a != 403 {
                    diffs.push(format!(
                        "{m} {p} [foreign-origin]: {a}, not the guard's 403"
                    ));
                }
            }
        }
    }
    // The converted handlers, against what `require*` answered: `require`
    // for /api/me/storage, `require_admin` for /api/admin/storage, and
    // `require_bytes` for a made file (a chat that does not exist: 404).
    // A signed-in caller without the right now gets 403 before the body
    // (decision 2): PUT /api/admin/storage with an empty body.
    let want: &[Pinned] = &[
        (
            "GET",
            "/api/me/storage",
            &[
                ("none", 401),
                ("bearer", 401),
                ("made-up-cookie", 401),
                ("foreign-origin", 403),
                ("member", 200),
                ("admin", 200),
                ("must-change", 403),
                ("api-token", 200),
                ("link-elsewhere", 401),
            ],
        ),
        (
            "GET",
            "/api/admin/storage",
            &[
                ("none", 401),
                ("bearer", 401),
                ("made-up-cookie", 401),
                ("foreign-origin", 403),
                ("member", 403),
                ("admin", 200),
                ("must-change", 403),
                ("api-token", 200),
                ("link-elsewhere", 401),
            ],
        ),
        (
            "PUT",
            "/api/admin/storage",
            &[
                ("none", 401),
                ("bearer", 401),
                ("made-up-cookie", 401),
                ("foreign-origin", 403),
                ("member", 403),
                ("admin", 400),
                ("must-change", 403),
                ("api-token", 400),
                ("link-elsewhere", 401),
            ],
        ),
        (
            "GET",
            "/api/sessions/{id}/made/{file_id}",
            &[
                ("none", 401),
                ("bearer", 401),
                ("made-up-cookie", 401),
                ("foreign-origin", 403),
                ("member", 404),
                ("admin", 404),
                ("must-change", 403),
                ("api-token", 404),
                ("link", 404),
            ],
        ),
        (
            "HEAD",
            "/api/sessions/{id}/made/{file_id}",
            &[
                ("none", 401),
                ("bearer", 401),
                ("made-up-cookie", 401),
                ("foreign-origin", 403),
                ("member", 404),
                ("admin", 404),
                ("must-change", 403),
                ("api-token", 404),
                ("link", 404),
            ],
        ),
    ];
    let mut pinned = 0;
    for (m, p, codes) in want {
        for (kind, code) in *codes {
            let k = format!("{m} {p} [{kind}]");
            pinned += 1;
            match converted.get(&k) {
                Some((got, _)) if got == code => {}
                other => diffs.push(format!("{k}: {other:?}, want {code}")),
            }
        }
    }
    assert_eq!(
        pinned,
        converted.len(),
        "a converted handler with no pinned answers: {:?}",
        converted.keys().collect::<Vec<_>>()
    );
    eprintln!("agreement walk: {n} request pairs");
    assert!(diffs.is_empty(), "\nDISAGREE:\n{}\n", diffs.join("\n"));
}

/// The Origin guard answers before the layer, as it answered before the
/// handlers: a page of another site gets 403 whatever it sends, a cookie
/// write without `Origin` gets 403 "origin required", and the page's own
/// flows pass.
#[tokio::test]
async fn the_origin_guard_answers_first() {
    let (rig, people) = start(false).await;
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
