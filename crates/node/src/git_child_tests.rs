//! A sub-agent's folder is node-owned and the node's git never opens the
//! user's git directory (R.1 follow-up: R1-1, R1-2 of
//! docs/security-review/2026-11-gate3-fixes.md, inverted from the
//! reviewer's proofs).

use super::*;

fn sh(dir: &Path, args: &[&str]) -> String {
    let o = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
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

/// `home/proj` (the session's repository), `home/other` (another repository
/// of the same user, inside the same workspace root) and the node's data
/// directory beside `home`.
struct Layout {
    _d: tempfile::TempDir,
    home: PathBuf,
    proj: PathBuf,
    other: PathBuf,
    base: PathBuf,
    bounds: Bounds,
}

fn layout() -> Layout {
    let d = tempfile::tempdir().unwrap();
    let top = d.path().canonicalize().unwrap();
    let home = top.join("home");
    let proj = home.join("proj");
    let other = home.join("other");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    sh(&other, &["init", "-q", "-b", "main"]);
    std::fs::write(other.join("s.txt"), "OTHER-REPOSITORY-SECRET\n").unwrap();
    sh(&other, &["add", "."]);
    sh(&other, &["commit", "-qm", "init"]);
    sh(&proj, &["init", "-q", "-b", "main"]);
    std::fs::write(proj.join("a.txt"), "one\n").unwrap();
    sh(&proj, &["add", "."]);
    sh(&proj, &["commit", "-qm", "init"]);
    let bounds = Bounds {
        ceiling: Some(home.clone()),
        protected: vec![top.join("data")],
        sandbox: None,
    };
    Layout {
        base: top.join("data/checkpoints"),
        _d: d,
        home,
        proj,
        other,
        bounds,
    }
}

/// R1-1: one line in the session's own `.git/commondir` named another
/// repository's git directory, and the sub-agent's worktree (and its
/// sandbox grant) went there. Now the node never asks git about the user's
/// repository; a `.git` with a `commondir` is not taken as a repository,
/// and nothing of the other repository reaches the child.
#[tokio::test]
async fn commondir_never_puts_a_sub_agent_in_another_repository() {
    let l = layout();
    std::fs::write(
        l.proj.join(".git/commondir"),
        format!("{}\n", l.other.join(".git").display()),
    )
    .unwrap();
    let before = sh(&l.other, &["worktree", "list", "--porcelain"]);
    if let Ok(Some(sub)) = sub_repo(&l.proj, Some(&l.base), &l.bounds).await {
        let base = sub.base("c1").await.unwrap();
        if let Ok(wt) = sub.make_folder("c1", &base, &sessions_of(&l.base)).await {
            assert!(
                !wt.path.starts_with(l.other.join(".git")) && !wt.path.starts_with(&l.home),
                "{:?}",
                wt.path
            );
            let shown = std::process::Command::new("git")
                .arg("-C")
                .arg(&wt.path)
                .args(["show", "main:s.txt"])
                .output()
                .unwrap();
            assert!(
                !String::from_utf8_lossy(&shown.stdout).contains("OTHER-REPOSITORY-SECRET"),
                "the child read the other repository"
            );
            remove_folder(&wt).await;
        }
    }
    assert!(!l.other.join(".git/worktrees").exists());
    assert_eq!(sh(&l.other, &["worktree", "list", "--porcelain"]), before);
}

/// R1-2: the worktree lived inside the parent's own folder, so a process
/// the parent left running could swap it for a link between the pin's
/// check and git's write, and the node's git wrote the parent's files into
/// another folder (7 to 17 of 120 tries). The child's folder is now
/// node-owned, outside every session's writable area: the same swapper,
/// run as many times, never finds it in the parent's folder and never
/// lands.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_swap_during_the_child_folder_fill_writes_into_another_folder() {
    const TRIES: usize = 120;
    let l = layout();
    std::fs::write(l.proj.join("authorized_keys"), "attacker-key\n").unwrap();
    let victim = l.home.join("victim-dot-ssh");
    std::fs::create_dir(&victim).unwrap();
    let sub = sub_repo(&l.proj, Some(&l.base), &l.bounds)
        .await
        .unwrap()
        .unwrap();
    let mut wins = 0;
    for i in 0..TRIES {
        let child = format!("c{i}");
        let base = sub.base(&child).await.unwrap();
        // Where a process of the parent's could find it: anywhere in the
        // parent's folder (it may write all of it).
        let path = l.proj.join(".git/harness-worktrees").join(&child);
        let (p, v) = (path.clone(), victim.clone());
        let delay = std::time::Duration::from_micros(200 + (i as u64 % 30) * 200);
        let swapper = std::thread::spawn(move || {
            let until = std::time::Instant::now() + std::time::Duration::from_millis(300);
            while !p.join(".git").exists() {
                if std::time::Instant::now() > until {
                    return;
                }
                std::thread::yield_now();
            }
            std::thread::sleep(delay);
            let away = p.with_extension("r");
            if std::fs::rename(&p, &away).is_ok() {
                let _ = std::os::unix::fs::symlink(&v, &p);
            }
        });
        let made = sub.make_folder(&child, &base, &sessions_of(&l.base)).await;
        swapper.join().unwrap();
        if let Ok(wt) = made {
            assert!(!wt.path.starts_with(&l.proj), "{:?}", wt.path);
            remove_folder(&wt).await;
        }
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(path.with_extension("r"));
        if victim.join("authorized_keys").exists() {
            wins += 1;
            std::fs::remove_file(victim.join("authorized_keys")).unwrap();
            let _ = std::fs::remove_file(victim.join("a.txt"));
        }
    }
    assert_eq!(wins, 0, "{wins} of {TRIES} fills wrote into another folder");
}

// ---- Merging a child's result back, per file, three ways (A.4) ----

/// A child set up in `l.proj`: its base and its folder (`c`).
async fn child_of(l: &Layout, id: &str) -> (SubRepo, String, ChildFolder) {
    let sub = sub_repo(&l.proj, Some(&l.base), &l.bounds)
        .await
        .unwrap()
        .unwrap();
    let base = sub.base(id).await.unwrap();
    let folder = sub
        .make_folder(id, &base, &sessions_of(&l.base))
        .await
        .unwrap();
    (sub, base, folder)
}

/// Ten lines, two "functions" far apart, so changes to each are separate
/// hunks.
fn two_functions(first: &str, last: &str) -> String {
    format!("fn first() {{ {first} }}\n2\n3\n4\n5\n6\n7\n8\n9\nfn last() {{ {last} }}\n")
}

