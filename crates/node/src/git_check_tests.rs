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

/// Review of PR 160, P1: the turn stops tracking `.env` (deletes it and
/// ignores it); the person then writes a new `.env`. Git ignores it, so
/// the snapshot taken before the undo does not hold it, and the check saw
/// no change: the undo wrote the old `.env` over the person's, which no
/// checkpoint held. Now it is a file in no checkpoint in the undo's way:
/// an unforced undo refuses and names it; a forced one names it again.
#[tokio::test]
async fn an_ignored_file_at_a_path_the_undo_writes_is_named_and_refused() {
    let r = rig().await;
    r.put(b".env", "OLD=1\n");
    let b = r.snap().await;
    std::fs::remove_file(r.at(b".env")).unwrap();
    r.put(b".gitignore", ".env\n");
    let a = r.snap().await;
    r.put(b".env", "PERSON=2\n");
    let e = restore_report(&r.repo, &b, &a, false).await.unwrap_err();
    assert!(
        e.contains("in no checkpoint") && e.contains("`.env`") && e.contains("Undo anyway"),
        "{e}"
    );
    assert_eq!(r.get(b".env").as_deref(), Some("PERSON=2\n"));
    let done = restore_report(&r.repo, &b, &a, true).await.unwrap();
    assert_eq!(done.plan.unsaved.shown, vec![".env".to_string()]);
    assert_eq!(r.get(b".env").as_deref(), Some("OLD=1\n"));
}

/// P1b: the same with an ignored file where a folder the undo puts back
/// goes.
#[tokio::test]
async fn an_ignored_file_where_a_folder_the_undo_writes_goes_is_named_and_refused() {
    let r = rig().await;
    r.put(b"cache/x", "x\n");
    let b = r.snap().await;
    std::fs::remove_dir_all(r.at(b"cache")).unwrap();
    r.put(b".gitignore", "cache\n");
    let a = r.snap().await;
    r.put(b"cache", "person's file\n");
    let e = restore_report(&r.repo, &b, &a, false).await.unwrap_err();
    assert!(
        e.contains("in no checkpoint") && e.contains("`cache`"),
        "{e}"
    );
    assert_eq!(r.get(b"cache").as_deref(), Some("person's file\n"));
    let done = restore_report(&r.repo, &b, &a, true).await.unwrap();
    assert_eq!(done.plan.unsaved.shown, vec!["cache".to_string()]);
    assert_eq!(r.get(b"cache/x").as_deref(), Some("x\n"));
}

/// The guard for the two above: ignored files beside what the undo writes
/// (a sibling, a `target/` next to a written file, one inside a folder it
/// writes into) are not in its way and do not count.
#[tokio::test]
async fn ignored_files_beside_the_paths_the_undo_writes_do_not_count() {
    let r = rig().await;
    r.put(b".gitignore", "target/\n*.log\n");
    r.put(b"a/x", "x\n");
    r.put(b"y", "y\n");
    let b = r.snap().await;
    r.put(b"a/x", "turn\n");
    std::fs::remove_file(r.at(b"y")).unwrap();
    let a = r.snap().await;
    r.put(b"target/out", "build\n");
    r.put(b"a/z.log", "log\n");
    r.put(b"y.log", "log\n");
    restore_report(&r.repo, &b, &a, false).await.unwrap();
    assert_eq!(r.get(b"a/x").as_deref(), Some("x\n"));
    assert_eq!(r.get(b"y").as_deref(), Some("y\n"));
    for p in [&b"target/out"[..], b"a/z.log", b"y.log"] {
        assert!(r.get(p).is_some(), "{p:?}");
    }
}

