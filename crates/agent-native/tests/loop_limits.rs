//! The loop's limits come from the node (node.toml `[limits]`): the step cap
//! and the longest a `bash` call may run. Nothing the model or an agent
//! definition says raises them.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use harness_agent_native::{
    ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError,
};
use harness_core::backend::{AgentBackend, TurnCtx, TurnHost};
use harness_core::event::{EventKind, ToolCallRef, TurnEndReason};
use harness_core::policy::{Decision, ToolRisk};
use serde_json::{Value, json};
use tokio::sync::mpsc;

#[derive(Default)]
struct Host(Mutex<Vec<EventKind>>);

#[async_trait::async_trait]
impl TurnHost for Host {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        self.0.lock().unwrap().push(kind);
        Ok(())
    }
    async fn approve(&self, _: &str, _: &str, _: &Value, _: ToolRisk) -> Decision {
        Decision::Allow
    }
}

/// Answers every request with one tool call, forever; counts the requests
/// and keeps the tools it was offered.
struct Forever {
    input: Value,
    calls: Mutex<usize>,
    offered: Mutex<Vec<Value>>,
}

impl Forever {
    fn new(input: Value) -> Arc<Self> {
        Arc::new(Self {
            input,
            calls: Mutex::new(0),
            offered: Mutex::default(),
        })
    }
}

#[async_trait::async_trait]
impl Provider for Forever {
    fn name(&self) -> &'static str {
        "forever"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        _: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        *self.calls.lock().unwrap() += 1;
        *self.offered.lock().unwrap() = req.tools.iter().map(|t| t.schema.clone()).collect();
        Ok(Completion {
            text: String::new(),
            tool_calls: vec![ToolCallRef {
                call_id: harness_core::new_id(),
                tool: "bash".into(),
                input: self.input.clone(),
            }],
            usage: None,
            stop: Default::default(),
        })
    }
}

fn turn<'a>(host: &'a Host, cwd: &std::path::Path) -> TurnCtx<'a> {
    TurnCtx {
        session_id: harness_core::SessionId::new(),
        turn_id: "t".into(),
        history: vec![],
        user_text: "go".into(),
        cwd: cwd.to_path_buf(),
        model: "x".into(),
        agent: None,
        compare: vec![],
        parent_session: None,
        extra: Default::default(),
        cancel: Default::default(),
        host,
    }
}

fn backend(p: Arc<Forever>, cfg: NativeConfig) -> NativeBackend {
    NativeBackend::new(p, harness_tools::builtin(), cfg)
}

#[tokio::test]
async fn max_steps_ends_the_turn_and_names_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let host = Host::default();
    let p = Forever::new(json!({"command": "true"}));
    let b = backend(
        p.clone(),
        NativeConfig {
            max_steps: 2,
            ..Default::default()
        },
    );
    let out = b.run_turn(turn(&host, dir.path())).await;
    assert_eq!(*p.calls.lock().unwrap(), 2, "exactly two steps");
    assert_eq!(out.reason, TurnEndReason::Error);
    assert_eq!(
        out.error.unwrap(),
        "stopped after 2 steps (`max_steps` in node.toml)"
    );
}

#[tokio::test]
async fn the_defaults_are_todays_values() {
    assert_eq!(NativeConfig::default().max_steps, 50);
    assert_eq!(
        NativeConfig::default().bash_timeout_max,
        Duration::from_secs(3600)
    );
    let dir = tempfile::tempdir().unwrap();
    let host = Host::default();
    let p = Forever::new(json!({"command": "true"}));
    let out = backend(p.clone(), NativeConfig::default())
        .run_turn(turn(&host, dir.path()))
        .await;
    assert_eq!(*p.calls.lock().unwrap(), 50);
    assert_eq!(
        out.error.unwrap(),
        "stopped after 50 steps (`max_steps` in node.toml)"
    );
}

#[tokio::test]
async fn an_agent_definition_cannot_run_more_steps_than_the_node_allows() {
    let dir = tempfile::tempdir().unwrap();
    let host = Host::default();
    let p = Forever::new(json!({"command": "true"}));
    let b = backend(
        p.clone(),
        NativeConfig {
            max_steps: 3,
            ..Default::default()
        },
    );
    // A spec that asks for all it can: every tool, delegation caps at the
    // top. It has no step or timeout field to ask with.
    let agent = harness_core::agent::AgentSpec {
        agent_id: "greedy".into(),
        name: "Greedy".into(),
        system_prompt: "never stop".into(),
        tools: harness_core::agent::ToolSelection::All(harness_core::agent::AllTools::All),
        model: None,
        fallbacks: vec![],
        unavailable: vec![],
        no_effort: vec![],
        context_windows: Default::default(),
        effort: None,
        budget: harness_core::agent::Budget::Standard,
        ui: Default::default(),
        prompt_hash: "h".into(),
        deny_tools: vec![],
        delegates: vec![],
        caps: Some(harness_core::agent::DelegateCaps {
            max_subagents: u32::MAX,
            max_tokens: u64::MAX,
        }),
    };
    let mut t = turn(&host, dir.path());
    t.agent = Some(agent);
    b.run_turn(t).await;
    assert_eq!(*p.calls.lock().unwrap(), 3);
}

#[tokio::test]
async fn bash_timeout_ceiling_cuts_a_call_that_asked_for_more() {
    let dir = tempfile::tempdir().unwrap();
    let host = Host::default();
    let marker = dir.path().join("grandchild.pid");
    let p = Forever::new(json!({
        "command": format!("sleep 5 & echo $! > {}; wait", marker.display()),
        "timeout_secs": 3600
    }));
    let b = backend(
        p,
        NativeConfig {
            max_steps: 1,
            bash_timeout_max: Duration::from_secs(1),
            ..Default::default()
        },
    );
    let started = Instant::now();
    b.run_turn(turn(&host, dir.path())).await;
    let took = started.elapsed();
    assert!(took < Duration::from_secs(4), "took {took:?}");
    let out = host
        .0
        .lock()
        .unwrap()
        .iter()
        .find_map(|e| match e {
            EventKind::ToolCallFinished(f) => Some(f.output.clone()),
            _ => None,
        })
        .expect("finished");
    assert!(out.contains("timed out"), "{out}");
    // The process tree is gone.
    let pid: i32 = std::fs::read_to_string(&marker)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    for _ in 0..50 {
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    panic!("grandchild {pid} survived the timeout");
}

#[tokio::test]
async fn the_model_is_told_this_nodes_ceiling() {
    let dir = tempfile::tempdir().unwrap();
    let host = Host::default();
    let p = Forever::new(json!({"command": "true"}));
    backend(
        p.clone(),
        NativeConfig {
            max_steps: 1,
            bash_timeout_max: Duration::from_secs(90),
            ..Default::default()
        },
    )
    .run_turn(turn(&host, dir.path()))
    .await;
    let offered = p.offered.lock().unwrap().clone();
    let text = offered
        .iter()
        .find(|s| s["properties"]["timeout_secs"].is_object())
        .map(|s| s["properties"]["timeout_secs"]["description"].to_string())
        .expect("bash offered");
    assert!(
        text.contains("from 5 to 90") && !text.contains("3600"),
        "{text}"
    );
}
