//! The native agent loop: stream a completion, run requested tools (through the
//! host's approval gate), feed results back, repeat until the model stops.

pub mod llm;
pub mod mock;
pub mod provider;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use harness_core::backend::{AgentBackend, TurnCtx, TurnOutcome};
use harness_core::event::*;
use harness_core::{RawEvent, Secret, new_id, secret::scrub};
use harness_tools::{ToolCtx, ToolOutcome, ToolSet};
use tokio::sync::mpsc;

pub use provider::{ChatMsg, ChatRequest, Completion, Effort, Provider, ProviderError, ToolSpec};

/// Flush coalesced deltas at least this often...
pub const FLUSH_INTERVAL: Duration = Duration::from_millis(75);
/// ...or when this many bytes are buffered.
pub const FLUSH_BYTES: usize = 2048;

const MAX_STEPS: usize = 50;

pub struct NativeConfig {
    pub system_prompt: String,
    /// Env vars removed from tool processes (they hold secrets).
    pub env_remove: Vec<String>,
    /// Values scrubbed from tool output before it is stored or sent to the model.
    pub secrets: Vec<Secret<String>>,
    pub allow_outside_workspace: bool,
    pub tool_timeout: Duration,
    /// Reasoning effort requested from the provider, if the model supports it.
    pub effort: Option<Effort>,
}

impl Default for NativeConfig {
    fn default() -> Self {
        Self {
            system_prompt: default_system_prompt(),
            env_remove: Vec::new(),
            secrets: Vec::new(),
            allow_outside_workspace: false,
            tool_timeout: Duration::from_secs(600),
            effort: None,
        }
    }
}

pub fn default_system_prompt() -> String {
    format!(
        "You are a coding agent running inside {}. You work in the user's repository through tools. \
         Read before you edit, keep changes minimal, and verify with commands when you can. \
         Be concise.",
        harness_core::APP_NAME
    )
}

pub struct NativeBackend {
    provider: Arc<dyn Provider>,
    tools: ToolSet,
    config: NativeConfig,
}

impl NativeBackend {
    pub fn new(provider: Arc<dyn Provider>, tools: ToolSet, config: NativeConfig) -> Self {
        Self {
            provider,
            tools,
            config,
        }
    }

    /// Tools offered to the model: the runtime's tools, narrowed to what the
    /// agent asks for. (Node policy still gates execution separately.)
    fn tool_specs(&self, agent: Option<&harness_core::agent::AgentSpec>) -> Vec<ToolSpec> {
        self.tools
            .iter()
            .filter(|t| agent.is_none_or(|a| a.tools.allows(t.name())))
            .map(|t| ToolSpec {
                name: t.name().to_string(),
                description: t.description().to_string(),
                schema: t.schema(),
            })
            .collect()
    }
}

fn parse_effort(s: &str) -> Option<Effort> {
    match s {
        "low" => Some(Effort::Low),
        "medium" => Some(Effort::Medium),
        "high" => Some(Effort::High),
        "max" => Some(Effort::Max),
        _ => None,
    }
}

/// Rebuild the provider conversation from a session log. Tool calls that never
/// got a result (interrupted turns) get a synthetic one so the transcript stays
/// valid for providers that require every call to be answered.
pub fn history_to_messages(history: &[RawEvent]) -> Vec<ChatMsg> {
    let mut out = Vec::new();
    let mut open: Vec<String> = Vec::new();
    let close_open = |out: &mut Vec<ChatMsg>, open: &mut Vec<String>| {
        for call_id in open.drain(..) {
            out.push(ChatMsg::Tool {
                call_id,
                content: "[no result: the turn was interrupted]".into(),
            });
        }
    };
    for e in history {
        let Ok(k) = e.typed() else { continue };
        match k {
            EventKind::UserMessage(m) => {
                close_open(&mut out, &mut open);
                out.push(ChatMsg::User(m.text));
            }
            EventKind::AssistantMessage(m) => {
                close_open(&mut out, &mut open);
                open = m.tool_calls.iter().map(|c| c.call_id.clone()).collect();
                if !m.text.is_empty() || !m.tool_calls.is_empty() {
                    out.push(ChatMsg::Assistant {
                        text: m.text,
                        tool_calls: m.tool_calls,
                    });
                }
            }
            EventKind::ToolCallFinished(f) => {
                if let Some(pos) = open.iter().position(|c| *c == f.call_id) {
                    open.remove(pos);
                    out.push(ChatMsg::Tool {
                        call_id: f.call_id,
                        content: f.output,
                    });
                }
            }
            _ => {}
        }
    }
    close_open(&mut out, &mut open);
    out
}

