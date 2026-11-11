//! The checkpoint root is found on the file system, never from git's
//! configuration, and nothing a checkpoint lists or an undo writes lies
//! outside it (R.1, S-3; docs/security-review/2026-11-gate3.md).
//!
//! The session's own `.git/config` is writable from the agent's sandboxed
//! shell (PLAN.md R2), and `core.worktree` there moved git's idea of the
//! top level to any ancestor: the node's own git (not sandboxed) then
//! snapshotted that ancestor every turn, named files outside the session
//! folder in the chat's events, and an undo wrote and deleted there.

use super::*;

fn sh(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

/// A home folder with a dotfile and another project, and the session's
/// project, a repository with one commit, inside it.
struct Home {
    _d: tempfile::TempDir,
    base: PathBuf,
    home: PathBuf,
    proj: PathBuf,
    other: PathBuf,
}

fn home() -> Home {
    let d = tempfile::tempdir().unwrap();
    let top = d.path().canonicalize().unwrap();
    let base = top.join("data/checkpoints");
    let home = top.join("home");
    let proj = home.join("proj");
    let other = home.join("other-project");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(home.join(".profile"), "export A=1\n").unwrap();
    std::fs::write(other.join("notes.txt"), "v1\n").unwrap();
    sh(&proj, &["init", "-q", "-b", "main"]);
    std::fs::write(proj.join("a.txt"), "one\n").unwrap();
    sh(&proj, &["add", "."]);
    sh(&proj, &["commit", "-qm", "init"]);
    Home {
        _d: d,
        base,
        home,
        proj,
        other,
    }
}

/// `core.worktree` in the session's own config moved git's top level to any
/// ancestor (S-3). The node no longer asks git: the root is the folder
/// that holds the `.git` directory, and a whole turn (snapshot, change
/// list, undo) stays inside it.
#[tokio::test]
async fn core_worktree_cannot_move_checkpoints_out_of_the_session_folder() {
    let h = home();
    // What the agent's shell does, inside its own folder (one command):
    sh(
        &h.proj,
        &[
            "config",
            "--local",
            "core.worktree",
            h.home.to_str().unwrap(),
        ],
    );
    let repo = repo_for(&h.proj, Some(&h.base)).await.unwrap();
    assert_eq!(repo.root, h.proj, "the checkpoint root left the folder");
    let before = snapshot(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    std::fs::write(h.other.join("notes.txt"), "v2\n").unwrap();
    std::fs::write(h.home.join(".profile"), "export A=2\n").unwrap();
    std::fs::write(h.proj.join("a.txt"), "two\n").unwrap();
    let after = snapshot(&repo, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    let ch: Vec<String> = changed(&repo, &before, &after)
        .await
        .unwrap()
        .into_iter()
        .map(|(_, p)| p)
        .collect();
    assert_eq!(ch, ["a.txt"]);
    restore(&repo, &before, &after, false).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(h.other.join("notes.txt")).unwrap(),
        "v2\n"
    );
    assert_eq!(
        std::fs::read_to_string(h.home.join(".profile")).unwrap(),
        "export A=2\n"
    );
    assert_eq!(
        std::fs::read_to_string(h.proj.join("a.txt")).unwrap(),
        "one\n"
    );
}

/// A `.git` that is not a plain repository directory (a gitfile, a link,
/// one with a `commondir`) is never followed: the session folder is
/// checkpointed as a plain folder, and a writing sub-agent is refused with
/// the reason.
#[tokio::test]
async fn a_git_entry_that_redirects_is_not_used_as_a_repository() {
    for plant in ["gitfile", "link", "commondir"] {
        let h = home();
        let git = h.proj.join(".git");
        match plant {
            "gitfile" => {
                std::fs::rename(&git, h.proj.join("real-git")).unwrap();
                std::fs::write(
                    &git,
                    format!("gitdir: {}\n", h.other.join(".git").display()),
                )
                .unwrap();
            }
            "link" => {
                std::fs::remove_dir_all(&git).unwrap();
                std::os::unix::fs::symlink(h.other.join(".git"), &git).unwrap();
            }
            _ => std::fs::write(git.join("commondir"), "../../other/.git\n").unwrap(),
        }
        let repo = repo_for(&h.proj, Some(&h.base)).await.unwrap();
        assert_eq!(repo.root, h.proj, "{plant}");
        assert!(!borrows(&repo), "{plant}: borrowed an object store");
        let e = sub_repo(&h.proj, Some(&h.base), &Bounds::default())
            .await
            .unwrap_err();
        assert!(!e.is_empty(), "{plant}");
    }
}

/// A session inside a repository whose top folder is above the workspace
/// root it is in: the walk stops at that root, and only the session
/// folder is checkpointed, as a plain folder.
#[tokio::test]
async fn a_repository_above_the_workspace_root_is_not_used() {
    let h = home();
    let sub = h.proj.join("sub");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("s.txt"), "s\n").unwrap();
    let bounds = Bounds {
        ceiling: Some(sub.clone()),
        protected: vec![],
    };
    let repo = checkpoint_repo(&sub, Some(&h.base), &bounds)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(repo.root, sub);
    assert!(
        sub_repo(&sub, Some(&h.base), &bounds)
            .await
            .unwrap()
            .is_none()
    );
    // Within the bounds, the repository is found by its `.git`.
    let bounds = Bounds {
        ceiling: Some(h.home.clone()),
        protected: vec![],
    };
    let repo = checkpoint_repo(&sub, Some(&h.base), &bounds)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(repo.root, h.proj);
}

/// A repository at `$HOME` holds the node's data directory: a session in a
/// plain folder below it checkpoints only that folder; a session whose own
/// folder holds the data directory gets none.
#[tokio::test]
async fn a_repository_that_holds_the_data_directory_is_not_the_root() {
    let h = home();
    sh(&h.home, &["init", "-q", "-b", "main"]);
    let data = h.home.join(".local/share/rizzy-harness");
    std::fs::create_dir_all(&data).unwrap();
    let notes = h.home.join("notes");
    std::fs::create_dir(&notes).unwrap();
    let bounds = Bounds {
        ceiling: Some(h.home.clone()),
        protected: vec![data.clone()],
    };
    let repo = checkpoint_repo(&notes, Some(&h.base), &bounds)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(repo.root, notes);
    assert!(
        checkpoint_repo(&h.home, Some(&h.base), &bounds)
            .await
            .is_err()
    );
}

/// A `checkpoint` event's file names are relative to the checkpoint's
/// folder and never name anything outside it: a link to another folder is
/// recorded as the link, never as the files it leads to.
#[tokio::test]
async fn checkpoint_names_stay_inside_the_folder() {
    let h = home();
    let repo = repo_for(&h.proj, Some(&h.base)).await.unwrap();
    assert_eq!(repo.root, h.proj);
    let before = snapshot(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    std::os::unix::fs::symlink(&h.other, h.proj.join("link")).unwrap();
    std::fs::write(h.proj.join("a.txt"), "two\n").unwrap();
    std::fs::write(h.other.join("notes.txt"), "v2\n").unwrap();
    let after = snapshot(&repo, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    let mut names: Vec<String> = changed(&repo, &before, &after)
        .await
        .unwrap()
        .into_iter()
        .map(|(_, p)| p)
        .collect();
    names.sort();
    assert_eq!(names, ["a.txt", "link"]);
    for n in &names {
        let p = Path::new(n);
        assert!(
            p.components()
                .all(|c| matches!(c, std::path::Component::Normal(_))),
            "{n}"
        );
    }
}

/// An undo never deletes or writes through a link the agent put in place
/// of a folder, forced or not, whatever the snapshot says.
#[tokio::test]
async fn a_restore_never_writes_or_deletes_outside_the_folder() {
    let h = home();
    let repo = repo_for(&h.proj, Some(&h.base)).await.unwrap();
    let before = snapshot(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    // The turn makes `d/notes.txt` (so an undo deletes it) and changes
    // `a.txt`.
    std::fs::create_dir(h.proj.join("d")).unwrap();
    std::fs::write(h.proj.join("d/notes.txt"), "made\n").unwrap();
    std::fs::write(h.proj.join("a.txt"), "two\n").unwrap();
    let after = snapshot(&repo, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    // Then `d` becomes a link to the other project, which has a file of
    // the same name.
    std::fs::remove_dir_all(h.proj.join("d")).unwrap();
    std::os::unix::fs::symlink(&h.other, h.proj.join("d")).unwrap();
    let e = restore(&repo, &before, &after, true).await.unwrap_err();
    assert!(e.contains("outside"), "{e}");
    assert_eq!(
        std::fs::read_to_string(h.other.join("notes.txt")).unwrap(),
        "v1\n"
    );
    assert_eq!(
        std::fs::read_to_string(h.proj.join("a.txt")).unwrap(),
        "two\n"
    );
}

/// While an undo writes and deletes in the folder, a process of the
/// session swaps a folder in it for a link to another folder, over and
/// over (R.1 follow-up). The writes and deletes are relative to
/// directories opened without following links (`git::at`), so nothing is
/// ever written or deleted in the other folder, however the race falls.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_folder_swapped_for_a_link_during_an_undo_is_never_written_through() {
    const TRIES: usize = 120;
    const FILES: usize = 20;
    let h = home();
    let victim = h.home.join("victim");
    std::fs::create_dir(&victim).unwrap();
    for k in 0..FILES {
        std::fs::write(victim.join(format!("f{k}")), "victim\n").unwrap();
        std::fs::write(victim.join(format!("n{k}")), "victim\n").unwrap();
    }
    let victim_state = || {
        let mut v: Vec<(String, String)> = std::fs::read_dir(&victim)
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (
                    e.file_name().to_string_lossy().into_owned(),
                    std::fs::read_to_string(e.path()).unwrap_or_default(),
                )
            })
            .collect();
        v.sort();
        v
    };
    let clean = victim_state();
    let d = h.proj.join("d");
    let away = h.proj.join("d.r");
    let repo = repo_for(&h.proj, Some(&h.base)).await.unwrap();
    for i in 0..TRIES {
        let _ = std::fs::remove_file(&d);
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&away);
        std::fs::create_dir(&d).unwrap();
        for k in 0..FILES {
            std::fs::write(d.join(format!("f{k}")), "old\n").unwrap();
        }
        let before = snapshot(&repo, &format!("refs/harness/s/{i}/before"), "b")
            .await
            .unwrap();
        for k in 0..FILES {
            std::fs::write(d.join(format!("f{k}")), "new\n").unwrap();
            std::fs::write(d.join(format!("n{k}")), "new\n").unwrap();
        }
        let after = snapshot(&repo, &format!("refs/harness/s/{i}/after"), "a")
            .await
            .unwrap();
        let (dd, aw, v) = (d.clone(), away.clone(), victim.clone());
        let delay = std::time::Duration::from_micros((i as u64 % 20) * 150);
        let swapper = std::thread::spawn(move || {
            std::thread::sleep(delay);
            let until = std::time::Instant::now() + std::time::Duration::from_millis(40);
            while std::time::Instant::now() < until {
                if std::fs::rename(&dd, &aw).is_ok() {
                    let _ = std::os::unix::fs::symlink(&v, &dd);
                    std::thread::yield_now();
                    let _ = std::fs::remove_file(&dd);
                    let _ = std::fs::rename(&aw, &dd);
                }
                std::thread::yield_now();
            }
        });
        let _ = restore(&repo, &before, &after, true).await;
        swapper.join().unwrap();
        assert_eq!(
            victim_state(),
            clean,
            "try {i} wrote or deleted through the link"
        );
    }
}
