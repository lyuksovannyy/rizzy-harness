//! Experiment B.2 (ADOPTION_PLAN): what the user's own git housekeeping does
//! to checkpoints that borrow objects from the user's repository.
//!
//! A checkpoint repository in a git folder borrows the user's objects
//! (`objects/info/alternates`, `crates/node/src/git.rs`), so it stores only
//! what the user's repository lacks. These tests assert what happens when
//! the user's own housekeeping (amend, `gc --prune=now`, `repack -ad`,
//! deleting a branch, re-cloning) removes objects a checkpoint refers to.
//!
//! The experiment found a gap; A.16 closed what can be closed while
//! checkpoints borrow (decision O11 is whether they stop):
//! - a restore is all or nothing: every blob it writes is checked and copied
//!   into the checkpoint repository's own objects before any file is touched,
//!   and a checkpoint that depends on content the user's repository no
//!   longer has is refused, with that reason, with the folder unchanged and
//!   nothing logged;
//! - a damaged checkpoint index (it names a blob that is gone) is rebuilt
//!   from the folder once, so later turns get checkpoints again.
//!
//! A.17 then closed the main gap: at the end of every turn the old versions
//! of the files it changed are copied into the checkpoint repository's own
//! store (`crates/node/tests/checkpoint_upkeep.rs`), so housekeeping in the
//! user's repository *after* a turn no longer takes its undo: the tests that
//! were named `..._is_refused_...` now assert the restore is `Ok` and the
//! file holds X. What remains refused is content gone *before* the turn
//! ended (the agent rewrote and pruned history inside its own turn), which
//! is how the all-or-nothing refusal of A.16 is still exercised here.
//!
//! Every git call here runs with a fixed identity, no system or global
//! config, and a cleared environment, in a temporary directory.

#[path = "support/rig.rs"]
mod rig;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use harness_core::event::*;
use rig::{AGENT_GIT, Restore, Rig, X};

const SCRATCH: &str = "untracked scratch file\n";

// ---- scenarios ----

/// The turn every scenario undoes: it overwrites `keep.txt` (committed by the
/// user with content X) with `v2`. With `dirty`, an untracked file makes the
/// folder differ from the user's last commit, so the checkpoint's root tree
/// is a new object; without it the whole tree equals the user's commit.
async fn turn_over_committed_x(rig: &mut Rig, dirty: bool) -> (String, String) {
    let x = rig.base_repo();
    if dirty {
        rig.write("scratch.txt", SCRATCH);
    }
    let t = rig.turn("echo v2 > keep.txt").await;
    assert_eq!(t.phases, ["before", "after"]);
    // Borrowing is really on (the unchanged `other.txt` is only in the
    // user's store), and X, which the turn changed, was copied (A.17).
    assert!(rig.user_has(&x));
    let other = rig.git(&["rev-parse", "HEAD:other.txt"]);
    assert!(
        !rig.shadow_owns(&other),
        "an unchanged file is in the shadow's own objects, so this is not testing borrowing"
    );
    assert!(
        rig.shadow_owns(&x),
        "X was not copied at the end of the turn"
    );
    (x, t.id.unwrap())
}

/// The agent's own history rewrite inside its turn, after it changed
/// files: amend, expire the reflog, prune everything unreachable.
fn agent_rewrite() -> String {
    format!(
        "{AGENT_GIT}; git commit -qa --amend -m amended && \
         git reflog expire --expire=now --all && git gc --prune=now -q"
    )
}

/// The turn overwrites `keep.txt` (X) with `v2` and then itself rewrites
/// history and prunes, so X is gone before the turn ends and could not be
/// copied: the content an undo needs is lost (the A.16 refusal path).
/// Dirty, so the checkpoint's root tree is its own and the turn has its
/// "after" checkpoint.
async fn turn_that_loses_x_inside(rig: &mut Rig) -> (String, String) {
    let x = rig.base_repo();
    rig.write("scratch.txt", SCRATCH);
    let mark = rig.seen.len();
    let t = rig
        .turn(&format!("echo v2 > keep.txt; {}", agent_rewrite()))
        .await;
    assert_eq!(t.phases, ["before", "after"]);
    assert!(!rig.user_has(&x), "the agent's gc did not remove X");
    let notes = rig.notes_since(mark);
    assert!(
        notes
            .iter()
            .any(|n| n.starts_with("This turn's undo is incomplete") && n.contains("`keep.txt`")),
        "{notes:?}"
    );
    (x, t.id.unwrap())
}

/// [`turn_that_loses_x_inside`], then the undo of that turn.
async fn lost_inside_scenario() -> (Rig, Restore) {
    let mut rig = Rig::new().await;
    let (_x, turn) = turn_that_loses_x_inside(&mut rig).await;
    let r = rig.restore(&turn, false).await;
    (rig, r)
}

/// The user's folder as the turn left it, for the unforced undo to accept.
fn the_turns_result_is_back(rig: &Rig) {
    rig.write("keep.txt", "v2\n");
}

fn x_bytes() -> Vec<u8> {
    X.as_bytes().to_vec()
}

