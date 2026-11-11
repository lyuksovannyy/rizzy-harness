//! Experiment B.2 (ADOPTION_PLAN): what the user's own git housekeeping does
//! to checkpoints that borrow objects from the user's repository.
//!
//! A checkpoint repository in a git folder borrows the user's objects
//! (`objects/info/alternates`, `crates/node/src/git.rs`), so it stores only
//! what the user's repository lacks. These tests assert what happens today
//! when the user's own housekeeping (amend, `gc --prune=now`, `repack -ad`,
//! deleting a branch, re-cloning) removes objects a checkpoint refers to.
//! Tests named for a loss are KNOWN GAPS: they pass while the gap exists and
//! their doc comment says what the assertion becomes once it is fixed.
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

fn lost_blob(e: &str, file: &str) -> bool {
    e.contains(&format!("unable to read sha1 file of {file}"))
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

/// KNOWN GAP (ADOPTION_PLAN B.2, final review D3). A checkpoint borrows the
/// blobs the user's repository already has (`objects/info/alternates`), so a
/// commit the user later amends away and `git gc --prune=now` removes takes
/// the checkpoint's content with it. The undo of the turn fails with a raw
/// git error.
///
/// This feeds the decision whether checkpoints stop borrowing and copy the
/// blobs they need, and a rewrite of FIX_PLAN 2.6's stated residual (it says
/// nothing about this). Once checkpoints own the blobs they need, the
/// assertion becomes: the restore is `Ok` and `keep.txt` holds X.
#[tokio::test]
async fn gc_in_the_users_repository_breaks_an_older_checkpoint() {
    let (_rig, _turn, r) = main_scenario(true).await;
    let e = r.result.clone().unwrap_err();
    assert!(lost_blob(&e, "keep.txt"), "{e}");
    // It is the checkpoint that is broken, not the folder's state: nothing
    // came back with content X.
    assert_ne!(r.folder_after.get("keep.txt"), Some(&x_bytes()));
}

/// KNOWN GAP, and the most serious form of it. When the blob is gone the
/// restore does not leave the folder untouched: `git checkout-index -f`
/// removes the file before it finds it cannot write the old content, so the
/// user's `keep.txt` (which held the turn's result `v2`) is deleted. The
/// pre-restore snapshot still holds it (see the next test), but the user is
/// left with a missing file and an error.
///
/// Fixed means: a restore that cannot complete changes nothing (read every
/// blob first, or write to temporary names and rename). The assertion
/// becomes `folder_after == folder_before`.
#[tokio::test]
async fn a_restore_that_hits_a_missing_blob_deletes_the_file_it_could_not_write() {
    let (_rig, _turn, r) = main_scenario(true).await;
    assert!(r.result.is_err());
    assert_eq!(r.folder_before.get("keep.txt"), Some(&b"v2\n".to_vec()));
    assert_eq!(
        r.folder_after.get("keep.txt"),
        None,
        "the folder was left half-restored: the file is gone"
    );
    // Files the restore had no business with are as they were.
    assert_eq!(
        r.folder_after.get("scratch.txt"),
        r.folder_before.get("scratch.txt")
    );
    assert_eq!(
        r.folder_after.get("other.txt"),
        r.folder_before.get("other.txt")
    );
}

/// KNOWN GAP. With two files in the turn, the one whose blob survived is
/// restored and the one whose blob is gone is deleted: a half-restored
/// folder, reported to the caller only as the error of the second file.
///
/// Fixed means all or nothing (`folder_after == folder_before`).
#[tokio::test]
async fn a_restore_with_one_lost_blob_is_half_restored() {
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
    assert!(lost_blob(r.result.as_ref().unwrap_err(), "keep.txt"));
    assert_eq!(
        r.folder_after.get("a_ok.txt"),
        Some(&b"A: original, only in the folder\n".to_vec()),
        "a_ok.txt was restored"
    );
    assert_eq!(r.folder_after.get("keep.txt"), None, "keep.txt was deleted");
}

/// What the caller sees: the error is git's own text, and it names the
/// checkpoint that holds the folder as it was. That recovery does not work
/// through the node: the saved checkpoint never got its `after`, so undoing
/// it answers "incomplete". The content is still in the checkpoint
/// repository (read here with git directly).
///
/// Fixed means the named checkpoint can be restored (or, better, the error
/// never happens).
#[tokio::test]
async fn the_checkpoint_saved_before_a_failed_restore_cannot_be_restored() {
    let (mut rig, _turn, r) = main_scenario(true).await;
    let e = r.result.clone().unwrap_err();
    let saved = r
        .saved
        .clone()
        .expect("a before_restore checkpoint was logged");
    assert!(e.contains(&format!("saved as checkpoint {saved}")), "{e}");
    assert_eq!(r.phases, ["before_restore"], "no `after`, no `restored`");
    let again = rig.restore(&saved, true).await;
    assert_eq!(
        again.result,
        Err("this turn's checkpoint is incomplete".to_string())
    );
    // The user's v2 is still in the checkpoint repository.
    let reff = rig
        .shadow_refs()
        .into_iter()
        .find(|r| r.ends_with(&format!("{saved}/before")))
        .unwrap();
    let shown = rig
        .git_cmd(&rig.shadow())
        .args(["show", &format!("{reff}:keep.txt")])
        .output()
        .unwrap();
    assert_eq!(shown.stdout, b"v2\n");
}

/// KNOWN GAP, other face. If the folder was clean when the checkpoint was
/// taken, the checkpoint's *tree* equals a tree the user's commit already
/// has, so even the tree is borrowed and the failure comes earlier, in the
/// diff, with no `before_restore` event; the folder is left alone.
///
/// Fixed means the restore succeeds with X.
#[tokio::test]
async fn gc_also_removes_the_trees_a_clean_folder_checkpoint_borrowed() {
    let (rig, _turn, r) = main_scenario(false).await;
    let e = r.result.clone().unwrap_err();
    assert!(e.starts_with("git diff: fatal: unable to read tree"), "{e}");
    assert!(r.phases.is_empty(), "{:?}", r.phases);
    assert_eq!(r.folder_after, r.folder_before, "the folder is untouched");
    // The pre-restore ref was cleaned up; only the turn's two remain.
    assert_eq!(rig.shadow_refs().len(), 2);
}

/// KNOWN GAP, and wider than one undo. A file the folder still has,
/// unchanged, whose blob is gone (the user ran `git reset HEAD~1` and gc:
/// the file stays, the commit and its blob go) stays in the checkpoint
/// repository's index. From then on `write-tree` refuses, so every later
/// turn runs with no checkpoint at all (only a warning in the node's log),
/// and every restore, of any turn, stops at its pre-restore snapshot.
/// Editing that file heals it.
///
/// Fixed means snapshots do not depend on the user's store: the turn gets
/// its checkpoint and the restore works.
#[tokio::test]
async fn a_lost_blob_of_an_unchanged_file_stops_every_later_snapshot() {
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

    let next = rig.turn("echo more > more.txt").await;
    assert!(
        next.phases.is_empty(),
        "no checkpoint at all: {:?}",
        next.phases
    );
    assert_eq!(
        rig.read("more.txt").as_deref(),
        Some("more\n"),
        "the turn itself ran"
    );

    let r = rig.restore(first.id.as_deref().unwrap(), true).await;
    let e = r.result.unwrap_err();
    assert!(
        e.starts_with("nothing was restored: the folder could not be saved first")
            && e.contains("invalid object")
            && e.contains("keep.txt"),
        "{e}"
    );
    assert_eq!(r.folder_after, r.folder_before);
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

/// KNOWN GAP. The object was only ever staged (`git add`, then
/// `git reset`): it lives in the user's store as a dangling loose object,
/// the checkpoint borrowed it, gc removes it.
#[tokio::test]
async fn gc_breaks_a_checkpoint_of_a_file_that_was_only_ever_staged() {
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
    assert!(lost_blob(r.result.as_ref().unwrap_err(), "staged.txt"));
    assert_eq!(
        r.folder_after.get("staged.txt"),
        None,
        "and the file is gone"
    );
}

/// KNOWN GAP. The work was on a branch the user then deleted (`branch -D`).
#[tokio::test]
async fn gc_breaks_a_checkpoint_after_the_branch_that_held_x_is_deleted() {
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
    assert!(lost_blob(r.result.as_ref().unwrap_err(), "keep.txt"));
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

/// KNOWN GAP, the slow version of the main scenario: a plain `git gc` with
/// default settings once the reflog entry has expired (default 30 days for
/// an unreachable commit) and the object is older than the two-week grace
/// period. Time is simulated: the reflog is expired by hand and the loose
/// objects are back-dated 20 days (the test cannot wait). What it shows: a
/// user who never runs `--prune=now` loses the object too, about a month
/// after the rewrite, and checkpoints are kept for as many turns, not days
/// (`checkpoints_keep`, default 10), so an old chat's undo can be that old.
#[tokio::test]
async fn default_gc_after_the_reflog_and_the_grace_period_breaks_the_checkpoint() {
    let mut rig = Rig::new().await;
    let (x, turn) = turn_over_committed_x(&mut rig, true).await;
    rig.amend_x_away();
    rig.git(&["reflog", "expire", "--expire=now", "--all"]);
    rig.age_objects(20);
    rig.git(&["gc", "-q"]);
    assert!(!rig.user_has(&x));
    the_turns_result_is_back(&rig);
    let r = rig.restore(&turn, false).await;
    assert!(lost_blob(r.result.as_ref().unwrap_err(), "keep.txt"));
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

/// KNOWN GAP. Plain `git repack -ad` after the reflog is expired removes the
/// packed object (no `gc`, no `--prune`).
#[tokio::test]
async fn repack_ad_after_the_reflog_is_expired_breaks_the_checkpoint() {
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
    assert!(lost_blob(r.result.as_ref().unwrap_err(), "keep.txt"));
}

/// The user deletes `.git` and clones again into the same folder. The shadow
/// is keyed by the folder, so it is reused and its alternates now name the
/// new clone's object directory. The commit that held X was never pushed, so
/// the new clone lacks it and the undo fails. (KNOWN GAP.)
#[tokio::test]
async fn re_cloning_the_repository_breaks_a_checkpoint_of_unpushed_work() {
    let (rig, r) = reclone_scenario(false).await;
    assert!(!rig.user_has_x);
    assert!(lost_blob(r.result.as_ref().unwrap_err(), "keep.txt"));
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
