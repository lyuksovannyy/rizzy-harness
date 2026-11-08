//! Routines over HTTP (docs/routines.md, API). A routine is private to its
//! owner: anyone else gets 404, server admins included; the API token may
//! read every routine but owns none. Changing or running one needs the
//! `routines` permission in its workspace now (and the agentic rights for
//! an agentic routine); deleting one never does, so an owner who lost the
//! permission can still clear it away.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use harness_core::SessionId;
use harness_store::routines::{
    NewRoutine, NewTrigger, RoutineCaps, RoutinePatch, RoutineRow, RunRow, TriggerRow,
};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, require};
use crate::Master;
use crate::auth::{self, Principal};
use crate::routines::schedule::{self, Schedule, ScheduleConfig};
use crate::routines::spec::{self as rspec, RoutineSpec};
use crate::routines::template::{self, TemplateError, Vars};
use crate::routines::{self as r, RoutinesConfig, runner};
use crate::workspaces::{self as ws, Access, WsError};

pub(super) fn routes() -> Router<Master> {
    Router::new()
        .route("/api/routines", get(list).post(create))
        .route("/api/routines/preview", post(preview))
        .route(
            "/api/routines/{id}",
            get(detail).patch(update).delete(remove),
        )
        .route("/api/routines/{id}/run", post(run_now))
        .route("/api/routines/{id}/runs", get(runs))
        .route("/api/routines/{id}/runs/{run_id}", get(run_detail))
        .route("/api/routines/{id}/runs/{run_id}/seen", post(seen))
}

/// An error answer: most are `ApiError`s; a template error also says
/// where it is (`{error, offset}`).
enum Error {
    Api(ApiError),
    At(TemplateError),
}

impl<E> From<E> for Error
where
    ApiError: From<E>,
{
    fn from(e: E) -> Self {
        Error::Api(ApiError::from(e))
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        match self {
            Error::Api(e) => e.into_response(),
            Error::At(e) => (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("prompt: {}", e.message), "offset": e.offset })),
            )
                .into_response(),
        }
    }
}

type Result<T> = std::result::Result<T, Error>;

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

fn forbidden(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::FORBIDDEN, msg.into())
}

fn conflict(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::CONFLICT, msg.into())
}

fn not_found() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "unknown routine".into())
}

/// A request body, with any shape problem as a 400.
fn body<T: DeserializeOwned>(v: Value) -> ApiResult<T> {
    serde_json::from_value(v).map_err(|e| bad(e.to_string()))
}