/// The plain refusal: nothing changed, and why (content the folder's
/// repository no longer has), naming `file` when one is given.
fn refused_for_lost_content(e: &str, file: Option<&str>) -> bool {
    e.starts_with("nothing was changed:")
        && e.contains("the git repository in this folder no longer has")
        && file.is_none_or(|f| e.contains(&format!("`{f}`")))
}

/// Refused with the plain reason; same bytes in every file, no checkpoint
/// event.
fn assert_refused(r: &Restore, file: Option<&str>) {
    let e = r.result.clone().unwrap_err();
    assert!(refused_for_lost_content(&e, file), "{e}");
    assert_eq!(r.folder_after, r.folder_before, "the folder changed");
    assert!(r.phases.is_empty(), "logged: {:?}", r.phases);
}

/// [`assert_refused`], and no pre-restore checkpoint ref left behind.
fn assert_refused_untouched(rig: &Rig, r: &Restore, file: Option<&str>) {
    assert_refused(r, file);
    assert!(
        !rig.shadow_refs().iter().any(|r| r.contains("/restore-")),
        "a pre-restore ref was left: {:?}",
        rig.shadow_refs()
    );
}

/// Amend + expire the reflog + `gc --prune=now`, then the turn's result is
/// put back in the folder; the undo of the turn is attempted.
async fn main_scenario(dirty: bool) -> (Rig, String, Restore) {
    let mut rig = Rig::new().await;
    let (x, turn) = turn_over_committed_x(&mut rig, dirty).await;
    rig.amend_x_away();
    rig.expire_reflog_and_gc_now();
    assert!(
        !rig.user_has(&x),
        "gc did not remove X from the user's repository"
    );
    the_turns_result_is_back(&rig);
    let r = rig.restore(&turn, false).await;
    (rig, turn, r)
}

// ---- the main scenario ----

/// ADOPTION_PLAN B.2, final review D3. A checkpoint borrows the blobs the
/// user's repository already has (`objects/info/alternates`), so a commit
/// the user later amends away and `git gc --prune=now` removes took the
/// checkpoint's content with it; A.16 made that undo a clean refusal. A.17
/// copies X into the checkpoint repository's own store at the end of the
/// turn: the undo works and `keep.txt` holds X.
#[tokio::test]
async fn gc_in_the_users_repository_after_the_turn_no_longer_takes_its_undo() {
    let (_rig, _turn, r) = main_scenario(true).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("keep.txt"), Some(&x_bytes()));
}

/// When the content is gone before it could be copied (the agent pruned it
/// inside its own turn), the refusal says why in plain words.
#[tokio::test]
async fn content_lost_inside_the_turn_is_refused_with_the_reason() {
    let (rig, r) = lost_inside_scenario().await;
    assert_refused_untouched(&rig, &r, Some("keep.txt"));
    let e = r.result.unwrap_err();
    assert!(e.contains("rewritten"), "says why: {e}");
}

/// Was the most serious form of the gap: `git checkout-index -f` removed the
/// file before it found it could not write the old content, so the user's
/// `keep.txt` (the turn's result `v2`) was deleted. Now every blob is
/// checked before any file is touched: the folder is byte for byte what it
/// was.
#[tokio::test]
async fn a_restore_that_needs_a_missing_blob_changes_nothing() {
    let (rig, r) = lost_inside_scenario().await;
    assert_refused_untouched(&rig, &r, Some("keep.txt"));
    assert_eq!(r.folder_after.get("keep.txt"), Some(&b"v2\n".to_vec()));
    assert_eq!(rig.read("keep.txt").as_deref(), Some("v2\n"));
    assert_eq!(
        r.folder_after.get("scratch.txt"),
        Some(&SCRATCH.as_bytes().to_vec())
    );
}

/// Was: with two files in the turn, the one whose blob survived was restored
/// and the other deleted (a half-restored folder). Now all or nothing:
/// neither file changes.
#[tokio::test]
async fn a_restore_with_one_lost_blob_changes_neither_file() {
    let mut rig = Rig::new().await;
    let x = rig.base_repo();
    // Never committed, so the checkpoint repository stores its blob itself
    // (and the agent's `commit -a` leaves it out).
    rig.write("a_ok.txt", "A: original, only in the folder\n");
    let t = rig
        .turn(&format!(
            "echo a2 > a_ok.txt; echo v2 > keep.txt; {}",
            agent_rewrite()
        ))
        .await;
    assert!(!rig.user_has(&x));
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_refused_untouched(&rig, &r, Some("keep.txt"));
    assert_eq!(r.folder_after.get("a_ok.txt"), Some(&b"a2\n".to_vec()));
    assert_eq!(r.folder_after.get("keep.txt"), Some(&b"v2\n".to_vec()));
}

