//! A turn's changes for the person (X.12, PLAN.md T53): which checkpoints
//! to compare, from the session's own `checkpoint` events, and the words
//! when there is nothing to show. The diff itself is `git::turn_diff`.
//! Nothing is appended to the log: a diff is a view, not an event.

use harness_core::event::{Checkpoint, CheckpointPhase, EventKind};
use harness_protocol::TurnDiff;

use super::Session;

/// What the checkpoint events say of one turn, as the session's fold reads
/// them (`SessionView`): its `before`, its `after`, and the latest
/// recorded state of the folder (the last checkpoint that is not a
/// `before`, what a rewind compares with, as `Session::restore`).
#[derive(Debug, Default, PartialEq, Eq)]
struct Range {
    known: bool,
    before: Option<String>,
    after: Option<String>,
    latest: Option<String>,
}

fn range_of(cps: impl Iterator<Item = Checkpoint>, turn_id: &str) -> Range {
    let mut r = Range::default();
    for c in cps {
        let before = matches!(
            c.phase,
            CheckpointPhase::Before | CheckpointPhase::BeforeRestore
        );
        if !before {
            r.latest = Some(c.git_ref.clone());
        }
        if c.turn_id != turn_id {
            continue;
        }
        r.known = true;
        match c.phase {
            CheckpointPhase::Before | CheckpointPhase::BeforeRestore => r.before = Some(c.git_ref),
            CheckpointPhase::After => r.after = Some(c.git_ref),
            CheckpointPhase::Restored => {}
        }
    }
    r
}

impl Session {
    /// What turn `turn_id` changed: its `before` against its `after`, or,
    /// with `rewind`, against the latest state (that turn and every later
    /// one, what "Rewind files to before this turn" undoes). Every error is
    /// scrubbed of the node's secrets, as the diff is.
    pub(crate) async fn diff(&self, turn_id: &str, rewind: bool) -> Result<TurnDiff, String> {
        let secrets = self.node.cfg.secrets.clone();
        self.diff_unscrubbed(turn_id, rewind, &|s: &str| secrets.scrub(s))
            .await
            .map_err(|e| secrets.scrub(&e))
    }

    async fn diff_unscrubbed(
        &self,
        turn_id: &str,
        rewind: bool,
        scrub: &(dyn Fn(&str) -> String + Sync),
    ) -> Result<TurnDiff, String> {
        let id = self.id;
        // Only this session's checkpoint events (review D8): the rest of
        // the log is never read or parsed for a diff.
        let events = self
            .node
            .cfg
            .store
            .call(move |s| s.events_of_kind(id, "checkpoint"))
            .await
            .map_err(|e| e.to_string())?;
        let cps = events.iter().filter_map(|e| match e.typed() {
            Ok(EventKind::Checkpoint(c)) => Some(c),
            _ => None,
        });
        let r = range_of(cps, turn_id);
        if !r.known {
            return Err("this turn has no checkpoint, so its changes cannot be shown".into());
        }
        let before = r.before.ok_or("this turn's checkpoint is incomplete")?;
        let target = if rewind { r.latest } else { r.after }.ok_or(
            "this turn's changes show once it has ended (its checkpoint after the turn is \
             not there yet, or could not be taken)",
        )?;
        let repo = self
            .checkpoint_repo()
            .await?
            .ok_or("checkpoints are not available for this folder any more")?;
        let pruned = || {
            format!(
                "this checkpoint is no longer kept (only the latest {} changes are), so its \
                 changes cannot be shown",
                self.node.cfg.policy.checkpoints_keep
            )
        };
        for sha in [&before, &target] {
            if !crate::git::exists(&repo, sha).await {
                return Err(pruned());
            }
        }
        match crate::git::turn_diff(&repo, &before, &target, scrub).await {
            Ok(d) => Ok(d),
            // Pruned between the check and the read (review D12): the
            // sentence, not git's "bad object".
            Err(_)
                if !crate::git::exists(&repo, &before).await
                    || !crate::git::exists(&repo, &target).await =>
            {
                Err(pruned())
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cp(turn: &str, phase: CheckpointPhase, sha: &str) -> Checkpoint {
        Checkpoint {
            turn_id: turn.into(),
            phase,
            git_ref: sha.into(),
            files: vec![],
            excluded: vec![],
            excluded_more: 0,
        }
    }

    #[test]
    fn the_range_is_the_folds() {
        use CheckpointPhase::*;
        let cps = vec![
            cp("t1", Before, "b1"),
            cp("t1", After, "a1"),
            cp("t2", Before, "b2"),
            cp("t2", After, "a2"),
            cp("r", BeforeRestore, "s"),
            cp("r", After, "ra"),
            cp("t2", Restored, "ra"),
        ];
        let r = range_of(cps.clone().into_iter(), "t1");
        assert_eq!(
            r,
            Range {
                known: true,
                before: Some("b1".into()),
                after: Some("a1".into()),
                latest: Some("ra".into()),
            }
        );
        assert!(!range_of(cps.into_iter(), "nope").known);
    }
}
