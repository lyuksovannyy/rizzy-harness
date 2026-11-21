//! The `history` tool's reading of a chat (PLAN.md T91): the rebuild with
//! compaction and shortening off ([`Rebuild::unshortened`]) and the tool's
//! answers over it.

use harness_core::event::*;
use harness_core::{RawEvent, SessionId};
use serde_json::{Value, json};

use crate::provider::*;
use crate::*;

/// A chat's log, built one event at a time with contiguous seqs.
pub(crate) struct Log {
    pub(crate) sid: SessionId,
    pub(crate) events: Vec<RawEvent>,
    n: usize,
}

impl Log {
    pub(crate) fn new() -> Self {
        Self {
            sid: SessionId::new(),
            events: Vec::new(),
            n: 0,
        }
    }

    /// Append one event, `ts` seconds into the chat; its seq.
    pub(crate) fn at(&mut self, ts: i64, k: EventKind) -> u64 {
        let (kind, payload) = k.into_parts();
        let seq = self.events.len() as u64 + 1;
        self.events.push(RawEvent {
            session_id: self.sid,
            seq,
            ts_ms: ts * 1000,
            kind,
            payload,
        });
        seq
    }

    pub(crate) fn push(&mut self, k: EventKind) -> u64 {
        self.at(0, k)
    }

    /// One turn of `agent`: the person's message, one step per call (each
    /// `(tool, input, output)`), then the answer. Returns the turn's id.
    pub(crate) fn turn(
        &mut self,
        agent: Option<&str>,
        text: &str,
        calls: &[(&str, Value, &str)],
        answer: &str,
    ) -> String {
        self.turn_with(agent, UserMessage::plain("", text), calls, answer)
    }

    pub(crate) fn turn_with(
        &mut self,
        agent: Option<&str>,
        mut msg: UserMessage,
        calls: &[(&str, Value, &str)],
        answer: &str,
    ) -> String {
        self.n += 1;
        let turn = format!("t{}", self.n);
        msg.turn_id = turn.clone();
        let ts = self.n as i64 * 100;
        self.at(
            ts,
            EventKind::TurnStarted(TurnStarted {
                turn_id: turn.clone(),
                agent: agent.map(|a| AgentRef {
                    agent_id: a.into(),
                    name: a.into(),
                    prompt_hash: "h".into(),
                    withheld: vec![],
                    effort: None,
                }),
                cause: None,
                environment: None,
                continued_by: None,
                retry: None,
            }),
        );
        self.at(ts, EventKind::UserMessage(msg));
        for (i, (tool, input, output)) in calls.iter().enumerate() {
            let call_id = format!("{turn}c{i}");
            self.at(
                ts,
                EventKind::AssistantMessage(AssistantMessage {
                    turn_id: turn.clone(),
                    message_id: format!("{turn}m{i}"),
                    text: String::new(),
                    tool_calls: vec![ToolCallRef {
                        call_id: call_id.clone(),
                        tool: (*tool).into(),
                        input: input.clone(),
                    }],
                    author: agent.map(Into::into),
                    variant: None,
                    blocks: vec![],
                    thinking: None,
                }),
            );
            self.at(
                ts,
                EventKind::ToolCallFinished(ToolCallFinished {
                    call_id,
                    ok: true,
                    output: (*output).into(),
                    exit_code: None,
                    truncated: false,
                    sources: vec![],
                    note: None,
                    seen: None,
                    image: None,
                }),
            );
        }
        self.at(
            ts,
            EventKind::AssistantMessage(AssistantMessage {
                turn_id: turn.clone(),
                message_id: format!("{turn}a"),
                text: answer.into(),
                tool_calls: vec![],
                author: agent.map(Into::into),
                variant: None,
                blocks: vec![],
                thinking: None,
            }),
        );
        self.at(
            ts,
            EventKind::TurnEnded(TurnEnded {
                turn_id: turn.clone(),
                reason: TurnEndReason::Completed,
                usage: None,
            }),
        );
        turn
    }

    /// A summary of everything up to now, as the loop writes one.
    pub(crate) fn summarize(&mut self, text: &str) -> u64 {
        let through_seq = self.events.len() as u64;
        self.push(EventKind::HistorySummarized(HistorySummarized {
            turn_id: String::new(),
            through_seq,
            text: text.into(),
            manual: false,
            focus: None,
            usage: None,
            tokens_after: None,
        }))
    }
}

/// A plain person's message.
trait Plain {
    fn plain(turn: &str, text: &str) -> UserMessage;
}

impl Plain for UserMessage {
    fn plain(turn: &str, text: &str) -> UserMessage {
        UserMessage {
            turn_id: turn.into(),
            text: text.into(),
            attachments: vec![],
            files: vec![],
            reply: None,
            images: vec![],
            queued: false,
            triggers: vec![],
            tags: vec![],
        }
    }
}

