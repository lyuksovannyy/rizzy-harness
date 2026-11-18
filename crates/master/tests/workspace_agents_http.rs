//! Workspace agents (ADOPTION_PLAN X.33, PLAN.md T79): a workspace's own
//! agents and its edits of the server's, over HTTP. Who may write them, that
//! one workspace's agents are never listed, resolved or run in another, that
//! a deleted one fails closed, that server instructions come before the
//! workspace's, the caps, the cascade when a workspace goes, and that a
//! running turn keeps the agent it started with.

mod common;
use common::{Login as _, SameOrigin as _};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_agent_native::{
    ChatMsg, ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError,
    mock::Mock,
};
use harness_core::Secret;
use harness_core::event::ToolCallRef;
use harness_master::auth::{Auth, Claim};
use harness_master::config::{Config, ModelConfig};
use harness_master::providers::ProviderRuntime;
use harness_master::{Master, router};
use harness_memory::SqliteMemory;
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::{Notify, mpsc};

/// Records every request. A message containing `LOOK` first calls `glob`;
/// while `hold` is set, the request after that tool's result waits for
/// `gate`, so a test can change agents in the middle of a turn.
struct Rec {
    reqs: Mutex<Vec<ChatRequest>>,
    hold: AtomicBool,
    gate: Notify,
}

#[async_trait::async_trait]
impl Provider for Rec {
    fn name(&self) -> &'static str {
        "mock"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        d: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        self.reqs.lock().unwrap().push(req.clone());
        let after_tool = matches!(req.messages.last(), Some(ChatMsg::Tool { .. }));
        if after_tool && self.hold.swap(false, Ordering::SeqCst) {
            self.gate.notified().await;
        }
        let asked = req
            .messages
            .iter()
            .rev()
            .find_map(|m| m.user_text().map(str::to_string))
            .unwrap_or_default();
        if asked.contains("LOOK") && !after_tool {
            return Ok(Completion {
                text: String::new(),
                tool_calls: vec![ToolCallRef {
                    call_id: "c1".into(),
                    tool: "glob".into(),
                    input: json!({"pattern": "*"}),
                }],
                usage: None,
                stop: Default::default(),
                thinking: None,
            });
        }
        Mock {
            delay: Duration::from_millis(0),
        }
        .complete(req, d)
        .await
    }
}

struct Server {
    base: String,
    ws: std::path::PathBuf,
    admin: String,
    master: Master,
    rec: Arc<Rec>,
    _dir: tempfile::TempDir,
}

fn config() -> Config {
    let mut cfg = Config::default();
    cfg.providers[0].models = vec![ModelConfig {
        id: "mock".into(),
        ..Default::default()
    }];
    cfg.nodes.local_node_users = harness_master::config::LocalNodeUsers::Granted;
    cfg
}

