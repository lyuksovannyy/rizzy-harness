//! Checking a reply against the session's own log (rich_plan §7.3-7.4).
//!
//! The node calls [`check_reply`] while it holds the session's turn slot,
//! so nothing can append between the check and the answer being recorded:
//! the log it reads is the log the answer joins. What the browser shows
//! (an open card, a Send button) is never trusted.

use std::collections::HashMap;

use super::answer::answer;
use crate::RawEvent;
use crate::event::{AssistantMessage, EventKind, FormReply, UiBlock};

/// The largest reply, serialized, in bytes.
pub const MAX_REPLY: usize = 32 * 1024;

pub const TOO_LARGE: &str = "the answer is larger than 32 KiB";

pub const NOT_OPEN: &str = "this form is no longer open";

/// Is `reply` over [`MAX_REPLY`] once serialized?
pub fn too_large(reply: &FormReply) -> bool {
    serde_json::to_vec(reply).map_or(true, |b| b.len() > MAX_REPLY)
}

/// The canonical text for `reply` to `block` (see `answer`), or why its
/// values do not answer it: the size first, then every value rule. Each
/// field has its own length limit, so a form of long text fields can pass
/// them all and still be too large in total. `web/src/ui/form.ts` runs the
/// same checks in the same order (ui.json holds the two together).
pub fn answer_reply(block: &UiBlock, reply: &FormReply) -> Result<String, String> {
    if too_large(reply) {
        return Err(TOO_LARGE.into());
    }
    answer(block, &reply.values)
}

/// The canonical text for `reply` (see `answer_reply`), or why the session
/// does not take it now.
pub fn check_reply(history: &[RawEvent], reply: &FormReply) -> Result<String, String> {
    let mut asked: Option<AssistantMessage> = None;
    let mut answered = false;
    let mut chosen: HashMap<String, String> = HashMap::new();
    for e in history {
        match e.typed() {
            Ok(EventKind::AssistantMessage(m)) if m.message_id == reply.message_id => {
                asked = Some(m);
                answered = false;
            }
            // Any later message from the user ends the question, an answer
            // or not: the conversation went on without it.
            Ok(EventKind::UserMessage(_)) if asked.is_some() => answered = true,
            Ok(EventKind::VariantSelected(v)) => {
                chosen.insert(v.group, v.message_id);
            }
            _ => {}
        }
    }
    let Some(m) = asked else {
        return Err("no such message".into());
    };
    if answered {
        return Err(NOT_OPEN.into());
    }
    if let Some(v) = &m.variant {
        let picked = match chosen.get(&v.group) {
            Some(id) => *id == m.message_id,
            None => v.index == 0,
        };
        if !picked {
            return Err("continue with this answer first".into());
        }
    }
    let Some(block) = m.blocks.iter().find(|b| b.id == reply.block_id) else {
        return Err("no such question or form in that message".into());
    };
    answer_reply(block, reply)
}

