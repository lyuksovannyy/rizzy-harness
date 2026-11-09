//! The native agent loop: stream a completion, run requested tools (through the
//! host's approval gate), feed results back, repeat until the model stops.

pub mod images;
pub mod llm;
pub mod mask;
pub mod mock;
pub mod models;
pub mod provider;
pub mod style;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use harness_core::backend::{AgentBackend, TurnCtx, TurnOutcome};
use harness_core::event::*;
use harness_core::refs::{RefBook, SharedRefs};
use harness_core::{RawEvent, new_id};
use harness_tools::{ToolCtx, ToolOutcome, ToolSet};
use tokio::sync::mpsc;

pub use provider::{
    ChatMsg, ChatRequest, Completion, Effort, ImagePart, Provider, ProviderError, ToolSpec,
    estimate_tokens,
};

/// Flush coalesced deltas at least this often...
pub const FLUSH_INTERVAL: Duration = Duration::from_millis(75);
/// ...or when this many bytes are buffered.
pub const FLUSH_BYTES: usize = 2048;

const MAX_STEPS: usize = 50;

/// The orchestration tool (handled by the loop itself, not a `Tool`).
pub const DELEGATE: &str = "delegate";

pub struct NativeConfig {
    pub system_prompt: String,
    /// Env vars removed from tool processes (they hold secrets).
    pub env_remove: Vec<String>,
    /// Values scrubbed from tool output before it is stored or sent to the model.
    pub secrets: harness_core::SecretSet,
    pub allow_outside_workspace: bool,
    pub tool_timeout: Duration,
    /// Reasoning effort requested from the provider, if the model supports it.
    pub effort: Option<Effort>,
    /// Summarise older turns once the history is estimated above this many
    /// tokens (0 = never).
    pub summarize_after_tokens: usize,
}

impl Default for NativeConfig {
    fn default() -> Self {
        Self {
            system_prompt: default_system_prompt(),
            env_remove: Vec::new(),
            secrets: Default::default(),
            allow_outside_workspace: false,
            tool_timeout: Duration::from_secs(600),
            effort: None,
            summarize_after_tokens: 0,
        }
    }
}

/// System prompt of the summarising call (the mock recognises it).
pub const SUMMARY_PROMPT: &str = "Summarize the conversation below for your own later use. \
Keep facts, decisions, names, numbers, file paths, open questions and the user's stated \
preferences; drop pleasantries. Write at most 300 words in the conversation's language.";

/// Turns kept verbatim after a summary.
const KEEP_TURNS: usize = 2;

/// The latest summary in a history, and the events after it.
pub fn split_summary(history: &[RawEvent]) -> (Option<String>, &[RawEvent]) {
    let mut found: Option<(String, u64)> = None;
    for e in history {
        if let Ok(EventKind::HistorySummarized(h)) = e.typed() {
            found = Some((h.text, h.through_seq));
        }
    }
    match found {
        None => (None, history),
        Some((text, through)) => {
            let start = history.partition_point(|e| e.seq <= through);
            (Some(text), &history[start..])
        }
    }
}

/// Plain transcript for the summariser (tool output shortened).
fn transcript(prev: Option<&str>, events: &[RawEvent]) -> String {
    let mut out = String::new();
    if let Some(p) = prev {
        out.push_str(&format!("Earlier summary:\n{p}\n\n"));
    }
    for m in history_to_messages(events) {
        match &m {
            // Images reach the summariser by name only.
            ChatMsg::User(_) | ChatMsg::UserImages { .. } => {
                let t = images::as_text(&m).unwrap_or_default();
                out.push_str(&format!("User: {t}\n\n"));
            }
            ChatMsg::Assistant { text, tool_calls } => {
                if !text.is_empty() {
                    out.push_str(&format!("Assistant: {text}\n\n"));
                }
                for c in tool_calls {
                    out.push_str(&format!("Assistant called {} {}\n", c.tool, c.input));
                }
            }
            ChatMsg::Tool { content, .. } => {
                let short: String = content.chars().take(2000).collect();
                out.push_str(&format!("Tool result: {short}\n\n"));
            }
            ChatMsg::System(_) => {}
        }
    }
    out
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
    /// Full outputs of filtered commands (`tool_output`).
    outputs: Arc<harness_tools::OutputStore>,
    repo_maps: std::sync::Mutex<RepoMaps>,
}

type RepoMaps =
    std::collections::HashMap<(std::path::PathBuf, usize), (std::time::Instant, Option<String>)>;

const REPO_MAP_TTL: Duration = Duration::from_secs(3600);

impl NativeBackend {
    pub fn new(provider: Arc<dyn Provider>, tools: ToolSet, config: NativeConfig) -> Self {
        Self {
            provider,
            tools,
            config,
            outputs: Arc::default(),
            repo_maps: Default::default(),
        }
    }

