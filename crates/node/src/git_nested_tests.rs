//! Experiment B.5 (ADOPTION_PLAN.md): what a checkpoint and an undo do with a
//! git repository nested in the folder.
//!
//! Every test states what happens TODAY. Several of those facts are gaps
//! (work that no snapshot holds and an undo leaves alone, or a snapshot that
//! cannot be taken at all); the doc comments say which, and what the
//! assertion should become once task A.15 and the nested-repository decision
//! are made. These tests live inside the crate because `restore` is test-only
//! and the `git` module is private.
//!
//! Each scenario runs with the outer folder both as a git repository and as a
//! plain folder (the checkpoint code uses the same shadow repository for both,
//! but the outer `.git` is a different thing to leave alone), and always with
//! a CONTROL: an ordinary sub-folder `ctl/` (no `.git` inside) that the turn
//! also changes. The control must be captured as files and restored exactly;
//! `the_control_fails_when_a_folder_is_not_captured` shows the control check
//! really fails when capture is missing.

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

/// Every file under `dir` (relative path, bytes). Symlinks are not followed.
/// Used to prove a directory (a nested `.git`) was not touched.
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

/// Names under `dir`, one level.
fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|r| {
            r.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
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
        let dep = self.dep();
        match nested {
            Nested::None => {}
            Nested::PlainFolder => {
                write(&dep.join("x.txt"), "x-1\n");
                write(&dep.join("y.txt"), "y-1\n");
            }
            Nested::Committed | Nested::Unborn => {
                std::fs::create_dir_all(&dep).unwrap();
                sh(&dep, &["init", "-q", "-b", "main"]);
                write(&dep.join("x.txt"), "x-1\n");
                write(&dep.join("y.txt"), "y-1\n");
                if nested == Nested::Committed {
                    sh(&dep, &["add", "."]);
                    sh(&dep, &["commit", "-qm", "dep one"]);
                }
            }
        }
    }

    async fn repo(&self) -> Repo {
        let r = repo_for(&self.ws, Some(&self.base)).await.unwrap();
        assert!(r.is_shadow());
        r
    }

    /// `git add -A` into a throw-away index, exactly as a snapshot does it:
    /// the exit status and what git printed on stderr.
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

/// A before/after pair around one simulated turn.
struct Turn {
    before: String,
    after: String,
}

/// The control's change, applied by every simulated turn.
fn change_control(ws: &Path) {
    write(&ws.join("ctl/c.txt"), "ctl-2\n");
    write(&ws.join("ctl/new.txt"), "agent made this\n");
}

