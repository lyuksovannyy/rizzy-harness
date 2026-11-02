//! The master: mirrors every node's sessions, routes commands to the owning
//! node, and serves the web UI. It never runs tools itself.

mod assets;
pub mod auth;
mod http;
pub mod secrets;
mod ws;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
            }),
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
    pub fn default_node(&self) -> Option<String> {
        self.nodes()
            .into_iter()
            .find(|n| n.embedded && n.connected)
            .or_else(|| self.nodes().into_iter().find(|n| n.connected))
            .map(|n| n.node_id)
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
            ..
        } = first
        else {
            return Err("first message was not node.hello".into());
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
                },
                tx: None,
                resuming: HashSet::new(),
                generation: 0,
            });
            entry.info.hostname = hostname;
            entry.info.version = version;
            entry.info.connected = true;
            entry.tx = Some(link.to_node.clone());
            entry.resuming.clear();
            entry.generation += 1;
            entry.generation
        };
        let _ = self.inner.live.send(Live::NodesChanged);

        // Ask for everything we have not mirrored yet.
        for h in sessions {
            let sid = h.session_id;
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

    pub async fn create_session(
        &self,
        node_id: Option<String>,
        title: String,
        cwd: String,
        model: Option<String>,
        approval_mode: Option<harness_core::policy::ApprovalMode>,
    ) -> Result<SessionId, CommandError> {
        let node_id = node_id
            .or_else(|| self.default_node())
            .ok_or_else(|| CommandError::NodeOffline("(none)".into()))?;
        let sid = SessionId::new();
        self.send_command(&node_id, |req_id| MasterToNode::Create {
            req_id,
            session_id: sid,
            title,
            cwd,
            model,
            approval_mode,
        })
        .await?;
        Ok(sid)
    }

    pub async fn prompt(&self, sid: SessionId, text: String) -> Result<(), CommandError> {
        let node = self.owner(sid).await?;
        self.send_command(&node, |req_id| MasterToNode::Prompt {
            req_id,
            session_id: sid,
            text,
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