/// The rule for an ignored file the TURN wrote in the undo's way: the node
/// cannot tell it from one the person wrote after the turn (neither is in
/// any checkpoint), so it counts the same: refused, named. It needs a turn
/// that makes a path ignored and writes a new file there where the tree
/// before it had a folder (or nothing) at that path: the turn replaces
/// `cache/` by an ignored file `cache`.
#[tokio::test]
async fn an_ignored_file_the_turn_wrote_in_the_undos_way_is_named_and_refused() {
    let r = rig().await;
    r.put(b"cache/x", "x\n");
    let b = r.snap().await;
    std::fs::remove_dir_all(r.at(b"cache")).unwrap();
    r.put(b".gitignore", "cache\n");
    r.put(b"cache", "turn\n");
    let a = r.snap().await;
    let e = restore_report(&r.repo, &b, &a, false).await.unwrap_err();
    assert!(
        e.contains("in no checkpoint") && e.contains("`cache`"),
        "{e}"
    );
    assert_eq!(r.get(b"cache").as_deref(), Some("turn\n"));
    let done = restore_report(&r.repo, &b, &a, true).await.unwrap();
    assert_eq!(done.plan.unsaved.shown, vec!["cache".to_string()]);
    assert_eq!(r.get(b"cache/x").as_deref(), Some("x\n"));
}

/// The other side of that rule: a file a snapshot held stays in the
/// checkpoints after it becomes ignored (the checkpoint index tracks it),
/// so the turn's new `.env` under a new ignore rule is held by the
/// snapshot before the undo, not in its way, and an undo of the undo
/// brings it back.
#[tokio::test]
async fn an_ignored_file_the_checkpoints_still_hold_is_not_in_the_way() {
    let r = rig().await;
    r.put(b".env", "OLD=1\n");
    let b = r.snap().await;
    r.put(b".gitignore", ".env\n");
    r.put(b".env", "TURN=1\n");
    let a = r.snap().await;
    let done = restore_report(&r.repo, &b, &a, false).await.unwrap();
    assert!(done.plan.unsaved.is_empty());
    assert_eq!(r.get(b".env").as_deref(), Some("OLD=1\n"));
    let held = run(
        &r.repo,
        &["cat-file", "-p", "refs/harness/test/now/before:.env"],
        None,
    )
    .await
    .unwrap();
    assert_eq!(held, "TURN=1");
}

/// The person's change below a path the undo writes a file to (the turn
/// deleted the file `a/b`, the person made `a/b/c`): refused. Before this
/// test only one random seed caught a check of "exactly at" instead of "at
/// or below".
#[tokio::test]
async fn a_change_below_a_path_the_undo_writes_is_refused() {
    let r = rig().await;
    r.put(b"a/b", "file\n");
    let b = r.snap().await;
    std::fs::remove_file(r.at(b"a/b")).unwrap();
    let a = r.snap().await;
    r.put(b"a/b/c", "person\n");
    let e = restore_report(&r.repo, &b, &a, false).await.unwrap_err();
    assert!(
        e.contains("changed after the turn") && e.contains("`a/b/c`"),
        "{e}"
    );
    assert_eq!(r.get(b"a/b/c").as_deref(), Some("person\n"));
}

/// A folder named like pathspec magic that the undo replaces by a file:
/// its files are looked up by the name's bytes as a literal pathspec (with
/// magic, `ls-tree` failed and so did the undo, forced or not).
#[tokio::test]
async fn a_folder_named_like_pathspec_magic_replaced_by_a_file_is_undone() {
    let r = rig().await;
    r.put(b":!q1", "file\n");
    let b = r.snap().await;
    std::fs::remove_file(r.at(b":!q1")).unwrap();
    r.put(b":!q1/k", "turn\n");
    let a = r.snap().await;
    restore_report(&r.repo, &b, &a, false).await.unwrap();
    assert_eq!(r.get(b":!q1").as_deref(), Some("file\n"));
}

/// A name cannot pass for two in a refusal: the backticks that mark names
/// are never written from a name.
#[tokio::test]
async fn a_name_with_backticks_reads_as_one_name_in_a_refusal() {
    let name = b"x`, `ctl.txt";
    let r = rig().await;
    r.put(name, "before\n");
    let b = r.snap().await;
    r.put(name, "turn\n");
    let a = r.snap().await;
    r.put(name, "person\n");
    let e = restore_report(&r.repo, &b, &a, false).await.unwrap_err();
    assert!(!e.contains("`ctl.txt`"), "{e}");
    assert_eq!(e.matches('`').count(), 2, "{e}");
}

