//! A person's fork of a chat at the end of a completed turn (ADOPTION_PLAN
//! C.2, docs/fork.md). The node decides, against its own log and its own
//! policy: the rule is `harness_core::fork::check` (shared with the master,
//! which answers the click from its mirror first). The new chat's log is the
//! parent's events up to the cut, at the same seqs, under a new id, then the
//! divider (`harness_core::fork::build`); it is written, with the image
//! bytes its copied events name, in ONE transaction, so a crash leaves
//! nothing or a whole chat, and the normal resume replays it to a master
//! that missed it. The parent is only read: its running turn, its events
//! after the cut and its folder are not touched, and the copy needs no turn
//! slot of it.
//!
//! Nothing here can be started by a model, a tool or a routine: only
//! `session.fork`, which the master sends for the parent's owner.

use std::path::PathBuf;
use std::sync::Arc;

use harness_core::RawEvent;
use harness_core::SessionId;
use harness_core::event::Profile;
use harness_core::fork;
use harness_protocol::FORK_REFUSED;

use super::Session;
use crate::Inner;

/// A fork's title is cut to this many characters (the master sends the
/// parent's title and a suffix; the log keeps what is written here).
const MAX_TITLE_CHARS: usize = 300;

impl Session {
    /// The folder the fork works in: the parent's, checked as at any
    /// create (inside `workspace_roots`, not protected, a directory). The
    /// one place that decides it; a fork with a checkout of its own would
    /// decide it here (ADOPTION_PLAN C.2, part 2).
    fn fork_folder(&self) -> Result<PathBuf, String> {
        match self.profile {
            // A chat session has no folder and runs no tools.
            Profile::Chat => Ok(self.cwd.clone()),
            Profile::Agentic => {
                let policy = &self.node.cfg.policy;
                // A mode the node no longer offers runs nothing, in a fork
                // as in the chat it came from (never re-chosen).
                policy.exec.choose(self.exec_mode)?;
                policy.check_cwd(&self.cwd.display().to_string())
            }
        }
    }

    /// Fork this chat at the end of `turn_id` into the new chat `new`
    /// (`session.fork`): the registered-ready session and the events to
    /// publish, in order. Nothing is written when anything is refused.
    pub(crate) async fn fork(
        self: &Arc<Self>,
        node: &Arc<Inner>,
        new: SessionId,
        turn_id: &str,
        title: &str,
    ) -> Result<(Arc<Session>, Vec<RawEvent>), String> {
        let refused = |why: String| format!("{FORK_REFUSED}: {why}");
        if node.session(new).is_some() {
            return Err("session already exists".into());
        }
        let parent = self.id;
        let log = node
            .cfg
            .store
            .call(move |s| s.events_since_turn_at(parent, i64::MIN, fork::KINDS))
            .await
            .map_err(|e| e.to_string())?;
        let cut = fork::check(&log, turn_id).map_err(refused)?;
        drop(log);
        let cwd = self.fork_folder().map_err(refused)?;
        let through = cut.through_seq as usize;
        let events = node
            .cfg
            .store
            .call(move |s| s.events_after(parent, 0, through))
            .await
            .map_err(|e| e.to_string())?;
        let title: String = title.chars().take(MAX_TITLE_CHARS).collect();
        let mut built = fork::build(
            &events,
            &cut,
            new,
            &title,
            &cwd.display().to_string(),
            harness_core::now_ms(),
        )
        .map_err(refused)?;
        drop(events);
        // The title is text from outside the log: scrubbed like any event's
        // free text (T3); the copied events were scrubbed when written.
        let kind = built[0].kind.clone();
        node.cfg.secrets.scrub_event(&kind, &mut built[0].payload);
        let owner = node.cfg.node_id.clone();
        let to_write = built.clone();
        node.cfg
            .store
            .call(move |s| s.write_fork(&owner, parent, &to_write))
            .await
            .map_err(|e| e.to_string())?;
        let s = Session::load(node.clone(), new)
            .await
            .map_err(|e| e.to_string())?;
        Ok((s, built))
    }
}