enum Step<T> {
    Done(T),
    Cancelled,
}

/// Drive `fut` while coalescing chunks from `rx` into flushed batches.
async fn coalesce<T, F>(
    fut: F,
    mut rx: mpsc::UnboundedReceiver<String>,
    cancel: &tokio_util::sync::CancellationToken,
    host: &dyn harness_core::backend::TurnHost,
    make: impl Fn(String) -> EventKind + Send + Sync,
) -> Step<T>
where
    F: std::future::Future<Output = T>,
{
    tokio::pin!(fut);
    let mut buf = String::new();
    let mut tick = tokio::time::interval(FLUSH_INTERVAL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let out = loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break Step::Cancelled,
            r = &mut fut => break Step::Done(r),
            Some(d) = rx.recv() => {
                buf.push_str(&d);
                if buf.len() >= FLUSH_BYTES {
                    host.emit(make(std::mem::take(&mut buf))).await;
                }
            }
            _ = tick.tick() => {
                if !buf.is_empty() {
                    host.emit(make(std::mem::take(&mut buf))).await;
                }
            }
        }
    };
    while let Ok(d) = rx.try_recv() {
        buf.push_str(&d);
    }
    if !buf.is_empty() {
        host.emit(make(buf)).await;
    }
    out
}

#[async_trait::async_trait]
impl AgentBackend for NativeBackend {
    fn name(&self) -> &'static str {
        "native"
    }

    async fn run_turn(&self, ctx: TurnCtx<'_>) -> TurnOutcome {
        let host = ctx.host;
        let agent = ctx.agent.as_ref();
        let author = agent.map(|a| a.agent_id.clone());
        let base_prompt = agent
            .map(|a| a.system_prompt.as_str())
            .unwrap_or(&self.config.system_prompt);
        let system = if ctx.cwd.as_os_str().is_empty() {
            base_prompt.to_string()
        } else {
            format!("{base_prompt}\n\nWorking directory: {}", ctx.cwd.display())
        };
        let mut messages = vec![ChatMsg::System(system)];
        messages.extend(history_to_messages(&ctx.history));
        messages.push(ChatMsg::User(ctx.user_text.clone()));
        let tools = self.tool_specs(agent);
        // The master only puts an effort on the spec when the model supports it.
        let effort = agent
            .and_then(|a| a.effort.as_deref())
            .and_then(parse_effort)
            .or(self.config.effort);
        let mut usage = Usage::default();
        let mut allowed_for_session: HashSet<String> = HashSet::new();

        for _ in 0..MAX_STEPS {
            let message_id = new_id();
            let req = ChatRequest {
                model: ctx.model.clone(),
                messages: messages.clone(),
                tools: tools.clone(),
                effort,
            };
            let (tx, rx) = mpsc::unbounded_channel();
            let turn_id = ctx.turn_id.clone();
            let mid = message_id.clone();
            let step = coalesce(
                self.provider.complete(&req, tx),
                rx,
                &ctx.cancel,
                host,
                |text| {
                    EventKind::AssistantDelta(AssistantDelta {
                        author: author.clone(),
                        variant: None,
                        turn_id: turn_id.clone(),
                        message_id: mid.clone(),
                        text,
                    })
                },
            )
            .await;
            let completion = match step {
                Step::Cancelled => {
                    return TurnOutcome {
                        reason: TurnEndReason::Cancelled,
                        usage: Some(usage),
                        error: None,
                    };
                }
                Step::Done(Err(e)) => {
                    return TurnOutcome {
                        reason: TurnEndReason::Error,
                        usage: Some(usage),
                        error: Some(e.to_string()),
                    };
                }
                Step::Done(Ok(c)) => c,
            };
            if let Some(u) = &completion.usage {
                usage.input_tokens += u.input_tokens;
                usage.output_tokens += u.output_tokens;
            }
            host.emit(EventKind::AssistantMessage(AssistantMessage {
                author: author.clone(),
                variant: None,
                turn_id: ctx.turn_id.clone(),
                message_id,
                text: completion.text.clone(),
                tool_calls: completion.tool_calls.clone(),
            }))
            .await;
            messages.push(ChatMsg::Assistant {
                text: completion.text,
                tool_calls: completion.tool_calls.clone(),
            });
            if completion.tool_calls.is_empty() {
                return TurnOutcome {
                    reason: TurnEndReason::Completed,
                    usage: Some(usage),
                    error: None,
                };
            }

            for call in completion.tool_calls {
                host.emit(EventKind::ToolCallStarted(ToolCallStarted {
                    author: author.clone(),
                    turn_id: ctx.turn_id.clone(),
                    call_id: call.call_id.clone(),
                    tool: call.tool.clone(),
                    input: call.input.clone(),
                }))
                .await;
                let outcome = self
                    .run_tool(&ctx, host, &call, &mut allowed_for_session)
                    .await;
                let cancelled = ctx.cancel.is_cancelled();
                let output = scrub(&outcome.output, &self.config.secrets);
                host.emit(EventKind::ToolCallFinished(ToolCallFinished {
                    call_id: call.call_id.clone(),
                    ok: outcome.ok,
                    output: output.clone(),
                    exit_code: outcome.exit_code,
                    truncated: outcome.truncated,
                }))
                .await;
                messages.push(ChatMsg::Tool {
                    call_id: call.call_id,
                    content: output,
                });
                if cancelled {
                    return TurnOutcome {
                        reason: TurnEndReason::Cancelled,
                        usage: Some(usage),
                        error: None,
                    };
                }
            }
        }
        TurnOutcome {
            reason: TurnEndReason::Error,
            usage: Some(usage),
            error: Some(format!("stopped after {MAX_STEPS} steps")),
        }
    }
}

