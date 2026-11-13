//! One session: the single writer of its log, the running turn, and the
//! approval gate.

use std::collections::HashMap;
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
    /// Set when the node's stop cancelled this turn (and nothing had
    /// before it): its end is then `stopped` (review N-7).
    by_stop: Arc<AtomicBool>,
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
    /// A chat that never gets a callback turn (`session_created`).
    no_callbacks: bool,
    turn: Mutex<Option<RunningTurn>>,
    approvals: Mutex<HashMap<String, oneshot::Sender<Decision>>>,
    /// An append of the running turn failed: the rest of its appends fail
    /// without being tried, so the turn ends (cleared when a turn starts).
    log_failed: AtomicBool,
    /// A turn's last events could not be written: its end is still missing
    /// from the log, and the next prompt writes it first.
    unclosed: AtomicBool,
    /// Approvals of the running turn whose request is in the log and whose
    /// answer is not: the turn's end denies them.
    unresolved: Mutex<Vec<String>>,
    /// The folder's last recorded state in this session (a turn's `after`
    /// or a restore's result): the next turn copies what the person changed
    /// since then (A.17 review, F5).
    last_state: Mutex<Option<String>>,
    /// The folder pinned at creation (or at load after a restart): its
    /// canonical path, device and inode. `None` for a chat session, and for
    /// an agentic one whose folder could not be pinned (it then refuses to
    /// work). Checked by [`Self::checked_folder`] before every use.
    pin: Option<harness_core::folder::FolderPin>,
    /// A sub-agent's node-owned folder (`git::ChildFolder`): the session
    /// works in it although it lies in the node's protected sessions
    /// folder. Not kept across a restart (the folder is removed when the
    /// sub-agent ends).
    owned: Option<harness_core::folder::OwnedFolder>,
    /// This session's background commands (T.8d): an agentic session that
    /// is not a sub-agent's. Dropping it stops every one.
    bg: Option<Arc<harness_tools::background::SessionBg>>,
    /// Ends that came while a turn ran: recorded at its next step boundary
    /// (`delivered`), or after its end.
    bg_queue: Mutex<Vec<harness_tools::background::Ended>>,
    /// Wakes the end recorder when the turn slot is freed.
    bg_flush: Arc<tokio::sync::Notify>,
    /// The last try to record a queued end failed (T8D-6).
    bg_failed: AtomicBool,
}

type BgEnds = tokio::sync::mpsc::UnboundedReceiver<harness_tools::background::Ended>;

/// A session's background table and the receiver of its ends, where it
/// gets one; `next` is the number its next id takes.
fn background_table(
    node: &Inner,
    profile: Profile,
    parent: Option<SessionId>,
    next: u64,
) -> (
    Option<Arc<harness_tools::background::SessionBg>>,
    Option<BgEnds>,
) {
    if profile != Profile::Agentic || parent.is_some() {
        return (None, None);
    }
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (Some(node.background.session(next, tx)), Some(rx))
}

/// Whether `bg_ids` may resume a chat whose log is `history` (T.8d step
/// 3, §4.4 step 5): each must name an end that may call back
/// ([`BackgroundEnded::calls_back`]) recorded after the chat's last turn
/// and its last refused callback. `Ok` holds the callback turns since the
/// person's last message (the budget spent); `Err` says what is not there,
/// behind [`harness_protocol::CALLBACK_NOTHING`].
fn callback_check(history: &[RawEvent], bg_ids: &[String]) -> Result<usize, String> {
    use harness_protocol::CALLBACK_NOTHING;
    let mut floor = 0u64;
    let mut in_a_row = 0usize;
    let mut waiting: Vec<String> = Vec::new();
    for e in history {
        match e.typed() {
            Ok(EventKind::TurnStarted(t)) => {
                floor = e.seq;
                waiting.clear();
                if t.cause.is_some() {
                    in_a_row += 1;
                }
            }
            Ok(EventKind::UserMessage(_)) => in_a_row = 0,
            Ok(EventKind::CallbackRefused(_)) => {
                floor = e.seq;
                waiting.clear();
            }
            Ok(EventKind::BackgroundEnded(b)) if e.seq > floor && b.calls_back() => {
                waiting.push(b.bg_id)
            }
            _ => {}
        }
    }
    for id in bg_ids {
        if !waiting.contains(id) {
            return Err(format!(
                "{CALLBACK_NOTHING}: {id} has no end in this chat that waits for a turn"
            ));
        }
    }
    Ok(in_a_row)
}

