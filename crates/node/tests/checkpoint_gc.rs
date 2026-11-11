//! Experiment B.2 (ADOPTION_PLAN): what the user's own git housekeeping does
//! to checkpoints that borrow objects from the user's repository.
//!
//! A checkpoint repository in a git folder borrows the user's objects
//! (`objects/info/alternates`, `crates/node/src/git.rs`), so it stores only
//! what the user's repository lacks. These tests assert what happens when
//! the user's own housekeeping (amend, `gc --prune=now`, `repack -ad`,
//! deleting a branch, re-cloning) removes objects a checkpoint refers to.
//!
//! The experiment found a gap; A.16 closed what can be closed while
//! checkpoints borrow (decision O11 is whether they stop):
//! - a restore is all or nothing: every blob it writes is checked and copied
//!   into the checkpoint repository's own objects before any file is touched,
//!   and a checkpoint that depends on content the user's repository no
//!   longer has is refused, with that reason, with the folder unchanged and
//!   nothing logged;
//! - a damaged checkpoint index (it names a blob that is gone) is rebuilt
//!   from the folder once, so later turns get checkpoints again.
//!
//! Tests named `..._is_refused_...` record the residual: while checkpoints
//! borrow, such a checkpoint cannot be restored. Once they own every blob
//! (O11), those assertions become: the restore is `Ok` and the file holds X.
//!
//! Every git call here runs with a fixed identity, no system or global
//! config, and a cleared environment, in a temporary directory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::event::*;
use harness_core::{RawEvent, SessionId};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};

const X: &str = "X: the content a checkpoint depends on\n";
const SCRATCH: &str = "untracked scratch file\n";

struct Rig {
    dir: tempfile::TempDir,
    ws: PathBuf,
    _node: NodeRuntime,
    link: NodeLink,
    seen: Vec<RawEvent>,
    sid: SessionId,
    n: u32,
}

#[derive(Debug)]
struct Turn {
    id: Option<String>,
    phases: Vec<String>,
}

#[derive(Debug)]
struct Restore {
    result: Result<(), String>,
    phases: Vec<String>,
    /// The id of the "folder as it was before this restore" checkpoint.
    saved: Option<String>,
    folder_before: BTreeMap<String, Vec<u8>>,
    folder_after: BTreeMap<String, Vec<u8>>,
}