fn now(m: &Master) -> i64 {
    r::handle(m).map_or_else(harness_core::now_ms, |h| h.clock.now())
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default)]
    workspace: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TriggerBody {
    #[serde(default)]
    trigger_id: Option<String>,
    kind: String,
    #[serde(default)]
    config: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateBody {
    #[serde(default)]
    workspace_id: Option<String>,
    name: String,
    #[serde(default = "yes")]
    enabled: bool,
    spec: Value,
    triggers: Vec<TriggerBody>,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchBody {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    spec: Option<Value>,
    #[serde(default)]
    triggers: Option<Vec<TriggerBody>>,
}

/// The routine, if the caller may see it: its owner, or the API token.
async fn visible(m: &Master, who: &Principal, id: &str) -> ApiResult<RoutineRow> {
    let rid = id.to_string();
    match m.inner.mirror.call(move |s| s.routine(&rid)).await? {
        Some(row) if who.can_see(Some(&row.owner_id)) => Ok(row),
        _ => Err(not_found()),
    }
}

/// The routine, if the caller may change or run it: its owner, a member
/// of its workspace with the `routines` permission.
async fn owned(m: &Master, who: &Principal, id: &str) -> ApiResult<(RoutineRow, Access)> {
    let row = visible(m, who, id).await?;
    if who.user_id.is_none() {
        return Err(forbidden(r::TOKEN_CANNOT_OWN));
    }
    let a = match ws::access(m, who, &row.workspace_id).await {
        Ok(a) => a,
        Err(WsError::NotFound) => {
            return Err(forbidden(
                "you are no longer a member of this routine's workspace; you can only delete it",
            ));
        }
        Err(e) => return Err(e.into()),
    };
    if let Some(why) = r::may_not_own(who, &a) {
        return Err(forbidden(why));
    }
    Ok((row, a))
}

/// Refuse (403) an agentic spec to someone without the agentic rights.
fn agentic_allowed(who: &Principal, a: &Access, spec: &RoutineSpec) -> ApiResult<()> {
    match spec
        .agentic()
        .then(|| ws::agentic_refusal(who, Some(a)))
        .flatten()
    {
        Some(why) => Err(forbidden(why)),
        None => Ok(()),
    }
}

/// The zone new schedules default to: the owner's, else UTC.
async fn default_zone(m: &Master, who: &Principal) -> String {
    crate::prefs::Prefs::load(m, who.user_id.as_deref())
        .await
        .timezone
        .filter(|t| jiff::tz::TimeZone::get(t.trim()).is_ok())
        .unwrap_or_else(|| "UTC".into())
}

/// A routine's triggers, checked.
struct Triggers {
    rows: Vec<NewTrigger>,
    schedules: Vec<Schedule>,
}

/// Check a trigger set: 1 to `max_triggers` schedules, or exactly one
/// event (webhooks come in a later version); ids the routine already has
/// keep their trigger.
fn parse_triggers(
    cfg: &RoutinesConfig,
    zone: &str,
    list: &[TriggerBody],
    existing: &[TriggerRow],
    now: i64,
) -> ApiResult<Triggers> {
    let schedules = list.iter().filter(|t| t.kind == "schedule").count();
    if let Some(t) = list
        .iter()
        .find(|t| t.kind != "schedule" && t.kind != "webhook")
    {
        return Err(bad(format!("unknown trigger kind {:?}", t.kind)));
    }
    if schedules > 0 && schedules < list.len() {
        return Err(bad(
            "A routine runs on a schedule or on one event, not both",
        ));
    }
    if schedules < list.len() {
        return Err(bad("webhooks are not available yet"));
    }
    if list.is_empty() {
        return Err(bad("add a schedule: a routine runs on 1 to 8 schedules"));
    }
    if schedules > cfg.max_triggers as usize {
        return Err(conflict(format!(
            "a routine has at most {} schedules",
            cfg.max_triggers
        )));
    }
    let mut out = Triggers {
        rows: Vec::new(),
        schedules: Vec::new(),
    };
    for t in list {
        let id = match &t.trigger_id {
            Some(id) if !existing.iter().any(|e| &e.trigger_id == id) => {
                return Err(bad(format!("unknown trigger {id:?}")));
            }
            Some(id) if out.rows.iter().any(|r| &r.trigger_id == id) => {
                return Err(bad(format!("trigger {id:?} is listed twice")));
            }
            Some(id) => id.clone(),
            None => auth::random_hex(16),
        };
        let config = ScheduleConfig::from_request(&t.config, zone, now).map_err(bad)?;
        let schedule = config.compile(cfg.min_interval_minutes, now).map_err(bad)?;
        out.rows.push(NewTrigger {
            trigger_id: id,
            kind: t.kind.clone(),
            config: serde_json::to_string(&config).expect("config serializes"),
            next_run_ms: schedule.next_after_ms(now),
            secret_hash: None,
        });
        out.schedules.push(schedule);
    }
    Ok(out)
}

/// The prompt checked against the namespaces this routine has (a schedule
/// routine has no `event.*`).
fn check_prompt(spec: &RoutineSpec) -> Result<()> {
    let t = template::parse(&spec.prompt).map_err(Error::At)?;
    t.check(None).map_err(Error::At)
}

fn trigger_json(t: &TriggerRow) -> Value {
    json!({
        "trigger_id": t.trigger_id,
        "kind": t.kind,
        "config": serde_json::from_str::<Value>(&t.config).unwrap_or(Value::Null),
        "next_run_ms": t.next_run_ms,
        "last_fired_ms": t.last_fired_ms,
    })
}

/// A run as the UI shows it; `preview` is the first sentence of its chat's
/// latest message.
fn run_json(run: &RunRow, preview: Option<&String>) -> Value {
    let usage = run
        .usage
        .as_deref()
        .and_then(|u| serde_json::from_str::<Value>(u).ok())
        .map(|u| json!({ "input_tokens": u["input_tokens"], "output_tokens": u["output_tokens"] }));
    let warnings: Vec<String> = run
        .warnings
        .as_deref()
        .and_then(|w| serde_json::from_str(w).ok())
        .unwrap_or_default();
    json!({
        "run_id": run.run_id,
        "routine_id": run.routine_id,
        "trigger_kind": run.trigger_kind,
        "scheduled_ms": run.scheduled_ms,
        "started_ms": run.started_ms,
        "ended_ms": run.ended_ms,
        "status": run.status,
        "reason": run.reason,
        "session_id": run.session_id,
        "chat_deleted": run.chat_deleted,
        "usage": usage,
        "warnings": warnings,
        "untrusted": run.untrusted,
        "attempt": run.attempt,
        "repeats": run.repeats,
        "seen": run.seen,
        "answer_preview": preview,
    })
}

/// The first sentence of each run chat's latest message, by session id.
async fn previews(m: &Master, runs: &[RunRow]) -> ApiResult<HashMap<String, String>> {
    let sids: Vec<SessionId> = runs
        .iter()
        .filter_map(|r| r.session_id.as_deref()?.parse().ok())
        .take(50)
        .collect();
    if sids.is_empty() {
        return Ok(HashMap::new());
    }
    let texts = m
        .inner
        .mirror
        .call(move |s| s.latest_message_texts(&sids))
        .await?;
    Ok(texts
        .into_iter()
        .map(|(sid, text)| (sid.to_string(), crate::search::first_sentence(&text, 140)))
        .collect())
}

/// A routine as the UI shows it. `blocked` is checked live: why its owner
/// may not run it now.
async fn routine_json(m: &Master, row: &RoutineRow, unseen: i64) -> ApiResult<Value> {
    let id = row.routine_id.clone();
    let (triggers, last) = m
        .inner
        .mirror
        .call(move |s| {
            let triggers = s.triggers(&id)?;
            let last = s.runs_page(&id, None, 1)?.into_iter().next();
            Ok::<_, harness_store::StoreError>((triggers, last))
        })
        .await?;
    let blocked = match runner::why_blocked(m, row).await {
        Ok(_) => None,
        Err(why) => why,
    };
    let firing = row.enabled && row.paused_reason.is_none();
    let next_run_ms = triggers
        .iter()
        .filter_map(|t| t.next_run_ms)
        .min()
        .filter(|_| firing);
    let last_run = match &last {
        Some(run) => {
            let p = previews(m, std::slice::from_ref(run)).await?;
            Some(run_json(
                run,
                run.session_id.as_ref().and_then(|s| p.get(s)),
            ))
        }
        None => None,
    };
    Ok(json!({
        "routine_id": row.routine_id,
        "workspace_id": row.workspace_id,
        "name": row.name,
        "enabled": row.enabled,
        "paused_reason": row.paused_reason,
        "spec": RoutineSpec::read(&row.spec).map(|s| json!(s)).unwrap_or(Value::Null),
        "triggers": triggers.iter().map(trigger_json).collect::<Vec<_>>(),
        "next_run_ms": next_run_ms,
        "last_run": last_run,
        "unseen": unseen,
        "blocked": blocked,
        // The paths of the last webhook body (webhooks come later).
        "event_paths": [],
        "created_ms": row.created_ms,
        "updated_ms": row.updated_ms,
    }))
}

async fn unseen_of(m: &Master, owner: &str) -> ApiResult<HashMap<String, i64>> {
    let owner = owner.to_string();
    Ok(m.inner
        .mirror
        .call(move |s| s.routine_unseen(Some(&owner)))
        .await?)
}

async fn one_json(m: &Master, row: &RoutineRow) -> ApiResult<Value> {
    let unseen = unseen_of(m, &row.owner_id).await?;
    routine_json(m, row, unseen.get(&row.routine_id).copied().unwrap_or(0)).await
}

fn limits(cfg: &RoutinesConfig) -> Value {
    json!({
        "max_per_user": cfg.max_per_user,
        "max_per_workspace": cfg.max_per_workspace,
        "max_triggers": cfg.max_triggers,
        "min_interval_minutes": cfg.min_interval_minutes,
        "max_prompt": rspec::MAX_PROMPT_TEMPLATE,
        "max_runtime_min": rspec::MAX_RUNTIME_MIN,
        "approval_timeout_hours": cfg.approval_timeout_hours,
    })
}

async fn list(
    State(m): State<Master>,
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let scope = ws::scope(&m, &who, q.workspace.as_deref()).await?;
    let ws_id = scope.as_ref().map(|a| a.id().to_string());
    // Each user lists their own; the API token every owner's.
    let (owner, filter) = (who.user_id.clone(), ws_id.clone());
    let rows = m
        .inner
        .mirror
        .call(move |s| s.routines_for(owner.as_deref(), filter.as_deref()))
        .await?;
    let owner = who.user_id.clone();
    let unseen = m
        .inner
        .mirror
        .call(move |s| s.routine_unseen(owner.as_deref()))
        .await?;
    let mut routines = Vec::new();
    for row in &rows {
        let n = unseen.get(&row.routine_id).copied().unwrap_or(0);
        routines.push(routine_json(&m, row, n).await?);
    }
    let cfg = r::config(&m);
    let why = match &scope {
        _ if !cfg.enabled => Some("Routines are turned off on this server".to_string()),
        Some(a) => r::may_not_own(&who, a),
        None => Some(r::TOKEN_CANNOT_OWN.to_string()),
    };
    let can_agentic = scope
        .as_ref()
        .is_some_and(|a| why.is_none() && ws::agentic_refusal(&who, Some(a)).is_none());
    Ok(Json(json!({
        "routines": routines,
        "workspace_id": ws_id,
        "can_create": why.is_none(),
        "can_agentic": can_agentic,
        "why": why,
        "limits": limits(&cfg),
        "variables": template::namespaces(None),
        // Event sources the viewer can use (the webhook comes later).
        "sources": [],
        "orphaned": why.is_some() && who.user_id.is_some() && !rows.is_empty(),
    })))
}

async fn create(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(raw): Json<Value>,
) -> Result<(StatusCode, Json<Value>)> {
    let who = require(&m, &headers).await?;
    let b: CreateBody = body(raw)?;
    let Some(uid) = who.user_id.clone() else {
        return Err(forbidden(r::TOKEN_CANNOT_OWN).into());
    };
    let cfg = r::config(&m);
    if !cfg.enabled {
        return Err(conflict("routines are turned off on this server").into());
    }
    let a = ws::scope(&m, &who, b.workspace_id.as_deref())
        .await?
        .ok_or_else(not_found)?;
    if let Some(why) = r::may_not_own(&who, &a) {
        return Err(forbidden(why).into());
    }
    let spec = RoutineSpec::parse(b.spec).map_err(bad)?;
    agentic_allowed(&who, &a, &spec)?;
    rspec::check(&m, &a, &spec).await.map_err(bad)?;
    check_prompt(&spec)?;
    let name = rspec::clean_name(&b.name).map_err(bad)?;
    let now = now(&m);
    let zone = default_zone(&m, &who).await;
    let triggers = parse_triggers(&cfg, &zone, &b.triggers, &[], now)?;
    let new = NewRoutine {
        routine_id: harness_core::new_id(),
        workspace_id: a.id().to_string(),
        owner_id: uid.clone(),
        name,
        enabled: b.enabled,
        spec: serde_json::to_string(&spec).expect("spec serializes"),
        triggers: triggers.rows,
    };
    let caps = RoutineCaps {
        per_user: cfg.max_per_user as usize,
        per_workspace: cfg.max_per_workspace as usize,
    };
    let row = m
        .inner
        .mirror
        .call(move |s| s.routine_create(&new, caps, now))
        .await?;
    r::audit(
        &m,
        "routine.created",
        json!({ "routine_id": row.routine_id, "owner": uid, "workspace": row.workspace_id }),
    )
    .await;
    r::changed(&m, &uid);
    if let Some(h) = r::handle(&m) {
        h.wake();
    }
    let routine = routine_json(&m, &row, 0).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "routine": routine, "secrets": {} })),
    ))
}

