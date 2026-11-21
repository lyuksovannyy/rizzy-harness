//! The reference reducer: a session's display state as a fold over its log.
//! `web/src/fold.ts` is a port of this and must stay in step.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::event::*;
use crate::policy::Decision;

/// One block of a step's thinking as the view keeps it: its kind
/// (`signed`, `redacted`, `text`) and its readable text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThoughtBlock {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl ThoughtBlock {
    fn of(b: &ThinkingBlock) -> Self {
        let (kind, text) = match b {
            ThinkingBlock::Signed { text, .. } => ("signed", Some(text.clone())),
            ThinkingBlock::Redacted { .. } => ("redacted", None),
            ThinkingBlock::Text { text } => ("text", Some(text.clone())),
        };
        Self {
            kind: kind.into(),
            text,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Item {
    User {
        turn_id: String,
        text: String,
        /// Attachment file names.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachments: Vec<String>,
        /// Paths the person mentioned with `@path` (T63): shown as chips,
        /// never their content.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        files: Vec<String>,
        /// The answer to an input block, when this message is one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply: Option<FormReply>,
        /// Images sent with it (references; the bytes stay in Media).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<ImageRef>,
        /// Sent while a turn ran (`user_message.queued`, X.5): where it was
        /// sent, and whether the agent has read it yet.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        queued: Option<Queued>,
        /// The agents it called by mention (`user_message.triggers`, X.37),
        /// in the order they answer.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        triggers: Vec<String>,
        /// The sub-agent-only agents it mentions (`user_message.tags`):
        /// tags, never called.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tags: Vec<String>,
    },
    /// Turns before this point reach the model only as a summary.
    Summary {
        through_seq: u64,
        text: String,
        /// Made at the person's `/compact` (X.20), with what they asked
        /// to keep in focus.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        manual: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        focus: Option<String>,
    },
    /// What the model is sent was shortened here (`context_shortened`,
    /// X.15): older tool results as one-line stubs (`cleared`), this turn's
    /// first `steps` steps as a summary (`text`, model-written). Everything
    /// stays shown in the chat as it was.
    Shortened {
        turn_id: String,
        cleared: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        steps: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        /// The oldest steps' thinking left out (X.25).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        thinking: bool,
        tokens_before: u64,
        tokens_after: u64,
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
    /// A step's thinking (`assistant_message.thinking`, X.25), before
    /// what the step said: each block's kind and readable text only (no
    /// signature, no redacted data: `event::for_browser`). The chat shows
    /// it collapsed, as text; nothing when nothing is readable.
    Thinking {
        message_id: String,
        blocks: Vec<ThoughtBlock>,
    },
    Tool {
        call_id: String,
        author: Option<String>,
        tool: String,
        input: Value,
        live_output: String,
        result: Option<ToolResult>,
        approval: Option<ApprovalState>,
        /// A `publish` call's file, once the node staged it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        staged: Option<Box<ArtifactStaged>>,
    },
    /// A file the user saved as an artifact ("Save as artifact"), or a
    /// staged file whose tool call is not in the log.
    Artifact { staged: Box<ArtifactStaged> },
    /// Saving a file as an artifact failed on the node.
    ArtifactFailed {
        path: String,
        name: Option<String>,
        error: String,
    },
    Error {
        message: String,
        /// Information for the person, not a failure (`ErrorEvent::notice`).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        notice: bool,
    },
    TurnEnd {
        turn_id: String,
        reason: TurnEndReason,
    },
    /// The chat switched agent or model at the message after it.
    AgentChanged {
        agent: AgentRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    /// The owner changed the chat's approval mode (`approval_mode_changed`).
    ApprovalModeChanged {
        mode: crate::policy::ApprovalMode,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        requested: Option<crate::policy::ApprovalMode>,
    },
    /// The owner changed the chat's step limit (`step_limit_changed`).
    StepLimitChanged {
        max_steps: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        requested: Option<i64>,
    },
    /// A hand-off note was written (`handoff_written`, X.16): `text` is the
    /// model's, shown as text; none when every model in `failed` failed.
    Handoff {
        through_seq: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        failed: Vec<NoteFailure>,
    },
    /// A background command ended (`background_ended`). `tail` is program
    /// output: shown as plain text, never rendered.
    BackgroundEnded {
        bg_id: String,
        reason: BackgroundReason,
        exit_code: Option<i32>,
        ran_ms: u64,
        /// A command's last output; a background sub-agent's result
        /// (X.24: its report quoted, model text), shown as plain text too.
        tail: String,
        /// Set for a background sub-agent: its agent's id.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent: Option<String>,
    },
    /// A turn the harness started because background commands ended while
    /// the chat was idle (`turn_started.cause`): where a person's message
    /// would be. No text: the chat names the ends from their own items.
    Callback {
        turn_id: String,
        bg_ids: Vec<String>,
    },
    /// A turn of an agent the person's message called after the first
    /// (`turn_started.cause`, X.37): where its answer begins, with who
    /// called it. No text: the agent's own answer follows.
    Called {
        turn_id: String,
        agent: Option<AgentRef>,
        /// The seq of the message that called it.
        message_seq: u64,
    },
    /// Agents a message called that were not asked (`peers_skipped`).
    PeersSkipped {
        agents: Vec<String>,
        reason: crate::event::PeersSkipReason,
    },
    /// The person's Retry of the failed turn `of` (`turn_started.retry`,
    /// X.21): a new turn that answers the same message, shown once above
    /// the failed attempt. No text: the chat says what it retries.
    Retry {
        turn_id: String,
        of: String,
        reason: TurnEndReason,
        /// The models of the failed attempt and of the retry, when known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        failed_model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    /// The node did not resume the agent for these ends
    /// (`callback_refused`); they wait for the person's next message.
    CallbackRefused { bg_ids: Vec<String>, reason: String },
    /// `model` answered this turn instead of `from` (`model_fallback`).
    Fallback {
        turn_id: String,
        model: String,
        from: String,
        /// Why `from` was not used: "unavailable", or the provider's error.
        #[serde(default)]
        reason: Option<String>,
    },
}

/// The chat's items, each with the seq of the event that made it and, once
/// a rewind hides it (`conversation_rewound`, X.46), the seq of that rewind:
/// written to the browser as the item's `rewound` field. A slice to every
/// reader; only the fold adds to it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Items {
    list: Vec<Item>,
    /// The seq of the event each item was made from.
    born: Vec<u64>,
    /// The earliest rewind that hides each item.
    mark: Vec<Option<u64>>,
    /// The seq of the event being folded.
    now: u64,
}

impl Items {
    fn at(&mut self, seq: u64) {
        self.now = seq;
    }

    fn push(&mut self, item: Item) {
        self.list.push(item);
        self.born.push(self.now);
        self.mark.push(None);
    }

    fn insert(&mut self, at: usize, item: Item) {
        self.list.insert(at, item);
        self.born.insert(at, self.now);
        self.mark.insert(at, None);
    }

    /// Mark each item as `ranges` says: after a rewind or an undo.
    fn remark(&mut self, ranges: &crate::rewind::Ranges) {
        for (m, b) in self.mark.iter_mut().zip(&self.born) {
            *m = ranges.by(*b);
        }
    }

    /// The seq of the rewind that hides item `i`.
    pub fn rewound(&self, i: usize) -> Option<u64> {
        self.mark.get(i).copied().flatten()
    }
}

impl std::ops::Deref for Items {
    type Target = [Item];
    fn deref(&self) -> &[Item] {
        &self.list
    }
}

impl std::ops::DerefMut for Items {
    fn deref_mut(&mut self) -> &mut [Item] {
        &mut self.list
    }
}

impl<'a> IntoIterator for &'a Items {
    type Item = &'a Item;
    type IntoIter = std::slice::Iter<'a, Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.list.iter()
    }
}

impl PartialEq<Vec<Item>> for Items {
    fn eq(&self, other: &Vec<Item>) -> bool {
        self.list == *other
    }
}

impl<const N: usize> PartialEq<[Item; N]> for Items {
    fn eq(&self, other: &[Item; N]) -> bool {
        self.list == *other
    }
}

impl Serialize for Items {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{Error, SerializeSeq};
        let mut seq = s.serialize_seq(Some(self.list.len()))?;
        for (i, item) in self.list.iter().enumerate() {
            match self.mark.get(i).copied().flatten() {
                None => seq.serialize_element(item)?,
                Some(by) => {
                    let mut v = serde_json::to_value(item).map_err(S::Error::custom)?;
                    if let Value::Object(o) = &mut v {
                        o.insert("rewound".into(), by.into());
                    }
                    seq.serialize_element(&v)?;
                }
            }
        }
        seq.end()
    }
}

/// One rewind of the chat as the view lists it (`conversation_rewound`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RewindInfo {
    /// The rewind event's own seq.
    pub seq: u64,
    /// The message it went back to, and that message's turn.
    pub message_seq: u64,
    pub turn_id: String,
    /// The turns it took out of the conversation (the message's own and
    /// every one that started after it).
    pub turns: u32,
    /// Whether the files were put back too.
    pub files: bool,
    /// Put back by `conversation_rewind_undone`.
    pub undone: bool,
    /// Not undone, and nothing (a message, a turn, a summary) came after it.
    pub can_undo: bool,
}

/// A message sent while a turn ran: its seq, which `queued_delivered`
/// names, and whether that has happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Queued {
    pub seq: u64,
    pub read: bool,
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
    /// Whether "allow for this turn" may be offered (see `ApprovalRequested`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub turn_allow: bool,
    /// Whether "allow for this chat" may be offered.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub chat_allow: bool,
    /// What the call would change, for the person (`ApprovalRequested::preview`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    pub decision: Option<Decision>,
    pub by: Option<ResolvedBy>,
    /// The person's note with a denial.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// A call the person allowed for the rest of the chat (`chat_allow_added`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatAllow {
    pub tool: String,
    pub input: Value,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FoldError {
    #[error("expected seq {expected}, got {got}")]
    OutOfOrder { expected: u64, got: u64 },
    #[error("event for another session")]
    WrongSession,
}

/// Only [`SessionView::fold`] builds a view (it is written to the browser,
/// never read back: `settled` and `live` are not part of its shape).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SessionView {
    pub session_id: Option<SessionId>,
    pub head_seq: u64,
    pub meta: Option<SessionCreated>,
    pub items: Items,
    pub running_turn: Option<String>,
    /// Approval ids awaiting a decision, in request order.
    pub pending_approvals: Vec<String>,
    /// Count of events with kinds this build does not understand.
    pub unknown_events: u64,
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
    /// Background commands by id (`background_started`, `_ended`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub background: BTreeMap<String, BackgroundState>,
    /// The agent's task list: the latest `todo_updated`'s (none: empty).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub todo: Vec<crate::todo::TodoItem>,
    /// The chat's own step limit (the latest `step_limit_changed`); none:
    /// the node's `[limits] max_steps`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_steps: Option<u64>,
    /// A model call waits to be sent again (`model_retry`); gone once
    /// output resumes, another model answers or the turn ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<ModelRetry>,
    /// The latest turn's limits (`turn_ended.usage`): the window of the
    /// model it started on, and where the next summary is written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_at: Option<u64>,
    /// The model `context_window` is of.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_model: Option<String>,
    /// Calls allowed for the rest of the chat, by grant id; removed ones
    /// are gone.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub chat_allows: BTreeMap<String, ChatAllow>,
    /// The latest turn's own usage (`turn_ended.usage`), for what the last
    /// turn cost; `usage` above is the whole chat's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_usage: Option<Usage>,
    /// What the finished turns and every other cost the chat carries (a
    /// hand-off, a summary, a background agent) used: `usage` is this plus
    /// `live` (X.39). Not part of the view's wire shape.
    #[serde(skip)]
    settled: Usage,
    /// The running turn's latest `turn_usage` (its id and running total).
    #[serde(skip)]
    live: Option<(String, Usage)>,
    /// The latest `before_restore`'s id: the restore whose `restored`
    /// events follow it (a failed restore leaves one with none, and the
    /// next restore writes its own). Not part of the view's wire shape.
    #[serde(skip)]
    saving: Option<String>,
    /// For the last [`MAX_SAVED`] `before_restore` ids: which turns were
    /// undone (and by which saved id) just before that restore. Restoring a
    /// saved id returns the folder to that moment, so it returns these flags
    /// (X.42). Not part of the view's wire shape.
    #[serde(skip)]
    undone_before: Vec<(String, BTreeMap<String, String>)>,
    /// In a sub-agent's chat: the turns that were follow-ups from the chat
    /// that started it (`turn_started.continued_by`, T68), in order. Its
    /// first run is not one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follow_ups: Vec<String>,
    /// The chat's rewinds, in the order they were made (X.46).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rewinds: Vec<RewindInfo>,
    /// The rewinds that count (not part of the view's wire shape).
    #[serde(skip)]
    ranges: crate::rewind::Ranges,
    /// Every turn's start (seq, turn id), for a rewind's turn count.
    #[serde(skip)]
    turn_starts: Vec<(u64, String)>,
}