impl Rig {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir(&ws).unwrap();
        let store = Store::open(&dir.path().join("node.db"), Durability::Full).unwrap();
        let cfg = NodeConfig {
            node_id: "local".into(),
            hostname: "test".into(),
            policy: NodePolicy {
                workspace_roots: vec![dir.path().into()],
                max_approval_mode: harness_core::policy::ApprovalMode::Auto,
                ..Default::default()
            },
            store,
            backend: Arc::new(NativeBackend::new(
                Arc::new(Mock {
                    delay: Duration::from_millis(5),
                }),
                harness_tools::builtin(),
                NativeConfig::default(),
            )),
            default_model: "mock".into(),
            secrets: harness_core::SecretSet::new(vec![]),
            profile: harness_core::event::Profile::Agentic,
            incognito_ttl: None,
            checkpoints_dir: Some(dir.path().join("checkpoints")),
            artifacts: None,
        };
        let node = NodeRuntime::start(cfg).await.unwrap();
        let mut link = node.attach().await;
        match link.from_node.recv().await.unwrap() {
            NodeToMaster::Hello { .. } => {}
            other => panic!("expected hello, got {other:?}"),
        }
        let mut rig = Self {
            dir,
            ws,
            _node: node,
            link,
            seen: vec![],
            sid: SessionId::new(),
            n: 0,
        };
        rig.send(MasterToNode::Create {
            profile: None,
            incognito: false,
            agent: None,
            req_id: "c".into(),
            session_id: rig.sid,
            title: "t".into(),
            cwd: rig.ws.display().to_string(),
            model: None,
            approval_mode: Some(harness_core::policy::ApprovalMode::Auto),
        });
        rig.until(|k| matches!(k, EventKind::SessionCreated(_)))
            .await;
        rig
    }

    fn send(&self, m: MasterToNode) {
        self.link.to_node.try_send(m).unwrap();
    }

    async fn recv(&mut self) -> NodeToMaster {
        tokio::time::timeout(Duration::from_secs(30), self.link.from_node.recv())
            .await
            .expect("timed out")
            .expect("link closed")
    }

    async fn until(&mut self, pred: impl Fn(&EventKind) -> bool) {
        loop {
            match self.recv().await {
                NodeToMaster::Event { event } => {
                    let k = event.typed().unwrap();
                    self.seen.push(event);
                    if pred(&k) {
                        return;
                    }
                }
                NodeToMaster::Ack { ok, error, .. } => assert!(ok, "nack: {error:?}"),
                _ => {}
            }
        }
    }

    fn checkpoints_since(&self, mark: usize) -> Vec<(String, String)> {
        self.seen[mark..]
            .iter()
            .filter_map(|e| match e.typed() {
                Ok(EventKind::Checkpoint(c)) => Some((
                    c.turn_id.clone(),
                    serde_json::to_value(c.phase)
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_owned(),
                )),
                _ => None,
            })
            .collect()
    }

    /// One agent turn running `cmd` in the folder; waits for the turn to end.
    async fn turn(&mut self, cmd: &str) -> Turn {
        self.n += 1;
        let mark = self.seen.len();
        self.send(MasterToNode::Prompt {
            savings: None,
            context: None,
            reply: None,
            agent: None,
            attachments: vec![],
            compare: vec![],
            req_id: format!("p{}", self.n),
            session_id: self.sid,
            text: format!("run: {cmd}"),
            images: vec![],
        });
        self.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
        let cps = self.checkpoints_since(mark);
        let id = cps
            .iter()
            .find(|(_, p)| p == "after")
            .map(|(t, _)| t.clone());
        Turn {
            id,
            phases: cps.into_iter().map(|(_, p)| p).collect(),
        }
    }

    /// Undo `turn`; returns what the caller sees and the folder around it.
    async fn restore(&mut self, turn: &str, force: bool) -> Restore {
        self.n += 1;
        let req = format!("r{}", self.n);
        let mark = self.seen.len();
        let folder_before = self.files();
        self.send(MasterToNode::Restore {
            req_id: req.clone(),
            session_id: self.sid,
            turn_id: turn.into(),
            force,
            rewind: false,
        });
        let result = loop {
            match self.recv().await {
                NodeToMaster::Event { event } => self.seen.push(event),
                NodeToMaster::Ack {
                    req_id, ok, error, ..
                } if req_id == req => {
                    break if ok {
                        Ok(())
                    } else {
                        Err(error.unwrap_or_default())
                    };
                }
                _ => {}
            }
        };
        let cps = self.checkpoints_since(mark);
        Restore {
            result,
            saved: cps
                .iter()
                .find(|(_, p)| p == "before_restore")
                .map(|(t, _)| t.clone()),
            phases: cps.into_iter().map(|(_, p)| p).collect(),
            folder_before,
            folder_after: self.files(),
        }
    }

    /// Every file in the folder but `.git`, with its bytes.
    fn files(&self) -> BTreeMap<String, Vec<u8>> {
        walkdir::WalkDir::new(&self.ws)
            .into_iter()
            .filter_entry(|e| e.file_name() != ".git")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| {
                (
                    e.path()
                        .strip_prefix(&self.ws)
                        .unwrap()
                        .display()
                        .to_string(),
                    std::fs::read(e.path()).unwrap(),
                )
            })
            .collect()
    }

    fn write(&self, name: &str, text: &str) {
        std::fs::write(self.ws.join(name), text).unwrap();
    }

    fn read(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.ws.join(name)).ok()
    }

    // ---- git, with a fixed identity and no global or system config ----

    fn git_cmd(&self, dir: &Path) -> Command {
        let mut c = Command::new("git");
        c.arg("-C")
            .arg(dir)
            .args([
                "-c",
                "commit.gpgSign=false",
                "-c",
                "init.defaultBranch=main",
            ])
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap())
            .env("HOME", self.dir.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "user")
            .env("GIT_AUTHOR_EMAIL", "user@example.com")
            .env("GIT_COMMITTER_NAME", "user")
            .env("GIT_COMMITTER_EMAIL", "user@example.com");
        c
    }

    fn git_in(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.git_cmd(dir).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn git(&self, args: &[&str]) -> String {
        self.git_in(&self.ws, args)
    }

    fn git_ok(&self, args: &[&str]) -> bool {
        self.git_cmd(&self.ws)
            .args(args)
            .output()
            .unwrap()
            .status
            .success()
    }

    /// Whether the user's repository still has the object.
    fn user_has(&self, oid: &str) -> bool {
        self.git_ok(&["cat-file", "-e", oid])
    }

    /// The checkpoint repository (there is one folder, so one).
    fn shadow(&self) -> PathBuf {
        let mut v: Vec<PathBuf> = std::fs::read_dir(self.dir.path().join("checkpoints"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "git"))
            .collect();
        assert_eq!(v.len(), 1, "{v:?}");
        v.pop().unwrap()
    }

    /// Whether the checkpoint repository holds the object in its *own*
    /// object directory: a copy of it with the alternates file removed.
    fn shadow_owns(&mut self, oid: &str) -> bool {
        self.n += 1;
        let copy = self.dir.path().join(format!("shadow-copy-{}", self.n));
        let _ = std::fs::remove_dir_all(&copy);
        assert!(
            Command::new("cp")
                .arg("-a")
                .arg(self.shadow())
                .arg(&copy)
                .status()
                .unwrap()
                .success()
        );
        let alt = copy.join("objects/info/alternates");
        assert!(alt.exists(), "no alternates file: borrowing is not on");
        std::fs::remove_file(alt).unwrap();
        let ok = self
            .git_cmd(&copy)
            .args(["cat-file", "-e", oid])
            .output()
            .unwrap()
            .status
            .success();
        std::fs::remove_dir_all(copy).unwrap();
        ok
    }

    /// Refs of the checkpoint repository.
    fn shadow_refs(&self) -> Vec<String> {
        let out = self
            .git_cmd(&self.shadow())
            .args(["for-each-ref", "--format=%(refname)"])
            .output()
            .unwrap();
        String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// base commit (other.txt), then a commit holding keep.txt = X.
    /// Returns X's blob id.
    fn base_repo(&self) -> String {
        self.git(&["init", "-q", "-b", "main"]);
        self.write("other.txt", "base\n");
        self.git(&["add", "."]);
        self.git(&["commit", "-qm", "base"]);
        self.write("keep.txt", X);
        self.git(&["add", "."]);
        self.git(&["commit", "-qm", "adds keep.txt"]);
        self.git(&["hash-object", "keep.txt"])
    }

    fn expire_reflog_and_gc_now(&self) {
        self.git(&["reflog", "expire", "--expire=now", "--all"]);
        self.git(&["gc", "--prune=now", "-q"]);
    }

    /// Back-date every loose object in the user's repository.
    fn age_objects(&self, days: u32) {
        let when = format!("{days} days ago");
        assert!(
            Command::new("find")
                .arg(self.ws.join(".git/objects"))
                .args(["-type", "f", "-exec", "touch", "-d", &when, "{}", "+"])
                .status()
                .unwrap()
                .success()
        );
    }

    /// The user rewrites the commit that held keep.txt = X.
    fn amend_x_away(&self) {
        self.write("keep.txt", "Y: the amended content\n");
        self.git(&["commit", "-qa", "--amend", "-m", "amended"]);
    }
}

// ---- scenarios ----

/// The turn every scenario undoes: it overwrites `keep.txt` (committed by the
/// user with content X) with `v2`. With `dirty`, an untracked file makes the
/// folder differ from the user's last commit, so the checkpoint's root tree
/// is a new object; without it the whole tree equals the user's commit.
async fn turn_over_committed_x(rig: &mut Rig, dirty: bool) -> (String, String) {
    let x = rig.base_repo();
    if dirty {
        rig.write("scratch.txt", SCRATCH);
    }
    let t = rig.turn("echo v2 > keep.txt").await;
    assert_eq!(t.phases, ["before", "after"]);
    // Borrowing is really on, and X is only in the user's object store.
    assert!(rig.user_has(&x));
    assert!(
        !rig.shadow_owns(&x),
        "X is in the shadow's own objects, so this is not testing borrowing"
    );
    (x, t.id.unwrap())
}

/// The user's folder as the turn left it, for the unforced undo to accept.
fn the_turns_result_is_back(rig: &Rig) {
    rig.write("keep.txt", "v2\n");
}

fn x_bytes() -> Vec<u8> {
    X.as_bytes().to_vec()
}

/// The plain refusal: nothing changed, and why (content the folder's
/// repository no longer has), naming `file` when one is given.
fn refused_for_lost_content(e: &str, file: Option<&str>) -> bool {
    e.starts_with("nothing was changed:")
        && e.contains("the git repository in this folder no longer has")
        && file.is_none_or(|f| e.contains(&format!("`{f}`")))
}

/// Refused with the plain reason; same bytes in every file, no checkpoint
/// event.
fn assert_refused(r: &Restore, file: Option<&str>) {
    let e = r.result.clone().unwrap_err();
    assert!(refused_for_lost_content(&e, file), "{e}");
    assert_eq!(r.folder_after, r.folder_before, "the folder changed");
    assert!(r.phases.is_empty(), "logged: {:?}", r.phases);
}

/// [`assert_refused`], and no pre-restore checkpoint ref left behind.
fn assert_refused_untouched(rig: &Rig, r: &Restore, file: Option<&str>) {
    assert_refused(r, file);
    assert!(
        !rig.shadow_refs().iter().any(|r| r.contains("/restore-")),
        "a pre-restore ref was left: {:?}",
        rig.shadow_refs()
    );
}

/// Amend + expire the reflog + `gc --prune=now`, then the turn's result is
/// put back in the folder; the undo of the turn is attempted.
async fn main_scenario(dirty: bool) -> (Rig, String, Restore) {
    let mut rig = Rig::new().await;
    let (x, turn) = turn_over_committed_x(&mut rig, dirty).await;
    rig.amend_x_away();
    rig.expire_reflog_and_gc_now();
    assert!(
        !rig.user_has(&x),
        "gc did not remove X from the user's repository"
    );
    the_turns_result_is_back(&rig);
    let r = rig.restore(&turn, false).await;
    (rig, turn, r)
}

// ---- the main scenario ----

/// ADOPTION_PLAN B.2, final review D3, fixed by A.16 as far as it can be
/// while checkpoints borrow. A checkpoint borrows the blobs the user's
/// repository already has (`objects/info/alternates`), so a commit the user
/// later amends away and `git gc --prune=now` removes takes the checkpoint's
/// content with it. The undo of the turn is refused before anything is
/// touched or logged, in plain words, naming the file.
///
/// Residual (O11): once checkpoints own the blobs they need, the assertion
/// becomes: the restore is `Ok` and `keep.txt` holds X.
#[tokio::test]
async fn gc_in_the_users_repository_makes_an_older_checkpoint_refused_with_the_reason() {
    let (rig, _turn, r) = main_scenario(true).await;
    assert_refused_untouched(&rig, &r, Some("keep.txt"));
    let e = r.result.unwrap_err();
    assert!(e.contains("rewritten"), "says why: {e}");
}

/// Was the most serious form of the gap: `git checkout-index -f` removed the
/// file before it found it could not write the old content, so the user's
/// `keep.txt` (the turn's result `v2`) was deleted. Now every blob is
/// checked before any file is touched: the folder is byte for byte what it
/// was.
#[tokio::test]
async fn a_restore_that_needs_a_missing_blob_changes_nothing() {
    let (rig, _turn, r) = main_scenario(true).await;
    assert_refused_untouched(&rig, &r, Some("keep.txt"));
    assert_eq!(r.folder_after.get("keep.txt"), Some(&b"v2\n".to_vec()));
    assert_eq!(rig.read("keep.txt").as_deref(), Some("v2\n"));
    assert_eq!(
        r.folder_after.get("scratch.txt"),
        Some(&SCRATCH.as_bytes().to_vec())
    );
}

/// Was: with two files in the turn, the one whose blob survived was restored
/// and the other deleted (a half-restored folder). Now all or nothing:
/// neither file changes.
#[tokio::test]
async fn a_restore_with_one_lost_blob_changes_neither_file() {
    let mut rig = Rig::new().await;
    let x = rig.base_repo();
    // Never committed, so the checkpoint repository stores its blob itself.
    rig.write("a_ok.txt", "A: original, only in the folder\n");
    let t = rig.turn("echo a2 > a_ok.txt; echo v2 > keep.txt").await;
    assert!(!rig.shadow_owns(&x));
    rig.amend_x_away();
    rig.expire_reflog_and_gc_now();
    the_turns_result_is_back(&rig);
    rig.write("a_ok.txt", "a2\n");
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_refused_untouched(&rig, &r, Some("keep.txt"));
    assert_eq!(r.folder_after.get("a_ok.txt"), Some(&b"a2\n".to_vec()));
    assert_eq!(r.folder_after.get("keep.txt"), Some(&b"v2\n".to_vec()));
}

/// Was: the error named a pre-restore checkpoint that never got its `after`
/// and could not be restored ("incomplete"). Now a refused restore writes no
/// checkpoint at all: no event, no ref. (A failure after the pre-restore
/// checkpoint is logged is covered by
/// `a_failure_after_the_pre_restore_checkpoint_names_it_and_it_restores`.)
#[tokio::test]
async fn a_refused_restore_saves_no_checkpoint() {
    let (rig, _turn, r) = main_scenario(true).await;
    assert_refused_untouched(&rig, &r, None);
    assert_eq!(r.saved, None);
    assert!(!r.result.unwrap_err().contains("saved as checkpoint"));
    // Only the turn's two refs.
    assert_eq!(rig.shadow_refs().len(), 2, "{:?}", rig.shadow_refs());
}

/// The other face. If the folder was clean when the checkpoint was taken,
/// the checkpoint's *tree* equals a tree the user's commit already has, so
/// even the tree is borrowed and gc removes it. That too is refused with the
/// plain reason (before, it was git's "unable to read tree").
#[tokio::test]
async fn a_checkpoint_whose_borrowed_tree_is_gone_is_refused_with_the_reason() {
    let (rig, _turn, r) = main_scenario(false).await;
    assert_refused_untouched(&rig, &r, None);
    // The pre-restore ref was never made; only the turn's two remain.
    assert_eq!(rig.shadow_refs().len(), 2);
}

/// Was: a file the folder still has, unchanged, whose blob is gone (the user
/// ran `git reset HEAD~1` and gc: the file stays, the commit and its blob go)
/// stayed in the checkpoint repository's index, `write-tree` refused, and
/// every later turn ran with no checkpoint (only a warning in the node's
/// log) and every restore stopped at its pre-restore snapshot.
///
/// Now the index is rebuilt from the folder once: the next turn has its
/// checkpoint and its undo works. The older checkpoint that depends on the
/// lost content is refused with the plain reason, and is not deleted.
#[tokio::test]
async fn a_lost_blob_of_an_unchanged_file_is_repaired_at_the_next_snapshot() {
    let mut rig = Rig::new().await;
    let x = rig.base_repo();
    // Git treats a file written in the same second as the index as possibly
    // changed and rehashes it (which would store the blob in the checkpoint
    // repository); wait so the file is settled, as in any real session.
    tokio::time::sleep(Duration::from_millis(2100)).await;
    let first = rig.turn("echo ok > made.txt").await;
    assert_eq!(first.phases, ["before", "after"]);
    assert!(!rig.shadow_owns(&x));
    rig.git(&["reset", "-q", "HEAD~1"]);
    rig.expire_reflog_and_gc_now();
    assert!(!rig.user_has(&x));
    assert_eq!(
        rig.read("keep.txt").as_deref(),
        Some(X),
        "the file is still there"
    );
    let refs_before = rig.shadow_refs();

    let next = rig.turn("echo more > more.txt").await;
    assert_eq!(
        next.phases,
        ["before", "after"],
        "the turn has a checkpoint"
    );
    assert_eq!(rig.read("more.txt").as_deref(), Some("more\n"));
    // No checkpoint was deleted by the repair.
    for r in &refs_before {
        assert!(rig.shadow_refs().contains(r), "{r} was deleted");
    }

    // The repaired turn undoes.
    let r = rig.restore(next.id.as_deref().unwrap(), false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("more.txt"), None);
    assert_eq!(r.folder_after.get("keep.txt"), Some(&x_bytes()));

    // The older one that needs what is gone: refused, untouched, kept, and
    // the refusal made no ref.
    let refs_now = rig.shadow_refs();
    let r = rig.restore(first.id.as_deref().unwrap(), true).await;
    assert_refused(&r, None);
    assert_eq!(rig.shadow_refs(), refs_now);
    for r in &refs_before {
        assert!(rig.shadow_refs().contains(r), "{r} was deleted");
    }
}

/// A blob the checkpoint only borrows, still present in the user's
/// repository: the restore works, and afterwards the blob is in the
/// checkpoint repository's own objects (it was copied before the write).
/// (That the copy writes nothing in the user's repository is tested in
/// `git.rs`, `securing_blobs_copies_them_and_writes_nothing_in_the_users_repository`.)
#[tokio::test]
async fn a_borrowed_blob_is_copied_into_the_checkpoint_repository_by_a_restore() {
    let mut rig = Rig::new().await;
    let (x, turn) = turn_over_committed_x(&mut rig, true).await;
    the_turns_result_is_back(&rig);
    let r = rig.restore(&turn, false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("keep.txt"), Some(&x_bytes()));
    assert!(rig.shadow_owns(&x), "X was not copied into the shadow");
}

/// Whether this process can write into a directory it made read-only (it
/// can as root, where these permission tests prove nothing).
fn read_only_is_enforced(dir: &Path) -> bool {
    std::fs::write(dir.join(".probe"), "").is_err()
}

fn set_mode(p: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// A real failure after the pre-restore checkpoint is logged: a folder the
/// restore must write into is read-only, so `a.txt` is restored and
/// `ro/b.txt` cannot be. The error names the pre-restore checkpoint, and
/// restoring that checkpoint through the normal call (unforced) brings the
/// folder back to what it was before the failed restore.
#[tokio::test]
async fn a_failure_after_the_pre_restore_checkpoint_names_it_and_it_restores() {
    let mut rig = Rig::new().await;
    std::fs::create_dir(rig.ws.join("ro")).unwrap();
    rig.write("a.txt", "a1\n");
    rig.write("ro/b.txt", "b1\n");
    let t = rig.turn("echo a2 > a.txt; echo b2 > ro/b.txt").await;
    assert_eq!(t.phases, ["before", "after"]);
    set_mode(&rig.ws.join("ro"), 0o555);
    if !read_only_is_enforced(&rig.ws.join("ro")) {
        set_mode(&rig.ws.join("ro"), 0o755);
        eprintln!("SKIPPED: read-only folders are writable here (root?)");
        return;
    }
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    set_mode(&rig.ws.join("ro"), 0o755);
    let e = r.result.clone().unwrap_err();
    let saved = r.saved.clone().expect("a before_restore checkpoint");
    assert!(e.contains(&format!("saved as checkpoint {saved}")), "{e}");
    assert_eq!(r.phases, ["before_restore"]);
    // Half restored: the residual the error is there for.
    assert_eq!(r.folder_after.get("a.txt"), Some(&b"a1\n".to_vec()));
    assert_eq!(r.folder_after.get("ro/b.txt"), Some(&b"b2\n".to_vec()));

    let mark = rig.seen.len();
    let back = rig.restore(&saved, false).await;
    assert_eq!(back.result, Ok(()));
    assert_eq!(back.folder_after, r.folder_before, "not back to before");
    assert_eq!(back.phases, ["before_restore", "after", "restored"]);
    // Restoring an interrupted restore does not check for edits made since;
    // the chat names the checkpoint that holds what it overwrote.
    let safety = back.saved.clone().unwrap();
    let notes = notes_since(&rig, mark);
    assert!(
        notes.iter().any(|n| n.contains(&safety)),
        "no note names {safety}: {notes:?}"
    );
    // And the turn itself can still be undone once the folder is writable.
    let again = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_eq!(again.result, Ok(()));
    assert_eq!(again.folder_after.get("a.txt"), Some(&b"a1\n".to_vec()));
    assert_eq!(again.folder_after.get("ro/b.txt"), Some(&b"b1\n".to_vec()));
}

/// When a turn runs without a checkpoint (here the checkpoint repository's
/// objects cannot be written, and the one repair does not help), the person
/// is told in the chat, not only the node's log.
#[tokio::test]
async fn a_turn_without_a_checkpoint_says_so_in_the_chat() {
    let mut rig = Rig::new().await;
    rig.write("a.txt", "a1\n");
    let first = rig.turn("echo a2 > a.txt").await;
    assert_eq!(first.phases, ["before", "after"]);
    let objects = rig.shadow().join("objects");
    let dirs: Vec<PathBuf> = walkdir::WalkDir::new(&objects)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir())
        .map(|e| e.path().to_path_buf())
        .collect();
    for d in &dirs {
        set_mode(d, 0o555);
    }
    let enforced = read_only_is_enforced(&objects);
    let mark = rig.seen.len();
    let t = if enforced {
        Some(rig.turn("echo new > new.txt").await)
    } else {
        None
    };
    for d in &dirs {
        set_mode(d, 0o755);
    }
    let Some(t) = t else {
        eprintln!("SKIPPED: read-only folders are writable here (root?)");
        return;
    };
    assert!(t.phases.is_empty(), "{:?}", t.phases);
    assert_eq!(
        rig.read("new.txt").as_deref(),
        Some("new\n"),
        "the turn ran"
    );
    let told = rig.seen[mark..].iter().any(|e| {
        matches!(e.typed(), Ok(EventKind::Error(m))
            if m.message.starts_with("This turn has no checkpoint"))
    });
    assert!(told, "nothing in the chat says the turn has no undo");
}

/// The window between the check and the write: the user's repository loses
/// X after the restore checked it and before it writes (a stand-in `git`
/// first on `PATH` runs `gc --prune=now` in the user's repository when the
/// restore's first writing step, `checkout-index`, starts). The restore
/// still writes X, because it was copied into the checkpoint repository
/// first. Runs in a child process: this one's `PATH` cannot be changed
/// without `unsafe`.
#[test]
fn a_blob_removed_between_the_check_and_the_write_is_still_restored() {
    if let Some(bin) = std::env::var_os(WINDOW_CHILD) {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(window_scenario(PathBuf::from(bin)));
        return;
    }
    let path = std::env::var("PATH").unwrap_or_default();
    let real = std::env::split_paths(&path)
        .map(|d| d.join("git"))
        .find(|g| g.is_file())
        .expect("git is needed for this test");
    let bin = tempfile::tempdir().unwrap();
    let b = bin.path().display();
    let wrapper = bin.path().join("git");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\n\
             for a in \"$@\"; do\n\
             if [ \"$a\" = checkout-index ] && [ -f '{b}/ws' ] && [ ! -e '{b}/fired' ]; then\n\
             : > '{b}/fired'\n\
             env -u GIT_INDEX_FILE -u GIT_ATTR_SOURCE GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
             '{r}' -C \"$(cat '{b}/ws')\" gc --prune=now -q || exit 99\n\
             fi\n\
             done\n\
             exec '{r}' \"$@\"\n",
            r = real.display()
        ),
    )
    .unwrap();
    set_mode(&wrapper, 0o755);
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "a_blob_removed_between_the_check_and_the_write_is_still_restored",
            "--exact",
            "--nocapture",
        ])
        .env(WINDOW_CHILD, bin.path())
        .env(
            "PATH",
            std::env::join_paths(
                std::iter::once(bin.path().to_path_buf()).chain(std::env::split_paths(&path)),
            )
            .unwrap(),
        )
        .status()
        .unwrap();
    assert!(status.success(), "the scenario failed: {status}");
}

