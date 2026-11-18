//! Which model used which tokens, through every path a turn's usage is
//! added up on: the main call, a retry, a fallback after a refusal, a
//! summary between turns and inside a turn, a sub-agent on another model,
//! parallel answers, a callback turn and a hand-off note. The provider
//! keeps a ledger of the usage it reported per model; the turn's
//! `by_model` must equal it exactly (a cost estimate prices each bucket at
//! its own model's price).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

use harness_agent_native::llm::{ModelRouter, Route};
use harness_agent_native::{
    CODING_SUMMARY_PROMPT, ChatMsg, ChatRequest, Completion, NativeBackend, NativeConfig, Provider,
    ProviderError, RetryHint, RetryPolicy, SUMMARY_PROMPT,
};
use harness_core::agent::{AgentSpec, ToolSelection};
use harness_core::backend::{
    AgentBackend, DelegateOutcome, NoteCtx, TurnCtx, TurnExtra, TurnHost, TurnOutcome,
};
use harness_core::event::*;
use harness_core::policy::{Decision, ToolRisk};
use harness_core::refs::SharedRefs;
use harness_core::{RawEvent, SessionId};
use harness_tools::{ToolCtx, ToolOutcome};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

// ---- the provider ---------------------------------------------------------

/// Answers per raw model id (the router strips the `p::` prefix):
/// `gone` is not found (404), `flaky` is overloaded once (503), `lead`
/// delegates to `reader` once, `noter` writes 60 long notes, every other
/// model answers. Summary requests are answered as summaries. Each answer
/// reports usage that differs by kind, and the ledger adds it up per model.
#[derive(Default)]
struct Script {
    ledger: Mutex<BTreeMap<String, ModelTokens>>,
    flaky_failed: Mutex<bool>,
    noter_steps: Mutex<usize>,
    summaries: Mutex<usize>,
}

impl Script {
    /// What the ledger says, under the prefixed ids the router was asked.
    fn ledger(&self) -> BTreeMap<String, ModelTokens> {
        self.ledger
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (format!("p::{k}"), v.clone()))
            .collect()
    }
}

fn tool_call(i: usize, tool: &str, input: Value) -> Completion {
    Completion {
        tool_calls: vec![ToolCallRef {
            call_id: format!("c{i}"),
            tool: tool.into(),
            input,
        }],
        ..Default::default()
    }
}

