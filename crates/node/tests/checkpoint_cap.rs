//! What a person is told about files left out of checkpoints for their size
//! (`[limits] checkpoint_untracked_max_kb`, docs/undo-and-stash.md §2.7),
//! through a real node: the turn's note, the undo's note, a sub-agent's
//! merge-back note. The cap is 4 KiB, so no file here is over 16 KiB.

#[path = "support/rig.rs"]
mod support;

use support::Rig;

async fn repo_rig() -> Rig {
    let r = Rig::with_checkpoint_cap_kb(4).await;
    r.git(&["init", "-q"]);
    r.write("a.txt", "one\n");
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "init"]);
    r
}

fn big(r: &Rig, name: &str, n: usize, b: u8) {
    std::fs::write(r.ws.join(name), vec![b; n]).unwrap();
}

/// A file made over the cap in a turn: the turn's note names it with its
/// size, the undo leaves it (and says so) and removes the small one.
#[tokio::test]
async fn a_file_made_over_the_cap_is_named_and_left_by_the_undo() {
    let mut r = repo_rig().await;
    let mark = r.seen.len();
    let turn = r
        .turn("head -c 9000 /dev/zero > big.bin; echo s > small.txt")
        .await;
    let notes = r.notes_since(mark);
    let note = notes
        .iter()
        .find(|n| n.starts_with("Not checkpointed"))
        .unwrap_or_else(|| panic!("{notes:#?}"));
    eprintln!("turn note: {note}");
    assert!(note.contains("1 file over 4.0 KiB"), "{note}");
    assert!(note.contains("`big.bin` (8.8 KiB)"), "{note}");
    assert!(note.contains("checkpoint_untracked_max_kb"), "{note}");
    let mark = r.seen.len();
    let undo = r.restore(&turn.id.unwrap(), false).await;
    assert_eq!(undo.result, Ok(()));
    assert!(r.read("small.txt").is_none());
    assert_eq!(
        std::fs::read(r.ws.join("big.bin")).unwrap(),
        vec![0u8; 9000]
    );
    let notes = r.notes_since(mark);
    eprintln!("undo notes: {notes:#?}");
    assert!(
        notes
            .iter()
            .any(|n| n.contains("Undo left these files as they are") && n.contains("`big.bin`")),
        "{notes:#?}"
    );
}

/// The same large files in every turn are named once, not at every turn.
#[tokio::test]
async fn the_same_large_files_are_named_once() {
    let mut r = repo_rig().await;
    big(&r, "data.bin", 9000, b'd');
    let mark = r.seen.len();
    r.turn("echo 1 > a.txt").await;
    assert_eq!(
        r.notes_since(mark)
            .iter()
            .filter(|n| n.starts_with("Not checkpointed"))
            .count(),
        1
    );
    let mark = r.seen.len();
    r.turn("echo 2 > a.txt").await;
    assert!(
        !r.notes_since(mark)
            .iter()
            .any(|n| n.contains("Not checkpointed")),
        "{:#?}",
        r.notes_since(mark)
    );
    // Changed between turns: named again.
    big(&r, "data.bin", 12000, b'e');
    let mark = r.seen.len();
    r.turn("echo 3 > a.txt").await;
    assert!(
        r.notes_since(mark)
            .iter()
            .any(|n| n.contains("`data.bin` (11.7 KiB)")),
        "{:#?}",
        r.notes_since(mark)
    );
}

/// A file over the cap before the turn that the turn replaced by a small
/// one: the turn's note says its earlier version cannot come back; the undo
/// leaves the small one and says the same.
#[tokio::test]
async fn a_large_file_the_turn_shrank_cannot_come_back_and_both_notes_say_so() {
    let mut r = repo_rig().await;
    big(&r, "data.bin", 9000, b'd');
    let mark = r.seen.len();
    let turn = r.turn("echo small > data.bin").await;
    let notes = r.notes_since(mark);
    assert!(
        notes
            .iter()
            .any(|n| n.contains("Made, changed or deleted during this turn")
                && n.contains("`data.bin`")
                && n.contains("cannot bring back")),
        "{notes:#?}"
    );
    let mark = r.seen.len();
    let undo = r.restore(&turn.id.unwrap(), false).await;
    assert_eq!(undo.result, Ok(()));
    assert_eq!(r.read("data.bin").as_deref(), Some("small\n"));
    let notes = r.notes_since(mark);
    assert!(
        notes
            .iter()
            .any(|n| n.contains("could not bring back the earlier versions")
                && n.contains("`data.bin` (8.8 KiB)")),
        "{notes:#?}"
    );
}