/// Was: the error named a pre-restore checkpoint that never got its `after`
/// and could not be restored ("incomplete"). Now a refused restore writes no
/// checkpoint at all: no event, no ref. (A failure after the pre-restore
/// checkpoint is logged is covered by
/// `a_failure_after_the_pre_restore_checkpoint_names_it_and_it_restores`.)
#[tokio::test]
async fn a_refused_restore_saves_no_checkpoint() {
    let (rig, r) = lost_inside_scenario().await;
    assert_refused_untouched(&rig, &r, None);
    assert_eq!(r.saved, None);
    assert!(!r.result.unwrap_err().contains("saved as checkpoint"));
    // Only the turn's two refs.
    assert_eq!(rig.shadow_refs().len(), 2, "{:?}", rig.shadow_refs());
}

/// The other face. If the folder was clean when the checkpoint was taken,
/// the checkpoint's *tree* equals a tree the user's commit already has, so
/// even the tree is borrowed and gc removes it (this was refused with the
/// plain reason after A.16). A.17 copies the trees a diff of the turn reads
/// too: the undo works.
#[tokio::test]
async fn a_checkpoint_whose_tree_the_user_pruned_still_restores() {
    let (_rig, _turn, r) = main_scenario(false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("keep.txt"), Some(&x_bytes()));
}

/// Was: a file the folder still has, unchanged, whose blob is gone (the user
/// ran `git reset HEAD~1` and gc: the file stays, the commit and its blob go)
/// stayed in the checkpoint repository's index, `write-tree` refused, and
/// every later turn ran with no checkpoint (only a warning in the node's
/// log) and every restore stopped at its pre-restore snapshot.
///
/// Now the index is rebuilt from the folder once: the next turn has its
/// checkpoint and its undo works. No checkpoint is deleted by the repair.
/// The older turn did not change `keep.txt`, so its undo needs nothing
/// that is gone (its trees were copied, A.17) and works too.
#[tokio::test]
async fn a_lost_blob_of_an_unchanged_file_is_repaired_at_the_next_snapshot() {
    let mut rig = Rig::new().await;
    let x = rig.base_repo();
    // Git treats a file written in the same second as the index as possibly
    // changed and rehashes it (which would store the blob in the checkpoint
    // repository); wait so the file is settled, as in any real session.
    tokio::time::sleep(Duration::from_millis(2100)).await;
    let first = rig.turn("echo ok > made.txt").await;
    assert_eq!(first.phases, ["before", "after"]);
    assert!(!rig.shadow_owns(&x));
    rig.git(&["reset", "-q", "HEAD~1"]);
    rig.expire_reflog_and_gc_now();
    assert!(!rig.user_has(&x));
    assert_eq!(
        rig.read("keep.txt").as_deref(),
        Some(X),
        "the file is still there"
    );
    let refs_before = rig.shadow_refs();

    let next = rig.turn("echo more > more.txt").await;
    assert_eq!(
        next.phases,
        ["before", "after"],
        "the turn has a checkpoint"
    );
    assert_eq!(rig.read("more.txt").as_deref(), Some("more\n"));
    // No checkpoint was deleted by the repair.
    for r in &refs_before {
        assert!(rig.shadow_refs().contains(r), "{r} was deleted");
    }

    // The repaired turn undoes.
    let r = rig.restore(next.id.as_deref().unwrap(), false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("more.txt"), None);
    assert_eq!(r.folder_after.get("keep.txt"), Some(&x_bytes()));

    // The older one, whose root tree was the user's pruned one.
    let r = rig.restore(first.id.as_deref().unwrap(), true).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("made.txt"), None);
    assert_eq!(r.folder_after.get("keep.txt"), Some(&x_bytes()));
}

/// A blob the checkpoint borrowed, still present in the user's repository:
/// the restore works, and the blob is in the checkpoint repository's own
/// objects (copied at the end of the turn, A.17, and again before the
/// write). (That the copies write nothing in the user's repository is
/// tested in `git.rs`, `securing_blobs_copies_them_and_writes_nothing_in_the_users_repository`,
/// and `git/upkeep.rs`, `pinning_and_collecting_write_nothing_in_the_users_repository`.)
#[tokio::test]
async fn a_borrowed_blob_is_copied_into_the_checkpoint_repository_by_a_restore() {
    let mut rig = Rig::new().await;
    let (x, turn) = turn_over_committed_x(&mut rig, true).await;
    the_turns_result_is_back(&rig);
    let r = rig.restore(&turn, false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("keep.txt"), Some(&x_bytes()));
    assert!(rig.shadow_owns(&x), "X was not copied into the shadow");
}

/// Whether this process can write into a directory it made read-only (it
/// can as root, where these permission tests prove nothing).
fn read_only_is_enforced(dir: &Path) -> bool {
    std::fs::write(dir.join(".probe"), "").is_err()
}