async fn detail(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let row = visible(&m, &who, &id).await?;
    Ok(Json(one_json(&m, &row).await?))
}

async fn update(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(raw): Json<Value>,
) -> Result<Json<Value>> {
    let who = require(&m, &headers).await?;
    let b: PatchBody = body(raw)?;
    let (row, a) = owned(&m, &who, &id).await?;
    let cfg = r::config(&m);
    let changed_spec = b.spec.is_some();
    let spec = match b.spec {
        Some(v) => RoutineSpec::parse(v).map_err(bad)?,
        None => RoutineSpec::read(&row.spec).unwrap_or_default(),
    };
    agentic_allowed(&who, &a, &spec)?;
    if changed_spec {
        rspec::check(&m, &a, &spec).await.map_err(bad)?;
    }
    check_prompt(&spec)?;
    let name = b
        .name
        .as_deref()
        .map(rspec::clean_name)
        .transpose()
        .map_err(bad)?;
    let now = now(&m);
    let rid = row.routine_id.clone();
    let existing = m.inner.mirror.call(move |s| s.triggers(&rid)).await?;
    // Enabling clears a pause and starts the schedules from now.
    let resume = b.enabled == Some(true) && (!row.enabled || row.paused_reason.is_some());
    let triggers = match &b.triggers {
        Some(list) => {
            let zone = default_zone(&m, &who).await;
            Some(parse_triggers(&cfg, &zone, list, &existing, now)?.rows)
        }
        None if resume => Some(runner::restarted(&existing, now)),
        None => None,
    };
    let patch = RoutinePatch {
        name,
        enabled: b.enabled,
        spec: changed_spec.then(|| serde_json::to_string(&spec).expect("spec serializes")),
        triggers,
        resume,
    };
    let row = m
        .inner
        .mirror
        .call(move |s| s.routine_update(&id, &patch, now))
        .await?
        .ok_or_else(not_found)?;
    r::audit(
        &m,
        "routine.updated",
        json!({ "routine_id": row.routine_id, "owner": row.owner_id }),
    )
    .await;
    r::changed(&m, &row.owner_id);
    if let Some(h) = r::handle(&m) {
        h.wake();
    }
    let routine = one_json(&m, &row).await?;
    Ok(Json(json!({ "routine": routine, "secrets": {} })))
}

