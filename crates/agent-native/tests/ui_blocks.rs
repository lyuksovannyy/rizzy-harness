//! `ui:` blocks in the agent loop: the agent's level decides which blocks a
//! recorded message carries, a sub-task is capped at `display`, blocks come
//! from the text as stored (secrets scrubbed), and the system prompt says
//! what will show. The mock's `progress: <n>` moves one block on per step.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_agent_native::{
    ChatMsg, ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError, mock,
};
use harness_core::agent::{AgentSpec, ToolSelection, UiLevel};
use harness_core::backend::{AgentBackend, TurnCtx, TurnHost};
use harness_core::event::{AssistantMessage, EventKind, SessionCreated, ToolCallRef, UiBlock};
use harness_core::policy::{Decision, ToolRisk};
use harness_core::{RawEvent, Secret, SecretSet, SessionId};
use serde_json::Value;
use tokio::sync::mpsc;

#[derive(Default)]
struct Host(Mutex<Vec<EventKind>>);

#[async_trait::async_trait]
impl TurnHost for Host {
    async fn emit(&self, kind: EventKind) {
        self.0.lock().unwrap().push(kind);
    }
    async fn approve(&self, _: &str, _: &str, _: &Value, _: ToolRisk) -> Decision {
        Decision::Allow
    }
}

impl Host {
    fn messages(&self) -> Vec<AssistantMessage> {
        let events = self.0.lock().unwrap();
        events
            .iter()
            .filter_map(|e| match e {
                EventKind::AssistantMessage(m) => Some(m.clone()),
                _ => None,
            })
            .collect()
    }
}

/// Answers like the mock (or with fixed text, whose first answer may also
/// call a tool) and keeps every request.
struct Rec {
    fixed: Option<String>,
    tool_first: bool,
    reqs: Mutex<Vec<ChatRequest>>,
}

#[async_trait::async_trait]
impl Provider for Rec {
    fn name(&self) -> &'static str {
        "rec"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        deltas: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        let first = {
            let mut reqs = self.reqs.lock().unwrap();
            reqs.push(req.clone());
            reqs.len() == 1
        };
        match &self.fixed {
            Some(text) => Ok(Completion {
                text: text.clone(),
                tool_calls: if self.tool_first && first {
                    vec![ToolCallRef {
                        call_id: "c1".into(),
                        tool: "read".into(),
                        input: serde_json::json!({"path": "x"}),
                    }]
                } else {
                    vec![]
                },
                ..Default::default()
            }),
            None => {
                let m = mock::Mock {
                    delay: Duration::ZERO,
                };
                m.complete(req, deltas).await
            }
        }
    }
}

fn agent(ui: UiLevel) -> AgentSpec {
    AgentSpec {
        agent_id: "general".into(),
        name: "General".into(),
        system_prompt: "Be helpful.".into(),
        tools: ToolSelection::List(vec!["bash".into(), "web_search".into()]),
        model: None,
        fallbacks: vec![],
        unavailable: vec![],
        no_effort: vec![],
        context_windows: Default::default(),
        effort: None,
        budget: Default::default(),
        ui,
        prompt_hash: "h".into(),
        deny_tools: vec![],
        delegates: vec![],
        caps: None,
    }
}

fn session_created(parent: Option<SessionId>) -> RawEvent {
    let (kind, payload) = EventKind::SessionCreated(SessionCreated {
        title: "t".into(),
        cwd: String::new(),
        model: "mock".into(),
        backend: "native".into(),
        node_id: "local".into(),
        approval_mode: Default::default(),
        profile: harness_core::event::Profile::Chat,
        agent_id: None,
        incognito: false,
        parent_session: parent,
    })
    .into_parts();
    RawEvent {
        session_id: SessionId::new(),
        seq: 1,
        ts_ms: 0,
        kind,
        payload,
    }
}

struct Turn {
    text: &'static str,
    ui: UiLevel,
    parent: Option<SessionId>,
    /// The log before the turn; `None` is the session's own creation.
    history: Option<Vec<RawEvent>>,
    compare: Vec<String>,
    fixed: Option<String>,
    tool_first: bool,
    secrets: SecretSet,
    tools: harness_tools::ToolSet,
}

impl Turn {
    fn new(text: &'static str, ui: UiLevel) -> Self {
        Self {
            text,
            ui,
            parent: None,
            history: None,
            compare: vec![],
            fixed: None,
            tool_first: false,
            secrets: SecretSet::default(),
            tools: vec![],
        }
    }

