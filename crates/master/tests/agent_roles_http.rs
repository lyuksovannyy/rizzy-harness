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
        secrets: Default::default(),
        profile,
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
    let chat = runtime(dir.path(), "master", Profile::Chat).await;
    master.add_link(chat.attach().await, true).await.unwrap();
    let worker = runtime(dir.path(), "local", Profile::Agentic).await;
    master.add_link(worker.attach().await, true).await.unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        master,
        _dir: dir,
    }
}

/// The error text of a refused call.
fn error(v: &Value) -> &str {
    v["error"].as_str().unwrap_or_default()
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
    // With an explicit agent the text is only text: a routine's event
    // starting with such a mention cannot fail its turn.
    let (st, v) = post(
        &t,
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": "@helper look at this", "agent_id": "general"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    events_until(&t, &sid, |e| e.iter().any(|x| x.kind == "turn_ended")).await;
    // Nothing else ran; an ordinary prompt still does, as the chat's agent.
    let (st, _) = post(
        &t,
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": "hello"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let evs = events_until(&t, &sid, |e| {
        e.iter().filter(|x| x.kind == "turn_ended").count() == 2
    })
    .await;
    assert_eq!(turn_agents(&evs), ["general", "general"]);
    assert!(!evs.iter().any(|e| e.kind == "agent_changed"));
}

