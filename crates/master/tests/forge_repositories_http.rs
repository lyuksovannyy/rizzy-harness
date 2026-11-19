//! Listing a person's repositories through one of their forge connections
//! (ADOPTION_PLAN X.27 step 2, PLAN.md T83) against a fake forge on
//! 127.0.0.1: only the connection's owner lists; the token is opened only
//! after the request is checked and the limits passed, and only with a key
//! that is proven; every request URL is ours (the search word one encoded
//! parameter, a `Link` header never followed); a hostile answer is bounded
//! and nothing unvalidated is passed on; floods meet a limit and a cache
//! that is per person and forgets a replaced token. No real forge and no
//! real token are used.

mod common;
use common::{Login as _, SameOrigin as _};

use std::io::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode as AxStatus};
use axum::response::{IntoResponse, Response};
use futures::{SinkExt, StreamExt};
use harness_core::Secret;
use harness_master::auth::{Auth, Claim};
use harness_master::forge::{ForgeHost, ForgeKind};
use harness_master::forge_repos;
use harness_master::sealed::Sealer;
use harness_master::{Master, router};
use harness_store::{Durability, Store};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

/// Alice's token for forge account 42, its replacement, Bob's (account 43).
/// Made up; all are searched for everywhere at the end.
const TOKEN_A: &str = "ghp_CANARYlistalice00000000000000000001";
const TOKEN_A2: &str = "ghp_CANARYlistalice0000000000replaced2";
const TOKEN_B: &str = "ghp_CANARYlistbob000000000000000000003";
const ALL: [&str; 3] = [TOKEN_A, TOKEN_A2, TOKEN_B];

fn logs() -> Arc<Mutex<Vec<u8>>> {
    static BUF: OnceLock<Arc<Mutex<Vec<u8>>>> = OnceLock::new();
    BUF.get_or_init(|| {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let sink = buf.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(move || Sink(sink.clone()))
            .finish();
        tracing::subscriber::set_global_default(subscriber).expect("one subscriber");
        buf
    })
    .clone()
}

struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct Seen {
    path_and_query: String,
    auth: Option<String>,
}

#[derive(Clone)]
struct Fake {
    seen: Arc<Mutex<Vec<Seen>>>,
    addr: Arc<OnceLock<SocketAddr>>,
}

fn gh_user(h: &HeaderMap) -> Response {
    let tok = h
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").or(v.strip_prefix("token ")))
        .map(str::to_string);
    let user = match tok.as_deref() {
        Some(TOKEN_A | TOKEN_A2) => json!({"id": 42, "login": "ada"}),
        Some(TOKEN_B) => json!({"id": 43, "login": "bob-gh"}),
        _ => return (AxStatus::UNAUTHORIZED, "no").into_response(),
    };
    axum::Json(user).into_response()
}

fn gh_repo(id: u64, full: &str) -> Value {
    json!({
        "id": id, "name": full.split('/').nth(1), "full_name": full,
        "owner": {"login": full.split('/').next(), "avatar_url": "http://169.254.169.254/a.png"},
        "private": true, "default_branch": "main",
        "description": "the <b>harness</b> \u{202E}evil\u{0007}",
        "updated_at": "2026-11-01T10:00:00Z",
        "clone_url": "https://evil.example/steal.git",
        "html_url": "javascript:alert(1)",
        "permissions": {"admin": false, "push": true, "pull": true},
    })
}

/// A GitHub list answer: `n` good repositories plus a `Link` to another
/// host, which must never be followed.
fn gh_list(page: &str, per_page: &str) -> Response {
    let mut repos = vec![
        gh_repo(1001, "ada/harness"),
        json!({"id": 1002, "full_name": "acme/.github", "private": false,
               "default_branch": null, "description": null, "updated_at": null,
               "permissions": {"push": false}}),
        json!({"id": 1003, "full_name": "bob/Notes", "private": false,
               "default_branch": "dev"}),
    ];
    if per_page == "100" {
        // The search reads a whole forge page: a few more to filter.
        for i in 0..20 {
            repos.push(
                json!({"id": 2000 + i, "full_name": format!("acme/tool-{i}"),
                              "private": false}),
            );
        }
    }
    let link = if page == "1" {
        r#"<http://169.254.169.254/latest?page=2>; rel="next", <http://evil.example/last>; rel="last""#
    } else {
        r#"<http://169.254.169.254/first>; rel="first""#
    };
    ([("link", link)], axum::Json(Value::Array(repos))).into_response()
}

