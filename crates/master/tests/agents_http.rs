//! Agents and the chat runtime over HTTP.

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::event::{EventKind, Profile};
use harness_core::{RawEvent, Secret};
use harness_master::auth::Auth;
use harness_master::{Master, ModelInfo, router};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;
use reqwest::StatusCode;
use serde_json::{Value, json};

async fn runtime(root: &std::path::Path, id: &str, profile: Profile) -> NodeRuntime {
    let tools = match profile {
        Profile::Agentic => harness_tools::builtin(),
        Profile::Chat => Vec::new(),
    };
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
            tools,
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: vec![],
        profile,
    })
    .await
    .unwrap()
}

struct T {
    base: String,
    master: Master,
    _dir: tempfile::TempDir,
    ws: String,
}

async fn start(with_worker: bool) -> T {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let master = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false),
    );
    master.set_models(vec![ModelInfo {
        id: "mock".into(),
        provider: "mock".into(),
        effort: false,
        vision: false,
        default: true,
    }]);
    let chat = runtime(dir.path(), "master", Profile::Chat).await;
    master.add_link(chat.attach().await, true).await.unwrap();
    if with_worker {
        let node = runtime(dir.path(), "local", Profile::Agentic).await;
        master.add_link(node.attach().await, true).await.unwrap();
    }
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        master,
        ws: ws.display().to_string(),
        _dir: dir,
    }
}

async fn call(t: &T, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let c = reqwest::Client::new();
    let mut rb = match method {
        "GET" => c.get(format!("{}{path}", t.base)),
        _ => c.post(format!("{}{path}", t.base)),
    }
    .bearer_auth("tok");
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    let r = rb.send().await.unwrap();
    (r.status(), r.json().await.unwrap_or(Value::Null))
}

async fn events_until(t: &T, sid: &str, pred: impl Fn(&[RawEvent]) -> bool) -> Vec<RawEvent> {
    for _ in 0..300 {
        let (_, v) = call(t, "GET", &format!("/api/sessions/{sid}/events"), None).await;
        let evs: Vec<RawEvent> = serde_json::from_value(v["events"].clone()).unwrap();
        if pred(&evs) {
            return evs;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("condition not reached");
}

fn turns_ended(evs: &[RawEvent]) -> usize {
    evs.iter().filter(|e| e.kind == "turn_ended").count()
}

#[tokio::test]
async fn chat_sessions_agents_and_mentions() {
    let t = start(true).await;
    let (st, v) = call(&t, "GET", "/api/agents", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["default"], "general");
    let ids: Vec<&str> = v["agents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["general", "coder", "writer", "scout"]);
    assert!(
        v["agents"][0].get("spec").is_none(),
        "prompts are not exposed in the list"
    );

    // Chat session: no cwd needed, lands on the master's chat runtime.
    let (st, v) = call(
        &t,
        "POST",
        "/api/sessions",
        Some(json!({"profile": "chat"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap().to_string();
    let (_, row) = call(&t, "GET", &format!("/api/sessions/{sid}"), None).await;
    assert_eq!(row["node_id"], "master");
    assert_eq!(row["profile"], "chat");
    assert_eq!(row["agent_id"], "general");

    // A chat session cannot run shell commands, whatever the model asks.
    call(
        &t,
        "POST",
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({"text": "run: echo hi"})),
    )
    .await;
    let evs = events_until(&t, &sid, |e| turns_ended(e) == 1).await;
    assert!(evs.iter().any(|e| matches!(e.typed(),
        Ok(EventKind::ToolCallFinished(f)) if !f.ok && f.output.contains("unknown tool"))));

    // @writer routes one turn; the session agent stays General.
    call(
        &t,
        "POST",
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({"text": "@writer draft it"})),
    )
    .await;
    let evs = events_until(&t, &sid, |e| turns_ended(e) == 2).await;
    let started: Vec<_> = evs
        .iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::TurnStarted(s)) => s.agent.map(|a| a.agent_id),
            _ => None,
        })
        .collect();
    assert_eq!(started, ["general", "writer"]);
    assert!(evs.iter().any(|e| matches!(e.typed(),
        Ok(EventKind::AssistantMessage(m)) if m.author.as_deref() == Some("writer"))));
    let (_, row) = call(&t, "GET", &format!("/api/sessions/{sid}"), None).await;
    assert_eq!(row["agent_id"], "general");

    // Switching the session agent records agent_changed and sticks.
    let (st, _) = call(
        &t,
        "POST",
        &format!("/api/sessions/{sid}/agent"),
        Some(json!({"agent_id": "scout"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    events_until(&t, &sid, |e| e.iter().any(|x| x.kind == "agent_changed")).await;
    let (_, row) = call(&t, "GET", &format!("/api/sessions/{sid}"), None).await;
    assert_eq!(row["agent_id"], "scout");

    // Unknown agents are rejected; an unknown @mention is just text.
    let (st, _) = call(
        &t,
        "POST",
        &format!("/api/sessions/{sid}/agent"),
        Some(json!({"agent_id": "nope"})),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    call(
        &t,
        "POST",
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({"text": "@nobody hello"})),
    )
    .await;
    let evs = events_until(&t, &sid, |e| turns_ended(e) == 3).await;
    let last_agent = evs.iter().rev().find_map(|e| match e.typed() {
        Ok(EventKind::TurnStarted(s)) => s.agent.map(|a| a.agent_id),
        _ => None,
    });
    assert_eq!(last_agent.as_deref(), Some("scout"));

    // Agentic sessions go to the worker node.
    let (st, v) = call(
        &t,
        "POST",
        "/api/sessions",
        Some(json!({"cwd": t.ws, "agent_id": "coder"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, row) = call(
        &t,
        "GET",
        &format!("/api/sessions/{}", v["session_id"].as_str().unwrap()),
        None,
    )
    .await;
    assert_eq!(row["node_id"], "local");
    assert_eq!(row["agent_id"], "coder");
}

#[tokio::test]
async fn agentic_needs_a_worker_and_effort_is_gated() {
    let t = start(false).await;
    let (st, v) = call(&t, "POST", "/api/sessions", Some(json!({"cwd": t.ws}))).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE);
    assert!(v["error"].as_str().unwrap().contains("worker node"), "{v}");

    // Effort survives only for models that declare support.
    let mut agents = harness_agents::builtins();
    agents.push(
        harness_agents::parse(
            "thinker",
            "---\nname: Thinker\ntools: all\neffort: high\n---\nThink.\n",
            false,
        )
        .unwrap(),
    );
    t.master
        .set_agents(harness_agents::AgentSet::from_agents(agents, "general"));
    assert_eq!(
        t.master
            .agent_spec(Some("thinker"), Some("mock"))
            .unwrap()
            .effort,
        None
    );
    t.master.set_models(vec![ModelInfo {
        id: "mock".into(),
        provider: "mock".into(),
        effort: true,
        vision: false,
        default: true,
    }]);
    assert_eq!(
        t.master
            .agent_spec(Some("thinker"), Some("mock"))
            .unwrap()
            .effort
            .as_deref(),
        Some("high")
    );
}

#[test]
fn leading_mentions() {
    use harness_master::leading_mention;
    assert_eq!(leading_mention("@writer draft"), Some("writer"));
    assert_eq!(leading_mention("  @coder, fix"), Some("coder"));
    assert_eq!(leading_mention("email me @bob"), None);
    assert_eq!(leading_mention("@ nothing"), None);
}
