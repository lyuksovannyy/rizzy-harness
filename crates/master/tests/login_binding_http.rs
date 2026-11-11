//! Gate 3 P-1: a login is bound to the page's origin, port included.
//!
//! Browsers do not separate cookies by port (RFC 6265 §8.5), so the login
//! cookie also reaches a server the agent started on another port of the
//! same host, and a non-browser client writes any `Origin` it likes. A
//! login is therefore two values: the cookie, and a login key returned once
//! in the login's answer, kept by the page in its own origin's storage and
//! sent as `x-harness-login` (and as a WebSocket's first message). A cookie
//! alone authenticates nothing that reads a chat or changes state.

mod common;
use common::SameOrigin as _;

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::Secret;
use harness_core::event::Profile;
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, router};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::{Durability, Store};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const KEY_HEADER: &str = "x-harness-login";

/// Everything the process logs, at every level, for every test in this file.
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

impl std::io::Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct T {
    base: String,
    addr: std::net::SocketAddr,
    dir: tempfile::TempDir,
}

/// A browser login: the cookie (`name=value`) and the login key.
#[derive(Clone)]
struct Login {
    cookie: String,
    key: String,
}

async fn node(root: &std::path::Path) -> NodeRuntime {
    NodeRuntime::start(NodeConfig {
        node_id: "local".into(),
        hostname: "h".into(),
        policy: NodePolicy {
            workspace_roots: vec![root.into()],
            ..Default::default()
        },
        store: Store::open_in_memory().unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(1),
            }),
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    })
    .await
    .unwrap()
}

/// A server with an agentic node, its store on disk (so the test can
/// search the database files), remote nodes on.
async fn start() -> T {
    logs();
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim = dir.path().join("claim-code");
    std::fs::write(&claim, "CLAIM").unwrap();
    let store = Store::open(&dir.path().join("master.db"), Durability::Normal).unwrap();
    let master = Master::new(
        store,
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim,
        }),
    );
    master.set_agents(harness_agents::AgentSet::load(&dir.path().join("none")).0);
    master.set_node_listener(7443, "fp".into());
    let work = dir.path().join("work");
    std::fs::create_dir(&work).unwrap();
    master
        .add_link(node(&work).await.attach().await, true)
        .await
        .unwrap();
    tokio::spawn(async move { axum::serve(listener, router(master)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        addr,
        dir,
    }
}

fn set_cookie(r: &reqwest::Response) -> Option<String> {
    r.headers()
        .get("set-cookie")
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string())
}

/// What the browser sends: the cookie, the page's own `Origin`, and the
/// key when the page has it.
async fn call(
    t: &T,
    method: Method,
    path: &str,
    body: Option<Value>,
    cookie: &str,
    key: Option<&str>,
) -> (StatusCode, Value) {
    let mut rb = reqwest::Client::new()
        .request(method, format!("{}{path}", t.base))
        .header("cookie", cookie)
        .same_origin();
    if let Some(k) = key {
        rb = rb.header(KEY_HEADER, k);
    }
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    let r = rb.send().await.unwrap();
    (r.status(), r.json().await.unwrap_or(Value::Null))
}

async fn as_login(
    t: &T,
    method: Method,
    path: &str,
    body: Option<Value>,
    l: &Login,
) -> (StatusCode, Value) {
    call(t, method, path, body, &l.cookie, Some(&l.key)).await
}

fn login_of(cookie: Option<String>, v: &Value) -> Login {
    Login {
        cookie: cookie.expect("a session cookie"),
        // Checked where it matters: a missing key fails the refusals.
        key: v["login_key"].as_str().unwrap_or_default().to_string(),
    }
}