impl NativeBackend {
    async fn run_tool(
        &self,
        ctx: &TurnCtx<'_>,
        host: &dyn harness_core::backend::TurnHost,
        call: &ToolCallRef,
        allowed_for_session: &mut HashSet<String>,
    ) -> ToolOutcome {
        let offered = ctx
            .agent
            .as_ref()
            .is_none_or(|a| a.tools.allows(&call.tool));
        let Some(tool) = self
            .tools
            .iter()
            .find(|t| t.name() == call.tool)
            .filter(|_| offered)
        else {
            return ToolOutcome::err(format!("unknown tool `{}`", call.tool));
        };
        if !call.input.is_object() {
            return ToolOutcome::err(format!(
                "tool arguments must be a JSON object, got: {}",
                call.input
            ));
        }
        if !allowed_for_session.contains(tool.name()) {
            let decision = host
                .approve(&call.call_id, tool.name(), &call.input, tool.risk())
                .await;
            match decision {
                harness_core::policy::Decision::Deny => {
                    return ToolOutcome::err(if ctx.cancel.is_cancelled() {
                        "[cancelled before approval]".to_string()
                    } else {
                        "The user denied this tool call. Ask how to proceed or try something else."
                            .to_string()
                    });
                }
                harness_core::policy::Decision::AllowSession => {
                    allowed_for_session.insert(tool.name().to_string());
                }
                harness_core::policy::Decision::Allow => {}
            }
        }
        let (tx, rx) = mpsc::unbounded_channel();
        let (out_tx, mut out_rx) = mpsc::unbounded_channel();
        let tctx = ToolCtx {
            workspace: ctx.cwd.clone(),
            cancel: ctx.cancel.child_token(),
            output: Some(out_tx),
            allow_outside_workspace: self.config.allow_outside_workspace,
            timeout: self.config.tool_timeout,
            env_remove: self.config.env_remove.clone(),
        };
        // stdout and stderr are interleaved into one coalesced stream (as a
        // terminal shows them); the final result carries both.
        let fwd = tokio::spawn(async move {
            while let Some((_stream, s)) = out_rx.recv().await {
                if tx.send(s).is_err() {
                    break;
                }
            }
        });
        let call_id = call.call_id.clone();
        let secrets = &self.config.secrets;
        // The tool itself observes cancellation (and kills its process group),
        // so we must not abandon its future: use a token that never fires here.
        let never = tokio_util::sync::CancellationToken::new();
        let step = coalesce(
            tool.run(&tctx, call.input.clone()),
            rx,
            &never,
            host,
            |text| {
                EventKind::ToolOutputDelta(ToolOutputDelta {
                    call_id: call_id.clone(),
                    stream: OutputStream::Stdout,
                    text: scrub(&text, secrets),
                })
            },
        )
        .await;
        fwd.abort();
        match step {
            Step::Done(o) => o,
            Step::Cancelled => ToolOutcome::err("[cancelled]"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::backend::TurnHost;
    use harness_core::policy::{Decision, ToolRisk};
    use harness_core::{RawEvent, SessionId};
    use serde_json::{Value, json};
    use std::sync::Mutex;

    struct RecHost {
        events: Mutex<Vec<EventKind>>,
        decision: Decision,
        asked: Mutex<Vec<String>>,
    }

    impl RecHost {
        fn new(decision: Decision) -> Self {
            Self {
                events: Mutex::new(vec![]),
                decision,
                asked: Mutex::new(vec![]),
            }
        }
        fn kinds(&self) -> Vec<&'static str> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .map(|e| e.name())
                .collect()
        }
    }

    #[async_trait::async_trait]
    impl TurnHost for RecHost {
        async fn emit(&self, kind: EventKind) {
            self.events.lock().unwrap().push(kind);
        }
        async fn approve(&self, _c: &str, tool: &str, _i: &Value, risk: ToolRisk) -> Decision {
            if risk == ToolRisk::ReadOnly {
                return Decision::Allow;
            }
            self.asked.lock().unwrap().push(tool.to_string());
            self.decision
        }
    }

    fn backend() -> NativeBackend {
        NativeBackend::new(
            Arc::new(mock::Mock {
                delay: Duration::from_millis(1),
            }),
            harness_tools::builtin(),
            NativeConfig::default(),
        )
    }

    fn ctx<'a>(host: &'a RecHost, text: &str, cwd: &std::path::Path) -> TurnCtx<'a> {
        TurnCtx {
            turn_id: "t".into(),
            history: vec![],
            user_text: text.into(),
            cwd: cwd.to_path_buf(),
            model: "mock".into(),
            agent: None,
            cancel: Default::default(),
            host,
        }
    }

