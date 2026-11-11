//! Running routines (docs/routines.md, Execution).
//!
//! * The **scheduler** (`tick`) fires every trigger due at clock-now: each
//!   occurrence is claimed once in the store, then executed.
//! * The **executor** admits a run (the owner's account, membership,
//!   permissions and caps, re-checked at every fire), renders its prompt,
//!   takes a place among the concurrent runs, starts the chat as the owner
//!   and prompts it.
//! * The **watcher** follows each run's chat through the master's live
//!   events: its first turn binds the run, an approval (in the chat or in a
//!   sub-agent it delegated to) parks it, the turn's end finishes it.
//!   Later turns, the owner continuing the chat, are not the run's.
//!
//! Every time comes from the handle's `Clock`, so tests drive it.

use std::sync::Arc;
use std::time::Duration;

use harness_core::policy::ApprovalMode;
use harness_core::{RawEvent, SessionId, new_id};
use harness_store::UserRow;
use harness_store::routines::{
    Claimed, HookClaim, NewRun, NewTrigger, RoutinePatch, RoutineRow, RunPatch, RunRow, TriggerRow,
};
use jiff::Timestamp;
use jiff::tz::TimeZone;
use serde_json::{Value, json};
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::watch;

use super::events::{self, Event};
use super::schedule::ScheduleConfig;
use super::spec::{CatchUp, RoutineSpec};
use super::template::{self, RenderError, Vars};
use super::{RoutinesHandle, audit, changed, config, handle};
use crate::auth::Principal;
use crate::prefs::Prefs;
use crate::workspaces::{self as ws, Access, Perm, WsError};
use crate::{CommandError, CreateSpec, Live, Master, PromptOpts, lock};

const MINUTE: i64 = 60_000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;
/// The longest a run waits for a place among the concurrent runs.
const SLOT_WAIT: Duration = Duration::from_secs(600);
/// Minutes before each retry of a run whose node was offline (3 retries).
const RETRY_MIN: [i64; 3] = [1, 5, 15];
/// Failures in a row that pause a routine.
const MAX_FAILURES: i64 = 5;
/// Triggers fired per tick (the rest are due on the next one).
const TICK_BATCH: usize = 200;
/// The scheduler re-reads the clock at least this often (a suspended
/// machine wakes to a late timer).
const MAX_SLEEP: Duration = Duration::from_secs(60);
const SWEEP_EVERY: Duration = Duration::from_secs(60);

/// A run's chat, followed by the watcher until its first turn ends.
pub(crate) struct Bound {
    run_id: String,
    routine_id: String,
    owner: String,
    turn_id: Option<String>,
    /// Its place among the concurrent runs, given back when the run starts
    /// waiting or ends.
    slot: Option<OwnedSemaphorePermit>,
    /// Waiting for an approval since.
    waiting_since: Option<i64>,
    /// Running (not waiting) since: `max_runtime_min` counts from here.
    running_since: i64,
    max_runtime_ms: i64,
    /// Recorded as timed out; its end only adds usage.
    timed_out: bool,
}

/// A run waiting to try again after its node was offline: when, and the
/// event it renders from (held here, whole, since the run keeps only a
/// cut copy; a restart turns the run `interrupted` anyway).
pub(crate) struct Retry {
    run_id: String,
    due: i64,
    event: Option<Arc<Event>>,
}

/// The one place a run's approval mode is chosen: the spec's `ask` or
/// `ask_all`, never `auto` (a node clamps it further, T5).
///
/// This is the seam for trusted routines (docs/routines.md, "The approval
/// seam"), which is why it takes what v1 ignores. A trusted mode would
/// return the mode plus the spec's tool allowlist, and only when the run is
/// not `untrusted` (no `event.*` in its prompt, so a webhook run never
/// qualifies) and `node` announces `preapproved_tools`. Nothing else on the
/// master chooses: `auto` stays refused at save, and an untrusted chat's
/// later turns never come through here.
pub fn approval_for(spec: &RoutineSpec, _untrusted: bool, _node: Option<&str>) -> ApprovalMode {
    match spec.approval_mode {
        ApprovalMode::AskAll => ApprovalMode::AskAll,
        ApprovalMode::Ask | ApprovalMode::Auto => ApprovalMode::Ask,
    }
}

fn ts(ms: i64) -> Timestamp {
    Timestamp::from_millisecond(ms).unwrap_or(Timestamp::UNIX_EPOCH)
}

/// The owner's time zone from their preferences, else UTC.
fn zone_of(prefs: &Prefs) -> TimeZone {
    prefs
        .timezone
        .as_deref()
        .and_then(|t| TimeZone::get(t.trim()).ok())
        .unwrap_or(TimeZone::UTC)
}

/// The trigger triggers to save when a routine starts firing again from
/// `now` (enabled, or its pause lifted): same ids, next runs from now.
pub(crate) fn restarted(rows: &[TriggerRow], now: i64) -> Vec<NewTrigger> {
    rows.iter()
        .map(|t| NewTrigger {
            trigger_id: t.trigger_id.clone(),
            kind: t.kind.clone(),
            config: t.config.clone(),
            next_run_ms: ScheduleConfig::read(&t.config)
                .and_then(|c| c.schedule().ok())
                .and_then(|s| s.next_after_ms(now)),
            secret_hash: None,
        })
        .collect()
}

// ---- the scheduler ----