const WINDOW_CHILD: &str = "HARNESS_TEST_A16_WINDOW_BIN";

async fn window_scenario(bin: PathBuf) {
    let mut rig = Rig::new().await;
    let (x, turn) = turn_over_committed_x(&mut rig, true).await;
    rig.amend_x_away();
    rig.git(&["reflog", "expire", "--expire=now", "--all"]);
    assert!(rig.user_has(&x), "unreachable, but present at the check");
    the_turns_result_is_back(&rig);
    std::fs::write(bin.join("ws"), rig.ws.to_str().unwrap()).unwrap();
    let r = rig.restore(&turn, false).await;
    assert!(bin.join("fired").exists(), "the seam did not run");
    assert!(!rig.user_has(&x), "X was not removed in the window");
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("keep.txt"), Some(&x_bytes()));
}

// ---- controls ----

/// The checks the experiment rests on, made once on their own: the folder is
/// a repository, the shadow borrows (alternates point at the user's object
/// directory), X is not in the shadow's own objects, and the same check does
/// find an object the shadow did write (`v2`), so it can say yes.
#[tokio::test]
async fn control_the_blob_is_borrowed_and_the_check_can_tell() {
    let mut rig = Rig::new().await;
    let (x, _turn) = turn_over_committed_x(&mut rig, true).await;
    let alternates = std::fs::read_to_string(rig.shadow().join("objects/info/alternates")).unwrap();
    assert_eq!(
        alternates.trim_end(),
        rig.ws.join(".git/objects").to_str().unwrap()
    );
    assert!(rig.user_has(&x));
    assert!(!rig.shadow_owns(&x));
    let v2 = rig.git(&["hash-object", "keep.txt"]);
    assert!(
        rig.shadow_owns(&v2),
        "the shadow stores what the user's repository lacks"
    );
}

