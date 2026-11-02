//! The reference reducer: a session's display state as a fold over its log.
//! `web/src/fold.ts` is a port of this and must stay in step.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::event::*;
use crate::policy::Decision;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Item {
    User {
        turn_id: String,
        text: String,
    },
    Assistant {
        message_id: String,
        text: String,
        done: bool,
    },
    Tool {
        call_id: String,
        tool: String,
        input: Value,
        live_output: String,
        result: Option<ToolResult>,
        approval: Option<ApprovalState>,
    },
    Error {
        message: String,
    },
    TurnEnd {
        turn_id: String,
        reason: TurnEndReason,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    pub ok: bool,
    pub output: String,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalState {
    pub approval_id: String,
    pub reason: String,
    pub decision: Option<Decision>,
    pub by: Option<ResolvedBy>,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FoldError {
    #[error("expected seq {expected}, got {got}")]
    OutOfOrder { expected: u64, got: u64 },
    #[error("event for another session")]
    WrongSession,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionView {
    pub session_id: Option<SessionId>,
    pub head_seq: u64,
    pub meta: Option<SessionCreated>,
    pub items: Vec<Item>,
    pub running_turn: Option<String>,
    /// Approval ids awaiting a decision, in request order.
    pub pending_approvals: Vec<String>,
    /// Count of events with kinds this build does not understand.
    pub unknown_events: u64,
}

impl SessionView {
    pub fn fold<'a>(events: impl IntoIterator<Item = &'a RawEvent>) -> Result<Self, FoldError> {
        let mut v = SessionView::default();
        for e in events {
            v.apply(e)?;
        }
        Ok(v)
    }

    /// Apply one event. Seq must be exactly `head_seq + 1`.
    pub fn apply(&mut self, e: &RawEvent) -> Result<(), FoldError> {
        if let Some(id) = self.session_id {
            if id != e.session_id {
                return Err(FoldError::WrongSession);
            }
        }
        if e.seq != self.head_seq + 1 {
            return Err(FoldError::OutOfOrder {
                expected: self.head_seq + 1,
                got: e.seq,
            });
        }
        self.session_id = Some(e.session_id);
        self.head_seq = e.seq;
        match e.typed() {
            Ok(k) => self.apply_kind(k),
            Err(_) => self.unknown_events += 1,
        }
        Ok(())
    }

    fn tool_mut(&mut self, call_id: &str) -> Option<&mut Item> {
        self.items
            .iter_mut()
            .rev()
            .find(|i| matches!(i, Item::Tool { call_id: c, .. } if c == call_id))
    }

    fn apply_kind(&mut self, k: EventKind) {
        match k {
            EventKind::SessionCreated(m) => self.meta = Some(m),
            EventKind::TurnStarted(t) => self.running_turn = Some(t.turn_id),
            EventKind::UserMessage(m) => self.items.push(Item::User {
                turn_id: m.turn_id,
                text: m.text,
            }),
            EventKind::AssistantDelta(d) => {
                let found = self.items.iter_mut().rev().find_map(|i| match i {
                    Item::Assistant {
                        message_id, text, ..
                    } if *message_id == d.message_id => Some(text),
                    _ => None,
                });
                match found {
                    Some(text) => text.push_str(&d.text),
                    None => self.items.push(Item::Assistant {
                        message_id: d.message_id,
                        text: d.text,
                        done: false,
                    }),
                }
            }
            EventKind::AssistantMessage(m) => {
                let found = self.items.iter_mut().rev().find(|i| {
                    matches!(i, Item::Assistant { message_id, .. } if *message_id == m.message_id)
                });
                match found {
                    Some(Item::Assistant { text, done, .. }) => {
                        *text = m.text;
                        *done = true;
                    }
                    _ => {
                        if !m.text.is_empty() {
                            self.items.push(Item::Assistant {
                                message_id: m.message_id,
                                text: m.text,
                                done: true,
                            })
                        }
                    }
                }
            }
            EventKind::ToolCallStarted(t) => self.items.push(Item::Tool {
                call_id: t.call_id,
                tool: t.tool,
                input: t.input,
                live_output: String::new(),
                result: None,
                approval: None,
            }),
            EventKind::ToolOutputDelta(d) => {
                if let Some(Item::Tool { live_output, .. }) = self.tool_mut(&d.call_id) {
                    live_output.push_str(&d.text);
                }
            }
            EventKind::ToolCallFinished(f) => {
                if let Some(Item::Tool { result, .. }) = self.tool_mut(&f.call_id) {
                    *result = Some(ToolResult {
                        ok: f.ok,
                        output: f.output,
                        exit_code: f.exit_code,
                    });
                }
            }
            EventKind::ApprovalRequested(a) => {
                self.pending_approvals.push(a.approval_id.clone());
                if let Some(Item::Tool { approval, .. }) = self.tool_mut(&a.call_id) {
                    *approval = Some(ApprovalState {
                        approval_id: a.approval_id,
                        reason: a.reason,
                        decision: None,
                        by: None,
                    });
                }
            }
            EventKind::ApprovalResolved(r) => {
                self.pending_approvals.retain(|id| *id != r.approval_id);
                for item in self.items.iter_mut().rev() {
                    if let Item::Tool {
                        approval: Some(a), ..
                    } = item
                    {
                        if a.approval_id == r.approval_id {
                            a.decision = Some(r.decision);
                            a.by = Some(r.by);
                            break;
                        }
                    }
                }
            }
            EventKind::Checkpoint(_) => {}
            EventKind::TurnEnded(t) => {
                self.running_turn = None;
                self.items.push(Item::TurnEnd {
                    turn_id: t.turn_id,
                    reason: t.reason,
                });
            }
            EventKind::Error(e) => self.items.push(Item::Error { message: e.message }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(sid: SessionId, seq: u64, k: EventKind) -> RawEvent {
        let (kind, payload) = k.into_parts();
        RawEvent {
            session_id: sid,
            seq,
            ts_ms: 0,
            kind,
            payload,
        }
    }

    #[test]
    fn folds_a_turn() {
        let sid = SessionId::new();
        let evs = vec![
            ev(
                sid,
                1,
                EventKind::TurnStarted(TurnStarted {
                    turn_id: "t".into(),
                }),
            ),
            ev(
                sid,
                2,
                EventKind::AssistantDelta(AssistantDelta {
                    turn_id: "t".into(),
                    message_id: "m".into(),
                    text: "he".into(),
                }),
            ),
            ev(
                sid,
                3,
                EventKind::AssistantDelta(AssistantDelta {
                    turn_id: "t".into(),
                    message_id: "m".into(),
                    text: "llo".into(),
                }),
            ),
            ev(
                sid,
                4,
                EventKind::ToolCallStarted(ToolCallStarted {
                    turn_id: "t".into(),
                    call_id: "c".into(),
                    tool: "bash".into(),
                    input: json!({}),
                }),
            ),
            ev(
                sid,
                5,
                EventKind::ApprovalRequested(ApprovalRequested {
                    approval_id: "a".into(),
                    call_id: "c".into(),
                    tool: "bash".into(),
                    input: json!({}),
                    reason: "r".into(),
                }),
            ),
        ];
        let v = SessionView::fold(&evs).unwrap();
        assert_eq!(v.head_seq, 5);
        assert_eq!(v.running_turn.as_deref(), Some("t"));
        assert_eq!(v.pending_approvals, vec!["a".to_string()]);
        match &v.items[0] {
            Item::Assistant { text, done, .. } => {
                assert_eq!(text, "hello");
                assert!(!done);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn rejects_gaps_and_duplicates() {
        let sid = SessionId::new();
        let e1 = ev(
            sid,
            1,
            EventKind::TurnStarted(TurnStarted {
                turn_id: "t".into(),
            }),
        );
        let mut v = SessionView::default();
        v.apply(&e1).unwrap();
        assert_eq!(
            v.apply(&e1),
            Err(FoldError::OutOfOrder {
                expected: 2,
                got: 1
            })
        );
        let mut e3 = e1.clone();
        e3.seq = 3;
        assert!(v.apply(&e3).is_err());
    }
}
