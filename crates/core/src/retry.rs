//! Which turn a person may Retry (ADOPTION_PLAN X.21, README "Retry"),
//! read from a chat's log: one rule for the node, which decides under the
//! turn slot against its own log, and the master, which answers the
//! person's click from its mirror before it asks the node.

use crate::RawEvent;
use crate::event::{
    CheckpointPhase, EventKind, RETRY_ERROR_CHARS, RetryOf, TurnEndReason, UserMessage, retryable,
};

/// Whether `failed` may be retried in `history` (the session's whole log),
/// and what its retry records: Ok with the failed turn's end, or why not,
/// in the person's words.
pub fn check(history: &[RawEvent], failed: &str) -> Result<RetryOf, String> {
    let last = history
        .iter()
        .rposition(|e| e.kind == "turn_started")
        .ok_or("this chat has no turn")?;
    let Ok(EventKind::TurnStarted(t)) = history[last].typed() else {
        return Err("its start is not readable".into());
    };
    if t.turn_id != failed {
        return Err("only the chat's last turn can be retried".into());
    }
    match &t.cause {
        Some(c) if c.is_callback() => return Err("the harness started it, not you".into()),
        // An agent a message called after the first (X.37): its turn has no
        // message of its own, so a retry could not say what it answers.
        Some(_) => {
            return Err(
                "it answered a message that called several agents; send that message again \
                 (or mention the agent in a new one)"
                    .into(),
            );
        }
        None => {}
    }
    let mut has_message = t.retry.is_some();
    let mut ended: Option<TurnEndReason> = None;
    let mut error: Option<String> = None;
    let mut failed_model: Option<String> = None;
    for e in &history[last + 1..] {
        match e.typed() {
            Ok(EventKind::UserMessage(m)) if ended.is_none() && m.turn_id == failed => {
                has_message |= !m.queued;
            }
            Ok(EventKind::QueuedDelivered(d)) if d.turn_id == failed => has_message = true,
            Ok(EventKind::Error(x)) if x.turn_id.as_deref() == Some(failed) => {
                error = Some(x.message)
            }
            Ok(EventKind::TurnEnded(x)) if x.turn_id == failed => {
                ended = Some(x.reason);
                failed_model = x.usage.and_then(|u| u.model);
            }
            // After its end: a message of the person's (one sent while
            // it ended) or an undo of its changes makes it something else
            // than what the retry would say it was.
            Ok(EventKind::UserMessage(m)) if !m.queued => {
                return Err("a message was sent after it".into());
            }
            Ok(EventKind::Checkpoint(c))
                if ended.is_some()
                    && matches!(
                        c.phase,
                        CheckpointPhase::Restored | CheckpointPhase::BeforeRestore
                    ) =>
            {
                return Err(
                    "its changes were undone since; send your message again instead".into(),
                );
            }
            _ => {}
        }
    }
    let reason = ended.ok_or("it has not ended")?;
    if !retryable(reason) {
        return Err(match reason {
            TurnEndReason::Completed | TurnEndReason::Limit => "it ended with an answer".into(),
            TurnEndReason::Cancelled => {
                "you stopped it; write what to do instead (or send it again)".into()
            }
            _ => "it did not fail".into(),
        });
    }
    if !has_message {
        return Err("its message was never recorded; send it again".into());
    }
    // A restart or a stop records the node's own sentence ("… Send a
    // message to continue."), not a failure of the attempt: the opening
    // says how it was cut off instead (review of PR 128, M1).
    let error = error.filter(|_| reason == TurnEndReason::Error);
    let error = error.map(|e| {
        let mut cut: String = e.chars().take(RETRY_ERROR_CHARS).collect();
        if cut.len() < e.len() {
            cut.push('…');
        }
        cut
    });
    Ok(RetryOf {
        turn_id: failed.to_string(),
        reason,
        error,
        failed_model,
        model: None,
    })
}

