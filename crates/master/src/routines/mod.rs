//! Routines: saved instructions that start a chat on a schedule or when a
//! webhook is called (docs/routines.md). A routine belongs to one workspace
//! and one owner, and each fire is a run that starts a new chat as that
//! owner, re-checking their access every time.
//!
//! * `spec`: what a routine runs (`RoutineSpec`) and how a saved one is
//!   checked;
//! * `schedule`: cron, interval and one-off schedules (croner + jiff);
//! * `template`: the prompt's `{{ variables }}`;
//! * `events`: event sources (the webhook) and what a run keeps of an event;
//! * `buckets`: the webhook throttles;
//! * `runner`: the scheduler, the executor and the watcher that follows a
//!   run's chat to its end;
//! * `results`: what the owner sees of runs (chat marks, attention, seen,
//!   retention).

pub(crate) mod buckets;
pub mod events;
pub mod results;
pub mod runner;
pub mod schedule;
pub mod spec;
pub mod template;

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use harness_core::SessionId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{Notify, Semaphore, watch};

use crate::Master;
use crate::auth::Principal;
use crate::workspaces::{Access, Perm};

/// `[routines]` in config.toml; left out of the file while it is the default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RoutinesConfig {
    /// Off: nothing fires and nothing new can be saved.
    pub enabled: bool,
    pub webhooks: bool,
    /// The shortest gap between two runs of one schedule.
    pub min_interval_minutes: u32,
    pub max_per_user: u32,
    pub max_per_workspace: u32,
    /// Schedules per routine.
    pub max_triggers: u32,
    /// Runs starting or running at once, server-wide (a run waiting for an
    /// approval does not hold a place).
    pub max_concurrent_runs: u32,
    pub max_concurrent_per_user: u32,
    pub max_runs_per_user_per_day: u32,
    pub hook_rate_per_minute: u32,
    /// Runs kept per routine (and none older than 90 days).
    pub keep_runs: u32,
    /// Seen, uncontinued run chats kept per routine.
    pub keep_run_chats: u32,
    /// A run waiting this long for an approval is cancelled.
    pub approval_timeout_hours: u32,
}

impl Default for RoutinesConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            webhooks: true,
            min_interval_minutes: 5,
            max_per_user: 25,
            max_per_workspace: 100,
            max_triggers: 8,
            max_concurrent_runs: 4,
            max_concurrent_per_user: 2,
            max_runs_per_user_per_day: 200,
            hook_rate_per_minute: 10,
            keep_runs: 200,
            keep_run_chats: 30,
            approval_timeout_hours: 24,
        }
    }
}

impl RoutinesConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// The routines settings in force: config.toml's, or the defaults when the
/// master runs without a provider runtime (tests).
pub(crate) fn config(m: &Master) -> RoutinesConfig {
    m.provider_runtime()
        .map(|rt| rt.config().routines)
        .unwrap_or_default()
}

/// Where routines read the time: the scheduler, the executor, the timeout
/// sweep, `hourly` and the template's `now.*`. Tests drive their own.
#[derive(Clone)]
pub struct Clock(Arc<dyn Fn() -> i64 + Send + Sync>);

impl Clock {
    /// The system clock, in milliseconds since the epoch.
    pub fn wall() -> Self {
        Self(Arc::new(harness_core::now_ms))
    }

    /// A clock that reads `ms`, for tests that move time themselves.
    pub fn manual(ms: Arc<AtomicI64>) -> Self {
        Self(Arc::new(move || ms.load(Ordering::SeqCst)))
    }

    pub fn now(&self) -> i64 {
        (self.0)()
    }
}

