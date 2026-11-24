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

/// The whole chat's range (X.61), as the fold reads the `checkpoint` events
/// in log order: every snapshot of a state before work (a turn's `before`, a
/// restore's `before_restore`), oldest first, and the latest recorded state
/// that is not one. The range runs from the oldest of the first that is
/// still kept to the second. An undo or a rewind is in the log as a restore
/// whose `after` is the latest state, so undone work drops out by itself; a
/// turn still running has a `before` and no `after` yet, so it is left out
/// until it ends. A start is only kept when it is before the end in log
/// order, so the range is never inverted.
fn whole_range(cps: impl Iterator<Item = Checkpoint>) -> (Vec<String>, Option<String>) {
    let (mut starts, mut latest) = (Vec::new(), None);
    let mut end_at = 0;
    for (i, c) in cps.enumerate() {
        match c.phase {
            CheckpointPhase::Before | CheckpointPhase::BeforeRestore => starts.push((i, c.git_ref)),
            _ => {
                latest = Some(c.git_ref);
                end_at = i;
            }
        }
    }
    let starts = starts
        .into_iter()
        .filter(|(i, _)| *i < end_at)
        .map(|(_, r)| r)
        .collect();
    (starts, latest)
}

impl Session {
    /// The chat's folder as a path inside the folder the diff's paths are
    /// relative to (`repo.root`: the repository the checkpoints follow, which
    /// may contain the chat's folder), `/`-separated; empty when they are the
    /// same (or the folder cannot be told: then the paths are as they were).
    fn folder_in(&self, repo: &crate::git::Repo) -> String {
        let canon = |p: &std::path::Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let Ok(dir) = self.checked_folder() else {
            return String::new();
        };
        canon(&dir)
            .strip_prefix(canon(&repo.root))
            .ok()
            .map(|r| {
                r.components()
                    .filter_map(|c| c.as_os_str().to_str())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .unwrap_or_default()
    }

    /// Everything this chat changed in its folder (X.61): the oldest
    /// checkpoint still kept against the latest finished state. Scrubbed as
    /// a turn's diff is.
    pub(crate) async fn diff_all(&self) -> Result<TurnDiff, String> {
        let secrets = self.node.cfg.secrets.clone();
        self.diff_all_unscrubbed(&|s: &str| secrets.scrub(s))
            .await
            .map_err(|e| secrets.scrub(&e))
    }

    async fn diff_all_unscrubbed(
        &self,
        scrub: &(dyn Fn(&str) -> String + Sync),
    ) -> Result<TurnDiff, String> {
        let id = self.id;
        let events = self
            .node
            .cfg
            .store
            .call(move |s| s.events_of_kind(id, "checkpoint"))
            .await
            .map_err(|e| e.to_string())?;
        let (starts, latest) = whole_range(events.iter().filter_map(|e| match e.typed() {
            Ok(EventKind::Checkpoint(c)) => Some(c),
            _ => None,
        }));
        // Nothing recorded and none can be: said, rather than an empty
        // answer that reads as "this chat changed nothing".
        let Some(repo) = self.checkpoint_repo().await? else {
            return Err(
                "checkpoints are not kept for this chat's folder, so its changes \
                        cannot be shown"
                    .into(),
            );
        };
        let (Some(latest), false) = (latest, starts.is_empty()) else {
            // No turn has finished (none yet, or the first is running).
            return Ok(TurnDiff::default());
        };
        let pruned = || {
            format!(
                "this chat's checkpoints are no longer kept (only the latest {} changes are), \
                 so its changes cannot be shown",
                self.node.cfg.policy.checkpoints_keep
            )
        };
        if !crate::git::exists(&repo, &latest).await {
            return Err(pruned());
        }
        // Pruning drops the oldest first, so "kept" only turns from false to
        // true along the list: the first kept one by bisection (a git call
        // per probe, not per checkpoint of a long chat). Whatever is found
        // was checked to exist, so a gap would only move the start later.
        let (mut lo, mut hi) = (0, starts.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            if crate::git::exists(&repo, &starts[mid]).await {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        let Some(start) = starts.get(lo) else {
            return Err(pruned());
        };
        match crate::git::turn_diff(&repo, start, &latest, scrub).await {
            Ok(mut d) => {
                d.since_pruned = lo > 0;
                d.folder = self.folder_in(&repo);
                Ok(d)
            }
            Err(_)
                if !crate::git::exists(&repo, start).await
                    || !crate::git::exists(&repo, &latest).await =>
            {
                Err(pruned())
            }
            Err(e) => Err(e),
        }
    }

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
            Ok(mut d) => {
                // What an undo would refuse over (T57), said before the
                // person clicks: the same check, on the files shown.
                let shown: std::collections::BTreeSet<String> = d
                    .files
                    .iter()
                    .flat_map(|f| std::iter::once(f.path.clone()).chain(f.old_path.clone()))
                    .collect();
                let span = self
                    .diff_span(&events, turn_id, rewind, &before, &target)
                    .await;
                let foreign = self.foreign_work(&repo, &span, &shown).await;
                d.others_wrote = foreign.wrote.iter().cloned().collect();
                d.others_may = foreign.maybe().into_iter().collect();
                d.folder = self.folder_in(&repo);
                Ok(d)
            }
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

impl Session {
    /// The undone span an undo of `turn_id` (or a rewind from it) would
    /// check, as `Session::restore` builds it: from the turn's
    /// `turn_started` to the checkpoint of the state compared with, with
    /// this chat's own `edit`/`write` calls in it. Reads only this chat's
    /// `turn_started` events and its tool calls from that turn on.
    async fn diff_span(
        &self,
        checkpoints: &[harness_core::RawEvent],
        turn_id: &str,
        rewind: bool,
        before: &str,
        target: &str,
    ) -> crate::others::Span {
        let id = self.id;
        let starts = self
            .node
            .cfg
            .store
            .call(move |s| s.events_of_kind(id, "turn_started"))
            .await
            .unwrap_or_default();
        let mut marks = starts;
        marks.extend(checkpoints.iter().cloned());
        marks.sort_by_key(|e| e.seq);
        let since = crate::others::span_start(&marks, turn_id);
        let mut end = i64::MAX;
        for e in checkpoints {
            if let Ok(EventKind::Checkpoint(c)) = e.typed() {
                let state = !matches!(
                    c.phase,
                    CheckpointPhase::Before | CheckpointPhase::BeforeRestore
                );
                if rewind && state {
                    end = e.ts_ms;
                } else if !rewind && c.turn_id == turn_id && c.phase == CheckpointPhase::After {
                    end = e.ts_ms;
                    break;
                }
            }
        }
        let calls = self
            .node
            .cfg
            .store
            .call(move |s| {
                s.events_since_turn_at(id, since, &["tool_call_started", "tool_call_finished"])
            })
            .await
            .unwrap_or_default();
        crate::others::Span {
            since,
            end,
            before: Some(before.to_string()),
            after: Some(target.to_string()),
            own: crate::others::own_writes(&calls, since, end),
            me: Some(self.id),
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
            head_moved: None,
        }
    }

    #[test]
    fn the_whole_range_is_every_before_and_the_latest_state() {
        use CheckpointPhase::*;
        let (starts, latest) = whole_range(
            vec![
                cp("t1", Before, "b1"),
                cp("t1", After, "a1"),
                cp("t2", Before, "b2"),
                cp("r", BeforeRestore, "s"),
                cp("r", After, "ra"),
                cp("t2", Restored, "ra"),
                cp("t3", Before, "b3"),
            ]
            .into_iter(),
        );
        // A running turn (t3) has no after: the latest finished state, and
        // its `before` is after the end, so not a start.
        assert_eq!(starts, ["b1", "b2", "s"]);
        assert_eq!(latest.as_deref(), Some("ra"));
        // Only a start after the end: no range, never an inverted one.
        let (starts, latest) =
            whole_range(vec![cp("t1", After, "a1"), cp("t2", Before, "b2")].into_iter());
        assert!(starts.is_empty());
        assert_eq!(latest.as_deref(), Some("a1"));
        assert_eq!(whole_range(std::iter::empty()), (vec![], None));
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
