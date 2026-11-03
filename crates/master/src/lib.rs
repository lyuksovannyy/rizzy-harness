//! The master: mirrors every node's sessions, routes commands to the owning
//! node, and serves the web UI. It never runs tools itself.

mod assets;
pub mod auth;
pub mod config;
mod http;
pub mod prefs;
pub mod providers;
pub mod search;
pub mod secrets;
mod ws;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::event::Profile;
use harness_core::{RawEvent, SessionId, new_id};
use harness_node::NodeLink;
use harness_protocol::{MasterToNode, NodeToMaster, PROTOCOL_VERSION};
use harness_store::{AppendOutcome, Store};
use serde::Serialize;
use tokio::sync::{broadcast, mpsc, oneshot};

pub use http::router;

const ACK_TIMEOUT: Duration = Duration::from_secs(15);

/// Messages fanned out to UI WebSockets.
#[derive(Debug, Clone)]
pub enum Live {
    Event(RawEvent),
    SessionsChanged,
    NodesChanged,
}

/// A model the UI may offer, with what it supports (PLAN.md §0: capabilities
/// come from configuration, never guessed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub provider: String,
    pub effort: bool,
    pub vision: bool,
    pub default: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeInfo {
    pub node_id: String,
    pub hostname: String,
    pub version: String,
    pub connected: bool,
    pub embedded: bool,
    /// Which sessions the node hosts (from its `profile:*` capability).
    pub profile: Profile,
}

struct NodeConn {
    info: NodeInfo,
    tx: Option<mpsc::UnboundedSender<MasterToNode>>,
    /// Sessions with a `resume` in flight, so a burst of gaps sends one.
    resuming: HashSet<SessionId>,
    /// Bumped on every (re)connect so a stale pump cannot mark a newer link down.
    generation: u64,
}

pub struct MasterInner {
    pub mirror: Store,
    nodes: Mutex<HashMap<String, NodeConn>>,
    live: broadcast::Sender<Live>,
    acks: Mutex<HashMap<String, oneshot::Sender<Result<(), String>>>>,
    pub auth: auth::Auth,
    models: Mutex<Vec<ModelInfo>>,
    agents: Mutex<harness_agents::AgentSet>,
    providers: Mutex<Option<Arc<providers::ProviderRuntime>>>,
    /// Sessions with a re-sent `session.delete` in flight.
    redeleting: Mutex<HashSet<SessionId>>,
}

/// What the UI asks for when starting a session.
#[derive(Debug, Clone, Default)]
pub struct CreateSpec {
    pub profile: Option<Profile>,
    pub node_id: Option<String>,
    pub title: String,
    pub cwd: String,
    pub model: Option<String>,
    pub approval_mode: Option<harness_core::policy::ApprovalMode>,
    pub agent_id: Option<String>,
    /// Temporary chat (chat profile only).
    pub incognito: bool,
}

/// Per-message choices on a prompt.
#[derive(Debug, Clone, Default)]
pub struct PromptOpts {
    /// Run this turn as another agent (the session's agent is unchanged).
    pub agent_id: Option<String>,
    /// `low` | `medium` | `high` | `max`; dropped if the model lacks support.
    pub effort: Option<String>,
    /// Appended to the agent's system prompt (user name, local time, place).
    pub context: Option<String>,
}

/// A leading `@agent-id` in a prompt routes that turn to the agent.
pub fn leading_mention(text: &str) -> Option<&str> {
    let rest = text.trim_start().strip_prefix('@')?;
    let id = rest
        .split(|c: char| c.is_whitespace() || c == ',' || c == ':')
        .next()?;
    (!id.is_empty()).then_some(id)
}