/// Snapshot, run `mutate` (the agent's turn, which also changes the control),
/// snapshot again.
async fn take_turn(fx: &Fx, repo: &Repo, mutate: impl FnOnce(&Path)) -> Result<Turn, String> {
    let before = snapshot(repo, "refs/harness/s/t/before", "b").await?;
    change_control(&fx.ws);
    mutate(&fx.ws);
    let after = snapshot(repo, "refs/harness/s/t/after", "a").await?;
    Ok(Turn { before, after })
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

/// CASE 1 (committed nested repository), today: a snapshot does not contain
/// the nested folder's files at all. It records a single gitlink (mode
/// 160000, the nested HEAD's commit id) at `dep`. `git add` succeeds and
/// prints a "warning: adding embedded git repository" plus a hint about
/// submodules on stderr, which the harness discards. The same on a plain
/// outer folder and on an outer repository.
///
/// Control: the same folder with `dep/` as an ordinary folder captures
/// `dep/x.txt` and `dep/y.txt` as files and no gitlink.
///
/// Known gap (feeds A.15 and the "skip nested repositories?" decision): the
/// nested files are in no snapshot, so no undo can restore them. If
/// checkpoints are changed to capture nested files, the 160000 assertion
/// becomes `100644 dep/x.txt` and `dep/y.txt`, and no `.git/` paths may ever
/// appear in a snapshot. If they are changed to skip nested repositories, the
/// gitlink assertion becomes "no entry for `dep`" (and the warning goes).
#[tokio::test]
async fn a_nested_repository_with_a_commit_is_a_gitlink_and_its_files_are_not_captured() {
    for outer in BOTH {
        let fx = Fx::new(outer, Nested::Committed);
        let repo = fx.repo().await;
        let (ok, stderr) = fx.add_output(&repo).await;
        assert!(ok, "outer_git={outer}: git add must not fail: {stderr}");
        assert!(
            stderr.contains("warning: adding embedded git repository: dep"),
            "outer_git={outer}: {stderr}"
        );
        let sha = snapshot(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        let e = entries(&repo, &sha).await;
        let head = sh(&fx.dep(), &["rev-parse", "HEAD"]);
        assert_eq!(
            e.get(&("160000".to_string(), "dep".to_string())),
            Some(&head),
            "outer_git={outer}: {e:?}"
        );
        assert!(
            !paths(&e).iter().any(|p| p.starts_with("dep/")),
            "outer_git={outer}: nested files are not in the snapshot: {:?}",
            paths(&e)
        );
        assert!(
            e.keys().any(|(m, p)| m == "100644" && p == "ctl/c.txt"),
            "control: ctl is captured"
        );

        // Control: an ordinary folder is captured as files.
        let ctl = Fx::new(outer, Nested::PlainFolder);
        let r = ctl.repo().await;
        let s = snapshot(&r, "refs/harness/s/t/before", "b").await.unwrap();
        let e = entries(&r, &s).await;
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

/// CASE 2 (nested repository with no commit yet), today: `git add -A` FAILS
/// ("'dep/' does not have a commit checked out"), so `snapshot` returns an
/// error and NO snapshot of the folder exists: not even the files outside
/// the nested folder are captured, and no ref is written. The same on a
/// plain outer folder and on an outer repository. In the session this means
/// the turn runs without a checkpoint (`checkpoint_before` logs a warning and
/// returns `None`).
///
/// Control: with `dep/` an ordinary folder holding the same files the
/// snapshot succeeds and captures them.
///
/// Known gap (feeds the "skip nested repositories?" decision, and is the
/// strongest argument for it): one `git init` by the user or the agent
/// disables undo for the whole folder until the nested repository gets a
/// commit. If checkpoints skip nested repositories (exclude every directory
/// containing `.git`) the assertion becomes `Ok` with `ctl/c.txt` and
/// `top.txt` captured and nothing under `dep`.
#[tokio::test]
async fn a_nested_repository_without_a_commit_makes_every_snapshot_fail() {
    for outer in BOTH {
        let fx = Fx::new(outer, Nested::Unborn);
        let repo = fx.repo().await;
        let (ok, stderr) = fx.add_output(&repo).await;
        assert!(!ok, "outer_git={outer}");
        assert!(
            stderr.contains("'dep/' does not have a commit checked out"),
            "outer_git={outer}: {stderr}"
        );
        let err = snapshot(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap_err();
        assert!(
            err.contains("does not have a commit checked out"),
            "outer_git={outer}: {err}"
        );
        // Nothing was recorded for the folder.
        assert!(
            run(&repo, &["for-each-ref", "refs/harness/"], None)
                .await
                .unwrap()
                .is_empty()
        );

        // Once the nested repository has a commit, snapshots work again.
        sh(&fx.dep(), &["add", "."]);
        sh(&fx.dep(), &["commit", "-qm", "first"]);
        assert!(
            snapshot(&repo, "refs/harness/s/t/before", "b")
                .await
                .is_ok()
        );

        // Control: the same files in an ordinary folder snapshot fine.
        let ctl = Fx::new(outer, Nested::PlainFolder);
        let r = ctl.repo().await;
        let s = snapshot(&r, "refs/harness/s/t/before", "b").await.unwrap();
        let e = entries(&r, &s).await;
        assert!(e.keys().any(|(_, p)| p == "dep/x.txt"), "control");
    }
}

/// CASE 2b, today: the realistic path to case 2. The "before" snapshot is
/// fine; the agent then runs `git init` in a new folder (files written, no
/// commit yet) and the "after" snapshot of the same turn fails. The turn has
/// a before checkpoint and no after checkpoint, so it cannot be undone from a
/// pair.
///
/// Known gap, same decision as case 2.
#[tokio::test]
async fn a_repository_initialised_during_a_turn_makes_the_after_snapshot_fail() {
    for outer in BOTH {
        let fx = Fx::new(outer, Nested::None);
        let repo = fx.repo().await;
        let err = take_turn(&fx, &repo, |_| {
            let dep = fx.dep();
            std::fs::create_dir_all(&dep).unwrap();
            sh(&dep, &["init", "-q", "-b", "main"]);
            write(&dep.join("x.txt"), "x-agent\n");
        })
        .await
        .err()
        .expect("the after snapshot fails");
        assert!(
            err.contains("does not have a commit checked out"),
            "outer_git={outer}: {err}"
        );
        // The before snapshot exists, the after one does not.
        let refs = run(
            &repo,
            &["for-each-ref", "--format=%(refname)", "refs/harness/"],
            None,
        )
        .await
        .unwrap();
        assert_eq!(refs, "refs/harness/s/t/before", "outer_git={outer}");
    }
}

// ---------------------------------------------------------------- case 3

/// CASE 3 (uncommitted work inside a committed nested repository), today:
/// the undo restores the control but NOTHING inside the nested repository.
/// The agent's edit of a tracked file, its new file and its deletion of
/// another tracked file all stay; the user's own uncommitted work that the
/// agent overwrote cannot come back. `changed(before, after)` lists no path
/// under `dep`, the undo reports success, and nothing warned or refused it
/// (the unforced check cannot see inside `dep` either). The nested `.git`
/// (index, HEAD, refs, objects) and, with an outer repository, the user's own
/// `.git` are byte for byte untouched by both snapshots and the undo.
///
/// Known gap, and a loss of the user's work (feeds A.15: refuse, or at least
/// warn, when an undo's turn touched a nested repository that no snapshot
/// holds; the refusal cannot come from the snapshot diff, which is empty, so
/// it has to come from looking at the nested repository's state, for example
/// its status or HEAD, before and after the turn). Once fixed the assertions
/// on `dep/x.txt`, `dep/new.txt` and `dep/y.txt` become "the user's content
/// is back" if nested files are captured, or the undo returns an error naming
/// `dep` if it is refused.
#[tokio::test]
async fn undo_does_not_restore_uncommitted_work_in_a_nested_repository() {
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
        let ch = changed(&repo, &t.before, &t.after).await.unwrap();
        assert!(
            !ch.iter().any(|(_, p)| p.starts_with("dep")),
            "outer_git={outer}: the diff has no trace of the nested edits: {ch:?}"
        );
        // Between the snapshots only the control changed: dep's `.git` too.
        assert_eq!(
            guarded_state(&fx).0,
            guard.0,
            "snapshots must not touch dep/.git"
        );

        let files = restore(&repo, &t.before, &t.after, false).await.unwrap();
        assert!(
            files.iter().all(|p| !p.starts_with("dep")),
            "outer_git={outer}: {files:?}"
        );
        assert_control_restored(&fx.ws);
        // The nested work is exactly as the agent left it, not as the user had it.
        assert_eq!(read(&fx.dep().join("x.txt")), "x-agent\n");
        assert_eq!(read(&fx.dep().join("new.txt")), "agent-overwrote\n");
        assert!(!fx.dep().join("y.txt").exists());
        // dep/.git and the outer .git are untouched by snapshot and restore.
        assert_eq!(guarded_state(&fx), guard, "outer_git={outer}");
    }
}

/// CASE 3b, today: when the agent COMMITS inside the nested repository the
/// gitlink changes, so `changed` shows `M dep` and the undo lists `dep` among
/// the files it restored, but the nested repository's HEAD and files stay
/// where the agent left them (restoring a gitlink only makes the directory
/// exist). The undo's own report says `dep` was restored when it was not.
/// The nested `.git` is untouched by the restore.
///
/// Known gap (A.15): a gitlink that differs must not be reported as
/// restored; refuse or warn naming `dep`. After the fix the final assertions
/// become the refusal (or, if nested repositories are skipped, `dep` is not
/// listed at all).
#[tokio::test]
async fn undo_reports_a_nested_repository_restored_when_the_agent_committed_in_it() {
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
        assert!(ch.contains(&('M', "dep".to_string())), "{ch:?}");
        let guard = guarded_state(&fx);

        let files = restore(&repo, &t.before, &t.after, false).await.unwrap();
        assert!(files.contains(&"dep".to_string()), "{files:?}");
        assert_control_restored(&fx.ws);
        assert_eq!(
            sh(&fx.dep(), &["rev-parse", "HEAD"]),
            head_after,
            "outer_git={outer}: HEAD was not moved back"
        );
        assert_eq!(read(&fx.dep().join("x.txt")), "x-agent\n");
        assert_eq!(guarded_state(&fx), guard, "restore must not touch any .git");
    }
}

// ---------------------------------------------------------------- case 4

/// CASE 4 (the turn deletes a committed nested repository), today: the undo
/// brings back an EMPTY directory `dep`: no files, no `.git`, no history.
/// The undo reports `dep` restored. Whatever the nested repository held, its
/// commits (which exist nowhere else if it had no remote) and uncommitted
/// work, is gone for good; ordinary folders (the control) come back whole.
///
/// Known gap and the most serious data loss here (A.15 should refuse a turn
/// that deleted a nested repository, or the harness should refuse to let the
/// agent delete one without a prompt; neither is a snapshot problem). If
/// checkpoints captured nested files the assertion on `names(dep)` becomes
/// `[".git"?, "x.txt", "y.txt"]` as the decision dictates (a `.git` must
/// never be written back from a snapshot of files); if they skip nested
/// repositories the undo would not touch `dep` at all and the directory would
/// stay deleted.
#[tokio::test]
async fn undo_of_a_deleted_nested_repository_brings_back_an_empty_folder() {
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
        assert!(ch.contains(&('D', "dep".to_string())), "{ch:?}");
        assert!(!fx.dep().exists());

        let files = restore(&repo, &t.before, &t.after, false).await.unwrap();
        assert!(files.contains(&"dep".to_string()), "{files:?}");
        assert_control_restored(&fx.ws);
        assert!(
            fx.dep().is_dir(),
            "outer_git={outer}: dep comes back as a directory"
        );
        assert_eq!(
            names(&fx.dep()),
            Vec::<String>::new(),
            "outer_git={outer}: but it is empty: no files and no .git"
        );
    }
}

/// CASE 4b, today: the reverse. The agent CREATES a repository during the
/// turn (a `git clone` or `git init` + commit). The "after" snapshot holds
/// it as a gitlink only; the undo lists `dep` among the files it handled but
/// removes nothing: the clone stays, with its `.git` and files. Nothing of
/// the user's is lost (the undo leaves the agent's new work behind), but the
/// undo is incomplete and says it was complete. Control: a new file in an
/// ordinary folder IS removed by the same undo.
///
/// Known gap (A.15/nested decision): the report must not list `dep`; after a
/// "skip nested repositories" decision `dep` is simply invisible to undo,
/// which is the same observable behaviour with a truthful report.
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
        assert!(ch.contains(&('A', "dep".to_string())), "{ch:?}");
        let guard = guarded_state(&fx);

        let files = restore(&repo, &t.before, &t.after, false).await.unwrap();
        assert!(files.contains(&"dep".to_string()), "{files:?}");
        assert_control_restored(&fx.ws);
        assert!(
            fx.dep().join(".git").is_dir(),
            "outer_git={outer}: the created repository is still there"
        );
        assert_eq!(read(&fx.dep().join("x.txt")), "x-1\n");
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

/// CASE 5a, today: a registered submodule is recorded exactly like an
/// unregistered nested repository: `.gitmodules` is a file in the snapshot,
/// `dep` is a gitlink and its files are not captured. Its `.git` is a
/// one-line file (`gitdir: ../.git/modules/dep`) pointing into the outer
/// repository; that file is not captured either. No difference in what a
/// snapshot holds. git's embedded-repository warning is printed or not as
/// the assertion below records.
///
/// Known gap, same as case 1.
#[tokio::test]
async fn a_registered_submodule_is_captured_as_a_gitlink_like_a_nested_repository() {
    let fx = submodule_fx();
    let repo = fx.repo().await;
    assert!(
        fx.dep().join(".git").is_file(),
        "a submodule's .git is a gitfile"
    );
    let (ok, stderr) = fx.add_output(&repo).await;
    assert!(ok, "{stderr}");
    let embedded_warning = stderr.contains("adding embedded git repository");
    let sha = snapshot(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    let e = entries(&repo, &sha).await;
    let head = sh(&fx.dep(), &["rev-parse", "HEAD"]);
    assert_eq!(
        e.get(&("160000".to_string(), "dep".to_string())),
        Some(&head)
    );
    assert!(e.keys().any(|(m, p)| m == "100644" && p == ".gitmodules"));
    assert!(
        !paths(&e).iter().any(|p| p.starts_with("dep/")),
        "{:?}",
        paths(&e)
    );
    assert!(
        e.keys().any(|(m, p)| m == "100644" && p == "ctl/c.txt"),
        "control"
    );
    // Recorded observation: git warns for a registered submodule too.
    assert!(embedded_warning, "{stderr}");
}

/// CASE 5b, today: undo does not restore uncommitted work in a registered
/// submodule (same as case 3) and does not touch the submodule's git
/// directory in `.git/modules/dep` or its gitfile.
///
/// Known gap, same as case 3 (A.15).
#[tokio::test]
async fn undo_does_not_restore_uncommitted_work_in_a_submodule() {
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
    let files = restore(&repo, &t.before, &t.after, false).await.unwrap();
    assert!(files.iter().all(|p| !p.starts_with("dep")), "{files:?}");
    assert_control_restored(&fx.ws);
    assert_eq!(read(&fx.dep().join("x.txt")), "x-agent\n");
    assert_eq!(read(&fx.dep().join("new.txt")), "agent-overwrote\n");
    assert_eq!(guarded_state(&fx), guard);
    assert_eq!(digest(&fx.ws.join(".git/modules/dep")), modules);
    assert_eq!(read(&fx.dep().join(".git")), gitfile);
}

/// CASE 5c, today: the turn deletes a registered submodule's working
/// folder. The undo brings back an empty directory `dep` (no gitfile, no
/// files), exactly as in case 4, but with one difference: the submodule's git
/// directory `.git/modules/dep` survives untouched in the outer repository,
/// so its COMMITTED content is recoverable with `git submodule update`
/// (uncommitted work is not). An unregistered nested repository keeps
/// nothing of the kind.
///
/// Known gap, same as case 4.
#[tokio::test]
async fn undo_of_a_deleted_submodule_brings_back_an_empty_folder_but_its_git_dir_survives() {
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
    restore(&repo, &t.before, &t.after, false).await.unwrap();
    assert_control_restored(&fx.ws);
    assert_eq!(names(&fx.dep()), Vec::<String>::new());
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