/// Pin an agentic session's folder (see [`Session::pin`]).
fn pin_of(profile: Profile, cwd: &std::path::Path) -> Option<harness_core::folder::FolderPin> {
    if profile != Profile::Agentic {
        return None;
    }
    match harness_core::folder::FolderPin::at(cwd) {
        Ok(p) => Some(p),
        Err(e) => {
            tracing::warn!(cwd = %cwd.display(), error = %e, "a session's folder could not be pinned");
            None
        }
    }
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

/// A sub-agent's private, node-owned folder.
struct Isolation {
    sub: crate::git::SubRepo,
    child: crate::git::ChildFolder,
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
    /// A sub-agent's node-owned folder, when it has one.
    pub owned: Option<harness_core::folder::OwnedFolder>,
    /// Never resumed by a callback turn (a routine's chat).
    pub no_callbacks: bool,
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
            owned,
            no_callbacks,
        } = new;
        let (bg, ends) = background_table(&node, profile, parent_session, 1);
        let s = Arc::new(Self {
            bg,
            bg_queue: Mutex::new(Vec::new()),
            bg_flush: Arc::default(),
            bg_failed: AtomicBool::new(false),
            node: node.clone(),
            id,
            head: tokio::sync::Mutex::new(0),
            cwd: cwd.clone(),
            model: model.clone(),
            mode,
            profile,
            incognito,
            parent_session,
            no_callbacks,
            turn: Mutex::new(None),
            approvals: Mutex::new(HashMap::new()),
            log_failed: AtomicBool::new(false),
            unclosed: AtomicBool::new(false),
            unresolved: Mutex::new(Vec::new()),
            last_state: Mutex::new(None),
            pin: pin_of(profile, &cwd),
            owned,
        });
        Self::record_ends(&s, ends);
        s.append(EventKind::SessionCreated(SessionCreated {
            no_callbacks,
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
        let last_state = events.iter().rev().find_map(|e| match e.typed() {
            Ok(EventKind::Checkpoint(c))
                if !matches!(
                    c.phase,
                    CheckpointPhase::Before | CheckpointPhase::BeforeRestore
                ) =>
            {
                Some(c.git_ref)
            }
            _ => None,
        });
        // After a restart the inode is taken now, from the path the log
        // recorded (canonical when the session was made): a folder that now
        // leads elsewhere is not pinned, and the session refuses to work.
        let pin = pin_of(meta.profile, std::path::Path::new(&meta.cwd));
        // Ids go on from the log's.
        let next = view
            .background
            .keys()
            .filter_map(|k| k.strip_prefix("bg-")?.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        let (bg, ends) = background_table(&node, meta.profile, meta.parent_session, next);
        let s = Arc::new(Self {
            bg,
            bg_queue: Mutex::new(Vec::new()),
            bg_flush: Arc::default(),
            bg_failed: AtomicBool::new(false),
            node,
            id,
            head: tokio::sync::Mutex::new(view.head_seq),
            cwd: PathBuf::from(meta.cwd),
            model: meta.model,
            mode,
            profile: meta.profile,
            incognito: meta.incognito,
            parent_session: meta.parent_session,
            no_callbacks: meta.no_callbacks,
            turn: Mutex::new(None),
            approvals: Mutex::new(HashMap::new()),
            log_failed: AtomicBool::new(false),
            unclosed: AtomicBool::new(false),
            unresolved: Mutex::new(Vec::new()),
            last_state: Mutex::new(last_state),
            pin,
            owned: None,
        });
        Self::record_ends(&s, ends);
        Ok(s)
    }

    /// The task that records this session's background ends, one at a
    /// time: at once when no turn runs, else queued for the turn's next
    /// step boundary or its end. It holds the session weakly and ends with
    /// its table.
    fn record_ends(s: &Arc<Self>, ends: Option<BgEnds>) {
        let Some(mut rx) = ends else { return };
        let weak = Arc::downgrade(s);
        let flush = s.bg_flush.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    m = rx.recv() => {
                        let Some(e) = m else { break };
                        let Some(s) = weak.upgrade() else { break };
                        s.on_background_end(e).await;
                    }
                    _ = flush.notified() => {
                        let Some(s) = weak.upgrade() else { break };
                        s.flush_background().await;
                    }
                }
            }
        });
    }

    async fn on_background_end(&self, e: harness_tools::background::Ended) {
        let Some(bg) = &self.bg else { return };
        if e.reason == BackgroundReason::SessionDeleted {
            bg.recorded();
            return;
        }
        {
            let turn = self.turn.lock().unwrap_or_else(|p| p.into_inner());
            if turn.is_some() {
                self.bg_queue
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push(e);
                return;
            }
        }
        self.record_background(vec![e]).await;
    }

    /// Ends queued during a turn, once no turn runs.
    async fn flush_background(&self) {
        let ends = {
            let turn = self.turn.lock().unwrap_or_else(|p| p.into_inner());
            if turn.is_some() {
                return;
            }
            std::mem::take(&mut *self.bg_queue.lock().unwrap_or_else(|p| p.into_inner()))
        };
        self.record_background(ends).await;
    }

    /// Record ends outside a turn (`delivered: false`: the model reads them
    /// in the next turn's history). One that cannot be written stays queued
    /// for the next try; it is never dropped silently.
    async fn record_background(&self, ends: Vec<harness_tools::background::Ended>) {
        let Some(bg) = &self.bg else { return };
        for e in ends {
            match self
                .append(EventKind::BackgroundEnded(e.event(false)))
                .await
            {
                Ok(_) => {
                    bg.recorded();
                    self.bg_failed.store(false, Ordering::SeqCst);
                }
                Err(err) => {
                    self.bg_failed.store(true, Ordering::SeqCst);
                    tracing::error!(session = %self.id, error = %err, bg = %e.bg_id, "could not record a background command's end; kept for the next try");
                    self.bg_queue
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .push(e);
                }
            }
        }
    }

    /// After a restart: every background command the log shows running
    /// died with the node (its guard ends it when its node is gone, the
    /// start's sweep the rest). Recorded as `node_restarted`; no turn
    /// starts for it (R12).
    pub(crate) async fn recover_background(&self) {
        let id = self.id;
        let events = match self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
            .await
        {
            Ok(e) => e,
            Err(e) => {
                tracing::error!(session = %self.id, error = %e, "could not read the log for background commands");
                return;
            }
        };
        let Ok(view) = SessionView::fold(&events) else {
            return;
        };
        for bg_id in view.running_background() {
            let ended = BackgroundEnded {
                bg_id,
                reason: BackgroundReason::NodeRestarted,
                exit_code: None,
                signal: None,
                confirmed: BackgroundConfirmed::Unknown,
                ran_ms: 0,
                output_bytes: 0,
                tail: String::new(),
                tail_cut: false,
                delivered: false,
            };
            if let Err(e) = self.append(EventKind::BackgroundEnded(ended)).await {
                tracing::error!(session = %self.id, error = %e, "could not record a background command's end at start");
            }
        }
    }

    /// Stop one background command (`bg_id`) or all, for the master: the
    /// person's stop (step 2) or the revocation sweep. Not waited for.
    pub(crate) fn stop_background(
        &self,
        bg_id: Option<&str>,
        by: harness_protocol::StopBackgroundBy,
    ) -> Result<(), String> {
        let bg = self
            .bg
            .as_ref()
            .ok_or("this chat has no background commands")?;
        let reason = match by {
            harness_protocol::StopBackgroundBy::User => BackgroundReason::StoppedByUser,
            harness_protocol::StopBackgroundBy::AccessRevoked => BackgroundReason::AccessRevoked,
        };
        match (bg_id, by) {
            (Some(id), _) => bg.stop(id, reason).map(|_| ()),
            // A revoked owner's chat starts nothing more until they prompt
            // it again (a start in its first wait is stopped when handed
            // over).
            (None, harness_protocol::StopBackgroundBy::AccessRevoked) => {
                bg.close(reason);
                Ok(())
            }
            (None, harness_protocol::StopBackgroundBy::User) => {
                bg.stop_all(reason);
                Ok(())
            }
        }
    }

    /// Order every background command to stop; not waited for.
    pub(crate) fn stop_background_now(&self, reason: BackgroundReason) {
        if let Some(bg) = &self.bg {
            bg.stop_all(reason);
        }
    }

    /// Stop every background command and wait for their ends, at most
    /// `bound`.
    pub(crate) async fn stop_background_within(
        &self,
        reason: BackgroundReason,
        bound: std::time::Duration,
    ) {
        if let Some(bg) = &self.bg {
            bg.stop_all_within(reason, bound).await;
        }
    }

    /// A background command runs or a start is being handed over.
    pub(crate) fn background_active(&self) -> bool {
        self.bg.as_ref().is_some_and(|b| b.active())
    }

    /// Ended background commands whose guard has not exited yet.
    pub(crate) fn background_stuck(&self) -> Vec<String> {
        self.bg.as_ref().map(|b| b.stuck()).unwrap_or_default()
    }

    /// Ends sent and not recorded yet (a stop waits for them).
    pub(crate) fn background_unrecorded(&self) -> bool {
        self.bg.as_ref().is_some_and(|b| b.unrecorded() > 0)
    }

    /// Try the queued ends again, when no turn runs (a stop's pass,
    /// T8D-6). True when an end is still not recorded and the last try
    /// failed: the log does not take it, and waiting longer buys nothing.
    pub(crate) async fn retry_background(&self) -> bool {
        self.flush_background().await;
        self.background_unrecorded() && self.bg_failed.load(Ordering::SeqCst)
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
        if let AppendOutcome::Refused { reason, .. } = out {
            // An event the log does not take (too large, say): nothing was
            // stored, so the turn stops on a log failure and the next one
            // starts from the same head.
            return Err(StoreError::Refused(format!(
                "the session log refused an event: {reason}"
            )));
        }
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

    /// An agentic session's folder, checked: still the directory pinned
    /// when the session was made (path, device and inode), still allowed
    /// by node.toml (inside `workspace_roots`, not holding or inside the
    /// data directory) and the sandbox still works. Checked before every
    /// turn, restore, delegation, publish and checkpoint, not only at
    /// create: sessions loaded from the log were made under older rules,
    /// and a folder can be replaced by a link after any check (R.1, T4).
    /// For a chat session: its (empty) folder, unchecked.
    pub(crate) fn checked_folder(&self) -> Result<PathBuf, String> {
        if self.profile != Profile::Agentic {
            return Ok(self.cwd.clone());
        }
        let dir = match &self.pin {
            Some(pin) => pin.check()?,
            None => {
                return Err(format!(
                    "{}: {} is not the directory this session was created in",
                    harness_core::folder::REPLACED,
                    self.cwd.display()
                ));
            }
        };
        // A sub-agent's node-owned folder: inside it, and the sandbox still
        // works; it is in the protected sessions folder by design.
        if let Some(o) = &self.owned {
            if let Some(why) = &self.node.cfg.policy.sandbox_refusal {
                return Err(why.clone());
            }
            if !dir.starts_with(&o.top) {
                return Err(format!(
                    "{}: {} is outside the sub-agent's folder",
                    harness_core::folder::REPLACED,
                    dir.display()
                ));
            }
            return Ok(dir);
        }
        self.node.cfg.policy.check_session(&dir)?;
        Ok(dir)
    }

    /// Where sub-agents' folders are made: `<data>/sessions`, beside the
    /// checkpoint store, protected like the data directory.
    fn sessions_dir(&self) -> Option<PathBuf> {
        crate::sessions_dir(self.node.cfg.checkpoints_dir.as_deref()?)
    }

    fn check_cwd_still_allowed(&self) -> Result<(), String> {
        self.checked_folder().map(|_| ())
    }

    /// What bounds this session's checkpoint root and sub-agent repository
    /// (`dir` is its checked folder).
    fn bounds(&self, dir: &std::path::Path) -> crate::git::Bounds {
        let policy = &self.node.cfg.policy;
        crate::git::Bounds {
            ceiling: policy.root_of(dir).ok(),
            protected: policy.protected_dirs.clone(),
            sandbox: self.node.cfg.git_sandbox.clone(),
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
        // The owner prompts the chat: it may start background commands
        // again after a revocation closed its table (the master admits the
        // prompt only for an owner with access).
        if let Some(bg) = &self.bg {
            bg.reopen();
        }
        // The node decides how far delegation goes (Gate 3 review P-2): the
        // master's spec may ask for less than `[limits]`, never more. Its
        // sub-agents' specs come from this one, so they are bounded too.
        let limits = &self.node.cfg.policy.limits;
        let agent = agent.map(|a| limits.bound_delegation(a));
        let turn_id = new_id();
        let cancel = CancellationToken::new();
        let by_stop = Arc::new(AtomicBool::new(false));
        self.claim_turn(RunningTurn {
            turn_id: turn_id.clone(),
            cancel: cancel.clone(),
            savings: extra.savings.clone(),
            publishes: Arc::default(),
            by_stop: by_stop.clone(),
        })?;
        // Claimed after a stop began: refused here, since the stop's
        // cancel may have passed this session already (A.19).
        if self.node.is_stopping() {
            self.release(&turn_id);
            return Err(harness_protocol::NODE_STOPPING.into());
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
        let (turn_id, user_text) = (msg.turn_id.clone(), msg.model_text());
        tokio::spawn(async move {
            this.run_turn(
                turn_id, user_text, history, agent, compare, extra, cancel, by_stop,
            )
            .await;
        });
        Ok(())
    }

    /// Resume this idle chat because background commands ended (a callback
    /// turn, T.8d step 3), as the master asks. The node decides: it starts
    /// the turn only if every id names an end in its own log that may call
    /// back (it exited or timed out, and no step was told of it) with no
    /// turn and no refusal after it, the chat may take callback turns at
    /// all, and its budget of turns without the person's message lasts.
    /// A refusal on the node's own rules is recorded (`callback_refused`)
    /// and the ends wait for the person; one that finds nothing to resume
    /// says so ([`harness_protocol::CALLBACK_NOTHING`]) and records
    /// nothing. Nothing in the command widens a bound: the step cap is
    /// `[limits]`, the approval mode is the session's (clamped at load),
    /// delegation is off, and a table a revocation closed stays closed.
    pub(crate) async fn callback(
        self: &Arc<Self>,
        bg_ids: Vec<String>,
        agent: Option<AgentSpec>,
        savings: harness_core::savings::Savings,
    ) -> Result<(), String> {
        use harness_protocol::{CALLBACK_NOTHING, MAX_CALLBACK_IDS};
        let mut wanted = bg_ids.clone();
        wanted.sort();
        wanted.dedup();
        if bg_ids.is_empty() || bg_ids.len() > MAX_CALLBACK_IDS || wanted.len() != bg_ids.len() {
            return Err(format!(
                "a callback names 1 to {MAX_CALLBACK_IDS} different background commands"
            ));
        }
        if self.parent_session.is_some() || self.bg.is_none() {
            return Err(format!(
                "{CALLBACK_NOTHING}: this chat runs no background commands"
            ));
        }
        self.check_cwd_still_allowed()?;
        let limits = &self.node.cfg.policy.limits;
        let turn_id = new_id();
        let cancel = CancellationToken::new();
        let by_stop = Arc::new(AtomicBool::new(false));
        self.claim_turn(RunningTurn {
            turn_id: turn_id.clone(),
            cancel: cancel.clone(),
            savings: savings.clone(),
            publishes: Arc::default(),
            by_stop: by_stop.clone(),
        })
        .map_err(|e| {
            if e == "a turn is already running" {
                format!("{CALLBACK_NOTHING}: {e}")
            } else {
                e
            }
        })?;
        // Under the slot: no other turn starts and every end that comes
        // meanwhile is queued, so the log read here is the log the turn
        // joins.
        if self.node.is_stopping() {
            self.release(&turn_id);
            return Err(harness_protocol::NODE_STOPPING.into());
        }
        if self.unclosed.load(Ordering::SeqCst) {
            let message = "The session log could not be written, so this turn was stopped.";
            if let Err(e) = self.close_open_turn(message, TurnEndReason::Error).await {
                self.release(&turn_id);
                return Err(e);
            }
            self.unclosed.store(false, Ordering::SeqCst);
        }
        let id = self.id;
        let history = match self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
            .await
        {
            Ok(h) => h,
            Err(e) => {
                self.release(&turn_id);
                return Err(e.to_string());
            }
        };
        let refusal = match callback_check(&history, &bg_ids) {
            Err(e) => {
                self.release(&turn_id);
                return Err(e);
            }
            Ok(in_a_row) => {
                if self.no_callbacks {
                    Some("this chat takes no callback turns (a routine's chat)".to_string())
                } else if self.bg.as_ref().is_some_and(|b| b.is_closed()) {
                    // A revocation closed the table; only the owner's own
                    // prompt opens it again.
                    Some("its owner lost access; nothing resumes it until they write".to_string())
                } else if limits.callback_turns == 0 {
                    Some("callback turns are off on this node (callback_turns = 0)".to_string())
                } else if in_a_row >= limits.callback_turns {
                    Some(format!(
                        "{in_a_row} turns in a row without you (node limit)"
                    ))
                } else {
                    None
                }
            }
        };
        if let Some(reason) = refusal {
            let recorded = self
                .append(EventKind::CallbackRefused(CallbackRefused {
                    bg_ids,
                    reason: reason.clone(),
                }))
                .await;
            self.release(&turn_id);
            recorded.map_err(|e| e.to_string())?;
            return Err(format!("{}: {reason}", harness_protocol::CALLBACK_REFUSED));
        }
        // No sub-agents in a turn nobody watches: the unattended spend is
        // `callback_turns × callback_max_steps` model calls, nothing more.
        let agent = agent.map(|a| {
            let mut a = limits.bound_delegation(a);
            a.delegates.clear();
            a
        });
        let cause = TurnCause::Callback(bg_ids);
        self.log_failed.store(false, Ordering::SeqCst);
        self.unresolved
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        if let Err(e) = self
            .emit(EventKind::TurnStarted(TurnStarted {
                turn_id: turn_id.clone(),
                agent: agent.as_ref().map(AgentSpec::agent_ref),
                cause: Some(cause.clone()),
            }))
            .await
        {
            self.release(&turn_id);
            return Err(e.to_string());
        }
        let extra = TurnExtra {
            savings,
            cause: Some(cause),
            max_steps: Some(limits.callback_steps()),
            ..Default::default()
        };
        let this = self.clone();
        tokio::spawn(async move {
            this.run_turn(
                turn_id,
                String::new(),
                history,
                agent,
                vec![],
                extra,
                cancel,
                by_stop,
            )
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
        self.log_failed.store(false, Ordering::SeqCst);
        self.unresolved
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        let started = self
            .emit(EventKind::TurnStarted(TurnStarted {
                cause: None,
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
            // Ends queued while it ran are recorded after it.
            self.bg_flush.notify_one();
        }
    }

    /// Run a turn that `begin` recorded.
    #[allow(clippy::too_many_arguments)]
    async fn run_turn(
        self: Arc<Self>,
        turn_id: String,
        user_text: String,
        history: Vec<RawEvent>,
        agent: Option<AgentSpec>,
        compare: Vec<String>,
        extra: TurnExtra,
        cancel: CancellationToken,
        by_stop: Arc<AtomicBool>,
    ) -> (TurnEndReason, Option<Usage>) {
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
        let mut collect = None;
        if let Some((root, before)) = checkpoint
            && !self.log_failed.load(Ordering::SeqCst)
        {
            self.checkpoint_after(&turn_id, &root, &before).await;
            collect = Some(root);
        }
        // The end is decided by who cancelled (review N-7): the node's stop
        // (`stopped`), or the person or the system before it
        // (`cancelled`). A turn the model had completed keeps `completed`
        // when the stop came during its after-turn checkpoint.
        let reason = match (cancel.is_cancelled(), reason) {
            (true, TurnEndReason::Error) | (false, _) => reason,
            (true, TurnEndReason::Completed) if by_stop.load(Ordering::SeqCst) => reason,
            (true, _) if by_stop.load(Ordering::SeqCst) => TurnEndReason::Stopped,
            (true, _) => TurnEndReason::Cancelled,
        };
        self.end_turn(&turn_id, reason, usage.clone(), error).await;
        // The checkpoint repository's garbage collection, when due, runs
        // after the turn has ended (A.18).
        if let Some(repo) = collect {
            crate::git::gc_in_background(repo);
        }
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
    /// `Ok(None)`: this session takes none (a chat, checkpoints off, a
    /// sub-agent's folder); `Err`: it should, and cannot, with the reason.
    async fn checkpoint_repo(&self) -> Result<Option<crate::git::Repo>, String> {
        // A sub-agent's folder is removed when it ends: no checkpoints.
        if self.profile != Profile::Agentic
            || self.node.cfg.policy.checkpoints_keep == 0
            || self.owned.is_some()
        {
            return Ok(None);
        }
        let dir = self.checked_folder()?;
        crate::git::checkpoint_repo(
            &dir,
            self.node.cfg.checkpoints_dir.as_deref(),
            &self.bounds(&dir),
        )
        .await
    }

    /// Snapshot the workspace before a turn (agentic sessions).
    async fn checkpoint(
        &self,
        turn_id: &str,
        phase: CheckpointPhase,
    ) -> Option<(crate::git::Repo, String)> {
        let repo = match self.checkpoint_repo().await {
            Ok(r) => r?,
            Err(e) => {
                tracing::warn!(session = %self.id, error = %e, "no checkpoint");
                self.no_checkpoint_notice(turn_id, &e).await;
                return None;
            }
        };
        let r = format!("refs/harness/{}/{turn_id}/before", self.id);
        let busy = crate::git::hold(&repo).await;
        let snap = crate::git::snapshot_full(&repo, &r, "harness: before turn").await;
        drop(busy);
        match snap {
            Ok(snap) => {
                let sha = snap.sha;
                // On failure the caller sees `log_failed` and stops; the
                // ref no event names goes.
                let recorded = self
                    .emit(EventKind::Checkpoint(Checkpoint {
                        turn_id: turn_id.into(),
                        phase,
                        git_ref: sha.clone(),
                        files: vec![],
                        excluded: snap.nested.shown,
                        excluded_more: snap.nested.more as u64,
                    }))
                    .await;
                if recorded.is_err() {
                    crate::git::delete_ref(&repo, &r).await;
                    return None;
                }
                self.pin_between_turns(turn_id, &repo, &sha).await;
                Some((repo, sha))
            }
            Err(e) => {
                tracing::warn!(session = %self.id, error = %e, "checkpoint failed");
                self.no_checkpoint_notice(turn_id, &e).await;
                None
            }
        }
    }

    /// Tell the person, in the chat, that this turn has no checkpoint (so
    /// no undo), with the reason; an `error` note, as the chat shows today.
    /// A failed append sets `log_failed`, which ends the turn as for any
    /// other event.
    async fn no_checkpoint_notice(&self, turn_id: &str, reason: &str) {
        let _ = self
            .emit(EventKind::Error(ErrorEvent {
                turn_id: Some(turn_id.into()),
                message: format!(
                    "This turn has no checkpoint, so it cannot be undone: the folder could \
                     not be saved ({reason})."
                ),
            }))
            .await;
    }

    async fn checkpoint_after(&self, turn_id: &str, repo: &crate::git::Repo, before: &str) {
        let r = format!("refs/harness/{}/{turn_id}/after", self.id);
        let busy = crate::git::hold(repo).await;
        let res = async {
            // The folder replaced during the turn: nothing of what is there
            // now is snapshotted or named in the chat.
            self.checked_folder()?;
            let snap = crate::git::snapshot_full(repo, &r, "harness: after turn").await?;
            let files = crate::git::changed(repo, before, &snap.sha).await?;
            Ok::<_, String>((snap, files))
        }
        .await;
        drop(busy);
        match res {
            Ok((snap, files)) => {
                let sha = snap.sha.clone();
                let ignore_changed: Vec<String> = files
                    .iter()
                    .map(|(_, p)| p)
                    .filter(|p| p.as_str() == ".gitignore" || p.ends_with("/.gitignore"))
                    .cloned()
                    .collect();
                let recorded = self
                    .emit(EventKind::Checkpoint(Checkpoint {
                        turn_id: turn_id.into(),
                        phase: CheckpointPhase::After,
                        git_ref: snap.sha,
                        files: files.into_iter().map(|(_, p)| p).collect(),
                        excluded: snap.nested.shown,
                        excluded_more: snap.nested.more as u64,
                    }))
                    .await;
                if recorded.is_ok() {
                    // S-6: files that ignore rules written during this turn
                    // now leave out are in neither snapshot, so an undo of
                    // the turn leaves them; say so.
                    if !ignore_changed.is_empty() {
                        let _ = self
                            .emit(EventKind::Error(ErrorEvent {
                                turn_id: Some(turn_id.into()),
                                message: format!(
                                    "This turn changed {}: files the new rules ignore are \
                                     not in this turn's checkpoint, so undoing the turn does \
                                     not remove the ones it made.",
                                    ignore_changed
                                        .iter()
                                        .take(crate::git::SHOWN_MAX)
                                        .map(|p| format!("`{p}`"))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                ),
                            }))
                            .await;
                    }
                    let group = format!("refs/harness/{}/{turn_id}", self.id);
                    self.pin_turn(turn_id, repo, &group, before, &sha).await;
                    *self.last_state.lock().unwrap_or_else(|p| p.into_inner()) = Some(sha);
                    crate::git::prune(repo, self.node.cfg.policy.checkpoints_keep).await;
                }
            }
            Err(e) => {
                tracing::warn!(session = %self.id, error = %e, "checkpoint failed");
                self.no_checkpoint_notice(turn_id, &e).await;
            }
        }
    }

    /// Copy what an undo of this turn needs into the checkpoint repository's
    /// own store (A.17, `git::pin_changed`), so it no longer depends on the
    /// user's repository keeping the old versions. When some are already
    /// gone (the turn itself rewrote history and pruned it) or the copy
    /// fails, the chat says the undo is incomplete, naming the files; the
    /// turn still completes. A failed append sets `log_failed` as for any
    /// other event.
    async fn pin_turn(
        &self,
        turn_id: &str,
        repo: &crate::git::Repo,
        group: &str,
        before: &str,
        after: &str,
    ) {
        let busy = crate::git::hold(repo).await;
        let pinned = crate::git::pin_changed(repo, group, before, after).await;
        drop(busy);
        let message = match pinned {
            Ok(p) if p.trees_gone => "This turn's undo is incomplete: the folder's earlier \
                 content was already gone from the git repository in this folder when the \
                 turn ended (its history was rewritten and cleaned up during the turn), so \
                 this turn cannot be undone."
                .to_string(),
            Ok(p) if p.missing.is_empty() => return,
            Ok(p) => format!(
                "This turn's undo is incomplete: the earlier versions of {} were already gone \
                 from the git repository in this folder when the turn ended (its history was \
                 rewritten and cleaned up during the turn), so undo cannot put them back and \
                 will refuse this turn.",
                p.missing.listed()
            ),
            Err(e) => {
                tracing::warn!(session = %self.id, error = %e, "checkpoint copies failed");
                format!(
                    "This turn's undo is incomplete: the earlier versions of the files it \
                     changed could not be copied into the checkpoint store ({e}), so undo \
                     depends on the git repository in this folder keeping them."
                )
            }
        };
        let _ = self
            .emit(EventKind::Error(ErrorEvent {
                turn_id: Some(turn_id.into()),
                message,
            }))
            .await;
    }

    /// What the person changed in the folder since this session's last
    /// recorded state (their own edits and commits between turns) is copied
    /// like a turn's changes, in this turn's group, so a rewind across it
    /// does not depend on the user's repository either (A.17 review, F5).
    /// One tree diff of two snapshots that exist; no scan. A loss here is
    /// only logged: it is the person's own history, not this turn's.
    async fn pin_between_turns(&self, turn_id: &str, repo: &crate::git::Repo, before: &str) {
        let Some(prev) = self
            .last_state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        else {
            return;
        };
        let group = format!("refs/harness/{}/{turn_id}", self.id);
        let busy = crate::git::hold(repo).await;
        let pinned = crate::git::pin_changed(repo, &group, &prev, before).await;
        drop(busy);
        match pinned {
            Ok(p) if !p.missing.is_empty() => tracing::warn!(
                session = %self.id, missing = %p.missing.listed(),
                "changes between turns: content already gone, not copied"
            ),
            Ok(_) => {}
            Err(e) => {
                tracing::debug!(session = %self.id, error = %e, "changes between turns not copied")
            }
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
    /// turn (see `git::restore_plan`). In order:
    /// 1. the diff is read (a checkpoint whose tree is gone is refused);
    /// 2. the folder as it is now is snapshotted (the ref only);
    /// 3. the plan is made (the unforced checks), and exactly the blobs it
    ///    writes are checked and made safe (`git::secure_blobs`); a
    ///    refusal in 1 to 3 deletes the ref and logs nothing;
    /// 4. the snapshot is recorded as a `before_restore` checkpoint (so a
    ///    forced restore loses nothing);
    /// 5. the files are written; the restored state is recorded as that
    ///    checkpoint's `after` and as each undone turn's `restored`.
    ///
    /// Restoring the `before_restore` checkpoint's id undoes the restore.
    /// If the restore failed after step 2, that checkpoint has no `after`:
    /// restoring it then puts back the paths its event lists to their
    /// saved content, over the folder as it is (see `interrupted`).
    pub(crate) async fn restore(
        &self,
        turn_id: &str,
        force: bool,
        rewind: bool,
    ) -> Result<(), String> {
        if self.is_running() {
            return Err("wait for the running turn to finish".into());
        }
        // A command still running would write over what is restored (R17).
        if let Some(ids) = self
            .bg
            .as_ref()
            .map(|b| b.running())
            .filter(|i| !i.is_empty())
        {
            return Err(format!(
                "this chat's background commands are running ({}): they could write over what \
                 an undo puts back. Stop them, or wait for them to end, then undo",
                ids.join(", ")
            ));
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
        let incomplete = || "this turn's checkpoint is incomplete".to_string();
        let before = cp.before.clone().ok_or_else(incomplete)?;
        // A restore that failed after saving the folder: its
        // `before_restore` event lists the paths it may have written. (A
        // turn without its `after` stays incomplete: nothing says what it
        // changed.)
        let interrupted: Option<Vec<String>> = match &cp.after {
            Some(_) => None,
            None => Some(
                events
                    .iter()
                    .find_map(|e| match e.typed() {
                        Ok(EventKind::Checkpoint(c))
                            if c.turn_id == turn_id
                                && c.phase == CheckpointPhase::BeforeRestore =>
                        {
                            Some(c.files)
                        }
                        _ => None,
                    })
                    .ok_or_else(incomplete)?,
            ),
        };
        let after = cp.after.clone().unwrap_or_default();
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
                if !matches!(
                    c.phase,
                    CheckpointPhase::Before | CheckpointPhase::BeforeRestore
                ) {
                    latest = c.git_ref.clone();
                }
            }
        }
        if !rewind && cp.restored {
            return Err("already undone".into());
        }
        let repo = self
            .checkpoint_repo()
            .await?
            .ok_or("checkpoints are not available for this folder any more")?;
        // Held until this restore ends, on every path.
        let _claim = self.claim_folder(&repo.root)?;
        // No garbage collection runs in the checkpoint repository while the
        // restore runs (it waits for one that is running): the copies
        // `secure_blobs` makes are protected by nothing else until the
        // files are written. Nothing below waits on a person.
        let busy = crate::git::hold(&repo).await;
        // `None`: an interrupted restore, put back over the folder as it is.
        let expected = match (&interrupted, rewind) {
            (Some(_), _) => None,
            (None, true) => Some(latest),
            (None, false) => Some(after),
        };
        for sha in std::iter::once(&before).chain(expected.as_ref()) {
            if !crate::git::exists(&repo, sha).await {
                return Err(format!(
                    "this checkpoint is no longer kept (only the latest {} changes are)",
                    self.node.cfg.policy.checkpoints_keep
                ));
            }
        }
        // The diff, read before the folder is saved: a checkpoint whose
        // tree is gone is refused here, leaving nothing behind.
        let checked = match &expected {
            Some(e) => Some(crate::git::restore_diff(&repo, &before, e).await?),
            None => None,
        };
        // The folder as it is, saved first: its own checkpoint, counted
        // against `checkpoints_keep` like a turn's. The same scan serves
        // the unforced check.
        let saved = format!("restore-{}", harness_core::new_id());
        let saved_ref = format!("refs/harness/{}/{saved}/before", self.id);
        let now = crate::git::snapshot_full(&repo, &saved_ref, "harness: before restore")
            .await
            .map_err(|e| {
                format!("nothing was restored: the folder could not be saved first ({e})")
            })?;
        let planned = async {
            let diff = match (checked, &interrupted) {
                (Some(d), _) => d,
                (None, files) => {
                    // An interrupted restore: back to its saved state, for
                    // the paths it may have written, over the folder now.
                    // Its "changed since" check compares the folder with
                    // itself, so it never refuses: edits made after the
                    // failed restore are overwritten without asking. They
                    // are in the snapshot just taken, which the note after
                    // the restore names.
                    let mut d = crate::git::restore_diff(&repo, &before, &now.sha).await?;
                    d.only(files.as_deref().unwrap_or_default());
                    d
                }
            };
            let plan = crate::git::restore_plan(&repo, &diff, &now.sha, !force).await?;
            // Exactly the blobs the plan writes (it knows the folder's
            // nested repositories only from the snapshot just taken).
            crate::git::secure_blobs(&repo, &plan).await?;
            Ok::<_, String>(plan)
        }
        .await;
        let plan = match planned {
            Ok(t) => t,
            Err(e) => {
                crate::git::delete_ref(&repo, &saved_ref).await;
                return Err(e);
            }
        };
        let files: Vec<String> = plan.touched.iter().map(|(_, p)| p.clone()).collect();
        if let Err(e) = self
            .append(EventKind::Checkpoint(Checkpoint {
                turn_id: saved.clone(),
                phase: CheckpointPhase::BeforeRestore,
                git_ref: now.sha.clone(),
                files: files.clone(),
                excluded: now.nested.shown,
                excluded_more: now.nested.more as u64,
            }))
            .await
        {
            crate::git::delete_ref(&repo, &saved_ref).await;
            return Err(format!("nothing was restored: {}", LogError(e.to_string())));
        }
        // The saved checkpoint is the way back, also if the write below
        // fails: the folder's versions of every path that differs from what
        // the restore writes are copied like a turn's (A.17); a loss is only
        // logged here.
        let group = format!("refs/harness/{}/{saved}", self.id);
        match crate::git::pin_changed(&repo, &group, &now.sha, &before).await {
            Ok(p) if !p.missing.is_empty() => tracing::warn!(
                session = %self.id, missing = %p.missing.listed(),
                "restore checkpoint: content already gone, not copied"
            ),
            Ok(_) => {}
            Err(e) => tracing::warn!(session = %self.id, error = %e, "checkpoint copies failed"),
        }
        // From here on the saved state is in the log; say so on failure.
        let kept = |e: String| {
            format!(
                "{e}; the folder as it was before this restore is saved as \
                 checkpoint {saved} (ref {saved_ref})"
            )
        };
        // The folder is still the session's own right before the write.
        self.checked_folder().map_err(kept)?;
        crate::git::restore_paths(&repo, &plan, crate::git::WriteLimits::UNDO)
            .await
            .map_err(kept)?;
        let state = crate::git::snapshot_full(
            &repo,
            &format!("refs/harness/{}/{saved}/after", self.id),
            "harness: restored",
        )
        .await
        .map_err(kept)?;
        self.append(EventKind::Checkpoint(Checkpoint {
            turn_id: saved.clone(),
            phase: CheckpointPhase::After,
            git_ref: state.sha.clone(),
            files: files.clone(),
            excluded: state.nested.shown,
            excluded_more: state.nested.more as u64,
        }))
        .await
        .map_err(|e| kept(LogError(e.to_string()).to_string()))?;
        let targets: Vec<String> = if rewind && interrupted.is_none() {
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
                git_ref: state.sha.clone(),
                files: files.clone(),
                excluded: plan.nested.shown.clone(),
                excluded_more: plan.nested.more as u64,
            }))
            .await
            .map_err(|e| kept(LogError(e.to_string()).to_string()))?;
        }
        // Only when something about repositories changed, in words the
        // chat shows today (pre-existing ones are in `excluded` alone).
        let safety = interrupted.as_ref().map(|_| saved.as_str());
        if let Some(message) = restore_notice(files.len(), &plan, safety) {
            self.append(EventKind::Error(ErrorEvent {
                turn_id: None,
                message,
            }))
            .await
            .map_err(|e| kept(LogError(e.to_string()).to_string()))?;
        }
        *self.last_state.lock().unwrap_or_else(|p| p.into_inner()) = Some(state.sha.clone());
        crate::git::prune(&repo, self.node.cfg.policy.checkpoints_keep).await;
        drop(busy);
        crate::git::gc_in_background(repo);
        Ok(())
    }

    /// Register a restore of `root` (the checkpoint's folder: the
    /// repository's top for a folder inside one) in the node's registry of
    /// folders being restored, refusing while another session has a turn
    /// running in an overlapping folder (the same, a containing or a
    /// contained one, by real path) or another restore of an overlapping
    /// folder is registered. Check and insert happen under the registry's
    /// lock, which a turn's start also takes (see [`Self::claim_turn`]), so
    /// no turn starts there while the restore runs. The entry goes when the
    /// returned guard drops, on every path. This session's own running turn
    /// (and so its sub-agents, which run inside it) is refused before this.
    /// Another session's sub-agent works in its own node-owned folder, but
    /// only inside its parent's running turn, and that parent's folder
    /// overlaps a restore here; its result is written into the parent's
    /// folder before that turn ends.
    fn claim_folder(&self, root: &std::path::Path) -> Result<RestoreClaim, String> {
        let root = real_path(root);
        // Real paths before the lock: no file system call under it.
        let others: Vec<Arc<Session>> = self
            .node
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .filter(|s| s.id != self.id)
            .cloned()
            .collect();
        let others: Vec<(Arc<Session>, PathBuf)> = others
            .into_iter()
            .map(|s| {
                let cwd = real_path(&s.cwd);
                (s, cwd)
            })
            .collect();
        let mut reg = self
            .node
            .restoring
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        // Under the registry's lock, which the stop takes after setting its
        // flag: either this sees the flag, or the stop sees this restore and
        // waits for it (review A19-4).
        if self.node.is_stopping() {
            return Err(harness_protocol::NODE_STOPPING.into());
        }
        if reg.iter().any(|r| overlaps(r, &root)) {
            return Err(UNDO_RUNNING.into());
        }
        if others
            .iter()
            .any(|(s, cwd)| overlaps(cwd, &root) && s.is_running())
        {
            return Err(
                "another chat is working in this folder right now; wait for it to finish \
                 or cancel it"
                    .into(),
            );
        }
        reg.push(root.clone());
        Ok(RestoreClaim {
            node: self.node.clone(),
            root,
        })
    }

    /// Take this session's turn slot for `turn`, unless a turn holds it or
    /// a restore of an overlapping folder is running (checked under the
    /// registry's lock, under which a restore registers).
    fn claim_turn(&self, turn: RunningTurn) -> Result<(), String> {
        let cwd = real_path(&self.cwd);
        let reg = self
            .node
            .restoring
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if reg.iter().any(|r| overlaps(r, &cwd)) {
            return Err(UNDO_RUNNING.into());
        }
        let mut t = self.turn.lock().unwrap_or_else(|p| p.into_inner());
        if t.is_some() {
            return Err("a turn is already running".into());
        }
        *t = Some(turn);
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
        // No new sub-agent while the node stops (its parent is cancelled
        // already; this is the window before the parent sees it).
        if self.node.is_stopping() {
            return Err(harness_protocol::NODE_STOPPING.into());
        }
        let child_id = SessionId::new();
        let short: String = task.chars().take(60).collect();
        // The parent's folder is still its own before a child works there.
        let here = self.checked_folder()?;
        // A sub-agent that can change files works in its own folder,
        // so parallel sub-agents do not trample each other and a child whose
        // log failed leaves the folder alone; without one it is not started.
        let iso = self.isolate(&agent, child_id, &here).await?;
        let cwd = iso.as_ref().map(|i| i.cwd.clone()).unwrap_or(here);
        let child = match Session::create(
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
                // A sub-agent's chat has no background table at all.
                no_callbacks: true,
                // A read-only grandchild works in its parent's node-owned folder.
                owned: iso
                    .as_ref()
                    .map(|i| i.child.owned())
                    .or_else(|| self.owned.clone()),
            },
        )
        .await
        {
            Ok(c) => c,
            Err(e) => {
                if let Some(i) = &iso {
                    crate::git::remove_folder(&i.child).await;
                }
                return Err(e.to_string());
            }
        };
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
        let by_stop = Arc::new(AtomicBool::new(false));
        if let Err(e) = child.claim_turn(RunningTurn {
            turn_id: turn_id.clone(),
            cancel: cancel.clone(),
            savings: savings.clone(),
            publishes: Arc::default(),
            by_stop: by_stop.clone(),
        }) {
            if let Some(i) = iso {
                crate::git::remove_folder(&i.child).await;
            }
            return Err(e);
        }
        // Claimed after a stop began (its folder took a while): refused, as
        // `prompt` does, so no child turn starts after the stop (R-7).
        if self.node.is_stopping() {
            child.release(&turn_id);
            if let Some(i) = iso {
                crate::git::remove_folder(&i.child).await;
            }
            return Err(harness_protocol::NODE_STOPPING.into());
        }
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
                    crate::git::remove_folder(&i.child).await;
                }
                return Err(e);
            }
        };
        let (reason, usage) = child
            .clone()
            .run_turn(
                msg.turn_id.clone(),
                msg.model_text(),
                history,
                Some(agent),
                vec![],
                extra,
                cancel,
                by_stop,
            )
            .await;
        // Each session's log gates its own effects: the parent's turn goes
        // on, but nothing of a child whose log failed (or whose parent's
        // did) reaches the parent's folder, since no event would record it.
        let log_failed =
            child.log_failed.load(Ordering::SeqCst) || self.log_failed.load(Ordering::SeqCst);
        let merge_note = match iso {
            Some(i) => Some(self.merge_back(i, child_id, !log_failed).await),
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

    /// Whether a sub-agent can change files here: its tools, narrowed by
    /// its own denials and by this node's policy, include `write`, `edit` or
    /// `bash` (no shell on a node is read-only).
    fn can_change_files(&self, agent: &AgentSpec) -> bool {
        let denied = &self.node.cfg.policy.denied_tools;
        self.profile == Profile::Agentic
            && ["write", "edit", "bash"]
                .iter()
                .any(|t| agent.allows_tool(t) && !denied.iter().any(|d| d == t))
    }

    /// Prepare a folder for a sub-agent that can change files, seeded with
    /// the current working state (uncommitted edits included). `Ok(None)`:
    /// the sub-agent cannot change files and works in this folder; `Err`: it
    /// can, and no folder could be made, so it must not start. The text is
    /// the delegation's result, for the parent model. `here` is this
    /// session's checked folder.
    ///
    /// The folder is node-owned (`<data>/sessions/<child>/files`, see
    /// `git::ChildFolder`): outside the user's repository and outside every
    /// other session's writable area. Its files and its own git directory
    /// are written by the node before the child starts; no git process of
    /// the node runs with the user's git directory (R.1 follow-up).
    async fn isolate(
        &self,
        agent: &AgentSpec,
        child: SessionId,
        here: &std::path::Path,
    ) -> Result<Option<Isolation>, String> {
        if !self.can_change_files(agent) {
            return Ok(None);
        }
        let who = &agent.agent_id;
        let not_started = |e: String| {
            tracing::warn!(session = %self.id, error = %e, "no folder for a sub-agent that changes files; not started");
            format!(
                "@{who} was not started: it can change files (write, edit or bash), and its \
                 own folder could not be made ({e}). Make the change yourself, or delegate to \
                 a sub-agent that only reads."
            )
        };
        if self.owned.is_some() {
            return Err(not_started(
                "a sub-agent's own sub-agents may only read".to_string(),
            ));
        }
        let Some(sessions) = self.sessions_dir() else {
            return Err(not_started(
                "this node keeps no checkpoint store, which sub-agent folders need".to_string(),
            ));
        };
        let sub = match crate::git::sub_repo(
            here,
            self.node.cfg.checkpoints_dir.as_deref(),
            &self.bounds(here),
        )
        .await
        {
            Ok(Some(s)) => s,
            Ok(None) => {
                return Err(format!(
                    "@{who} was not started: it can change files (write, edit or bash), and a \
                     sub-agent that changes files works on its own copy of a git repository, \
                     but this folder is not a git repository (within this node's workspace \
                     roots). Make the change yourself, delegate to a sub-agent that only reads, \
                     or ask the user to run `git init` in this folder."
                ));
            }
            Err(e) => return Err(not_started(e)),
        };
        let id = child.to_string();
        let base_ref = format!("refs/harness/{child}/base");
        let mut made: Option<crate::git::ChildFolder> = None;
        let res = async {
            let base = sub.base(&id).await?;
            let folder = sub.make_folder(&id, &base, &sessions).await?;
            made = Some(folder.clone());
            let rel = here
                .strip_prefix(&sub.repo.root)
                .unwrap_or(std::path::Path::new(""));
            let cwd = folder.path.join(rel);
            tokio::fs::create_dir_all(&cwd)
                .await
                .map_err(|e| format!("{}: {e}", cwd.display()))?;
            let cwd = cwd
                .canonicalize()
                .map_err(|e| format!("{}: {e}", cwd.display()))?;
            if !cwd.starts_with(&folder.path) {
                return Err(format!(
                    "{} leads out of the sub-agent's folder",
                    rel.display()
                ));
            }
            Ok::<_, String>(Isolation {
                sub: sub.clone(),
                child: folder,
                base,
                cwd,
            })
        }
        .await;
        match res {
            Ok(i) => Ok(Some(i)),
            Err(e) => {
                // Leave nothing behind: no half-made folder, no base ref.
                if let Some(f) = made {
                    crate::git::remove_folder(&f).await;
                }
                crate::git::delete_ref(&sub.repo, &base_ref).await;
                Err(not_started(e))
            }
        }
    }

    /// Bring a sub-agent's changes back into the main working directory:
    /// its folder is snapshotted in the shadow and the changed files are
    /// written into this session's folder with the restore machinery (no
    /// `git apply` under the user's config). `apply: false` keeps the
    /// result in the node's checkpoint store and changes nothing in the
    /// folder.
    async fn merge_back(&self, iso: Isolation, child: SessionId, apply: bool) -> String {
        let id = child.to_string();
        let kept = "it is kept in this node's checkpoint store (sub-agent snapshots)";
        let note = match iso.sub.result(&iso.child, &id).await {
            Err(e) => format!("its changes could not be collected: {e}"),
            Ok(result) if !apply => format!(
                "its changes were not applied, because a session log could not be written; \
                 {kept} as {result}"
            ),
            Ok(result) => {
                let applied = match self.checked_folder() {
                    Ok(_) => iso.sub.apply(&iso.base, &result, &id).await,
                    Err(e) => Err(e),
                };
                match applied {
                    Ok(0) => "no file changes".to_string(),
                    Ok(n) => format!("{n} changed file(s) applied to the folder"),
                    Err(e) => format!(
                        "its changes conflict with the folder and were NOT applied ({e}); \
                         {kept} as {result}"
                    ),
                }
            }
        };
        crate::git::remove_folder(&iso.child).await;
        iso.sub.prune(self.node.cfg.policy.checkpoints_keep).await;
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

    pub(crate) fn id(&self) -> SessionId {
        self.id
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

    /// Mark the running turn as the stop's to end, unless it was cancelled
    /// already (by its person, or its parent): `NodeRuntime::stop` marks
    /// every running turn first and then cancels them.
    pub(crate) fn mark_stopped(&self) {
        if let Some(t) = self.turn.lock().unwrap_or_else(|p| p.into_inner()).as_ref()
            && !t.cancel.is_cancelled()
        {
            t.by_stop.store(true, Ordering::SeqCst);
        }
    }

    /// The last turn's end could not be written (it is still open in the
    /// log; the next prompt or start closes it).
    pub(crate) fn is_unclosed(&self) -> bool {
        self.unclosed.load(Ordering::SeqCst)
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
            pin: self.pin.clone(),
            count,
        }))
    }

    fn tool_denied(&self, tool: &str) -> bool {
        self.node.cfg.policy.denied_tools.iter().any(|t| t == tool)
    }

    fn background(&self) -> Option<Arc<dyn std::any::Any + Send + Sync>> {
        self.bg
            .clone()
            .map(|b| b as Arc<dyn std::any::Any + Send + Sync>)
    }

    async fn background_notes(
        &self,
    ) -> Result<Vec<BackgroundEnded>, harness_core::backend::LogError> {
        let Some(bg) = &self.bg else {
            return Ok(Vec::new());
        };
        let queued = std::mem::take(&mut *self.bg_queue.lock().unwrap_or_else(|p| p.into_inner()));
        let mut out = Vec::new();
        let mut rest = queued.into_iter();
        while let Some(e) = rest.next() {
            // What is appended is scrubbed; so is what goes to the model
            // (T8D-2).
            let mut ev = e.event(true);
            ev.tail = self.node.cfg.secrets.scrub(&ev.tail);
            if let Err(err) = Session::emit(self, EventKind::BackgroundEnded(ev.clone())).await {
                // The turn ends; these are recorded after it.
                let mut q = self.bg_queue.lock().unwrap_or_else(|p| p.into_inner());
                q.push(e);
                q.extend(rest);
                return Err(err);
            }
            bg.recorded();
            out.push(ev);
        }
        Ok(out)
    }

    fn folder_pin(&self) -> Option<harness_core::folder::FolderPin> {
        self.pin.clone()
    }

    fn owned_folder(&self) -> Option<harness_core::folder::OwnedFolder> {
        self.owned.clone()
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
        // "For this turn" is offered on every request a person answers: it
        // covers only this exact call, the one on the card (the loop keys it
        // with `harness_tools::call_key`, PLAN.md T4). The answer reaches
        // only this call: the approval id is bound to this one waiting
        // request, and the loop records the key of the call it is running.
        let background_start = is_background_start(tool, input);
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
                reason: approval_reason(tool, risk, input, &self.node.cfg.policy),
                // "For this turn" would cover every identical start of the
                // turn (N-2): a background start is asked for each time.
                turn_allow: !background_start,
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
        // The cancel first (review N-8): an answer that arrives with it,
        // after a stop or a cancel began, allows nothing.
        let (decision, by) = tokio::select! {
            biased;
            _ = cancel.cancelled() => (Decision::Deny, ResolvedBy::Cancelled),
            r = rx => match r {
                Ok(d) => (d, ResolvedBy::User),
                Err(_) => (Decision::Deny, ResolvedBy::Cancelled),
            },
        };
        self.approvals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&approval_id);
        // An approval the log does not hold allows nothing.
        if !self.resolve(approval_id, decision, by).await {
            return Decision::Deny;
        }
        // Not offered for a background start, and not taken either: one
        // click is one command.
        if background_start && decision == Decision::AllowSession {
            return Decision::Allow;
        }
        decision
    }
}

/// A `bash` call that asks to keep running.
fn is_background_start(tool: &str, input: &Value) -> bool {
    tool == "bash" && input.get("background").and_then(Value::as_bool) == Some(true)
}

/// Refusal while a restore of an overlapping folder runs.
const UNDO_RUNNING: &str = "an undo is running in this folder; try again in a moment";

/// A folder registered as being restored; unregistered when dropped.
struct RestoreClaim {
    node: Arc<Inner>,
    root: PathBuf,
}

impl Drop for RestoreClaim {
    fn drop(&mut self) {
        let mut reg = self
            .node
            .restoring
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(i) = reg.iter().position(|r| *r == self.root) {
            reg.swap_remove(i);
        }
    }
}

/// Where `p` really is (as given when it cannot be resolved).
fn real_path(p: &std::path::Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// The same folder, or one inside the other.
fn overlaps(a: &std::path::Path, b: &std::path::Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

/// The chat's note after an undo, only when repositories inside the folder
/// appeared or went away during the undone turn, or a `.git` sits in a
/// folder the undo wrote into. It starts with what was restored, so it does
/// not read as a failure. `None`: nothing to say (stable nested
/// repositories are in the checkpoint's `excluded` alone).
///
/// Also when a forced undo deleted files no checkpoint holds, and after a
/// restore of an interrupted restore (`safety`: the checkpoint that saved
/// the folder first), which overwrote its paths without checking for
/// later edits.
fn restore_notice(
    files: usize,
    plan: &crate::git::RestorePlan,
    safety: Option<&str>,
) -> Option<String> {
    if plan.changed_repos.is_empty()
        && plan.git_inside.is_empty()
        && plan.unsaved.is_empty()
        && safety.is_none()
    {
        return None;
    }
    let mut s = format!(
        "Undo restored {files} file{}.",
        if files == 1 { "" } else { "s" }
    );
    if let Some(id) = safety {
        s.push_str(&format!(
            " They were put back as saved before the failed undo, over any later edits; \
             the folder as it was just before is saved as checkpoint {id}."
        ));
    }
    if !plan.unsaved.is_empty() {
        s.push_str(&format!(
            " These files were deleted and were not saved (they are in no checkpoint, and \
             a file replaced the folder that held them): {}.",
            plan.unsaved.listed()
        ));
    }
    if !plan.git_inside.is_empty() {
        s.push_str(&format!(
            " {} now hold a `.git` (a repository made there, or a stray `.git`) that no \
             checkpoint covers: the files the checkpoint holds there were restored and the \
             `.git` was left as it is.",
            plan.git_inside.listed()
        ));
    }
    if !plan.changed_repos.is_empty() {
        s.push_str(&format!(
            " Git repositories inside the folder appeared or went away during the turn and \
             were left alone ({}): undo never covers a repository inside your folder; use \
             git in it to undo changes there.",
            plan.changed_repos.listed()
        ));
    }
    Some(s)
}

/// What an approval card says the tool would do. Publishing changes nothing
/// on this node, it sends a copy away, so it says that instead.
fn approval_reason(
    tool: &str,
    risk: ToolRisk,
    input: &Value,
    policy: &crate::NodePolicy,
) -> String {
    // A background start says what it is, in words (§4.1): the field alone
    // is easy to miss.
    if is_background_start(tool, input) {
        let ceiling = std::time::Duration::from_secs(policy.limits.bash_timeout_max_s);
        let t = harness_tools::background::timeout_for(
            input.get("timeout_secs").and_then(Value::as_u64),
            harness_tools::background::DEFAULT_TIMEOUT,
            ceiling,
        );
        let mut s = format!(
            "`bash` starts a program that KEEPS RUNNING in the background after this step, \
             with nobody watching, for up to {} (its timeout), unless it ends or is stopped first",
            harness_core::text::duration_words(t.as_millis() as u64)
        );
        if policy
            .sandbox
            .settings()
            .is_ok_and(|s| s.network == harness_tools::sandbox::Network::On)
        {
            s.push_str("; the network is on, so it can accept connections");
        }
        return s;
    }
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
        let p = crate::NodePolicy::default();
        let none = Value::Null;
        let publish = approval_reason("publish", ToolRisk::Mutating, &none, &p);
        assert!(
            publish.contains("Artifacts") && !publish.contains("run programs"),
            "{publish}"
        );
        assert_eq!(
            approval_reason("bash", ToolRisk::Mutating, &none, &p),
            "`bash` can change files or run programs"
        );
    }

    /// §7 test 10: the card of a background start says it keeps running,
    /// and until when, in its reason, not only in the input field.
    #[test]
    fn a_background_start_says_it_keeps_running() {
        let p = crate::NodePolicy::default();
        let input =
            serde_json::json!({"command": "npm run dev", "background": true, "timeout_secs": 1800});
        let r = approval_reason("bash", ToolRisk::Mutating, &input, &p);
        assert!(
            r.contains("KEEPS RUNNING") && r.contains("30 min 0 s"),
            "{r}"
        );
        assert!(r.contains("accept connections"), "{r}");
        let fg = serde_json::json!({"command": "ls", "background": false});
        assert!(!approval_reason("bash", ToolRisk::Mutating, &fg, &p).contains("KEEPS"));
    }
}