#[derive(Clone)]
pub struct Master {
    pub(crate) inner: Arc<MasterInner>,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("unknown session")]
    UnknownSession,
    #[error("node {0} is not connected")]
    NodeOffline(String),
    #[error("node did not answer in time")]
    Timeout,
    #[error("{0}")]
    Rejected(String),
    #[error("store: {0}")]
    Store(#[from] harness_store::StoreError),
    #[error("unknown agent {0:?}")]
    UnknownAgent(String),
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Master {
    pub fn new(mirror: Store, auth: auth::Auth) -> Self {
        let (live, _) = broadcast::channel(4096);
        Self {
            inner: Arc::new(MasterInner {
                mirror,
                nodes: Mutex::new(HashMap::new()),
                live,
                acks: Mutex::new(HashMap::new()),
                auth,
                models: Mutex::new(Vec::new()),
                providers: Mutex::new(None),
                redeleting: Mutex::new(HashSet::new()),
                agents: Mutex::new(harness_agents::AgentSet::from_agents(
                    harness_agents::builtins(),
                    harness_agents::DEFAULT_AGENT,
                )),
            }),
        }
    }

    /// Hand the master the live provider so admins can reconfigure it.
    pub fn set_provider_runtime(&self, rt: Arc<providers::ProviderRuntime>) {
        self.set_models(rt.models());
        *lock(&self.inner.providers) = Some(rt);
    }

    pub fn provider_runtime(&self) -> Option<Arc<providers::ProviderRuntime>> {
        lock(&self.inner.providers).clone()
    }

    pub fn set_models(&self, models: Vec<ModelInfo>) {
        *lock(&self.inner.models) = models;
    }

    pub fn models(&self) -> Vec<ModelInfo> {
        lock(&self.inner.models).clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Live> {
        self.inner.live.subscribe()
    }

    pub fn nodes(&self) -> Vec<NodeInfo> {
        let mut v: Vec<NodeInfo> = lock(&self.inner.nodes)
            .values()
            .map(|n| n.info.clone())
            .collect();
        v.sort_by(|a, b| b.embedded.cmp(&a.embedded).then(a.node_id.cmp(&b.node_id)));
        v
    }

    /// The id of the embedded node, if any (the default target for new sessions).
    /// The node a new session of `profile` goes to when none is chosen:
    /// an embedded one first.
    pub fn default_node(&self, profile: Profile) -> Option<String> {
        let nodes: Vec<_> = self
            .nodes()
            .into_iter()
            .filter(|n| n.connected && n.profile == profile)
            .collect();
        nodes
            .iter()
            .find(|n| n.embedded)
            .or(nodes.first())
            .map(|n| n.node_id.clone())
    }

    pub fn set_agents(&self, agents: harness_agents::AgentSet) {
        *lock(&self.inner.agents) = agents;
    }

    pub fn agents(&self) -> harness_agents::AgentSet {
        lock(&self.inner.agents).clone()
    }

    /// Resolve an agent id (or the default) to the spec a node runs, dropping
    /// effort when the model is not known to support it.
    pub fn agent_spec(
        &self,
        agent_id: Option<&str>,
        session_model: Option<&str>,
    ) -> Result<harness_core::agent::AgentSpec, CommandError> {
        self.agent_spec_with(agent_id, session_model, None)
    }

    /// Like `agent_spec`, with an optional per-message effort that replaces
    /// the agent's. The model-capability gate applies either way.
    pub fn agent_spec_with(
        &self,
        agent_id: Option<&str>,
        session_model: Option<&str>,
        effort: Option<&str>,
    ) -> Result<harness_core::agent::AgentSpec, CommandError> {
        if let Some(e) = effort
            && !matches!(e, "low" | "medium" | "high" | "max")
        {
            return Err(CommandError::Rejected(format!(
                "effort must be low, medium, high or max, not {e:?}"
            )));
        }
        let set = self.agents();
        let agent = match agent_id {
            Some(id) => set
                .get(id)
                .ok_or_else(|| CommandError::UnknownAgent(id.into()))?,
            None => set
                .default_agent()
                .ok_or_else(|| CommandError::UnknownAgent("(default)".into()))?,
        };
        let mut spec = agent.spec.clone();
        if let Some(e) = effort {
            spec.effort = Some(e.to_string());
        }
        let models = self.models();
        let model = spec
            .model
            .clone()
            .or(session_model.map(str::to_string))
            .or_else(|| models.iter().find(|m| m.default).map(|m| m.id.clone()));
        let effort_ok = model
            .as_deref()
            .is_some_and(|m| models.iter().any(|x| x.id == m && x.effort));
        if !effort_ok {
            spec.effort = None;
        }
        Ok(spec)
    }

    /// Attach a node link. The first message must be `node.hello`.
    pub async fn add_link(&self, mut link: NodeLink, embedded: bool) -> Result<String, String> {
        let first = link
            .from_node
            .recv()
            .await
            .ok_or("link closed before hello")?;
        let NodeToMaster::Hello {
            protocol_version,
            node_id,
            hostname,
            version,
            sessions,
            capabilities,
        } = first
        else {
            return Err("first message was not node.hello".into());
        };
        let profile = if capabilities.iter().any(|c| c == "profile:chat") {
            Profile::Chat
        } else {
            Profile::Agentic
        };
        if protocol_version != PROTOCOL_VERSION {
            return Err(format!(
                "protocol version {protocol_version} not supported (want {PROTOCOL_VERSION})"
            ));
        }
        let _ = link.to_node.send(MasterToNode::Welcome {
            protocol_version: PROTOCOL_VERSION,
        });
        let generation = {
            let mut nodes = lock(&self.inner.nodes);
            let entry = nodes.entry(node_id.clone()).or_insert_with(|| NodeConn {
                info: NodeInfo {
                    node_id: node_id.clone(),
                    hostname: hostname.clone(),
                    version: version.clone(),
                    connected: false,
                    embedded,
                    profile,
                },
                tx: None,
                resuming: HashSet::new(),
                generation: 0,
            });
            entry.info.hostname = hostname;
            entry.info.version = version;
            entry.info.connected = true;
            entry.info.profile = profile;
            entry.tx = Some(link.to_node.clone());
            entry.resuming.clear();
            entry.generation += 1;
            entry.generation
        };
        let _ = self.inner.live.send(Live::NodesChanged);

        // Ask for everything we have not mirrored yet.
        for h in sessions {
            let sid = h.session_id;
            let deleted = self
                .inner
                .mirror
                .call(move |s| s.is_deleted(sid))
                .await
                .unwrap_or(false);
            if deleted {
                self.redelete(&node_id, sid);
                continue;
            }
            let mirror_head = self
                .inner
                .mirror
                .call(move |s| s.head_seq(sid))
                .await
                .unwrap_or(0);
            if h.head_seq > mirror_head {
                self.request_resume(&node_id, sid, mirror_head);
            }
        }

        let this = self.clone();
        let nid = node_id.clone();
        tokio::spawn(async move {
            while let Some(msg) = link.from_node.recv().await {
                this.on_node_message(&nid, msg).await;
            }
            tracing::info!(node = %nid, "node link closed");
            let mut nodes = lock(&this.inner.nodes);
            if let Some(n) = nodes.get_mut(&nid)
                && n.generation == generation
            {
                n.info.connected = false;
                n.tx = None;
            }
            drop(nodes);
            let _ = this.inner.live.send(Live::NodesChanged);
        });
        Ok(node_id)
    }

    fn request_resume(&self, node_id: &str, sid: SessionId, last_seq: u64) {
        let mut nodes = lock(&self.inner.nodes);
        if let Some(n) = nodes.get_mut(node_id)
            && n.resuming.insert(sid)
            && let Some(tx) = &n.tx
        {
            let _ = tx.send(MasterToNode::Resume {
                session_id: sid,
                last_seq,
            });
        }
    }

    fn resume_done(&self, node_id: &str, sid: SessionId) {
        if let Some(n) = lock(&self.inner.nodes).get_mut(node_id) {
            n.resuming.remove(&sid);
        }
    }

    async fn on_node_message(&self, node_id: &str, msg: NodeToMaster) {
        match msg {
            NodeToMaster::Event { event } => self.apply(node_id, event).await,
            NodeToMaster::Snapshot {
                session_id,
                head_seq,
                events,
            } => {
                for e in events {
                    self.apply(node_id, e).await;
                }
                let sid = session_id;
                let head = self
                    .inner
                    .mirror
                    .call(move |s| s.head_seq(sid))
                    .await
                    .unwrap_or(0);
                if head >= head_seq {
                    self.resume_done(node_id, session_id);
                }
            }
            NodeToMaster::Ack { req_id, ok, error } => {
                if let Some(tx) = lock(&self.inner.acks).remove(&req_id) {
                    let _ = tx.send(if ok {
                        Ok(())
                    } else {
                        Err(error.unwrap_or_else(|| "rejected".into()))
                    });
                }
            }
            NodeToMaster::Hello { .. } => {
                tracing::warn!(node = %node_id, "unexpected second hello");
            }
            NodeToMaster::Deleted { session_id } => {
                // Only the owning node may delete its session.
                let owner = self.owner(session_id).await.ok();
                if owner.as_deref() == Some(node_id) {
                    self.forget(session_id).await;
                }
            }
        }
    }

    /// The replication rule (PLAN.md §3).
    async fn apply(&self, node_id: &str, e: RawEvent) {
        let owner = node_id.to_string();
        let ev = e.clone();
        let res = self.inner.mirror.call(move |s| s.append(&owner, &ev)).await;
        match res {
            Ok(AppendOutcome::Applied) => {
                let structural = matches!(
                    e.kind.as_str(),
                    "session_created" | "turn_started" | "turn_ended"
                );
                let _ = self.inner.live.send(Live::Event(e));
                if structural {
                    let _ = self.inner.live.send(Live::SessionsChanged);
                }
            }
            Ok(AppendOutcome::Duplicate) => {}
            Ok(AppendOutcome::Gap { head }) => self.request_resume(node_id, e.session_id, head),
            Ok(AppendOutcome::Deleted) => self.redelete(node_id, e.session_id),
            Err(err) => {
                tracing::warn!(node = %node_id, session = %e.session_id, error = %err, "rejected event");
            }
        }
    }

    async fn send_command(
        &self,
        node_id: &str,
        build: impl FnOnce(String) -> MasterToNode,
    ) -> Result<(), CommandError> {
        let req_id = new_id();
        let (tx, rx) = oneshot::channel();
        lock(&self.inner.acks).insert(req_id.clone(), tx);
        let sent = {
            let nodes = lock(&self.inner.nodes);
            nodes
                .get(node_id)
                .and_then(|n| n.tx.as_ref())
                .map(|tx| tx.send(build(req_id.clone())).is_ok())
                .unwrap_or(false)
        };
        if !sent {
            lock(&self.inner.acks).remove(&req_id);
            return Err(CommandError::NodeOffline(node_id.into()));
        }
        match tokio::time::timeout(ACK_TIMEOUT, rx).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(e))) => Err(CommandError::Rejected(e)),
            Ok(Err(_)) | Err(_) => {
                lock(&self.inner.acks).remove(&req_id);
                Err(CommandError::Timeout)
            }
        }
    }

    async fn owner(&self, sid: SessionId) -> Result<String, CommandError> {
        self.inner
            .mirror
            .call(move |s| s.owner(sid))
            .await?
            .ok_or(CommandError::UnknownSession)
    }

    pub async fn create_session(&self, spec: CreateSpec) -> Result<SessionId, CommandError> {
        let profile = spec.profile.unwrap_or(Profile::Agentic);
        let node_id = spec
            .node_id
            .or_else(|| self.default_node(profile))
            .ok_or_else(|| {
                CommandError::NodeOffline(match profile {
                    Profile::Chat => "(chat runtime)".into(),
                    Profile::Agentic => "(no worker node; set one up for agentic sessions)".into(),
                })
            })?;
        let agent = self.agent_spec(spec.agent_id.as_deref(), spec.model.as_deref())?;
        let sid = SessionId::new();
        self.send_command(&node_id, |req_id| MasterToNode::Create {
            req_id,
            session_id: sid,
            title: spec.title,
            cwd: spec.cwd,
            model: spec.model,
            approval_mode: spec.approval_mode,
            profile: Some(profile),
            agent: Some(agent),
            incognito: spec.incognito,
        })
        .await?;
        Ok(sid)
    }

    /// Run a turn. The agent is, in order: `agent_id` (explicit, e.g. from the
    /// mention popup), a leading `@agent` in the text, the session's agent.
    pub async fn prompt(
        &self,
        sid: SessionId,
        text: String,
        opts: PromptOpts,
    ) -> Result<(), CommandError> {
        let PromptOpts {
            agent_id,
            effort,
            context,
        } = opts;
        let row = self
            .inner
            .mirror
            .call(move |s| s.session(sid))
            .await?
            .ok_or(CommandError::UnknownSession)?;
        let mention = leading_mention(&text)
            .filter(|id| self.agents().get(id).is_some())
            .map(str::to_string);
        let chosen = agent_id.or(mention).or(row.agent_id.clone());
        let mut agent =
            self.agent_spec_with(chosen.as_deref(), Some(&row.model), effort.as_deref())?;
        if let Some(c) = context {
            // prompt_hash still identifies the agent definition, not the context.
            agent.system_prompt = format!("{}\n\n{c}", agent.system_prompt);
        }
        self.send_command(&row.node_id, |req_id| MasterToNode::Prompt {
            req_id,
            session_id: sid,
            text,
            agent: Some(agent),
        })
        .await
    }

    /// Change the session's active agent for future turns.
    pub async fn set_agent(&self, sid: SessionId, agent_id: &str) -> Result<(), CommandError> {
        let row = self
            .inner
            .mirror
            .call(move |s| s.session(sid))
            .await?
            .ok_or(CommandError::UnknownSession)?;
        let agent = self.agent_spec(Some(agent_id), Some(&row.model))?;
        self.send_command(&row.node_id, |req_id| MasterToNode::SetAgent {
            req_id,
            session_id: sid,
            agent,
        })
        .await
    }

    pub async fn cancel(&self, sid: SessionId) -> Result<(), CommandError> {
        let node = self.owner(sid).await?;
        self.send_command(&node, |req_id| MasterToNode::Cancel {
            req_id,
            session_id: sid,
        })
        .await
    }

    /// Delete a session everywhere. The mirror copy goes (with a tombstone)
    /// even if the node is offline; the node is told again when it returns.
    pub async fn delete_session(&self, sid: SessionId) -> Result<(), CommandError> {
        let node = self.owner(sid).await?;
        let res = self
            .send_command(&node, |req_id| MasterToNode::Delete {
                req_id,
                session_id: sid,
            })
            .await;
        match res {
            Ok(()) | Err(CommandError::NodeOffline(_)) | Err(CommandError::Timeout) => {}
            Err(e) => return Err(e),
        }
        self.forget(sid).await;
        Ok(())
    }

    /// Drop the mirror copy of a session and refuse its events from now on.
    async fn forget(&self, sid: SessionId) {
        let res = self
            .inner
            .mirror
            .call(move |s| {
                s.delete_session(sid, true)?;
                s.checkpoint()
            })
            .await;
        if let Err(e) = res {
            tracing::warn!(session = %sid, error = %e, "deleting mirror copy failed");
        }
        let _ = self.inner.live.send(Live::SessionsChanged);
    }

    /// A node still has a session we deleted (it was offline): tell it again.
    fn redelete(&self, node_id: &str, sid: SessionId) {
        let first = lock(&self.inner.redeleting).insert(sid);
        if !first {
            return;
        }
        let this = self.clone();
        let node = node_id.to_string();
        tokio::spawn(async move {
            let _ = this
                .send_command(&node, |req_id| MasterToNode::Delete {
                    req_id,
                    session_id: sid,
                })
                .await;
            lock(&this.inner.redeleting).remove(&sid);
        });
    }

    /// Mirror-side incognito purge, so expired chats vanish from the master
    /// even while their node is offline.
    pub async fn purge_incognito(&self, ttl: Duration) -> usize {
        let before = harness_core::now_ms() - ttl.as_millis() as i64;
        let ids = self
            .inner
            .mirror
            .call(move |s| s.expired_incognito(before))
            .await
            .unwrap_or_default();
        let n = ids.len();
        for sid in ids {
            let _ = self.delete_session(sid).await;
        }
        n
    }

    pub fn start_incognito_purge(&self, ttl: Duration) {
        let this = self.clone();
        let every = (ttl / 10).clamp(Duration::from_secs(1), Duration::from_secs(300));
        tokio::spawn(async move {
            loop {
                this.purge_incognito(ttl).await;
                tokio::time::sleep(every).await;
            }
        });
    }

    pub async fn respond_approval(
        &self,
        sid: SessionId,
        approval_id: String,
        decision: harness_core::policy::Decision,
    ) -> Result<(), CommandError> {
        let node = self.owner(sid).await?;
        self.send_command(&node, |req_id| MasterToNode::ApprovalRespond {
            req_id,
            session_id: sid,
            approval_id,
            decision,
        })
        .await
    }
}
