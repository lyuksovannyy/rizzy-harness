//! A turn that changed more than `RECOMMENDED_MAX_FILES_PER_TURN` files says
//! so at its end and on its undo (ADOPTION_PLAN T.9, `docs/undo-and-stash.md`
//! 2.6): a note in the chat, never a block. The count is the `after`
//! checkpoint's own list, so nested repositories and ignored files are not in
//! it, and a sub-agent's changes count toward the turn that applied them.

#[path = "support/rig.rs"]
mod support;

use harness_core::policy::RECOMMENDED_MAX_FILES_PER_TURN as MAX;
use support::Rig;

async fn repo_rig() -> Rig {
    let r = Rig::new().await;
    r.git(&["init", "-q"]);
    r.write(".gitignore", "ignored/\n");
    r.write("a.txt", "a\n");
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "init"]);
    r
}

/// A shell command that writes `n` files named `<prefix>1.txt`...
fn files(prefix: &str, n: usize) -> String {
    format!("for i in $(seq 1 {n}); do echo $i > {prefix}$i.txt; done")
}

fn warnings(notes: &[String]) -> Vec<&String> {
    notes
        .iter()
        .filter(|n| n.contains("more than the recommended"))
        .collect()
}

/// A turn of `n` new files, then its undo; returns the notes of each.
async fn turn_and_undo(r: &mut Rig, cmd: &str) -> (Vec<String>, Vec<String>) {
    let mark = r.seen.len();
    let before = r.files();
    let turn = r.turn(cmd).await.id.expect("a checkpoint");
    let at_end = r.notes_since(mark);
    let mark = r.seen.len();
    let undo = r.restore(&turn, false).await;
    assert_eq!(undo.result, Ok(()));
    assert_eq!(r.files(), before, "the undo put the folder back");
    (at_end, r.notes_since(mark))
}

#[tokio::test]
async fn exactly_the_limit_warns_nobody() {
    let mut r = repo_rig().await;
    let (end, undo) = turn_and_undo(&mut r, &files("f", MAX)).await;
    assert!(warnings(&end).is_empty(), "{end:?}");
    assert!(warnings(&undo).is_empty(), "{undo:?}");
}

#[tokio::test]
async fn one_over_the_limit_warns_at_the_end_and_on_the_undo_and_both_complete() {
    let mut r = repo_rig().await;
    let n = MAX + 1;
    let (end, undo) = turn_and_undo(&mut r, &files("f", n)).await;
    let w = warnings(&end);
    assert_eq!(w.len(), 1, "{end:?}");
    assert_eq!(
        w[0].as_str(),
        format!(
            "This turn changed {n} files, more than the recommended {MAX}. \
             Undoing it will put back {n} files."
        )
    );
    let w = warnings(&undo);
    assert_eq!(w.len(), 1, "{undo:?}");
    assert_eq!(
        w[0].as_str(),
        format!("This undo put back {n} files, more than the recommended {MAX}.")
    );
}

/// Both notes are notices, and a failure written by the same session is not.
#[tokio::test]
async fn the_two_notes_are_notices() {
    let mut r = repo_rig().await;
    let n = MAX + 1;
    let mark = r.seen.len();
    let turn = r.turn(&files("f", n)).await.id.expect("a checkpoint");
    let end = r.notices_since(mark);
    assert_eq!(end.len(), 1, "{end:?}");
    assert!(end[0].starts_with("This turn changed"), "{end:?}");
    let mark = r.seen.len();
    assert_eq!(r.restore(&turn, false).await.result, Ok(()));
    let undo = r.notices_since(mark);
    assert_eq!(undo.len(), 1, "{undo:?}");
    assert!(undo[0].starts_with("This undo put back"), "{undo:?}");
}

/// 100 files and one more inside a nested repository, and one more in an
/// ignored folder: neither is counted, so nobody is warned.
#[tokio::test]
async fn nested_repositories_and_ignored_files_are_not_counted() {
    let mut r = repo_rig().await;
    std::fs::create_dir(r.ws.join("sub")).unwrap();
    r.git_in(&r.ws.join("sub"), &["init", "-q"]);
    let cmd = format!(
        "{}; echo x > sub/inner.txt; mkdir ignored; echo y > ignored/z.txt",
        files("f", MAX)
    );
    let mark = r.seen.len();
    let turn = r.turn(&cmd).await.id.expect("a checkpoint");
    let end = r.notes_since(mark);
    assert!(warnings(&end).is_empty(), "{end:?}");
    let mark = r.seen.len();
    assert_eq!(r.restore(&turn, false).await.result, Ok(()));
    assert!(warnings(&r.notes_since(mark)).is_empty());
}

/// Two sub-agents apply 60 and 50 files in one turn: the turn changed 110.
#[tokio::test]
async fn sub_agent_changes_count_toward_the_turn_that_applied_them() {
    let mut r = repo_rig().await;
    let mark = r.seen.len();
    let before = r.files();
    let (turn, outs) = r
        .lead_turn(&format!(
            "delegate: dev=run: {} | dev=run: {}",
            files("a", 60),
            files("b", 50)
        ))
        .await;
    assert_eq!(outs.len(), 2, "{outs:#?}");
    let end = r.notes_since(mark);
    let w = warnings(&end);
    assert_eq!(w.len(), 1, "{end:?}");
    assert!(w[0].contains("This turn changed 110 files"), "{end:?}");
    let mark = r.seen.len();
    let undo = r.restore(&turn.id.expect("a checkpoint"), false).await;
    assert_eq!(undo.result, Ok(()));
    assert_eq!(r.files(), before);
    assert_eq!(warnings(&r.notes_since(mark)).len(), 1);
}
