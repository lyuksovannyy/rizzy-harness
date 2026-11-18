//! A person's forge connections (ADOPTION_PLAN X.27 step 1, PLAN.md T83)
//! against a fake forge on 127.0.0.1: the token is checked with the forge,
//! stored sealed, never shown, never logged, never in a URL; others cannot
//! see, use or delete it; hostile hosts and answers are refused; floods and
//! the cap are limited; the account's deletion takes its connections.
//! No real forge and no real token are used.

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
use harness_master::sealed::Sealer;
use harness_master::{Master, router};
use harness_store::{Durability, Store};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

/// Alice's token for forge account 42, a replacement for the same account,
/// Bob's (account 43) and one the forge refuses. Made up; all are searched
/// for everywhere at the end.
const TOKEN_A: &str = "ghp_CANARYalice000000000000000000000001";
const TOKEN_A2: &str = "ghp_CANARYalice000000000000000replaced2";
const TOKEN_B: &str = "ghp_CANARYbob00000000000000000000000003";
const TOKEN_BAD: &str = "ghp_CANARYwrong0000000000000000000000004";
const ALL: [&str; 4] = [TOKEN_A, TOKEN_A2, TOKEN_B, TOKEN_BAD];

/// Everything the process logs, at every level.
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

/// What the fake forge was asked: path and query, and the auth headers.
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

fn bearer(h: &HeaderMap, scheme: &str) -> Option<String> {
    h.get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix(scheme)
        .map(str::to_string)
}

