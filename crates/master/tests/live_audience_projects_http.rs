//! The UI sockets' content-free nudges (`sessions_changed`,
//! `memory_changed`, …) reach only the sockets of those who may see what
//! changed (PLAN.md T58): a chat's changes its owner, a person's memory
//! that person, a workspace's shared things its members at the time of
//! sending. Nobody else learns when someone works in a private chat or
//! saves a personal fact, a server admin included.
//!
//! Silence is asserted without sleeping: the observer then makes a change
//! only it is told about (it creates a workspace of its own), and the
//! first message its socket gets must be that one. The broadcast channel
//! is first in, first out, so anything raised before the marker would
//! arrive before it.

mod common;
use common::{Login as _, SameOrigin as _};

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::Secret;
use harness_master::auth::{Auth, Claim};
use harness_master::config::Config;
use harness_master::providers::ProviderRuntime;
use harness_master::{Master, router};
use harness_memory::SqliteMemory;
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

fn agents() -> harness_agents::AgentSet {
    let mut all = harness_agents::builtins();
    for (id, text) in [
        (
            "boss",
            "---\nname: Boss\ntools: all\ndelegates: [helper]\n---\nHand work to Helper.\n",
        ),
        (
            "helper",
            "---\nname: Helper\ntools: [read, remember]\n---\nHelp.\n",
        ),
    ] {
        all.push(harness_agents::parse(id, text, false).unwrap());
    }
    harness_agents::AgentSet::from_agents(all, harness_agents::DEFAULT_AGENT)
}

struct Server {
    base: String,
    addr: std::net::SocketAddr,
    ws: std::path::PathBuf,
    admin: String,
    _master: Master,
    _dir: tempfile::TempDir,
}

async fn start() -> Server {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let mut tools = harness_tools::builtin();
    tools.push(Arc::new(harness_memory::Remember));
    let node = NodeRuntime::start(NodeConfig {
        node_id: "local".into(),
        hostname: "h".into(),
        policy: NodePolicy {
            workspace_roots: vec![dir.path().into()],
            max_approval_mode: harness_core::policy::ApprovalMode::Auto,
            ..Default::default()
        },
        store: Store::open_in_memory().unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(0),
            }),
            tools,
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: harness_core::event::Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    })
    .await
    .unwrap();
    let mut cfg = Config::default();
    cfg.nodes.local_node_users = harness_master::config::LocalNodeUsers::Granted;
    let (rt, _) = ProviderRuntime::start(cfg, dir.path(), Default::default()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, "CLAIM").unwrap();
    let auth =
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim_path,
        });
    let master = Master::new(Store::open_in_memory().unwrap(), auth);
    master.set_memory(Arc::new(SqliteMemory::open_in_memory().unwrap()));
    master.set_provider_runtime(Arc::new(rt));
    master.set_agents(agents());
    master.add_link(node.attach().await, true).await.unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    let mut s = Server {
        base: format!("http://{addr}"),
        addr,
        ws,
        admin: String::new(),
        _master: master,
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
    cookie: Option<&str>,
) -> (StatusCode, Option<String>, Value) {
    let mut rb = reqwest::Client::new().request(method, format!("{}{path}", s.base));
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    if let Some(c) = cookie {
        rb = rb.login(c).same_origin();
    }
    let r = rb.send().await.unwrap();
    let status = r.status();
    let cookie = common::set_cookie(&r);
    let body = r.json().await.unwrap_or(Value::Null);
    if let Some(c) = &cookie {
        common::remember(c, &body);
    }
    (status, cookie, body)
}

async fn ok(s: &Server, method: Method, path: &str, body: Option<Value>, who: &str) -> Value {
    let (st, _, v) = send(s, method, path, body, Some(who)).await;
    assert_eq!(st, StatusCode::OK, "{path}: {v}");
    v
}

/// An account created by the admin, signed in with its own password.
async fn user(s: &Server, name: &str) -> String {
    let v = ok(
        s,
        Method::POST,
        "/api/users",
        Some(json!({"username": name, "exec_ceiling": "host"})),
        &s.admin,
    )
    .await;
    let pw = v["password"].as_str().unwrap().to_string();
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
    cookie.unwrap()
}

async fn team(s: &Server, owner: &str, name: &str) -> String {
    let v = ok(
        s,
        Method::POST,
        "/api/workspaces",
        Some(json!({ "name": name })),
        owner,
    )
    .await;
    v["workspace_id"].as_str().unwrap().to_string()
}

/// Add `username` to `ws` as a member; their user id.
async fn add(s: &Server, by: &str, ws: &str, username: &str) -> String {
    let v = ok(
        s,
        Method::POST,
        &format!("/api/workspaces/{ws}/members"),
        Some(json!({ "username": username, "role": "member" })),
        by,
    )
    .await;
    v["user_id"].as_str().unwrap().to_string()
}

async fn events(s: &Server, who: &str, sid: &str) -> Vec<Value> {
    ok(
        s,
        Method::GET,
        &format!("/api/sessions/{sid}/events"),
        None,
        who,
    )
    .await["events"]
        .as_array()
        .unwrap()
        .clone()
}

/// A fresh agentic chat of `who` in `ws` with one finished turn.
async fn turn(s: &Server, who: &str, ws: &str, text: &str) -> String {
    turn_with(s, who, ws, text, "general", None).await
}