/// The running scheduler, in `MasterInner.routines`.
pub struct RoutinesHandle {
    pub(crate) clock: Clock,
    /// Wakes the scheduler early (a routine was saved or run).
    wake: Notify,
    /// Places for runs starting or running (`max_concurrent_runs`).
    slots: Arc<Semaphore>,
    /// Chats of runs that have not finished, followed by the watcher.
    bound: Mutex<HashMap<SessionId, runner::Bound>>,
    /// Runs waiting to try again after their node was offline.
    retries: Mutex<Vec<runner::Retry>>,
    /// Held while a retry is recorded on its run and queued, and while a
    /// tick takes the due retries: a run whose row already says "trying
    /// again" is always in `retries` for the next tick (a tick in between
    /// found the queue empty, and the run stayed pending until some later
    /// tick).
    queueing: tokio::sync::Mutex<()>,
    /// The webhook throttles.
    pub(crate) hooks: Mutex<buckets::HookLimits>,
    /// Set once the startup reconcile ran: nothing fires before it, so a
    /// run left pending by the last process is never mistaken for a new one.
    ready: watch::Receiver<bool>,
}

impl RoutinesHandle {
    /// Process everything due at clock-now: retries, then triggers.
    pub async fn tick(&self, m: &Master) {
        runner::tick(self, m).await
    }

    /// Cancel runs waiting past the approval timeout or running past their
    /// routine's `max_runtime_min` (recorded as `timed_out`).
    pub async fn sweep_timeouts(&self, m: &Master) {
        runner::sweep_timeouts(self, m).await
    }

    /// Wake the scheduler loop now.
    pub(crate) fn wake(&self) {
        self.wake.notify_one();
    }
}

/// Install the scheduler with the wall clock and start its loops.
pub fn start(m: &Master) {
    start_with(m, Clock::wall(), true);
}

/// Install the scheduler with `clock`. The watcher always runs (it follows
/// events, not time); `spawn_loops` starts the timed loops (the scheduler
/// and the timeout sweep), which tests leave off to call `tick` and
/// `sweep_timeouts` themselves.
pub fn start_with(m: &Master, clock: Clock, spawn_loops: bool) {
    let (ready_tx, ready) = watch::channel(false);
    let slots = Arc::new(Semaphore::new(config(m).max_concurrent_runs.max(1) as usize));
    let handle = RoutinesHandle {
        clock,
        wake: Notify::new(),
        slots,
        bound: Mutex::new(HashMap::new()),
        retries: Mutex::new(Vec::new()),
        queueing: tokio::sync::Mutex::new(()),
        hooks: Mutex::new(buckets::HookLimits::default()),
        ready,
    };
    if m.inner.routines.set(handle).is_err() {
        tracing::warn!("routines were started twice; keeping the first scheduler");
        return;
    }
    runner::start_watcher(m, ready_tx);
    if spawn_loops {
        runner::start_loops(m);
    }
}

/// The scheduler, once `start` ran.
pub fn handle(m: &Master) -> Option<&RoutinesHandle> {
    m.inner.routines.get()
}

/// Routines run as their owner, which the API token is not.
/// What "run now" and a webhook call get while the server stops (503): no
/// run is recorded, so nothing is left to end at the next start.
pub const STOPPING: &str = "the server is stopping: try again once it is back";

pub(crate) const TOKEN_CANNOT_OWN: &str = "The API token can read routines but cannot own one";

/// Why `who` may not own routines in `a`, or `None` when they may.
pub(crate) fn may_not_own(who: &Principal, a: &Access) -> Option<String> {
    if who.user_id.is_none() {
        return Some(TOKEN_CANNOT_OWN.into());
    }
    if a.can(Perm::Routines) {
        return None;
    }
    let ws = &a.workspace.name;
    Some(match a.role {
        Some(crate::workspaces::Role::Guest) => format!("Guests can't run routines in {ws}"),
        _ => format!("Your role in {ws} can't run routines"),
    })
}

