//! A turn's changes over the node's link (X.12, PLAN.md T53): answered by
//! one `session.diff_result`, read from the turn's checkpoints, with no
//! event appended; and the node's words when there is nothing to show.

#[path = "support/rig.rs"]
mod support;

use harness_protocol::{DiffStatus, TurnDiff};

fn names(d: &TurnDiff) -> Vec<&str> {
    let mut v: Vec<&str> = d.files.iter().map(|f| f.path.as_str()).collect();
    v.sort();
    v
}

/// A shell command's changes are a diff like any other, and reading it
/// writes nothing to the chat's log.
#[tokio::test]
async fn a_turns_diff_is_answered_and_logs_nothing() {
    let mut rig = support::Rig::new().await;
    assert!(rig.caps.iter().any(|c| c == harness_protocol::CAP_DIFF));
    rig.write("keep.txt", "v1\n");
    let t = rig.turn("echo v2 > keep.txt; echo new > made.txt").await;
    let turn = t.id.expect("the turn has a checkpoint");
    let seen = rig.seen.len();
    let d = rig.diff(&turn, false).await.unwrap();
    assert_eq!(names(&d), ["keep.txt", "made.txt"]);
    let keep = d.files.iter().find(|f| f.path == "keep.txt").unwrap();
    assert_eq!(keep.status, DiffStatus::Modified);
    let lines: Vec<&str> = keep.hunks[0].lines.iter().map(String::as_str).collect();
    assert_eq!(lines, ["-v1", "+v2"]);
    assert_eq!(rig.seen.len(), seen, "a diff appends no event");
    // The undo is untouched by having looked.
    assert!(rig.restore(&turn, false).await.result.is_ok());
    assert_eq!(rig.read("keep.txt").as_deref(), Some("v1\n"));
}

/// Rewind's range: the turn's `before` against the latest state.
#[tokio::test]
async fn rewind_covers_the_later_turns() {
    let mut rig = support::Rig::new().await;
    let first = rig.turn("echo 1 > a.txt").await.id.unwrap();
    rig.turn("echo 2 > b.txt").await;
    assert_eq!(names(&rig.diff(&first, false).await.unwrap()), ["a.txt"]);
    assert_eq!(
        names(&rig.diff(&first, true).await.unwrap()),
        ["a.txt", "b.txt"]
    );
}

/// A turn the log does not know, and a pruned checkpoint, are sentences.
#[tokio::test]
async fn unknown_and_pruned_turns_are_said_plainly() {
    let mut rig = support::Rig::with_keep(2).await;
    let first = rig.turn("echo 1 > a.txt").await.id.unwrap();
    rig.turn("echo 2 > a.txt").await;
    rig.turn("echo 3 > a.txt").await;
    let e = rig.diff(&first, false).await.unwrap_err();
    assert!(e.contains("no longer kept"), "{e}");
    let e = rig.diff("no-such-turn", false).await.unwrap_err();
    assert!(e.contains("has no checkpoint"), "{e}");
}
