//! Cloning a forge repository into a new project's folder over HTTP
//! (ADOPTION_PLAN X.27 step 3, PLAN.md T86), against a fake forge on
//! 127.0.0.1 and fake nodes: `POST /api/projects` with `folder_on`,
//! `connection_id` and `full_name`. Only the connection's owner clones
//! through it; the repository is looked up by name with that connection
//! (one forge call) before any node is asked; the token goes to the node
//! only for a private repository; an older node is never sent the message;
//! the project records where its folder came from. No real forge and no
//! real token.

use crate::common;
use common::{Login as _, SameOrigin as _};

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

use crate::capture::logs;

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
        // A listing that takes a while: it holds the person's listing slot.
        "/gh/user/repos" => {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            axum::Json(json!([])).into_response()
        }
        "/gh/repos/ada/secret" => repo("ada/secret", true),
        "/gh/repos/ada/open" => repo("ada/open", false),
        // A forge that is slow to answer: the lookup holds the person's slot.
        "/gh/repos/ada/slow" => {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            repo("ada/slow", false)
        }
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

#[derive(Clone)]
struct Node {
    seen: Arc<Mutex<Vec<MasterToNode>>>,
    /// Requests for a project named `Hold…`, not answered until
    /// [`Node::answer`].
    held: Arc<Mutex<Vec<String>>>,
    tx: mpsc::Sender<NodeToMaster>,
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

/// What the node answers a clone of a project named `name` with: a folder,
/// or its refusal for the `Fail …` names (the node's own words).
fn node_answer(node: &str, name: &str) -> (Option<String>, Option<String>) {
    let why = match name {
        "Fail host" => {
            "this node does not clone from that host: only from the hosts its \
                        node.toml lists ([forge] hosts)"
        }
        "Fail size" => {
            "the clone grew past what this node allows ([limits] clone_max_mb = 1 MB); \
                        git was stopped and its files removed"
        }
        "Fail time" => {
            "the clone took longer than this node allows ([limits] clone_timeout_s = \
                        1); git was stopped and its files removed"
        }
        "Fail credential" => "the repository's credential is malformed; nothing was cloned",
        _ => return (Some(folder_of(node, name)), None),
    };
    (None, Some(why.to_string()))
}

impl Node {
    /// Answer the oldest held request with its folder.
    fn answer(&self, node: &str, name: &str) {
        let req_id = self.held.lock().unwrap().remove(0);
        self.tx
            .try_send(NodeToMaster::ProjectCreated {
                req_id,
                path: Some(folder_of(node, name)),
                error: None,
            })
            .unwrap();
    }

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

/// What a leaking node says: the token itself, the same split by a
/// zero-width space, its Basic form, control characters and a bidi
/// override, then enough text to pass the 300-character cut. `late`: after
/// 270 characters, so that the cut falls inside the token.
fn leak_text(username: Option<&str>, token: Option<&str>, late: bool) -> String {
    use base64::Engine as _;
    let (u, t) = (username.unwrap_or("nobody"), token.unwrap_or("none"));
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("{u}:{t}"));
    let (a, b) = t.split_at(t.len() / 2);
    let lead = if late { "x".repeat(270) } else { String::new() };
    format!(
        "{lead}git said: fatal: Authentication failed for {t} (Basic {basic}) again \
         {a}\u{200b}{b}; line\nbreak\u{7} \u{202e}txet desrever\u{2066} {}",
        "y".repeat(400)
    )
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
    let n = Node {
        seen: Arc::default(),
        held: Arc::default(),
        tx: tx.clone(),
    };
    let (keep, held, node) = (n.seen.clone(), n.held.clone(), id.to_string());
    tokio::spawn(async move {
        while let Some(msg) = commands.recv().await {
            keep.lock().unwrap().push(msg.clone());
            match &msg {
                // A node whose refusal echoes the credential it was sent,
                // in its own spelling and as an HTTP Basic header, around
                // hidden characters and past the cut (`leak_text`).
                MasterToNode::ProjectClone {
                    req_id,
                    name,
                    username,
                    token,
                    ..
                } if name.starts_with("Leak") => {
                    let token = token.as_ref().map(|t| t.expose().clone());
                    let _ = tx.try_send(NodeToMaster::ProjectCreated {
                        req_id: req_id.clone(),
                        path: None,
                        error: Some(leak_text(
                            username.as_deref(),
                            token.as_deref(),
                            name.starts_with("Leak late"),
                        )),
                    });
                }
                MasterToNode::ProjectCreate { req_id, name }
                | MasterToNode::ProjectClone { req_id, name, .. } => {
                    if name.starts_with("Hold") {
                        held.lock().unwrap().push(req_id.clone());
                    } else {
                        let (path, error) = node_answer(&node, name);
                        let _ = tx.try_send(NodeToMaster::ProjectCreated {
                            req_id: req_id.clone(),
                            path,
                            error,
                        });
                    }
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

/// `who`'s pending projects (in their current workspace, or `ws`).
async fn pending(t: &T, who: &str, ws: Option<&str>) -> Vec<Value> {
    let path = match ws {
        Some(w) => format!("/api/projects?workspace={w}"),
        None => "/api/projects".into(),
    };
    let (st, v) = req(t, Method::GET, &path, None, who).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["pending"].as_array().cloned().unwrap_or_default()
}

/// Wait until project `id` exists for `who`; its detail.
async fn ready(t: &T, who: &str, id: &str) -> Value {
    for _ in 0..200 {
        let (st, v) = req(t, Method::GET, &format!("/api/projects/{id}"), None, who).await;
        if st == StatusCode::OK {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("project {id} never became ready");
}

/// Wait until `who`'s pending project `id` (in `ws`, or their current
/// workspace) has failed; its entry.
async fn failed(t: &T, who: &str, id: &str, ws: Option<&str>) -> Value {
    for _ in 0..200 {
        let p = pending(t, who, ws).await;
        if let Some(e) = p
            .iter()
            .find(|e| e["project_id"] == id && e["state"] == "failed")
        {
            return e.clone();
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("clone {id} never failed");
}

/// Wait until the held clone count of `n` is `want`.
async fn held(n: &Node, want: usize) {
    for _ in 0..200 {
        if n.held.lock().unwrap().len() == want {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the node never held {want} clones");
}

/// A team workspace of Alex's with Alice (admin) and Bob (member), both
/// at the sandbox level there.
async fn team(t: &T, alice: &str, bob: &str) -> String {
    let (st, v) = req(
        t,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "team"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let team = v["workspace_id"].as_str().unwrap().to_string();
    for (u, id, role) in [("alice", alice, "admin"), ("bob", bob, "member")] {
        let (st, v) = req(
            t,
            Method::POST,
            &format!("/api/workspaces/{team}/members"),
            Some(json!({"username": u, "role": role})),
            &t.alex,
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
        let (st, v) = req(
            t,
            Method::PUT,
            &format!("/api/workspaces/{team}/members/{id}/exec-level"),
            Some(json!({"level": "sandbox"})),
            &t.alex,
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }
    team
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

/// None of `needles` in the data directory (the database's pages and
/// write-ahead log included: reasons are stored in the clear), the logs
/// at TRACE, or any HTTP answer.
fn assert_absent(t: &T, needles: &[String]) {
    let mut files = String::new();
    for e in walk(t.dir.path()) {
        if let Ok(b) = std::fs::read(&e) {
            files.push_str(&String::from_utf8_lossy(&b));
        }
    }
    let logs = String::from_utf8_lossy(&logs().lock().unwrap()).to_string();
    let answers = t.answers.lock().unwrap().clone();
    for n in needles {
        for (what, text) in [
            ("data directory", &files),
            ("logs", &logs),
            ("HTTP answers", &answers),
        ] {
            assert!(!text.contains(n.as_str()), "{n} found in {what}");
        }
    }
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
    assert_eq!(st, StatusCode::ACCEPTED, "{v}");
    let v = ready(&t, &alice, v["project_id"].as_str().unwrap()).await;
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
    assert_eq!(st, StatusCode::ACCEPTED, "{v}");
    ready(&t, &alice, v["project_id"].as_str().unwrap()).await;
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
    // A refusal the request answered leaves no pending project either.
    assert!(t.master.inner_mirror().clones(None).unwrap().is_empty());
    assert!(pending(&t, &alice, None).await.is_empty());
    assert_eq!(project_count(&t, &alice).await, 0);
    assert_no_token_anywhere(&t);
}

// ------------------------------------------------------------ pending

/// The request answers at once (202) with the project pending: stored
/// (another device's list shows it), `cloning`, with what is cloned, not a
/// project (no detail, no chat in it); `projects_changed` goes out; once
/// the node answers, it is a project with its folder and source, and
/// `projects_changed` goes out again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_clone_answers_at_once_and_becomes_a_project_when_its_node_is_done() {
    let t = start().await;
    let (alice, _) = member(&t, "alice").await;
    let conn = connect(&t, &alice, TOKEN_A).await;
    let mut live = t.master.subscribe();
    let mut body = clone_body(&conn, "Ada/Secret", "remote");
    body["name"] = json!("Hold me");
    let (st, v) = req(&t, Method::POST, "/api/projects", Some(body), &alice).await;
    assert_eq!(st, StatusCode::ACCEPTED, "{v}");
    assert_eq!(v["state"], "cloning", "{v}");
    assert_eq!(
        v["source"],
        json!({"host": "127.0.0.1", "full_name": "ada/secret"})
    );
    assert_eq!(v["mine"], true);
    assert_eq!(v["node_id"], "remote");
    let id = v["project_id"].as_str().unwrap().to_string();
    held(&t.remote, 1).await;
    // Stored and listed apart; not a project.
    let p = pending(&t, &alice, None).await;
    assert_eq!(p.len(), 1);
    assert_eq!(p[0]["project_id"], id.as_str());
    assert_eq!(p[0]["state"], "cloning");
    assert_eq!(p[0]["reason"], Value::Null);
    assert_eq!(project_count(&t, &alice).await, 0);
    let rows = t.master.inner_mirror().clones(None).unwrap();
    assert_eq!(rows.len(), 1, "the pending project is not stored");
    let (st, _) = req(
        &t,
        Method::GET,
        &format!("/api/projects/{id}"),
        None,
        &alice,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let changes = |live: &mut tokio::sync::broadcast::Receiver<harness_master::Live>| {
        let mut n = 0;
        while let Ok(l) = live.try_recv() {
            if matches!(l, harness_master::Live::ProjectsChanged(_)) {
                n += 1;
            }
        }
        n
    };
    assert!(changes(&mut live) >= 1, "no projects_changed at the start");
    t.remote.answer("remote", "Hold me");
    let d = ready(&t, &alice, &id).await;
    assert_eq!(
        d["settings"]["cwd"],
        folder_of("remote", "Hold me").as_str()
    );
    assert_eq!(d["settings"]["node_id"], "remote");
    assert_eq!(
        d["source"],
        json!({"host": "127.0.0.1", "full_name": "ada/secret"})
    );
    assert!(pending(&t, &alice, None).await.is_empty());
    assert!(changes(&mut live) >= 1, "no projects_changed at the end");
    assert_no_token_anywhere(&t);
}

/// Two tabs or a double click: one clone. The same person, repository
/// (in any case) and node while one is pending is 409 with a plain
/// message and both tabs list the one pending project; at once, exactly
/// one of two requests starts a clone. Another repository is not held up,
/// and once the first has failed the same one may be tried again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_same_repository_twice_is_one_clone() {
    let t = start().await;
    let (alice, _) = member(&t, "alice").await;
    let conn = connect(&t, &alice, TOKEN_A).await;
    let mut body = clone_body(&conn, "ada/secret", "remote");
    body["name"] = json!("Hold one");
    let (a, b) = tokio::join!(
        req(
            &t,
            Method::POST,
            "/api/projects",
            Some(body.clone()),
            &alice
        ),
        req(
            &t,
            Method::POST,
            "/api/projects",
            Some(body.clone()),
            &alice
        ),
    );
    let mut codes = [a.0, b.0];
    codes.sort();
    assert_eq!(
        codes,
        [StatusCode::ACCEPTED, StatusCode::CONFLICT],
        "{a:?} {b:?}"
    );
    let refused = if a.0 == StatusCode::CONFLICT {
        &a.1
    } else {
        &b.1
    };
    assert!(
        refused["error"]
            .as_str()
            .unwrap()
            .contains("is being cloned onto this node for you already"),
        "{refused}"
    );
    held(&t.remote, 1).await;
    let mut again = body.clone();
    again["full_name"] = json!("ADA/SECRET");
    let (st, _) = req(&t, Method::POST, "/api/projects", Some(again), &alice).await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert_eq!(t.remote.clones().len(), 1, "a second clone was sent");
    assert_eq!(pending(&t, &alice, None).await.len(), 1);
    let mut other = clone_body(&conn, "ada/open", "remote");
    other["name"] = json!("Hold two");
    let (st, v) = req(&t, Method::POST, "/api/projects", Some(other), &alice).await;
    assert_eq!(st, StatusCode::ACCEPTED, "{v}");
    held(&t.remote, 2).await;
    // Once the first has ended, the same repository is a new clone.
    t.remote.answer("remote", "Hold one");
    for _ in 0..200 {
        if pending(&t, &alice, None).await.len() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let (st, v) = req(&t, Method::POST, "/api/projects", Some(body), &alice).await;
    assert_eq!(st, StatusCode::ACCEPTED, "{v}");
}

/// The node refuses (host not allowed, size cap, time limit, credential
/// not received) or does not answer in time: the pending project ends
/// `failed` with the node's reason, for its starter only; it is no
/// project; another member sees nothing of it and cannot dismiss it; its
/// starter dismisses it; a person keeps at most ten failed ones, and
/// failed ones count against no limit.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_clone_shows_its_reason_to_its_starter_only() {
    let t = start().await;
    let (alice, alice_id) = member(&t, "alice").await;
    let (bob, bob_id) = member(&t, "bob").await;
    let ws = team(&t, &alice_id, &bob_id).await;
    let conn = connect(&t, &alice, TOKEN_A).await;
    let mut live = t.master.subscribe();
    let mut ids = Vec::new();
    for (name, said) in [
        ("Fail host", "does not clone from that host"),
        ("Fail size", "clone_max_mb"),
        ("Fail time", "clone_timeout_s"),
        ("Fail credential", "credential is malformed"),
    ] {
        let mut body = clone_body(&conn, "ada/secret", "remote");
        body["name"] = json!(name);
        body["workspace_id"] = json!(ws);
        let (st, v) = req(&t, Method::POST, "/api/projects", Some(body), &alice).await;
        assert_eq!(st, StatusCode::ACCEPTED, "{name}: {v}");
        let id = v["project_id"].as_str().unwrap().to_string();
        let mut e = Value::Null;
        for _ in 0..200 {
            if let Some(x) = pending(&t, &alice, Some(&ws))
                .await
                .into_iter()
                .find(|x| x["project_id"] == id.as_str() && x["state"] == "failed")
            {
                e = x;
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(
            e["reason"].as_str().unwrap_or("").contains(said),
            "{name}: {e}"
        );
        ids.push(id);
    }
    // No answer in time.
    t.master.set_project_folder_wait(Duration::from_millis(50));
    let mut body = clone_body(&conn, "ada/open", "remote");
    body["name"] = json!("Hold late");
    body["workspace_id"] = json!(ws);
    let (st, v) = req(&t, Method::POST, "/api/projects", Some(body), &alice).await;
    assert_eq!(st, StatusCode::ACCEPTED, "{v}");
    let id = v["project_id"].as_str().unwrap().to_string();
    let mut e = Value::Null;
    for _ in 0..200 {
        if let Some(x) = pending(&t, &alice, Some(&ws))
            .await
            .into_iter()
            .find(|x| x["project_id"] == id.as_str() && x["state"] == "failed")
        {
            e = x;
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(
        e["reason"]
            .as_str()
            .unwrap()
            .contains("did not answer in time"),
        "{e}"
    );
    ids.push(id);
    let mut n = 0;
    while let Ok(l) = live.try_recv() {
        if matches!(l, harness_master::Live::ProjectsChanged(_)) {
            n += 1;
        }
    }
    assert!(n >= 10, "projects_changed at each start and end: {n}");
    // No project; Bob, a member, sees none of the failures.
    let (_, v) = req(
        &t,
        Method::GET,
        &format!("/api/projects?workspace={ws}"),
        None,
        &alice,
    )
    .await;
    assert_eq!(v["projects"], json!([]));
    assert!(pending(&t, &bob, Some(&ws)).await.is_empty());
    for id in &ids {
        let (st, _) = req(
            &t,
            Method::GET,
            &format!("/api/projects/{id}"),
            None,
            &alice,
        )
        .await;
        assert_eq!(st, StatusCode::NOT_FOUND);
        let (st, _) = req(
            &t,
            Method::DELETE,
            &format!("/api/projects/{id}/clone"),
            None,
            &bob,
        )
        .await;
        assert_eq!(st, StatusCode::NOT_FOUND, "Bob dismissed Alice's failure");
        let (st, _) = req(
            &t,
            Method::DELETE,
            &format!("/api/projects/{id}/clone"),
            None,
            &t.alex,
        )
        .await;
        assert_eq!(
            st,
            StatusCode::NOT_FOUND,
            "the workspace's owner dismissed Alice's failure"
        );
    }
    let (st, _) = req(
        &t,
        Method::DELETE,
        &format!("/api/projects/{}/clone", ids[0]),
        None,
        &alice,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(pending(&t, &alice, Some(&ws)).await.len(), ids.len() - 1);
    // At most ten kept per person; the newest stay.
    t.master.set_project_folder_wait(Duration::from_millis(0));
    for i in 0..8 {
        let mut body = clone_body(&conn, "ada/secret", "remote");
        body["name"] = json!("Fail host");
        body["workspace_id"] = json!(ws);
        let (st, v) = req(&t, Method::POST, "/api/projects", Some(body), &alice).await;
        assert_eq!(st, StatusCode::ACCEPTED, "{i}: {v}");
        failed(&t, &alice, v["project_id"].as_str().unwrap(), Some(&ws)).await;
    }
    let left = t.master.inner_mirror().clones(None).unwrap();
    assert_eq!(left.len(), 10, "failed clones are not bounded");
    assert!(left.iter().all(|c| c.failed.is_some()));
    assert_no_token_anywhere(&t);
}

/// A master restart while a clone runs: at start every running clone is
/// failed with a reason that says the server restarted; none stays
/// cloning.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_master_restart_leaves_no_clone_cloning() {
    let t = start().await;
    let (alice, _) = member(&t, "alice").await;
    let conn = connect(&t, &alice, TOKEN_A).await;
    let mut body = clone_body(&conn, "ada/secret", "remote");
    body["name"] = json!("Hold over restart");
    let (st, v) = req(&t, Method::POST, "/api/projects", Some(body), &alice).await;
    assert_eq!(st, StatusCode::ACCEPTED, "{v}");
    held(&t.remote, 1).await;
    // A second master over the same database is what a restart starts.
    let store = Store::open(&t.dir.path().join("master.db"), Durability::Normal).unwrap();
    let _again = Master::new(
        store,
        Auth::new(Secret::new("api-token-xyz".into()), vec![], false),
    );
    let c = rusqlite::Connection::open(t.dir.path().join("master.db")).unwrap();
    let rows: Vec<(String, Option<String>)> = c
        .prepare("SELECT state, reason FROM project_clones")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "failed");
    assert!(
        rows[0].1.as_deref().unwrap().contains("server restarted"),
        "{rows:?}"
    );
    let e = failed(&t, &alice, v["project_id"].as_str().unwrap(), None).await;
    assert!(e["reason"].as_str().unwrap().contains("server restarted"));
}

/// A member who is not the starter sees `cloning` (what is cloned, not
/// whose connection: no login, no user id) and cannot use it: every route
/// that takes a project id refuses a pending one, for the starter too;
/// another workspace's member sees nothing of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pending_project_is_no_project_for_anyone() {
    let t = start().await;
    let (alice, alice_id) = member(&t, "alice").await;
    let (bob, bob_id) = member(&t, "bob").await;
    let (carol, _) = member(&t, "carol").await;
    let ws = team(&t, &alice_id, &bob_id).await;
    let conn = connect(&t, &alice, TOKEN_A).await;
    let mut body = clone_body(&conn, "ada/secret", "remote");
    body["name"] = json!("Hold pending");
    body["workspace_id"] = json!(ws);
    let (st, v) = req(&t, Method::POST, "/api/projects", Some(body), &alice).await;
    assert_eq!(st, StatusCode::ACCEPTED, "{v}");
    let id = v["project_id"].as_str().unwrap().to_string();
    held(&t.remote, 1).await;
    let seen = pending(&t, &bob, Some(&ws)).await;
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0]["state"], "cloning");
    assert_eq!(seen[0]["mine"], false);
    assert_eq!(
        seen[0]["source"],
        json!({"host": "127.0.0.1", "full_name": "ada/secret"})
    );
    let text = seen[0].to_string();
    assert!(
        !text.contains(&alice_id) && !text.contains("\"ada\"") && !text.contains("alice"),
        "{text}"
    );
    // Carol is in no workspace with it.
    let (st, v) = req(
        &t,
        Method::GET,
        &format!("/api/projects?workspace={ws}"),
        None,
        &carol,
    )
    .await;
    assert_ne!(st, StatusCode::OK, "{v}");
    assert!(pending(&t, &carol, None).await.is_empty());
    // A chat of Bob's and of Alice's in the team, to try linking.
    let mut chats = Vec::new();
    for c in [&alice, &bob] {
        let (st, v) = req(
            &t,
            Method::POST,
            "/api/sessions",
            Some(
                json!({"profile": "agentic", "title": "x", "workspace_id": ws,
                        "node_id": "remote", "cwd": "/home/remote/work"}),
            ),
            c,
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
        chats.push(v["session_id"].as_str().unwrap().to_string());
    }
    let routes: Vec<(Method, String, Option<Value>)> = vec![
        (Method::GET, format!("/api/projects/{id}"), None),
        (
            Method::PATCH,
            format!("/api/projects/{id}"),
            Some(json!({"name": "x"})),
        ),
        (Method::DELETE, format!("/api/projects/{id}"), None),
        (Method::GET, format!("/api/projects/{id}/board"), None),
        (
            Method::POST,
            format!("/api/projects/{id}/board/columns"),
            Some(json!({"name": "c"})),
        ),
        (
            Method::POST,
            format!("/api/projects/{id}/board/cards"),
            Some(json!({"column_id": "c", "title": "t"})),
        ),
        (
            Method::POST,
            format!("/api/projects/{id}/board/template"),
            Some(json!({"template_id": "basic"})),
        ),
        (Method::GET, format!("/api/projects/{id}/memory"), None),
        (
            Method::POST,
            format!("/api/projects/{id}/memory"),
            Some(json!({"text": "a fact"})),
        ),
        (
            Method::POST,
            format!("/api/projects/{id}/folder"),
            Some(json!({"node_id": "remote"})),
        ),
        (Method::GET, format!("/api/projects/{id}/mcp"), None),
        (
            Method::POST,
            "/api/board-templates".into(),
            Some(json!({"name": "t", "project_id": id})),
        ),
        (
            Method::POST,
            "/api/sessions".into(),
            Some(
                json!({"profile": "agentic", "title": "x", "workspace_id": ws, "project_id": id,
                        "node_id": "remote", "cwd": "/home/remote/work"}),
            ),
        ),
        (
            Method::POST,
            "/api/routines".into(),
            Some(json!({"name": "r", "workspace_id": ws,
                        "spec": {"profile": "chat", "agent_id": "general", "prompt": "p", "project_id": id},
                        "triggers": [{"kind": "schedule", "config": {"type": "interval", "every_minutes": 60}}]})),
        ),
    ];
    for (who, chat) in [(&alice, &chats[0]), (&bob, &chats[1])] {
        for (method, path, body) in &routes {
            let (st, v) = req(&t, method.clone(), path, body.clone(), who).await;
            // Refused for the project, not for anything else in the body.
            assert!(
                (st == StatusCode::NOT_FOUND || st == StatusCode::BAD_REQUEST)
                    && v["error"].as_str().unwrap_or("").contains("project"),
                "{method} {path}: {st} {v}"
            );
        }
        let (st, v) = req(
            &t,
            Method::PUT,
            &format!("/api/sessions/{chat}/project"),
            Some(json!({"project_id": id})),
            who,
        )
        .await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{v}");
    }
    // Still pending, never written as a project by any of it.
    assert!(t.master.inner_mirror().project(&id).unwrap().is_none());
    assert_eq!(pending(&t, &bob, Some(&ws)).await.len(), 1);
    let (st, _) = req(
        &t,
        Method::DELETE,
        &format!("/api/projects/{id}/clone"),
        None,
        &alice,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "a running clone was dismissed");
}

// ------------------------------------------------ review of PR 164, round 2

/// A node whose link closes as soon as it is sent a `project.clone`: a node
/// that loses its link mid-clone (its own clone task would go on).
fn dropper(id: &str) -> NodeLink {
    let (to_node, mut commands) = mpsc::channel::<MasterToNode>(64);
    let (tx, from_node) = mpsc::channel::<NodeToMaster>(64);
    tx.try_send(NodeToMaster::Hello {
        protocol_version: PROTOCOL_VERSION,
        node_id: id.into(),
        hostname: "h".into(),
        version: "1".into(),
        capabilities: vec![
            "native".into(),
            "profile:agentic".into(),
            "exec:sandbox".into(),
            CAP_PROJECTS.into(),
            "forge-clone:600".into(),
        ],
        sessions: vec![],
    })
    .unwrap();
    tokio::spawn(async move {
        while let Some(msg) = commands.recv().await {
            if matches!(msg, MasterToNode::ProjectClone { .. }) {
                break;
            }
            if let Some(req_id) = msg.req_id() {
                let _ = tx.try_send(NodeToMaster::Ack {
                    req_id: req_id.into(),
                    ok: true,
                    error: None,
                });
            }
        }
        drop(tx);
        drop(commands);
    });
    NodeLink { to_node, from_node }
}

/// Wait until the stored clone `id` has failed; its reason.
async fn stored_failure(t: &T, id: &str) -> String {
    for _ in 0..200 {
        if let Some(r) = t.master.inner_mirror().clone_of(id).unwrap()
            && let Some(why) = r.failed
        {
            return why;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("clone {id} never failed");
}

/// M1: the node's link closing while it clones is not "not connected, start
/// it or choose another node": the node goes on cloning, and a retry would
/// start a second clone. The reason says the clone may still finish there.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_link_lost_mid_clone_says_the_clone_may_still_finish() {
    let t = start().await;
    t.master.add_link(dropper("flaky"), false).await.unwrap();
    let (alice, _) = member(&t, "alice").await;
    let conn = connect(&t, &alice, TOKEN_A).await;
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/projects",
        Some(clone_body(&conn, "ada/secret", "flaky")),
        &alice,
    )
    .await;
    assert_eq!(st, StatusCode::ACCEPTED, "{v}");
    let id = v["project_id"].as_str().unwrap().to_string();
    let e = failed(&t, &alice, &id, None).await;
    let why = e["reason"].as_str().unwrap();
    assert!(why.contains("link closed while it was cloning"), "{why}");
    assert!(why.contains("may still finish"), "{why}");
    assert!(why.contains("Existing folder"), "{why}");
    assert!(why.contains("harness-"), "the folder's name: {why}");
    for advice in ["not connected", "start it", "choose another", "try again"] {
        assert!(!why.contains(advice), "{advice:?} in: {why}");
    }
}

/// What a starter loses while their clone runs.
enum Loss {
    Removed,
    RoleLowered,
    Disabled,
}

/// M2: the starter's right was checked at the POST only; it is checked again
/// when the node answers. Without it no project is written in their name,
/// and the row fails with a reason that names the folder the node made.
async fn starter_loses_right_mid_clone(loss: Loss) {
    let t = start().await;
    let (alice, alice_id) = member(&t, "alice").await;
    let (_bob, bob_id) = member(&t, "bob").await;
    let ws = team(&t, &alice_id, &bob_id).await;
    let conn = connect(&t, &alice, TOKEN_A).await;
    let mut body = clone_body(&conn, "ada/secret", "remote");
    body["name"] = json!("Hold lost");
    body["workspace_id"] = json!(ws);
    let (st, v) = req(&t, Method::POST, "/api/projects", Some(body), &alice).await;
    assert_eq!(st, StatusCode::ACCEPTED, "{v}");
    let id = v["project_id"].as_str().unwrap().to_string();
    held(&t.remote, 1).await;
    let (st, v) = match loss {
        Loss::Removed => {
            req(
                &t,
                Method::DELETE,
                &format!("/api/workspaces/{ws}/members/{alice_id}"),
                None,
                &t.alex,
            )
            .await
        }
        Loss::RoleLowered => {
            req(
                &t,
                Method::PATCH,
                &format!("/api/workspaces/{ws}/members/{alice_id}"),
                Some(json!({"role": "member"})),
                &t.alex,
            )
            .await
        }
        Loss::Disabled => {
            req(
                &t,
                Method::PATCH,
                &format!("/api/users/{alice_id}"),
                Some(json!({"disabled": true})),
                &t.alex,
            )
            .await
        }
    };
    assert_eq!(st, StatusCode::OK, "{v}");
    t.remote.answer("remote", "Hold lost");
    let why = stored_failure(&t, &id).await;
    let folder = folder_of("remote", "Hold lost");
    assert!(why.contains(&folder), "the folder is named: {why}");
    assert!(why.contains("no project names it"), "{why}");
    let projects = t.master.inner_mirror().projects(Some(&ws)).unwrap();
    assert!(
        projects.is_empty(),
        "a project was written for a starter without the right: {projects:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_starter_removed_mid_clone_gets_no_project() {
    starter_loses_right_mid_clone(Loss::Removed).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_starter_whose_role_fell_mid_clone_gets_no_project() {
    starter_loses_right_mid_clone(Loss::RoleLowered).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_starter_disabled_mid_clone_gets_no_project() {
    starter_loses_right_mid_clone(Loss::Disabled).await;
}

/// M3 and L5: a node's refusal is cleaned (control and hidden format
/// characters out) and cut to 300 characters, and the token the master sent
/// is not in it, in the node's spelling, split by a zero-width space, as an
/// HTTP Basic header, or cut in half by the 300-character limit. Nowhere
/// else either: not in a row of master.db, a log line or an answer.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_nodes_failure_reason_is_cleaned_cut_and_holds_no_token() {
    use base64::Engine as _;
    let t = start().await;
    let (alice, _) = member(&t, "alice").await;
    let conn = connect(&t, &alice, TOKEN_A).await;
    for name in ["Leak", "Leak late"] {
        let mut body = clone_body(&conn, "ada/secret", "remote");
        body["name"] = json!(name);
        let (st, v) = req(&t, Method::POST, "/api/projects", Some(body), &alice).await;
        assert_eq!(st, StatusCode::ACCEPTED, "{v}");
        let id = v["project_id"].as_str().unwrap().to_string();
        let e = failed(&t, &alice, &id, None).await;
        let why = e["reason"].as_str().unwrap();
        assert!(
            why.chars().count() <= 300,
            "{name}: {} characters",
            why.len()
        );
        assert!(
            why.chars()
                .all(|c| !c.is_control() && !matches!(c, '\u{200b}' | '\u{202e}' | '\u{2066}')),
            "{name}: control or format characters in {why:?}"
        );
        assert!(
            !why.contains("ghp_") && !why.contains("CANARY"),
            "{name}: {why}"
        );
        if name == "Leak" {
            assert!(why.contains("[REDACTED]"), "{why}");
            assert!(why.contains("Authentication failed"), "{why}");
        }
    }
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("ada:{TOKEN_A}"));
    assert_absent(&t, &[basic, TOKEN_A[8..].to_string()]);
    // The stored rows themselves, read raw.
    let c = rusqlite::Connection::open(t.dir.path().join("master.db")).unwrap();
    let reasons: Vec<String> = c
        .prepare("SELECT reason FROM project_clones")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(reasons.len(), 2);
    for r in reasons {
        assert!(!r.contains("CANARY") && !r.contains("ghp_"), "{r}");
    }
    assert_no_token_anywhere(&t);
}

/// L6: a reason that names the node's folder keeps the folder whole and
/// cuts the free text; the whole is bounded.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_orphaned_folders_reason_cuts_the_text_and_keeps_the_path() {
    let t = start().await;
    let (alice, alice_id) = member(&t, "alice").await;
    let (_bob, bob_id) = member(&t, "bob").await;
    let ws = team(&t, &alice_id, &bob_id).await;
    let conn = connect(&t, &alice, TOKEN_A).await;
    let mut body = clone_body(&conn, "ada/secret", "remote");
    body["name"] = json!("Hold orphan");
    body["workspace_id"] = json!(ws);
    let (st, v) = req(&t, Method::POST, "/api/projects", Some(body), &alice).await;
    assert_eq!(st, StatusCode::ACCEPTED, "{v}");
    let id = v["project_id"].as_str().unwrap().to_string();
    held(&t.remote, 1).await;
    // The right goes first, so that the reason is the long one.
    let (st, _) = req(
        &t,
        Method::DELETE,
        &format!("/api/workspaces/{ws}/members/{alice_id}"),
        None,
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    // A node's path of the longest length taken.
    let req_id = t.remote.held.lock().unwrap().remove(0);
    let path = format!("/{}", "p".repeat(1023));
    t.remote
        .tx
        .try_send(NodeToMaster::ProjectCreated {
            req_id,
            path: Some(path.clone()),
            error: None,
        })
        .unwrap();
    let why = stored_failure(&t, &id).await;
    assert!(why.contains(&path), "the path is whole");
    assert!(
        why.chars().count() <= 1500,
        "{} characters",
        why.chars().count()
    );
}

/// L3: clones have a budget of their own: a person at their limit of
/// running clones is still given a folder for a plain new project.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_full_clone_budget_does_not_refuse_a_plain_new_project() {
    let t = start().await;
    let (alice, _) = member(&t, "alice").await;
    let conn = connect(&t, &alice, TOKEN_A).await;
    for (i, repo) in ["ada/secret", "ada/open"].iter().enumerate() {
        let mut body = clone_body(&conn, repo, "remote");
        body["name"] = json!(format!("Hold {i}"));
        let (st, v) = req(&t, Method::POST, "/api/projects", Some(body), &alice).await;
        assert_eq!(st, StatusCode::ACCEPTED, "{v}");
    }
    held(&t.remote, 2).await;
    let (l, _other) = fake("remote2", &[CAP_PROJECTS, "forge-clone:600"]);
    t.master.add_link(l, false).await.unwrap();
    let mut third = clone_body(&conn, "ada/open", "remote2");
    third["name"] = json!("Hold third");
    let (st, v) = req(&t, Method::POST, "/api/projects", Some(third), &alice).await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS, "a third clone: {v}");
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/projects",
        Some(json!({"name": "Plain", "folder_on": "remote"})),
        &alice,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "a plain new project: {v}");
}

/// L1: a clone's one repository lookup does not wait behind, or fail for, the
/// person's repository listing (its own slot).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_listing_in_flight_does_not_refuse_the_clone() {
    let t = Arc::new(start().await);
    let (alice, _) = member(&t, "alice").await;
    let conn = connect(&t, &alice, TOKEN_A).await;
    let listing = {
        let (t, a, c) = (t.clone(), alice.clone(), conn.clone());
        tokio::spawn(async move {
            req(
                &t,
                Method::GET,
                &format!("/api/connections/{c}/repositories"),
                None,
                &a,
            )
            .await
        })
    };
    tokio::time::sleep(Duration::from_millis(400)).await;
    let at = std::time::Instant::now();
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/projects",
        Some(clone_body(&conn, "ada/open", "remote")),
        &alice,
    )
    .await;
    assert_eq!(st, StatusCode::ACCEPTED, "{v}");
    // Not queued behind the listing (still running for about a second).
    assert!(
        at.elapsed() < Duration::from_millis(700),
        "waited {:?}",
        at.elapsed()
    );
    assert_eq!(listing.await.unwrap().0, StatusCode::OK);
}

/// L2: a request refused for a limit that needs no forge call (the
/// person's rate) never shows a `cloning` row, not even for a moment: a
/// reader polling the table all the while finds none.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_request_refused_for_a_limit_never_shows_a_row() {
    let t = start().await;
    let (alice, _) = member(&t, "alice").await;
    let conn = connect(&t, &alice, TOKEN_A).await;
    // Use up the person's forge calls for this minute.
    for _ in 0..harness_master::forge_repos::LIST_PER_USER {
        let mut body = clone_body(&conn, "ada/secret", "remote");
        body["name"] = json!("Fail host");
        let (st, v) = req(&t, Method::POST, "/api/projects", Some(body), &alice).await;
        if st == StatusCode::TOO_MANY_REQUESTS {
            break;
        }
        assert_eq!(st, StatusCode::ACCEPTED, "{v}");
        failed(&t, &alice, v["project_id"].as_str().unwrap(), None).await;
    }
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let store = t.master.inner_mirror();
    let poller = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let mut seen = Vec::new();
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                for c in store.clones(None).unwrap() {
                    if c.name == "Refused for rate" {
                        seen.push(c);
                    }
                }
            }
            seen
        })
    };
    for i in 0..40 {
        let mut body = clone_body(&conn, "ada/secret", "remote");
        body["name"] = json!("Refused for rate");
        let (st, v) = req(&t, Method::POST, "/api/projects", Some(body), &alice).await;
        assert_eq!(st, StatusCode::TOO_MANY_REQUESTS, "{i}: {v}");
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let seen = poller.join().unwrap();
    assert!(
        seen.is_empty(),
        "a row was visible for a refused request: {seen:?}"
    );
}

/// L9: a double click is told it is cloning already (409), never a slot's
/// 429, however the two requests interleave: exactly one of two at once is
/// 202 and the other 409, many times over; and when the forge is slow the
/// second does not wait behind the first's lookup to be told so.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_double_click_is_always_409_and_never_waits_for_a_slot() {
    let t = Arc::new(start().await);
    let (alice, _) = member(&t, "alice").await;
    let conn = connect(&t, &alice, TOKEN_A).await;
    for round in 0..8 {
        let mut body = clone_body(&conn, "ada/open", "remote");
        body["name"] = json!(format!("Hold double {round}"));
        let both = tokio::join!(
            req(
                &t,
                Method::POST,
                "/api/projects",
                Some(body.clone()),
                &alice
            ),
            req(
                &t,
                Method::POST,
                "/api/projects",
                Some(body.clone()),
                &alice
            ),
            req(
                &t,
                Method::POST,
                "/api/projects",
                Some(body.clone()),
                &alice
            ),
        );
        let mut codes = [both.0.0, both.1.0, both.2.0];
        codes.sort();
        assert_eq!(
            codes,
            [
                StatusCode::ACCEPTED,
                StatusCode::CONFLICT,
                StatusCode::CONFLICT
            ],
            "round {round}: {both:?}"
        );
        held(&t.remote, 1).await;
        t.remote.answer("remote", &format!("Hold double {round}"));
        for _ in 0..200 {
            if pending(&t, &alice, None).await.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    // A slow forge: the lookup of the first is still running.
    let mut slow = clone_body(&conn, "ada/slow", "remote");
    slow["name"] = json!("Hold slow");
    // Two at once on a slow forge: the second is told at once, not after
    // waiting for the first's lookup (claim), nor 429 after the slot wait.
    let timed = |t: Arc<T>, a: String, b: Value| async move {
        let at = std::time::Instant::now();
        let r = req(&t, Method::POST, "/api/projects", Some(b), &a).await;
        (r.0, at.elapsed())
    };
    let (x, y) = tokio::join!(
        timed(t.clone(), alice.clone(), slow.clone()),
        timed(t.clone(), alice.clone(), slow.clone())
    );
    let (ok, dup) = if x.0 == StatusCode::ACCEPTED {
        (x, y)
    } else {
        (y, x)
    };
    assert_eq!(ok.0, StatusCode::ACCEPTED);
    assert_eq!(dup.0, StatusCode::CONFLICT);
    assert!(
        dup.1 < Duration::from_millis(1000),
        "the duplicate waited {:?}",
        dup.1
    );
    held(&t.remote, 1).await;
    t.remote.answer("remote", "Hold slow");
    for _ in 0..200 {
        if pending(&t, &alice, None).await.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let first = {
        let (t, a, b) = (t.clone(), alice.clone(), slow.clone());
        tokio::spawn(async move { req(&t, Method::POST, "/api/projects", Some(b), &a).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    let at = std::time::Instant::now();
    let (st, v) = req(&t, Method::POST, "/api/projects", Some(slow), &alice).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        at.elapsed() < Duration::from_millis(1000),
        "the second waited {:?} behind the first's lookup",
        at.elapsed()
    );
    assert_eq!(first.await.unwrap().0, StatusCode::ACCEPTED);
}
