//! Compaction keyed to the model's context window, and recovery when the
//! provider rejects a request for being too long: one compaction, one
//! retry, then a clear error. Summaries are made only by a model whose
//! window holds what it must read.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use harness_agent_native::{
    ChatMsg, ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError,
    RetryHint, SUMMARY_PROMPT, estimate_tokens,
};
use harness_core::agent::{AgentSpec, ToolSelection};
use harness_core::backend::{AgentBackend, TurnCtx, TurnHost, TurnOutcome};
use harness_core::event::*;
use harness_core::policy::{Decision, ToolRisk};
use harness_core::{RawEvent, SessionId};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct Host(Mutex<Vec<EventKind>>);

#[async_trait::async_trait]
impl TurnHost for Host {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        self.0.lock().unwrap().push(kind);
        Ok(())
    }
    async fn approve(&self, _: &str, _: &str, _: &Value, _: ToolRisk) -> Decision {
        Decision::Deny
    }
}

impl Host {
    fn summaries(&self) -> Vec<HistorySummarized> {
        let evs = self.0.lock().unwrap();
        evs.iter()
            .filter_map(|k| match k {
                EventKind::HistorySummarized(h) => Some(h.clone()),
                _ => None,
            })
            .collect()
    }
    fn answers(&self) -> usize {
        let evs = self.0.lock().unwrap();
        evs.iter()
            .filter(|k| matches!(k, EventKind::AssistantMessage(_)))
            .count()
    }
}

/// One request the provider saw.
#[derive(Debug, Clone)]
struct Call {
    summary: bool,
    tokens: u64,
    text: String,
}

/// Rejects a request estimated above its window like OpenAI does (400,
/// `context_length_exceeded`); summary requests have their own window.
/// Summaries are numbered; the `n`th summary call may be held for `hold`.
struct Windowed {
    window: u64,
    summary_window: u64,
    hold: Option<(usize, Duration)>,
    calls: Mutex<Vec<Call>>,
}

impl Windowed {
    fn new(window: u64, summary_window: u64) -> Arc<Self> {
        Arc::new(Self {
            window,
            summary_window,
            hold: None,
            calls: Mutex::default(),
        })
    }
    fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }
    fn kinds(&self) -> Vec<&'static str> {
        self.calls()
            .iter()
            .map(|c| if c.summary { "summary" } else { "main" })
            .collect()
    }
}

fn overflow() -> ProviderError {
    ProviderError::Status {
        status: 400,
        body: r#"{"error":{"message":"This model's maximum context length is 8192 tokens. However, your messages resulted in 9000 tokens.","type":"invalid_request_error","param":"messages","code":"context_length_exceeded"}}"#.into(),
        hint: RetryHint::default(),
    }
}

#[async_trait::async_trait]
impl Provider for Windowed {
    fn name(&self) -> &'static str {
        "windowed"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        _deltas: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        let summary =
            matches!(req.messages.first(), Some(ChatMsg::System(s)) if s == SUMMARY_PROMPT);
        let tokens = estimate_tokens(&req.messages);
        let text = req
            .messages
            .iter()
            .filter_map(|m| m.user_text())
            .collect::<Vec<_>>()
            .join("\n");
        let n = {
            let mut calls = self.calls.lock().unwrap();
            calls.push(Call {
                summary,
                tokens,
                text,
            });
            calls.iter().filter(|c| c.summary).count()
        };
        let window = if summary {
            self.summary_window
        } else {
            self.window
        };
        if tokens > window {
            return Err(overflow());
        }
        if summary {
            if let Some((at, d)) = self.hold
                && at == n
            {
                tokio::time::sleep(d).await;
            }
            return Ok(Completion {
                text: format!("SUMMARY{n}"),
                ..Default::default()
            });
        }
        Ok(Completion {
            text: "answer".into(),
            ..Default::default()
        })
    }
}

/// `turns` finished turns whose user message is about `tokens` tokens.
/// seq 1 is the session's creation.
fn history(turns: usize, tokens: usize) -> Vec<RawEvent> {
    let sid = SessionId::new();
    let mut seq = 1;
    let mut out = vec![];
    let mut push = |k: EventKind| {
        seq += 1;
        let (kind, payload) = k.into_parts();
        out.push(RawEvent {
            session_id: sid,
            seq,
            ts_ms: 0,
            kind,
            payload,
        });
    };
    for n in 1..=turns {
        let t = format!("t{n}");
        push(EventKind::TurnStarted(TurnStarted {
            agent: None,
            turn_id: t.clone(),
        }));
        push(EventKind::UserMessage(UserMessage {
            turn_id: t.clone(),
            text: format!("question {n} {}", "w".repeat(tokens * 4)),
            attachments: vec![],
            reply: None,
            images: vec![],
        }));
        push(EventKind::AssistantMessage(AssistantMessage {
            author: None,
            variant: None,
            turn_id: t.clone(),
            message_id: format!("m{n}"),
            text: format!("answer {n}"),
            tool_calls: vec![],
            blocks: vec![],
        }));
        push(EventKind::TurnEnded(TurnEnded {
            turn_id: t,
            reason: TurnEndReason::Completed,
            usage: None,
        }));
    }
    out
}

