//! A person's rewind of the conversation to an earlier message of theirs,
//! and putting it back (ADOPTION_PLAN X.46, docs/rewind.md, README
//! "Rewind"). The node decides, against its own log, under the turn slot:
//! the rule is `harness_core::rewind::check` (shared with the master, which
//! answers the click from its mirror first). Nothing is deleted: the rewind
//! is one event, `conversation_rewound`, that the folds honour, written by
//! this session's one writer; the model is sent the chat without the
//! rewound range from the next turn on (the file tools' seen table is
//! rebuilt from that history at each turn's start, so a file read in a
//! rewound turn is not read any more).
//!
//! With `files` the folder is first put back to before the earliest rewound
//! turn that has a checkpoint, exactly as the button "Rewind files to
//! before this turn" does ([`Session::restore_checked`], `rewind = true`);
//! if that is refused, nothing is written. An undo never touches files.
//! Nothing here can be started by a model, a tool or a routine: only
//! `session.rewind` and `session.rewind_undo`, which the master sends for
//! the chat's owner.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use harness_core::RawEvent;
use harness_core::event::{
    ConversationRewindUndone, ConversationRewound, EventKind, TurnEndReason,
};
use harness_core::new_id;
use harness_core::rewind;
use harness_core::view::SessionView;
use harness_protocol::REWIND_REFUSED;
use tokio_util::sync::CancellationToken;

use super::{RunningTurn, Session};

/// What the operation holds from the claim of the turn slot to its end:
/// the slot is released on every path, also on a panic's unwind.
pub(super) struct Slot<'a> {
    session: &'a Session,
    turn_id: String,
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        self.session.release(&self.turn_id);
    }
}

impl Session {
    /// Take the chat back to the person's message at `message_seq`
    /// (`session.rewind`). Refused while a turn runs or is being stopped, a
    /// `/compact` runs, or any background command or sub-agent of this chat
    /// runs or has an end not recorded yet; in a sub-agent's chat; on a
    /// stopping node; and for any message `rewind::check` refuses. Acked
    /// once `conversation_rewound` is written.
    pub(crate) async fn rewind(
        self: &Arc<Self>,
        message_seq: u64,
        files: bool,
        force: bool,
    ) -> Result<(), String> {
        if self.parent_session.is_some() {
            return Err(format!(
                "{REWIND_REFUSED}: a sub-agent's chat is rewound by the agent that delegated it"
            ));
        }
        let queue = self.queue.lock().await;
        let slot = self.take_slot()?;
        if self.node.is_stopping() {
            return Err(harness_protocol::NODE_STOPPING.into());
        }
        if self.peers_pending() {
            return Err(format!("{REWIND_REFUSED}: {}", super::peers::PEERS_PENDING));
        }
        self.refuse_background_work()?;
        // A failed turn whose end could not be written is closed first.
        if self.unclosed.load(Ordering::SeqCst) {
            let message = "The session log could not be written, so this turn was stopped.";
            self.close_open_turn(message, TurnEndReason::Error).await?;
            self.unclosed.store(false, Ordering::SeqCst);
        }
        // As for a turn (S3-4): no background end lands between the read
        // and the check; nothing of this chat runs while the files go back.
        let (history, target) = {
            let _gate = self.bg_gate.lock().await;
            let history = self.read_log().await?;
            let target = rewind::check(&history, message_seq)
                .map_err(|e| format!("{REWIND_REFUSED}: {e}"))?;
            (history, target)
        };
        let mut files_done = false;
        if files {
            let view = SessionView::fold(&history).map_err(|e| e.to_string())?;
            if let Some(turn) = files_turn(&history, &view, &target) {
                // The folder is the chat's own and nothing of it runs:
                // `restore_checked`, which holds the folder's claim.
                self.restore_checked(&turn, force, true).await?;
                files_done = true;
            }
        }
        {
            let _gate = self.bg_gate.lock().await;
            self.append(EventKind::ConversationRewound(ConversationRewound {
                message_seq: target.message_seq,
                turn_id: target.turn_id,
                files: files_done,
            }))
            .await
            .map_err(|e| e.to_string())?;
        }
        self.reload_after_rewind().await;
        drop(slot);
        drop(queue);
        Ok(())
    }

