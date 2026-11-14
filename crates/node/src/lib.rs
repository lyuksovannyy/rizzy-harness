//! Node runtime: owns sessions, runs agent turns, is the single writer of each
//! session's event log, and talks to the master over a `NodeLink`.

pub mod artifacts;
mod audit;
pub mod exec;
mod git;
mod images;
mod instructions;
mod outbox;
pub mod policy;
mod session;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::SessionId;
use harness_core::backend::AgentBackend;
use harness_protocol::{MasterToNode, NODE_STOPPING, NodeToMaster, PROTOCOL_VERSION, SessionHead};
use harness_store::Store;
use tokio::sync::mpsc;

pub use git::SNAPSHOT_TIMEOUT;
pub use outbox::Backlog;
pub use policy::NodePolicy;
use session::Session;

/// Commands that start new work, refused while the node stops: a session,
/// a turn, a restore, a staged file.
fn starts_work(msg: &MasterToNode) -> bool {
    matches!(
        msg,
        MasterToNode::Create { .. }
            | MasterToNode::Prompt { .. }
            | MasterToNode::Callback { .. }
            | MasterToNode::Restore { .. }
            | MasterToNode::ArtifactStage { .. }
    )
}

/// The fields of a `session.create` command.
struct CreateArgs {
    title: String,
    cwd: String,
    model: Option<String>,
    approval_mode: Option<harness_core::policy::ApprovalMode>,
    profile: Option<harness_core::event::Profile>,
    agent: Option<harness_core::agent::AgentSpec>,
    incognito: bool,
    no_callbacks: bool,
    /// A request only: [`exec::ExecOffers::choose`] decides.
    exec_mode: Option<harness_core::policy::ExecMode>,
}

/// The longest a delete waits for its session's background commands: the
/// guard's grace and bound, its sweep and the pipes' drain.
const BACKGROUND_STOP_WAIT: Duration =
    harness_tools::contain::STOP_BOUND.saturating_add(Duration::from_secs(3));

/// Max events per `session.snapshot` batch.
const SNAPSHOT_BATCH: usize = 500;
/// ...and at most this many bytes of serialized events, a sixteenth of the
/// link's frame limit, so a batch always fits in a frame, and a batch the
/// link is writing holds a Stop's ack back by at most 2 s at 256 KiB/s
/// (A.21; it was a quarter, 2 MiB). A single event larger than this still
/// goes, alone in its batch.
pub const SNAPSHOT_BYTES: usize = harness_protocol::MAX_FRAME / 16;

/// Commands from the master waiting for the command loop. Past this, the
/// link stops reading the master's socket.
const IN_QUEUE: usize = 64;

/// Sessions with a lane (a command queued or running) at once. Past this a
/// command for another session is refused at once, so a master naming
/// fresh session ids cannot make the node keep a task per id while each
/// waits for a slow store (review A21-2). A lane ends when its queue is
/// empty, and a prompt's lane when its turn has started, so a node rarely
/// has more than a few.
const MAX_LANES: usize = 256;

/// Commands that may wait for one session while its lane is busy; past
/// this the master gets a refusal at once.
const LANE_DEPTH: usize = 64;

/// The command queue of each session with a lane (see `NodeRuntime::lane`).
type Lanes = Arc<Mutex<HashMap<SessionId, mpsc::Sender<LaneCmd>>>>;

enum LaneCmd {
    Msg(Box<MasterToNode>),
    /// Cancel whatever runs by now, unacked: the tail of a cancel that was
    /// answered inline (see `NodeRuntime::attach`).
    Cancel,
}

/// The session whose lane runs `msg`, for commands that go to a lane.
fn lane_of(msg: &MasterToNode) -> Option<SessionId> {
    match msg {
        MasterToNode::Resume { session_id, .. }
        | MasterToNode::Create { session_id, .. }
        | MasterToNode::SelectVariant { session_id, .. }
        | MasterToNode::Restore { session_id, .. }
        | MasterToNode::Delete { session_id, .. }
        | MasterToNode::Prompt { session_id, .. }
        | MasterToNode::Callback { session_id, .. }
        | MasterToNode::SetAgent { session_id, .. }
        | MasterToNode::SetApprovalMode { session_id, .. }
        | MasterToNode::ArtifactStage { session_id, .. }
        | MasterToNode::ArtifactRelease { session_id, .. } => Some(*session_id),
        MasterToNode::Welcome { .. }
        | MasterToNode::Cancel { .. }
        | MasterToNode::StopBackground { .. }
        | MasterToNode::ApprovalRespond { .. }
        | MasterToNode::ArtifactFetch { .. } => None,
    }
}

pub struct NodeConfig {
    pub node_id: String,
    pub hostname: String,
    pub policy: NodePolicy,
    pub store: Store,
    pub backend: Arc<dyn AgentBackend>,
    /// Only when a create names no model: the master resolves the current
    /// default for every session it creates, so this is a last resort.
    pub default_model: String,
    /// Values scrubbed from every event payload before it is stored.
    pub secrets: harness_core::SecretSet,
    /// Which sessions this runtime hosts: `Agentic` (workspace + tools, the
    /// normal node) or `Chat` (the master's workspace-less chat runtime).
    pub profile: harness_core::event::Profile,
    /// Incognito sessions idle this long are deleted by the node itself.
    pub incognito_ttl: Option<std::time::Duration>,
    /// Shadow git repositories for checkpoints of folders without git.
    /// `None`: only folders inside a repository get checkpoints.
    pub checkpoints_dir: Option<std::path::PathBuf>,
    /// Publishing files as artifacts (worker nodes); `None` for the chat
    /// runtime, which never publishes.
    pub artifacts: Option<artifacts::StageConfig>,
    /// The Landlock sandbox the node's own git runs in when it reads a
    /// session's folder (checkpoints, sub-agents' bases and results, a
    /// published file's state; R.1 R1b-2). `None`: unconfined (`[sandbox]
    /// mode = "off"`, and tests).
    pub git_sandbox: Option<Arc<harness_tools::sandbox::Sandbox>>,
}

