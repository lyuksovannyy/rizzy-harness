//! Undo's check of what changed after the turn (`restore_plan`, PLAN.md
//! T33): an unforced undo refuses when it would write over anything the
//! person changed since the turn, wherever that change lies (at, below or
//! above a path the undo writes) and whatever the name's bytes are, and it
//! does not refuse over a change it would not touch.

use super::tests::sh;
use super::*;
use std::os::unix::ffi::OsStrExt;

struct Rig {
    _d: tempfile::TempDir,
    dir: PathBuf,
    repo: Repo,
    n: std::cell::Cell<u32>,
}

/// A repository with one committed file and its checkpoint repository.
async fn rig() -> Rig {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().canonicalize().unwrap().join("repo");
    let base = d.path().canonicalize().unwrap().join("checkpoints");
    std::fs::create_dir_all(&dir).unwrap();
    sh(&dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("ctl.txt"), "ctl\n").unwrap();
    sh(&dir, &["add", "."]);
    sh(&dir, &["commit", "-qm", "init"]);
    let repo = repo_for(&dir, Some(&base)).await.unwrap();
    Rig {
        _d: d,
        dir,
        repo,
        n: std::cell::Cell::new(0),
    }
}

impl Rig {
    fn at(&self, name: &[u8]) -> PathBuf {
        self.dir.join(std::ffi::OsStr::from_bytes(name))
    }

