//! A project folder's own MCP servers over HTTP (ADOPTION_PLAN X.36,
//! PLAN.md T82): `GET /api/projects/{id}/mcp`, `POST …/mcp/approve` and
//! `POST …/mcp/remove` ask the project's node with the caller's user id,
//! the project's folder and the request, nothing else; the node's answer
//! is passed on bounded. Who may: a member of the project's workspace who
//! may run agent chats on its node; never the API token, a non-member, an
//! account without a level, a project without a folder, a node that did
//! not announce the feature or is away. Approvals and removals get an
//! audit line without the entry's command or environment; a flood is
//! refused.

mod common;
use common::{Login as _, SameOrigin as _};

use std::sync::{Arc, Mutex};

use harness_core::Secret;
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, router};
use harness_node::NodeLink;
use harness_protocol::{
    CAP_PROJECT_MCP, MasterToNode, NodeToMaster, PROTOCOL_VERSION, ProjectMcpEntry, ProjectMcpList,
    ProjectMcpRequest, ProjectMcpState,
};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

type Seen = Arc<Mutex<Vec<MasterToNode>>>;

fn listed(name: &str, state: ProjectMcpState) -> ProjectMcpEntry {
    ProjectMcpEntry {
        name: name.into(),
        state,
        command: "npx".into(),
        args: vec!["-y".into(), "srv".into()],
        env_keys: vec!["TOKEN".into()],
        digest: Some("d".repeat(64)),
        reason: None,
        approved_command: None,
        approved_args: None,
        approved_env_keys: None,
    }
}

/// An agentic node `id` announcing `caps`: it answers `project.mcp` with
/// `answer` (the list it holds), after an approval with the entry approved.
fn fake(id: &str, caps: &[&str], answer: Arc<Mutex<ProjectMcpList>>) -> (NodeLink, Seen) {
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
    let seen: Seen = Arc::default();
    let keep = seen.clone();
    tokio::spawn(async move {
        while let Some(msg) = commands.recv().await {
            keep.lock().unwrap().push(msg.clone());
            if let MasterToNode::ProjectMcp {
                req_id, request, ..
            } = &msg
            {
                let mut l = answer.lock().unwrap().clone();
                if let ProjectMcpRequest::Approve { name, .. } = request {
                    for e in &mut l.entries {
                        if &e.name == name {
                            e.state = ProjectMcpState::Approved;
                        }
                    }
                }
                let _ = tx.try_send(NodeToMaster::ProjectMcpResult {
                    req_id: req_id.clone(),
                    list: Some(Box::new(l)),
                    error: None,
                });
            }
            if let Some(r) = msg.req_id() {
                let _ = tx.try_send(NodeToMaster::Ack {
                    req_id: r.into(),
                    ok: true,
                    error: None,
                });
            }
        }
    });
    (NodeLink { to_node, from_node }, seen)
}

struct T {
    base: String,
    store: Store,
    alex: String,
    remote: Seen,
    answer: Arc<Mutex<ProjectMcpList>>,
    _dir: tempfile::TempDir,
}

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
    let store = Store::open_in_memory().unwrap();
    let master = Master::new(store.clone(), auth);
    let answer = Arc::new(Mutex::new(ProjectMcpList {
        entries: vec![listed("fs", ProjectMcpState::New)],
        ..Default::default()
    }));
    let (l, remote) = fake("remote", &[CAP_PROJECT_MCP], answer.clone());
    master.add_link(l, false).await.unwrap();
    let (l, _) = fake("plain", &[], answer.clone());
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
        store,
        alex,
        remote,
        answer,
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