#[async_trait::async_trait]
impl Provider for Script {
    fn name(&self) -> &'static str {
        "script"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        _deltas: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        let model = req.model.as_str();
        let summary = matches!(
            req.messages.first(),
            Some(ChatMsg::System(s)) if s == SUMMARY_PROMPT || s == CODING_SUMMARY_PROMPT
        );
        let note = matches!(
            req.messages.first(),
            Some(ChatMsg::System(s)) if s == harness_agent_native::HANDOFF_PROMPT
        );
        match model {
            "gone" => {
                return Err(ProviderError::Status {
                    status: 404,
                    body: "The model `gone` does not exist".into(),
                    hint: RetryHint::default(),
                });
            }
            // Works one step, then its provider refuses it (no fallback):
            // a sub-agent that fails after spending tokens.
            "boom"
                if req
                    .messages
                    .iter()
                    .any(|m| matches!(m, ChatMsg::Tool { .. })) =>
            {
                return Err(ProviderError::Status {
                    status: 401,
                    body: "Incorrect API key".into(),
                    hint: RetryHint::default(),
                });
            }
            "flaky" => {
                let mut failed = self.flaky_failed.lock().unwrap();
                if !*failed {
                    *failed = true;
                    return Err(ProviderError::Status {
                        status: 503,
                        body: "overloaded".into(),
                        hint: RetryHint::default(),
                    });
                }
            }
            _ => {}
        }
        let tokens = if summary {
            *self.summaries.lock().unwrap() += 1;
            ModelTokens {
                input_tokens: 500,
                output_tokens: 50,
                cache_read_tokens: 0,
                cache_write_tokens: 20,
            }
        } else if note {
            ModelTokens {
                input_tokens: 900,
                output_tokens: 60,
                ..Default::default()
            }
        } else {
            ModelTokens {
                input_tokens: 1000,
                output_tokens: 10,
                cache_read_tokens: 400,
                cache_write_tokens: 0,
            }
        };
        let mut c = if summary {
            Completion {
                text: "SUMMARY: read the files.".into(),
                ..Default::default()
            }
        } else if note {
            Completion {
                text: "Status:\nHalfway.".into(),
                ..Default::default()
            }
        } else {
            match model {
                "lead"
                    if !req
                        .messages
                        .iter()
                        .any(|m| matches!(m, ChatMsg::Tool { .. })) =>
                {
                    tool_call(0, "delegate", json!({"agent": "reader", "task": "look"}))
                }
                "boom" => tool_call(0, "note", json!({"text": "x"})),
                "noter" => {
                    let i = {
                        let mut n = self.noter_steps.lock().unwrap();
                        *n += 1;
                        *n - 1
                    };
                    if i < 60 {
                        let big = "n".repeat(3_000);
                        tool_call(i, "note", json!({ "text": format!("{i} {big}") }))
                    } else {
                        Completion {
                            text: "done".into(),
                            ..Default::default()
                        }
                    }
                }
                _ => Completion {
                    text: format!("answer from {model}"),
                    ..Default::default()
                },
            }
        };
        c.usage = Some(Usage {
            input_tokens: tokens.input_tokens,
            output_tokens: tokens.output_tokens,
            cache_read_tokens: tokens.cache_read_tokens,
            cache_write_tokens: tokens.cache_write_tokens,
            ..Default::default()
        });
        self.ledger
            .lock()
            .unwrap()
            .entry(model.to_string())
            .or_default()
            .add(&tokens);
        Ok(c)
    }
}

// ---- the tools and the host -----------------------------------------------

struct Note;

#[async_trait::async_trait]
impl harness_tools::Tool for Note {
    fn name(&self) -> &'static str {
        "note"
    }
    fn description(&self) -> &'static str {
        "write a note"
    }
    fn schema(&self) -> Value {
        json!({"type": "object"})
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }
    async fn run(&self, _: &ToolCtx, _: Value) -> ToolOutcome {
        ToolOutcome::ok("noted")
    }
}

/// Runs a delegated sub-task as the node does: a child turn on the same
/// backend, on the child agent's own model, its outcome's usage returned.
#[derive(Default)]
struct Host {
    backend: OnceLock<Arc<NativeBackend>>,
    events: Mutex<Vec<EventKind>>,
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
    async fn delegate(
        &self,
        agent: AgentSpec,
        task: String,
        _refs: SharedRefs,
        _observed: Arc<Vec<String>>,
        cancel: CancellationToken,
        started: Box<dyn FnOnce(SessionId) + Send>,
    ) -> Result<DelegateOutcome, String> {
        let child = SessionId::new();
        started(child);
        let b = self.backend.get().unwrap().clone();
        let host = Host::default();
        let model = agent.model.clone().unwrap();
        let out = b
            .run_turn(TurnCtx {
                session_id: child,
                turn_id: "child".into(),
                history: vec![],
                user_text: task,
                cwd: Default::default(),
                model,
                agent: Some(agent),
                compare: vec![],
                parent_session: None,
                extra: Default::default(),
                cancel,
                host: &host,
            })
            .await;
        Ok(DelegateOutcome {
            child,
            text: "looked".into(),
            reason: out.reason,
            usage: out.usage,
            sources: vec![],
            finish: None,
            error: None,
            note: None,
            unapplied: None,
            applied: true,
        })
    }
}