/// Control: not a repository, nothing to borrow, the same turn and undo.
#[tokio::test]
async fn control_a_folder_that_is_not_a_repository_restores_x() {
    let mut rig = Rig::new().await;
    rig.write("keep.txt", X);
    rig.write("scratch.txt", SCRATCH);
    let t = rig.turn("echo v2 > keep.txt").await;
    assert!(!rig.ws.join(".git").exists());
    assert!(
        !rig.shadow().join("objects/info/alternates").exists(),
        "no borrowing in a plain folder"
    );
    the_turns_result_is_back(&rig);
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("keep.txt"), Some(&x_bytes()));
}

/// Control: the same rewrite of history, without the gc. X is still in the
/// user's store (unreachable but present), so the undo works.
#[tokio::test]
async fn control_without_the_gc_step_the_restore_succeeds() {
    let mut rig = Rig::new().await;
    let (x, turn) = turn_over_committed_x(&mut rig, true).await;
    rig.amend_x_away();
    rig.git(&["reflog", "expire", "--expire=now", "--all"]);
    assert!(rig.user_has(&x), "unreachable, but not yet pruned");
    the_turns_result_is_back(&rig);
    let r = rig.restore(&turn, false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.phases, ["before_restore", "after", "restored"]);
    assert_eq!(r.folder_after.get("keep.txt"), Some(&x_bytes()));
}

