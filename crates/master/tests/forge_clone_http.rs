//! Cloning a forge repository into a new project's folder over HTTP
//! (ADOPTION_PLAN X.27 step 3, PLAN.md T86), against a fake forge on
//! 127.0.0.1 and fake nodes: `POST /api/projects` with `folder_on`,
//! `connection_id` and `full_name`. Only the connection's owner clones
//! through it; the repository is looked up by name with that connection
//! (one forge call) before any node is asked; the token goes to the node
//! only for a private repository; an older node is never sent the message;
//! the project records where its folder came from. No real forge and no
//! real token.

mod common;
use common::{Login as _, SameOrigin as _};

use std::io::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use axum::extract::{Request, State};
use axum::http::StatusCode as AxStatus;
use axum::response::{IntoResponse, Response};
use harness_core::Secret;
use harness_master::auth::{Auth, Claim};
use harness_master::forge::{ForgeHost, ForgeKind};
use harness_master::sealed::Sealer;
use harness_master::{Master, router};
use harness_node::NodeLink;
use harness_protocol::{CAP_PROJECTS, MasterToNode, NodeToMaster, PROTOCOL_VERSION};
use harness_store::{Durability, Store};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

/// Alice's token (forge account 42, login "ada") and Bob's (43). Made up;
/// searched for everywhere at the end.
const TOKEN_A: &str = "ghp_CANARYclonealice00000000000000000001";
const TOKEN_B: &str = "ghp_CANARYclonebob0000000000000000000002";

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

// ------------------------------------------------------------ the forge

/// (path, Authorization) of one request.
type Call = (String, Option<String>);

#[derive(Clone, Default)]
struct Forge {
    seen: Arc<Mutex<Vec<Call>>>,
    addr: Arc<OnceLock<SocketAddr>>,
}

impl Forge {
    fn repo_calls(&self) -> Vec<Call> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p.contains("/repos/"))
            .cloned()
            .collect()
    }
    fn host(&self) -> ForgeHost {
        ForgeHost {
            host: "127.0.0.1".into(),
            kind: ForgeKind::Github,
            api_base: format!("http://127.0.0.1:{}/gh", self.addr.get().unwrap().port()),
            api_host: None,
            private_address: true,
        }
    }
}

async fn forge(State(f): State<Forge>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    let auth = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    f.seen.lock().unwrap().push((path.clone(), auth.clone()));
    let who = match auth.as_deref().and_then(|a| a.strip_prefix("Bearer ")) {
        Some(TOKEN_A) => json!({"id": 42, "login": "ada"}),
        Some(TOKEN_B) => json!({"id": 43, "login": "bob-gh"}),
        _ => return (AxStatus::UNAUTHORIZED, "no").into_response(),
    };
    let repo = |full: &str, private: bool| {
        axum::Json(json!({
            "id": 1001, "full_name": full, "private": private, "default_branch": "main",
            "clone_url": "https://evil.example/steal.git",
            "permissions": {"push": true},
        }))
        .into_response()
    };
    match path.to_ascii_lowercase().as_str() {
        "/gh/user" => axum::Json(who).into_response(),
        "/gh/repos/ada/secret" => repo("ada/secret", true),
        "/gh/repos/ada/open" => repo("ada/open", false),
        "/gh/repos/ada/renamed" => repo("ada/other", true),
        "/gh/repos/ada/broken" => axum::Json(json!({"id": 5, "full_name": "../x"})).into_response(),
        _ => (AxStatus::NOT_FOUND, "no").into_response(),
    }
}

async fn fake_forge() -> Forge {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let f = Forge::default();
    f.addr.set(l.local_addr().unwrap()).unwrap();
    let app = axum::Router::new().fallback(forge).with_state(f.clone());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    f
}

// ------------------------------------------------------------ the nodes

#[derive(Clone, Default)]
struct Node {
    seen: Arc<Mutex<Vec<MasterToNode>>>,
}

/// What a `project.clone` carried, the token opened for the test.
#[derive(Debug, PartialEq)]
struct Asked {
    name: String,
    host: String,
    full_name: String,
    username: Option<String>,
    token: Option<String>,
}

impl Node {
    fn clones(&self) -> Vec<Asked> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                MasterToNode::ProjectClone {
                    name,
                    host,
                    full_name,
                    username,
                    token,
                    ..
                } => Some(Asked {
                    name: name.clone(),
                    host: host.clone(),
                    full_name: full_name.clone(),
                    username: username.clone(),
                    token: token.as_ref().map(|t| t.expose().clone()),
                }),
                _ => None,
            })
            .collect()
    }
}