    /// Put back what the rewind at `rewind_seq` took out
    /// (`session.rewind_undo`): only before a message, a turn or a summary
    /// came after it (`rewind::check_undo`), under the same slot and
    /// refusals as the rewind. Files are never touched.
    pub(crate) async fn rewind_undo(self: &Arc<Self>, rewind_seq: u64) -> Result<(), String> {
        if self.parent_session.is_some() {
            return Err(format!(
                "{REWIND_REFUSED}: a sub-agent's chat is rewound by the agent that delegated it"
            ));
        }
        let queue = self.queue.lock().await;
        let slot = self.take_slot()?;
        let gate = self.bg_gate.lock().await;
        let history = self.read_log().await?;
        rewind::check_undo(&history, rewind_seq).map_err(|e| format!("{REWIND_REFUSED}: {e}"))?;
        let wrote = self
            .append(EventKind::ConversationRewindUndone(
                ConversationRewindUndone { rewind_seq },
            ))
            .await
            .map_err(|e| e.to_string());
        drop(gate);
        wrote?;
        self.reload_after_rewind().await;
        drop(slot);
        drop(queue);
        Ok(())
    }

    /// Claim the turn slot for an operation of this kind: no turn runs,
    /// none is being stopped, no `/compact` runs. Held until the returned
    /// guard drops.
    pub(super) fn take_slot(&self) -> Result<Slot<'_>, String> {
        let turn_id = new_id();
        self.claim_turn(RunningTurn {
            turn_id: turn_id.clone(),
            cancel: CancellationToken::new(),
            savings: Default::default(),
            context: None,
            standing: None,
            publishes: Arc::default(),
            by_stop: Arc::new(AtomicBool::new(false)),
            agent_id: None,
            callback: false,
            compare: false,
            began: std::time::SystemTime::now(),
            delegated_by: None,
        })?;
        Ok(Slot {
            session: self,
            turn_id,
        })
    }

    /// Refuse while this chat has background work: a command or a
    /// sub-agent running, or an end that came and is not recorded yet (it
    /// would land after the rewind for something a rewound turn started).
    /// `restore` looks at commands only; a sub-agent's work writes into the
    /// folder as well.
    fn refuse_background_work(&self) -> Result<(), String> {
        match self.background_work() {
            None => Ok(()),
            Some(what) => Err(format!(
                "{REWIND_REFUSED}: this chat's background work is running or its end is not \
                 recorded yet ({what}). Stop it, or wait for it to end, then rewind"
            )),
        }
    }

    /// What of this chat's background work is running or has an end not
    /// recorded yet, in words; `None` when nothing is. Also read by the
    /// switch of the execution mode (`session_exec.rs`).
    pub(super) fn background_work(&self) -> Option<String> {
        let mut running: Vec<String> = Vec::new();
        if let Some(bg) = &self.bg {
            running.extend(bg.running());
            running.extend(bg.running_agents());
            if running.is_empty() && bg.active() {
                running.push("one that is starting".into());
            }
        }
        let unrecorded = !self
            .bg_queue
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_empty()
            || !self
                .bg_children
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_empty();
        if running.is_empty() && !unrecorded {
            return None;
        }
        Some(if running.is_empty() {
            "an end is waiting".to_string()
        } else {
            running.join(", ")
        })
    }

    pub(super) async fn read_log(&self) -> Result<Vec<RawEvent>, String> {
        let id = self.id;
        self.node
            .cfg
            .store
            .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
            .await
            .map_err(|e| e.to_string())
    }

    /// The log cut to what [`split_rewound_ends`] reads: the kinds of the
    /// rewind rule ([`rewind::KINDS`]) and `background_started`, filtered
    /// in SQL, so no tool output and no delta is read or parsed.
    pub(super) async fn read_rewind_kinds(&self) -> Result<Vec<RawEvent>, String> {
        let id = self.id;
        self.node
            .cfg
            .store
            .call(move |s| {
                let mut kinds: Vec<&str> = rewind::KINDS.to_vec();
                kinds.push("background_started");
                s.events_since_turn_at(id, i64::MIN, &kinds)
            })
            .await
            .map_err(|e| e.to_string())
    }

    /// What the log says now that a rewind or its undo is in it: the
    /// messages sent during an earlier turn and not read yet are the
    /// visible ones'. (The file tools' table and the model's history are
    /// rebuilt from the visible log at each turn's start, never patched.)
    async fn reload_after_rewind(&self) {
        let Ok(history) = self.read_log().await else {
            return;
        };
        *self.pending.lock().unwrap_or_else(|p| p.into_inner()) = unread_visible(&history);
        // An end that was in the channel for a command a rewound turn
        // started is recorded now, not delivered: the model is not handed
        // its output, whichever turn comes next.
        let _gate = self.bg_gate.lock().await;
        let queued = std::mem::take(&mut *self.bg_queue.lock().unwrap_or_else(|p| p.into_inner()));
        let (kept, gone) = split_rewound_ends(&history, queued);
        self.bg_queue
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .extend(kept);
        self.record_background(gone).await;
        *self
            .queued_context
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = None;
    }
}