pub(super) async fn tick(h: &RoutinesHandle, m: &Master) {
    let mut ready = h.ready.clone();
    let _ = ready.wait_for(|r| *r).await;
    let cfg = config(m);
    if !cfg.enabled {
        return;
    }
    let now = h.clock.now();
    lift_cap_pauses(m, cfg.max_runs_per_user_per_day, now).await;
    let retries: Vec<Retry> = {
        let _queued = h.queueing.lock().await;
        let mut list = lock(&h.retries);
        let (due, later) = list.drain(..).partition(|r| r.due <= now);
        *list = later;
        due
    };
    for r in retries {
        spawn_execute(m, r.run_id, r.event);
    }
    let due = m
        .inner
        .mirror
        .call(move |s| s.routine_triggers_due(now, TICK_BATCH))
        .await;
    match due {
        Ok(due) => {
            for (t, r) in due {
                fire(m, t, r, now).await;
            }
        }
        Err(e) => tracing::warn!(error = %e, "routines: could not read due triggers"),
    }
}

fn new_run(r: &RoutineRow, t: &TriggerRow, status: &str, scheduled: i64, now: i64) -> NewRun {
    NewRun {
        run_id: new_id(),
        routine_id: r.routine_id.clone(),
        trigger_id: Some(t.trigger_id.clone()),
        trigger_kind: t.kind.clone(),
        scheduled_ms: Some(scheduled),
        started_ms: now,
        status: status.into(),
        reason: None,
        repeats: 1,
        event: None,
    }
}

/// Fire trigger `t`'s due occurrence: on time, late within its grace, or
/// missed (the server was down). However many occurrences went by, there is
/// at most one run, and the trigger moves past now.
async fn fire(m: &Master, t: TriggerRow, r: RoutineRow, now: i64) {
    let Some(expected) = t.next_run_ms else {
        return;
    };
    let schedule = ScheduleConfig::read(&t.config).and_then(|c| c.schedule().ok());
    let Some(schedule) = schedule else {
        let why = "its schedule cannot be read; edit the routine";
        block(m, &r, None, why, now).await;
        return;
    };
    let spec = RoutineSpec::read(&r.spec).unwrap_or_default();
    let after_now = schedule.next_after_ms(now);
    let (next, run) = if expected < now - schedule.grace_ms(expected) {
        match spec.catch_up {
            CatchUp::Once => (after_now, new_run(&r, &t, "pending", expected, now)),
            CatchUp::Skip => {
                let mut run = new_run(&r, &t, "missed", expected, now);
                run.reason = Some("the server was not running".into());
                run.repeats = schedule.count_between(expected, now) as i64;
                (after_now, run)
            }
        }
    } else {
        let next = schedule
            .next_after_ms(expected)
            .filter(|n| *n > now)
            .or(after_now);
        (next, new_run(&r, &t, "pending", expected, now))
    };
    let (tid, claim) = (t.trigger_id.clone(), run.clone());
    let res = m
        .inner
        .mirror
        .call(move |s| s.claim_occurrence(&tid, expected, next, &claim))
        .await;
    match res {
        Ok(Some(Claimed::Started)) => {
            changed(m, &r.owner_id);
            spawn_execute(m, run.run_id, None);
        }
        Ok(Some(_)) => changed(m, &r.owner_id),
        // Someone else fired it.
        Ok(None) => {}
        Err(e) => {
            tracing::warn!(routine = %r.routine_id, error = %e, "routines: could not claim a run")
        }
    }
}

/// Lift `cap` pauses of owners back under the daily cap, from now on.
async fn lift_cap_pauses(m: &Master, cap: u32, now: i64) {
    let capped = m
        .inner
        .mirror
        .call(|s| s.routines_paused("cap"))
        .await
        .unwrap_or_default();
    for r in capped {
        let (owner, id) = (r.owner_id.clone(), r.routine_id.clone());
        let res = m
            .inner
            .mirror
            .call(move |s| {
                let (today, _) = s.owner_run_counts(&owner, now - DAY)?;
                if today >= i64::from(cap) {
                    return Ok(false);
                }
                let patch = RoutinePatch {
                    triggers: Some(restarted(&s.triggers(&id)?, now)),
                    resume: true,
                    ..Default::default()
                };
                s.routine_update(&id, &patch, now).map(|r| r.is_some())
            })
            .await;
        if let Ok(true) = res {
            changed(m, &r.owner_id);
        }
    }
}

pub(super) fn start_loops(m: &Master) {
    let mm = m.clone();
    tokio::spawn(async move {
        let Some(h) = handle(&mm) else { return };
        loop {
            h.tick(&mm).await;
            let wait = if config(&mm).enabled {
                next_wait(&mm, h).await
            } else {
                MAX_SLEEP
            };
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = h.wake.notified() => {}
            }
        }
    });
    let mm = m.clone();
    tokio::spawn(async move {
        let Some(h) = handle(&mm) else { return };
        loop {
            tokio::time::sleep(SWEEP_EVERY).await;
            h.sweep_timeouts(&mm).await;
        }
    });
}

/// Until the next trigger or retry is due, within [200 ms, 60 s].
async fn next_wait(m: &Master, h: &RoutinesHandle) -> Duration {
    let next = m
        .inner
        .mirror
        .call(|s| s.routine_next_due())
        .await
        .ok()
        .flatten();
    let retry = lock(&h.retries).iter().map(|r| r.due).min();
    match [next, retry].into_iter().flatten().min() {
        Some(at) => {
            let ms = (at - h.clock.now()).clamp(200, MAX_SLEEP.as_millis() as i64);
            Duration::from_millis(ms as u64)
        }
        None => MAX_SLEEP,
    }
}

// ---- the executor ----

/// What a run needs once admitted.
pub(crate) struct Admitted {
    run: RunRow,
    routine: RoutineRow,
    spec: RoutineSpec,
    owner: UserRow,
    access: Access,
    /// The event that fired it, whole.
    event: Option<Arc<Event>>,
}

/// Why a run did not start (already recorded on the run).
pub(crate) enum Refused {
    /// The run or its routine is gone, or it is no longer pending.
    Gone,
    /// Access or settings refused it; the routine is paused.
    Blocked(String),
    /// Over a cap; recorded as skipped.
    Cap,
    /// Something failed on the way (a store error).
    Failed,
}