/// The message turn `failed` answers, with its seq: the turn's own
/// message, the last message it read that was sent while an earlier turn
/// ran, or, for a retry, the message of the turn it retried (followed down
/// the chain). `None` when `log` does not hold it. One pass; only the
/// kinds it needs are parsed.
pub fn message_event(log: &[RawEvent], failed: &str) -> Option<(u64, UserMessage)> {
    use std::collections::HashMap;
    let mut own: HashMap<String, (u64, UserMessage)> = HashMap::new();
    let mut retried: HashMap<String, String> = HashMap::new();
    let mut delivered: HashMap<String, u64> = HashMap::new();
    let mut by_seq: HashMap<u64, UserMessage> = HashMap::new();
    for e in log {
        if !matches!(
            e.kind.as_str(),
            "user_message" | "turn_started" | "queued_delivered"
        ) {
            continue;
        }
        match e.typed() {
            Ok(EventKind::UserMessage(m)) if m.queued => {
                by_seq.insert(e.seq, m);
            }
            Ok(EventKind::UserMessage(m)) => {
                own.entry(m.turn_id.clone()).or_insert((e.seq, m));
            }
            Ok(EventKind::TurnStarted(t)) => {
                if let Some(r) = t.retry {
                    retried.insert(t.turn_id, r.turn_id);
                }
            }
            Ok(EventKind::QueuedDelivered(d)) => {
                if let Some(&s) = d.seqs.last() {
                    delivered.insert(d.turn_id, s);
                }
            }
            _ => {}
        }
    }
    let mut turn = failed.to_string();
    // Each retry names an earlier turn: at most one step per turn.
    for _ in 0..=retried.len() {
        if let Some(m) = own.remove(&turn) {
            return Some(m);
        }
        if let Some(seq) = delivered.get(&turn) {
            return by_seq.remove(seq).map(|m| (*seq, m));
        }
        turn = retried.remove(&turn)?;
    }
    None
}

/// The person's text of the message turn `failed` answers
/// ([`message_event`]), for what the master looks up by it (recall,
/// project facts).
pub fn message(log: &[RawEvent], failed: &str) -> Option<String> {
    message_event(log, failed).map(|(_, m)| m.text)
}

