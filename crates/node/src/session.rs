//! One session: the single writer of its log, the running turn, and the
//! approval gate.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use harness_core::agent::AgentSpec;
use harness_core::backend::{TurnCtx, TurnHost};
use harness_core::event::*;
use harness_core::policy::{ApprovalMode, Decision, ToolRisk};
use harness_core::view::SessionView;
use harness_core::{RawEvent, SessionId, new_id, now_ms};
use harness_protocol::NodeToMaster;
use harness_store::{AppendOutcome, StoreError};
use serde_json::Value;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::Inner;

struct RunningTurn {
    turn_id: String,
    cancel: CancellationToken,
}

pub(crate) struct Session {
    node: Arc<Inner>,
    id: SessionId,
    /// Serializes appends: holds the current head seq.
    head: tokio::sync::Mutex<u64>,
    cwd: PathBuf,
    model: String,
    mode: ApprovalMode,
    turn: Mutex<Option<RunningTurn>>,
    approvals: Mutex<HashMap<String, oneshot::Sender<Decision>>>,
}

/// Everything needed to start a session, already validated by the runtime.
pub(crate) struct NewSession {
    pub title: String,
    pub cwd: PathBuf,
    pub model: String,
    pub mode: ApprovalMode,
    pub profile: Profile,
    pub agent_id: Option<String>,
}

impl Session {
    pub(crate) async fn create(
        node: Arc<Inner>,
        id: SessionId,
        new: NewSession,
    ) -> Result<Arc<Self>, StoreError> {
        let NewSession {
            title,
            cwd,
            model,
            mode,
            profile,
            agent_id,
        } = new;
        let s = Arc::new(Self {
            node: node.clone(),
            id,
            head: tokio::sync::Mutex::new(0),
            cwd: cwd.clone(),
            model: model.clone(),
            mode,
            turn: Mutex::new(None),
            approvals: Mutex::new(HashMap::new()),
        });
        s.append(EventKind::SessionCreated(SessionCreated {
            profile,
            agent_id,
            incognito: false,
            title,
            cwd: cwd.display().to_string(),
            model,
            backend: node.cfg.backend.name().to_string(),
            node_id: node.cfg.node_id.clone(),
            approval_mode: mode,
        }))
        .await?;
        Ok(s)
    }

    pub(crate) async fn load(node: Arc<Inner>, id: SessionId) -> Result<Arc<Self>, StoreError> {
        let events = node
            .cfg
            .store
            .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
            .await?;
        let view = SessionView::fold(&events)
            .map_err(|e| StoreError::Corrupt(format!("session {id}: {e}")))?;
        let meta = view
            .meta
            .clone()
            .ok_or_else(|| StoreError::Corrupt(format!("session {id} has no session_created")))?;
        // Re-clamp on load: the node policy may have been tightened since.
        let mode = ApprovalMode::clamp(meta.approval_mode, node.cfg.policy.max_approval_mode);
        Ok(Arc::new(Self {
            node,
            id,
            head: tokio::sync::Mutex::new(view.head_seq),
            cwd: PathBuf::from(meta.cwd),
            model: meta.model,
            mode,
            turn: Mutex::new(None),
            approvals: Mutex::new(HashMap::new()),
        }))
    }

    /// Close out a turn that was running when the process died (PLAN.md §2).
    pub(crate) async fn recover_interrupted(&self) {
        let id = self.id;
        let Ok(events) = self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
            .await
        else {
            return;
        };
        let Ok(view) = SessionView::fold(&events) else {
            return;
        };
        let Some(turn_id) = view.running_turn else {
            return;
        };
        for approval_id in view.pending_approvals {
            self.emit(EventKind::ApprovalResolved(ApprovalResolved {
                approval_id,
                decision: Decision::Deny,
                by: ResolvedBy::Interrupted,
            }))
            .await;
        }
        self.emit(EventKind::Error(ErrorEvent {
            turn_id: Some(turn_id.clone()),
            message: "The node restarted while this turn was running. Send a message to continue."
                .into(),
        }))
        .await;
        self.emit(EventKind::TurnEnded(TurnEnded {
            turn_id,
            reason: TurnEndReason::Interrupted,
            usage: None,
        }))
        .await;
    }

    /// Assign the next seq, scrub, persist (fsync), then publish.
    async fn append(&self, kind: EventKind) -> Result<RawEvent, StoreError> {
        let mut head = self.head.lock().await;
        let (kind, mut payload) = kind.into_parts();
        if !self.node.cfg.secrets.is_empty() {
            let raw = payload.to_string();
            let clean = self.node.cfg.secrets.scrub(&raw);
            if clean != raw {
                payload = serde_json::from_str(&clean).unwrap_or(Value::Null);
            }
        }
        let ev = RawEvent {
            session_id: self.id,
            seq: *head + 1,
            ts_ms: now_ms(),
            kind,
            payload,
        };
        let owner = self.node.cfg.node_id.clone();
        let e2 = ev.clone();
        let out = self
            .node
            .cfg
            .store
            .call(move |s| s.append(&owner, &e2))
            .await?;
        if out != AppendOutcome::Applied {
            // Only this struct writes this session, so this is a bug.
            return Err(StoreError::Corrupt(format!(
                "single-writer violation on {}: {out:?}",
                self.id
            )));
        }
        *head = ev.seq;
        self.node.publish(NodeToMaster::Event { event: ev.clone() });
        Ok(ev)
    }

    async fn emit(&self, kind: EventKind) {
        if let Err(e) = self.append(kind).await {
            tracing::error!(session = %self.id, error = %e, "failed to append event");
        }
    }