fn spec(id: &str, model: Option<&str>, tools: &[&str], fallbacks: &[&str]) -> AgentSpec {
    AgentSpec {
        agent_id: id.into(),
        name: id.into(),
        system_prompt: "Be brief.".into(),
        tools: ToolSelection::List(tools.iter().map(|s| s.to_string()).collect()),
        model: model.map(str::to_string),
        fallbacks: fallbacks.iter().map(|s| s.to_string()).collect(),
        unavailable: vec![],
        no_effort: vec![],
        vision: vec![],
        context_windows: [("p::noter".to_string(), 32_768u64)].into(),
        effort: None,
        ui: Default::default(),
        no_personalisation: false,
        prompt_hash: "h".into(),
        deny_tools: vec![],
        deny_skills: vec![],
        deny_mcp_servers: vec![],
        delegates: vec![],
        caps: None,
    }
}

fn backend(p: Arc<Script>, summarize_after: Option<usize>) -> Arc<NativeBackend> {
    let router = ModelRouter::new(vec![Route {
        prefix: "p".into(),
        provider: p,
    }]);
    Arc::new(NativeBackend::new(
        Arc::new(router),
        vec![Arc::new(Note)],
        NativeConfig {
            summarize_after_tokens: summarize_after,
            retry: RetryPolicy {
                retries: 2,
                base: std::time::Duration::from_millis(1),
                max: std::time::Duration::from_millis(5),
            },
            ..Default::default()
        },
    ))
}

struct Turn {
    model: &'static str,
    agent: AgentSpec,
    history: Vec<RawEvent>,
    compare: Vec<String>,
    extra: TurnExtra,
    sid: SessionId,
}

impl Turn {
    fn new(model: &'static str, agent: AgentSpec) -> Self {
        Self {
            model,
            agent,
            history: vec![],
            compare: vec![],
            extra: Default::default(),
            sid: SessionId::new(),
        }
    }
    async fn run(self, b: &Arc<NativeBackend>) -> TurnOutcome {
        let host = Host::default();
        let _ = host.backend.set(b.clone());
        b.run_turn(TurnCtx {
            session_id: self.sid,
            turn_id: "now".into(),
            history: self.history,
            user_text: "go".into(),
            cwd: Default::default(),
            model: self.model.into(),
            agent: Some(self.agent),
            compare: self.compare,
            parent_session: None,
            extra: self.extra,
            cancel: Default::default(),
            host: &host,
        })
        .await
    }
}

/// The turn's breakdown is exactly what each model reported, and the
/// totals are its sum (nothing unattributed).
fn assert_matches(out: &TurnOutcome, p: &Script) -> BTreeMap<String, ModelTokens> {
    let u = out.usage.clone().unwrap();
    assert_eq!(u.by_model, p.ledger(), "{:?}", out.error);
    let sum = u
        .by_model
        .values()
        .fold(ModelTokens::default(), |mut a, t| {
            a.add(t);
            a
        });
    assert_eq!(
        (
            sum.input_tokens,
            sum.output_tokens,
            sum.cache_read_tokens,
            sum.cache_write_tokens
        ),
        (
            u.input_tokens,
            u.output_tokens,
            u.cache_read_tokens,
            u.cache_write_tokens
        ),
        "every token is attributed"
    );
    u.by_model
}

fn log(sid: SessionId, kinds: Vec<EventKind>) -> Vec<RawEvent> {
    kinds
        .into_iter()
        .enumerate()
        .map(|(i, k)| {
            let (kind, payload) = k.into_parts();
            RawEvent {
                session_id: sid,
                seq: i as u64 + 1,
                ts_ms: 0,
                kind,
                payload,
            }
        })
        .collect()
}

