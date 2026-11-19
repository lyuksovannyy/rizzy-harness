//! A person's message that calls several agents (ADOPTION_PLAN X.37,
//! README "Calling several agents"), through the master and a real node:
//! the owner's own examples, who is called (the chat's own workspace set
//! only), who is not (a plain mention, another person's name, the API
//! token, an explicit agent), the limit, a node that cannot, the size guard.

mod common;
use common::{Login as _, SameOrigin as _};

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_agent_native::{
    ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError, mock::Mock,
};
use harness_core::event::{EventKind, TurnCause};
use harness_core::{RawEvent, Secret};
use harness_master::auth::{Auth, Claim};
use harness_master::config::{Config, ModelConfig};
use harness_master::providers::ProviderRuntime;
use harness_master::{Master, router};
use harness_memory::SqliteMemory;
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

/// Records every request and answers as the mock does.
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
    _master: Master,
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
        _master: master,
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

/// The chat's events.
async fn log(s: &Server, who: &str, sid: &str) -> Vec<RawEvent> {
    let v = get(s, &format!("/api/sessions/{sid}/events"), who).await;
    serde_json::from_value(v["events"].clone()).unwrap()
}

/// The agents of the turns in `evs`, in order, with whether each turn was
/// started as a mention turn.
fn who_answered(evs: &[RawEvent]) -> Vec<(String, bool)> {
    evs.iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::TurnStarted(t)) => Some((
                t.agent.map(|a| a.agent_id).unwrap_or_default(),
                matches!(t.cause, Some(TurnCause::Mention(_))),
            )),
            _ => None,
        })
        .collect()
}

/// The sub-agent-only names the last message mentioned (its tags).
fn tags_of(evs: &[RawEvent]) -> Vec<String> {
    evs.iter()
        .rev()
        .find_map(|e| match e.typed() {
            Ok(EventKind::UserMessage(m)) if !m.queued => Some(m.tags),
            _ => None,
        })
        .unwrap_or_default()
}

fn triggers_of(evs: &[RawEvent]) -> Vec<Vec<String>> {
    evs.iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::UserMessage(m)) => Some(m.triggers),
            _ => None,
        })
        .collect()
}

/// Send `text` as the person and wait for `n` more turns to end.
async fn ask(s: &Server, who: &str, sid: &str, body: Value, n: usize) -> Vec<RawEvent> {
    let before = log(s, who, sid)
        .await
        .iter()
        .filter(|e| e.kind == "turn_ended")
        .count();
    let (st, v) = prompt(s, who, sid, body).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    turns_ended(s, who, sid, before + n).await;
    // Nothing else starts afterwards.
    tokio::time::sleep(Duration::from_millis(150)).await;
    let evs = log(s, who, sid).await;
    assert_eq!(
        evs.iter().filter(|e| e.kind == "turn_ended").count(),
        before + n
    );
    evs
}

fn last_turns(evs: &[RawEvent], n: usize) -> Vec<(String, bool)> {
    let all = who_answered(evs);
    all[all.len() - n..].to_vec()
}

fn pair(a: &str, mention: bool) -> (String, bool) {
    (a.to_string(), mention)
}