fn query_of(pq: &str) -> Vec<(String, String)> {
    let u = reqwest::Url::parse(&format!("http://x{pq}")).unwrap();
    u.query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

fn param(pq: &str, k: &str) -> String {
    query_of(pq)
        .into_iter()
        .find(|(a, _)| a == k)
        .map(|(_, v)| v)
        .unwrap_or_default()
}

/// Hostile names, branches, ids and shapes, with one good entry.
fn hostile_entries() -> Value {
    let long = "a".repeat(10_000);
    json!([
        {"id": 1, "full_name": "ada/good", "private": false, "default_branch": "--upload-pack=touch /tmp/x"},
        {"id": 2, "full_name": "ada/..", "private": false},
        {"id": 3, "full_name": "../etc", "private": false},
        {"id": 4, "full_name": "-x/y", "private": false},
        {"id": 5, "full_name": "ada/-y", "private": false},
        {"id": 6, "full_name": "a/b/c", "private": false},
        {"id": 7, "full_name": "ada\u{202E}/b", "private": false},
        {"id": 8, "full_name": "ada/b\u{0000}", "private": false},
        {"id": 9, "full_name": "ada/b\u{200B}", "private": false},
        {"id": 10, "full_name": format!("ada/{long}"), "private": false},
        {"id": 11, "full_name": "noslash", "private": false},
        {"id": 12, "full_name": "ada/b c", "private": false},
        {"id": 13, "full_name": "ada//b", "private": false},
        {"id": 14, "full_name": "/ada", "private": false},
        {"id": 15, "full_name": 5, "private": false},
        {"id": 16, "full_name": "ada/ok2", "private": "yes"},
        {"id": 1e30, "full_name": "ada/huge", "private": false},
        {"id": -5, "full_name": "ada/neg", "private": false},
        {"id": "17", "full_name": "ada/strid", "private": false},
        {"id": 0, "full_name": "ada/zero", "private": false},
        {"id": 1, "full_name": "ada/dup", "private": false},
        {"id": 18, "full_name": "ada/branch2", "private": true, "default_branch": "a..b",
         "updated_at": "yesterday", "description": format!("<script>{}</script>", "x".repeat(5000)),
         "permissions": {"push": "true"}},
        {"id": 19, "full_name": "ada/branch3", "private": true, "default_branch": "x\u{0007}y",
         "updated_at": 1_700_000_000},
        {"id": 20, "full_name": "ada/branch4", "private": true, "default_branch": "release/v1.lock"},
        "a string entry",
        null,
        [],
        {"id": 21, "full_name": "Ada-L/My.Repo_1", "private": true, "default_branch": "feature/x-1"},
    ])
}

async fn forge(State(f): State<Fake>, req: Request) -> Response {
    let h = req.headers().clone();
    let pq = req
        .uri()
        .path_and_query()
        .map(|p| p.to_string())
        .unwrap_or_default();
    let auth = h
        .get("authorization")
        .or(h.get("private-token"))
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    f.seen.lock().unwrap().push(Seen {
        path_and_query: pq.clone(),
        auth: auth.clone(),
    });
    let path = req.uri().path().to_string();
    let (prefix, rest) = path[1..].split_once('/').unwrap_or((&path[1..], ""));
    let port = f.addr.get().unwrap().port();
    if rest == "user" && prefix != "gl" {
        return gh_user(&h);
    }
    match (prefix, rest) {
        ("gl", "user") => match h.get("private-token").and_then(|v| v.to_str().ok()) {
            Some(TOKEN_A) => axum::Json(json!({"id": 7, "username": "ada.gl"})).into_response(),
            _ => (AxStatus::UNAUTHORIZED, "401").into_response(),
        },
        ("gl", "personal_access_tokens/self") => {
            axum::Json(json!({"scopes": ["read_api"]})).into_response()
        }
        ("gl", "projects") => (
            [("x-next-page", "2"), ("x-total-pages", "9")],
            axum::Json(json!([
                {"id": 70, "path_with_namespace": "grp/sub/proj", "visibility": "internal",
                 "default_branch": "main", "description": "GL", "last_activity_at": "2026-10-30T08:00:00.123Z",
                 "permissions": {"project_access": null, "group_access": {"access_level": 30}}},
                {"id": 71, "path_with_namespace": "ada.gl/pub", "visibility": "public",
                 "default_branch": null, "permissions": {"project_access": {"access_level": 20}, "group_access": null}},
                {"id": 72, "path_with_namespace": "ada.gl/x.git", "visibility": "public"},
                {"id": 73, "path_with_namespace": "solo", "visibility": "public"},
                {"id": 74, "path_with_namespace": "grp/odd", "visibility": "secret"},
            ])),
        )
            .into_response(),
        ("gt", "repos/search") => (
            [("link", format!("<http://localhost:{port}/gt/repos/search?page=2>; rel=\"next\""))],
            axum::Json(json!({"ok": true, "data": [
                {"id": 90, "full_name": "ada/forgejo-thing", "private": true, "default_branch": "main",
                 "updated_at": "2026-11-02T00:00:00+01:00", "permissions": {"admin": true, "push": true, "pull": true}},
                {"id": 91, "full_name": "org.x/y_z", "private": false, "permissions": {"push": false}},
            ]})),
        )
            .into_response(),
        ("gtbad", "repos/search") => axum::Json(json!({"ok": false, "data": []})).into_response(),
        ("gh" | "gh2", "user/repos") => {
            if auth.as_deref() != Some(&format!("Bearer {TOKEN_A}"))
                && auth.as_deref() != Some(&format!("Bearer {TOKEN_A2}"))
                && auth.as_deref() != Some(&format!("Bearer {TOKEN_B}"))
            {
                return (AxStatus::UNAUTHORIZED, "no").into_response();
            }
            gh_list(&param(&pq, "page"), &param(&pq, "per_page"))
        }
        ("e401", "user/repos") => {
            (AxStatus::UNAUTHORIZED, format!("revoked: {auth:?}")).into_response()
        }
        ("e403", "user/repos") => (AxStatus::FORBIDDEN, "scope").into_response(),
        ("e429", "user/repos") => (AxStatus::TOO_MANY_REQUESTS, "slow down").into_response(),
        ("e500", "user/repos") => (
            AxStatus::INTERNAL_SERVER_ERROR,
            format!("internal error, token was {auth:?}"),
        )
            .into_response(),
        ("hang", "user/repos") => {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            "late".into_response()
        }
        ("drip", "user/repos") => {
            let s = futures::stream::unfold((), |_| async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                Some((
                    Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b" ")),
                    (),
                ))
            });
            Response::new(Body::from_stream(s))
        }
        ("big", "user/repos") => {
            // 50 MB, never with a length up front.
            let chunk = axum::body::Bytes::from(vec![b' '; 64 * 1024]);
            let head = futures::stream::once(async {
                Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"["))
            });
            let body = futures::stream::iter((0..800).map(move |_| Ok(chunk.clone())));
            Response::new(Body::from_stream(head.chain(body)))
        }
        ("many", "user/repos") => {
            let all: Vec<Value> = (1..=10_000)
                .map(|i| json!({"id": i, "full_name": format!("a/r{i}"), "private": false}))
                .collect();
            axum::Json(Value::Array(all)).into_response()
        }
        ("names", "user/repos") => axum::Json(hostile_entries()).into_response(),
        // Review of PR 158, HIGH 1: answers as costly to parse as fit.
        ("objs", "user/repos") => worst_body("objs").into_response(),
        ("zeros", "user/repos") => worst_body("zeros").into_response(),
        ("slowobjs", "user/repos") => {
            tokio::time::sleep(Duration::from_millis(700)).await;
            worst_body("objs").into_response()
        }
        // HIGH 2: Alice's token is refused for listing; others work.
        ("reva", "user/repos") => {
            if auth.as_deref() == Some(&format!("Bearer {TOKEN_A}")) {
                (AxStatus::UNAUTHORIZED, "revoked").into_response()
            } else {
                gh_list(&param(&pq, "page"), &param(&pq, "per_page"))
            }
        }
        ("obj", "user/repos") => axum::Json(json!({"repos": []})).into_response(),
        ("notjson", "user/repos") => "<html>hi</html>".into_response(),
        ("redir-in", "user/repos") => (
            AxStatus::FOUND,
            [("location", "http://169.254.169.254/latest/meta-data/")],
        )
            .into_response(),
        ("redir-host", "user/repos") => (
            AxStatus::FOUND,
            [("location", format!("http://localhost:{port}/gh/user/repos"))],
        )
            .into_response(),
        _ => (AxStatus::NOT_FOUND, "no").into_response(),
    }
}

