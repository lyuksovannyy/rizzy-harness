//! The master: mirrors every node's sessions, routes commands to the owning
//! node, and serves the web UI. It never runs tools itself.

mod assets;
pub mod attachments;
pub mod auth;
pub mod avatars;
pub mod config;
mod favicon;
mod http;
pub mod model_cache;
pub mod node_setups;
pub mod nodes;
pub mod notices;
pub mod prefs;
pub mod presets;
pub mod providers;
pub mod provision;
pub mod resolver;
pub mod search;
pub mod secrets;
pub mod voice;
pub mod workspaces;
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
pub use resolver::Resolver;

const ACK_TIMEOUT: Duration = Duration::from_secs(15);

/// Messages fanned out to UI WebSockets.
#[derive(Debug, Clone)]
pub enum Live {
    Event(RawEvent),
    SessionsChanged,
    NodesChanged,
    MemoryChanged,
    /// A workspace, its members, their roles or the server's workspace
    /// policy changed (content-free: each client refetches its own view).
    WorkspacesChanged,
}

/// A model the UI may offer, with what it supports (PLAN.md §0: capabilities
/// come from configuration, never guessed; unreported is `false`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ModelInfo {
    /// `<provider_id>::<raw>`.
    pub id: String,
    pub raw: String,
    /// The provider's display name.
    pub provider: String,
    pub provider_id: String,
    pub name: Option<String>,
    pub context: Option<u64>,
    pub max_output: Option<u64>,
    pub effort: bool,
    pub vision: bool,
    pub tools: bool,
    pub default: bool,
    /// Missing from its provider's latest listing; the composer does not
    /// offer it, but sessions and agents naming it still resolve.
    pub unavailable: bool,
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
    /// The built-in node's graphify switch and its node.toml.
    graphify: Mutex<
        Option<(
            Arc<harness_tools::graphify::GraphifyState>,
            std::path::PathBuf,
        )>,
    >,
    /// Sessions with a re-sent `session.delete` in flight.
    redeleting: Mutex<HashSet<SessionId>>,
    memory: Mutex<Option<Arc<dyn harness_memory::MemoryStore>>>,
    /// The built-in store, when that is the backend (embedder switched live).
    builtin_memory: Mutex<Option<Arc<harness_memory::SqliteMemory>>>,
    /// Stop handles of live remote links, by node id (revoke drops them).
    remote_stops: Mutex<HashMap<String, tokio_util::sync::CancellationToken>>,
    /// Fingerprint of the node listener's certificate, if it runs.
    node_listener: Mutex<Option<(u16, String)>>,
    transcriber: Mutex<Option<Arc<voice::Transcriber>>>,
    provision: provision::Jobs,
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
    /// The account creating it (`None`: the API token).
    pub owner_id: Option<String>,
    /// The workspace to put it in. Membership is checked again when the
    /// chat is stamped; a Personal workspace needs no mapping.
    pub workspace_id: Option<String>,
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
    /// Documents for this message (already extracted and owner-checked).
    pub attachments: Vec<harness_core::event::Attachment>,
    /// Parallel answers from these models (2-4, chat only).
    pub compare: Vec<String>,
    /// An answer to a choices or form block; the prompt text is then the
    /// user's note. The node checks it and writes the message.
    pub reply: Option<harness_core::event::FormReply>,
}

/// Marks a session created without a model: it follows the defaults.
fn auto_model_key(sid: SessionId) -> String {
    format!("session-model-auto:{sid}")
}

