//! Undo's check of what changed after the turn, through a real node (the
//! `Restore` request a person's Undo sends): an unforced undo refuses over
//! the person's later work above a path it writes, under a name that reads
//! as pathspec magic, and under a name that is not UTF-8 (PLAN.md T33; the
//! unit tests are in `crates/node/src/git_check_tests.rs`).

#[path = "support/rig.rs"]
mod support;

use std::os::unix::ffi::OsStrExt;
use support::Rig;

async fn repo_rig() -> Rig {
    let r = Rig::new().await;
    r.git(&["init", "-q"]);
    r.write("ctl.txt", "ctl\n");
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "init"]);
    r
}

/// The turn deleted the folder `a`; the person then made a file `a`.
/// Undo would write `a/x` over it: refused, the file stays; "Undo anyway"
/// goes ahead and its pre-restore checkpoint holds the file.
#[tokio::test]
async fn undo_refuses_over_a_file_the_person_put_where_a_folder_comes_back() {
    let mut r = repo_rig().await;
    std::fs::create_dir(r.ws.join("a")).unwrap();
    r.write("a/x", "before\n");
    let turn = r.turn("rm -r a").await.id.unwrap();
    r.write("a", "person's file\n");
    let undo = r.restore(&turn, false).await;
    let e = undo.result.unwrap_err();
    assert!(
        e.contains("changed after the turn") && e.contains("`a`"),
        "{e}"
    );
    assert_eq!(r.read("a").as_deref(), Some("person's file\n"));
    let forced = r.restore(&turn, true).await;
    assert_eq!(forced.result, Ok(()));
    assert_eq!(r.read("a/x").as_deref(), Some("before\n"));
    // Undoing the forced undo brings the person's file back.
    let saved = forced.saved.expect("a pre-restore checkpoint");
    assert_eq!(r.restore(&saved, false).await.result, Ok(()));
    assert_eq!(r.read("a").as_deref(), Some("person's file\n"));
}

/// A file whose name is not UTF-8, changed by the turn and then by the
/// person: refused, the person's version stays.
#[tokio::test]
async fn undo_refuses_over_a_later_change_to_a_name_that_is_not_utf8() {
    let mut r = repo_rig().await;
    let p = r.ws.join(std::ffi::OsStr::from_bytes(b"n\xffm"));
    std::fs::write(&p, "before\n").unwrap();
    let turn = r
        .turn("printf 'turn\\n' > \"$(printf 'n\\377m')\"")
        .await
        .id
        .unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), b"turn\n");
    std::fs::write(&p, "person\n").unwrap();
    let e = r.restore(&turn, false).await.result.unwrap_err();
    assert!(e.contains("changed after the turn"), "{e}");
    assert_eq!(std::fs::read(&p).unwrap(), b"person\n");
}

/// A file named like pathspec magic, changed by the turn and then by the
/// person: refused, the person's version stays.
#[tokio::test]
async fn undo_refuses_over_a_later_change_to_a_name_that_reads_as_pathspec_magic() {
    let mut r = repo_rig().await;
    r.write(":(attr:zz)k", "before\n");
    let turn = r.turn("printf 'turn\\n' > ':(attr:zz)k'").await.id.unwrap();
    assert_eq!(r.read(":(attr:zz)k").as_deref(), Some("turn\n"));
    r.write(":(attr:zz)k", "person\n");
    let e = r.restore(&turn, false).await.result.unwrap_err();
    assert!(e.contains("changed after the turn"), "{e}");
    assert_eq!(r.read(":(attr:zz)k").as_deref(), Some("person\n"));
}

/// Review of PR 160, P1, through a node: the turn untracks `.env`, the
/// person writes a new one (ignored, so in no checkpoint). Undo is refused
/// naming `.env` before anything is written; "Undo anyway" goes ahead and
/// its note names it again.
#[tokio::test]
async fn undo_refuses_over_an_ignored_file_in_its_way_and_names_it() {
    let mut r = repo_rig().await;
    r.write(".env", "OLD=1\n");
    r.git(&["add", ".env"]);
    r.git(&["commit", "-qm", "env"]);
    let turn = r
        .turn("git rm -q --cached .env && rm .env && echo .env > .gitignore")
        .await
        .id
        .unwrap();
    r.write(".env", "PERSON=2\n");
    let e = r.restore(&turn, false).await.result.unwrap_err();
    assert!(
        e.contains("in no checkpoint") && e.contains("`.env`") && e.contains("Undo anyway"),
        "{e}"
    );
    assert_eq!(r.read(".env").as_deref(), Some("PERSON=2\n"));
    let mark = r.seen.len();
    let forced = r.restore(&turn, true).await;
    assert_eq!(forced.result, Ok(()));
    assert_eq!(r.read(".env").as_deref(), Some("OLD=1\n"));
    let notes = r.notes_since(mark);
    assert!(
        notes
            .iter()
            .any(|n| n.contains("`.env`") && n.contains("not saved")),
        "{notes:?}"
    );
}

/// Review 2 of PR 160, L1, through a node: a turn that puts a named pipe
/// where a tracked file was still gets its checkpoint (before, every
/// snapshot failed while the pipe was there), and the chat says once that
/// the checkpoint left the pipe out.
#[tokio::test]
async fn a_turn_that_leaves_a_pipe_where_a_file_was_is_checkpointed_and_says_so() {
    let mut r = repo_rig().await;
    r.write("p", "v0\n");
    r.write("m", "m old\n");
    r.git(&["add", "p", "m"]);
    r.git(&["commit", "-qm", "p"]);
    let mark = r.seen.len();
    // The turn also changes a tracked file next to the pipe.
    let turn = r.turn("rm p && mkfifo p && echo 'm new' > m").await;
    assert!(turn.id.is_some(), "{:?}", r.notes_since(mark));
    let notes = r.notes_since(mark);
    let said: Vec<&String> = notes.iter().filter(|n| n.contains("`p`")).collect();
    assert_eq!(said.len(), 1, "{notes:?}");
    assert!(said[0].contains("named pipe"), "{notes:?}");
    assert!(
        !notes.iter().any(|n| n.contains("no checkpoint")),
        "{notes:?}"
    );
    // The turn's change list names `m` as changed, not deleted, and an
    // Undo of the turn brings its old content back.
    let diff = r.diff(turn.id.as_deref().unwrap(), false).await.unwrap();
    let m = diff.files.iter().find(|f| f.path == "m");
    assert!(
        m.is_some_and(|f| f.status == harness_protocol::DiffStatus::Modified),
        "{:?}",
        diff.files
    );
    // A second turn with the pipe still there: nothing new to say.
    let mark = r.seen.len();
    r.turn("true").await;
    assert!(
        !r.notes_since(mark).iter().any(|n| n.contains("`p`")),
        "{:?}",
        r.notes_since(mark)
    );
    let undo = r
        .restore_with(turn.id.as_deref().unwrap(), true, true)
        .await;
    assert_eq!(undo.result, Ok(()));
    assert_eq!(r.read("m").as_deref(), Some("m old\n"));
}
