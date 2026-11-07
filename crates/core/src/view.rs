//! The reference reducer: a session's display state as a fold over its log.
//! `web/src/fold.ts` is a port of this and must stay in step.

use std::collections::BTreeMap;

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
        /// Attachment file names.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachments: Vec<String>,
        /// The answer to an input block, when this message is one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply: Option<FormReply>,
    },
    /// Turns before this point reach the model only as a summary.
    Summary {
        through_seq: u64,
        text: String,
    },
    Assistant {
        message_id: String,
        text: String,
        done: bool,
        author: Option<String>,
        variant: Option<Variant>,
        /// Set by the final `assistant_message`; deltas never carry blocks.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        blocks: Vec<UiBlock>,
    },
    Tool {
        call_id: String,
        author: Option<String>,
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
    AgentChanged {
        agent: AgentRef,
    },
    /// `model` answered this turn instead of `from` (`model_fallback`).
    Fallback {
        turn_id: String,
        model: String,
        from: Option<String>,
        /// Why `from` was not used: "unavailable", or the provider's error.
        #[serde(default)]
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    pub ok: bool,
    pub output: String,
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<Source>,
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
    /// The session's selected agent (selector), if any.
    pub active_agent: Option<String>,
    /// The agent running the current turn (may differ via @mention).
    pub running_agent: Option<AgentRef>,
    /// Parallel-answer group -> chosen message id.
    pub variant_selection: BTreeMap<String, String>,
    /// Tokens used by all turns so far.
    pub usage: Usage,
    /// Context size of the latest turn (its last model call's input).
    pub context_tokens: u64,
    /// Git snapshots per turn (agentic sessions in a repository).
    pub checkpoints: BTreeMap<String, CheckpointState>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CheckpointState {
    pub before: Option<String>,
    pub after: Option<String>,
    /// Files the turn changed.
    pub files: Vec<String>,
    pub restored: bool,
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
        if let Some(id) = self.session_id
            && id != e.session_id
        {
            return Err(FoldError::WrongSession);
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
            EventKind::SessionCreated(m) => {
                self.active_agent = m.agent_id.clone();
                self.meta = Some(m);
            }
            EventKind::TurnStarted(t) => {
                self.running_turn = Some(t.turn_id);
                self.running_agent = t.agent;
            }
            EventKind::UserMessage(m) => self.items.push(Item::User {
                turn_id: m.turn_id,
                text: m.text,
                attachments: m.attachments.into_iter().map(|a| a.name).collect(),
                reply: m.reply,
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
                        author: d.author,
                        variant: d.variant,
                        blocks: vec![],
                    }),
                }
            }
            EventKind::AssistantMessage(m) => {
                let found = self.items.iter_mut().rev().find(|i| {
                    matches!(i, Item::Assistant { message_id, .. } if *message_id == m.message_id)
                });
                match found {
                    Some(Item::Assistant {
                        text,
                        done,
                        author,
                        variant,
                        blocks,
                        ..
                    }) => {
                        *text = m.text;
                        *done = true;
                        *blocks = m.blocks;
                        if m.author.is_some() {
                            *author = m.author;
                        }
                        if m.variant.is_some() {
                            *variant = m.variant;
                        }
                    }
                    _ => {
                        if !m.text.is_empty() {
                            self.items.push(Item::Assistant {
                                message_id: m.message_id,
                                text: m.text,
                                done: true,
                                author: m.author,
                                variant: m.variant,
                                blocks: m.blocks,
                            })
                        }
                    }
                }
            }
            EventKind::ToolCallStarted(t) => self.items.push(Item::Tool {
                call_id: t.call_id,
                author: t.author,
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
                        sources: f.sources,
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
                        && a.approval_id == r.approval_id
                    {
                        a.decision = Some(r.decision);
                        a.by = Some(r.by);
                        break;
                    }
                }
            }
            EventKind::Checkpoint(c) => {
                let st = self.checkpoints.entry(c.turn_id).or_default();
                match c.phase {
                    CheckpointPhase::Before => st.before = Some(c.git_ref),
                    CheckpointPhase::After => {
                        st.after = Some(c.git_ref);
                        st.files = c.files;
                    }
                    CheckpointPhase::Restored => st.restored = true,
                }
            }
            EventKind::TurnEnded(t) => {
                if let Some(u) = &t.usage {
                    self.usage.add(u);
                    self.context_tokens = u.context_tokens;
                }
                self.running_turn = None;
                self.running_agent = None;
                self.items.push(Item::TurnEnd {
                    turn_id: t.turn_id,
                    reason: t.reason,
                });
            }
            EventKind::Error(e) => self.items.push(Item::Error { message: e.message }),
            EventKind::AgentChanged(a) => {
                self.active_agent = Some(a.agent.agent_id.clone());
                self.items.push(Item::AgentChanged { agent: a.agent });
            }
            EventKind::VariantSelected(v) => {
                self.variant_selection.insert(v.group, v.message_id);
            }
            EventKind::HistorySummarized(h) => self.items.push(Item::Summary {
                through_seq: h.through_seq,
                text: h.text,
            }),
            EventKind::ModelFallback(f) => self.items.push(Item::Fallback {
                turn_id: f.turn_id,
                model: f.model,
                from: Some(f.from),
                reason: f.reason,
            }),
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
                    agent: None,
                    turn_id: "t".into(),
                }),
            ),
            ev(
                sid,
                2,
                EventKind::AssistantDelta(AssistantDelta {
                    author: None,
                    variant: None,
                    turn_id: "t".into(),
                    message_id: "m".into(),
                    text: "he".into(),
                }),
            ),
            ev(
                sid,
                3,
                EventKind::AssistantDelta(AssistantDelta {
                    author: None,
                    variant: None,
                    turn_id: "t".into(),
                    message_id: "m".into(),
                    text: "llo".into(),
                }),
            ),
            ev(
                sid,
                4,
                EventKind::ToolCallStarted(ToolCallStarted {
                    author: None,
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
    fn tool_results_keep_their_sources() {
        let sid = SessionId::new();
        let source = Source {
            id: "S1".into(),
            url: "https://a.example/".into(),
            title: "A".into(),
        };
        let evs = vec![
            ev(
                sid,
                1,
                EventKind::ToolCallStarted(ToolCallStarted {
                    author: None,
                    turn_id: "t".into(),
                    call_id: "c".into(),
                    tool: "web_search".into(),
                    input: json!({"query": "a"}),
                }),
            ),
            ev(
                sid,
                2,
                EventKind::ToolCallFinished(ToolCallFinished {
                    call_id: "c".into(),
                    ok: true,
                    output: "[S1] A".into(),
                    exit_code: None,
                    truncated: false,
                    sources: vec![source.clone()],
                }),
            ),
        ];
        let v = SessionView::fold(&evs).unwrap();
        let Item::Tool {
            result: Some(r), ..
        } = &v.items[0]
        else {
            panic!("{:?}", v.items[0])
        };
        assert_eq!(r.sources, vec![source]);
    }

    #[test]
    fn assistant_items_take_the_final_messages_blocks() {
        let sid = SessionId::new();
        let block = UiBlock {
            ordinal: 0,
            hash: "811c9dc5".into(),
            kind: UiKind::Progress,
            id: "p".into(),
            spec: Some(json!({"title": "T"})),
            error: None,
        };
        let message = |id: &str, blocks: Vec<UiBlock>| {
            EventKind::AssistantMessage(AssistantMessage {
                turn_id: "t".into(),
                message_id: id.into(),
                text: "x".into(),
                tool_calls: vec![],
                author: None,
                variant: None,
                blocks,
            })
        };
        let evs = vec![
            ev(
                sid,
                1,
                EventKind::AssistantDelta(AssistantDelta {
                    author: None,
                    variant: None,
                    turn_id: "t".into(),
                    message_id: "m1".into(),
                    text: "x".into(),
                }),
            ),
            ev(sid, 2, message("m1", vec![block.clone()])),
            ev(sid, 3, message("m2", vec![block.clone()])),
        ];
        let v = SessionView::fold(&evs).unwrap();
        for item in &v.items {
            let Item::Assistant { blocks, .. } = item else {
                panic!("{item:?}")
            };
            assert_eq!(blocks, &vec![block.clone()]);
        }
        let json = serde_json::to_value(&v.items[0]).unwrap();
        assert_eq!(json["blocks"][0]["type"], "progress");
    }

    #[test]
    fn user_items_keep_their_reply() {
        let sid = SessionId::new();
        let reply = FormReply {
            message_id: "m1".into(),
            block_id: "db".into(),
            values: json!({"selected": ["pg"]}).as_object().unwrap().clone(),
        };
        let msg = |reply| {
            EventKind::UserMessage(UserMessage {
                turn_id: "t".into(),
                text: "Choice answer · db: pg".into(),
                attachments: vec![],
                reply,
            })
        };
        let evs = vec![ev(sid, 1, msg(Some(reply.clone()))), ev(sid, 2, msg(None))];
        let v = SessionView::fold(&evs).unwrap();
        let replies: Vec<_> = v
            .items
            .iter()
            .map(|i| match i {
                Item::User { reply, .. } => reply.clone(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(replies, [Some(reply), None]);
        let json = serde_json::to_value(&v.items[1]).unwrap();
        assert!(json.get("reply").is_none(), "omitted when absent: {json}");
    }

    #[test]
    fn rejects_gaps_and_duplicates() {
        let sid = SessionId::new();
        let e1 = ev(
            sid,
            1,
            EventKind::TurnStarted(TurnStarted {
                agent: None,
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