/// What sits where a file was: a named pipe (never opened) or a socket.
fn special(p: &Path, fifo: bool) -> Option<std::os::unix::net::UnixListener> {
    let _ = std::fs::remove_file(p);
    if fifo {
        nix::unistd::mkfifo(p, nix::sys::stat::Mode::from_bits_truncate(0o644)).unwrap();
        None
    } else {
        Some(std::os::unix::net::UnixListener::bind(p).unwrap())
    }
}

/// Review 2 of PR 160, L1 (on main too): a named pipe or a socket where a
/// tracked file was made `add -u` fail ("can only add regular files"), so
/// every snapshot failed while it was there: no checkpoint, no Undo. Now
/// the file counts as gone in that snapshot, which names the path; the
/// pipe is never opened (a read of it would hang this test).
#[tokio::test]
async fn a_pipe_or_socket_where_a_tracked_file_was_is_left_out_and_named() {
    for fifo in [true, false] {
        let r = rig().await;
        r.put(b"p", "v0\n");
        r.put(b"m", "m old\n");
        let b = r.snap().await;
        let _keep = special(&r.at(b"p"), fifo);
        r.put(b"q", "other\n");
        // A tracked file changed next to it (review 3 of PR 162): only the
        // pipe leaves the index, and `add -u` runs again, so `m` is held
        // with its new content, not as deleted and not as before.
        r.put(b"m", "m new, longer\n");
        let s = snapshot_full(&r.repo, "refs/harness/s/x/after", "s")
            .await
            .unwrap();
        assert_eq!(s.special.shown, vec!["p".to_string()], "fifo {fifo}");
        let ls = run(&r.repo, &["ls-tree", "--name-only", &s.sha], None)
            .await
            .unwrap();
        assert_eq!(
            ls.lines().collect::<Vec<_>>(),
            ["ctl.txt", "m", "q"],
            "fifo {fifo}"
        );
        let m = run(
            &r.repo,
            &["cat-file", "blob", &format!("{}:m", s.sha)],
            None,
        )
        .await
        .unwrap();
        assert_eq!(m, "m new, longer", "fifo {fifo}");
        // The next snapshot too (the checkpoint index no longer has it).
        let again = snapshot_full(&r.repo, "refs/harness/s/y/after", "s")
            .await
            .unwrap();
        assert!(again.special.is_empty());
        // An Undo of that span brings `m`'s old content back (forced: the
        // pipe is in no checkpoint and in its way).
        restore_report(&r.repo, &b, &s.sha, true).await.unwrap();
        assert_eq!(r.get(b"m").as_deref(), Some("m old\n"), "fifo {fifo}");
        assert_eq!(r.get(b"p").as_deref(), Some("v0\n"), "fifo {fifo}");
    }
}

/// Review 3 of PR 162, LOW-2: the fallback looks at a path without
/// following links on the way (`git::at`): a tracked `d/f` whose folder is
/// now a link to a folder outside, where `f` is a pipe, is gone by git's
/// rules (a path through a link), never named, and nothing outside is
/// touched. A real pipe at `p` makes the fallback run.
#[tokio::test]
async fn the_fallback_never_looks_through_a_link_above_a_path() {
    let r = rig().await;
    let outside = r.dir.parent().unwrap().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    r.put(b"d/f", "f\n");
    r.put(b"p", "v0\n");
    r.snap().await;
    std::fs::remove_dir_all(r.at(b"d")).unwrap();
    let _o = special(&outside.join("f"), true);
    std::os::unix::fs::symlink(&outside, r.at(b"d")).unwrap();
    let _p = special(&r.at(b"p"), true);
    let s = snapshot_full(&r.repo, "refs/harness/s/x/after", "s")
        .await
        .unwrap();
    assert_eq!(s.special.shown, vec!["p".to_string()]);
    let ls = run(&r.repo, &["ls-tree", "-r", "--name-only", &s.sha], None)
        .await
        .unwrap();
    assert_eq!(ls.lines().collect::<Vec<_>>(), ["ctl.txt", "d"]);
    let m = std::fs::symlink_metadata(outside.join("f")).unwrap();
    assert!(std::os::unix::fs::FileTypeExt::is_fifo(&m.file_type()));
}

