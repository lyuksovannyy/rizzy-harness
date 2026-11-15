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
        .apply_with(&base, &result, "m7", &person)
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