fn folder_of(node: &str, name: &str) -> String {
    format!("/home/{node}/rizzy-projects/{}-ab1c", name.to_lowercase())
}

/// An agentic node `id` announcing `caps`; it answers `project.create`
/// and `project.clone` with [`folder_of`].
fn fake(id: &str, caps: &[&str]) -> (NodeLink, Node) {
    let (to_node, mut commands) = mpsc::channel::<MasterToNode>(1024);
    let (tx, from_node) = mpsc::channel::<NodeToMaster>(1024);
    let mut capabilities = vec![
        "native".to_string(),
        "profile:agentic".to_string(),
        "exec:sandbox".to_string(),
    ];
    capabilities.extend(caps.iter().map(|c| c.to_string()));
    tx.try_send(NodeToMaster::Hello {
        protocol_version: PROTOCOL_VERSION,
        node_id: id.into(),
        hostname: "h".into(),
        version: "1".into(),
        capabilities,
        sessions: vec![],
    })
    .unwrap();
    let n = Node::default();
    let (keep, node) = (n.seen.clone(), id.to_string());
    tokio::spawn(async move {
        while let Some(msg) = commands.recv().await {
            keep.lock().unwrap().push(msg.clone());
            match &msg {
                MasterToNode::ProjectCreate { req_id, name }
                | MasterToNode::ProjectClone { req_id, name, .. } => {
                    let _ = tx.try_send(NodeToMaster::ProjectCreated {
                        req_id: req_id.clone(),
                        path: Some(folder_of(&node, name)),
                        error: None,
                    });
                }
                _ => {}
            }
            if let Some(req_id) = msg.req_id() {
                let _ = tx.try_send(NodeToMaster::Ack {
                    req_id: req_id.into(),
                    ok: true,
                    error: None,
                });
            }
        }
    });
    (NodeLink { to_node, from_node }, n)
}

// ------------------------------------------------------------ the rig

struct T {
    base: String,
    master: Master,
    forge: Forge,
    remote: Node,
    old: Node,
    dir: tempfile::TempDir,
    alex: String,
    answers: Arc<Mutex<String>>,
}