/// Both ends of a link as seen from the master: commands in, messages out.
/// In-process for the embedded node; bridged over WSS for remote nodes.
/// Both directions are bounded, so neither end can make the other buffer
/// without limit.
pub struct NodeLink {
    pub to_node: mpsc::Sender<MasterToNode>,
    pub from_node: mpsc::Receiver<NodeToMaster>,
}

pub(crate) struct Inner {
    pub(crate) cfg: NodeConfig,
    pub(crate) sessions: Mutex<HashMap<SessionId, Arc<Session>>>,
    /// Folders (real paths) a restore is running in: no turn starts in an
    /// overlapping folder meanwhile, and no second restore (`session.rs`).
    pub(crate) restoring: Mutex<Vec<std::path::PathBuf>>,
    /// The current link to the master, if connected.
    link: Mutex<Option<Arc<outbox::Outbox>>>,
    /// The audit lines of inline commands (`audit.rs`).
    audit_log: audit::AuditLog,
    /// Where published files are staged; `None` when this runtime does not
    /// publish (the chat runtime, or `[artifacts] enabled = false`).
    pub(crate) stages: Option<Arc<artifacts::Stages>>,
    /// Set once by [`NodeRuntime::stop`]: new work is refused from then on
    /// ([`NODE_STOPPING`]) and a cancelled turn ends `stopped`.
    stopping: AtomicBool,
    /// Background commands (T.8d): the node's count and its `[limits]`.
    pub(crate) background: Arc<harness_tools::background::Registry>,
}

impl Inner {
    pub(crate) fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    pub(crate) fn session(&self, sid: SessionId) -> Option<Arc<Session>> {
        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&sid)
            .cloned()
    }

    /// The current link's outbox, if it is still open.
    fn outbox(&self) -> Option<Arc<outbox::Outbox>> {
        let mut link = self.link.lock().unwrap_or_else(|p| p.into_inner());
        match link.as_ref() {
            Some(o) if !o.is_closed() => Some(o.clone()),
            Some(_) => {
                *link = None;
                None
            }
            None => None,
        }
    }

    /// Send in order without waiting, for replies that are not part of the
    /// log (artifact chunks). When the queue is full
    /// the reply is dropped and the master's transfer times out and asks
    /// again; events never go this way.
    pub(crate) fn publish(&self, msg: NodeToMaster) {
        if let Some(o) = self.outbox()
            && !o.try_send(msg)
        {
            tracing::warn!("link to the master is full; dropped a reply");
        }
    }

    /// Send in order, waiting while the link's queue is full: the caller
    /// (a session appending an event, a lane acking its command) pauses
    /// until the master reads, which is the back-pressure. Waiting ends
    /// when the link closes (the link gives up on a peer that reads nothing
    /// for `IDLE_LIMIT`); then the message is dropped and, for events, the
    /// next connection's hello and `resume` replay it from the store.
    pub(crate) async fn publish_wait(&self, msg: NodeToMaster) {
        if let Some(o) = self.outbox() {
            o.send(msg).await;
        }
    }

    /// Send ahead of the queued output, never waiting: the acks of the
    /// commands the command loop answers inline, and its refusals of
    /// commands no lane takes (`outbox`). Dropped when the master has more
    /// than `outbox::URGENT_QUEUE` of them unread.
    fn publish_urgent(&self, msg: NodeToMaster) {
        if let Some(o) = self.outbox()
            && !o.send_urgent(msg)
        {
            tracing::warn!("the master has not read the last acks; dropped one");
        }
    }
}

/// What the sandbox tests in `crates/cli` drive directly (they have the
/// `sandbox-exec` binary; this crate's tests do not). Not an API.
#[cfg(feature = "testing")]
pub mod testing {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use harness_tools::sandbox::Sandbox;

    type Holds = std::sync::Mutex<Vec<(harness_core::SessionId, Arc<EndHold>)>>;
    static END_HOLD: Holds = std::sync::Mutex::new(Vec::new());

    /// Holds the next background end a session records outside a turn,
    /// after its check of the turn slot and before its append (review S3-4).
    pub struct EndHold {
        held: tokio::sync::Notify,
        go: tokio::sync::Semaphore,
        is_held: std::sync::atomic::AtomicBool,
    }

    impl EndHold {
        /// Until an end waits here.
        pub async fn held(&self) {
            while !self.is_held.load(std::sync::atomic::Ordering::SeqCst) {
                self.held.notified().await;
            }
        }
        /// Let it go on.
        pub fn release(&self) {
            self.go.add_permits(1);
        }
    }

    /// Arm the hold for the next end of session `sid`.
    pub fn hold_next_end(sid: harness_core::SessionId) -> Arc<EndHold> {
        let h = Arc::new(EndHold {
            held: tokio::sync::Notify::new(),
            go: tokio::sync::Semaphore::new(0),
            is_held: Default::default(),
        });
        END_HOLD.lock().unwrap().push((sid, h.clone()));
        h
    }

    pub(crate) async fn end_hold(sid: harness_core::SessionId) {
        let h = {
            let mut all = END_HOLD.lock().unwrap();
            all.iter()
                .position(|(s, _)| *s == sid)
                .map(|i| all.remove(i).1)
        };
        if let Some(h) = h {
            h.is_held.store(true, std::sync::atomic::Ordering::SeqCst);
            h.held.notify_waiters();
            h.held.notify_one();
            if let Ok(p) = h.go.acquire().await {
                p.forget();
            }
        }
    }

    fn bounds(sandbox: Option<Arc<Sandbox>>, protected: &[PathBuf]) -> crate::git::Bounds {
        crate::git::Bounds {
            ceiling: None,
            protected: protected.to_vec(),
            sandbox,
        }
    }

    /// A checkpoint of `folder` (as a turn takes one) in the store `base`,
    /// with the node's git in `sandbox`; then the bytes the snapshot holds
    /// at `path`, if any.
    pub async fn snapshot_holds(
        folder: &Path,
        base: &Path,
        sandbox: Option<Arc<Sandbox>>,
        protected: &[PathBuf],
        ref_name: &str,
        path: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        let repo = crate::git::checkpoint_repo(folder, Some(base), &bounds(sandbox, protected))
            .await?
            .ok_or("no checkpoint repository")?;
        let sha = crate::git::snapshot(&repo, ref_name, "test").await?;
        Ok(crate::git::blob_at(&repo, &sha, path).await)
    }

