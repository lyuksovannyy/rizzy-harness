//! A new project's folder over HTTP (ADOPTION_PLAN X.26, PLAN.md T77):
//! `POST /api/projects` with `folder_on` asks that node for a folder made
//! from the project's name (nothing else: no path), and the project exists
//! only once the folder does; a node that refuses, did not offer it or is
//! away leaves no project. `POST /api/projects/{id}/folder` gives a project
//! without one a folder. Who may: those who may change the workspace's
//! projects, and run agent chats on that node; nobody else, an admin who
//! is not a member included.

mod common;
use common::{Login as _, SameOrigin as _};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use harness_core::RawEvent;
use harness_core::Secret;
use harness_core::event::{EventKind, Profile, SessionCreated};
use harness_core::policy::ApprovalMode;
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, router};
use harness_node::NodeLink;
use harness_protocol::{CAP_PROJECTS, MasterToNode, NodeToMaster, PROTOCOL_VERSION};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

struct Fake {
    seen: Arc<Mutex<Vec<MasterToNode>>>,
    refuse: Arc<AtomicBool>,
    leave: Option<mpsc::UnboundedSender<()>>,
}

impl Fake {
    fn names(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                MasterToNode::ProjectCreate { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect()
    }
    fn cwds(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                MasterToNode::Create { cwd, .. } => Some(cwd.clone()),
                _ => None,
            })
            .collect()
    }
}

/// The folder this fake node "makes" for `name`.
fn folder_of(node: &str, name: &str) -> String {
    format!(
        "/home/{node}/rizzy-projects/{}-ab1c",
        name.to_lowercase().replace(' ', "-")
    )
}

