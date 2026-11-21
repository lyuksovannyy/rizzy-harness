//! A person's `/compact` (ADOPTION_PLAN X.20, PLAN.md T64): the chat's
//! history summarised now, between turns, by the same summariser, shrink
//! check and breaker as the automatic summary at a turn's start (A.7), with
//! the person's focus text. The node runs it as an operation of the
//! session's one writer and records the outcome (`history_summarized` with
//! `manual`, or `compact_failed`); this module only computes it.
//!
//! What is kept verbatim: the last turn ([`MANUAL_KEEP_TURNS`]; the
//! automatic summary keeps two), the task list and the standing context
//! (both given back after any summary), and what the next turn's opening
//! reads from the log (an unread queued message, `opening_cut`).

use harness_core::backend::{CompactCtx, CompactOutcome, TurnHost};
use harness_core::event::{EventKind, Usage};
use harness_core::{RawEvent, SessionId};
use serde_json::Value;

use crate::{ChatMsg, NativeBackend, opening_cut, provider, sized, split_summary};

/// The turns a manual summary keeps verbatim: the last one. The person
/// asked for a shorter context now; every other harness that has the
/// command summarises more than its automatic summary keeps (Claude Code's
/// `/compact` replaces the history with the summary, Codex keeps only recent
/// user messages; research round 1 "Manual compaction", round 2 appendix
/// Codex compaction). One turn stays so a follow-up about the last answer
/// still has it word for word.
pub const MANUAL_KEEP_TURNS: usize = 1;

/// Below this many tokens before the kept turn there is nothing worth
/// summarising: the summary would not be smaller.
pub const MANUAL_MIN_TOKENS: usize = 1_000;

/// After this many failed `/compact`s since the last turn, the next is
/// refused until a turn runs (the breaker of A.7, per chat between turns).
pub const MANUAL_MAX_FAILURES: usize = 2;

/// Why a summary did not come, in the person's words (the end of
/// [`failed_reason`]'s sentence).
pub(crate) struct Manual {
    pub focus: Option<String>,
    pub miss: Option<String>,
}

pub(crate) const TOO_SHORT: &str = "there is too little before your last message to summarise";
pub(crate) const TOO_LARGE: &str = "one turn is larger than the summarising model's context window";
pub(crate) const MODEL_FAILED: &str = "the summarising model returned an error";
pub(crate) const STOPPED: &str = "it was stopped";
pub(crate) const NOT_SMALLER: &str = "the summary was not smaller than what it would replace";

/// The sentence a `compact_failed` carries.
pub fn failed_reason(why: &str) -> String {
    harness_core::event::compact_failed_reason(why)
}

/// Whether a `/compact` of `history` could make a summary: the plain
/// answer when it could not. Pure, from the log alone.
pub fn check(history: &[RawEvent]) -> Result<(), String> {
    // Rewound turns are not part of the chat (X.46).
    let history = &*harness_core::rewind::visible(history);
    // The breaker: failures since the last turn or summary.
    let since = history
        .iter()
        .rposition(|e| e.kind == "turn_started" || e.kind == "history_summarized")
        .map_or(0, |i| i + 1);
    let failed = history[since..]
        .iter()
        .filter(|e| e.kind == "compact_failed")
        .count();
    if failed >= MANUAL_MAX_FAILURES {
        return Err(format!(
            "The last {failed} summaries of this chat failed: send a message first, then try \
             again."
        ));
    }
    let (_, recent) = split_summary(history);
    let (old, _) = old_part(recent);
    let tokens = provider::estimate_tokens(&sized(old)) as usize;
    if old.is_empty() || tokens < MANUAL_MIN_TOKENS {
        return Err(format!(
            "This chat is too short to summarise: what comes before your last message is about \
             {tokens} tokens, and a summary needs at least {MANUAL_MIN_TOKENS}."
        ));
    }
    Ok(())
}