fn gh_user(token: Option<&str>) -> Response {
    let user = match token {
        Some(TOKEN_A | TOKEN_A2) => json!({
            "id": 42, "login": "ada", "name": "Ada \u{202E}<b>Lovelace</b>\u{0007}",
            "email": "ada@example.com"
        }),
        Some(TOKEN_B) => json!({"id": 43, "login": "bob-gh", "name": null, "email": null}),
        Some(t) => {
            // A forge that echoes what it was sent: never shown onward.
            return (AxStatus::UNAUTHORIZED, format!("Bad credentials for {t}")).into_response();
        }
        None => return (AxStatus::UNAUTHORIZED, "no token").into_response(),
    };
    ([("x-oauth-scopes", "repo, read:user")], axum::Json(user)).into_response()
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
        auth,
    });
    let port = f.addr.get().unwrap().port();
    match req.uri().path() {
        "/gh/user" => gh_user(bearer(&h, "Bearer ").as_deref()),
        "/gl/user" => match h.get("private-token").and_then(|v| v.to_str().ok()) {
            Some(TOKEN_A) => axum::Json(json!({
                "id": 7, "username": "ada.gl", "name": "Ada GL",
                "email": "ada@private.example", "commit_email": "ada@commits.example"
            }))
            .into_response(),
            _ => (AxStatus::UNAUTHORIZED, "401").into_response(),
        },
        "/gl/personal_access_tokens/self" => {
            axum::Json(json!({"scopes": ["api", "read_user", "<b>x</b>"]})).into_response()
        }
        "/gt/user" => match bearer(&h, "token ").as_deref() {
            Some(TOKEN_A) => axum::Json(json!({
                "id": 9, "login": "ada", "full_name": "", "email": "ada@noreply.localhost"
            }))
            .into_response(),
            _ => (AxStatus::UNAUTHORIZED, "401").into_response(),
        },
        "/e500/user" => (
            AxStatus::INTERNAL_SERVER_ERROR,
            format!("internal error, token was {h:?}"),
        )
            .into_response(),
        "/hang/user" => {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            "late".into_response()
        }
        "/drip/user" => {
            let s = futures::stream::unfold((), |_| async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                Some((
                    Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b" ")),
                    (),
                ))
            });
            Response::new(Body::from_stream(s))
        }
        "/big/user" => {
            // 50 MB of a JSON string, never with a length up front.
            let chunk = axum::body::Bytes::from(vec![b'a'; 64 * 1024]);
            let head = futures::stream::once(async {
                Ok::<_, std::io::Error>(axum::body::Bytes::from_static(
                    b"{\"id\":1,\"login\":\"x\",\"name\":\"",
                ))
            });
            let body = futures::stream::iter((0..800).map(move |_| Ok(chunk.clone())));
            Response::new(Body::from_stream(head.chain(body)))
        }
        "/redir-host/user" => (
            AxStatus::FOUND,
            [("location", format!("http://localhost:{port}/gh/user"))],
        )
            .into_response(),
        "/redir-in/user" => (
            AxStatus::FOUND,
            [(
                "location",
                "http://169.254.169.254/latest/meta-data/".to_string(),
            )],
        )
            .into_response(),
        "/html/user" => {
            axum::Json(json!({"id": 5, "login": "<img src=x onerror=alert(1)>"})).into_response()
        }
        "/noid/user" => axum::Json(json!({"id": "42; DROP", "login": "ada"})).into_response(),
        _ => (AxStatus::NOT_FOUND, "no").into_response(),
    }
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
    /// A host entry for this fake: `host` (127.0.0.1 or localhost), `path`
    /// its API under the fake.
    fn host(&self, host: &str, kind: ForgeKind, path: &str) -> ForgeHost {
        ForgeHost {
            host: host.into(),
            kind,
            api_base: format!("http://{host}:{}{path}", self.addr.get().unwrap().port()),
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
    /// Every answer this test's HTTP calls got: status, headers, body.
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

/// `who`: a cookie, `Some("api")` for the API token, `None` for nobody.
async fn req(
    t: &T,
    method: Method,
    path: &str,
    body: Option<Value>,
    who: Option<&str>,
) -> (StatusCode, Value) {
    let mut rb = reqwest::Client::new().request(method, format!("{}{path}", t.base));
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
    let mut rb = reqwest::Client::new()
        .post(format!("{}{path}", t.base))
        .json(&body);
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

async fn connect(t: &T, c: &str, host: &str, token: &str) -> (StatusCode, Value) {
    req(
        t,
        Method::POST,
        "/api/connections",
        Some(json!({"host": host, "token": token})),
        Some(c),
    )
    .await
}

/// A UI socket of `c` whose frames land in the returned buffer.
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

/// Every byte under the data directory (master.db, its WAL, the key, the
/// audit table inside), the logs, the answers, the frames and what the
/// forge saw in its URLs: none holds a token in the clear.
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
            // Not even the part after a common prefix.
            assert!(!text.contains(&tok[8..]), "part of {tok} found in {what}");
        }
    }
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn connect_replace_disconnect_and_the_token_is_nowhere() {
    let t = start().await;
    let [_, (alice, alice_id), _] = users(&t).await;
    let frames = socket(&t, &alice).await;

    let (st, v) = req(&t, Method::GET, "/api/connections", None, Some(&alice)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["connections"], json!([]));
    assert_eq!(v["hosts"][0]["host"], "127.0.0.1");
    assert!(
        v["hosts"][0]["scopes_needed"]
            .as_str()
            .unwrap()
            .contains("repo")
    );

    // Connect: checked with the forge, the account kept, the token not shown.
    let (st, c) = connect(&t, &alice, "127.0.0.1", &format!("  {TOKEN_A}\n")).await;
    assert_eq!(st, StatusCode::OK, "{c}");
    assert_eq!(c["login"], "ada");
    assert_eq!(c["account_id"], "42");
    assert_eq!(
        c["display_name"], "Ada <b>Lovelace</b>",
        "hidden characters go"
    );
    assert_eq!(c["commit_email"], "ada@example.com");
    assert_eq!(c["scopes"], json!(["repo", "read:user"]));
    let id = c["id"].as_str().unwrap().to_string();
    // The forge got the token in the header, nowhere else.
    {
        let seen = t.forge.seen.lock().unwrap();
        let last = seen.last().unwrap();
        assert_eq!(last.path_and_query, "/gh/user");
        assert_eq!(
            last.auth.as_deref(),
            Some(format!("Bearer {TOKEN_A}").as_str())
        );
    }
    // Stored sealed and bound to this row: it opens as the token.
    let tok = t
        .master
        .connection_token(&alice_id, &id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(tok.expose(), TOKEN_A);
    assert!(t.dir.path().join("keys/secrets.key").exists());

    // GitLab: its own header, the commit e-mail, the scopes from its own call.
    let (st, g) = connect(&t, &alice, "localhost", TOKEN_A).await;
    assert_eq!(st, StatusCode::OK, "{g}");
    assert_eq!(g["login"], "ada.gl");
    assert_eq!(g["commit_email"], "ada@commits.example");
    assert_eq!(g["scopes"], json!(["api", "read_user"]));

    // The same account again replaces the token, keeping the one row.
    let (st, again) = connect(&t, &alice, "127.0.0.1", TOKEN_A2).await;
    assert_eq!(st, StatusCode::OK, "{again}");
    assert_eq!(again["id"], id.as_str());
    let (_, l) = req(&t, Method::GET, "/api/connections", None, Some(&alice)).await;
    assert_eq!(l["connections"].as_array().unwrap().len(), 2);
    assert_eq!(
        t.master
            .connection_token(&alice_id, &id)
            .await
            .unwrap()
            .unwrap()
            .expose(),
        TOKEN_A2
    );
    // Replace token: another account's token is refused, the same one's taken.
    let path = format!("/api/connections/{id}/token");
    let (st, e) = req(
        &t,
        Method::PUT,
        &path,
        Some(json!({"token": TOKEN_B})),
        Some(&alice),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{e}");
    assert!(e["error"].as_str().unwrap().contains("@bob-gh"), "{e}");
    let (st, _) = req(
        &t,
        Method::PUT,
        &path,
        Some(json!({"token": TOKEN_A})),
        Some(&alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // A refused token: the forge's body (which echoes it) is not passed on.
    let (st, e) = connect(&t, &alice, "127.0.0.1", TOKEN_BAD).await;
    assert_eq!(st, StatusCode::BAD_GATEWAY, "{e}");
    assert!(e["error"].as_str().unwrap().contains("HTTP 401"), "{e}");
    assert!(!e.to_string().contains("Bad credentials"), "{e}");

    // A forge that answers 500, and one that never answers.
    t.master.set_forge_hosts(vec![
        t.forge.host("127.0.0.1", ForgeKind::Github, "/e500"),
        t.forge.host("localhost", ForgeKind::Github, "/hang"),
    ]);
    let (st, e) = connect(&t, &alice, "127.0.0.1", TOKEN_B).await;
    assert_eq!(st, StatusCode::BAD_GATEWAY, "{e}");
    assert!(e["error"].as_str().unwrap().contains("HTTP 500"), "{e}");
    assert!(!e.to_string().contains("internal error"), "{e}");
    let started = std::time::Instant::now();
    let (st, e) = connect(&t, &alice, "localhost", TOKEN_B).await;
    assert_eq!(st, StatusCode::BAD_GATEWAY, "{e}");
    assert!(e["error"].as_str().unwrap().contains("in time"), "{e}");
    assert!(started.elapsed() < Duration::from_secs(10));

    // Disconnect.
    let (st, _) = req(
        &t,
        Method::DELETE,
        &format!("/api/connections/{id}"),
        None,
        Some(&alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        t.master
            .connection_token(&alice_id, &id)
            .await
            .unwrap()
            .is_none()
    );
    let (st, _) = req(
        &t,
        Method::DELETE,
        &format!("/api/connections/{id}"),
        None,
        Some(&alice),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // The audit names who, which host and which account; never a token.
    let c = rusqlite::Connection::open(t.dir.path().join("master.db")).unwrap();
    let n: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM audit WHERE kind = 'forge_connection'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 5, "connect, connect, connect again, replace, disconnect");

    tokio::time::sleep(Duration::from_millis(300)).await;
    let frames = frames.lock().unwrap().clone();
    assert!(
        frames.contains("connections"),
        "the list change reached the socket: {frames}"
    );
    // The store is flushed for the file scan.
    drop(c);
    t.store.call(|s| s.sealed_count()).await.unwrap();
    assert_no_token_anywhere(&t, &frames);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn others_cannot_see_use_or_delete_a_connection() {
    let t = start().await;
    let [(alex, _), (alice, alice_id), (bob, _)] = users(&t).await;
    let (_, c) = connect(&t, &alice, "127.0.0.1", TOKEN_A).await;
    let id = c["id"].as_str().unwrap().to_string();
    for who in [&alex, &bob] {
        let (st, l) = req(&t, Method::GET, "/api/connections", None, Some(who)).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(
            l["connections"],
            json!([]),
            "a server admin sees none of hers either"
        );
        let (st, _) = req(
            &t,
            Method::DELETE,
            &format!("/api/connections/{id}"),
            None,
            Some(who),
        )
        .await;
        assert_eq!(st, StatusCode::NOT_FOUND);
        let (st, _) = req(
            &t,
            Method::PUT,
            &format!("/api/connections/{id}/token"),
            Some(json!({"token": TOKEN_A})),
            Some(who),
        )
        .await;
        assert_eq!(st, StatusCode::NOT_FOUND);
    }
    // The API token has no account: no route of this area takes it.
    for (m, p, b) in [
        (Method::GET, "/api/connections".to_string(), None),
        (
            Method::POST,
            "/api/connections".to_string(),
            Some(json!({"host": "127.0.0.1", "token": TOKEN_B})),
        ),
        (Method::DELETE, format!("/api/connections/{id}"), None),
        (
            Method::PUT,
            format!("/api/connections/{id}/token"),
            Some(json!({"token": TOKEN_A})),
        ),
    ] {
        let (st, e) = req(&t, m.clone(), &p, b, Some("api")).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{m} {p}: {e}");
    }
    let (st, _) = req(&t, Method::GET, "/api/connections", None, None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    // Bob's own connection of the same forge account is his, apart.
    let (st, b) = connect(&t, &bob, "127.0.0.1", TOKEN_A).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    assert_ne!(b["id"], c["id"]);
    assert!(
        t.master
            .connection_token(&alice_id, &id)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hosts_not_listed_or_spelled_otherwise_are_refused_without_a_call() {
    let t = start().await;
    let [_, (alice, _), _] = users(&t).await;
    let port = t.forge.addr.get().unwrap().port();
    let before = t.forge.count();
    for host in [
        "github.com".to_string(),
        "evil.example".into(),
        format!("http://127.0.0.1:{port}/gh"),
        format!("127.0.0.1:{port}"),
        "user@127.0.0.1".into(),
        "127.0.0.1/gh".into(),
        "LOCALHOST".into(),
        "localhost.".into(),
        "lоcalhost".into(), // Cyrillic о
        "169.254.169.254".into(),
        "".into(),
    ] {
        let (st, e) = connect(&t, &alice, &host, TOKEN_A).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{host}: {e}");
        assert!(
            e["error"].as_str().unwrap().contains("not a forge host"),
            "{host}: {e}"
        );
    }
    // A malformed body is refused with words that quote none of it.
    for body in [
        json!({"host": "127.0.0.1"}),
        json!({"host": "127.0.0.1", "token": TOKEN_A, "api_base": "http://x"}),
        json!({"host": 5, "token": TOKEN_A}),
    ] {
        let (st, e) = req(
            &t,
            Method::POST,
            "/api/connections",
            Some(body),
            Some(&alice),
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{e}");
        assert!(!e.to_string().contains("CANARY"), "{e}");
    }
    for bad in ["", "a b", "x\r\nX-Evil: 1", &"a".repeat(513)] {
        let (st, e) = connect(&t, &alice, "127.0.0.1", bad).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{e}");
    }
    assert_eq!(t.forge.count(), before, "no request reached the forge");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hostile_forge_answers_are_refused() {
    let t = start().await;
    let [_, (alice, _), _] = users(&t).await;
    let cases = [
        ("/redir-host", "HTTP 302"),
        ("/redir-in", "HTTP 302"),
        ("/big", "too large"),
        ("/drip", "in time"),
        ("/html", "account name"),
        ("/noid", "account id"),
    ];
    for (path, needle) in cases {
        t.master
            .set_forge_hosts(vec![t.forge.host("127.0.0.1", ForgeKind::Github, path)]);
        let before = t.forge.count();
        let started = std::time::Instant::now();
        let (st, e) = connect(&t, &alice, "127.0.0.1", TOKEN_A).await;
        assert_eq!(st, StatusCode::BAD_GATEWAY, "{path}: {e}");
        assert!(e["error"].as_str().unwrap().contains(needle), "{path}: {e}");
        assert!(started.elapsed() < Duration::from_secs(8), "{path}");
        // One request: no redirect was followed to another host.
        assert_eq!(t.forge.count(), before + 1, "{path}");
    }
    let (_, l) = req(&t, Method::GET, "/api/connections", None, Some(&alice)).await;
    assert_eq!(l["connections"], json!([]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_private_address_needs_the_entry_to_say_so() {
    let t = start().await;
    let [_, (alice, _), _] = users(&t).await;
    let mut h = t.forge.host("127.0.0.1", ForgeKind::Gitea, "/gt");
    h.private_address = false;
    t.master.set_forge_hosts(vec![h.clone()]);
    let before = t.forge.count();
    let (st, e) = connect(&t, &alice, "127.0.0.1", TOKEN_A).await;
    assert_eq!(st, StatusCode::BAD_GATEWAY, "{e}");
    assert!(
        e["error"].as_str().unwrap().contains("private_address"),
        "{e}"
    );
    assert_eq!(t.forge.count(), before);
    // With it, the same entry works (Gitea's header and fields).
    h.private_address = true;
    t.master.set_forge_hosts(vec![h]);
    let (st, c) = connect(&t, &alice, "127.0.0.1", TOKEN_A).await;
    assert_eq!(st, StatusCode::OK, "{c}");
    assert_eq!(
        (c["login"].as_str(), c["display_name"].is_null()),
        (Some("ada"), true)
    );
    assert_eq!(c["scopes"], Value::Null, "Gitea does not report scopes");
    assert_eq!(
        t.forge.seen.lock().unwrap().last().unwrap().auth.as_deref(),
        Some(format!("token {TOKEN_A}").as_str())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn floods_are_limited() {
    let t = start().await;
    let [_, (alice, _), (bob, _)] = users(&t).await;
    let mut limited = None;
    for i in 0..15 {
        let (st, _) = connect(&t, &alice, "127.0.0.1", TOKEN_BAD).await;
        if st == StatusCode::TOO_MANY_REQUESTS {
            limited = Some(i);
            break;
        }
        assert_eq!(st, StatusCode::BAD_GATEWAY);
    }
    assert_eq!(limited, Some(harness_master::forge::PER_USER));
    let before = t.forge.count();
    let (st, _) = connect(&t, &alice, "127.0.0.1", TOKEN_A).await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        t.forge.count(),
        before,
        "a limited attempt reaches no forge"
    );
    // Another person is not held up by her.
    let (st, _) = connect(&t, &bob, "127.0.0.1", TOKEN_B).await;
    assert_eq!(st, StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_cap_and_the_account_deletion() {
    let t = start().await;
    let [(alex, _), (alice, alice_id), _] = users(&t).await;
    let (_, c) = connect(&t, &alice, "127.0.0.1", TOKEN_A).await;
    assert!(c["id"].is_string(), "{c}");
    // Fill her to the cap directly (the attempt limit stops a person
    // pasting 20 tokens in one go anyway).
    for i in 1..harness_master::forge::MAX_PER_USER {
        t.store
            .connection_put(
                &harness_store::connections::NewConnection {
                    connection_id: format!("filler{i}"),
                    user_id: alice_id.clone(),
                    host: "localhost".into(),
                    account_id: i.to_string(),
                    login: format!("f{i}"),
                    display_name: None,
                    commit_email: None,
                    scopes: None,
                    sealed_token: vec![1],
                },
                harness_master::forge::MAX_PER_USER,
            )
            .unwrap();
    }
    let (st, e) = connect(&t, &alice, "127.0.0.1", TOKEN_B).await;
    assert_eq!(st, StatusCode::CONFLICT, "{e}");
    assert!(e["error"].as_str().unwrap().contains("20"), "{e}");
    // Replacing an existing account's token still works at the cap.
    let (st, _) = connect(&t, &alice, "127.0.0.1", TOKEN_A2).await;
    assert_eq!(st, StatusCode::OK);

    let (st, _) = req(
        &t,
        Method::DELETE,
        &format!("/api/users/{alice_id}"),
        None,
        Some(&alex),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(t.store.connections_of(&alice_id).unwrap().is_empty());
    assert_eq!(t.store.sealed_count().unwrap(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sealed_tokens_stay_with_their_row_and_their_key() {
    let t = start().await;
    let [_, (alice, alice_id), (bob, bob_id)] = users(&t).await;
    let (_, a) = connect(&t, &alice, "127.0.0.1", TOKEN_A).await;
    let (_, b) = connect(&t, &bob, "127.0.0.1", TOKEN_B).await;
    let (a, b) = (a["id"].as_str().unwrap(), b["id"].as_str().unwrap());
    // Alice's sealed token copied onto Bob's row does not open as his.
    let db = rusqlite::Connection::open(t.dir.path().join("master.db")).unwrap();
    db.execute(
        "UPDATE user_connections SET sealed_token =
           (SELECT sealed_token FROM user_connections WHERE connection_id = ?1)
         WHERE connection_id = ?2",
        [a, b],
    )
    .unwrap();
    let e = t.master.connection_token(&bob_id, b).await.unwrap_err();
    assert!(e.to_string().contains("did not open"), "{e}");
    assert!(!e.to_string().contains("CANARY"));
    assert_eq!(
        t.master
            .connection_token(&alice_id, a)
            .await
            .unwrap()
            .unwrap()
            .expose(),
        TOKEN_A
    );

    // A copy of master.db without secrets.key opens nothing.
    let other = tempfile::tempdir().unwrap();
    db.execute_batch(&format!(
        "VACUUM INTO '{}'",
        other.path().join("master.db").display()
    ))
    .unwrap();
    let copy = Master::new(
        Store::open(&other.path().join("master.db"), Durability::Normal).unwrap(),
        Auth::new(Secret::new("x".into()), vec![], false),
    );
    copy.set_sealer(Sealer::new(other.path()));
    let e = copy.connection_token(&alice_id, a).await.unwrap_err();
    assert!(e.to_string().contains("secrets.key is missing"), "{e}");
    assert!(
        !other.path().join("keys/secrets.key").exists(),
        "opening makes no key"
    );

    // secrets.key gone while tokens exist: the server refuses to seal
    // and does not make a new key.
    std::fs::remove_file(t.dir.path().join("keys/secrets.key")).unwrap();
    let fresh = Master::new(
        t.store.clone(),
        Auth::new(Secret::new("y".into()), vec![], false),
    );
    fresh.set_sealer(Sealer::new(t.dir.path()));
    let e = fresh.connection_token(&alice_id, a).await.unwrap_err();
    assert!(e.to_string().contains("Restore"), "{e}");
    assert!(!t.dir.path().join("keys/secrets.key").exists());
}

/// The key file missing while tokens exist, over HTTP: a paste is refused
/// with a pointer to the admin, nothing is stored, no key is made.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lost_key_file_refuses_new_tokens() {
    let t = start().await;
    let [_, (alice, _), (bob, _)] = users(&t).await;
    let (st, _) = connect(&t, &alice, "127.0.0.1", TOKEN_A).await;
    assert_eq!(st, StatusCode::OK);
    // A restart with the file gone (a fresh master on the same database).
    std::fs::remove_file(t.dir.path().join("keys/secrets.key")).unwrap();
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
    tokio::spawn(async move { axum::serve(listener, router(m)).await.unwrap() });
    let t2 = T {
        base: format!("http://{addr}"),
        addr,
        master: t.master.clone(),
        store: t.store.clone(),
        forge: t.forge.clone(),
        dir: tempfile::tempdir().unwrap(),
        answers: Arc::default(),
    };
    let (st, e) = connect(&t2, &bob, "127.0.0.1", TOKEN_B).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{e}");
    assert!(e["error"].as_str().unwrap().contains("admin"), "{e}");
    assert!(
        !t.dir.path().join("keys/secrets.key").exists(),
        "no new key"
    );
    assert_eq!(t.store.sealed_count().unwrap(), 1);
    let logs = String::from_utf8_lossy(&logs().lock().unwrap()).to_string();
    assert!(
        logs.contains("secrets.key is missing"),
        "the log says what to do"
    );
}
