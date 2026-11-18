//! A workspace's custom connectors and each person's own account
//! (ADOPTION_PLAN X.35 step 1, PLAN.md T84) against a fake remote MCP
//! server (`fake_mcp`): the token is checked by speaking MCP, stored
//! sealed, never shown, never logged, sent in a header only and only to
//! the connector's origin; others cannot see, use or delete it; editors
//! alone define connectors; an edited address disconnects everyone first;
//! hostile addresses are refused before any call; floods are limited.
//! No real server and no real token are used.

mod common;
mod fake_mcp;
use common::{Login as _, SameOrigin as _};

use std::io::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use fake_mcp::{CANARY, Fake, GOOD};
use futures::{SinkExt, StreamExt};
use harness_core::Secret;
use harness_master::auth::{Auth, Claim};
use harness_master::sealed::Sealer;
use harness_master::{Master, router};
use harness_protocol::{MasterToNode, NodePlugins, NodeToMaster, PROTOCOL_VERSION};
use harness_store::{Durability, Store};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

/// Tokens the fake refuses (made up; searched for everywhere at the end).
const BAD: &str = "mcp_CANARYwrong00000000000000000000000002";
const BAD2: &str = "mcp_CANARYwrong00000000000000000000000003";
const ALL: [&str; 3] = [GOOD, BAD, BAD2];

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

/// A node link the test drives: it announces what it is told to.
struct Node {
    tx: mpsc::Sender<NodeToMaster>,
}

impl Node {
    async fn announce(&self, servers: &[&str]) {
        self.tx
            .send(NodeToMaster::Plugins {
                plugins: NodePlugins {
                    skills: vec![],
                    mcp_servers: servers.iter().map(|s| s.to_string()).collect(),
                    left_out: 0,
                },
            })
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn fake_node() -> (harness_node::NodeLink, Node) {
    let (to_node, mut commands) = mpsc::channel::<MasterToNode>(1024);
    let (to_master, from_node) = mpsc::channel::<NodeToMaster>(1024);
    to_master
        .try_send(NodeToMaster::Hello {
            protocol_version: PROTOCOL_VERSION,
            node_id: "master".into(),
            hostname: "h".into(),
            version: "0".into(),
            capabilities: vec!["profile:chat".into()],
            sessions: vec![],
        })
        .unwrap();
    let tx = to_master.clone();
    tokio::spawn(async move {
        while let Some(msg) = commands.recv().await {
            if let Some(req_id) = msg.req_id().map(str::to_string) {
                let _ = to_master.try_send(NodeToMaster::Ack {
                    req_id,
                    ok: true,
                    error: None,
                });
            }
        }
    });
    (harness_node::NodeLink { to_node, from_node }, Node { tx })
}

struct T {
    base: String,
    addr: SocketAddr,
    master: Master,
    store: Store,
    fake: Fake,
    node: Node,
    dir: tempfile::TempDir,
    _cert_dir: tempfile::TempDir,
    /// Every answer this test's HTTP calls got: status, headers, body.
    answers: Arc<Mutex<String>>,
    /// A listener that speaks no TLS (`plain.test`), and its connections.
    plain_hits: Arc<std::sync::atomic::AtomicUsize>,
}

async fn silent_listener() -> (u16, Arc<std::sync::atomic::AtomicUsize>) {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let n2 = n.clone();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((s, _)) = l.accept().await {
            n2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            held.push(s);
        }
    });
    (port, n)
}

fn net(t_fake: &Fake, plain: u16, allow_private: bool, master: &Master) {
    let mut table = t_fake.table();
    table.insert(
        "plain.test".into(),
        vec![format!("127.0.0.1:{plain}").parse().unwrap()],
    );
    master.set_connector_test_net(
        table,
        allow_private,
        Some(t_fake.cert_der.clone()),
        Duration::from_secs(2),
    );
}