/// Review 3 of PR 162, MEDIUM-1: asked by name and read from the whole
/// listing, `not_held` gives the same answer, below, at and above the
/// switch between them, for names git or a shell would read as something
/// else, folders and files swapped between `now` and the disk, a link, a
/// path below a held file, a duplicate and nothing at all.
#[tokio::test]
async fn by_name_and_the_listing_agree() {
    let r = rig().await;
    let odd: Vec<&[u8]> = vec![
        b"\xff\xfe",
        b"nl\nname",
        b":!q1",
        b":(attr:zz)k",
        b"a*",
        b"[x]",
        b"-lead",
        b"abc",
        b"xx",
        b"sp ace",
        b"back\\slash",
        b"q?",
    ];
    // `now` holds every other odd name, `abc` (which `a*` would match as a
    // pattern), a folder `dirheld/`, files `fileheld` and `heldfile`, a link.
    for (i, w) in odd.iter().enumerate() {
        if i % 2 == 0 {
            r.put(w, "now\n");
        }
    }
    r.put(b"abc", "now\n");
    r.put(b"dirheld/x", "now\n");
    r.put(b"fileheld", "now\n");
    r.put(b"heldfile", "now\n");
    std::os::unix::fs::symlink("target", r.at(b"lnk")).unwrap();
    let now = r.snap().await;
    let mut asked: Vec<Vec<u8>> = odd.iter().map(|w| w.to_vec()).collect();
    asked.extend([
        b"dirheld".to_vec(),
        b"fileheld/y".to_vec(),
        b"fileheld".to_vec(),
        b"lnk".to_vec(),
        b"heldfile/below".to_vec(),
        b"heldfile".to_vec(),
        b"a*".to_vec(),
    ]);
    let mut want: Vec<Vec<u8>> = odd
        .iter()
        .enumerate()
        .filter(|(i, w)| i % 2 == 1 && **w != b"abc")
        .map(|(_, w)| w.to_vec())
        .collect();
    want.extend([
        b"dirheld".to_vec(),
        b"fileheld/y".to_vec(),
        b"heldfile/below".to_vec(),
    ]);
    want.sort();
    let sorted = |mut v: Vec<Vec<u8>>| {
        v.sort();
        v
    };
    for by_name in [true, false] {
        let got = not_held(&r.repo, &now, asked.clone(), by_name)
            .await
            .unwrap();
        assert_eq!(sorted(got), want, "by name {by_name}");
        assert!(
            not_held(&r.repo, &now, Vec::new(), by_name)
                .await
                .unwrap()
                .is_empty()
        );
    }
    // Paths `now` does not hold push the count over the switch.
    let n = asked.len();
    for total in [255usize, 256, 257, 400] {
        let mut all = asked.clone();
        all.extend((0..total - n).map(|i| format!("fill/{i:03}").into_bytes()));
        assert_eq!(by_name(&all), total <= 256, "total {total}");
        let a = sorted(not_held(&r.repo, &now, all.clone(), true).await.unwrap());
        let b = sorted(not_held(&r.repo, &now, all.clone(), false).await.unwrap());
        assert_eq!(a, b, "total {total}");
        let fill = a.iter().filter(|p| p.starts_with(b"fill/")).count();
        assert_eq!(fill, total - n, "total {total}");
        let rest: Vec<Vec<u8>> = a.into_iter().filter(|p| !p.starts_with(b"fill/")).collect();
        assert_eq!(rest, want, "total {total}");
    }
    // Long names: few of them, but many bytes, are read from the listing.
    let long: Vec<Vec<u8>> = (0..100)
        .map(|i| format!("{i:03}{}", "l".repeat(1000)).into_bytes())
        .collect();
    assert!(!by_name(&long));
}