/// What the user message says: the canonical answer, then the user's note.
pub fn answer_text(canonical: String, note: &str) -> String {
    match note.trim() {
        "" => canonical,
        note => format!("{canonical}\n\n{note}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionId;
    use crate::agent::UiLevel;
    use crate::event::*;
    use crate::policy::ApprovalMode;
    use serde_json::{Value, json};

    const ASK: &str = "Pick:\n\n```ui:choices\n{\"id\": \"db\", \"question\": \"Which?\", \"options\": [\"a\", \"b\"]}\n```\n\n```ui:progress\n{\"id\": \"p\", \"title\": \"T\", \"steps\": [{\"label\": \"x\", \"status\": \"done\"}]}\n```\n\n```ui:form\n{\"id\": \"f\"}\n```\n";

    fn created() -> EventKind {
        EventKind::SessionCreated(SessionCreated {
            title: "t".into(),
            cwd: "/w".into(),
            model: "m".into(),
            backend: "native".into(),
            node_id: "local".into(),
            approval_mode: ApprovalMode::Ask,
            profile: Profile::Chat,
            agent_id: None,
            incognito: false,
            parent_session: None,
        })
    }

    fn asked(id: &str, variant: Option<(&str, u32)>) -> EventKind {
        EventKind::AssistantMessage(AssistantMessage {
            turn_id: "t".into(),
            message_id: id.into(),
            text: ASK.into(),
            tool_calls: vec![],
            author: None,
            variant: variant.map(|(group, index)| Variant {
                group: group.into(),
                index,
                model: None,
            }),
            blocks: super::super::extract(ASK, UiLevel::Rich),
        })
    }

    fn user() -> EventKind {
        EventKind::UserMessage(UserMessage {
            turn_id: "t2".into(),
            text: "hi".into(),
            attachments: vec![],
            reply: None,
        })
    }

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

    fn reply(message: &str, block: &str, values: Value) -> FormReply {
        FormReply {
            message_id: message.into(),
            block_id: block.into(),
            values: values.as_object().unwrap().clone(),
        }
    }

    fn pick() -> FormReply {
        reply("m1", "db", json!({"selected": ["o2"]}))
    }

    #[test]
    fn an_open_question_takes_its_answer() {
        let h = log(vec![created(), user(), asked("m1", None)]);
        assert_eq!(
            check_reply(&h, &pick()),
            Ok("Choice answer · db: o2".into())
        );
        assert_eq!(
            check_reply(&h, &reply("m1", "db", json!({"selected": ["zz"]}))),
            Err("selected: not one of the options".into())
        );
    }

    #[test]
    fn a_later_user_message_closes_it() {
        let h = log(vec![created(), asked("m1", None), user()]);
        assert_eq!(check_reply(&h, &pick()), Err(NOT_OPEN.into()));
    }

    #[test]
    fn unknown_messages_and_blocks() {
        let h = log(vec![created(), asked("m1", None)]);
        assert_eq!(
            check_reply(&h, &reply("m9", "db", json!({}))),
            Err("no such message".into())
        );
        for (block, err) in [
            ("zz", "no such question or form in that message"),
            ("p", "that block is not a question or form"),
            // The form is the message's second input block: an error.
            ("f", "that block was not shown"),
        ] {
            assert_eq!(
                check_reply(&h, &reply("m1", block, json!({}))),
                Err(err.into()),
                "{block}"
            );
        }
    }

    #[test]
    fn only_the_chosen_parallel_answer_is_open() {
        let both = || {
            vec![
                created(),
                asked("m1", Some(("g", 0))),
                asked("m2", Some(("g", 1))),
            ]
        };
        let h = log(both());
        let second = reply("m2", "db", json!({"selected": ["o1"]}));
        assert!(check_reply(&h, &pick()).is_ok(), "index 0 by default");
        assert_eq!(
            check_reply(&h, &second),
            Err("continue with this answer first".into())
        );
        let mut kinds = both();
        kinds.push(EventKind::VariantSelected(VariantSelected {
            group: "g".into(),
            message_id: "m2".into(),
        }));
        let h = log(kinds);
        assert!(check_reply(&h, &second).is_ok());
        assert_eq!(
            check_reply(&h, &pick()),
            Err("continue with this answer first".into())
        );
    }

    #[test]
    fn oversized_replies_are_refused() {
        let h = log(vec![created(), asked("m1", None)]);
        let big = reply(
            "m1",
            "db",
            json!({"selected": ["o1"], "other": "x".repeat(MAX_REPLY)}),
        );
        assert_eq!(check_reply(&h, &big), Err(TOO_LARGE.into()));
    }

    #[test]
    fn long_fields_can_be_too_large_together() {
        let fields: Vec<Value> = (1..=9)
            .map(|i| json!({"id": format!("p{i}"), "label": "Part", "type": "text", "multiline": true, "max_length": 4000}))
            .collect();
        let text = format!(
            "```ui:form\n{}\n```\n",
            json!({"id": "essay", "title": "Essay", "fields": fields})
        );
        let block = &super::super::extract(&text, UiLevel::Rich)[0];
        let each = |n: usize| {
            let values: serde_json::Map<String, Value> = (1..=9)
                .map(|i| (format!("p{i}"), json!("x".repeat(n))))
                .collect();
            reply("m1", "essay", Value::Object(values))
        };
        assert!(answer_reply(block, &each(3500)).is_ok());
        // Every field is within its 4000 characters; together they are not.
        assert_eq!(answer_reply(block, &each(3700)), Err(TOO_LARGE.into()));
    }

    #[test]
    fn the_note_follows_the_answer() {
        assert_eq!(answer_text("A".into(), "  "), "A");
        assert_eq!(answer_text("A".into(), " thanks\n"), "A\n\nthanks");
    }
}