async fn remove(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let row = visible(&m, &who, &id).await?;
    if who.user_id.is_none() {
        return Err(forbidden(r::TOKEN_CANNOT_OWN));
    }
    let rid = row.routine_id.clone();
    m.inner.mirror.call(move |s| s.routine_delete(&rid)).await?;
    r::audit(
        &m,
        "routine.deleted",
        json!({ "routine_id": row.routine_id, "owner": row.owner_id, "workspace": row.workspace_id }),
    )
    .await;
    r::changed(&m, &row.owner_id);
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RunBody {
    #[serde(default)]
    event: Option<Value>,
}

async fn run_now(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    raw: Option<Json<Value>>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let who = require(&m, &headers).await?;
    let b: RunBody = match raw {
        Some(Json(v)) if !v.is_null() => body(v)?,
        _ => RunBody::default(),
    };
    let (row, a) = owned(&m, &who, &id).await?;
    if b.event.is_some() {
        return Err(bad("sample events are not available yet"));
    }
    let spec = RoutineSpec::read(&row.spec).unwrap_or_default();
    agentic_allowed(&who, &a, &spec)?;
    if !r::config(&m).enabled {
        return Err(conflict("routines are turned off on this server"));
    }
    // A disabled routine still runs now (to try it); a paused one first
    // needs turning on again, which says the cause was dealt with.
    if let Some(p) = &row.paused_reason {
        return Err(conflict(format!(
            "this routine is paused ({p}); turn it on again first"
        )));
    }
    let answer = match runner::run_now(&m, &row).await {
        runner::Manual::Started(run_id) => json!({ "run_id": run_id }),
        runner::Manual::Skipped(why) => json!({ "skipped": why }),
        runner::Manual::Blocked(why) => return Err(conflict(why)),
        runner::Manual::Failed => {
            return Err(ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "could not start the run".into(),
            ));
        }
    };
    Ok((StatusCode::ACCEPTED, Json(answer)))
}