/// The owner's own examples, as written, end to end: the opening run calls
/// both; the closing line (with the owner's own name in it) calls both; a
/// mention in the middle of a sentence calls nobody; one that is also in the
/// closing line calls that agent alone.
#[tokio::test]
async fn the_owners_four_examples_end_to_end() {
    let s = start().await;
    let ws = personal_id(&s, &s.admin).await;
    let sid = chat(&s, &s.admin, &ws, json!({"agent_id": "general"})).await;
    // 1. Opening run, text after it on the same line.
    let evs = ask(
        &s,
        &s.admin,
        &sid,
        json!({"text": "@coder @writer, yo how it is goin?"}),
        2,
    )
    .await;
    assert_eq!(
        last_turns(&evs, 2),
        [pair("coder", false), pair("writer", true)]
    );
    assert_eq!(triggers_of(&evs), [["coder", "writer"]]);
    // 2. Closing line; `@alex` is the person: no turn.
    let evs = ask(
        &s,
        &s.admin,
        &sid,
        json!({"text": "have anybody seen the gta6 release?\n@coder @writer @alex"}),
        2,
    )
    .await;
    assert_eq!(
        last_turns(&evs, 2),
        [pair("coder", false), pair("writer", true)]
    );
    // 3. A plain mention: the chat's own agent answers.
    let evs = ask(
        &s,
        &s.admin,
        &sid,
        json!({"text": "my guy, the @coder have done good work, give him a raise"}),
        1,
    )
    .await;
    assert_eq!(last_turns(&evs, 1), [pair("general", false)]);
    // 4. Plain in the middle, called in the closing line: that agent alone,
    //    exactly as a single mention.
    let evs = ask(
        &s,
        &s.admin,
        &sid,
        json!({"text": "my guy, the @coder have done good work, give him a raise\n@coder @alex"}),
        1,
    )
    .await;
    assert_eq!(last_turns(&evs, 1), [pair("coder", false)]);
    assert_eq!(triggers_of(&evs).last().unwrap().len(), 0);
    // Each later agent's request holds the earlier agent's answer and its
    // own opening line.
    let reqs = s.rec.reqs.lock().unwrap().clone();
    let w = reqs
        .iter()
        .find(|r| format!("{:?}", r.messages).contains(harness_core::event::CALLED_LEAD))
        .expect("a mention turn was asked");
    assert!(format!("{:?}", w.messages).contains("You are @writer."));
}