    /// Record a change of the session's active agent.
    pub(crate) async fn set_agent(&self, agent: AgentSpec) -> Result<(), String> {
        self.append(EventKind::AgentChanged(AgentChanged {
            agent: agent.agent_ref(),
        }))
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    pub(crate) async fn prompt(
        self: &Arc<Self>,
        text: String,
        agent: Option<AgentSpec>,
    ) -> Result<(), String> {
        let turn_id = new_id();
        let cancel = CancellationToken::new();
        {
            let mut t = self.turn.lock().unwrap_or_else(|p| p.into_inner());
            if t.is_some() {
                return Err("a turn is already running".into());
            }
            *t = Some(RunningTurn {
                turn_id: turn_id.clone(),
                cancel: cancel.clone(),
            });
        }
        let this = self.clone();
        tokio::spawn(async move { this.run_turn(turn_id, text, agent, cancel).await });
        Ok(())
    }

    async fn run_turn(
        self: Arc<Self>,
        turn_id: String,
        text: String,
        agent: Option<AgentSpec>,
        cancel: CancellationToken,
    ) {
        let id = self.id;
        let history = self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
            .await
            .unwrap_or_default();
        self.emit(EventKind::TurnStarted(TurnStarted {
            agent: agent.as_ref().map(AgentSpec::agent_ref),
            turn_id: turn_id.clone(),
        }))
        .await;
        self.emit(EventKind::UserMessage(UserMessage {
            turn_id: turn_id.clone(),
            text: text.clone(),
        }))
        .await;

        // Run the backend in its own task so a panic still ends the turn.
        let this = self.clone();
        let tid = turn_id.clone();
        let c = cancel.clone();
        let res = tokio::spawn(async move {
            let backend = this.node.cfg.backend.clone();
            backend
                .run_turn(TurnCtx {
                    turn_id: tid,
                    history,
                    user_text: text,
                    cwd: this.cwd.clone(),
                    model: agent
                        .as_ref()
                        .and_then(|a| a.model.clone())
                        .unwrap_or_else(|| this.model.clone()),
                    agent,
                    cancel: c,
                    host: &*this,
                })
                .await
        })
        .await;
        let (reason, usage, error) = match res {
            Ok(o) => (o.reason, o.usage, o.error),
            Err(e) => (
                TurnEndReason::Error,
                None,
                Some(format!("agent backend crashed: {e}")),
            ),
        };
        if let Some(message) = error {
            self.emit(EventKind::Error(ErrorEvent {
                turn_id: Some(turn_id.clone()),
                message,
            }))
            .await;
        }
        let reason = if cancel.is_cancelled() && reason != TurnEndReason::Error {
            TurnEndReason::Cancelled
        } else {
            reason
        };
        self.emit(EventKind::TurnEnded(TurnEnded {
            turn_id: turn_id.clone(),
            reason,
            usage,
        }))
        .await;
        let mut t = self.turn.lock().unwrap_or_else(|p| p.into_inner());
        if t.as_ref().is_some_and(|t| t.turn_id == turn_id) {
            *t = None;
        }
    }

    pub(crate) fn cancel(&self) {
        if let Some(t) = self.turn.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            t.cancel.cancel();
        }
    }

    pub(crate) fn respond(&self, approval_id: &str, decision: Decision) -> Result<(), String> {
        let tx = self
            .approvals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(approval_id)
            .ok_or_else(|| "no such pending approval".to_string())?;
        tx.send(decision)
            .map_err(|_| "approval is no longer pending".to_string())
    }

    fn current_cancel(&self) -> Option<CancellationToken> {
        self.turn
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|t| t.cancel.clone())
    }
}

#[async_trait::async_trait]
impl TurnHost for Session {
    async fn emit(&self, kind: EventKind) {
        Session::emit(self, kind).await
    }

    async fn approve(&self, call_id: &str, tool: &str, input: &Value, risk: ToolRisk) -> Decision {
        let policy = &self.node.cfg.policy;
        if policy.denied_tools.iter().any(|t| t == tool) {
            let approval_id = new_id();
            // Record the refusal so the user sees why.
            self.emit(EventKind::ApprovalRequested(ApprovalRequested {
                approval_id: approval_id.clone(),
                call_id: call_id.into(),
                tool: tool.into(),
                input: input.clone(),
                reason: format!("`{tool}` is denied by this node's policy"),
            }))
            .await;
            self.emit(EventKind::ApprovalResolved(ApprovalResolved {
                approval_id,
                decision: Decision::Deny,
                by: ResolvedBy::Policy,
            }))
            .await;
            return Decision::Deny;
        }
        if !self.mode.needs_approval(risk) {
            return Decision::Allow;
        }
        let approval_id = new_id();
        let (tx, rx) = oneshot::channel();
        self.approvals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(approval_id.clone(), tx);
        self.emit(EventKind::ApprovalRequested(ApprovalRequested {
            approval_id: approval_id.clone(),
            call_id: call_id.into(),
            tool: tool.into(),
            input: input.clone(),
            reason: match risk {
                ToolRisk::Mutating => format!("`{tool}` can change files or run programs"),
                ToolRisk::ReadOnly => format!("approval mode requires asking for `{tool}`"),
            },
        }))
        .await;
        let cancel = self.current_cancel().unwrap_or_default();
        let (decision, by) = tokio::select! {
            r = rx => match r {
                Ok(d) => (d, ResolvedBy::User),
                Err(_) => (Decision::Deny, ResolvedBy::Cancelled),
            },
            _ = cancel.cancelled() => (Decision::Deny, ResolvedBy::Cancelled),
        };
        self.approvals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&approval_id);
        self.emit(EventKind::ApprovalResolved(ApprovalResolved {
            approval_id,
            decision,
            by,
        }))
        .await;
        decision
    }
}
