//! Node runtime: owns sessions, runs agent turns, is the single writer of each
//! session's event log, and talks to the master over a `NodeLink`.

pub mod artifacts;
mod attached;
mod audit;
mod board;
mod connectors;
mod docs;
mod environment;
pub mod exec;
mod forge_clone;
mod git;
mod history;
mod images;
mod instructions;
mod others;
mod outbox;
pub mod policy;
mod project_mcp;
pub mod projects;
mod recall;
mod session;
mod stored_skills;
#[cfg(test)]
mod test_script;
mod uploads;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::SessionId;
use harness_core::backend::AgentBackend;
use harness_protocol::{
    CallbackMsg, MasterToNode, NODE_STOPPING, NodeToMaster, PROTOCOL_VERSION, PromptMsg, RetryMsg,
    SessionHead,
};
use harness_store::Store;
use tokio::sync::mpsc;

pub use git::SNAPSHOT_TIMEOUT;
pub use outbox::Backlog;
pub use policy::{InstructionsPolicy, NodePolicy, SkillsPolicy, WebPolicy};
use session::Session;

/// Commands that start new work, refused while the node stops: a session,
/// a turn, a restore, a staged file.
fn starts_work(msg: &MasterToNode) -> bool {
    matches!(
        msg,
        MasterToNode::Create { .. }
            | MasterToNode::Fork { .. }
            | MasterToNode::Prompt(_)
            | MasterToNode::Callback(_)
            | MasterToNode::Retry(_)
            | MasterToNode::Rewind { .. }
            | MasterToNode::Handoff { .. }
            | MasterToNode::Compact { .. }
            | MasterToNode::Restore { .. }
            | MasterToNode::ArtifactStage { .. }
            | MasterToNode::UploadChunk { .. }
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
        // The new chat's lane, as a create's.
        | MasterToNode::Fork { session_id, .. }
        | MasterToNode::SelectVariant { session_id, .. }
        | MasterToNode::Restore { session_id, .. }
        | MasterToNode::Delete { session_id, .. }
        | MasterToNode::Prompt(PromptMsg { session_id, .. })
        | MasterToNode::Callback(CallbackMsg { session_id, .. })
        | MasterToNode::Retry(RetryMsg { session_id, .. })
        | MasterToNode::Rewind { session_id, .. }
        | MasterToNode::RewindUndo { session_id, .. }
        | MasterToNode::Pin { session_id, .. }
        | MasterToNode::Handoff { session_id, .. }
        | MasterToNode::Compact { session_id, .. }
        | MasterToNode::SetAgent { session_id, .. }
        | MasterToNode::SetApprovalMode { session_id, .. }
        | MasterToNode::SetExecMode { session_id, .. }
        | MasterToNode::RevokeAllow { session_id, .. }
        | MasterToNode::SetStepLimit { session_id, .. }
        | MasterToNode::ArtifactStage { session_id, .. }
        | MasterToNode::UploadChunk { session_id, .. }
        | MasterToNode::ArtifactRelease { session_id, .. } => Some(*session_id),
        MasterToNode::Welcome { .. }
        | MasterToNode::Cancel { .. }
        | MasterToNode::StopBackground { .. }
        | MasterToNode::ApprovalRespond { .. }
        | MasterToNode::ArtifactFetch { .. }
        | MasterToNode::RecallResult { .. }
        | MasterToNode::AttachedResult { .. }
        | MasterToNode::DocsResult { .. }
        | MasterToNode::BoardResult { .. }
        | MasterToNode::SkillResult { .. }
        | MasterToNode::ConnectorResult { .. }
        | MasterToNode::Diff { .. }
        | MasterToNode::BackgroundOutput { .. }
        | MasterToNode::Files { .. }
        | MasterToNode::DocPreview { .. }
        | MasterToNode::FolderFile { .. }
        | MasterToNode::DocPageEdit { .. }
        | MasterToNode::ProjectCreate { .. }
        | MasterToNode::ProjectClone { .. }
        | MasterToNode::ProjectMcp { .. } => None,
    }
}

/// A turn command's `owner` (X.36): a malformed one refuses the command
/// (nothing about it is logged); a well-formed one is kept for the chat's
/// table.
async fn learn_owner(
    s: &Session,
    owner: Option<String>,
    identity: Option<harness_core::identity::CommitIdentity>,
) -> Result<(), String> {
    if owner
        .as_deref()
        .is_some_and(|o| !harness_protocol::valid_user_id(o))
    {
        return Err("the command names a malformed account id as the chat's owner".into());
    }
    s.learn_owner(owner, identity).await;
    Ok(())
}

