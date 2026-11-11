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