    /// A sub-agent's folder made from `folder` (its base snapshot, then
    /// the fill), with the node's git in `sandbox`; returns the folder and
    /// removes nothing (the caller's temp directory does).
    pub async fn sub_agent_folder(
        folder: &Path,
        base: &Path,
        sandbox: Option<Arc<Sandbox>>,
        protected: &[PathBuf],
        child: &str,
    ) -> Result<PathBuf, String> {
        let sub = crate::git::sub_repo(folder, Some(base), &bounds(sandbox, protected))
            .await?
            .ok_or("not a repository")?;
        let snap = sub.base(child).await?;
        let sessions = crate::sessions_dir(base).ok_or("no sessions folder")?;
        Ok(sub.make_folder(child, &snap, &sessions).await?.path)
    }

    /// The checkpoint repository of `folder` is made (the object store is
    /// accepted for borrowing then), `between` runs, then a snapshot is
    /// taken with that same handle.
    pub async fn snapshot_after(
        folder: &Path,
        base: &Path,
        sandbox: Option<Arc<Sandbox>>,
        protected: &[PathBuf],
        between: impl FnOnce(),
    ) -> Result<String, String> {
        let repo = crate::git::checkpoint_repo(folder, Some(base), &bounds(sandbox, protected))
            .await?
            .ok_or("no checkpoint repository")?;
        between();
        crate::git::snapshot(&repo, "refs/harness/t/s/after", "test").await
    }

    /// A turn and its undo: snapshot `folder`, run `change` on it, snapshot
    /// again, undo (unforced), all with the node's git in `sandbox`.
    pub async fn turn_and_undo(
        folder: &Path,
        base: &Path,
        sandbox: Option<Arc<Sandbox>>,
        protected: &[PathBuf],
        change: impl FnOnce(),
    ) -> Result<Vec<String>, String> {
        use crate::git;
        let repo = git::checkpoint_repo(folder, Some(base), &bounds(sandbox, protected))
            .await?
            .ok_or("no checkpoint repository")?;
        let before = git::snapshot(&repo, "refs/harness/t/u/before", "before").await?;
        change();
        let after = git::snapshot(&repo, "refs/harness/t/u/after", "after").await?;
        let diff = git::restore_diff(&repo, &before, &after).await?;
        let now = git::snapshot(&repo, "refs/harness/t/r/before", "now").await?;
        let plan = git::restore_plan(&repo, &diff, &now, true).await?;
        git::secure_blobs(&repo, &plan).await?;
        git::restore_paths(&repo, &plan, git::WriteLimits::UNDO).await
    }

    /// The files a snapshot of `folder` lists (a turn's checkpoint).
    pub async fn snapshot_files(
        folder: &Path,
        base: &Path,
        sandbox: Option<Arc<Sandbox>>,
        protected: &[PathBuf],
    ) -> Result<Vec<(String, String)>, String> {
        let repo = crate::git::checkpoint_repo(folder, Some(base), &bounds(sandbox, protected))
            .await?
            .ok_or("no checkpoint repository")?;
        let sha = crate::git::snapshot(&repo, "refs/harness/t/files", "test").await?;
        Ok(crate::git::tree_listing(&repo, &sha).await)
    }
}

/// Where sub-agents' folders live: `<data>/sessions`, beside the checkpoint
/// store `checkpoints` (R.1 follow-up; the per-session layout of
/// docs/nodes-credentials-environments.md §4.11 puts every session there).
pub(crate) fn sessions_dir(checkpoints: &std::path::Path) -> Option<std::path::PathBuf> {
    let abs = std::path::absolute(checkpoints).ok()?;
    Some(abs.parent()?.join("sessions"))
}

#[derive(Clone)]
pub struct NodeRuntime {
    inner: Arc<Inner>,
}