pub(crate) fn spawn_execute(m: &Master, run_id: String, event: Option<Arc<Event>>) {
    let m = m.clone();
    tokio::spawn(async move {
        if let Ok(a) = admit(&m, &run_id, event).await {
            launch(m, a).await;
        }
    });
}

/// Why `r`'s owner may not run it now, or what it runs as: the account,
/// its membership and permissions in the routine's workspace, and what the
/// spec names. `Err(None)` for a failure to check (not a refusal).
pub(crate) async fn why_blocked(
    m: &Master,
    r: &RoutineRow,
) -> Result<(UserRow, Access, RoutineSpec), Option<String>> {
    let uid = r.owner_id.clone();
    let owner = match m.inner.mirror.call(move |s| s.user_by_id(&uid)).await {
        Ok(Some(u)) => u,
        Ok(None) => return Err(Some("the owner's account is gone".into())),
        Err(e) => {
            tracing::warn!(error = %e, "routines: could not load an owner");
            return Err(None);
        }
    };
    if owner.disabled {
        return Err(Some("your account is disabled".into()));
    }
    if owner.must_change_password {
        return Err(Some("you need to change your password first".into()));
    }
    let who = Principal::from_user(owner.clone());
    let access = match ws::access(m, &who, &r.workspace_id).await {
        Ok(a) => a,
        Err(WsError::NotFound) => {
            return Err(Some("you are no longer a member of its workspace".into()));
        }
        Err(e) => {
            tracing::warn!(error = %e, "routines: could not check workspace access");
            return Err(None);
        }
    };
    access
        .require(Perm::Routines)
        .map_err(|e| Some(e.to_string()))?;
    let Some(spec) = RoutineSpec::read(&r.spec) else {
        return Err(Some("its settings cannot be read; edit the routine".into()));
    };
    if spec.agentic()
        && let Some(why) = ws::agentic_refusal(&who, Some(&access)).or_else(|| {
            spec.node_id
                .as_deref()
                .and_then(|n| ws::node_refusal(m, &who, n))
        })
    {
        return Err(Some(why));
    }
    if let Some(why) = super::spec::missing(m, access.id(), &spec).await {
        return Err(Some(why));
    }
    Ok((owner, access, spec))
}

/// Check a pending run against its owner's access and the caps, recording
/// any refusal on it. A run an event fired also needs its source's consent.
pub(crate) async fn admit(
    m: &Master,
    run_id: &str,
    event: Option<Arc<Event>>,
) -> Result<Admitted, Refused> {
    let Some(h) = handle(m) else {
        return Err(Refused::Gone);
    };
    let id = run_id.to_string();
    let loaded = m
        .inner
        .mirror
        .call(move |s| {
            let Some(run) = s.run(&id)? else {
                return Ok(None);
            };
            Ok::<_, harness_store::StoreError>(s.routine(&run.routine_id)?.map(|r| (run, r)))
        })
        .await;
    let (run, routine) = match loaded {
        Ok(Some((run, r))) if run.status == "pending" => (run, r),
        Ok(_) => return Err(Refused::Gone),
        Err(e) => {
            tracing::warn!(run = %run_id, error = %e, "routines: could not load a run");
            return Err(Refused::Gone);
        }
    };
    let (owner, access, spec) = match why_blocked(m, &routine).await {
        Ok(x) => x,
        Err(Some(why)) => {
            block(m, &routine, Some(run_id), &why, h.clock.now()).await;
            return Err(Refused::Blocked(why));
        }
        Err(None) => {
            let why = "could not check the owner's access; try again";
            finish(m, &run, &routine, "failed", Some(why.into())).await;
            return Err(Refused::Failed);
        }
    };
    if let Some(src) = event.as_ref().and_then(|e| events::source(e.kind))
        && let Err(why) = src.may_use(&Principal::from_user(owner.clone()), &access)
    {
        block(m, &routine, Some(run_id), &why, h.clock.now()).await;
        return Err(Refused::Blocked(why));
    }
    let cfg = config(m);
    let now = h.clock.now();
    let uid = owner.user_id.clone();
    let counts = m
        .inner
        .mirror
        .call(move |s| s.owner_run_counts(&uid, now - DAY))
        .await;
    let Ok((today, running)) = counts else {
        finish(
            m,
            &run,
            &routine,
            "failed",
            Some("could not count runs".into()),
        )
        .await;
        return Err(Refused::Failed);
    };
    // Both counts include this run.
    if today > i64::from(cfg.max_runs_per_user_per_day) {
        let why = format!(
            "the daily limit of {} routine runs was reached",
            cfg.max_runs_per_user_per_day
        );
        finish(m, &run, &routine, "skipped", Some(why)).await;
        // Paused until the day's window has room again (`lift_cap_pauses`).
        let id = routine.routine_id.clone();
        let paused = m
            .inner
            .mirror
            .call(move |s| s.routine_pause(&id, "cap", now))
            .await
            .unwrap_or(false);
        if paused {
            audit(
                m,
                "routine.paused",
                json!({ "routine_id": routine.routine_id, "reason": "cap" }),
            )
            .await;
        }
        return Err(Refused::Cap);
    }
    if running > i64::from(cfg.max_concurrent_per_user) {
        let why = format!(
            "{} of your routines were already running",
            cfg.max_concurrent_per_user
        );
        finish(m, &run, &routine, "skipped", Some(why)).await;
        return Err(Refused::Cap);
    }
    Ok(Admitted {
        run,
        routine,
        spec,
        owner,
        access,
        event,
    })
}

