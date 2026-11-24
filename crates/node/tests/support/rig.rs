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
use harness_protocol::{MasterToNode, NodeToMaster, PromptMsg};
use harness_store::{Durability, Store};

pub struct Rig {
    /// Every model request of every session of the node, in order.
    pub requests: Requests,
    /// The lead agent's caps in [`Rig::lead_turn`] (`None`: the defaults).
    pub lead_caps: Option<harness_core::agent::DelegateCaps>,
    /// More sub-agents the lead may delegate to, or the rig's own with
    /// other tools (a later one of the same id replaces the rig's).
    pub delegates: Vec<harness_core::agent::AgentSpec>,
    /// The master's context [`Rig::lead_turn`] sends (`None`: none).
    pub lead_context: Option<harness_core::backend::MasterContext>,
    /// The stored skills the lead's spec carries (X.34 step B) and the
    /// lead's own `deny_skills`.
    pub lead_stored: Vec<harness_core::skills::StoredSkillEntry>,
    pub lead_deny_skills: Vec<String>,
    /// The lead's own tools in [`Rig::lead_turn`] (`None`: `bash`, `write`).
    pub lead_tools: Option<Vec<&'static str>>,
    pub dir: tempfile::TempDir,
    pub ws: PathBuf,
    _node: Option<NodeRuntime>,
    policy: NodePolicy,
    link: NodeLink,
    pub seen: Vec<RawEvent>,
    /// What the node's hello announced.
    pub caps: Vec<String>,
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

/// The model requests a [`Recording`] provider saw.
pub type Requests = Arc<std::sync::Mutex<Vec<harness_agent_native::provider::ChatRequest>>>;

/// The scripted agent, keeping every request it is sent.
pub struct Recording {
    inner: Mock,
    seen: Requests,
}

#[async_trait::async_trait]
impl harness_agent_native::provider::Provider for Recording {
    fn name(&self) -> &'static str {
        "mock"
    }

    async fn complete(
        &self,
        req: &harness_agent_native::provider::ChatRequest,
        deltas: tokio::sync::mpsc::UnboundedSender<String>,
    ) -> Result<
        harness_agent_native::provider::Completion,
        harness_agent_native::provider::ProviderError,
    > {
        self.seen.lock().unwrap().push(req.clone());
        self.inner.complete(req, deltas).await
    }
}