/// A JSON list just under the answer cap: `objs` is `[{"a":0},…]`,
/// `zeros` is `[0,0,…]` (review of PR 158, HIGH 1).
fn worst_body(kind: &str) -> ([(&'static str, &'static str); 1], axum::body::Bytes) {
    static BODIES: OnceLock<(axum::body::Bytes, axum::body::Bytes)> = OnceLock::new();
    let (objs, zeros) = BODIES.get_or_init(|| {
        let cap = forge_repos::MAX_BODY - 16;
        let mut a = String::from("[");
        while a.len() + 9 < cap {
            a.push_str(r#"{"a":0},"#);
        }
        a.pop();
        a.push(']');
        let mut z = String::from("[");
        while z.len() + 3 < cap {
            z.push_str("0,");
        }
        z.pop();
        z.push(']');
        (a.into(), z.into())
    });
    let b = if kind == "objs" { objs } else { zeros };
    ([("content-type", "application/json")], b.clone())
}

async fn fake_forge() -> Fake {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let f = Fake {
        seen: Arc::default(),
        addr: Arc::default(),
    };
    f.addr.set(listener.local_addr().unwrap()).unwrap();
    let app = axum::Router::new().fallback(forge).with_state(f.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    f
}

impl Fake {
    fn count(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
    fn last(&self) -> Seen {
        self.seen.lock().unwrap().last().unwrap().clone()
    }
    fn host(&self, host: &str, kind: ForgeKind, path: &str) -> ForgeHost {
        ForgeHost {
            host: host.into(),
            kind,
            api_base: format!("http://{host}:{}{path}", self.addr.get().unwrap().port()),
            api_host: None,
            private_address: true,
        }
    }
}

struct T {
    base: String,
    addr: SocketAddr,
    master: Master,
    store: Store,
    forge: Fake,
    dir: tempfile::TempDir,
    answers: Arc<Mutex<String>>,
}

async fn start() -> T {
    logs();
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, "X").unwrap();
    let store = Store::open(&dir.path().join("master.db"), Durability::Normal).unwrap();
    let master = Master::new(
        store.clone(),
        Auth::new(
            Secret::new("api-token-xyz".into()),
            vec![addr.to_string()],
            false,
        )
        .with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim_path,
        }),
    );
    master.set_sealer(Sealer::new(dir.path()));
    let forge = fake_forge().await;
    master.set_forge_hosts(vec![
        forge.host("127.0.0.1", ForgeKind::Github, "/gh"),
        forge.host("localhost", ForgeKind::Gitlab, "/gl"),
    ]);
    master.set_forge_timeout(Duration::from_secs(2));
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        addr,
        master,
        store,
        forge,
        dir,
        answers: Arc::default(),
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

async fn req(
    t: &T,
    method: Method,
    path: &str,
    body: Option<Value>,
    who: Option<&str>,
) -> (StatusCode, Value) {
    let mut rb = client().request(method, format!("{}{path}", t.base));
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    match who {
        Some("api") => rb = rb.bearer_auth("api-token-xyz"),
        Some(c) => rb = rb.login(c).same_origin(),
        None => {}
    }
    let r = rb.send().await.unwrap();
    let status = r.status();
    let headers = format!("{:?}", r.headers());
    let text = r.text().await.unwrap();
    t.answers
        .lock()
        .unwrap()
        .push_str(&format!("{status} {headers} {text}\n"));
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

async fn post_anon(t: &T, path: &str, body: Value, c: Option<&str>) -> reqwest::Response {
    let mut rb = client().post(format!("{}{path}", t.base)).json(&body);
    if let Some(c) = c {
        rb = rb.login(c).same_origin();
    }
    rb.send().await.unwrap()
}

async fn member(t: &T, admin: &str, name: &str) -> String {
    let (_, v) = req(
        t,
        Method::POST,
        "/api/users",
        Some(json!({"username": name})),
        Some(admin),
    )
    .await;
    let pw = v["password"].as_str().unwrap().to_string();
    let first = post_anon(
        t,
        "/api/login",
        json!({"username": name, "password": pw}),
        None,
    )
    .await;
    common::signed_in(
        post_anon(
            t,
            "/api/me/password",
            json!({"current": pw, "new": format!("{name}s own password")}),
            Some(&common::signed_in(first).await),
        )
        .await,
    )
    .await
}

/// The admin "alex", members "alice" and "bob": (cookie, user id) each.
async fn users(t: &T) -> [(String, String); 3] {
    let alex = common::signed_in(
        post_anon(
            t,
            "/api/setup",
            json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}),
            None,
        )
        .await,
    )
    .await;
    let alice = member(t, &alex, "alice").await;
    let bob = member(t, &alex, "bob").await;
    let mut out = Vec::new();
    for c in [alex, alice, bob] {
        let (_, me) = req(t, Method::GET, "/api/me", None, Some(&c)).await;
        out.push((c, me["user"]["user_id"].as_str().unwrap().to_string()));
    }
    out.try_into().unwrap()
}

async fn connect(t: &T, c: &str, host: &str, token: &str) -> String {
    let (st, v) = req(
        t,
        Method::POST,
        "/api/connections",
        Some(json!({"host": host, "token": token})),
        Some(c),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["id"].as_str().unwrap().to_string()
}

async fn list(t: &T, c: &str, id: &str, query: &str) -> (StatusCode, Value) {
    req(
        t,
        Method::GET,
        &format!("/api/connections/{id}/repositories{query}"),
        None,
        Some(c),
    )
    .await
}

async fn socket(t: &T, c: &str) -> Arc<Mutex<String>> {
    let mut r = format!("ws://{}/api/ws", t.addr)
        .into_client_request()
        .unwrap();
    r.headers_mut().insert("cookie", c.parse().unwrap());
    r.headers_mut()
        .insert("origin", format!("http://{}", t.addr).parse().unwrap());
    let (mut s, _) = tokio_tungstenite::connect_async(r).await.unwrap();
    s.send(Message::Text(common::socket_login(c).into()))
        .await
        .unwrap();
    let frames = Arc::new(Mutex::new(String::new()));
    let f2 = frames.clone();
    tokio::spawn(async move {
        while let Some(Ok(m)) = s.next().await {
            if let Message::Text(text) = m {
                f2.lock().unwrap().push_str(&text);
                f2.lock().unwrap().push('\n');
            }
        }
    });
    frames
}

fn walk(p: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() {
                out.extend(walk(&path));
            } else {
                out.push(path);
            }
        }
    }
    out
}