/// MEDIUM-1 end to end: more than 256 files in an Undo's way, most of
/// them ignored (in no checkpoint), five the snapshot holds: the refusal
/// and the forced Undo's list name exactly the ignored ones.
#[tokio::test]
async fn an_undo_with_hundreds_of_ignored_files_in_its_way_names_exactly_those() {
    let r = rig().await;
    for i in 0..300 {
        r.put(format!("g/f{i:03}").as_bytes(), "before\n");
    }
    for i in 0..5 {
        r.put(format!("g/k{i}").as_bytes(), "before\n");
    }
    let b = r.snap().await;
    for i in 0..300 {
        std::fs::remove_file(r.at(format!("g/f{i:03}").as_bytes())).unwrap();
    }
    for i in 0..5 {
        r.put(format!("g/k{i}").as_bytes(), "turn\n");
    }
    r.put(b".gitignore", "/g/f*\n");
    let a = r.snap().await;
    for i in 0..300 {
        r.put(format!("g/f{i:03}").as_bytes(), "person\n");
    }
    let e = restore_report(&r.repo, &b, &a, false).await.unwrap_err();
    assert!(
        e.contains("in no checkpoint") && e.contains("and 250 more"),
        "{e}"
    );
    assert!(!e.contains("g/k") && !e.contains(".gitignore"), "{e}");
    let done = restore_report(&r.repo, &b, &a, true).await.unwrap();
    assert_eq!(done.plan.unsaved.shown.len() + done.plan.unsaved.more, 300);
    assert!(done.plan.unsaved.shown.iter().all(|n| n.starts_with("g/f")));
}
/// A pipe that git never tracked: its walk does not list it, so staging
/// skips it and nothing is named.
#[tokio::test]
async fn an_untracked_pipe_is_neither_listed_nor_staged() {
    let r = rig().await;
    let _ = special(&r.at(b"f"), true);
    let others = run_raw(
        &r.repo,
        &["ls-files", "-z", "--others", "--exclude-standard"],
        None,
        None,
    )
    .await
    .unwrap();
    // Only the committed file the checkpoint index does not have yet.
    assert_eq!(
        others,
        b"ctl.txt\0",
        "{:?}",
        String::from_utf8_lossy(&others)
    );
    let s = snapshot_full(&r.repo, "refs/harness/s/x/after", "s")
        .await
        .unwrap();
    assert!(s.special.is_empty());
    let ls = run(&r.repo, &["ls-tree", "--name-only", &s.sha], None)
        .await
        .unwrap();
    assert_eq!(ls, "ctl.txt");
}

/// An Undo across a turn that put a pipe where a file was: the pipe is in
/// no checkpoint and in the Undo's way, so the unforced Undo refuses naming
/// it and a forced one puts the file back and names it (the rules for a
/// file in no checkpoint).
#[tokio::test]
async fn an_undo_over_a_pipe_the_turn_left_refuses_or_names_it() {
    let r = rig().await;
    r.put(b"p", "v0\n");
    let b = r.snap().await;
    let _ = special(&r.at(b"p"), true);
    let a = r.snap().await;
    let e = restore_report(&r.repo, &b, &a, false).await.unwrap_err();
    assert!(e.contains("in no checkpoint") && e.contains("`p`"), "{e}");
    let done = restore_report(&r.repo, &b, &a, true).await.unwrap();
    assert_eq!(done.plan.unsaved.shown, vec!["p".to_string()]);
    assert_eq!(r.get(b"p").as_deref(), Some("v0\n"));
}

/// The delete side of `Top::in_the_way`: the turn made `p`; the person put
/// a pipe there, which the snapshot before the Undo leaves out. The Undo
/// deletes `p`: unforced, refused (changed after the turn); forced, it
/// deletes the pipe and names it as in no checkpoint.
#[tokio::test]
async fn a_forced_undo_names_a_pipe_it_deletes() {
    let r = rig().await;
    let b = r.snap().await;
    r.put(b"p", "turn\n");
    let a = r.snap().await;
    let _ = special(&r.at(b"p"), true);
    let e = restore_report(&r.repo, &b, &a, false).await.unwrap_err();
    assert!(
        e.contains("changed after the turn") && e.contains("`p`"),
        "{e}"
    );
    let done = restore_report(&r.repo, &b, &a, true).await.unwrap();
    assert_eq!(done.plan.unsaved.shown, vec!["p".to_string()]);
    assert!(std::fs::symlink_metadata(r.at(b"p")).is_err());
}

