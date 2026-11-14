//! A delegate whose declared tools are none of those registered and allowed
//! on this node is not offered and is refused by name; one that declares no
//! tools on purpose, or has some of them, is unaffected.

use super::*;
use harness_core::SessionId;
use harness_core::agent::{AgentSpec, AllTools, ToolSelection};
use harness_core::backend::TurnHost;
use harness_core::policy::{Decision, ToolRisk};
use provider::{ChatRequest, Completion, ProviderError};
use serde_json::{Value, json};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Host {
    events: Mutex<Vec<EventKind>>,
    started: AtomicUsize,
    denied: Vec<&'static str>,
}

#[async_trait::async_trait]
impl TurnHost for Host {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        self.events.lock().unwrap().push(kind);
        Ok(())
    }
    async fn approve(&self, _: &str, _: &str, _: &Value, _: ToolRisk) -> Decision {
        Decision::Allow
    }
    fn tool_denied(&self, tool: &str) -> bool {
        self.denied.contains(&tool)
    }
    async fn delegate(
        &self,
        agent: AgentSpec,
        _task: String,
        _refs: SharedRefs,
        _observed: Arc<Vec<String>>,
        _cancel: tokio_util::sync::CancellationToken,
        started: Box<dyn FnOnce(SessionId) + Send>,
    ) -> Result<harness_core::backend::DelegateOutcome, String> {
        self.started.fetch_add(1, Ordering::SeqCst);
        let child = SessionId::new();
        started(child);
        Ok(harness_core::backend::DelegateOutcome {
            child,
            text: format!("{} ran", agent.agent_id),
            reason: TurnEndReason::Completed,
            usage: None,
            sources: vec![],
            finish: None,
            error: None,
            note: None,
        })
    }
}

struct Rec {
    reqs: Mutex<Vec<ChatRequest>>,
}

#[async_trait::async_trait]
impl provider::Provider for Rec {
    fn name(&self) -> &'static str {
        "rec"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        deltas: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        self.reqs.lock().unwrap().push(req.clone());
        mock::Mock {
            delay: Duration::from_millis(1),
        }
        .complete(req, deltas)
        .await
    }
}

fn spec(id: &str, tools: ToolSelection) -> AgentSpec {
    AgentSpec {
        agent_id: id.into(),
        name: id.into(),
        system_prompt: "p".into(),
        tools,
        model: None,
        fallbacks: vec![],
        unavailable: vec![],
        no_effort: vec![],
        context_windows: Default::default(),
        effort: None,
        budget: Default::default(),
        ui: Default::default(),
        prompt_hash: "h".into(),
        deny_tools: vec![],
        delegates: vec![],
        caps: None,
    }
}

fn list(names: &[&str]) -> ToolSelection {
    ToolSelection::List(names.iter().map(|s| s.to_string()).collect())
}

struct Fake(&'static str);

#[async_trait::async_trait]
impl harness_tools::Tool for Fake {
    fn name(&self) -> &'static str {
        self.0
    }
    fn description(&self) -> &'static str {
        "fake"
    }
    fn schema(&self) -> Value {
        json!({"type": "object"})
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }
    async fn run(&self, _: &harness_tools::ToolCtx, _: Value) -> ToolOutcome {
        ToolOutcome::err("unused")
    }
}