async fn claim(t: &T) -> Login {
    let r = reqwest::Client::new()
        .post(format!("{}/api/setup", t.base))
        .json(&json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let c = set_cookie(&r);
    login_of(c, &r.json().await.unwrap())
}

async fn login(t: &T) -> Login {
    let r = reqwest::Client::new()
        .post(format!("{}/api/login", t.base))
        .json(&json!({"username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let c = set_cookie(&r);
    login_of(c, &r.json().await.unwrap())
}

/// An agentic chat with an approval pending: (chat id, approval id).
async fn pending_approval(t: &T, l: &Login) -> (String, String) {
    let cwd = t.dir.path().join("work").display().to_string();
    let (st, v) = as_login(
        t,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "agentic", "cwd": cwd})),
        l,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap().to_string();
    let (st, v) = as_login(
        t,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({"text": "run: echo hi"})),
        l,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    for _ in 0..500 {
        let (_, v) = as_login(
            t,
            Method::GET,
            &format!("/api/sessions/{sid}/events"),
            None,
            l,
        )
        .await;
        if let Some(e) = v["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["kind"] == "approval_requested")
        {
            let aid = e["payload"]["approval_id"].as_str().unwrap().to_string();
            return (sid, aid);
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no approval was requested");
}

/// The four kinds of request P-1 names: a read of a chat, an approval
/// answer, a prompt and an admin action. Their statuses, for one way of
/// authenticating.
async fn four(t: &T, sid: &str, aid: &str, cookie: &str, key: Option<&str>) -> [StatusCode; 4] {
    [
        call(
            t,
            Method::GET,
            &format!("/api/sessions/{sid}/events"),
            None,
            cookie,
            key,
        )
        .await
        .0,
        call(
            t,
            Method::POST,
            &format!("/api/sessions/{sid}/approvals/{aid}"),
            Some(json!({"decision": "allow"})),
            cookie,
            key,
        )
        .await
        .0,
        call(
            t,
            Method::POST,
            &format!("/api/sessions/{sid}/prompt"),
            Some(json!({"text": "hello"})),
            cookie,
            key,
        )
        .await
        .0,
        call(t, Method::POST, "/api/admin/nodes/join", None, cookie, key)
            .await
            .0,
    ]
}

const REFUSED: [StatusCode; 4] = [StatusCode::UNAUTHORIZED; 4];

#[tokio::test]
async fn a_cookie_without_its_login_key_authenticates_nothing() {
    let t = start().await;
    let alex = claim(&t).await;
    let (sid, aid) = pending_approval(&t, &alex).await;
    assert_eq!(
        four(&t, &sid, &aid, &alex.cookie, None).await,
        REFUSED,
        "the cookie alone"
    );
    assert_eq!(
        four(&t, &sid, &aid, &alex.cookie, Some(&"0".repeat(64))).await,
        REFUSED,
        "a wrong key"
    );
    // Another login of the same user: its key does not fit this cookie,
    // nor this key that cookie.
    let other = login(&t).await;
    assert_eq!(
        four(&t, &sid, &aid, &alex.cookie, Some(&other.key)).await,
        REFUSED
    );
    assert_eq!(
        four(&t, &sid, &aid, &other.cookie, Some(&alex.key)).await,
        REFUSED
    );
    // The refusal says nothing about which half was wrong.
    let (_, a) = call(&t, Method::GET, "/api/me", None, &alex.cookie, None).await;
    let (_, b) = call(&t, Method::GET, "/api/me", None, "x=y", Some(&alex.key)).await;
    assert_eq!(a, b);
    // With both: the approval is answered and the rest works (the prompt
    // is refused as busy, 409, until the approved turn has ended).
    let ok = four(&t, &sid, &aid, &alex.cookie, Some(&alex.key)).await;
    assert_eq!(ok[0], StatusCode::OK);
    assert_eq!(ok[1], StatusCode::OK);
    assert!(
        ok[2] == StatusCode::OK || ok[2] == StatusCode::CONFLICT,
        "{ok:?}"
    );
    assert_eq!(ok[3], StatusCode::OK, "a join token is minted");
    for _ in 0..500 {
        let (st, _) = as_login(
            &t,
            Method::POST,
            &format!("/api/sessions/{sid}/prompt"),
            Some(json!({"text": "hello"})),
            &alex,
        )
        .await;
        if st == StatusCode::OK {
            return;
        }
        assert_eq!(st, StatusCode::CONFLICT);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the prompt never went through");
}

/// The abuse case end to end. The person's browser opens a page the
/// agent's dev server serves on another port of the same host; by the
/// cookie rules (RFC 6265 §5.4 matches on host and path, never on port)
/// it sends the harness cookie along, and the dev server replays it with a
/// forged `Origin`. It reads no events and answers no approval.
#[tokio::test]
async fn the_agents_dev_server_cannot_use_the_cookie_it_receives() {
    let t = start().await;
    let alex = claim(&t).await;
    let (sid, aid) = pending_approval(&t, &alex).await;

    // The agent's dev server: it keeps the Cookie header of whatever
    // request reaches it.
    let dev = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dev_addr = dev.local_addr().unwrap();
    assert_ne!(dev_addr.port(), t.addr.port());
    let stolen = tokio::spawn(async move {
        let (mut s, _) = dev.accept().await.unwrap();
        let mut buf = vec![0u8; 8192];
        let n = s.read(&mut buf).await.unwrap();
        let head = String::from_utf8_lossy(&buf[..n]).to_string();
        s.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nhi")
            .await
            .unwrap();
        head.lines()
            .find_map(|l| l.strip_prefix("cookie: ").or(l.strip_prefix("Cookie: ")))
            .map(str::to_string)
    });
    // The browser: host-only cookie for 127.0.0.1, sent to every port.
    reqwest::Client::new()
        .get(format!("http://{dev_addr}/"))
        .header("cookie", &alex.cookie)
        .send()
        .await
        .unwrap();
    let cookie = stolen.await.unwrap().expect("the browser sent the cookie");
    assert_eq!(cookie, alex.cookie);

    // The dev server, as a plain HTTP client with the harness's Origin.
    let forged = format!("http://{}", t.addr);
    let c = reqwest::Client::new();
    let read = c
        .get(format!("{}/api/sessions/{sid}/events", t.base))
        .header("cookie", &cookie)
        .header("origin", &forged)
        .send()
        .await
        .unwrap();
    assert_eq!(read.status(), StatusCode::UNAUTHORIZED);
    assert!(!read.text().await.unwrap().contains("approval_requested"));
    let answer = c
        .post(format!("{}/api/sessions/{sid}/approvals/{aid}", t.base))
        .header("cookie", &cookie)
        .header("origin", &forged)
        .json(&json!({"decision": "allow"}))
        .send()
        .await
        .unwrap();
    assert_eq!(answer.status(), StatusCode::UNAUTHORIZED);
    // The approval is still pending: the person answers it.
    let (st, _) = as_login(
        &t,
        Method::POST,
        &format!("/api/sessions/{sid}/approvals/{aid}"),
        Some(json!({"decision": "deny"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
}

#[tokio::test]
async fn only_the_keys_hash_is_stored_and_it_is_never_logged() {
    let t = start().await;
    let alex = claim(&t).await;
    let other = login(&t).await;
    for l in [&alex, &other] {
        assert_eq!(l.key.len(), 64, "256 bits, hex");
        let (st, _) = as_login(&t, Method::GET, "/api/me", None, l).await;
        assert_eq!(st, StatusCode::OK);
    }
    let mut files = String::new();
    for e in std::fs::read_dir(t.dir.path()).unwrap() {
        let p = e.unwrap().path();
        if p.is_file() {
            files.push_str(&String::from_utf8_lossy(&std::fs::read(&p).unwrap()));
        }
    }
    for l in [&alex, &other] {
        assert!(!files.contains(&l.key), "the key is in the database files");
        let logged = String::from_utf8_lossy(&logs().lock().unwrap()).to_string();
        assert!(!logged.contains(&l.key), "the key was logged");
    }
}

#[tokio::test]
async fn the_api_token_needs_no_key_and_logout_ends_the_key() {
    let t = start().await;
    let alex = claim(&t).await;
    let r = reqwest::Client::new()
        .get(format!("{}/api/sessions", t.base))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let r = reqwest::Client::new()
        .post(format!("{}/api/admin/nodes/join", t.base))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let (st, _) = as_login(&t, Method::POST, "/api/logout", None, &alex).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = as_login(&t, Method::GET, "/api/me", None, &alex).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED, "logged out");
}

/// Gate 3 fix review 2, R2-3: a page that lost its key (storage cleared)
/// could not end its login, which then lived for 30 days. A logout with
/// the cookie alone now ends that one login, and answers the same whether
/// the cookie was live or not.
#[tokio::test]
async fn a_logout_with_the_cookie_alone_ends_that_login() {
    let t = start().await;
    let alex = claim(&t).await;
    let other = login(&t).await;
    let (st, v) = call(&t, Method::POST, "/api/logout", None, &alex.cookie, None).await;
    assert_eq!((st, v.clone()), (StatusCode::OK, json!({"ok": true})));
    let (st, _) = as_login(&t, Method::GET, "/api/me", None, &alex).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED, "the login lived on");
    // Only that login: the person's other one is untouched.
    let (st, _) = as_login(&t, Method::GET, "/api/me", None, &other).await;
    assert_eq!(st, StatusCode::OK);
    // A dead or made-up cookie gets the same answer.
    let dead = format!(
        "{}={}",
        alex.cookie.split('=').next().unwrap(),
        "0".repeat(64)
    );
    for c in [&alex.cookie, &dead] {
        assert_eq!(
            call(&t, Method::POST, "/api/logout", None, c, None).await,
            (StatusCode::OK, v.clone())
        );
    }
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn open_socket(t: &T, cookie: &str) -> Socket {
    let mut r = format!("ws://{}/api/ws", t.addr)
        .into_client_request()
        .unwrap();
    r.headers_mut().insert("cookie", cookie.parse().unwrap());
    r.headers_mut()
        .insert("origin", format!("http://{}", t.addr).parse().unwrap());
    tokio_tungstenite::connect_async(r).await.unwrap().0
}

/// Every text message until the socket ends, and its close code; panics
/// if it stays open for `within`.
async fn drain(s: &mut Socket, within: Duration) -> (Vec<Value>, Option<u16>) {
    let mut got = Vec::new();
    let end = tokio::time::Instant::now() + within;
    loop {
        let msg = tokio::time::timeout_at(end, s.next())
            .await
            .expect("the socket stayed open");
        match msg {
            Some(Ok(Message::Text(t))) => got.push(serde_json::from_str(&t).unwrap()),
            Some(Ok(Message::Close(f))) => return (got, f.map(|f| u16::from(f.code))),
            None | Some(Err(_)) => return (got, None),
            Some(Ok(_)) => {}
        }
    }
}

fn subscribe(sid: &str) -> Message {
    Message::Text(
        json!({"type": "subscribe", "session_id": sid, "after_seq": 0})
            .to_string()
            .into(),
    )
}

/// Gate 3 fix review 2, R2-1: the cookie alone took every one of the
/// person's eight socket places (their page got 429), and the upgrade's
/// status and `/api/favicon` told its holder whether the login was live.
#[tokio::test]
async fn a_cookie_alone_takes_no_socket_place_and_learns_nothing() {
    let t = start().await;
    let alex = claim(&t).await;
    let dead = format!(
        "{}={}",
        alex.cookie.split('=').next().unwrap(),
        "0".repeat(64)
    );
    // Twelve sockets on the cookie alone, none sends a key; and a dead one.
    let mut held = Vec::new();
    for _ in 0..12 {
        held.push(open_socket(&t, &alex.cookie).await);
    }
    let mut d = open_socket(&t, &dead).await;
    // The person's own page still gets its socket, and it works.
    let mut s = open_socket(&t, &alex.cookie).await;
    let hello = json!({"type": "login", "key": alex.key}).to_string();
    s.send(Message::Text(hello.into())).await.unwrap();
    s.send(Message::Text("hello".into())).await.unwrap();
    let msg = tokio::time::timeout(Duration::from_secs(5), s.next())
        .await
        .expect("the person's socket went quiet")
        .unwrap()
        .unwrap();
    let Message::Text(text) = msg else {
        panic!("{msg:?}")
    };
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["type"],
        "error"
    );
    // A live cookie and a dead one end the same way, with nothing sent.
    let (got, code) = drain(&mut held[0], Duration::from_secs(15)).await;
    assert!(got.is_empty());
    assert_eq!(
        (got, code),
        drain(&mut d, Duration::from_secs(15)).await,
        "a live cookie ends unlike a dead one"
    );
    drop(held);

    // The favicon takes the key (or a link), never the cookie alone.
    let http = reqwest::Client::new();
    let favicon = |cookie: Option<&str>, key: Option<&str>| {
        let mut q = http.get(format!("{}/api/favicon?url=not-a-url", t.base));
        if let Some(c) = cookie {
            q = q.header("cookie", c);
        }
        if let Some(k) = key {
            q = q.header(KEY_HEADER, k);
        }
        async move { q.send().await.unwrap().status() }
    };
    assert_eq!(favicon(None, None).await, StatusCode::UNAUTHORIZED);
    assert_eq!(favicon(Some(&dead), None).await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        favicon(Some(&alex.cookie), None).await,
        StatusCode::UNAUTHORIZED,
        "a live cookie without its key gets past the login check"
    );
    assert_eq!(
        favicon(Some(&alex.cookie), Some(&alex.key)).await,
        StatusCode::BAD_REQUEST
    );
    // An <img> loads it through a link minted for this login.
    let (st, v) = as_login(
        &t,
        Method::POST,
        "/api/links",
        Some(json!({"urls": ["/api/favicon?url=not-a-url"]})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let link = v["links"]["/api/favicon?url=not-a-url"]
        .as_str()
        .unwrap_or_else(|| panic!("no link: {v}"))
        .to_string();
    let r = http
        .get(format!("{}{link}", t.base))
        .header("cookie", &alex.cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_socket_must_present_its_key_first() {
    let t = start().await;
    let alex = claim(&t).await;
    let (sid, _) = pending_approval(&t, &alex).await;

    // Says nothing: closed as signed out, with nothing delivered.
    let mut s = open_socket(&t, &alex.cookie).await;
    let (got, code) = drain(&mut s, Duration::from_secs(15)).await;
    assert!(got.is_empty(), "{got:?}");
    assert_eq!(code, Some(4401));

    // Subscribes straight away, without the key.
    let mut s = open_socket(&t, &alex.cookie).await;
    s.send(subscribe(&sid)).await.unwrap();
    let (got, code) = drain(&mut s, Duration::from_secs(15)).await;
    assert!(got.is_empty(), "{got:?}");
    assert_eq!(code, Some(4401));

    // A wrong key, then a subscribe.
    let mut s = open_socket(&t, &alex.cookie).await;
    let wrong = json!({"type": "login", "key": "0".repeat(64)}).to_string();
    s.send(Message::Text(wrong.into())).await.unwrap();
    let _ = s.send(subscribe(&sid)).await;
    let (got, code) = drain(&mut s, Duration::from_secs(15)).await;
    assert!(got.is_empty(), "{got:?}");
    assert_eq!(code, Some(4401));

    // The key first: the socket works as before.
    let mut s = open_socket(&t, &alex.cookie).await;
    let hello = json!({"type": "login", "key": alex.key}).to_string();
    s.send(Message::Text(hello.into())).await.unwrap();
    s.send(subscribe(&sid)).await.unwrap();
    let mut kinds = Vec::new();
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), s.next())
            .await
            .expect("socket went quiet")
            .unwrap()
            .unwrap();
        let Message::Text(text) = msg else { continue };
        let v: Value = serde_json::from_str(&text).unwrap();
        if v["type"] == "replayed" {
            break;
        }
        if v["type"] == "event" {
            kinds.push(v["event"]["kind"].as_str().unwrap().to_string());
        }
    }
    assert!(kinds.iter().any(|k| k == "approval_requested"), "{kinds:?}");
}