/// Another person's name, an unknown name, a sub-agent-only agent, the
/// API token's message, an explicit agent and an answer to a card call
/// nobody extra; the limit refuses before anything is sent.
#[tokio::test]
async fn who_is_not_called_and_the_limit() {
    let s = start().await;
    let ws = personal_id(&s, &s.admin).await;
    let sid = chat(&s, &s.admin, &ws, json!({"agent_id": "general"})).await;
    user(&s, "bob").await;
    // Another person's login is text: the run ends at it.
    let evs = ask(&s, &s.admin, &sid, json!({"text": "@bob @coder hi"}), 1).await;
    assert_eq!(last_turns(&evs, 1), [pair("general", false)]);
    // Unknown between two known: the run ends at the first unknown.
    let evs = ask(
        &s,
        &s.admin,
        &sid,
        json!({"text": "@coder @nobody @writer hi"}),
        1,
    )
    .await;
    assert_eq!(last_turns(&evs, 1), [pair("coder", false)]);
    // A sub-agent-only agent is a tag (the owner, 2026-11-19): mentioned,
    // recorded on the message, never called, in any position; the run goes
    // on past it.
    let evs = ask(&s, &s.admin, &sid, json!({"text": "@coder @scout hi"}), 1).await;
    assert_eq!(last_turns(&evs, 1), [pair("coder", false)]);
    assert_eq!(tags_of(&evs), ["scout"]);
    let evs = ask(&s, &s.admin, &sid, json!({"text": "@scout @coder hi"}), 1).await;
    assert_eq!(last_turns(&evs, 1), [pair("coder", false)]);
    assert_eq!(tags_of(&evs), ["scout"]);
    // Only tags where a call would be: nobody is called, the chat's own
    // agent answers (not an error, as a leading sub-agent used to be).
    let evs = ask(&s, &s.admin, &sid, json!({"text": "@scout hi\n@scout"}), 1).await;
    assert_eq!(last_turns(&evs, 1), [pair("general", false)]);
    assert_eq!(tags_of(&evs), ["scout"]);
    // The API token keeps its refusal of a leading sub-agent.
    let (st, v) = token(
        &s,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({"text": "@scout hi"})),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    // An explicit agent (the picker) wins, calls nobody.
    let evs = ask(
        &s,
        &s.admin,
        &sid,
        json!({"text": "@coder @writer hi", "agent_id": "smol"}),
        1,
    )
    .await;
    assert_eq!(last_turns(&evs, 1), [pair("smol", false)]);
    // The API token: today's single leading mention, the rest is text.
    let (st, v) = token(
        &s,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({"text": "@coder @writer hi"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    turns_ended(&s, &s.admin, &sid, 7).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    let evs = log(&s, &s.admin, &sid).await;
    assert_eq!(last_turns(&evs, 1), [pair("coder", false)]);
    assert_eq!(evs.iter().filter(|e| e.kind == "turn_ended").count(), 7);
    // Five agents: refused with the limit, nothing sent.
    let before = s.rec.reqs.lock().unwrap().len();
    let n = log(&s, &s.admin, &sid).await.len();
    let (st, v) = prompt(
        &s,
        &s.admin,
        &sid,
        json!({"text": "@general @smol @researcher @coder @writer go"}),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    let e = v["error"].as_str().unwrap();
    assert!(
        e.contains("at most 4") && e.contains("Mention fewer"),
        "{v}"
    );
    assert_eq!(log(&s, &s.admin, &sid).await.len(), n);
    assert_eq!(s.rec.reqs.lock().unwrap().len(), before);
    // Four are fine.
    let evs = ask(
        &s,
        &s.admin,
        &sid,
        json!({"text": "@general @smol @researcher @coder go"}),
        4,
    )
    .await;
    assert_eq!(
        last_turns(&evs, 4),
        [
            pair("general", false),
            pair("smol", true),
            pair("researcher", true),
            pair("coder", true)
        ]
    );
}

/// The chat's own workspace set decides: an agent another workspace made,
/// or one that was deleted, is text.
#[tokio::test]
async fn another_workspaces_or_a_removed_agent_is_text() {
    let s = start().await;
    let a = team(&s, &s.admin, "A").await;
    let b = team(&s, &s.admin, "B").await;
    let (st, v) = create(&s, &s.admin, &b, helper("helper", "You help.")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let in_a = chat(&s, &s.admin, &a, json!({"agent_id": "general"})).await;
    let in_b = chat(&s, &s.admin, &b, json!({"agent_id": "general"})).await;
    // In A there is no `helper`: the run ends at it, nothing is called.
    let evs = ask(&s, &s.admin, &in_a, json!({"text": "@helper @coder hi"}), 1).await;
    assert_eq!(last_turns(&evs, 1), [pair("general", false)]);
    let evs = ask(
        &s,
        &s.admin,
        &in_a,
        json!({"text": "hi\n@coder @helper"}),
        1,
    )
    .await;
    assert_eq!(last_turns(&evs, 1), [pair("general", false)]);
    // In B it is called.
    let evs = ask(&s, &s.admin, &in_b, json!({"text": "@helper @coder hi"}), 2).await;
    assert_eq!(
        last_turns(&evs, 2),
        [pair("helper", false), pair("coder", true)]
    );
    // Removed: text again.
    let (st, v) = call(
        &s,
        Method::DELETE,
        &format!("/api/agents/helper?workspace={b}"),
        None,
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let evs = ask(&s, &s.admin, &in_b, json!({"text": "@helper @coder hi"}), 1).await;
    assert_eq!(last_turns(&evs, 1), [pair("general", false)]);
}

// ---- a fake node, for what a real one cannot be made to do -------------

use harness_core::event::SessionCreated;
use harness_node::NodeLink;
use harness_protocol::{MasterToNode, NodeToMaster, PROTOCOL_VERSION};

type Seen = Arc<Mutex<Vec<MasterToNode>>>;

/// A node that opens chats, acks everything and keeps every command.
fn fake(caps: &[&str]) -> (NodeLink, Seen, mpsc::Sender<NodeToMaster>) {
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
        node_id: "fake".into(),
        hostname: "h".into(),
        version: "1".into(),
        capabilities,
        sessions: vec![],
    })
    .unwrap();
    let seen: Seen = Arc::default();
    let keep = seen.clone();
    let events = tx.clone();
    tokio::spawn(async move {
        while let Some(msg) = commands.recv().await {
            keep.lock().unwrap().push(msg.clone());
            if let MasterToNode::Create {
                session_id,
                exec_mode,
                ..
            } = &msg
            {
                let (kind, payload) = EventKind::SessionCreated(SessionCreated {
                    exec_mode: *exec_mode,
                    no_callbacks: false,
                    title: "t".into(),
                    cwd: "/w".into(),
                    model: "demo::mock".into(),
                    backend: "native".into(),
                    node_id: "fake".into(),
                    approval_mode: harness_core::policy::ApprovalMode::Ask,
                    profile: harness_core::event::Profile::Agentic,
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
            if let Some(req_id) = msg.req_id() {
                let _ = tx.try_send(NodeToMaster::Ack {
                    req_id: req_id.into(),
                    ok: true,
                    error: None,
                });
            }
        }
    });
    (NodeLink { to_node, from_node }, seen, events)
}

struct F {
    base: String,
    cookie: String,
    seen: Seen,
    master: Master,
    events: mpsc::Sender<NodeToMaster>,
    _dir: tempfile::TempDir,
}

async fn start_fake(caps: &[&str]) -> F {
    let dir = tempfile::tempdir().unwrap();
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
    let (link, seen, events) = fake(caps);
    master.add_link(link, true).await.unwrap();
    master.set_models(vec![
        harness_master::ModelInfo {
            id: "demo::mock".into(),
            provider: "demo".into(),
            default: true,
            ..Default::default()
        },
        harness_master::ModelInfo {
            id: "demo::other".into(),
            provider: "demo".into(),
            ..Default::default()
        },
    ]);
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
    let cookie = common::signed_in(r).await;
    F {
        base,
        cookie,
        seen,
        master,
        events,
        _dir: dir,
    }
}

impl F {
    async fn req(&self, method: Method, path: &str, body: Value) -> (StatusCode, Value) {
        let r = reqwest::Client::new()
            .request(method, format!("{}{path}", self.base))
            .json(&body)
            .login(&self.cookie)
            .same_origin()
            .send()
            .await
            .unwrap();
        let st = r.status();
        (st, r.json().await.unwrap_or(Value::Null))
    }

    async fn chat(&self) -> String {
        let (st, v) = self
            .req(
                Method::POST,
                "/api/sessions",
                json!({"cwd": "/w", "title": "t", "node_id": "fake", "agent_id": "general"}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "{v}");
        v["session_id"].as_str().unwrap().to_string()
    }

    async fn say(&self, sid: &str, text: &str) -> (StatusCode, Value) {
        self.req(
            Method::POST,
            &format!("/api/sessions/{sid}/prompt"),
            json!({"text": text}),
        )
        .await
    }

    fn prompts(&self) -> Vec<(String, Vec<String>, usize)> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                MasterToNode::Prompt { agent, peers, .. } => Some((
                    agent
                        .as_ref()
                        .map(|a| a.agent_id.clone())
                        .unwrap_or_default(),
                    peers.iter().map(|p| p.agent_id.clone()).collect(),
                    serde_json::to_string(m).unwrap().len(),
                )),
                _ => None,
            })
            .collect()
    }
}

/// A node that did not announce `peers` is never sent the field (it would
/// answer with the first agent alone): the message is refused with a plain
/// sentence, nothing sent; one agent per message works as before.
#[tokio::test]
async fn a_node_without_the_capability_is_refused_the_list() {
    let f = start_fake(&[]).await;
    let sid = f.chat().await;
    let (st, v) = f.say(&sid, "@coder @writer hi").await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("too old"), "{v}");
    assert!(f.prompts().is_empty());
    let (st, v) = f.say(&sid, "@coder hi").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        f.prompts(),
        [("coder".to_string(), vec![], f.prompts()[0].2)]
    );
}

/// A node that does announce it gets the first agent as `agent` and the
/// others, in order, as `peers`, with no tool the harness's own turns may
/// not have; with four agents at realistic caps (a long prompt, the delegate
/// list) the command is well under the link's guard, and four agents whose
/// prompts are as large as a request may be are refused by the guard with
/// its plain sentence before anything is sent.
#[tokio::test]
async fn the_list_goes_in_order_and_the_size_guard_holds_with_four_specs() {
    let f = start_fake(&[harness_protocol::CAP_PEERS]).await;
    let sid = f.chat().await;
    let (st, v) = f.say(&sid, "@teamlead @coder @writer @researcher go").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let p = f.prompts();
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].0, "teamlead");
    assert_eq!(p[0].1, ["coder", "writer", "researcher"]);
    assert!(p[0].2 < 256 * 1024, "{} bytes", p[0].2);
    // Five agents are refused by the master itself, before anything is sent.
    let before = f.prompts().len();
    let (st, v) = f
        .say(&sid, "@teamlead @coder @writer @researcher @smol go")
        .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"].as_str().unwrap().contains("Mention fewer"),
        "{v}"
    );
    assert_eq!(f.prompts().len(), before);
    {
        let seen = f.seen.lock().unwrap();
        let MasterToNode::Prompt { peers, .. } = seen
            .iter()
            .rev()
            .find(|m| matches!(m, MasterToNode::Prompt { .. }))
            .unwrap()
        else {
            unreachable!()
        };
        for p in peers {
            assert!(p.connector_tools.is_empty());
            for t in ["remember", "recall"] {
                assert!(
                    p.deny_tools.iter().any(|d| d == t),
                    "{t} for {}",
                    p.agent_id
                );
            }
        }
    }
    // Four workspace agents at the largest an agent may be (a stored
    // agent is at most 64 KiB; its prompt at most 20,000 characters): the
    // command with all four is far under the link's guard.
    let ws = {
        let (_, v) = f.req(Method::GET, "/api/workspaces", json!(null)).await;
        v["workspaces"][0]["workspace_id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let big = "x".repeat(20_000);
    for id in ["big-a", "big-b", "big-c", "big-d"] {
        let (st, v) = f
            .req(
                Method::POST,
                &format!("/api/agents?workspace={ws}"),
                json!({"id": id, "name": id, "prompt": big, "features": {
                    "memory": false, "web": false, "read_files": true, "edit_files": false,
                    "commands": false, "delegate": false, "as_agent": true, "as_subagent": false}}),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "{id}: {v}");
    }
    let (st, v) = f
        .req(
            Method::POST,
            "/api/sessions",
            json!({"cwd": "/w", "title": "t", "node_id": "fake", "workspace_id": ws, "agent_id": "general"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap().to_string();
    let (st, v) = f.say(&sid, "@big-a @big-b @big-c @big-d go").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let last = f.prompts().pop().unwrap();
    assert_eq!(last.1, ["big-b", "big-c", "big-d"]);
    assert!(
        last.2 > 4 * 20_000 && last.2 < 1024 * 1024,
        "{} bytes",
        last.2
    );
}

/// A message with more than 16 resolved mentions is refused with a
/// sentence, before anything is sent. (A definition must be available as an
/// agent, a sub-agent or both, so a name that is neither does not exist to
/// be mentioned; the resolver maps it to text all the same.)
#[tokio::test]
async fn resolved_mentions_are_bounded() {
    let s = start().await;
    let ws = personal_id(&s, &s.admin).await;
    let sid = chat(&s, &s.admin, &ws, json!({"agent_id": "general"})).await;
    // 17 mentions of one agent, in the middle of text: refused.
    let n = log(&s, &s.admin, &sid).await.len();
    let many = format!("{} go", "see @coder and ".repeat(17));
    let (st, v) = prompt(&s, &s.admin, &sid, json!({ "text": many })).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("at most 16"), "{v}");
    assert_eq!(log(&s, &s.admin, &sid).await.len(), n);
    let ok = format!("{} go", "see @coder and ".repeat(16));
    let evs = ask(&s, &s.admin, &sid, json!({ "text": ok }), 1).await;
    assert_eq!(last_turns(&evs, 1), [pair("general", false)]);
}

/// The master's gate (review of PR 166, M3): a multi-mention message calls
/// several agents only from the chat owner's own signed-in message, and every
/// other way a message reaches a chat answers as a message with one leading
/// mention always did. The node here announces no `peers`, so a gate term
/// that lets a message through as a list shows as a refusal ("too old"),
/// and the command that reached the node must carry no `peers`. Each row
/// breaks if its own term of the gate is removed.
#[tokio::test]
async fn every_term_of_the_gate_keeps_a_message_to_its_first_mention() {
    use harness_core::event::FormReply;
    use harness_master::PromptOpts;
    let f = start_fake(&[]).await;
    let sid_s = f.chat().await;
    let sid: harness_core::SessionId = sid_s.parse().unwrap();
    let by_owner = || PromptOpts {
        by_owner: true,
        ..Default::default()
    };
    let text = "@coder @writer hi";
    let cases: Vec<(&str, PromptOpts)> = vec![
        ("the API token (not signed in)", PromptOpts::default()),
        (
            "a run of a routine",
            PromptOpts {
                routine_run: Some("run-1".into()),
                ..by_owner()
            },
        ),
        (
            "a chat that holds an automation's event text",
            PromptOpts {
                untrusted: true,
                ..by_owner()
            },
        ),
        (
            "an answer to a card",
            PromptOpts {
                reply: Some(FormReply {
                    message_id: "m1".into(),
                    block_id: "b1".into(),
                    values: Default::default(),
                }),
                ..by_owner()
            },
        ),
        (
            "an explicitly named agent",
            PromptOpts {
                agent_id: Some("coder".into()),
                ..by_owner()
            },
        ),
        (
            "a Retry",
            PromptOpts {
                retry: Some("t1".into()),
                ..by_owner()
            },
        ),
        (
            "parallel answers",
            PromptOpts {
                compare: vec!["demo::other".into()],
                ..by_owner()
            },
        ),
    ];
    for (what, opts) in cases {
        let before = f.seen.lock().unwrap().len();
        let r = f.master.prompt(sid, text.into(), opts).await;
        assert!(r.is_ok(), "{what}: refused: {r:?}");
        let seen = f.seen.lock().unwrap();
        let sent: Vec<&MasterToNode> = seen[before..]
            .iter()
            .filter(|m| matches!(m, MasterToNode::Prompt { .. } | MasterToNode::Retry { .. }))
            .collect();
        assert_eq!(sent.len(), 1, "{what}");
        match sent[0] {
            MasterToNode::Prompt { agent, peers, .. } => {
                assert!(peers.is_empty(), "{what}: peers were sent");
                assert!(agent.is_some(), "{what}");
            }
            MasterToNode::Retry { .. } => {}
            _ => unreachable!(),
        }
    }
    // A sub-agent's chat: a node-made chat whose parent is this one.
    let child = harness_core::SessionId::new();
    let (kind, payload) = EventKind::SessionCreated(SessionCreated {
        exec_mode: Some(harness_core::policy::ExecMode::Sandbox),
        no_callbacks: false,
        title: "c".into(),
        cwd: "/w".into(),
        model: "demo::mock".into(),
        backend: "native".into(),
        node_id: "fake".into(),
        approval_mode: harness_core::policy::ApprovalMode::Ask,
        profile: harness_core::event::Profile::Agentic,
        agent_id: Some("coder".into()),
        incognito: false,
        parent_session: Some(sid),
    })
    .into_parts();
    f.events
        .send(NodeToMaster::Event {
            event: RawEvent {
                session_id: child,
                seq: 1,
                ts_ms: harness_core::now_ms(),
                kind,
                payload,
            },
        })
        .await
        .unwrap();
    for _ in 0..200 {
        if f.master
            .inner_mirror()
            .session(child)
            .ok()
            .flatten()
            .is_some()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let before = f.seen.lock().unwrap().len();
    let r = f.master.prompt(child, text.into(), by_owner()).await;
    assert!(r.is_ok(), "a sub-agent's chat: refused: {r:?}");
    let seen = f.seen.lock().unwrap();
    let sent = seen[before..]
        .iter()
        .find_map(|m| match m {
            MasterToNode::Prompt { peers, .. } => Some(peers.len()),
            _ => None,
        })
        .expect("a prompt reached the node");
    assert_eq!(sent, 0, "a sub-agent's chat: peers were sent");
    // And the one term that may call several: the owner's own message.
    drop(seen);
    let r = f.master.prompt(sid, text.into(), by_owner()).await;
    assert!(
        format!("{r:?}").contains("too old"),
        "the owner's own message is the list: {r:?}"
    );
}
