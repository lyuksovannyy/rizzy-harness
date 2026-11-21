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
use harness_core::event::{ErrorEvent, EventKind, Profile};
use harness_core::fork;
use harness_protocol::{FORK_REFUSED, ForkCheckout};

use super::Session;
use crate::Inner;

/// A fork's title is cut to this many characters (the master sends the
/// parent's title and a suffix; the log keeps what is written here).
const MAX_TITLE_CHARS: usize = 300;

impl Session {
    /// The folder a fork that shares its parent's works in: the parent's,
    /// checked as at any create (inside `workspace_roots`, not protected, a
    /// directory). A fork with a folder of its own gets it from
    /// [`Self::own_copy`]; `fork` decides between the two.
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
        checkout: ForkCheckout,
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
        // Always checked: the parent's folder and mode are what the fork
        // would share, and what an own copy is made from.
        let shared = self.fork_folder().map_err(refused)?;
        // The copy is made before anything of the chat is written, off the
        // parent's actor (this runs in the new chat's lane, which nothing
        // else uses yet): a failure leaves no chat and no folder.
        let mut copy = match checkout {
            ForkCheckout::Shared => None,
            ForkCheckout::Own => Some(
                self.own_copy(node, new, cut.through_seq)
                    .await
                    .map_err(refused)?,
            ),
        };
        let cwd = copy.as_ref().map_or(shared, |c| c.folder.clone());
        let through = cut.through_seq as usize;
        let events = node
            .cfg
            .store
            .call(move |s| s.events_after(parent, 0, through))
            .await;
        let events = match events {
            Ok(e) => e,
            Err(e) => {
                discard(copy).await;
                return Err(e.to_string());
            }
        };
        let title: String = title.chars().take(MAX_TITLE_CHARS).collect();
        let built = fork::build(
            &events,
            &cut,
            new,
            &title,
            &cwd.display().to_string(),
            harness_core::now_ms(),
        );
        let mut built = match built {
            Ok(b) => b,
            Err(e) => {
                discard(copy).await;
                return Err(refused(e));
            }
        };
        drop(events);
        if copy.is_some() {
            // The creation says the folder is the fork's own (the page's
            // note and the delete dialog read it).
            built[0].payload["forked"]["own_folder"] = true.into();
        }
        // What the person should know about its folder, said in the chat.
        if let Some(c) = &copy {
            for note in &c.notes {
                let (kind, payload) = EventKind::Error(ErrorEvent {
                    turn_id: None,
                    message: note.clone(),
                    notice: true,
                })
                .into_parts();
                let last = built.last().map_or(0, |e| e.seq);
                built.push(RawEvent {
                    session_id: new,
                    seq: last + 1,
                    ts_ms: harness_core::now_ms(),
                    kind,
                    payload,
                });
            }
        }
        // The title is text from outside the log: scrubbed like any event's
        // free text (T3); the copied events were scrubbed when written.
        let kind = built[0].kind.clone();
        node.cfg.secrets.scrub_event(&kind, &mut built[0].payload);
        let owner = node.cfg.node_id.clone();
        let to_write = built.clone();
        let wrote = node
            .cfg
            .store
            .call(move |s| s.write_fork(&owner, parent, &to_write))
            .await;
        if let Err(e) = wrote {
            // No chat: no folder either.
            discard(copy).await;
            return Err(e.to_string());
        }
        // The chat exists: the folder is its own, no start sweeps it.
        if let Some(c) = copy.as_mut() {
            c.created();
        }
        let s = Session::load(node.clone(), new)
            .await
            .map_err(|e| e.to_string())?;
        Ok((s, built))
    }

    /// The fork's own folder (`checkout: "own"`): a clone of the parent's
    /// repository made from outside it, the files written from the cut's
    /// checkpoint (`fork_checkout`, `git::fork_copy`). Refused, with a
    /// sentence that says which and that `"shared"` is possible, for a chat
    /// with no folder, a folder that is no git repository, a node with no
    /// projects folder or an old git, and a cut whose checkpoint was pruned.
    async fn own_copy(
        &self,
        node: &Arc<Inner>,
        new: SessionId,
        through: u64,
    ) -> Result<crate::fork_checkout::Copy, String> {
        let shared = |why: String| format!("{why}; the fork can share this chat's folder instead");
        if self.profile != Profile::Agentic {
            return Err(shared("this chat has no folder to copy".into()));
        }
        let dir = self.checked_folder()?;
        let Some(repo) = self.checkpoint_repo().await.map_err(shared)? else {
            return Err(shared(
                "this node keeps no checkpoint store, which the copy's files are written from"
                    .into(),
            ));
        };
        let parent = self.id;
        let checkpoints = node
            .cfg
            .store
            .call(move |s| s.events_since_turn_at(parent, i64::MIN, &["checkpoint"]))
            .await
            .map_err(|e| e.to_string())?;
        let cut = crate::fork_checkout::cut_state(&checkpoints, through);
        drop(checkpoints);
        if let Some(snap) = &cut.snapshot
            && !crate::git::exists(&repo, snap).await
        {
            return Err(shared(
                "this node no longer keeps the checkpoint of that point (it keeps the latest                  ones), so the copy's files cannot be rebuilt"
                    .into(),
            ));
        }
        // Waits its turn behind other copies; none is refused for another's.
        let _slot = node
            .fork_slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| e.to_string())?;
        let short = crate::fork_checkout::short_id(&new.to_string());
        let req = crate::fork_checkout::Request {
            bounds: self.bounds(&dir),
            cwd: dir,
            shadow: repo,
            cut,
            short,
        };
        let res = crate::fork_checkout::make(
            &node.cfg.policy,
            &node.projects_made,
            req,
            &crate::projects::suffix,
        )
        .await;
        node.audit_log.record(
            "session.fork_copy",
            serde_json::json!({
                "session": new.to_string(),
                "ok": res.is_ok(),
                "folder": res.as_ref().ok().map(|c| c.root.display().to_string()),
                "branch": res.as_ref().ok().map(|c| c.branch.clone()),
                "head_exact": res.as_ref().ok().map(|c| c.head_exact),
                "upstream": res.as_ref().ok().map(|c| c.upstream),
            })
            .to_string(),
        );
        res.map_err(shared)
    }
}

/// Remove a copy whose chat could not be written.
async fn discard(copy: Option<crate::fork_checkout::Copy>) {
    if let Some(c) = copy {
        c.discard().await;
    }
}