    /// The recorded messages and the system prompt of the first request.
    async fn run(self) -> (Vec<AssistantMessage>, String) {
        let rec = Arc::new(Rec {
            fixed: self.fixed,
            tool_first: self.tool_first,
            reqs: Mutex::new(vec![]),
        });
        let backend = NativeBackend::new(
            rec.clone(),
            self.tools,
            NativeConfig {
                secrets: self.secrets,
                ..Default::default()
            },
        );
        let host = Host::default();
        backend
            .run_turn(TurnCtx {
                session_id: harness_core::SessionId::new(),
                turn_id: "t1".into(),
                history: self
                    .history
                    .unwrap_or_else(|| vec![session_created(self.parent)]),
                user_text: self.text.into(),
                cwd: Default::default(),
                model: "mock".into(),
                agent: Some(agent(self.ui)),
                compare: self.compare,
                parent_session: self.parent,
                extra: Default::default(),
                cancel: Default::default(),
                host: &host,
            })
            .await;
        let system = match rec.reqs.lock().unwrap()[0].messages.first() {
            Some(ChatMsg::System(s)) => s.clone(),
            other => panic!("{other:?}"),
        };
        (host.messages(), system)
    }
}

fn summary(blocks: &[UiBlock]) -> Vec<(&str, Option<&str>)> {
    blocks
        .iter()
        .map(|b| (b.kind.as_str(), b.error.as_deref()))
        .collect()
}

#[tokio::test]
async fn the_agents_level_decides_the_blocks() {
    let (msgs, _) = Turn::new("ui: choices", UiLevel::Rich).run().await;
    assert_eq!(summary(&msgs[0].blocks), [("choices", None)]);
    assert_eq!(msgs[0].blocks[0].id, "colour");

    let (msgs, _) = Turn::new("ui: choices", UiLevel::Display).run().await;
    assert_eq!(
        summary(&msgs[0].blocks),
        [("choices", Some(harness_core::ui::OFF_FOR_AGENT))]
    );
    let (msgs, _) = Turn::new("ui: progress", UiLevel::Display).run().await;
    assert_eq!(summary(&msgs[0].blocks), [("progress", None)]);

    let (msgs, _) = Turn::new("ui: progress", UiLevel::Off).run().await;
    assert!(msgs[0].blocks.is_empty() && msgs[0].text.contains("```ui:progress"));
}

#[tokio::test]
async fn a_sub_task_is_capped_at_display() {
    let mut turn = Turn::new("ui: choices", UiLevel::Rich);
    turn.parent = Some(SessionId::new());
    let (msgs, system) = turn.run().await;
    assert_eq!(
        summary(&msgs[0].blocks),
        [("choices", Some(harness_core::ui::OFF_FOR_AGENT))]
    );
    assert!(system.contains("ui:progress") && !system.contains("ui:choices"));
    // The cap comes from the session, not its log: a sub-task whose log
    // could not be read is still capped.
    let mut turn = Turn::new("ui: choices", UiLevel::Rich);
    (turn.parent, turn.history) = (Some(SessionId::new()), Some(vec![]));
    let (msgs, _) = turn.run().await;
    assert_eq!(
        summary(&msgs[0].blocks),
        [("choices", Some(harness_core::ui::OFF_FOR_AGENT))]
    );
}

/// A question ends the turn: one in a message that also calls tools is an
/// error, and only the last message of the turn may ask.
#[tokio::test]
async fn a_message_that_calls_tools_asks_nothing() {
    let mut turn = Turn::new("anything", UiLevel::Rich);
    turn.fixed = Some(
        "Which?\n\n```ui:choices\n{\"id\": \"q\", \"question\": \"Which?\", \"options\": [\"a\", \"b\"]}\n```\n"
            .into(),
    );
    turn.tool_first = true;
    let (msgs, _) = turn.run().await;
    assert_eq!(msgs.len(), 2, "the tool call, then the final reply");
    assert_eq!(
        summary(&msgs[0].blocks),
        [("choices", Some(harness_core::ui::ENDS_REPLY))]
    );
    assert_eq!(summary(&msgs[1].blocks), [("choices", None)]);
}