fn set_mode(p: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// A real failure after the pre-restore checkpoint is logged: a folder the
/// restore must write into is read-only, so `a.txt` is restored and
/// `ro/b.txt` cannot be. The error names the pre-restore checkpoint, and
/// restoring that checkpoint through the normal call (unforced) brings the
/// folder back to what it was before the failed restore.
#[tokio::test]
async fn a_failure_after_the_pre_restore_checkpoint_names_it_and_it_restores() {
    let mut rig = Rig::new().await;
    std::fs::create_dir(rig.ws.join("ro")).unwrap();
    rig.write("a.txt", "a1\n");
    rig.write("ro/b.txt", "b1\n");
    let t = rig.turn("echo a2 > a.txt; echo b2 > ro/b.txt").await;
    assert_eq!(t.phases, ["before", "after"]);
    set_mode(&rig.ws.join("ro"), 0o555);
    if !read_only_is_enforced(&rig.ws.join("ro")) {
        set_mode(&rig.ws.join("ro"), 0o755);
        eprintln!("SKIPPED: read-only folders are writable here (root?)");
        return;
    }
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    set_mode(&rig.ws.join("ro"), 0o755);
    let e = r.result.clone().unwrap_err();
    let saved = r.saved.clone().expect("a before_restore checkpoint");
    assert!(e.contains(&format!("saved as checkpoint {saved}")), "{e}");
    assert_eq!(r.phases, ["before_restore"]);
    // Half restored: the residual the error is there for.
    assert_eq!(r.folder_after.get("a.txt"), Some(&b"a1\n".to_vec()));
    assert_eq!(r.folder_after.get("ro/b.txt"), Some(&b"b2\n".to_vec()));

    let mark = rig.seen.len();
    let back = rig.restore(&saved, false).await;
    assert_eq!(back.result, Ok(()));
    assert_eq!(back.folder_after, r.folder_before, "not back to before");
    assert_eq!(back.phases, ["before_restore", "after", "restored"]);
    // Restoring an interrupted restore does not check for edits made since;
    // the chat names the checkpoint that holds what it overwrote.
    let safety = back.saved.clone().unwrap();
    let notes = notes_since(&rig, mark);
    assert!(
        notes.iter().any(|n| n.contains(&safety)),
        "no note names {safety}: {notes:?}"
    );
    // And the turn itself can still be undone once the folder is writable.
    let again = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_eq!(again.result, Ok(()));
    assert_eq!(again.folder_after.get("a.txt"), Some(&b"a1\n".to_vec()));
    assert_eq!(again.folder_after.get("ro/b.txt"), Some(&b"b1\n".to_vec()));
}

/// When a turn runs without a checkpoint (here the checkpoint repository's
/// objects cannot be written, and the one repair does not help), the person
/// is told in the chat, not only the node's log.
#[tokio::test]
async fn a_turn_without_a_checkpoint_says_so_in_the_chat() {
    let mut rig = Rig::new().await;
    rig.write("a.txt", "a1\n");
    let first = rig.turn("echo a2 > a.txt").await;
    assert_eq!(first.phases, ["before", "after"]);
    let objects = rig.shadow().join("objects");
    let dirs: Vec<PathBuf> = walkdir::WalkDir::new(&objects)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir())
        .map(|e| e.path().to_path_buf())
        .collect();
    for d in &dirs {
        set_mode(d, 0o555);
    }
    let enforced = read_only_is_enforced(&objects);
    let mark = rig.seen.len();
    let t = if enforced {
        Some(rig.turn("echo new > new.txt").await)
    } else {
        None
    };
    for d in &dirs {
        set_mode(d, 0o755);
    }
    let Some(t) = t else {
        eprintln!("SKIPPED: read-only folders are writable here (root?)");
        return;
    };
    assert!(t.phases.is_empty(), "{:?}", t.phases);
    assert_eq!(
        rig.read("new.txt").as_deref(),
        Some("new\n"),
        "the turn ran"
    );
    let told = rig.seen[mark..].iter().any(|e| {
        matches!(e.typed(), Ok(EventKind::Error(m))
            if m.message.starts_with("This turn has no checkpoint"))
    });
    assert!(told, "nothing in the chat says the turn has no undo");
}

/// The window between the check and the write: the user's repository loses
/// X after the restore checked it and before it writes (a stand-in `git`
/// first on `PATH` runs `gc --prune=now` in the user's repository when the
/// restore reads the blobs it writes, `cat-file --batch`, just before the first write). The restore
/// still writes X, because it was copied into the checkpoint repository
/// first. Runs in a child process: this one's `PATH` cannot be changed
/// without `unsafe`.
#[test]
fn a_blob_removed_between_the_check_and_the_write_is_still_restored() {
    if let Some(bin) = std::env::var_os(WINDOW_CHILD) {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(window_scenario(PathBuf::from(bin)));
        return;
    }
    let path = std::env::var("PATH").unwrap_or_default();
    let real = std::env::split_paths(&path)
        .map(|d| d.join("git"))
        .find(|g| g.is_file())
        .expect("git is needed for this test");
    let bin = tempfile::tempdir().unwrap();
    let b = bin.path().display();
    let wrapper = bin.path().join("git");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\n\
             for a in \"$@\"; do\n\
             if [ \"$a\" = --batch ] && [ -f '{b}/ws' ] && [ ! -e '{b}/fired' ]; then\n\
             : > '{b}/fired'\n\
             env -u GIT_INDEX_FILE -u GIT_ATTR_SOURCE GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
             '{r}' -C \"$(cat '{b}/ws')\" gc --prune=now -q || exit 99\n\
             fi\n\
             done\n\
             exec '{r}' \"$@\"\n",
            r = real.display()
        ),
    )
    .unwrap();
    set_mode(&wrapper, 0o755);
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "a_blob_removed_between_the_check_and_the_write_is_still_restored",
            "--exact",
            "--nocapture",
        ])
        .env(WINDOW_CHILD, bin.path())
        .env(
            "PATH",
            std::env::join_paths(
                std::iter::once(bin.path().to_path_buf()).chain(std::env::split_paths(&path)),
            )
            .unwrap(),
        )
        .status()
        .unwrap();
    assert!(status.success(), "the scenario failed: {status}");
}