async fn start() -> T {
    logs();
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, "CLAIM").unwrap();
    let store = Store::open(&dir.path().join("master.db"), Durability::Normal).unwrap();
    let master = Master::new(
        store,
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
    master.set_forge_hosts(vec![forge.host()]);
    master.set_forge_timeout(Duration::from_secs(2));
    let (l, remote) = fake("remote", &[CAP_PROJECTS, "forge-clone:600"]);
    master.add_link(l, false).await.unwrap();
    let (l, old) = fake("old", &[CAP_PROJECTS]);
    master.add_link(l, false).await.unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    let base = format!("http://{addr}");
    let r = reqwest::Client::new()
        .post(format!("{base}/api/setup"))
        .json(&json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    let alex = common::signed_in(r).await;
    T {
        base,
        master,
        forge,
        remote,
        old,
        dir,
        alex,
        answers: Arc::default(),
    }
}

async fn req(
    t: &T,
    method: Method,
    path: &str,
    body: Option<Value>,
    who: &str,
) -> (StatusCode, Value) {
    let mut rb = reqwest::Client::new().request(method, format!("{}{path}", t.base));
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    rb = match who {
        "api" => rb.bearer_auth("api-token-xyz"),
        c => rb.login(c).same_origin(),
    };
    let r = rb.send().await.unwrap();
    let st = r.status();
    let text = r.text().await.unwrap();
    t.answers
        .lock()
        .unwrap()
        .push_str(&format!("{st} {text}\n"));
    (st, serde_json::from_str(&text).unwrap_or(Value::Null))
}

/// A member with the sandbox level, logged in: (cookie, user id).
async fn member(t: &T, name: &str) -> (String, String) {
    let (st, v) = req(
        t,
        Method::POST,
        "/api/users",
        Some(json!({"username": name, "role": "member", "exec_ceiling": "sandbox"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let id = v["user"]["user_id"]
        .as_str()
        .or(v["user_id"].as_str())
        .unwrap_or_default()
        .to_string();
    let pw = v["password"].as_str().unwrap().to_string();
    let r = reqwest::Client::new()
        .post(format!("{}/api/login", t.base))
        .json(&json!({"username": name, "password": pw}))
        .send()
        .await
        .unwrap();
    let temp = common::signed_in(r).await;
    let r = reqwest::Client::new()
        .post(format!("{}/api/me/password", t.base))
        .json(&json!({"current": pw, "new": "a password of my own"}))
        .login(&temp)
        .same_origin()
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let c = common::signed_in(r).await;
    let id = if id.is_empty() {
        let (_, me) = req(t, Method::GET, "/api/me", None, &c).await;
        me["user"]["user_id"].as_str().unwrap().to_string()
    } else {
        id
    };
    (c, id)
}

async fn connect(t: &T, c: &str, token: &str) -> String {
    let (st, v) = req(
        t,
        Method::POST,
        "/api/connections",
        Some(json!({"host": "127.0.0.1", "token": token})),
        c,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["id"].as_str().unwrap().to_string()
}

fn clone_body(conn: &str, full: &str, node: &str) -> Value {
    json!({"name": "Harness", "folder_on": node, "connection_id": conn, "full_name": full})
}

async fn project_count(t: &T, who: &str) -> usize {
    let (_, v) = req(t, Method::GET, "/api/projects", None, who).await;
    v["projects"].as_array().map_or(0, Vec::len)
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

/// No token in the master's data directory (its database holds the
/// sealed row, which is ciphertext), its logs at TRACE, any HTTP answer
/// or its audit lines.
fn assert_no_token_anywhere(t: &T) {
    let mut files = String::new();
    for e in walk(t.dir.path()) {
        if let Ok(b) = std::fs::read(&e) {
            files.push_str(&String::from_utf8_lossy(&b));
        }
    }
    let logs = String::from_utf8_lossy(&logs().lock().unwrap()).to_string();
    let answers = t.answers.lock().unwrap().clone();
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
    for tok in [TOKEN_A, TOKEN_B] {
        for (what, text) in [
            ("data directory", files.as_str()),
            ("logs", logs.as_str()),
            ("HTTP answers", answers.as_str()),
            ("audit", &audit.join("\n")),
        ] {
            assert!(!text.contains(&tok[8..]), "{tok} found in {what}");
        }
    }
}

// ------------------------------------------------------------ tests

/// A private repository: one forge call by name with the connection's
/// token; the node is sent the connection's host, the forge's spelling of
/// the name, the login and the token; the project records the source and
/// the folder. The token is nowhere else.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_private_repository_is_cloned_with_its_connections_token() {
    let t = start().await;
    let (alice, _) = member(&t, "alice").await;
    let conn = connect(&t, &alice, TOKEN_A).await;
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/projects",
        Some(clone_body(&conn, "Ada/Secret", "remote")),
        &alice,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        t.remote.clones(),
        vec![Asked {
            name: "Harness".into(),
            host: "127.0.0.1".into(),
            full_name: "ada/secret".into(),
            username: Some("ada".into()),
            token: Some(TOKEN_A.into()),
        }]
    );
    assert_eq!(
        t.forge.repo_calls(),
        vec![(
            "/gh/repos/Ada/Secret".to_string(),
            Some(format!("Bearer {TOKEN_A}"))
        )]
    );
    assert_eq!(
        v["source"],
        json!({"host": "127.0.0.1", "full_name": "ada/secret"})
    );
    assert_eq!(v["settings"]["node_id"], "remote");
    assert_eq!(
        v["settings"]["cwd"],
        folder_of("remote", "Harness").as_str()
    );
    let pid = v["project_id"].as_str().unwrap();
    let (_, d) = req(
        &t,
        Method::GET,
        &format!("/api/projects/{pid}"),
        None,
        &alice,
    )
    .await;
    assert_eq!(d["source"]["full_name"], "ada/secret");
    // A PATCH cannot set it (the settings document has no such field).
    let (st, _) = req(
        &t,
        Method::PATCH,
        &format!("/api/projects/{pid}"),
        Some(json!({"source": {"host": "evil.example", "full_name": "x/y"}})),
        &alice,
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text_free_of_url(&v));
    assert_no_token_anywhere(&t);
}

fn text_free_of_url(v: &Value) -> bool {
    !v.to_string().contains("evil.example")
}

/// A public repository: no token and no login leave the master.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_public_repository_is_cloned_without_a_credential() {
    let t = start().await;
    let (alice, _) = member(&t, "alice").await;
    let conn = connect(&t, &alice, TOKEN_A).await;
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/projects",
        Some(clone_body(&conn, "ada/open", "remote")),
        &alice,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let asked = t.remote.clones();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].token, None, "a public repository took the token");
    assert_eq!(asked[0].username, None);
    assert_eq!(asked[0].full_name, "ada/open");
    assert_no_token_anywhere(&t);
}

/// Whose token: Bob with Alice's connection is 404 (as the listing), the
/// API token 403, a disabled Alice 401; nobody's token is opened, no
/// forge call made, no node asked, no project left.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_the_connections_owner_clones_through_it() {
    let t = start().await;
    let (alice, alice_id) = member(&t, "alice").await;
    let (bob, _) = member(&t, "bob").await;
    let conn = connect(&t, &alice, TOKEN_A).await;
    let _ = connect(&t, &bob, TOKEN_B).await;
    let before = t.forge.seen.lock().unwrap().len();
    let body = clone_body(&conn, "ada/secret", "remote");
    let (st, v) = req(&t, Method::POST, "/api/projects", Some(body.clone()), &bob).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{v}");
    // The admin, in their own workspace, with Alice's connection id: 404.
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/projects",
        Some(body.clone()),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{v}");
    let mut api = body.clone();
    api["workspace_id"] = json!(
        t.master
            .inner_mirror()
            .personal_workspace(&alice_id)
            .unwrap()
    );
    let (st, v) = req(&t, Method::POST, "/api/projects", Some(api), "api").await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert!(v["error"].as_str().unwrap().contains("API token"), "{v}");
    let (st, _) = req(
        &t,
        Method::PATCH,
        &format!("/api/users/{alice_id}"),
        Some(json!({"disabled": true})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = req(&t, Method::POST, "/api/projects", Some(body), &alice).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    assert_eq!(
        t.forge.seen.lock().unwrap().len(),
        before,
        "a forge call was made"
    );
    assert!(t.remote.clones().is_empty());
    assert_eq!(project_count(&t, &bob).await, 0);
    assert_no_token_anywhere(&t);
}

/// Refused before the forge is asked: a host the server no longer allows
/// (409, as the listing), an older node (409, never sent the message), a
/// name of another shape (400), the fields without each other or without
/// `folder_on` (400).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn refusals_come_before_the_forge_and_the_node() {
    let t = start().await;
    let (alice, _) = member(&t, "alice").await;
    let conn = connect(&t, &alice, TOKEN_A).await;
    let before = t.forge.seen.lock().unwrap().len();
    let post = |b: Value| req(&t, Method::POST, "/api/projects", Some(b), &alice);
    let (st, v) = post(clone_body(&conn, "ada/secret", "old")).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("cannot clone"), "{v}");
    assert!(
        t.old.clones().is_empty(),
        "an older node was sent project.clone"
    );
    for bad in [
        "../x",
        "ada/..",
        "a/b/c",
        "ada/x y",
        "https://evil.example/a/b",
        "-a/b",
        "ada/b.git/x",
        "ada",
        "ada/b\u{0}",
    ] {
        let (st, v) = post(clone_body(&conn, bad, "remote")).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{bad:?}: {v}");
    }
    let (st, _) = post(json!({"name": "x", "folder_on": "remote", "connection_id": conn})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) =
        post(json!({"name": "x", "folder_on": "remote", "full_name": "ada/secret"})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) =
        post(json!({"name": "x", "connection_id": conn, "full_name": "ada/secret"})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = post(
        json!({"name": "x", "folder_on": "remote", "connection_id": conn,
                              "full_name": "ada/secret", "url": "file:///etc"}),
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "an unknown field");
    assert_eq!(t.forge.seen.lock().unwrap().len(), before);
    t.master.set_forge_hosts(vec![]);
    let (st, v) = post(clone_body(&conn, "ada/secret", "remote")).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"].as_str().unwrap().contains("no longer allows"),
        "{v}"
    );
    assert_eq!(t.forge.seen.lock().unwrap().len(), before);
    assert!(t.remote.clones().is_empty());
    assert_eq!(project_count(&t, &alice).await, 0);
    assert_no_token_anywhere(&t);
}

/// The forge's answer decides: a repository it does not have for this
/// token is 404; one it answers under another name (renamed, moved) is
/// refused; a malformed answer is refused; no node is asked for any.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_forge_must_know_the_repository_by_that_name() {
    let t = start().await;
    let (alice, _) = member(&t, "alice").await;
    let conn = connect(&t, &alice, TOKEN_A).await;
    let post = |b: Value| req(&t, Method::POST, "/api/projects", Some(b), &alice);
    let (st, v) = post(clone_body(&conn, "ada/nothing", "remote")).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{v}");
    let (st, v) = post(clone_body(&conn, "ada/renamed", "remote")).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    let (st, v) = post(clone_body(&conn, "ada/broken", "remote")).await;
    assert_eq!(st, StatusCode::BAD_GATEWAY, "{v}");
    assert!(t.remote.clones().is_empty());
    assert_eq!(project_count(&t, &alice).await, 0);
    assert_no_token_anywhere(&t);
}