/// The values the prompt template of `run` can use.
async fn vars_for(m: &Master, a: &Admitted, prefs: &Prefs, now: i64) -> Vars {
    let (rid, run_id) = (a.routine.routine_id.clone(), a.run.run_id.clone());
    let trigger = a.run.trigger_id.clone();
    let (number, last, trigger) = m
        .inner
        .mirror
        .call(move |s| {
            let number = s.run_count(&rid).unwrap_or(1);
            let last = s.runs_page(&rid, Some(&run_id), 1).unwrap_or_default();
            let trigger = trigger.and_then(|t| s.trigger(&t).ok().flatten());
            (number, last.into_iter().next(), trigger)
        })
        .await;
    // `now.*` in the trigger's zone; Run now and intervals use the owner's.
    let tz = trigger
        .and_then(|t| ScheduleConfig::read(&t.config))
        .and_then(|c| c.tz().and_then(|z| TimeZone::get(z).ok()))
        .unwrap_or_else(|| zone_of(prefs));
    let zoned = |ms: i64| ts(ms).to_zoned(tz.clone());
    let display = prefs.display_name.as_deref().map(str::trim).unwrap_or("");
    Vars {
        now: zoned(now),
        routine_name: a.routine.name.clone(),
        routine_id: a.routine.routine_id.clone(),
        run_trigger: a.run.trigger_kind.clone(),
        run_number: number,
        run_scheduled: a.run.scheduled_ms.map(zoned),
        user_name: if display.is_empty() {
            a.owner.username.clone()
        } else {
            display.to_string()
        },
        workspace_name: a.access.workspace.name.clone(),
        last_run: last.map(|l| (l.status, zoned(l.started_ms))),
        event: a.event.as_ref().map(|e| e.data.clone()),
    }
}

/// "YYYY-MM-DD HH:MM" at clock-now in `owner`'s time zone: an event's
/// `received_at`.
pub(crate) async fn local_now(m: &Master, owner: &str) -> String {
    let now = handle(m).map_or_else(harness_core::now_ms, |h| h.clock.now());
    let prefs = Prefs::load(m, Some(owner)).await;
    ts(now)
        .to_zoned(zone_of(&prefs))
        .strftime("%Y-%m-%d %H:%M")
        .to_string()
}

/// Render the prompt, take a place, start the chat and prompt it.
async fn launch(m: Master, a: Admitted) {
    let Some(h) = handle(&m) else { return };
    let now = h.clock.now();
    let prefs = Prefs::load(&m, Some(&a.owner.user_id)).await;
    let vars = vars_for(&m, &a, &prefs, now).await;
    let rendered = match template::parse(&a.spec.prompt) {
        Ok(t) => t.render(&vars, a.spec.on_missing),
        Err(e) => {
            let why = format!("the prompt template is invalid: {e}");
            finish(&m, &a.run, &a.routine, "failed", Some(why)).await;
            return;
        }
    };
    let rendered = match rendered {
        Ok(r) => r,
        Err(e @ RenderError::Missing(_)) => {
            finish(&m, &a.run, &a.routine, "skipped", Some(e.to_string())).await;
            return;
        }
        Err(e) => {
            finish(&m, &a.run, &a.routine, "failed", Some(e.to_string())).await;
            return;
        }
    };
    let slot = match tokio::time::timeout(SLOT_WAIT, h.slots.clone().acquire_owned()).await {
        Ok(Ok(slot)) => slot,
        _ => {
            let why = "the server was busy: no place for the run within 10 minutes";
            finish(&m, &a.run, &a.routine, "failed", Some(why.into())).await;
            return;
        }
    };
    let zone = zone_of(&prefs);
    let local = ts(now)
        .to_zoned(zone)
        .strftime("%Y-%m-%d %H:%M")
        .to_string();
    let sid = match a.run.session_id.as_deref().map(str::parse::<SessionId>) {
        Some(Ok(sid)) => sid,
        _ => {
            let spec = CreateSpec {
                profile: Some(a.spec.profile),
                node_id: a.spec.node_id.clone(),
                // Never event data.
                title: format!("{} · {local}", a.routine.name),
                cwd: a.spec.cwd.clone().unwrap_or_default(),
                model: a.spec.model.clone(),
                approval_mode: Some(approval_for(
                    &a.spec,
                    rendered.untrusted,
                    a.spec.node_id.as_deref(),
                )),
                agent_id: Some(a.spec.agent_id.clone()),
                owner_id: Some(a.owner.user_id.clone()),
                workspace_id: Some(a.routine.workspace_id.clone()),
                project_id: a.spec.project_id.clone(),
                ..Default::default()
            };
            match m.create_session(spec).await {
                Ok(sid) => sid,
                Err(e) => {
                    drop(slot);
                    launch_failed(&m, &a, &a.run, e).await;
                    return;
                }
            }
        }
    };
    // Bound before the prompt goes out, so its first turn is never missed.
    lock(&h.bound).insert(
        sid,
        Bound {
            run_id: a.run.run_id.clone(),
            routine_id: a.routine.routine_id.clone(),
            owner: a.owner.user_id.clone(),
            turn_id: None,
            slot: Some(slot),
            waiting_since: None,
            running_since: now,
            max_runtime_ms: i64::from(a.spec.max_runtime_min) * MINUTE,
            timed_out: false,
        },
    );
    let patch = RunPatch {
        status: Some("running".into()),
        reason: Some(None),
        session_id: Some(sid.to_string()),
        prompt: Some(rendered.text.clone()),
        untrusted: Some(rendered.untrusted),
        warnings: Some(json!(rendered.warnings).to_string()),
        only_active: true,
        ..Default::default()
    };
    let run_id = a.run.run_id.clone();
    let stored = m
        .inner
        .mirror
        .call(move |s| s.run_update(&run_id, &patch))
        .await;
    if !matches!(stored, Ok(true)) {
        // Deleted or ended meanwhile.
        lock(&h.bound).remove(&sid);
        return;
    }
    // The event by its size and hash only, never its content.
    audit(
        &m,
        "routine.run",
        json!({ "routine_id": a.routine.routine_id, "run_id": a.run.run_id,
                "trigger": a.run.trigger_kind, "session_id": sid,
                "untrusted": rendered.untrusted,
                "event_bytes": a.event.as_ref().map(|e| e.size),
                "event_sha256": a.event.as_ref().map(|e| &e.sha256) }),
    )
    .await;
    changed(&m, &a.owner.user_id);
    // A retry whose last prompt did reach the node (only its ack was lost)
    // must not ask twice: follow the turn that is there.
    if a.run.session_id.is_some() {
        let started = m
            .inner
            .mirror
            .call(move |s| s.first_event(sid, "turn_started"))
            .await;
        if let Ok(Some(_)) = started {
            let run = RunRow {
                status: "running".into(),
                session_id: Some(sid.to_string()),
                ..a.run.clone()
            };
            reconcile_run(&m, h, &run, true).await;
            return;
        }
    }
    let trigger = match a.run.trigger_kind.as_str() {
        "manual" => "Run now",
        other => other,
    };
    let note = format!(
        "Started by routine \u{2039}{}\u{203a} ({trigger}) at {local}, not typed by the user just now.",
        a.routine.name
    );
    let context = format!(
        "{}\n\n{note}",
        prefs.model_context(&a.owner.username, ts(now))
    );
    let opts = PromptOpts {
        agent_id: Some(a.spec.agent_id.clone()),
        effort: a.spec.effort.clone(),
        context: Some(context),
        untrusted: rendered.untrusted,
        ..Default::default()
    };
    if let Err(e) = m.prompt(sid, rendered.text, opts).await {
        lock(&h.bound).remove(&sid);
        let run = RunRow {
            session_id: Some(sid.to_string()),
            ..a.run.clone()
        };
        launch_failed(&m, &a, &run, e).await;
    }
}

