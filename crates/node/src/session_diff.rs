//! A turn's changes for the person (X.12, PLAN.md T53): which checkpoints
//! to compare, from the session's own log, and the words when there is
//! nothing to show. The diff itself is `git::turn_diff`. Nothing is
//! appended to the log: a diff is a view, not an event.

use harness_core::event::{CheckpointPhase, EventKind};
use harness_core::view::SessionView;
use harness_protocol::TurnDiff;

use super::Session;

/// The latest recorded state of the folder: the last checkpoint that is
/// not a `before` (what a rewind compares with, as `Session::restore`).
fn latest_state(events: &[harness_core::RawEvent]) -> Option<String> {
    events
        .iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::Checkpoint(c))
                if !matches!(
                    c.phase,
                    CheckpointPhase::Before | CheckpointPhase::BeforeRestore
                ) =>
            {
                Some(c.git_ref)
            }
            _ => None,
        })
        .next_back()
}

impl Session {
    /// What turn `turn_id` changed: its `before` against its `after`, or,
    /// with `rewind`, against the latest state (that turn and every later
    /// one, what "Rewind files to before this turn" undoes).
    pub(crate) async fn diff(&self, turn_id: &str, rewind: bool) -> Result<TurnDiff, String> {
        let id = self.id;
        let events = self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
            .await
            .map_err(|e| e.to_string())?;
        let view = SessionView::fold(&events).map_err(|e| e.to_string())?;
        let cp = view
            .checkpoints
            .get(turn_id)
            .ok_or("this turn has no checkpoint, so its changes cannot be shown")?;
        let before = cp
            .before
            .clone()
            .ok_or("this turn's checkpoint is incomplete")?;
        let target = match (rewind, &cp.after) {
            (true, _) => latest_state(&events),
            (false, after) => after.clone(),
        }
        .ok_or(
            "this turn's changes show once it has ended (its checkpoint after the turn is \
             not there yet, or could not be taken)",
        )?;
        let repo = self
            .checkpoint_repo()
            .await?
            .ok_or("checkpoints are not available for this folder any more")?;
        for sha in [&before, &target] {
            if !crate::git::exists(&repo, sha).await {
                return Err(format!(
                    "this checkpoint is no longer kept (only the latest {} changes are), so \
                     its changes cannot be shown",
                    self.node.cfg.policy.checkpoints_keep
                ));
            }
        }
        // The scrubber's rule for events holds for this view too: the
        // node's secrets never leave it, in a path or a line.
        let secrets = self.node.cfg.secrets.clone();
        crate::git::turn_diff(&repo, &before, &target, &|s: &str| secrets.scrub(s)).await
    }
}
