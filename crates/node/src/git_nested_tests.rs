//! Checkpoints and undo with a git repository nested in the folder
//! (experiment B.5, fixed by task A.15 in ADOPTION_PLAN.md).
//!
//! The rule (PLAN.md §checkpoints, T33): a nested repository (what git
//! itself treats as one: a directory its walk will not enter because it
//! holds a valid `.git` directory, gitfile or link to one) is left out of
//! every snapshot,
//! never recorded as a gitlink and never as files, and its presence never
//! stops the snapshot of the rest of the folder. An undo never touches one:
//! it does not delete, recreate, empty it or move its HEAD, never lists it
//! as restored, and names it as left alone. Snapshots and undo never run
//! git inside it and never touch its `.git`.
//!
//! The first twelve tests are B.5's, turned from "what happens today" into
//! the fixed behaviour. Each scenario runs with the outer folder both as a
//! git repository and as a plain folder, and always with a CONTROL: an
//! ordinary sub-folder `ctl/` (no `.git` inside) that the turn also changes.
//! The control must be captured as files and restored exactly;
//! `the_control_fails_when_a_folder_is_not_captured` shows the control check
//! really fails when capture is missing. These tests live inside the crate
//! because `restore` is test-only and the `git` module is private.

use super::*;
use std::collections::BTreeMap;

fn sh(dir: &Path, args: &[&str]) -> String {
    let o = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim_end().to_string()
}

fn write(p: &Path, text: &str) {
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(p, text).unwrap();
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| format!("<{e}>"))
}

/// Every entry under `dir` (relative path, bytes; a symlink as its
/// target). Symlinks are not followed. Used to prove a directory (a nested
/// repository, `.git` included) was not touched.
fn digest(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
            let m = std::fs::symlink_metadata(&p).unwrap();
            if m.is_dir() {
                out.insert(format!("{rel}/"), vec![]);
                walk(base, &p, out);
            } else if m.is_file() {
                out.insert(rel, std::fs::read(&p).unwrap());
            } else if m.file_type().is_symlink() {
                let t = std::fs::read_link(&p).unwrap();
                out.insert(format!("{rel}@"), t.to_string_lossy().as_bytes().to_vec());
            }
        }
    }
    let mut out = BTreeMap::new();
    // A submodule's `.git` is a one-line file, not a directory.
    if std::fs::symlink_metadata(dir).unwrap().is_file() {
        out.insert(".git".to_string(), std::fs::read(dir).unwrap());
        return out;
    }
    walk(dir, dir, &mut out);
    out
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Nested {
    /// Nothing nested.
    None,
    /// `dep/` is a repository with one commit (x.txt, y.txt).
    Committed,
    /// `dep/` was `git init`ed and holds files but has no commit.
    Unborn,
    /// `dep/` is an ordinary folder with the same files (control for case 1/2).
    PlainFolder,
}

struct Fx {
    _d: tempfile::TempDir,
    ws: PathBuf,
    base: PathBuf,
    outer_git: bool,
}

impl Fx {
    /// A folder with the control sub-folder `ctl/`, and `dep/` as asked.
    /// The outer repository (when asked) is committed before `dep/` exists,
    /// so `dep/` is an untracked folder in it.
    fn new(outer_git: bool, nested: Nested) -> Self {
        let d = tempfile::tempdir().unwrap();
        let ws = d.path().join("ws");
        write(&ws.join("ctl/c.txt"), "ctl-1\n");
        write(&ws.join("top.txt"), "top-1\n");
        if outer_git {
            sh(&ws, &["init", "-q", "-b", "main"]);
            sh(&ws, &["add", "."]);
            sh(&ws, &["commit", "-qm", "outer"]);
        }
        let fx = Self {
            base: d.path().join("cp"),
            _d: d,
            ws,
            outer_git,
        };
        fx.make_dep(nested);
        fx
    }

    fn dep(&self) -> PathBuf {
        self.ws.join("dep")
    }

    fn make_dep(&self, nested: Nested) {
        make_repo(&self.dep(), nested);
    }

    async fn repo(&self) -> Repo {
        let r = repo_for(&self.ws, Some(&self.base)).await.unwrap();
        assert!(r.is_shadow());
        r
    }