/// `turns` earlier turns of about `chars` characters each.
fn history(sid: SessionId, turns: usize, chars: usize) -> Vec<RawEvent> {
    let mut kinds = vec![];
    for t in 0..turns {
        let turn_id = format!("old{t}");
        kinds.push(EventKind::TurnStarted(TurnStarted {
            turn_id: turn_id.clone(),
            agent: None,
            cause: None,
            environment: None,
            continued_by: None,
            retry: None,
        }));
        kinds.push(EventKind::UserMessage(UserMessage {
            turn_id: turn_id.clone(),
            text: "x".repeat(chars),
            attachments: vec![],
            reply: None,
            images: vec![],
            queued: false,
            files: vec![],
        }));
        kinds.push(EventKind::AssistantMessage(AssistantMessage {
            author: None,
            variant: None,
            turn_id: turn_id.clone(),
            message_id: format!("m{t}"),
            text: "ok".into(),
            tool_calls: vec![],
            blocks: vec![],
            thinking: None,
        }));
        kinds.push(EventKind::TurnEnded(TurnEnded {
            turn_id,
            reason: TurnEndReason::Completed,
            usage: None,
        }));
    }
    log(sid, kinds)
}

// ---- the paths ------------------------------------------------------------

#[tokio::test]
async fn the_main_call_and_a_retry_land_under_their_model() {
    let p = Arc::new(Script::default());
    let b = backend(p.clone(), None);
    let out = Turn::new("p::flaky", spec("a", None, &[], &[]))
        .run(&b)
        .await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    let by = assert_matches(&out, &p);
    assert_eq!(by.keys().collect::<Vec<_>>(), ["p::flaky"]);
    assert_eq!(
        by["p::flaky"].input_tokens, 1000,
        "the failed try cost none"
    );
}

#[tokio::test]
async fn a_fallback_after_a_refusal_lands_under_the_fallback() {
    let p = Arc::new(Script::default());
    let b = backend(p.clone(), None);
    let out = Turn::new("p::gone", spec("a", None, &[], &["p::ok"]))
        .run(&b)
        .await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    let by = assert_matches(&out, &p);
    assert_eq!(by.keys().collect::<Vec<_>>(), ["p::ok"]);
    // The window label follows the fallback too (unchanged behaviour).
    assert_eq!(out.usage.unwrap().model.as_deref(), Some("p::ok"));
}

#[tokio::test]
async fn a_summary_between_turns_lands_under_the_turns_model() {
    let p = Arc::new(Script::default());
    let b = backend(p.clone(), Some(2_000));
    let mut t = Turn::new("p::m", spec("a", None, &[], &[]));
    t.history = history(t.sid, 6, 4_000);
    let out = t.run(&b).await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    assert!(*p.summaries.lock().unwrap() > 0, "a summary was written");
    let by = assert_matches(&out, &p);
    assert_eq!(by["p::m"].cache_write_tokens % 20, 0);
    assert!(by["p::m"].input_tokens > 1000, "the summary's tokens count");
}

#[tokio::test]
async fn an_in_turn_summary_lands_under_the_turns_model() {
    let p = Arc::new(Script::default());
    let b = backend(p.clone(), None);
    let out = Turn::new("p::noter", spec("a", None, &["note"], &[]))
        .run(&b)
        .await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    assert!(
        *p.summaries.lock().unwrap() > 0,
        "the steps were summarised"
    );
    let by = assert_matches(&out, &p);
    assert_eq!(by.keys().collect::<Vec<_>>(), ["p::noter"]);
}

#[tokio::test]
async fn a_sub_agent_on_another_model_lands_under_its_own() {
    // The small model, as a read-only sub-agent is given it (`[savings]
    // small_subagents`: the master puts it in the child's `model`).
    let p = Arc::new(Script::default());
    let b = backend(p.clone(), None);
    let mut lead = spec("lead", None, &[], &[]);
    lead.delegates = vec![spec("reader", Some("p::small"), &[], &["p::lead"])];
    let out = Turn::new("p::lead", lead).run(&b).await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    let by = assert_matches(&out, &p);
    assert_eq!(by.keys().collect::<Vec<_>>(), ["p::lead", "p::small"]);
    assert_eq!(by["p::lead"].input_tokens, 2000, "two lead calls");
    assert_eq!(by["p::small"].input_tokens, 1000, "one child call");
}

