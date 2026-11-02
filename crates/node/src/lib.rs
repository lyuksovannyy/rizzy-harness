//! Node runtime: owns sessions, runs agent turns, is the single writer of each
//! session's event log, and talks to the master over a `NodeLink`.

pub mod policy;
mod session;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use harness_core::backend::AgentBackend;
use harness_core::{Secret, SessionId};
use harness_protocol::{MasterToNode, NodeToMaster, PROTOCOL_VERSION, SessionHead};
use harness_store::Store;
use tokio::sync::mpsc;

pub use policy::NodePolicy;
use session::Session;

/// Max events per `session.snapshot` batch.
const SNAPSHOT_BATCH: usize = 500;

pub struct NodeConfig {
    pub node_id: String,
    pub hostname: String,
    pub policy: NodePolicy,
    pub store: Store,
    pub backend: Arc<dyn AgentBackend>,
    pub default_model: String,
    /// Values scrubbed from every event payload before it is stored.
    pub secrets: Vec<Secret<String>>,
}

/// Both ends of a link as seen from the master: commands in, messages out.
/// In-process for the embedded node; bridged over WSS for remote nodes.
pub struct NodeLink {
    pub to_node: mpsc::UnboundedSender<MasterToNode>,
    pub from_node: mpsc::UnboundedReceiver<NodeToMaster>,
}

pub(crate) struct Inner {
    pub(crate) cfg: NodeConfig,
    sessions: Mutex<HashMap<SessionId, Arc<Session>>>,
    /// The current link to the master, if connected.
    link: Mutex<Option<mpsc::UnboundedSender<NodeToMaster>>>,
}

impl Inner {
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
        let inner = Arc::new(Inner {
            cfg,
            sessions: Mutex::new(HashMap::new()),
            link: Mutex::new(None),
        });
        let rows = inner.cfg.store.call(|s| s.sessions()).await?;
        for row in rows {
            let s = Session::load(inner.clone(), row.session_id).await?;
            s.recover_interrupted().await;
            inner
                .sessions
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(row.session_id, s);
        }
        Ok(Self { inner })
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
            let _ = out_tx.send(NodeToMaster::Hello {
                protocol_version: PROTOCOL_VERSION,
                node_id: self.inner.cfg.node_id.clone(),
                hostname: self.inner.cfg.hostname.clone(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                capabilities: vec!["native".into()],
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
            while let Some(msg) = in_rx.recv().await {
                this.handle(msg).await;
            }
        });
        NodeLink {
            to_node: in_tx,
            from_node: out_rx,
        }
    }

    fn session(&self, sid: SessionId) -> Option<Arc<Session>> {
        self.inner
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&sid)
            .cloned()
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
                ..
            } => {
                self.audit("session.create", session_id).await;
                self.create(session_id, title, cwd, model, approval_mode)
                    .await
            }
            MasterToNode::Prompt {
                session_id, text, ..
            } => {
                self.audit("session.prompt", session_id).await;
                match self.session(session_id) {
                    Some(s) => s.prompt(text).await,
                    None => Err("unknown session".into()),
                }
            }
            MasterToNode::Cancel { session_id, .. } => {
                self.audit("session.cancel", session_id).await;
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
                self.audit("approval.respond", session_id).await;
                match self.session(session_id) {
                    Some(s) => s.respond(&approval_id, decision),
                    None => Err("unknown session".into()),
                }
            }
        };
        if let Some(req_id) = req_id {
            self.inner.publish(NodeToMaster::Ack {
                req_id,
                ok: result.is_ok(),
                error: result.err(),
            });
        }
    }

    async fn create(
        &self,
        sid: SessionId,
        title: String,
        cwd: String,
        model: Option<String>,
        approval_mode: Option<harness_core::policy::ApprovalMode>,
    ) -> Result<(), String> {
        if self.session(sid).is_some() {
            return Err("session already exists".into());
        }
        let policy = &self.inner.cfg.policy;
        let cwd = policy.check_cwd(&cwd)?;
        let mode = policy.effective_mode(approval_mode);
        let model = model.unwrap_or_else(|| self.inner.cfg.default_model.clone());
        let s = Session::create(self.inner.clone(), sid, title, cwd, model, mode)
            .await
            .map_err(|e| e.to_string())?;
        self.inner
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(sid, s);
        Ok(())
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
