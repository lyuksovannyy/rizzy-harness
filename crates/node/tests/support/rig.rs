//! A node with one agentic session in a temporary folder, driven by the
//! scripted agent (`run: <cmd>` is one `bash` call), and the git helpers the
//! checkpoint tests share. Every git call here runs with a fixed identity, no
//! system or global config, and a cleared environment.

#![allow(dead_code)]

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

pub struct Rig {
    pub dir: tempfile::TempDir,
    pub ws: PathBuf,
    _node: NodeRuntime,
    link: NodeLink,
    pub seen: Vec<RawEvent>,
    sid: SessionId,
    pub n: u32,
}

#[derive(Debug)]
pub struct Turn {
    pub id: Option<String>,
    pub phases: Vec<String>,
}

#[derive(Debug)]
pub struct Restore {
    pub result: Result<(), String>,
    pub phases: Vec<String>,
    /// The id of the "folder as it was before this restore" checkpoint.
    pub saved: Option<String>,
    pub folder_before: BTreeMap<String, Vec<u8>>,
    pub folder_after: BTreeMap<String, Vec<u8>>,
}

/// The environment a scripted agent's shell command puts in front of its
/// own git calls (the agent's shell inherits the node's environment).
pub const AGENT_GIT: &str = "export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 \
     GIT_AUTHOR_NAME=agent GIT_AUTHOR_EMAIL=agent@example.com \
     GIT_COMMITTER_NAME=agent GIT_COMMITTER_EMAIL=agent@example.com";

impl Rig {
    pub async fn new() -> Self {
        Self::with_keep(NodePolicy::default().checkpoints_keep).await
    }