async fn start() -> T {
    logs();
    let dir = tempfile::tempdir().unwrap();
    let cert_dir = tempfile::tempdir().unwrap();
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
    let (link, node) = fake_node();
    master.add_link(link, true).await.unwrap();
    let fake = fake_mcp::start(cert_dir.path()).await;
    let (plain, plain_hits) = silent_listener().await;
    net(&fake, plain, true, &master);
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        addr,
        master,
        store,
        fake,
        node,
        dir,
        _cert_dir: cert_dir,
        answers: Arc::default(),
        plain_hits,
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

/// `who`: a cookie, `Some("api")` for the API token, `None` for nobody.
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

/// Who is who: the server admin alex (no member of the team), alice (the
/// team's owner), bob (a member), carol (the team's admin) and dan (not
/// in the team; owner of his own team `other`).
struct People {
    alex: String,
    alice: (String, String),
    bob: (String, String),
    carol: String,
    dan: String,
    team: String,
    other: String,
}

async fn people(t: &T) -> People {
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
    let carol = member(t, &alex, "carol").await;
    let dan = member(t, &alex, "dan").await;
    let id = |c: String| async move {
        let (_, me) = req(t, Method::GET, "/api/me", None, Some(&c)).await;
        (c, me["user"]["user_id"].as_str().unwrap().to_string())
    };
    let alice = id(alice).await;
    let bob = id(bob).await;
    let (st, v) = req(
        t,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "Team"})),
        Some(&alice.0),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let team = v["workspace_id"].as_str().unwrap().to_string();
    for (who, role) in [("bob", "member"), ("carol", "admin")] {
        let (st, v) = req(
            t,
            Method::POST,
            &format!("/api/workspaces/{team}/members"),
            Some(json!({"username": who, "role": role})),
            Some(&alice.0),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }
    let (_, v) = req(
        t,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "Other"})),
        Some(&dan),
    )
    .await;
    let other = v["workspace_id"].as_str().unwrap().to_string();
    People {
        alex,
        alice,
        bob,
        carol,
        dan,
        team,
        other,
    }
}

fn base(ws: &str) -> String {
    format!("/api/workspaces/{ws}/connectors")
}

/// Add connector `name` at `https://mcp.test/<path>` signing in by token.
async fn add(t: &T, c: &str, ws: &str, name: &str, path: &str) -> (StatusCode, Value) {
    req(
        t,
        Method::POST,
        &base(ws),
        Some(json!({"name": name, "url": format!("https://mcp.test/{path}"), "auth": "token"})),
        Some(c),
    )
    .await
}

async fn connect(t: &T, c: &str, ws: &str, id: &str, token: &str) -> (StatusCode, Value) {
    req(
        t,
        Method::PUT,
        &format!("{}/{id}/account", base(ws)),
        Some(json!({"token": token})),
        Some(c),
    )
    .await
}

async fn check(t: &T, c: &str, ws: &str, id: &str) -> (StatusCode, Value) {
    req(
        t,
        Method::POST,
        &format!("{}/{id}/check", base(ws)),
        None,
        Some(c),
    )
    .await
}

async fn edit(t: &T, c: &str, ws: &str, id: &str, body: Value) -> (StatusCode, Value) {
    req(
        t,
        Method::PUT,
        &format!("{}/{id}", base(ws)),
        Some(body),
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

/// Nothing of any token anywhere: the data directory's files, the log at
/// TRACE, every answer, the socket frames, the URLs the fake saw, the
/// audit table; and no remote text in the answers or the log.
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
        .fake
        .seen()
        .iter()
        .map(|s| format!("{}?{}\n", s.path, s.query.clone().unwrap_or_default()))
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
            ("the fake's URLs", urls.as_str()),
            ("audit", &audit.join("\n")),
        ] {
            assert!(!text.contains(tok), "{tok} found in {what}");
            assert!(!text.contains(&tok[10..]), "part of {tok} found in {what}");
        }
    }
    assert!(!answers.contains(CANARY), "remote text in an answer");
    assert!(!logs.contains(CANARY), "remote text in the log");
}