    fn scout() -> harness_core::agent::AgentSpec {
        harness_core::agent::AgentSpec {
            agent_id: "scout".into(),
            name: "Scout".into(),
            system_prompt: "read only".into(),
            tools: harness_core::agent::ToolSelection::List(vec!["read".into()]),
            model: None,
            effort: None,
            budget: Default::default(),
            prompt_hash: "h".into(),
        }
    }

    /// An agent that does not list `bash` cannot run it, even if the model
    /// asks; and everything it produces carries its id as author.
    #[tokio::test]
    async fn agent_tool_list_is_enforced_and_author_set() {
        let dir = tempfile::tempdir().unwrap();
        let host = RecHost::new(Decision::Allow);
        let mut c = ctx(&host, "run: touch hacked", dir.path());
        c.agent = Some(scout());
        let b = backend();
        assert_eq!(b.tool_specs(c.agent.as_ref()).len(), 1);
        b.run_turn(c).await;
        assert!(!dir.path().join("hacked").exists());
        assert!(host.asked.lock().unwrap().is_empty(), "never even asked");
        let ev = host.events.lock().unwrap();
        assert!(ev.iter().any(|e| matches!(e,
            EventKind::ToolCallFinished(f) if !f.ok && f.output.contains("unknown tool"))));
        assert!(ev.iter().all(|e| match e {
            EventKind::AssistantMessage(m) => m.author.as_deref() == Some("scout"),
            EventKind::AssistantDelta(d) => d.author.as_deref() == Some("scout"),
            EventKind::ToolCallStarted(t) => t.author.as_deref() == Some("scout"),
            _ => true,
        }));
    }