/// Run `a` (as `run` stands now) could not start its turn: the node was
/// away (retry), the routine names something gone (blocked), or it failed.
async fn launch_failed(m: &Master, a: &Admitted, run: &RunRow, e: CommandError) {
    let now = handle(m).map_or_else(harness_core::now_ms, |h| h.clock.now());
    let r = &a.routine;
    match e {
        CommandError::NodeOffline(_) | CommandError::Timeout => {
            retry_later(m, run, r, e.to_string(), a.event.clone()).await
        }
        CommandError::UnknownAgent(_)
        | CommandError::SubagentOnly { .. }
        | CommandError::AgentGone(_)
        | CommandError::BadDefault(_)
        | CommandError::Rejected(_) => block(m, r, Some(&run.run_id), &e.to_string(), now).await,
        _ => finish(m, run, r, "failed", Some(e.to_string())).await,
    }
}

/// Try `run` again after the next retry delay, unless it used them all or
/// its trigger's next occurrence comes first.
async fn retry_later(
    m: &Master,
    run: &RunRow,
    r: &RoutineRow,
    why: String,
    event: Option<Arc<Event>>,
) {
    let Some(h) = handle(m) else { return };
    let now = h.clock.now();
    let Some(delay) = RETRY_MIN.get((run.attempt - 1).max(0) as usize) else {
        finish(m, run, r, "failed", Some(why)).await;
        return;
    };
    let due = now + delay * MINUTE;
    let tid = run.trigger_id.clone();
    let next = match tid {
        Some(tid) => m
            .inner
            .mirror
            .call(move |s| s.trigger(&tid))
            .await
            .ok()
            .flatten()
            .and_then(|t| t.next_run_ms),
        None => None,
    };
    if next.is_some_and(|n| due >= n) {
        finish(
            m,
            run,
            r,
            "failed",
            Some(format!("{why}; the next run is due first")),
        )
        .await;
        return;
    }
    let patch = RunPatch {
        status: Some("pending".into()),
        attempt: Some(run.attempt + 1),
        reason: Some(Some(format!("{why}; trying again in {delay} min"))),
        only_active: true,
        ..Default::default()
    };
    let id = run.run_id.clone();
    // The row and the queue change together for a tick (`queueing`).
    let _queueing = h.queueing.lock().await;
    if let Ok(true) = m
        .inner
        .mirror
        .call(move |s| s.run_update(&id, &patch))
        .await
    {
        lock(&h.retries).push(Retry {
            run_id: run.run_id.clone(),
            due,
            event,
        });
        h.wake();
        changed(m, &r.owner_id);
    }
}

/// End `run` as `status`, counting it against the routine.
async fn finish(m: &Master, run: &RunRow, r: &RoutineRow, status: &str, reason: Option<String>) {
    end_run(m, &run.run_id, &r.routine_id, &r.owner_id, status, reason).await
}

async fn end_run(
    m: &Master,
    run_id: &str,
    routine_id: &str,
    owner: &str,
    status: &str,
    reason: Option<String>,
) {
    let now = handle(m).map_or_else(harness_core::now_ms, |h| h.clock.now());
    let (id, st) = (run_id.to_string(), status.to_string());
    let ended = m
        .inner
        .mirror
        .call(move |s| s.run_finish(&id, &st, reason.as_deref(), now))
        .await;
    match ended {
        Ok(true) => {
            counted(m, routine_id, status, now).await;
            changed(m, owner);
        }
        Ok(false) => {}
        Err(e) => tracing::warn!(run = %run_id, error = %e, "routines: could not end a run"),
    }
}