/// The turn a rewind with `files` puts the folder back to before: the
/// earliest turn of the range (the message's own, then every turn that
/// started after it) that has a checkpoint and is not undone already. None:
/// no turn of the range changed files, or its changes are undone.
fn files_turn(history: &[RawEvent], view: &SessionView, target: &rewind::Target) -> Option<String> {
    let mut turns: Vec<String> = vec![target.turn_id.clone()];
    for e in history.iter().filter(|e| e.seq > target.message_seq) {
        if let Ok(EventKind::TurnStarted(t)) = e.typed()
            && !turns.contains(&t.turn_id)
        {
            turns.push(t.turn_id);
        }
    }
    turns.into_iter().find(|t| {
        view.checkpoints
            .get(t)
            .is_some_and(|c| c.before.is_some() && !c.restored)
    })
}

/// `ends`, the ends of background commands that came and are not recorded
/// yet, as (kept, rewound): the rewound ones are of a command whose
/// `background_started` a rewind of `history` holds. Their output belongs
/// to a turn the model no longer has, so the next turn must not hand it
/// over (the end is still recorded, as not delivered).
pub(crate) fn split_rewound_ends(
    history: &[RawEvent],
    ends: Vec<harness_tools::background::Ended>,
) -> (
    Vec<harness_tools::background::Ended>,
    Vec<harness_tools::background::Ended>,
) {
    let ranges = rewind::rewound(history);
    if ranges.is_empty() {
        return (ends, Vec::new());
    }
    let gone: std::collections::HashSet<&str> = history
        .iter()
        .filter(|e| e.kind == "background_started" && ranges.holds(e.seq))
        .filter_map(|e| e.payload.get("bg_id")?.as_str())
        .collect();
    ends.into_iter()
        .partition(|e| !gone.contains(e.bg_id.as_str()))
}