#[derive(Deserialize)]
struct RunsQuery {
    #[serde(default)]
    before: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn runs(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<RunsQuery>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let row = visible(&m, &who, &id).await?;
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let rid = row.routine_id.clone();
    let page = m
        .inner
        .mirror
        .call(move |s| s.runs_page(&rid, q.before.as_deref(), limit))
        .await?;
    let p = previews(&m, &page).await?;
    let next = (page.len() == limit).then(|| page.last().map(|r| r.run_id.clone()));
    let list: Vec<Value> = page
        .iter()
        .map(|run| run_json(run, run.session_id.as_ref().and_then(|s| p.get(s))))
        .collect();
    Ok(Json(json!({ "runs": list, "next": next.flatten() })))
}

/// One run of a routine the caller may see (404 otherwise).
async fn run_of(
    m: &Master,
    who: &Principal,
    id: &str,
    run_id: &str,
) -> ApiResult<(RoutineRow, RunRow)> {
    let row = visible(m, who, id).await?;
    let rid = run_id.to_string();
    match m.inner.mirror.call(move |s| s.run(&rid)).await? {
        Some(run) if run.routine_id == row.routine_id => Ok((row, run)),
        _ => Err(ApiError(StatusCode::NOT_FOUND, "unknown run".into())),
    }
}

async fn run_detail(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, run_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let (_, run) = run_of(&m, &who, &id, &run_id).await?;
    let p = previews(&m, std::slice::from_ref(&run)).await?;
    let mut v = run_json(&run, run.session_id.as_ref().and_then(|s| p.get(s)));
    v["prompt"] = json!(run.prompt);
    v["event"] = run
        .event
        .as_deref()
        .and_then(|e| serde_json::from_str(e).ok())
        .unwrap_or(Value::Null);
    Ok(Json(v))
}

async fn seen(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, run_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let (row, run) = run_of(&m, &who, &id, &run_id).await?;
    if who.user_id.is_none() {
        return Err(forbidden(r::TOKEN_CANNOT_OWN));
    }
    let rid = row.routine_id.clone();
    let marked = m
        .inner
        .mirror
        .call(move |s| s.mark_run_seen(&rid, &run.run_id))
        .await?;
    if marked {
        r::changed(&m, &row.owner_id);
        let _ = m.inner.live.send(crate::Live::SessionsChanged);
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewBody {
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    routine_id: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    triggers: Option<Vec<TriggerBody>>,
    /// For webhook routines, which come later; ignored until then.
    #[serde(default)]
    #[allow(dead_code)]
    sample_event: Option<Value>,
}

/// What a routine being edited would do: its next runs, about how many
/// chats a day, and its prompt rendered with this moment's values. Problems
/// come back in `errors`, not as a failure, so the editor can show them as
/// the user types.
async fn preview(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(raw): Json<Value>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let b: PreviewBody = body(raw)?;
    let routine = match &b.routine_id {
        Some(id) => Some(visible(&m, &who, id).await?),
        None => None,
    };
    let requested = b
        .workspace_id
        .clone()
        .or_else(|| routine.as_ref().map(|r| r.workspace_id.clone()));
    let scope = ws::scope(&m, &who, requested.as_deref()).await?;
    let cfg = r::config(&m);
    let now = now(&m);
    let zone = default_zone(&m, &who).await;
    let mut errors: Vec<Value> = Vec::new();
    let mut schedules = Vec::new();
    let mut first_zone: Option<String> = None;
    match (&b.triggers, &routine) {
        (Some(list), _) => {
            let existing: Vec<TriggerRow> = Vec::new();
            for (i, t) in list.iter().enumerate() {
                // Checked one by one, so each problem names its trigger.
                let one = [TriggerBody {
                    trigger_id: None,
                    kind: t.kind.clone(),
                    config: t.config.clone(),
                }];
                match parse_triggers(&cfg, &zone, &one, &existing, now) {
                    Ok(mut p) => {
                        first_zone = first_zone.or_else(|| tz_of(&p.rows[0].config));
                        schedules.append(&mut p.schedules);
                    }
                    Err(ApiError(_, msg)) => errors.push(
                        json!({ "offset": null, "message": format!("schedule {}: {msg}", i + 1) }),
                    ),
                }
            }
            let kinds = list.iter().filter(|t| t.kind == "schedule").count();
            if kinds > 0 && kinds < list.len() {
                errors.push(json!({ "offset": null,
                    "message": "A routine runs on a schedule or on one event, not both" }));
            }
            if kinds > cfg.max_triggers as usize {
                errors.push(json!({ "offset": null,
                    "message": format!("a routine has at most {} schedules", cfg.max_triggers) }));
            }
        }
        (None, Some(r)) => {
            let rid = r.routine_id.clone();
            for t in m.inner.mirror.call(move |s| s.triggers(&rid)).await? {
                let cfg = ScheduleConfig::read(&t.config);
                first_zone = first_zone.or_else(|| tz_of(&t.config));
                if let Some(s) = cfg.and_then(|c| c.schedule().ok()) {
                    schedules.push(s);
                }
            }
        }
        (None, None) => {}
    }
    let next_runs = schedule::next_runs(&schedules, now, 5);
    let chats_per_day = (!schedules.is_empty()).then(|| schedule::chats_per_day(&schedules, now));
    let stored = routine.as_ref().and_then(|r| RoutineSpec::read(&r.spec));
    let prompt = b
        .prompt
        .clone()
        .or_else(|| stored.as_ref().map(|s| s.prompt.clone()))
        .unwrap_or_default();
    let mut warnings: Vec<String> = Vec::new();
    let mut rendered = None;
    match template::parse(&prompt).and_then(|t| t.check(None).map(|_| t)) {
        Err(e) => errors.push(json!({ "offset": e.offset, "message": e.message })),
        Ok(t) => {
            let on_missing = stored.as_ref().map(|s| s.on_missing).unwrap_or_default();
            let vars = sample_vars(
                &m,
                &who,
                scope.as_ref(),
                routine.as_ref(),
                first_zone.as_deref().unwrap_or(&zone),
                next_runs.first().copied(),
                now,
            )
            .await?;
            match t.render(&vars, on_missing) {
                Ok(r) => {
                    warnings = r.warnings;
                    rendered = Some(r.text);
                }
                Err(e) => errors.push(json!({ "offset": null, "message": e.to_string() })),
            }
        }
    }
    Ok(Json(json!({
        "next_runs": next_runs,
        "chats_per_day": chats_per_day,
        "rendered": rendered,
        "untrusted": false,
        "warnings": warnings,
        "errors": errors,
        "event_paths": [],
    })))
}

fn tz_of(config: &str) -> Option<String> {
    ScheduleConfig::read(config).and_then(|c| c.tz().map(str::to_string))
}

/// The values a preview renders with: this moment, and the routine's own
/// (or a new routine's) names and last run.
async fn sample_vars(
    m: &Master,
    who: &Principal,
    scope: Option<&Access>,
    routine: Option<&RoutineRow>,
    zone: &str,
    next_run: Option<i64>,
    now: i64,
) -> ApiResult<Vars> {
    let tz = jiff::tz::TimeZone::get(zone).unwrap_or(jiff::tz::TimeZone::UTC);
    let zoned = |ms: i64| {
        jiff::Timestamp::from_millisecond(ms)
            .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
            .to_zoned(tz.clone())
    };
    let (number, last) = match routine {
        Some(r) => {
            let rid = r.routine_id.clone();
            m.inner
                .mirror
                .call(move |s| {
                    let n = s.run_count(&rid)?;
                    let last = s.runs_page(&rid, None, 1)?.into_iter().next();
                    Ok::<_, harness_store::StoreError>((n + 1, last))
                })
                .await?
        }
        None => (1, None),
    };
    let prefs = crate::prefs::Prefs::load(m, who.user_id.as_deref()).await;
    let display = prefs.display_name.as_deref().map(str::trim).unwrap_or("");
    Ok(Vars {
        now: zoned(now),
        routine_name: routine.map_or_else(|| "New routine".into(), |r| r.name.clone()),
        routine_id: routine.map(|r| r.routine_id.clone()).unwrap_or_default(),
        run_trigger: if next_run.is_some() {
            "schedule"
        } else {
            "manual"
        }
        .into(),
        run_number: number,
        run_scheduled: next_run.map(zoned),
        user_name: if display.is_empty() {
            who.username.clone()
        } else {
            display.to_string()
        },
        workspace_name: scope.map(|a| a.workspace.name.clone()).unwrap_or_default(),
        last_run: last.map(|l| (l.status, zoned(l.started_ms))),
        event: None,
    })
}