const WINDOW_CHILD: &str = "HARNESS_TEST_A16_WINDOW_BIN";

async fn window_scenario(bin: PathBuf) {
    let mut rig = Rig::new().await;
    let (x, turn) = turn_over_committed_x(&mut rig, true).await;
    rig.amend_x_away();
    rig.git(&["reflog", "expire", "--expire=now", "--all"]);
    assert!(rig.user_has(&x), "unreachable, but present at the check");
    the_turns_result_is_back(&rig);
    std::fs::write(bin.join("ws"), rig.ws.to_str().unwrap()).unwrap();
    let r = rig.restore(&turn, false).await;
    assert!(bin.join("fired").exists(), "the seam did not run");
    assert!(!rig.user_has(&x), "X was not removed in the window");
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("keep.txt"), Some(&x_bytes()));
}

// ---- controls ----

/// The checks the experiment rests on, made once on their own: the folder is
/// a repository, the shadow borrows (alternates point at the user's object
/// directory), a committed file the turn did not change (`other.txt`) is not
/// in the shadow's own objects, and the same check does find an object the
/// shadow did write (`v2`), so it can say yes.
#[tokio::test]
async fn control_the_blob_is_borrowed_and_the_check_can_tell() {
    let mut rig = Rig::new().await;
    let (x, _turn) = turn_over_committed_x(&mut rig, true).await;
    let alternates = std::fs::read_to_string(rig.shadow().join("objects/info/alternates")).unwrap();
    assert_eq!(
        alternates.trim_end(),
        rig.ws.join(".git/objects").to_str().unwrap()
    );
    assert!(rig.user_has(&x));
    let other = rig.git(&["rev-parse", "HEAD:other.txt"]);
    assert!(!rig.shadow_owns(&other));
    let v2 = rig.git(&["hash-object", "keep.txt"]);
    assert!(
        rig.shadow_owns(&v2),
        "the shadow stores what the user's repository lacks"
    );
}

/// Control: not a repository, nothing to borrow, the same turn and undo.
#[tokio::test]
async fn control_a_folder_that_is_not_a_repository_restores_x() {
    let mut rig = Rig::new().await;
    rig.write("keep.txt", X);
    rig.write("scratch.txt", SCRATCH);
    let t = rig.turn("echo v2 > keep.txt").await;
    assert!(!rig.ws.join(".git").exists());
    assert!(
        !rig.shadow().join("objects/info/alternates").exists(),
        "no borrowing in a plain folder"
    );
    the_turns_result_is_back(&rig);
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("keep.txt"), Some(&x_bytes()));
}

/// Control: the same rewrite of history, without the gc. X is still in the
/// user's store (unreachable but present), so the undo works.
#[tokio::test]
async fn control_without_the_gc_step_the_restore_succeeds() {
    let mut rig = Rig::new().await;
    let (x, turn) = turn_over_committed_x(&mut rig, true).await;
    rig.amend_x_away();
    rig.git(&["reflog", "expire", "--expire=now", "--all"]);
    assert!(rig.user_has(&x), "unreachable, but not yet pruned");
    the_turns_result_is_back(&rig);
    let r = rig.restore(&turn, false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.phases, ["before_restore", "after", "restored"]);
    assert_eq!(r.folder_after.get("keep.txt"), Some(&x_bytes()));
}

// ---- variants ----

/// The object was only ever staged (`git add`, then `git reset`): it lives
/// in the user's store as a dangling loose object, the checkpoint borrowed
/// it, gc removes it. Before A.17 refused (before A.16 the file was
/// deleted); now it was copied at the end of the turn and the undo works.
#[tokio::test]
async fn a_checkpoint_of_a_file_that_was_only_ever_staged_restores_after_gc() {
    let mut rig = Rig::new().await;
    rig.base_repo();
    rig.write("staged.txt", "S: only ever staged\n");
    rig.git(&["add", "staged.txt"]);
    let s = rig.git(&["hash-object", "staged.txt"]);
    rig.git(&["reset", "-q"]);
    let t = rig.turn("echo v2 > staged.txt").await;
    assert!(rig.user_has(&s));
    rig.expire_reflog_and_gc_now();
    assert!(!rig.user_has(&s));
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(
        r.folder_after.get("staged.txt"),
        Some(&b"S: only ever staged\n".to_vec())
    );
}