pub(crate) fn tool_texts(msgs: &[ChatMsg]) -> Vec<String> {
    msgs.iter()
        .filter_map(|m| match m {
            ChatMsg::Tool { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect()
}

fn big(tag: &str) -> String {
    format!("{tag}: {}", "line of the result\n".repeat(400))
}

/// A chat compacted twice and shortened once: what the loop sends is a
/// summary and stubs, and the unshortened rebuild still holds every
/// result's text as the tool returned it, byte for byte.
#[test]
fn the_unshortened_rebuild_gives_the_original_result_text_after_two_summaries_and_a_clearing() {
    let mut log = Log::new();
    let (r1, r2, r3) = (big("one"), big("two"), big("three"));
    log.turn(
        None,
        "first",
        &[("read", json!({"path": "a"}), &r1)],
        "done 1",
    );
    log.summarize("summary of the first turn");
    log.turn(
        None,
        "second",
        &[("read", json!({"path": "b"}), &r2)],
        "done 2",
    );
    log.summarize("summary of two turns");
    let last = log.turn(
        None,
        "third",
        &[("read", json!({"path": "c"}), &r3)],
        "done 3",
    );
    log.push(EventKind::ContextShortened(ContextShortened {
        turn_id: last,
        cleared: Some(Cleared {
            turn_id: "t3".into(),
            results: 99,
        }),
        folded: None,
        thinking: None,
        tokens_before: 10,
        tokens_after: 5,
    }));
    // What a request carries: stubs for the cleared results.
    let sent = rebuild(&log.events, &Rebuild::default());
    assert!(
        !tool_texts(&sent).contains(&r1) && !tool_texts(&sent).contains(&r3),
        "the loop's rebuild must stub the cleared results"
    );
    let full = rebuild(
        &log.events,
        &Rebuild {
            unshortened: true,
            ..Default::default()
        },
    );
    assert!(
        tool_texts(&full) == vec![r1, r2, r3],
        "a result came back changed"
    );
}

fn unshortened() -> Rebuild<'static> {
    Rebuild {
        unshortened: true,
        ..Default::default()
    }
}

/// A rewound range is not in the unshortened rebuild: the rewind is the
/// loop's rule too and is not given up with the shortening.
#[test]
fn the_unshortened_rebuild_leaves_out_a_rewound_range() {
    let mut log = Log::new();
    log.turn(None, "keep", &[("read", json!({}), "kept result")], "ok 1");
    let back_to = log.events.len() as u64 + 2;
    log.turn(
        None,
        "undo me",
        &[("read", json!({}), "rewound result")],
        "ok 2",
    );
    log.push(EventKind::ConversationRewound(ConversationRewound {
        message_seq: back_to,
        turn_id: "t2".into(),
        files: false,
    }));
    log.turn(
        None,
        "after",
        &[("read", json!({}), "later result")],
        "ok 3",
    );
    let msgs = rebuild(&log.events, &unshortened());
    let all = format!("{msgs:?}");
    assert!(all.contains("kept result") && all.contains("later result"));
    assert!(
        !all.contains("rewound result") && !all.contains("undo me"),
        "a rewound message came back"
    );
}

/// An image the provider refused stays withheld.
#[test]
fn the_unshortened_rebuild_keeps_a_withheld_image_withheld() {
    let mut log = Log::new();
    let turn = log.turn(
        None,
        "look",
        &[("read", json!({"path": "a.png"}), "")],
        "seen",
    );
    // The result's image reference, and the provider's refusal after it.
    let call = format!("{turn}c0");
    let at = log
        .events
        .iter()
        .position(|e| e.kind == "tool_call_finished")
        .unwrap();
    let mut f = match log.events[at].typed().unwrap() {
        EventKind::ToolCallFinished(f) => f,
        _ => unreachable!(),
    };
    f.image = Some(ToolImage {
        sha256: "ab".repeat(32),
        mime: "image/png".into(),
        width: 1,
        height: 1,
        bytes: 1,
    });
    log.events[at].payload = EventKind::ToolCallFinished(f).into_parts().1;
    log.push(EventKind::ImagesWithheld(ImagesWithheld {
        turn_id: turn,
        calls: vec![call],
        status: 400,
        reason: "no".into(),
    }));
    let msgs = rebuild(&log.events, &unshortened());
    let states: Vec<ImageState> = msgs
        .iter()
        .filter_map(|m| match m {
            ChatMsg::ToolImage { state, .. } => Some(*state),
            _ => None,
        })
        .collect();
    assert!(states == vec![ImageState::Withheld], "{states:?}");
}
