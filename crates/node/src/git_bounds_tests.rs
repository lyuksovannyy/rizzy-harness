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
    // The next turn's checkpoint: refused, never the user's home.
    let repo = repo_for(&h.proj, Some(&h.base)).await;
    assert!(
        repo.is_none(),
        "the checkpoint root followed core.worktree to {:?}",
        repo.map(|r| r.root)
    );
    // Nothing outside the session folder was touched.
    assert_eq!(
        std::fs::read_to_string(h.other.join("notes.txt")).unwrap(),
        "v1\n"
    );
    assert_eq!(
        std::fs::read_to_string(h.home.join(".profile")).unwrap(),
        "export A=1\n"
    );
}

/// The refusal says why, for the chat's "no checkpoint" note.
#[tokio::test]
async fn core_worktree_refusal_names_the_reason() {
    let h = home();
    sh(
        &h.proj,
        &[
            "config",
            "--local",
            "core.worktree",
            h.home.to_str().unwrap(),
        ],
    );
    let e = checkpoint_repo(&h.proj, Some(&h.base), &Bounds::default())
        .await
        .unwrap_err();
    assert!(e.contains("core.worktree"), "{e}");
    assert!(
        sub_repo(&h.proj, Some(&h.base), &Bounds::default())
            .await
            .is_err()
    );
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