/// Withhold `remember` from an agent and from every sub-agent it may hand
/// work to (a sub-agent runs with its own spec).
fn deny_remember(spec: &mut harness_core::agent::AgentSpec) {
    spec.deny_tools.push("remember".into());
    for d in &mut spec.delegates {
        d.deny_tools.push("remember".into());
    }
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

pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
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
                graphify: Mutex::new(None),
                redeleting: Mutex::new(HashSet::new()),
                memory: Mutex::new(None),
                builtin_memory: Mutex::new(None),
                remote_stops: Mutex::new(HashMap::new()),
                node_listener: Mutex::new(None),
                transcriber: Mutex::new(None),
                provision: Mutex::new(HashMap::new()),
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

    pub fn set_graphify(
        &self,
        state: Arc<harness_tools::graphify::GraphifyState>,
        node_toml: std::path::PathBuf,
    ) {
        *lock(&self.inner.graphify) = Some((state, node_toml));
    }

    pub fn graphify(
        &self,
    ) -> Option<(
        Arc<harness_tools::graphify::GraphifyState>,
        std::path::PathBuf,
    )> {
        lock(&self.inner.graphify).clone()
    }

    /// The admin's token-saving choice (`None` without a config: tests).
    pub fn token_savings(&self) -> Option<harness_core::savings::TokenSavings> {
        self.resolver(None).token_savings()
    }

    pub fn set_memory(&self, m: Arc<dyn harness_memory::MemoryStore>) {
        *lock(&self.inner.memory) = Some(m);
    }

    pub fn set_transcriber(&self, t: voice::Transcriber) {
        *lock(&self.inner.transcriber) = Some(Arc::new(t));
    }

    pub fn transcriber(&self) -> Option<Arc<voice::Transcriber>> {
        lock(&self.inner.transcriber).clone()
    }

    /// The mirror store (tests and tools).
    pub fn inner_mirror(&self) -> Store {
        self.inner.mirror.clone()
    }

    /// Record where remote nodes connect (for the "add node" command).
    pub fn set_node_listener(&self, port: u16, fingerprint: String) {
        *lock(&self.inner.node_listener) = Some((port, fingerprint));
    }

    pub fn node_listener(&self) -> Option<(u16, String)> {
        lock(&self.inner.node_listener).clone()
    }

    pub fn set_builtin_memory(&self, m: Arc<harness_memory::SqliteMemory>) {
        *lock(&self.inner.builtin_memory) = Some(m);
    }

    pub fn builtin_memory(&self) -> Option<Arc<harness_memory::SqliteMemory>> {
        lock(&self.inner.builtin_memory).clone()
    }

    pub fn memory(&self) -> Option<Arc<dyn harness_memory::MemoryStore>> {
        lock(&self.inner.memory).clone()
    }

    /// A mirrored `remember` call becomes a proposal for the chat's owner
    /// (never for incognito chats, nor where the owner keeps their memory
    /// out of a team workspace). The call id makes replays harmless.
    async fn file_proposal(&self, e: &RawEvent) {
        let Some(mem) = self.memory() else { return };
        let (Some(text), Some(call_id)) = (
            e.payload.pointer("/input/text").and_then(|v| v.as_str()),
            e.payload.get("call_id").and_then(|v| v.as_str()),
        ) else {
            return;
        };
        let sid = e.session_id;
        let Ok(Some(row)) = self.inner.mirror.call(move |s| s.session(sid)).await else {
            return;
        };
        // The calling row's own workspace and owner: a sub-agent's session
        // inherits its parent's.
        if row.incognito || !workspaces::uses_memory(self, &row).await {
            return;
        }
        let kind = match e.payload.pointer("/input/kind").and_then(|v| v.as_str()) {
            Some("preference") => harness_memory::FactKind::Preference,
            Some("location") => harness_memory::FactKind::Location,
            _ => harness_memory::FactKind::Fact,
        };
        let res = mem
            .add(harness_memory::NewFact {
                owner_id: row.owner_id,
                text: text.to_string(),
                kind,
                status: harness_memory::Status::Proposed,
                source_session: Some(sid),
                dedupe_key: Some(format!("{sid}:{call_id}")),
            })
            .await;
        match res {
            Ok(Some(_)) => {
                let _ = self.inner.live.send(Live::MemoryChanged);
            }
            Ok(None) => {}
            Err(err) => tracing::warn!(error = %err, "could not file a memory proposal"),
        }
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

    /// A listed model by its `<provider>::<raw>` id.
    pub fn find_model(&self, id: &str) -> Option<ModelInfo> {
        self.models().into_iter().find(|m| m.id == id)
    }

    /// The server's model for `agent_id` and its fallbacks, if assigned.
    pub fn agent_chain(&self, agent_id: &str) -> Option<config::AgentChain> {
        self.resolver(None).agent_chain(agent_id)
    }

    /// The agent's main model at server scope (see `Resolver`).
    pub fn agent_main_model(&self, agent: &harness_agents::Agent) -> Option<String> {
        self.resolver(None).agent_main_model(agent)
    }

    /// How `agent_id` looks: the admin's choice or the one its id gives.
    pub fn agent_avatar(&self, agent_id: &str) -> avatars::AgentAvatar {
        self.provider_runtime()
            .and_then(|rt| rt.config().agent_avatars.get(agent_id).copied())
            .unwrap_or_else(|| avatars::AgentAvatar::default_for(agent_id))
    }

    /// The fallbacks of `agent_id` on `model` at server scope (see
    /// `Resolver::agent_fallbacks`).
    pub fn agent_fallbacks(&self, agent_id: &str, model: Option<&str>) -> Vec<String> {
        self.resolver(None).agent_fallbacks(agent_id, model)
    }

    /// The spec a node runs for an agent id (or the default) at server
    /// scope; chats use their workspace's resolver.
    pub fn agent_spec(
        &self,
        agent_id: Option<&str>,
        session_model: Option<&str>,
    ) -> Result<harness_core::agent::AgentSpec, CommandError> {
        self.agent_spec_with(agent_id, session_model, None)
    }

    /// Like `agent_spec`, with a per-message effort (see
    /// `Resolver::agent_spec_with`).
    pub fn agent_spec_with(
        &self,
        agent_id: Option<&str>,
        session_model: Option<&str>,
        effort: Option<&str>,
    ) -> Result<harness_core::agent::AgentSpec, CommandError> {
        self.resolver(None)
            .agent_spec_with(agent_id, session_model, effort)
    }

    /// The model a session chose, or `None` when it follows the defaults
    /// (created without one).
    async fn session_model(&self, row: &harness_store::SessionRow) -> Option<String> {
        let key = auto_model_key(row.session_id);
        let auto = self.inner.mirror.call(move |s| s.kv_get(&key)).await;
        match auto {
            Ok(Some(_)) => None,
            _ => Some(row.model.clone()),
        }
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
                    "session_created"
                        | "turn_started"
                        | "turn_ended"
                        | "approval_requested"
                        | "approval_resolved"
                );
                if e.kind == "tool_call_started"
                    && e.payload.get("tool").and_then(|v| v.as_str()) == Some("remember")
                {
                    self.file_proposal(&e).await;
                }
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
        let (owner, workspace) = (spec.owner_id.clone(), spec.workspace_id.clone());
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
        // A chosen model is kept as sent; without one the session follows
        // the agent's default in its workspace, resolved now and on every
        // turn. Clients always send the model they show, which is that
        // default unless the user picked another: the default counts as no
        // choice, so the session keeps following the defaults.
        let r = self.resolver_for(workspace.as_deref()).await;
        let auto = r.agent_spec_with(spec.agent_id.as_deref(), None, None)?;
        let chosen = spec
            .model
            .filter(|m| !m.trim().is_empty())
            .filter(|m| auto.model.as_deref() != Some(m.as_str()));
        let agent = match &chosen {
            None => auto,
            Some(m) => r.agent_spec_with(spec.agent_id.as_deref(), Some(m), None)?,
        };
        let model = chosen.clone().or(agent.model.clone());
        let sid = SessionId::new();
        self.send_command(&node_id, |req_id| MasterToNode::Create {
            req_id,
            session_id: sid,
            title: spec.title,
            cwd: spec.cwd,
            model,
            approval_mode: spec.approval_mode,
            profile: Some(profile),
            agent: Some(agent),
            incognito: spec.incognito,
        })
        .await?;
        // The node's session_created is mirrored before its ack (same
        // ordered link), so the row exists: stamp owner and workspace in one
        // transaction, then tell the UIs (the first SessionsChanged went out
        // before the chat had an owner).
        let auto = chosen.is_none().then(|| auto_model_key(sid));
        self.inner
            .mirror
            .call(move |s| {
                if let Some(key) = auto {
                    s.kv_set(&key, "1")?;
                }
                if owner.is_some() || workspace.is_some() {
                    s.claim_session(sid, owner.as_deref(), workspace.as_deref())?;
                }
                Ok::<_, harness_store::StoreError>(())
            })
            .await?;
        let _ = self.inner.live.send(Live::SessionsChanged);
        Ok(sid)
    }

    /// Run a turn. The agent is, in order: `agent_id` (explicit, e.g. from the
    /// mention popup), a leading `@agent` in the text, the session's agent.
    /// An answer (`reply`) goes to the agent that asked, ahead of all three.
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
            attachments,
            compare,
            reply,
        } = opts;
        let row = self
            .inner
            .mirror
            .call(move |s| s.session(sid))
            .await?
            .ok_or(CommandError::UnknownSession)?;
        // Agents, models and chains as the chat's workspace sets them.
        let r = self.resolver_for(row.workspace_id.as_deref()).await;
        if let Some(c) = compare.iter().find(|c| r.find_model(c).is_none()) {
            return Err(CommandError::Rejected(format!("unknown model {c:?}")));
        }
        // An answer goes to the agent that asked, whatever the note says: an
        // @mentioned agent's turn leaves the session's agent as it was, and
        // only the asker knows its ids and has the prompt that explains the
        // answer. If that agent is gone, the usual order applies.
        let asker = match &reply {
            Some(r) => {
                let message_id = r.message_id.clone();
                self.inner
                    .mirror
                    .call(move |s| s.open_message_author(sid, &message_id))
                    .await?
            }
            None => None,
        };
        let asker = asker.filter(|id| r.agents().get(id).is_some());
        let mention = leading_mention(&text)
            .filter(|id| reply.is_none() && r.agents().get(id).is_some())
            .map(str::to_string);
        let chosen = asker.or(agent_id).or(mention).or(row.agent_id.clone());
        let session_model = self.session_model(&row).await;
        let mut agent = r.agent_spec_with(
            chosen.as_deref(),
            session_model.as_deref(),
            effort.as_deref(),
        )?;
        // Per-turn context goes after the history, not into the system
        // prompt: a system prompt that changes every turn would make the
        // provider re-read (and bill) the whole conversation each time.
        let mut turn_context: Vec<String> = context.into_iter().collect();
        if row.incognito || !workspaces::uses_memory(self, &row).await {
            // Incognito chats, and chats in a team workspace where the owner
            // keeps their memory out, neither write nor read memory.
            deny_remember(&mut agent);
        } else if let Some(mem) = self.memory().filter(|_| !text.trim().is_empty()) {
            // An answer with no note has nothing to recall by: the answer
            // itself is ids and values, written on the node.
            match mem.recall(row.owner_id.as_deref(), &text, 6).await {
                Ok(facts) => turn_context.extend(harness_memory::context_block(&facts)),
                Err(err) => tracing::warn!(error = %err, "memory recall failed"),
            }
        }
        let chat = row.profile.as_deref() == Some("chat");
        let savings = r.token_savings().map(|t| t.for_turn(chat));
        self.send_command(&row.node_id, |req_id| MasterToNode::Prompt {
            req_id,
            session_id: sid,
            text,
            agent: Some(agent),
            attachments,
            compare,
            savings,
            context: (!turn_context.is_empty()).then(|| turn_context.join("\n\n")),
            reply,
        })
        .await
    }

    pub async fn select_variant(
        &self,
        sid: SessionId,
        group: String,
        message_id: String,
    ) -> Result<(), CommandError> {
        let node = self.owner(sid).await?;
        self.send_command(&node, |req_id| MasterToNode::SelectVariant {
            req_id,
            session_id: sid,
            group,
            message_id,
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
        let session_model = self.session_model(&row).await;
        let agent = self
            .resolver_for(row.workspace_id.as_deref())
            .await
            .agent_spec_with(Some(agent_id), session_model.as_deref(), None)?;
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

    /// Undo one turn's file changes on the owning node.
    pub async fn restore_turn(
        &self,
        sid: SessionId,
        turn_id: String,
        force: bool,
        rewind: bool,
    ) -> Result<(), CommandError> {
        let node = self.owner(sid).await?;
        self.send_command(&node, |req_id| MasterToNode::Restore {
            req_id,
            session_id: sid,
            turn_id,
            force,
            rewind,
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
                s.kv_delete(&auto_model_key(sid))?;
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
                // Uploads never sent with a message expire with the same TTL.
                let before = harness_core::now_ms() - ttl.as_millis() as i64;
                let _ = this
                    .inner
                    .mirror
                    .call(move |s| s.purge_attachments(before))
                    .await;
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