    /// A rig whose node keeps `keep` checkpoints.
    pub async fn with_keep(keep: usize) -> Self {
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
                checkpoints_keep: keep,
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
            git_sandbox: None,
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
            exec_mode: None,
            no_callbacks: false,
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

    /// The rig's one session.
    pub fn session_id(&self) -> SessionId {
        self.sid
    }

    pub fn send(&self, m: MasterToNode) {
        self.link.to_node.try_send(m).unwrap();
    }

    pub async fn recv(&mut self) -> NodeToMaster {
        tokio::time::timeout(Duration::from_secs(60), self.link.from_node.recv())
            .await
            .expect("timed out")
            .expect("link closed")
    }

    pub async fn until(&mut self, pred: impl Fn(&EventKind) -> bool) {
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

    pub fn checkpoints_since(&self, mark: usize) -> Vec<(String, String)> {
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
    pub async fn turn(&mut self, cmd: &str) -> Turn {
        let mark = self.seen.len();
        self.start_turn(cmd);
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

    /// Send the prompt of one agent turn running `cmd`; does not wait.
    pub fn start_turn(&mut self, cmd: &str) {
        self.n += 1;
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
            agent_named: false,
        });
    }

    /// Undo `turn`; returns what the caller sees and the folder around it.
    pub async fn restore(&mut self, turn: &str, force: bool) -> Restore {
        self.restore_with(turn, force, false).await
    }

    /// Undo `turn`, or with `rewind` it and every later turn.
    pub async fn restore_with(&mut self, turn: &str, force: bool, rewind: bool) -> Restore {
        self.n += 1;
        let req = format!("r{}", self.n);
        let mark = self.seen.len();
        let folder_before = self.files();
        self.send(MasterToNode::Restore {
            req_id: req.clone(),
            session_id: self.sid,
            turn_id: turn.into(),
            force,
            rewind,
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

    /// The chat notes (`error` events) seen since `mark`.
    pub fn notes_since(&self, mark: usize) -> Vec<String> {
        self.seen[mark..]
            .iter()
            .filter_map(|e| match e.typed() {
                Ok(EventKind::Error(m)) => Some(m.message),
                _ => None,
            })
            .collect()
    }

    /// Every file in the folder but `.git`, with its bytes.
    pub fn files(&self) -> BTreeMap<String, Vec<u8>> {
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

    pub fn write(&self, name: &str, text: &str) {
        std::fs::write(self.ws.join(name), text).unwrap();
    }

    pub fn read(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.ws.join(name)).ok()
    }

    // ---- git, with a fixed identity and no global or system config ----

    pub fn git_cmd(&self, dir: &Path) -> Command {
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

    pub fn git_in(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.git_cmd(dir).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    pub fn git(&self, args: &[&str]) -> String {
        self.git_in(&self.ws, args)
    }

    pub fn git_ok(&self, args: &[&str]) -> bool {
        self.git_cmd(&self.ws)
            .args(args)
            .output()
            .unwrap()
            .status
            .success()
    }

    /// Whether the user's repository still has the object.
    pub fn user_has(&self, oid: &str) -> bool {
        self.git_ok(&["cat-file", "-e", oid])
    }

    /// The checkpoint repository (there is one folder, so one).
    pub fn shadow(&self) -> PathBuf {
        let mut v: Vec<PathBuf> = std::fs::read_dir(self.dir.path().join("checkpoints"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "git"))
            .collect();
        assert_eq!(v.len(), 1, "{v:?}");
        v.pop().unwrap()
    }

    /// A copy of the checkpoint repository with its alternates file removed
    /// (only its own objects), removed again when `f` returns.
    fn with_own_copy<T>(&mut self, f: impl FnOnce(&Self, &Path) -> T) -> T {
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
        let out = f(self, &copy);
        std::fs::remove_dir_all(copy).unwrap();
        out
    }

    /// Whether the checkpoint repository holds the object in its *own*
    /// object directory: a copy of it with the alternates file removed.
    pub fn shadow_owns(&mut self, oid: &str) -> bool {
        self.with_own_copy(|rig, copy| {
            rig.git_cmd(copy)
                .args(["cat-file", "-e", oid])
                .output()
                .unwrap()
                .status
                .success()
        })
    }

    /// The blobs in the checkpoint repository's own object directory.
    pub fn shadow_own_blobs(&mut self) -> std::collections::BTreeSet<String> {
        self.with_own_copy(|rig, copy| {
            let out = rig.git_in(
                copy,
                &[
                    "cat-file",
                    "--batch-all-objects",
                    "--batch-check=%(objectname) %(objecttype)",
                ],
            );
            out.lines()
                .filter_map(|l| l.strip_suffix(" blob"))
                .map(str::to_string)
                .collect()
        })
    }

    /// Bytes under the checkpoint repository's `objects` directory.
    pub fn shadow_object_bytes(&self) -> u64 {
        walkdir::WalkDir::new(self.shadow().join("objects"))
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
            .sum()
    }

    /// The `.keep` files in the checkpoint repository's pack directory.
    pub fn shadow_keeps(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.shadow().join("objects/pack"))
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|e| e == "keep"))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Refs of the checkpoint repository.
    pub fn shadow_refs(&self) -> Vec<String> {
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
    pub fn base_repo(&self) -> String {
        self.git(&["init", "-q", "-b", "main"]);
        self.write("other.txt", "base\n");
        self.git(&["add", "."]);
        self.git(&["commit", "-qm", "base"]);
        self.write("keep.txt", X);
        self.git(&["add", "."]);
        self.git(&["commit", "-qm", "adds keep.txt"]);
        self.git(&["hash-object", "keep.txt"])
    }

    pub fn expire_reflog_and_gc_now(&self) {
        self.git(&["reflog", "expire", "--expire=now", "--all"]);
        self.git(&["gc", "--prune=now", "-q"]);
    }

    /// Back-date every loose object in the user's repository.
    pub fn age_objects(&self, days: u32) {
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
    pub fn amend_x_away(&self) {
        self.write("keep.txt", "Y: the amended content\n");
        self.git(&["commit", "-qa", "--amend", "-m", "amended"]);
    }

    /// The user throws all history away: one new root commit of the folder
    /// as it is, every older commit unreachable, reflogs expired, pruned.
    pub fn replace_history_and_prune(&self) {
        self.git(&["checkout", "-q", "--orphan", "fresh"]);
        self.git(&["add", "-A"]);
        self.git(&["commit", "-qm", "fresh start"]);
        self.git(&["branch", "-qD", "main"]);
        self.git(&["branch", "-qm", "main"]);
        self.expire_reflog_and_gc_now();
    }
}

pub const X: &str = "X: the content a checkpoint depends on\n";
