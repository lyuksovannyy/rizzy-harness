//! Where an agent may run (`available_as`), enforced by the server: an agent
//! that is only a sub-agent never runs a chat, whichever way it is named
//! (explicit `agent_id`, a leading `@mention`, `set_agent`, a new session),
//! and one that is only an agent is never handed a sub-task.

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_agents::{AgentSet, builtins, parse};
use harness_core::event::{EventKind, Profile};
use harness_core::{RawEvent, Secret};
use harness_master::auth::Auth;
use harness_master::{Master, ModelInfo, router};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;
use reqwest::StatusCode;
use serde_json::{Value, json};

async fn runtime(root: &std::path::Path) -> NodeRuntime {
    NodeRuntime::start(NodeConfig {
        node_id: "master".into(),
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
            Vec::new(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: Profile::Chat,
        incognito_ttl: None,
        checkpoints_dir: None,
        artifacts: None,
    })
    .await
    .unwrap()
}

/// The built-ins plus `helper` (sub-agent only), `lead` (agent only) and
/// `boss`, which names both as delegates.
fn agents(helper_as: &str) -> AgentSet {
    let mut all = builtins();
    let def = |id: &str, text: &str| parse(id, text, false).unwrap();
    all.push(def(
        "helper",
        &format!("---\nname: Helper\ntools: [read]\navailable_as: {helper_as}\n---\nHelp.\n"),
    ));
    all.push(def(
        "lead",
        "---\nname: Lead\ntools: [read]\navailable_as: [agent]\n---\nLead.\n",
    ));
    all.push(def(
        "boss",
        "---\nname: Boss\ntools: [read]\ndelegates: [helper, lead]\navailable_as: [agent]\n---\nBoss.\n",
    ));
    AgentSet::from_agents(all, harness_agents::DEFAULT_AGENT)
}

struct T {
    base: String,
    master: Master,
    _dir: tempfile::TempDir,
}

async fn start() -> T {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let master = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false),
    );
    master.set_models(vec![ModelInfo {
        id: "mock".into(),
        provider: "mock".into(),
        default: true,
        ..Default::default()
    }]);
    master.set_agents(agents("[subagent]"));
    let chat = runtime(dir.path()).await;
    master.add_link(chat.attach().await, true).await.unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        master,
        _dir: dir,
    }
}

async fn post(t: &T, path: &str, body: Value) -> (StatusCode, Value) {
    let r = reqwest::Client::new()
        .post(format!("{}{path}", t.base))
        .bearer_auth("tok")
        .json(&body)
        .send()
        .await
        .unwrap();
    (r.status(), r.json().await.unwrap_or(Value::Null))
}

async fn get(t: &T, path: &str) -> Value {
    let r = reqwest::Client::new()
        .get(format!("{}{path}", t.base))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap();
    r.json().await.unwrap()
}

async fn chat(t: &T, agent: Option<&str>) -> String {
    let mut body = json!({"profile": "chat"});
    if let Some(a) = agent {
        body["agent_id"] = json!(a);
    }
    let (st, v) = post(t, "/api/sessions", body).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["session_id"].as_str().unwrap().to_string()
}

async fn events_until(t: &T, sid: &str, pred: impl Fn(&[RawEvent]) -> bool) -> Vec<RawEvent> {
    for _ in 0..300 {
        let v = get(t, &format!("/api/sessions/{sid}/events")).await;
        let evs: Vec<RawEvent> = serde_json::from_value(v["events"].clone()).unwrap();
        if pred(&evs) {
            return evs;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("condition not reached");
}

fn turn_agents(evs: &[RawEvent]) -> Vec<String> {
    evs.iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::TurnStarted(s)) => s.agent.map(|a| a.agent_id),
            _ => None,
        })
        .collect()
}

fn says_subagent(v: &Value) -> bool {
    v["error"]
        .as_str()
        .is_some_and(|e| e.contains("Helper") && e.contains("sub-agent"))
}

#[tokio::test]
async fn a_subagent_only_agent_never_runs_a_chat() {
    let t = start().await;
    // A new session with it.
    let (st, v) = post(
        &t,
        "/api/sessions",
        json!({"profile": "chat", "agent_id": "helper"}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert!(says_subagent(&v), "{v}");

    let sid = chat(&t, None).await;
    // An explicit agent for one turn.
    let (st, v) = post(
        &t,
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": "hi", "agent_id": "helper"}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert!(says_subagent(&v), "{v}");
    // The session's agent.
    let (st, v) = post(
        &t,
        &format!("/api/sessions/{sid}/agent"),
        json!({"agent_id": "helper"}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert!(says_subagent(&v), "{v}");
    // A leading @mention is refused too, saying why: the user named an
    // agent that exists, so answering with another would mislead.
    let (st, v) = post(
        &t,
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": "@helper look at this"}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert!(says_subagent(&v), "{v}");
    // Nothing ran; an ordinary prompt still does, as the session's agent.
    let (st, _) = post(
        &t,
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": "hello"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let evs = events_until(&t, &sid, |e| e.iter().any(|x| x.kind == "turn_ended")).await;
    assert_eq!(turn_agents(&evs), ["general"]);
    assert!(!evs.iter().any(|e| e.kind == "agent_changed"));
}

#[tokio::test]
async fn a_chat_whose_agent_became_subagent_only_runs_as_the_default() {
    let t = start().await;
    t.master.set_agents(agents("[agent, subagent]"));
    let sid = chat(&t, Some("helper")).await;
    // The definition changes under the chat (a file edited on the server).
    t.master.set_agents(agents("[subagent]"));
    let (st, v) = post(
        &t,
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": "hello"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let evs = events_until(&t, &sid, |e| e.iter().any(|x| x.kind == "turn_ended")).await;
    assert_eq!(turn_agents(&evs), ["general"]);
}

#[test]
fn delegate_rosters_hold_subagents_only() {
    let m = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("t".into()), vec![], false),
    );
    m.set_agents(agents("[subagent]"));
    let spec = m.agent_spec(Some("boss"), None).unwrap();
    let ids: Vec<&str> = spec.delegates.iter().map(|d| d.agent_id.as_str()).collect();
    assert_eq!(ids, ["helper"], "lead runs as an agent only");
    // The same roster when the set was built without edits applied.
    let r = m.resolver(None);
    let raw = r.agents().get("boss").unwrap();
    assert_eq!(raw.delegates, ["helper"]);
}