#[tokio::test]
async fn parallel_answers_land_under_each_model_with_their_cache() {
    let p = Arc::new(Script::default());
    let b = backend(p.clone(), None);
    let mut t = Turn::new("p::a", spec("x", None, &[], &[]));
    t.compare = vec!["p::a".into(), "p::b".into()];
    let out = t.run(&b).await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    let by = assert_matches(&out, &p);
    assert_eq!(by.keys().collect::<Vec<_>>(), ["p::a", "p::b"]);
    assert_eq!(out.usage.unwrap().cache_read_tokens, 800);
}

#[tokio::test]
async fn a_callback_turn_lands_under_its_model() {
    let p = Arc::new(Script::default());
    let b = backend(p.clone(), None);
    let mut t = Turn::new("p::m", spec("a", None, &[], &[]));
    let mut h = history(t.sid, 1, 20);
    h.extend(log(
        t.sid,
        vec![EventKind::BackgroundEnded(BackgroundEnded {
            child: None,
            bg_id: "bg-1".into(),
            reason: BackgroundReason::Exited,
            exit_code: Some(0),
            signal: None,
            confirmed: BackgroundConfirmed::Ended,
            ran_ms: 10,
            output_bytes: 4,
            tail: "done".into(),
            tail_cut: false,
            delivered: false,
        })],
    ));
    for (i, e) in h.iter_mut().enumerate() {
        e.seq = i as u64 + 1;
    }
    t.history = h;
    t.extra.cause = Some(TurnCause::Callback(vec!["bg-1".into()]));
    let out = t.run(&b).await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    let by = assert_matches(&out, &p);
    assert_eq!(by.keys().collect::<Vec<_>>(), ["p::m"]);
}

#[tokio::test]
async fn a_hand_off_note_lands_under_the_model_that_wrote_it() {
    let p = Arc::new(Script::default());
    let b = backend(p.clone(), None);
    let sid = SessionId::new();
    let out = b
        .write_note(NoteCtx {
            session_id: sid,
            history: history(sid, 2, 200),
            // The small model is not known to this node's router: the
            // chat's own model writes it, and only its call is counted.
            models: vec![("x::small".into(), None), ("p::chat".into(), None)],
            low_effort: vec![],
            cancel: CancellationToken::new(),
        })
        .await
        .unwrap();
    assert_eq!(out.model.as_deref(), Some("p::chat"));
    let u = out.usage.unwrap();
    assert_eq!(u.by_model, p.ledger());
    assert_eq!(u.by_model["p::chat"].input_tokens, 900);
}

#[tokio::test]
async fn a_sub_agent_that_fails_still_counts_what_it_spent() {
    // The child's turn ends in an error after one model call: the node
    // returns its usage whatever the reason, and the parent adds it.
    let p = Arc::new(Script::default());
    let b = backend(p.clone(), None);
    let mut lead = spec("lead", None, &[], &[]);
    lead.delegates = vec![spec("reader", Some("p::boom"), &["note"], &[])];
    let out = Turn::new("p::lead", lead).run(&b).await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    let by = assert_matches(&out, &p);
    assert_eq!(
        by["p::boom"].input_tokens, 1000,
        "the failed child's one call"
    );
}

/// A person's `/compact` (X.20) summarises with the chat's model; its
/// calls land under that model, as every other call does.
#[tokio::test]
async fn a_manual_compact_lands_under_the_chats_model() {
    let p = Arc::new(Script::default());
    let b = backend(p.clone(), None);
    let sid = SessionId::new();
    let out = b
        .compact_now(harness_core::backend::CompactCtx {
            session_id: sid,
            history: history(sid, 4, 4_000),
            model: "p::chat".into(),
            window: None,
            focus: Some("keep the numbers".into()),
            cancel: CancellationToken::new(),
        })
        .await
        .unwrap();
    let harness_core::backend::CompactOutcome::Done { usage, .. } = out else {
        panic!("no summary: {out:?}")
    };
    assert!(*p.summaries.lock().unwrap() > 0, "a summary was written");
    assert_eq!(usage.by_model, p.ledger());
    assert_eq!(usage.by_model.keys().collect::<Vec<_>>(), ["p::chat"]);
}