/// A secret the rig's node holds: it must never come out in a diff.
pub const RIG_SECRET: &str = "sk-rig-secret-0123456789abcdef";

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
        Self::build(keep, NativeConfig::default(), |_| {}).await
    }

    /// A rig whose node's policy is changed by `f` (after the rig's own).
    pub async fn with_policy(f: impl FnOnce(&mut NodePolicy)) -> Self {
        Self::build(
            NodePolicy::default().checkpoints_keep,
            NativeConfig::default(),
            f,
        )
        .await
    }

    /// A rig whose agents summarise their history past `tokens`.
    pub async fn summarizing(tokens: usize) -> Self {
        Self::build(
            NodePolicy::default().checkpoints_keep,
            NativeConfig {
                summarize_after_tokens: Some(tokens),
                ..Default::default()
            },
            |_| {},
        )
        .await
    }

    async fn build(keep: usize, native: NativeConfig, tweak: impl FnOnce(&mut NodePolicy)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir(&ws).unwrap();
        let store = Store::open(&dir.path().join("node.db"), Durability::Full).unwrap();
        let requests: Requests = Arc::default();
        let mut policy = NodePolicy {
            workspace_roots: vec![dir.path().into()],
            max_approval_mode: harness_core::policy::ApprovalMode::Auto,
            checkpoints_keep: keep,
            // Never the developer's own skills (`~/.agents/skills`):
            // they would be in every turn's context.
            skills: harness_node::SkillsPolicy {
                roots: vec![],
                ..Default::default()
            },
            ..Default::default()
        };
        tweak(&mut policy);
        let kept_policy = policy.clone();
        let cfg = NodeConfig {
            node_id: "local".into(),
            hostname: "test".into(),
            policy,
            store,
            backend: Arc::new(NativeBackend::new(
                Arc::new(Recording {
                    inner: Mock {
                        delay: Duration::from_millis(5),
                    },
                    seen: requests.clone(),
                }),
                harness_tools::builtin(),
                native,
            )),
            default_model: "mock".into(),
            secrets: harness_core::SecretSet::new(vec![harness_core::Secret::new(
                RIG_SECRET.to_string(),
            )]),
            profile: harness_core::event::Profile::Agentic,
            incognito_ttl: None,
            checkpoints_dir: Some(dir.path().join("checkpoints")),
            git_sandbox: None,
            artifacts: None,
        };
        let node = NodeRuntime::start(cfg).await.unwrap();
        let mut link = node.attach().await;
        let caps = match link.from_node.recv().await.unwrap() {
            NodeToMaster::Hello { capabilities, .. } => capabilities,
            other => panic!("expected hello, got {other:?}"),
        };
        let mut rig = Self {
            requests,
            lead_caps: None,
            delegates: vec![],
            lead_context: None,
            lead_stored: vec![],
            lead_deny_skills: vec![],
            lead_tools: None,
            dir,
            ws,
            _node: Some(node),
            policy: kept_policy,
            link,
            seen: vec![],
            caps,
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

    /// Stop the node and start another on the same data directory (its
    /// logs, checkpoints and sessions), as a restart does; the sessions are
    /// loaded again from their logs on first use.
    pub async fn restart(&mut self) {
        drop(self._node.take());
        tokio::time::sleep(Duration::from_millis(300)).await;
        let store = Store::open(&self.dir.path().join("node.db"), Durability::Full).unwrap();
        let node = NodeRuntime::start(NodeConfig {
            node_id: "local".into(),
            hostname: "test".into(),
            policy: self.policy.clone(),
            store,
            backend: Arc::new(NativeBackend::new(
                Arc::new(Recording {
                    inner: Mock {
                        delay: Duration::from_millis(5),
                    },
                    seen: self.requests.clone(),
                }),
                harness_tools::builtin(),
                NativeConfig::default(),
            )),
            default_model: "mock".into(),
            secrets: harness_core::SecretSet::new(vec![]),
            profile: harness_core::event::Profile::Agentic,
            incognito_ttl: None,
            checkpoints_dir: Some(self.dir.path().join("checkpoints")),
            git_sandbox: None,
            artifacts: None,
        })
        .await
        .unwrap();
        let mut link = node.attach().await;
        match link.from_node.recv().await.unwrap() {
            NodeToMaster::Hello { .. } => {}
            other => panic!("expected hello, got {other:?}"),
        }
        self.link = link;
        self._node = Some(node);
    }

    /// The node's request places: background output reads, `@` completion.
    pub fn places(&self) -> (Arc<tokio::sync::Semaphore>, Arc<tokio::sync::Semaphore>) {
        self._node
            .as_ref()
            .expect("a running node")
            .request_places()
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
        self.send(MasterToNode::Prompt(PromptMsg {
            req_id: format!("p{}", self.n),
            ..PromptMsg::test(self.sid, format!("run: {cmd}"))
        }));
    }

    /// One turn of a lead agent that may delegate to `dev` (a `bash` and
    /// `write` sub-agent: one that changes files, so its shell writes), `look` (reads only: `read`, `grep`) and `editor` (`read`
    /// and `edit`), running the scripted `text` (`delegate: dev=... | ...`);
    /// waits for the lead's turn to end and returns it with the outputs of
    /// its `delegate` calls.
    pub async fn lead_turn(&mut self, text: &str) -> (Turn, Vec<String>) {
        self.lead_turn_in(self.sid, text).await
    }

    /// An agent spec of the rig: `id` with `tools` and `delegates`; its
    /// system prompt names it (`You are @id.`).
    pub fn spec(
        id: &str,
        tools: &[&str],
        delegates: Vec<harness_core::agent::AgentSpec>,
    ) -> harness_core::agent::AgentSpec {
        harness_core::agent::AgentSpec {
            agent_id: id.into(),
            name: id.into(),
            system_prompt: format!("You are @{id}."),
            tools: harness_core::agent::ToolSelection::List(
                tools.iter().map(|t| t.to_string()).collect(),
            ),
            model: None,
            fallbacks: vec![],
            unavailable: vec![],
            no_effort: vec![],
            vision: vec![],
            context_windows: Default::default(),
            effort: None,
            ui: Default::default(),
            no_personalisation: false,
            prompt_hash: "h".into(),
            deny_tools: vec![],
            deny_skills: vec![],
            deny_mcp_servers: vec![],
            stored_skills: vec![],
            connector_tools: vec![],
            delegates,
            caps: None,
        }
    }

    /// The lead agent [`Rig::lead_turn`] runs.
    pub fn lead(&self) -> harness_core::agent::AgentSpec {
        let mut lead = Self::spec(
            "lead",
            self.lead_tools.as_deref().unwrap_or(&["bash", "write"]),
            vec![
                Self::spec("dev", &["bash", "write"], vec![]),
                Self::spec("look", &["read", "grep"], vec![]),
                Self::spec("editor", &["read", "edit"], vec![]),
            ],
        );
        lead.caps = self.lead_caps;
        lead.stored_skills = self.lead_stored.clone();
        lead.deny_skills = self.lead_deny_skills.clone();
        for d in &self.delegates {
            lead.delegates.retain(|x| x.agent_id != d.agent_id);
            lead.delegates.push(d.clone());
        }
        lead
    }

    /// One agent turn running `cmd` in the chat `sid` (another chat of the
    /// rig); waits for that chat's turn to end.
    pub async fn turn_in(&mut self, sid: SessionId, cmd: &str) {
        self.n += 1;
        self.send(MasterToNode::Prompt(PromptMsg {
            req_id: format!("p{}", self.n),
            ..PromptMsg::test(sid, format!("run: {cmd}"))
        }));
        loop {
            match self.recv().await {
                NodeToMaster::Event { event } => {
                    let ended = event.session_id == sid
                        && matches!(event.typed(), Ok(EventKind::TurnEnded(_)));
                    self.seen.push(event);
                    if ended {
                        return;
                    }
                }
                NodeToMaster::Ack { ok, error, .. } => assert!(ok, "nack: {error:?}"),
                _ => {}
            }
        }
    }

    /// A fork of `parent` at the end of `turn`: the new chat.
    pub async fn fork(&mut self, parent: SessionId, turn: &str) -> SessionId {
        let child = SessionId::new();
        self.n += 1;
        let req = format!("f{}", self.n);
        self.send(MasterToNode::Fork {
            req_id: req.clone(),
            session_id: child,
            parent,
            turn_id: turn.into(),
            title: "t (fork)".into(),
        });
        loop {
            match self.recv().await {
                NodeToMaster::Event { event } => self.seen.push(event),
                NodeToMaster::Ack {
                    req_id, ok, error, ..
                } if req_id == req => {
                    assert!(ok, "fork refused: {error:?}");
                    return child;
                }
                _ => {}
            }
        }
    }

    /// Another agentic chat on the node, in the rig's folder, in Auto.
    pub async fn new_chat(&mut self) -> SessionId {
        let sid = SessionId::new();
        self.send(MasterToNode::Create {
            exec_mode: None,
            no_callbacks: false,
            profile: None,
            incognito: false,
            agent: None,
            req_id: format!("c-{sid}"),
            session_id: sid,
            title: "t".into(),
            cwd: self.ws.display().to_string(),
            model: None,
            approval_mode: Some(harness_core::policy::ApprovalMode::Auto),
        });
        self.until(|k| matches!(k, EventKind::SessionCreated(_)))
            .await;
        sid
    }

    /// [`Rig::lead_turn`] in the chat `sid`.
    pub async fn lead_turn_in(&mut self, sid: SessionId, text: &str) -> (Turn, Vec<String>) {
        let mark = self.seen.len();
        self.n += 1;
        self.send(MasterToNode::Prompt(PromptMsg {
            context: self.lead_context.clone().map(Box::new),
            agent: Some(self.lead()),
            req_id: format!("p{}", self.n),
            ..PromptMsg::test(sid, text)
        }));
        loop {
            self.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
            if self.seen.last().is_some_and(|e| e.session_id == sid) {
                break;
            }
        }
        let mine: Vec<EventKind> = self.seen[mark..]
            .iter()
            .filter(|e| e.session_id == sid)
            .filter_map(|e| e.typed().ok())
            .collect();
        let calls: Vec<String> = mine
            .iter()
            .filter_map(|k| match k {
                EventKind::ToolCallStarted(c) if c.tool == "delegate" => Some(c.call_id.clone()),
                _ => None,
            })
            .collect();
        let outs = mine
            .iter()
            .filter_map(|k| match k {
                EventKind::ToolCallFinished(f) if calls.contains(&f.call_id) => {
                    Some(f.output.clone())
                }
                _ => None,
            })
            .collect();
        let id = mine.iter().find_map(|k| match k {
            EventKind::Checkpoint(c) if c.phase == CheckpointPhase::After => {
                Some(c.turn_id.clone())
            }
            _ => None,
        });
        (Turn { id, phases: vec![] }, outs)
    }

    /// Undo `turn`; returns what the caller sees and the folder around it.
    pub async fn restore(&mut self, turn: &str, force: bool) -> Restore {
        self.restore_with(turn, force, false).await
    }

    /// Undo `turn`, or with `rewind` it and every later turn.
    pub async fn restore_with(&mut self, turn: &str, force: bool, rewind: bool) -> Restore {
        self.restore_of(self.sid, turn, force, rewind).await
    }

    /// [`Rig::restore_with`] in the chat `sid` (another chat of the rig,
    /// or a sub-agent's).
    pub async fn restore_of(
        &mut self,
        sid: SessionId,
        turn: &str,
        force: bool,
        rewind: bool,
    ) -> Restore {
        self.n += 1;
        let req = format!("r{}", self.n);
        let mark = self.seen.len();
        let folder_before = self.files();
        self.send(MasterToNode::Restore {
            req_id: req.clone(),
            session_id: sid,
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

    /// What `turn` changed (X.12), or with `rewind` it and every later
    /// turn; the node's words when there is nothing to show.
    pub async fn diff(
        &mut self,
        turn: &str,
        rewind: bool,
    ) -> Result<harness_protocol::TurnDiff, String> {
        self.diff_of(self.sid, turn, rewind).await
    }

    /// [`Rig::diff`] of a turn of the chat `sid`.
    pub async fn diff_of(
        &mut self,
        sid: SessionId,
        turn: &str,
        rewind: bool,
    ) -> Result<harness_protocol::TurnDiff, String> {
        self.n += 1;
        let req = format!("d{}", self.n);
        self.send(MasterToNode::Diff {
            req_id: req.clone(),
            session_id: sid,
            turn_id: turn.into(),
            rewind,
            all: false,
        });
        self.diff_answer(req).await
    }

    /// Everything the chat changed (X.61): the node's whole-chat diff.
    pub async fn diff_all(&mut self) -> Result<harness_protocol::TurnDiff, String> {
        self.diff_all_of(self.sid).await
    }

    /// [`Rig::diff_all`] of the chat `sid`.
    pub async fn diff_all_of(
        &mut self,
        sid: SessionId,
    ) -> Result<harness_protocol::TurnDiff, String> {
        self.n += 1;
        let req = format!("d{}", self.n);
        self.send(MasterToNode::Diff {
            req_id: req.clone(),
            session_id: sid,
            turn_id: String::new(),
            rewind: false,
            all: true,
        });
        self.diff_answer(req).await
    }

    async fn diff_answer(&mut self, req: String) -> Result<harness_protocol::TurnDiff, String> {
        loop {
            match self.recv().await {
                NodeToMaster::Event { event } => self.seen.push(event),
                NodeToMaster::DiffResult {
                    req_id,
                    diff,
                    error,
                } if req_id == req => {
                    break match (diff, error) {
                        (Some(d), None) => Ok(*d),
                        (_, e) => Err(e.unwrap_or_default()),
                    };
                }
                _ => {}
            }
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

    /// The `error` events seen since `mark` that are notices (information
    /// for the person, not a failure), by message.
    pub fn notices_since(&self, mark: usize) -> Vec<String> {
        self.seen[mark..]
            .iter()
            .filter_map(|e| match e.typed() {
                Ok(EventKind::Error(m)) if m.notice => Some(m.message),
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
            // Nor the global ignore file (`~/.config/git/ignore`).
            .env("XDG_CONFIG_HOME", "/nonexistent")
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
