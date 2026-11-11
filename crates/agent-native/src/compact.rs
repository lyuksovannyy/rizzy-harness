//! When to compact a conversation: keyed to the model's context window
//! when the master knows it, measured with the provider's own token count
//! from the previous turn when there is one.

use harness_core::RawEvent;
use harness_core::event::EventKind;

/// The trigger when nothing is configured and the window is unknown.
pub const DEFAULT_COMPACT_AFTER: usize = 24_000;

/// Tokens kept free for the reply and the step's tool results: a fifth of
/// the window, at least 16k, never more than half of it.
pub fn reserve(window: u64) -> u64 {
    (window / 5).max(16_384).min(window / 2)
}

/// The history size (tokens) above which older turns are summarised:
/// the configured value when there is one (0 = never), else the window
/// less the reserve, else [`DEFAULT_COMPACT_AFTER`].
pub fn threshold(configured: Option<usize>, window: Option<u64>) -> usize {
    match (configured, window) {
        (Some(n), _) => n,
        (None, Some(w)) if w > 0 => (w - reserve(w)) as usize,
        (None, _) => DEFAULT_COMPACT_AFTER,
    }
}

/// Tokens of what the summariser itself needs beside the transcript: its
/// prompt and a reply of at most 300 words.
pub const SUMMARY_OVERHEAD: u64 = 2_048;

/// How large the history is, in tokens. The previous turn's last model
/// call reported its input size (`context_tokens`): that number, plus an
/// `estimate` of what that call did not read (its own reply and anything
/// after), beats a character count. Without one, all of it is estimated.
pub fn history_tokens(events: &[RawEvent], estimate: impl Fn(&[RawEvent]) -> u64) -> u64 {
    let ended = events.iter().rposition(|e| {
        matches!(
            e.typed(),
            Ok(EventKind::TurnEnded(t)) if t.usage.as_ref().is_some_and(|u| u.context_tokens > 0)
        )
    });
    let Some(end) = ended else {
        return estimate(events);
    };
    let Ok(EventKind::TurnEnded(t)) = events[end].typed() else {
        return estimate(events);
    };
    let reported = t.usage.map(|u| u.context_tokens).unwrap_or_default();
    // The reply of that last call is the turn's last assistant message.
    let from = events[..end]
        .iter()
        .rposition(|e| e.kind == "assistant_message")
        .unwrap_or(end);
    reported + estimate(&events[from..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_follows_the_window_unless_configured() {
        // Unknown window, nothing configured: the old fixed number.
        assert_eq!(threshold(None, None), 24_000);
        // A large window does not compact at 24k.
        assert_eq!(threshold(None, Some(200_000)), 160_000);
        // A small one compacts earlier than 24k.
        assert_eq!(threshold(None, Some(16_384)), 8_192);
        assert_eq!(threshold(None, Some(32_768)), 16_384);
        // An explicit value wins, 0 included (never).
        assert_eq!(threshold(Some(50_000), Some(200_000)), 50_000);
        assert_eq!(threshold(Some(0), Some(8_192)), 0);
        assert_eq!(threshold(None, Some(0)), 24_000);
    }

    #[test]
    fn reported_usage_beats_the_estimate() {
        use harness_core::event::*;
        let sid = harness_core::SessionId::new();
        let ev = |seq: u64, k: EventKind| {
            let (kind, payload) = k.into_parts();
            RawEvent {
                session_id: sid,
                seq,
                ts_ms: 0,
                kind,
                payload,
            }
        };
        let reply = ev(
            2,
            EventKind::AssistantMessage(AssistantMessage {
                author: None,
                variant: None,
                turn_id: "t".into(),
                message_id: "m".into(),
                text: "hi".into(),
                tool_calls: vec![],
                blocks: vec![],
            }),
        );
        let ended = |context_tokens| {
            ev(
                3,
                EventKind::TurnEnded(TurnEnded {
                    turn_id: "t".into(),
                    reason: TurnEndReason::Completed,
                    usage: Some(Usage {
                        context_tokens,
                        ..Default::default()
                    }),
                }),
            )
        };
        // Each event counts 10 in this estimate.
        let est = |e: &[RawEvent]| 10 * e.len() as u64;
        let h = vec![reply.clone(), ended(50_000)];
        // The provider's count, plus the reply and what follows it.
        assert_eq!(history_tokens(&h, est), 50_020);
        // Nothing reported: the estimate.
        let h = vec![reply, ended(0)];
        assert_eq!(history_tokens(&h, est), 20);
    }
}