/// The work was on a branch the user then deleted (`branch -D`) and
/// pruned: before A.17 refused, now the undo works.
#[tokio::test]
async fn a_checkpoint_restores_after_the_branch_that_held_x_is_deleted_and_gc() {
    let mut rig = Rig::new().await;
    rig.git(&["init", "-q", "-b", "main"]);
    rig.write("other.txt", "base\n");
    rig.git(&["add", "."]);
    rig.git(&["commit", "-qm", "base"]);
    rig.git(&["checkout", "-qb", "feature"]);
    rig.write("keep.txt", X);
    rig.git(&["add", "."]);
    rig.git(&["commit", "-qm", "feature work"]);
    let x = rig.git(&["hash-object", "keep.txt"]);
    rig.write("scratch.txt", SCRATCH);
    let t = rig.turn("echo v2 > keep.txt").await;
    rig.git(&["checkout", "-qf", "main"]);
    rig.git(&["branch", "-D", "feature"]);
    rig.expire_reflog_and_gc_now();
    assert!(!rig.user_has(&x));
    the_turns_result_is_back(&rig);
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("keep.txt"), Some(&x_bytes()));
}

/// `git gc --auto` with default settings does nothing in a repository this
/// small (it waits for about 6,700 loose objects), so nothing is lost. In
/// practice: auto gc only bites repositories that are large or old, and then
/// only after the reflog and the grace period (next two tests).
#[tokio::test]
async fn gc_auto_with_default_settings_does_not_remove_anything_in_a_small_repository() {
    let mut rig = Rig::new().await;
    let (x, turn) = turn_over_committed_x(&mut rig, true).await;
    rig.amend_x_away();
    rig.git(&["gc", "--auto", "-q"]);
    assert!(rig.user_has(&x));
    the_turns_result_is_back(&rig);
    let r = rig.restore(&turn, false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("keep.txt"), Some(&x_bytes()));
}

/// A plain `git gc` right after the rewrite keeps X twice over: the reflog
/// still names the old commit, and an unreachable object younger than two
/// weeks is not pruned. The undo works.
#[tokio::test]
async fn default_gc_right_after_the_rewrite_keeps_x() {
    let mut rig = Rig::new().await;
    let (x, turn) = turn_over_committed_x(&mut rig, true).await;
    rig.amend_x_away();
    rig.git(&["gc", "-q"]);
    assert!(rig.user_has(&x));
    the_turns_result_is_back(&rig);
    let r = rig.restore(&turn, false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("keep.txt"), Some(&x_bytes()));
}

/// The slow version of the main scenario: a plain `git gc` with
/// default settings once the reflog entry has expired (default 30 days for
/// an unreachable commit) and the object is older than the two-week grace
/// period. Time is simulated: the reflog is expired by hand and the loose
/// objects are back-dated 20 days (the test cannot wait). What it shows: a
/// user who never runs `--prune=now` loses the object too, about a month
/// after the rewrite, and checkpoints are kept for as many turns, not days
/// (`checkpoints_keep`, default 10), so an old chat's undo can be that old.
/// Before A.17 refused; now the copy made at the end of the turn serves.
#[tokio::test]
async fn default_gc_after_the_reflog_and_the_grace_period_keeps_the_checkpoint() {
    let mut rig = Rig::new().await;
    let (x, turn) = turn_over_committed_x(&mut rig, true).await;
    rig.amend_x_away();
    rig.git(&["reflog", "expire", "--expire=now", "--all"]);
    rig.age_objects(20);
    rig.git(&["gc", "-q"]);
    assert!(!rig.user_has(&x));
    the_turns_result_is_back(&rig);
    let r = rig.restore(&turn, false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("keep.txt"), Some(&x_bytes()));
}

/// `git repack -ad` drops unreachable objects that sit in a pack, but it
/// counts the reflog as a root: with the reflog intact the object stays.
#[tokio::test]
async fn repack_ad_keeps_x_while_the_reflog_names_it() {
    let mut rig = Rig::new().await;
    let x = rig.base_repo();
    rig.git(&["gc", "-q"]); // X is in a pack, as in any older repository
    rig.write("scratch.txt", SCRATCH);
    let t = rig.turn("echo v2 > keep.txt").await;
    rig.amend_x_away();
    rig.git(&["repack", "-ad", "-q"]);
    assert!(rig.user_has(&x));
    the_turns_result_is_back(&rig);
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_eq!(r.result, Ok(()));
}

/// Plain `git repack -ad` after the reflog is expired removes the packed
/// object (no `gc`, no `--prune`): before A.17 refused, now the undo works.
#[tokio::test]
async fn repack_ad_after_the_reflog_is_expired_keeps_the_checkpoint() {
    let mut rig = Rig::new().await;
    let x = rig.base_repo();
    rig.git(&["gc", "-q"]);
    rig.write("scratch.txt", SCRATCH);
    let t = rig.turn("echo v2 > keep.txt").await;
    rig.amend_x_away();
    rig.git(&["reflog", "expire", "--expire=now", "--all"]);
    rig.git(&["repack", "-ad", "-q"]);
    assert!(!rig.user_has(&x));
    the_turns_result_is_back(&rig);
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("keep.txt"), Some(&x_bytes()));
}

