//! A repository inside the folder (a nested repository, or a submodule the
//! user's index records as a gitlink) is the agent's to write: its
//! `.git/config` and `.git/info/attributes` can define a `filter.<x>.clean`
//! program. `git diff-index` and `git status` against a work tree spawn
//! `git status` inside such a gitlink, which reads that config and runs the
//! filter on a file whose stat changed. No node git call may do that (T19).
//! Every test here gives the nested repository a clean filter that makes a
//! marker file, and asserts the marker never appears.

use super::*;

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

struct Rig {
    _d: tempfile::TempDir,
    dir: PathBuf,
    base: PathBuf,
    marker: PathBuf,
}

/// A repository on `main` with `a.txt`, and `sm/`: a nested repository whose
/// own config and attributes define a clean filter that makes the marker,
/// holding a committed file that was then rewritten with the same size (so
/// its stat differs and its content has to be filtered to be compared).
/// With `gitlink` the user's index and HEAD record `sm` as a submodule, with
/// a `.gitmodules`; without it `sm` is only a folder the agent made.
fn rig(gitlink: bool, gitmodules: bool) -> Rig {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    let dir = root.join("repo");
    let base = root.join("checkpoints");
    let marker = root.join("marker");
    let sm = dir.join("sm");
    std::fs::create_dir_all(&sm).unwrap();
    sh(&dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    sh(&dir, &["add", "a.txt"]);
    sh(&dir, &["commit", "-qm", "init"]);
    sh(&sm, &["init", "-q", "-b", "main"]);
    std::fs::write(sm.join("f.txt"), "aaaa\n").unwrap();
    sh(&sm, &["add", "f.txt"]);
    sh(&sm, &["commit", "-qm", "sm"]);
    let head = sh(&sm, &["rev-parse", "HEAD"]).trim().to_string();
    if gitlink {
        sh(
            &dir,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{head},sm"),
            ],
        );
        if gitmodules {
            std::fs::write(
                dir.join(".gitmodules"),
                "[submodule \"sm\"]\n\tpath = sm\n\turl = ./sm\n",
            )
            .unwrap();
            sh(&dir, &["add", ".gitmodules"]);
        }
        sh(&dir, &["commit", "-qm", "submodule"]);
    }
    let script = root.join("filter.sh");
    std::fs::write(
        &script,
        format!("#!/bin/sh\necho ran >> {}\ncat\n", marker.display()),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut config = std::fs::read_to_string(sm.join(".git/config")).unwrap();
    config.push_str(&format!("[filter \"x\"]\n\tclean = {}\n", script.display()));
    std::fs::write(sm.join(".git/config"), config).unwrap();
    std::fs::create_dir_all(sm.join(".git/info")).unwrap();
    std::fs::write(sm.join(".git/info/attributes"), "* filter=x\n").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(sm.join("f.txt"), "bbbb\n").unwrap();
    Rig {
        _d: d,
        dir,
        base,
        marker,
    }
}

impl Rig {
    fn clean(&self, what: &str) {
        assert!(
            !self.marker.exists(),
            "{what}: the nested repository's clean filter ran on the node"
        );
    }
}

/// The published file's dirty flag (`changed_since`, `diff-index` against the
/// work tree with a copy of the user's index that holds the gitlink).
#[tokio::test]
async fn describe_does_not_run_a_submodules_filter() {
    for gitmodules in [false, true] {
        let r = rig(true, gitmodules);
        let bounds = Bounds::default();
        let got = describe(&r.dir, Some(&r.base), &bounds).await;
        // What changed inside the nested repository is not the folder's
        // change (and is never looked at).
        assert!(got.as_ref().is_some_and(|g| !g.2), "{got:?}");
        r.clean(&format!("describe (.gitmodules: {gitmodules})"));
        // A change to a file of the repository itself is still seen.
        std::fs::write(r.dir.join("a.txt"), "changed\n").unwrap();
        let got = describe(&r.dir, Some(&r.base), &bounds).await;
        assert!(got.as_ref().is_some_and(|g| g.2), "{got:?}");
    }
}

/// Turn checkpoints over a folder whose user index holds the gitlink and
/// over one with only a nested folder. The shadow starts from its own index,
/// never the user's, so this pins that a checkpoint runs nothing; the removal
/// of a gitlink from the shadow's index is pinned by the test further down.
#[tokio::test]
async fn a_checkpoint_does_not_run_a_nested_repositorys_filter() {
    for (gitlink, gitmodules) in [(true, false), (true, true), (false, false)] {
        let r = rig(gitlink, gitmodules);
        let repo = repo_for(&r.dir, Some(&r.base)).await.unwrap();
        let a = snapshot(&repo, "refs/harness/s/t1/before", "s")
            .await
            .unwrap();
        std::fs::write(r.dir.join("sm/f.txt"), "cccc\n").unwrap();
        std::fs::write(r.dir.join("new.txt"), "n\n").unwrap();
        std::fs::write(r.dir.join("a.txt"), "changed\n").unwrap();
        let b = snapshot_full(&repo, "refs/harness/s/t1/after", "s")
            .await
            .unwrap()
            .sha;
        let c = snapshot(&repo, "refs/harness/s/t2/before", "s")
            .await
            .unwrap();
        r.clean(&format!(
            "snapshot (gitlink {gitlink}, .gitmodules {gitmodules})"
        ));
        // The diff, and an undo of the turn, over the same folder.
        let _ = diff::turn_diff(&repo, &a, &b, &|s: &str| s.to_string()).await;
        let _ = restore(&repo, &a, &b, true).await;
        let _ = restore(&repo, &c, &b, true).await;
        let _ = changed(&repo, &a, &b).await;
        r.clean(&format!(
            "diff and restore (gitlink {gitlink}, .gitmodules {gitmodules})"
        ));
    }
}

/// A nested repository in the folder never reaches the shadow's index as a
/// gitlink through the folder alone (it is left out of every snapshot), so
/// the `add -u` of the next snapshot has no gitlink to compare. This is the
/// invariant `add -u` relies on: it has no flag that skips submodules, and
/// `git add -u` with a gitlink in its index runs the nested repository's
/// filter.
#[tokio::test]
async fn the_shadows_index_never_holds_a_gitlink_the_folder_made() {
    for gitlink in [true, false] {
        let r = rig(gitlink, false);
        let repo = repo_for(&r.dir, Some(&r.base)).await.unwrap();
        let snap = snapshot_full(&repo, "refs/harness/s/t1/before", "s")
            .await
            .unwrap();
        assert!(!snap.nested.is_empty(), "the nested repository is left out");
        let index = repo.shadow.clone().unwrap().join("index");
        let out = std::process::Command::new("git")
            .arg("--git-dir")
            .arg(repo.shadow.as_ref().unwrap())
            .args(["ls-files", "-s"])
            .env("GIT_INDEX_FILE", &index)
            .output()
            .unwrap();
        let listed = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(!listed.contains("160000"), "gitlink {gitlink}: {listed}");
        assert!(listed.contains("a.txt"), "{listed}");
    }
}

/// Even a gitlink an older snapshot left in the shadow's own index (an old
/// node, a snapshot racing the agent's `mv`) is taken out before anything
/// compares the folder with the index.
#[tokio::test]
async fn a_gitlink_in_the_shadows_own_index_is_removed_before_the_next_snapshot_compares() {
    let r = rig(false, false);
    let repo = repo_for(&r.dir, Some(&r.base)).await.unwrap();
    snapshot(&repo, "refs/harness/s/t1/before", "s")
        .await
        .unwrap();
    let shadow = repo.shadow.clone().unwrap();
    let sm_head = sh(&r.dir.join("sm"), &["rev-parse", "HEAD"]);
    let o = std::process::Command::new("git")
        .arg("--git-dir")
        .arg(&shadow)
        .args(["update-index", "--add", "--cacheinfo"])
        .arg(format!("160000,{},sm", sm_head.trim()))
        .env("GIT_INDEX_FILE", shadow.join("index"))
        .output()
        .unwrap();
    assert!(o.status.success(), "{o:?}");
    std::fs::write(r.dir.join("sm/f.txt"), "cccc\n").unwrap();
    std::fs::write(r.dir.join("a.txt"), "changed\n").unwrap();
    let again = snapshot(&repo, "refs/harness/s/t2/before", "s").await;
    assert!(again.is_ok(), "{again:?}");
    r.clean("a snapshot with a gitlink in the shadow's own index");
}
