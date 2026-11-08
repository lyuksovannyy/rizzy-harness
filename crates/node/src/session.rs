//! One session: the single writer of its log, the running turn, and the
//! approval gate.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use harness_core::agent::AgentSpec;
use harness_core::backend::{TurnCtx, TurnExtra, TurnHost};
use harness_core::event::*;
use harness_core::policy::{ApprovalMode, Decision, ToolRisk};
use harness_core::refs::SharedRefs;
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
    /// Sub-agents started by this turn use the same token-saving methods.
    savings: harness_core::savings::Savings,
    /// Files this turn published (`publish` is capped per turn).
    publishes: Arc<std::sync::atomic::AtomicUsize>,
}

pub(crate) struct Session {
    node: Arc<Inner>,
    id: SessionId,
    /// Serializes appends: holds the current head seq.
    head: tokio::sync::Mutex<u64>,
    cwd: PathBuf,
    model: String,
    mode: ApprovalMode,
    profile: Profile,
    incognito: bool,
    /// Set on a delegated sub-task's session; limits what its turns may do.
    parent_session: Option<SessionId>,
    turn: Mutex<Option<RunningTurn>>,
    approvals: Mutex<HashMap<String, oneshot::Sender<Decision>>>,
}

/// What the user sent with a prompt.
pub(crate) struct Input {
    pub text: String,
    pub attachments: Vec<Attachment>,
    /// An answer to a choices or form block: `text` is then the user's
    /// note, and the message records the canonical answer before it.
    pub reply: Option<FormReply>,
}

/// A sub-agent's private worktree.
struct Isolation {
    root: PathBuf,
    worktree: PathBuf,
    base: String,
    cwd: PathBuf,
}