/// What a manual summary would replace in `recent` (the events after the
/// latest summary), and where the kept part starts.
fn old_part(recent: &[RawEvent]) -> (&[RawEvent], usize) {
    let starts: Vec<usize> = recent
        .iter()
        .enumerate()
        .filter(|(_, e)| e.kind == "turn_started")
        .map(|(i, _)| i)
        .collect();
    if starts.len() <= MANUAL_KEEP_TURNS {
        return (&[], 0);
    }
    let kept = starts[starts.len() - MANUAL_KEEP_TURNS];
    let end = kept.min(opening_cut(recent, None));
    (&recent[..end], end)
}

/// Collects what `compact` emits: only the summaries are kept (a retry's
/// countdown belongs to no turn here).
#[derive(Default)]
struct Collect {
    summaries: std::sync::Mutex<Vec<harness_core::event::HistorySummarized>>,
}

#[async_trait::async_trait]
impl TurnHost for Collect {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        if let EventKind::HistorySummarized(h) = kind {
            self.summaries
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(h);
        }
        Ok(())
    }
    async fn approve(
        &self,
        _call_id: &str,
        _tool: &str,
        _input: &Value,
        _risk: harness_core::policy::ToolRisk,
    ) -> harness_core::policy::Decision {
        harness_core::policy::Decision::Deny
    }
}

impl NativeBackend {
    /// Summarise `ctx.history` now (a person's `/compact`).
    pub(crate) async fn manual_compact(&self, ctx: CompactCtx) -> CompactOutcome {
        let CompactCtx {
            session_id,
            history,
            model,
            window,
            focus,
            cancel,
        } = ctx;
        let collect = Collect::default();
        // What a summary covers is the chat as it stands after its rewinds
        // (X.46): a summary in a rewound range is not one, and a rewound
        // turn is not summarised.
        let visible = harness_core::rewind::visible(&history);
        let history: &[RawEvent] = &visible;
        let (prev, recent) = split_summary(history);
        let tctx = turn_ctx(session_id, history, &model, cancel, &collect);
        let mut usage = Usage::default();
        let mut failures = 0u32;
        let mut manual = Manual { focus, miss: None };
        let res = self
            .compact_with(
                &tctx,
                &model,
                window,
                prev.as_deref(),
                recent,
                MANUAL_KEEP_TURNS,
                MANUAL_MIN_TOKENS,
                &mut usage,
                &mut failures,
                Some(&mut manual),
            )
            .await;
        let done = match res {
            Ok(Some(d)) => d,
            // `Collect` never fails to append.
            Ok(None) | Err(_) => {
                return CompactOutcome::Failed {
                    reason: failed_reason(manual.miss.as_deref().unwrap_or(TOO_SHORT)),
                    usage,
                };
            }
        };
        let (text, through_seq) = done;
        // The next request's size: what the last turn's last call reported,
        // less what the summary replaces, plus the summary (the system
        // prompt and tools stay counted); estimated alone without one.
        let covered = &recent[..recent.partition_point(|e| e.seq <= through_seq)];
        let replaced = provider::estimate_tokens(&sized(covered))
            + prev.as_deref().map_or(0, |p| {
                provider::estimate_tokens(&[ChatMsg::System(p.into())])
            });
        let summary = provider::estimate_tokens(&[ChatMsg::System(text.clone())]);
        let reported = last_context(history);
        let tokens_after = match reported {
            Some(r) => r.saturating_sub(replaced) + summary,
            None => summary + provider::estimate_tokens(&sized(&recent[covered.len()..])),
        };
        CompactOutcome::Done {
            through_seq,
            text,
            usage,
            tokens_after,
        }
    }
}

/// The context size the chat's last turn reported (`turn_ended.usage`).
fn last_context(history: &[RawEvent]) -> Option<u64> {
    history.iter().rev().find_map(|e| match e.typed() {
        Ok(EventKind::TurnEnded(t)) => t.usage.map(|u| u.context_tokens).filter(|n| *n > 0),
        _ => None,
    })
}