    /// `git add -A` into a throw-away index, exactly as a snapshot did it
    /// before A.15: the exit status and what git printed on stderr. Used
    /// only to show the hazard is present in a fixture.
    async fn add_output(&self, repo: &Repo) -> (bool, String) {
        let idx = self
            .base
            .join(format!("probe-index-{}", harness_core::new_id()));
        let mut c = command(repo, Some(&idx));
        c.args(["add", "-A", "--", "."]);
        let o = exec(c, None, GIT_TIMEOUT).await.unwrap();
        let _ = std::fs::remove_file(&idx);
        (
            o.status.success(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        )
    }
}

/// `dep` as `nested` says (x.txt and y.txt in it).
fn make_repo(dep: &Path, nested: Nested) {
    match nested {
        Nested::None => {}
        Nested::PlainFolder => {
            write(&dep.join("x.txt"), "x-1\n");
            write(&dep.join("y.txt"), "y-1\n");
        }
        Nested::Committed | Nested::Unborn => {
            std::fs::create_dir_all(dep).unwrap();
            sh(dep, &["init", "-q", "-b", "main"]);
            write(&dep.join("x.txt"), "x-1\n");
            write(&dep.join("y.txt"), "y-1\n");
            if nested == Nested::Committed {
                sh(dep, &["add", "."]);
                sh(dep, &["commit", "-qm", "dep one"]);
            }
        }
    }
}

/// `ls-tree -r` of a snapshot: `(mode, path)` -> object id.
async fn entries(repo: &Repo, sha: &str) -> BTreeMap<(String, String), String> {
    run(repo, &["ls-tree", "-r", "-z", sha], None)
        .await
        .unwrap()
        .split('\0')
        .filter(|l| !l.is_empty())
        .map(|l| {
            let (meta, path) = l.split_once('\t').unwrap();
            let f: Vec<&str> = meta.split(' ').collect();
            ((f[0].to_string(), path.to_string()), f[2].to_string())
        })
        .collect()
}

fn paths(e: &BTreeMap<(String, String), String>) -> Vec<&str> {
    e.keys().map(|(_, p)| p.as_str()).collect()
}

/// Nothing at or under `dir` in the snapshot, and no gitlink anywhere.
fn assert_absent(e: &BTreeMap<(String, String), String>, dir: &str, ctx: &str) {
    let under = format!("{dir}/");
    assert!(
        !paths(e).iter().any(|p| *p == dir || p.starts_with(&under)),
        "{ctx}: nothing of {dir} may be in the snapshot: {:?}",
        paths(e)
    );
    assert!(
        !e.keys().any(|(m, _)| m == "160000"),
        "{ctx}: no gitlink: {e:?}"
    );
}

/// A before/after pair around one simulated turn.
struct Turn {
    before: String,
    after: String,
    before_nested: Vec<String>,
    after_nested: Vec<String>,
}

/// The control's change, applied by every simulated turn.
fn change_control(ws: &Path) {
    write(&ws.join("ctl/c.txt"), "ctl-2\n");
    write(&ws.join("ctl/new.txt"), "agent made this\n");
}

/// Snapshot, run `mutate` (the agent's turn, which also changes the control),
/// snapshot again.
async fn take_turn(fx: &Fx, repo: &Repo, mutate: impl FnOnce(&Path)) -> Result<Turn, String> {
    let before = snapshot_full(repo, "refs/harness/s/t/before", "b").await?;
    change_control(&fx.ws);
    mutate(&fx.ws);
    let after = snapshot_full(repo, "refs/harness/s/t/after", "a").await?;
    Ok(Turn {
        before: before.sha,
        after: after.sha,
        before_nested: before.nested.shown,
        after_nested: after.nested.shown,
    })
}

/// The control must be captured as files by both snapshots and be restored
/// exactly by an undo. Fails if `ctl/` is missing from a snapshot (the
/// restore assertions below then fail as well).
async fn assert_control_captured(repo: &Repo, t: &Turn) {
    let b = entries(repo, &t.before).await;
    let a = entries(repo, &t.after).await;
    assert!(
        b.keys().any(|(m, p)| m == "100644" && p == "ctl/c.txt"),
        "control: ctl/c.txt must be in the before snapshot as a file: {:?}",
        paths(&b)
    );
    assert!(
        a.keys().any(|(m, p)| m == "100644" && p == "ctl/new.txt"),
        "control: ctl/new.txt must be in the after snapshot: {:?}",
        paths(&a)
    );
    let ch = changed(repo, &t.before, &t.after).await.unwrap();
    assert!(
        ch.contains(&('M', "ctl/c.txt".to_string())),
        "control: {ch:?}"
    );
    assert!(
        ch.contains(&('A', "ctl/new.txt".to_string())),
        "control: {ch:?}"
    );
}

fn assert_control_restored(ws: &Path) {
    assert_eq!(
        read(&ws.join("ctl/c.txt")),
        "ctl-1\n",
        "control not restored"
    );
    assert!(
        !ws.join("ctl/new.txt").exists(),
        "control: file the turn created was not removed"
    );
    assert_eq!(read(&ws.join("top.txt")), "top-1\n");
}

/// Nothing of `dep` among the files a restore wrote, and `dep` named as
/// left alone.
fn assert_left_alone(r: &Restored, ctx: &str) {
    assert!(
        r.files.iter().all(|p| p != "dep" && !p.starts_with("dep/")),
        "{ctx}: a nested repository is never listed as restored: {:?}",
        r.files
    );
    assert_eq!(
        r.nested,
        ["dep"],
        "{ctx}: the undo names what it left alone"
    );
}

/// The nested repository's whole `.git` and, for an outer repository, the
/// user's own `.git` (minus nothing).
type Files = BTreeMap<String, Vec<u8>>;

fn guarded_state(fx: &Fx) -> (Files, Option<Files>) {
    (
        digest(&fx.dep().join(".git")),
        fx.outer_git.then(|| digest(&fx.ws.join(".git"))),
    )
}

const BOTH: [bool; 2] = [true, false];

// ---------------------------------------------------------------- case 1

/// CASE 1 (committed nested repository): plain `git add -A` would record a
/// gitlink at `dep` (the precondition below shows the hazard is present);
/// a snapshot records nothing at all for `dep`, no gitlink and no files,
/// names it as left out, and does not touch it.
///
/// Control: the same folder with `dep/` as an ordinary folder captures
/// `dep/x.txt` and `dep/y.txt` as files and leaves nothing out.
#[tokio::test]
async fn a_nested_repository_with_a_commit_is_left_out_of_snapshots() {
    for outer in BOTH {
        let fx = Fx::new(outer, Nested::Committed);
        let repo = fx.repo().await;
        let (ok, stderr) = fx.add_output(&repo).await;
        assert!(ok, "outer_git={outer}: precondition: {stderr}");
        assert!(
            stderr.contains("warning: adding embedded git repository: dep"),
            "outer_git={outer}: precondition: {stderr}"
        );
        let untouched = digest(&fx.dep());
        let s = snapshot_full(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        assert_eq!(s.nested.shown, ["dep"], "outer_git={outer}");
        let e = entries(&repo, &s.sha).await;
        assert_absent(&e, "dep", &format!("outer_git={outer}"));
        assert!(
            e.keys().any(|(m, p)| m == "100644" && p == "ctl/c.txt"),
            "control: ctl is captured"
        );
        assert!(e.keys().any(|(_, p)| p == "top.txt"), "control");
        assert_eq!(digest(&fx.dep()), untouched, "outer_git={outer}");

        // Control: an ordinary folder is captured as files.
        let ctl = Fx::new(outer, Nested::PlainFolder);
        let r = ctl.repo().await;
        let s = snapshot_full(&r, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        assert!(s.nested.shown.is_empty(), "control: {:?}", s.nested);
        let e = entries(&r, &s.sha).await;
        assert!(
            e.keys().any(|(m, p)| m == "100644" && p == "dep/x.txt"),
            "control: {:?}",
            paths(&e)
        );
        assert!(e.keys().any(|(m, p)| m == "100644" && p == "dep/y.txt"));
        assert!(!e.keys().any(|(m, _)| m == "160000"));
    }
}

// ---------------------------------------------------------------- case 2

/// CASE 2 (nested repository with no commit yet): plain `git add -A` fails
/// here (the precondition), which used to leave the whole folder without a
/// snapshot. Now the snapshot succeeds, holds `ctl/c.txt` and `top.txt`,
/// nothing under `dep`, and names `dep` as left out; the same once `dep`
/// gets a commit.
///
/// Control: with `dep/` an ordinary folder holding the same files the
/// snapshot captures them.
#[tokio::test]
async fn a_nested_repository_without_a_commit_does_not_stop_the_snapshot() {
    for outer in BOTH {
        let fx = Fx::new(outer, Nested::Unborn);
        let repo = fx.repo().await;
        let (ok, stderr) = fx.add_output(&repo).await;
        assert!(!ok, "outer_git={outer}: precondition: {stderr}");
        let s = snapshot_full(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        assert_eq!(s.nested.shown, ["dep"], "outer_git={outer}");
        let e = entries(&repo, &s.sha).await;
        assert_absent(&e, "dep", &format!("outer_git={outer}"));
        assert!(e.keys().any(|(_, p)| p == "ctl/c.txt"), "{:?}", paths(&e));
        assert!(e.keys().any(|(_, p)| p == "top.txt"), "{:?}", paths(&e));
        assert_eq!(
            run(
                &repo,
                &["for-each-ref", "--format=%(refname)", "refs/harness/"],
                None
            )
            .await
            .unwrap(),
            "refs/harness/s/t/before"
        );

        // With a commit in the nested repository: still left out.
        sh(&fx.dep(), &["add", "."]);
        sh(&fx.dep(), &["commit", "-qm", "first"]);
        let s = snapshot_full(&repo, "refs/harness/s/t2/before", "b")
            .await
            .unwrap();
        assert_eq!(s.nested.shown, ["dep"]);
        assert_absent(&entries(&repo, &s.sha).await, "dep", "committed later");

        // Control: the same files in an ordinary folder snapshot fine.
        let ctl = Fx::new(outer, Nested::PlainFolder);
        let r = ctl.repo().await;
        let s = snapshot(&r, "refs/harness/s/t/before", "b").await.unwrap();
        let e = entries(&r, &s).await;
        assert!(e.keys().any(|(_, p)| p == "dep/x.txt"), "control");
    }
}

/// CASE 2b: the realistic path to case 2. The "before" snapshot is fine;
/// the agent then runs `git init` in a new folder (files written, no commit
/// yet). The "after" snapshot of the same turn succeeds, without `dep`, and
/// names it; the turn can be undone from the pair.
#[tokio::test]
async fn a_repository_initialised_during_a_turn_is_left_out_of_the_after_snapshot() {
    for outer in BOTH {
        let fx = Fx::new(outer, Nested::None);
        let repo = fx.repo().await;
        let t = take_turn(&fx, &repo, |_| {
            let dep = fx.dep();
            std::fs::create_dir_all(&dep).unwrap();
            sh(&dep, &["init", "-q", "-b", "main"]);
            write(&dep.join("x.txt"), "x-agent\n");
        })
        .await
        .expect("the after snapshot succeeds");
        assert_control_captured(&repo, &t).await;
        assert!(t.before_nested.is_empty());
        assert_eq!(t.after_nested, ["dep"], "outer_git={outer}");
        assert_absent(&entries(&repo, &t.after).await, "dep", "after");
        let refs = run(
            &repo,
            &["for-each-ref", "--format=%(refname)", "refs/harness/"],
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            refs, "refs/harness/s/t/after\nrefs/harness/s/t/before",
            "outer_git={outer}"
        );
    }
}

// ---------------------------------------------------------------- case 3

/// CASE 3 (uncommitted work inside a committed nested repository): the
/// undo restores the control and leaves the nested repository exactly as
/// the turn left it (its files, `.git`, index and HEAD), lists nothing of
/// it as restored and names it as left alone. The user's own uncommitted
/// work that the agent overwrote is not brought back: no snapshot holds
/// it, by design (capturing another repository's work tree as plain files
/// would restore over the user's work without its history). The nested
/// `.git` and, with an outer repository, the user's own `.git` are byte for
/// byte untouched by both snapshots and the undo.
#[tokio::test]
async fn undo_leaves_uncommitted_work_in_a_nested_repository_alone_and_names_it() {
    for outer in BOTH {
        let fx = Fx::new(outer, Nested::Committed);
        // The user's uncommitted work: a modified tracked file, a staged
        // change, and a new untracked file.
        write(&fx.dep().join("x.txt"), "x-user-dirty\n");
        write(&fx.dep().join("y.txt"), "y-user-staged\n");
        sh(&fx.dep(), &["add", "y.txt"]);
        write(&fx.dep().join("new.txt"), "user-untracked\n");
        let repo = fx.repo().await;
        let guard = guarded_state(&fx);

        let t = take_turn(&fx, &repo, |ws| {
            write(&ws.join("dep/x.txt"), "x-agent\n");
            write(&ws.join("dep/new.txt"), "agent-overwrote\n");
            std::fs::remove_file(ws.join("dep/y.txt")).unwrap();
        })
        .await
        .unwrap();
        assert_control_captured(&repo, &t).await;
        assert_eq!(t.before_nested, ["dep"]);
        assert_eq!(t.after_nested, ["dep"]);
        let ch = changed(&repo, &t.before, &t.after).await.unwrap();
        assert!(
            !ch.iter().any(|(_, p)| p.starts_with("dep")),
            "outer_git={outer}: {ch:?}"
        );
        assert_eq!(
            guarded_state(&fx).0,
            guard.0,
            "snapshots must not touch dep/.git"
        );
        let as_left = digest(&fx.dep());

        let r = restore_report(&repo, &t.before, &t.after, false)
            .await
            .unwrap();
        assert_left_alone(&r, &format!("outer_git={outer}"));
        assert_control_restored(&fx.ws);
        // The nested repository is exactly as the agent left it.
        assert_eq!(digest(&fx.dep()), as_left, "outer_git={outer}");
        assert_eq!(read(&fx.dep().join("x.txt")), "x-agent\n");
        assert_eq!(read(&fx.dep().join("new.txt")), "agent-overwrote\n");
        assert!(!fx.dep().join("y.txt").exists());
        // dep/.git and the outer .git are untouched by snapshot and restore.
        assert_eq!(guarded_state(&fx), guard, "outer_git={outer}");
    }
}

/// CASE 3b: the agent COMMITS inside the nested repository. Nothing of
/// `dep` is in either snapshot, so the diff shows nothing for it, the undo
/// does not list it as restored (it used to, while its HEAD stayed put),
/// names it as left alone, and its HEAD and files stay where the agent left
/// them. The nested `.git` is untouched by the restore.
#[tokio::test]
async fn undo_does_not_list_a_nested_repository_the_agent_committed_in() {
    for outer in BOTH {
        let fx = Fx::new(outer, Nested::Committed);
        let repo = fx.repo().await;
        let head_before = sh(&fx.dep(), &["rev-parse", "HEAD"]);
        let t = take_turn(&fx, &repo, |ws| {
            write(&ws.join("dep/x.txt"), "x-agent\n");
            sh(&ws.join("dep"), &["commit", "-qam", "agent commit"]);
        })
        .await
        .unwrap();
        assert_control_captured(&repo, &t).await;
        let head_after = sh(&fx.dep(), &["rev-parse", "HEAD"]);
        assert_ne!(head_before, head_after);
        let ch = changed(&repo, &t.before, &t.after).await.unwrap();
        assert!(!ch.iter().any(|(_, p)| p.starts_with("dep")), "{ch:?}");
        let guard = guarded_state(&fx);

        let r = restore_report(&repo, &t.before, &t.after, false)
            .await
            .unwrap();
        assert_left_alone(&r, &format!("outer_git={outer}"));
        assert_control_restored(&fx.ws);
        assert_eq!(
            sh(&fx.dep(), &["rev-parse", "HEAD"]),
            head_after,
            "outer_git={outer}: undo never moves a nested HEAD"
        );
        assert_eq!(read(&fx.dep().join("x.txt")), "x-agent\n");
        assert_eq!(guarded_state(&fx), guard, "restore must not touch any .git");
    }
}

// ---------------------------------------------------------------- case 4

/// CASE 4 (the turn deletes a committed nested repository): the undo does
/// not create an empty `dep` (it used to: no files, no `.git`), does not
/// list it as restored, and names it as left alone. What the repository
/// held is not in any snapshot; ordinary folders (the control) come back
/// whole. (Refusing the agent's deletion needs a prompt, not a snapshot.)
#[tokio::test]
async fn undo_of_a_deleted_nested_repository_creates_no_empty_folder() {
    for outer in BOTH {
        let fx = Fx::new(outer, Nested::Committed);
        let repo = fx.repo().await;
        let t = take_turn(&fx, &repo, |ws| {
            std::fs::remove_dir_all(ws.join("dep")).unwrap();
        })
        .await
        .unwrap();
        assert_control_captured(&repo, &t).await;
        let ch = changed(&repo, &t.before, &t.after).await.unwrap();
        assert!(!ch.iter().any(|(_, p)| p.starts_with("dep")), "{ch:?}");
        assert!(!fx.dep().exists());

        let r = restore_report(&repo, &t.before, &t.after, false)
            .await
            .unwrap();
        assert_left_alone(&r, &format!("outer_git={outer}"));
        assert_control_restored(&fx.ws);
        assert!(
            !fx.dep().exists(),
            "outer_git={outer}: no empty folder where the repository was"
        );
    }
}

/// CASE 4b: the agent CREATES a repository during the turn (a `git clone`
/// or `git init` + commit). Undo leaves it in place, with its `.git` and
/// files, does not list it as restored and names it as left alone.
/// Control: a new file in an ordinary folder IS removed by the same undo.
#[tokio::test]
async fn undo_of_a_turn_that_created_a_repository_leaves_it_in_place() {
    for outer in BOTH {
        let fx = Fx::new(outer, Nested::None);
        let repo = fx.repo().await;
        let t = take_turn(&fx, &repo, |_| {
            fx.make_dep(Nested::Committed);
        })
        .await
        .unwrap();
        assert_control_captured(&repo, &t).await;
        let ch = changed(&repo, &t.before, &t.after).await.unwrap();
        assert!(!ch.iter().any(|(_, p)| p.starts_with("dep")), "{ch:?}");
        let guard = guarded_state(&fx);
        let made = digest(&fx.dep());

        let r = restore_report(&repo, &t.before, &t.after, false)
            .await
            .unwrap();
        assert_left_alone(&r, &format!("outer_git={outer}"));
        assert_control_restored(&fx.ws);
        assert!(
            fx.dep().join(".git").is_dir(),
            "outer_git={outer}: the created repository is still there"
        );
        assert_eq!(digest(&fx.dep()), made);
        assert_eq!(guarded_state(&fx), guard);
    }
}

// ---------------------------------------------------------------- case 5

/// A registered submodule, offline: an upstream repository in the temp dir,
/// added with `submodule add` (local file transport allowed for this one
/// call) and committed in the outer repository. Needs an outer repository
/// (`.gitmodules` and the gitlink live in its index), so there is no
/// plain-folder variant: a plain folder with a nested repository is case 1.
fn submodule_fx() -> Fx {
    let fx = Fx::new(true, Nested::None);
    let up = fx.base.parent().unwrap().join("upstream");
    std::fs::create_dir_all(&up).unwrap();
    sh(&up, &["init", "-q", "-b", "main"]);
    write(&up.join("x.txt"), "x-1\n");
    write(&up.join("y.txt"), "y-1\n");
    sh(&up, &["add", "."]);
    sh(&up, &["commit", "-qm", "up one"]);
    sh(
        &fx.ws,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            up.to_str().unwrap(),
            "dep",
        ],
    );
    sh(&fx.ws, &["commit", "-qm", "add submodule"]);
    fx
}

/// CASE 5a: a registered submodule (its `.git` is a one-line file,
/// `gitdir: ../.git/modules/dep`) is left out exactly like an unregistered
/// nested repository: `.gitmodules` is a file in the snapshot, nothing of
/// `dep` is, and `dep` is named as left out.
#[tokio::test]
async fn a_registered_submodule_is_left_out_of_snapshots_like_a_nested_repository() {
    let fx = submodule_fx();
    let repo = fx.repo().await;
    assert!(
        fx.dep().join(".git").is_file(),
        "a submodule's .git is a gitfile"
    );
    let (ok, stderr) = fx.add_output(&repo).await;
    assert!(ok, "{stderr}");
    assert!(
        stderr.contains("adding embedded git repository"),
        "precondition: {stderr}"
    );
    let s = snapshot_full(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    assert_eq!(s.nested.shown, ["dep"]);
    let e = entries(&repo, &s.sha).await;
    assert_absent(&e, "dep", "submodule");
    assert!(e.keys().any(|(m, p)| m == "100644" && p == ".gitmodules"));
    assert!(
        e.keys().any(|(m, p)| m == "100644" && p == "ctl/c.txt"),
        "control"
    );
}

/// CASE 5b: undo leaves uncommitted work in a registered submodule as the
/// turn left it (as in case 3), names it, and does not touch the
/// submodule's git directory in `.git/modules/dep` or its gitfile.
#[tokio::test]
async fn undo_leaves_uncommitted_work_in_a_submodule_alone_and_names_it() {
    let fx = submodule_fx();
    write(&fx.dep().join("x.txt"), "x-user-dirty\n");
    write(&fx.dep().join("new.txt"), "user-untracked\n");
    let repo = fx.repo().await;
    let modules = digest(&fx.ws.join(".git/modules/dep"));
    let guard = guarded_state(&fx);
    let gitfile = read(&fx.dep().join(".git"));

    let t = take_turn(&fx, &repo, |ws| {
        write(&ws.join("dep/x.txt"), "x-agent\n");
        write(&ws.join("dep/new.txt"), "agent-overwrote\n");
    })
    .await
    .unwrap();
    assert_control_captured(&repo, &t).await;
    let r = restore_report(&repo, &t.before, &t.after, false)
        .await
        .unwrap();
    assert_left_alone(&r, "submodule");
    assert_control_restored(&fx.ws);
    assert_eq!(read(&fx.dep().join("x.txt")), "x-agent\n");
    assert_eq!(read(&fx.dep().join("new.txt")), "agent-overwrote\n");
    assert_eq!(guarded_state(&fx), guard);
    assert_eq!(digest(&fx.ws.join(".git/modules/dep")), modules);
    assert_eq!(read(&fx.dep().join(".git")), gitfile);
}

/// CASE 5c: the turn deletes a registered submodule's working folder. The
/// undo creates no empty `dep` and names it; the submodule's git directory
/// `.git/modules/dep` survives untouched in the outer repository, so its
/// COMMITTED content is recoverable with `git submodule update`
/// (uncommitted work is not).
#[tokio::test]
async fn undo_of_a_deleted_submodule_creates_no_empty_folder_and_its_git_dir_survives() {
    let fx = submodule_fx();
    let repo = fx.repo().await;
    let modules = digest(&fx.ws.join(".git/modules/dep"));
    assert!(!modules.is_empty());
    let t = take_turn(&fx, &repo, |ws| {
        std::fs::remove_dir_all(ws.join("dep")).unwrap();
    })
    .await
    .unwrap();
    assert_control_captured(&repo, &t).await;
    let r = restore_report(&repo, &t.before, &t.after, false)
        .await
        .unwrap();
    assert_left_alone(&r, "submodule");
    assert_control_restored(&fx.ws);
    assert!(!fx.dep().exists(), "no empty folder");
    assert_eq!(digest(&fx.ws.join(".git/modules/dep")), modules);
}

// ------------------------------------------------------- controls, proven

/// The control for every test above: an ordinary sub-folder with the same
/// kind of content, under the same turn, is fully captured and fully
/// restored, including edits, a deletion and a new file.
#[tokio::test]
async fn control_an_ordinary_sub_folder_is_fully_captured_and_restored() {
    for outer in BOTH {
        let fx = Fx::new(outer, Nested::PlainFolder);
        write(&fx.dep().join("x.txt"), "x-user-dirty\n");
        write(&fx.dep().join("new.txt"), "user-untracked\n");
        let repo = fx.repo().await;
        let t = take_turn(&fx, &repo, |ws| {
            write(&ws.join("dep/x.txt"), "x-agent\n");
            write(&ws.join("dep/new.txt"), "agent-overwrote\n");
            std::fs::remove_file(ws.join("dep/y.txt")).unwrap();
        })
        .await
        .unwrap();
        assert_control_captured(&repo, &t).await;
        let ch = changed(&repo, &t.before, &t.after).await.unwrap();
        assert!(ch.contains(&('M', "dep/x.txt".to_string())), "{ch:?}");
        assert!(ch.contains(&('M', "dep/new.txt".to_string())), "{ch:?}");
        assert!(ch.contains(&('D', "dep/y.txt".to_string())), "{ch:?}");
        restore(&repo, &t.before, &t.after, false).await.unwrap();
        assert_control_restored(&fx.ws);
        assert_eq!(read(&fx.dep().join("x.txt")), "x-user-dirty\n");
        assert_eq!(read(&fx.dep().join("new.txt")), "user-untracked\n");
        assert_eq!(read(&fx.dep().join("y.txt")), "y-1\n");
    }
}

/// Proof that the control check can fail: when the control folder is not
/// captured (here: excluded in the shadow), the control assertion panics.
#[tokio::test]
#[should_panic(expected = "control")]
async fn the_control_fails_when_a_folder_is_not_captured() {
    let fx = Fx::new(false, Nested::None);
    let repo = fx.repo().await;
    let shadow = repo.shadow.clone().unwrap();
    let mut ex = read(&shadow.join("info/exclude"));
    ex.push_str("\nctl/\n");
    std::fs::write(shadow.join("info/exclude"), ex).unwrap();
    let t = take_turn(&fx, &repo, |_| {}).await.unwrap();
    assert_control_captured(&repo, &t).await;
}

// ------------------------------------------------------- A.15, new cases

/// A nested repository with no commit, there before the turn or made by
/// the agent during it: the rest of the folder is checkpointed and the
/// undo restores it; the nested directory, `.git` included, is byte for
/// byte as the turn left it.
#[tokio::test]
async fn an_unborn_nested_repository_does_not_stop_undo_and_is_left_untouched() {
    for outer in BOTH {
        for mid_turn in [false, true] {
            let ctx = format!("outer_git={outer} mid_turn={mid_turn}");
            let fx = Fx::new(
                outer,
                if mid_turn {
                    Nested::None
                } else {
                    Nested::Unborn
                },
            );
            let repo = fx.repo().await;
            let t = take_turn(&fx, &repo, |ws| {
                if mid_turn {
                    make_repo(&ws.join("dep"), Nested::Unborn);
                }
                write(&ws.join("dep/x.txt"), "x-agent\n");
            })
            .await
            .unwrap_or_else(|e| panic!("{ctx}: {e}"));
            assert_control_captured(&repo, &t).await;
            let as_left = digest(&fx.dep());
            assert!(as_left.contains_key(".git/HEAD"), "{ctx}");
            let r = restore_report(&repo, &t.before, &t.after, false)
                .await
                .unwrap_or_else(|e| panic!("{ctx}: {e}"));
            assert_left_alone(&r, &ctx);
            assert_control_restored(&fx.ws);
            assert_eq!(digest(&fx.dep()), as_left, "{ctx}");
        }
    }
}

/// Detection is git's: nested at depth three, a name with a space, a
/// non-UTF-8 name, a `.git` symlink to a real repository (nested; the link
/// is not followed by us, and nothing outside changes). A `.git` git does
/// not accept (a dangling symlink, an empty directory, a junk file) makes
/// nothing nested: the folder's files are captured, the `.git` entry is
/// not (git never stages a `.git`). A repository inside an ignored folder
/// is ignored anyway and not reported.
#[cfg(unix)]
#[tokio::test]
async fn nested_repositories_are_found_at_any_depth_and_odd_spelling_and_never_followed() {
    use std::os::unix::ffi::OsStrExt;
    for outer in BOTH {
        let fx = Fx::new(outer, Nested::None);
        let ws = &fx.ws;
        make_repo(&ws.join("a/b/deep"), Nested::Committed);
        make_repo(&ws.join("with space/repo"), Nested::Unborn);
        let odd = ws.join(std::ffi::OsStr::from_bytes(b"odd\xffname"));
        make_repo(&odd, Nested::Committed);
        // A `.git` symlink to a repository outside the folder.
        let elsewhere = fx.base.parent().unwrap().join("elsewhere");
        make_repo(&elsewhere, Nested::Committed);
        write(&ws.join("lnk/f.txt"), "l\n");
        std::os::unix::fs::symlink(elsewhere.join(".git"), ws.join("lnk/.git")).unwrap();
        write(&ws.join("dangling/f.txt"), "d\n");
        std::os::unix::fs::symlink("/nonexistent/.git", ws.join("dangling/.git")).unwrap();
        write(&ws.join("emptygit/f.txt"), "e\n");
        std::fs::create_dir_all(ws.join("emptygit/.git")).unwrap();
        write(&ws.join("junkfile/f.txt"), "j\n");
        write(&ws.join("junkfile/.git"), "not a gitdir line\n");
        // Ignored: by the outer repository's .gitignore, or by the shadow's
        // own excludes for a plain folder.
        if outer {
            write(&ws.join(".gitignore"), "ign/\n");
            make_repo(&ws.join("ign/inner"), Nested::Unborn);
        } else {
            make_repo(&ws.join("node_modules/pkg"), Nested::Unborn);
        }
        write(&ws.join("plain/sub/p.txt"), "p\n");
        let outside = digest(&elsewhere);

        let repo = fx.repo().await;
        let s = snapshot_full(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        let mut want = vec![
            "a/b/deep".to_string(),
            "lnk".into(),
            String::from_utf8_lossy(b"odd\xffname").into_owned(),
            "with space/repo".into(),
        ];
        want.sort();
        assert_eq!(s.nested.shown, want, "outer_git={outer}");
        let e = entries(&repo, &s.sha).await;
        for n in ["a/b/deep", "lnk", "with space/repo"] {
            assert_absent(&e, n, &format!("outer_git={outer}"));
        }
        for f in ["dangling/f.txt", "emptygit/f.txt", "junkfile/f.txt"] {
            assert!(e.keys().any(|(_, p)| p == f), "{f}: {:?}", paths(&e));
        }
        assert!(
            !paths(&e)
                .iter()
                .any(|p| p.ends_with("/.git") || p.contains("/.git/")),
            "no .git entry is ever captured: {:?}",
            paths(&e)
        );
        assert!(
            !paths(&e).iter().any(|p| p.starts_with("odd")),
            "{:?}",
            paths(&e)
        );
        assert!(!paths(&e).iter().any(|p| p.starts_with("ign/")));
        assert!(!paths(&e).iter().any(|p| p.starts_with("node_modules/")));
        assert!(e.keys().any(|(_, p)| p == "plain/sub/p.txt"), "{e:?}");
        assert!(e.keys().any(|(_, p)| p == "ctl/c.txt"));
        assert_eq!(
            digest(&elsewhere),
            outside,
            "a .git symlink is not followed"
        );
    }
}

/// A folder that was ordinary in the turn's snapshots and became a
/// repository afterwards (the user ran `git init` in it): its files are in
/// the checkpoints, so the undo restores them, leaves the new `.git` as it
/// is and says the folder holds one. And the reverse: a repository whose
/// `.git` the turn removed is still left alone by the undo (its "before" is
/// not in the snapshot), so the undo deletes none of its files.
#[tokio::test]
async fn undo_restores_checkpointed_files_and_never_files_no_checkpoint_knows() {
    for outer in BOTH {
        let fx = Fx::new(outer, Nested::PlainFolder);
        let repo = fx.repo().await;
        let t = take_turn(&fx, &repo, |ws| {
            write(&ws.join("dep/x.txt"), "x-agent\n");
        })
        .await
        .unwrap();
        // After the turn the user makes dep a repository.
        sh(&fx.dep(), &["init", "-q", "-b", "main"]);
        let git_dir = digest(&fx.dep().join(".git"));
        let r = restore_report(&repo, &t.before, &t.after, false)
            .await
            .unwrap();
        assert_eq!(r.files, ["ctl/c.txt", "ctl/new.txt", "dep/x.txt"]);
        assert!(r.nested.is_empty(), "{:?}", r.nested);
        assert_eq!(r.plan.git_inside.shown, ["dep"]);
        assert_control_restored(&fx.ws);
        assert_eq!(read(&fx.dep().join("x.txt")), "x-1\n");
        assert_eq!(digest(&fx.dep().join(".git")), git_dir, "the .git stays");

        let fx = Fx::new(outer, Nested::Committed);
        let repo = fx.repo().await;
        let t = take_turn(&fx, &repo, |ws| {
            std::fs::remove_dir_all(ws.join("dep/.git")).unwrap();
        })
        .await
        .unwrap();
        assert!(t.after_nested.is_empty());
        let r = restore_report(&repo, &t.before, &t.after, false)
            .await
            .unwrap();
        assert_left_alone(&r, &format!("was, outer_git={outer}"));
        assert_control_restored(&fx.ws);
        assert_eq!(read(&fx.dep().join("x.txt")), "x-1\n", "not deleted");
        assert_eq!(read(&fx.dep().join("y.txt")), "y-1\n", "not deleted");
        assert_eq!(r.plan.changed_repos.shown, ["dep"], "it went away");
    }
}

/// What a turn plants in `src/`, after changing files the checkpoint holds
/// there.
#[derive(Clone, Copy, Debug)]
enum Plant {
    EmptyDir,
    JunkFile,
    Init,
    InitAndCommit,
}

/// G1, G2, G4: a `.git` made inside a folder during a turn, of any kind,
/// does not hide the turn's changes there or stop the undo: the change
/// list says modified, deleted and added as they were, and the undo puts
/// back every file the "before" checkpoint holds there, byte for byte,
/// deletes what the turn added, and leaves the `.git` exactly as it is
/// (and names the folder). For a real repository this also records what
/// git does: it enters a repository made over files the index tracks.
#[tokio::test]
async fn a_dot_git_made_during_a_turn_does_not_stop_undo_of_files_under_it() {
    for outer in BOTH {
        for plant in [
            Plant::EmptyDir,
            Plant::JunkFile,
            Plant::Init,
            Plant::InitAndCommit,
        ] {
            let ctx = format!("outer_git={outer} {plant:?}");
            let fx = Fx::new(outer, Nested::None);
            let ws = fx.ws.clone();
            write(&ws.join("src/main.rs"), "good\n");
            write(&ws.join("src/lib.rs"), "good lib\n");
            write(&ws.join("src/deep/mod.rs"), "good mod\n");
            if outer {
                sh(&ws, &["add", "."]);
                sh(&ws, &["commit", "-qm", "src"]);
            }
            let held = digest(&ws.join("src"));
            let repo = fx.repo().await;
            let t = take_turn(&fx, &repo, |ws| {
                let src = ws.join("src");
                write(&src.join("main.rs"), "EVIL\n");
                std::fs::remove_file(src.join("lib.rs")).unwrap();
                write(&src.join("deep/mod.rs"), "EVIL mod\n");
                write(&src.join("added.rs"), "new\n");
                match plant {
                    Plant::EmptyDir => std::fs::create_dir(src.join(".git")).unwrap(),
                    Plant::JunkFile => write(&src.join(".git"), "x"),
                    Plant::Init | Plant::InitAndCommit => {
                        sh(&src, &["init", "-q", "-b", "main"]);
                        if matches!(plant, Plant::InitAndCommit) {
                            sh(&src, &["add", "-A"]);
                            sh(&src, &["commit", "-qm", "agent"]);
                        }
                    }
                }
            })
            .await
            .unwrap_or_else(|e| panic!("{ctx}: {e}"));
            assert!(t.after_nested.is_empty(), "{ctx}: {:?}", t.after_nested);
            let mut ch = changed(&repo, &t.before, &t.after).await.unwrap();
            ch.retain(|(_, p)| p.starts_with("src/"));
            assert_eq!(
                ch,
                [
                    ('A', "src/added.rs".to_string()),
                    ('M', "src/deep/mod.rs".to_string()),
                    ('D', "src/lib.rs".to_string()),
                    ('M', "src/main.rs".to_string()),
                ],
                "{ctx}"
            );
            let git_entry = std::fs::symlink_metadata(ws.join("src/.git"))
                .map(|m| m.is_dir())
                .unwrap();
            let dot_git = if git_entry {
                digest(&ws.join("src/.git"))
            } else {
                BTreeMap::from([(
                    ".git".to_string(),
                    std::fs::read(ws.join("src/.git")).unwrap(),
                )])
            };
            let r = restore_report(&repo, &t.before, &t.after, false)
                .await
                .unwrap_or_else(|e| panic!("{ctx}: {e}"));
            assert_control_restored(&ws);
            let mut now = digest(&ws.join("src"));
            now.retain(|k, _| !k.starts_with(".git"));
            assert_eq!(now, held, "{ctx}: every held file is back, nothing added");
            let dot_git_now = if git_entry {
                digest(&ws.join("src/.git"))
            } else {
                BTreeMap::from([(
                    ".git".to_string(),
                    std::fs::read(ws.join("src/.git")).unwrap(),
                )])
            };
            assert_eq!(dot_git_now, dot_git, "{ctx}: the .git is left as it is");
            assert!(r.nested.is_empty(), "{ctx}");
            assert_eq!(r.plan.git_inside.shown, ["src"], "{ctx}");
        }
    }
}

/// G3: a repository there before the turn (with or without a commit) stays
/// out, and a turn that adds a `.git` elsewhere does not change that; an
/// undo writes nothing into it.
#[tokio::test]
async fn a_repository_there_before_the_turn_stays_out_of_undo() {
    for outer in BOTH {
        for nested in [Nested::Committed, Nested::Unborn] {
            let fx = Fx::new(outer, nested);
            let repo = fx.repo().await;
            let t = take_turn(&fx, &repo, |ws| {
                write(&ws.join("dep/x.txt"), "x-agent\n");
                std::fs::create_dir(ws.join("ctl/.git")).unwrap();
            })
            .await
            .unwrap();
            assert_eq!(t.before_nested, ["dep"]);
            assert_eq!(t.after_nested, ["dep"]);
            let as_left = digest(&fx.dep());
            let r = restore_report(&repo, &t.before, &t.after, false)
                .await
                .unwrap();
            assert_left_alone(&r, &format!("outer_git={outer} {nested:?}"));
            assert!(r.plan.changed_repos.is_empty());
            assert_control_restored(&fx.ws);
            assert_eq!(digest(&fx.dep()), as_left);
            assert!(
                fx.ws.join("ctl/.git").is_dir(),
                "an undo never deletes a .git"
            );
        }
    }
}

/// G2: a tracked file the turn replaced by a repository of the same name
/// is not reported deleted: the change list leaves it out and the
/// checkpoint names it as not recorded (left out); the undo leaves the
/// repository alone.
#[tokio::test]
async fn a_file_replaced_by_a_repository_is_not_reported_deleted() {
    for outer in BOTH {
        let fx = Fx::new(outer, Nested::None);
        write(&fx.dep(), "a file\n");
        if outer {
            sh(&fx.ws, &["add", "."]);
            sh(&fx.ws, &["commit", "-qm", "dep file"]);
        }
        let repo = fx.repo().await;
        let t = take_turn(&fx, &repo, |ws| {
            std::fs::remove_file(ws.join("dep")).unwrap();
            make_repo(&ws.join("dep"), Nested::Unborn);
        })
        .await
        .unwrap();
        assert_eq!(t.after_nested, ["dep"]);
        let ch = changed(&repo, &t.before, &t.after).await.unwrap();
        assert!(!ch.iter().any(|(_, p)| p == "dep"), "{ch:?}");
        let as_left = digest(&fx.dep());
        let r = restore_report(&repo, &t.before, &t.after, false)
            .await
            .unwrap();
        assert_left_alone(&r, &format!("outer_git={outer}"));
        assert_eq!(r.plan.changed_repos.shown, ["dep"]);
        assert_eq!(digest(&fx.dep()), as_left);
    }
}

/// Names shown in events and notes: at most 50, the rest counted, control
/// characters replaced; the snapshot itself still knows every one, so the
/// undo leaves all of them alone.
#[cfg(unix)]
#[tokio::test]
async fn shown_names_are_capped_and_cleaned_but_every_repository_stays_out() {
    let fx = Fx::new(false, Nested::None);
    for i in 0..60 {
        make_repo(&fx.ws.join(format!("r{i:02}")), Nested::Unborn);
    }
    let evil = fx.ws.join("zz\u{1b}[31m\nred");
    make_repo(&evil, Nested::Unborn);
    let repo = fx.repo().await;
    let s = snapshot_full(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    assert_eq!(s.nested.shown.len(), SHOWN_MAX);
    assert_eq!(s.nested.more, 11);
    assert_eq!(nested_in(&repo, &s.sha).await.len(), 61);
    let names = Names::of(&nested_in(&repo, &s.sha).await);
    let last = Names::of(&nested_in(&repo, &s.sha).await[60..]);
    assert_eq!(last.shown, ["zz\u{FFFD}[31m\u{FFFD}red"]);
    assert!(
        names.listed().ends_with("and 11 more"),
        "{}",
        names.listed()
    );
    // The 61st (not shown) is still left alone by an undo.
    let t = take_turn(&fx, &repo, |_| {
        std::fs::remove_dir_all(evil.join(".git")).unwrap();
    })
    .await
    .unwrap();
    let r = restore_report(&repo, &t.before, &t.after, false)
        .await
        .unwrap();
    assert!(
        r.files.iter().all(|f| !f.starts_with("zz")),
        "{:?}",
        r.files
    );
    assert!(evil.join("x.txt").exists(), "not deleted");
}

/// A shadow repository written before A.15 holds gitlinks: in its index
/// and in old snapshots. The next snapshot drops the gitlink (and leaves
/// the repository out), and a restore of an old snapshot never recreates
/// a folder from a gitlink.
#[tokio::test]
async fn a_gitlink_from_an_older_snapshot_is_dropped_and_never_restored() {
    let fx = Fx::new(false, Nested::Committed);
    let repo = fx.repo().await;
    // What an older snapshot did: `add -A` into the shadow's own index.
    let index = repo.shadow.clone().unwrap().join("index");
    let mut c = command(&repo, Some(&index));
    c.args(["add", "-A", "--", "."]);
    assert!(exec(c, None, GIT_TIMEOUT).await.unwrap().status.success());
    let tree = run(&repo, &["write-tree"], Some(&index)).await.unwrap();
    let old = run(&repo, &["commit-tree", tree.as_str(), "-m", "old"], None)
        .await
        .unwrap();
    assert!(
        entries(&repo, &old)
            .await
            .contains_key(&("160000".to_string(), "dep".to_string())),
        "precondition: the old snapshot holds a gitlink"
    );
    let s = snapshot_full(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    assert_eq!(s.nested.shown, ["dep"]);
    assert_absent(&entries(&repo, &s.sha).await, "dep", "next snapshot");

    // The repository goes; undoing back to the old snapshot creates no
    // empty folder from its gitlink.
    std::fs::remove_dir_all(fx.dep()).unwrap();
    let now = snapshot(&repo, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    let r = restore_report(&repo, &old, &now, false).await.unwrap();
    assert_left_alone(&r, "old gitlink");
    assert!(!fx.dep().exists());
}

/// Timing: snapshot of ~100,000 files with nested repositories at several
/// depths, the detection (A.15) against plain `git add -A` (before A.15),
/// cold and warm. `cargo test -p harness-node --lib bench_nested -- --ignored --nocapture`.
#[tokio::test]
#[ignore]
async fn bench_nested_detection_100k_files() {
    let d = tempfile::tempdir().unwrap();
    let ws = d.path().join("big");
    for i in 0..1000 {
        let dir = ws.join(format!("d{}/e{}/f{i}", i % 10, i % 100));
        std::fs::create_dir_all(&dir).unwrap();
        for j in 0..100 {
            std::fs::write(dir.join(format!("f{j}.txt")), format!("file {i} {j}\n")).unwrap();
        }
    }
    // Committed, so plain `add -A` can run at all.
    for n in ["n1", "d3/n2", "d4/e14/f14/n3", "d5/e25/f25/x/y/n4"] {
        make_repo(&ws.join(n), Nested::Committed);
    }
    let base = d.path().join("cp");
    let legacy_base = d.path().join("cp-legacy");
    let repo = repo_for(&ws, Some(&base)).await.unwrap();
    let legacy = repo_for(&ws, Some(&legacy_base)).await.unwrap();
    let legacy_index = legacy.shadow.clone().unwrap().join("index");
    for round in 1..=3 {
        let t = std::time::Instant::now();
        let mut c = command(&legacy, Some(&legacy_index));
        c.args(["add", "-A", "--", "."]);
        assert!(exec(c, None, GIT_TIMEOUT).await.unwrap().status.success());
        run(&legacy, &["write-tree"], Some(&legacy_index))
            .await
            .unwrap();
        let old = t.elapsed();
        // The same work: stage the folder and write its tree.
        let t = std::time::Instant::now();
        let tree = worktree_tree(&repo).await.unwrap();
        let new = t.elapsed();
        assert_eq!(tree.nested.len(), 4);
        eprintln!("round {round}: add -A {old:?}; A.15 staging {new:?}");
        std::fs::write(ws.join("d0/e0/f0/f0.txt"), format!("edit {round}\n")).unwrap();
    }
}

// ------------------------------------------- review proofs (A.15 review)

/// `git add -A` into `legacy`'s own index (as before A.15), tree id.
async fn legacy_tree(legacy: &Repo) -> Result<String, String> {
    let idx = legacy.shadow.clone().unwrap().join("index");
    let mut c = command(legacy, Some(&idx));
    c.args(["add", "-A", "--", "."]);
    let o = exec(c, None, GIT_TIMEOUT).await?;
    if !o.status.success() {
        return Err(String::from_utf8_lossy(&o.stderr).into_owned());
    }
    run(legacy, &["write-tree"], Some(&idx)).await
}

async fn ls_tree(repo: &Repo, tree: &str) -> String {
    run(repo, &["ls-tree", "-r", tree], None).await.unwrap()
}

type Step = (&'static str, Box<dyn Fn(&Path)>);

/// (Review proof.) The staging must build the same tree as `add -A` for a
/// folder without nested repositories, step after step (the index carries
/// over, as in real use).
#[tokio::test]
async fn staging_builds_the_same_tree_as_add_all_without_nested_repositories() {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{PermissionsExt, symlink};
    for outer in BOTH {
        let d = tempfile::tempdir().unwrap();
        let ws = d.path().join("ws");
        write(&ws.join("a.txt"), "a\n");
        write(&ws.join("sub/b.txt"), "b\n");
        write(&ws.join("exec.sh"), "echo\n");
        write(&ws.join("f2"), "file then dir\n");
        write(&ws.join("d2/in.txt"), "dir then file\n");
        write(&ws.join("Readme"), "case\n");
        write(&ws.join("t/x"), "t\n");
        write(&ws.join("tracked-then-ignored.txt"), "x\n");
        write(&ws.join(".gitignore"), "*.log\n");
        write(&ws.join("sub/.gitignore"), "*.tmp\n");
        write(&ws.join("ignored.log"), "log\n");
        write(&ws.join("sub/c.tmp"), "tmp\n");
        if outer {
            sh(&ws, &["init", "-q", "-b", "main"]);
            sh(&ws, &["add", "."]);
            sh(&ws, &["commit", "-qm", "outer"]);
        }
        let repo = repo_for(&ws, Some(&d.path().join("cp"))).await.unwrap();
        let legacy = repo_for(&ws, Some(&d.path().join("cp-legacy")))
            .await
            .unwrap();
        let steps: Vec<Step> = vec![
            ("initial", Box::new(|_| {})),
            (
                "odd names, symlinks, long path, x.git",
                Box::new(|ws| {
                    write(&ws.join("-dash"), "d\n");
                    write(&ws.join("with space"), "s\n");
                    write(&ws.join("nl\nname"), "n\n");
                    std::fs::write(ws.join(std::ffi::OsStr::from_bytes(b"bad\xff\xfe")), "u")
                        .unwrap();
                    let long = "x".repeat(200);
                    write(&ws.join(format!("{long}/{long}/{long}/f")), "l\n");
                    write(&ws.join("x.git/f"), "in x.git\n");
                    symlink("a.txt", ws.join("link")).unwrap();
                    symlink("nowhere", ws.join("dangling")).unwrap();
                    symlink("a.txt", ws.join("link2")).unwrap();
                }),
            ),
            (
                "delete, mode, file<->dir, symlink->file, case rename, dir->symlink",
                Box::new(|ws| {
                    std::fs::remove_file(ws.join("a.txt")).unwrap();
                    std::fs::set_permissions(
                        ws.join("exec.sh"),
                        std::fs::Permissions::from_mode(0o755),
                    )
                    .unwrap();
                    std::fs::remove_file(ws.join("f2")).unwrap();
                    write(&ws.join("f2/inner"), "now dir\n");
                    std::fs::remove_dir_all(ws.join("d2")).unwrap();
                    write(&ws.join("d2"), "now file\n");
                    std::fs::remove_file(ws.join("link2")).unwrap();
                    write(&ws.join("link2"), "now a file\n");
                    std::fs::rename(ws.join("Readme"), ws.join("README")).unwrap();
                    std::fs::remove_dir_all(ws.join("t")).unwrap();
                    write(&ws.join("elsewhere/x"), "t2\n");
                    symlink("elsewhere", ws.join("t")).unwrap();
                    std::fs::create_dir_all(ws.join("empty/dir")).unwrap();
                }),
            ),
            (
                ".gitignore changed",
                Box::new(|ws| {
                    write(&ws.join(".gitignore"), "tracked-then-ignored.txt\n");
                    write(&ws.join("tracked-then-ignored.txt"), "changed\n");
                    write(&ws.join("sub/.gitignore"), "\n");
                }),
            ),
        ];
        for (name, step) in steps {
            step(&ws);
            let old = legacy_tree(&legacy).await;
            let new = worktree_tree(&repo).await.map(|t| t.id);
            match (&old, &new) {
                (Ok(o), Ok(n)) if o != n => panic!(
                    "outer={outer} step {name:?}: trees differ\nadd -A:\n{}\nnew:\n{}",
                    ls_tree(&legacy, o).await,
                    ls_tree(&repo, n).await
                ),
                (Ok(_), Ok(_)) => {}
                _ => panic!("outer={outer} step {name:?}: add -A {old:?}, new {new:?}"),
            }
        }
        // An unreadable new file: both must agree on failing (or not).
        write(&ws.join("secret"), "s\n");
        std::fs::set_permissions(ws.join("secret"), std::fs::Permissions::from_mode(0o000))
            .unwrap();
        let old = legacy_tree(&legacy).await;
        let new = worktree_tree(&repo).await.map(|t| t.id);
        std::fs::set_permissions(ws.join("secret"), std::fs::Permissions::from_mode(0o644))
            .unwrap();
        eprintln!("outer={outer} unreadable: add -A {old:?}, new {new:?}");
        assert_eq!(
            old.is_ok(),
            new.is_ok(),
            "unreadable: add -A {old:?}, new {new:?}"
        );
    }
}

/// (Review proof.) An agent that plants an empty `src/.git` after changing
/// files under `src/` cannot stop the undo of those changes.
#[tokio::test]
async fn a_planted_empty_dot_git_does_not_defeat_undo() {
    for outer in BOTH {
        let d = tempfile::tempdir().unwrap();
        let ws = d.path().join("ws");
        write(&ws.join("src/main.rs"), "good\n");
        write(&ws.join("src/lib.rs"), "good lib\n");
        if outer {
            sh(&ws, &["init", "-q", "-b", "main"]);
            sh(&ws, &["add", "."]);
            sh(&ws, &["commit", "-qm", "outer"]);
        }
        let repo = repo_for(&ws, Some(&d.path().join("cp"))).await.unwrap();
        let before = snapshot_full(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        // The turn: wreck src/, then plant an empty `.git` directory.
        write(&ws.join("src/main.rs"), "EVIL\n");
        std::fs::remove_file(ws.join("src/lib.rs")).unwrap();
        std::fs::create_dir(ws.join("src/.git")).unwrap();
        let after = snapshot_full(&repo, "refs/harness/s/t/after", "a")
            .await
            .unwrap();
        let files = changed(&repo, &before.sha, &after.sha).await.unwrap();
        eprintln!(
            "outer={outer} after.nested={:?} turn files={files:?}",
            after.nested
        );
        let r = restore_report(&repo, &before.sha, &after.sha, false)
            .await
            .unwrap();
        eprintln!(
            "outer={outer} restored={:?} left alone={:?}",
            r.files, r.nested
        );
        assert_eq!(
            read(&ws.join("src/main.rs")),
            "good\n",
            "outer={outer}: undo must restore src/main.rs"
        );
        assert_eq!(read(&ws.join("src/lib.rs")), "good lib\n", "outer={outer}");
    }
}

/// `--data-dir` is used as given (crates/cli/src/main.rs:156,525),
/// so the shadow's `--git-dir` can be relative to the node's working
/// directory. Git runs in the folder's top, so a relative one would
/// resolve against the folder (review proof; it did, and every snapshot failed).
#[tokio::test]
async fn a_relative_checkpoint_dir_still_snapshots() {
    let d = tempfile::tempdir().unwrap();
    let ws = d.path().join("ws");
    write(&ws.join("a.txt"), "a\n");
    // A relative base, as `--data-dir data` gives.
    let base_abs = tempfile::tempdir_in(".").unwrap();
    let rel = base_abs
        .path()
        .strip_prefix(std::env::current_dir().unwrap())
        .unwrap()
        .join("checkpoints");
    assert!(rel.is_relative());
    let repo = repo_for(&ws, Some(&rel)).await.unwrap();
    let s = snapshot_full(&repo, "refs/harness/s/t/before", "b").await;
    assert!(s.is_ok(), "snapshot with a relative data dir: {s:?}");
}

/// Context: before A.15, `add -A` captured files under a
/// folder holding an empty `.git` directory, so undo covered them.
#[tokio::test]
async fn add_all_captured_files_beside_an_empty_dot_git() {
    let d = tempfile::tempdir().unwrap();
    let ws = d.path().join("ws");
    write(&ws.join("src/main.rs"), "good\n");
    std::fs::create_dir(ws.join("src/.git")).unwrap();
    let legacy = repo_for(&ws, Some(&d.path().join("cp"))).await.unwrap();
    let t = legacy_tree(&legacy).await.unwrap();
    assert!(ls_tree(&legacy, &t).await.contains("src/main.rs"));
}

/// (Review proof.) A one-byte `.git` file planted in `src/` does not make the turn's
/// change list say it deleted `src/main.rs`, which it modified.
#[tokio::test]
async fn a_planted_dot_git_file_does_not_falsify_the_turn_files() {
    let d = tempfile::tempdir().unwrap();
    let ws = d.path().join("ws");
    write(&ws.join("src/main.rs"), "good\n");
    let repo = repo_for(&ws, Some(&d.path().join("cp"))).await.unwrap();
    let before = snapshot_full(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    write(&ws.join("src/main.rs"), "EVIL\n");
    write(&ws.join("src/.git"), "x");
    let after = snapshot_full(&repo, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    let files = changed(&repo, &before.sha, &after.sha).await.unwrap();
    assert_eq!(
        files,
        vec![('M', "src/main.rs".to_string())],
        "the turn modified src/main.rs; nested={:?}",
        after.nested
    );
}
