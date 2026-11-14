//! Who may run agentic sessions on the embedded `local` node (FIX_PLAN
//! 2.1, decision Q7): by default server admins and the API token only,
//! since that node shares the master's OS user and data directory. Other
//! accounts with the agentic grant use a remote node. `[nodes]
//! local_node_users = "granted"` lets everyone with the grant in.

mod common;
use common::{Login as _, SameOrigin as _};

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::Secret;
use harness_core::event::Profile;
use harness_master::auth::{Auth, Claim};
use harness_master::config::{Config, LocalNodeUsers};
use harness_master::providers::ProviderRuntime;
use harness_master::{Master, router};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

const TOKEN: &str = "api-secret";
const REMOTE: &str = "node-remote1";

struct Server {
    base: String,
    ws: std::path::PathBuf,
    admin: String,
    rt: Arc<ProviderRuntime>,
    _dir: tempfile::TempDir,
}

async fn node(root: &std::path::Path, id: &str, profile: Profile) -> NodeRuntime {
    NodeRuntime::start(NodeConfig {
        node_id: id.into(),
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
            match profile {
                Profile::Agentic => harness_tools::builtin(),
                Profile::Chat => Vec::new(),
            },
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    })
    .await
    .unwrap()
}

/// The embedded agentic node `local`, the chat runtime, a remote worker
/// (`with_remote`), and a claimed admin.
async fn start(users: LocalNodeUsers, with_remote: bool) -> Server {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let mut cfg = Config::default();
    cfg.nodes.local_node_users = users;
    let (rt, _) = ProviderRuntime::start(cfg, dir.path(), Default::default()).unwrap();
    let rt = Arc::new(rt);
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
    master.set_provider_runtime(rt.clone());
    let local = node(dir.path(), "local", Profile::Agentic).await;
    let chats = node(dir.path(), "master", Profile::Chat).await;
    master.add_link(local.attach().await, true).await.unwrap();
    master.add_link(chats.attach().await, true).await.unwrap();
    if with_remote {
        let remote = node(dir.path(), REMOTE, Profile::Agentic).await;
        master.add_link(remote.attach().await, false).await.unwrap();
    }
    tokio::spawn(async move { axum::serve(listener, router(master)).await.unwrap() });
    let mut s = Server {
        base: format!("http://{addr}"),
        ws,
        admin: String::new(),
        rt,
        _dir: dir,
    };
    let (st, cookie, v) = send(
        &s,
        Method::POST,
        "/api/setup",
        Some(json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"})),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    s.admin = cookie.unwrap();
    s
}

async fn send(
    s: &Server,
    method: Method,
    path: &str,
    body: Option<Value>,
    auth: Option<&str>,
) -> (StatusCode, Option<String>, Value) {
    let mut rb = reqwest::Client::new().request(method, format!("{}{path}", s.base));
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    match auth {
        Some(TOKEN) => rb = rb.bearer_auth(TOKEN),
        Some(c) => rb = rb.login(c).same_origin(),
        None => {}
    }
    let r = rb.send().await.unwrap();
    let status = r.status();
    let cookie = r
        .headers()
        .get("set-cookie")
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
    let body = r.json().await.unwrap_or(Value::Null);
    if let Some(c) = &cookie {
        common::remember(c, &body);
    }
    (status, cookie, body)
}

async fn call(
    s: &Server,
    method: Method,
    path: &str,
    body: Option<Value>,
    auth: &str,
) -> (StatusCode, Value) {
    let (st, _, v) = send(s, method, path, body, Some(auth)).await;
    (st, v)
}

/// A non-admin account with the agentic grant, logged in. (cookie, id)
async fn member(s: &Server, name: &str) -> (String, String) {
    let (st, v) = call(
        s,
        Method::POST,
        "/api/users",
        Some(json!({"username": name, "exec_ceiling": "host"})),
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let pw = v["password"].as_str().unwrap().to_string();
    let id = v["user_id"].as_str().unwrap().to_string();
    let (_, temp, _) = send(
        s,
        Method::POST,
        "/api/login",
        Some(json!({"username": name, "password": pw})),
        None,
    )
    .await;
    let (st, cookie, v) = send(
        s,
        Method::POST,
        "/api/me/password",
        Some(json!({"current": pw, "new": "a password of my own"})),
        temp.as_deref(),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    (cookie.unwrap(), id)
}

async fn agentic(s: &Server, who: &str, node: Option<&str>) -> (StatusCode, Value) {
    let mut body = json!({"cwd": s.ws.display().to_string(), "title": "t"});
    if let Some(n) = node {
        body["node_id"] = json!(n);
    }
    call(s, Method::POST, "/api/sessions", Some(body), who).await
}

async fn node_of(s: &Server, who: &str, sid: &str) -> String {
    let (st, v) = call(s, Method::GET, &format!("/api/sessions/{sid}"), None, who).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["node_id"].as_str().unwrap().to_string()
}

async fn prompt(s: &Server, who: &str, sid: &str, text: &str) -> (StatusCode, Value) {
    call(
        s,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({ "text": text })),
        who,
    )
    .await
}

async fn wait_idle(s: &Server, who: &str, sid: &str) {
    for _ in 0..200 {
        let (_, v) = call(s, Method::GET, &format!("/api/sessions/{sid}"), None, who).await;
        if v["running"] == false {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("session {sid} kept running");
}

fn agentic_nodes(v: &Value) -> Vec<String> {
    v["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["profile"] == "agentic")
        .map(|n| n["node_id"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn by_default_only_admins_use_the_local_node() {
    let s = start(LocalNodeUsers::default(), true).await;
    let (bob, _) = member(&s, "bob").await;

    // Named explicitly: refused, with what to do instead.
    let (st, v) = agentic(&s, &bob, Some("local")).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    let msg = v.to_string();
    assert!(msg.contains("remote node"), "{msg}");
    assert!(msg.contains("admin"), "{msg}");

    // Unnamed: goes to a node they may use, never to the local one.
    let (st, v) = agentic(&s, &bob, None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap();
    assert_eq!(node_of(&s, &bob, sid).await, REMOTE);
    // And it works there.
    let (st, v) = prompt(&s, &bob, sid, "hello").await;
    assert_eq!(st, StatusCode::OK, "{v}");

    // Admins and the API token still may.
    for who in [s.admin.as_str(), TOKEN] {
        let (st, v) = agentic(&s, who, Some("local")).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        let sid = v["session_id"].as_str().unwrap();
        let (st, v) = prompt(&s, who, sid, "hello").await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }

    // Chats are unaffected.
    let (st, v) = call(
        &s,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat", "title": "c"})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap();
    let (st, v) = prompt(&s, &bob, sid, "hello").await;
    assert_eq!(st, StatusCode::OK, "{v}");

    // The node picker does not offer it to them.
    let (_, v) = call(&s, Method::GET, "/api/nodes", None, &bob).await;
    assert_eq!(agentic_nodes(&v), vec![REMOTE.to_string()], "{v}");
    let (_, v) = call(&s, Method::GET, "/api/nodes", None, &s.admin).await;
    assert!(agentic_nodes(&v).contains(&"local".to_string()), "{v}");
}

#[tokio::test]
async fn without_a_remote_node_a_member_is_told_what_to_do() {
    let s = start(LocalNodeUsers::default(), false).await;
    let (bob, _) = member(&s, "bob").await;
    let (st, v) = agentic(&s, &bob, None).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert!(v.to_string().contains("remote node"), "{v}");
}

#[tokio::test]
async fn granted_lets_every_account_with_the_grant_in() {
    let s = start(LocalNodeUsers::Granted, true).await;
    let (bob, _) = member(&s, "bob").await;
    let (st, v) = agentic(&s, &bob, Some("local")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap();
    let (st, v) = prompt(&s, &bob, sid, "hello").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, v) = call(&s, Method::GET, "/api/nodes", None, &bob).await;
    assert!(agentic_nodes(&v).contains(&"local".to_string()), "{v}");
}

/// The rule is checked on every action that makes the chat work, so a chat
/// made while it was looser does no more work once it applies; stopping
/// work stays possible.
#[tokio::test]
async fn existing_local_chats_follow_the_setting() {
    let s = start(LocalNodeUsers::Granted, true).await;
    let (bob, _) = member(&s, "bob").await;
    let (st, v) = agentic(&s, &bob, Some("local")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap().to_string();
    let (st, v) = prompt(&s, &bob, &sid, "hello").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    wait_idle(&s, &bob, &sid).await;

    let mut cfg = s.rt.config();
    cfg.nodes.local_node_users = LocalNodeUsers::Admins;
    s.rt.save(cfg).unwrap();

    let (st, v) = prompt(&s, &bob, &sid, "again").await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert!(v.to_string().contains("remote node"), "{v}");
    let (st, v) = call(
        &s,
        Method::POST,
        &format!("/api/sessions/{sid}/turns/x/restore"),
        Some(json!({})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    let (st, v) = call(
        &s,
        Method::POST,
        &format!("/api/sessions/{sid}/approvals/a1"),
        Some(json!({"decision": "allow"})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    // The chat says why it is read-only.
    let (_, v) = call(&s, Method::GET, &format!("/api/sessions/{sid}"), None, &bob).await;
    assert!(
        v["blocked"]
            .as_str()
            .is_some_and(|b| b.contains("remote node")),
        "{v}"
    );
    // Stopping and denying are never refused for this reason.
    let (st, v) = call(
        &s,
        Method::POST,
        &format!("/api/sessions/{sid}/cancel"),
        None,
        &bob,
    )
    .await;
    assert_ne!(st, StatusCode::FORBIDDEN, "{v}");
    let (st, v) = call(
        &s,
        Method::POST,
        &format!("/api/sessions/{sid}/approvals/a1"),
        Some(json!({"decision": "deny"})),
        &bob,
    )
    .await;
    assert_ne!(st, StatusCode::FORBIDDEN, "{v}");
}

/// An admin who loses the role loses the local node at once: their running
/// turn there stops, as when the agentic grant is taken away.
#[tokio::test]
async fn demoted_admins_running_local_turns_stop() {
    let s = start(LocalNodeUsers::default(), true).await;
    let (carol, carol_id) = member(&s, "carol").await;
    let (st, v) = call(
        &s,
        Method::PATCH,
        &format!("/api/users/{carol_id}"),
        Some(json!({"role": "admin"})),
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = agentic(&s, &carol, Some("local")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap().to_string();
    let (st, v) = prompt(&s, &carol, &sid, "slow: 20000").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    for _ in 0..200 {
        let (_, v) = call(
            &s,
            Method::GET,
            &format!("/api/sessions/{sid}"),
            None,
            &carol,
        )
        .await;
        if v["running"] == true {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let (st, v) = call(
        &s,
        Method::PATCH,
        &format!("/api/users/{carol_id}"),
        Some(json!({"role": "member"})),
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    wait_idle(&s, &carol, &sid).await;
    let (st, _) = prompt(&s, &carol, &sid, "hello").await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}