fn agent(window: Option<u64>) -> AgentSpec {
    AgentSpec {
        agent_id: "general".into(),
        name: "General".into(),
        system_prompt: "Be brief.".into(),
        tools: ToolSelection::List(vec![]),
        model: None,
        fallbacks: vec![],
        unavailable: vec![],
        no_effort: vec![],
        context_windows: window.map(|w| ("m".to_string(), w)).into_iter().collect(),
        effort: None,
        budget: Default::default(),
        ui: Default::default(),
        prompt_hash: "h".into(),
        deny_tools: vec![],
        delegates: vec![],
        caps: None,
    }
}

fn backend(p: Arc<dyn Provider>, configured: Option<usize>) -> NativeBackend {
    NativeBackend::new(
        p,
        Vec::new(),
        NativeConfig {
            summarize_after_tokens: configured,
            ..Default::default()
        },
    )
}

async fn run(
    b: &NativeBackend,
    host: &Host,
    history: Vec<RawEvent>,
    agent: AgentSpec,
    cancel: CancellationToken,
) -> TurnOutcome {
    b.run_turn(TurnCtx {
        session_id: harness_core::SessionId::new(),
        turn_id: "now".into(),
        history,
        user_text: "next".into(),
        cwd: Default::default(),
        model: "m".into(),
        agent: Some(agent),
        compare: vec![],
        parent_session: None,
        extra: Default::default(),
        cancel,
        host,
    })
    .await
}

const BIG: u64 = 10_000_000;

#[tokio::test]
async fn a_large_known_window_does_not_compact_at_24k() {
    // About 32k tokens of history: over the old fixed 24k.
    let h = history(4, 8_000);
    let p = Windowed::new(BIG, BIG);
    let b = backend(p.clone(), None);
    let host = Host::default();
    let out = run(
        &b,
        &host,
        h.clone(),
        agent(Some(200_000)),
        Default::default(),
    )
    .await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    assert_eq!(p.kinds(), ["main"], "no summary below 200k - reserve");
    assert!(host.summaries().is_empty());

    // The window unknown: the fixed 24k still applies.
    let p = Windowed::new(BIG, BIG);
    let b = backend(p.clone(), None);
    let host = Host::default();
    let out = run(&b, &host, h, agent(None), Default::default()).await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    assert_eq!(p.kinds(), ["summary", "main"]);
}

#[tokio::test]
async fn a_small_window_compacts_earlier_than_24k() {
    // About 12k tokens: under 24k, over 16k - 8k reserve.
    let h = history(4, 3_000);
    let p = Windowed::new(BIG, BIG);
    let b = backend(p.clone(), None);
    let host = Host::default();
    let out = run(&b, &host, h, agent(Some(16_384)), Default::default()).await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    assert_eq!(p.kinds(), ["summary", "main"]);
    assert_eq!(host.summaries().len(), 1);
}

#[tokio::test]
async fn a_configured_value_still_wins() {
    let h = history(4, 3_000);
    // Configured above the history: no summary, whatever the window says.
    let p = Windowed::new(BIG, BIG);
    let b = backend(p.clone(), Some(50_000));
    let out = run(
        &b,
        &Host::default(),
        h.clone(),
        agent(Some(16_384)),
        Default::default(),
    )
    .await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    assert_eq!(p.kinds(), ["main"]);
    // Configured below it: a summary, though the window is large.
    let p = Windowed::new(BIG, BIG);
    let b = backend(p.clone(), Some(5_000));
    let out = run(
        &b,
        &Host::default(),
        h,
        agent(Some(200_000)),
        Default::default(),
    )
    .await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    assert_eq!(p.kinds(), ["summary", "main"]);
}

#[tokio::test]
async fn context_overflow_compacts_once_and_retries() {
    // 12k tokens of history against a 10k window the master does not know
    // about (nothing proactive): the provider's rejection is what tells.
    let h = history(4, 3_000);
    let p = Windowed::new(10_000, BIG);
    let b = backend(p.clone(), None);
    let host = Host::default();
    let out = run(&b, &host, h, agent(None), Default::default()).await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    assert_eq!(p.kinds(), ["main", "summary", "main"]);
    let s = host.summaries();
    assert_eq!(s.len(), 1);
    // Turns 1-2 (seq 2..=9) summarised, the last two kept.
    assert_eq!(s[0].through_seq, 9);
    assert_eq!(host.answers(), 1);
}

