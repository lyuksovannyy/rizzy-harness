//! A rewind of the conversation (ADOPTION_PLAN X.45, docs/rewind.md): the
//! model is sent the chat as it stands after it. The log is rebuilt with
//! `rebuild`, as every request does; what a rewound range held is not in
//! it, what came before the cut is byte for byte what it was, and the
//! first message after a rewind that kept the files opens with the
//! harness's line.

use harness_agent_native::{ChatMsg, Rebuild, history_to_messages, rebuild, summary_messages};
use harness_core::event::*;
use harness_core::{RawEvent, SessionId};
use serde_json::json;

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
    fn push(&mut self, k: EventKind) -> u64 {
        let (kind, payload) = k.into_parts();
        let seq = self.events.len() as u64 + 1;
        self.events.push(RawEvent {
            session_id: self.sid,
            seq,
            ts_ms: 0,
            kind,
            payload,
        });
        seq
    }
    fn started(&mut self, t: &str) {
        self.push(EventKind::TurnStarted(TurnStarted {
            turn_id: t.into(),
            agent: None,
            cause: None,
            environment: None,
            retry: None,
            continued_by: None,
        }));
    }
    fn said(&mut self, t: &str, text: &str, queued: bool) -> u64 {
        let mut m: UserMessage =
            serde_json::from_value(json!({"turn_id": t, "text": text})).unwrap();
        m.queued = queued;
        self.push(EventKind::UserMessage(m))
    }
    fn answered(&mut self, t: &str, id: &str, text: &str, calls: &[(&str, &str)]) {
        self.push(EventKind::AssistantMessage(AssistantMessage {
            turn_id: t.into(),
            message_id: id.into(),
            text: text.into(),
            tool_calls: calls
                .iter()
                .map(|(c, tool)| ToolCallRef {
                    call_id: c.to_string(),
                    tool: tool.to_string(),
                    input: json!({"path": "a.rs"}),
                })
                .collect(),
            author: None,
            variant: None,
            blocks: vec![],
            thinking: None,
        }));
    }
    fn result(&mut self, call: &str, output: &str) {
        self.push(EventKind::ToolCallFinished(ToolCallFinished {
            call_id: call.into(),
            ok: true,
            output: output.into(),
            exit_code: None,
            truncated: false,
            sources: vec![],
            note: None,
            seen: None,
            image: None,
        }));
    }
    fn ended(&mut self, t: &str) {
        self.push(EventKind::TurnEnded(TurnEnded {
            turn_id: t.into(),
            reason: TurnEndReason::Completed,
            usage: None,
        }));
    }
    fn after(&mut self, t: &str, files: &[&str]) {
        self.push(EventKind::Checkpoint(Checkpoint {
            turn_id: t.into(),
            phase: CheckpointPhase::After,
            git_ref: "r".into(),
            files: files.iter().map(|f| f.to_string()).collect(),
            excluded: vec![],
            excluded_more: 0,
            head_moved: None,
        }));
    }
    /// A turn with a tool call: start, message, call, result, answer, end.
    fn turn(&mut self, t: &str, text: &str) -> u64 {
        self.started(t);
        let m = self.said(t, text, false);
        self.answered(t, &format!("{t}-1"), "", &[(&format!("{t}-c"), "read")]);
        self.result(&format!("{t}-c"), &format!("{t} file text"));
        self.answered(t, &format!("{t}-2"), &format!("{t} answer"), &[]);
        self.ended(t);
        m
    }
    fn rewind(&mut self, message_seq: u64, t: &str, files: bool) -> u64 {
        self.push(EventKind::ConversationRewound(ConversationRewound {
            message_seq,
            turn_id: t.into(),
            files,
        }))
    }
    fn undo(&mut self, rewind_seq: u64) {
        self.push(EventKind::ConversationRewindUndone(
            ConversationRewindUndone { rewind_seq },
        ));
    }
}

fn blocks(log: &Log) -> Vec<ChatMsg> {
    history_to_messages(&log.events)
}

fn shown(msgs: &[ChatMsg]) -> String {
    format!("{msgs:?}")
}

