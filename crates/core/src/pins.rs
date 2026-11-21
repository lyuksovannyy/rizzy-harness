//! Pinned messages (`message_pinned`, docs/pins.md): a bookmark the chat's
//! owner puts on one message to find it again. The one rule every reader
//! shares: the Rust fold (`view.rs`), the node's check and the master's
//! answer. `web/src/fold.ts` is a second implementation, pinned by the
//! shared fold fixture.
//!
//! A pin is for the person. The model is sent nothing about it, no reader
//! of the conversation (the rebuild, the `history` tool, the hand-off note,
//! search) looks at it, and nothing but the chat's owner writes one: the
//! node, for `session.pin`, after [`check`].

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::RawEvent;
use crate::event::EventKind;
use crate::rewind::{self, Ranges};

/// The most pins one chat holds; [`check`] refuses the next.
pub const MAX_PINS: usize = 50;

/// How many characters of a message a pin shows.
pub const PREVIEW_CHARS: usize = 120;

/// The event kinds [`pins`] and [`check`] read: a reader (the master, from
/// its mirror) may load only these.
pub const KINDS: &[&str] = &[
    "session_created",
    "user_message",
    "assistant_message",
    "message_pinned",
    "conversation_rewound",
    "conversation_rewind_undone",
];

/// Who wrote a pinned message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Who {
    User,
    Assistant,
}

/// One pinned message as a list shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Pin {
    /// The seq of the message's event.
    pub seq: u64,
    pub who: Who,
    /// Its first [`PREVIEW_CHARS`] characters on one line.
    pub text: String,
    /// The turn it belongs to.
    pub turn_id: String,
    /// When the message was written (ms since the epoch).
    pub at: i64,
    /// While a rewind hides the message, the seq of that rewind: the pin is
    /// kept and comes back with an undo, but is not listed. Not part of
    /// the wire shape: the view lists only pins that are not rewound.
    #[serde(skip)]
    pub rewound: Option<u64>,
}