impl NodeRuntime {
    /// Open the runtime and recover any turns interrupted by a crash.
    pub async fn start(mut cfg: NodeConfig) -> Result<Self, harness_store::StoreError> {
        // Lock files a killed git left in a checkpoint repository, and kept
        // copies a power loss damaged (A.18). Before any session runs.
        if let Some(base) = cfg.checkpoints_dir.clone() {
            let b = base.clone();
            let _ = tokio::task::spawn_blocking(move || git::sweep_stale_locks(&b)).await;
            git::check_kept_packs(&base).await;
            // Sub-agents' folders: protected like the data directory (no
            // session folder may hold them, no file tool or sandbox grant
            // reaches them) except for the one child each belongs to. The
            // folders of sub-agents that were running when the node stopped
            // are gone with their sessions.
            if let Some(s) = sessions_dir(&base) {
                let _ = std::fs::remove_dir_all(&s);
                cfg.policy.protect(&s);
            }
        }
        let rows = cfg.store.call(|s| s.sessions()).await?;
        let hosted: std::collections::HashSet<SessionId> =
            rows.iter().map(|r| r.session_id).collect();
        let stages = open_stages(&cfg, &hosted);
        let bg_limits = cfg.policy.limits.background();
        let bg_secrets = cfg.secrets.clone();
        let audit_log = audit::AuditLog::start(cfg.store.clone());
        let inner = Arc::new(Inner {
            cfg,
            sessions: Mutex::new(HashMap::new()),
            restoring: Mutex::new(Vec::new()),
            link: Mutex::new(None),
            audit_log,
            stages,
            stopping: AtomicBool::new(false),
            background: harness_tools::background::Registry::new(bg_limits, bg_secrets),
        });
        for row in rows {
            let s = Session::load(inner.clone(), row.session_id).await?;
            s.recover_interrupted().await;
            // After the turn's end: what ran in the background died with
            // the node, and no turn starts for it (R12).
            s.recover_background().await;
            inner
                .sessions
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(row.session_id, s);
        }
        let this = Self { inner };
        // Ended background commands are pruned by time whether their chat
        // calls in or not (T8D-1).
        {
            let reg = this.inner.background.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(harness_tools::background::PRUNE_EVERY).await;
                    reg.prune_all();
                }
            });
        }
        if let Some(stages) = this.inner.stages.clone() {
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(artifacts::EXPIRE_EVERY).await;
                    let s = stages.clone();
                    let _ = tokio::task::spawn_blocking(move || s.expire()).await;
                }
            });
        }
        if let Some(ttl) = this.inner.cfg.incognito_ttl {
            let rt = this.clone();
            // Check often enough that a chat never outlives its TTL by much.
            let every = (ttl / 10).clamp(
                std::time::Duration::from_secs(1),
                std::time::Duration::from_secs(300),
            );
            tokio::spawn(async move {
                loop {
                    rt.purge_incognito(ttl).await;
                    tokio::time::sleep(every).await;
                }
            });
        }
        Ok(this)
    }

    /// Bytes of background commands' output this node holds in memory
    /// now (bounded by `background::kept_bound` of its `[limits]`).
    pub fn background_held(&self) -> usize {
        self.inner.background.held()
    }

    pub fn node_id(&self) -> &str {
        &self.inner.cfg.node_id
    }

    pub fn store(&self) -> &Store {
        &self.inner.cfg.store
    }

    /// What waits for the master on the current link (diagnostics).
    pub fn backlog(&self) -> Backlog {
        self.inner.outbox().map(|o| o.backlog()).unwrap_or_default()
    }

    /// Connect a (new) master link. Any previous link is replaced. `hello` is
    /// queued before any event so the master always sees it first.
    pub async fn attach(&self) -> NodeLink {
        let (out, out_rx) = outbox::Outbox::open();
        let (in_tx, mut in_rx) = mpsc::channel::<MasterToNode>(IN_QUEUE);
        let heads = self
            .inner
            .cfg
            .store
            .call(|s| s.heads())
            .await
            .unwrap_or_default();
        {
            let mut link = self.inner.link.lock().unwrap_or_else(|p| p.into_inner());
            let mut capabilities = vec![
                "native".to_string(),
                match self.inner.cfg.profile {
                    harness_core::event::Profile::Chat => "profile:chat".into(),
                    harness_core::event::Profile::Agentic => "profile:agentic".into(),
                },
            ];
            if self.inner.stages.is_some() {
                capabilities.push(harness_protocol::CAP_ARTIFACTS.into());
            }
            capabilities.push(harness_protocol::CAP_IMAGES.into());
            // `remember` where this node offers it: registered, and not
            // refused by its own policy (the master files nothing otherwise).
            if self.inner.cfg.backend.has_tool("remember")
                && !self.inner.cfg.policy.denies("remember")
            {
                capabilities.push(harness_protocol::CAP_REMEMBER.into());
            }
            if self.inner.cfg.profile == harness_core::event::Profile::Agentic {
                // The modes it offers and their gaps (N.10a), as decided at
                // start.
                capabilities.extend(self.inner.cfg.policy.exec.capabilities());
                // Its approval modes (T48): the ceiling, and what a new chat
                // gets; each chat's own limit (a `host` chat's) the UI knows
                // from the chat.
                let p = &self.inner.cfg.policy;
                capabilities.push(format!(
                    "{}{}",
                    harness_protocol::CAP_APPROVAL_MAX_PREFIX,
                    p.max_approval_mode.id()
                ));
                capabilities.push(format!(
                    "{}{}",
                    harness_protocol::CAP_APPROVAL_DEFAULT_PREFIX,
                    p.effective_mode(None).id()
                ));
                capabilities.push(harness_protocol::CAP_BACKGROUND.into());
                // Off is silent: with `callback_turns = 0` the master asks
                // nothing, and an end waits for the person as before.
                if self.inner.cfg.policy.limits.callback_turns > 0 {
                    capabilities.push(harness_protocol::CAP_CALLBACK.into());
                }
            }
            // A fresh queue: there is room.
            let _ = out.try_send(NodeToMaster::Hello {
                protocol_version: PROTOCOL_VERSION,
                node_id: self.inner.cfg.node_id.clone(),
                hostname: self.inner.cfg.hostname.clone(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                capabilities,
                sessions: heads
                    .into_iter()
                    .map(|(session_id, head_seq)| SessionHead {
                        session_id,
                        head_seq,
                    })
                    .collect(),
            });
            if let Some(old) = link.replace(out) {
                old.retire();
            }
        }
        let this = self.clone();
        tokio::spawn(async move {
            let lanes = Lanes::default();
            while let Some(msg) = in_rx.recv().await {
                match msg {
                    // A fetch is answered from a task of its own: a transfer
                    // never waits behind a command, nor a command behind a
                    // disk read.
                    MasterToNode::ArtifactFetch {
                        req_id,
                        session_id,
                        stage_id,
                        offset,
                        len,
                    } => this.fetch(req_id, session_id, stage_id, offset, len),
                    // Answered here, at once: they must not wait behind a
                    // slow restore or delete (AUDIT S10), nor behind the
                    // output queued for the master (A.21): their acks go
                    // ahead of it and never wait for room. Nothing here
                    // awaits the link or the store (their audit lines go to
                    // the audit writer, `audit.rs`).
                    //
                    // A cancel stops the running turn now, and also leaves a
                    // marker in the lane so a prompt sent before it, still
                    // queued, is stopped too: a cancel covers everything
                    // sent before it, as when the loop was serial. For a
                    // session that does not exist yet but has a lane (its
                    // create is still queued) the cancel waits its turn
                    // there; for one with neither it is refused here, at
                    // once (review A21-2).
                    MasterToNode::Cancel { session_id, .. }
                        if this.session(session_id).is_none()
                            && lanes
                                .lock()
                                .unwrap_or_else(|p| p.into_inner())
                                .contains_key(&session_id) =>
                    {
                        this.enqueue(&lanes, session_id, LaneCmd::Msg(Box::new(msg)))
                    }
                    MasterToNode::Cancel { session_id, .. } => {
                        this.handle(msg).await;
                        this.mark_cancel(&lanes, session_id);
                    }
                    m @ (MasterToNode::ApprovalRespond { .. }
                    | MasterToNode::StopBackground { .. }
                    | MasterToNode::Welcome { .. }) => this.handle(m).await,
                    // Everything else goes to its session's lane: in order
                    // for that session, concurrently with other sessions.
                    other => match lane_of(&other) {
                        Some(sid) => this.enqueue(&lanes, sid, LaneCmd::Msg(Box::new(other))),
                        None => this.handle(other).await,
                    },
                }
            }
        });
        NodeLink {
            to_node: in_tx,
            from_node: out_rx,
        }
    }

    fn session(&self, sid: SessionId) -> Option<Arc<Session>> {
        self.inner.session(sid)
    }

    /// Queue `msg` on its session's lane, starting the lane if there is
    /// none. A full lane refuses the command at once (a nack, never a
    /// silent drop, never a wait that would hold up other sessions).
    fn enqueue(&self, lanes: &Lanes, sid: SessionId, cmd: LaneCmd) {
        let mut map = lanes.lock().unwrap_or_else(|p| p.into_inner());
        let Some(tx) = map.get(&sid) else {
            if map.len() >= MAX_LANES {
                drop(map);
                return self.refuse(
                    cmd,
                    format!(
                        "this node has {MAX_LANES} sessions with commands in progress; try again"
                    ),
                );
            }
            return self.start_lane(lanes, &mut map, sid, cmd);
        };
        let cmd = match tx.try_send(cmd) {
            Ok(()) => return,
            Err(mpsc::error::TrySendError::Full(cmd)) => {
                drop(map);
                return self.refuse(
                    cmd,
                    format!("this session already has {LANE_DEPTH} commands waiting; try again"),
                );
            }
            // The lane's task ended without removing itself (it panicked):
            // nothing of its queue is left, so a new lane keeps the order.
            Err(mpsc::error::TrySendError::Closed(cmd)) => cmd,
        };
        self.start_lane(lanes, &mut map, sid, cmd);
    }

    /// A command no lane takes: a nack at once, never a silent drop. It
    /// follows no event, so it goes ahead of the queued output like the
    /// inline commands' acks (re-check D-1: in the ordered queue it was
    /// dropped while the uplink was full).
    fn refuse(&self, cmd: LaneCmd, error: String) {
        if let LaneCmd::Msg(msg) = cmd
            && let Some(req_id) = msg.req_id()
        {
            self.inner.publish_urgent(NodeToMaster::Ack {
                req_id: req_id.to_string(),
                ok: false,
                error: Some(error),
            });
        }
    }

    fn start_lane(
        &self,
        lanes: &Lanes,
        map: &mut HashMap<SessionId, mpsc::Sender<LaneCmd>>,
        sid: SessionId,
        first: LaneCmd,
    ) {
        let (tx, rx) = mpsc::channel(LANE_DEPTH);
        let _ = tx.try_send(first);
        map.insert(sid, tx);
        tokio::spawn(self.clone().lane(lanes.clone(), sid, rx));
    }

    /// After an inline cancel: stop whatever the lane starts from commands
    /// queued before it. No lane, nothing queued, nothing to do; a full
    /// lane gets no marker (the turn already running was cancelled; a
    /// prompt among the 64 waiting commands may still start).
    fn mark_cancel(&self, lanes: &Lanes, sid: SessionId) {
        let map = lanes.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(tx) = map.get(&sid) {
            let _ = tx.try_send(LaneCmd::Cancel);
        }
    }

    /// One session's commands, one at a time, in the order they came. Ends
    /// as soon as its queue is empty, so lanes exist only for sessions with
    /// a command in progress (a master that names many sessions cannot pile
    /// up idle tasks). The check that the queue is empty and the lane's
    /// removal happen under the map's lock, so no command can be queued on
    /// a lane that is ending.
    async fn lane(self, lanes: Lanes, sid: SessionId, mut rx: mpsc::Receiver<LaneCmd>) {
        loop {
            let cmd = match rx.try_recv() {
                Ok(cmd) => cmd,
                Err(_) => {
                    let mut map = lanes.lock().unwrap_or_else(|p| p.into_inner());
                    match rx.try_recv() {
                        Ok(cmd) => cmd,
                        Err(_) => {
                            map.remove(&sid);
                            return;
                        }
                    }
                }
            };
            match cmd {
                LaneCmd::Msg(msg) => self.handle(*msg).await,
                LaneCmd::Cancel => {
                    if let Some(s) = self.session(sid) {
                        s.cancel();
                    }
                }
            }
        }
    }

    /// Answer `artifact.fetch` (never acked: the chunk is the answer).
    fn fetch(&self, req_id: String, sid: SessionId, stage_id: String, offset: u64, len: u32) {
        match &self.inner.stages {
            Some(st) => {
                let inner = self.inner.clone();
                let out: artifacts::Reply = Arc::new(move |m| inner.publish(m));
                st.fetch(out, req_id, sid, stage_id, offset, len)
            }
            None => self.inner.publish(NodeToMaster::ArtifactChunk {
                req_id,
                stage_id,
                offset,
                data: String::new(),
                eof: false,
                error: Some("publishing is off on this node".into()),
            }),
        }
    }

    /// "Save as artifact": check the path now (the ack carries a refusal),
    /// copy it later, ending in `artifact_staged` or
    /// `artifact_stage_failed` on the session's log.
    async fn stage_for_user(
        &self,
        sid: SessionId,
        path: String,
        name: Option<String>,
    ) -> Result<(), String> {
        use harness_core::artifacts::StagedBy;
        use harness_core::event::{ArtifactStageFailed, EventKind};
        let stages = self
            .inner
            .stages
            .clone()
            .ok_or("publishing is off on this node")?;
        let s = self.session(sid).ok_or("unknown session")?;
        s.publish_cwd()
            .ok_or("this chat cannot save files as artifacts")?;
        // Pinned folder and node policy, as before a turn (R.1, T4).
        let cwd = s.checked_folder()?;
        let name = name
            .map(|n| harness_core::artifacts::check_name(&n))
            .transpose()?;
        let src = stages.check(&cwd, &path, StagedBy::User)?;
        tokio::spawn(async move {
            let req = artifacts::Request {
                session: sid,
                by: StagedBy::User,
                name: name.clone(),
                notes: None,
                kind_hint: None,
                call_id: None,
            };
            let mut stage_id = None;
            let kind = match stages.stage(req, src, Default::default()).await {
                Ok(staged) => {
                    stage_id = Some(staged.stage_id.clone());
                    EventKind::ArtifactStaged(Box::new(staged))
                }
                Err(error) => {
                    EventKind::ArtifactStageFailed(ArtifactStageFailed { path, name, error })
                }
            };
            if let Err(e) = s.record(kind).await {
                tracing::error!(session = %sid, error = %e, "could not record a staged file");
                // No event names the copy: drop it.
                if let Some(id) = stage_id {
                    let _ = stages.release(sid, &id);
                }
            }
        });
        Ok(())
    }

    /// The master is done with a stage.
    fn release(&self, sid: SessionId, stage_id: &str) -> Result<(), String> {
        self.inner
            .stages
            .as_ref()
            .ok_or("publishing is off on this node")?
            .release(sid, stage_id)
    }

    async fn audit(&self, kind: &'static str, sid: SessionId) {
        self.audit_with(kind, sid, serde_json::json!({})).await
    }

    /// [`Self::audit`] with more ids (never content).
    async fn audit_with(&self, kind: &'static str, sid: SessionId, mut extra: serde_json::Value) {
        extra["session_id"] = serde_json::json!(sid);
        let detail = extra.to_string();
        let _ = self
            .inner
            .cfg
            .store
            .call(move |s| s.audit(kind, &detail))
            .await;
    }

    /// The audit line of an inline command, handed to the audit writer:
    /// never waits (`audit.rs`).
    fn audit_inline(&self, kind: &'static str, sid: SessionId) {
        let detail = serde_json::json!({ "session_id": sid }).to_string();
        self.inner.audit_log.record(kind, detail);
    }

    /// Handle one message from the master.
    pub async fn handle(&self, msg: MasterToNode) {
        let req_id = msg.req_id().map(str::to_string);
        // Answered inline by the command loop: their acks go ahead of the
        // queued output and never wait (A.21). Not a create's ack, nor any
        // other lane command's: the master relies on those coming after
        // the events their command appended.
        let inline = match &msg {
            MasterToNode::Cancel { session_id, .. } => Some(("session.cancel", *session_id)),
            MasterToNode::StopBackground { session_id, .. } => {
                Some(("session.stop_background", *session_id))
            }
            MasterToNode::ApprovalRespond { session_id, .. } => {
                Some(("approval.respond", *session_id))
            }
            _ => None,
        };
        // A stopping node starts nothing new; answers to approvals, cancels,
        // replays, deletes and fetches are still taken.
        if self.inner.is_stopping() && starts_work(&msg) {
            if let Some(req_id) = req_id {
                self.inner
                    .publish_wait(NodeToMaster::Ack {
                        req_id,
                        ok: false,
                        error: Some(NODE_STOPPING.into()),
                    })
                    .await;
            }
            return;
        }
        let result: Result<(), String> = match msg {
            MasterToNode::Welcome { protocol_version } => {
                if protocol_version != PROTOCOL_VERSION {
                    tracing::warn!(protocol_version, "master speaks a different protocol");
                }
                Ok(())
            }
            MasterToNode::Resume {
                session_id,
                last_seq,
            } => {
                self.replay(session_id, last_seq).await;
                Ok(())
            }
            MasterToNode::Create {
                no_callbacks,
                session_id,
                title,
                cwd,
                model,
                approval_mode,
                profile,
                agent,
                incognito,
                exec_mode,
                ..
            } => {
                // The mode asked for (N.10a review N-d); what was chosen is
                // in `session_created`.
                self.audit_with(
                    "session.create",
                    session_id,
                    serde_json::json!({ "exec_mode": exec_mode.map(|m| m.id()) }),
                )
                .await;
                self.create(
                    session_id,
                    CreateArgs {
                        title,
                        cwd,
                        model,
                        approval_mode,
                        profile,
                        agent,
                        incognito,
                        no_callbacks,
                        exec_mode,
                    },
                )
                .await
            }
            MasterToNode::SelectVariant {
                session_id,
                group,
                message_id,
                ..
            } => match self.session(session_id) {
                Some(s) => s.select_variant(group, message_id).await,
                None => Err("unknown session".into()),
            },
            MasterToNode::Restore {
                session_id,
                turn_id,
                force,
                rewind,
                ..
            } => {
                self.audit("session.restore", session_id).await;
                match self.session(session_id) {
                    Some(s) => s.restore(&turn_id, force, rewind).await,
                    None => Err("unknown session".into()),
                }
            }
            MasterToNode::Delete { session_id, .. } => {
                self.audit("session.delete", session_id).await;
                self.delete(session_id).await
            }
            MasterToNode::Prompt {
                session_id,
                text,
                agent,
                agent_named,
                attachments,
                compare,
                savings,
                context,
                reply,
                images,
                ..
            } => {
                self.audit("session.prompt", session_id).await;
                let extra = harness_core::backend::TurnExtra {
                    savings: savings.unwrap_or_default(),
                    context,
                    ..Default::default()
                };
                let input = session::Input {
                    text,
                    attachments,
                    reply,
                    images,
                };
                match self.session(session_id) {
                    Some(s) => s.prompt(input, agent, agent_named, compare, extra).await,
                    None => Err("unknown session".into()),
                }
            }
            MasterToNode::Callback {
                session_id,
                bg_ids,
                agent,
                savings,
                ..
            } => {
                self.audit("session.callback", session_id).await;
                match self.session(session_id) {
                    Some(s) => s.callback(bg_ids, agent, savings.unwrap_or_default()).await,
                    None => Err("unknown session".into()),
                }
            }
            MasterToNode::SetAgent {
                session_id, agent, ..
            } => {
                self.audit("session.set_agent", session_id).await;
                match self.session(session_id) {
                    Some(s) => s.set_agent(agent).await,
                    None => Err("unknown session".into()),
                }
            }
            MasterToNode::Cancel { session_id, .. } => match self.session(session_id) {
                Some(s) => {
                    s.cancel();
                    Ok(())
                }
                None => Err("unknown session".into()),
            },
            // In the session's lane, which a running turn does not hold: a
            // change made while a turn runs applies from its next call (T48).
            MasterToNode::SetApprovalMode {
                session_id, mode, ..
            } => {
                self.audit("session.set_approval_mode", session_id).await;
                match self.session(session_id) {
                    Some(s) => s.set_mode(mode).await.map(|_| ()),
                    None => Err("unknown session".into()),
                }
            }
            MasterToNode::StopBackground {
                session_id,
                bg_id,
                by,
                ..
            } => match self.session(session_id) {
                Some(s) => s.stop_background(bg_id.as_deref(), by),
                None => Err("unknown session".into()),
            },
            MasterToNode::ApprovalRespond {
                session_id,
                approval_id,
                decision,
                ..
            } => match self.session(session_id) {
                Some(s) => s.respond(&approval_id, decision),
                None => Err("unknown session".into()),
            },
            MasterToNode::ArtifactFetch {
                req_id,
                session_id,
                stage_id,
                offset,
                len,
            } => {
                self.fetch(req_id, session_id, stage_id, offset, len);
                Ok(())
            }
            MasterToNode::ArtifactStage {
                session_id,
                path,
                name,
                ..
            } => {
                self.audit("artifact.stage", session_id).await;
                self.stage_for_user(session_id, path, name).await
            }
            MasterToNode::ArtifactRelease {
                session_id,
                stage_id,
                ..
            } => self.release(session_id, &stage_id),
        };
        if let Some(req_id) = req_id {
            let ack = NodeToMaster::Ack {
                req_id,
                ok: result.is_ok(),
                error: result.err(),
            };
            if inline.is_some() {
                self.inner.publish_urgent(ack);
            } else {
                self.inner.publish_wait(ack).await;
            }
        }
        // Acted on and acked first: the audit line never holds them up
        // (review A21-1).
        if let Some((kind, sid)) = inline {
            self.audit_inline(kind, sid);
        }
    }

    async fn create(&self, sid: SessionId, args: CreateArgs) -> Result<(), String> {
        use harness_core::event::Profile;
        let CreateArgs {
            title,
            cwd,
            model,
            approval_mode,
            profile,
            agent,
            incognito,
            no_callbacks,
            exec_mode,
        } = args;
        if self.session(sid).is_some() {
            return Err("session already exists".into());
        }
        let hosted = self.inner.cfg.profile;
        let profile = profile.unwrap_or(Profile::Agentic);
        if incognito && profile != Profile::Chat {
            return Err("incognito is only available for chat sessions".into());
        }
        if profile != hosted {
            return Err(format!(
                "this runtime hosts {hosted:?} sessions, not {profile:?}"
            ));
        }
        let policy = &self.inner.cfg.policy;
        // The mode first (N.10a): chosen inside what this node offers,
        // whatever the master asked; a chat session runs no commands and has
        // none.
        let exec_mode = match profile {
            Profile::Agentic => Some(policy.exec.choose(exec_mode)?),
            Profile::Chat if exec_mode.is_some() => {
                return Err("a chat session runs no commands and has no execution mode".into());
            }
            Profile::Chat => None,
        };
        // Chat sessions have no workspace; their runtime offers no file tools.
        let cwd = match profile {
            Profile::Agentic => policy.check_cwd(&cwd)?,
            Profile::Chat => std::path::PathBuf::new(),
        };
        // Within node.toml's ceiling, and a `host` session never in `auto`,
        // whatever was asked (review N-e, T48).
        let mode = policy.session_mode(approval_mode, exec_mode);
        let model = agent
            .as_ref()
            .and_then(|a| a.model.clone())
            .or(model)
            .unwrap_or_else(|| self.inner.cfg.default_model.clone());
        let s = Session::create(
            self.inner.clone(),
            sid,
            session::NewSession {
                title,
                cwd,
                model,
                mode,
                profile,
                agent_id: agent.map(|a| a.agent_id),
                incognito,
                parent_session: None,
                owned: None,
                no_callbacks,
                exec_mode,
                above: Vec::new(),
            },
        )
        .await
        .map_err(|e| e.to_string())?;
        self.inner
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(sid, s);
        Ok(())
    }

    /// Stop (SIGTERM or SIGINT, A.19): from now on new work is refused with
    /// [`NODE_STOPPING`]; every running turn is cancelled through its normal
    /// cancel path (a command gets SIGTERM, then its guard's kill; a pending
    /// approval is denied) and ends `stopped`; then this waits for every
    /// turn and restore to end, at most `bound`. Returns the sessions whose
    /// turn had not ended by then, or ended without its end in the log
    /// (the next start closes them as `interrupted`), and those with a
    /// background command's end the log did not take (the next start
    /// records it as `node_restarted`). Background commands are stopped,
    /// not waited for. Idempotent.
    pub async fn stop(&self, bound: Duration) -> Vec<SessionId> {
        self.inner.stopping.store(true, Ordering::SeqCst);
        let by = tokio::time::Instant::now() + bound;
        let mut logged_stuck = std::collections::HashSet::new();
        loop {
            // Read again each time: a prompt that claimed its turn just
            // before the flag was set is cancelled on the next pass (and
            // one after it refuses itself, `Session::prompt`).
            let sessions: Vec<Arc<Session>> = self
                .inner
                .sessions
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .values()
                .cloned()
                .collect();
            let running: Vec<&Arc<Session>> = sessions.iter().filter(|s| s.is_running()).collect();
            // Background commands are not waited for: stopped now, each
            // end recorded as `node_stopping` (R13). Ordered on every pass,
            // for one a turn started just before the flag.
            for s in &sessions {
                s.stop_background_now(harness_core::event::BackgroundReason::NodeStopping);
            }
            // Ends the log did not take are tried again on each pass; once
            // the commands are gone and a try failed, waiting buys nothing
            // (T8D-6): those sessions are named in the result.
            let mut stuck = std::collections::HashSet::new();
            for s in &sessions {
                if s.retry_background().await {
                    stuck.insert(s.id());
                }
            }
            // Waited for: commands that run and ends not yet recorded. Not
            // node slots: a still-alive end keeps its slot until its guard
            // exits, which a stop cannot hasten (R-1); it is logged once.
            for s in &sessions {
                for bg_id in s.background_stuck() {
                    if logged_stuck.insert((s.id(), bg_id.clone())) {
                        tracing::warn!(session = %s.id(), bg = %bg_id, "a background command's end is recorded but a process of it is still alive (its guard keeps killing it); the stop does not wait for it");
                    }
                }
            }
            let background = sessions.iter().any(|s| {
                s.background_active() || (s.background_unrecorded() && !stuck.contains(&s.id()))
            });
            // All marked before any is cancelled: a sub-agent cancelled by
            // its parent's token is the stop's too (review N-7).
            for s in &running {
                s.mark_stopped();
            }
            for s in &running {
                s.cancel();
            }
            // After the flag, under the registry's lock: a restore either
            // registered before (and is waited for) or refuses itself
            // (`Session::claim_folder`, review A19-4).
            let restoring = !self
                .inner
                .restoring
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_empty();
            let done = running.is_empty() && !restoring && !background;
            if done || tokio::time::Instant::now() >= by {
                // A turn whose end could not be written is not closed
                // either (review A19-5).
                return sessions
                    .iter()
                    .filter(|s| s.is_running() || s.is_unclosed() || s.background_unrecorded())
                    .map(|s| s.id())
                    .collect();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Wait until every message queued for the master has been taken by the
    /// link, and every audit line queued has been written or counted and
    /// the count written, at most `bound` (a stop's last events reach the
    /// master before the process ends; anything left replays at the next
    /// hello; re-check D-2 for the audit lines). True when both are done
    /// (no link counts as done for the first); when the bound runs out
    /// with audit lines unwritten, says at error level how many.
    pub async fn flush(&self, bound: Duration) -> bool {
        let by = tokio::time::Instant::now() + bound;
        loop {
            let empty = self.inner.outbox().is_none_or(|o| o.is_drained());
            let audits = self.inner.audit_log.pending();
            if empty && audits.is_empty() {
                return true;
            }
            if tokio::time::Instant::now() >= by {
                if !audits.is_empty() {
                    tracing::error!(
                        queued = audits.queued,
                        being_written = audits.taken,
                        counted = audits.counted,
                        "audit lines not written before the stop (and a count of lines not \
                         recorded, if any)"
                    );
                }
                return false;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Delete a session: cancel its turn, wait for it to end, drop the log.
    pub async fn delete(&self, sid: SessionId) -> Result<(), String> {
        if let Some(s) = self.session(sid) {
            s.cancel();
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
            while s.is_running() {
                if tokio::time::Instant::now() > deadline {
                    return Err("the running turn did not stop".into());
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            // Its background commands go with it, all at once and before
            // its log; their ends are not recorded (the log goes).
            s.stop_background_within(
                harness_core::event::BackgroundReason::SessionDeleted,
                BACKGROUND_STOP_WAIT,
            )
            .await;
        }
        self.inner
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&sid);
        // The session's sandbox temp folder (TMPDIR) goes with it.
        if let Some(root) = &self.inner.cfg.policy.sandbox_tmp {
            let _ = std::fs::remove_dir_all(root.join(sid.to_string()));
        }
        let existed = self
            .inner
            .cfg
            .store
            .call(move |s| {
                let existed = s.delete_session(sid, false)?;
                s.checkpoint()?;
                Ok::<_, harness_store::StoreError>(existed)
            })
            .await
            .map_err(|e| e.to_string())?;
        if existed {
            self.inner
                .publish_wait(NodeToMaster::Deleted { session_id: sid })
                .await;
        }
        Ok(())
    }

    /// Delete incognito sessions idle for longer than `ttl`. Returns how many.
    pub async fn purge_incognito(&self, ttl: std::time::Duration) -> usize {
        let before = harness_core::now_ms() - ttl.as_millis() as i64;
        let ids = self
            .inner
            .cfg
            .store
            .call(move |s| s.expired_incognito(before))
            .await
            .unwrap_or_default();
        let mut n = 0;
        for sid in ids {
            if self.delete(sid).await.is_ok() {
                n += 1;
            }
        }
        n
    }

    async fn replay(&self, sid: SessionId, mut after: u64) {
        loop {
            let store = self.inner.cfg.store.clone();
            let res = store
                .call(move |s| {
                    let head = s.head_seq(sid)?;
                    let evs = s.events_after(sid, after, SNAPSHOT_BATCH)?;
                    Ok::<_, harness_store::StoreError>((head, evs))
                })
                .await;
            let (head, mut events) = match res {
                Ok(v) => v,
                Err(e) => {
                    tracing::error!(%sid, error = %e, "replay failed");
                    return;
                }
            };
            let mut done = events.len() < SNAPSHOT_BATCH;
            let keep = within_bytes(&events);
            if keep < events.len() {
                events.truncate(keep);
                done = false;
            }
            let last = events.last().map(|e| e.seq);
            self.inner
                .publish_wait(NodeToMaster::Snapshot {
                    session_id: sid,
                    head_seq: head,
                    events,
                })
                .await;
            match last {
                Some(l) if !done => after = l,
                _ => return,
            }
        }
    }
}

/// How many of `events` (at least one) fit in one snapshot of
/// `SNAPSHOT_BYTES`, the envelope included.
fn within_bytes(events: &[harness_core::RawEvent]) -> usize {
    const ENVELOPE: usize = 256;
    let mut total = ENVELOPE;
    for (i, e) in events.iter().enumerate() {
        total += serde_json::to_vec(e).map_or(0, |v| v.len()) + 1;
        if total > SNAPSHOT_BYTES {
            return i.max(1);
        }
    }
    events.len()
}

/// The stage, when this runtime publishes: a worker node with
/// `NodeConfig.artifacts` and `[artifacts] enabled` in node.toml. A stage
/// that cannot be opened turns publishing off rather than the node.
fn open_stages(
    cfg: &NodeConfig,
    hosted: &std::collections::HashSet<SessionId>,
) -> Option<Arc<artifacts::Stages>> {
    let sc = cfg.artifacts.as_ref()?;
    if !cfg.policy.artifacts.enabled || cfg.profile != harness_core::event::Profile::Agentic {
        return None;
    }
    match artifacts::Stages::open(sc, &cfg.policy, hosted) {
        Ok(s) => Some(Arc::new(s.with_checkpoints(
            cfg.checkpoints_dir.clone(),
            cfg.git_sandbox.clone(),
        ))),
        Err(e) => {
            tracing::warn!(error = %e, dir = %sc.stage_dir.display(), "cannot open the artifact stage; publishing is off");
            None
        }
    }
}