/// A chat whose agent became sub-agent-only refuses its next prompt with
/// the reason; choosing another agent makes it usable again.
#[tokio::test]
async fn a_chat_whose_agent_became_subagent_only_is_refused_until_changed() {
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
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert!(says_subagent(&v), "{v}");
    let (st, v) = post(
        &t,
        &format!("/api/sessions/{sid}/agent"),
        json!({"agent_id": "writer"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = post(
        &t,
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": "hello"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let evs = events_until(&t, &sid, |e| e.iter().any(|x| x.kind == "turn_ended")).await;
    assert_eq!(turn_agents(&evs), ["writer"]);
}

/// The built-in roster: `@scout` is refused (Scout is a sub-agent only),
/// `@ai` is the Assistant, stored as `general`.
#[tokio::test]
async fn built_in_mentions_and_aliases() {
    let t = start().await;
    let sid = chat(&t, None).await;
    let (st, v) = post(
        &t,
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": "@scout where is main?"}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["error"].as_str().unwrap().contains("sub-agent"), "{v}");
    let (st, v) = post(
        &t,
        &format!("/api/sessions/{sid}/agent"),
        json!({"agent_id": "writer"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = post(
        &t,
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": "@ai hello"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let evs = events_until(&t, &sid, |e| e.iter().any(|x| x.kind == "turn_ended")).await;
    assert_eq!(turn_agents(&evs), ["general"]);
    assert!(evs.iter().any(|e| matches!(e.typed(),
        Ok(EventKind::AssistantMessage(m)) if m.author.as_deref() == Some("general"))));
    // The mention routed one turn; the chat's agent stays the Writer.
    let row = get(&t, &format!("/api/sessions/{sid}")).await;
    assert_eq!(row["agent_id"], "writer");
    // An explicit alias is stored as the id too.
    let (st, v) = post(
        &t,
        &format!("/api/sessions/{sid}/agent"),
        json!({"agent_id": "research"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    events_until(&t, &sid, |e| e.iter().any(|x| x.kind == "agent_changed")).await;
    let row = get(&t, &format!("/api/sessions/{sid}")).await;
    assert_eq!(row["agent_id"], "researcher");
}

/// Chats made with Ultracode, which is gone, refuse a prompt with an
/// error that names the agent and says what to do; the chat still lists and
/// opens, and works again once another agent is chosen. Never a silent
/// stand-in.
#[tokio::test]
async fn an_ultracode_chat_is_refused_until_another_agent_is_chosen() {
    let t = start().await;
    let mut old = builtins();
    old.push(
        parse(
            "ultracode",
            "---\nname: Ultracode\ntools: all\navailable_as: [agent]\n---\nOrchestrate.\n",
            false,
        )
        .unwrap(),
    );
    t.master
        .set_agents(AgentSet::from_agents(old, harness_agents::DEFAULT_AGENT));
    let sid = chat(&t, Some("ultracode")).await;
    t.master.set_agents(agents("[subagent]"));
    let (st, v) = post(
        &t,
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": "hello"}),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    let e = error(&v);
    assert!(
        e.contains("ultracode") && e.contains("choose another agent"),
        "{v}"
    );
    // Listing and opening it still work.
    let row = get(&t, &format!("/api/sessions/{sid}")).await;
    assert_eq!(row["agent_id"], "ultracode");
    let list = get(&t, "/api/sessions").await;
    assert!(list.to_string().contains(&sid), "{list}");
    // One turn with an explicit agent works without changing the chat.
    let (st, v) = post(
        &t,
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": "hello", "agent_id": "writer"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    events_until(&t, &sid, |e| e.iter().any(|x| x.kind == "turn_ended")).await;
    // Naming it is an unknown agent, named in the error.
    let (st, v) = post(
        &t,
        &format!("/api/sessions/{sid}/agent"),
        json!({"agent_id": "ultracode"}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert!(error(&v).contains("ultracode"), "{v}");
    // Another agent, and the chat goes on.
    let (st, v) = post(
        &t,
        &format!("/api/sessions/{sid}/agent"),
        json!({"agent_id": "general"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = post(
        &t,
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": "hello again"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let evs = events_until(&t, &sid, |e| {
        e.iter().filter(|x| x.kind == "turn_ended").count() == 2
    })
    .await;
    assert_eq!(turn_agents(&evs), ["writer", "general"]);
}

/// Unknown ids are refused wherever an agent is named, and the error names
/// the id; a leading `@name` that is no agent stays plain text.
#[tokio::test]
async fn unknown_agents_are_named_in_the_error() {
    let t = start().await;
    let (st, v) = post(
        &t,
        "/api/sessions",
        json!({"profile": "chat", "agent_id": "nope"}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert!(error(&v).contains("nope"), "{v}");
    let sid = chat(&t, None).await;
    for (path, body) in [
        ("prompt", json!({"text": "hi", "agent_id": "nope"})),
        ("agent", json!({"agent_id": "nope"})),
    ] {
        let (st, v) = post(&t, &format!("/api/sessions/{sid}/{path}"), body).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{path}: {v}");
        let e = error(&v);
        assert!(
            e.contains("\"nope\"") && e.contains("choose"),
            "{path}: {v}"
        );
    }
    let (st, v) = post(
        &t,
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": "@everyone the build is green"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let evs = events_until(&t, &sid, |e| e.iter().any(|x| x.kind == "turn_ended")).await;
    assert_eq!(turn_agents(&evs), ["general"]);
}

/// Agent-only agents (the Teamlead, the Assistant) are in no roster, and a
/// sub-agent's spec never carries delegates of its own: depth one.
#[test]
fn rosters_of_the_built_ins() {
    let m = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("t".into()), vec![], false),
    );
    let mut all = builtins();
    all.push(
        parse(
            "greedy",
            "---\nname: Greedy\ntools: all\ndelegates: [teamlead, general, ai, dev]\n---\nGo.\n",
            false,
        )
        .unwrap(),
    );
    m.set_agents(AgentSet::from_agents(all, harness_agents::DEFAULT_AGENT));
    let ids = |spec: &harness_core::agent::AgentSpec| -> Vec<String> {
        spec.delegates.iter().map(|d| d.agent_id.clone()).collect()
    };
    let greedy = m.agent_spec(Some("greedy"), None).unwrap();
    assert_eq!(ids(&greedy), ["coder"], "aliases resolve; agent-only go");
    let lead = m.agent_spec(Some("teamlead"), None).unwrap();
    assert_eq!(
        ids(&lead),
        ["coder", "researcher", "writer", "scout", "reviewer"]
    );
    let coder = lead
        .delegates
        .iter()
        .find(|d| d.agent_id == "coder")
        .unwrap();
    assert!(coder.delegates.is_empty() && coder.caps.is_none());
    // As a chat's agent the Software Developer may delegate.
    let dev = m.agent_spec(Some("dev"), None).unwrap();
    assert_eq!(dev.agent_id, "coder");
    assert_eq!(ids(&dev), ["scout", "researcher", "reviewer"]);
    assert!(
        m.agent_spec(Some("general"), None)
            .unwrap()
            .delegates
            .is_empty()
    );
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