fn commit_file(l: &Layout, name: &[u8], text: &[u8]) {
    use std::os::unix::ffi::OsStrExt;
    std::fs::write(l.proj.join(std::ffi::OsStr::from_bytes(name)), text).unwrap();
    sh(&l.proj, &["add", "-A"]);
    sh(&l.proj, &["commit", "-qm", "f"]);
}

/// Different parts of one file: both changes land, and the note can say
/// it was merged.
#[tokio::test]
async fn changes_to_different_parts_of_a_file_both_land() {
    let l = layout();
    commit_file(&l, b"f.rs", two_functions("a", "z").as_bytes());
    let (sub, base, c) = child_of(&l, "m1").await;
    std::fs::write(c.path.join("f.rs"), two_functions("a", "CHILD")).unwrap();
    std::fs::write(c.path.join("new.txt"), "from the child\n").unwrap();
    let result = sub.result(&c, "m1").await.unwrap();
    // The folder (another child landed first) changed the other function.
    std::fs::write(l.proj.join("f.rs"), two_functions("FOLDER", "z")).unwrap();
    let a = sub.apply(&base, &result, "m1").await.unwrap();
    assert_eq!(
        std::fs::read_to_string(l.proj.join("f.rs")).unwrap(),
        two_functions("FOLDER", "CHILD")
    );
    assert_eq!(
        std::fs::read_to_string(l.proj.join("new.txt")).unwrap(),
        "from the child\n"
    );
    assert_eq!(a.written, 2, "{a:?}");
    assert_eq!(a.merged.shown, vec!["f.rs".to_string()], "{a:?}");
}

/// Overlapping changes: nothing of the child is written, not even its
/// other files, and the conflict names the file.
#[tokio::test]
async fn overlapping_changes_write_nothing_of_the_child() {
    let l = layout();
    commit_file(&l, b"f.rs", two_functions("a", "z").as_bytes());
    let (sub, base, c) = child_of(&l, "m2").await;
    std::fs::write(c.path.join("f.rs"), two_functions("CHILD", "z")).unwrap();
    std::fs::write(c.path.join("other.txt"), "x\n").unwrap();
    let result = sub.result(&c, "m2").await.unwrap();
    std::fs::write(l.proj.join("f.rs"), two_functions("FOLDER", "z")).unwrap();
    let e = sub.apply(&base, &result, "m2").await.unwrap_err();
    assert_eq!(
        e,
        NotApplied::Conflict(vec![("f.rs".into(), "both changed the same lines")])
    );
    assert_eq!(
        std::fs::read_to_string(l.proj.join("f.rs")).unwrap(),
        two_functions("FOLDER", "z")
    );
    assert!(!l.proj.join("other.txt").exists());
}

/// Each case the merge does not combine is a conflict, never a silent pick
/// of one side; what only one side changed is taken.
#[tokio::test]
async fn binary_deleted_link_added_and_oversized_cases_are_explicit() {
    let big = "line\n".repeat((merge::MERGE_BYTES as usize / 5) + 10);
    let cases: Vec<(&str, &str)> = vec![
        ("binary", "binary content changed on both sides"),
        ("deleted", "deleted on one side, changed on the other"),
        ("link", "a link or nested repository changed on both sides"),
        ("added", "added on both sides with different content"),
        (
            "big",
            "too large to merge line by line (over 1 MiB) and changed on both sides",
        ),
    ];
    for (case, why) in cases {
        let l = layout();
        let p = |d: &Path| d.join("f");
        match case {
            "binary" => commit_file(&l, b"f", b"\0bin\n1\n2\n3\n4\n5\n6\n7\nend\n"),
            "deleted" => commit_file(&l, b"f", two_functions("a", "z").as_bytes()),
            "link" => {
                std::os::unix::fs::symlink("one", p(&l.proj)).unwrap();
                sh(&l.proj, &["add", "-A"]);
                sh(&l.proj, &["commit", "-qm", "f"]);
            }
            "big" => commit_file(&l, b"f", format!("top\n{big}bottom\n").as_bytes()),
            _ => {}
        }
        let (sub, base, c) = child_of(&l, "m3").await;
        match case {
            "binary" => std::fs::write(p(&c.path), b"\0bin\n1\n2\n3\n4\n5\n6\n7\nCHILD\n").unwrap(),
            "deleted" => std::fs::remove_file(p(&c.path)).unwrap(),
            "link" => {
                std::fs::remove_file(p(&c.path)).unwrap();
                std::os::unix::fs::symlink("child", p(&c.path)).unwrap();
            }
            "added" => std::fs::write(p(&c.path), "child\n").unwrap(),
            "big" => std::fs::write(p(&c.path), format!("top\n{big}CHILD\n")).unwrap(),
            _ => unreachable!(),
        }
        let result = sub.result(&c, "m3").await.unwrap();
        match case {
            "binary" => std::fs::write(p(&l.proj), b"\0BIN\n1\n2\n3\n4\n5\n6\n7\nend\n").unwrap(),
            "deleted" => std::fs::write(p(&l.proj), two_functions("FOLDER", "z")).unwrap(),
            "link" => {
                std::fs::remove_file(p(&l.proj)).unwrap();
                std::os::unix::fs::symlink("folder", p(&l.proj)).unwrap();
            }
            "added" => std::fs::write(p(&l.proj), "folder\n").unwrap(),
            "big" => std::fs::write(p(&l.proj), format!("TOP\n{big}bottom\n")).unwrap(),
            _ => unreachable!(),
        }
        let before = std::fs::symlink_metadata(p(&l.proj))
            .ok()
            .map(|m| m.file_type().is_symlink());
        let e = sub.apply(&base, &result, "m3").await.unwrap_err();
        assert_eq!(e, NotApplied::Conflict(vec![("f".into(), why)]), "{case}");
        assert_eq!(
            std::fs::symlink_metadata(p(&l.proj))
                .ok()
                .map(|m| m.file_type().is_symlink()),
            before,
            "{case}"
        );
        remove_folder(&c).await;
    }
}