/// A file the turn grew over the cap: the undo puts the earlier version
/// back (the checkpoint holds it), and undoing the undo brings the big one
/// back (the pre-restore checkpoint holds it whatever its size).
#[tokio::test]
async fn a_file_grown_over_the_cap_is_undone_and_the_undo_can_be_undone() {
    let mut r = repo_rig().await;
    r.write("log.txt", "small\n");
    let turn = r.turn("head -c 9000 /dev/zero >> log.txt").await;
    let grown = std::fs::read(r.ws.join("log.txt")).unwrap();
    assert_eq!(grown.len(), 9006);
    let undo = r.restore(&turn.id.unwrap(), false).await;
    assert_eq!(undo.result, Ok(()));
    assert_eq!(r.read("log.txt").as_deref(), Some("small\n"));
    let again = r.restore(&undo.saved.unwrap(), false).await;
    assert_eq!(again.result, Ok(()));
    assert_eq!(std::fs::read(r.ws.join("log.txt")).unwrap(), grown);
}

/// A writing sub-agent does not get the file over the cap; its own change
/// lands, the parent's file is untouched, and the parent is told.
#[tokio::test]
async fn a_sub_agent_works_without_the_large_file_and_the_parent_is_told() {
    let mut r = repo_rig().await;
    big(&r, "data.bin", 9000, b'd');
    let (_, outs) = r
        .lead_turn("delegate: dev=run: ls data.bin; echo hi > c.txt")
        .await;
    assert_eq!(outs.len(), 1, "{outs:#?}");
    let out = &outs[0];
    eprintln!("delegate result: {out}");
    assert!(
        out.contains("No such file") || out.contains("cannot access"),
        "{out}"
    );
    assert!(
        out.contains("not copied into the sub-agent's folder"),
        "{out}"
    );
    assert!(out.contains("`data.bin` (8.8 KiB)"), "{out}");
    assert_eq!(r.read("c.txt").as_deref(), Some("hi\n"));
    assert_eq!(
        std::fs::read(r.ws.join("data.bin")).unwrap(),
        vec![b'd'; 9000]
    );
}

/// A sub-agent that writes where the parent has a file over the cap is not
/// applied, and nothing of it is written.
#[tokio::test]
async fn a_sub_agent_writing_over_the_large_file_is_not_applied() {
    let mut r = repo_rig().await;
    big(&r, "data.bin", 9000, b'd');
    let (_, outs) = r
        .lead_turn("delegate: dev=run: echo mine > data.bin; echo hi > c.txt")
        .await;
    let out = &outs[0];
    assert!(out.contains("NOT applied"), "{out}");
    assert!(out.contains("over the checkpoint size limit"), "{out}");
    assert!(r.read("c.txt").is_none());
    assert_eq!(
        std::fs::read(r.ws.join("data.bin")).unwrap(),
        vec![b'd'; 9000]
    );
}

/// An undo that writes a file where the turn made a folder holding a large
/// file removes that file: the note says so and that undoing the undo
/// brings it back (review of PR 154, P2).
#[tokio::test]
async fn an_undo_that_removes_a_large_file_says_so() {
    let mut r = repo_rig().await;
    r.write("d", "small\n");
    let turn = r
        .turn("rm d && mkdir d && head -c 9000 /dev/zero > d/big && echo x > d/x")
        .await;
    let mark = r.seen.len();
    let undo = r.restore(&turn.id.unwrap(), false).await;
    assert_eq!(undo.result, Ok(()));
    assert_eq!(r.read("d").as_deref(), Some("small\n"));
    let notes = r.notes_since(mark);
    assert!(
        notes.iter().any(|n| n.contains("Undo removed these files")
            && n.contains("`d/big` (8.8 KiB)")
            && n.contains("undoing this undo brings them back")),
        "{notes:#?}"
    );
    let again = r.restore(&undo.saved.unwrap(), false).await;
    assert_eq!(again.result, Ok(()));
    assert_eq!(std::fs::read(r.ws.join("d/big")).unwrap(), vec![0u8; 9000]);
}

/// More files over the cap than a checkpoint names, and the turn shrank one
/// it did not name: the undo does not delete it and says why (review of PR
/// 154, P4).
#[tokio::test]
async fn an_undo_beside_more_large_files_than_are_named_deletes_none_unnamed() {
    let mut r = repo_rig().await;
    std::fs::create_dir(r.ws.join("deps")).unwrap();
    for i in 0..1200 {
        big(&r, &format!("deps/{i:05}"), 5000, b'd');
    }
    big(&r, "f", 4100, b'f');
    let turn = r.turn("echo small > f").await;
    let mark = r.seen.len();
    let undo = r.restore(&turn.id.unwrap(), false).await;
    assert_eq!(undo.result, Ok(()));
    assert_eq!(r.read("f").as_deref(), Some("small\n"));
    let notes = r.notes_since(mark);
    assert!(
        notes
            .iter()
            .any(|n| n.contains("Undo did not delete these files") && n.contains("`f`")),
        "{notes:#?}"
    );
}