// ---- variants ----

/// The object was only ever staged (`git add`, then `git reset`): it lives
/// in the user's store as a dangling loose object, the checkpoint borrowed
/// it, gc removes it. Refused with the reason; the file is left as it was
/// (before A.16 it was deleted).
#[tokio::test]
async fn a_checkpoint_of_a_file_that_was_only_ever_staged_is_refused_after_gc() {
    let mut rig = Rig::new().await;
    rig.base_repo();
    rig.write("staged.txt", "S: only ever staged\n");
    rig.git(&["add", "staged.txt"]);
    let s = rig.git(&["hash-object", "staged.txt"]);
    rig.git(&["reset", "-q"]);
    let t = rig.turn("echo v2 > staged.txt").await;
    assert!(rig.user_has(&s));
    assert!(
        !rig.shadow_owns(&s),
        "the checkpoint borrowed the staged blob"
    );
    rig.expire_reflog_and_gc_now();
    assert!(!rig.user_has(&s));
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_refused_untouched(&rig, &r, Some("staged.txt"));
    assert_eq!(r.folder_after.get("staged.txt"), Some(&b"v2\n".to_vec()));
}

/// The work was on a branch the user then deleted (`branch -D`): refused
/// with the reason, nothing changed.
#[tokio::test]
async fn a_checkpoint_is_refused_after_the_branch_that_held_x_is_deleted_and_gc() {
    let mut rig = Rig::new().await;
    rig.git(&["init", "-q", "-b", "main"]);
    rig.write("other.txt", "base\n");
    rig.git(&["add", "."]);
    rig.git(&["commit", "-qm", "base"]);
    rig.git(&["checkout", "-qb", "feature"]);
    rig.write("keep.txt", X);
    rig.git(&["add", "."]);
    rig.git(&["commit", "-qm", "feature work"]);
    let x = rig.git(&["hash-object", "keep.txt"]);
    rig.write("scratch.txt", SCRATCH);
    let t = rig.turn("echo v2 > keep.txt").await;
    assert!(!rig.shadow_owns(&x));
    rig.git(&["checkout", "-qf", "main"]);
    rig.git(&["branch", "-D", "feature"]);
    rig.expire_reflog_and_gc_now();
    assert!(!rig.user_has(&x));
    the_turns_result_is_back(&rig);
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_refused_untouched(&rig, &r, Some("keep.txt"));
}