/// One background command as the log shows it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BackgroundState {
    /// The `bash` call that started it.
    pub call_id: String,
    /// When its timeout ends it at the latest, on the node's clock.
    pub ends_by_ms: i64,
    /// The timeout it runs under: what is left can never be more.
    pub timeout_s: u64,
    pub ended: bool,
    /// A background sub-agent (X.24), not a command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child: Option<crate::event::BackgroundChild>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CheckpointState {
    pub before: Option<String>,
    pub after: Option<String>,
    /// Files the turn changed.
    pub files: Vec<String>,
    pub restored: bool,
    /// Repositories inside the folder that a snapshot of this checkpoint
    /// left out, or that its restore left alone (undo never touches them).
    /// Sorted, no duplicates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded: Vec<String>,
    /// The most any of its events said it did not name.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub excluded_more: u64,
    /// How the person's repository's HEAD moved during the turn, when it
    /// did (X.42): the `after` event's note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_moved: Option<crate::event::HeadMoved>,
    /// While `restored`: the checkpoint the folder was saved as just before
    /// the undo that restored this turn (its `before_restore`). Restoring
    /// that id puts back what the undo replaced. Dropped, with `restored`,
    /// when that restore is itself undone (X.42).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved: Option<String>,
}

/// How many saved ids' undone flags the fold remembers: the checkpoints a
/// folder keeps. A restore of an older one fails on the node and changes none.
pub const MAX_SAVED: usize = 100;