/// Everything needed to start a session, already validated by the runtime.
pub(crate) struct NewSession {
    pub title: String,
    pub cwd: PathBuf,
    pub model: String,
    pub mode: ApprovalMode,
    pub profile: Profile,
    pub agent_id: Option<String>,
    pub incognito: bool,
    pub parent_session: Option<SessionId>,
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
            incognito,
            parent_session,
        } = new;
        let s = Arc::new(Self {
            node: node.clone(),
            id,
            head: tokio::sync::Mutex::new(0),
            cwd: cwd.clone(),
            model: model.clone(),
            mode,
            profile,
            incognito,
            parent_session,
            turn: Mutex::new(None),
            approvals: Mutex::new(HashMap::new()),
        });
        s.append(EventKind::SessionCreated(SessionCreated {
            profile,
            agent_id,
            incognito,
            parent_session,
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
            profile: meta.profile,
            incognito: meta.incognito,
            parent_session: meta.parent_session,
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

    /// Append an event from outside a turn's own flow (a staged file).
    pub(crate) async fn record(&self, kind: EventKind) -> Result<(), String> {
        self.append(kind)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// The folder this chat publishes from, when it may publish at all:
    /// agentic chats only (never incognito ones).
    pub(crate) fn publish_cwd(&self) -> Option<PathBuf> {
        (self.profile == Profile::Agentic && !self.incognito).then(|| self.cwd.clone())
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
        input: Input,
        agent: Option<AgentSpec>,
        compare: Vec<String>,
        extra: TurnExtra,
    ) -> Result<(), String> {
        if !compare.is_empty() && (self.profile != Profile::Chat || compare.len() > 4) {
            return Err("parallel answers need a chat and at most 4 models".into());
        }
        if input.reply.is_some() && (!input.attachments.is_empty() || !compare.is_empty()) {
            return Err("an answer goes alone, without attachments or parallel answers".into());
        }
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
                savings: extra.savings.clone(),
                publishes: Arc::default(),
            });
        }
        let msg = match self.user_message(turn_id.clone(), input).await {
            Ok(msg) => msg,
            Err(e) => {
                self.release(&turn_id);
                return Err(e);
            }
        };
        let this = self.clone();
        tokio::spawn(async move {
            this.run_turn(msg, agent, compare, extra, cancel).await;
        });
        Ok(())
    }

    /// The message a turn starts with. An answer is checked against the
    /// log here, while the caller holds the turn slot: nothing else can
    /// append (another prompt, a variant choice) until the turn has
    /// recorded it, so the log checked is the log it joins.
    async fn user_message(&self, turn_id: String, input: Input) -> Result<UserMessage, String> {
        let Input {
            text,
            attachments,
            reply,
        } = input;
        let text = match &reply {
            None => text,
            Some(r) => {
                let id = self.id;
                let history = self
                    .node
                    .cfg
                    .store
                    .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
                    .await
                    .map_err(|e| e.to_string())?;
                let canonical = harness_core::ui::check_reply(&history, r)?;
                harness_core::ui::answer_text(canonical, &text)
            }
        };
        Ok(UserMessage {
            turn_id,
            text,
            attachments,
            reply,
        })
    }

    /// Free the turn slot if `turn_id` still holds it.
    fn release(&self, turn_id: &str) {
        let mut t = self.turn.lock().unwrap_or_else(|p| p.into_inner());
        if t.as_ref().is_some_and(|t| t.turn_id == turn_id) {
            *t = None;
        }
    }

    async fn run_turn(
        self: Arc<Self>,
        msg: UserMessage,
        agent: Option<AgentSpec>,
        compare: Vec<String>,
        extra: TurnExtra,
        cancel: CancellationToken,
    ) -> (TurnEndReason, Option<Usage>) {
        let turn_id = msg.turn_id.clone();
        let user_text = msg.model_text();
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
        self.emit(EventKind::UserMessage(msg)).await;
        let checkpoint = self.checkpoint(&turn_id, CheckpointPhase::Before).await;

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
                    user_text,
                    cwd: this.cwd.clone(),
                    model: agent
                        .as_ref()
                        .and_then(|a| a.model.clone())
                        .unwrap_or_else(|| this.model.clone()),
                    agent,
                    compare,
                    parent_session: this.parent_session,
                    extra,
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
        if let Some((root, before)) = checkpoint {
            self.checkpoint_after(&turn_id, &root, &before).await;
        }
        let reason = if cancel.is_cancelled() && reason != TurnEndReason::Error {
            TurnEndReason::Cancelled
        } else {
            reason
        };
        self.emit(EventKind::TurnEnded(TurnEnded {
            turn_id: turn_id.clone(),
            reason,
            usage: usage.clone(),
        }))
        .await;
        self.release(&turn_id);
        (reason, usage)
    }

    /// Where this session's checkpoints go, if it gets any.
    async fn checkpoint_repo(&self) -> Option<crate::git::Repo> {
        if self.profile != Profile::Agentic || self.node.cfg.policy.checkpoints_keep == 0 {
            return None;
        }
        crate::git::repo_for(&self.cwd, self.node.cfg.checkpoints_dir.as_deref()).await
    }

    /// Snapshot the workspace before a turn (agentic sessions).
    async fn checkpoint(
        &self,
        turn_id: &str,
        phase: CheckpointPhase,
    ) -> Option<(crate::git::Repo, String)> {
        let repo = self.checkpoint_repo().await?;
        let r = format!("refs/harness/{}/{turn_id}/before", self.id);
        match crate::git::snapshot(&repo, &r, "harness: before turn").await {
            Ok(sha) => {
                self.emit(EventKind::Checkpoint(Checkpoint {
                    turn_id: turn_id.into(),
                    phase,
                    git_ref: sha.clone(),
                    files: vec![],
                }))
                .await;
                Some((repo, sha))
            }
            Err(e) => {
                tracing::warn!(session = %self.id, error = %e, "checkpoint failed");
                None
            }
        }
    }

    async fn checkpoint_after(&self, turn_id: &str, repo: &crate::git::Repo, before: &str) {
        let r = format!("refs/harness/{}/{turn_id}/after", self.id);
        let res = async {
            let sha = crate::git::snapshot(repo, &r, "harness: after turn").await?;
            let files = crate::git::changed(repo, before, &sha).await?;
            Ok::<_, String>((sha, files))
        }
        .await;
        match res {
            Ok((sha, files)) => {
                self.emit(EventKind::Checkpoint(Checkpoint {
                    turn_id: turn_id.into(),
                    phase: CheckpointPhase::After,
                    git_ref: sha,
                    files: files.into_iter().map(|(_, p)| p).collect(),
                }))
                .await;
                crate::git::prune(repo, self.node.cfg.policy.checkpoints_keep).await;
            }
            Err(e) => tracing::warn!(session = %self.id, error = %e, "checkpoint failed"),
        }
    }

    /// Record which parallel answer the conversation continues from.
    pub(crate) async fn select_variant(
        &self,
        group: String,
        message_id: String,
    ) -> Result<(), String> {
        if self.is_running() {
            return Err("wait for the answers to finish".into());
        }
        let id = self.id;
        let events = self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
            .await
            .map_err(|e| e.to_string())?;
        let known = events.iter().any(|e| {
            matches!(e.typed(), Ok(EventKind::AssistantMessage(m))
                if m.message_id == message_id && m.variant.as_ref().is_some_and(|v| v.group == group))
        });
        if !known {
            return Err("no such answer".into());
        }
        self.append(EventKind::VariantSelected(VariantSelected {
            group,
            message_id,
        }))
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    /// Undo one turn's file changes, or rewind: undo it and every later
    /// turn (see `git::restore`). Records the new state as a snapshot.
    pub(crate) async fn restore(
        &self,
        turn_id: &str,
        force: bool,
        rewind: bool,
    ) -> Result<(), String> {
        if self.is_running() {
            return Err("wait for the running turn to finish".into());
        }
        let id = self.id;
        let events = self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
            .await
            .map_err(|e| e.to_string())?;
        let view = SessionView::fold(&events).map_err(|e| e.to_string())?;
        let cp = view
            .checkpoints
            .get(turn_id)
            .ok_or("this turn has no checkpoint")?;
        let (Some(before), Some(after)) = (cp.before.clone(), cp.after.clone()) else {
            return Err("this turn's checkpoint is incomplete".into());
        };
        // Turns from this one on, in order, and the latest recorded state.
        let mut later: Vec<String> = Vec::new();
        let mut latest = after.clone();
        let mut seen = false;
        for e in &events {
            if let Ok(EventKind::Checkpoint(c)) = e.typed() {
                if c.turn_id == turn_id {
                    seen = true;
                }
                if seen && c.phase == CheckpointPhase::After && !later.contains(&c.turn_id) {
                    later.push(c.turn_id.clone());
                }
                if c.phase != CheckpointPhase::Before {
                    latest = c.git_ref.clone();
                }
            }
        }
        if !rewind && cp.restored {
            return Err("already undone".into());
        }
        let repo = self
            .checkpoint_repo()
            .await
            .ok_or("checkpoints are not available for this folder any more")?;
        let expected = if rewind { latest } else { after };
        for sha in [&before, &expected] {
            if !crate::git::exists(&repo, sha).await {
                return Err(format!(
                    "this checkpoint is no longer kept (only the latest {} changes are)",
                    self.node.cfg.policy.checkpoints_keep
                ));
            }
        }
        let files = crate::git::restore(&repo, &before, &expected, force).await?;
        let state = crate::git::snapshot(
            &repo,
            &format!("refs/harness/{}/{turn_id}/restored", self.id),
            "harness: restored",
        )
        .await?;
        let targets: Vec<String> = if rewind {
            later
                .into_iter()
                .filter(|t| view.checkpoints.get(t).is_some_and(|c| !c.restored))
                .collect()
        } else {
            vec![turn_id.to_string()]
        };
        for t in targets {
            self.emit(EventKind::Checkpoint(Checkpoint {
                turn_id: t,
                phase: CheckpointPhase::Restored,
                git_ref: state.clone(),
                files: files.clone(),
            }))
            .await;
        }
        Ok(())
    }

    /// Run a delegated sub-task in a new child session and wait for it.
    async fn run_child(
        &self,
        agent: AgentSpec,
        task: String,
        refs: SharedRefs,
        cancel: CancellationToken,
        started: Box<dyn FnOnce(SessionId) + Send>,
    ) -> Result<harness_core::backend::DelegateOutcome, String> {
        let child_id = SessionId::new();
        let short: String = task.chars().take(60).collect();
        // A sub-agent that can change files works in its own git worktree,
        // so parallel sub-agents do not trample each other.
        let iso = self.isolate(&agent, child_id).await;
        let cwd = iso
            .as_ref()
            .map(|i| i.cwd.clone())
            .unwrap_or_else(|| self.cwd.clone());
        let child = Session::create(
            self.node.clone(),
            child_id,
            NewSession {
                title: format!("@{}: {short}", agent.agent_id),
                cwd,
                model: agent.model.clone().unwrap_or_else(|| self.model.clone()),
                mode: self.mode,
                profile: self.profile,
                agent_id: Some(agent.agent_id.clone()),
                incognito: self.incognito,
                parent_session: Some(self.id),
            },
        )
        .await
        .map_err(|e| e.to_string())?;
        self.node
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(child_id, child.clone());
        started(child_id);
        let turn_id = new_id();
        let savings = self
            .turn
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|t| t.savings.clone())
            .unwrap_or_default();
        *child.turn.lock().unwrap_or_else(|p| p.into_inner()) = Some(RunningTurn {
            turn_id: turn_id.clone(),
            cancel: cancel.clone(),
            savings: savings.clone(),
            publishes: Arc::default(),
        });
        let msg = UserMessage {
            turn_id,
            text: task,
            attachments: vec![],
            reply: None,
        };
        let extra = TurnExtra {
            savings,
            context: None,
            refs: Some(refs),
        };
        let (reason, usage) = child
            .run_turn(msg, Some(agent), vec![], extra, cancel)
            .await;
        let merge_note = match iso {
            Some(i) => Some(self.merge_back(i, child_id).await),
            None => None,
        };
        let log = self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(child_id, 0, usize::MAX >> 1))
            .await
            .unwrap_or_default();
        let text = log
            .iter()
            .rev()
            .find_map(|e| match e.typed() {
                Ok(EventKind::AssistantMessage(m)) if !m.text.trim().is_empty() => Some(m.text),
                _ => None,
            })
            .unwrap_or_default();
        // Whatever the answer cites, the parent resolves through these.
        let sources = harness_core::refs::sources_in(&log);
        let text = match merge_note {
            Some(n) => format!("{text}\n\n[{n}]"),
            None => text,
        };
        Ok(harness_core::backend::DelegateOutcome {
            child: child_id,
            text,
            reason,
            usage,
            sources,
        })
    }

    /// Prepare a worktree for a sub-agent with write access, seeded with the
    /// current working state (uncommitted edits included).
    async fn isolate(&self, agent: &AgentSpec, child: SessionId) -> Option<Isolation> {
        if self.profile != Profile::Agentic
            || !["write", "edit", "bash"]
                .iter()
                .any(|t| agent.allows_tool(t))
        {
            return None;
        }
        let root = crate::git::repo_root(&self.cwd).await?;
        let project = crate::git::Repo::project(&root);
        let res = async {
            let base = crate::git::snapshot(
                &project,
                &format!("refs/harness/{child}/base"),
                "harness: sub-agent base",
            )
            .await?;
            let wt = crate::git::worktrees_dir(&root)
                .await?
                .join(child.to_string());
            crate::git::worktree_add(&root, &wt, &base).await?;
            let rel = self
                .cwd
                .strip_prefix(&root)
                .unwrap_or(std::path::Path::new(""));
            let cwd = wt.join(rel);
            // The node's policy still decides where sessions may work.
            if let Err(e) = self.node.cfg.policy.check_cwd(&cwd.display().to_string()) {
                crate::git::worktree_remove(&root, &wt).await;
                return Err(e);
            }
            Ok::<_, String>(Isolation {
                root: root.clone(),
                worktree: wt,
                base,
                cwd,
            })
        }
        .await;
        match res {
            Ok(i) => Some(i),
            Err(e) => {
                tracing::warn!(session = %self.id, error = %e, "no worktree for sub-agent; sharing the folder");
                None
            }
        }
    }

    /// Bring a sub-agent's changes back into the main working directory.
    async fn merge_back(&self, iso: Isolation, child: SessionId) -> String {
        let result_ref = format!("refs/harness/{child}/result");
        let note = match crate::git::snapshot(
            &crate::git::Repo::project(&iso.worktree),
            &result_ref,
            "harness: sub-agent result",
        )
        .await
        {
            Err(e) => format!("its changes could not be collected: {e}"),
            Ok(result) => match crate::git::apply(&iso.root, &iso.base, &result).await {
                Ok(0) => "no file changes".to_string(),
                Ok(n) => format!("{n} changed file(s) applied to the folder"),
                Err(e) => format!(
                    "its changes conflict with the folder and were NOT applied ({e}); \
                     to apply by hand: git diff {} {} | git apply",
                    iso.base, result
                ),
            },
        };
        crate::git::worktree_remove(&iso.root, &iso.worktree).await;
        note
    }

    pub(crate) fn is_running(&self) -> bool {
        self.turn
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
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

    async fn delegate(
        &self,
        agent: AgentSpec,
        task: String,
        refs: SharedRefs,
        cancel: CancellationToken,
        started: Box<dyn FnOnce(SessionId) + Send>,
    ) -> Result<harness_core::backend::DelegateOutcome, String> {
        self.run_child(agent, task, refs, cancel, started).await
    }

    fn artifacts(&self) -> Option<Arc<dyn harness_core::artifacts::ArtifactSink>> {
        let stages = self.node.stages.clone()?;
        let cwd = self.publish_cwd()?;
        let count = self
            .turn
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()?
            .publishes
            .clone();
        Some(Arc::new(crate::artifacts::Sink {
            node: self.node.clone(),
            stages,
            session: self.id,
            cwd,
            count,
        }))
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
