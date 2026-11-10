//! Node runtime: owns sessions, runs agent turns, is the single writer of each
//! session's event log, and talks to the master over a `NodeLink`.

pub mod artifacts;
mod git;
mod images;
pub mod policy;
mod session;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use harness_core::SessionId;
use harness_core::backend::AgentBackend;
use harness_protocol::{MasterToNode, NodeToMaster, PROTOCOL_VERSION, SessionHead};
use harness_store::Store;
use tokio::sync::mpsc;

pub use policy::NodePolicy;
use session::Session;

/// The fields of a `session.create` command.
struct CreateArgs {
    title: String,
    cwd: String,
    model: Option<String>,
    approval_mode: Option<harness_core::policy::ApprovalMode>,
    profile: Option<harness_core::event::Profile>,
    agent: Option<harness_core::agent::AgentSpec>,
    incognito: bool,
}

/// Max events per `session.snapshot` batch.
const SNAPSHOT_BATCH: usize = 500;

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
        | MasterToNode::SetAgent { session_id, .. }
        | MasterToNode::ArtifactStage { session_id, .. }
        | MasterToNode::ArtifactRelease { session_id, .. } => Some(*session_id),
        MasterToNode::Welcome { .. }
        | MasterToNode::Cancel { .. }
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
}

/// Both ends of a link as seen from the master: commands in, messages out.
/// In-process for the embedded node; bridged over WSS for remote nodes.
pub struct NodeLink {
    pub to_node: mpsc::UnboundedSender<MasterToNode>,
    pub from_node: mpsc::UnboundedReceiver<NodeToMaster>,
}

pub(crate) struct Inner {
    pub(crate) cfg: NodeConfig,
    pub(crate) sessions: Mutex<HashMap<SessionId, Arc<Session>>>,
    /// The current link to the master, if connected.
    link: Mutex<Option<mpsc::UnboundedSender<NodeToMaster>>>,
    /// Where published files are staged; `None` when this runtime does not
    /// publish (the chat runtime, or `[artifacts] enabled = false`).
    pub(crate) stages: Option<Arc<artifacts::Stages>>,
}

impl Inner {
    pub(crate) fn session(&self, sid: SessionId) -> Option<Arc<Session>> {
        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&sid)
            .cloned()
    }

    pub(crate) fn publish(&self, msg: NodeToMaster) {
        let mut link = self.link.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(tx) = link.as_ref()
            && tx.send(msg).is_err()
        {
            // Master side went away; events stay in the store and are
            // replayed on the next connection.
            *link = None;
        }
    }
}

#[derive(Clone)]
pub struct NodeRuntime {
    inner: Arc<Inner>,
}