async fn turn_with(
    s: &Server,
    who: &str,
    ws: &str,
    text: &str,
    agent: &str,
    project: Option<&str>,
) -> String {
    let mut body = json!({ "cwd": s.ws.display().to_string(), "workspace_id": ws,
                       "agent_id": agent, "approval_mode": "auto" });
    if let Some(p) = project {
        body["project_id"] = json!(p);
    }
    let v = ok(s, Method::POST, "/api/sessions", Some(body), who).await;
    let sid = v["session_id"].as_str().unwrap().to_string();
    ok(
        s,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({ "text": text })),
        who,
    )
    .await;
    for _ in 0..500 {
        if events(s, who, &sid)
            .await
            .iter()
            .any(|e| e["kind"] == "turn_ended")
        {
            return sid;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("no turn_ended in {sid}");
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn socket(s: &Server, c: &str) -> Socket {
    let mut r = format!("ws://{}/api/ws", s.addr)
        .into_client_request()
        .unwrap();
    r.headers_mut().insert("cookie", c.parse().unwrap());
    let origin = format!("http://{}", r.uri().authority().unwrap());
    r.headers_mut().insert("origin", origin.parse().unwrap());
    let (mut so, _) = tokio_tungstenite::connect_async(r).await.unwrap();
    so.send(Message::Text(common::socket_login(c).into()))
        .await
        .unwrap();
    so.send(Message::Text("hello".into())).await.unwrap();
    // The server answers the bad message: the socket is listening.
    loop {
        if next_type(&mut so).await == "error" {
            return so;
        }
    }
}

async fn next_type(so: &mut Socket) -> String {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(10), so.next())
            .await
            .expect("socket went quiet");
        if let Some(Ok(Message::Text(text))) = msg {
            let v: Value = serde_json::from_str(&text).unwrap();
            return v["type"].as_str().unwrap().to_string();
        }
    }
}

/// The types `so` receives up to and including the first `want`.
async fn until(so: &mut Socket, want: &str) -> Vec<String> {
    let mut got = Vec::new();
    loop {
        let t = next_type(so).await;
        got.push(t.clone());
        if t == want {
            return got;
        }
    }
}

/// `who` creates a workspace of its own: a `workspaces_changed` only it is
/// sent. Everything raised before it reaches its socket before it.
async fn marker(s: &Server, who: &str, n: &mut u32) {
    *n += 1;
    team(s, who, &format!("Marker {n}")).await;
}

/// Nothing reached `so` before the marker its user just made.
async fn silent(so: &mut Socket, what: &str) {
    let got = until(so, "workspaces_changed").await;
    assert_eq!(
        got,
        ["workspaces_changed"],
        "{what}: this socket was told about something it may not see"
    );
}

/// Review of PR 113: a server admin who is NOT in the workspace, a chat in
/// a project, a sub-agent session of the owner's chat, and moving a chat
/// into a project: none reaches a colleague or the admin; the owner's
/// second socket hears it.
#[tokio::test]
async fn review_projects_subagents_and_an_outside_admin_stay_silent() {
    let s = start().await;
    let bob = user(&s, "bob").await;
    let carol = user(&s, "carol").await;
    let acme = team(&s, &bob, "Acme").await;
    add(&s, &bob, &acme, "carol").await;
    let pv = ok(
        &s,
        Method::POST,
        "/api/projects",
        Some(json!({"name": "Rocket", "workspace_id": acme})),
        &bob,
    )
    .await;
    let pid = pv["project_id"].as_str().unwrap().to_string();
    let mut to_bob2 = socket(&s, &bob).await;
    let mut to_carol = socket(&s, &carol).await;
    let mut to_alex = socket(&s, &s.admin).await;
    let mut n = 0;

    // A chat in the project, with a sub-agent.
    let sid = turn_with(
        &s,
        &bob,
        &acme,
        "delegate: helper=read: nothing.txt",
        "boss",
        Some(&pid),
    )
    .await;
    let all = ok(
        &s,
        Method::GET,
        &format!("/api/sessions?workspace={acme}"),
        None,
        &bob,
    )
    .await;
    let list = all["sessions"].as_array().unwrap();
    for r in list {
        eprintln!(
            "row {} parent={} project={}",
            r["session_id"], r["parent_id"], r["project_id"]
        );
    }
    assert!(
        list.iter()
            .any(|r| r["parent_id"].as_str() == Some(sid.as_str())),
        "a sub-agent session of {sid}: {all}"
    );
    // A plain chat moved into the project and out again.
    let sid2 = turn(&s, &bob, &acme, "hello").await;
    ok(
        &s,
        Method::PUT,
        &format!("/api/sessions/{sid2}/project"),
        Some(json!({"project_id": pid})),
        &bob,
    )
    .await;
    ok(
        &s,
        Method::PUT,
        &format!("/api/sessions/{sid2}/project"),
        Some(json!({"project_id": null})),
        &bob,
    )
    .await;
    ok(
        &s,
        Method::PUT,
        &format!("/api/sessions/{sid}/title"),
        Some(json!({"title": "x"})),
        &bob,
    )
    .await;

    marker(&s, &carol, &mut n).await;
    silent(&mut to_carol, "a member, project chat and sub-agent").await;
    marker(&s, &s.admin, &mut n).await;
    silent(&mut to_alex, "a server admin outside the workspace").await;
    marker(&s, &bob, &mut n).await;
    let got = until(&mut to_bob2, "workspaces_changed").await;
    eprintln!("bob second socket got: {got:?}");
    assert!(got.iter().any(|t| t == "sessions_changed"), "{got:?}");
    assert!(got.iter().any(|t| t == "projects_changed"), "{got:?}");
}