fn turn_ctx<'a>(
    session_id: SessionId,
    history: &[RawEvent],
    model: &str,
    cancel: tokio_util::sync::CancellationToken,
    host: &'a dyn TurnHost,
) -> harness_core::backend::TurnCtx<'a> {
    harness_core::backend::TurnCtx {
        session_id,
        turn_id: String::new(),
        history: history.to_vec(),
        user_text: String::new(),
        cwd: Default::default(),
        model: model.to_string(),
        agent: None,
        compare: vec![],
        parent_session: None,
        extra: Default::default(),
        cancel,
        host,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::event::*;

    fn log(kinds: Vec<EventKind>) -> Vec<RawEvent> {
        let sid = SessionId::new();
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

    fn turn(id: &str, words: usize) -> Vec<EventKind> {
        vec![
            EventKind::TurnStarted(TurnStarted {
                turn_id: id.into(),
                agent: None,
                cause: None,
                environment: None,
                continued_by: None,
                retry: None,
            }),
            EventKind::UserMessage(UserMessage {
                turn_id: id.into(),
                text: "word ".repeat(words),
                attachments: vec![],
                reply: None,
                images: vec![],
                files: vec![],
                queued: false,
                triggers: vec![],
                tags: vec![],
            }),
            EventKind::AssistantMessage(AssistantMessage {
                turn_id: id.into(),
                message_id: format!("m{id}"),
                text: "ok".into(),
                tool_calls: vec![],
                author: None,
                variant: None,
                blocks: vec![],
                thinking: None,
            }),
            EventKind::TurnEnded(TurnEnded {
                turn_id: id.into(),
                reason: TurnEndReason::Completed,
                usage: None,
            }),
        ]
    }

    fn failed() -> EventKind {
        EventKind::CompactFailed(CompactFailed {
            reason: failed_reason(MODEL_FAILED),
            usage: None,
        })
    }

    /// One turn, or a small one before the last, is too short; enough
    /// before the last turn is not; two failures since the last turn or
    /// summary refuse the next, a turn between them does not.
    #[test]
    fn what_a_manual_summary_needs() {
        let one = log(turn("a", 5_000));
        assert!(check(&one).unwrap_err().contains("too short"));
        let small = log([turn("a", 10), turn("b", 5_000)].concat());
        assert!(check(&small).unwrap_err().contains("too short"));
        let enough = [turn("a", 2_000), turn("b", 10)].concat();
        assert_eq!(check(&log(enough.clone())), Ok(()));
        let twice = [enough.clone(), vec![failed(), failed()]].concat();
        assert!(
            check(&log(twice))
                .unwrap_err()
                .contains("send a message first")
        );
        let once = [enough.clone(), vec![failed()]].concat();
        assert_eq!(check(&log(once)), Ok(()));
        // A turn after them resets the count (and is the new last turn).
        let reset = [
            turn("a", 2_000),
            vec![failed(), failed()],
            turn("b", 2_000),
            turn("c", 10),
        ]
        .concat();
        assert_eq!(check(&log(reset)), Ok(()));
    }

    /// What the next request reads after a manual summary is the same
    /// split the automatic one makes: the summary, then the kept turn.
    #[test]
    fn a_manual_summary_splits_the_history_like_any_summary() {
        let mut kinds = [turn("a", 2_000), turn("b", 10)].concat();
        kinds.push(EventKind::HistorySummarized(HistorySummarized {
            turn_id: String::new(),
            through_seq: 4,
            text: "SUMMARY".into(),
            manual: true,
            focus: Some("keep it".into()),
            usage: None,
            tokens_after: Some(10),
        }));
        let h = log(kinds);
        let (summary, recent) = split_summary(&h);
        assert_eq!(summary.as_deref(), Some("SUMMARY"));
        assert_eq!(recent.first().map(|e| e.seq), Some(5));
        assert_eq!(
            recent.first().map(|e| e.kind.as_str()),
            Some("turn_started")
        );
    }
}