    /// Tools offered to the model: the runtime's tools, narrowed to what the
    /// agent asks for. (Node policy still gates execution separately.)
    fn tool_specs(
        &self,
        agent: Option<&harness_core::agent::AgentSpec>,
        savings: &harness_core::savings::Savings,
    ) -> Vec<ToolSpec> {
        let mut specs: Vec<ToolSpec> = self
            .tools
            .iter()
            .filter(|t| t.enabled(savings))
            .filter(|t| agent.is_none_or(|a| a.allows_tool(t.name())))
            .map(|t| ToolSpec {
                name: t.name().to_string(),
                description: t.description().to_string(),
                schema: t.schema(),
            })
            .collect();
        if let Some(a) = agent.filter(|a| !a.delegates.is_empty())
            && !a.deny_tools.iter().any(|t| t == DELEGATE)
        {
            let caps = a.caps.unwrap_or_default();
            let roster: Vec<String> = a
                .delegates
                .iter()
                .map(|d| format!("{} ({})", d.agent_id, d.name))
                .collect();
            specs.push(ToolSpec {
                name: DELEGATE.into(),
                description: format!(
                    "Hand a self-contained sub-task to a specialist, who works in its own session \
                     on the same machine and returns its final answer. Available: {}. Several \
                     calls in one step run in parallel. Limits per turn: {} sub-agents, {} tokens.",
                    roster.join(", "),
                    caps.max_subagents,
                    caps.max_tokens
                ),
                schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "agent": {
                            "type": "string",
                            "enum": a.delegates.iter().map(|d| d.agent_id.clone()).collect::<Vec<_>>()
                        },
                        "task": { "type": "string", "description": "Everything the specialist needs to know" }
                    },
                    "required": ["agent", "task"]
                }),
            });
        }
        specs
    }

    /// Scrubbed like the output, so a source never shows what the output
    /// it labels hides.
    fn scrub_sources(&self, sources: Vec<Source>) -> Vec<Source> {
        sources
            .into_iter()
            .map(|s| Source {
                url: self.config.secrets.scrub(&s.url),
                title: self.config.secrets.scrub(&s.title),
                ..s
            })
            .collect()
    }

    /// Run one `delegate` call. Returns the result and the child's usage.
    /// The child labels its web results from `refs`, this turn's book.
    async fn run_delegate(
        &self,
        ctx: &TurnCtx<'_>,
        host: &dyn harness_core::backend::TurnHost,
        call: &ToolCallRef,
        refs: &SharedRefs,
    ) -> (ToolOutcome, Usage) {
        let none = Usage::default();
        let Some(agent) = ctx.agent.as_ref() else {
            return (ToolOutcome::err("delegation needs an agent"), none);
        };
        let target = call
            .input
            .get("agent")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let task = call
            .input
            .get("task")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let Some(spec) = agent.delegates.iter().find(|d| d.agent_id == target) else {
            return (
                ToolOutcome::err(format!(
                    "`{target}` is not an agent this one can delegate to"
                )),
                none,
            );
        };
        if task.is_empty() {
            return (ToolOutcome::err("`task` must describe the sub-task"), none);
        }
        let (tx, mut rx) = mpsc::unbounded_channel();
        let started: Box<dyn FnOnce(harness_core::SessionId) + Send> = Box::new(move |sid| {
            let _ = tx.send(sid);
        });
        let run = host.delegate(
            spec.clone(),
            task,
            refs.clone(),
            ctx.cancel.child_token(),
            started,
        );
        let call_id = call.call_id.clone();
        // Link to the child as soon as it exists, while it still works.
        let link = async {
            if let Some(sid) = rx.recv().await {
                host.emit(EventKind::ToolOutputDelta(ToolOutputDelta {
                    call_id: call_id.clone(),
                    stream: OutputStream::Stdout,
                    text: format!("session: {sid}\n"),
                }))
                .await;
            }
        };
        let (res, ()) = tokio::join!(run, link);
        match res {
            Err(e) => (ToolOutcome::err(e), none),
            Ok(o) => {
                let ok = o.reason == TurnEndReason::Completed;
                let reason = format!("{:?}", o.reason).to_lowercase();
                let body = if o.text.trim().is_empty() {
                    "(no answer)".to_string()
                } else {
                    o.text
                };
                (
                    ToolOutcome {
                        ok,
                        output: format!(
                            "session: {}\n@{} ended ({reason}):\n{body}",
                            o.child, spec.agent_id
                        ),
                        // The labels the answer cites resolve through these
                        // in the parent, as they did in the child.
                        sources: o.sources,
                        ..Default::default()
                    },
                    o.usage.unwrap_or_default(),
                )
            }
        }
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
    // Parallel answers: only the chosen one (default: the first) is history.
    let mut chosen: std::collections::HashMap<String, String> = Default::default();
    for e in history {
        if let Ok(EventKind::VariantSelected(v)) = e.typed() {
            chosen.insert(v.group, v.message_id);
        }
    }
    let keep = |m: &AssistantMessage| match &m.variant {
        None => true,
        Some(v) => match chosen.get(&v.group) {
            Some(id) => *id == m.message_id,
            None => v.index == 0,
        },
    };
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
                out.push(images::user_msg(m.model_text(), &m.images));
            }
            EventKind::AssistantMessage(m) if !keep(&m) => {}
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

/// What the user and tools said in this conversation (not the model, not
/// the system prompt): the provenance set for network tools.
fn observed_text(messages: &[ChatMsg]) -> Vec<String> {
    messages
        .iter()
        .filter_map(|m| match m {
            ChatMsg::Tool { content, .. } => Some(content.clone()),
            other => other.user_text().map(str::to_string),
        })
        .collect()
}

enum Step<T> {
    Done(T),
    Cancelled,
}

/// Drive `fut` while coalescing chunks from `rx` into flushed batches.
async fn coalesce<T, F>(
    fut: F,
    rx: mpsc::UnboundedReceiver<String>,
    cancel: &tokio_util::sync::CancellationToken,
    host: &dyn harness_core::backend::TurnHost,
    make: impl Fn(String) -> EventKind + Send + Sync,
) -> Step<T>
where
    F: std::future::Future<Output = T>,
{
    coalesce_led(fut, rx, cancel, host, &mut None, make).await.0
}

/// `coalesce`, emitting `lead` (taken) just before the first batch. Also
/// says whether any batch was emitted.
async fn coalesce_led<T, F>(
    fut: F,
    mut rx: mpsc::UnboundedReceiver<String>,
    cancel: &tokio_util::sync::CancellationToken,
    host: &dyn harness_core::backend::TurnHost,
    lead: &mut Option<EventKind>,
    make: impl Fn(String) -> EventKind + Send + Sync,
) -> (Step<T>, bool)
where
    F: std::future::Future<Output = T>,
{
    tokio::pin!(fut);
    let mut buf = String::new();
    let mut emitted = false;
    let mut tick = tokio::time::interval(FLUSH_INTERVAL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let flush = async |buf: String, lead: &mut Option<EventKind>| {
        if let Some(l) = lead.take() {
            host.emit(l).await;
        }
        host.emit(make(buf)).await;
    };
    let out = loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break Step::Cancelled,
            r = &mut fut => break Step::Done(r),
            Some(d) = rx.recv() => {
                buf.push_str(&d);
                if buf.len() >= FLUSH_BYTES {
                    flush(std::mem::take(&mut buf), lead).await;
                    emitted = true;
                }
            }
            _ = tick.tick() => {
                if !buf.is_empty() {
                    flush(std::mem::take(&mut buf), lead).await;
                    emitted = true;
                }
            }
        }
    };
    while let Ok(d) = rx.try_recv() {
        buf.push_str(&d);
    }
    if !buf.is_empty() {
        flush(buf, lead).await;
        emitted = true;
    }
    (out, emitted)
}