/// The first [`PREVIEW_CHARS`] characters of `text` on one line: runs of
/// white space (newlines included) become one space, other control
/// characters are dropped.
pub fn preview(text: &str) -> String {
    let mut out = String::new();
    let mut n = 0;
    let mut gap = false;
    for c in text.chars() {
        if c.is_whitespace() {
            gap = !out.is_empty();
            continue;
        }
        if c.is_control() {
            continue;
        }
        if gap {
            out.push(' ');
            n += 1;
            gap = false;
            if n >= PREVIEW_CHARS {
                break;
            }
        }
        out.push(c);
        n += 1;
        if n >= PREVIEW_CHARS {
            break;
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Msg {
    who: Who,
    text: String,
    turn_id: String,
    at: i64,
}

/// What a fold of a log knows about pins: the messages that can carry one
/// and which of them are marked. Kept by the view and by [`pins`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Book {
    msgs: BTreeMap<u64, Msg>,
    marked: BTreeSet<u64>,
}

impl Book {
    /// A message of the person's or the assistant's, at `seq`.
    pub fn message(&mut self, seq: u64, who: Who, text: &str, turn_id: &str, at: i64) {
        self.msgs.insert(
            seq,
            Msg {
                who,
                text: preview(text),
                turn_id: turn_id.to_string(),
                at,
            },
        );
    }

    /// A `message_pinned` written at `seq`. One that names no earlier
    /// message of this log counts for nothing.
    pub fn pin(&mut self, seq: u64, message_seq: u64, pinned: bool) {
        if message_seq >= seq || !self.msgs.contains_key(&message_seq) {
            return;
        }
        if pinned {
            self.marked.insert(message_seq);
        } else {
            self.marked.remove(&message_seq);
        }
    }

    /// Whether `message_seq` is a message of this log.
    pub fn has_message(&self, message_seq: u64) -> bool {
        self.msgs.contains_key(&message_seq)
    }

    pub fn is_pinned(&self, message_seq: u64) -> bool {
        self.marked.contains(&message_seq)
    }

    /// How many messages are marked (rewound ones included).
    pub fn count(&self) -> usize {
        self.marked.len()
    }

    /// The pins in log order, each marked with the rewind that hides its
    /// message, if one does.
    pub fn list(&self, ranges: &Ranges) -> Vec<Pin> {
        self.marked
            .iter()
            .filter_map(|seq| {
                let m = &self.msgs.get(seq)?;
                Some(Pin {
                    seq: *seq,
                    who: m.who,
                    text: m.text.clone(),
                    turn_id: m.turn_id.clone(),
                    at: m.at,
                    rewound: ranges.by(*seq),
                })
            })
            .collect()
    }

    fn read(&mut self, e: &RawEvent) {
        match e.typed() {
            Ok(EventKind::UserMessage(m)) => {
                self.message(e.seq, Who::User, &m.text, &m.turn_id, e.ts_ms)
            }
            Ok(EventKind::AssistantMessage(m)) if !m.text.is_empty() => {
                self.message(e.seq, Who::Assistant, &m.text, &m.turn_id, e.ts_ms)
            }
            Ok(EventKind::MessagePinned(p)) => self.pin(e.seq, p.message_seq, p.pinned),
            _ => {}
        }
    }
}

fn book(log: &[RawEvent]) -> Book {
    let mut b = Book::default();
    for e in log {
        b.read(e);
    }
    b
}

/// The chat's pins, in log order, as a list shows them: the latest event
/// for a message wins, and a pin of a message a rewind hides is not listed
/// until the rewind is undone.
pub fn pins(log: &[RawEvent]) -> Vec<Pin> {
    let ranges = rewind::rewound(log);
    book(log)
        .list(&ranges)
        .into_iter()
        .filter(|p| p.rewound.is_none())
        .collect()
}

/// Whether the owner may now pin (`pinned`) or unpin the message at
/// `message_seq` of `log` (the chat's whole log, or what it reads of it,
/// [`KINDS`]): `Ok` or why not, in the person's words. Refused: a seq that
/// is not a message of the person's or the assistant's in this log, one a
/// rewind hides, pinning what is pinned or unpinning what is not, and a
/// pin beyond [`MAX_PINS`].
pub fn check(log: &[RawEvent], message_seq: u64, pinned: bool) -> Result<(), String> {
    let b = book(log);
    if !b.has_message(message_seq) {
        return Err("only a message of this chat can be pinned".into());
    }
    if rewind::rewound(log).holds(message_seq) {
        return Err("that message is rewound; undo the rewind first".into());
    }
    match (pinned, b.is_pinned(message_seq)) {
        (true, true) => Err("that message is already pinned".into()),
        (false, false) => Err("that message is not pinned".into()),
        (true, false) if b.count() >= MAX_PINS => Err(format!(
            "this chat has {MAX_PINS} pinned messages; unpin one first"
        )),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionId;
    use crate::event::{
        AssistantMessage, ConversationRewindUndone, ConversationRewound, MessagePinned, UserMessage,
    };
    use serde_json::json;

    struct Log(Vec<RawEvent>);

    impl Log {
        fn new() -> Self {
            Log(vec![])
        }
        fn push(&mut self, k: EventKind) -> u64 {
            let (kind, payload) = k.into_parts();
            let seq = self.0.len() as u64 + 1;
            self.0.push(RawEvent {
                session_id: SessionId::new(),
                seq,
                ts_ms: seq as i64 * 1000,
                kind,
                payload,
            });
            seq
        }
        fn user(&mut self, turn: &str, text: &str) -> u64 {
            let m: UserMessage =
                serde_json::from_value(json!({"turn_id": turn, "text": text})).unwrap();
            self.push(EventKind::UserMessage(m))
        }
        fn answer(&mut self, turn: &str, text: &str) -> u64 {
            let m: AssistantMessage = serde_json::from_value(
                json!({"turn_id": turn, "message_id": format!("m{}", self.0.len()), "text": text}),
            )
            .unwrap();
            self.push(EventKind::AssistantMessage(m))
        }
        fn pin(&mut self, seq: u64, pinned: bool) -> u64 {
            self.push(EventKind::MessagePinned(MessagePinned {
                message_seq: seq,
                pinned,
            }))
        }
    }

    #[test]
    fn latest_event_for_a_message_wins() {
        let mut l = Log::new();
        let u = l.user("t", "hello");
        let a = l.answer("t", "hi there");
        l.pin(a, true);
        l.pin(u, true);
        l.pin(a, false);
        l.pin(a, true);
        l.pin(u, false);
        let p = pins(&l.0);
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].seq, p[0].who), (a, Who::Assistant));
        assert_eq!(p[0].text, "hi there");
        assert_eq!(p[0].turn_id, "t");
        assert_eq!(p[0].at, a as i64 * 1000);
    }

    #[test]
    fn pins_come_in_log_order_of_the_messages_not_of_the_pins() {
        let mut l = Log::new();
        let u = l.user("t", "q");
        let a = l.answer("t", "a");
        l.pin(a, true);
        l.pin(u, true);
        let seqs: Vec<u64> = pins(&l.0).iter().map(|p| p.seq).collect();
        assert_eq!(seqs, vec![u, a]);
    }

    #[test]
    fn the_preview_is_one_clean_line_of_120_characters() {
        let long = format!("  first\n\n  line\t{}\u{7}", "x".repeat(300));
        let p = preview(&long);
        assert!(p.starts_with("first line xxx"));
        assert_eq!(p.chars().count(), PREVIEW_CHARS);
        assert!(!p.contains(['\n', '\t', '\u{7}']));
        assert_eq!(
            preview("é".repeat(200).as_str()).chars().count(),
            PREVIEW_CHARS
        );
        assert_eq!(preview(" \n "), "");
    }

    #[test]
    fn check_refuses_what_is_not_a_message_of_this_chat() {
        let mut l = Log::new();
        let u = l.user("t", "q");
        l.pin(u, true);
        let pin_seq = 2;
        assert!(
            check(&l.0, 99, true)
                .unwrap_err()
                .contains("message of this chat")
        );
        assert!(check(&l.0, 0, true).is_err());
        // The pin event itself, and the session's other events, are no message.
        assert!(check(&l.0, pin_seq, true).is_err());
        // An assistant step with no text (a tool-call step) is no message either.
        let mut l = Log::new();
        let empty = l.answer("t", "");
        assert!(check(&l.0, empty, true).is_err());
    }

    #[test]
    fn check_refuses_a_repeat_and_an_unpin_of_nothing() {
        let mut l = Log::new();
        let u = l.user("t", "q");
        assert!(check(&l.0, u, false).unwrap_err().contains("not pinned"));
        assert!(check(&l.0, u, true).is_ok());
        l.pin(u, true);
        assert!(check(&l.0, u, true).unwrap_err().contains("already pinned"));
        assert!(check(&l.0, u, false).is_ok());
        l.pin(u, false);
        assert!(check(&l.0, u, false).is_err());
    }

    #[test]
    fn the_fifty_first_pin_is_refused_and_unpinning_makes_room() {
        let mut l = Log::new();
        let seqs: Vec<u64> = (0..=MAX_PINS)
            .map(|i| l.user("t", &format!("m{i}")))
            .collect();
        for s in &seqs[..MAX_PINS] {
            assert!(check(&l.0, *s, true).is_ok());
            l.pin(*s, true);
        }
        let err = check(&l.0, seqs[MAX_PINS], true).unwrap_err();
        assert_eq!(err, "this chat has 50 pinned messages; unpin one first");
        // Unpinning is never refused for the limit.
        assert!(check(&l.0, seqs[0], false).is_ok());
        l.pin(seqs[0], false);
        assert!(check(&l.0, seqs[MAX_PINS], true).is_ok());
    }

    fn rewind_to(l: &mut Log, message_seq: u64, turn: &str) -> u64 {
        l.push(EventKind::ConversationRewound(ConversationRewound {
            message_seq,
            turn_id: turn.into(),
            files: false,
        }))
    }

    #[test]
    fn a_rewound_message_cannot_be_pinned_and_its_pin_waits_for_the_undo() {
        let mut l = Log::new();
        let u1 = l.user("t1", "one");
        let a1 = l.answer("t1", "first answer");
        let u2 = l.user("t2", "two");
        let a2 = l.answer("t2", "second answer");
        l.pin(a1, true);
        l.pin(a2, true);
        let r = rewind_to(&mut l, u2, "t2");
        // The rewound answer is not listed and cannot be pinned or unpinned.
        let p = pins(&l.0);
        assert_eq!(p.iter().map(|p| p.seq).collect::<Vec<_>>(), vec![a1]);
        assert!(check(&l.0, a2, false).unwrap_err().contains("rewound"));
        assert!(check(&l.0, u2, true).unwrap_err().contains("rewound"));
        assert!(check(&l.0, u1, true).is_ok());
        // Its pin still counts toward the limit and comes back with the undo.
        l.push(EventKind::ConversationRewindUndone(
            ConversationRewindUndone { rewind_seq: r },
        ));
        let p = pins(&l.0);
        assert_eq!(p.iter().map(|p| p.seq).collect::<Vec<_>>(), vec![a1, a2]);
        assert!(check(&l.0, a2, false).is_ok());
    }

    #[test]
    fn a_pin_that_names_nothing_before_it_counts_for_nothing() {
        let mut l = Log::new();
        let u = l.user("t", "q");
        // A pin of its own seq, a later seq, and a seq that is no message.
        l.pin(3, true);
        l.pin(77, true);
        l.pin(2, true);
        assert!(pins(&l.0).is_empty());
        l.pin(u, true);
        assert_eq!(pins(&l.0).len(), 1);
    }

    #[test]
    fn a_forks_copied_events_carry_the_parents_pins() {
        // The fork keeps the copied events as they are (seqs and kinds), so
        // the same fold lists the same pins; a pin written after the cut is
        // not copied.
        let mut l = Log::new();
        let u = l.user("t", "q");
        let a = l.answer("t", "a");
        l.pin(u, true);
        let cut = l.0.len();
        l.pin(a, true);
        let copy: Vec<RawEvent> = l.0[..cut].to_vec();
        assert_eq!(
            pins(&copy).iter().map(|p| p.seq).collect::<Vec<_>>(),
            vec![u]
        );
        assert_eq!(pins(&l.0).len(), 2);
    }

    #[test]
    fn a_log_cut_to_kinds_gives_the_same_answer() {
        let mut l = Log::new();
        let u = l.user("t", "q");
        l.push(EventKind::Error(crate::event::ErrorEvent {
            turn_id: None,
            message: "x".into(),
            notice: false,
        }));
        l.pin(u, true);
        let cut: Vec<RawEvent> =
            l.0.iter()
                .filter(|e| KINDS.contains(&e.kind.as_str()))
                .cloned()
                .collect();
        // The cut renumbers nothing: seqs are the event's own.
        assert_eq!(pins(&cut), pins(&l.0));
    }
}
