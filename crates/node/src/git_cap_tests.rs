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

fn names(left: &[LeftAlone]) -> Vec<(String, bool)> {
    left.iter()
        .map(|l| (String::from_utf8_lossy(&l.path).into_owned(), l.lost))
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
