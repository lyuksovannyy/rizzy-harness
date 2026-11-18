//! `@` file mentions over HTTP (PLAN.md T63): the completion route and a
//! prompt's `files` are the chat owner's only (every other account gets
//! 404, admins and workspace admins included, T10); the master relays
//! names to the owning node and keeps nothing; an old node is asked
//! nothing; a chat without a folder offers no files.

mod common;
use common::{Login as _, SameOrigin as _};

use std::sync::{Arc, Mutex};

use harness_core::event::{EventKind, Profile, SessionCreated};
use harness_core::policy::ApprovalMode;
use harness_core::{RawEvent, Secret, SessionId};
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, router};
use harness_node::NodeLink;
use harness_protocol::{CAP_FILES, MasterToNode, NodeToMaster, PROTOCOL_VERSION};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

const TOKEN: &str = "api-secret";

struct Fake {
    seen: Arc<Mutex<Vec<MasterToNode>>>,
    _leave: mpsc::UnboundedSender<()>,
}

impl Fake {
    /// The queries of every `session.files` this node was sent.
    fn queries(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                MasterToNode::Files { query, .. } => Some(query.clone()),
                _ => None,
            })
            .collect()
    }

    /// The `files` of every prompt this node was sent.
    fn prompted(&self) -> Vec<Vec<String>> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                MasterToNode::Prompt { files, .. } => Some(files.clone()),
                _ => None,
            })
            .collect()
    }
}

/// An agentic node `id` announcing `caps`: it opens chats, acks every
/// command, and answers every `session.files` with `src/main.rs`.
fn fake(id: &str, caps: &[&str]) -> (NodeLink, Fake) {
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
    let seen: Arc<Mutex<Vec<MasterToNode>>> = Arc::default();
    let (leave, mut left) = mpsc::unbounded_channel::<()>();
    let (node, keep) = (id.to_string(), seen.clone());
    tokio::spawn(async move {
        loop {
            tokio::select! {
                msg = commands.recv() => {
                    let Some(msg) = msg else { return };
                    keep.lock().unwrap().push(msg.clone());
                    match &msg {
                        MasterToNode::Create { session_id, exec_mode, .. } => {
                            let (kind, payload) = EventKind::SessionCreated(SessionCreated {
                                exec_mode: *exec_mode,
                                no_callbacks: false,
                                title: "t".into(),
                                cwd: "/w".into(),
                                model: "demo::mock".into(),
                                backend: "native".into(),
                                node_id: node.clone(),
                                approval_mode: ApprovalMode::Ask,
                                profile: Profile::Agentic,
                                agent_id: None,
                                incognito: false,
                                parent_session: None,
                                owner_id: None,
                            })
                            .into_parts();
                            let _ = tx.try_send(NodeToMaster::Event {
                                event: RawEvent {
                                    session_id: *session_id,
                                    seq: 1,
                                    ts_ms: harness_core::now_ms(),
                                    kind,
                                    payload,
                                },
                            });
                        }
                        MasterToNode::Files { req_id, .. } => {
                            let _ = tx.try_send(NodeToMaster::FilesResult {
                                req_id: req_id.clone(),
                                paths: vec!["src/main.rs".into()],
                                error: None,
                            });
                        }
                        _ => {}
                    }
                    if let Some(req_id) = msg.req_id() {
                        let _ = tx.try_send(NodeToMaster::Ack { req_id: req_id.into(), ok: true, error: None });
                    }
                }
                _ = left.recv() => return,
            }
        }
    });
    (
        NodeLink { to_node, from_node },
        Fake {
            seen,
            _leave: leave,
        },
    )
}

struct T {
    base: String,
    _master: Master,
    alex: String,
    local: Fake,
    old: Fake,
    _dir: tempfile::TempDir,
}

/// A master with the fake node `local` (announcing `files`) and `old` (a
/// node from before T63), and alex, its claimed admin.
async fn start() -> T {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, "CLAIM").unwrap();
    let auth =
        Auth::new(Secret::new(TOKEN.into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim_path,
        });
    let master = Master::new(Store::open_in_memory().unwrap(), auth);
    let (l, local) = fake("local", &[CAP_FILES]);
    master.add_link(l, true).await.unwrap();
    let (l, old) = fake("old", &[]);
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
    assert_eq!(r.status(), StatusCode::OK);
    let alex = common::signed_in(r).await;
    T {
        base,
        _master: master,
        alex,
        local,
        old,
        _dir: dir,
    }
}