/// A project of `who` (in their Personal workspace) with a folder on
/// `node` (none: no folder).
async fn project(t: &T, who: &str, node: Option<&str>) -> String {
    let (st, v) = req(
        t,
        Method::POST,
        "/api/projects",
        Some(json!({"name": "P"})),
        who,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let pid = v["project_id"].as_str().unwrap().to_string();
    if let Some(node) = node {
        let (st, v) = req(
            t,
            Method::PATCH,
            &format!("/api/projects/{pid}"),
            Some(json!({"settings": {"node_id": node, "cwd": "/home/remote/repo"}})),
            who,
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }
    pid
}

fn asked(seen: &Seen) -> Vec<(String, String, ProjectMcpRequest)> {
    seen.lock()
        .unwrap()
        .iter()
        .filter_map(|m| match m {
            MasterToNode::ProjectMcp {
                user_id,
                folder,
                request,
                ..
            } => Some((user_id.clone(), folder.clone(), request.clone())),
            _ => None,
        })
        .collect()
}

async fn audit(t: &T, kind: &str) -> Vec<String> {
    t.store
        .audit_lines(kind)
        .unwrap()
        .into_iter()
        .map(|(_, d)| d)
        .collect()
}

#[tokio::test]
async fn the_node_is_asked_with_the_callers_id_and_the_projects_folder() {
    let t = start().await;
    let (_, me) = req(&t, Method::GET, "/api/me", None, &t.alex).await;
    let alex_id = me["user"]["user_id"].as_str().unwrap().to_string();
    let pid = project(&t, &t.alex, Some("remote")).await;
    let (st, v) = req(
        &t,
        Method::GET,
        &format!("/api/projects/{pid}/mcp"),
        None,
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["node_id"], "remote");
    assert_eq!(v["entries"][0]["name"], "fs");
    assert_eq!(v["entries"][0]["env_keys"], json!(["TOKEN"]));
    let (st, v) = req(
        &t,
        Method::POST,
        &format!("/api/projects/{pid}/mcp/approve"),
        Some(json!({"name": "fs", "digest": "d".repeat(64)})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["entries"][0]["state"], "approved");
    let (st, _) = req(
        &t,
        Method::POST,
        &format!("/api/projects/{pid}/mcp/remove"),
        Some(json!({"name": "fs"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let got = asked(&t.remote);
    assert_eq!(got.len(), 3, "{got:?}");
    for (user, folder, _) in &got {
        assert_eq!(user, &alex_id);
        assert_eq!(folder, "/home/remote/repo");
    }
    assert_eq!(got[0].2, ProjectMcpRequest::List);
    assert_eq!(
        got[1].2,
        ProjectMcpRequest::Approve {
            name: "fs".into(),
            digest: "d".repeat(64)
        }
    );
    // An audit line each, naming the server and never the command.
    let a = audit(&t, "project_mcp.approve").await;
    assert_eq!(a.len(), 1, "{a:?}");
    assert!(
        a[0].contains("\"server\":\"fs\"") && a[0].contains("approved"),
        "{a:?}"
    );
    assert!(!a[0].contains("npx") && !a[0].contains("TOKEN"), "{a:?}");
    assert_eq!(audit(&t, "project_mcp.remove").await.len(), 1);
    // Bodies of the wrong shape never reach the node.
    for body in [
        json!({"name": "Fs", "digest": "d".repeat(64)}),
        json!({"name": "fs", "digest": "short"}),
        json!({"name": "fs", "digest": "d".repeat(64), "command": "sh"}),
    ] {
        let (st, _) = req(
            &t,
            Method::POST,
            &format!("/api/projects/{pid}/mcp/approve"),
            Some(body.clone()),
            &t.alex,
        )
        .await;
        assert!(st.is_client_error(), "{body}: {st}");
    }
    assert_eq!(asked(&t.remote).len(), 3);
}

#[tokio::test]
async fn only_a_member_who_may_run_agent_chats_there_may_ask() {
    let t = start().await;
    let bob = account(&t, "bob", "member", "host").await;
    let dana = account(&t, "dana", "member", "none").await;
    let pid = project(&t, &t.alex, Some("remote")).await;
    let path = format!("/api/projects/{pid}/mcp");
    // Bob is not a member of alex's workspace.
    let (st, _) = req(&t, Method::GET, &path, None, &bob).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // The API token has no approvals.
    let r = reqwest::Client::new()
        .get(format!("{}{path}", t.base))
        .bearer_auth("api-secret")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    // Dana may run no agent chat, also in her own project.
    let dp = project(&t, &dana, Some("remote")).await;
    let (st, v) = req(
        &t,
        Method::GET,
        &format!("/api/projects/{dp}/mcp"),
        None,
        &dana,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    // No folder; a node without the feature; a node that is gone.
    let none = project(&t, &t.alex, None).await;
    let (st, v) = req(
        &t,
        Method::GET,
        &format!("/api/projects/{none}/mcp"),
        None,
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    let plain = project(&t, &t.alex, Some("plain")).await;
    let (st, v) = req(
        &t,
        Method::GET,
        &format!("/api/projects/{plain}/mcp"),
        None,
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v.to_string().contains("project_servers"), "{v}");
    let gone = project(&t, &t.alex, Some("ghost")).await;
    let (st, v) = req(
        &t,
        Method::GET,
        &format!("/api/projects/{gone}/mcp"),
        None,
        &t.alex,
    )
    .await;
    assert!(
        st == StatusCode::NOT_FOUND || st == StatusCode::SERVICE_UNAVAILABLE,
        "{st} {v}"
    );
    assert!(asked(&t.remote).is_empty(), "{:?}", asked(&t.remote));
}

/// What a node answers is passed on bounded and as one-line text, whatever
/// it sent; a flood of requests from one account is refused.
#[tokio::test]
async fn answers_are_bounded_and_floods_refused() {
    let t = start().await;
    {
        let mut a = t.answer.lock().unwrap();
        let mut hostile = listed("x", ProjectMcpState::Unsupported);
        hostile.command = format!("evil\n\u{1b}[2J{}", "c".repeat(10_000));
        hostile.args = vec!["a".into(); 500];
        a.entries = vec![hostile; 300];
    }
    let pid = project(&t, &t.alex, Some("remote")).await;
    let path = format!("/api/projects/{pid}/mcp");
    let (st, v) = req(&t, Method::GET, &path, None, &t.alex).await;
    assert_eq!(st, StatusCode::OK);
    let entries = v["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 64);
    let cmd = entries[0]["command"].as_str().unwrap();
    assert!(
        cmd.chars().count() <= 2048 && !cmd.chars().any(char::is_control),
        "{cmd:?}"
    );
    assert_eq!(entries[0]["args"].as_array().unwrap().len(), 64);
    assert!(
        entries[0].get("digest").is_none(),
        "an unsupported entry's digest"
    );
    let mut refused = false;
    for _ in 0..40 {
        let (st, _) = req(&t, Method::GET, &path, None, &t.alex).await;
        if st == StatusCode::TOO_MANY_REQUESTS {
            refused = true;
            break;
        }
        assert_eq!(st, StatusCode::OK);
    }
    assert!(refused, "40 requests in a row were all sent to the node");
    assert!(asked(&t.remote).len() <= harness_master::project_mcp::PER_MINUTE);
}