/// The user deletes `.git` and clones again into the same folder. The shadow
/// is keyed by the folder, so it is reused and its alternates now name the
/// new clone's object directory. The commit that held X was never pushed, so
/// the new clone lacks it: before A.17 refused, now the copy made at the end
/// of the turn serves and the undo works.
#[tokio::test]
async fn re_cloning_the_repository_keeps_a_checkpoint_of_unpushed_work() {
    let (rig, r) = reclone_scenario(false).await;
    assert!(!rig.user_has_x);
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("keep.txt"), Some(&x_bytes()));
}

/// Control for the previous test: the work had been pushed, the new clone has
/// the object (content-addressed, so same id), and the undo works.
#[tokio::test]
async fn re_cloning_a_repository_that_has_the_pushed_work_keeps_the_checkpoint() {
    let (rig, r) = reclone_scenario(true).await;
    assert!(rig.user_has_x);
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("keep.txt"), Some(&x_bytes()));
}

struct Cloned {
    user_has_x: bool,
}

async fn reclone_scenario(pushed: bool) -> (Cloned, Restore) {
    let mut rig = Rig::new().await;
    let x = rig.base_repo();
    let origin = rig.dir.path().join("origin.git");
    rig.git_in(rig.dir.path(), &["init", "-q", "--bare", "origin.git"]);
    let o = origin.display().to_string();
    if pushed {
        rig.git(&["push", "-q", &o, "main"]);
    } else {
        // Only the base commit; the commit with keep.txt stays local.
        rig.git(&["push", "-q", &o, "HEAD~1:refs/heads/main"]);
    }
    rig.write("scratch.txt", SCRATCH);
    let t = rig.turn("echo v2 > keep.txt").await;
    std::fs::remove_dir_all(rig.ws.join(".git")).unwrap();
    rig.git_in(rig.dir.path(), &["clone", "-q", &o, "reclone"]);
    std::fs::rename(rig.dir.path().join("reclone/.git"), rig.ws.join(".git")).unwrap();
    let user_has_x = rig.user_has(&x);
    the_turns_result_is_back(&rig);
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    (Cloned { user_has_x }, r)
}

// ---- the write path (A.16 review) ----

/// The chat notes (`error` events) seen since `mark`.
fn notes_since(rig: &Rig, mark: usize) -> Vec<String> {
    rig.notes_since(mark)
}

/// Undo of a turn that replaced a directory by a file of the same name:
/// the file goes, the directory and its file come back.
#[tokio::test]
async fn undo_of_a_directory_replaced_by_a_file_puts_the_directory_back() {
    let mut rig = Rig::new().await;
    std::fs::create_dir(rig.ws.join("d")).unwrap();
    rig.write("d/f", "f1\n");
    let t = rig.turn("rm -r d; echo x > d").await;
    assert_eq!(t.phases, ["before", "after"]);
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_eq!(r.result, Ok(()), "{:?}", r.phases);
    assert_eq!(r.phases, ["before_restore", "after", "restored"]);
    assert_eq!(r.folder_after.get("d/f"), Some(&b"f1\n".to_vec()));
}

/// Undo of a turn that replaced a file by a directory of the same name;
/// then, if it failed, the named pre-restore checkpoint.
#[tokio::test]
async fn undo_of_a_file_replaced_by_a_directory_puts_the_file_back() {
    let mut rig = Rig::new().await;
    rig.write("d", "d1\n");
    let t = rig.turn("rm d; mkdir d; echo y > d/f").await;
    assert_eq!(t.phases, ["before", "after"]);
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    if let Err(e) = &r.result {
        eprintln!("undo failed: {e}; folder now {:?}", r.folder_after.keys());
        let saved = r.saved.clone().unwrap();
        let back = rig.restore(&saved, false).await;
        eprintln!(
            "restore of {saved}: {:?}; folder now {:?}",
            back.result,
            back.folder_after.keys()
        );
    }
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.phases, ["before_restore", "after", "restored"]);
    assert_eq!(r.folder_after.get("d"), Some(&b"d1\n".to_vec()));
}

/// The same two swaps two levels deep: file `a` against `a/b/c`.
#[tokio::test]
async fn undo_of_swaps_two_levels_deep_works_both_ways() {
    let mut rig = Rig::new().await;
    rig.write("a", "a1\n");
    let t = rig.turn("rm a; mkdir -p a/b; echo c > a/b/c").await;
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("a"), Some(&b"a1\n".to_vec()));
    assert_eq!(r.folder_after.len(), 1, "{:?}", r.folder_after.keys());

    let mut rig = Rig::new().await;
    std::fs::create_dir_all(rig.ws.join("a/b")).unwrap();
    rig.write("a/b/c", "c1\n");
    let t = rig.turn("rm -r a; echo a > a").await;
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("a/b/c"), Some(&b"c1\n".to_vec()));
    assert_eq!(r.folder_after.len(), 1, "{:?}", r.folder_after.keys());
}