/// Only one side changed it: the child's binary file, its deletion, its new
/// link and its mode change are taken as they are.
#[tokio::test]
async fn what_only_the_child_changed_is_taken_whatever_it_is() {
    use std::os::unix::fs::PermissionsExt;
    let l = layout();
    commit_file(&l, b"gone.txt", b"bye\n");
    commit_file(&l, b"run.sh", b"echo\n");
    let (sub, base, c) = child_of(&l, "m4").await;
    std::fs::write(c.path.join("bin.dat"), b"\0\x01\x02").unwrap();
    std::fs::remove_file(c.path.join("gone.txt")).unwrap();
    std::os::unix::fs::symlink("a.txt", c.path.join("link")).unwrap();
    std::fs::set_permissions(
        c.path.join("run.sh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let result = sub.result(&c, "m4").await.unwrap();
    // The folder changed only a file the child did not touch: kept.
    std::fs::write(l.proj.join("a.txt"), "folder's\n").unwrap();
    let a = sub.apply(&base, &result, "m4").await.unwrap();
    assert_eq!(a.written, 4, "{a:?}");
    assert!(a.merged.is_empty());
    assert_eq!(
        std::fs::read(l.proj.join("bin.dat")).unwrap(),
        b"\0\x01\x02"
    );
    assert!(!l.proj.join("gone.txt").exists());
    assert_eq!(
        std::fs::read_link(l.proj.join("link")).unwrap(),
        Path::new("a.txt")
    );
    assert_eq!(
        std::fs::metadata(l.proj.join("run.sh"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0o111
    );
    assert_eq!(
        std::fs::read_to_string(l.proj.join("a.txt")).unwrap(),
        "folder's\n"
    );
}

/// A hostile repository: a merge driver, a filter and a diff driver in its
/// config, asked for by `.gitattributes`, files named like options and a
/// path with a line break. Nothing runs, the names are names, and the
/// merge still works.
#[tokio::test]
async fn a_hostile_repository_runs_nothing_and_odd_names_merge() {
    use std::os::unix::fs::PermissionsExt;
    let l = layout();
    // Committed by the test's own git before the repository turns hostile.
    let names: [&[u8]; 4] = [b"--theirs", b"-p", b"a\nb.txt", b":(glob)*"];
    for n in names {
        commit_file(&l, n, two_functions("a", "z").as_bytes());
    }
    let marks = l.home.join("marks");
    std::fs::create_dir(&marks).unwrap();
    let script = l.home.join("evil.sh");
    std::fs::write(
        &script,
        format!("#!/bin/sh\ntouch '{}/ran'\ncat\n", marks.display()),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let s = script.display().to_string();
    for (k, v) in [
        ("merge.evil.driver", format!("{s} %O %A %B")),
        ("merge.evil.name", "evil".into()),
        ("filter.evil.clean", s.clone()),
        ("filter.evil.smudge", s.clone()),
        ("diff.evil.command", s.clone()),
        ("diff.evil.textconv", s.clone()),
        ("merge.default", "evil".into()),
    ] {
        sh(&l.proj, &["config", k, &v]);
    }
    std::fs::write(
        l.proj.join(".gitattributes"),
        "* merge=evil filter=evil diff=evil\n",
    )
    .unwrap();
    let (sub, base, c) = child_of(&l, "m5").await;
    use std::os::unix::ffi::OsStrExt;
    let at = |d: &Path, n: &[u8]| d.join(std::ffi::OsStr::from_bytes(n));
    for n in names {
        std::fs::write(at(&c.path, n), two_functions("a", "CHILD")).unwrap();
    }
    let result = sub.result(&c, "m5").await.unwrap();
    for n in names {
        std::fs::write(at(&l.proj, n), two_functions("FOLDER", "z")).unwrap();
    }
    let a = sub.apply(&base, &result, "m5").await.unwrap();
    assert_eq!(a.written, 4, "{a:?}");
    for n in names {
        assert_eq!(
            std::fs::read_to_string(at(&l.proj, n)).unwrap(),
            two_functions("FOLDER", "CHILD"),
            "{}",
            String::from_utf8_lossy(n)
        );
    }
    // The shown name of the one with a line break has no line break.
    assert!(a.merged.shown.iter().all(|n| !n.contains('\n')), "{a:?}");
    let ran: Vec<_> = std::fs::read_dir(&marks).unwrap().collect();
    assert!(ran.is_empty(), "the repository's config ran");
    remove_folder(&c).await;

    // A conflict on the odd name: listed without its line break.
    let (sub, base, c) = child_of(&l, "m6").await;
    std::fs::write(at(&c.path, b"a\nb.txt"), two_functions("CHILD", "CHILD")).unwrap();
    let result = sub.result(&c, "m6").await.unwrap();
    std::fs::write(at(&l.proj, b"a\nb.txt"), two_functions("OTHER", "CHILD")).unwrap();
    let e = sub.apply(&base, &result, "m6").await.unwrap_err();
    assert!(!e.text().contains('\n'), "{e:?}");
    assert!(e.text().contains("a\u{FFFD}b.txt"), "{e:?}");
    assert!(std::fs::read_dir(&marks).unwrap().next().is_none());
}

/// A file the person (or another chat) edits while the merge runs is not
/// overwritten, and nothing else of the child is written either.
#[tokio::test]
async fn a_file_edited_during_the_merge_is_not_overwritten() {
    let l = layout();
    commit_file(&l, b"f.rs", two_functions("a", "z").as_bytes());
    let (sub, base, c) = child_of(&l, "m7").await;
    std::fs::write(c.path.join("f.rs"), two_functions("a", "CHILD")).unwrap();
    std::fs::write(c.path.join("new.txt"), "n\n").unwrap();
    let result = sub.result(&c, "m7").await.unwrap();
    std::fs::write(l.proj.join("f.rs"), two_functions("FOLDER", "z")).unwrap();
    let f = l.proj.join("f.rs");
    let person = move || std::fs::write(&f, "the person's edit\n").unwrap();
    let e = sub
        .apply_with(
            &base,
            &result,
            "m7",
            merge::Hooks {
                between: Some(&person),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert_eq!(
        e,
        NotApplied::Conflict(vec![(
            "f.rs".into(),
            "changed in the folder while the merge ran"
        )])
    );
    assert_eq!(
        std::fs::read_to_string(l.proj.join("f.rs")).unwrap(),
        "the person's edit\n"
    );
    assert!(!l.proj.join("new.txt").exists());
}

// ---- From the independent review of PR 118 ----

/// A write that fails partway (a folder made unwritable, review of PR 118
/// H2): what was written is put back from the snapshot taken right before,
/// so "nothing was written" is true.
#[tokio::test]
async fn a_failure_partway_is_put_back() {
    use std::os::unix::fs::PermissionsExt;
    let l = layout();
    let (sub, base, c) = child_of(&l, "r1").await;
    std::fs::write(c.path.join("a.txt"), "child\n").unwrap();
    std::fs::create_dir(c.path.join("z")).unwrap();
    std::fs::write(c.path.join("z/b.txt"), "child b\n").unwrap();
    let result = sub.result(&c, "r1").await.unwrap();
    // The person has an (empty, so unsnapshotted) folder `z`, read-only.
    std::fs::create_dir(l.proj.join("z")).unwrap();
    std::fs::set_permissions(l.proj.join("z"), std::fs::Permissions::from_mode(0o555)).unwrap();
    let r = sub.apply(&base, &result, "r1").await;
    std::fs::set_permissions(l.proj.join("z"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let a = std::fs::read_to_string(l.proj.join("a.txt")).unwrap();
    match &r {
        Err(e @ NotApplied::Failed(t)) => {
            assert!(e.nothing_written() && t.contains("put back"), "{t}")
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(a, "one\n", "the first file was put back");
    assert!(!l.proj.join("z/b.txt").exists());
}

/// The same failure, and putting back fails too (the folder's top made
/// read-only before it): a distinct outcome naming the file that holds
/// the child's version and the snapshot of the folder before.
#[tokio::test]
async fn a_failed_put_back_says_what_was_written() {
    use std::os::unix::fs::PermissionsExt;
    let l = layout();
    let (sub, base, c) = child_of(&l, "r1b").await;
    std::fs::write(c.path.join("a.txt"), "child\n").unwrap();
    std::fs::create_dir(c.path.join("z")).unwrap();
    std::fs::write(c.path.join("z/b.txt"), "child b\n").unwrap();
    let result = sub.result(&c, "r1b").await.unwrap();
    std::fs::create_dir(l.proj.join("z")).unwrap();
    std::fs::set_permissions(l.proj.join("z"), std::fs::Permissions::from_mode(0o555)).unwrap();
    let top = l.proj.clone();
    let lock =
        move || std::fs::set_permissions(&top, std::fs::Permissions::from_mode(0o555)).unwrap();
    let r = sub
        .apply_with(
            &base,
            &result,
            "r1b",
            merge::Hooks {
                before_put_back: Some(&lock),
                ..Default::default()
            },
        )
        .await;
    std::fs::set_permissions(&l.proj, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(l.proj.join("z"), std::fs::Permissions::from_mode(0o755)).unwrap();
    match &r {
        Err(
            e @ NotApplied::Partial {
                written, before, ..
            },
        ) => {
            assert!(!e.nothing_written());
            assert_eq!(written.shown, vec!["a.txt".to_string()], "{e:?}");
            assert!(e.text().contains(before.as_str()), "{}", e.text());
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        std::fs::read_to_string(l.proj.join("a.txt")).unwrap(),
        "child\n"
    );
}

// ---- From the re-check of PR 118 ----

/// Every file, folder and link under `root` but `.git`: kind, mode and
/// content, so a leftover empty folder or a changed mode shows.
fn tree(root: &Path) -> std::collections::BTreeMap<String, String> {
    use std::os::unix::fs::PermissionsExt;
    walkdir::WalkDir::new(root)
        .min_depth(1)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.file_name() != ".git")
        .map(|e| e.unwrap())
        .map(|e| {
            let rel = e.path().strip_prefix(root).unwrap().display().to_string();
            let t = e.file_type();
            let what = if t.is_dir() {
                "folder".to_string()
            } else if t.is_symlink() {
                format!("link {}", std::fs::read_link(e.path()).unwrap().display())
            } else {
                let mode = e.metadata().unwrap().permissions().mode() & 0o777;
                format!("{mode:o} {}", std::fs::read_to_string(e.path()).unwrap())
            };
            (rel, what)
        })
        .collect()
}

/// RC1: the put-back after a failed write restored every path the write
/// meant to change from the snapshot before it, with no look at the disk,
/// so a file the person edited between the failure and the put-back was
/// reverted and the outcome still said "put back as they were". Now only
/// what still holds the child's version is put back; a file that changed
/// meanwhile is left as it is and named.
#[tokio::test]
async fn a_put_back_leaves_what_changed_after_the_failed_write() {
    use std::os::unix::fs::PermissionsExt;
    let l = layout();
    commit_file(&l, b"m.txt", b"m\n");
    commit_file(&l, b"zz.txt", b"zz\n");
    let (sub, base, c) = child_of(&l, "rc1").await;
    std::fs::write(c.path.join("a.txt"), "child\n").unwrap();
    std::fs::write(c.path.join("m.txt"), "child m\n").unwrap();
    std::fs::create_dir(c.path.join("z")).unwrap();
    std::fs::write(c.path.join("z/b.txt"), "child b\n").unwrap();
    std::fs::write(c.path.join("zz.txt"), "child zz\n").unwrap();
    let result = sub.result(&c, "rc1").await.unwrap();
    std::fs::create_dir(l.proj.join("z")).unwrap();
    std::fs::set_permissions(l.proj.join("z"), std::fs::Permissions::from_mode(0o555)).unwrap();
    // `a.txt` was written by the child, `zz.txt` never reached; the person
    // edits both before the put-back.
    let (a, zz) = (l.proj.join("a.txt"), l.proj.join("zz.txt"));
    let person = move || {
        std::fs::write(&a, "person a\n").unwrap();
        std::fs::write(&zz, "person\n").unwrap();
    };
    let r = sub
        .apply_with(
            &base,
            &result,
            "rc1",
            merge::Hooks {
                before_put_back: Some(&person),
                ..Default::default()
            },
        )
        .await;
    std::fs::set_permissions(l.proj.join("z"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let read = |p: &str| std::fs::read_to_string(l.proj.join(p)).unwrap();
    assert_eq!(read("zz.txt"), "person\n", "the put-back overwrote an edit");
    assert_eq!(
        read("a.txt"),
        "person a\n",
        "the put-back overwrote an edit"
    );
    assert_eq!(read("m.txt"), "m\n", "the child's file was put back");
    match &r {
        Err(e @ NotApplied::Failed(t)) => {
            assert!(e.nothing_written(), "{t}");
            assert!(
                t.contains("put back as they were: `m.txt`")
                    && t.contains("left as they are: `a.txt`, `zz.txt`"),
                "{t}"
            );
        }
        other => panic!("{other:?}"),
    }
}

/// What the re-check did not reach: the put-back removes the files and
/// folders the failed write made (and only those), restores a mode the
/// write changed, and the turn's Undo afterwards still puts the folder
/// back exactly as it was before the turn.
#[tokio::test]
async fn a_put_back_removes_only_what_the_failed_write_made_and_undo_is_exact() {
    use std::os::unix::fs::PermissionsExt;
    let l = layout();
    commit_file(&l, b"m.sh", b"m\n");
    std::fs::create_dir(l.proj.join("keep")).unwrap();
    commit_file(&l, b"keep/k.txt", b"k\n");
    let turn = checkpoint_repo(&l.proj, Some(&l.base), &l.bounds)
        .await
        .unwrap()
        .unwrap();
    // The person's own empty folder `z`, read-only: the write fails there.
    std::fs::create_dir(l.proj.join("z")).unwrap();
    std::fs::set_permissions(l.proj.join("z"), std::fs::Permissions::from_mode(0o555)).unwrap();
    let start = tree(&l.proj);
    let before = snapshot(&turn, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    let (sub, base, c) = child_of(&l, "rc6").await;
    // The parent's own change in the same turn.
    std::fs::write(l.proj.join("keep/k.txt"), "parent\n").unwrap();
    std::fs::create_dir_all(c.path.join("a/b")).unwrap();
    std::fs::write(c.path.join("a/b/new.txt"), "new\n").unwrap();
    std::fs::write(c.path.join("keep/n.txt"), "n\n").unwrap();
    std::fs::set_permissions(c.path.join("m.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::create_dir(c.path.join("z")).unwrap();
    std::fs::write(c.path.join("z/b.txt"), "child b\n").unwrap();
    let result = sub.result(&c, "rc6").await.unwrap();
    let r = sub.apply(&base, &result, "rc6").await;
    match &r {
        Err(e @ NotApplied::Failed(t)) => assert!(
            e.nothing_written()
                && t.contains("put back as they were: `a/b/new.txt`, `keep/n.txt`, `m.sh`"),
            "{t}"
        ),
        other => panic!("{other:?}"),
    }
    let mut want = start.clone();
    want.insert("keep/k.txt".into(), "644 parent\n".into());
    assert_eq!(tree(&l.proj), want, "exactly the folder before the write");
    let after = snapshot(&turn, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    restore(&turn, &before, &after, false).await.unwrap();
    std::fs::set_permissions(l.proj.join("z"), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(tree(&l.proj), start, "Undo put the folder back exactly");
}

/// RC2: a child that replaces a tracked folder by a file of the same name
/// was refused with "a file of yours that git ignores is in the way" (the
/// folder the plan itself empties). It lands.
#[tokio::test]
async fn a_child_replacing_a_folder_by_a_file_lands() {
    let l = layout();
    std::fs::create_dir(l.proj.join("d")).unwrap();
    commit_file(&l, b"d/x", b"x\n");
    let (sub, base, c) = child_of(&l, "rc2").await;
    std::fs::remove_file(c.path.join("d/x")).unwrap();
    std::fs::remove_dir(c.path.join("d")).unwrap();
    std::fs::write(c.path.join("d"), "file\n").unwrap();
    let result = sub.result(&c, "rc2").await.unwrap();
    let r = sub.apply(&base, &result, "rc2").await;
    assert_eq!(r.unwrap().written, 2);
    assert_eq!(std::fs::read_to_string(l.proj.join("d")).unwrap(), "file\n");
}

/// The same, and the folder also holds an ignored file of the person's:
/// that file is in the way, named; nothing is written and it stays.
#[tokio::test]
async fn a_child_replacing_a_folder_with_an_ignored_file_in_it_is_a_conflict() {
    let l = layout();
    commit_file(&l, b".gitignore", b"*.log\n");
    std::fs::create_dir(l.proj.join("d")).unwrap();
    commit_file(&l, b"d/x", b"x\n");
    std::fs::write(l.proj.join("d/keep.log"), "person\n").unwrap();
    let (sub, base, c) = child_of(&l, "rc2b").await;
    std::fs::remove_file(c.path.join("d/x")).unwrap();
    std::fs::remove_dir(c.path.join("d")).unwrap();
    std::fs::write(c.path.join("d"), "file\n").unwrap();
    let result = sub.result(&c, "rc2b").await.unwrap();
    let r = sub.apply(&base, &result, "rc2b").await;
    assert_eq!(
        r,
        Err(NotApplied::Conflict(vec![(
            "d/keep.log".into(),
            "a file of yours that git ignores is in the way"
        )]))
    );
    assert_eq!(std::fs::read_to_string(l.proj.join("d/x")).unwrap(), "x\n");
    assert_eq!(
        std::fs::read_to_string(l.proj.join("d/keep.log")).unwrap(),
        "person\n"
    );
}

/// A dangling, ignored link in the way is a conflict and stays.
#[tokio::test]
async fn a_dangling_ignored_link_in_the_way_is_a_conflict() {
    let l = layout();
    commit_file(&l, b".gitignore", b".env\n");
    std::os::unix::fs::symlink("/nonexistent-rc3-target", l.proj.join(".env")).unwrap();
    let (sub, base, c) = child_of(&l, "rc3").await;
    std::fs::write(c.path.join(".gitignore"), "").unwrap();
    std::fs::write(c.path.join(".env"), "CHILD=1\n").unwrap();
    let result = sub.result(&c, "rc3").await.unwrap();
    let r = sub.apply(&base, &result, "rc3").await;
    assert!(matches!(r, Err(NotApplied::Conflict(_))), "{r:?}");
    assert_eq!(
        std::fs::read_link(l.proj.join(".env")).unwrap(),
        Path::new("/nonexistent-rc3-target")
    );
}

/// Blob content that looks like `cat-file --batch` headers does not
/// desynchronise the reader; a clean three-way merge still lands.
#[tokio::test]
async fn batch_header_lookalikes_in_content_merge_cleanly() {
    let l = layout();
    let fake = "0123456789012345678901234567890123456789 blob 2\n";
    let body = |a: &str, z: &str| format!("{a}\n{fake}1\n2\n3\n{fake}4\n5\n6\n{fake}{z}\n");
    commit_file(&l, b"f.txt", body("a", "z").as_bytes());
    let (sub, base, c) = child_of(&l, "rc4").await;
    std::fs::write(c.path.join("f.txt"), body("a", "CHILD")).unwrap();
    let result = sub.result(&c, "rc4").await.unwrap();
    std::fs::write(l.proj.join("f.txt"), body("FOLDER", "z")).unwrap();
    let a = sub.apply(&base, &result, "rc4").await.unwrap();
    assert_eq!(a.written, 1);
    assert_eq!(
        std::fs::read_to_string(l.proj.join("f.txt")).unwrap(),
        body("FOLDER", "CHILD")
    );
}

/// An ordinary new file in a new folder still lands (no false positive of
/// the in-the-way check).
#[tokio::test]
async fn an_ordinary_new_file_lands() {
    let l = layout();
    let (sub, base, c) = child_of(&l, "rc5").await;
    std::fs::create_dir(c.path.join("n")).unwrap();
    std::fs::write(c.path.join("n/new.txt"), "n\n").unwrap();
    let result = sub.result(&c, "rc5").await.unwrap();
    let r = sub.apply(&base, &result, "rc5").await;
    assert_eq!(r.unwrap().written, 1);
    assert_eq!(
        std::fs::read_to_string(l.proj.join("n/new.txt")).unwrap(),
        "n\n"
    );
}

/// A child that un-ignores a path and creates it there (review of PR 118
/// H3): the person's ignored file is in the way, a conflict; nothing of
/// the child is written and the file keeps its content.
#[tokio::test]
async fn a_child_unignoring_a_path_does_not_overwrite_the_persons_ignored_file() {
    let l = layout();
    commit_file(&l, b".gitignore", b".env\n");
    std::fs::write(l.proj.join(".env"), "PERSON=secret-value\n").unwrap();
    let (sub, base, c) = child_of(&l, "r2").await;
    assert!(
        !c.path.join(".env").exists(),
        "the child never saw the person's .env"
    );
    std::fs::write(c.path.join(".gitignore"), "").unwrap();
    std::fs::write(c.path.join(".env"), "CHILD=1\n").unwrap();
    let result = sub.result(&c, "r2").await.unwrap();
    let r = sub.apply(&base, &result, "r2").await;
    assert_eq!(
        r,
        Err(NotApplied::Conflict(vec![(
            ".env".into(),
            "a file of yours that git ignores is in the way"
        )]))
    );
    assert_eq!(
        std::fs::read_to_string(l.proj.join(".env")).unwrap(),
        "PERSON=secret-value\n"
    );
    assert_eq!(
        std::fs::read_to_string(l.proj.join(".gitignore")).unwrap(),
        ".env\n"
    );
}

/// A file the child creates under an ignored pattern is not brought back,
/// and the parent can be told which (`SubRepo::ignored`; the note).
#[tokio::test]
async fn a_childs_ignored_new_file_is_named() {
    let l = layout();
    commit_file(&l, b".gitignore", b"*.log\nbuild/\n");
    let (sub, base, c) = child_of(&l, "r3").await;
    std::fs::write(c.path.join("report.log"), "work\n").unwrap();
    std::fs::create_dir(c.path.join("build")).unwrap();
    std::fs::write(c.path.join("build/out.o"), "o").unwrap();
    let result = sub.result(&c, "r3").await.unwrap();
    let ignored = sub.ignored(&c).await;
    assert_eq!(
        ignored.shown,
        vec!["build/".to_string(), "report.log".to_string()]
    );
    let r = sub.apply(&base, &result, "r3").await;
    assert_eq!(r, Ok(Applied::default()));
    assert!(!l.proj.join("report.log").exists());
}

fn three_parts(a: &str, m: &str, z: &str) -> String {
    format!("fn a() {{ {a} }}\n1\n2\n3\n4\n5\nfn m() {{ {m} }}\n6\n7\n8\n9\n10\nfn z() {{ {z} }}\n")
}

/// R4: three children of one base on one file, A (part a), B (part z),
/// C (part a, differently), applied in every order: what was applied is
/// never lost, exactly one of A and C is refused, B always lands.
#[tokio::test]
async fn three_children_in_every_order_never_lose_an_applied_change() {
    let orders: [[usize; 3]; 6] = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    for order in orders {
        let l = layout();
        commit_file(&l, b"f.rs", three_parts("a", "m", "z").as_bytes());
        let mut kids = Vec::new();
        for (i, want) in [
            three_parts("A", "m", "z"),
            three_parts("a", "m", "B"),
            three_parts("C", "m", "z"),
        ]
        .iter()
        .enumerate()
        {
            let id = format!("r4-{i}");
            let (sub, base, c) = child_of(&l, &id).await;
            std::fs::write(c.path.join("f.rs"), want).unwrap();
            let result = sub.result(&c, &id).await.unwrap();
            kids.push((sub, base, result, id, c));
        }
        let mut applied = Vec::new();
        for i in order {
            let (sub, base, result, id, _) = &kids[i];
            if sub.apply(base, result, id).await.is_ok() {
                applied.push(i);
            }
        }
        let now = std::fs::read_to_string(l.proj.join("f.rs")).unwrap();
        let first = if applied.contains(&0) { "A" } else { "C" };
        eprintln!(
            "R4 order {order:?}: applied {applied:?}; f.rs first line {:?}",
            now.lines().next()
        );
        assert_eq!(applied.len(), 2, "{order:?}");
        assert!(applied.contains(&1));
        assert_eq!(now, three_parts(first, "m", "B"), "{order:?}");
        for (_, _, _, _, c) in &kids {
            remove_folder(c).await;
        }
    }
}

/// R4b: two applies started at once on one folder are serialised (both
/// disjoint changes land).
#[tokio::test]
async fn two_applies_at_once_are_serialised() {
    let l = layout();
    commit_file(&l, b"f.rs", three_parts("a", "m", "z").as_bytes());
    let (s1, b1, c1) = child_of(&l, "r4b1").await;
    let (s2, b2, c2) = child_of(&l, "r4b2").await;
    std::fs::write(c1.path.join("f.rs"), three_parts("ONE", "m", "z")).unwrap();
    std::fs::write(c2.path.join("f.rs"), three_parts("a", "m", "TWO")).unwrap();
    let r1 = s1.result(&c1, "r4b1").await.unwrap();
    let r2 = s2.result(&c2, "r4b2").await.unwrap();
    let (x, y) = tokio::join!(s1.apply(&b1, &r1, "r4b1"), s2.apply(&b2, &r2, "r4b2"));
    assert!(x.is_ok() && y.is_ok());
    assert_eq!(
        std::fs::read_to_string(l.proj.join("f.rs")).unwrap(),
        three_parts("ONE", "m", "TWO")
    );
}

/// R5: content cases the table does not name, each changed on both sides.
#[tokio::test]
async fn unnamed_content_cases_merge_correctly_or_conflict() {
    use std::os::unix::fs::PermissionsExt;
    let ten = |first: &str, last: &str, eol: &str| {
        let mut s = format!("{first}{eol}");
        for i in 2..10 {
            s.push_str(&format!("{i}{eol}"));
        }
        s.push_str(&format!("{last}{eol}"));
        s
    };
    // (name, base, child, folder, expected Ok content or Err reason)
    let utf16 = |s: &str| -> Vec<u8> { s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect() };
    let mut latin_b = b"caf\xe9 a\n".to_vec();
    latin_b.extend_from_slice(b"2\n3\n4\n5\n6\n7\n8\n9\n\xff z\n");
    let mut latin_c = b"caf\xe9 a\n".to_vec();
    latin_c.extend_from_slice(b"2\n3\n4\n5\n6\n7\n8\n9\n\xff CHILD\n");
    let mut latin_n = b"caf\xe9 FOLDER\n".to_vec();
    latin_n.extend_from_slice(b"2\n3\n4\n5\n6\n7\n8\n9\n\xff z\n");
    let mut latin_m = b"caf\xe9 FOLDER\n".to_vec();
    latin_m.extend_from_slice(b"2\n3\n4\n5\n6\n7\n8\n9\n\xff CHILD\n");
    type Case = (
        &'static str,
        Vec<u8>,
        Vec<u8>,
        Vec<u8>,
        Result<Vec<u8>, &'static str>,
    );
    let cases: Vec<Case> = vec![
        (
            "crlf",
            ten("a", "z", "\r\n").into(),
            ten("a", "CHILD", "\r\n").into(),
            ten("FOLDER", "z", "\r\n").into(),
            Ok(ten("FOLDER", "CHILD", "\r\n").into()),
        ),
        (
            "crlf-vs-lf",
            ten("a", "z", "\n").into(),
            ten("a", "z", "\r\n").into(),
            ten("FOLDER", "z", "\n").into(),
            Err("both changed the same lines"),
        ),
        (
            "no-final-newline",
            ten("a", "z", "\n").trim_end().as_bytes().to_vec(),
            ten("a", "CHILD", "\n").trim_end().as_bytes().to_vec(),
            ten("FOLDER", "z", "\n").trim_end().as_bytes().to_vec(),
            Ok(ten("FOLDER", "CHILD", "\n").trim_end().as_bytes().to_vec()),
        ),
        (
            "empty-base",
            b"".to_vec(),
            b"child\n".to_vec(),
            b"folder\n".to_vec(),
            Err("both changed the same lines"),
        ),
        (
            "to-empty-vs-edit",
            ten("a", "z", "\n").into(),
            b"".to_vec(),
            ten("FOLDER", "z", "\n").into(),
            Err("both changed the same lines"),
        ),
        (
            "utf16",
            utf16(&ten("a", "z", "\n")),
            utf16(&ten("a", "CHILD", "\n")),
            utf16(&ten("FOLDER", "z", "\n")),
            Err("binary content changed on both sides"),
        ),
        ("invalid-utf8", latin_b, latin_c, latin_n, Ok(latin_m)),
    ];
    for (name, b, c_, n, want) in cases {
        let l = layout();
        commit_file(&l, b"f", &b);
        let (sub, base, c) = child_of(&l, "r5").await;
        std::fs::write(c.path.join("f"), &c_).unwrap();
        std::fs::write(c.path.join("side.txt"), "side\n").unwrap();
        let result = sub.result(&c, "r5").await.unwrap();
        std::fs::write(l.proj.join("f"), &n).unwrap();
        let r = sub.apply(&base, &result, "r5").await;
        let now = std::fs::read(l.proj.join("f")).unwrap();
        match want {
            Ok(m) => {
                assert!(r.is_ok(), "{name}: {r:?}");
                assert_eq!(now, m, "{name}: {}", String::from_utf8_lossy(&now));
            }
            Err(why) => {
                assert_eq!(
                    r,
                    Err(NotApplied::Conflict(vec![("f".into(), why)])),
                    "{name}"
                );
                assert_eq!(now, n, "{name}: folder changed");
                assert!(!l.proj.join("side.txt").exists(), "{name}");
            }
        }
        remove_folder(&c).await;
    }

    // Mode on one side, content on the other (both directions).
    for child_mode in [true, false] {
        let l = layout();
        commit_file(&l, b"run.sh", ten("a", "z", "\n").as_bytes());
        let (sub, base, c) = child_of(&l, "r5m").await;
        let (cp, np) = (c.path.join("run.sh"), l.proj.join("run.sh"));
        if child_mode {
            std::fs::set_permissions(&cp, std::fs::Permissions::from_mode(0o755)).unwrap();
        } else {
            std::fs::write(&cp, ten("a", "CHILD", "\n")).unwrap();
        }
        let result = sub.result(&c, "r5m").await.unwrap();
        if child_mode {
            std::fs::write(&np, ten("FOLDER", "z", "\n")).unwrap();
        } else {
            std::fs::set_permissions(&np, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let r = sub.apply(&base, &result, "r5m").await;
        let mode = std::fs::metadata(&np).unwrap().permissions().mode() & 0o111;
        let text = std::fs::read_to_string(&np).unwrap();
        assert!(r.is_ok());
        assert_eq!(mode, 0o111, "child_mode={child_mode}: exec bit lost");
        let want = if child_mode {
            ten("FOLDER", "z", "\n")
        } else {
            ten("a", "CHILD", "\n")
        };
        assert_eq!(text, want, "child_mode={child_mode}");
        remove_folder(&c).await;
    }

    // A file replaced by a folder of files in the child, edited in the folder.
    let l = layout();
    commit_file(&l, b"d", ten("a", "z", "\n").as_bytes());
    let (sub, base, c) = child_of(&l, "r5d").await;
    std::fs::remove_file(c.path.join("d")).unwrap();
    std::fs::create_dir(c.path.join("d")).unwrap();
    std::fs::write(c.path.join("d/x"), "x\n").unwrap();
    let result = sub.result(&c, "r5d").await.unwrap();
    std::fs::write(l.proj.join("d"), ten("FOLDER", "z", "\n")).unwrap();
    let r = sub.apply(&base, &result, "r5d").await;
    match &r {
        Err(NotApplied::Conflict(v)) => assert!(
            v.contains(&("d".into(), "deleted on one side, changed on the other")),
            "{v:?}"
        ),
        other => panic!("{other:?}"),
    }
    assert!(l.proj.join("d").is_file());
}

/// R6: git without `merge-file --object-id` (older than 2.43), faked by a
/// stand-in that fails that call like an old git would, with a path
/// outside the folder in its message.
#[tokio::test]
async fn an_old_git_makes_a_two_sided_change_a_conflict_without_raw_text() {
    use std::os::unix::fs::PermissionsExt;
    let l = layout();
    commit_file(&l, b"f.rs", two_functions("a", "z").as_bytes());
    let (sub, base, c) = child_of(&l, "r6").await;
    std::fs::write(c.path.join("f.rs"), two_functions("a", "CHILD")).unwrap();
    let result = sub.result(&c, "r6").await.unwrap();
    std::fs::write(l.proj.join("f.rs"), two_functions("FOLDER", "z")).unwrap();
    let fake = l.home.join("oldgit");
    std::fs::write(
        &fake,
        "#!/bin/sh\nfor a; do if [ \"$a\" = merge-file ]; then echo \"error: unknown option \
         \\`object-id' (/home/someone/private/path)\" >&2; exit 129; fi; done\nexec /usr/bin/git \"$@\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    TEST_GIT.with(|g| *g.borrow_mut() = Some(fake));
    let r = sub.apply(&base, &result, "r6").await;
    TEST_GIT.with(|g| *g.borrow_mut() = None);
    assert_eq!(
        r,
        Err(NotApplied::Conflict(vec![(
            "f.rs".into(),
            "git could not merge it"
        )]))
    );
    assert!(!r.unwrap_err().text().contains("/home/someone"));
    assert_eq!(
        std::fs::read_to_string(l.proj.join("f.rs")).unwrap(),
        two_functions("FOLDER", "z")
    );
}

/// Bounded work under the merge lock (review of PR 118, M2): a child
/// with more than `MAX_TWO_SIDED` files changed on both sides is a
/// conflict, nothing merged or written.
#[tokio::test]
async fn too_many_two_sided_files_are_a_conflict() {
    let l = layout();
    let files = merge::MAX_TWO_SIDED + 1;
    for i in 0..files {
        std::fs::write(l.proj.join(format!("f{i}.rs")), two_functions("a", "z")).unwrap();
    }
    sh(&l.proj, &["add", "-A"]);
    sh(&l.proj, &["commit", "-qm", "many"]);
    let (sub, base, c) = child_of(&l, "r7").await;
    for i in 0..files {
        std::fs::write(c.path.join(format!("f{i}.rs")), two_functions("a", "CHILD")).unwrap();
    }
    std::fs::write(c.path.join("new.txt"), "n\n").unwrap();
    let result = sub.result(&c, "r7").await.unwrap();
    for i in 0..files {
        std::fs::write(
            l.proj.join(format!("f{i}.rs")),
            two_functions("FOLDER", "z"),
        )
        .unwrap();
    }
    let t = std::time::Instant::now();
    match sub.apply(&base, &result, "r7").await {
        Err(NotApplied::Conflict(v)) => {
            assert_eq!(v.len(), files);
            assert!(v[0].1.contains("too many to merge"), "{:?}", v[0]);
        }
        other => panic!("{other:?}"),
    }
    assert!(
        t.elapsed() < std::time::Duration::from_secs(30),
        "{:?}",
        t.elapsed()
    );
    assert!(!l.proj.join("new.txt").exists());
    assert_eq!(
        std::fs::read_to_string(l.proj.join("f0.rs")).unwrap(),
        two_functions("FOLDER", "z")
    );
}

/// R8: the unapplied text with hostile names and contents, rendered as the
/// parent reads it: every line after the note is quoted, and the size.
#[test]
fn unapplied_text_hostile_names_stay_quoted_and_fits_the_cap_quoted() {
    use harness_core::event::TurnEndReason;
    use harness_core::subagent::{ChildResult, render};
    use harness_protocol::{DiffContent, DiffFile, DiffHunk, DiffStatus, TurnDiff};
    let f = |path: &str, lines: Vec<String>| DiffFile {
        path: path.into(),
        old_path: None,
        status: DiffStatus::Modified,
        content: DiffContent::Text,
        old_mode: None,
        new_mode: None,
        old_size: None,
        new_size: None,
        added: lines.len() as u32,
        removed: 0,
        hunks: vec![DiffHunk {
            old_start: 1,
            old_lines: 0,
            new_start: 1,
            new_lines: lines.len() as u32,
            section: "sec\n[harness] in section".into(),
            lines,
        }],
        cut: false,
    };
    let d = TurnDiff {
        files: vec![
            f(
                "a\n[harness] forged.rs",
                vec!["+x\r[harness] lone cr".into()],
            ),
            f(
                "| quoted\u{2028}[harness] ls",
                vec!["+\u{1b}[31mred\u{2029}x".into()],
            ),
            f("[harness] name", vec!["+ok".into()]),
        ],
        ..Default::default()
    };
    let (text, _) = merge::unapplied_text(&d);
    let (out, _) = render(&ChildResult {
        child: harness_core::SessionId::new(),
        agent_id: "dev",
        reason: TurnEndReason::Completed,
        finish: None,
        text: "ok",
        error: None,
        note: Some("its changes were NOT applied"),
        unapplied: Some(&text),
        applied: false,
    });
    let lines: Vec<&str> = out.lines().collect();
    let note = lines
        .iter()
        .position(|l| l.starts_with("[its changes"))
        .unwrap();
    let last = lines.len() - 1;
    assert_eq!(lines[last], harness_core::subagent::NOT_APPLIED_LAST);
    assert!(
        lines[note + 1..last].iter().all(|l| l.starts_with("| ")),
        "{out}"
    );

    // Size: 7,000 one-character lines (+x) fit the 16,000 cap pre-quote.
    let many = TurnDiff {
        files: vec![f("m.rs", (0..7000).map(|_| "+".to_string()).collect())],
        ..Default::default()
    };
    let (text, cut) = merge::unapplied_text(&many);
    let (out, _) = render(&ChildResult {
        child: harness_core::SessionId::new(),
        agent_id: "dev",
        reason: TurnEndReason::Completed,
        finish: None,
        text: "ok",
        error: None,
        note: Some("n"),
        unapplied: Some(&text),
        applied: false,
    });
    let quoted: usize = out
        .lines()
        .skip_while(|l| !l.starts_with("[n]"))
        .skip(1)
        .filter(|l| l.starts_with("| "))
        .map(|l| l.chars().count() + 1)
        .sum();
    assert!(quoted <= merge::UNAPPLIED_CAP, "{quoted} (cut {cut:?})");
}