/// Count a finished run: failures in a row pause the routine; a completed
/// run resets them. Skipped, missed, cancelled and blocked runs count for
/// neither.
async fn counted(m: &Master, routine_id: &str, status: &str, now: i64) {
    let failed = matches!(status, "error" | "failed" | "timed_out" | "interrupted");
    if !failed && status != "completed" {
        return;
    }
    let id = routine_id.to_string();
    let res = m
        .inner
        .mirror
        .call(move |s| {
            let n = s.routine_count_failure(&id, failed)?;
            if failed && n >= MAX_FAILURES {
                return s.routine_pause(&id, "failures", now);
            }
            Ok(false)
        })
        .await;
    if let Ok(true) = res {
        audit(
            m,
            "routine.paused",
            json!({ "routine_id": routine_id, "reason": "failures" }),
        )
        .await;
    }
}

/// Refuse a run (or the routine itself): record it as blocked, pause the
/// routine and say why in the audit log and the owner's attention list.
async fn block(m: &Master, r: &RoutineRow, run_id: Option<&str>, why: &str, now: i64) {
    let (rid, run, reason) = (
        r.routine_id.clone(),
        run_id.map(str::to_string),
        why.to_string(),
    );
    let paused = m
        .inner
        .mirror
        .call(move |s| {
            if let Some(run) = &run {
                s.run_finish(run, "blocked", Some(&reason), now)?;
            }
            s.routine_pause(&rid, "blocked", now)
        })
        .await;
    audit(
        m,
        "routine.blocked",
        json!({ "routine_id": r.routine_id, "run_id": run_id, "owner": r.owner_id,
                "workspace": r.workspace_id, "reason": why }),
    )
    .await;
    if let Ok(true) = paused {
        audit(
            m,
            "routine.paused",
            json!({ "routine_id": r.routine_id, "reason": "blocked" }),
        )
        .await;
    }
    changed(m, &r.owner_id);
}

