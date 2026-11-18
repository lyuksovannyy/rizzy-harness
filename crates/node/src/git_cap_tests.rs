//! The size cap on files git does not track (docs/undo-and-stash.md §2.7):
//! an untracked file over `[limits] checkpoint_untracked_max_kb` is left out
//! of a snapshot, the snapshot records it (path and size), and no restore
//! deletes, overwrites or miscounts a file a snapshot left out. Every row of
//! the table in §2.7 is here, unforced and forced. The cap is a few KiB, so
//! no test writes more than a few MiB.

use super::*;

/// The cap every test uses unless it says otherwise.
const CAP: u64 = 4096;

fn sh(dir: &Path, args: &[&str]) -> String {
    let o = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("XDG_CONFIG_HOME", "/nonexistent")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {o:?}");
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// A repository with a committed `ctl.txt` (the control every restore must
/// put back) and a committed file over the cap (`tracked.bin`).
struct Rig {
    _d: tempfile::TempDir,
    dir: PathBuf,
    base: PathBuf,
}

fn rig() -> Rig {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().canonicalize().unwrap().join("repo");
    std::fs::create_dir_all(&dir).unwrap();
    sh(&dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("ctl.txt"), "ctl0\n").unwrap();
    std::fs::write(dir.join("tracked.bin"), vec![b't'; 3 * CAP as usize]).unwrap();
    sh(&dir, &["add", "."]);
    sh(&dir, &["commit", "-qm", "init"]);
    Rig {
        base: d.path().canonicalize().unwrap().join("checkpoints"),
        _d: d,
        dir,
    }
}

impl Rig {
    async fn repo(&self, cap: u64) -> Repo {
        let bounds = Bounds {
            untracked_max: cap,
            ..Default::default()
        };
        checkpoint_repo(&self.dir, Some(&self.base), &bounds)
            .await
            .unwrap()
            .unwrap()
    }

    fn put(&self, name: &str, state: &State) {
        let p = self.dir.join(name);
        match state {
            State::Absent => {
                let _ = std::fs::remove_file(&p);
            }
            State::Small(t) => std::fs::write(&p, t).unwrap(),
            State::Big(n, b) => std::fs::write(&p, vec![*b; *n]).unwrap(),
        }
    }

    fn get(&self, name: &str) -> Option<Vec<u8>> {
        std::fs::read(self.dir.join(name)).ok()
    }
}

#[derive(Debug, Clone)]
enum State {
    Absent,
    Small(&'static str),
    /// Over the cap: `n` bytes of one byte value.
    Big(usize, u8),
}

impl State {
    fn bytes(&self) -> Option<Vec<u8>> {
        match self {
            Self::Absent => None,
            Self::Small(t) => Some(t.as_bytes().to_vec()),
            Self::Big(n, b) => Some(vec![*b; *n]),
        }
    }
}

/// What a restore left, as (name, its earlier version is lost); a file
/// that was removed or not deleted for another reason fails the caller's
/// comparison (see [`kinds`]).
fn names(left: &[LeftAlone]) -> Vec<(String, bool)> {
    left.iter()
        .map(|l| {
            assert!(matches!(l.kind, LeftKind::Lost | LeftKind::Kept), "{l:?}");
            (
                String::from_utf8_lossy(&l.path).into_owned(),
                l.kind == LeftKind::Lost,
            )
        })
        .collect()
}

fn kinds(left: &[LeftAlone]) -> Vec<(String, LeftKind)> {
    left.iter()
        .map(|l| (String::from_utf8_lossy(&l.path).into_owned(), l.kind))
        .collect()
}

/// The size of `path` in snapshot `sha`, if it holds it.
async fn size_in(repo: &Repo, sha: &str, path: &str) -> Option<u64> {
    run(repo, &["cat-file", "-s", &format!("{sha}:{path}")], None)
        .await
        .ok()
        .and_then(|s| s.parse().ok())
}

#[tokio::test]
async fn an_untracked_file_over_the_cap_is_left_out_and_recorded_and_a_tracked_one_is_not() {
    let r = rig();
    let repo = r.repo(CAP).await;
    r.put("big.bin", &State::Big(2 * CAP as usize, b'x'));
    r.put("small.txt", &State::Small("s\n"));
    // A link to the big file is a link, never skipped.
    std::os::unix::fs::symlink("big.bin", r.dir.join("link")).unwrap();
    // A sparse file counts at its apparent size (what git would read).
    let f = std::fs::File::create(r.dir.join("sparse.img")).unwrap();
    f.set_len(1 << 20).unwrap();
    drop(f);
    // Exactly the cap is kept.
    r.put("edge.bin", &State::Big(CAP as usize, b'e'));
    let snap = snapshot_full(&repo, "refs/harness/s/t/before", "before")
        .await
        .unwrap();
    assert_eq!(
        snap.skipped.files,
        vec![
            (b"sparse.img".to_vec(), 1 << 20),
            (b"big.bin".to_vec(), 2 * CAP)
        ],
        "largest first"
    );
    assert_eq!(snap.skipped.more, 0);
    for kept in ["small.txt", "tracked.bin", "link", "edge.bin", "ctl.txt"] {
        assert!(
            size_in(&repo, &snap.sha, kept).await.is_some(),
            "{kept} must be in the snapshot"
        );
    }
    assert!(size_in(&repo, &snap.sha, "big.bin").await.is_none());
    assert!(size_in(&repo, &snap.sha, "sparse.img").await.is_none());
    // The record reads back from the commit.
    assert_eq!(skipped_in(&repo, &snap.sha).await, snap.skipped);
}

#[tokio::test]
async fn the_cap_at_zero_captures_everything_and_records_nothing() {
    let r = rig();
    let repo = r.repo(0).await;
    r.put("big.bin", &State::Big(2 * CAP as usize, b'x'));
    let snap = snapshot_full(&repo, "refs/harness/s/t/before", "before")
        .await
        .unwrap();
    assert_eq!(size_in(&repo, &snap.sha, "big.bin").await, Some(2 * CAP));
    assert_eq!(snap.skipped, Skipped::default());
    let msg = run(&repo, &["cat-file", "commit", &snap.sha], None)
        .await
        .unwrap();
    assert!(!msg.contains("harness-skipped"), "{msg}");
}

/// A file already in the shadow's index (captured by an earlier snapshot)
/// that grows over the cap is taken out, not restaged.
#[tokio::test]
async fn a_captured_file_that_grows_over_the_cap_is_left_out_of_the_next_snapshot() {
    let r = rig();
    let repo = r.repo(CAP).await;
    r.put("f.bin", &State::Small("small\n"));
    let one = snapshot(&repo, "refs/harness/s/a/before", "a")
        .await
        .unwrap();
    assert!(size_in(&repo, &one, "f.bin").await.is_some());
    r.put("f.bin", &State::Big(2 * CAP as usize, b'g'));
    let two = snapshot_full(&repo, "refs/harness/s/a/after", "a")
        .await
        .unwrap();
    assert!(size_in(&repo, &two.sha, "f.bin").await.is_none());
    assert_eq!(two.skipped.files, vec![(b"f.bin".to_vec(), 2 * CAP)]);
    // And back under it: captured again.
    r.put("f.bin", &State::Small("again\n"));
    let three = snapshot_full(&repo, "refs/harness/s/b/before", "b")
        .await
        .unwrap();
    assert!(size_in(&repo, &three.sha, "f.bin").await.is_some());
    assert!(three.skipped.files.is_empty());
}

/// One row of the table: `f` before and after the turn, what the folder
/// holds after the undo, and what the plan says it left alone.
async fn row(before: State, after: State, force: bool) {
    let r = rig();
    let repo = r.repo(CAP).await;
    r.put("f", &before);
    let b = snapshot(&repo, "refs/harness/s/t/before", "before")
        .await
        .unwrap();
    std::fs::write(r.dir.join("ctl.txt"), "ctl1\n").unwrap();
    r.put("f", &after);
    let a = snapshot(&repo, "refs/harness/s/t/after", "after")
        .await
        .unwrap();
    let ctx = format!("{before:?} -> {after:?}, force {force}");
    let done = restore_report(&repo, &b, &a, force)
        .await
        .unwrap_or_else(|e| panic!("{ctx}: {e}"));
    assert_eq!(
        r.get("ctl.txt").as_deref(),
        Some(&b"ctl0\n"[..]),
        "{ctx}: control"
    );
    use State::*;
    let (want, left, restored): (Option<Vec<u8>>, Vec<(String, bool)>, bool) =
        match (&before, &after) {
            // As on main.
            (Small(_), Small(_)) | (Small(_), Absent) => (before.bytes(), vec![], true),
            (Absent, Small(_)) => (None, vec![], true),
            (Absent, Absent) => (None, vec![], false),
            // Grown over the cap during the turn: the earlier version is in
            // the checkpoint and goes back; the big one is saved first.
            (Small(_), Big(..)) => (before.bytes(), vec![], true),
            // Made over the cap during the turn: left, not deleted.
            (Absent, Big(..)) => (after.bytes(), vec![("f".into(), false)], false),
            // Its earlier version was never saved.
            (Big(..), Small(_)) | (Big(..), Big(..)) | (Big(..), Absent) => {
                (after.bytes(), vec![("f".into(), true)], false)
            }
        };
    assert_eq!(r.get("f"), want, "{ctx}: content");
    assert_eq!(names(&done.plan.left), left, "{ctx}: left alone");
    assert_eq!(
        done.files.iter().any(|p| p == "f"),
        restored,
        "{ctx}: counted as restored ({:?})",
        done.files
    );
    if let (Small(_), Big(n, _)) = (&before, &after) {
        // Undoing the undo can bring the big version back.
        assert_eq!(
            size_in(&repo, &done.now, "f").await,
            Some(*n as u64),
            "{ctx}: the pre-restore snapshot holds the version it replaced"
        );
    }
}

fn states() -> [State; 3] {
    [
        State::Small("b\n"),
        State::Absent,
        State::Big(2 * CAP as usize, b'B'),
    ]
}

fn after_states() -> [State; 3] {
    // Different from the "before" ones, and a different size when big.
    [
        State::Small("a\n"),
        State::Absent,
        State::Big(3 * CAP as usize, b'A'),
    ]
}

#[tokio::test]
async fn every_row_of_the_table_unforced() {
    for b in states() {
        for a in after_states() {
            row(b.clone(), a, false).await;
        }
    }
}

#[tokio::test]
async fn every_row_of_the_table_forced() {
    for b in states() {
        for a in after_states() {
            row(b.clone(), a, true).await;
        }
    }
}

/// Over the cap before and after with the same size: nothing to say (the
/// content may have changed; no checkpoint can tell).
#[tokio::test]
async fn skipped_on_both_sides_with_the_same_size_is_not_named() {
    let r = rig();
    let repo = r.repo(CAP).await;
    r.put("f", &State::Big(2 * CAP as usize, b'1'));
    let b = snapshot(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    r.put("f", &State::Big(2 * CAP as usize, b'2'));
    let a = snapshot(&repo, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    let done = restore_report(&repo, &b, &a, false).await.unwrap();
    assert!(done.plan.left.is_empty(), "{:?}", done.plan.left);
    assert_eq!(r.get("f"), Some(vec![b'2'; 2 * CAP as usize]));
}

/// After the turn the person grows a file the turn changed over the cap:
/// the pre-restore snapshot holds it all the same, so the unforced undo sees
/// the change and refuses; forced, it is overwritten and kept for undoing
/// the undo.
#[tokio::test]
async fn a_file_grown_over_the_cap_after_the_turn_is_seen_as_changed() {
    let r = rig();
    let repo = r.repo(CAP).await;
    r.put("f", &State::Small("b\n"));
    let b = snapshot(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    r.put("f", &State::Small("a\n"));
    let a = snapshot(&repo, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    r.put("f", &State::Big(2 * CAP as usize, b'P'));
    let e = restore_report(&repo, &b, &a, false).await.unwrap_err();
    assert!(e.contains("changed after the turn"), "{e}");
    assert_eq!(r.get("f"), Some(vec![b'P'; 2 * CAP as usize]));
    let done = restore_report(&repo, &b, &a, true).await.unwrap();
    assert_eq!(r.get("f").as_deref(), Some(&b"b\n"[..]));
    assert_eq!(size_in(&repo, &done.now, "f").await, Some(2 * CAP));
}

/// The turn grew a file over the cap; after it the person shrank it (or
/// replaced it): the size the turn's checkpoint recorded differs, so the
/// unforced undo refuses; forced puts the earlier version back and keeps the
/// person's.
#[tokio::test]
async fn a_file_grown_in_the_turn_and_changed_after_it_is_seen_as_changed() {
    let r = rig();
    let repo = r.repo(CAP).await;
    r.put("f", &State::Small("b\n"));
    let b = snapshot(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    r.put("f", &State::Big(2 * CAP as usize, b'A'));
    let a = snapshot(&repo, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    r.put("f", &State::Small("person\n"));
    let e = restore_report(&repo, &b, &a, false).await.unwrap_err();
    assert!(e.contains("changed after the turn"), "{e}");
    let done = restore_report(&repo, &b, &a, true).await.unwrap();
    assert_eq!(r.get("f").as_deref(), Some(&b"b\n"[..]));
    assert!(size_in(&repo, &done.now, "f").await.is_some());
}

/// The turn grew a file over the cap and the person deleted it after the
/// turn: nothing is lost by putting the earlier version back.
#[tokio::test]
async fn a_file_grown_in_the_turn_and_deleted_after_it_comes_back_as_before() {
    let r = rig();
    let repo = r.repo(CAP).await;
    r.put("f", &State::Small("b\n"));
    let b = snapshot(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    r.put("f", &State::Big(2 * CAP as usize, b'A'));
    let a = snapshot(&repo, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    r.put("f", &State::Absent);
    restore_report(&repo, &b, &a, false).await.unwrap();
    assert_eq!(r.get("f").as_deref(), Some(&b"b\n"[..]));
}

/// A file over the cap before the turn whose path the turn made a folder:
/// the folder's files are the turn's own and are removed; the old file
/// cannot come back, and the plan says so.
#[tokio::test]
async fn a_skipped_file_replaced_by_a_folder() {
    let r = rig();
    let repo = r.repo(CAP).await;
    r.put("f", &State::Big(2 * CAP as usize, b'B'));
    let b = snapshot(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    std::fs::remove_file(r.dir.join("f")).unwrap();
    std::fs::create_dir(r.dir.join("f")).unwrap();
    std::fs::write(r.dir.join("f/x"), "x\n").unwrap();
    let a = snapshot(&repo, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    let done = restore_report(&repo, &b, &a, false).await.unwrap();
    assert!(!r.dir.join("f/x").exists());
    assert_eq!(names(&done.plan.left), vec![("f".to_string(), true)]);
}

/// A file over the cap before the turn that the turn replaced by a link:
/// the link is captured, its path was skipped before, so it is left.
#[tokio::test]
async fn a_skipped_file_replaced_by_a_link_is_left() {
    for force in [false, true] {
        let r = rig();
        let repo = r.repo(CAP).await;
        r.put("f", &State::Big(2 * CAP as usize, b'B'));
        let b = snapshot(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        std::fs::remove_file(r.dir.join("f")).unwrap();
        std::os::unix::fs::symlink("ctl.txt", r.dir.join("f")).unwrap();
        let a = snapshot(&repo, "refs/harness/s/t/after", "a")
            .await
            .unwrap();
        let done = restore_report(&repo, &b, &a, force).await.unwrap();
        assert_eq!(
            std::fs::read_link(r.dir.join("f")).unwrap(),
            Path::new("ctl.txt"),
            "force {force}"
        );
        assert_eq!(names(&done.plan.left), vec![("f".to_string(), true)]);
        assert!(!done.files.iter().any(|p| p == "f"));
    }
}

/// Skipped before the turn, then `git add` by the person: tracked, so the
/// "after" snapshot holds it, and it reads as created by the turn. An undo
/// must not delete it.
#[tokio::test]
async fn a_skipped_file_the_person_then_adds_to_git_is_not_deleted() {
    for force in [false, true] {
        let r = rig();
        let repo = r.repo(CAP).await;
        r.put("f", &State::Big(2 * CAP as usize, b'B'));
        let b = snapshot_full(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        assert_eq!(b.skipped.files.len(), 1);
        sh(&r.dir, &["add", "f"]);
        let a = snapshot_full(&repo, "refs/harness/s/t/after", "a")
            .await
            .unwrap();
        assert!(a.skipped.files.is_empty(), "tracked now: captured");
        let done = restore_report(&repo, &b.sha, &a.sha, force).await.unwrap();
        assert_eq!(r.get("f"), Some(vec![b'B'; 2 * CAP as usize]));
        assert_eq!(names(&done.plan.left), vec![("f".to_string(), true)]);
    }
}

/// A committed file is captured whatever its size, before and after: an
/// undo of an edit to it works as on main.
#[tokio::test]
async fn an_edit_to_a_big_tracked_file_is_undone() {
    let r = rig();
    let repo = r.repo(CAP).await;
    let b = snapshot(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    std::fs::write(r.dir.join("tracked.bin"), vec![b'z'; 5 * CAP as usize]).unwrap();
    let a = snapshot(&repo, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    let done = restore_report(&repo, &b, &a, false).await.unwrap();
    assert_eq!(r.get("tracked.bin"), Some(vec![b't'; 3 * CAP as usize]));
    assert!(done.plan.left.is_empty());
}

/// Names with a line break, bytes that are not UTF-8 and a leading dash are
/// recorded byte for byte, and left alone by an undo.
#[tokio::test]
async fn odd_names_are_recorded_byte_exact() {
    use std::os::unix::ffi::OsStrExt;
    let r = rig();
    let repo = r.repo(CAP).await;
    let b = snapshot(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    let odd: [&[u8]; 3] = [b"new\nline.bin", b"\xff\xfe.bin", b"-rf.bin"];
    for (i, n) in odd.iter().enumerate() {
        std::fs::write(
            r.dir.join(std::ffi::OsStr::from_bytes(n)),
            vec![b'0' + i as u8; (2 + i) * CAP as usize],
        )
        .unwrap();
    }
    let a = snapshot_full(&repo, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    let mut got: Vec<Vec<u8>> = a.skipped.files.iter().map(|(p, _)| p.clone()).collect();
    got.sort();
    let mut want: Vec<Vec<u8>> = odd.iter().map(|n| n.to_vec()).collect();
    want.sort();
    assert_eq!(got, want);
    assert_eq!(skipped_in(&repo, &a.sha).await, a.skipped);
    let done = restore_report(&repo, &b, &a.sha, true).await.unwrap();
    for n in odd {
        assert!(r.dir.join(std::ffi::OsStr::from_bytes(n)).exists());
    }
    let mut left: Vec<Vec<u8>> = done.plan.left.iter().map(|l| l.path.clone()).collect();
    left.sort();
    assert_eq!(left, want);
}

/// An old checkpoint (no record: taken with no cap, as before this change)
/// restores exactly as before, also when the node now has a cap: the big
/// file the turn changed is in both snapshots and goes back.
#[tokio::test]
async fn an_old_checkpoint_without_a_record_restores_as_before() {
    let r = rig();
    let old = r.repo(0).await;
    r.put("f", &State::Big(2 * CAP as usize, b'1'));
    let b = snapshot(&old, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    r.put("f", &State::Big(3 * CAP as usize, b'2'));
    let a = snapshot(&old, "refs/harness/s/t/after", "a").await.unwrap();
    let now = r.repo(CAP).await;
    let done = restore_report(&now, &b, &a, false).await.unwrap();
    assert_eq!(r.get("f"), Some(vec![b'1'; 2 * CAP as usize]));
    assert!(done.files.iter().any(|p| p == "f"));
    assert!(done.plan.left.is_empty());
}

/// 10,000 files over the cap: the record keeps the largest 1,000 and counts
/// the rest; an undo of a turn beside them works; one the record no longer
/// names, grown in the turn, is never written over without the pre-restore
/// snapshot holding it.
#[tokio::test]
async fn ten_thousand_skipped_files_keep_the_record_bounded() {
    let r = rig();
    let cap = 1024u64;
    let repo = r.repo(cap).await;
    let deps = r.dir.join("deps");
    std::fs::create_dir(&deps).unwrap();
    for i in 0..10_000u32 {
        // Sizes 1,100..=1,199: never the smallest skipped file below.
        std::fs::write(
            deps.join(format!("{i:05}")),
            vec![b'd'; 1100 + (i % 100) as usize],
        )
        .unwrap();
    }
    r.put("f", &State::Small("b\n"));
    let t = std::time::Instant::now();
    let b = snapshot_full(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    eprintln!("snapshot of 10,000 skipped files: {:?}", t.elapsed());
    assert_eq!(b.skipped.files.len(), SKIPPED_RECORD_MAX);
    assert_eq!(b.skipped.more, 10_000 - SKIPPED_RECORD_MAX);
    assert!(b.skipped.files.iter().all(|(_, n)| *n >= 1100 + 90));
    let msg = run(&repo, &["cat-file", "commit", &b.sha], None)
        .await
        .unwrap();
    assert!(msg.len() < 200 * SKIPPED_RECORD_MAX, "{} bytes", msg.len());
    std::fs::write(r.dir.join("ctl.txt"), "ctl1\n").unwrap();
    // Just over the cap: the smallest skipped file, so not in the record.
    r.put("f", &State::Big(1025, b'A'));
    let a = snapshot_full(&repo, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    assert!(!a.skipped.files.iter().any(|(p, _)| p == b"f"));
    // Unrecorded: the unforced check cannot match the size, and refuses.
    let e = restore_report(&repo, &b.sha, &a.sha, false)
        .await
        .unwrap_err();
    assert!(e.contains("changed after the turn"), "{e}");
    let t = std::time::Instant::now();
    let done = restore_report(&repo, &b.sha, &a.sha, true).await.unwrap();
    eprintln!("forced undo beside 10,000 skipped files: {:?}", t.elapsed());
    assert_eq!(r.get("ctl.txt").as_deref(), Some(&b"ctl0\n"[..]));
    assert_eq!(r.get("f").as_deref(), Some(&b"b\n"[..]));
    assert_eq!(size_in(&repo, &done.now, "f").await, Some(1025));
    assert_eq!(std::fs::read_dir(&deps).unwrap().count(), 10_000);
}

/// A rewind (expected = the latest state, several turns later) follows the
/// same rules.
#[tokio::test]
async fn a_rewind_over_two_turns_leaves_a_file_made_over_the_cap() {
    let r = rig();
    let repo = r.repo(CAP).await;
    let b = snapshot(&repo, "refs/harness/s/t1/before", "b")
        .await
        .unwrap();
    r.put("made.bin", &State::Big(2 * CAP as usize, b'M'));
    let _a1 = snapshot(&repo, "refs/harness/s/t1/after", "a")
        .await
        .unwrap();
    std::fs::write(r.dir.join("ctl.txt"), "ctl2\n").unwrap();
    let a2 = snapshot(&repo, "refs/harness/s/t2/after", "a")
        .await
        .unwrap();
    let done = restore_report(&repo, &b, &a2, false).await.unwrap();
    assert_eq!(r.get("ctl.txt").as_deref(), Some(&b"ctl0\n"[..]));
    assert!(r.dir.join("made.bin").exists());
    assert_eq!(
        names(&done.plan.left),
        vec![("made.bin".to_string(), false)]
    );
}

/// A sub-agent's folder is filled from a capped base: the file over the cap
/// is not in it, and a child that leaves that path alone lands, the parent's
/// file untouched.
#[tokio::test]
async fn a_sub_agent_does_not_get_a_skipped_file_and_merge_back_leaves_it() {
    let r = rig();
    r.put("big.bin", &State::Big(2 * CAP as usize, b'P'));
    let bounds = Bounds {
        untracked_max: CAP,
        ..Default::default()
    };
    let sub = sub_repo(&r.dir, Some(&r.base), &bounds)
        .await
        .unwrap()
        .unwrap();
    let base = sub.base_full("c").await.unwrap();
    assert_eq!(base.skipped.files, vec![(b"big.bin".to_vec(), 2 * CAP)]);
    let folder = sub
        .make_folder("c", &base.sha, &sessions_of(&r.base))
        .await
        .unwrap();
    assert!(!folder.path.join("big.bin").exists());
    assert!(folder.path.join("tracked.bin").exists(), "tracked: copied");
    std::fs::write(folder.path.join("ctl.txt"), "child\n").unwrap();
    let result = sub.result(&folder, "c").await.unwrap();
    let applied = sub.apply(&base.sha, &result, "c").await.unwrap();
    assert_eq!(applied.written, 1);
    assert_eq!(r.get("ctl.txt").as_deref(), Some(&b"child\n"[..]));
    assert_eq!(r.get("big.bin"), Some(vec![b'P'; 2 * CAP as usize]));
    remove_folder(&folder).await;
}

/// A child that writes a path where the parent has a file over the cap (one
/// it never saw) is a conflict: nothing of it is written.
#[tokio::test]
async fn a_sub_agent_writing_over_a_skipped_file_is_a_conflict() {
    for small_now in [false, true] {
        let r = rig();
        r.put("big.bin", &State::Big(2 * CAP as usize, b'P'));
        let bounds = Bounds {
            untracked_max: CAP,
            ..Default::default()
        };
        let sub = sub_repo(&r.dir, Some(&r.base), &bounds)
            .await
            .unwrap()
            .unwrap();
        let base = sub.base_full("c").await.unwrap();
        let folder = sub
            .make_folder("c", &base.sha, &sessions_of(&r.base))
            .await
            .unwrap();
        std::fs::write(folder.path.join("big.bin"), "child's\n").unwrap();
        std::fs::write(folder.path.join("ctl.txt"), "child\n").unwrap();
        let result = sub.result(&folder, "c").await.unwrap();
        if small_now {
            // The parent's file shrank under the cap meanwhile: still one
            // the child never saw.
            r.put("big.bin", &State::Small("parent\n"));
        }
        let e = sub.apply(&base.sha, &result, "c").await.unwrap_err();
        match &e {
            NotApplied::Conflict(c) => {
                assert_eq!(c.len(), 1, "{c:?}");
                assert_eq!(c[0].0, "big.bin");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(r.get("ctl.txt").as_deref(), Some(&b"ctl0\n"[..]));
        if !small_now {
            assert_eq!(r.get("big.bin"), Some(vec![b'P'; 2 * CAP as usize]));
        }
        remove_folder(&folder).await;
    }
}

/// A link is never left out, however long its target (with a cap smaller
/// than the target's length).
#[tokio::test]
async fn a_link_is_never_left_out() {
    let r = rig();
    let repo = r.repo(1024).await;
    let target = "t".repeat(2000);
    std::os::unix::fs::symlink(&target, r.dir.join("link")).unwrap();
    let snap = snapshot_full(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    assert!(snap.skipped.files.is_empty(), "{:?}", snap.skipped);
    assert_eq!(size_in(&repo, &snap.sha, "link").await, Some(2000));
}

/// The second line: a restore whose snapshot of the folder was taken
/// without waiving the cap (as a sub-agent's merge-back takes it) never
/// writes over or deletes a file that snapshot left out, at the path, where
/// the path's folder would go, or below the path.
#[tokio::test]
async fn a_restore_plan_never_touches_what_the_folders_snapshot_left_out() {
    // (path in the checkpoints, what the folder holds there now)
    for (path, big_at) in [("f", "f"), ("d/x", "d"), ("e", "e/big")] {
        let r = rig();
        let repo = r.repo(CAP).await;
        let full = r.dir.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, "b\n").unwrap();
        let b = snapshot(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        std::fs::write(&full, "a\n").unwrap();
        let a = snapshot(&repo, "refs/harness/s/t/after", "a")
            .await
            .unwrap();
        // The folder now: a big untracked file at, above or below the path.
        if path == "d/x" {
            std::fs::remove_dir_all(r.dir.join("d")).unwrap();
        } else if big_at == "e/big" {
            std::fs::remove_file(&full).unwrap();
            std::fs::create_dir(&full).unwrap();
        }
        std::fs::write(r.dir.join(big_at), vec![b'N'; 2 * CAP as usize]).unwrap();
        let diff = restore_diff(&repo, &b, &a).await.unwrap();
        let now = snapshot_full(&repo, "refs/harness/s/n/before", "now")
            .await
            .unwrap();
        assert_eq!(now.skipped.files.len(), 1, "{path}");
        let plan = restore_plan(&repo, &diff, &now.sha, false).await.unwrap();
        assert!(plan.touched.is_empty(), "{path}: {:?}", plan.touched);
        assert_eq!(plan.left.len(), 1, "{path}: {:?}", plan.left);
        assert_eq!(
            std::fs::read(r.dir.join(big_at)).unwrap(),
            vec![b'N'; 2 * CAP as usize]
        );
    }
}

/// A child that puts a folder where the parent has a file over the cap, or
/// a file where the parent has a folder holding one, is a conflict for that
/// reason.
#[tokio::test]
async fn a_sub_agent_writing_above_or_below_a_skipped_file_is_a_conflict() {
    for (parent_big, child_writes) in [("big.bin", "big.bin/x"), ("d/big.bin", "d")] {
        let r = rig();
        std::fs::create_dir_all(r.dir.join(parent_big).parent().unwrap()).unwrap();
        r.put(parent_big, &State::Big(2 * CAP as usize, b'P'));
        let bounds = Bounds {
            untracked_max: CAP,
            ..Default::default()
        };
        let sub = sub_repo(&r.dir, Some(&r.base), &bounds)
            .await
            .unwrap()
            .unwrap();
        let base = sub.base_full("c").await.unwrap();
        let folder = sub
            .make_folder("c", &base.sha, &sessions_of(&r.base))
            .await
            .unwrap();
        let w = folder.path.join(child_writes);
        std::fs::create_dir_all(w.parent().unwrap()).unwrap();
        std::fs::write(&w, "child\n").unwrap();
        let result = sub.result(&folder, "c").await.unwrap();
        let e = sub.apply(&base.sha, &result, "c").await.unwrap_err();
        match &e {
            NotApplied::Conflict(c) => {
                assert_eq!(c.len(), 1, "{c:?}");
                assert!(c[0].1.contains("size limit"), "{child_writes}: {c:?}");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(r.get(parent_big), Some(vec![b'P'; 2 * CAP as usize]));
        remove_folder(&folder).await;
    }
}

/// A sub-agent's own result is not capped: a large file it makes is
/// brought back (its write limits bound it instead).
#[tokio::test]
async fn a_large_file_a_sub_agent_makes_is_brought_back() {
    let r = rig();
    let bounds = Bounds {
        untracked_max: CAP,
        ..Default::default()
    };
    let sub = sub_repo(&r.dir, Some(&r.base), &bounds)
        .await
        .unwrap()
        .unwrap();
    let base = sub.base_full("c").await.unwrap();
    let folder = sub
        .make_folder("c", &base.sha, &sessions_of(&r.base))
        .await
        .unwrap();
    std::fs::write(folder.path.join("made.bin"), vec![b'm'; 2 * CAP as usize]).unwrap();
    let result = sub.result(&folder, "c").await.unwrap();
    let applied = sub.apply(&base.sha, &result, "c").await.unwrap();
    assert_eq!(applied.written, 1);
    assert_eq!(r.get("made.bin"), Some(vec![b'm'; 2 * CAP as usize]));
    remove_folder(&folder).await;
}

/// When the user's index cannot be used in the shadow (a split index: its
/// shared part stays in `.git`), "tracked" falls back to `HEAD`'s tree: a
/// committed file over the cap is still checkpointed.
#[tokio::test]
async fn a_split_index_falls_back_to_head() {
    let r = rig();
    sh(&r.dir, &["update-index", "--split-index"]);
    assert!(
        std::fs::read_dir(r.dir.join(".git")).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("sharedindex.")),
        "git made a split index"
    );
    let repo = r.repo(CAP).await;
    std::fs::write(r.dir.join("tracked.bin"), vec![b'z'; 5 * CAP as usize]).unwrap();
    let snap = snapshot_full(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    assert!(snap.skipped.files.is_empty(), "{:?}", snap.skipped);
    assert_eq!(
        size_in(&repo, &snap.sha, "tracked.bin").await,
        Some(5 * CAP)
    );
}

// ---- review of PR 154 (proofs in .claude/handoff-2026-11-15/r-ckpt-cap/) ----

/// A sub-agent's merge, with the parent's `z` read-only so the write fails
/// part-way after `a.bin` (over the cap) was written: everything the child
/// wrote is put back (A.16). Review P11: the put-back's snapshot left the
/// large file out, so it stayed on disk (and a small parent file it
/// overwrote was not put back).
async fn failed_merge_puts_back(parent_has_small: bool, children: usize) {
    use std::os::unix::fs::PermissionsExt;
    let r = rig();
    if parent_has_small {
        std::fs::write(r.dir.join("a.bin"), "parent\n").unwrap();
    }
    let bounds = Bounds {
        untracked_max: CAP,
        ..Default::default()
    };
    let sub = sub_repo(&r.dir, Some(&r.base), &bounds)
        .await
        .unwrap()
        .unwrap();
    // A first child that lands: a large file of its own.
    if children == 2 {
        let base = sub.base_full("c0").await.unwrap();
        let f = sub
            .make_folder("c0", &base.sha, &sessions_of(&r.base))
            .await
            .unwrap();
        std::fs::write(f.path.join("first.bin"), vec![b'F'; 2 * CAP as usize]).unwrap();
        let result = sub.result(&f, "c0").await.unwrap();
        sub.apply(&base.sha, &result, "c0").await.unwrap();
        remove_folder(&f).await;
    }
    let base = sub.base_full("c").await.unwrap();
    let folder = sub
        .make_folder("c", &base.sha, &sessions_of(&r.base))
        .await
        .unwrap();
    std::fs::write(folder.path.join("a.bin"), vec![b'C'; 2 * CAP as usize]).unwrap();
    std::fs::create_dir(folder.path.join("z")).unwrap();
    std::fs::write(folder.path.join("z/b.txt"), "child b\n").unwrap();
    let result = sub.result(&folder, "c").await.unwrap();
    std::fs::create_dir(r.dir.join("z")).unwrap();
    std::fs::set_permissions(r.dir.join("z"), std::fs::Permissions::from_mode(0o555)).unwrap();
    let res = sub.apply(&base.sha, &result, "c").await;
    std::fs::set_permissions(r.dir.join("z"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let ctx = format!("parent small {parent_has_small}, children {children}");
    let e = res.expect_err(&ctx);
    assert!(e.nothing_written(), "{ctx}: {}", e.text());
    assert!(
        e.text().contains("put back as they were"),
        "{ctx}: {}",
        e.text()
    );
    let want = parent_has_small.then(|| b"parent\n".to_vec());
    assert_eq!(r.get("a.bin"), want, "{ctx}: a.bin");
    assert!(!r.dir.join("z/b.txt").exists());
    if children == 2 {
        assert_eq!(r.get("first.bin"), Some(vec![b'F'; 2 * CAP as usize]));
    }
    remove_folder(&folder).await;
}

#[tokio::test]
async fn a_failed_sub_agent_write_takes_back_a_large_file_it_made() {
    failed_merge_puts_back(false, 1).await;
}

#[tokio::test]
async fn a_failed_sub_agent_write_puts_back_a_small_file_it_made_large() {
    failed_merge_puts_back(true, 1).await;
}

#[tokio::test]
async fn a_failed_second_sub_agent_write_is_put_back_and_the_first_stays() {
    failed_merge_puts_back(false, 2).await;
    failed_merge_puts_back(true, 2).await;
}

/// Review P3: in a linked worktree (`.git` is a file) the node cannot tell
/// what git tracks, so nothing is left out: an edit to a large committed
/// file is undone as on main.
#[tokio::test]
async fn a_linked_worktree_leaves_nothing_out_and_undoes_a_large_tracked_file() {
    let r = rig();
    let wt = r.dir.parent().unwrap().join("wt");
    sh(&r.dir, &["worktree", "add", "-q", wt.to_str().unwrap()]);
    let bounds = Bounds {
        untracked_max: CAP,
        ..Default::default()
    };
    let repo = checkpoint_repo(&wt, Some(&r.base), &bounds)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(repo.untracked_max(), 0);
    std::fs::write(wt.join("untracked.bin"), vec![b'u'; 2 * CAP as usize]).unwrap();
    let b = snapshot_full(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    assert!(b.skipped.is_empty(), "{:?}", b.skipped);
    std::fs::write(wt.join("tracked.bin"), vec![b'Z'; 5 * CAP as usize]).unwrap();
    let a = snapshot_full(&repo, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    let done = restore_report(&repo, &b.sha, &a.sha, false).await.unwrap();
    assert_eq!(
        std::fs::read(wt.join("tracked.bin")).unwrap(),
        vec![b't'; 3 * CAP as usize]
    );
    assert!(done.plan.left.is_empty());
}

/// Neither the index (split) nor `HEAD` (packed-refs over 1 MiB, the loose
/// ref gone) can be read: tracking is unknown, so nothing is left out.
#[tokio::test]
async fn nothing_is_left_out_when_neither_the_index_nor_head_can_be_read() {
    let r = rig();
    sh(&r.dir, &["update-index", "--split-index"]);
    sh(&r.dir, &["pack-refs", "--all"]);
    let packed = r.dir.join(".git/packed-refs");
    let mut t = std::fs::read_to_string(&packed).unwrap();
    for i in 0..40_000 {
        t.push_str(&format!("{:040x} refs/tags/pad{i}\n", 1));
    }
    std::fs::write(&packed, t).unwrap();
    let _ = std::fs::remove_file(r.dir.join(".git/refs/heads/main"));
    let repo = r.repo(CAP).await;
    std::fs::write(r.dir.join("tracked.bin"), vec![b'Z'; 5 * CAP as usize]).unwrap();
    r.put("untracked.bin", &State::Big(2 * CAP as usize, b'u'));
    let snap = snapshot_full(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    assert!(snap.skipped.is_empty(), "{:?}", snap.skipped);
    assert_eq!(
        size_in(&repo, &snap.sha, "tracked.bin").await,
        Some(5 * CAP)
    );
}

/// A folder in no repository tracks nothing: the cap applies.
#[tokio::test]
async fn a_plain_folder_keeps_the_cap() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().canonicalize().unwrap().join("plain");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("big.bin"), vec![b'x'; 2 * CAP as usize]).unwrap();
    std::fs::write(dir.join("small"), "s\n").unwrap();
    let bounds = Bounds {
        untracked_max: CAP,
        ..Default::default()
    };
    let repo = checkpoint_repo(&dir, Some(&d.path().join("cp")), &bounds)
        .await
        .unwrap()
        .unwrap();
    let snap = snapshot_full(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    assert_eq!(snap.skipped.files, vec![(b"big.bin".to_vec(), 2 * CAP)]);
}

/// The skipped-record lines of snapshot `sha`, in bytes.
async fn record_bytes(repo: &Repo, sha: &str) -> usize {
    run(repo, &["cat-file", "commit", sha], None)
        .await
        .unwrap()
        .lines()
        .filter(|l| l.starts_with(SKIPPED_TRAILER))
        .map(|l| l.len() + 1)
        .sum()
}

/// Review P1: 1,000 skipped files with ordinary 70-byte paths put the
/// record over Linux's 128 KiB limit for one argument, and every snapshot
/// of the folder failed. Now the message goes on stdin and the record is
/// bounded in bytes; past the bound the rest is counted.
#[tokio::test]
async fn a_record_of_long_paths_is_bounded_in_bytes_and_never_an_argument() {
    let r = rig();
    let cap = 1024u64;
    let repo = r.repo(cap).await;
    let deps = r.dir.join("data/train/images/batch_0001");
    std::fs::create_dir_all(&deps).unwrap();
    for i in 0..1000u32 {
        std::fs::write(
            deps.join(format!("sample_{i:06}_augmented_rgb.tif")),
            vec![b'd'; 1100],
        )
        .unwrap();
    }
    let snap = snapshot_full(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    assert_eq!(snap.skipped.count(), 1000);
    assert!(record_bytes(&repo, &snap.sha).await <= SKIPPED_RECORD_BYTES);
    // Deep paths: far fewer fit the byte bound; the rest are counted.
    let deep = r.dir.join(format!("{}/x", "d".repeat(200)).repeat(8));
    std::fs::create_dir_all(&deep).unwrap();
    for i in 0..100u32 {
        std::fs::write(deep.join(format!("{i:03}")), vec![b'e'; 2000]).unwrap();
    }
    let snap = snapshot_full(&repo, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    assert_eq!(snap.skipped.count(), 1100);
    assert!(snap.skipped.more > 0);
    let bytes = record_bytes(&repo, &snap.sha).await;
    assert!(bytes <= SKIPPED_RECORD_BYTES, "{bytes}");
    assert!(bytes > SKIPPED_RECORD_BYTES - 4 * 1700, "{bytes}: filled");
    assert_eq!(skipped_in(&repo, &snap.sha).await, snap.skipped);
}

/// The longest path the folder allows, with bytes that are not UTF-8, over
/// the cap: recorded byte for byte, and left by an undo.
#[tokio::test]
async fn a_path_of_maximum_length_is_recorded_byte_exact() {
    use std::os::unix::ffi::OsStrExt;
    let r = rig();
    let repo = r.repo(CAP).await;
    let b = snapshot(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    // PATH_MAX is 4,096 with its NUL: the folder's own path takes the rest.
    let room = 4095 - r.dir.as_os_str().len() - 1;
    let mut rel: Vec<u8> = Vec::new();
    while room - rel.len() > 256 {
        rel.extend(std::iter::repeat_n(b'\xe9', 200));
        rel.push(b'/');
    }
    let last = room - rel.len();
    rel.extend(std::iter::repeat_n(b'\xff', last));
    assert_eq!(rel.len(), room);
    let full = r.dir.join(std::ffi::OsStr::from_bytes(&rel));
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(&full, vec![b'm'; 2 * CAP as usize]).unwrap();
    let a = snapshot_full(&repo, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    assert_eq!(a.skipped.files, vec![(rel.clone(), 2 * CAP)]);
    assert_eq!(skipped_in(&repo, &a.sha).await, a.skipped);
    assert!(record_bytes(&repo, &a.sha).await <= SKIPPED_RECORD_BYTES);
    let done = restore_report(&repo, &b, &a.sha, true).await.unwrap();
    assert!(full.exists());
    assert_eq!(kinds(&done.plan.left)[0].1, LeftKind::Kept);
}

/// Review P2: before, `d` is a file; the turn made `d/` with a large `d/big`
/// and `d/x`. The undo writes `d` back, which removes `d/big`: it is named as
/// removed (not "left"), and the snapshot taken first holds it.
#[tokio::test]
async fn a_large_file_below_a_path_the_undo_writes_is_named_removed_and_kept() {
    for force in [false, true] {
        let r = rig();
        let repo = r.repo(CAP).await;
        std::fs::write(r.dir.join("d"), "b\n").unwrap();
        let b = snapshot(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        std::fs::remove_file(r.dir.join("d")).unwrap();
        std::fs::create_dir(r.dir.join("d")).unwrap();
        std::fs::write(r.dir.join("d/big"), vec![b'B'; 2 * CAP as usize]).unwrap();
        std::fs::write(r.dir.join("d/x"), "x\n").unwrap();
        let a = snapshot(&repo, "refs/harness/s/t/after", "a")
            .await
            .unwrap();
        let done = restore_report(&repo, &b, &a, force).await.unwrap();
        assert_eq!(r.get("d").as_deref(), Some(&b"b\n"[..]));
        assert_eq!(
            kinds(&done.plan.left),
            vec![("d/big".to_string(), LeftKind::Removed)]
        );
        assert_eq!(size_in(&repo, &done.now, "d/big").await, Some(2 * CAP));
    }
}

/// Review P10: before, `d/` holds `d/x`; the turn replaced it by a large
/// file `d`. The undo puts `d/x` back, removing `d`: named as removed, and
/// undoing the undo brings the large file back.
#[tokio::test]
async fn a_large_file_above_a_path_the_undo_writes_is_named_removed_and_comes_back() {
    let r = rig();
    let repo = r.repo(CAP).await;
    std::fs::create_dir(r.dir.join("d")).unwrap();
    std::fs::write(r.dir.join("d/x"), "x\n").unwrap();
    let b = snapshot(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    std::fs::remove_dir_all(r.dir.join("d")).unwrap();
    std::fs::write(r.dir.join("d"), vec![b'B'; 2 * CAP as usize]).unwrap();
    let a = snapshot(&repo, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    let done = restore_report(&repo, &b, &a, false).await.unwrap();
    assert_eq!(r.get("d/x").as_deref(), Some(&b"x\n"[..]));
    assert_eq!(
        kinds(&done.plan.left),
        vec![("d".to_string(), LeftKind::Removed)]
    );
    assert_eq!(size_in(&repo, &done.now, "d").await, Some(2 * CAP));
    let here = snapshot(&repo, "refs/harness/s/u/after", "u")
        .await
        .unwrap();
    restore_report(&repo, &done.now, &here, false)
        .await
        .unwrap();
    assert_eq!(r.get("d"), Some(vec![b'B'; 2 * CAP as usize]));
}

/// 1,200 skipped `deps/` files beside `f`, the smallest of them (so the
/// record does not name it when it is over the cap).
async fn incomplete_rig(cap: u64) -> (Rig, Repo) {
    let r = rig();
    let repo = r.repo(cap).await;
    let deps = r.dir.join("deps");
    std::fs::create_dir(&deps).unwrap();
    for i in 0..1200u32 {
        std::fs::write(deps.join(format!("{i:05}")), vec![b'd'; 2000]).unwrap();
    }
    (r, repo)
}

/// Review P4: with more skipped files than the record names, a file over
/// the cap before the turn and not named, shrunk by the turn, was deleted
/// by an unforced undo with no note. Now it is not deleted, and named.
#[tokio::test]
async fn an_unnamed_large_file_the_turn_shrank_is_not_deleted() {
    let (r, repo) = incomplete_rig(1024).await;
    r.put("f", &State::Big(1025, b'P'));
    let b = snapshot_full(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    assert!(b.skipped.more > 0);
    assert!(!b.skipped.files.iter().any(|(p, _)| p == b"f"));
    r.put("f", &State::Small("a\n"));
    let a = snapshot(&repo, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    for force in [false, true] {
        let done = restore_report(&repo, &b.sha, &a, force).await.unwrap();
        assert_eq!(r.get("f").as_deref(), Some(&b"a\n"[..]), "force {force}");
        assert!(!done.files.iter().any(|p| p == "f"));
        assert_eq!(
            kinds(&done.plan.left),
            vec![("f".to_string(), LeftKind::Unsure)]
        );
    }
}

/// Every cell of the table with the checkpoint's record incomplete (1,200
/// skipped, `f` the smallest): whatever the undo changes on disk, the
/// snapshot it took first holds as it was, and nothing the turn found on
/// disk unnamed is deleted.
#[tokio::test]
async fn with_an_incomplete_record_no_cell_loses_a_file() {
    let small_b = State::Small("b\n");
    let big_b = State::Big(1025, b'B');
    let small_a = State::Small("a\n");
    let big_a = State::Big(1030, b'A');
    for before in [small_b.clone(), State::Absent, big_b.clone()] {
        for after in [small_a.clone(), State::Absent, big_a.clone()] {
            for force in [false, true] {
                let ctx = format!("{before:?} -> {after:?}, force {force}");
                let (r, repo) = incomplete_rig(1024).await;
                r.put("f", &before);
                let b = snapshot(&repo, "refs/harness/s/t/before", "b")
                    .await
                    .unwrap();
                r.put("f", &after);
                let a = snapshot(&repo, "refs/harness/s/t/after", "a")
                    .await
                    .unwrap();
                let on_disk = r.get("f");
                let done = match restore_report(&repo, &b, &a, force).await {
                    Ok(d) => d,
                    Err(e) => {
                        // A refusal changes nothing.
                        assert!(!force, "{ctx}: {e}");
                        assert_eq!(r.get("f"), on_disk, "{ctx}");
                        continue;
                    }
                };
                let now = r.get("f");
                if now != on_disk
                    && let Some(was) = &on_disk
                {
                    let held = run_raw(
                        &repo,
                        &["cat-file", "blob", &format!("{}:f", done.now)],
                        None,
                        None,
                    )
                    .await
                    .unwrap_or_default();
                    assert_eq!(&held, was, "{ctx}: changed and not saved first");
                }
                // A file over the cap before the turn is never deleted.
                if matches!(before, State::Big(..)) && on_disk.is_some() {
                    assert!(now.is_some(), "{ctx}: deleted");
                }
                assert_eq!(std::fs::read_dir(r.dir.join("deps")).unwrap().count(), 1200);
            }
        }
    }
}

/// Review P9: names git would quote (non-ASCII, not UTF-8) grown over the
/// cap in the turn pass the unforced check (matched as bytes) and are
/// undone; changed after it, they are refused and named readably.
#[tokio::test]
async fn non_ascii_names_are_matched_as_bytes_by_the_unforced_check() {
    use std::os::unix::ffi::OsStrExt;
    for name in [&b"\xc3\xa9.bin"[..], &b"\xff\xfe.bin"[..]] {
        let r = rig();
        let repo = r.repo(CAP).await;
        let p = r.dir.join(std::ffi::OsStr::from_bytes(name));
        std::fs::write(&p, "b\n").unwrap();
        let b = snapshot(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        std::fs::write(&p, vec![b'B'; 2 * CAP as usize]).unwrap();
        let a = snapshot(&repo, "refs/harness/s/t/after", "a")
            .await
            .unwrap();
        restore_report(&repo, &b, &a, false).await.unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"b\n");
        // Changed after the turn: refused, the name as a person reads it.
        let r = rig();
        let repo = r.repo(CAP).await;
        let p = r.dir.join(std::ffi::OsStr::from_bytes(name));
        std::fs::write(&p, "b\n").unwrap();
        let b = snapshot(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        std::fs::write(&p, "a\n").unwrap();
        let a = snapshot(&repo, "refs/harness/s/t/after", "a")
            .await
            .unwrap();
        std::fs::write(&p, "person\n").unwrap();
        let e = restore_report(&repo, &b, &a, false).await.unwrap_err();
        assert!(e.contains(&shown(name)), "{e}");
        assert!(!e.contains("\\3"), "{e}");
    }
}

/// Review P8: a child writing where the parent has a file over the cap that
/// the base's record does not name is a conflict for that reason, not "a
/// file git ignores".
#[tokio::test]
async fn an_unnamed_skipped_file_in_the_way_of_a_child_is_named_for_its_size() {
    let r = rig();
    let deps = r.dir.join("deps");
    std::fs::create_dir(&deps).unwrap();
    for i in 0..1200u32 {
        std::fs::write(deps.join(format!("{i:05}")), vec![b'd'; 2000]).unwrap();
    }
    r.put("p.bin", &State::Big(1025, b'P'));
    let bounds = Bounds {
        untracked_max: 1024,
        ..Default::default()
    };
    let sub = sub_repo(&r.dir, Some(&r.base), &bounds)
        .await
        .unwrap()
        .unwrap();
    let base = sub.base_full("c").await.unwrap();
    assert!(!base.skipped.files.iter().any(|(p, _)| p == b"p.bin"));
    let folder = sub
        .make_folder("c", &base.sha, &sessions_of(&r.base))
        .await
        .unwrap();
    std::fs::write(folder.path.join("p.bin"), "child\n").unwrap();
    let result = sub.result(&folder, "c").await.unwrap();
    match sub.apply(&base.sha, &result, "c").await {
        Err(NotApplied::Conflict(c)) => {
            assert_eq!(c.len(), 1, "{c:?}");
            assert!(c[0].1.contains("size limit"), "{c:?}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(r.get("p.bin"), Some(vec![b'P'; 1025]));
    remove_folder(&folder).await;
}

/// L4: hidden and bidi characters in a name reach a note by code point.
#[test]
fn hidden_characters_in_a_name_are_shown_by_code_point() {
    assert_eq!(
        shown("a\u{202E}b\u{200B}c\n".as_bytes()),
        "a<U+202E>b<U+200B>c\u{FFFD}"
    );
    let s = Skipped {
        files: vec![("x\u{202E}y".as_bytes().to_vec(), 2048)],
        more: 0,
        more_bytes: 0,
    };
    assert!(s.listed(5).contains("x<U+202E>y"), "{}", s.listed(5));
}

/// Review M3: what deciding "tracked" costs with a user index of 200,000
/// paths and one large untracked file: the copy is streamed and kept, git
/// is asked about the one candidate only. The old way (read the whole
/// index into memory, write it, list every path, build a set) is timed
/// beside it. Ignored; run with `--ignored --nocapture`.
#[tokio::test]
#[ignore]
async fn tracked_lookup_cost_with_a_large_index() {
    let r = rig();
    let blob = {
        let o = std::process::Command::new("git")
            .arg("-C")
            .arg(&r.dir)
            .args(["hash-object", "-w", "/dev/null"])
            .output()
            .unwrap();
        String::from_utf8(o.stdout).unwrap().trim().to_string()
    };
    let mut info = Vec::new();
    for i in 0..200_000u32 {
        info.extend(format!("100644 {blob}\tsrc/m{:03}/f{i:06}.rs\0", i % 500).into_bytes());
    }
    let mut c = std::process::Command::new("git")
        .arg("-C")
        .arg(&r.dir)
        .args(["update-index", "-z", "--index-info"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    c.stdin.take().unwrap().write_all(&info).unwrap();
    assert!(c.wait().unwrap().success());
    let index_len = std::fs::metadata(r.dir.join(".git/index")).unwrap().len();
    r.put("big.bin", &State::Big(2 * CAP as usize, b'x'));
    let repo = r.repo(CAP).await;
    for run_no in 1..=3 {
        let t = std::time::Instant::now();
        let got = user_tracked(&repo, &[b"big.bin".to_vec()]).await.unwrap();
        eprintln!(
            "new, call {run_no}: {:?} ({} found; index {} KiB)",
            t.elapsed(),
            got.len(),
            index_len >> 10
        );
    }
    let t = std::time::Instant::now();
    let snap = snapshot_full(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    eprintln!("snapshot with the lookup: {:?}", t.elapsed());
    assert_eq!(snap.skipped.files.len(), 1);
    // The old way, as PR 154's first version did it.
    for run_no in 1..=3 {
        let t = std::time::Instant::now();
        let bytes = user_repo::read_bounded(&r.dir.join(".git/index"), 256 << 20).unwrap();
        let held = bytes.len();
        let tmp = repo.shadow.clone().unwrap().join("old-index-copy");
        std::fs::write(&tmp, &bytes).unwrap();
        drop(bytes);
        let out = run_raw(&repo, &["ls-files", "-z"], Some(&tmp), None)
            .await
            .unwrap();
        let set: std::collections::HashSet<Vec<u8>> = out
            .split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .map(<[u8]>::to_vec)
            .collect();
        eprintln!(
            "old, call {run_no}: {:?} ({} paths in a set, {} KiB held, {} KiB listed)",
            t.elapsed(),
            set.len(),
            held >> 10,
            out.len() >> 10
        );
        let _ = std::fs::remove_file(&tmp);
    }
}

/// With an incomplete record, a file the turn made and `git add`ed is
/// tracked, so never one the checkpoint left out: the undo deletes it.
#[tokio::test]
async fn with_an_incomplete_record_a_tracked_new_file_is_still_deleted() {
    let (r, repo) = incomplete_rig(1024).await;
    let b = snapshot_full(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    assert!(b.skipped.more > 0);
    r.put("new.txt", &State::Small("n\n"));
    r.put("loose.txt", &State::Small("l\n"));
    sh(&r.dir, &["add", "new.txt"]);
    let a = snapshot(&repo, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    let done = restore_report(&repo, &b.sha, &a, false).await.unwrap();
    assert!(!r.dir.join("new.txt").exists());
    assert!(r.dir.join("loose.txt").exists());
    assert_eq!(
        kinds(&done.plan.left),
        vec![("loose.txt".to_string(), LeftKind::Unsure)]
    );
}

/// The failed merge of P11, and putting back fails too: the outcome names
/// the large file that holds the child's version (the snapshot that finds
/// it waives the cap).
#[tokio::test]
async fn a_failed_put_back_names_a_large_file_it_left() {
    use std::os::unix::fs::PermissionsExt;
    let r = rig();
    let bounds = Bounds {
        untracked_max: CAP,
        ..Default::default()
    };
    let sub = sub_repo(&r.dir, Some(&r.base), &bounds)
        .await
        .unwrap()
        .unwrap();
    let base = sub.base_full("c").await.unwrap();
    let folder = sub
        .make_folder("c", &base.sha, &sessions_of(&r.base))
        .await
        .unwrap();
    std::fs::write(folder.path.join("a.bin"), vec![b'C'; 2 * CAP as usize]).unwrap();
    std::fs::create_dir(folder.path.join("z")).unwrap();
    std::fs::write(folder.path.join("z/b.txt"), "child b\n").unwrap();
    let result = sub.result(&folder, "c").await.unwrap();
    std::fs::create_dir(r.dir.join("z")).unwrap();
    std::fs::set_permissions(r.dir.join("z"), std::fs::Permissions::from_mode(0o555)).unwrap();
    let top = r.dir.clone();
    let lock =
        move || std::fs::set_permissions(&top, std::fs::Permissions::from_mode(0o555)).unwrap();
    let res = sub
        .apply_with(
            &base.sha,
            &result,
            "c",
            merge::Hooks {
                before_put_back: Some(&lock),
                ..Default::default()
            },
        )
        .await;
    std::fs::set_permissions(&r.dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(r.dir.join("z"), std::fs::Permissions::from_mode(0o755)).unwrap();
    match &res {
        Err(NotApplied::Partial { written, .. }) => {
            assert_eq!(written.shown, vec!["a.bin".to_string()], "{res:?}");
        }
        other => panic!("{other:?}"),
    }
    remove_folder(&folder).await;
}

/// Each lookup has a copy of the index of its own, gone when it is done
/// (review 2, RV7-RACE: a shared copy one snapshot removed under another's
/// `ls-files` read a tracked file as untracked).
#[tokio::test]
async fn each_lookup_copies_the_index_for_itself_and_removes_it() {
    let r = rig();
    let repo = r.repo(CAP).await;
    let one = user_index_copy(&repo).await.unwrap();
    let two = user_index_copy(&repo).await.unwrap();
    assert_ne!(one.0, two.0);
    let p = one.0.clone();
    drop(one);
    assert!(!p.exists());
    assert!(two.0.is_file());
    let shadow = repo.shadow.clone().unwrap();
    drop(two);
    user_tracked(&repo, &[b"tracked.bin".to_vec()])
        .await
        .unwrap();
    let left: Vec<String> = std::fs::read_dir(&shadow)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("harness-index-"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

/// RV7-RACE for real: one task keeps rewriting the index, another keeps
/// asking whether a tracked file is tracked. It is never read as untracked
/// (an answer may be "unknown", which skips nothing).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lookup_racing_index_rewrites_never_reads_a_tracked_file_as_untracked() {
    let r = rig();
    let repo = std::sync::Arc::new(r.repo(CAP).await);
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (rp, st, dir) = (repo.clone(), stop.clone(), r.dir.clone());
    let writer = tokio::spawn(async move {
        let mut i = 0u32;
        while !st.load(std::sync::atomic::Ordering::Relaxed) {
            std::fs::write(
                dir.join("n.txt"),
                format!("{i}\n").repeat(i as usize % 7 + 1),
            )
            .unwrap();
            sh(&dir, &["add", "n.txt"]);
            let _ = user_tracked(&rp, &[b"n.txt".to_vec()]).await;
            i += 1;
        }
        i
    });
    let (mut wrong, mut total) = (0, 0);
    let t = std::time::Instant::now();
    while t.elapsed() < std::time::Duration::from_secs(3) {
        if let Some(s) = user_tracked(&repo, &[b"tracked.bin".to_vec()]).await
            && !s.contains(b"tracked.bin".as_slice())
        {
            wrong += 1;
        }
        total += 1;
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let writes = writer.await.unwrap();
    assert!(
        total > 10 && writes > 10,
        "{total} lookups, {writes} writes"
    );
    assert_eq!(wrong, 0, "a tracked file read as untracked, of {total}");
}

// ---- second review of PR 154 (proofs in .claude/handoff-2026-11-15/r-ckpt-cap-2/) ----

/// NEW-1 (RV7-CHECK): the turn deletes the file `d`; afterwards the person
/// makes `d/new`. An unforced undo, which would write `d`, refuses, with
/// the cap on and off, as on main (git's pathspec `d` matches `d/new`).
#[tokio::test]
async fn an_unforced_undo_refuses_over_a_new_file_below_a_path_it_writes() {
    for cap in [0, CAP] {
        let r = rig();
        let repo = r.repo(cap).await;
        std::fs::write(r.dir.join("d"), "b\n").unwrap();
        let b = snapshot(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        std::fs::remove_file(r.dir.join("d")).unwrap();
        let a = snapshot(&repo, "refs/harness/s/t/after", "a")
            .await
            .unwrap();
        std::fs::create_dir(r.dir.join("d")).unwrap();
        std::fs::write(r.dir.join("d/new"), "person's work\n").unwrap();
        let e = restore_report(&repo, &b, &a, false).await.unwrap_err();
        assert!(
            e.contains("changed after the turn") && e.contains("d/new"),
            "cap {cap}: {e}"
        );
        assert_eq!(r.get("d/new").as_deref(), Some(&b"person's work\n"[..]));
    }
}

/// NEW-1's equivalence, with git as the oracle: on random small trees and
/// change sets (files, folders, file and folder swaps, non-ASCII and odd
/// names), `changed_under` with runs forced small answers exactly what one
/// `git diff --name-only -z --no-renames <a> <b> -- <literal pathspecs>`
/// answers.
#[tokio::test]
async fn changed_under_answers_as_git_pathspecs_do() {
    let names: [&[u8]; 11] = [
        b"a",
        b"b",
        b"a/x",
        b"a/y",
        b"b/x",
        b"\xc3\xa9",
        b"\xc3\xa9/z",
        b"-n",
        b"s p/q",
        b"x*",
        b"x1",
    ];
    let r = rig();
    let repo = r.repo(0).await;
    let mut x: u64 = 0x2545_f491;
    let mut next = |n: u64| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x % n
    };
    use std::os::unix::ffi::OsStrExt;
    let tree = |r: &Rig, picks: &[usize]| {
        for e in std::fs::read_dir(&r.dir).unwrap() {
            let e = e.unwrap();
            if e.file_name() != ".git" {
                let p = e.path();
                if p.is_dir() {
                    std::fs::remove_dir_all(p).unwrap();
                } else {
                    std::fs::remove_file(p).unwrap();
                }
            }
        }
        for (k, i) in picks.iter().enumerate() {
            let p = r.dir.join(std::ffi::OsStr::from_bytes(names[*i]));
            if p.parent().is_some_and(|q| q.is_file()) || p.is_dir() {
                continue;
            }
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, format!("{k}\n")).unwrap();
        }
    };
    for case in 0..300 {
        let a_picks: Vec<usize> = (0..next(5)).map(|_| next(11) as usize).collect();
        let b_picks: Vec<usize> = (0..next(5)).map(|_| next(11) as usize).collect();
        tree(&r, &a_picks);
        let a = snapshot(&repo, "refs/harness/s/o/a", "a").await.unwrap();
        tree(&r, &b_picks);
        let b = snapshot(&repo, "refs/harness/s/o/b", "b").await.unwrap();
        let specs: Vec<Vec<u8>> = (0..1 + next(4))
            .map(|_| names[next(11) as usize].to_vec())
            .collect();
        let mut c = command(&repo, None);
        c.env("GIT_LITERAL_PATHSPECS", "1").args([
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--name-only",
            "-z",
            &a,
            &b,
            "--",
        ]);
        for p in &specs {
            c.arg(rel_path(p));
        }
        let o = exec(c, None, GIT_TIMEOUT).await.unwrap();
        let mut want: Vec<Vec<u8>> = o
            .stdout
            .split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .map(<[u8]>::to_vec)
            .collect();
        let mut got = changed_under(&repo, &a, &b, &specs, 1).await.unwrap();
        want.sort();
        want.dedup();
        got.sort();
        assert_eq!(got, want, "case {case}: specs {specs:?}");
    }
}

/// NEW-2: every layout, whether the cap is on, and that a TRACKED file over
/// the cap is never left out in any of them.
#[tokio::test]
async fn the_cap_applies_only_where_the_node_read_the_index_itself() {
    use std::os::unix::fs::PermissionsExt;
    struct Case {
        name: &'static str,
        /// Whether an untracked file over the cap is left out.
        cap_on: bool,
    }
    let cases = [
        Case {
            name: "plain folder",
            cap_on: true,
        },
        Case {
            name: "repository at the root",
            cap_on: true,
        },
        Case {
            name: "workspace root inside a repository",
            cap_on: false,
        },
        Case {
            name: "linked worktree",
            cap_on: false,
        },
        Case {
            name: "submodule-style gitfile",
            cap_on: false,
        },
        Case {
            name: "split index",
            cap_on: false,
        },
        Case {
            name: "no index yet",
            cap_on: false,
        },
        Case {
            name: "no commit yet",
            cap_on: true,
        },
        Case {
            name: "index unreadable",
            cap_on: false,
        },
        Case {
            name: "index gone after the folder was looked at",
            cap_on: false,
        },
    ];
    for c in cases {
        let d = tempfile::tempdir().unwrap();
        let top = d.path().canonicalize().unwrap();
        let repo_dir = top.join("repo");
        std::fs::create_dir(&repo_dir).unwrap();
        let mut ceiling = None;
        let mut folder = repo_dir.clone();
        let mut tracked: Option<PathBuf> = None;
        let big = vec![b'T'; 3 * CAP as usize];
        match c.name {
            "plain folder" => {}
            "linked worktree" => {
                sh(&repo_dir, &["init", "-q", "-b", "main"]);
                std::fs::write(repo_dir.join("t.bin"), &big).unwrap();
                sh(&repo_dir, &["add", "."]);
                sh(&repo_dir, &["commit", "-qm", "c"]);
                folder = top.join("wt");
                sh(
                    &repo_dir,
                    &["worktree", "add", "-q", folder.to_str().unwrap()],
                );
                tracked = Some(folder.join("t.bin"));
            }
            "submodule-style gitfile" => {
                let real = top.join("real.git");
                std::fs::create_dir(&real).unwrap();
                sh(&real, &["init", "-q", "--bare"]);
                std::fs::write(
                    repo_dir.join(".git"),
                    format!("gitdir: {}\n", real.display()),
                )
                .unwrap();
            }
            _ => {
                sh(&repo_dir, &["init", "-q", "-b", "main"]);
                if c.name != "no index yet" {
                    std::fs::write(repo_dir.join("ws-t.bin"), &big).unwrap();
                    sh(&repo_dir, &["add", "."]);
                    tracked = Some(repo_dir.join("ws-t.bin"));
                    if c.name != "no commit yet" {
                        sh(&repo_dir, &["commit", "-qm", "c"]);
                    }
                }
                match c.name {
                    "workspace root inside a repository" => {
                        let ws = repo_dir.join("ws");
                        folder = ws.join("chat");
                        std::fs::create_dir_all(&folder).unwrap();
                        std::fs::write(folder.join("t.bin"), &big).unwrap();
                        sh(&repo_dir, &["add", "."]);
                        sh(&repo_dir, &["commit", "-qm", "c2"]);
                        tracked = Some(folder.join("t.bin"));
                        ceiling = Some(ws);
                    }
                    "split index" => {
                        sh(&repo_dir, &["update-index", "--split-index"]);
                    }
                    _ => {}
                }
            }
        }
        let bounds = Bounds {
            untracked_max: CAP,
            ceiling,
            ..Default::default()
        };
        let repo = checkpoint_repo(&folder, Some(&top.join("cp")), &bounds)
            .await
            .unwrap()
            .unwrap();
        match c.name {
            "index unreadable" => std::fs::set_permissions(
                repo_dir.join(".git/index"),
                std::fs::Permissions::from_mode(0o000),
            )
            .unwrap(),
            "index gone after the folder was looked at" => {
                std::fs::remove_file(repo_dir.join(".git/index")).unwrap()
            }
            _ => {}
        }
        std::fs::write(folder.join("u.bin"), vec![b'u'; 2 * CAP as usize]).unwrap();
        let snap = snapshot_full(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        let names: Vec<String> = snap
            .skipped
            .files
            .iter()
            .map(|(p, _)| String::from_utf8_lossy(p).into_owned())
            .collect();
        assert_eq!(
            names.contains(&"u.bin".to_string()),
            c.cap_on,
            "{}: {names:?}",
            c.name
        );
        if let Some(t) = &tracked {
            let rel = t
                .strip_prefix(&repo.root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            assert!(!names.contains(&rel), "{}: tracked {rel} left out", c.name);
        }
        let _ = std::fs::set_permissions(
            repo_dir.join(".git/index"),
            std::fs::Permissions::from_mode(0o644),
        );
    }
}

/// NEW-2 b (RV5-SPLIT): with a split index the index cannot be read here, and
/// `HEAD` does not know a file that is staged and never committed: nothing
/// is left out, and an edit to it is undone.
#[tokio::test]
async fn a_staged_large_file_with_a_split_index_is_undone() {
    let r = rig();
    sh(&r.dir, &["update-index", "--split-index"]);
    std::fs::write(r.dir.join("staged.bin"), vec![b's'; 3 * CAP as usize]).unwrap();
    sh(&r.dir, &["add", "staged.bin"]);
    let repo = r.repo(CAP).await;
    let b = snapshot_full(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    assert!(b.skipped.is_empty(), "{:?}", b.skipped);
    std::fs::write(r.dir.join("staged.bin"), vec![b'Z'; 5 * CAP as usize]).unwrap();
    let a = snapshot(&repo, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    let done = restore_report(&repo, &b.sha, &a, false).await.unwrap();
    assert_eq!(r.get("staged.bin"), Some(vec![b's'; 3 * CAP as usize]));
    assert!(done.plan.left.is_empty());
}

/// NEW-2 a (RV5-CEIL): the workspace root lies inside a repository; the walk
/// stops at it, so the node cannot tell what git tracks: no cap, and an edit
/// to a large tracked file is undone.
#[tokio::test]
async fn a_workspace_root_inside_a_repository_leaves_nothing_out() {
    let r = rig();
    let ws = r.dir.join("ws");
    let chat = ws.join("chat");
    std::fs::create_dir_all(&chat).unwrap();
    std::fs::write(chat.join("big-tracked.bin"), vec![b't'; 3 * CAP as usize]).unwrap();
    sh(&r.dir, &["add", "."]);
    sh(&r.dir, &["commit", "-qm", "c"]);
    let bounds = Bounds {
        untracked_max: CAP,
        ceiling: Some(ws.clone()),
        ..Default::default()
    };
    let repo = checkpoint_repo(&chat, Some(&r.base), &bounds)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(repo.untracked_max(), 0);
    let b = snapshot(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    std::fs::write(chat.join("big-tracked.bin"), vec![b'Z'; 5 * CAP as usize]).unwrap();
    let a = snapshot(&repo, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    restore_report(&repo, &b, &a, false).await.unwrap();
    assert_eq!(
        std::fs::read(chat.join("big-tracked.bin")).unwrap(),
        vec![b't'; 3 * CAP as usize]
    );
}

/// NEW-3 (INC1): with an incomplete record, before has `d/` (an unnamed
/// large `d/x`, a small `d/s`); the turn made `d` a file. The undo puts
/// `d/s` back, which removes the file `d`: named removed, not "did not
/// delete".
#[tokio::test]
async fn an_unsure_file_the_undo_must_remove_is_named_removed() {
    for force in [false, true] {
        let (r, repo) = incomplete_rig(1024).await;
        std::fs::create_dir(r.dir.join("d")).unwrap();
        std::fs::write(r.dir.join("d/x"), vec![b'X'; 1025]).unwrap();
        std::fs::write(r.dir.join("d/s"), "s\n").unwrap();
        let b = snapshot_full(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        assert!(!b.skipped.files.iter().any(|(p, _)| p == b"d/x"));
        std::fs::remove_dir_all(r.dir.join("d")).unwrap();
        std::fs::write(r.dir.join("d"), "turn\n").unwrap();
        let a = snapshot(&repo, "refs/harness/s/t/after", "a")
            .await
            .unwrap();
        let done = restore_report(&repo, &b.sha, &a, force).await.unwrap();
        assert_eq!(r.get("d/s").as_deref(), Some(&b"s\n"[..]), "force {force}");
        assert_eq!(
            kinds(&done.plan.left),
            vec![("d".to_string(), LeftKind::Removed)],
            "force {force}"
        );
        assert_eq!(size_in(&repo, &done.now, "d").await, Some(5));
        assert!(done.plan.incomplete);
    }
}

/// NEW-4: with an incomplete record the plan says so (and the undo note
/// with it) where nothing else is named: an unnamed large file the turn
/// deleted, an unnamed `d/x` whose folder the turn replaced, an unnamed
/// large file the turn renamed.
#[tokio::test]
async fn an_incomplete_record_is_said_when_nothing_else_is_named() {
    for case in ["deleted", "folder replaced", "renamed"] {
        let (r, repo) = incomplete_rig(1024).await;
        match case {
            "folder replaced" => {
                std::fs::create_dir(r.dir.join("d")).unwrap();
                std::fs::write(r.dir.join("d/x"), vec![b'X'; 1025]).unwrap();
            }
            _ => r.put("f", &State::Big(1025, b'P')),
        }
        let b = snapshot_full(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        assert!(b.skipped.more > 0);
        match case {
            "deleted" => r.put("f", &State::Absent),
            "folder replaced" => {
                std::fs::remove_dir_all(r.dir.join("d")).unwrap();
            }
            _ => std::fs::rename(r.dir.join("f"), r.dir.join("g")).unwrap(),
        }
        std::fs::write(r.dir.join("ctl.txt"), "turn\n").unwrap();
        let a = snapshot(&repo, "refs/harness/s/t/after", "a")
            .await
            .unwrap();
        let done = restore_report(&repo, &b.sha, &a, false).await.unwrap();
        assert!(done.plan.incomplete, "{case}");
        assert_eq!(r.get("ctl.txt").as_deref(), Some(&b"ctl0\n"[..]));
    }
}

/// A malformed record line (only the node writes them) makes the record
/// incomplete, never shorter in silence; an overflowing count too.
#[tokio::test]
async fn a_malformed_record_counts_as_incomplete() {
    let r = rig();
    let repo = r.repo(CAP).await;
    let tree = run(
        &repo,
        &["write-tree"],
        Some(&repo.shadow.clone().unwrap().join("i0")),
    )
    .await
    .unwrap();
    for msg in [
        "m\n\nharness-skipped: notanumber 6162\n",
        "m\n\nharness-skipped: 10 zz\n",
        "m\n\nharness-skipped-more: 99999999999999999999999 1\n",
    ] {
        let sha = run_raw(
            &repo,
            &["commit-tree", "--no-gpg-sign", &tree, "-F", "-"],
            None,
            Some(msg.as_bytes().to_vec()),
        )
        .await
        .unwrap();
        let sha = String::from_utf8_lossy(&sha).trim().to_string();
        assert!(skipped_in(&repo, &sha).await.more > 0, "{msg:?}");
    }
}

/// A name holding the text `<U+200B>` never reads like one holding that
/// character.
#[test]
fn a_literal_code_in_a_name_differs_from_the_character() {
    assert_ne!(
        shown("a<U+200B>b".as_bytes()),
        shown("a\u{200B}b".as_bytes())
    );
    assert_ne!(shown("<\u{200B}".as_bytes()), shown("<U+200B>".as_bytes()));
    assert_eq!(shown("a\u{200B}b".as_bytes()), "a<U+200B>b");
}

/// A large untracked file that appears during a merge where the child adds
/// a folder is named for its size, not as a file git ignores.
#[tokio::test]
async fn a_large_file_where_a_child_adds_a_folder_is_named_for_its_size() {
    let r = rig();
    let bounds = Bounds {
        untracked_max: CAP,
        ..Default::default()
    };
    let sub = sub_repo(&r.dir, Some(&r.base), &bounds)
        .await
        .unwrap()
        .unwrap();
    let base = sub.base_full("c").await.unwrap();
    let folder = sub
        .make_folder("c", &base.sha, &sessions_of(&r.base))
        .await
        .unwrap();
    std::fs::create_dir(folder.path.join("q")).unwrap();
    std::fs::write(folder.path.join("q/x.txt"), "child\n").unwrap();
    let result = sub.result(&folder, "c").await.unwrap();
    let top = r.dir.clone();
    let person = move || std::fs::write(top.join("q"), vec![b'P'; 2 * CAP as usize]).unwrap();
    let res = sub
        .apply_with(
            &base.sha,
            &result,
            "c",
            merge::Hooks {
                between: Some(&person),
                ..Default::default()
            },
        )
        .await;
    match &res {
        Err(NotApplied::Conflict(c)) => {
            assert!(c.iter().all(|(_, why)| why.contains("size limit")), "{c:?}")
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(r.get("q"), Some(vec![b'P'; 2 * CAP as usize]));
    remove_folder(&folder).await;
}

/// Found by the random test (seed 30, cap 0 and on; the same on main): the
/// turn deleted `a/` (with `a/x`); afterwards the person made a file `a`.
/// Writing `a/x` back would replace it: the unforced undo refuses. A change
/// elsewhere in a folder the undo writes into does not count.
#[tokio::test]
async fn an_unforced_undo_refuses_over_a_new_file_where_its_folder_goes() {
    for cap in [0, CAP] {
        let r = rig();
        let repo = r.repo(cap).await;
        std::fs::create_dir(r.dir.join("a")).unwrap();
        std::fs::write(r.dir.join("a/x"), "x\n").unwrap();
        std::fs::write(r.dir.join("a/y"), "y\n").unwrap();
        let b = snapshot(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        std::fs::remove_file(r.dir.join("a/x")).unwrap();
        let a = snapshot(&repo, "refs/harness/s/t/after", "a")
            .await
            .unwrap();
        // A sibling changed: not in the way.
        std::fs::write(r.dir.join("a/y"), "person\n").unwrap();
        restore_report(&repo, &b, &a, false).await.unwrap();
        assert_eq!(r.get("a/x").as_deref(), Some(&b"x\n"[..]));
        // The folder replaced by the person's file: refused.
        std::fs::remove_file(r.dir.join("a/x")).unwrap();
        let a2 = snapshot(&repo, "refs/harness/s/t2/after", "a")
            .await
            .unwrap();
        std::fs::remove_dir_all(r.dir.join("a")).unwrap();
        std::fs::write(r.dir.join("a"), "person's file\n").unwrap();
        let e = restore_report(&repo, &b, &a2, false).await.unwrap_err();
        assert!(
            e.contains("changed after the turn") && e.contains('a'),
            "cap {cap}: {e}"
        );
        assert_eq!(r.get("a").as_deref(), Some(&b"person's file\n"[..]));
    }
}