/// `git gc --auto` with default settings does nothing in a repository this
/// small (it waits for about 6,700 loose objects), so nothing is lost. In
/// practice: auto gc only bites repositories that are large or old, and then
/// only after the reflog and the grace period (next two tests).
#[tokio::test]
async fn gc_auto_with_default_settings_does_not_remove_anything_in_a_small_repository() {
    let mut rig = Rig::new().await;
    let (x, turn) = turn_over_committed_x(&mut rig, true).await;
    rig.amend_x_away();
    rig.git(&["gc", "--auto", "-q"]);
    assert!(rig.user_has(&x));
    the_turns_result_is_back(&rig);
    let r = rig.restore(&turn, false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("keep.txt"), Some(&x_bytes()));
}

/// A plain `git gc` right after the rewrite keeps X twice over: the reflog
/// still names the old commit, and an unreachable object younger than two
/// weeks is not pruned. The undo works.
#[tokio::test]
async fn default_gc_right_after_the_rewrite_keeps_x() {
    let mut rig = Rig::new().await;
    let (x, turn) = turn_over_committed_x(&mut rig, true).await;
    rig.amend_x_away();
    rig.git(&["gc", "-q"]);
    assert!(rig.user_has(&x));
    the_turns_result_is_back(&rig);
    let r = rig.restore(&turn, false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("keep.txt"), Some(&x_bytes()));
}

/// The slow version of the main scenario: a plain `git gc` with
/// default settings once the reflog entry has expired (default 30 days for
/// an unreachable commit) and the object is older than the two-week grace
/// period. Time is simulated: the reflog is expired by hand and the loose
/// objects are back-dated 20 days (the test cannot wait). What it shows: a
/// user who never runs `--prune=now` loses the object too, about a month
/// after the rewrite, and checkpoints are kept for as many turns, not days
/// (`checkpoints_keep`, default 10), so an old chat's undo can be that old.
/// Refused with the reason, nothing changed.
#[tokio::test]
async fn default_gc_after_the_reflog_and_the_grace_period_makes_the_checkpoint_refused() {
    let mut rig = Rig::new().await;
    let (x, turn) = turn_over_committed_x(&mut rig, true).await;
    rig.amend_x_away();
    rig.git(&["reflog", "expire", "--expire=now", "--all"]);
    rig.age_objects(20);
    rig.git(&["gc", "-q"]);
    assert!(!rig.user_has(&x));
    the_turns_result_is_back(&rig);
    let r = rig.restore(&turn, false).await;
    assert_refused_untouched(&rig, &r, Some("keep.txt"));
}