/// The refusal says what "Undo anyway" does to such files: an Undo also
/// writes over them, not only deletes.
#[tokio::test]
async fn the_refusal_says_undo_anyway_deletes_or_replaces_them() {
    let r = rig().await;
    r.put(b".env", "OLD=1\n");
    let b = r.snap().await;
    std::fs::remove_file(r.at(b".env")).unwrap();
    r.put(b".gitignore", ".env\n");
    let a = r.snap().await;
    r.put(b".env", "PERSON=2\n");
    let e = restore_report(&r.repo, &b, &a, false).await.unwrap_err();
    assert!(
        e.ends_with("\"Undo anyway\" deletes or replaces them"),
        "{e}"
    );
}

/// A path the snapshot `now` holds as a folder while the disk has a file
/// there (`now` older than the disk): the file is not held, so it is in no
/// checkpoint and named.
#[tokio::test]
async fn a_file_where_now_holds_a_folder_counts_as_not_held() {
    let r = rig().await;
    r.put(b"p", "old\n");
    let b = r.snap().await;
    std::fs::remove_file(r.at(b"p")).unwrap();
    r.put(b"p/q", "turn\n");
    let a = r.snap().await;
    std::fs::remove_dir_all(r.at(b"p")).unwrap();
    r.put(b".gitignore", "/p\n");
    r.put(b"p", "PERSON\n");
    let diff = restore_diff(&r.repo, &b, &a).await.unwrap();
    // `now` = `a`, which holds `p/` as a folder.
    let plan = restore_plan(&r.repo, &diff, &a, false).await.unwrap();
    assert_eq!(plan.unsaved.shown, vec!["p".to_string()]);
}

/// The time of `unsaved_in_the_way` for an Undo of `UNDO_CHANGED` changed
/// files (default all) in a folder of `UNDO_N` files (default 50,000), in
/// `UNDO_DIR` (default a temporary folder). Ignored.
#[tokio::test]
#[ignore]
async fn bench_unsaved_in_the_way() {
    let n: usize = std::env::var("UNDO_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(50_000);
    let d = match std::env::var("UNDO_DIR") {
        Ok(p) => tempfile::tempdir_in(p).unwrap(),
        Err(_) => tempfile::tempdir().unwrap(),
    };
    let dir = d.path().canonicalize().unwrap().join("repo");
    std::fs::create_dir_all(&dir).unwrap();
    sh(&dir, &["init", "-q", "-b", "main"]);
    let repo = repo_for(&dir, Some(&d.path().join("cp"))).await.unwrap();
    let changed: usize = std::env::var("UNDO_CHANGED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(n);
    let put = |t: &str, k: usize| {
        for i in 0..k {
            let p = dir.join(format!("d{}/e{}/f{i}", i % 50, i % 7));
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, t).unwrap();
        }
    };
    put("v0\n", n);
    let b = snapshot(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    put("turn\n", changed);
    let a = snapshot(&repo, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    let t = std::time::Instant::now();
    let now = snapshot(&repo, "refs/harness/s/t/now", "n").await.unwrap();
    eprintln!(
        "UNDO-WAY n={n}: the snapshot before the undo {:?}",
        t.elapsed()
    );
    let diff = restore_diff(&repo, &b, &a).await.unwrap();
    let rows: Vec<&DiffRow> = diff.rows.iter().collect();
    for round in 0..3 {
        let t = std::time::Instant::now();
        let found = unsaved_in_the_way(&repo, &rows, &now).await.unwrap();
        eprintln!(
            "UNDO-WAY n={n} changed={changed} round {round}: {:?} (unsaved {})",
            t.elapsed(),
            found.len()
        );
    }
}