/// The turn of `log` a Retry may still need the message of: the running
/// retry's failed turn, else the log's last turn when it may be retried.
/// [`message_event`] of it is what no summary may replace (X.21).
pub fn pending(log: &[RawEvent], running: Option<&RetryOf>) -> Option<String> {
    if let Some(r) = running {
        return Some(r.turn_id.clone());
    }
    let last = log.iter().rposition(|e| e.kind == "turn_started")?;
    let Ok(EventKind::TurnStarted(t)) = log[last].typed() else {
        return None;
    };
    check(&log[last..], &t.turn_id).ok().map(|_| t.turn_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionId;
    use crate::event::*;

    struct Log {
        sid: SessionId,
        events: Vec<RawEvent>,
    }

    impl Log {
        fn new() -> Log {
            Log {
                sid: SessionId::new(),
                events: vec![],
            }
        }
        fn push(&mut self, k: EventKind) -> &mut Self {
            let (kind, payload) = k.into_parts();
            self.events.push(RawEvent {
                session_id: self.sid,
                seq: self.events.len() as u64 + 1,
                ts_ms: 0,
                kind,
                payload,
            });
            self
        }
        fn started(
            &mut self,
            t: &str,
            cause: Option<TurnCause>,
            retry: Option<RetryOf>,
        ) -> &mut Self {
            self.push(EventKind::TurnStarted(TurnStarted {
                turn_id: t.into(),
                agent: None,
                cause,
                environment: None,
                retry,
                continued_by: None,
            }))
        }
        fn message(&mut self, t: &str, text: &str, queued: bool) -> &mut Self {
            let mut m: UserMessage = serde_json::from_value(serde_json::json!({
                "turn_id": t,
                "text": text,
            }))
            .unwrap();
            m.queued = queued;
            self.push(EventKind::UserMessage(m))
        }
        fn error(&mut self, t: &str, msg: &str) -> &mut Self {
            self.push(EventKind::Error(ErrorEvent {
                turn_id: Some(t.into()),
                message: msg.into(),
            }))
        }
        fn ended(&mut self, t: &str, reason: TurnEndReason) -> &mut Self {
            self.push(EventKind::TurnEnded(TurnEnded {
                turn_id: t.into(),
                reason,
                usage: None,
            }))
        }
        fn turn(&mut self, t: &str, reason: TurnEndReason) -> &mut Self {
            self.started(t, None, None)
                .message(t, "fix the bug", false)
                .ended(t, reason)
        }
    }

    #[test]
    fn a_failed_last_turn_of_the_persons_is_retried_with_its_error() {
        for reason in [
            TurnEndReason::Error,
            TurnEndReason::Interrupted,
            TurnEndReason::Stopped,
        ] {
            let mut l = Log::new();
            l.turn("t1", TurnEndReason::Completed)
                .started("t2", None, None)
                .message("t2", "fix the bug", false)
                .error("t2", "first")
                .error("t2", "the model is not available")
                .ended("t2", reason);
            let r = check(&l.events, "t2").unwrap();
            assert_eq!(r.turn_id, "t2");
            assert_eq!(r.reason, reason);
            // Only an error is the attempt's own; a restart's or a stop's
            // sentence ("Send a message to continue.") is the node's.
            let want = (reason == TurnEndReason::Error).then_some("the model is not available");
            assert_eq!(r.error.as_deref(), want, "{reason:?}");
        }
    }

    #[test]
    fn the_pending_message_is_the_running_retrys_or_the_failed_last_turns() {
        let mut l = Log::new();
        l.turn("t1", TurnEndReason::Error);
        assert_eq!(pending(&l.events, None).as_deref(), Some("t1"));
        let (seq, m) = message_event(&l.events, "t1").unwrap();
        assert_eq!((seq, m.text.as_str()), (2, "fix the bug"));
        let r = RetryOf {
            turn_id: "t1".into(),
            reason: TurnEndReason::Error,
            error: None,
            failed_model: None,
            model: None,
        };
        assert_eq!(pending(&l.events, Some(&r)).as_deref(), Some("t1"));
        // A completed last turn needs nothing kept.
        l.turn("t2", TurnEndReason::Completed);
        assert_eq!(pending(&l.events, None), None);
        assert_eq!(pending(&[], None), None);
    }

    #[test]
    fn only_the_last_failed_turn_the_person_started_is_retried() {
        let mut l = Log::new();
        l.turn("t1", TurnEndReason::Error)
            .turn("t2", TurnEndReason::Completed);
        let e = check(&l.events, "t1").unwrap_err();
        assert!(e.contains("last turn"), "{e}");
        let e = check(&l.events, "t2").unwrap_err();
        assert!(e.contains("answer"), "{e}");
        let mut l = Log::new();
        l.turn("t1", TurnEndReason::Limit);
        assert!(check(&l.events, "t1").is_err(), "a limit has its answer");
        let mut l = Log::new();
        l.turn("t1", TurnEndReason::Cancelled);
        let e = check(&l.events, "t1").unwrap_err();
        assert!(e.contains("you stopped it"), "{e}");
        // A callback turn: nobody wrote its message.
        let mut l = Log::new();
        l.started("t1", Some(TurnCause::Callback(vec!["bg-1".into()])), None)
            .ended("t1", TurnEndReason::Error);
        let e = check(&l.events, "t1").unwrap_err();
        assert!(e.contains("harness started"), "{e}");
        // A mention turn (X.37): no message of its own either, and the
        // refusal says what to do.
        let mut l = Log::new();
        l.started(
            "t1",
            Some(TurnCause::Mention(MentionCall {
                message_seq: 2,
                by: CalledBy::Person,
                tags: vec![],
            })),
            None,
        )
        .ended("t1", TurnEndReason::Error);
        let e = check(&l.events, "t1").unwrap_err();
        assert!(e.contains("called several agents"), "{e}");
        // Still open (running, or its end not written).
        let mut l = Log::new();
        l.started("t1", None, None).message("t1", "x", false);
        assert!(check(&l.events, "t1").is_err());
        // Its message was never recorded.
        let mut l = Log::new();
        l.started("t1", None, None)
            .ended("t1", TurnEndReason::Error);
        let e = check(&l.events, "t1").unwrap_err();
        assert!(e.contains("never recorded"), "{e}");
        assert!(check(&[], "t1").is_err());
    }

    #[test]
    fn a_message_or_an_undo_after_the_failure_refuses_the_retry() {
        let mut l = Log::new();
        l.turn("t1", TurnEndReason::Error)
            .message("t1", "late", false);
        assert!(check(&l.events, "t1").is_err());
        let mut l = Log::new();
        l.turn("t1", TurnEndReason::Error)
            .push(EventKind::Checkpoint(Checkpoint {
                turn_id: "t1".into(),
                phase: CheckpointPhase::Restored,
                git_ref: "r".into(),
                files: vec!["a.txt".into()],
                excluded: vec![],
                excluded_more: 0,
            }));
        let e = check(&l.events, "t1").unwrap_err();
        assert!(e.contains("undone"), "{e}");
        // A message sent while it ran and never read does not: it waits
        // for the turn after the retry.
        let mut l = Log::new();
        l.started("t1", None, None)
            .message("t1", "fix it", false)
            .message("t1", "also this", true)
            .ended("t1", TurnEndReason::Interrupted);
        assert!(check(&l.events, "t1").is_ok());
    }

    #[test]
    fn a_failed_retry_and_a_turn_that_read_queued_messages_are_retried() {
        let first = RetryOf {
            turn_id: "t1".into(),
            reason: TurnEndReason::Error,
            error: None,
            failed_model: None,
            model: None,
        };
        let mut l = Log::new();
        l.turn("t1", TurnEndReason::Error)
            .started("t2", None, Some(first))
            .ended("t2", TurnEndReason::Error);
        assert!(check(&l.events, "t2").is_ok());
        let mut l = Log::new();
        l.started("t1", None, None)
            .message("t1", "x", false)
            .message("t1", "y", true)
            .ended("t1", TurnEndReason::Completed)
            .started("t2", None, None)
            .push(EventKind::QueuedDelivered(QueuedDelivered {
                turn_id: "t2".into(),
                seqs: vec![3],
            }))
            .ended("t2", TurnEndReason::Error);
        assert!(check(&l.events, "t2").is_ok());
    }

    #[test]
    fn the_message_a_retry_answers_is_found_through_the_retries() {
        let first = RetryOf {
            turn_id: "t1".into(),
            reason: TurnEndReason::Error,
            error: None,
            failed_model: None,
            model: None,
        };
        let second = RetryOf {
            turn_id: "t2".into(),
            ..first.clone()
        };
        let mut l = Log::new();
        l.turn("t0", TurnEndReason::Completed)
            .started("t1", None, None)
            .message("t1", "migrate it", false)
            .ended("t1", TurnEndReason::Error)
            .started("t2", None, Some(first))
            .ended("t2", TurnEndReason::Error)
            .started("t3", None, Some(second))
            .ended("t3", TurnEndReason::Interrupted);
        assert_eq!(message(&l.events, "t3").as_deref(), Some("migrate it"));
        assert_eq!(message(&l.events, "t0").as_deref(), Some("fix the bug"));
        // A turn that read a message sent while another ran: that message.
        let mut l = Log::new();
        l.started("t1", None, None)
            .message("t1", "x", false)
            .message("t1", "sent meanwhile", true)
            .ended("t1", TurnEndReason::Completed)
            .started("t2", None, None)
            .push(EventKind::QueuedDelivered(QueuedDelivered {
                turn_id: "t2".into(),
                seqs: vec![3],
            }))
            .ended("t2", TurnEndReason::Error);
        assert_eq!(message(&l.events, "t2").as_deref(), Some("sent meanwhile"));
        assert_eq!(message(&l.events, "nope"), None);
    }

    #[test]
    fn a_long_error_is_cut() {
        let mut l = Log::new();
        l.started("t1", None, None)
            .message("t1", "x", false)
            .error("t1", &"é".repeat(RETRY_ERROR_CHARS + 50))
            .ended("t1", TurnEndReason::Error);
        let r = check(&l.events, "t1").unwrap();
        assert_eq!(r.error.unwrap().chars().count(), RETRY_ERROR_CHARS + 1);
    }
}