/// Turn diffs computed at once (X.12); one more is refused at once with
/// [`DIFF_BUSY`], so a master asking in a loop cannot pile up git work.
const DIFF_AT_ONCE: usize = 2;
const DIFF_BUSY: &str =
    "this node is already reading the changes of other turns; try again in a moment";

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
    /// What this node announces for the Plugins screen (X.34), read once
    /// per runtime by the first link and shared by every later one; a read
    /// still in flight is waited for, never started twice.
    plugins: tokio::sync::OnceCell<harness_protocol::NodePlugins>,
    /// The audit lines of inline commands (`audit.rs`).
    audit_log: audit::AuditLog,
    /// Where published files are staged; `None` when this runtime does not
    /// publish (the chat runtime, or `[artifacts] enabled = false`).
    pub(crate) stages: Option<Arc<artifacts::Stages>>,
    /// Files the master pushed for a prompt that has not taken them yet
    /// (PLAN.md T94); off where this runtime has no data directory.
    pub(crate) uploads: uploads::Staging,
    /// Set once by [`NodeRuntime::stop`]: new work is refused from then on
    /// ([`NODE_STOPPING`]) and a cancelled turn ends `stopped`.
    stopping: AtomicBool,
    /// Background commands (T.8d): the node's count and its `[limits]`.
    pub(crate) background: Arc<harness_tools::background::Registry>,
    /// `recall` calls waiting for the master's answer (PLAN.md T55).
    pub(crate) recalls: recall::Pending,
    /// `attached_file` calls waiting for the master's answer (PLAN.md T95).
    pub(crate) attacheds: attached::Pending,
    /// `doc_create` and `sheet_create` calls waiting for the master (T97).
    pub(crate) docs_pending: docs::Pending,
    /// `board` and `board_edit` calls waiting for the master (PLAN.md T78).
    pub(crate) boards: board::Pending,
    /// Loads of a stored skill waiting for their `skill.result`.
    pub(crate) stored_skills: stored_skills::Pending,
    /// Connector calls waiting for the master's answer (X.35 step 2).
    pub(crate) connectors: connectors::Pending,
    /// Room for turn diffs being read ([`DIFF_AT_ONCE`]).
    diffs: Arc<tokio::sync::Semaphore>,
    /// Room for `@` completions being walked (T63), as many as diffs.
    file_lists: Arc<tokio::sync::Semaphore>,
    /// Room for background-output answers (X.62), apart from `@`
    /// completion's: an open row cannot refuse a file list, nor the reverse.
    bg_outputs: Arc<tokio::sync::Semaphore>,
    /// Room for one page preview at a time (X.59, T99): the helper has two
    /// places, and a person's previews take at most one, so they cannot
    /// starve the agent's own `doc_view`.
    previews: Arc<tokio::sync::Semaphore>,
    /// The one place a person's text-file views take (X.63).
    file_views: Arc<tokio::sync::Semaphore>,
    /// New project folders made in the last hour (T77), and room for the
    /// ones being made (one at a time).
    projects_made: projects::Made,
    project_slots: Arc<tokio::sync::Semaphore>,
    /// Room for the clone being run (`project.clone`, T86): one at a time,
    /// apart from `project.create`'s, which a ten-minute clone must not hold.
    clone_slots: Arc<tokio::sync::Semaphore>,
    /// A repository's MCP servers (X.36, PLAN.md T82): only where node.toml
    /// says `[mcp] project_servers = "ask"` on an agentic node; `None`
    /// otherwise, and then no `.mcp.json` is ever read.
    pub(crate) project_mcp: Option<Arc<project_mcp::ProjectMcp>>,
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

    /// Run the document helper as `sh -c <script>` instead of this binary
    /// (`tests/folder_preview_busy.rs`): to be set before anything asks for
    /// the helper, once per process.
    pub fn doc_helper_script(script: &str) {
        let _ = DOC_HELPER_SCRIPT.set(script.to_string());
    }
    pub(crate) static DOC_HELPER_SCRIPT: std::sync::OnceLock<String> = std::sync::OnceLock::new();

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

    static QUEUED_HOLD: Holds = std::sync::Mutex::new(Vec::new());

    static FAIL_END: std::sync::Mutex<Vec<harness_core::SessionId>> =
        std::sync::Mutex::new(Vec::new());

    /// The next background end session `sid` records between turns is
    /// refused by its log once, as a full disk would (X.24 review F4): the
    /// end stays queued, built, for the next try.
    pub fn fail_next_end_append(sid: harness_core::SessionId) {
        FAIL_END.lock().unwrap().push(sid);
    }

    pub(crate) fn take_fail_end(sid: harness_core::SessionId) -> bool {
        let mut all = FAIL_END.lock().unwrap();
        match all.iter().position(|s| *s == sid) {
            Some(i) => {
                all.remove(i);
                true
            }
            None => false,
        }
    }

    /// Arm a hold for session `sid`'s next start of a turn for the messages
    /// sent while the last one ran: after the ended turn freed the slot and
    /// before the next one claims it (X.20 review M1, a `/compact` in that
    /// window).
    pub fn hold_next_queued_start(sid: harness_core::SessionId) -> Arc<EndHold> {
        let h = Arc::new(EndHold {
            held: tokio::sync::Notify::new(),
            go: tokio::sync::Semaphore::new(0),
            is_held: Default::default(),
        });
        QUEUED_HOLD.lock().unwrap().push((sid, h.clone()));
        h
    }

    pub(crate) async fn queued_hold(sid: harness_core::SessionId) {
        wait_on(&QUEUED_HOLD, sid).await;
    }

    static PEERS_HOLD: Holds = std::sync::Mutex::new(Vec::new());

    /// Arm a hold for session `sid`'s next start of a called agent's turn
    /// (X.37): after the turn before it ended and freed the slot, before
    /// the next claims it, so a Stop or a message can be sent in that gap.
    pub fn hold_next_peer_start(sid: harness_core::SessionId) -> Arc<EndHold> {
        let h = Arc::new(EndHold {
            held: tokio::sync::Notify::new(),
            go: tokio::sync::Semaphore::new(0),
            is_held: Default::default(),
        });
        PEERS_HOLD.lock().unwrap().push((sid, h.clone()));
        h
    }

    pub(crate) async fn peers_hold(sid: harness_core::SessionId) {
        wait_on(&PEERS_HOLD, sid).await;
    }

    static PEERS_CLAIM_HOLD: Holds = std::sync::Mutex::new(Vec::new());

    /// Arm a hold for session `sid`'s next called agent, after it was taken
    /// off the list and before its turn claims the slot (X.37, review of
    /// PR 166, L6).
    pub fn hold_next_peer_claim(sid: harness_core::SessionId) -> Arc<EndHold> {
        let h = Arc::new(EndHold {
            held: tokio::sync::Notify::new(),
            go: tokio::sync::Semaphore::new(0),
            is_held: Default::default(),
        });
        PEERS_CLAIM_HOLD.lock().unwrap().push((sid, h.clone()));
        h
    }

    pub(crate) async fn peers_claim_hold(sid: harness_core::SessionId) {
        wait_on(&PEERS_CLAIM_HOLD, sid).await;
    }

    pub(crate) async fn end_hold(sid: harness_core::SessionId) {
        wait_on(&END_HOLD, sid).await;
    }

    async fn wait_on(holds: &Holds, sid: harness_core::SessionId) {
        let h = {
            let mut all = holds.lock().unwrap();
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
        // MCP servers' `env` values never reach an event (X.3). One that is
        // part of the redaction marker cannot be a secret and is never
        // redacted (PLAN.md T3): named once, by its variable.
        for (name, server) in &cfg.policy.mcp_servers {
            for (var, v) in &server.env.0 {
                if harness_core::secret::is_marker_part(v.expose()) {
                    tracing::warn!(
                        "[mcp_servers.{name}] env {var}: a value that is part of \"{}\" is not \
                         treated as a secret",
                        harness_core::secret::MARKER
                    );
                }
                cfg.secrets.add(v.clone());
            }
        }
        let project_mcp = match (
            &cfg.policy.mcp.project_servers,
            &cfg.policy.mcp.approvals_file,
        ) {
            (policy::ProjectServers::Ask, Some(file))
                if cfg.profile == harness_core::event::Profile::Agentic =>
            {
                Some(Arc::new(project_mcp::ProjectMcp::open(file.clone())))
            }
            (policy::ProjectServers::Ask, None)
                if cfg.profile == harness_core::event::Profile::Agentic =>
            {
                tracing::warn!(
                    "[mcp] project_servers = \"ask\", but this runtime keeps no approvals file: \
                     a repository's MCP servers stay off"
                );
                None
            }
            _ => None,
        };
        let rows = cfg.store.call(|s| s.sessions()).await?;
        let hosted: std::collections::HashSet<SessionId> =
            rows.iter().map(|r| r.session_id).collect();
        let stages = open_stages(&cfg, &hosted);
        let uploads = uploads::Staging::new(
            cfg.artifacts
                .as_ref()
                .filter(|_| {
                    cfg.profile == harness_core::event::Profile::Agentic
                        && cfg.policy.limits.upload_max_mb > 0
                        && cfg.policy.limits.uploads_per_message > 0
                })
                .map(|a| a.data_dir.join("uploads-stage")),
        );
        let bg_limits = cfg.policy.limits.background();
        let bg_secrets = cfg.secrets.clone();
        let audit_log = audit::AuditLog::start(cfg.store.clone());
        let inner = Arc::new(Inner {
            cfg,
            sessions: Mutex::new(HashMap::new()),
            restoring: Mutex::new(Vec::new()),
            link: Mutex::new(None),
            plugins: tokio::sync::OnceCell::new(),
            audit_log,
            stages,
            uploads,
            stopping: AtomicBool::new(false),
            background: harness_tools::background::Registry::new(bg_limits, bg_secrets),
            recalls: Default::default(),
            attacheds: Default::default(),
            docs_pending: Default::default(),
            boards: Default::default(),
            stored_skills: Default::default(),
            connectors: Default::default(),
            diffs: Arc::new(tokio::sync::Semaphore::new(DIFF_AT_ONCE)),
            file_lists: Arc::new(tokio::sync::Semaphore::new(DIFF_AT_ONCE)),
            bg_outputs: Arc::new(tokio::sync::Semaphore::new(DIFF_AT_ONCE)),
            previews: Arc::new(tokio::sync::Semaphore::new(1)),
            file_views: Arc::new(tokio::sync::Semaphore::new(1)),
            projects_made: Default::default(),
            project_slots: Arc::new(tokio::sync::Semaphore::new(1)),
            clone_slots: Arc::new(tokio::sync::Semaphore::new(1)),
            project_mcp,
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
        // MCP servers no call used for a while are stopped (X.3): every
        // chat stays loaded, so they would pile up otherwise.
        if !this.inner.cfg.policy.mcp_servers.is_empty() || this.inner.project_mcp.is_some() {
            let weak = Arc::downgrade(&this.inner);
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(harness_tools::background::PRUNE_EVERY).await;
                    let Some(inner) = weak.upgrade() else { break };
                    let sessions: Vec<Arc<Session>> = inner
                        .sessions
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .values()
                        .cloned()
                        .collect();
                    for s in sessions {
                        s.mcp_idle(harness_tools::mcp::IDLE_STOP);
                    }
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

    /// What this node announces for the Plugins screen (ADOPTION_PLAN
    /// X.34, PLAN.md T80): in an agentic node that offers the `skill` tool,
    /// the names and descriptions of the skills in its own node.toml
    /// `[skills] roots` (never a repository's: those belong to a session's
    /// folder), and the names of its enabled `[mcp_servers]` that node.toml
    /// does not deny. Names only: no path, command, argument or
    /// environment. Read once per runtime ([`Inner::plugins`]) in a
    /// blocking task with no time limit: a hung folder costs one thread and
    /// the announcement, never hello, the link or a second thread at a
    /// reconnect. Bounded by the protocol's caps.
    async fn plugins(&self) -> harness_protocol::NodePlugins {
        use harness_tools::{mcp, skills};
        let cfg = &self.inner.cfg;
        if cfg.profile != harness_core::event::Profile::Agentic {
            return Default::default();
        }
        let policy = &cfg.policy;
        let mcp_servers = policy
            .mcp_servers
            .iter()
            .filter(|(name, c)| c.enabled && !policy.denies(&format!("{}{name}__", mcp::PREFIX)))
            .map(|(name, _)| name.clone())
            .collect();
        let mut found = harness_protocol::NodePlugins {
            mcp_servers,
            ..Default::default()
        };
        if cfg.backend.has_tool(skills::NAME) && !policy.denies(skills::NAME) {
            let roots: Vec<skills::Root> = policy
                .skills
                .roots()
                .unwrap_or_default()
                .into_iter()
                .map(|path| skills::Root {
                    path,
                    origin: harness_core::skills::SkillOrigin::Node,
                    bound: None,
                })
                .collect();
            let protected = policy.protected_dirs.clone();
            let read = tokio::task::spawn_blocking(move || skills::discover(&roots, &protected));
            match read.await {
                Ok(f) => {
                    found.left_out = u32::try_from(f.catalogue.refused.len()).unwrap_or(u32::MAX);
                    found.skills = f
                        .catalogue
                        .skills
                        .into_iter()
                        .map(|s| harness_protocol::AnnouncedSkill {
                            name: s.name,
                            description: s.description,
                        })
                        .collect();
                }
                Err(e) => {
                    tracing::warn!(error = %e, "skills: the node's skill folders were not read for the master")
                }
            }
        }
        found.bounded()
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
            // It reads `deny_skills` and `deny_mcp_servers` (X.34), in
            // either profile: a chat-profile node runs neither.
            capabilities.push(harness_protocol::CAP_PLUGIN_ACCESS.into());
            // A person's Retry of a failed last turn (X.21), in chats of
            // either profile.
            capabilities.push(harness_protocol::CAP_RETRY.into());
            // A person's rewind of the conversation to an earlier message
            // and its undo (X.46), in chats of either profile.
            capabilities.push(harness_protocol::CAP_REWIND.into());
            // The owner's bookmark on a message (docs/pins.md), in chats
            // of either profile.
            capabilities.push(harness_protocol::CAP_PIN.into());
            // A person's fork of a chat at a finished turn (C.2), in chats
            // of either profile.
            capabilities.push(harness_protocol::CAP_FORK.into());
            // The agents a message called after the first, asked one after
            // another (X.37).
            capabilities.push(harness_protocol::CAP_PEERS.into());
            // `remember` where this node offers it: registered, and not
            // refused by its own policy (the master files nothing otherwise).
            if self.inner.cfg.backend.has_tool("remember")
                && !self.inner.cfg.policy.denies("remember")
            {
                capabilities.push(harness_protocol::CAP_REMEMBER.into());
            }
            // `recall` likewise (PLAN.md T55): the master answers searches
            // only from a node that offers it.
            if self.inner.cfg.backend.has_tool("recall") && !self.inner.cfg.policy.denies("recall")
            {
                capabilities.push(harness_protocol::CAP_RECALL.into());
            }
            // `attached_file` likewise (PLAN.md T95): the master keeps a
            // plain chat's file, and answers reads of it, only for a node
            // that offers the tool.
            if self
                .inner
                .cfg
                .backend
                .has_tool(harness_core::attached::TOOL)
                && !self.inner.cfg.policy.denies(harness_core::attached::TOOL)
            {
                capabilities.push(harness_protocol::CAP_ATTACHED.into());
            }
            // `doc_create` and `sheet_create` likewise (PLAN.md T97): the
            // master makes and keeps a plain chat's file only for a node that
            // offers the tools.
            if harness_core::docs::TOOLS
                .iter()
                .any(|t| self.inner.cfg.backend.has_tool(t) && !self.inner.cfg.policy.denies(t))
            {
                capabilities.push(harness_protocol::CAP_DOCS.into());
            }
            // The board tools likewise (PLAN.md T78): the master answers
            // board requests only from a node that offers one of them.
            let offers =
                |t: &str| self.inner.cfg.backend.has_tool(t) && !self.inner.cfg.policy.denies(t);
            if offers(harness_core::board::READ_TOOL) || offers(harness_core::board::EDIT_TOOL) {
                capabilities.push(harness_protocol::CAP_BOARD.into());
            }
            // Stored skills (X.34 step B, PLAN.md T81), in either profile:
            // where it offers `skill` and node.toml `[skills] from_master`
            // is on. Without it the master sends no list and answers no
            // request.
            if offers(harness_tools::skills::NAME) && self.inner.cfg.policy.skills.from_master {
                capabilities.push(harness_protocol::CAP_STORED_SKILLS.into());
            }
            // Custom connectors (X.35 step 2, PLAN.md T85), in either
            // profile: where node.toml `[mcp] master_connectors` is on and
            // `denied_tools` does not deny every MCP tool. Without it the
            // master sends no list and answers no call.
            if self.inner.cfg.policy.mcp.master_connectors
                && !self.inner.cfg.policy.denies(harness_core::agent::MCP_ALL)
            {
                capabilities.push(harness_protocol::CAP_CONNECTORS.into());
            }
            if self.inner.cfg.profile == harness_core::event::Profile::Agentic {
                // The modes it offers and their gaps (N.10a), as decided at
                // start.
                capabilities.extend(self.inner.cfg.policy.exec.capabilities());
                // Its approval modes (T50): the ceiling, and what a new chat
                // gets.
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
                capabilities.push(harness_protocol::CAP_ANSWERS.into());
                // The owner's switch of a chat's execution mode (N.10b).
                capabilities.push(harness_protocol::CAP_EXEC_SWITCH.into());
                // Its step limit (X.14): the most a chat's own may be.
                capabilities.push(format!(
                    "{}{}",
                    harness_protocol::CAP_STEPS_MAX_PREFIX,
                    p.limits.max_steps
                ));
                capabilities.push(harness_protocol::CAP_BACKGROUND.into());
                // A turn's changes, read from its checkpoints (X.12).
                capabilities.push(harness_protocol::CAP_DIFF.into());
                // ...and the whole chat's changes in one diff (X.61).
                capabilities.push(harness_protocol::CAP_DIFF_ALL.into());
                // The tail of a background command's output for the
                // chat's strip (X.62).
                capabilities.push(harness_protocol::CAP_BG_OUTPUT.into());
                // `@` file mentions and their completion (T63).
                capabilities.push(harness_protocol::CAP_FILES.into());
                // A text file of a folder for the page's panel (X.63).
                capabilities.push(harness_protocol::CAP_FOLDER_FILE.into());
                // A Word or Excel file of a folder for the page's panel
                // (X.59): only where the helper can be started.
                if docs::shared_helper().is_some() {
                    capabilities.push(harness_protocol::CAP_DOC_PREVIEW.into());
                    // A person's text changes saved as a copy (X.60): only
                    // where the agent's own editing tools are on offer, since
                    // node.toml's say over them (a denial, or the plan ceiling) is the say
                    // over this.
                    let on = |t: &str| docs::page_edit_usable(&self.inner.cfg, t);
                    if on(harness_core::docs::EDIT_DOC_TOOL)
                        || on(harness_core::docs::EDIT_SHEET_TOOL)
                    {
                        capabilities.push(harness_protocol::CAP_DOC_PAGE_EDIT.into());
                    }
                }
                // New project folders (X.26, T77): only where node.toml's
                // projects folder passed its checks at start. Never the path.
                if self.inner.cfg.policy.projects.located.is_some()
                    && self.inner.cfg.policy.limits.projects_per_hour > 0
                {
                    capabilities.push(harness_protocol::CAP_PROJECTS.into());
                    // Clones into new project folders (X.27, T86): only
                    // where node.toml lets it clone from some host. The
                    // time limit, so the master waits long enough; never
                    // the hosts.
                    if self.inner.cfg.policy.forge.clones() {
                        capabilities.push(format!(
                            "{}{}",
                            harness_protocol::CAP_FORGE_CLONE_PREFIX,
                            self.inner.cfg.policy.limits.clone_timeout_s
                        ));
                    }
                }
                // A repository's MCP servers (X.36, T82): only with node.toml
                // `[mcp] project_servers = "ask"`.
                if self.inner.project_mcp.is_some() {
                    capabilities.push(harness_protocol::CAP_PROJECT_MCP.into());
                }
                // Hand-off notes (X.16): the master asks once a chat idles.
                // Off is silent, as for callbacks: with
                // `handoff_notes_per_hour = 0` the master asks nothing.
                if self.inner.cfg.policy.limits.handoff_notes_per_hour > 0 {
                    capabilities.push(harness_protocol::CAP_HANDOFF.into());
                }
                capabilities.push(harness_protocol::CAP_COMPACT.into());
                // Files the model cannot read as text, saved into the chat's
                // folder (T94): only with a staging folder, and the node's
                // own numbers, so the master refuses what this node would.
                if self.inner.uploads.enabled() {
                    let l = &self.inner.cfg.policy.limits;
                    capabilities.push(harness_protocol::CAP_UPLOADS.into());
                    capabilities.push(format!(
                        "{}{}",
                        harness_protocol::CAP_UPLOAD_MAX_PREFIX,
                        l.upload_max_mb
                    ));
                    capabilities.push(format!(
                        "{}{}",
                        harness_protocol::CAP_UPLOADS_PER_MESSAGE_PREFIX,
                        l.uploads_per_message
                    ));
                }
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
            // What it has for the Plugins screen (X.34): sent once it is
            // read (the first link reads it, later ones reuse it), never
            // holding hello or the link up.
            let announce = out.clone();
            let this = self.clone();
            tokio::spawn(async move {
                let plugins = this
                    .inner
                    .plugins
                    .get_or_init(|| this.plugins())
                    .await
                    .clone();
                let _ = announce.try_send(NodeToMaster::Plugins { plugins });
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
                    // The answer to a search a tool call waits for.
                    MasterToNode::RecallResult {
                        req_id,
                        hits,
                        error,
                    } => this.inner.recalls.answer(&req_id, hits, error),
                    // Likewise a file read's answer (PLAN.md T95).
                    MasterToNode::AttachedResult {
                        req_id,
                        answer,
                        error,
                    } => this.inner.attacheds.answer(&req_id, answer, error),
                    // Likewise a made file's answer (PLAN.md T97).
                    MasterToNode::DocsResult {
                        req_id,
                        answer,
                        error,
                    } => this.inner.docs_pending.answer(&req_id, answer, error),
                    // Likewise a board call's answer.
                    MasterToNode::BoardResult {
                        req_id,
                        answer,
                        error,
                    } => this.inner.boards.answer(&req_id, answer, error),
                    // Likewise a stored skill's body (X.34 step B).
                    MasterToNode::SkillResult {
                        req_id,
                        body,
                        error,
                    } => this.inner.stored_skills.answer(&req_id, body, error),
                    // Likewise a connector call's answer (X.35 step 2).
                    MasterToNode::ConnectorResult {
                        req_id,
                        answer,
                        error,
                    } => this.inner.connectors.answer(&req_id, answer, error),
                    // Likewise a turn's diff (X.12): a read of two
                    // checkpoints, in a task of its own, never behind a
                    // command of the session (a restore holds its lane).
                    MasterToNode::Diff {
                        req_id,
                        session_id,
                        turn_id,
                        rewind,
                        all,
                    } => this.diff(req_id, session_id, turn_id, rewind, all),
                    // A background command's output for the strip (X.62): a
                    // bounded read of the ring, never behind a command of
                    // the session.
                    MasterToNode::BackgroundOutput {
                        req_id,
                        session_id,
                        bg_id,
                    } => this.background_output(req_id, session_id, bg_id),
                    // The composer's completion (T63): a bounded walk of
                    // names, in a task of its own, nothing logged.
                    MasterToNode::Files {
                        req_id,
                        session_id,
                        query,
                    } => this.files(req_id, session_id, query),
                    // A file for the page's panel (X.59): a task of its own,
                    // answered by one `session.doc_preview_result`; no event.
                    MasterToNode::DocPreview {
                        req_id,
                        session_id,
                        path,
                        sheet,
                    } => this.doc_preview(req_id, session_id, path, sheet),
                    // A text file for the page's panel (X.63): the same kind
                    // of task, answered by one `session.folder_file_result`.
                    MasterToNode::FolderFile {
                        req_id,
                        session_id,
                        path,
                    } => this.folder_file(req_id, session_id, path),
                    // A person's text changes to such a file (X.60): the same
                    // kind of task, answered by one
                    // `session.doc_page_edit_result`; no event.
                    MasterToNode::DocPageEdit {
                        req_id,
                        session_id,
                        path,
                        sheet,
                        changes,
                    } => this.doc_page_edit(req_id, session_id, path, sheet, changes),
                    // A new project's folder (T77): no session, its own
                    // task, answered by one `project.created`.
                    MasterToNode::ProjectCreate { req_id, name } => {
                        this.create_project(req_id, name)
                    }
                    // A new project's folder cloned from a forge (T86):
                    // no session, its own task, answered by one
                    // `project.created`.
                    MasterToNode::ProjectClone {
                        req_id,
                        name,
                        host,
                        full_name,
                        username,
                        token,
                    } => this.clone_project(
                        req_id,
                        forge_clone::CloneRequest {
                            name,
                            host,
                            full_name,
                            username,
                            token,
                        },
                    ),
                    // A person's request about a project's `.mcp.json`
                    // (X.36): no session, its own task.
                    MasterToNode::ProjectMcp {
                        req_id,
                        user_id,
                        folder,
                        request,
                    } => this.project_mcp(req_id, user_id, folder, request),
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

    /// For tests: raise or lower the mark a chat carries while its
    /// execution mode is being switched, as `set_exec_mode` does around its
    /// own work (the window a real switch holds open only while MCP
    /// servers stop). Not an API.
    #[cfg(feature = "testing")]
    pub fn hold_exec_switch(&self, sid: SessionId, on: bool) {
        if let Some(s) = self.inner.session(sid) {
            s.exec_switching
                .store(on, std::sync::atomic::Ordering::SeqCst);
        }
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

    /// One chunk of a file the master pushed for a prompt (T94): checked
    /// against this node's own limits and the chat's kind, then staged.
    /// The node decides: nothing the chunk says widens `[limits]`, and a
    /// chat with no folder takes none.
    async fn upload_chunk(
        &self,
        sid: SessionId,
        upload_id: String,
        offset: u64,
        size: u64,
        sha256: String,
        data: String,
    ) -> Result<(), String> {
        use base64::Engine as _;
        let Some(s) = self.session(sid) else {
            return Err("unknown session".into());
        };
        s.accepts_uploads()?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data.as_bytes())
            .map_err(|_| "the chunk is not base64".to_string())?;
        let max = self
            .inner
            .cfg
            .policy
            .limits
            .upload_max_mb
            .saturating_mul(1 << 20);
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            inner
                .uploads
                .chunk(sid, &upload_id, offset, size, &sha256, &bytes, max)
        })
        .await
        .map_err(|e| format!("staging failed: {e}"))?
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

    /// Answer `session.diff` (X.12) from a task of its own; never acked:
    /// the result is the answer. Nothing is logged or stored: a diff is a
    /// view of two checkpoints the log already names. A stopping node
    /// still answers (it starts no work).
    fn diff(&self, req_id: String, sid: SessionId, turn_id: String, rewind: bool, all: bool) {
        // The node's own record that its master read a chat's files (the
        // session id only, never a path or a line).
        self.audit_inline("session.diff", sid);
        let Ok(permit) = self.inner.diffs.clone().try_acquire_owned() else {
            return self.inner.publish(NodeToMaster::DiffResult {
                req_id,
                diff: None,
                error: Some(DIFF_BUSY.into()),
            });
        };
        let inner = self.inner.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let res = match inner.session(sid) {
                Some(s) if all => s.diff_all().await,
                Some(s) => s.diff(&turn_id, rewind).await,
                None => Err("unknown session".to_string()),
            };
            let msg = match res {
                Ok(d) => NodeToMaster::DiffResult {
                    req_id,
                    diff: Some(Box::new(d)),
                    error: None,
                },
                Err(e) => NodeToMaster::DiffResult {
                    req_id,
                    diff: None,
                    error: Some(e),
                },
            };
            // In order, waiting for room: an answer is never dropped for a
            // full queue (the master's request would only time out).
            inner.publish_wait(msg).await;
        });
    }

    /// Answer `session.background_output` (X.62): one bounded read of the
    /// ring `bash_output` reads, from a task of its own; never acked, no
    /// event, nothing stored, and no audit line: it is the owner reading
    /// their own chat's output, as reading its events is not audited either
    /// (a row polls every 3 s: an audit row each would be 29,000 a day).
    /// At most [`DIFF_AT_ONCE`] are answered at once, in places of its own
    /// (not `@` completion's); past them the answer is `busy`, which is
    /// not final.
    fn background_output(&self, req_id: String, sid: SessionId, bg_id: String) {
        let reply = |output, error| NodeToMaster::BackgroundOutputResult {
            req_id: req_id.clone(),
            output,
            error,
            busy: false,
        };
        let Ok(permit) = self.inner.bg_outputs.clone().try_acquire_owned() else {
            return self.inner.publish(NodeToMaster::BackgroundOutputResult {
                req_id,
                output: None,
                error: None,
                busy: true,
            });
        };
        let res = match self.inner.session(sid) {
            Some(s) => s.background_output(&bg_id),
            // Not loaded: the node restarted since, and a command ends with
            // its node.
            None => Err(
                "this chat is not running on the node now (it restarted): its \
                         background commands ended with it"
                    .to_string(),
            ),
        };
        let msg = match res {
            Ok(o) => reply(Some(o), None),
            Err(e) => reply(None, Some(e)),
        };
        let inner = self.inner.clone();
        tokio::spawn(async move {
            let _permit = permit;
            inner.publish_wait(msg).await;
        });
    }

    /// The two request places `@` completion and a background output read
    /// each have (for the test that they are not the same).
    #[doc(hidden)]
    pub fn request_places(&self) -> (Arc<tokio::sync::Semaphore>, Arc<tokio::sync::Semaphore>) {
        (self.inner.bg_outputs.clone(), self.inner.file_lists.clone())
    }

    /// Answer `session.files` (T63) from a task of its own; never acked:
    /// the result is the answer. Nothing is logged but the node's audit
    /// line (the session id only). At most [`DIFF_AT_ONCE`] at once,
    /// beside the diffs' own room.
    fn files(&self, req_id: String, sid: SessionId, query: String) {
        self.audit_inline("session.files", sid);
        let Ok(permit) = self.inner.file_lists.clone().try_acquire_owned() else {
            return self.inner.publish(NodeToMaster::FilesResult {
                req_id,
                paths: vec![],
                error: Some("the node is listing files for other requests; try again".into()),
            });
        };
        let inner = self.inner.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let res = match inner.session(sid) {
                Some(s) => s.complete_files(query).await,
                None => Err("unknown session".to_string()),
            };
            let msg = match res {
                Ok(paths) => NodeToMaster::FilesResult {
                    req_id,
                    paths,
                    error: None,
                },
                Err(e) => NodeToMaster::FilesResult {
                    req_id,
                    paths: vec![],
                    error: Some(e),
                },
            };
            inner.publish_wait(msg).await;
        });
    }

    /// Answer `session.doc_preview` (X.59, T99) from a task of its own; never
    /// acked, nothing logged but the node's audit line (the session id only,
    /// never the path), no event. One at a time on the node.
    fn doc_preview(&self, req_id: String, sid: SessionId, path: String, sheet: Option<u32>) {
        use harness_protocol::PreviewRefusal as R;
        self.audit_inline("session.doc_preview", sid);
        let refuse = |inner: &Arc<Inner>, req_id: String, r: R| {
            inner.publish(NodeToMaster::DocPreviewResult {
                req_id,
                preview: None,
                refusal: Some(r),
            })
        };
        let Ok(permit) = self.inner.previews.clone().try_acquire_owned() else {
            return refuse(&self.inner, req_id, R::Busy);
        };
        let inner = self.inner.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let res = match inner.session(sid) {
                Some(s) => s.preview_folder_file(&path, sheet).await,
                None => Err(R::NotFound),
            };
            let msg = match res {
                Ok(v) => NodeToMaster::DocPreviewResult {
                    req_id,
                    preview: Some(v),
                    refusal: None,
                },
                Err(r) => NodeToMaster::DocPreviewResult {
                    req_id,
                    preview: None,
                    refusal: Some(r),
                },
            };
            inner.publish_wait(msg).await;
        });
    }

    /// Answer `session.folder_file` (X.63, T99) from a task of its own; never
    /// acked, nothing logged but the node's audit line (the session id only,
    /// never the path), no event. One at a time on the node, in a place of its
    /// own: a text file's read is not held up by a document's helper.
    fn folder_file(&self, req_id: String, sid: SessionId, path: String) {
        use harness_protocol::FileRefusal as R;
        self.audit_inline("session.folder_file", sid);
        let refuse = |inner: &Arc<Inner>, req_id: String, r: R| {
            inner.publish(NodeToMaster::FolderFileResult {
                req_id,
                file: None,
                refusal: Some(r),
            })
        };
        let Ok(permit) = self.inner.file_views.clone().try_acquire_owned() else {
            return refuse(&self.inner, req_id, R::Busy);
        };
        let inner = self.inner.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let res = match inner.session(sid) {
                Some(s) => s.view_folder_file(&path).await,
                None => Err(R::NotFound),
            };
            let msg = match res {
                Ok(f) => NodeToMaster::FolderFileResult {
                    req_id,
                    file: Some(f),
                    refusal: None,
                },
                Err(r) => NodeToMaster::FolderFileResult {
                    req_id,
                    file: None,
                    refusal: Some(r),
                },
            };
            inner.publish_wait(msg).await;
        });
    }

    /// Answer `session.doc_page_edit` (X.60, T100) from a task of its own;
    /// never acked, nothing logged but the node's audit line (the session id
    /// only: never the path, a change or the new file's name), no event. It
    /// takes the same one place as the preview, so a person's previews and
    /// edits together hold at most one of the helper's two places. The
    /// request is bounded here again, whatever the master checked.
    fn doc_page_edit(
        &self,
        req_id: String,
        sid: SessionId,
        path: String,
        sheet: Option<u32>,
        changes: Vec<harness_protocol::PageChange>,
    ) {
        use harness_protocol::PreviewRefusal as R;
        self.audit_inline("session.doc_page_edit", sid);
        let refuse = |inner: &Arc<Inner>, req_id: String, r: R| {
            inner.publish(NodeToMaster::DocPageEditResult {
                req_id,
                saved: None,
                results: vec![],
                refusal: Some(r),
            })
        };
        if path.len() > harness_protocol::MAX_PREVIEW_PATH_BYTES
            || changes.is_empty()
            || changes.len() > harness_protocol::MAX_PAGE_EDIT_CHANGES
            || changes.iter().any(|c| {
                c.at.len() > harness_protocol::MAX_PAGE_EDIT_AT_BYTES
                    || c.text.len() > harness_protocol::MAX_PAGE_EDIT_TEXT_BYTES
            })
        {
            return refuse(&self.inner, req_id, R::NotFound);
        }
        let Ok(permit) = self.inner.previews.clone().try_acquire_owned() else {
            return refuse(&self.inner, req_id, R::Busy);
        };
        let inner = self.inner.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let res = match inner.session(sid) {
                Some(s) => s.edit_folder_file(&path, sheet, changes).await,
                None => Err(R::NotFound),
            };
            let msg = match res {
                Ok(done) => NodeToMaster::DocPageEditResult {
                    req_id,
                    saved: done.path,
                    results: done.results,
                    refusal: None,
                },
                Err(r) => NodeToMaster::DocPageEditResult {
                    req_id,
                    saved: None,
                    results: vec![],
                    refusal: Some(r),
                },
            };
            inner.publish_wait(msg).await;
        });
    }

    /// Answer `project.create` (X.26, PLAN.md T77) from a task of its own;
    /// never acked: `project.created` is the answer. Only an agentic node
    /// whose node.toml names a usable projects folder makes one; one at a
    /// time, and at most `[limits] projects_per_hour`. The node's audit
    /// line has the outcome and the folder made (on this node's own log).
    fn create_project(&self, req_id: String, name: String) {
        let refuse = |error: String| NodeToMaster::ProjectCreated {
            req_id: req_id.clone(),
            path: None,
            error: Some(error),
        };
        if self.inner.cfg.profile != harness_core::event::Profile::Agentic {
            return self.inner.publish(refuse(projects::OFF.into()));
        }
        let Ok(permit) = self.inner.project_slots.clone().try_acquire_owned() else {
            return self.inner.publish(refuse(
                "this node is making another project's folder; try again".into(),
            ));
        };
        let inner = self.inner.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let res = projects::create(
                &inner.cfg.policy,
                &inner.projects_made,
                &name,
                &projects::suffix,
            )
            .await;
            let detail = match &res {
                Ok(p) => serde_json::json!({ "ok": true, "folder": p }),
                Err(_) => serde_json::json!({ "ok": false }),
            };
            inner.audit_log.record("project.create", detail.to_string());
            let msg = match res {
                Ok(p) => NodeToMaster::ProjectCreated {
                    req_id,
                    path: Some(p.to_string_lossy().into_owned()),
                    error: None,
                },
                Err(e) => NodeToMaster::ProjectCreated {
                    req_id,
                    path: None,
                    error: Some(e),
                },
            };
            inner.publish_wait(msg).await;
        });
    }

    /// Answer `project.clone` (X.27 step 3, PLAN.md T86) from a task of its
    /// own; never acked: `project.created` is the answer. Only an agentic
    /// node whose node.toml lets it clone and names a usable projects
    /// folder; one clone at a time. The audit line has the outcome, the
    /// host and repository (when they are of the shape cloned), whether a
    /// credential came, and the folder made; never the credential.
    fn clone_project(&self, req_id: String, req: forge_clone::CloneRequest) {
        let refuse = |error: String| NodeToMaster::ProjectCreated {
            req_id: req_id.clone(),
            path: None,
            error: Some(error),
        };
        if self.inner.cfg.profile != harness_core::event::Profile::Agentic {
            return self.inner.publish(refuse(forge_clone::OFF.into()));
        }
        let Ok(permit) = self.inner.clone_slots.clone().try_acquire_owned() else {
            return self.inner.publish(refuse(
                "this node is cloning another repository; try again when it has finished".into(),
            ));
        };
        let inner = self.inner.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let mut detail = serde_json::json!({ "credential": req.token.is_some() });
            if crate::policy::valid_forge_host(&req.host)
                && forge_clone::valid_full_name(&req.full_name)
            {
                detail["host"] = req.host.clone().into();
                detail["full_name"] = req.full_name.clone().into();
            }
            let bounds = forge_clone::Bounds::of(&inner.cfg.policy);
            let res = forge_clone::clone(
                &inner.cfg.policy,
                &inner.projects_made,
                req,
                bounds,
                &projects::suffix,
            )
            .await;
            detail["ok"] = res.is_ok().into();
            if let Ok(p) = &res {
                detail["folder"] = serde_json::json!(p);
            }
            inner.audit_log.record("project.clone", detail.to_string());
            let msg = match res {
                Ok(p) => NodeToMaster::ProjectCreated {
                    req_id,
                    path: Some(p.to_string_lossy().into_owned()),
                    error: None,
                },
                Err(e) => NodeToMaster::ProjectCreated {
                    req_id,
                    path: None,
                    error: Some(e),
                },
            };
            inner.publish_wait(msg).await;
        });
    }

    /// Answer `project.mcp` (X.36, PLAN.md T82) from a task of its own;
    /// never acked: `project.mcp_result` is the answer. Refused unless
    /// node.toml says `[mcp] project_servers = "ask"`; at most
    /// `project_mcp::AT_ONCE` at a time on this node. An approval taken or
    /// removed gets an audit line (ids and the server's name, never the
    /// entry's environment).
    fn project_mcp(
        &self,
        req_id: String,
        user_id: String,
        folder: String,
        request: harness_protocol::ProjectMcpRequest,
    ) {
        let refuse = |error: String| NodeToMaster::ProjectMcpResult {
            req_id: req_id.clone(),
            list: None,
            error: Some(error),
        };
        let Some(pm) = self.inner.project_mcp.clone() else {
            return self.inner.publish(refuse(project_mcp::OFF.into()));
        };
        let Ok(permit) = pm.busy.clone().try_acquire_owned() else {
            return self.inner.publish(refuse(
                "this node is answering other requests about repository servers; try again".into(),
            ));
        };
        let inner = self.inner.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let (u, f, r) = (user_id.clone(), folder.clone(), request);
            let pm2 = pm.clone();
            let inner2 = inner.clone();
            let work =
                tokio::task::spawn_blocking(move || pm2.handle(&inner2.cfg.policy, &u, &f, &r));
            let out = match tokio::time::timeout(project_mcp::WAIT, work).await {
                Ok(Ok(o)) => o,
                Ok(Err(e)) => project_mcp::Outcome {
                    list: Err(format!("the node failed to answer: {e}")),
                    changed: None,
                },
                Err(_) => project_mcp::Outcome {
                    list: Err("the node could not read the project's folder in time".into()),
                    changed: None,
                },
            };
            // Every chat of the folder takes up what the request changed
            // now (an approval removed, an entry gone from the file): its
            // server stops at once, not at that chat's next turn.
            if let Ok(real) = inner.cfg.policy.check_cwd(&folder) {
                let chats: Vec<Arc<Session>> = inner
                    .sessions
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .values()
                    .filter(|s| s.table_folder() == Some(real.as_path()))
                    .cloned()
                    .collect();
                for s in chats {
                    s.sync_project_mcp();
                }
            }
            if let Some((what, name)) = &out.changed {
                let detail = serde_json::json!({
                    "user": user_id,
                    "folder": folder,
                    "server": name,
                });
                tracing::info!(user = %user_id, folder = %folder.escape_debug(), server = %name, "project MCP server {what}d");
                let kind = if *what == "approve" {
                    "project_mcp.approve"
                } else {
                    "project_mcp.remove"
                };
                inner.audit_log.record(kind, detail.to_string());
            }
            let msg = match out.list {
                Ok(l) => NodeToMaster::ProjectMcpResult {
                    req_id,
                    list: Some(Box::new(l)),
                    error: None,
                },
                Err(e) => NodeToMaster::ProjectMcpResult {
                    req_id,
                    list: None,
                    error: Some(e),
                },
            };
            inner.publish_wait(msg).await;
        });
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
            MasterToNode::Fork {
                session_id,
                parent,
                turn_id,
                title,
                ..
            } => {
                self.audit_with(
                    "session.fork",
                    session_id,
                    serde_json::json!({ "parent": parent }),
                )
                .await;
                self.fork(session_id, parent, &turn_id, &title).await
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
            MasterToNode::Prompt(PromptMsg {
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
                files,
                owner,
                identity,
                peers,
                tags,
                uploads,
                attached,
                ..
            }) => {
                self.audit("session.prompt", session_id).await;
                // The master's context: this message's part, and the
                // standing parts the loop keeps as sections (X.1).
                let (context, standing) = match context {
                    Some(c) => (c.turn, Some(c.standing)),
                    None => (None, None),
                };
                let extra = harness_core::backend::TurnExtra {
                    savings: savings.unwrap_or_default(),
                    context,
                    standing,
                    ..Default::default()
                };
                let input = session::Input {
                    text,
                    attachments,
                    reply,
                    images,
                    files,
                    uploads,
                    attached,
                };
                match self.session(session_id) {
                    Some(s) => match learn_owner(&s, owner, identity).await {
                        Ok(()) => {
                            s.prompt(input, agent, agent_named, compare, extra, peers, tags)
                                .await
                        }
                        Err(e) => Err(e),
                    },
                    None => Err("unknown session".into()),
                }
            }
            MasterToNode::UploadChunk {
                session_id,
                upload_id,
                offset,
                size,
                sha256,
                data,
                ..
            } => {
                if offset == 0 {
                    // Once a file, never per chunk (the audit stays small).
                    self.audit("session.upload", session_id).await;
                }
                self.upload_chunk(session_id, upload_id, offset, size, sha256, data)
                    .await
            }
            MasterToNode::Callback(CallbackMsg {
                session_id,
                bg_ids,
                agent,
                savings,
                context,
                owner,
                identity,
                ..
            }) => {
                self.audit("session.callback", session_id).await;
                // The standing parts only; this turn's own line never.
                let standing = context.map(|c| c.standing);
                match self.session(session_id) {
                    Some(s) => match learn_owner(&s, owner, identity).await {
                        Ok(()) => {
                            s.callback(bg_ids, agent, savings.unwrap_or_default(), standing)
                                .await
                        }
                        Err(e) => Err(e),
                    },
                    None => Err("unknown session".into()),
                }
            }
            MasterToNode::Retry(RetryMsg {
                session_id,
                turn_id,
                agent,
                savings,
                context,
                agent_named,
                owner,
                identity,
                ..
            }) => {
                self.audit("session.retry", session_id).await;
                let (context, standing) = match context {
                    Some(c) => (c.turn, Some(c.standing)),
                    None => (None, None),
                };
                let extra = harness_core::backend::TurnExtra {
                    savings: savings.unwrap_or_default(),
                    context,
                    standing,
                    ..Default::default()
                };
                match self.session(session_id) {
                    Some(s) => match learn_owner(&s, owner, identity).await {
                        Ok(()) => s.retry(&turn_id, agent, agent_named, extra).await,
                        Err(e) => Err(e),
                    },
                    None => Err("unknown session".into()),
                }
            }
            MasterToNode::Rewind {
                session_id,
                message_seq,
                files,
                force,
                ..
            } => {
                self.audit("session.rewind", session_id).await;
                match self.session(session_id) {
                    Some(s) => s.rewind(message_seq, files, force).await,
                    None => Err("unknown session".into()),
                }
            }
            MasterToNode::RewindUndo {
                session_id,
                rewind_seq,
                ..
            } => {
                self.audit("session.rewind_undo", session_id).await;
                match self.session(session_id) {
                    Some(s) => s.rewind_undo(rewind_seq).await,
                    None => Err("unknown session".into()),
                }
            }
            MasterToNode::Pin {
                session_id,
                message_seq,
                pinned,
                ..
            } => {
                self.audit("session.pin", session_id).await;
                match self.session(session_id) {
                    Some(s) => s.pin_message(message_seq, pinned).await,
                    None => Err("unknown session".into()),
                }
            }
            MasterToNode::Handoff {
                session_id,
                small_model,
                windows,
                low_effort,
                ..
            } => {
                // It starts a model call: audited like a callback.
                self.audit("session.handoff", session_id).await;
                match self.session(session_id) {
                    Some(s) => s.handoff(small_model, windows, low_effort).await,
                    None => Err("unknown session".into()),
                }
            }
            MasterToNode::Compact {
                session_id,
                focus,
                windows,
                ..
            } => {
                // It starts a model call: audited like a hand-off note.
                self.audit("session.compact", session_id).await;
                match self.session(session_id) {
                    Some(s) => s.compact(focus, windows).await,
                    None => Err("unknown session".into()),
                }
            }
            // Nothing is written: the master keeps the pick; refused while
            // the chat works.
            MasterToNode::SetAgent { session_id, .. } => {
                self.audit("session.set_agent", session_id).await;
                match self.session(session_id) {
                    Some(s) => s.set_agent(),
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
            // change made while a turn runs applies from its next call (T50).
            MasterToNode::SetApprovalMode {
                session_id,
                mode,
                lowering,
                ..
            } => {
                self.audit("session.set_approval_mode", session_id).await;
                match self.session(session_id) {
                    Some(s) => s.set_mode(mode, lowering).await.map(|_| ()),
                    None => Err("unknown session".into()),
                }
            }
            // In the session's lane, which a running turn does not hold: the
            // session itself refuses while anything of the chat runs, and
            // decides again what this node offers (N.10b). Audited like the
            // create that asked for the mode.
            MasterToNode::SetExecMode {
                session_id, mode, ..
            } => {
                self.audit_with(
                    "session.set_exec_mode",
                    session_id,
                    serde_json::json!({ "exec_mode": mode.id() }),
                )
                .await;
                match self.session(session_id) {
                    Some(s) => s.set_exec_mode(mode).await,
                    None => Err("unknown session".into()),
                }
            }
            // As the mode: in the session's lane; the running turn reads
            // the limit before each request (X.14).
            MasterToNode::SetStepLimit {
                session_id,
                max_steps,
                lowering,
                ..
            } => {
                self.audit("session.set_step_limit", session_id).await;
                match self.session(session_id) {
                    Some(s) => s.set_step_limit(max_steps, lowering).await.map(|_| ()),
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
                note,
                ..
            } => match self.session(session_id) {
                Some(s) => s.respond(&approval_id, decision, note),
                None => Err("unknown session".into()),
            },
            MasterToNode::RevokeAllow {
                session_id,
                grant_id,
                ..
            } => {
                self.audit("session.revoke_allow", session_id).await;
                match self.session(session_id) {
                    Some(s) => s.revoke_allow(&grant_id).await,
                    None => Err("unknown session".into()),
                }
            }
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
            MasterToNode::RecallResult {
                req_id,
                hits,
                error,
            } => {
                self.inner.recalls.answer(&req_id, hits, error);
                Ok(())
            }
            MasterToNode::AttachedResult {
                req_id,
                answer,
                error,
            } => {
                self.inner.attacheds.answer(&req_id, answer, error);
                Ok(())
            }
            MasterToNode::DocsResult {
                req_id,
                answer,
                error,
            } => {
                self.inner.docs_pending.answer(&req_id, answer, error);
                Ok(())
            }
            MasterToNode::BoardResult {
                req_id,
                answer,
                error,
            } => {
                self.inner.boards.answer(&req_id, answer, error);
                Ok(())
            }
            MasterToNode::SkillResult {
                req_id,
                body,
                error,
            } => {
                self.inner.stored_skills.answer(&req_id, body, error);
                Ok(())
            }
            MasterToNode::ConnectorResult {
                req_id,
                answer,
                error,
            } => {
                self.inner.connectors.answer(&req_id, answer, error);
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
            MasterToNode::Diff {
                req_id,
                session_id,
                turn_id,
                rewind,
                all,
            } => {
                self.diff(req_id, session_id, turn_id, rewind, all);
                Ok(())
            }
            MasterToNode::BackgroundOutput {
                req_id,
                session_id,
                bg_id,
            } => {
                self.background_output(req_id, session_id, bg_id);
                Ok(())
            }
            MasterToNode::Files {
                req_id,
                session_id,
                query,
            } => {
                self.files(req_id, session_id, query);
                Ok(())
            }
            MasterToNode::DocPreview {
                req_id,
                session_id,
                path,
                sheet,
            } => {
                self.doc_preview(req_id, session_id, path, sheet);
                Ok(())
            }
            MasterToNode::FolderFile {
                req_id,
                session_id,
                path,
            } => {
                self.folder_file(req_id, session_id, path);
                Ok(())
            }
            MasterToNode::DocPageEdit {
                req_id,
                session_id,
                path,
                sheet,
                changes,
            } => {
                self.doc_page_edit(req_id, session_id, path, sheet, changes);
                Ok(())
            }
            MasterToNode::ProjectCreate { req_id, name } => {
                self.create_project(req_id, name);
                Ok(())
            }
            MasterToNode::ProjectClone {
                req_id,
                name,
                host,
                full_name,
                username,
                token,
            } => {
                self.clone_project(
                    req_id,
                    forge_clone::CloneRequest {
                        name,
                        host,
                        full_name,
                        username,
                        token,
                    },
                );
                Ok(())
            }
            MasterToNode::ProjectMcp {
                req_id,
                user_id,
                folder,
                request,
            } => {
                self.project_mcp(req_id, user_id, folder, request);
                Ok(())
            }
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
        // Within node.toml's ceiling, whatever was asked (T50).
        let mode = policy.effective_mode(approval_mode);
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
                sub_folder: None,
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

    /// Fork `parent` at the end of `turn_id` into `sid` (C.2): the new
    /// chat's events are written whole (`Session::fork`), the session is
    /// registered, and its events go to the master in order; the ack the
    /// caller sends follows the last of them.
    async fn fork(
        &self,
        sid: SessionId,
        parent: SessionId,
        turn_id: &str,
        title: &str,
    ) -> Result<(), String> {
        let Some(p) = self.session(parent) else {
            return Err("unknown session".into());
        };
        let (s, events) = p.fork(&self.inner, sid, turn_id, title).await?;
        self.inner
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(sid, s);
        for event in events {
            self.inner.publish_wait(NodeToMaster::Event { event }).await;
        }
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
                // MCP servers (X.3) end with the node, after a stop as after
                // a crash; waited for below like background commands.
                s.close_mcp_now();
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
                s.background_active()
                    || (s.background_unrecorded() && !stuck.contains(&s.id()))
                    || s.mcp_running() > 0
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
        let folder = self.session(sid).map(|s| {
            s.cwd()
                .canonicalize()
                .unwrap_or_else(|_| s.cwd().to_path_buf())
                .to_string_lossy()
                .into_owned()
        });
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
            // Background sub-agents' ends not recorded: their folders go
            // unmerged (X.24).
            s.drop_child_ends().await;
            // And its MCP servers (X.3).
            if !s.close_mcp(harness_tools::mcp::STOP_WAIT).await {
                tracing::warn!(session = %sid, "an MCP server of a deleted session did not end in time");
            }
        }
        self.inner
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&sid);
        self.inner.uploads.drop_session(sid);
        // What the backend keeps for it in memory (the file tools' record).
        self.inner.cfg.backend.forget_session(sid);
        // The session's sandbox temp folder (TMPDIR) goes with it.
        if let Some(root) = &self.inner.cfg.policy.sandbox_tmp {
            let _ = std::fs::remove_dir_all(root.join(sid.to_string()));
        }
        // Its file changes stay in the folder after its log goes: a write
        // tombstone (folder and time span, no content) keeps another
        // chat's undo refusing over them (PLAN.md T57).
        let now = harness_core::now_ms();
        let existed = self
            .inner
            .cfg
            .store
            .call(move |s| {
                let writes = match folder {
                    Some(f) => {
                        // Not what a fork copied from its parent (C.2).
                        let through = s.fork_of(sid)?.map_or(0, |r| r.through_seq);
                        let mut e = s.events_since_turn_at(sid, i64::MIN, others::KINDS)?;
                        e.retain(|e| !harness_core::fork::inherited(through, e.seq));
                        others::write_span(&e, now).map(|(a, b)| (f, a, b))
                    }
                    None => None,
                };
                let existed = s.delete_session_leaving(
                    sid,
                    false,
                    writes.as_ref().map(|(f, a, b)| (f.as_str(), *a, *b)),
                )?;
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