/// The `routines` block of `/api/features` for `who` in `scope`: shown to
/// whoever may own routines there, and to an owner who still has some there
/// they no longer may run (so they can delete them).
pub async fn features(m: &Master, who: &Principal, scope: Option<&Access>) -> Value {
    let cfg = config(m);
    let webhooks = cfg.webhooks;
    if !cfg.enabled {
        return json!({ "enabled": false, "show": false, "webhooks": webhooks,
                       "why": "Routines are turned off on this server" });
    }
    let Some(a) = scope else {
        // The API token without a workspace reads every routine.
        return json!({ "enabled": true, "show": true, "webhooks": webhooks,
                       "why": TOKEN_CANNOT_OWN });
    };
    let why = may_not_own(who, a);
    let owns = match (&why, &who.user_id) {
        (Some(_), Some(uid)) => {
            let (uid, ws) = (uid.clone(), a.id().to_string());
            m.inner
                .mirror
                .call(move |s| s.routines_for(Some(&uid), Some(&ws)))
                .await
                .map(|r| !r.is_empty())
                .unwrap_or(false)
        }
        _ => false,
    };
    let show = why.is_none() || owns || who.user_id.is_none();
    json!({ "enabled": true, "show": show, "webhooks": webhooks, "why": why })
}

/// Give each chat row its `routine` mark, and leave out the run chats the
/// sessions list hides unless `include_runs`.
// A `Vec`, not a slice: the filter removes rows.
#[allow(clippy::ptr_arg)]
pub async fn annotate(
    m: &Master,
    rows: &mut Vec<Value>,
    include_runs: bool,
) -> harness_store::Result<()> {
    results::annotate(m, rows, include_runs).await
}

/// `annotate` for one chat row.
pub async fn annotate_one(m: &Master, row: &mut Value) -> harness_store::Result<()> {
    results::annotate_one(m, row).await
}

/// Called before every prompt the owner sends: marks a run's chat as
/// continued and says whether the turn must run untrusted.
pub async fn on_owner_prompt(m: &Master, sid: SessionId) -> PromptFlags {
    results::on_owner_prompt(m, sid).await
}

/// Whether chat `sid` holds data from an external event (it, or the chat
/// that delegated to it, is marked), without marking any run continued.
/// Fails closed. For checks outside a prompt, such as filing a `remember`
/// call: the turn's recorded memory is gone after a restart, the mark is not.
pub async fn chat_untrusted(m: &Master, sid: SessionId) -> bool {
    m.inner
        .mirror
        .call(move |s| s.chat_untrusted(sid))
        .await
        .unwrap_or(true)
}

/// What a routine run decides about the owner's own turns in its chat.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PromptFlags {
    /// The chat holds data from an external event: every later turn there
    /// runs untrusted too (`PromptOpts::untrusted`).
    pub untrusted: bool,
}

/// Fire webhook routine `r` from its trigger `t` (`http/hooks.rs`, once the
/// call proved its secret).
pub async fn fire_event(
    m: &Master,
    t: &harness_store::routines::TriggerRow,
    r: &harness_store::routines::RoutineRow,
    event: events::Event,
) -> runner::Fired {
    runner::fire_event(m, t, r, event).await
}

/// The `routines` rows of `GET /api/attention` for `who`.
pub async fn attention(m: &Master, who: &Principal) -> harness_store::Result<Value> {
    results::attention(m, who).await
}

/// The hourly part of routines: run retention and purges.
pub async fn hourly(m: &Master) {
    results::hourly(m).await
}

/// Tell the owner's open pages that their routines or runs changed.
pub(crate) fn changed(m: &Master, owner: &str) {
    m.changed("routines", Some(owner.to_string()));
}

/// Record `kind` with `detail` (ids and reasons only: never prompt text).
pub(crate) async fn audit(m: &Master, kind: &'static str, detail: Value) {
    let detail = detail.to_string();
    if let Err(e) = m.inner.mirror.call(move |s| s.audit(kind, &detail)).await {
        tracing::warn!(error = %e, kind, "could not write a routine audit line");
    }
}