/// The models a turn may use, in order: its own, then the agent's
/// fallbacks, leaving out repeats and the ones the master marked
/// unavailable. When that leaves nothing, the turn's own model is tried.
fn model_chain(model: &str, agent: Option<&harness_core::agent::AgentSpec>) -> Vec<String> {
    let Some(a) = agent else {
        return vec![model.to_string()];
    };
    let mut out: Vec<String> = Vec::new();
    for m in std::iter::once(model).chain(a.fallbacks.iter().map(String::as_str)) {
        if !a.unavailable.iter().any(|u| u == m) && !out.iter().any(|o| o == m) {
            out.push(m.to_string());
        }
    }
    if out.is_empty() {
        out.push(model.to_string());
    }
    out
}

/// The note that `model` answers this turn instead of `from`.
fn fallback_event(turn_id: &str, model: &str, from: &str, reason: String) -> EventKind {
    let mut reason = reason;
    if reason.chars().count() > 200 {
        reason = reason.chars().take(199).collect::<String>() + "…";
    }
    EventKind::ModelFallback(ModelFallback {
        turn_id: turn_id.to_string(),
        model: model.to_string(),
        from: from.to_string(),
        reason: Some(reason),
    })
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
        let sv = &ctx.extra.savings;
        let keep = sv.keep_tool_outputs.map(|k| k as usize);
        let mut system = if ctx.cwd.as_os_str().is_empty() {
            base_prompt.to_string()
        } else {
            format!("{base_prompt}\n\nWorking directory: {}", ctx.cwd.display())
        };
        // Which ui blocks this turn's replies may show; enforced below when
        // each message is recorded, not only asked for in the prompt.
        let ui = harness_core::ui::effective_level(
            agent.map(|a| a.ui).unwrap_or_default(),
            ctx.parent_session.is_some(),
        );
        for part in [style::fragment(sv.style, ui), style::ui_fragment(ui)] {
            if !part.is_empty() {
                system = format!("{system}\n\n{part}");
            }
        }
        if let Some(map) = self.repo_map(&ctx).await {
            system = format!("{system}\n\n{map}");
        }
        let mut usage = Usage::default();
        let (mut summary, mut recent) = split_summary(&ctx.history);
        let limit = self.config.summarize_after_tokens;
        let history_tokens = |events: &[RawEvent]| {
            let mut m = history_to_messages(events);
            if let Some(k) = keep {
                mask::mask_old_outputs(&mut m, k);
            }
            provider::estimate_tokens(&m) as usize
        };
        if limit > 0
            && history_tokens(recent) > limit
            && let Some((text, through_seq, u)) =
                self.summarize(&ctx, summary.as_deref(), recent).await
        {
            usage.add(&u);
            host.emit(EventKind::HistorySummarized(HistorySummarized {
                turn_id: ctx.turn_id.clone(),
                through_seq,
                text: text.clone(),
            }))
            .await;
            let start = recent.partition_point(|e| e.seq <= through_seq);
            recent = &recent[start..];
            summary = Some(text);
        }
        let system = match &summary {
            Some(t) => format!("{system}\n\nSummary of the earlier conversation:\n{t}"),
            None => system,
        };
        let mut messages = vec![ChatMsg::System(system)];
        messages.extend(history_to_messages(recent));
        // Per-turn context sits after the history: the system prompt and the
        // history stay byte-identical from turn to turn (prompt cache).
        let user_text = match &ctx.extra.context {
            Some(c) => format!("<context>\n{c}\n</context>\n\n{}", ctx.user_text),
            None => ctx.user_text.clone(),
        };
        messages.push(images::user_msg(user_text, &ctx.extra.images));
        let wanted = images::wanted(&messages);
        let loaded = if wanted.is_empty() {
            Vec::new()
        } else {
            host.images(&wanted).await
        };
        images::attach(&mut messages, &loaded);
        if !ctx.compare.is_empty() {
            return self.compare(&ctx, messages, usage, author, ui).await;
        }
        let tools = self.tool_specs(agent, sv);
        // The master only puts an effort on the spec when the model supports it.
        let effort = agent
            .and_then(|a| a.effort.as_deref())
            .and_then(parse_effort)
            .or(self.config.effort);
        let mut allowed_for_session: HashSet<String> = HashSet::new();
        let (mut subagents, mut sub_tokens) = (0u32, 0u64);
        // Citation labels continue from the whole log (not only the part
        // after a summary), so `[S3]` names one page for the whole session.
        // A sub-task numbers on in its parent's book instead: its answer goes
        // back to the parent verbatim, labels and all.
        let refs = ctx
            .extra
            .refs
            .clone()
            .unwrap_or_else(|| Arc::new(std::sync::Mutex::new(RefBook::seed(&ctx.history))));

        // The model answering this turn. Until the turn has any output, a
        // model-level failure moves on to the next of the agent's fallbacks;
        // the note saying so goes out just before the answer.
        let mut chain = std::collections::VecDeque::from(model_chain(&ctx.model, agent));
        let mut model = chain.pop_front().unwrap_or_else(|| ctx.model.clone());
        // Why the turn's own model is not the one answering: it is
        // unavailable, or the first error it gave; a later fallback's own
        // failure only goes to the log.
        let mut from_reason = (model != ctx.model).then(|| "unavailable".to_string());
        let mut lead = from_reason
            .clone()
            .map(|r| fallback_event(&ctx.turn_id, &model, &ctx.model, r));
        let mut started = false;
        for _ in 0..MAX_STEPS {
            let message_id = new_id();
            let mut sent = messages.clone();
            if let Some(k) = keep {
                mask::mask_old_outputs(&mut sent, k);
            }
            let req = ChatRequest {
                model: model.clone(),
                messages: sent,
                tools: tools.clone(),
                // Only for models that take one (a fallback may not).
                effort: effort.filter(|_| !agent.is_some_and(|a| a.no_effort.contains(&model))),
                cache: sv.prompt_cache,
            };
            let (tx, rx) = mpsc::unbounded_channel();
            let turn_id = ctx.turn_id.clone();
            let mid = message_id.clone();
            let (step, emitted) = coalesce_led(
                self.provider.complete(&req, tx),
                rx,
                &ctx.cancel,
                host,
                &mut lead,
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
            started |= emitted;
            let completion = match step {
                Step::Cancelled => {
                    return TurnOutcome {
                        reason: TurnEndReason::Cancelled,
                        usage: Some(usage),
                        error: None,
                    };
                }
                Step::Done(Err(e)) if !started && e.is_model_level() && !chain.is_empty() => {
                    let next = chain.pop_front().unwrap_or_default();
                    let why = self.config.secrets.scrub(&e.to_string());
                    tracing::warn!(failed = %model, next = %next, error = %why, "model failed; trying a fallback");
                    let reason = from_reason.get_or_insert(why).clone();
                    lead = Some(fallback_event(&ctx.turn_id, &next, &ctx.model, reason));
                    model = next;
                    continue;
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
            if let Some(l) = lead.take() {
                host.emit(l).await;
            }
            started = true;
            if let Some(u) = &completion.usage {
                usage.add(u);
                usage.context_tokens = u.input_tokens;
            }
            host.emit(self.assistant_message(
                AssistantMessage {
                    author: author.clone(),
                    variant: None,
                    turn_id: ctx.turn_id.clone(),
                    message_id,
                    text: completion.text.clone(),
                    tool_calls: completion.tool_calls.clone(),
                    blocks: vec![],
                },
                ui,
            ))
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

            // Delegations of this step run in parallel, within the caps.
            let (delegations, calls): (Vec<_>, Vec<_>) = completion
                .tool_calls
                .into_iter()
                .partition(|c| c.tool == DELEGATE && tools.iter().any(|t| t.name == DELEGATE));
            if !delegations.is_empty() {
                let caps = agent.and_then(|a| a.caps).unwrap_or_default();
                let mut admitted = Vec::new();
                for call in delegations {
                    host.emit(EventKind::ToolCallStarted(ToolCallStarted {
                        author: author.clone(),
                        turn_id: ctx.turn_id.clone(),
                        call_id: call.call_id.clone(),
                        tool: call.tool.clone(),
                        input: call.input.clone(),
                    }))
                    .await;
                    let refusal = if subagents >= caps.max_subagents {
                        Some(format!(
                            "cap reached: at most {} sub-agents per turn",
                            caps.max_subagents
                        ))
                    } else if sub_tokens >= caps.max_tokens {
                        Some(format!(
                            "cap reached: sub-agents used {sub_tokens} of {} tokens this turn",
                            caps.max_tokens
                        ))
                    } else {
                        None
                    };
                    match refusal {
                        Some(r) => {
                            host.emit(EventKind::ToolCallFinished(ToolCallFinished {
                                call_id: call.call_id.clone(),
                                ok: false,
                                output: r.clone(),
                                exit_code: None,
                                truncated: false,
                                sources: vec![],
                            }))
                            .await;
                            messages.push(ChatMsg::Tool {
                                call_id: call.call_id,
                                content: r,
                            });
                        }
                        None => {
                            subagents += 1;
                            admitted.push(call);
                        }
                    }
                }
                let results = futures::future::join_all(
                    admitted
                        .iter()
                        .map(|c| self.run_delegate(&ctx, host, c, &refs)),
                )
                .await;
                for (call, (outcome, u)) in admitted.into_iter().zip(results) {
                    sub_tokens += u.input_tokens + u.output_tokens;
                    usage.add(&u);
                    let output = self.config.secrets.scrub(&outcome.output);
                    host.emit(EventKind::ToolCallFinished(ToolCallFinished {
                        call_id: call.call_id.clone(),
                        ok: outcome.ok,
                        output: output.clone(),
                        exit_code: None,
                        truncated: outcome.truncated,
                        sources: self.scrub_sources(outcome.sources),
                    }))
                    .await;
                    messages.push(ChatMsg::Tool {
                        call_id: call.call_id,
                        content: output,
                    });
                }
                if ctx.cancel.is_cancelled() {
                    return TurnOutcome {
                        reason: TurnEndReason::Cancelled,
                        usage: Some(usage),
                        error: None,
                    };
                }
            }
            for call in calls {
                host.emit(EventKind::ToolCallStarted(ToolCallStarted {
                    author: author.clone(),
                    turn_id: ctx.turn_id.clone(),
                    call_id: call.call_id.clone(),
                    tool: call.tool.clone(),
                    input: call.input.clone(),
                }))
                .await;
                let observed = observed_text(&messages);
                let outcome = self
                    .run_tool(
                        &ctx,
                        host,
                        &call,
                        &mut allowed_for_session,
                        &observed,
                        &refs,
                    )
                    .await;
                let cancelled = ctx.cancel.is_cancelled();
                let output = self.config.secrets.scrub(&outcome.output);
                host.emit(EventKind::ToolCallFinished(ToolCallFinished {
                    call_id: call.call_id.clone(),
                    ok: outcome.ok,
                    output: output.clone(),
                    exit_code: outcome.exit_code,
                    truncated: outcome.truncated,
                    sources: self.scrub_sources(outcome.sources),
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
    /// The repository map for this turn's working folder, if enabled. Built
    /// once per folder and kept for an hour: it sits in the system prompt,
    /// so rebuilding it every turn would defeat the prompt cache.
    async fn repo_map(&self, ctx: &TurnCtx<'_>) -> Option<String> {
        let budget = ctx.extra.savings.repo_map_tokens? as usize;
        if ctx.cwd.as_os_str().is_empty() {
            return None;
        }
        let key = (ctx.cwd.clone(), budget);
        if let Some((at, map)) = self
            .repo_maps
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&key)
            && at.elapsed() < REPO_MAP_TTL
        {
            return map.clone();
        }
        let cwd = ctx.cwd.clone();
        let map = tokio::task::spawn_blocking(move || harness_codemap::repo_map(&cwd, budget))
            .await
            .ok()
            .flatten();
        self.repo_maps
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key, (std::time::Instant::now(), map.clone()));
        map
    }

    /// The event for a finished message: its text as it will be stored
    /// (the node scrubs every payload; doing it here first makes the
    /// blocks' hashes match the stored text) and the blocks found in it. A
    /// message that calls tools does not end the turn, so it may not ask.
    fn assistant_message(
        &self,
        mut m: AssistantMessage,
        ui: harness_core::agent::UiLevel,
    ) -> EventKind {
        m.text = self.config.secrets.scrub(&m.text);
        m.blocks = harness_core::ui::extract(&m.text, ui);
        if !m.tool_calls.is_empty() {
            harness_core::ui::calls_tools(&mut m.blocks);
        }
        EventKind::AssistantMessage(m)
    }

    /// Parallel answers: one completion per model, no tools, each recorded
    /// as a variant of the same group (the turn id).
    async fn compare(
        &self,
        ctx: &TurnCtx<'_>,
        messages: Vec<ChatMsg>,
        mut usage: Usage,
        author: Option<String>,
        ui: harness_core::agent::UiLevel,
    ) -> TurnOutcome {
        let host = ctx.host;
        let runs = ctx.compare.iter().enumerate().map(|(i, model)| {
            let messages = messages.clone();
            let author = author.clone();
            async move {
                let message_id = new_id();
                let variant = Variant {
                    group: ctx.turn_id.clone(),
                    index: i as u32,
                    model: Some(model.clone()),
                };
                let req = ChatRequest {
                    model: model.clone(),
                    messages,
                    tools: vec![],
                    effort: None,
                    cache: ctx.extra.savings.prompt_cache,
                };
                let (tx, rx) = mpsc::unbounded_channel();
                let (turn_id, mid, v2, a2) = (
                    ctx.turn_id.clone(),
                    message_id.clone(),
                    variant.clone(),
                    author.clone(),
                );
                let step = coalesce(
                    self.provider.complete(&req, tx),
                    rx,
                    &ctx.cancel,
                    host,
                    |text| {
                        EventKind::AssistantDelta(AssistantDelta {
                            author: a2.clone(),
                            variant: Some(v2.clone()),
                            turn_id: turn_id.clone(),
                            message_id: mid.clone(),
                            text,
                        })
                    },
                )
                .await;
                match step {
                    Step::Done(Ok(c)) => {
                        host.emit(self.assistant_message(
                            AssistantMessage {
                                author,
                                variant: Some(variant),
                                turn_id: ctx.turn_id.clone(),
                                message_id,
                                text: c.text,
                                tool_calls: vec![],
                                blocks: vec![],
                            },
                            ui,
                        ))
                        .await;
                        Ok(c.usage)
                    }
                    Step::Done(Err(e)) => Err(format!("{model}: {e}")),
                    Step::Cancelled => Err("cancelled".into()),
                }
            }
        });
        let results = futures::future::join_all(runs).await;
        let mut errors = vec![];
        for r in results {
            match r {
                Ok(Some(u)) => {
                    usage.input_tokens += u.input_tokens;
                    usage.output_tokens += u.output_tokens;
                    usage.context_tokens = usage.context_tokens.max(u.input_tokens);
                }
                Ok(None) => {}
                Err(e) => errors.push(e),
            }
        }
        if ctx.cancel.is_cancelled() {
            return TurnOutcome {
                reason: TurnEndReason::Cancelled,
                usage: Some(usage),
                error: None,
            };
        }
        TurnOutcome {
            reason: if errors.len() == ctx.compare.len() {
                TurnEndReason::Error
            } else {
                TurnEndReason::Completed
            },
            usage: Some(usage),
            error: (!errors.is_empty()).then(|| errors.join("; ")),
        }
    }

    /// Summarise everything before the last `KEEP_TURNS` turns. Returns the
    /// summary, the last seq it covers and the call's usage; `None` when
    /// there is nothing old enough or the call fails (the turn then simply
    /// sends the full history).
    async fn summarize(
        &self,
        ctx: &TurnCtx<'_>,
        prev: Option<&str>,
        recent: &[RawEvent],
    ) -> Option<(String, u64, Usage)> {
        let starts: Vec<u64> = recent
            .iter()
            .filter(|e| e.kind == "turn_started")
            .map(|e| e.seq)
            .collect();
        if starts.len() <= KEEP_TURNS {
            return None;
        }
        let through = starts[starts.len() - KEEP_TURNS] - 1;
        let old = &recent[..recent.partition_point(|e| e.seq <= through)];
        // If the kept turns alone are what is large, summarising the rest
        // saves little and would repeat on every turn: skip it.
        let old_tokens = provider::estimate_tokens(&history_to_messages(old)) as usize;
        if old_tokens < self.config.summarize_after_tokens / 4 {
            return None;
        }
        let req = ChatRequest {
            model: ctx.model.clone(),
            messages: vec![
                ChatMsg::System(SUMMARY_PROMPT.into()),
                ChatMsg::User(transcript(prev, old)),
            ],
            tools: vec![],
            effort: None,
            cache: false,
        };
        let (tx, _rx) = mpsc::unbounded_channel();
        let c = tokio::select! {
            r = self.provider.complete(&req, tx) => r.ok()?,
            _ = ctx.cancel.cancelled() => return None,
        };
        let text = c.text.trim().to_string();
        if text.is_empty() {
            return None;
        }
        Some((text, through, c.usage.unwrap_or_default()))
    }

    async fn run_tool(
        &self,
        ctx: &TurnCtx<'_>,
        host: &dyn harness_core::backend::TurnHost,
        call: &ToolCallRef,
        allowed_for_session: &mut HashSet<String>,
        observed: &[String],
        refs: &SharedRefs,
    ) -> ToolOutcome {
        let offered = ctx.agent.as_ref().is_none_or(|a| a.allows_tool(&call.tool));
        let Some(tool) = self
            .tools
            .iter()
            .find(|t| t.name() == call.tool)
            .filter(|t| offered && t.enabled(&ctx.extra.savings))
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
            observed: Arc::new(observed.to_vec()),
            savings: ctx.extra.savings.clone(),
            outputs: Some(self.outputs.clone()),
            refs: refs.clone(),
            artifacts: host.artifacts(),
            call_id: call.call_id.clone(),
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
                    text: secrets.scrub(&text),
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
            compare: vec![],
            parent_session: None,
            extra: Default::default(),
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
            fallbacks: vec![],
            unavailable: vec![],
            no_effort: vec![],
            effort: None,
            budget: Default::default(),
            ui: Default::default(),
            prompt_hash: "h".into(),
            deny_tools: vec![],
            delegates: vec![],
            caps: None,
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
        assert_eq!(b.tool_specs(c.agent.as_ref(), &Default::default()).len(), 1);
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

    /// The rich renderer's demo reaches the log as written: the browser, not
    /// the agent loop, strips hidden characters and decides what is math.
    #[tokio::test]
    async fn mock_rich_demo() {
        let dir = tempfile::tempdir().unwrap();
        let host = RecHost::new(Decision::Allow);
        let out = backend()
            .run_turn(ctx(&host, "rich: demo", dir.path()))
            .await;
        assert_eq!(out.reason, TurnEndReason::Completed);
        let ev = host.events.lock().unwrap();
        let EventKind::AssistantMessage(m) = ev.last().unwrap() else {
            panic!()
        };
        for part in [
            "| core | 12.5 | 40 |",
            "```rust\n",
            "\\(a^2+b^2=c^2\\)",
            "$$\n\\int_0^1",
            "$5 and $10",
            "> [!WARNING] Check before you run",
            "![x](https://img.example/p.png?q=s)",
            "[x](https://evil.example/a?q=1)",
            "\\label{user-content-fn-1}",
            "\\(\\char\"202E x\\)",
            "| query | [y](https://evil.example/t?q=2) |",
            "[&#x202E;moc.knab](https://evil.example/)",
            "abc\u{202E}def",
        ] {
            assert!(m.text.contains(part), "missing {part:?}");
        }
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
                secrets: harness_core::SecretSet::new(vec![harness_core::Secret::new(
                    "sk-supersecret-123".into(),
                )]),
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

    /// Stands in for the web tools: labels the page in `query` (a URL) and
    /// one fixed page, the way web_search does.
    struct FakeSearch;

    #[async_trait::async_trait]
    impl harness_tools::Tool for FakeSearch {
        fn name(&self) -> &'static str {
            "web_search"
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
        async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome {
            let urls = [
                input["query"].as_str().unwrap_or(""),
                "https://fixed.example/",
            ];
            let sources: Vec<Source> = urls
                .iter()
                .map(|u| Source {
                    id: ctx.label(u),
                    url: u.to_string(),
                    title: format!("Title sk-supersecret-123 {u}"),
                })
                .collect();
            let output = sources
                .iter()
                .map(|s| format!("[{}] {}\n    {}", s.id, s.title, s.url))
                .collect::<Vec<_>>()
                .join("\n");
            ToolOutcome {
                sources,
                ..ToolOutcome::ok(output)
            }
        }
    }

    /// Labels continue from the session's log, the same page keeps its
    /// label, the finished event carries the sources (scrubbed like the
    /// output), and the mock's `cite:` answer cites what the result gave.
    #[tokio::test]
    async fn tool_results_carry_session_stable_sources() {
        let dir = tempfile::tempdir().unwrap();
        let host = RecHost::new(Decision::Allow);
        let b = NativeBackend::new(
            Arc::new(mock::Mock {
                delay: Duration::from_millis(1),
            }),
            vec![Arc::new(FakeSearch)],
            NativeConfig {
                secrets: harness_core::SecretSet::new(vec![harness_core::Secret::new(
                    "sk-supersecret-123".into(),
                )]),
                ..Default::default()
            },
        );
        let sid = SessionId::new();
        let earlier = |seq: u64, id: &str, url: &str| {
            let (kind, payload) = EventKind::ToolCallFinished(ToolCallFinished {
                call_id: format!("old{seq}"),
                ok: true,
                output: String::new(),
                exit_code: None,
                truncated: false,
                sources: vec![Source {
                    id: id.into(),
                    url: url.into(),
                    title: "old".into(),
                }],
            })
            .into_parts();
            RawEvent {
                session_id: sid,
                seq,
                ts_ms: 0,
                kind,
                payload,
            }
        };
        let mut c = ctx(&host, "cite: https://new.example/", dir.path());
        c.history = vec![
            earlier(2, "S1", "https://fixed.example/"),
            earlier(3, "S3", "https://other.example/"),
        ];
        assert_eq!(b.run_turn(c).await.reason, TurnEndReason::Completed);
        let ev = host.events.lock().unwrap();
        let f = ev
            .iter()
            .find_map(|e| match e {
                EventKind::ToolCallFinished(f) => Some(f.clone()),
                _ => None,
            })
            .unwrap();
        let ids: Vec<(&str, &str)> = f
            .sources
            .iter()
            .map(|s| (s.id.as_str(), s.url.as_str()))
            .collect();
        assert_eq!(
            ids,
            [
                ("S4", "https://new.example/"),
                ("S1", "https://fixed.example/")
            ]
        );
        assert!(f.output.starts_with("[S4] "), "{}", f.output);
        for s in &f.sources {
            assert!(!s.title.contains("sk-supersecret"), "{}", s.title);
            assert!(s.title.contains("[REDACTED]"), "{}", s.title);
        }
        let EventKind::AssistantMessage(m) = ev.last().unwrap() else {
            panic!("{:?}", ev.last())
        };
        assert_eq!(m.text, "Answer [S4] and [S1], both [S4][S1]. Unknown [S9].");
    }

    /// Runs a delegated task as a child turn on the same backend, the way
    /// the node's session does: own events, the parent's book, and the
    /// child's sources handed back.
    struct DelegatingHost {
        parent: RecHost,
        child: RecHost,
        backend: Arc<NativeBackend>,
        cwd: std::path::PathBuf,
    }

    #[async_trait::async_trait]
    impl TurnHost for DelegatingHost {
        async fn emit(&self, kind: EventKind) {
            self.parent.emit(kind).await
        }
        async fn approve(&self, c: &str, tool: &str, i: &Value, risk: ToolRisk) -> Decision {
            self.parent.approve(c, tool, i, risk).await
        }
        async fn delegate(
            &self,
            agent: harness_core::agent::AgentSpec,
            task: String,
            refs: SharedRefs,
            cancel: tokio_util::sync::CancellationToken,
            started: Box<dyn FnOnce(SessionId) + Send>,
        ) -> Result<harness_core::backend::DelegateOutcome, String> {
            let child = SessionId::new();
            started(child);
            let mut c = ctx(&self.child, &task, &self.cwd);
            c.agent = Some(agent);
            c.extra.refs = Some(refs);
            c.cancel = cancel;
            let out = self.backend.run_turn(c).await;
            let ev = self.child.events.lock().unwrap();
            let text = ev
                .iter()
                .rev()
                .find_map(|e| match e {
                    EventKind::AssistantMessage(m) if !m.text.is_empty() => Some(m.text.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            let sources = ev
                .iter()
                .filter_map(|e| match e {
                    EventKind::ToolCallFinished(f) => Some(f.sources.clone()),
                    _ => None,
                })
                .flatten()
                .collect();
            Ok(harness_core::backend::DelegateOutcome {
                child,
                text,
                reason: out.reason,
                usage: out.usage,
                sources,
            })
        }
    }

    /// A sub-task numbers its web results on from the parent's book, and the
    /// delegate result carries the child's sources, so the labels in the
    /// answer it hands back name the same pages in the parent.
    #[tokio::test]
    async fn delegated_answers_cite_in_the_parents_numbering() {
        let dir = tempfile::tempdir().unwrap();
        let backend = Arc::new(NativeBackend::new(
            Arc::new(mock::Mock {
                delay: Duration::from_millis(1),
            }),
            vec![Arc::new(FakeSearch)],
            NativeConfig::default(),
        ));
        let host = DelegatingHost {
            parent: RecHost::new(Decision::Allow),
            child: RecHost::new(Decision::Allow),
            backend: backend.clone(),
            cwd: dir.path().to_path_buf(),
        };
        let researcher = harness_core::agent::AgentSpec {
            agent_id: "researcher".into(),
            tools: harness_core::agent::ToolSelection::List(vec!["web_search".into()]),
            ..scout()
        };
        let lead = harness_core::agent::AgentSpec {
            agent_id: "lead".into(),
            tools: harness_core::agent::ToolSelection::List(vec![]),
            delegates: vec![researcher],
            ..scout()
        };
        let (kind, payload) = EventKind::ToolCallFinished(ToolCallFinished {
            call_id: "old".into(),
            ok: true,
            output: String::new(),
            exit_code: None,
            truncated: false,
            sources: vec![
                Source {
                    id: "S1".into(),
                    url: "https://fixed.example/".into(),
                    title: "old".into(),
                },
                Source {
                    id: "S2".into(),
                    url: "https://other.example/".into(),
                    title: "old".into(),
                },
            ],
        })
        .into_parts();
        let mut c = ctx(
            &host.parent,
            "delegate: researcher=cite: https://new.example/",
            dir.path(),
        );
        c.agent = Some(lead);
        c.history = vec![RawEvent {
            session_id: SessionId::new(),
            seq: 1,
            ts_ms: 0,
            kind,
            payload,
        }];
        // Coerce the host to the trait object the context holds.
        let parent_host: &dyn TurnHost = &host;
        c.host = parent_host;
        assert_eq!(backend.run_turn(c).await.reason, TurnEndReason::Completed);

        let labels = |sources: &[Source]| -> Vec<(String, String)> {
            sources
                .iter()
                .map(|s| (s.id.clone(), s.url.clone()))
                .collect()
        };
        let want = vec![
            ("S3".to_string(), "https://new.example/".to_string()),
            ("S1".to_string(), "https://fixed.example/".to_string()),
        ];
        let child = host.child.events.lock().unwrap();
        let searched = child
            .iter()
            .find_map(|e| match e {
                EventKind::ToolCallFinished(f) => Some(f.sources.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(labels(&searched), want);
        drop(child);

        let parent = host.parent.events.lock().unwrap();
        let delegated = parent
            .iter()
            .find_map(|e| match e {
                EventKind::ToolCallFinished(f) => Some(f.clone()),
                _ => None,
            })
            .unwrap();
        assert!(
            delegated.output.contains("Answer [S3] and [S1]"),
            "{}",
            delegated.output
        );
        assert_eq!(labels(&delegated.sources), want);
        let EventKind::AssistantMessage(m) = parent.last().unwrap() else {
            panic!("{:?}", parent.last())
        };
        assert_eq!(
            m.text,
            "Sub-agent said: Answer [S3] and [S1], both [S3][S1]. Unknown [S9]."
        );
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
                    attachments: vec![],
                    reply: None,
                    images: vec![],
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
                    blocks: vec![],
                }),
            ),
            mk(
                3,
                EventKind::UserMessage(UserMessage {
                    turn_id: "t2".into(),
                    text: "again".into(),
                    attachments: vec![],
                    reply: None,
                    images: vec![],
                }),
            ),
        ];
        let m = history_to_messages(&h);
        assert_eq!(m.len(), 4);
        assert!(matches!(&m[2], ChatMsg::Tool { call_id, .. } if call_id == "c1"));
        assert!(matches!(&m[3], ChatMsg::User(t) if t == "again"));
    }

    /// Records every request, answers like the mock.
    struct Rec {
        inner: mock::Mock,
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
            self.reqs.lock().unwrap().push(req.clone());
            self.inner.complete(req, deltas).await
        }
    }

    fn turn_events(sid: SessionId, seq: &mut u64, n: usize) -> Vec<RawEvent> {
        let mut out = vec![];
        let mut push = |k: EventKind, out: &mut Vec<RawEvent>| {
            *seq += 1;
            let (kind, payload) = k.into_parts();
            out.push(RawEvent {
                session_id: sid,
                seq: *seq,
                ts_ms: 0,
                kind,
                payload,
            });
        };
        let t = format!("t{n}");
        push(
            EventKind::TurnStarted(TurnStarted {
                agent: None,
                turn_id: t.clone(),
            }),
            &mut out,
        );
        push(
            EventKind::UserMessage(UserMessage {
                turn_id: t.clone(),
                text: format!("question {n} {}", "word ".repeat(200)),
                attachments: vec![],
                reply: None,
                images: vec![],
            }),
            &mut out,
        );
        push(
            EventKind::AssistantMessage(AssistantMessage {
                author: None,
                variant: None,
                turn_id: t.clone(),
                message_id: format!("m{n}"),
                text: format!("answer {n}"),
                tool_calls: vec![],
                blocks: vec![],
            }),
            &mut out,
        );
        push(
            EventKind::TurnEnded(TurnEnded {
                turn_id: t,
                reason: TurnEndReason::Completed,
                usage: None,
            }),
            &mut out,
        );
        out
    }

    #[tokio::test]
    async fn long_history_is_summarised_once_and_reused() {
        let sid = SessionId::new();
        let mut seq = 1; // seq 1 = session_created (not needed here)
        let mut history = vec![];
        for n in 1..=4 {
            history.extend(turn_events(sid, &mut seq, n));
        }
        let rec = Arc::new(Rec {
            inner: mock::Mock {
                delay: Duration::from_millis(0),
            },
            reqs: Mutex::new(vec![]),
        });
        let b = NativeBackend::new(
            rec.clone(),
            vec![],
            NativeConfig {
                summarize_after_tokens: 300,
                ..Default::default()
            },
        );
        let host = RecHost::new(Decision::Allow);
        let dir = tempfile::tempdir().unwrap();
        let mut c = ctx(&host, "next", dir.path());
        c.history = history.clone();
        let out = b.run_turn(c).await;
        assert_eq!(out.reason, TurnEndReason::Completed);
        let events = host.events.lock().unwrap().clone();
        let EventKind::HistorySummarized(h) = &events[0] else {
            panic!("{events:?}")
        };
        // Turns 1-2 summarised, turns 3-4 kept: turn 3 starts at seq 10.
        assert_eq!(h.through_seq, 9);
        assert_eq!(h.text, "Summary of 2 earlier messages.");
        let reqs = rec.reqs.lock().unwrap();
        assert_eq!(reqs.len(), 2);
        let main = &reqs[1];
        let ChatMsg::System(sys) = &main.messages[0] else {
            panic!()
        };
        assert!(sys.contains("Summary of the earlier conversation:\nSummary of 2"));
        let users: Vec<_> = main
            .messages
            .iter()
            .filter_map(|m| match m {
                ChatMsg::User(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(users.len(), 3, "turns 3, 4 and the new message");
        assert!(users[0].starts_with("question 3"));
        // Usage covers the summary call too.
        assert!(out.usage.unwrap().input_tokens > estimate_tokens(&main.messages));

        // Next turn: the stored summary is reused, no new summarising call
        // while the recent part stays small.
        drop(reqs);
        let mut history2 = history;
        seq += 1;
        let (kind, payload) = events[0].clone().into_parts();
        history2.push(RawEvent {
            session_id: sid,
            seq,
            ts_ms: 0,
            kind,
            payload,
        });
        let (sum, recent) = split_summary(&history2);
        assert_eq!(sum.as_deref(), Some("Summary of 2 earlier messages."));
        assert_eq!(recent.first().map(|e| e.seq), Some(10));
    }

    #[tokio::test]
    async fn compare_runs_models_in_parallel_and_history_follows_the_choice() {
        let rec = Arc::new(Rec {
            inner: mock::Mock {
                delay: Duration::from_millis(5),
            },
            reqs: Mutex::new(vec![]),
        });
        let b = NativeBackend::new(
            rec.clone(),
            harness_tools::builtin(),
            NativeConfig::default(),
        );
        let host = RecHost::new(Decision::Allow);
        let dir = tempfile::tempdir().unwrap();
        let mut c = ctx(&host, "hello", dir.path());
        c.compare = vec!["mock".into(), "mock-big".into()];
        let out = b.run_turn(c).await;
        assert_eq!(out.reason, TurnEndReason::Completed);
        assert!(
            rec.reqs.lock().unwrap().iter().all(|r| r.tools.is_empty()),
            "no tools in compare turns"
        );
        let evs = host.events.lock().unwrap().clone();
        let answers: Vec<AssistantMessage> = evs
            .iter()
            .filter_map(|e| match e {
                EventKind::AssistantMessage(m) => Some(m.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(answers.len(), 2);
        let big = answers
            .iter()
            .find(|m| m.variant.as_ref().unwrap().model.as_deref() == Some("mock-big"))
            .unwrap();
        assert!(big.text.starts_with("[mock-big] You said: hello"));

        // History: first answer by default, the chosen one after a selection.
        let sid = harness_core::SessionId::new();
        let mut seq = 0;
        let mut raw: Vec<RawEvent> = evs
            .into_iter()
            .map(|k| {
                seq += 1;
                let (kind, payload) = k.into_parts();
                RawEvent {
                    session_id: sid,
                    seq,
                    ts_ms: 0,
                    kind,
                    payload,
                }
            })
            .collect();
        let texts = |raw: &[RawEvent]| -> Vec<String> {
            history_to_messages(raw)
                .into_iter()
                .filter_map(|m| match m {
                    ChatMsg::Assistant { text, .. } => Some(text),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(texts(&raw), ["You said: hello"]);
        let (kind, payload) = EventKind::VariantSelected(VariantSelected {
            group: big.variant.as_ref().unwrap().group.clone(),
            message_id: big.message_id.clone(),
        })
        .into_parts();
        raw.push(RawEvent {
            session_id: sid,
            seq: seq + 1,
            ts_ms: 0,
            kind,
            payload,
        });
        assert_eq!(texts(&raw), ["[mock-big] You said: hello"]);
    }
}