/// Runs one turn of a lead that may delegate to `delegates`, on a node
/// whose registry is the builtin tools plus `extra`, the model saying
/// `prompt`. Returns the delegate ids the model was offered, the sub-agents
/// started, and the tool results.
async fn run(
    extra: &[&'static str],
    denied: Vec<&'static str>,
    delegates: Vec<AgentSpec>,
    prompt: &str,
) -> (Vec<String>, usize, Vec<String>) {
    let mut tools = harness_tools::builtin();
    tools.extend(
        extra
            .iter()
            .map(|n| Arc::new(Fake(n)) as Arc<dyn harness_tools::Tool>),
    );
    let rec = Arc::new(Rec {
        reqs: Mutex::default(),
    });
    let backend = NativeBackend::new(rec.clone(), tools, NativeConfig::default());
    let host = Host {
        events: Mutex::default(),
        started: AtomicUsize::new(0),
        denied,
    };
    let dir = tempfile::tempdir().unwrap();
    backend
        .run_turn(TurnCtx {
            session_id: SessionId::new(),
            turn_id: "t".into(),
            history: vec![],
            user_text: prompt.into(),
            cwd: dir.path().to_path_buf(),
            model: "mock".into(),
            agent: Some(AgentSpec {
                delegates,
                ..spec("lead", ToolSelection::All(AllTools::All))
            }),
            compare: vec![],
            parent_session: None,
            extra: Default::default(),
            cancel: Default::default(),
            host: &host,
        })
        .await;
    let offered = rec.reqs.lock().unwrap()[0]
        .tools
        .iter()
        .find(|t| t.name == DELEGATE)
        .map(|t| {
            t.schema["properties"]["agent"]["enum"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default();
    let outs = host
        .events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            EventKind::ToolCallFinished(f) => Some(f.output.clone()),
            _ => None,
        })
        .collect();
    (offered, host.started.load(Ordering::SeqCst), outs)
}

fn researcher() -> AgentSpec {
    spec("researcher", list(&["web_search", "web_fetch", "remember"]))
}

#[tokio::test]
async fn a_delegate_with_no_available_tool_is_not_offered_and_is_refused() {
    let all = vec![researcher(), spec("scout", list(&["read"]))];
    let (offered, started, outs) = run(&[], vec![], all, "delegate: researcher=ask").await;
    assert_eq!(offered, ["scout"]);
    assert_eq!(started, 0, "no child session");
    assert_eq!(
        outs,
        [
            "@researcher was not started: none of its tools (web search, web fetch, memory) \
          are available on this node. Do the work another way or tell the user."
        ]
    );
}

#[tokio::test]
async fn in_the_chat_runtime_the_researcher_is_offered_and_runs() {
    let all = vec![researcher(), spec("scout", list(&["read"]))];
    let (offered, started, outs) = run(
        &["web_search", "web_fetch", "remember"],
        vec![],
        all,
        "delegate: researcher=ask",
    )
    .await;
    assert_eq!(offered, ["researcher", "scout"]);
    assert_eq!(started, 1);
    assert!(outs[0].contains("researcher ran"), "{outs:?}");
}

#[tokio::test]
async fn an_agent_that_declares_no_tools_stays_offered_and_runs() {
    let all = vec![spec("smol", list(&[]))];
    let (offered, started, _) = run(&[], vec![], all, "delegate: smol=rephrase").await;
    assert_eq!(offered, ["smol"]);
    assert_eq!(started, 1);
}

#[tokio::test]
async fn some_available_tools_is_enough_and_none_is_refused() {
    let all = vec![
        spec("half", list(&["read", "web_search"])),
        spec("none", list(&["web_search", "web_fetch"])),
    ];
    let (offered, started, outs) = run(&[], vec![], all, "delegate: half=a | none=b").await;
    assert_eq!(offered, ["half"]);
    assert_eq!(started, 1);
    assert_eq!(outs.len(), 2, "{outs:?}");
    assert!(outs.iter().any(|o| o.contains("half ran")), "{outs:?}");
    assert!(
        outs.iter()
            .any(|o| o.starts_with("@none was not started") && o.contains("web search, web fetch")),
        "{outs:?}"
    );
}

/// On an agentic node that offers `remember` but no web tools, Researcher
/// is still not offered: proposing a memory does none of a task's work, so
/// it alone makes no delegate available (A.11 holds once nodes register it).
#[tokio::test]
async fn memory_alone_does_not_make_a_delegate_available() {
    let all = vec![researcher(), spec("scout", list(&["read"]))];
    let (offered, started, outs) =
        run(&["remember"], vec![], all, "delegate: researcher=ask").await;
    assert_eq!(offered, ["scout"]);
    assert_eq!(started, 0, "no child session");
    assert!(
        outs[0].starts_with("@researcher was not started"),
        "{outs:?}"
    );
}

/// The node's own `denied_tools` count as "not available there".
#[tokio::test]
async fn a_tool_the_node_denies_does_not_count() {
    let all = vec![
        spec("shell", list(&["bash"])),
        spec("scout", list(&["read"])),
    ];
    let (offered, started, outs) = run(&[], vec!["bash"], all, "delegate: shell=go").await;
    assert_eq!(offered, ["scout"]);
    assert_eq!(started, 0);
    assert!(outs[0].starts_with("@shell was not started"), "{outs:?}");
}