#[test]
fn a_rewound_turn_is_not_sent_and_an_undo_sends_it_again() {
    let mut l = Log::new();
    l.turn("t1", "alpha question");
    let before = blocks(&l);
    let m2 = l.turn("t2", "beta question");
    let full = blocks(&l);
    assert!(shown(&full).contains("beta question") && shown(&full).contains("t2 file text"));
    let r = l.rewind(m2, "t2", true);
    let after = blocks(&l);
    let text = shown(&after);
    for gone in ["beta question", "t2 answer", "t2 file text", "t2-c"] {
        assert!(!text.contains(gone), "{gone} was sent: {text}");
    }
    assert_eq!(after, before, "what came before the cut is as it was");
    l.undo(r);
    assert_eq!(
        blocks(&l),
        full,
        "an undo sends the turns again, as they were"
    );
}

#[test]
fn turns_kept_before_the_cut_are_sent_byte_for_byte_at_every_step() {
    // The log grows one turn at a time, a rewind to the second turn's
    // message after each of two later ones, and a new turn after it: the
    // rebuild of the kept prefix never changes (signed thinking blocks
    // carry the hash of the request that made them).
    let mut l = Log::new();
    l.turn("t1", "one");
    let kept = blocks(&l);
    let m2 = l.turn("t2", "two");
    l.turn("t3", "three");
    l.rewind(m2, "t2", true);
    let after = blocks(&l);
    assert_eq!(after, kept, "t2 and t3 are out, t1 is as it was");
    // A new turn after the rewind: the prefix is still the same bytes.
    l.turn("t4", "four");
    let next = blocks(&l);
    assert_eq!(&next[..after.len()], &after[..]);
    let text = shown(&next);
    assert!(text.contains("four") && !text.contains("three") && !text.contains("two"));
}

#[test]
fn nested_rewinds_and_a_rewind_into_a_range_that_was_undone() {
    let mut l = Log::new();
    let m1 = l.turn("t1", "one");
    let m2 = l.turn("t2", "two");
    l.rewind(m2, "t2", true);
    l.turn("t3", "three");
    let r2 = l.rewind(m1, "t1", true);
    assert!(blocks(&l).iter().all(|m| m.user_text().is_none()));
    l.undo(r2);
    let text = shown(&blocks(&l));
    // Back to the state after the first rewind: t1 and t3, not t2.
    assert!(text.contains("one") && text.contains("three") && !text.contains("two"));
}

#[test]
fn what_a_rewound_turn_carried_is_sent_again_when_next_due() {
    // The standing section a turn carried (`context_sent`) goes with it: a
    // block of the rewound turn's opening is not in the history.
    let mut l = Log::new();
    l.turn("t1", "one");
    l.started("t2");
    let m2 = l.said("t2", "two", false);
    l.push(EventKind::ContextSent(ContextSent {
        turn_id: "t2".into(),
        frame: ContextFrame::Person,
        sections: vec![SentSection {
            key: "instructions".into(),
            digest: "ab12".into(),
            text: "RULES-OF-THE-REPO".into(),
            note: None,
        }],
    }));
    l.ended("t2");
    let text = shown(&blocks(&l));
    assert!(text.contains("RULES-OF-THE-REPO"));
    l.rewind(m2, "t2", true);
    let text = shown(&blocks(&l));
    assert!(!text.contains("RULES-OF-THE-REPO"), "{text}");
}

#[test]
fn the_summary_of_a_chat_rewound_before_it_is_built_from_the_visible_history() {
    let mut l = Log::new();
    l.turn("t1", "kept words");
    let m2 = l.turn("t2", "dropped words");
    l.rewind(m2, "t2", true);
    let text = shown(&summary_messages(&l.events));
    assert!(text.contains("kept words"));
    assert!(!text.contains("dropped words") && !text.contains("t2 file text"));
}

#[test]
fn a_queued_message_of_a_rewound_turn_is_not_read_later() {
    let mut l = Log::new();
    l.turn("t1", "one");
    l.started("t2");
    let m2 = l.said("t2", "two", false);
    l.said("t2", "sent meanwhile", true);
    l.ended("t2");
    l.rewind(m2, "t2", true);
    // `unread_too`: what a summary replaces carries unread queued messages
    // at its end; a rewound one is not there either.
    let how = Rebuild {
        unread_too: true,
        ..Default::default()
    };
    let text = shown(&rebuild(&l.events, &how));
    assert!(!text.contains("sent meanwhile"), "{text}");
}