/// `git repack -ad` drops unreachable objects that sit in a pack, but it
/// counts the reflog as a root: with the reflog intact the object stays.
#[tokio::test]
async fn repack_ad_keeps_x_while_the_reflog_names_it() {
    let mut rig = Rig::new().await;
    let x = rig.base_repo();
    rig.git(&["gc", "-q"]); // X is in a pack, as in any older repository
    rig.write("scratch.txt", SCRATCH);
    let t = rig.turn("echo v2 > keep.txt").await;
    assert!(!rig.shadow_owns(&x));
    rig.amend_x_away();
    rig.git(&["repack", "-ad", "-q"]);
    assert!(rig.user_has(&x));
    the_turns_result_is_back(&rig);
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_eq!(r.result, Ok(()));
}

/// Plain `git repack -ad` after the reflog is expired removes the packed
/// object (no `gc`, no `--prune`): refused with the reason.
#[tokio::test]
async fn repack_ad_after_the_reflog_is_expired_makes_the_checkpoint_refused() {
    let mut rig = Rig::new().await;
    let x = rig.base_repo();
    rig.git(&["gc", "-q"]);
    rig.write("scratch.txt", SCRATCH);
    let t = rig.turn("echo v2 > keep.txt").await;
    assert!(!rig.shadow_owns(&x));
    rig.amend_x_away();
    rig.git(&["reflog", "expire", "--expire=now", "--all"]);
    rig.git(&["repack", "-ad", "-q"]);
    assert!(!rig.user_has(&x));
    the_turns_result_is_back(&rig);
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_refused_untouched(&rig, &r, Some("keep.txt"));
}

/// The user deletes `.git` and clones again into the same folder. The shadow
/// is keyed by the folder, so it is reused and its alternates now name the
/// new clone's object directory. The commit that held X was never pushed, so
/// the new clone lacks it: the undo is refused with the reason, nothing
/// changed.
#[tokio::test]
async fn re_cloning_the_repository_makes_a_checkpoint_of_unpushed_work_refused() {
    let (rig, r) = reclone_scenario(false).await;
    assert!(!rig.user_has_x);
    assert_refused(&r, Some("keep.txt"));
}

/// Control for the previous test: the work had been pushed, the new clone has
/// the object (content-addressed, so same id), and the undo works.
#[tokio::test]
async fn re_cloning_a_repository_that_has_the_pushed_work_keeps_the_checkpoint() {
    let (rig, r) = reclone_scenario(true).await;
    assert!(rig.user_has_x);
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("keep.txt"), Some(&x_bytes()));
}

struct Cloned {
    user_has_x: bool,
}

async fn reclone_scenario(pushed: bool) -> (Cloned, Restore) {
    let mut rig = Rig::new().await;
    let x = rig.base_repo();
    let origin = rig.dir.path().join("origin.git");
    rig.git_in(rig.dir.path(), &["init", "-q", "--bare", "origin.git"]);
    let o = origin.display().to_string();
    if pushed {
        rig.git(&["push", "-q", &o, "main"]);
    } else {
        // Only the base commit; the commit with keep.txt stays local.
        rig.git(&["push", "-q", &o, "HEAD~1:refs/heads/main"]);
    }
    rig.write("scratch.txt", SCRATCH);
    let t = rig.turn("echo v2 > keep.txt").await;
    assert!(!rig.shadow_owns(&x));
    std::fs::remove_dir_all(rig.ws.join(".git")).unwrap();
    rig.git_in(rig.dir.path(), &["clone", "-q", &o, "reclone"]);
    std::fs::rename(rig.dir.path().join("reclone/.git"), rig.ws.join(".git")).unwrap();
    let user_has_x = rig.user_has(&x);
    the_turns_result_is_back(&rig);
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    (Cloned { user_has_x }, r)
}

// ---- the write path (A.16 review) ----

/// The chat notes (`error` events) seen since `mark`.
fn notes_since(rig: &Rig, mark: usize) -> Vec<String> {
    rig.seen[mark..]
        .iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::Error(m)) => Some(m.message),
            _ => None,
        })
        .collect()
}

/// Undo of a turn that replaced a directory by a file of the same name:
/// the file goes, the directory and its file come back.
#[tokio::test]
async fn undo_of_a_directory_replaced_by_a_file_puts_the_directory_back() {
    let mut rig = Rig::new().await;
    std::fs::create_dir(rig.ws.join("d")).unwrap();
    rig.write("d/f", "f1\n");
    let t = rig.turn("rm -r d; echo x > d").await;
    assert_eq!(t.phases, ["before", "after"]);
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_eq!(r.result, Ok(()), "{:?}", r.phases);
    assert_eq!(r.phases, ["before_restore", "after", "restored"]);
    assert_eq!(r.folder_after.get("d/f"), Some(&b"f1\n".to_vec()));
}

/// Undo of a turn that replaced a file by a directory of the same name;
/// then, if it failed, the named pre-restore checkpoint.
#[tokio::test]
async fn undo_of_a_file_replaced_by_a_directory_puts_the_file_back() {
    let mut rig = Rig::new().await;
    rig.write("d", "d1\n");
    let t = rig.turn("rm d; mkdir d; echo y > d/f").await;
    assert_eq!(t.phases, ["before", "after"]);
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    if let Err(e) = &r.result {
        eprintln!("undo failed: {e}; folder now {:?}", r.folder_after.keys());
        let saved = r.saved.clone().unwrap();
        let back = rig.restore(&saved, false).await;
        eprintln!(
            "restore of {saved}: {:?}; folder now {:?}",
            back.result,
            back.folder_after.keys()
        );
    }
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.phases, ["before_restore", "after", "restored"]);
    assert_eq!(r.folder_after.get("d"), Some(&b"d1\n".to_vec()));
}