/// The messages sent during a turn that nothing has read, as the chat
/// stands after its rewinds: one sent inside a rewound range is gone with it
/// and is never read by a later turn.
pub(crate) fn unread_visible(history: &[RawEvent]) -> Vec<(u64, harness_core::event::UserMessage)> {
    super::unread(&rewind::visible(history))
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::SessionId;

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

    fn said(turn: &str, text: &str, queued: bool) -> EventKind {
        let mut m: harness_core::event::UserMessage =
            serde_json::from_value(serde_json::json!({"turn_id": turn, "text": text})).unwrap();
        m.queued = queued;
        EventKind::UserMessage(m)
    }

    fn started(turn: &str) -> EventKind {
        EventKind::TurnStarted(harness_core::event::TurnStarted {
            turn_id: turn.into(),
            agent: None,
            cause: None,
            environment: None,
            retry: None,
            continued_by: None,
        })
    }

    fn ended(bg_id: &str) -> harness_tools::background::Ended {
        harness_tools::background::Ended {
            bg_id: bg_id.into(),
            reason: harness_core::event::BackgroundReason::Exited,
            exit_code: Some(0),
            signal: None,
            confirmed: harness_core::event::BackgroundConfirmed::Ended,
            ran_ms: 5,
            output_bytes: 9,
            tail: "SECRET-OF-A-REWOUND-TURN".into(),
            tail_cut: false,
        }
    }

    fn bg_started(turn: &str, bg: &str) -> EventKind {
        EventKind::BackgroundStarted(harness_core::event::BackgroundStarted {
            bg_id: bg.into(),
            call_id: "c".into(),
            turn_id: turn.into(),
            timeout_s: 60,
            ends_by_ms: 0,
            child: None,
        })
    }

    /// A command a rewound turn started exited before the rewind and its
    /// end was still in the channel: the next turn must not be handed its
    /// output. The end of a command the visible turns started goes on being
    /// handed over, and with no rewind nothing is split. (A unit test on
    /// the selection: the window between the exit and the node's reading
    /// of the channel cannot be timed from outside.)
    #[test]
    fn the_end_of_a_rewound_turns_command_is_not_handed_to_the_model() {
        let mut kinds = vec![
            started("t1"),
            said("t1", "one", false),
            bg_started("t1", "bg-1"),
            started("t2"),
            said("t2", "two", false),
            bg_started("t2", "bg-2"),
        ];
        let plain = log(kinds.clone());
        let (kept, gone) = split_rewound_ends(&plain, vec![ended("bg-1"), ended("bg-2")]);
        assert_eq!((kept.len(), gone.len()), (2, 0), "no rewind");
        kinds.push(EventKind::ConversationRewound(ConversationRewound {
            message_seq: 5,
            turn_id: "t2".into(),
            files: false,
        }));
        let l = log(kinds.clone());
        let (kept, gone) = split_rewound_ends(&l, vec![ended("bg-1"), ended("bg-2")]);
        assert_eq!(
            kept.iter().map(|e| e.bg_id.as_str()).collect::<Vec<_>>(),
            ["bg-1"]
        );
        assert_eq!(
            gone.iter().map(|e| e.bg_id.as_str()).collect::<Vec<_>>(),
            ["bg-2"]
        );
        // Put back: both are the model's again.
        kinds.push(EventKind::ConversationRewindUndone(
            ConversationRewindUndone { rewind_seq: 7 },
        ));
        let (kept, gone) = split_rewound_ends(&log(kinds), vec![ended("bg-1"), ended("bg-2")]);
        assert_eq!((kept.len(), gone.len()), (2, 0));
    }

    /// A message the person sent while a turn ran, which the turn never
    /// read, is rewound with the turn: the next turn must not read it, after
    /// a rewind, and after the node restarted (`Session::load`).
    #[test]
    fn an_unread_queued_message_of_a_rewound_turn_is_not_read_later() {
        let mut kinds = vec![
            started("t1"),
            said("t1", "one", false),
            started("t2"),
            said("t2", "two", false),
            said("t2", "sent meanwhile", true),
        ];
        let l = log(kinds.clone());
        assert_eq!(super::super::unread(&l).len(), 1);
        assert_eq!(unread_visible(&l).len(), 1, "no rewind yet");
        kinds.push(EventKind::ConversationRewound(ConversationRewound {
            message_seq: 4,
            turn_id: "t2".into(),
            files: false,
        }));
        let l = log(kinds.clone());
        assert_eq!(
            super::super::unread(&l).len(),
            1,
            "the raw log still has it"
        );
        assert!(unread_visible(&l).is_empty());
        // Put back: it waits again.
        kinds.push(EventKind::ConversationRewindUndone(
            ConversationRewindUndone { rewind_seq: 6 },
        ));
        assert_eq!(unread_visible(&log(kinds)).len(), 1);
    }
}