/// Run now: start a run of `r` that no trigger owns.
pub(crate) enum Manual {
    Started(String),
    /// `overlap` or `cap`.
    Skipped(&'static str),
    Blocked(String),
    Failed,
}

/// Run `r` now, with a `sample` event in place of a webhook's (the run is
/// then untrusted as soon as the prompt uses it).
pub(crate) async fn run_now(m: &Master, r: &RoutineRow, sample: Option<Event>) -> Manual {
    let Some(h) = handle(m) else {
        return Manual::Failed;
    };
    let run = NewRun {
        run_id: new_id(),
        routine_id: r.routine_id.clone(),
        trigger_id: None,
        trigger_kind: "manual".into(),
        scheduled_ms: None,
        started_ms: h.clock.now(),
        status: "pending".into(),
        reason: None,
        repeats: 1,
        event: sample.as_ref().map(|e| events::stored(&e.data)),
    };
    let claim = run.clone();
    let started = m.inner.mirror.call(move |s| s.run_start(&claim)).await;
    changed(m, &r.owner_id);
    match started {
        Ok(Claimed::Started) => {}
        Ok(_) => return Manual::Skipped("overlap"),
        Err(e) => {
            tracing::warn!(error = %e, "routines: could not start a run");
            return Manual::Failed;
        }
    }
    match admit_and_launch(m, &run.run_id, sample.map(Arc::new)).await {
        Ok(()) => Manual::Started(run.run_id),
        Err(Refused::Cap) => Manual::Skipped("cap"),
        Err(Refused::Blocked(why)) => Manual::Blocked(why),
        Err(Refused::Gone | Refused::Failed) => Manual::Failed,
    }
}

/// Admit a run just recorded as pending and launch it in the background.
/// Admitted here, not in the background, so the caller hears of a cap or
/// a block.
async fn admit_and_launch(
    m: &Master,
    run_id: &str,
    event: Option<Arc<Event>>,
) -> Result<(), Refused> {
    let a = admit(m, run_id, event).await?;
    tokio::spawn(launch(m.clone(), a));
    Ok(())
}

/// What a webhook call came to (`http/hooks.rs` answers from it).
pub enum Fired {
    Started(String),
    /// Its `Idempotency-Key` came with the call that started this run.
    Duplicate(String),
    /// `overlap` or `cap`.
    Skipped(&'static str),
    /// Its owner may no longer run it: blocked, and the routine paused.
    Paused,
    Failed,
}

/// Fire webhook routine `r` from trigger `t` with `event`: record the run
/// (once per `Idempotency-Key`), admit it and launch it.
pub(crate) async fn fire_event(m: &Master, t: &TriggerRow, r: &RoutineRow, event: Event) -> Fired {
    let Some(h) = handle(m) else {
        return Fired::Failed;
    };
    let now = h.clock.now();
    let claim = NewRun {
        run_id: new_id(),
        routine_id: r.routine_id.clone(),
        trigger_id: Some(t.trigger_id.clone()),
        trigger_kind: t.kind.clone(),
        scheduled_ms: None,
        started_ms: now,
        status: "pending".into(),
        reason: None,
        repeats: 1,
        event: Some(events::stored(&event.data)),
    };
    let (rid, tid, key) = (
        r.routine_id.clone(),
        t.trigger_id.clone(),
        event.dedupe_key.clone(),
    );
    let res = m
        .inner
        .mirror
        .call(move |s| s.webhook_claim(&rid, &tid, key.as_deref(), &claim, now))
        .await;
    let run_id = match res {
        Ok(HookClaim::Duplicate(first)) => return Fired::Duplicate(first),
        Ok(HookClaim::Claimed(Claimed::Started, id)) => id,
        Ok(HookClaim::Claimed(..)) => {
            changed(m, &r.owner_id);
            return Fired::Skipped("overlap");
        }
        Err(e) => {
            tracing::warn!(routine = %r.routine_id, error = %e, "routines: could not record a webhook call");
            return Fired::Failed;
        }
    };
    changed(m, &r.owner_id);
    match admit_and_launch(m, &run_id, Some(Arc::new(event))).await {
        Ok(()) => Fired::Started(run_id),
        Err(Refused::Cap) => Fired::Skipped("cap"),
        Err(Refused::Blocked(_)) => Fired::Paused,
        Err(Refused::Gone | Refused::Failed) => Fired::Failed,
    }
}

// ---- the watcher ----

pub(super) fn start_watcher(m: &Master, ready: watch::Sender<bool>) {
    // Subscribed before reconciling, so an event applied in between is
    // never missed.
    let rx = m.subscribe();
    let m = m.clone();
    tokio::spawn(async move {
        reconcile(&m, true).await;
        let _ = ready.send(true);
        watch_events(&m, rx).await;
    });
}

async fn watch_events(m: &Master, mut rx: tokio::sync::broadcast::Receiver<Live>) {
    loop {
        match rx.recv().await {
            Ok(Live::Event(e)) => on_event(m, &e).await,
            Ok(_) => {}
            Err(RecvError::Lagged(n)) => {
                tracing::warn!(missed = n, "routines: the watcher fell behind; reconciling");
                reconcile(m, false).await;
            }
            Err(RecvError::Closed) => return,
        }
    }
}

async fn on_event(m: &Master, e: &RawEvent) {
    let Some(h) = handle(m) else { return };
    match e.kind.as_str() {
        "turn_started" => turn_started(m, h, e).await,
        "approval_requested" => approval(m, h, e.session_id, true).await,
        "approval_resolved" => approval(m, h, e.session_id, false).await,
        "turn_ended" => turn_ended(m, h, e).await,
        _ => {}
    }
}

async fn turn_started(m: &Master, h: &RoutinesHandle, e: &RawEvent) {
    let Some(turn) = e.payload.get("turn_id").and_then(Value::as_str) else {
        return;
    };
    let run_id = {
        let mut bound = lock(&h.bound);
        match bound.get_mut(&e.session_id) {
            Some(b) if b.turn_id.is_none() => {
                b.turn_id = Some(turn.to_string());
                b.run_id.clone()
            }
            _ => return,
        }
    };
    let patch = RunPatch {
        turn_id: Some(turn.to_string()),
        only_active: true,
        ..Default::default()
    };
    let _ = m
        .inner
        .mirror
        .call(move |s| s.run_update(&run_id, &patch))
        .await;
}

/// An approval was asked or answered in `sid`: the run's chat, or a
/// sub-agent session it delegated to.
async fn approval(m: &Master, h: &RoutinesHandle, sid: SessionId, asked: bool) {
    let chat = if lock(&h.bound).contains_key(&sid) {
        sid
    } else {
        let parent = m
            .inner
            .mirror
            .call(move |s| s.run_by_child_session(sid))
            .await;
        match parent
            .ok()
            .flatten()
            .and_then(|r| r.session_id)
            .and_then(|s| s.parse().ok())
        {
            Some(chat) => chat,
            None => return,
        }
    };
    let now = h.clock.now();
    if !asked {
        let pending = m
            .inner
            .mirror
            .call(move |s| s.pending_approvals_with_children(chat))
            .await
            .unwrap_or(1);
        if pending > 0 {
            return;
        }
    }
    let (run_id, owner) = {
        let mut bound = lock(&h.bound);
        let Some(b) = bound.get_mut(&chat) else {
            return;
        };
        if b.timed_out || b.waiting_since.is_some() == asked {
            return;
        }
        if asked {
            b.waiting_since = Some(now);
            // A parked run gives its place to the next one.
            b.slot = None;
        } else {
            b.waiting_since = None;
            b.running_since = now;
        }
        (b.run_id.clone(), b.owner.clone())
    };
    let patch = RunPatch {
        status: Some(if asked { "waiting" } else { "running" }.into()),
        waiting_since_ms: Some(asked.then_some(now)),
        only_active: true,
        ..Default::default()
    };
    let _ = m
        .inner
        .mirror
        .call(move |s| s.run_update(&run_id, &patch))
        .await;
    changed(m, &owner);
}

async fn turn_ended(m: &Master, h: &RoutinesHandle, e: &RawEvent) {
    let turn = e.payload.get("turn_id").and_then(Value::as_str);
    let b = {
        let mut bound = lock(&h.bound);
        match bound.get(&e.session_id) {
            Some(b) if b.turn_id.is_none() || b.turn_id.as_deref() == turn => {
                bound.remove(&e.session_id)
            }
            _ => None,
        }
    };
    if let Some(b) = b {
        finalize(m, &b, &e.payload).await;
    }
}

/// Record how the run's turn ended (`turn_ended` payload `end`).
async fn finalize(m: &Master, b: &Bound, end: &Value) {
    if let Some(usage) = end.get("usage").filter(|u| u.is_object()) {
        let patch = RunPatch {
            usage: Some(usage.to_string()),
            ..Default::default()
        };
        let id = b.run_id.clone();
        let _ = m
            .inner
            .mirror
            .call(move |s| s.run_update(&id, &patch))
            .await;
    }
    if b.timed_out {
        changed(m, &b.owner);
        return;
    }
    let (status, reason) = match end.get("reason").and_then(Value::as_str) {
        Some("completed") => ("completed", None),
        Some("cancelled") => ("cancelled", Some("the turn was cancelled".to_string())),
        Some("interrupted") => ("interrupted", Some("the turn was interrupted".to_string())),
        _ => ("error", Some("the turn ended with an error".to_string())),
    };
    end_run(m, &b.run_id, &b.routine_id, &b.owner, status, reason).await;
}

/// Bring active runs in line with the mirror: at startup, pending runs were
/// never started (`interrupted`, never retried) and the others are
/// followed again or finished from their chat; after the watcher fell
/// behind, the same for runs that are not in flight here.
async fn reconcile(m: &Master, startup: bool) {
    let Some(h) = handle(m) else { return };
    let runs = match m.inner.mirror.call(|s| s.active_runs()).await {
        Ok(runs) => runs,
        Err(e) => {
            tracing::warn!(error = %e, "routines: could not list active runs");
            return;
        }
    };
    for run in runs {
        if run.status == "pending" {
            if startup {
                let why = "the server restarted before the run started";
                end_run_of(m, &run, "interrupted", why).await;
            }
            continue;
        }
        reconcile_run(m, h, &run, startup).await;
    }
}

async fn end_run_of(m: &Master, run: &RunRow, status: &str, why: &str) {
    let rid = run.routine_id.clone();
    let owner = m
        .inner
        .mirror
        .call(move |s| s.routine(&rid))
        .await
        .ok()
        .flatten()
        .map(|r| r.owner_id)
        .unwrap_or_default();
    end_run(
        m,
        &run.run_id,
        &run.routine_id,
        &owner,
        status,
        Some(why.into()),
    )
    .await;
}

/// Follow or finish one running or waiting run from its chat's state.
/// Unless `all`, a run this process just prompted (its turn not started
/// yet) is left to the watcher.
async fn reconcile_run(m: &Master, h: &RoutinesHandle, run: &RunRow, all: bool) {
    let Some(sid) = run
        .session_id
        .as_deref()
        .and_then(|s| s.parse::<SessionId>().ok())
    else {
        end_run_of(m, run, "interrupted", "the run lost its chat").await;
        return;
    };
    if !all
        && lock(&h.bound)
            .get(&sid)
            .is_some_and(|b| b.turn_id.is_none())
    {
        return;
    }
    let rid = run.routine_id.clone();
    let state = m
        .inner
        .mirror
        .call(move |s| {
            let row = s.session(sid)?;
            let end = s.first_event(sid, "turn_ended")?;
            let start = s.first_event(sid, "turn_started")?;
            let pending = s.pending_approvals_with_children(sid)?;
            let routine = s.routine(&rid)?;
            Ok::<_, harness_store::StoreError>((row, end, start, pending, routine))
        })
        .await;
    let Ok((row, end, start, pending, Some(routine))) = state else {
        return;
    };
    let spec = RoutineSpec::read(&routine.spec).unwrap_or_default();
    let now = h.clock.now();
    let existing = lock(&h.bound).remove(&sid);
    let mut b = existing.unwrap_or_else(|| Bound {
        run_id: run.run_id.clone(),
        routine_id: run.routine_id.clone(),
        owner: routine.owner_id.clone(),
        turn_id: run.turn_id.clone(),
        slot: None,
        waiting_since: run.waiting_since_ms,
        running_since: now,
        max_runtime_ms: i64::from(spec.max_runtime_min) * MINUTE,
        timed_out: false,
    });
    if b.turn_id.is_none() {
        b.turn_id = start
            .as_ref()
            .and_then(|s| s.get("turn_id")?.as_str().map(str::to_string));
    }
    if let Some(end) = end {
        finalize(m, &b, &end).await;
        return;
    }
    if !row.is_some_and(|r| r.running) {
        let why = "the run's turn stopped without finishing";
        end_run(
            m,
            &b.run_id,
            &b.routine_id,
            &b.owner,
            "interrupted",
            Some(why.into()),
        )
        .await;
        return;
    }
    let (run_id, owner, waiting) = (b.run_id.clone(), b.owner.clone(), b.waiting_since);
    lock(&h.bound).insert(sid, b);
    // Waiting or not as the chat says now.
    let asked = pending > 0;
    if asked != waiting.is_some() || (asked && run.status != "waiting") {
        {
            let mut bound = lock(&h.bound);
            if let Some(b) = bound.get_mut(&sid) {
                b.waiting_since = asked.then(|| waiting.unwrap_or(now));
                if asked {
                    b.slot = None;
                } else {
                    b.running_since = now;
                }
            }
        }
        let patch = RunPatch {
            status: Some(if asked { "waiting" } else { "running" }.into()),
            waiting_since_ms: Some(asked.then(|| waiting.unwrap_or(now))),
            only_active: true,
            ..Default::default()
        };
        let _ = m
            .inner
            .mirror
            .call(move |s| s.run_update(&run_id, &patch))
            .await;
        changed(m, &owner);
    }
}

// ---- timeouts ----

pub(super) async fn sweep_timeouts(h: &RoutinesHandle, m: &Master) {
    let now = h.clock.now();
    let hours = config(m).approval_timeout_hours;
    let wait_ms = i64::from(hours) * HOUR;
    let due: Vec<(SessionId, String, String, String, bool, String)> = {
        let mut bound = lock(&h.bound);
        bound
            .iter_mut()
            .filter_map(|(sid, b)| {
                if b.timed_out {
                    return None;
                }
                let why = match b.waiting_since {
                    Some(w) if now - w >= wait_ms => {
                        format!("waited {hours} hours for an approval")
                    }
                    None if now - b.running_since >= b.max_runtime_ms => {
                        format!("ran past its limit of {} min", b.max_runtime_ms / MINUTE)
                    }
                    _ => return None,
                };
                b.timed_out = true;
                b.slot = None;
                Some((
                    *sid,
                    b.run_id.clone(),
                    b.routine_id.clone(),
                    b.owner.clone(),
                    b.turn_id.is_none(),
                    why,
                ))
            })
            .collect()
    };
    for (sid, run_id, routine_id, owner, no_turn, why) in due {
        end_run(m, &run_id, &routine_id, &owner, "timed_out", Some(why)).await;
        if let Err(e) = m.cancel(sid).await {
            tracing::warn!(session = %sid, error = %e, "routines: could not cancel a timed-out run");
        }
        // No turn started, so none will end: stop following it.
        if no_turn {
            lock(&h.bound).remove(&sid);
        }
    }
}