    #[tokio::test]
    async fn echo_turn() {
        let dir = tempfile::tempdir().unwrap();
        let host = RecHost::new(Decision::Allow);
        let out = backend()
            .run_turn(ctx(&host, "hello there", dir.path()))
            .await;
        assert_eq!(out.reason, TurnEndReason::Completed);
        let ev = host.events.lock().unwrap();
        let EventKind::AssistantMessage(m) = ev.last().unwrap() else {
            panic!()
        };
        assert_eq!(m.text, "You said: hello there");
        // Deltas were coalesced (fewer events than words) and sum to the text.
        let deltas: String = ev
            .iter()
            .filter_map(|e| match e {
                EventKind::AssistantDelta(d) => Some(d.text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(deltas, m.text);
    }

    #[tokio::test]
    async fn tool_turn_with_approval() {
        let dir = tempfile::tempdir().unwrap();
        let host = RecHost::new(Decision::Allow);
        let out = backend()
            .run_turn(ctx(&host, "run: echo hi", dir.path()))
            .await;
        assert_eq!(out.reason, TurnEndReason::Completed);
        assert_eq!(*host.asked.lock().unwrap(), vec!["bash".to_string()]);
        let kinds = host.kinds();
        assert!(kinds.contains(&"tool_call_started"));
        assert!(kinds.contains(&"tool_call_finished"));
        let ev = host.events.lock().unwrap();
        let EventKind::AssistantMessage(m) = ev.last().unwrap() else {
            panic!()
        };
        assert_eq!(m.text, "Tool said: hi");
    }

    #[tokio::test]
    async fn denied_tool_reports_to_model() {
        let dir = tempfile::tempdir().unwrap();
        let host = RecHost::new(Decision::Deny);
        let out = backend()
            .run_turn(ctx(&host, "run: touch should-not-exist", dir.path()))
            .await;
        assert_eq!(out.reason, TurnEndReason::Completed);
        assert!(!dir.path().join("should-not-exist").exists());
        let ev = host.events.lock().unwrap();
        assert!(ev.iter().any(|e| matches!(e,
            EventKind::ToolCallFinished(f) if !f.ok && f.output.contains("denied"))));
    }

    #[tokio::test]
    async fn secrets_are_scrubbed_from_tool_output() {
        let dir = tempfile::tempdir().unwrap();
        let host = RecHost::new(Decision::Allow);
        let b = NativeBackend::new(
            Arc::new(mock::Mock {
                delay: Duration::from_millis(1),
            }),
            harness_tools::builtin(),
            NativeConfig {
                secrets: vec![Secret::new("sk-supersecret-123".into())],
                ..Default::default()
            },
        );
        b.run_turn(ctx(&host, "run: echo sk-supersecret-123", dir.path()))
            .await;
        // Inputs are scrubbed by the node at append time; the backend is
        // responsible for what the tool produced (and what the model sees).
        let outputs: Vec<String> = host
            .events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                EventKind::ToolOutputDelta(d) => Some(d.text.clone()),
                EventKind::ToolCallFinished(f) => Some(f.output.clone()),
                _ => None,
            })
            .collect();
        let all = outputs.concat();
        assert!(!all.contains("sk-supersecret-123"), "{all}");
        assert!(all.contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn cancel_mid_stream() {
        let dir = tempfile::tempdir().unwrap();
        let host = RecHost::new(Decision::Allow);
        let c = ctx(&host, "slow: 1000", dir.path());
        let cancel = c.cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            cancel.cancel();
        });
        let out = backend().run_turn(c).await;
        assert_eq!(out.reason, TurnEndReason::Cancelled);
    }

    #[test]
    fn history_closes_interrupted_calls() {
        let sid = SessionId::new();
        let mk = |seq, k: EventKind| {
            let (kind, payload) = k.into_parts();
            RawEvent {
                session_id: sid,
                seq,
                ts_ms: 0,
                kind,
                payload,
            }
        };
        let h = vec![
            mk(
                1,
                EventKind::UserMessage(UserMessage {
                    turn_id: "t".into(),
                    text: "hi".into(),
                }),
            ),
            mk(
                2,
                EventKind::AssistantMessage(AssistantMessage {
                    author: None,
                    variant: None,
                    turn_id: "t".into(),
                    message_id: "m".into(),
                    text: "".into(),
                    tool_calls: vec![ToolCallRef {
                        call_id: "c1".into(),
                        tool: "bash".into(),
                        input: json!({}),
                    }],
                }),
            ),
            mk(
                3,
                EventKind::UserMessage(UserMessage {
                    turn_id: "t2".into(),
                    text: "again".into(),
                }),
            ),
        ];
        let m = history_to_messages(&h);
        assert_eq!(m.len(), 4);
        assert!(matches!(&m[2], ChatMsg::Tool { call_id, .. } if call_id == "c1"));
        assert!(matches!(&m[3], ChatMsg::User(t) if t == "again"));
    }
}