    fn put(&self, name: &[u8], text: &str) {
        let p = self.at(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn get(&self, name: &[u8]) -> Option<String> {
        std::fs::read_to_string(self.at(name)).ok()
    }

    async fn snap(&self) -> String {
        self.n.set(self.n.get() + 1);
        snapshot(
            &self.repo,
            &format!("refs/harness/s/t{}/after", self.n.get()),
            "s",
        )
        .await
        .unwrap()
    }
}

/// Fault 2 (review 3 of PR 154, the random test's seed 30): the turn
/// deleted `a/x`; afterwards the person put a file (or a link) where the
/// folder `a` was. Writing `a/x` back replaces it, and main's check, which
/// looked only at and below the paths written, went ahead.
#[tokio::test]
async fn an_unforced_undo_refuses_over_a_file_where_a_folder_it_writes_goes() {
    // (files before the turn, what the turn deletes, the folder replaced)
    type Case = (&'static [&'static [u8]], &'static [u8], &'static [u8]);
    let cases: [Case; 3] = [
        (&[b"a/x", b"a/y"], b"a/x", b"a"),
        (&[b"a/x"], b"a", b"a"),
        (&[b"a/b/x"], b"a", b"a"),
    ];
    for (files, deleted, folder) in cases {
        for link in [false, true] {
            let r = rig().await;
            for f in files {
                r.put(f, "before\n");
            }
            let b = r.snap().await;
            let d = r.at(deleted);
            if d.is_dir() {
                std::fs::remove_dir_all(&d).unwrap();
            } else {
                std::fs::remove_file(&d).unwrap();
            }
            let a = r.snap().await;
            let _ = std::fs::remove_dir_all(r.at(folder));
            if link {
                std::os::unix::fs::symlink("ctl.txt", r.at(folder)).unwrap();
            } else {
                r.put(folder, "person's file\n");
            }
            let e = restore_report(&r.repo, &b, &a, false).await.unwrap_err();
            assert!(
                e.contains("changed after the turn") && e.contains("`a`"),
                "{files:?} link {link}: {e}"
            );
            let m = std::fs::symlink_metadata(r.at(folder)).unwrap();
            assert_eq!(m.file_type().is_symlink(), link);
            if !link {
                assert_eq!(r.get(folder).as_deref(), Some("person's file\n"));
            }
            // Forced: it goes ahead, and the snapshot taken before it
            // holds the person's file.
            restore_report(&r.repo, &b, &a, true).await.unwrap();
            assert_eq!(r.get(files[0]).as_deref(), Some("before\n"));
            let held = run(
                &r.repo,
                &["cat-file", "-p", "refs/harness/test/now/before:a"],
                None,
            )
            .await
            .unwrap();
            assert_eq!(held, if link { "ctl.txt" } else { "person's file" });
        }
    }
}

/// The other side of fault 2: a change elsewhere in a folder the undo
/// writes into is not in the way, and stays.
#[tokio::test]
async fn a_change_elsewhere_in_a_folder_the_undo_writes_into_does_not_count() {
    let r = rig().await;
    r.put(b"a/x", "x\n");
    r.put(b"a/y", "y\n");
    let b = r.snap().await;
    std::fs::remove_file(r.at(b"a/x")).unwrap();
    let a = r.snap().await;
    r.put(b"a/y", "person\n");
    r.put(b"a/new", "person\n");
    restore_report(&r.repo, &b, &a, false).await.unwrap();
    assert_eq!(r.get(b"a/x").as_deref(), Some("x\n"));
    assert_eq!(r.get(b"a/y").as_deref(), Some("person\n"));
    assert_eq!(r.get(b"a/new").as_deref(), Some("person\n"));
}

/// Fault 3, the false refusal: main gave the names to git as pathspecs,
/// so `a*` also matched the person's change to `ab`, which the undo does
/// not write.
#[tokio::test]
async fn a_name_with_glob_characters_matches_only_itself() {
    let r = rig().await;
    r.put(b"a*", "before\n");
    r.put(b"ab", "before\n");
    let b = r.snap().await;
    r.put(b"a*", "turn\n");
    let a = r.snap().await;
    r.put(b"ab", "person\n");
    restore_report(&r.repo, &b, &a, false).await.unwrap();
    assert_eq!(r.get(b"a*").as_deref(), Some("before\n"));
    assert_eq!(r.get(b"ab").as_deref(), Some("person\n"));
}

/// Fault 3, the miss: a name that reads as pathspec magic (`:(attr:…)`)
/// matched nothing on main, so the person's change to it was written over.
#[tokio::test]
async fn a_name_that_reads_as_pathspec_magic_changed_after_the_turn_is_refused() {
    for name in [&b":(attr:zz)k"[..], b":(exclude)k", b":!k"] {
        let r = rig().await;
        r.put(name, "before\n");
        let b = r.snap().await;
        r.put(name, "turn\n");
        let a = r.snap().await;
        r.put(name, "person\n");
        let e = restore_report(&r.repo, &b, &a, false).await.unwrap_err();
        assert!(e.contains("changed after the turn"), "{e}");
        assert_eq!(r.get(name).as_deref(), Some("person\n"));
    }
}

/// Fault 4: main gave git the names as lossy UTF-8, so `n<FF>m` became a
/// name that matched nothing and the person's change was written over.
#[tokio::test]
async fn a_name_that_is_not_utf8_changed_after_the_turn_is_refused() {
    for name in [&b"n\xffm"[..], b"d\xfe/k", b"\xc3\xa9"] {
        let r = rig().await;
        r.put(name, "before\n");
        let b = r.snap().await;
        r.put(name, "turn\n");
        let a = r.snap().await;
        r.put(name, "person\n");
        let e = restore_report(&r.repo, &b, &a, false).await.unwrap_err();
        assert!(e.contains("changed after the turn"), "{e}");
        assert_eq!(r.get(name).as_deref(), Some("person\n"));
        // Unchanged since: it goes ahead.
        r.put(name, "turn\n");
        restore_report(&r.repo, &b, &a, false).await.unwrap();
        assert_eq!(r.get(name).as_deref(), Some("before\n"));
    }
}

/// `unsaved_in_replaced_dirs` with a folder name that is not UTF-8: main
/// asked git about the lossy name, found nothing held, and refused over
/// files every checkpoint holds.
#[tokio::test]
async fn a_folder_with_a_name_that_is_not_utf8_replaced_by_a_file_is_undone() {
    let r = rig().await;
    r.put(b"n\xffm", "file\n");
    let b = r.snap().await;
    std::fs::remove_file(r.at(b"n\xffm")).unwrap();
    r.put(b"n\xffm/k", "turn\n");
    let a = r.snap().await;
    restore_report(&r.repo, &b, &a, false).await.unwrap();
    assert_eq!(r.get(b"n\xffm").as_deref(), Some("file\n"));
}

/// Renames after the turn: the undo writes `x`, the person renamed it to
/// `y`. Refused (the check reads every change since the turn without
/// rename detection, which would report only `y` and hide that `x` went).
#[tokio::test]
async fn a_rename_after_the_turn_is_refused() {
    let r = rig().await;
    r.put(b"x", "a long enough line for rename detection to pair it\n");
    let b = r.snap().await;
    r.put(
        b"x",
        "a long enough line for rename detection to pair it!\n",
    );
    let a = r.snap().await;
    std::fs::rename(r.at(b"x"), r.at(b"y")).unwrap();
    let e = restore_report(&r.repo, &b, &a, false).await.unwrap_err();
    assert!(e.contains("changed after the turn"), "{e}");
    assert!(r.get(b"x").is_none());
}

/// Found by review 3 of PR 154 (about 20 in 6,000 random scenarios) and
/// by the random test here (seed 187): a folder the last snapshot held,
/// replaced by a link that loops (`a -> a`). Git's `add -u` cannot tell
/// that `a/x` went (`lstat` fails with ELOOP, which it does not read as
/// "deleted") and keeps it, so adding the link failed ("'a' appears as both
/// a file and as a directory") and the turn had no checkpoint, so no Undo.
#[tokio::test]
async fn a_folder_replaced_by_a_looping_link_is_checkpointed_and_undone() {
    for target in ["a", "nowhere"] {
        let r = rig().await;
        r.put(b"a/x", "x\n");
        let b = r.snap().await;
        std::fs::remove_dir_all(r.at(b"a")).unwrap();
        std::os::unix::fs::symlink(target, r.at(b"a")).unwrap();
        let a = r.snap().await;
        let link = run(&r.repo, &["cat-file", "-p", &format!("{a}:a")], None)
            .await
            .unwrap();
        assert_eq!(link, target);
        restore_report(&r.repo, &b, &a, false).await.unwrap();
        assert_eq!(r.get(b"a/x").as_deref(), Some("x\n"));
    }
}
