//! One session: the single writer of its log, the running turn, and the
//! approval gate.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use harness_core::agent::AgentSpec;
use harness_core::backend::{LogError, TurnCtx, TurnExtra, TurnHost};
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
    /// Tools a person approved in the running turn. "Allow for this turn"
    /// counts only once the tool is in here (cleared when a turn starts).
    turn_approved: Mutex<HashSet<String>>,
    /// An append of the running turn failed: the rest of its appends fail
    /// without being tried, so the turn ends (cleared when a turn starts).
    log_failed: AtomicBool,
    /// A turn's last events could not be written: its end is still missing
    /// from the log, and the next prompt writes it first.
    unclosed: AtomicBool,
    /// Approvals of the running turn whose request is in the log and whose
    /// answer is not: the turn's end denies them.
    unresolved: Mutex<Vec<String>>,
}

/// What the user sent with a prompt.
pub(crate) struct Input {
    pub text: String,
    pub attachments: Vec<Attachment>,
    /// An answer to a choices or form block: `text` is then the user's
    /// note, and the message records the canonical answer before it.
    pub reply: Option<FormReply>,
    /// Images, with their bytes (`session.prompt.images`).
    pub images: Vec<harness_protocol::ChatImage>,
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
            turn_approved: Mutex::new(HashSet::new()),
            log_failed: AtomicBool::new(false),
            unclosed: AtomicBool::new(false),
            unresolved: Mutex::new(Vec::new()),
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
            turn_approved: Mutex::new(HashSet::new()),
            log_failed: AtomicBool::new(false),
            unclosed: AtomicBool::new(false),
            unresolved: Mutex::new(Vec::new()),
        }))
    }

    /// Close out a turn that was running when the process died (PLAN.md §2).
    pub(crate) async fn recover_interrupted(&self) {
        let message = "The node restarted while this turn was running. Send a message to continue.";
        if let Err(e) = self
            .close_open_turn(message, TurnEndReason::Interrupted)
            .await
        {
            // The next prompt tries again.
            self.unclosed.store(true, Ordering::SeqCst);
            tracing::error!(session = %self.id, error = %e, "could not close the interrupted turn");
        }
    }

    /// End the turn the log shows running, if any: deny its pending
    /// approvals, say why, end it. Each event is tried once.
    async fn close_open_turn(&self, message: &str, reason: TurnEndReason) -> Result<(), String> {
        let id = self.id;
        let events = self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
            .await
            .map_err(|e| e.to_string())?;
        let view = SessionView::fold(&events).map_err(|e| e.to_string())?;
        let Some(turn_id) = view.running_turn else {
            return Ok(());
        };
        let fail = |e: StoreError| LogError(e.to_string()).to_string();
        for approval_id in view.pending_approvals {
            self.append(EventKind::ApprovalResolved(ApprovalResolved {
                approval_id,
                decision: Decision::Deny,
                by: ResolvedBy::Interrupted,
            }))
            .await
            .map_err(fail)?;
        }
        self.append(EventKind::Error(ErrorEvent {
            turn_id: Some(turn_id.clone()),
            message: message.into(),
        }))
        .await
        .map_err(fail)?;
        self.append(EventKind::TurnEnded(TurnEnded {
            turn_id,
            reason,
            usage: None,
        }))
        .await
        .map_err(fail)?;
        Ok(())
    }

    /// Assign the next seq, scrub, persist (fsync), then publish.
    async fn append(&self, kind: EventKind) -> Result<RawEvent, StoreError> {
        let mut head = self.head.lock().await;
        let (kind, mut payload) = kind.into_parts();
        if !self.node.cfg.secrets.is_empty() {
            // The string fields, not the serialized text: a secret holding a
            // quote or backslash is escaped there, and a replacement can
            // break the JSON.
            self.node.cfg.secrets.scrub_value(&mut payload);
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
        // Still under `head`, so this session's events reach the link in
        // seq order. A full link pauses the session here until the master
        // reads (or the link drops; the event is in the store either way).
        self.node
            .publish_wait(NodeToMaster::Event { event: ev.clone() })
            .await;
        Ok(ev)
    }

    /// Append one of the running turn's events. The log is written first:
    /// on `Err` the caller must not do what the event describes. The first
    /// failure stops the turn (its token is cancelled) and makes the rest of
    /// its appends fail untried; the backend then ends the turn.
    pub(crate) async fn emit(&self, kind: EventKind) -> Result<(), LogError> {
        if self.log_failed.load(Ordering::SeqCst) {
            return Err(LogError(
                "an earlier event of this turn was not written".into(),
            ));
        }
        match self.append(kind).await {
            Ok(_) => Ok(()),
            Err(e) => {
                self.note_log_failure(&e);
                if let Some(c) = self.current_cancel() {
                    c.cancel();
                }
                Err(LogError(e.to_string()))
            }
        }
    }

    /// Log a failed append, once per turn. The line names the store's
    /// error only: never the event's payload.
    fn note_log_failure(&self, e: &StoreError) {
        if !self.log_failed.swap(true, Ordering::SeqCst) {
            tracing::error!(session = %self.id, error = %e, "could not write the session log; stopping the turn");
        } else {
            tracing::debug!(session = %self.id, error = %e, "session log still not writable");
        }
    }

    /// Append one of the events that end a turn: tried once even after a
    /// failure, since a turn that cannot record its end must still end.
    async fn append_end(&self, kind: EventKind) -> bool {
        match self.append(kind).await {
            Ok(_) => true,
            Err(e) => {
                self.note_log_failure(&e);
                false
            }
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

    /// An agentic session's folder may never include the node's data
    /// directory. Checked again before every turn and restore, not only at
    /// create: sessions loaded from the log were created under older rules,
    /// and a folder can be replaced by a link after the check.
    fn check_cwd_still_allowed(&self) -> Result<(), String> {
        if self.profile != Profile::Agentic {
            return Ok(());
        }
        self.node.cfg.policy.check_session(&self.cwd)
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
        if input.reply.is_some()
            && (!input.attachments.is_empty() || !compare.is_empty() || !input.images.is_empty())
        {
            return Err(
                "an answer goes alone, without attachments, images or parallel answers".into(),
            );
        }
        if self.incognito && !input.images.is_empty() {
            return Err("images can't go into an incognito chat".into());
        }
        self.check_cwd_still_allowed()?;
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
        if self.unclosed.load(Ordering::SeqCst) {
            let message = "The session log could not be written, so this turn was stopped.";
            if let Err(e) = self.close_open_turn(message, TurnEndReason::Error).await {
                self.release(&turn_id);
                return Err(e);
            }
            self.unclosed.store(false, Ordering::SeqCst);
        }
        let msg = match self.user_message(turn_id.clone(), input).await {
            Ok(msg) => msg,
            Err(e) => {
                self.release(&turn_id);
                return Err(e);
            }
        };
        let extra = TurnExtra {
            images: msg.images.clone(),
            ..extra
        };
        // Recorded before the prompt is answered, so a log that cannot be
        // written refuses the prompt itself and the sender sees why.
        let history = match self.begin(&msg, agent.as_ref()).await {
            Ok(h) => h,
            Err(e) => {
                self.release(&turn_id);
                return Err(e);
            }
        };
        let this = self.clone();
        tokio::spawn(async move {
            this.run_turn(msg, history, agent, compare, extra, cancel)
                .await;
        });
        Ok(())
    }

    /// Start a turn in the log: read the history before it, then record
    /// `turn_started` and the user's message. Returns the history. When an
    /// append fails nothing has run, and whatever was written is closed.
    async fn begin(
        &self,
        msg: &UserMessage,
        agent: Option<&AgentSpec>,
    ) -> Result<Vec<RawEvent>, String> {
        let id = self.id;
        let history = self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
            .await
            .map_err(|e| e.to_string())?;
        self.turn_approved
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        self.log_failed.store(false, Ordering::SeqCst);
        self.unresolved
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        let started = self
            .emit(EventKind::TurnStarted(TurnStarted {
                agent: agent.map(AgentSpec::agent_ref),
                turn_id: msg.turn_id.clone(),
            }))
            .await;
        if let Err(e) = started {
            return Err(e.to_string());
        }
        if let Err(e) = self.emit(EventKind::UserMessage(msg.clone())).await {
            self.end_turn(
                &msg.turn_id,
                TurnEndReason::Error,
                None,
                Some(e.to_string()),
            )
            .await;
            return Err(e.to_string());
        }
        Ok(history)
    }

    /// Check a message's images and keep their bytes with the chat, before
    /// the message that names them is recorded.
    async fn keep_images(
        &self,
        images: Vec<harness_protocol::ChatImage>,
    ) -> Result<Vec<ImageRef>, String> {
        if images.is_empty() {
            return Ok(vec![]);
        }
        let (refs, kept) = crate::images::accept(images)?;
        let sid = self.id.to_string();
        self.node
            .cfg
            .store
            .call(move |s| s.session_images_put(&sid, &kept))
            .await
            .map_err(|e| e.to_string())?;
        Ok(refs)
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
            images,
        } = input;
        let images = self.keep_images(images).await?;
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
            images,
        })
    }

    /// Free the turn slot if `turn_id` still holds it.
    fn release(&self, turn_id: &str) {
        let mut t = self.turn.lock().unwrap_or_else(|p| p.into_inner());
        if t.as_ref().is_some_and(|t| t.turn_id == turn_id) {
            *t = None;
        }
    }

    /// Run a turn that `begin` recorded.
    async fn run_turn(
        self: Arc<Self>,
        msg: UserMessage,
        history: Vec<RawEvent>,
        agent: Option<AgentSpec>,
        compare: Vec<String>,
        extra: TurnExtra,
        cancel: CancellationToken,
    ) -> (TurnEndReason, Option<Usage>) {
        let turn_id = msg.turn_id.clone();
        let user_text = msg.model_text();
        let checkpoint = self.checkpoint(&turn_id, CheckpointPhase::Before).await;
        if self.log_failed.load(Ordering::SeqCst) {
            // The checkpoint's event was not written: nothing else runs.
            let error = LogError("the checkpoint could not be recorded".into()).to_string();
            self.end_turn(&turn_id, TurnEndReason::Error, None, Some(error))
                .await;
            return (TurnEndReason::Error, None);
        }

        // Run the backend in its own task so a panic still ends the turn.
        let this = self.clone();
        let tid = turn_id.clone();
        let c = cancel.clone();
        let res = tokio::spawn(async move {
            let backend = this.node.cfg.backend.clone();
            backend
                .run_turn(TurnCtx {
                    session_id: this.id,
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
        if let Some((root, before)) = checkpoint
            && !self.log_failed.load(Ordering::SeqCst)
        {
            self.checkpoint_after(&turn_id, &root, &before).await;
        }
        let reason = if cancel.is_cancelled() && reason != TurnEndReason::Error {
            TurnEndReason::Cancelled
        } else {
            reason
        };
        self.end_turn(&turn_id, reason, usage.clone(), error).await;
        (reason, usage)
    }

    /// Record the turn's error and end, each tried once, and free the slot.
    /// If the end could not be written, the next prompt writes it first.
    async fn end_turn(
        &self,
        turn_id: &str,
        reason: TurnEndReason,
        usage: Option<Usage>,
        error: Option<String>,
    ) {
        if let Some(message) = error {
            self.append_end(EventKind::Error(ErrorEvent {
                turn_id: Some(turn_id.into()),
                message,
            }))
            .await;
        }
        // An approval whose answer was not written would stay pending in
        // the log after the turn: deny it first. If that cannot be written
        // either, the turn stays open for the next prompt to close.
        let unresolved =
            std::mem::take(&mut *self.unresolved.lock().unwrap_or_else(|p| p.into_inner()));
        let mut closed = true;
        for approval_id in unresolved {
            closed &= self
                .append_end(EventKind::ApprovalResolved(ApprovalResolved {
                    approval_id,
                    decision: Decision::Deny,
                    by: ResolvedBy::Cancelled,
                }))
                .await;
        }
        let ended = closed
            && self
                .append_end(EventKind::TurnEnded(TurnEnded {
                    turn_id: turn_id.into(),
                    reason,
                    usage,
                }))
                .await;
        if !ended {
            self.unclosed.store(true, Ordering::SeqCst);
        }
        self.release(turn_id);
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
                // On failure the caller sees `log_failed` and stops; the
                // ref no event names goes.
                let recorded = self
                    .emit(EventKind::Checkpoint(Checkpoint {
                        turn_id: turn_id.into(),
                        phase,
                        git_ref: sha.clone(),
                        files: vec![],
                    }))
                    .await;
                if recorded.is_err() {
                    crate::git::delete_ref(&repo, &r).await;
                    return None;
                }
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
                let recorded = self
                    .emit(EventKind::Checkpoint(Checkpoint {
                        turn_id: turn_id.into(),
                        phase: CheckpointPhase::After,
                        git_ref: sha,
                        files: files.into_iter().map(|(_, p)| p).collect(),
                    }))
                    .await;
                if recorded.is_ok() {
                    crate::git::prune(repo, self.node.cfg.policy.checkpoints_keep).await;
                }
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
        self.check_cwd_still_allowed()?;
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
            self.append(EventKind::Checkpoint(Checkpoint {
                turn_id: t,
                phase: CheckpointPhase::Restored,
                git_ref: state.clone(),
                files: files.clone(),
            }))
            .await
            .map_err(|e| LogError(e.to_string()).to_string())?;
        }
        Ok(())
    }

    /// Run a delegated sub-task in a new child session and wait for it.
    async fn run_child(
        &self,
        agent: AgentSpec,
        task: String,
        refs: SharedRefs,
        observed: Arc<Vec<String>>,
        cancel: CancellationToken,
        started: Box<dyn FnOnce(SessionId) + Send>,
    ) -> Result<harness_core::backend::DelegateOutcome, String> {
        let child_id = SessionId::new();
        let short: String = task.chars().take(60).collect();
        // A sub-agent that can change files works in its own git worktree,
        // so parallel sub-agents do not trample each other.
        let (iso, iso_note) = match self.isolate(&agent, child_id).await {
            Ok(i) => (i, None),
            Err(e) => (
                None,
                Some(format!(
                    "no separate worktree ({e}); it worked in the shared folder"
                )),
            ),
        };
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
        // A sub-agent gets its task as text only, never the images.
        let msg = UserMessage {
            turn_id,
            text: task,
            attachments: vec![],
            reply: None,
            images: vec![],
        };
        let extra = TurnExtra {
            savings,
            refs: Some(refs),
            observed: Some(observed),
            ..Default::default()
        };
        let history = match child.begin(&msg, Some(&agent)).await {
            Ok(h) => h,
            Err(e) => {
                child.release(&msg.turn_id);
                if let Some(i) = iso {
                    crate::git::worktree_remove(&i.root, &i.worktree).await;
                }
                return Err(e);
            }
        };
        let (reason, usage) = child
            .clone()
            .run_turn(msg, history, Some(agent), vec![], extra, cancel)
            .await;
        // Each session's log gates its own effects: the parent's turn goes
        // on, but nothing of a child whose log failed (or whose parent's
        // did) reaches the shared folder, since no event would record it.
        let log_failed =
            child.log_failed.load(Ordering::SeqCst) || self.log_failed.load(Ordering::SeqCst);
        let merge_note = match iso {
            Some(i) => Some(self.merge_back(i, child_id, !log_failed).await),
            None => iso_note,
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
        let text = if child.log_failed.load(Ordering::SeqCst) {
            "The sub-agent stopped because its session log could not be written.".to_string()
        } else {
            text
        };
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
    /// current working state (uncommitted edits included). `Ok(None)`: no
    /// worktree is wanted (read-only agent, no repository); `Err`: one was
    /// wanted but could not be made, and the sub-agent shares the folder.
    async fn isolate(
        &self,
        agent: &AgentSpec,
        child: SessionId,
    ) -> Result<Option<Isolation>, String> {
        if self.profile != Profile::Agentic
            || !["write", "edit", "bash"]
                .iter()
                .any(|t| agent.allows_tool(t))
        {
            return Ok(None);
        }
        let Some(root) = crate::git::repo_root(&self.cwd).await else {
            return Ok(None);
        };
        let project = crate::git::Repo::project(&root);
        let res = async {
            // Before anything runs in the user's repository: its own config
            // must not define commands that `add`/checkout would run.
            crate::git::check_worktree_safe(&root).await?;
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
            Ok(i) => Ok(Some(i)),
            Err(e) => {
                tracing::warn!(session = %self.id, error = %e, "no worktree for sub-agent; sharing the folder");
                Err(e)
            }
        }
    }

    /// Bring a sub-agent's changes back into the main working directory.
    /// `apply: false` keeps the result as a ref and changes nothing in the
    /// folder.
    async fn merge_back(&self, iso: Isolation, child: SessionId, apply: bool) -> String {
        let result_ref = format!("refs/harness/{child}/result");
        let note = match crate::git::snapshot(
            &crate::git::Repo::project(&iso.worktree),
            &result_ref,
            "harness: sub-agent result",
        )
        .await
        {
            Err(e) => format!("its changes could not be collected: {e}"),
            Ok(result) if !apply => format!(
                "its changes were not applied, because a session log could not be written; \
                 to apply by hand: git diff {} {result} | git apply",
                iso.base
            ),
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
        // Sub-agent refs are the only ones left in the user's repository
        // (turn checkpoints live in the shadow); keep them bounded too.
        crate::git::prune(
            &crate::git::Repo::project(&iso.root),
            self.node.cfg.policy.checkpoints_keep,
        )
        .await;
        note
    }

    /// Record an approval's answer; `false` if the log refused it (the
    /// approval then stays in `unresolved` for the turn's end to deny).
    async fn resolve(&self, approval_id: String, decision: Decision, by: ResolvedBy) -> bool {
        let written = self
            .emit(EventKind::ApprovalResolved(ApprovalResolved {
                approval_id: approval_id.clone(),
                decision,
                by,
            }))
            .await
            .is_ok();
        if written {
            self.unresolved
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .retain(|a| *a != approval_id);
        }
        written
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
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        Session::emit(self, kind).await
    }

    async fn delegate(
        &self,
        agent: AgentSpec,
        task: String,
        refs: SharedRefs,
        observed: Arc<Vec<String>>,
        cancel: CancellationToken,
        started: Box<dyn FnOnce(SessionId) + Send>,
    ) -> Result<harness_core::backend::DelegateOutcome, String> {
        self.run_child(agent, task, refs, observed, cancel, started)
            .await
    }

    async fn images(&self, sha256: &[String]) -> Vec<harness_core::backend::ImageData> {
        use base64::Engine as _;
        let (sid, wanted) = (self.id.to_string(), sha256.to_vec());
        match self
            .node
            .cfg
            .store
            .call(move |s| s.session_images(&sid, &wanted))
            .await
        {
            Ok(found) => found
                .into_iter()
                .map(|i| harness_core::backend::ImageData {
                    data: base64::engine::general_purpose::STANDARD
                        .encode(&i.bytes)
                        .into(),
                    sha256: i.sha256,
                    mime: i.mime,
                })
                .collect(),
            Err(e) => {
                // The model is then told the images' names instead.
                tracing::warn!(session = %self.id, error = %e, "could not read the chat's images");
                Vec::new()
            }
        }
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
            // Record the refusal so the user sees why. Denied either way.
            let requested = self
                .emit(EventKind::ApprovalRequested(ApprovalRequested {
                    approval_id: approval_id.clone(),
                    call_id: call_id.into(),
                    tool: tool.into(),
                    input: input.clone(),
                    reason: format!("`{tool}` is denied by this node's policy"),
                    turn_allow: false,
                }))
                .await;
            if requested.is_ok() {
                self.unresolved
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push(approval_id.clone());
                self.resolve(approval_id, Decision::Deny, ResolvedBy::Policy)
                    .await;
            }
            return Decision::Deny;
        }
        if !self.mode.needs_approval(risk) {
            return Decision::Allow;
        }
        // "For this turn" is offered, and honoured, only after this tool has
        // been approved once in the turn, so its first call is always seen.
        let turn_allow = self
            .turn_approved
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains(tool);
        let approval_id = new_id();
        let (tx, rx) = oneshot::channel();
        self.approvals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(approval_id.clone(), tx);
        let requested = self
            .emit(EventKind::ApprovalRequested(ApprovalRequested {
                approval_id: approval_id.clone(),
                call_id: call_id.into(),
                tool: tool.into(),
                input: input.clone(),
                reason: approval_reason(tool, risk),
                turn_allow,
            }))
            .await;
        if requested.is_ok() {
            self.unresolved
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(approval_id.clone());
        } else {
            // Nobody can see a request the log does not hold; the turn is
            // already stopping.
            self.approvals
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&approval_id);
            return Decision::Deny;
        }
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
        // Whatever the client sent: not offered, not honoured.
        let decision = match decision {
            Decision::AllowSession if !turn_allow => Decision::Allow,
            d => d,
        };
        // An approval the log does not hold allows nothing.
        if !self.resolve(approval_id, decision, by).await {
            return Decision::Deny;
        }
        if decision.allows() {
            self.turn_approved
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(tool.to_string());
        }
        decision
    }
}

/// What an approval card says the tool would do. Publishing changes nothing
/// on this node, it sends a copy away, so it says that instead.
fn approval_reason(tool: &str, risk: ToolRisk) -> String {
    match (tool, risk) {
        ("publish", _) => "`publish` uploads a copy of a file from this folder to Artifacts".into(),
        (_, ToolRisk::Mutating) => format!("`{tool}` can change files or run programs"),
        (_, ToolRisk::ReadOnly) => format!("approval mode requires asking for `{tool}`"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_cards_say_what_the_tool_does() {
        let publish = approval_reason("publish", ToolRisk::Mutating);
        assert!(
            publish.contains("Artifacts") && !publish.contains("run programs"),
            "{publish}"
        );
        assert_eq!(
            approval_reason("bash", ToolRisk::Mutating),
            "`bash` can change files or run programs"
        );
    }
}