fn is_zero(n: &u64) -> bool {
    *n == 0
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
        self.items.at(e.seq);
        match e.typed() {
            Ok(k) => self.apply_kind(k, e.seq),
            Err(_) => self.unknown_events += 1,
        }
        Ok(())
    }

    /// `usage` = the settled sums plus the running turn's (X.39).
    fn show_usage(&mut self) {
        self.usage = self.settled.clone();
        if let Some((_, live)) = &self.live {
            self.usage.add(live);
        }
    }

    /// A cost the chat carries whatever turn runs: settled.
    fn settle(&mut self, u: &Usage) {
        self.settled.add(u);
        self.show_usage();
    }

    /// A message, a turn or a summary after a rewind: it can no longer be
    /// put back (`rewind::check_undo`).
    fn no_more_undo(&mut self) {
        for r in &mut self.rewinds {
            r.can_undo = false;
        }
    }

    fn tool_mut(&mut self, call_id: &str) -> Option<&mut Item> {
        self.items
            .iter_mut()
            .rev()
            .find(|i| matches!(i, Item::Tool { call_id: c, .. } if c == call_id))
    }

    fn apply_kind(&mut self, k: EventKind, seq: u64) {
        // Anything the turn shows next means the wait is over.
        if matches!(
            k,
            EventKind::TurnStarted(_)
                | EventKind::AssistantDelta(_)
                | EventKind::AssistantMessage(_)
                | EventKind::ToolCallStarted(_)
                | EventKind::TurnEnded(_)
                | EventKind::HistorySummarized(_)
                | EventKind::ContextShortened(_)
                | EventKind::ModelFallback(_)
                | EventKind::RetryGivenUp(_)
        ) {
            self.retry = None;
        }
        match k {
            EventKind::SessionCreated(m) => {
                self.meta = Some(m);
            }
            EventKind::TurnStarted(t) => {
                self.turn_starts.push((seq, t.turn_id.clone()));
                self.no_more_undo();
                if t.continued_by.is_some() {
                    self.follow_ups.push(t.turn_id.clone());
                }
                match &t.cause {
                    Some(c) if c.is_callback() => self.items.push(Item::Callback {
                        turn_id: t.turn_id.clone(),
                        bg_ids: c.callback_ids().to_vec(),
                    }),
                    Some(c) => {
                        if let Some(m) = c.mention() {
                            self.items.push(Item::Called {
                                turn_id: t.turn_id.clone(),
                                agent: t.agent.clone(),
                                message_seq: m.message_seq,
                            });
                        }
                    }
                    None => {}
                }
                if let Some(r) = &t.retry {
                    self.items.push(Item::Retry {
                        turn_id: t.turn_id.clone(),
                        of: r.turn_id.clone(),
                        reason: r.reason,
                        failed_model: r.failed_model.clone(),
                        model: r.model.clone(),
                    });
                }
                // A turn that never ended keeps what it had used.
                if let Some((_, live)) = self.live.take() {
                    self.settled.add(&live);
                    self.show_usage();
                }
                self.running_turn = Some(t.turn_id);
                self.running_agent = t.agent;
            }
            EventKind::UserMessage(m) => {
                self.no_more_undo();
                self.items.push(Item::User {
                    turn_id: m.turn_id,
                    text: m.text,
                    attachments: m.attachments.into_iter().map(|a| a.name).collect(),
                    files: m.files.into_iter().map(|f| f.path).collect(),
                    reply: m.reply,
                    images: m.images,
                    queued: m.queued.then_some(Queued { seq, read: false }),
                    triggers: m.triggers,
                    tags: m.tags,
                });
            }
            EventKind::PeersSkipped(p) => self.items.push(Item::PeersSkipped {
                agents: p.agents,
                reason: p.reason,
            }),
            EventKind::QueuedDelivered(d) => {
                for it in self.items.iter_mut() {
                    if let Item::User {
                        queued: Some(q), ..
                    } = it
                        && d.seqs.contains(&q.seq)
                    {
                        q.read = true;
                    }
                }
            }
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
                // Its thinking, before what the step said (X.25).
                if let Some(t) = &m.thinking {
                    let item = Item::Thinking {
                        message_id: m.message_id.clone(),
                        blocks: t.blocks.iter().map(ThoughtBlock::of).collect(),
                    };
                    let at = self.items.iter().rposition(|i| {
                        matches!(i, Item::Assistant { message_id, .. } if *message_id == m.message_id)
                    });
                    match at {
                        Some(i) => self.items.insert(i, item),
                        None => self.items.push(item),
                    }
                }
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
                staged: None,
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
            // The row says its image was not shown (X.23, T71).
            EventKind::ImagesWithheld(w) => {
                for c in &w.calls {
                    if let Some(Item::Tool {
                        result: Some(r), ..
                    }) = self.tool_mut(c)
                    {
                        r.output.push_str("\n\n");
                        r.output.push_str(crate::event::IMAGE_WITHHELD_ROW);
                    }
                }
            }
            EventKind::ApprovalRequested(a) => {
                self.pending_approvals.push(a.approval_id.clone());
                if let Some(Item::Tool { approval, .. }) = self.tool_mut(&a.call_id) {
                    *approval = Some(ApprovalState {
                        approval_id: a.approval_id,
                        reason: a.reason,
                        turn_allow: a.turn_allow,
                        chat_allow: a.chat_allow,
                        preview: a.preview,
                        decision: None,
                        by: None,
                        note: None,
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
                        a.note = r.note.clone();
                        break;
                    }
                }
            }
            EventKind::Checkpoint(c) => {
                let turn = c.turn_id.clone();
                let st = self.checkpoints.entry(c.turn_id).or_default();
                if !c.excluded.is_empty() {
                    st.excluded.extend(c.excluded);
                    st.excluded.sort();
                    st.excluded.dedup();
                }
                st.excluded_more = st.excluded_more.max(c.excluded_more);
                match c.phase {
                    // A restore's own checkpoint is undone like a turn's.
                    CheckpointPhase::Before => st.before = Some(c.git_ref),
                    CheckpointPhase::BeforeRestore => {
                        st.before = Some(c.git_ref);
                        // The flags as they are just before this restore.
                        let now: BTreeMap<String, String> = self
                            .checkpoints
                            .iter()
                            .filter(|(_, c)| c.restored)
                            .map(|(k, c)| (k.clone(), c.saved.clone().unwrap_or_default()))
                            .collect();
                        self.undone_before.retain(|(k, _)| *k != turn);
                        self.undone_before.push((turn.clone(), now));
                        if self.undone_before.len() > MAX_SAVED {
                            self.undone_before.remove(0);
                        }
                        self.saving = Some(turn);
                    }
                    CheckpointPhase::After => {
                        st.after = Some(c.git_ref);
                        st.files = c.files;
                        st.head_moved = c.head_moved;
                    }
                    CheckpointPhase::Restored => {
                        let back = self
                            .undone_before
                            .iter()
                            .find(|(k, _)| *k == turn)
                            .map(|(_, f)| f.clone());
                        if let Some(flags) = back {
                            // A saved id restored: the folder is as it was
                            // just before the restore that saved it.
                            for (k, c) in self.checkpoints.iter_mut() {
                                match flags.get(k) {
                                    Some(by) => {
                                        c.restored = true;
                                        c.saved = (!by.is_empty()).then(|| by.clone());
                                    }
                                    None => {
                                        c.restored = false;
                                        c.saved = None;
                                    }
                                }
                            }
                        } else {
                            let saving = self.saving.clone();
                            if let Some(c) = self.checkpoints.get_mut(&turn) {
                                c.restored = true;
                                c.saved = saving;
                            }
                        }
                    }
                }
            }
            EventKind::TurnEnded(t) => {
                // X.39: the end's usage REPLACES the running figure; an end
                // without one keeps the last running figure as the turn's.
                let running = match &self.live {
                    Some((id, _)) if *id == t.turn_id => self.live.take().map(|(_, u)| u),
                    _ => None,
                };
                if let Some(u) = &t.usage {
                    self.settled.add(u);
                    self.context_tokens = u.context_tokens;
                    self.context_window = u.window;
                    self.summary_at = u.summary_at;
                    self.context_model = u.model.clone();
                } else if let Some(u) = &running {
                    self.settled.add(u);
                }
                self.show_usage();
                // The turn that ended, also when it reported none (an
                // interrupted one): never an older turn's as the last.
                self.turn_usage = t.usage.as_ref().or(running.as_ref()).map(Usage::capped);
                self.running_turn = None;
                self.running_agent = None;
                self.items.push(Item::TurnEnd {
                    turn_id: t.turn_id,
                    reason: t.reason,
                });
            }
            EventKind::TurnUsage(t) => {
                // X.39: what the running turn has used so far, a running
                // total. Only the running turn's counts, a total below the
                // last is stale, and a late one (its turn ended) changes
                // nothing: the fold trusts no order.
                if self.running_turn.as_deref() != Some(t.turn_id.as_str()) {
                    return;
                }
                let tokens = |u: &Usage| u.input_tokens.saturating_add(u.output_tokens);
                if let Some((id, last)) = &self.live
                    && *id == t.turn_id
                    && tokens(&t.usage) < tokens(last)
                {
                    return;
                }
                let u = t.usage.capped();
                // A summary call's usage has no context size of its own
                // (0): the meter keeps the last one. The turn's limits
                // are applied as `turn_ended` does.
                if u.context_tokens > 0 {
                    self.context_tokens = u.context_tokens;
                }
                self.context_window = u.window;
                self.summary_at = u.summary_at;
                self.context_model = u.model.clone();
                self.turn_usage = Some(u.clone());
                self.live = Some((t.turn_id, u));
                self.show_usage();
            }
            EventKind::Error(e) => self.items.push(Item::Error {
                message: e.message,
                notice: e.notice,
            }),
            EventKind::AgentChanged(a) => {
                self.items.push(Item::AgentChanged {
                    agent: a.agent,
                    model: a.model,
                });
            }
            EventKind::VariantSelected(v) => {
                self.variant_selection.insert(v.group, v.message_id);
            }
            EventKind::HistorySummarized(h) => {
                self.no_more_undo();
                // A person's `/compact`: its cost is the chat's, like a
                // hand-off note's, and the meter shows the shorter context
                // until the next turn reports its own (X.20).
                if let Some(u) = &h.usage {
                    self.settle(u);
                }
                if let Some(n) = h.tokens_after {
                    self.context_tokens = n;
                }
                self.items.push(Item::Summary {
                    through_seq: h.through_seq,
                    text: h.text,
                    manual: h.manual,
                    focus: h.focus,
                })
            }
            EventKind::CompactFailed(c) => {
                if let Some(u) = &c.usage {
                    self.settle(u);
                }
                self.items.push(Item::Error {
                    message: c.reason,
                    notice: false,
                });
            }
            EventKind::ContextShortened(c) => self.items.push(Item::Shortened {
                turn_id: c.turn_id,
                cleared: c.cleared.is_some(),
                steps: c.folded.as_ref().map(|f| f.steps),
                text: c.folded.map(|f| f.text),
                thinking: c.thinking.is_some(),
                tokens_before: c.tokens_before,
                tokens_after: c.tokens_after,
            }),
            EventKind::ModelFallback(f) => self.items.push(Item::Fallback {
                turn_id: f.turn_id,
                model: f.model,
                from: f.from,
                reason: f.reason,
            }),
            EventKind::ModelRetry(r) => self.retry = Some(r),
            EventKind::RetryGivenUp(_) => {}
            EventKind::ArtifactStaged(a) => self.stage(a),
            EventKind::ArtifactStageFailed(f) => self.items.push(Item::ArtifactFailed {
                path: f.path,
                name: f.name,
                error: f.error,
            }),
            EventKind::BackgroundStarted(b) => {
                self.background.insert(
                    b.bg_id,
                    BackgroundState {
                        call_id: b.call_id,
                        ends_by_ms: b.ends_by_ms,
                        timeout_s: b.timeout_s,
                        ended: false,
                        child: b.child,
                    },
                );
            }
            EventKind::ConversationRewound(r) => {
                // The target's own start stands before its message.
                let turns = 1 + self
                    .turn_starts
                    .iter()
                    .filter(|(s, t)| r.message_seq < *s && *s < seq && *t != r.turn_id)
                    .map(|(_, t)| t)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len() as u32;
                self.ranges.add(seq, r.message_seq, &r.turn_id);
                self.rewinds.push(RewindInfo {
                    seq,
                    message_seq: r.message_seq,
                    turn_id: r.turn_id,
                    turns,
                    files: r.files,
                    undone: false,
                    can_undo: true,
                });
                self.items.remark(&self.ranges);
            }
            EventKind::ConversationRewindUndone(u) => {
                if let Some(r) = self.rewinds.iter_mut().find(|r| r.seq == u.rewind_seq) {
                    r.undone = true;
                    r.can_undo = false;
                }
                self.ranges.remove(u.rewind_seq);
                self.items.remark(&self.ranges);
            }
            EventKind::TodoUpdated(t) => self.todo = t.items,
            // What the model was sent with the turn's message; the chat
            // never showed it (X.1).
            EventKind::ContextSent(_) => {}
            EventKind::ThinkingWithheld(_) => {}
            EventKind::ChatAllowAdded(g) => {
                self.chat_allows.insert(
                    g.grant_id,
                    ChatAllow {
                        tool: g.tool,
                        input: g.input,
                    },
                );
            }
            EventKind::ChatAllowRemoved(g) => {
                self.chat_allows.remove(&g.grant_id);
            }
            EventKind::ApprovalModeChanged(c) => {
                if let Some(m) = &mut self.meta {
                    m.approval_mode = c.mode;
                }
                self.items.push(Item::ApprovalModeChanged {
                    mode: c.mode,
                    requested: c.requested,
                });
            }
            EventKind::StepLimitChanged(c) => {
                self.max_steps = Some(c.max_steps);
                self.items.push(Item::StepLimitChanged {
                    max_steps: c.max_steps,
                    requested: c.requested,
                });
            }
            EventKind::HandoffWritten(h) => {
                // Its cost is the chat's, like a turn's (only the sums: the
                // context size stays the last turn's).
                if let Some(u) = &h.usage {
                    self.settle(u);
                }
                self.items.push(Item::Handoff {
                    through_seq: h.through_seq,
                    text: h.text,
                    model: h.model,
                    failed: h.failed,
                });
            }
            EventKind::CallbackRefused(r) => self.items.push(Item::CallbackRefused {
                bg_ids: r.bg_ids,
                reason: r.reason,
            }),
            EventKind::BackgroundEnded(b) => {
                if let Some(st) = self.background.get_mut(&b.bg_id) {
                    st.ended = true;
                }
                let (tail, agent) = match b.child {
                    Some(c) => {
                        // Its cost is the chat's, as a foreground
                        // sub-agent's is its parent turn's.
                        if let Some(u) = &c.usage {
                            self.settle(u);
                        }
                        (c.result, Some(c.agent_id))
                    }
                    None => (b.tail, None),
                };
                self.items.push(Item::BackgroundEnded {
                    bg_id: b.bg_id,
                    reason: b.reason,
                    exit_code: b.exit_code,
                    ran_ms: b.ran_ms,
                    tail,
                    agent,
                });
            }
        }
    }

    /// Background commands the log shows started and not ended.
    pub fn running_background(&self) -> Vec<String> {
        self.background
            .iter()
            .filter(|(_, s)| !s.ended)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// An agent's staged file joins its `publish` card; the user's (or one
    /// whose call is not in the log) stands alone.
    fn stage(&mut self, a: Box<ArtifactStaged>) {
        let card = match (&a.call_id, a.by) {
            (Some(c), crate::artifacts::StagedBy::Agent) => self.tool_mut(c),
            _ => None,
        };
        match card {
            Some(Item::Tool { staged, .. }) => *staged = Some(a),
            _ => self.items.push(Item::Artifact { staged: a }),
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

    /// X.42: the `after` checkpoint's `head_moved` is the turn's; a turn
    /// whose HEAD did not move has none; a restore's events never add one.
    #[test]
    fn a_turns_head_move_is_kept_per_turn() {
        use crate::event::{Checkpoint, CheckpointPhase, HeadAt, HeadMoved};
        let sid = SessionId::new();
        let cp = |turn: &str, phase, moved| {
            EventKind::Checkpoint(Checkpoint {
                turn_id: turn.into(),
                phase,
                git_ref: "r".into(),
                files: vec![],
                excluded: vec![],
                excluded_more: 0,
                head_moved: moved,
            })
        };
        let moved = HeadMoved {
            from: HeadAt {
                branch: Some("main".into()),
                commit: "a".repeat(40),
            },
            to: HeadAt {
                branch: None,
                commit: "b".repeat(40),
            },
        };
        let evs = vec![
            ev(sid, 1, cp("t1", CheckpointPhase::Before, None)),
            ev(
                sid,
                2,
                cp("t1", CheckpointPhase::After, Some(moved.clone())),
            ),
            ev(sid, 3, cp("t2", CheckpointPhase::Before, None)),
            ev(sid, 4, cp("t2", CheckpointPhase::After, None)),
            ev(sid, 5, cp("t1", CheckpointPhase::Restored, None)),
        ];
        let v = SessionView::fold(&evs).unwrap();
        assert_eq!(v.checkpoints["t1"].head_moved, Some(moved));
        assert_eq!(v.checkpoints["t2"].head_moved, None);
    }

    /// X.42: an undo's `restored` turns remember the checkpoint the folder
    /// was saved as first; restoring that one undoes the undo, and the turns
    /// are no longer undone. A restore that failed after saving leaves its
    /// id unused: the next undo's own is the one kept.
    #[test]
    fn an_undo_remembers_what_it_saved_and_a_put_back_clears_it() {
        use crate::event::{Checkpoint, CheckpointPhase as P};
        let sid = SessionId::new();
        let cp = |turn: &str, phase| {
            EventKind::Checkpoint(Checkpoint {
                turn_id: turn.into(),
                phase,
                git_ref: format!("{turn}-{phase:?}"),
                files: vec!["a".into()],
                excluded: vec![],
                excluded_more: 0,
                head_moved: None,
            })
        };
        let mut evs = vec![
            cp("t1", P::Before),
            cp("t1", P::After),
            cp("t2", P::Before),
            cp("t2", P::After),
            // A restore that failed after saving.
            cp("restore-dead", P::BeforeRestore),
            // A rewind of both turns.
            cp("restore-a", P::BeforeRestore),
            cp("restore-a", P::After),
            cp("t1", P::Restored),
            cp("t2", P::Restored),
        ];
        let fold = |evs: &[EventKind]| {
            let raw: Vec<RawEvent> = evs
                .iter()
                .enumerate()
                .map(|(i, k)| ev(sid, i as u64 + 1, k.clone()))
                .collect();
            SessionView::fold(&raw).unwrap()
        };
        let v = fold(&evs);
        for t in ["t1", "t2"] {
            assert!(v.checkpoints[t].restored, "{t}");
            assert_eq!(v.checkpoints[t].saved.as_deref(), Some("restore-a"), "{t}");
        }
        // Putting it back: its own save, its after, and `restored` on it.
        evs.extend([
            cp("restore-b", P::BeforeRestore),
            cp("restore-b", P::After),
            cp("restore-a", P::Restored),
        ]);
        let v = fold(&evs);
        for t in ["t1", "t2"] {
            assert!(!v.checkpoints[t].restored, "{t}");
            assert_eq!(v.checkpoints[t].saved, None, "{t}");
        }
        // A saved id's checkpoint keeps its own before and after.
        let a = &v.checkpoints["restore-a"];
        assert_eq!(a.before.as_deref(), Some("restore-a-BeforeRestore"));
        assert_eq!(a.after.as_deref(), Some("restore-a-After"));
    }

    /// T50: the latest `approval_mode_changed` is the chat's mode, and the
    /// chat shows where it changed.
    #[test]
    fn the_latest_mode_change_is_the_chats_mode() {
        use crate::policy::ApprovalMode;
        let sid = SessionId::new();
        let created = SessionCreated {
            title: "t".into(),
            cwd: "/w".into(),
            model: "m".into(),
            backend: "native".into(),
            node_id: "n".into(),
            approval_mode: ApprovalMode::Ask,
            profile: Profile::Agentic,
            agent_id: None,
            incognito: false,
            parent_session: None,
            no_callbacks: false,
            exec_mode: None,
        };
        let change = |mode, requested| {
            EventKind::ApprovalModeChanged(ApprovalModeChanged { mode, requested })
        };
        let evs = vec![
            ev(sid, 1, EventKind::SessionCreated(created)),
            ev(sid, 2, change(ApprovalMode::Auto, None)),
            ev(
                sid,
                3,
                change(ApprovalMode::AcceptEdits, Some(ApprovalMode::Auto)),
            ),
        ];
        let v = SessionView::fold(&evs).unwrap();
        assert_eq!(v.meta.unwrap().approval_mode, ApprovalMode::AcceptEdits);
        assert_eq!(
            v.items,
            [
                Item::ApprovalModeChanged {
                    mode: ApprovalMode::Auto,
                    requested: None
                },
                Item::ApprovalModeChanged {
                    mode: ApprovalMode::AcceptEdits,
                    requested: Some(ApprovalMode::Auto)
                }
            ]
        );
    }

    #[test]
    fn a_notice_keeps_its_flag_through_the_fold_and_a_failure_does_not_get_one() {
        let sid = SessionId::new();
        let err = |seq, notice| {
            ev(
                sid,
                seq,
                EventKind::Error(ErrorEvent {
                    turn_id: None,
                    message: "m".into(),
                    notice,
                }),
            )
        };
        let evs = vec![err(1, true), err(2, false)];
        let v = SessionView::fold(&evs).unwrap();
        let json = serde_json::to_value(&v.items).unwrap();
        assert_eq!(
            json[0],
            serde_json::json!({"type": "error", "message": "m", "notice": true})
        );
        assert_eq!(
            json[1],
            serde_json::json!({"type": "error", "message": "m"})
        );
    }

    #[test]
    fn folds_a_turn() {
        let sid = SessionId::new();
        let evs = vec![
            ev(
                sid,
                1,
                EventKind::TurnStarted(TurnStarted {
                    cause: None,
                    agent: None,
                    turn_id: "t".into(),
                    environment: None,
                    continued_by: None,
                    retry: None,
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
                    turn_allow: false,
                    chat_allow: false,
                    preview: None,
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
                    image: None,
                    seen: None,
                    call_id: "c".into(),
                    ok: true,
                    output: "[S1] A".into(),
                    exit_code: None,
                    truncated: false,
                    sources: vec![source.clone()],
                    note: None,
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
                thinking: None,
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
                images: vec![],
                queued: false,
                triggers: vec![],
                tags: vec![],
                files: vec![],
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
    fn a_retry_shows_until_output_resumes_and_the_turn_keeps_its_limits() {
        let sid = SessionId::new();
        let retry = ModelRetry {
            turn_id: "t".into(),
            model: "p::m".into(),
            attempt: 1,
            of: 3,
            at_ms: 1_700_000_002_000,
            wait_ms: 2000,
            reason: RetryReason::RateLimit,
        };
        let mut v = SessionView::default();
        let kinds = [
            EventKind::TurnStarted(TurnStarted {
                cause: None,
                agent: None,
                turn_id: "t".into(),
                environment: None,
                continued_by: None,
                retry: None,
            }),
            EventKind::ModelRetry(retry.clone()),
        ];
        for (i, k) in kinds.into_iter().enumerate() {
            v.apply(&ev(sid, i as u64 + 1, k)).unwrap();
        }
        assert_eq!(v.retry, Some(retry));
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json["retry"]["reason"], "rate_limit");
        v.apply(&ev(
            sid,
            3,
            EventKind::AssistantDelta(AssistantDelta {
                author: None,
                variant: None,
                turn_id: "t".into(),
                message_id: "m".into(),
                text: "hi".into(),
            }),
        ))
        .unwrap();
        assert_eq!(v.retry, None, "output resumed");
        v.apply(&ev(
            sid,
            4,
            EventKind::TurnEnded(TurnEnded {
                turn_id: "t".into(),
                reason: TurnEndReason::Completed,
                usage: Some(Usage {
                    input_tokens: 10,
                    output_tokens: 2,
                    context_tokens: 9,
                    window: Some(128_000),
                    summary_at: Some(102_400),
                    ..Default::default()
                }),
            }),
        ))
        .unwrap();
        assert_eq!(
            (v.context_tokens, v.context_window, v.summary_at),
            (9, Some(128_000), Some(102_400))
        );
        // The running total never carries a turn's limits.
        assert_eq!((v.usage.window, v.usage.summary_at), (None, None));
        let json = serde_json::to_value(&v).unwrap();
        assert!(json.get("retry").is_none());
        assert!(json["usage"].get("window").is_none());
    }

    #[test]
    fn rejects_gaps_and_duplicates() {
        let sid = SessionId::new();
        let e1 = ev(
            sid,
            1,
            EventKind::TurnStarted(TurnStarted {
                cause: None,
                agent: None,
                turn_id: "t".into(),
                environment: None,
                continued_by: None,
                retry: None,
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