fn find<'a>(l: &'a Value, name: &str) -> &'a Value {
    l["connectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no {name} in {l}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn define_connect_check_and_disconnect_and_the_token_is_nowhere() {
    let t = start().await;
    let p = people(&t).await;
    let (alice, alice_id) = (&p.alice.0, &p.alice.1);
    let frames = socket(&t, alice).await;
    let (st, c) = add(&t, alice, &p.team, "fakemcp", "auth/mcp").await;
    assert_eq!(st, StatusCode::OK, "{c}");
    assert_eq!(c["url"], "https://mcp.test/auth/mcp");
    assert_eq!(c["header"], "authorization");
    assert_eq!(c["connected"], false);
    let id = c["id"].as_str().unwrap().to_string();
    assert_eq!(t.fake.count(), 0, "defining calls nothing");

    // A refused token: nothing stored, the fake's words not passed on.
    let (st, e) = connect(&t, alice, &p.team, &id, BAD).await;
    assert_eq!(st, StatusCode::BAD_GATEWAY, "{e}");
    assert!(e["error"].as_str().unwrap().contains("HTTP 401"), "{e}");
    assert_eq!(t.store.sealed_count().unwrap(), 0);

    // Connect: checked by speaking MCP, the offer shown, the token sealed.
    let (st, c) = connect(&t, alice, &p.team, &id, &format!("  {GOOD}\n")).await;
    assert_eq!(st, StatusCode::OK, "{c}");
    assert_eq!(c["connected"], true);
    assert_eq!(c["offer"]["server_name"], "fake");
    assert_eq!(c["offer"]["tools"].as_array().unwrap().len(), 3);
    assert_eq!(c["offer"]["tools"][0]["agent_name"], "mcp__fakemcp__tool_0");
    assert_eq!(t.store.sealed_count().unwrap(), 1);
    assert!(t.dir.path().join("keys/secrets.key").exists());
    for s in t.fake.seen() {
        assert_eq!(s.host, "mcp.test");
    }
    assert_eq!(t.fake.settle().await, 0, "no connection left open");
    assert_eq!(t.master.connector_tasks_left(), 0, "no task left");

    // The list shows it hers, with the offer of her last check.
    let (_, l) = req(&t, Method::GET, &base(&p.team), None, Some(alice)).await;
    let mine = find(&l, "fakemcp");
    assert_eq!(mine["connected"], true);
    assert_eq!(mine["offer"]["tools"].as_array().unwrap().len(), 3);
    // Check: opened from sealed storage and sent again, in the header.
    let before = t.fake.count();
    let (st, c) = check(&t, alice, &p.team, &id).await;
    assert_eq!(st, StatusCode::OK, "{c}");
    assert!(t.fake.count() > before);
    let last = t.fake.seen().pop().unwrap();
    assert_eq!(
        last.header("authorization"),
        Some(format!("Bearer {GOOD}").as_str())
    );
    // Replace: a refused one changes nothing, a good one replaces.
    let (st, _) = connect(&t, alice, &p.team, &id, BAD2).await;
    assert_eq!(st, StatusCode::BAD_GATEWAY);
    let (st, _) = connect(&t, alice, &p.team, &id, GOOD).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(t.store.sealed_count().unwrap(), 1);

    // An edit forgets every offer of the connector (here a header only).
    let (st, _) = edit(
        &t,
        alice,
        &p.team,
        &id,
        json!({"url": "https://mcp.test/auth/mcp", "auth": "token", "header": "X-API-Key"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_, l) = req(&t, Method::GET, &base(&p.team), None, Some(alice)).await;
    assert_eq!(
        find(&l, "fakemcp")["connected"],
        true,
        "a header change keeps her account"
    );
    assert_eq!(find(&l, "fakemcp")["offer"], Value::Null);
    let (st, _) = check(&t, alice, &p.team, &id).await;
    assert_eq!(st, StatusCode::OK);
    // Failed checks of every kind say what failed in fixed words.
    for (path, needle) in [
        ("e500/mcp", "HTTP 500"),
        ("hang/mcp", "in time"),
        ("redirect-other/mcp", "redirected"),
        ("bigjson/mcp", "more than this server reads"),
    ] {
        let (st, e) = edit(
            &t,
            alice,
            &p.team,
            &id,
            json!({"url": format!("https://mcp.test/{path}"), "auth": "token"}),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{e}");
        let (st, e) = connect(&t, alice, &p.team, &id, GOOD).await;
        assert_eq!(st, StatusCode::BAD_GATEWAY, "{path}: {e}");
        assert!(e["error"].as_str().unwrap().contains(needle), "{path}: {e}");
    }
    // A server that speaks no TLS.
    let (st, _) = edit(
        &t,
        alice,
        &p.team,
        &id,
        json!({"url": "https://plain.test/mcp", "auth": "token"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, e) = connect(&t, alice, &p.team, &id, GOOD).await;
    assert_eq!(st, StatusCode::BAD_GATEWAY, "{e}");
    assert!(t.plain_hits.load(std::sync::atomic::Ordering::SeqCst) >= 1);
    assert_eq!(
        t.store.sealed_count().unwrap(),
        0,
        "the edits disconnected her"
    );

    // Back, connected, and disconnected.
    edit(
        &t,
        alice,
        &p.team,
        &id,
        json!({"url": "https://mcp.test/auth/mcp", "auth": "token"}),
    )
    .await;
    let (st, _) = connect(&t, alice, &p.team, &id, GOOD).await;
    assert_eq!(st, StatusCode::OK);
    let acct = format!("{}/{id}/account", base(&p.team));
    let (st, _) = req(&t, Method::DELETE, &acct, None, Some(alice)).await;
    assert_eq!(st, StatusCode::OK);
    let (_, l) = req(&t, Method::GET, &base(&p.team), None, Some(alice)).await;
    assert_eq!(
        find(&l, "fakemcp")["offer"],
        Value::Null,
        "her offer went with it"
    );
    let (st, _) = req(&t, Method::DELETE, &acct, None, Some(alice)).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    assert_eq!(t.store.sealed_count().unwrap(), 0);
    let (st, e) = check(&t, alice, &p.team, &id).await;
    assert_eq!(st, StatusCode::CONFLICT, "{e}");

    // Bob's account goes with his deletion.
    let (bob, bob_id) = (&p.bob.0, &p.bob.1);
    let (st, _) = connect(&t, bob, &p.team, &id, GOOD).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = req(
        &t,
        Method::DELETE,
        &format!("/api/users/{bob_id}"),
        None,
        Some(&p.alex),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(t.store.sealed_count().unwrap(), 0);

    // The audit says who, which connector and what; never a token.
    let db = rusqlite::Connection::open(t.dir.path().join("master.db")).unwrap();
    let n: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM audit WHERE kind = 'custom_connector'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(n >= 10, "{n}");
    drop(db);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let frames = frames.lock().unwrap().clone();
    assert!(
        frames.contains("connectors"),
        "the change reached the socket"
    );
    t.store.call(|s| s.sealed_count()).await.unwrap();
    let _ = alice_id;
    assert_no_token_anywhere(&t, &frames);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn others_cannot_see_use_replace_or_delete_an_account() {
    let t = start().await;
    let p = people(&t).await;
    let alice = &p.alice.0;
    let (_, c) = add(&t, alice, &p.team, "fakemcp", "auth/mcp").await;
    let id = c["id"].as_str().unwrap().to_string();
    let (st, _) = connect(&t, alice, &p.team, &id, GOOD).await;
    assert_eq!(st, StatusCode::OK);
    let acct = format!("{}/{id}/account", base(&p.team));
    // Bob (a member) and carol (the team's admin): her account is not theirs.
    for who in [&p.bob.0, &p.carol] {
        let (st, l) = req(&t, Method::GET, &base(&p.team), None, Some(who)).await;
        assert_eq!(st, StatusCode::OK, "{l}");
        let row = find(&l, "fakemcp");
        assert_eq!(row["connected"], false);
        assert_eq!(row["offer"], Value::Null, "her offer is hers");
        let (st, _) = req(&t, Method::DELETE, &acct, None, Some(who)).await;
        assert_eq!(st, StatusCode::NOT_FOUND);
        let (st, e) = check(&t, who, &p.team, &id).await;
        assert_eq!(st, StatusCode::CONFLICT, "{e}");
        // Connecting is theirs alone: it makes their own row.
    }
    // The server admin and a stranger are no members: nothing at all.
    for who in [&p.alex, &p.dan] {
        for (m, path, b) in [
            (Method::GET, base(&p.team), None),
            (Method::DELETE, acct.clone(), None),
            (Method::PUT, acct.clone(), Some(json!({"token": BAD}))),
            (Method::POST, format!("{}/{id}/check", base(&p.team)), None),
            (Method::DELETE, format!("{}/{id}", base(&p.team)), None),
        ] {
            let (st, e) = req(&t, m.clone(), &path, b, Some(who)).await;
            assert_eq!(st, StatusCode::NOT_FOUND, "{m} {path}: {e}");
        }
    }
    // Another workspace's path with this id: not there.
    let (st, _) = req(
        &t,
        Method::POST,
        &format!("{}/{id}/check", base(&p.other)),
        None,
        Some(&p.dan),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // The API token acts as an owner for definitions, but has no account.
    for (m, path, b) in [
        (Method::DELETE, acct.clone(), None),
        (Method::PUT, acct.clone(), Some(json!({"token": GOOD}))),
        (Method::POST, format!("{}/{id}/check", base(&p.team)), None),
    ] {
        let (st, e) = req(&t, m.clone(), &path, b, Some("api")).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{m} {path}: {e}");
    }
    let (st, l) = req(&t, Method::GET, &base(&p.team), None, Some("api")).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(find(&l, "fakemcp")["connected"], false);
    // A member without `settings` defines nothing.
    let bob = &p.bob.0;
    let (st, _) = add(&t, bob, &p.team, "bobs", "ok").await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = edit(
        &t,
        bob,
        &p.team,
        &id,
        json!({"url": "https://mcp.test/x", "auth": "none"}),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = req(
        &t,
        Method::DELETE,
        &format!("{}/{id}", base(&p.team)),
        None,
        Some(bob),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    assert_eq!(t.store.sealed_count().unwrap(), 1, "hers is still there");
    // Carol (admin) may define; the API token too.
    let (st, _) = add(&t, &p.carol, &p.team, "carols", "ok").await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = add(&t, "api", &p.team, "apis", "ok").await;
    assert_eq!(st, StatusCode::OK);
}

/// An edited address disconnects everyone before anything is sent there;
/// an old sealed row put back does not open for the new address.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_edited_address_disconnects_everyone_first() {
    let t = start().await;
    let p = people(&t).await;
    let (alice, alice_id) = (&p.alice.0, &p.alice.1);
    let (_, c) = add(&t, alice, &p.team, "fakemcp", "auth/mcp").await;
    let id = c["id"].as_str().unwrap().to_string();
    for who in [alice, &p.bob.0] {
        let (st, _) = connect(&t, who, &p.team, &id, GOOD).await;
        assert_eq!(st, StatusCode::OK);
    }
    let db = rusqlite::Connection::open(t.dir.path().join("master.db")).unwrap();
    let saved: Vec<u8> = db
        .query_row(
            "SELECT sealed_token FROM connector_accounts WHERE user_id = ?1",
            [alice_id],
            |r| r.get(0),
        )
        .unwrap();
    // A new header for the same address keeps the accounts.
    let (st, e) = edit(
        &t,
        alice,
        &p.team,
        &id,
        json!({"url": "https://mcp.test/auth/mcp", "auth": "token", "header": "X-API-Key"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{e}");
    assert_eq!(e["disconnected"], 0);
    assert_eq!(e["header"], "x-api-key");
    let before = t.fake.count();
    let (st, e) = edit(
        &t,
        alice,
        &p.team,
        &id,
        json!({"url": "https://mcp.test/auth/v2", "auth": "token", "header": "X-API-Key"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{e}");
    assert_eq!(e["disconnected"], 2);
    assert_eq!(t.fake.count(), before, "the edit itself calls nothing");
    assert_eq!(t.store.sealed_count().unwrap(), 0);
    let (st, e) = check(&t, alice, &p.team, &id).await;
    assert_eq!(st, StatusCode::CONFLICT, "{e}");
    assert_eq!(t.fake.count(), before, "no token went to the new address");
    // Her old sealed token copied back does not open: it is bound to the
    // old address.
    db.execute(
        "INSERT INTO connector_accounts (user_id, workspace_id, connector_id, sealed_token, created_ms, updated_ms)
         VALUES (?1, ?2, ?3, ?4, 0, 0)",
        rusqlite::params![alice_id, p.team, id, saved],
    )
    .unwrap();
    let (st, e) = check(&t, alice, &p.team, &id).await;
    assert_eq!(st, StatusCode::CONFLICT, "{e}");
    assert!(
        e["error"].as_str().unwrap().contains("does not open"),
        "{e}"
    );
    assert_eq!(t.fake.count(), before);
    // Bob's copy onto her row does not open as hers either.
    db.execute("DELETE FROM connector_accounts", []).unwrap();
    let (st, _) = connect(&t, &p.bob.0, &p.team, &id, GOOD).await;
    assert_eq!(st, StatusCode::OK);
    db.execute(
        "INSERT INTO connector_accounts (user_id, workspace_id, connector_id, sealed_token, created_ms, updated_ms)
         SELECT ?1, workspace_id, connector_id, sealed_token, 0, 0 FROM connector_accounts",
        [alice_id],
    )
    .unwrap();
    let (st, e) = check(&t, alice, &p.team, &id).await;
    assert_eq!(st, StatusCode::CONFLICT, "{e}");
    // Sign-in none: no token is taken, and the check sends none.
    let (st, e) = edit(
        &t,
        alice,
        &p.team,
        &id,
        json!({"url": "https://mcp.test/ok", "auth": "none"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{e}");
    assert_eq!(t.store.sealed_count().unwrap(), 0);
    let before = t.fake.count();
    let (st, e) = connect(&t, alice, &p.team, &id, GOOD).await;
    assert_eq!(st, StatusCode::CONFLICT, "{e}");
    assert_eq!(t.fake.count(), before, "the token was sent nowhere");
    let (st, c) = check(&t, alice, &p.team, &id).await;
    assert_eq!(st, StatusCode::OK, "{c}");
    assert_eq!(c["connected"], Value::Null);
    assert!(
        t.fake
            .seen()
            .last()
            .unwrap()
            .header("authorization")
            .is_none()
    );
}

/// Addresses, names and headers that break a rule are refused before any
/// lookup or connection.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bad_definitions_are_refused_without_a_call() {
    let t = start().await;
    let p = people(&t).await;
    let alice = &p.alice.0;
    let path = base(&p.team);
    let post = |body: Value| req(&t, Method::POST, &path, Some(body), Some(alice));
    for url in [
        "http://mcp.test/mcp",
        "https://127.0.0.1/mcp",
        "https://[::1]/mcp",
        "https://localhost/mcp",
        "https://u:p@mcp.test/mcp",
        "https://mcp.test:8443/mcp",
        "https://mcp.test/mcp#x",
        "https://mcp.test/mcp?k=v",
        "https://xn--mcp-dqa.test/mcp",
        "https://mсp.test/mcp",
        "https://intranet/mcp",
    ] {
        let (st, e) = post(json!({"name": "x", "url": url, "auth": "none"})).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{url}: {e}");
    }
    for name in ["Linear", "a__b", "mcp__x", "", "-a"] {
        let (st, e) =
            post(json!({"name": name, "url": "https://mcp.test/ok", "auth": "none"})).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{name}: {e}");
    }
    for header in [
        "Host",
        "Content-Length",
        "Cookie",
        "Transfer-Encoding",
        "Connection",
        "X-Key\r\nX: 1",
        "X-Key: 1",
    ] {
        let (st, e) = post(
            json!({"name": "x", "url": "https://mcp.test/ok", "auth": "token", "header": header}),
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{header:?}: {e}");
    }
    let (st, e) = post(json!({"name": "x", "url": "https://mcp.test/ok", "auth": "oauth"})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert!(
        e["error"].as_str().unwrap().contains("not built yet"),
        "{e}"
    );
    let (st, _) =
        post(json!({"name": "x", "url": "https://mcp.test/ok", "auth": "none", "extra": 1})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert_eq!(t.fake.count(), 0);
    assert!(t.master.connector_names_asked().is_empty(), "no lookup");

    // Tokens: visible ASCII, bounded.
    let (_, c) = add(&t, alice, &p.team, "fakemcp", "auth/mcp").await;
    let id = c["id"].as_str().unwrap().to_string();
    for bad in ["", "a b", "a\r\nX-Evil: 1", "tökén", &"a".repeat(2049)] {
        let (st, _) = connect(&t, alice, &p.team, &id, bad).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{bad:?}");
    }
    assert_eq!(t.fake.count(), 0);

    // A name that resolves inward is not connected to (no test escape).
    let (plain, _) = silent_listener().await;
    net(&t.fake, plain, false, &t.master);
    let (st, e) = connect(&t, alice, &p.team, &id, GOOD).await;
    assert_eq!(st, StatusCode::BAD_GATEWAY, "{e}");
    assert_eq!(t.fake.accepted.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(
        t.master
            .connector_names_asked()
            .contains(&"mcp.test".to_string())
    );
}

/// Names: unique per workspace, never a node's own server's; a custom
/// connector is a Plugins item whose Shared / Assigned choice works by
/// name; a skill of the same name is another kind and stays apart.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn names_clashes_and_the_plugins_listing() {
    let t = start().await;
    let p = people(&t).await;
    let alice = &p.alice.0;
    let (st, _) = add(&t, alice, &p.team, "later", "ok").await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = add(&t, alice, &p.team, "later", "ok").await;
    assert_eq!(st, StatusCode::CONFLICT);
    // Another workspace may use the same name.
    let (st, _) = add(&t, &p.dan, &p.other, "later", "ok").await;
    assert_eq!(st, StatusCode::OK);
    t.node.announce(&["nodemcp", "later"]).await;
    let (st, e) = add(&t, alice, &p.team, "nodemcp", "ok").await;
    assert_eq!(st, StatusCode::CONFLICT, "{e}");
    let (_, l) = req(&t, Method::GET, &base(&p.team), None, Some(alice)).await;
    assert_eq!(
        find(&l, "later")["clash"],
        true,
        "announced after it was added"
    );
    // In the Plugins list: one item by name, marked custom.
    let plugins = format!("/api/workspaces/{}/plugins", p.team);
    let (_, v) = req(&t, Method::GET, &plugins, None, Some(alice)).await;
    let item = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == "later" && i["kind"] == "mcp")
        .cloned()
        .unwrap();
    assert_eq!(item["custom"]["host"], "mcp.test");
    let (st, e) = req(
        &t,
        Method::PUT,
        &format!("{plugins}/mcp/later"),
        Some(json!({"access": "assigned", "agents": []})),
        Some(alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{e}");
    let (_, v) = req(&t, Method::GET, &plugins, None, Some(alice)).await;
    let item = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == "later" && i["kind"] == "mcp")
        .cloned()
        .unwrap();
    assert_eq!(item["access"], "assigned");
    // Dan's workspace does not see the team's.
    let (_, v) = req(
        &t,
        Method::GET,
        &format!("/api/workspaces/{}/plugins", p.other),
        None,
        Some(&p.dan),
    )
    .await;
    assert!(
        v["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["custom"]["host"] != "mcp.test" || i["name"] == "later"),
        "{v}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn floods_and_caps_are_limited() {
    let t = start().await;
    let p = people(&t).await;
    let alice = &p.alice.0;
    let (_, c) = req(
        &t,
        Method::POST,
        &base(&p.team),
        Some(json!({"name": "fakemcp", "url": "https://mcp.test/ok", "auth": "none"})),
        Some(alice),
    )
    .await;
    let id = c["id"].as_str().unwrap().to_string();
    for i in 0..harness_master::connectors::PER_USER {
        let (st, e) = check(&t, alice, &p.team, &id).await;
        assert_eq!(st, StatusCode::OK, "check {i}: {e}");
    }
    let before = t.fake.count();
    let (st, e) = check(&t, alice, &p.team, &id).await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS, "{e}");
    assert_eq!(t.fake.count(), before, "refused before any call");
    // Bob has his own budget.
    let (st, _) = check(&t, &p.bob.0, &p.team, &id).await;
    assert_eq!(st, StatusCode::OK);
    // Connectors per workspace.
    for i in 1..harness_master::connectors::MAX_PER_WORKSPACE {
        let (st, e) = add(&t, alice, &p.team, &format!("c{i}"), "ok").await;
        assert_eq!(st, StatusCode::OK, "{i}: {e}");
    }
    let (st, e) = add(&t, alice, &p.team, "onemore", "ok").await;
    assert_eq!(st, StatusCode::CONFLICT, "{e}");
    assert!(t.master.connector_offers_cached() <= harness_master::connectors::MAX_CACHED);
}

/// After a restart that cannot seal: connect is refused before the token
/// goes anywhere, and nothing is stored.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lost_key_refuses_connect_before_any_call() {
    let t = start().await;
    let p = people(&t).await;
    let alice = &p.alice.0;
    let (_, c) = add(&t, alice, &p.team, "fakemcp", "auth/mcp").await;
    let id = c["id"].as_str().unwrap().to_string();
    let (st, _) = connect(&t, alice, &p.team, &id, GOOD).await;
    assert_eq!(st, StatusCode::OK);
    std::fs::remove_file(t.dir.path().join("keys/secrets.key")).unwrap();
    // A fresh master on the same database and data directory.
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
    let (plain, _) = silent_listener().await;
    net(&t.fake, plain, true, &m);
    let m2 = m.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    let t2 = T {
        base: format!("http://{addr}"),
        addr,
        master: m,
        store: t.store.clone(),
        fake: t.fake.clone(),
        node: fake_node().1,
        dir: tempfile::tempdir().unwrap(),
        _cert_dir: tempfile::tempdir().unwrap(),
        answers: Arc::default(),
        plain_hits: Arc::default(),
    };
    let (_, l) = req(&t2, Method::GET, &base(&p.team), None, Some(&p.bob.0)).await;
    assert_eq!(l["available"], false, "{l}");
    let before = t.fake.count();
    let (st, e) = connect(&t2, &p.bob.0, &p.team, &id, GOOD).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{e}");
    let (st, e) = check(&t2, alice, &p.team, &id).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{e}");
    assert_eq!(t.fake.count(), before, "the token was sent nowhere");
    assert_eq!(t.store.sealed_count().unwrap(), 1, "nothing stored");
    assert!(
        !t.dir.path().join("keys/secrets.key").exists(),
        "no new key"
    );
}
