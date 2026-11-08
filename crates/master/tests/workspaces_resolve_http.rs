//! Workspace settings at work: models, agents, notices and new chats follow
//! the scope's resolver; turns follow their chat's workspace; the agentic
//! gates on prompts, approvals and restores; the revocation sweep; new
//! chats announced after stamping; and each member's memory switch.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_agent_native::{
    ChatMsg, ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError,
    mock::Mock,
};
use harness_core::Secret;
use harness_master::auth::{Auth, Claim};
use harness_master::config::{Config, ModelConfig};
use harness_master::providers::ProviderRuntime;
use harness_master::{Live, Master, router};
use harness_memory::SqliteMemory;
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

/// Records every request the node's model gets, sub-agents' included.
struct Rec {
    reqs: Mutex<Vec<ChatRequest>>,
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

/// The demo provider with `demo::mock` (the server default), `demo::fast`,
/// and `demo::old`, which its provider no longer lists.
fn config() -> Config {
    let mut cfg = Config::default();
    cfg.providers[0].models = ["mock", "fast", "old"]
        .iter()
        .map(|id| ModelConfig {
            id: id.to_string(),
            unavailable: *id == "old",
            ..Default::default()
        })
        .collect();
    cfg
}

/// The built-in agents, plus Boss, who hands work to Helper; both may use
/// every tool (`remember` included).
fn agents() -> harness_agents::AgentSet {
    let mut all = harness_agents::builtins();
    for (id, text) in [
        (
            "boss",
            "---\nname: Boss\ntools: all\ndelegates: [helper]\n---\nHand work to Helper.\n",
        ),
        ("helper", "---\nname: Helper\ntools: all\n---\nHelp.\n"),
    ] {
        all.push(harness_agents::parse(id, text, false).unwrap());
    }
    harness_agents::AgentSet::from_agents(all, harness_agents::DEFAULT_AGENT)
}

/// One agentic node whose model is recorded and which offers `remember`,
/// built-in memory, the providers above and a claimed admin.
async fn start() -> Server {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let rec = Arc::new(Rec {
        reqs: Mutex::new(vec![]),
    });
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
            rec.clone(),
            tools,
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: harness_core::event::Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: None,
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
    master.set_agents(agents());
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
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    if let Some(c) = cookie {
        rb = rb.header("cookie", c);
    }
    let r = rb.send().await.unwrap();
    let status = r.status();
    let cookie = r
        .headers()
        .get("set-cookie")
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
    (status, cookie, r.json().await.unwrap_or(Value::Null))
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

async fn get(s: &Server, path: &str, cookie: &str) -> Value {
    let (st, v) = call(s, Method::GET, path, None, cookie).await;
    assert_eq!(st, StatusCode::OK, "{path}: {v}");
    v
}

/// An account created by the admin, logged in with its own password.
/// Returns (cookie, user id).
async fn user(s: &Server, name: &str, can_agentic: bool) -> (String, String) {
    let (st, v) = call(
        s,
        Method::POST,
        "/api/users",
        Some(json!({"username": name, "password": "temporary pw", "can_agentic": can_agentic})),
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let id = v["user_id"].as_str().unwrap().to_string();
    let (_, temp, _) = send(
        s,
        Method::POST,
        "/api/login",
        Some(json!({"username": name, "password": "temporary pw"})),
        None,
    )
    .await;
    let (st, cookie, v) = send(
        s,
        Method::POST,
        "/api/me/password",
        Some(json!({"current": "temporary pw", "new": "a password of my own"})),
        temp.as_deref(),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    (cookie.unwrap(), id)
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
}

async fn settings(s: &Server, by: &str, ws: &str, settings: Value) {
    let (st, v) = call(
        s,
        Method::PATCH,
        &format!("/api/workspaces/{ws}"),
        Some(json!({ "settings": settings })),
        by,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

async fn personal_id(s: &Server, who: &str) -> String {
    let v = get(s, "/api/workspaces", who).await;
    assert_eq!(v["workspaces"][0]["personal"], true, "{v}");
    v["workspaces"][0]["workspace_id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// An agentic chat in `ws`; `extra` adds to the body (agent, model, ...).
async fn new_chat(s: &Server, who: &str, ws: &str, extra: Value) -> String {
    let mut body = json!({ "cwd": s.ws.display().to_string(), "workspace_id": ws });
    for (k, v) in extra.as_object().unwrap() {
        body[k] = v.clone();
    }
    let (st, v) = call(s, Method::POST, "/api/sessions", Some(body), who).await;
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

async fn events(s: &Server, who: &str, sid: &str) -> Vec<Value> {
    get(s, &format!("/api/sessions/{sid}/events"), who).await["events"]
        .as_array()
        .unwrap()
        .clone()
}

/// Wait for the `n`th event of `kind` in a chat and return it.
async fn nth_event(s: &Server, who: &str, sid: &str, kind: &str, n: usize) -> Value {
    for _ in 0..500 {
        let found: Vec<Value> = events(s, who, sid)
            .await
            .into_iter()
            .filter(|e| e["kind"] == kind)
            .collect();
        if found.len() >= n {
            return found[n - 1].clone();
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("no {kind} #{n} in {sid}");
}

/// A fresh chat in `ws` (no approvals) with one finished turn.
async fn turn(s: &Server, who: &str, ws: &str, text: &str, agent: &str) -> String {
    let sid = new_chat(
        s,
        who,
        ws,
        json!({"agent_id": agent, "approval_mode": "auto"}),
    )
    .await;
    let (st, v) = prompt(s, who, &sid, json!({ "text": text })).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    nth_event(s, who, &sid, "turn_ended", 1).await;
    sid
}

async fn wait_stopped(s: &Server, who: &str, sid: &str) {
    for _ in 0..500 {
        if get(s, &format!("/api/sessions/{sid}"), who).await["running"] == false {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("{sid} kept running");
}

/// Like `wait_stopped`, read from the store: for chats whose owner can no
/// longer sign in.
async fn wait_stopped_in_store(s: &Server, sid: &str) {
    let sid = sid.parse().unwrap();
    for _ in 0..500 {
        let row = s.master.inner_mirror().session(sid).unwrap().unwrap();
        if !row.running {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("{sid} kept running");
}

fn requests(s: &Server) -> usize {
    s.rec.reqs.lock().unwrap().len()
}

/// What the model was asked last: the newest user message, where the
/// per-turn context (recalled memory) goes.
fn last_user_message(s: &Server) -> String {
    let reqs = s.rec.reqs.lock().unwrap();
    reqs.last()
        .unwrap()
        .messages
        .iter()
        .rev()
        .find_map(|m| match m {
            ChatMsg::User(u) => Some(u.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// Whether any request since the `from`th offered the model `remember`.
fn offered_remember_since(s: &Server, from: usize) -> bool {
    s.rec.reqs.lock().unwrap()[from..]
        .iter()
        .any(|r| r.tools.iter().any(|t| t.name == "remember"))
}

fn defaults(v: &Value) -> Vec<String> {
    v["models"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["default"] == true)
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect()
}

fn agent<'a>(v: &'a Value, id: &str) -> &'a Value {
    v["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == id)
        .unwrap_or_else(|| panic!("no agent {id}: {v}"))
}

#[tokio::test]
async fn models_agents_and_new_chats_follow_the_workspace() {
    let s = start().await;
    let (bob, _) = user(&s, "bob", true).await;
    let (carol, _) = user(&s, "carol", true).await;
    let acme = team(&s, &bob, "Acme").await;
    let personal = personal_id(&s, &bob).await;
    settings(
        &s,
        &bob,
        &acme,
        json!({"default_agent": "coder", "agent_models": {
            "general": {"model": "demo::fast", "fallbacks": ["demo::mock"]},
        }}),
    )
    .await;

    // General's chain is the workspace default model; the server's stays.
    let models_in = format!("/api/models?workspace={acme}");
    assert_eq!(defaults(&get(&s, &models_in, &bob).await), ["demo::fast"]);
    assert_eq!(
        defaults(&get(&s, "/api/models?scope=server", &bob).await),
        ["demo::mock"]
    );
    assert_eq!(
        defaults(&get(&s, "/api/models", &bob).await),
        ["demo::mock"],
        "the active workspace (Personal) by default"
    );
    let (st, _) = call(
        &s,
        Method::PUT,
        "/api/me/workspace",
        Some(json!({ "workspace_id": acme })),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        defaults(&get(&s, "/api/models", &bob).await),
        ["demo::fast"]
    );

    // Agents: the workspace's default agent and chains, or the server's.
    let a = get(&s, &format!("/api/agents?workspace={acme}"), &bob).await;
    assert_eq!(a["default"], "coder");
    let general = agent(&a, "general");
    assert_eq!(general["default_model"], "demo::fast");
    assert_eq!(general["main_model"], "demo::fast");
    assert_eq!(general["fallbacks"], json!(["demo::mock"]));
    assert_eq!(
        agent(&a, "scout")["main_model"],
        "demo::fast",
        "an agent without a chain runs on the default"
    );
    let a = get(&s, "/api/agents?scope=server", &bob).await;
    assert_eq!(a["default"], "general");
    assert_eq!(agent(&a, "general")["default_model"], Value::Null);
    assert_eq!(agent(&a, "general")["main_model"], "demo::mock");

    // Asking for both scopes, or another one, is a mistake; other people's
    // workspaces do not exist for you.
    for path in [
        format!("/api/models?workspace={acme}&scope=server"),
        "/api/agents?scope=everything".to_string(),
    ] {
        let (st, _) = call(&s, Method::GET, &path, None, &bob).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{path}");
    }
    let carols = personal_id(&s, &carol).await;
    for what in ["models", "agents", "notices"] {
        let path = format!("/api/{what}?workspace={carols}");
        let (st, _) = call(&s, Method::GET, &path, None, &bob).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{path}");
    }

    // A new chat there without a choice gets the workspace's agent and
    // model, and keeps following the defaults.
    let auto = |sid: &str| {
        s.master
            .inner_mirror()
            .kv_get(&format!("session-model-auto:{sid}"))
            .unwrap()
            .is_some()
    };
    let row = |sid: String| {
        let (s, bob) = (&s, bob.clone());
        async move { get(s, &format!("/api/sessions/{sid}"), &bob).await }
    };
    let sid = new_chat(&s, &bob, &acme, json!({})).await;
    let r = row(sid.clone()).await;
    assert_eq!(
        (&r["agent_id"], &r["model"]),
        (&json!("coder"), &json!("demo::fast"))
    );
    assert!(auto(&sid));
    // The model the composer shows there counts as no choice...
    let shown = new_chat(
        &s,
        &bob,
        &acme,
        json!({"agent_id": "general", "model": "demo::fast"}),
    )
    .await;
    assert!(auto(&shown));
    // ...another one is a choice.
    let pinned = new_chat(
        &s,
        &bob,
        &acme,
        json!({"agent_id": "general", "model": "demo::mock"}),
    )
    .await;
    assert!(!auto(&pinned));
    // Personal has the server's defaults.
    let home = new_chat(&s, &bob, &personal, json!({})).await;
    let r = row(home.clone()).await;
    assert_eq!(
        (&r["agent_id"], &r["model"]),
        (&json!("general"), &json!("demo::mock"))
    );
    assert!(auto(&home));

    // Turns run on what the chat's workspace sets now.
    let model_of_turn = |sid: String, n: usize| {
        let (s, bob) = (&s, bob.clone());
        async move {
            let (st, v) = prompt(s, &bob, &sid, json!({"text": "hello"})).await;
            assert_eq!(st, StatusCode::OK, "{v}");
            nth_event(s, &bob, &sid, "turn_ended", n).await;
            s.rec.reqs.lock().unwrap().last().unwrap().model.clone()
        }
    };
    assert_eq!(model_of_turn(shown.clone(), 1).await, "demo::fast");
    assert_eq!(model_of_turn(pinned.clone(), 1).await, "demo::mock");
    settings(
        &s,
        &bob,
        &acme,
        json!({"agent_models": {"general": {"model": "demo::mock", "fallbacks": ["demo::fast"]}}}),
    )
    .await;
    assert_eq!(model_of_turn(shown, 2).await, "demo::mock", "it follows");
    let spec = s
        .master
        .resolver_for(Some(&acme))
        .await
        .agent_spec_with(None, None, None)
        .unwrap();
    assert_eq!(
        (spec.agent_id.as_str(), spec.model.as_deref()),
        ("general", Some("demo::mock")),
        "no default agent any more: the server's"
    );
    assert_eq!(spec.fallbacks, ["demo::fast"]);
    // The server-scope helpers are unchanged.
    let spec = s.master.agent_spec(None, None).unwrap();
    assert_eq!(spec.model.as_deref(), Some("demo::mock"));
    assert!(spec.fallbacks.is_empty());
}

#[tokio::test]
async fn stale_workspace_settings_are_dropped_when_read() {
    let s = start().await;
    let (bob, _) = user(&s, "bob", false).await;
    let acme = team(&s, &bob, "Acme").await;
    settings(
        &s,
        &bob,
        &acme,
        json!({"default_agent": "coder", "agent_models": {
            "general": {"model": "demo::fast", "fallbacks": ["demo::mock"]},
            "coder": {"model": "demo::mock", "fallbacks": ["demo::fast"]},
        }}),
    )
    .await;
    // A server admin removes a model the workspace names.
    let (st, v) = call(
        &s,
        Method::DELETE,
        "/api/admin/providers/demo/models/fast",
        None,
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let path = format!("/api/workspaces/{acme}");
    let d = get(&s, &path, &bob).await;
    assert_eq!(
        d["settings"],
        json!({"default_agent": "coder", "agent_models": {
            "coder": {"model": "demo::mock", "fallbacks": []},
        }})
    );
    // The stale chain is not in effect...
    assert_eq!(
        defaults(&get(&s, &format!("/api/models?workspace={acme}"), &bob).await),
        ["demo::mock"]
    );
    // ...and what was read saves as it is.
    let (st, v) = call(
        &s,
        Method::PATCH,
        &path,
        Some(json!({ "settings": d["settings"] })),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["settings"], d["settings"]);
}

#[tokio::test]
async fn agentic_work_stops_when_access_goes() {
    let s = start().await;
    let (bob, _) = user(&s, "bob", true).await;
    let (carol, carol_id) = user(&s, "carol", true).await;
    let acme = team(&s, &bob, "Acme").await;
    add(&s, &bob, &acme, "carol", "member").await;
    let sid = new_chat(&s, &carol, &acme, json!({})).await;
    let set_role = |role: &str| {
        let (s, bob, path) = (
            &s,
            bob.clone(),
            format!("/api/workspaces/{acme}/members/{carol_id}"),
        );
        let body = json!({ "role": role });
        async move {
            let (st, v) = call(s, Method::PATCH, &path, Some(body), &bob).await;
            assert_eq!(st, StatusCode::OK, "{v}");
        }
    };

    // A turn waits for Carol to approve a command when she becomes a
    // guest: it is cancelled, and her chat is read-only from then on.
    let (st, v) = prompt(&s, &carol, &sid, json!({"text": "run: echo hi"})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let asked = nth_event(&s, &carol, &sid, "approval_requested", 1).await;
    let approval = asked["payload"]["approval_id"]
        .as_str()
        .unwrap()
        .to_string();
    set_role("guest").await;
    wait_stopped(&s, &carol, &sid).await;
    let why = "your role in Acme cannot run agentic sessions";
    let r = get(&s, &format!("/api/sessions/{sid}"), &carol).await;
    assert_eq!(r["blocked"], why);

    let r = reqwest::Client::new()
        .post(format!("{}/api/attachments?name=notes.txt", s.base))
        .header("cookie", &carol)
        .body("tram timetable".as_bytes().to_vec())
        .send()
        .await
        .unwrap();
    let upload = r.json::<Value>().await.unwrap()["attachment_id"].clone();
    let (st, v) = prompt(
        &s,
        &carol,
        &sid,
        json!({"text": "again", "attachments": [upload]}),
    )
    .await;
    assert_eq!(
        (st, v["error"].as_str()),
        (StatusCode::FORBIDDEN, Some(why))
    );
    let answer = |decision: &str| {
        let (s, carol) = (&s, carol.clone());
        let path = format!("/api/sessions/{sid}/approvals/{approval}");
        let body = json!({ "decision": decision });
        async move { call(s, Method::POST, &path, Some(body), &carol).await }
    };
    for decision in ["allow", "allow_session"] {
        let (st, v) = answer(decision).await;
        assert_eq!(
            (st, v["error"].as_str()),
            (StatusCode::FORBIDDEN, Some(why))
        );
    }
    // Refusing stays hers: it reaches the node (which has nothing pending
    // any more).
    let (st, v) = answer("deny").await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("pending"), "{v}");
    let (st, v) = call(
        &s,
        Method::POST,
        &format!("/api/sessions/{sid}/turns/t1/restore"),
        Some(json!({})),
        &carol,
    )
    .await;
    assert_eq!(
        (st, v["error"].as_str()),
        (StatusCode::FORBIDDEN, Some(why))
    );

    // A member again: the refused prompt did not use up her upload.
    set_role("member").await;
    let (st, v) = prompt(
        &s,
        &carol,
        &sid,
        json!({"text": "again", "attachments": [upload]}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    nth_event(&s, &carol, &sid, "turn_ended", 2).await;

    // Without the server grant: a running turn stops, and nothing more runs.
    let (st, v) = prompt(&s, &carol, &sid, json!({"text": "run: echo again"})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    nth_event(&s, &carol, &sid, "approval_requested", 2).await;
    let (st, v) = call(
        &s,
        Method::PATCH,
        &format!("/api/users/{carol_id}"),
        Some(json!({"can_agentic": false})),
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    wait_stopped(&s, &carol, &sid).await;
    let grant = "agentic sessions are not enabled for this account; ask an admin";
    let (st, v) = prompt(&s, &carol, &sid, json!({"text": "hello"})).await;
    assert_eq!(
        (st, v["error"].as_str()),
        (StatusCode::FORBIDDEN, Some(grant))
    );
    let r = get(&s, &format!("/api/sessions/{sid}"), &carol).await;
    assert_eq!(r["blocked"], grant);
}

/// Disabling an account, or deleting it, stops its running agentic turns
/// (here, ones waiting for an approval that would never come).
#[tokio::test]
async fn disabled_or_deleted_accounts_stop_their_agentic_work() {
    let s = start().await;
    let (bob, _) = user(&s, "bob", true).await;
    let acme = team(&s, &bob, "Acme").await;
    for (name, method) in [("carol", Method::PATCH), ("dave", Method::DELETE)] {
        let (cookie, id) = user(&s, name, true).await;
        add(&s, &bob, &acme, name, "member").await;
        let sid = new_chat(&s, &cookie, &acme, json!({})).await;
        let (st, v) = prompt(&s, &cookie, &sid, json!({"text": "run: echo hi"})).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        nth_event(&s, &cookie, &sid, "approval_requested", 1).await;
        let body = (method == Method::PATCH).then(|| json!({"disabled": true}));
        let (st, v) = call(
            &s,
            method.clone(),
            &format!("/api/users/{id}"),
            body,
            &s.admin,
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{method}: {v}");
        // Their cookie went with the account, so ask the store.
        wait_stopped_in_store(&s, &sid).await;
    }
}

/// A new chat is announced again once it has its owner and workspace: the
/// first `sessions_changed` (its `session_created`) goes out before the
/// master stamps it, so a UI refetching then would not list it yet.
#[tokio::test]
async fn creating_a_chat_announces_it_after_stamping() {
    let s = start().await;
    let (bob, _) = user(&s, "bob", true).await;
    let acme = team(&s, &bob, "Acme").await;
    let mut live = s.master.subscribe();
    let sid = new_chat(&s, &bob, &acme, json!({})).await;
    // Everything sent before the create answered is waiting in `live`.
    let mut after_created = None;
    while let Ok(l) = live.try_recv() {
        match l {
            Live::Event(e) if e.kind == "session_created" && e.session_id.to_string() == sid => {
                after_created = Some(0)
            }
            Live::SessionsChanged => {
                if let Some(n) = after_created.as_mut() {
                    *n += 1;
                }
            }
            _ => {}
        }
    }
    assert_eq!(
        after_created,
        Some(2),
        "session_created's own, then the one after stamping"
    );
    let row = get(&s, &format!("/api/sessions/{sid}"), &bob).await;
    assert_eq!(row["workspace_id"], json!(acme));
}

#[tokio::test]
async fn the_memory_switch_keeps_memory_out_of_team_chats() {
    let s = start().await;
    let (bob, _) = user(&s, "bob", true).await;
    let acme = team(&s, &bob, "Acme").await;
    let personal = personal_id(&s, &bob).await;
    let (st, v) = call(
        &s,
        Method::POST,
        "/api/memory",
        Some(json!({"text": "Bob rides the number 7 tram to work"})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let memories = || {
        let (s, bob) = (&s, bob.clone());
        async move {
            get(s, "/api/memory", &bob).await["memories"]
                .as_array()
                .unwrap()
                .len()
        }
    };

    // Recalled in Personal and, until he says otherwise, in Acme.
    let ask = "which tram do I take?";
    for ws in [&personal, &acme] {
        turn(&s, &bob, ws, ask, "general").await;
        assert!(last_user_message(&s).contains("number 7 tram"), "{ws}");
    }

    // Bob keeps his memory out of Acme: no recall, no `remember` for the
    // agent or its sub-agents, and nothing filed from there.
    let (st, v) = call(
        &s,
        Method::PUT,
        &format!("/api/workspaces/{acme}/memory"),
        Some(json!({"use_memory": false})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let from = requests(&s);
    turn(&s, &bob, &acme, ask, "general").await;
    assert!(!last_user_message(&s).contains("number 7 tram"));
    turn(&s, &bob, &acme, "remember: Bob drinks tea", "general").await;
    turn(
        &s,
        &bob,
        &acme,
        "delegate: helper=remember: Bob drinks coffee",
        "boss",
    )
    .await;
    assert!(!offered_remember_since(&s, from));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(memories().await, 1, "nothing filed from Acme");

    // In Personal the same sub-agent proposes it (its session inherits the
    // parent's workspace and owner).
    let from = requests(&s);
    turn(
        &s,
        &bob,
        &personal,
        "delegate: helper=remember: Bob drinks coffee",
        "boss",
    )
    .await;
    assert!(offered_remember_since(&s, from));
    for _ in 0..100 {
        if memories().await == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(memories().await, 2);

    // Switched back on, Acme recalls again.
    call(
        &s,
        Method::PUT,
        &format!("/api/workspaces/{acme}/memory"),
        Some(json!({"use_memory": true})),
        &bob,
    )
    .await;
    turn(&s, &bob, &acme, ask, "general").await;
    assert!(last_user_message(&s).contains("number 7 tram"));
}

#[tokio::test]
async fn notices_follow_the_workspace() {
    let s = start().await;
    let (bob, _) = user(&s, "bob", false).await;
    let (carol, _) = user(&s, "carol", false).await;
    let (dave, _) = user(&s, "dave", false).await;
    let acme = team(&s, &bob, "Acme").await;
    add(&s, &bob, &acme, "carol", "admin").await;
    add(&s, &bob, &acme, "dave", "member").await;
    let notices = |who: &str, q: String| {
        let (s, who) = (&s, who.to_string());
        async move {
            get(s, &format!("/api/notices{q}"), &who).await["notices"]
                .as_array()
                .unwrap()
                .clone()
        }
    };
    let here = format!("?workspace={acme}");

    // Acme's Coder falls back to a model its provider dropped: Acme's
    // admins hear of it and may fix it there; the server is not involved.
    settings(
        &s,
        &carol,
        &acme,
        json!({"agent_models": {"coder": {"model": "demo::mock", "fallbacks": ["demo::old"]}}}),
    )
    .await;
    let n = notices(&carol, here.clone()).await;
    assert_eq!(n.len(), 1, "{n:?}");
    assert_eq!(n[0]["model"], "demo::old");
    assert_eq!(n[0]["agents"], json!(["coder"]));
    assert_eq!(
        (
            &n[0]["workspace"],
            &n[0]["can_fix_workspace"],
            &n[0]["can_fix"],
            &n[0]["global_default"],
            &n[0]["workspace_default"]
        ),
        (
            &json!(true),
            &json!(true),
            &json!(false),
            &json!(false),
            &json!(false)
        )
    );
    assert!(
        notices(&dave, here.clone()).await.is_empty(),
        "a fallback is not a member's business"
    );
    assert!(notices(&carol, "?scope=server".into()).await.is_empty());
    assert!(
        notices(&carol, String::new()).await.is_empty(),
        "her active workspace is Personal"
    );

    // As Acme's default model it touches every member there; it is Acme's
    // default, not the server's.
    settings(
        &s,
        &bob,
        &acme,
        json!({"agent_models": {"general": {"model": "demo::old"}}}),
    )
    .await;
    let n = notices(&dave, here.clone()).await;
    assert_eq!(n.len(), 1, "{n:?}");
    assert_eq!(
        (
            &n[0]["model"],
            &n[0]["workspace_default"],
            &n[0]["global_default"],
            &n[0]["workspace"],
            &n[0]["can_fix_workspace"]
        ),
        (
            &json!("demo::old"),
            &json!(true),
            &json!(false),
            &json!(true),
            &json!(false)
        )
    );
    assert!(notices(&dave, "?scope=server".into()).await.is_empty());

    // The server default stays the server's where Acme only names it as a
    // fallback: a server admin fixes it, Acme's admins may drop the
    // fallback.
    settings(
        &s,
        &bob,
        &acme,
        json!({"agent_models": {"coder": {"model": "demo::mock", "fallbacks": ["demo::old"]}}}),
    )
    .await;
    let (st, v) = call(
        &s,
        Method::PUT,
        "/api/admin/agent-models",
        Some(json!({"agent_models": {}, "default_model": "demo::old"})),
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let n = notices(&carol, here.clone()).await;
    assert_eq!(n.len(), 1, "{n:?}");
    assert_eq!(
        (
            &n[0]["global_default"],
            &n[0]["workspace_default"],
            &n[0]["workspace"],
            &n[0]["can_fix_workspace"]
        ),
        (&json!(true), &json!(false), &json!(true), &json!(true))
    );
    let n = notices(&dave, here.clone()).await;
    assert_eq!(n[0]["global_default"], true, "{n:?}");
}