#[tokio::test]
async fn a_second_overflow_fails_cleanly() {
    let h = history(4, 3_000);
    // Even the compacted request is too long.
    let p = Windowed::new(1_000, BIG);
    let b = backend(p.clone(), None);
    let host = Host::default();
    let t = Instant::now();
    let out = run(&b, &host, h, agent(None), Default::default()).await;
    assert_eq!(out.reason, TurnEndReason::Error);
    assert_eq!(p.kinds(), ["main", "summary", "main"], "one retry, no loop");
    let err = out.error.unwrap();
    assert!(err.contains("context window"), "{err}");
    assert!(
        !err.contains("maximum context length is"),
        "no provider body: {err}"
    );
    assert!(t.elapsed() < Duration::from_secs(5));

    // Nothing to compact: fails at once, without a pointless retry.
    let p = Windowed::new(1_000, BIG);
    let b = backend(p.clone(), None);
    let out = run(
        &b,
        &Host::default(),
        history(1, 3_000),
        agent(None),
        Default::default(),
    )
    .await;
    assert_eq!(out.reason, TurnEndReason::Error);
    assert_eq!(p.kinds(), ["main"]);
}

#[tokio::test]
async fn the_summariser_reads_only_what_fits_its_window() {
    // Old part: turns 1-3 of 2k tokens each (6k) against a summariser
    // window of 5k: summarised in pieces, each piece fitting, each next
    // one reading the previous summary, so the middle is not lost.
    let h = history(5, 2_000);
    let p = Windowed::new(BIG, 5_000);
    let b = backend(p.clone(), Some(1_000));
    let host = Host::default();
    let out = run(&b, &host, h, agent(Some(5_000)), Default::default()).await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    let sums: Vec<Call> = p.calls().into_iter().filter(|c| c.summary).collect();
    assert!(sums.len() >= 2, "{sums:?}");
    assert!(sums.iter().all(|c| c.tokens <= 5_000), "{sums:?}");
    for (i, c) in sums.iter().enumerate().skip(1) {
        assert!(
            c.text.contains(&format!("SUMMARY{i}")),
            "piece {i} lost the earlier summary"
        );
    }
    let s = host.summaries();
    assert_eq!(s.len(), sums.len());
    assert!(s.windows(2).all(|w| w[0].through_seq < w[1].through_seq));
    // Through turn 3 (seq 2..=13) in the end.
    assert_eq!(s.last().unwrap().through_seq, 13);

    // One turn alone is larger than the summariser's window: no summary
    // at all rather than one that silently drops that turn.
    let p = Windowed::new(BIG, 1_000);
    let b = backend(p.clone(), Some(1_000));
    let host = Host::default();
    let out = run(
        &b,
        &host,
        history(4, 3_000),
        agent(Some(1_000)),
        Default::default(),
    )
    .await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    assert!(host.summaries().is_empty());
    assert_eq!(p.kinds(), ["main"], "no summary call that cannot fit");
}

#[tokio::test]
async fn cancel_during_compaction_leaves_a_consistent_log() {
    // Two pieces; the second is held, and the turn is cancelled meanwhile.
    let h = history(5, 2_000);
    let p = Arc::new(Windowed {
        window: BIG,
        summary_window: 5_000,
        hold: Some((2, Duration::from_secs(30))),
        calls: Mutex::default(),
    });
    let b = backend(p.clone(), Some(1_000));
    let host = Host::default();
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        c2.cancel();
    });
    let t = Instant::now();
    let out = run(&b, &host, h.clone(), agent(Some(5_000)), cancel).await;
    assert_eq!(out.reason, TurnEndReason::Cancelled);
    assert!(t.elapsed() < Duration::from_secs(5));
    // The first piece's summary stands on its own; nothing after it.
    let evs = host.0.lock().unwrap().clone();
    assert_eq!(evs.len(), 1, "{evs:?}");
    let EventKind::HistorySummarized(s) = &evs[0] else {
        panic!("{evs:?}")
    };
    // It ends on a turn boundary of the log.
    let next = h.iter().find(|e| e.seq == s.through_seq + 1).unwrap();
    assert_eq!(next.kind, "turn_started");
    assert_eq!(
        p.kinds(),
        ["summary", "summary"],
        "no model call after the cancel"
    );

    // Cancelled before any piece: nothing at all.
    let p = Arc::new(Windowed {
        window: BIG,
        summary_window: BIG,
        hold: Some((1, Duration::from_secs(30))),
        calls: Mutex::default(),
    });
    let b = backend(p.clone(), Some(1_000));
    let host = Host::default();
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        c2.cancel();
    });
    let out = run(&b, &host, h, agent(None), cancel).await;
    assert_eq!(out.reason, TurnEndReason::Cancelled);
    assert!(host.0.lock().unwrap().is_empty());
}