/// A file the turn turned into a directory, where the user's build then
/// wrote an ignored file: the undo would replace the directory by the
/// file and delete what no checkpoint holds. Unforced, it is refused,
/// naming the directory, and nothing changes.
#[tokio::test]
async fn an_unforced_undo_refuses_to_delete_files_no_checkpoint_holds() {
    let mut rig = Rig::new().await;
    rig.write(".gitignore", "*.o\n");
    rig.write("d", "d1\n");
    let t = rig.turn("rm d; mkdir d; echo y > d/f").await;
    assert_eq!(t.phases, ["before", "after"]);
    rig.write("d/cache.o", "build output\n");
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    let e = r.result.clone().unwrap_err();
    assert!(
        e.starts_with("nothing was changed:") && e.contains("`d`") && e.contains("no checkpoint"),
        "{e}"
    );
    assert_eq!(r.folder_after, r.folder_before);
    assert!(r.phases.is_empty(), "{:?}", r.phases);
    assert_eq!(
        r.folder_after.get("d/cache.o"),
        Some(&b"build output\n".to_vec()),
        "an ignored file was deleted by an unforced undo"
    );
}

/// The forced case: the undo proceeds, and the chat says which files were
/// deleted without being saved.
#[tokio::test]
async fn a_forced_undo_says_which_unsaved_files_it_deleted() {
    let mut rig = Rig::new().await;
    rig.write(".gitignore", "*.o\n");
    rig.write("d", "d1\n");
    let t = rig.turn("rm d; mkdir d; echo y > d/f").await;
    rig.write("d/cache.o", "build output\n");
    let mark = rig.seen.len();
    let r = rig.restore(t.id.as_deref().unwrap(), true).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("d"), Some(&b"d1\n".to_vec()));
    let notes = notes_since(&rig, mark);
    assert!(
        notes
            .iter()
            .any(|n| n.contains("`d/cache.o`") && n.contains("not saved")),
        "{notes:?}"
    );
}

/// A blob of a path the restore skips (inside a repository that appeared
/// in the folder after the turn) is neither needed nor checked: its loss
/// does not refuse the restore.
#[tokio::test]
async fn a_lost_blob_under_a_skipped_nested_repository_does_not_refuse_the_restore() {
    let mut rig = Rig::new().await;
    rig.git(&["init", "-q", "-b", "main"]);
    rig.write("other.txt", "base\n");
    rig.git(&["add", "."]);
    rig.git(&["commit", "-qm", "base"]);
    std::fs::create_dir(rig.ws.join("sub")).unwrap();
    rig.write("sub/keep.txt", X);
    rig.git(&["add", "."]);
    rig.git(&["commit", "-qm", "adds sub/keep.txt"]);
    let x = rig.git(&["hash-object", "sub/keep.txt"]);
    // Untracked, so the checkpoint's `sub` tree is its own (only the blob
    // is borrowed; a borrowed tree that is gone is refused at the diff).
    rig.write("sub/extra.txt", SCRATCH);
    // The agent removes the folder and prunes the history that held X in
    // the same turn, so X is gone before it could be copied (A.17).
    let t = rig
        .turn(&format!(
            "rm -r sub; {AGENT_GIT}; git commit -qa --amend --allow-empty -m amended && \
             git reflog expire --expire=now --all && git gc --prune=now -q"
        ))
        .await;
    assert_eq!(t.phases, ["before", "after"]);
    assert!(!rig.user_has(&x));
    assert!(!rig.shadow_owns(&x));
    // A repository appears where the turn removed the files.
    std::fs::create_dir(rig.ws.join("sub")).unwrap();
    rig.git_in(&rig.ws.join("sub"), &["init", "-q"]);
    rig.write("sub/mine.txt", "the other repository's work\n");
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("sub/keep.txt"), None, "written into it");
    assert_eq!(
        r.folder_after.get("sub/mine.txt"),
        Some(&b"the other repository's work\n".to_vec())
    );
}

/// Modes, symlinks, an empty file and a new file in a new directory.
#[tokio::test]
async fn undo_restores_modes_symlinks_and_empty_files() {
    use std::os::unix::fs::PermissionsExt;
    let mut rig = Rig::new().await;
    rig.write("run.sh", "#!/bin/sh\n");
    set_mode(&rig.ws.join("run.sh"), 0o755);
    rig.write("a.txt", "a\n");
    std::os::unix::fs::symlink("a.txt", rig.ws.join("link")).unwrap();
    std::os::unix::fs::symlink("nowhere", rig.ws.join("dangling")).unwrap();
    rig.write("empty", "");
    let t = rig
        .turn("chmod 644 run.sh; rm link; echo z > link; rm dangling; rm empty; mkdir -p 'new dir' && echo n > 'new dir/x'")
        .await;
    assert_eq!(t.phases, ["before", "after"]);
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_eq!(r.result, Ok(()));
    let m = std::fs::metadata(rig.ws.join("run.sh"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(m & 0o111, 0o111, "exec bit lost: {m:o}");
    assert_eq!(
        std::fs::read_link(rig.ws.join("link")).unwrap(),
        Path::new("a.txt")
    );
    assert_eq!(
        std::fs::read_link(rig.ws.join("dangling")).unwrap(),
        Path::new("nowhere")
    );
    assert_eq!(rig.read("empty").as_deref(), Some(""));
    assert!(!rig.ws.join("new dir/x").exists());
}