fn assert_no_token_anywhere(t: &T, frames: &str) {
    let mut files = String::new();
    for e in walk(t.dir.path()) {
        if let Ok(b) = std::fs::read(&e) {
            files.push_str(&String::from_utf8_lossy(&b));
        }
    }
    let logs = String::from_utf8_lossy(&logs().lock().unwrap()).to_string();
    let answers = t.answers.lock().unwrap().clone();
    let urls: String = t
        .forge
        .seen
        .lock()
        .unwrap()
        .iter()
        .map(|s| s.path_and_query.clone() + "\n")
        .collect();
    let audit: Vec<String> = {
        let c = rusqlite::Connection::open(t.dir.path().join("master.db")).unwrap();
        let mut st = c
            .prepare("SELECT kind || ' ' || detail FROM audit")
            .unwrap();
        st.query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    for tok in ALL {
        for (what, text) in [
            ("data directory", files.as_str()),
            ("logs", logs.as_str()),
            ("HTTP answers", answers.as_str()),
            ("socket frames", frames),
            ("forge URLs", urls.as_str()),
            ("audit", &audit.join("\n")),
        ] {
            assert!(!text.contains(tok), "{tok} found in {what}");
            assert!(!text.contains(&tok[8..]), "part of {tok} found in {what}");
        }
    }
}

/// Point the GitHub entry "127.0.0.1" at the fake's `path`.
fn github_at(t: &T, path: &str) {
    t.master.set_forge_hosts(vec![
        t.forge.host("127.0.0.1", ForgeKind::Github, path),
        t.forge.host("localhost", ForgeKind::Gitlab, "/gl"),
    ]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lists_repositories_cleaned_and_the_token_is_nowhere() {
    let t = start().await;
    let [_, (alice, _), _] = users(&t).await;
    let frames = socket(&t, &alice).await;
    let id = connect(&t, &alice, "127.0.0.1", TOKEN_A).await;

    let (st, v) = list(&t, &alice, &id, "").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // The exact request: our path and parameters, the token in its header.
    let seen = t.forge.last();
    assert_eq!(
        seen.path_and_query,
        "/gh/user/repos?visibility=all&affiliation=owner%2Ccollaborator%2Corganization_member\
         &sort=updated&direction=desc&per_page=30&page=1"
    );
    assert_eq!(
        seen.auth.as_deref(),
        Some(format!("Bearer {TOKEN_A}").as_str())
    );
    assert_eq!(v["page"], 1);
    assert_eq!(v["has_more"], true, "rel=next read, not followed");
    assert_eq!(v["left_out"], 0);
    assert_eq!(v["search"], "none");
    let r = &v["repositories"];
    assert_eq!(r.as_array().unwrap().len(), 3, "{v}");
    assert_eq!(
        r[0],
        json!({
            "id": "1001", "full_name": "ada/harness", "private": true,
            "default_branch": "main", "description": "the <b>harness</b> evil",
            "updated_ms": 1_793_527_200_000i64, "push": true,
        })
    );
    assert_eq!(
        r[1],
        json!({"id": "1002", "full_name": "acme/.github", "private": false,
               "default_branch": null, "description": null, "updated_ms": null, "push": false})
    );
    assert_eq!(r[2]["push"], Value::Null, "no permissions: not guessed");
    // No URL from the forge is passed on, and none was requested.
    let text = v.to_string();
    for u in [
        "evil.example",
        "169.254",
        "javascript:",
        "avatar",
        "clone_url",
    ] {
        assert!(!text.contains(u), "{u} in {text}");
    }
    let n = t.forge.count();
    // Page 2: no next link.
    let (st, v) = list(&t, &alice, &id, "?page=2").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["has_more"], false);
    assert_eq!(t.forge.count(), n + 1, "one request, no link followed");

    // The failures, each without the token anywhere.
    let mut failures = 0;
    for (path, status, needle) in [
        ("/e401", StatusCode::BAD_GATEWAY, "replace the token"),
        ("/e403", StatusCode::BAD_GATEWAY, "replace the token"),
        ("/e429", StatusCode::BAD_GATEWAY, "limiting this account"),
        ("/e500", StatusCode::BAD_GATEWAY, "HTTP 500"),
        ("/hang", StatusCode::BAD_GATEWAY, "in time"),
        ("/big", StatusCode::BAD_GATEWAY, "too large"),
        ("/redir-in", StatusCode::BAD_GATEWAY, "HTTP 302"),
        ("/redir-host", StatusCode::BAD_GATEWAY, "HTTP 302"),
        ("/obj", StatusCode::BAD_GATEWAY, "not a list"),
        ("/notjson", StatusCode::BAD_GATEWAY, "not JSON"),
    ] {
        github_at(&t, path);
        let before = t.forge.count();
        let started = std::time::Instant::now();
        let (st, e) = list(&t, &alice, &id, &format!("?q={}", &path[1..])).await;
        assert_eq!(st, status, "{path}: {e}");
        let msg = e["error"].as_str().unwrap();
        assert!(msg.contains(needle), "{path}: {msg}");
        assert!(
            !msg.contains("revoked: ") && !msg.contains("internal error"),
            "{msg}"
        );
        assert!(started.elapsed() < Duration::from_secs(8), "{path}");
        assert_eq!(t.forge.count(), before + 1, "{path}: one request");
        // Only what says the host is failing counts against it.
        let host_failed = !matches!(path, "/e401" | "/e403" | "/e429");
        assert_eq!(
            t.master.forge_host_failures("127.0.0.1"),
            failures + usize::from(host_failed),
            "{path}"
        );
        failures = t.master.forge_host_failures("127.0.0.1");
    }
    // An https entry whose server speaks no TLS.
    let port = t.forge.addr.get().unwrap().port();
    t.master.set_forge_hosts(vec![ForgeHost {
        api_base: format!("https://127.0.0.1:{port}/gh"),
        ..t.forge.host("127.0.0.1", ForgeKind::Github, "/gh")
    }]);
    let (st, e) = list(&t, &alice, &id, "?q=tls").await;
    assert_eq!(st, StatusCode::BAD_GATEWAY, "{e}");
    // A 401 does not delete the connection.
    let (_, l) = req(&t, Method::GET, "/api/connections", None, Some(&alice)).await;
    assert_eq!(l["connections"].as_array().unwrap().len(), 1);
    // Listing is a read: no audit line.
    let c = rusqlite::Connection::open(t.dir.path().join("master.db")).unwrap();
    let n: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM audit WHERE detail LIKE '%repositor%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
    drop(c);
    tokio::time::sleep(Duration::from_millis(300)).await;
    t.store.call(|s| s.sealed_count()).await.unwrap();
    let frames = frames.lock().unwrap().clone();
    assert_no_token_anywhere(&t, &frames);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn each_forge_kind_is_asked_its_documented_way() {
    let t = start().await;
    let [_, (alice, _), _] = users(&t).await;
    // GitLab: membership projects, server-side search, x-next-page.
    let gl = connect(&t, &alice, "localhost", TOKEN_A).await;
    let (st, v) = list(&t, &alice, &gl, "?q=proj").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let seen = t.forge.last();
    assert_eq!(
        seen.path_and_query,
        "/gl/projects?membership=true&order_by=last_activity_at&sort=desc&per_page=30&page=1&search=proj&search_namespaces=true"
    );
    assert_eq!(seen.auth.as_deref(), Some(TOKEN_A), "PRIVATE-TOKEN header");
    assert_eq!(v["has_more"], true);
    assert_eq!(v["search"], "forge");
    let r = v["repositories"].as_array().unwrap();
    assert_eq!(
        r[0],
        json!({"id": "70", "full_name": "grp/sub/proj", "private": true,
               "default_branch": "main", "description": "GL",
               "updated_ms": 1_793_347_200_123i64, "push": true})
    );
    assert_eq!(r[1]["private"], false);
    assert_eq!(r[1]["push"], false, "Reporter cannot push");
    // `x.git`, a one-part path and an unknown visibility are left out.
    assert_eq!((r.len(), v["left_out"].as_u64()), (2, Some(3)), "{v}");

    // Gitea and Forgejo: /repos/search by the account's id, {ok, data}.
    for kind in [ForgeKind::Gitea, ForgeKind::Forgejo] {
        t.master
            .set_forge_hosts(vec![t.forge.host("127.0.0.1", kind, "/gt")]);
        let id = connect(&t, &alice, "127.0.0.1", TOKEN_A).await;
        let (st, v) = list(&t, &alice, &id, "?q=thing&page=3").await;
        assert_eq!(st, StatusCode::OK, "{v}");
        let seen = t.forge.last();
        assert_eq!(
            seen.path_and_query,
            "/gt/repos/search?uid=42&private=true&sort=updated&order=desc&limit=30&page=3&q=thing"
        );
        assert_eq!(
            seen.auth.as_deref(),
            Some(format!("token {TOKEN_A}").as_str())
        );
        assert_eq!(v["has_more"], true);
        let r = v["repositories"].as_array().unwrap();
        assert_eq!(r.len(), 2, "{v}");
        assert_eq!(r[0]["updated_ms"], 1_793_574_000_000i64);
        assert_eq!(
            (r[0]["push"].as_bool(), r[1]["full_name"].as_str()),
            (Some(true), Some("org.x/y_z"))
        );
        let (st, e) = {
            t.master
                .set_forge_hosts(vec![t.forge.host("127.0.0.1", kind, "/gtbad")]);
            list(&t, &alice, &id, "?q=x").await
        };
        assert_eq!(st, StatusCode::BAD_GATEWAY, "{e}");
        // Cleanup for the next kind: disconnect.
        req(
            &t,
            Method::DELETE,
            &format!("/api/connections/{id}"),
            None,
            Some(&alice),
        )
        .await;
    }
}

/// GitHub cannot search the listing on its side: a search word filters one
/// forge page of 100 here, and never reaches GitHub.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn github_search_filters_a_page_and_sends_no_word() {
    let t = start().await;
    let [_, (alice, _), _] = users(&t).await;
    let id = connect(&t, &alice, "127.0.0.1", TOKEN_A).await;
    let (st, v) = list(&t, &alice, &id, "?q=TOOL-1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        t.forge.last().path_and_query,
        "/gh/user/repos?visibility=all&affiliation=owner%2Ccollaborator%2Corganization_member\
         &sort=updated&direction=desc&per_page=100&page=1"
    );
    assert_eq!(v["search"], "within_page");
    let names: Vec<&str> = v["repositories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["full_name"].as_str().unwrap())
        .collect();
    let mut want = vec!["acme/tool-1".to_string()];
    want.extend((10..20).map(|i| format!("acme/tool-{i}")));
    assert_eq!(names, want);
    assert_eq!(v["has_more"], true);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn others_cannot_list_with_her_connection() {
    let t = start().await;
    let [(alex, _), (alice, alice_id), (bob, bob_id)] = users(&t).await;
    let id = connect(&t, &alice, "127.0.0.1", TOKEN_A).await;
    // Bob is a workspace admin of a workspace Alice is in.
    let (st, w) = req(
        &t,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "Acme"})),
        Some(&bob),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{w}");
    let acme = w["workspace_id"].as_str().unwrap().to_string();
    let (st, v) = req(
        &t,
        Method::POST,
        &format!("/api/workspaces/{acme}/members"),
        Some(json!({"username": "alice", "role": "member"})),
        Some(&bob),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // A cached answer of hers is not one of theirs either.
    let (st, _) = list(&t, &alice, &id, "").await;
    assert_eq!(st, StatusCode::OK);
    let before = t.forge.count();
    for who in [&alex, &bob] {
        let (st, e) = list(&t, who, &id, "").await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{e}");
    }
    let (st, e) = list(&t, "api", &id, "").await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{e}");
    let (st, _) = req(
        &t,
        Method::GET,
        &format!("/api/connections/{id}/repositories"),
        None,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    // Bob's own connection does not reach Alice's cache or token.
    let bobs = connect(&t, &bob, "127.0.0.1", TOKEN_B).await;
    let (st, _) = list(&t, &bob, &bobs, "").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        t.forge.last().auth.as_deref(),
        Some(format!("Bearer {TOKEN_B}").as_str())
    );
    assert_eq!(t.forge.count(), before + 2, "his connect and his listing");
    // A disabled account lists nothing.
    let (st, v) = req(
        &t,
        Method::PATCH,
        &format!("/api/users/{alice_id}"),
        Some(json!({"disabled": true})),
        Some(&alex),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let before = t.forge.count();
    let (st, _) = list(&t, &alice, &id, "?q=again").await;
    assert!(
        st == StatusCode::UNAUTHORIZED || st == StatusCode::FORBIDDEN,
        "{st}"
    );
    assert_eq!(t.forge.count(), before);
    let _ = bob_id;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_hostile_page_is_bounded_and_nothing_unvalidated_passes() {
    let t = start().await;
    let [_, (alice, _), _] = users(&t).await;
    let id = connect(&t, &alice, "127.0.0.1", TOKEN_A).await;
    github_at(&t, "/names");
    let (st, v) = list(&t, &alice, &id, "").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let r = v["repositories"].as_array().unwrap();
    let names: Vec<&str> = r.iter().map(|x| x["full_name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "ada/good",
            "ada/branch2",
            "ada/branch3",
            "ada/branch4",
            "Ada-L/My.Repo_1"
        ],
        "{v}"
    );
    // 28 entries; 5 kept; 30 is the page so none are cut by count.
    assert_eq!(v["left_out"], 23, "{v}");
    for x in &r[..4] {
        assert_eq!(x["default_branch"], Value::Null, "{x}");
    }
    assert_eq!(r[4]["default_branch"], "feature/x-1");
    assert_eq!(r[1]["updated_ms"], Value::Null, "not a timestamp");
    assert_eq!(
        r[2]["updated_ms"],
        Value::Null,
        "a bare number: unit unknown"
    );
    assert_eq!(r[1]["push"], Value::Null, "a string is not a boolean");
    let d = r[1]["description"].as_str().unwrap();
    assert!(d.chars().count() <= forge_repos::MAX_DESCRIPTION, "{d}");
    // 10,000 entries: one page of ours, the rest counted. (A changed
    // api_base is not answered from the cache of the old one.)
    github_at(&t, "/many");
    let (st, v) = list(&t, &alice, &id, "").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["repositories"].as_array().unwrap().len(), 30);
    assert_eq!(v["left_out"], 10_000 - 30);
    // A stream that never ends is cut at the time limit.
    github_at(&t, "/drip");
    let started = std::time::Instant::now();
    let (st, e) = list(&t, &alice, &id, "?q=drip").await;
    assert_eq!(st, StatusCode::BAD_GATEWAY, "{e}");
    assert!(started.elapsed() < Duration::from_secs(8));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_search_word_and_page_cannot_change_the_request() {
    let t = start().await;
    let [_, (alice, _), _] = users(&t).await;
    let gl = connect(&t, &alice, "localhost", TOKEN_A).await;
    // Everything that could break out of a parameter is one value.
    let word = "a&b=c#d?e%2F/../x y;page=9";
    let enc: String = url_encode(word);
    let (st, v) = list(&t, &alice, &gl, &format!("?q={enc}&page=2")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let seen = t.forge.last().path_and_query;
    assert_eq!(
        seen,
        "/gl/projects?membership=true&order_by=last_activity_at&sort=desc&per_page=30&page=2\
         &search=a%26b%3Dc%23d%3Fe%252F%2F..%2Fx+y%3Bpage%3D9&search_namespaces=true"
    );
    let q = query_of(&seen);
    assert_eq!(q.iter().filter(|(k, _)| k == "search").count(), 1);
    assert_eq!(param(&seen, "search"), word);
    assert_eq!(param(&seen, "page"), "2");
    // Refused here, with no request made.
    let before = t.forge.count();
    let long = "x".repeat(forge_repos::MAX_QUERY + 1);
    for bad in [
        format!("?q={}", url_encode("a\u{0007}b")),
        format!("?q={}", url_encode("a\nb")),
        format!("?q={}", url_encode("a\u{202E}b")),
        format!("?q={long}"),
        "?page=0".into(),
        "?page=-1".into(),
        "?page=11".into(),
        "?page=99999999999999999999".into(),
        "?page=abc".into(),
        "?page=".into(),
        "?page=1.5".into(),
        "?page=%2B1".into(),
        "?q=a&q=b".into(),
        "?page=1&page=2".into(),
        "?api_base=http://evil".into(),
        "?q=%FF".into(),
    ] {
        let (st, e) = list(&t, &alice, &gl, &bad).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{bad}: {e}");
        assert!(!e.to_string().contains("evil"), "{e}");
    }
    assert_eq!(t.forge.count(), before, "no request reached the forge");
    // The last page we serve says there is no more, whatever the forge says.
    let (st, v) = list(&t, &alice, &gl, &format!("?page={}", forge_repos::MAX_PAGE)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["has_more"], false);
    // Blank and spaced words are trimmed; empty is no search.
    let (st, v) = list(&t, &alice, &gl, "?q=%20%20").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["search"], "none");
    let last = t.forge.last().path_and_query;
    assert!(
        !last.contains("search=") && !last.contains("search_namespaces"),
        "{last}"
    );
}

fn url_encode(s: &str) -> String {
    let mut u = reqwest::Url::parse("http://x/").unwrap();
    u.query_pairs_mut().append_pair("v", s);
    u.query().unwrap()[2..].to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn floods_meet_the_limit_and_the_cache() {
    let t = start().await;
    let [_, (alice, _), (bob, _)] = users(&t).await;
    let id = connect(&t, &alice, "127.0.0.1", TOKEN_A).await;
    // The same request again is answered from the cache: one forge call.
    let before = t.forge.count();
    for _ in 0..5 {
        let (st, _) = list(&t, &alice, &id, "?q=harness").await;
        assert_eq!(st, StatusCode::OK);
    }
    assert_eq!(t.forge.count(), before + 1);
    // Distinct words: the per-person limit, then nothing reaches the forge.
    let mut limited = None;
    for i in 0..(forge_repos::LIST_PER_USER + 5) {
        let (st, _) = list(&t, &alice, &id, &format!("?q=w{i}")).await;
        if st == StatusCode::TOO_MANY_REQUESTS {
            limited = Some(i);
            break;
        }
        assert_eq!(st, StatusCode::OK);
    }
    assert_eq!(
        limited,
        Some(forge_repos::LIST_PER_USER - 1),
        "one call was used above"
    );
    let before = t.forge.count();
    let (st, _) = list(&t, &alice, &id, "?q=more").await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(t.forge.count(), before);
    // A cached answer still comes while she is limited.
    let (st, _) = list(&t, &alice, &id, "?q=harness").await;
    assert_eq!(st, StatusCode::OK);
    // Bob is not held up by her.
    let bobs = connect(&t, &bob, "127.0.0.1", TOKEN_B).await;
    let (st, _) = list(&t, &bob, &bobs, "?q=harness").await;
    assert_eq!(st, StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_replaced_token_is_not_answered_from_the_old_cache() {
    let t = start().await;
    let [_, (alice, _), _] = users(&t).await;
    let id = connect(&t, &alice, "127.0.0.1", TOKEN_A).await;
    let (st, _) = list(&t, &alice, &id, "").await;
    assert_eq!(st, StatusCode::OK);
    // Replace the token: the next listing asks the forge with the new one.
    let (st, v) = req(
        &t,
        Method::PUT,
        &format!("/api/connections/{id}/token"),
        Some(json!({"token": TOKEN_A2})),
        Some(&alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let before = t.forge.count();
    let (st, _) = list(&t, &alice, &id, "").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(t.forge.count(), before + 1);
    assert_eq!(
        t.forge.last().auth.as_deref(),
        Some(format!("Bearer {TOKEN_A2}").as_str())
    );
    // Pasting the same account again (Connect) replaces it too.
    let again = connect(&t, &alice, "127.0.0.1", TOKEN_A).await;
    assert_eq!(again, id);
    let before = t.forge.count();
    let (st, _) = list(&t, &alice, &id, "").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(t.forge.count(), before + 1);
    assert_eq!(
        t.forge.last().auth.as_deref(),
        Some(format!("Bearer {TOKEN_A}").as_str())
    );
    // Disconnected: gone, cache and all.
    let (st, _) = req(
        &t,
        Method::DELETE,
        &format!("/api/connections/{id}"),
        None,
        Some(&alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = list(&t, &alice, &id, "").await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    assert_eq!(t.master.forge_repo_cache_len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_host_no_longer_allowed_is_refused_without_a_call() {
    let t = start().await;
    let [_, (alice, _), _] = users(&t).await;
    let id = connect(&t, &alice, "127.0.0.1", TOKEN_A).await;
    let (st, _) = list(&t, &alice, &id, "").await;
    assert_eq!(st, StatusCode::OK);
    t.master
        .set_forge_hosts(vec![t.forge.host("localhost", ForgeKind::Gitlab, "/gl")]);
    let before = t.forge.count();
    for q in ["", "?q=x"] {
        let (st, e) = list(&t, &alice, &id, q).await;
        assert_eq!(st, StatusCode::CONFLICT, "{q}: {e}");
        assert!(e["error"].as_str().unwrap().contains("no longer allows"));
    }
    assert_eq!(t.forge.count(), before, "not even from the cache");
}

/// A fresh master on `t`'s database and data directory, as after a restart.
async fn restart(t: &T) -> T {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let m = Master::new(
        t.store.clone(),
        Auth::new(
            Secret::new("api-token-xyz".into()),
            vec![addr.to_string()],
            false,
        ),
    );
    harness_master::forge::open(&m, t.dir.path());
    m.set_forge_hosts(t.master.forge_hosts());
    let m2 = m.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        addr,
        master: m,
        store: t.store.clone(),
        forge: t.forge.clone(),
        dir: tempfile::tempdir().unwrap(),
        answers: Arc::default(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_missing_or_wrong_key_refuses_before_any_call() {
    let t = start().await;
    let [_, (alice, _), _] = users(&t).await;
    let id = connect(&t, &alice, "127.0.0.1", TOKEN_A).await;
    let key = t.dir.path().join("keys/secrets.key");
    let good = std::fs::read(&key).unwrap();
    // A key of another backup.
    std::fs::write(&key, [0x5au8; 32]).unwrap();
    let t2 = restart(&t).await;
    let before = t.forge.count();
    let (st, e) = list(&t2, &alice, &id, "").await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{e}");
    assert!(e["error"].as_str().unwrap().contains("admin"), "{e}");
    assert_eq!(t.forge.count(), before);
    // No key at all.
    std::fs::remove_file(&key).unwrap();
    let t3 = restart(&t).await;
    let (st, e) = list(&t3, &alice, &id, "").await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{e}");
    assert_eq!(t.forge.count(), before);
    assert!(!key.exists(), "no key is made by listing");
    // The right key back: it works.
    std::fs::write(&key, good).unwrap();
    std::fs::set_permissions(&key, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
    let t4 = restart(&t).await;
    let (st, e) = list(&t4, &alice, &id, "").await;
    assert_eq!(st, StatusCode::OK, "{e}");
    assert_eq!(t.forge.count(), before + 1);
}

/// Failed listings count against the host's shared budget of failures
/// (step 1's), and a full budget refuses a listing before any request.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn failed_listings_use_the_hosts_shared_budget() {
    let t = start().await;
    let [(alex, _), (alice, _), (bob, _)] = users(&t).await;
    let carol = member(&t, &alex, "carol").await;
    let mut ids = Vec::new();
    for (c, tok) in [
        (&alex, TOKEN_A),
        (&alice, TOKEN_A),
        (&bob, TOKEN_B),
        (&carol, TOKEN_B),
    ] {
        ids.push((c.clone(), connect(&t, c, "127.0.0.1", tok).await));
    }
    github_at(&t, "/e500");
    // Three people, 20 failed listings each: the host's 60.
    for (c, id) in &ids[..3] {
        for i in 0..forge_repos::LIST_PER_USER {
            let (st, e) = list(&t, c, id, &format!("?q=f{i}")).await;
            assert_eq!(st, StatusCode::BAD_GATEWAY, "{e}");
        }
    }
    let before = t.forge.count();
    let (c, id) = &ids[3];
    let (st, e) = list(&t, c, id, "").await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS, "{e}");
    assert!(e["error"].as_str().unwrap().contains("failed"), "{e}");
    assert_eq!(t.forge.count(), before, "no request");
}

/// Review of PR 158, HIGH 2: refusals of one person's token do not use up
/// the host's shared budget (three people, 20 refused listings each, were
/// 60 failures: everyone's listings and connects to that host were then
/// refused); a refusal is remembered for its token, so a retry makes no
/// forge call; a new token is asked at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refused_token_does_not_lock_the_host_for_others() {
    let t = start().await;
    let [(alex, _), (alice, _), (bob, _)] = users(&t).await;
    let carol = member(&t, &alex, "carol").await;
    let mut revoked = Vec::new();
    for c in [&alex, &alice, &carol] {
        revoked.push((c.clone(), connect(&t, c, "127.0.0.1", TOKEN_A).await));
    }
    let bobs = connect(&t, &bob, "127.0.0.1", TOKEN_B).await;
    github_at(&t, "/reva");
    let before = t.forge.count();
    for (c, id) in &revoked {
        for i in 0..forge_repos::LIST_PER_USER {
            let (st, e) = list(&t, c, id, &format!("?q=r{i}")).await;
            assert_eq!(st, StatusCode::BAD_GATEWAY, "{e}");
            assert!(
                e["error"].as_str().unwrap().contains("replace the token"),
                "{e}"
            );
        }
    }
    assert_eq!(
        t.forge.count(),
        before + 3,
        "one forge call per refused token"
    );
    assert_eq!(
        t.master.forge_host_failures("127.0.0.1"),
        0,
        "refusals are not the host's failures"
    );
    // Bob lists and connects to that host as before.
    let (st, e) = list(&t, &bob, &bobs, "?q=bob").await;
    assert_eq!(st, StatusCode::OK, "{e}");
    let (st, e) = req(
        &t,
        Method::POST,
        "/api/connections",
        Some(json!({"host": "127.0.0.1", "token": TOKEN_B})),
        Some(&bob),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{e}");
    // Alice's retry: the same refusal, no forge call.
    let (alice_c, alice_id) = &revoked[1];
    let before = t.forge.count();
    let (st, _) = list(&t, alice_c, alice_id, "?q=retry").await;
    assert_eq!(st, StatusCode::BAD_GATEWAY);
    assert_eq!(t.forge.count(), before);
    // A new token is asked at once.
    let (st, v) = req(
        &t,
        Method::PUT,
        &format!("/api/connections/{alice_id}/token"),
        Some(json!({"token": TOKEN_A2})),
        Some(alice_c),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let before = t.forge.count();
    let (st, e) = list(&t, alice_c, alice_id, "?q=retry").await;
    assert_eq!(st, StatusCode::OK, "{e}");
    assert_eq!(t.forge.count(), before + 1);
}

/// Review of PR 158, HIGH 1: at most two listings are in flight on the
/// server; a third is refused at once (not queued); a slot is given back
/// on every path: a timeout, an error, a request the client gave up.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn listings_in_flight_are_limited_and_always_released() {
    let t = Arc::new(start().await);
    let [(alex, _), (alice, _), (bob, _)] = users(&t).await;
    let mut ids = Vec::new();
    for (c, tok) in [(&alex, TOKEN_A), (&alice, TOKEN_A), (&bob, TOKEN_B)] {
        ids.push((c.clone(), connect(&t, c, "127.0.0.1", tok).await));
    }
    github_at(&t, "/hang");
    let spawn = |i: usize, q: &str| {
        let (t, (c, id), q) = (t.clone(), ids[i].clone(), q.to_string());
        tokio::spawn(async move { list(&t, &c, &id, &q).await })
    };
    let (a, b) = (spawn(0, "?q=h1"), spawn(1, "?q=h2"));
    tokio::time::sleep(Duration::from_millis(400)).await;
    let before = t.forge.count();
    let started = std::time::Instant::now();
    let (st, e) = list(&t, &ids[2].0, &ids[2].1, "?q=third").await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS, "{e}");
    assert!(e["error"].as_str().unwrap().contains("busy"), "{e}");
    assert!(started.elapsed() < Duration::from_millis(500), "not queued");
    assert_eq!(t.forge.count(), before, "no request");
    // Both time out (2 s here): their slots come back.
    for h in [a, b] {
        let (st, e) = h.await.unwrap();
        assert_eq!(st, StatusCode::BAD_GATEWAY, "{e}");
    }
    github_at(&t, "/gh");
    let (st, e) = list(&t, &ids[2].0, &ids[2].1, "?q=after-timeouts").await;
    assert_eq!(st, StatusCode::OK, "{e}");
    // Errors give them back too.
    github_at(&t, "/e500");
    for (c, id) in &ids {
        let (st, _) = list(&t, c, id, "?q=err").await;
        assert_eq!(st, StatusCode::BAD_GATEWAY);
    }
    // Requests the client gives up on: the slots come back (when the
    // server drops the request, or at the latest at the call's limit).
    github_at(&t, "/hang");
    let (a, b) = (spawn(0, "?q=gone1"), spawn(1, "?q=gone2"));
    tokio::time::sleep(Duration::from_millis(400)).await;
    a.abort();
    b.abort();
    github_at(&t, "/gh");
    let started = std::time::Instant::now();
    let st = loop {
        let (st, _) = list(&t, &ids[2].0, &ids[2].1, "?q=after-abort").await;
        if st != StatusCode::TOO_MANY_REQUESTS || started.elapsed() > Duration::from_secs(4) {
            break st;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert_eq!(st, StatusCode::OK);
    eprintln!(
        "slots back {:?} after the clients gave up",
        started.elapsed()
    );
}

/// Review of PR 158, HIGH 1: memory. Runs in a child process of this test
/// binary so that no other test shares the measurement.
#[test]
fn hostile_answers_cost_little_memory() {
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "memory_child", "--ignored", "--nocapture"])
        .env("FORGE_MEMORY_CHILD", "1")
        .output()
        .unwrap();
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    eprintln!("{text}");
    assert!(out.status.success() && text.contains("1 passed"), "{text}");
}

fn status_kb(field: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find(|l| l.starts_with(field))
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap()
        .parse()
        .unwrap()
}

/// Peak resident growth (kB) while `f` runs.
async fn peak_growth<F: std::future::Future>(f: F) -> (F::Output, u64) {
    tokio::time::sleep(Duration::from_millis(200)).await;
    let before = status_kb("VmRSS:");
    std::fs::write("/proc/self/clear_refs", "5").unwrap();
    let out = f.await;
    (out, status_kb("VmHWM:").saturating_sub(before))
}

/// One listing's growth, debug build (the bound has room for the
/// allocator; before the fix one such listing took about 60 MB).
const ONE_LISTING_KB: u64 = 12 * 1024;
/// Twenty at once, of which the server runs two.
const TWENTY_LISTINGS_KB: u64 = 24 * 1024;

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "run by hostile_answers_cost_little_memory"]
async fn memory_child() {
    if std::env::var("FORGE_MEMORY_CHILD").is_err() {
        return;
    }
    let t = Arc::new(start().await);
    let [(alex, _), (alice, _), (bob, _)] = users(&t).await;
    let carol = member(&t, &alex, "carol").await;
    let mut ids = Vec::new();
    for c in [&alex, &alice, &bob, &carol] {
        ids.push((c.clone(), connect(&t, c, "127.0.0.1", TOKEN_A).await));
    }
    worst_body("objs");
    for kind in ["/objs", "/zeros"] {
        github_at(&t, kind);
        let ((st, v), kb) =
            peak_growth(list(&t, &ids[0].0, &ids[0].1, &format!("?q=x{kind}"))).await;
        eprintln!(
            "one {kind} listing: {st}, left_out {}, peak growth {kb} kB",
            v["left_out"]
        );
        assert_eq!(st, StatusCode::OK, "{v}");
        assert!(kb < ONE_LISTING_KB, "{kind}: {kb} kB");
    }
    github_at(&t, "/slowobjs");
    let (codes, kb) = peak_growth(async {
        let mut hs = Vec::new();
        for i in 0..20 {
            let (t, (c, id)) = (t.clone(), ids[i % 4].clone());
            hs.push(tokio::spawn(async move {
                list(&t, &c, &id, &format!("?q=p{i}")).await.0
            }));
        }
        let mut codes = std::collections::BTreeMap::new();
        for h in hs {
            *codes.entry(h.await.unwrap().as_u16()).or_insert(0) += 1;
        }
        codes
    })
    .await;
    let after = status_kb("VmRSS:");
    eprintln!("20 at once: {codes:?}, peak growth {kb} kB, RSS after {after} kB");
    assert_eq!(codes.get(&200), Some(&2), "{codes:?}");
    assert_eq!(codes.get(&429), Some(&18), "{codes:?}");
    assert!(kb < TWENTY_LISTINGS_KB, "{kb} kB");
}