async fn req(
    t: &T,
    method: Method,
    path: &str,
    body: Option<Value>,
    c: &str,
) -> (StatusCode, Value) {
    let mut rb = reqwest::Client::new().request(method, format!("{}{path}", t.base));
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    rb = match c {
        TOKEN => rb.bearer_auth(TOKEN),
        c => rb.login(c).same_origin(),
    };
    let r = rb.send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

/// An account with its own password, logged in (its cookie).
async fn account(t: &T, name: &str, role: &str) -> String {
    let (st, v) = req(
        t,
        Method::POST,
        "/api/users",
        Some(json!({"username": name, "role": role, "exec_ceiling": "host"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
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
    common::signed_in(r).await
}

async fn chat(t: &T, who: &str, node: &str, workspace: Option<&str>) -> SessionId {
    let mut body = json!({"cwd": "/w", "title": "t", "node_id": node});
    if let Some(w) = workspace {
        body["workspace_id"] = json!(w);
    }
    let (st, v) = req(t, Method::POST, "/api/sessions", Some(body), who).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["session_id"].as_str().unwrap().parse().unwrap()
}

fn files(sid: SessionId, q: &str) -> String {
    format!("/api/sessions/{sid}/files?q={q}")
}

fn prompt(sid: SessionId) -> String {
    format!("/api/sessions/{sid}/prompt")
}

#[tokio::test]
async fn the_owner_lists_and_mentions_files_through_the_node() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    let (st, v) = req(&t, Method::GET, &files(sid, "main"), None, &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v, json!({"paths": ["src/main.rs"]}));
    assert_eq!(t.local.queries(), ["main"]);
    let body = json!({"text": "fix @src/main.rs", "files": ["src/main.rs", "../../etc/passwd"]});
    let (st, v) = req(&t, Method::POST, &prompt(sid), Some(body), &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // Names only, as typed: the node decides what each may show.
    assert_eq!(
        t.local.prompted(),
        [vec![
            "src/main.rs".to_string(),
            "../../etc/passwd".to_string()
        ]]
    );
}

#[tokio::test]
async fn nobody_else_lists_or_mentions_not_even_an_admin() {
    let t = start().await;
    let bob = account(&t, "bob", "member").await;
    let carol = account(&t, "carol", "admin").await;
    let dana = account(&t, "dana", "member").await;
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "team"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let team = v["workspace_id"].as_str().unwrap().to_string();
    for who in ["bob", "dana"] {
        let role = if who == "dana" { "admin" } else { "member" };
        let (st, v) = req(
            &t,
            Method::POST,
            &format!("/api/workspaces/{team}/members"),
            Some(json!({"username": who, "role": role})),
            &t.alex,
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }
    let sid = chat(&t, &t.alex, "local", Some(&team)).await;
    for (who, name) in [
        (&bob, "a member"),
        (&carol, "a server admin"),
        (&dana, "a workspace admin"),
    ] {
        let (st, v) = req(&t, Method::GET, &files(sid, "main"), None, who).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{name}: {v}");
        assert_eq!(v["error"], "unknown session", "{name}");
        let body = json!({"text": "x", "files": ["src/main.rs"]});
        let (st, v) = req(&t, Method::POST, &prompt(sid), Some(body), who).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{name}: {v}");
    }
    assert!(t.local.queries().is_empty());
    assert!(t.local.prompted().is_empty());
}

#[tokio::test]
async fn an_old_node_is_asked_nothing_and_bad_mentions_are_refused() {
    let t = start().await;
    let old = chat(&t, &t.alex, "old", None).await;
    let (st, v) = req(&t, Method::GET, &files(old, "main"), None, &t.alex).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    let body = json!({"text": "x", "files": ["a.rs"]});
    let (st, v) = req(&t, Method::POST, &prompt(old), Some(body), &t.alex).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("older"), "{v}");
    assert!(t.old.prompted().is_empty() && t.old.queries().is_empty());
    let sid = chat(&t, &t.alex, "local", None).await;
    let many: Vec<String> = (0..65).map(|i| format!("f{i}.rs")).collect();
    let (st, _) = req(
        &t,
        Method::POST,
        &prompt(sid),
        Some(json!({"text": "x", "files": many})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let long = "a".repeat(1025);
    let (st, _) = req(
        &t,
        Method::POST,
        &prompt(sid),
        Some(json!({"text": "x", "files": [long]})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let reply = json!({"message_id": "m", "block_id": "b", "values": {}});
    let (st, _) = req(
        &t,
        Method::POST,
        &prompt(sid),
        Some(json!({"text": "", "reply": reply, "files": ["a.rs"]})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert!(t.local.prompted().is_empty());
}