/// An agentic node `id` announcing `caps`: it opens chats, acks every
/// command, and answers `project.create` with [`folder_of`] (or a refusal
/// while `refuse` is set).
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
    let refuse = Arc::new(AtomicBool::new(false));
    let (leave, mut left) = mpsc::unbounded_channel::<()>();
    let (node, keep, no) = (id.to_string(), seen.clone(), refuse.clone());
    tokio::spawn(async move {
        loop {
            tokio::select! {
                msg = commands.recv() => {
                    let Some(msg) = msg else { return };
                    keep.lock().unwrap().push(msg.clone());
                    match &msg {
                        MasterToNode::Create { session_id, exec_mode, cwd, .. } => {
                            let (kind, payload) = EventKind::SessionCreated(SessionCreated {
                                exec_mode: *exec_mode,
                                no_callbacks: false,
                                title: "t".into(),
                                cwd: cwd.clone(),
                                model: "demo::mock".into(),
                                backend: "native".into(),
                                node_id: node.clone(),
                                approval_mode: ApprovalMode::Ask,
                                profile: Profile::Agentic,
                                agent_id: None,
                                incognito: false,
                                parent_session: None,
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
                        MasterToNode::ProjectCreate { req_id, name } => {
                            let _ = tx.try_send(if no.load(Ordering::SeqCst) {
                                NodeToMaster::ProjectCreated {
                                    req_id: req_id.clone(),
                                    path: None,
                                    error: Some("this node has made 20 project folders in the last hour".into()),
                                }
                            } else {
                                NodeToMaster::ProjectCreated {
                                    req_id: req_id.clone(),
                                    path: Some(folder_of(&node, name)),
                                    error: None,
                                }
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
            refuse,
            leave: Some(leave),
        },
    )
}

struct T {
    base: String,
    master: Master,
    alex: String,
    local: Fake,
    remote: Fake,
    old: Fake,
    _dir: tempfile::TempDir,
}

/// A master with fake nodes `local` (embedded) and `remote` making project
/// folders, and `old` that does not; alex is its claimed admin.
async fn start() -> T {
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
    let (l, local) = fake("local", &[CAP_PROJECTS]);
    master.add_link(l, true).await.unwrap();
    let (l, remote) = fake("remote", &[CAP_PROJECTS]);
    master.add_link(l, false).await.unwrap();
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
        master,
        alex,
        local,
        remote,
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
    let r = rb.login(c).same_origin().send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

/// An account with its own password, logged in (its cookie).
async fn account(t: &T, name: &str, role: &str, ceiling: &str) -> String {
    let (st, v) = req(
        t,
        Method::POST,
        "/api/users",
        Some(json!({"username": name, "role": role, "exec_ceiling": ceiling})),
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

async fn projects(t: &T, who: &str, ws: Option<&str>) -> Vec<Value> {
    let q = ws.map(|w| format!("?workspace={w}")).unwrap_or_default();
    let (st, v) = req(t, Method::GET, &format!("/api/projects{q}"), None, who).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["projects"].as_array().unwrap().clone()
}

/// The new project's settings name the node and the folder it answered
/// with; the node was sent the name and nothing else; a chat started in
/// the project without a folder goes to that folder on that node.
#[tokio::test]
async fn a_new_project_gets_its_folder_from_the_node() {
    let t = start().await;
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/projects",
        Some(json!({"name": "Project Name Example", "icon": "code", "folder_on": "local"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let path = folder_of("local", "Project Name Example");
    assert_eq!(v["settings"]["node_id"], "local");
    assert_eq!(v["settings"]["cwd"], path.as_str());
    assert_eq!(t.local.names(), ["Project Name Example"]);
    assert!(t.remote.names().is_empty());
    // The list shows each project's node and folder (the popup's rows).
    let list = projects(&t, &t.alex, None).await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["cwd"], path.as_str());
    assert_eq!(list[0]["node_id"], "local");
    let pid = v["project_id"].as_str().unwrap();
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/sessions",
        Some(json!({"title": "t", "project_id": pid})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(t.local.cwds(), [path]);
}

/// The order: the folder first. A node that refuses, did not offer it, is
/// unknown, or is away: no project, and the reason in words.
#[tokio::test]
async fn no_folder_no_project() {
    let mut t = start().await;
    t.local.refuse.store(true, Ordering::SeqCst);
    let mk = |node: &str| json!({"name": "Half", "folder_on": node});
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/projects",
        Some(mk("local")),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("last hour"), "{v}");
    let (st, v) = req(&t, Method::POST, "/api/projects", Some(mk("old")), &t.alex).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"]
            .as_str()
            .unwrap()
            .contains("makes no project folders"),
        "{v}"
    );
    assert!(t.old.names().is_empty(), "an old node is asked nothing");
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/projects",
        Some(mk("nowhere")),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{v}");
    // Away: the link closes.
    drop(t.remote.leave.take());
    for _ in 0..100 {
        if !t.master.is_connected("remote") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/projects",
        Some(mk("remote")),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert!(
        projects(&t, &t.alex, None).await.is_empty(),
        "a half-made project"
    );
}

/// Who may: a member whose role cannot change projects gets what it gets
/// today (403) and the node is asked nothing; an account that may not use
/// the embedded node, or may run no agent chat, is refused before the
/// node is asked; a server admin who is not a member of the workspace
/// sees no project of it (404) and cannot give it a folder.
#[tokio::test]
async fn only_those_who_may_change_projects_and_use_the_node() {
    let t = start().await;
    let bob = account(&t, "bob", "member", "host").await;
    let carol = account(&t, "carol", "admin", "host").await;
    let dana = account(&t, "dana", "member", "none").await;
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
    let (st, v) = req(
        &t,
        Method::POST,
        &format!("/api/workspaces/{team}/members"),
        Some(json!({"username": "bob", "role": "member"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // Bob, a member of team: may not change its projects.
    let body = json!({"name": "B", "folder_on": "remote", "workspace_id": team});
    let (st, v) = req(&t, Method::POST, "/api/projects", Some(body), &bob).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    // In his Personal workspace he may, but not on the embedded node
    // (admins only by default); on the remote node he may.
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/projects",
        Some(json!({"name": "B", "folder_on": "local"})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert!(t.local.names().is_empty());
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/projects",
        Some(json!({"name": "Bobs", "folder_on": "remote"})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(t.remote.names(), ["Bobs"]);
    // Dana may run no agent chat: no folder made for her.
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/projects",
        Some(json!({"name": "D", "folder_on": "remote"})),
        &dana,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(t.remote.names(), ["Bobs"]);
    // Alex's team project without a folder: carol (a server admin, not a
    // member) cannot see it or give it one; bob cannot change it.
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/projects",
        Some(json!({"name": "Team", "workspace_id": team})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let pid = v["project_id"].as_str().unwrap().to_string();
    let give = format!("/api/projects/{pid}/folder");
    let (st, _) = req(
        &t,
        Method::POST,
        &give,
        Some(json!({"node_id": "remote"})),
        &carol,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _) = req(
        &t,
        Method::POST,
        &give,
        Some(json!({"node_id": "remote"})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    assert_eq!(t.remote.names(), ["Bobs"]);
}

/// "Create its folder on <node>": a project without one gets one; a
/// second time is refused (one folder per project).
#[tokio::test]
async fn a_project_without_a_folder_gets_one_once() {
    let t = start().await;
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/projects",
        Some(json!({"name": "Late"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["settings"]["cwd"], Value::Null);
    let give = format!("/api/projects/{}/folder", v["project_id"].as_str().unwrap());
    let (st, v) = req(
        &t,
        Method::POST,
        &give,
        Some(json!({"node_id": "local"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["settings"]["cwd"], folder_of("local", "Late").as_str());
    assert_eq!(v["settings"]["node_id"], "local");
    let (st, v) = req(
        &t,
        Method::POST,
        &give,
        Some(json!({"node_id": "remote"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(t.remote.names().is_empty());
    // A path field is not part of the request.
    let (st, _) = req(
        &t,
        Method::POST,
        "/api/projects",
        Some(json!({"name": "P", "folder_on": "local", "cwd": "/etc"})),
        &t.alex,
    )
    .await;
    assert!(st.is_client_error(), "{st}");
}