#[tokio::test]
async fn blocks_come_from_the_scrubbed_text() {
    let secret = "sk-live-0123456789abcdef";
    let body = format!(
        r#"{{"id": "p", "title": "Using {secret}", "steps": [{{"label": "a", "status": "done"}}]}}"#
    );
    let mut turn = Turn::new("anything", UiLevel::Rich);
    turn.fixed = Some(format!("Here:\n\n```ui:progress\n{body}\n```\n"));
    turn.secrets = SecretSet::new(vec![Secret::new(secret.to_string())]);
    let (msgs, _) = turn.run().await;
    let m = &msgs[0];
    assert!(!m.text.contains(secret) && m.text.contains("[REDACTED]"));
    let stored = body.replace(secret, "[REDACTED]");
    assert_eq!(
        m.blocks[0].hash,
        format!("{:08x}", harness_core::ui::fnv1a32(stored.as_bytes()))
    );
    assert_eq!(
        m.blocks[0].spec.as_ref().unwrap()["title"],
        "Using [REDACTED]"
    );
}

#[tokio::test]
async fn parallel_answers_carry_blocks_too() {
    let mut turn = Turn::new("ui: chart", UiLevel::Rich);
    turn.compare = vec!["mock".into(), "mock-b".into()];
    let (msgs, _) = turn.run().await;
    assert_eq!(msgs.len(), 2);
    assert!(msgs.iter().all(|m| summary(&m.blocks) == [("chart", None)]));
}

#[tokio::test]
async fn the_system_prompt_says_what_each_level_shows() {
    let (_, rich) = Turn::new("hi", UiLevel::Rich).run().await;
    assert!(rich.contains("ui:choices") && rich.contains("ui:progress"));
    let (_, display) = Turn::new("hi", UiLevel::Display).run().await;
    assert!(display.contains("ui:progress") && !display.contains("ui:form"));
    let (_, off) = Turn::new("hi", UiLevel::Off).run().await;
    assert!(!off.contains("ui:") && off.starts_with("Be helpful."));
}

/// A tool that only answers, under a real tool's name.
struct Stub(&'static str);

#[async_trait::async_trait]
impl harness_tools::Tool for Stub {
    fn name(&self) -> &'static str {
        self.0
    }
    fn description(&self) -> &'static str {
        "stub"
    }
    fn schema(&self) -> Value {
        serde_json::json!({"type": "object"})
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }
    async fn run(&self, _: &harness_tools::ToolCtx, _: Value) -> harness_tools::ToolOutcome {
        harness_tools::ToolOutcome::ok("ok")
    }
}

/// Per message: the block's step statuses and the tool it calls.
fn progress_run(msgs: &[AssistantMessage]) -> Vec<(Vec<String>, Option<String>)> {
    msgs.iter()
        .map(|m| {
            assert_eq!(summary(&m.blocks), [("progress", None)], "{}", m.text);
            assert_eq!(m.blocks[0].id, "steps");
            let steps = m.blocks[0].spec.as_ref().unwrap()["steps"]
                .as_array()
                .unwrap()
                .clone();
            let statuses = steps
                .iter()
                .map(|s| s["status"].as_str().unwrap().to_string())
                .collect();
            (
                statuses,
                m.tool_calls
                    .first()
                    .map(|c| format!("{} {}", c.tool, c.input)),
            )
        })
        .collect()
}

#[tokio::test]
async fn progress_moves_one_block_on_per_step() {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let mut turn = Turn::new("progress: 3", UiLevel::Rich);
    turn.tools = vec![Arc::new(Stub("bash")), Arc::new(Stub("web_search"))];
    let (msgs, _) = turn.run().await;
    let bash = Some(r#"bash {"command":"true"}"#.to_string());
    assert_eq!(
        progress_run(&msgs),
        [
            (s(&["active", "pending", "pending"]), bash.clone()),
            (s(&["done", "active", "pending"]), bash),
            (s(&["done", "done", "done"]), None),
        ]
    );
    // Without bash the calls are searches; without any tool, one message.
    let mut turn = Turn::new("progress: 2", UiLevel::Rich);
    turn.tools = vec![Arc::new(Stub("web_search"))];
    let (msgs, _) = turn.run().await;
    let search = Some(r#"web_search {"query":"progress"}"#.to_string());
    assert_eq!(
        progress_run(&msgs),
        [
            (s(&["active", "pending"]), search),
            (s(&["done", "done"]), None)
        ]
    );
    let (msgs, _) = Turn::new("progress: 3", UiLevel::Rich).run().await;
    assert_eq!(progress_run(&msgs), [(s(&["done", "done", "done"]), None)]);
}