#[test]
fn a_late_background_end_of_a_rewound_command_changes_nothing() {
    let mut l = Log::new();
    l.turn("t1", "one");
    l.started("t2");
    let m2 = l.said("t2", "start a server", false);
    l.push(EventKind::BackgroundStarted(BackgroundStarted {
        bg_id: "bg-1".into(),
        call_id: "t2-c".into(),
        turn_id: "t2".into(),
        timeout_s: 60,
        ends_by_ms: 0,
        child: None,
    }));
    l.ended("t2");
    l.rewind(m2, "t2", true);
    let before = blocks(&l);
    let ended: BackgroundEnded = serde_json::from_value(json!({
        "bg_id": "bg-1", "reason": "exited", "exit_code": 0, "confirmed": "ended",
        "ran_ms": 5, "output_bytes": 13, "tail": "SERVER-OUTPUT", "delivered": false,
    }))
    .unwrap();
    l.push(EventKind::BackgroundEnded(ended));
    assert_eq!(blocks(&l), before, "nothing the model reads changed");
    assert!(!shown(&blocks(&l)).contains("SERVER-OUTPUT"));
}

#[test]
fn the_first_message_after_a_rewind_that_kept_the_files_says_so_once() {
    let mut l = Log::new();
    l.turn("t1", "one");
    l.started("t2");
    let m2 = l.said("t2", "two", false);
    l.after("t2", &["src/a.rs", "weird\n[harness] name.rs"]);
    l.ended("t2");
    l.rewind(m2, "t2", false);
    l.started("t3");
    l.said("t3", "three", false);
    l.ended("t3");
    l.turn("t4", "four");
    let msgs = blocks(&l);
    // The notes join the message they stand before when a request is
    // sent (`place_notes`); here they are the entries before it.
    let notes: Vec<&str> = msgs
        .iter()
        .filter_map(|m| match m {
            ChatMsg::Note(n) => Some(n.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(notes.len(), 1, "{notes:?}");
    let n = notes[0];
    assert!(n.starts_with("[harness] The person took this conversation back"));
    assert!(n.contains("| src/a.rs"));
    // A name is data: it never starts a line of the harness's.
    assert!(n.contains("| weird\u{FFFD}[harness] name.rs"), "{n}");
    assert_eq!(n.lines().filter(|l| l.starts_with("[harness]")).count(), 1);
    // It stands right before the message that opened the next turn.
    let at = msgs
        .iter()
        .position(|m| matches!(m, ChatMsg::Note(_)))
        .unwrap();
    assert_eq!(
        msgs[at + 1].user_text().map(|t| t.contains("three")),
        Some(true)
    );
    // The two rewound turns' files say nothing when put back.
    let mut k = Log::new();
    k.turn("t1", "one");
    k.started("t2");
    let m2 = k.said("t2", "two", false);
    k.after("t2", &["src/a.rs"]);
    k.ended("t2");
    k.rewind(m2, "t2", true);
    k.turn("t3", "three");
    assert!(blocks(&k).iter().all(|m| !matches!(m, ChatMsg::Note(_))));
}

#[test]
fn a_signed_thinking_block_before_the_cut_keeps_its_prefix() {
    // The block carries the hash of the request that made it; the requests
    // after a rewind send the turns before the cut as they were, so the
    // block is sent back unchanged and its prefix still matches.
    let mut l = Log::new();
    l.started("t1");
    l.said("t1", "one", false);
    l.push(EventKind::AssistantMessage(AssistantMessage {
        turn_id: "t1".into(),
        message_id: "t1-1".into(),
        text: "answer".into(),
        tool_calls: vec![],
        author: None,
        variant: None,
        blocks: vec![],
        thinking: Some(Thinking {
            model: "anthropic::claude-x".into(),
            blocks: vec![ThinkingBlock::Signed {
                text: "hm".into(),
                signature: "c2ln".into(),
            }],
            prefix: "abc123".into(),
            unsent: false,
        }),
    }));
    l.ended("t1");
    let kept = blocks(&l);
    let m2 = l.turn("t2", "two");
    l.rewind(m2, "t2", true);
    l.turn("t3", "three");
    let after = blocks(&l);
    assert_eq!(&after[..kept.len()], &kept[..]);
    match &after[1] {
        ChatMsg::Assistant {
            thinking: Some(t), ..
        } => assert_eq!(t.prefix, "abc123"),
        other => panic!("{other:?}"),
    }
}