impl NodeRuntime {
    /// Open the runtime and recover any turns interrupted by a crash.
    pub async fn start(cfg: NodeConfig) -> Result<Self, harness_store::StoreError> {
        let rows = cfg.store.call(|s| s.sessions()).await?;
        let hosted: std::collections::HashSet<SessionId> =
            rows.iter().map(|r| r.session_id).collect();
        let stages = open_stages(&cfg, &hosted);
        let inner = Arc::new(Inner {
            cfg,
            sessions: Mutex::new(HashMap::new()),
            link: Mutex::new(None),
            stages,
        });
        for row in rows {
            let s = Session::load(inner.clone(), row.session_id).await?;
            s.recover_interrupted().await;
            inner
                .sessions
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(row.session_id, s);
        }
        let this = Self { inner };
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

    pub fn node_id(&self) -> &str {
        &self.inner.cfg.node_id
    }

    pub fn store(&self) -> &Store {
        &self.inner.cfg.store
    }

    /// Connect a (new) master link. Any previous link is replaced. `hello` is
    /// queued before any event so the master always sees it first.
    pub async fn attach(&self) -> NodeLink {
        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let (in_tx, mut in_rx) = mpsc::unbounded_channel::<MasterToNode>();
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
            let _ = out_tx.send(NodeToMaster::Hello {
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
            *link = Some(out_tx);
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
                    // slow restore or delete (AUDIT S10). Neither awaits
                    // anything (their audit line is written from a task of
                    // its own).
                    //
                    // A cancel stops the running turn now, and also leaves a
                    // marker in the lane so a prompt sent before it, still
                    // queued, is stopped too: a cancel covers everything
                    // sent before it, as when the loop was serial. For a
                    // session that does not exist yet (its create is still
                    // queued) the cancel waits its turn in the lane.
                    MasterToNode::Cancel { session_id, .. }
                        if this.session(session_id).is_none() =>
                    {
                        this.enqueue(&lanes, session_id, LaneCmd::Msg(Box::new(msg)))
                    }
                    MasterToNode::Cancel { session_id, .. } => {
                        this.handle(msg).await;
                        this.mark_cancel(&lanes, session_id);
                    }
                    m @ (MasterToNode::ApprovalRespond { .. } | MasterToNode::Welcome { .. }) => {
                        this.handle(m).await
                    }
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
            return self.start_lane(lanes, &mut map, sid, cmd);
        };
        let cmd = match tx.try_send(cmd) {
            Ok(()) => return,
            Err(mpsc::error::TrySendError::Full(cmd)) => {
                drop(map);
                if let LaneCmd::Msg(msg) = cmd
                    && let Some(req_id) = msg.req_id()
                {
                    self.inner.publish(NodeToMaster::Ack {
                        req_id: req_id.to_string(),
                        ok: false,
                        error: Some(format!(
                            "this session already has {LANE_DEPTH} commands waiting; try again"
                        )),
                    });
                }
                return;
            }
            // The lane's task ended without removing itself (it panicked):
            // nothing of its queue is left, so a new lane keeps the order.
            Err(mpsc::error::TrySendError::Closed(cmd)) => cmd,
        };
        self.start_lane(lanes, &mut map, sid, cmd);
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
        let cwd = s
            .publish_cwd()
            .ok_or("this chat cannot save files as artifacts")?;
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
            let kind = match stages.stage(req, src, Default::default()).await {
                Ok(staged) => EventKind::ArtifactStaged(Box::new(staged)),
                Err(error) => {
                    EventKind::ArtifactStageFailed(ArtifactStageFailed { path, name, error })
                }
            };
            if let Err(e) = s.record(kind).await {
                tracing::error!(session = %sid, error = %e, "could not record a staged file");
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
        let detail = serde_json::json!({ "session_id": sid }).to_string();
        let _ = self
            .inner
            .cfg
            .store
            .call(move |s| s.audit(kind, &detail))
            .await;
    }

    /// [`Self::audit`] from a task of its own, for commands that must be
    /// answered without waiting for the store.
    fn audit_later(&self, kind: &'static str, sid: SessionId) {
        let this = self.clone();
        tokio::spawn(async move { this.audit(kind, sid).await });
    }

    /// Handle one message from the master.
    pub async fn handle(&self, msg: MasterToNode) {
        let req_id = msg.req_id().map(str::to_string);
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
                session_id,
                title,
                cwd,
                model,
                approval_mode,
                profile,
                agent,
                incognito,
                ..
            } => {
                self.audit("session.create", session_id).await;
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
                    Some(s) => s.prompt(input, agent, compare, extra).await,
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
            MasterToNode::Cancel { session_id, .. } => {
                self.audit_later("session.cancel", session_id);
                match self.session(session_id) {
                    Some(s) => {
                        s.cancel();
                        Ok(())
                    }
                    None => Err("unknown session".into()),
                }
            }
            MasterToNode::ApprovalRespond {
                session_id,
                approval_id,
                decision,
                ..
            } => {
                self.audit_later("approval.respond", session_id);
                match self.session(session_id) {
                    Some(s) => s.respond(&approval_id, decision),
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
            self.inner.publish(NodeToMaster::Ack {
                req_id,
                ok: result.is_ok(),
                error: result.err(),
            });
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
        // Chat sessions have no workspace; their runtime offers no file tools.
        let cwd = match profile {
            Profile::Agentic => policy.check_cwd(&cwd)?,
            Profile::Chat => std::path::PathBuf::new(),
        };
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
        }
        self.inner
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&sid);
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
                .publish(NodeToMaster::Deleted { session_id: sid });
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
            let (head, events) = match res {
                Ok(v) => v,
                Err(e) => {
                    tracing::error!(%sid, error = %e, "replay failed");
                    return;
                }
            };
            let last = events.last().map(|e| e.seq);
            let done = events.len() < SNAPSHOT_BATCH;
            self.inner.publish(NodeToMaster::Snapshot {
                session_id: sid,
                head_seq: head,
                events,
            });
            match last {
                Some(l) if !done => after = l,
                _ => return,
            }
        }
    }
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
        Ok(s) => Some(Arc::new(s)),
        Err(e) => {
            tracing::warn!(error = %e, dir = %sc.stage_dir.display(), "cannot open the artifact stage; publishing is off");
            None
        }
    }
}