/// The same two swaps two levels deep: file `a` against `a/b/c`.
#[tokio::test]
async fn undo_of_swaps_two_levels_deep_works_both_ways() {
    let mut rig = Rig::new().await;
    rig.write("a", "a1\n");
    let t = rig.turn("rm a; mkdir -p a/b; echo c > a/b/c").await;
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("a"), Some(&b"a1\n".to_vec()));
    assert_eq!(r.folder_after.len(), 1, "{:?}", r.folder_after.keys());

    let mut rig = Rig::new().await;
    std::fs::create_dir_all(rig.ws.join("a/b")).unwrap();
    rig.write("a/b/c", "c1\n");
    let t = rig.turn("rm -r a; echo a > a").await;
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("a/b/c"), Some(&b"c1\n".to_vec()));
    assert_eq!(r.folder_after.len(), 1, "{:?}", r.folder_after.keys());
}

/// A file the turn turned into a directory, where the user's build then
/// wrote an ignored file: the undo would replace the directory by the
/// file and delete what no checkpoint holds. Unforced, it is refused,
/// naming the directory, and nothing changes.
#[tokio::test]
async fn an_unforced_undo_refuses_to_delete_files_no_checkpoint_holds() {
    let mut rig = Rig::new().await;
    rig.write(".gitignore", "*.o\n");
    rig.write("d", "d1\n");
    let t = rig.turn("rm d; mkdir d; echo y > d/f").await;
    assert_eq!(t.phases, ["before", "after"]);
    rig.write("d/cache.o", "build output\n");
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    let e = r.result.clone().unwrap_err();
    assert!(
        e.starts_with("nothing was changed:") && e.contains("`d`") && e.contains("no checkpoint"),
        "{e}"
    );
    assert_eq!(r.folder_after, r.folder_before);
    assert!(r.phases.is_empty(), "{:?}", r.phases);
    assert_eq!(
        r.folder_after.get("d/cache.o"),
        Some(&b"build output\n".to_vec()),
        "an ignored file was deleted by an unforced undo"
    );
}

/// The forced case: the undo proceeds, and the chat says which files were
/// deleted without being saved.
#[tokio::test]
async fn a_forced_undo_says_which_unsaved_files_it_deleted() {
    let mut rig = Rig::new().await;
    rig.write(".gitignore", "*.o\n");
    rig.write("d", "d1\n");
    let t = rig.turn("rm d; mkdir d; echo y > d/f").await;
    rig.write("d/cache.o", "build output\n");
    let mark = rig.seen.len();
    let r = rig.restore(t.id.as_deref().unwrap(), true).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("d"), Some(&b"d1\n".to_vec()));
    let notes = notes_since(&rig, mark);
    assert!(
        notes
            .iter()
            .any(|n| n.contains("`d/cache.o`") && n.contains("not saved")),
        "{notes:?}"
    );
}

/// A blob of a path the restore skips (inside a repository that appeared
/// in the folder after the turn) is neither needed nor checked: its loss
/// does not refuse the restore.
#[tokio::test]
async fn a_lost_blob_under_a_skipped_nested_repository_does_not_refuse_the_restore() {
    let mut rig = Rig::new().await;
    rig.git(&["init", "-q", "-b", "main"]);
    rig.write("other.txt", "base\n");
    rig.git(&["add", "."]);
    rig.git(&["commit", "-qm", "base"]);
    std::fs::create_dir(rig.ws.join("sub")).unwrap();
    rig.write("sub/keep.txt", X);
    rig.git(&["add", "."]);
    rig.git(&["commit", "-qm", "adds sub/keep.txt"]);
    let x = rig.git(&["hash-object", "sub/keep.txt"]);
    // Untracked, so the checkpoint's `sub` tree is its own (only the blob
    // is borrowed; a borrowed tree that is gone is refused at the diff).
    rig.write("sub/extra.txt", SCRATCH);
    let t = rig.turn("rm -r sub").await;
    assert_eq!(t.phases, ["before", "after"]);
    assert!(!rig.shadow_owns(&x));
    rig.git(&["commit", "-qa", "--amend", "--allow-empty", "-m", "amended"]);
    rig.expire_reflog_and_gc_now();
    assert!(!rig.user_has(&x));
    // A repository appears where the turn removed the files.
    std::fs::create_dir(rig.ws.join("sub")).unwrap();
    rig.git_in(&rig.ws.join("sub"), &["init", "-q"]);
    rig.write("sub/mine.txt", "the other repository's work\n");
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("sub/keep.txt"), None, "written into it");
    assert_eq!(
        r.folder_after.get("sub/mine.txt"),
        Some(&b"the other repository's work\n".to_vec())
    );
}

/// Modes, symlinks, an empty file and a new file in a new directory.
#[tokio::test]
async fn undo_restores_modes_symlinks_and_empty_files() {
    use std::os::unix::fs::PermissionsExt;
    let mut rig = Rig::new().await;
    rig.write("run.sh", "#!/bin/sh\n");
    set_mode(&rig.ws.join("run.sh"), 0o755);
    rig.write("a.txt", "a\n");
    std::os::unix::fs::symlink("a.txt", rig.ws.join("link")).unwrap();
    std::os::unix::fs::symlink("nowhere", rig.ws.join("dangling")).unwrap();
    rig.write("empty", "");
    let t = rig
        .turn("chmod 644 run.sh; rm link; echo z > link; rm dangling; rm empty; mkdir -p 'new dir' && echo n > 'new dir/x'")
        .await;
    assert_eq!(t.phases, ["before", "after"]);
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_eq!(r.result, Ok(()));
    let m = std::fs::metadata(rig.ws.join("run.sh"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(m & 0o111, 0o111, "exec bit lost: {m:o}");
    assert_eq!(
        std::fs::read_link(rig.ws.join("link")).unwrap(),
        Path::new("a.txt")
    );
    assert_eq!(
        std::fs::read_link(rig.ws.join("dangling")).unwrap(),
        Path::new("nowhere")
    );
    assert_eq!(rig.read("empty").as_deref(), Some(""));
    assert!(!rig.ws.join("new dir/x").exists());
}