async fn start() -> Server {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    std::fs::write(ws.join("a.txt"), "a").unwrap();
    let rec = Arc::new(Rec {
        reqs: Mutex::new(vec![]),
        hold: AtomicBool::new(false),
        gate: Notify::new(),
    });
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
            rec.clone(),
            harness_tools::builtin(),
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
    let (rt, _) = ProviderRuntime::start(config(), dir.path(), Default::default()).unwrap();
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
    master.add_link(node.attach().await, true).await.unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    let mut s = Server {
        base: format!("http://{addr}"),
        ws,
        admin: String::new(),
        master,
        rec,
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
    match cookie {
        Some(c) => rb = rb.login(c).same_origin(),
        None => rb = rb.bearer_auth("tok"),
    }
    if let Some(b) = body {
        rb = rb.json(&b);
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
    cookie: &str,
) -> (StatusCode, Value) {
    let (st, _, v) = send(s, method, path, body, Some(cookie)).await;
    (st, v)
}

/// The API token (no cookie).
async fn token(s: &Server, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let (st, _, v) = send(s, method, path, body, None).await;
    (st, v)
}

async fn get(s: &Server, path: &str, cookie: &str) -> Value {
    let (st, v) = call(s, Method::GET, path, None, cookie).await;
    assert_eq!(st, StatusCode::OK, "{path}: {v}");
    v
}

/// An account with the agentic grant, logged in with its own password.
async fn user(s: &Server, name: &str) -> String {
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
    let (st, v) = call(
        s,
        Method::POST,
        "/api/workspaces",
        Some(json!({ "name": name })),
        owner,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["workspace_id"].as_str().unwrap().to_string()
}

/// Adds `username` with `role` at the `sandbox` level.
async fn add(s: &Server, by: &str, ws: &str, username: &str, role: &str) {
    let (st, v) = call(
        s,
        Method::POST,
        &format!("/api/workspaces/{ws}/members"),
        Some(json!({ "username": username, "role": role })),
        by,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let uid = v["user_id"].as_str().unwrap();
    let (st, v) = call(
        s,
        Method::PUT,
        &format!("/api/workspaces/{ws}/members/{uid}/exec-level"),
        Some(json!({ "level": "sandbox" })),
        by,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

async fn personal_id(s: &Server, who: &str) -> String {
    let v = get(s, "/api/workspaces", who).await;
    let w = v["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["personal"] == true)
        .unwrap();
    w["workspace_id"].as_str().unwrap().to_string()
}

fn features() -> Value {
    json!({
        "memory": false, "web": false, "read_files": true, "edit_files": true,
        "commands": false, "delegate": false, "as_agent": true, "as_subagent": true
    })
}

fn helper(id: &str, prompt: &str) -> Value {
    json!({"id": id, "name": "Helper", "prompt": prompt, "features": features()})
}

async fn create(s: &Server, who: &str, ws: &str, body: Value) -> (StatusCode, Value) {
    call(
        s,
        Method::POST,
        &format!("/api/agents?workspace={ws}"),
        Some(body),
        who,
    )
    .await
}

async fn listed(s: &Server, who: &str, ws: &str) -> Value {
    get(s, &format!("/api/agents?workspace={ws}"), who).await
}

fn ids(v: &Value) -> Vec<String> {
    v["agents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap().to_string())
        .collect()
}

async fn new_chat(s: &Server, who: &str, ws: &str, extra: Value) -> (StatusCode, Value) {
    let mut body = json!({
        "cwd": s.ws.display().to_string(), "workspace_id": ws, "approval_mode": "auto"
    });
    for (k, v) in extra.as_object().unwrap() {
        body[k] = v.clone();
    }
    call(s, Method::POST, "/api/sessions", Some(body), who).await
}

async fn chat(s: &Server, who: &str, ws: &str, extra: Value) -> String {
    let (st, v) = new_chat(s, who, ws, extra).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["session_id"].as_str().unwrap().to_string()
}

async fn prompt(s: &Server, who: &str, sid: &str, body: Value) -> (StatusCode, Value) {
    call(
        s,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(body),
        who,
    )
    .await
}

async fn turns_ended(s: &Server, who: &str, sid: &str, n: usize) {
    for _ in 0..500 {
        let v = get(s, &format!("/api/sessions/{sid}/events"), who).await;
        let ended = v["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == "turn_ended")
            .count();
        if ended >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("{sid}: fewer than {n} turns ended");
}

/// One finished turn of `text`; the system prompt the model got.
async fn say(s: &Server, who: &str, sid: &str, text: &str, n: usize) -> String {
    let (st, v) = prompt(s, who, sid, json!({ "text": text })).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    turns_ended(s, who, sid, n).await;
    last_system(s)
}

fn last_system(s: &Server) -> String {
    match s.rec.reqs.lock().unwrap().last().unwrap().messages.first() {
        Some(ChatMsg::System(p)) => p.clone(),
        other => panic!("no system prompt: {other:?}"),
    }
}

/// Point 3 of the brief: only `settings` holders write a workspace's
/// agents; a workspace admin cannot touch the server's scope or another
/// workspace; a server admin who is not a member neither reads nor writes
/// them; a write names its scope.
#[tokio::test]
async fn who_may_write_workspace_agents() {
    let s = start().await;
    let bo = user(&s, "bo").await;
    let cy = user(&s, "cy").await;
    let di = user(&s, "di").await;
    let a = team(&s, &bo, "Acme").await;
    let b = team(&s, &di, "Beta").await;
    add(&s, &bo, &a, "cy", "member").await;

    let (st, v) = create(&s, &bo, &a, helper("helper", "HELPER-PROMPT")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["origin"], "custom");
    assert_eq!(v["scope"], "workspace");

    // A member without `settings`: reads, cannot write.
    let l = listed(&s, &cy, &a).await;
    assert!(ids(&l).contains(&"helper".into()), "{l}");
    assert_eq!(l["may_edit"], false);
    assert_eq!(listed(&s, &bo, &a).await["may_edit"], true);
    let (st, _) = create(&s, &cy, &a, helper("mine", "x")).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = call(
        &s,
        Method::PATCH,
        &format!("/api/agents/helper?workspace={a}"),
        Some(json!({"prompt": "taken over"})),
        &cy,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = call(
        &s,
        Method::PATCH,
        &format!("/api/agents/general?workspace={a}"),
        Some(json!({"instructions": "cy was here"})),
        &cy,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = call(
        &s,
        Method::DELETE,
        &format!("/api/agents/helper?workspace={a}"),
        None,
        &cy,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // A workspace admin: not the server's scope, not another workspace.
    let (st, _) = call(
        &s,
        Method::POST,
        "/api/agents?scope=server",
        Some(helper("srv", "x")),
        &bo,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = call(
        &s,
        Method::PATCH,
        "/api/agents/general?scope=server",
        Some(json!({"instructions": "bo was here"})),
        &bo,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = create(&s, &bo, &b, helper("intruder", "x")).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // A write without a scope is refused, not guessed.
    let (st, v) = call(&s, Method::POST, "/api/agents", Some(helper("x", "x")), &bo).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");

    // A server admin who is not a member: neither reads nor writes.
    let (st, _) = call(
        &s,
        Method::GET,
        &format!("/api/agents?workspace={a}"),
        None,
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _) = call(
        &s,
        Method::GET,
        &format!("/api/agents/helper?workspace={a}"),
        None,
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _) = create(&s, &s.admin, &a, helper("adm", "x")).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _) = call(
        &s,
        Method::DELETE,
        &format!("/api/agents/helper?workspace={a}"),
        None,
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // At the server's scope it does not exist.
    let (st, _) = call(
        &s,
        Method::GET,
        "/api/agents/helper?scope=server",
        None,
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // Everyone makes agents in their own Personal workspace.
    let mine = personal_id(&s, &cy).await;
    let (st, v) = create(&s, &cy, &mine, helper("cys", "CY-PROMPT")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(!ids(&listed(&s, &cy, &a).await).contains(&"cys".into()));
    assert!(!ids(&listed(&s, &bo, &personal_id(&s, &bo).await).await).contains(&"cys".into()));
}

/// Point 1: an agent of workspace A is never listed, resolved or run in B:
/// not by `agent_id`, an `@mention`, the agent switch, a default, a
/// routine, its page, or the API token without a workspace.
#[tokio::test]
async fn a_workspace_agent_stays_in_its_workspace() {
    let s = start().await;
    let bo = user(&s, "bo").await;
    let cy = user(&s, "cy").await;
    let di = user(&s, "di").await;
    let a = team(&s, &bo, "Acme").await;
    let b = team(&s, &di, "Beta").await;
    add(&s, &bo, &a, "cy", "member").await;
    let (st, v) = create(&s, &bo, &a, helper("helper", "HELPER-PROMPT")).await;
    assert_eq!(st, StatusCode::OK, "{v}");

    // In A, members run it.
    let sid = chat(&s, &cy, &a, json!({"agent_id": "helper"})).await;
    assert!(say(&s, &cy, &sid, "hi", 1).await.contains("HELPER-PROMPT"));

    // In B: not listed, no page, not by id, mention, switch or default.
    assert!(!ids(&listed(&s, &di, &b).await).contains(&"helper".into()));
    let (st, _) = call(
        &s,
        Method::GET,
        &format!("/api/agents/helper?workspace={b}"),
        None,
        &di,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, v) = new_chat(&s, &di, &b, json!({"agent_id": "helper"})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    let other = chat(&s, &di, &b, json!({})).await;
    let (st, v) = prompt(&s, &di, &other, json!({"text": "hi", "agent_id": "helper"})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    // A mention of no agent here is text: the chat's own agent answers.
    assert!(
        !say(&s, &di, &other, "@helper hi", 1)
            .await
            .contains("HELPER-PROMPT")
    );
    let (st, v) = call(
        &s,
        Method::POST,
        &format!("/api/sessions/{other}/agent"),
        Some(json!({"agent_id": "helper"})),
        &di,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    let (st, v) = call(
        &s,
        Method::PATCH,
        &format!("/api/workspaces/{b}"),
        Some(json!({"settings": {"default_agent": "helper"}})),
        &di,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    let (st, v) = call(
        &s,
        Method::POST,
        "/api/routines",
        Some(json!({"name": "r", "workspace_id": b,
            "spec": {"profile": "chat", "agent_id": "helper", "prompt": "hi"},
            "triggers": [{"kind": "schedule", "config": {"type": "interval", "every_minutes": 60}}]})),
        &di,
    )
    .await;
    assert!(st.is_client_error(), "{st} {v}");
    // Model chains name only agents of the workspace.
    let (st, v) = call(
        &s,
        Method::PATCH,
        &format!("/api/workspaces/{b}"),
        Some(json!({"settings": {"agent_models": {"helper": {"model": "demo::mock"}}}})),
        &di,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    // In A they may: default and chain.
    let (st, v) = call(
        &s,
        Method::PATCH,
        &format!("/api/workspaces/{a}"),
        Some(json!({"settings": {"default_agentic_agent": "helper",
            "agent_models": {"helper": {"model": "demo::mock"}}}})),
        &bo,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let by_default = chat(&s, &bo, &a, json!({})).await;
    assert!(
        say(&s, &bo, &by_default, "hi", 1)
            .await
            .contains("HELPER-PROMPT")
    );

    // The API token without a workspace sees the server's set.
    let (_, v) = token(&s, Method::GET, "/api/agents", None).await;
    assert!(!ids(&v).contains(&"helper".into()), "{v}");
    let (st, _) = token(&s, Method::GET, "/api/agents/helper", None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // A workspace's id cannot take a server agent's id or alias.
    for (id, aliases) in [
        ("coder", json!([])),
        ("new", json!([])),
        ("mine", json!(["dev"])),
    ] {
        let mut body = helper(id, "x");
        body["aliases"] = aliases;
        let (st, v) = create(&s, &bo, &a, body).await;
        assert_eq!(st, StatusCode::CONFLICT, "{id}: {v}");
    }
}

/// Point 2: deleting a workspace agent that a chat, a default and a
/// routine still name degrades like a removed server agent: the chat is
/// refused (`AgentGone`), the default is `BadDefault`, the routine is
/// blocked. Nothing runs in its place.
#[tokio::test]
async fn a_deleted_workspace_agent_fails_closed() {
    let s = start().await;
    let bo = user(&s, "bo").await;
    let a = team(&s, &bo, "Acme").await;
    let (st, v) = create(&s, &bo, &a, helper("helper", "HELPER-PROMPT")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = chat(&s, &bo, &a, json!({"agent_id": "helper"})).await;
    say(&s, &bo, &sid, "hi", 1).await;
    let (st, v) = call(
        &s,
        Method::PATCH,
        &format!("/api/workspaces/{a}"),
        Some(json!({"settings": {"default_agentic_agent": "helper",
            "agent_models": {"helper": {"model": "demo::mock"}}}})),
        &bo,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = call(
        &s,
        Method::POST,
        "/api/routines",
        Some(json!({"name": "r", "workspace_id": a,
            "spec": {"profile": "agentic", "agent_id": "helper", "prompt": "hi",
                     "node_id": "local", "cwd": s.ws.display().to_string()},
            "triggers": [{"kind": "schedule", "config": {"type": "interval", "every_minutes": 60}}]})),
        &bo,
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{v}");

    let (st, v) = call(
        &s,
        Method::DELETE,
        &format!("/api/agents/helper?workspace={a}"),
        None,
        &bo,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = prompt(&s, &bo, &sid, json!({"text": "again"})).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v.to_string().contains("helper"), "{v}");
    let (st, v) = new_chat(&s, &bo, &a, json!({})).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v.to_string().contains("default_agentic_agent"), "{v}");
    let l = get(&s, &format!("/api/routines?workspace={a}"), &bo).await;
    let blocked = l["routines"][0]["blocked"].as_str().unwrap_or_default();
    assert!(blocked.contains("helper"), "{l}");
    // Its chain went with it: a new agent with the id starts fresh.
    let w = s.master.inner_mirror().workspace(&a).unwrap().unwrap();
    let stored: Value = serde_json::from_str(&w.settings).unwrap();
    assert!(stored["agent_models"].get("helper").is_none(), "{stored}");
    assert_eq!(stored["default_agentic_agent"], "helper", "{stored}");
}

/// Point 7 and 8: the server's additional instructions come first, then
/// the workspace's; a workspace's edit stays in it; every write is seen at
/// once by the next listing and the next turn.
#[tokio::test]
async fn server_then_workspace_instructions_seen_at_once() {
    let s = start().await;
    let bo = user(&s, "bo").await;
    let di = user(&s, "di").await;
    let a = team(&s, &bo, "Acme").await;
    let b = team(&s, &di, "Beta").await;
    let patch = |id: &str, scope: String, body: Value, who: String| {
        let s = &s;
        let path = format!("/api/agents/{id}?{scope}");
        async move { call(s, Method::PATCH, &path, Some(body), &who).await }
    };
    let (st, v) = patch(
        "coder",
        "scope=server".into(),
        json!({"instructions": "SERVER-RULE"}),
        s.admin.clone(),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = patch(
        "coder",
        format!("workspace={a}"),
        json!({"instructions": "TEAM-RULE", "summary": "Acme's coder"}),
        bo.clone(),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["instructions"], "TEAM-RULE");
    assert_eq!(v["server_instructions"], "SERVER-RULE");
    // A built-in stays a built-in: its prompt is not the workspace's.
    let (st, _) = patch(
        "coder",
        format!("workspace={a}"),
        json!({"prompt": "replaced"}),
        bo.clone(),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT);

    let in_a = chat(&s, &bo, &a, json!({"agent_id": "coder"})).await;
    let p = say(&s, &bo, &in_a, "hi", 1).await;
    let (server, team) = (p.find("SERVER-RULE").unwrap(), p.find("TEAM-RULE").unwrap());
    assert!(server < team, "{p}");
    let in_b = chat(&s, &di, &b, json!({"agent_id": "coder"})).await;
    let p = say(&s, &di, &in_b, "hi", 1).await;
    assert!(p.contains("SERVER-RULE") && !p.contains("TEAM-RULE"), "{p}");
    let coder = |v: &Value| {
        v["agents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == "coder")
            .unwrap()["description"]
            .clone()
    };
    assert_eq!(coder(&listed(&s, &bo, &a).await), "Acme's coder");
    assert_ne!(coder(&listed(&s, &di, &b).await), "Acme's coder");

    // Seen at once: a workspace write, then a server write.
    let (st, _) = patch(
        "coder",
        format!("workspace={a}"),
        json!({"instructions": "TEAM-RULE-2"}),
        bo.clone(),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(say(&s, &bo, &in_a, "hi", 2).await.contains("TEAM-RULE-2"));
    let (st, _) = patch(
        "coder",
        "scope=server".into(),
        json!({"instructions": "SERVER-RULE-2"}),
        s.admin.clone(),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let p = say(&s, &bo, &in_a, "hi", 3).await;
    assert!(
        p.contains("SERVER-RULE-2") && p.contains("TEAM-RULE-2"),
        "{p}"
    );
    let (st, v) = create(&s, &bo, &a, helper("fresh", "FRESH")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(ids(&listed(&s, &bo, &a).await).contains(&"fresh".into()));
}

/// A workspace custom agent that delegates gets the sub-agents of its own
/// workspace and the server's, never another workspace's; a server agent
/// gets only the server's, so a workspace cannot plant an agent under a
/// server agent's name.
#[tokio::test]
async fn delegates_come_from_the_agents_own_scope() {
    let s = start().await;
    let bo = user(&s, "bo").await;
    let di = user(&s, "di").await;
    let a = team(&s, &bo, "Acme").await;
    let b = team(&s, &di, "Beta").await;
    let mut f = features();
    f["delegate"] = json!(true);
    let (st, v) = create(
        &s,
        &bo,
        &a,
        json!({"id": "lead", "name": "Lead", "prompt": "Lead.", "features": f}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = create(&s, &bo, &a, helper("helper", "HELPER-PROMPT")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = create(&s, &di, &b, helper("bhelp", "B-PROMPT")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = token(
        &s,
        Method::POST,
        "/api/agents?scope=server",
        Some(json!({"id": "slead", "name": "Server lead", "prompt": "Lead.", "features": f})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let lead = |v: &Value, id: &str| -> Vec<String> {
        v["agents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == id)
            .unwrap()["delegates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d.as_str().unwrap().to_string())
            .collect()
    };
    let la = listed(&s, &bo, &a).await;
    let d = lead(&la, "lead");
    assert!(
        d.contains(&"helper".into()) && d.contains(&"scout".into()),
        "{d:?}"
    );
    assert!(!d.contains(&"bhelp".into()), "{d:?}");
    let d = lead(&la, "slead");
    assert!(
        !d.contains(&"helper".into()) && d.contains(&"scout".into()),
        "{d:?}"
    );
    assert!(!lead(&listed(&s, &di, &b).await, "slead").contains(&"bhelp".into()));
}

/// Point 5: a cap on custom agents per workspace and on one agent's size,
/// refused past it, never cut.
#[tokio::test]
async fn caps_refuse_past_the_limit() {
    let s = start().await;
    let bo = user(&s, "bo").await;
    let a = team(&s, &bo, "Acme").await;
    for i in 0..30 {
        let (st, v) = create(&s, &bo, &a, helper(&format!("h{i}"), "x")).await;
        assert_eq!(st, StatusCode::OK, "{i}: {v}");
    }
    let (st, v) = create(&s, &bo, &a, helper("h30", "x")).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v.to_string().contains("30"), "{v}");
    // Deleting one makes room.
    let (st, _) = call(
        &s,
        Method::DELETE,
        &format!("/api/agents/h0?workspace={a}"),
        None,
        &bo,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    // 20,000 characters of four bytes each: within the prompt's characters,
    // past a stored agent's bytes.
    let (st, v) = create(&s, &bo, &a, helper("big", &"😀".repeat(20_000))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert!(!ids(&listed(&s, &bo, &a).await).contains(&"big".into()));
    let (st, v) = create(&s, &bo, &a, helper("h30", "x")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

/// Point 6: deleting a workspace deletes its agents (the real delete path).
#[tokio::test]
async fn deleting_a_workspace_deletes_its_agents() {
    let s = start().await;
    let bo = user(&s, "bo").await;
    let a = team(&s, &bo, "Acme").await;
    let (st, v) = create(&s, &bo, &a, helper("helper", "x")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = call(
        &s,
        Method::PATCH,
        &format!("/api/agents/general?workspace={a}"),
        Some(json!({"instructions": "TEAM"})),
        &bo,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        s.master.inner_mirror().workspace_agents(&a).unwrap().len(),
        2
    );
    let (st, v) = call(
        &s,
        Method::DELETE,
        &format!("/api/workspaces/{a}"),
        None,
        &bo,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(
        s.master
            .inner_mirror()
            .workspace_agents(&a)
            .unwrap()
            .is_empty()
    );
    // And the id is free at the server's scope again.
    let (st, v) = token(
        &s,
        Method::POST,
        "/api/agents?scope=server",
        Some(helper("helper", "x")),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

/// Point 6: a turn runs the agent it started with. Changing the agent's
/// prompt between the turn's two model requests leaves that turn alone;
/// the next turn has the new prompt. Deleting it mid-turn lets the turn
/// finish and refuses the next.
#[tokio::test]
async fn a_running_turn_keeps_its_agent() {
    let s = start().await;
    let bo = user(&s, "bo").await;
    let a = team(&s, &bo, "Acme").await;
    let (st, v) = create(&s, &bo, &a, helper("helper", "FIRST-PROMPT")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = chat(&s, &bo, &a, json!({"agent_id": "helper"})).await;
    let before = s.rec.reqs.lock().unwrap().len();
    s.rec.hold.store(true, Ordering::SeqCst);
    let (st, v) = prompt(&s, &bo, &sid, json!({"text": "LOOK around"})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // Wait for the request after the tool result: it is held.
    for _ in 0..500 {
        if s.rec.reqs.lock().unwrap().len() >= before + 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let (st, v) = call(
        &s,
        Method::PATCH,
        &format!("/api/agents/helper?workspace={a}"),
        Some(json!({"prompt": "SECOND-PROMPT"})),
        &bo,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    s.rec.gate.notify_one();
    turns_ended(&s, &bo, &sid, 1).await;
    {
        let reqs = s.rec.reqs.lock().unwrap();
        let turn: Vec<_> = reqs[before..].iter().collect();
        assert_eq!(turn.len(), 2);
        for r in turn {
            let Some(ChatMsg::System(p)) = r.messages.first() else {
                panic!("no system prompt")
            };
            assert!(
                p.contains("FIRST-PROMPT") && !p.contains("SECOND-PROMPT"),
                "{p}"
            );
        }
    }
    assert!(say(&s, &bo, &sid, "hi", 2).await.contains("SECOND-PROMPT"));

    // Deleted mid-turn: the turn ends as it would, the next is refused.
    let before = s.rec.reqs.lock().unwrap().len();
    s.rec.hold.store(true, Ordering::SeqCst);
    let (st, v) = prompt(&s, &bo, &sid, json!({"text": "LOOK again"})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    for _ in 0..500 {
        if s.rec.reqs.lock().unwrap().len() >= before + 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let (st, _) = call(
        &s,
        Method::DELETE,
        &format!("/api/agents/helper?workspace={a}"),
        None,
        &bo,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    s.rec.gate.notify_one();
    turns_ended(&s, &bo, &sid, 3).await;
    let (st, v) = prompt(&s, &bo, &sid, json!({"text": "once more"})).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
}

fn strong() -> Value {
    json!({
        "memory": true, "recall": true, "web": true, "read_files": true, "edit_files": true,
        "commands": true, "mcp": true, "delegate": false, "as_agent": true, "as_subagent": false
    })
}

/// H1 of the review, as the owner decided it ("Allow it", 2026-11-18): a
/// team workspace's admin, who is no server admin, may make an agent that
/// searches the member's earlier chats and memory and also runs commands,
/// uses MCP servers and the web, make it both defaults, and give an agent
/// a member already chose more switches later. The member's ordinary chat
/// then runs it under the admin's system prompt. Pinned so that changing
/// this is a deliberate decision (PLAN.md T79, accepted leftover).
#[tokio::test]
async fn a_workspace_admin_may_give_members_agents_chat_search_with_commands_as_decided() {
    let s = start().await;
    let bo = user(&s, "bo").await;
    let cy = user(&s, "cy").await;
    let a = team(&s, &bo, "Acme").await;
    add(&s, &bo, &a, "cy", "member").await;
    let (st, v) = create(
        &s,
        &bo,
        &a,
        json!({"id": "aide", "name": "Team aide", "prompt": "AIDE-PROMPT", "features": strong()}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = call(
        &s,
        Method::PATCH,
        &format!("/api/workspaces/{a}"),
        Some(json!({"settings": {"default_agent": "aide", "default_agentic_agent": "aide"}})),
        &bo,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let spec = s
        .master
        .resolver_for(Some(&a))
        .await
        .agent_spec_with(Some("aide"), None, None)
        .unwrap();
    for t in ["recall", "bash", "web_fetch", "mcp"] {
        assert!(spec.allows_tool(t), "{t}");
    }
    let sid = chat(&s, &cy, &a, json!({})).await;
    assert!(say(&s, &cy, &sid, "hi", 1).await.contains("AIDE-PROMPT"));
    // Switches added later reach a chat that already chose the agent.
    let weak = json!({"memory": false, "web": false, "read_files": true, "edit_files": true,
        "commands": false, "delegate": false, "as_agent": true, "as_subagent": false});
    let (st, v) = create(
        &s,
        &bo,
        &a,
        json!({"id": "reader", "name": "Reader", "prompt": "READER", "features": weak}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let picked = chat(&s, &cy, &a, json!({"agent_id": "reader"})).await;
    say(&s, &cy, &picked, "hi", 1).await;
    let (st, v) = call(
        &s,
        Method::PATCH,
        &format!("/api/agents/reader?workspace={a}"),
        Some(json!({"features": strong(), "prompt": "NOW-STRONGER"})),
        &bo,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(
        say(&s, &cy, &picked, "again", 2)
            .await
            .contains("NOW-STRONGER")
    );
    let tools: Vec<String> = s
        .rec
        .reqs
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .tools
        .iter()
        .map(|t| t.name.clone())
        .collect();
    assert!(tools.iter().any(|t| t == "bash"), "{tools:?}");
}

/// H1b: impersonation is not part of it. A workspace's agent may not be
/// called what a server agent is called, or its id or an alias, after
/// cleaning and case-folding; a name it already had stays when a server
/// agent takes it later.
#[tokio::test]
async fn a_workspace_agent_cannot_take_a_server_agents_name() {
    let s = start().await;
    let bo = user(&s, "bo").await;
    let a = team(&s, &bo, "Acme").await;
    for name in [
        "Assistant",
        " assistant ",
        "A-SSISTANT",
        "Assist\u{200b}ant",
        "Coder",
        "dev",
    ] {
        let (st, v) = create(
            &s,
            &bo,
            &a,
            json!({"id": "fake", "name": name, "prompt": "x", "features": features()}),
        )
        .await;
        assert_eq!(st, StatusCode::CONFLICT, "{name:?}: {v}");
    }
    let (st, v) = create(
        &s,
        &bo,
        &a,
        json!({"id": "mine", "name": "Planner", "prompt": "x", "features": features()}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, _) = call(
        &s,
        Method::PATCH,
        &format!("/api/agents/mine?workspace={a}"),
        Some(json!({"name": "Software developer"})),
        &bo,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT);
    // The server takes the name later: allowed; the workspace's keeps it
    // and can still be changed.
    let (st, v) = token(
        &s,
        Method::POST,
        "/api/agents?scope=server",
        Some(json!({"id": "splanner", "name": "Planner", "prompt": "x", "features": features()})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = call(
        &s,
        Method::PATCH,
        &format!("/api/agents/mine?workspace={a}"),
        Some(json!({"prompt": "y"})),
        &bo,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

/// M1 (r2 reversed): one rule, as for files: a server agent wins. Making
/// a server agent whose id some account's workspace agent has succeeds and
/// says nothing about workspaces; in that workspace the id now means the
/// server's agent, and the workspace's editors see theirs as hidden.
#[tokio::test]
async fn a_server_agent_wins_over_a_workspace_agents_id() {
    let s = start().await;
    let cy = user(&s, "cy").await;
    let p = personal_id(&s, &cy).await;
    let (st, v) = create(&s, &cy, &p, helper("planner", "CY-PLANNER")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = chat(&s, &cy, &p, json!({"agent_id": "planner"})).await;
    assert!(say(&s, &cy, &sid, "hi", 1).await.contains("CY-PLANNER"));
    let (st, v) = call(
        &s,
        Method::POST,
        "/api/agents?scope=server",
        Some(helper("planner", "SERVER-PLANNER")),
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let text = v.to_string();
    assert!(!text.contains("CY-PLANNER") && !text.contains(&p), "{v}");
    // In cy's workspace @planner is the server's now.
    assert!(
        say(&s, &cy, &sid, "again", 2)
            .await
            .contains("SERVER-PLANNER")
    );
    let l = listed(&s, &cy, &p).await;
    let shown: Vec<&Value> = l["agents"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["id"] == "planner")
        .collect();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0]["scope"], "server");
    assert_eq!(
        l["hidden"],
        json!([{"id": "planner", "name": "Helper"}]),
        "{l}"
    );
    // Its editors may delete it; then nothing is hidden.
    let (st, v) = call(
        &s,
        Method::DELETE,
        &format!("/api/agents/planner?workspace={p}"),
        None,
        &cy,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(listed(&s, &cy, &p).await["hidden"], json!([]));
}

/// L1 (r3): ids win over aliases, and in a workspace's set the server's
/// agents claim aliases first. A server agent may take an alias that is a
/// workspace agent's id: in that workspace the id wins (`@helper` is the
/// workspace's), everywhere else the alias is the server agent's.
#[tokio::test]
async fn a_workspace_agents_id_beats_a_server_alias_in_its_workspace() {
    let s = start().await;
    let bo = user(&s, "bo").await;
    let a = team(&s, &bo, "Acme").await;
    let (st, v) = create(&s, &bo, &a, helper("helper", "WS-HELPER")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let mut body = helper("srv", "SERVER-SRV");
    body["aliases"] = json!(["helper", "aide"]);
    let (st, v) = token(&s, Method::POST, "/api/agents?scope=server", Some(body)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let r = s.master.resolver_for(Some(&a)).await;
    assert_eq!(r.top_level("helper").unwrap().id, "helper");
    assert_eq!(r.top_level("aide").unwrap().id, "srv");
    assert_eq!(
        s.master.resolver(None).top_level("helper").unwrap().id,
        "srv"
    );
    // A workspace agent may not take a server agent's alias.
    let mut mine = helper("mine", "x");
    mine["aliases"] = json!(["aide"]);
    let (st, _) = create(&s, &bo, &a, mine).await;
    assert_eq!(st, StatusCode::CONFLICT);
}

/// M2 (r4): a workspace's overlay on a server agent goes when that agent
/// is deleted, so it never lands on a later, unrelated agent with its id.
#[tokio::test]
async fn a_deleted_server_agent_takes_workspace_overlays_with_it() {
    let s = start().await;
    let bo = user(&s, "bo").await;
    let a = team(&s, &bo, "Acme").await;
    let (st, v) = token(
        &s,
        Method::POST,
        "/api/agents?scope=server",
        Some(helper("tool", "OLD")),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = call(
        &s,
        Method::PATCH,
        &format!("/api/agents/tool?workspace={a}"),
        Some(json!({"instructions": "STALE-WS-RULE"})),
        &bo,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(
        tool_prompt(&s, &a)
            .await
            .unwrap()
            .ends_with("STALE-WS-RULE")
    );
    let (st, v) = token(&s, Method::DELETE, "/api/agents/tool?scope=server", None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(
        s.master
            .inner_mirror()
            .workspace_agents(&a)
            .unwrap()
            .is_empty()
    );
    let (st, v) = token(
        &s,
        Method::POST,
        "/api/agents?scope=server",
        Some(helper("tool", "NEW-UNRELATED")),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(tool_prompt(&s, &a).await.as_deref(), Some("NEW-UNRELATED"));
}

async fn tool_prompt(s: &Server, ws: &str) -> Option<String> {
    s.master
        .resolver_for(Some(ws))
        .await
        .agents()
        .get("tool")
        .map(|t| t.spec.system_prompt.clone())
}
