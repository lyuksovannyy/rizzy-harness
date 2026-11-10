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
use crate::routines::events::{self, Event, EventSource};
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
        .route(
            "/api/routines/{id}/triggers/{tid}/secret",
            post(rotate_secret),
        )
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
/// Its node must be one they may use, too (the embedded node is for admins
/// by default).
fn agentic_allowed(m: &Master, who: &Principal, a: &Access, spec: &RoutineSpec) -> ApiResult<()> {
    match spec
        .agentic()
        .then(|| {
            ws::agentic_refusal(who, Some(a)).or_else(|| {
                spec.node_id
                    .as_deref()
                    .and_then(|n| ws::node_refusal(m, who, n))
            })
        })
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
    /// The event source of its one event trigger (the webhook), if any:
    /// what `event.*` the prompt may use.
    source: Option<&'static dyn EventSource>,
}

/// The event source of a stored trigger set.
fn source_of(rows: &[TriggerRow]) -> Option<&'static dyn EventSource> {
    rows.iter().find_map(|t| events::source(&t.kind))
}

/// Check a trigger set: 1 to `max_triggers` schedules, or exactly one
/// event (the webhook). Ids the routine already has keep their trigger
/// (a webhook its URL and secret); a trigger never changes kind.
fn parse_triggers(
    cfg: &RoutinesConfig,
    zone: &str,
    list: &[TriggerBody],
    existing: &[TriggerRow],
    now: i64,
) -> ApiResult<Triggers> {
    if let Some(t) = list
        .iter()
        .find(|t| t.kind != "schedule" && events::source(&t.kind).is_none())
    {
        return Err(bad(format!("unknown trigger kind {:?}", t.kind)));
    }
    check_set(cfg, list)?;
    let mut out = Triggers {
        rows: Vec::new(),
        schedules: Vec::new(),
        source: None,
    };
    for t in list {
        let trigger_id = trigger_id(t, existing, &out.rows)?;
        let (config, next_run_ms) = match events::source(&t.kind) {
            Some(src) => {
                out.source = Some(src);
                (src.check_config(&t.config).map_err(bad)?.to_string(), None)
            }
            None => {
                let config = ScheduleConfig::from_request(&t.config, zone, now).map_err(bad)?;
                let schedule = config.compile(cfg.min_interval_minutes, now).map_err(bad)?;
                let next = schedule.next_after_ms(now);
                out.schedules.push(schedule);
                let config = serde_json::to_string(&config).expect("config serializes");
                (config, next)
            }
        };
        out.rows.push(NewTrigger {
            trigger_id,
            kind: t.kind.clone(),
            config,
            next_run_ms,
            secret_hash: None,
        });
    }
    Ok(out)
}

/// What only a whole trigger set can break: none at all, schedules mixed
/// with an event, more than one event, too many schedules, or webhooks
/// turned off on this server.
fn check_set(cfg: &RoutinesConfig, list: &[TriggerBody]) -> ApiResult<()> {
    let schedules = list.iter().filter(|t| t.kind == "schedule").count();
    let fired = list
        .iter()
        .filter(|t| events::source(&t.kind).is_some())
        .count();
    if schedules > 0 && fired > 0 {
        return Err(bad(
            "A routine runs on a schedule or on one event, not both",
        ));
    }
    if list.is_empty() {
        return Err(bad(
            "add a schedule or a webhook: a routine runs on 1 to 8 schedules or on one webhook",
        ));
    }
    if fired > 1 {
        return Err(bad("a routine runs on one webhook at most"));
    }
    if fired == 1 && !cfg.webhooks {
        return Err(conflict("webhooks are turned off on this server"));
    }
    if schedules > cfg.max_triggers as usize {
        return Err(conflict(format!(
            "a routine has at most {} schedules",
            cfg.max_triggers
        )));
    }
    Ok(())
}

/// The id `t` is saved under: its own, if the routine has it, is not listed
/// twice and keeps its kind; else a new one.
fn trigger_id(t: &TriggerBody, existing: &[TriggerRow], taken: &[NewTrigger]) -> ApiResult<String> {
    let Some(id) = &t.trigger_id else {
        return Ok(auth::random_hex(16));
    };
    match existing.iter().find(|e| &e.trigger_id == id) {
        None => Err(bad(format!("unknown trigger {id:?}"))),
        Some(_) if taken.iter().any(|r| &r.trigger_id == id) => {
            Err(bad(format!("trigger {id:?} is listed twice")))
        }
        Some(e) if e.kind != t.kind => Err(bad(format!(
            "trigger {id:?} is a {}; remove it and add a new one",
            e.kind
        ))),
        Some(_) => Ok(id.clone()),
    }
}

/// Give each new webhook trigger in `rows` a secret: stored as its SHA-256,
/// returned once by trigger id. A trigger the routine already has keeps its
/// secret (rotation is its own call).
fn new_secrets(rows: &mut [NewTrigger], existing: &[TriggerRow]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for t in rows.iter_mut() {
        let new = !existing.iter().any(|e| e.trigger_id == t.trigger_id);
        if new && events::source(&t.kind).is_some() {
            let secret = auth::random_hex(32);
            t.secret_hash = Some(auth::sha256_hex(&secret));
            out.insert(t.trigger_id.clone(), secret);
        }
    }
    out
}

/// Refuse (403) an event source `who` may not use in `a`.
fn source_allowed(
    who: &Principal,
    a: &Access,
    source: Option<&'static dyn EventSource>,
) -> ApiResult<()> {
    match source.map(|s| s.may_use(who, a)) {
        Some(Err(why)) => Err(forbidden(why)),
        _ => Ok(()),
    }
}

/// The prompt checked against the namespaces this routine has: `event.*`
/// only with an event source, and then only that source's.
fn check_prompt(spec: &RoutineSpec, source: Option<&dyn EventSource>) -> Result<()> {
    let t = template::parse(&spec.prompt).map_err(Error::At)?;
    t.check(source).map_err(Error::At)
}

/// What the variable picker offers from `src`.
fn source_json(src: &dyn EventSource) -> Value {
    let vars: Vec<Value> = src
        .vars()
        .iter()
        .map(|v| json!({ "path": v.path, "example": v.example }))
        .collect();
    json!({ "kind": src.kind(), "vars": vars, "free_prefix": src.free_prefix() })
}

/// A trigger as the UI shows it; a webhook also says where it is called
/// and whether it has a secret (never the secret).
fn trigger_json(t: &TriggerRow) -> Value {
    let mut v = json!({
        "trigger_id": t.trigger_id,
        "kind": t.kind,
        "config": serde_json::from_str::<Value>(&t.config).unwrap_or(Value::Null),
        "next_run_ms": t.next_run_ms,
        "last_fired_ms": t.last_fired_ms,
    });
    if events::source(&t.kind).is_some() {
        v["url_path"] = json!(format!("/api/hooks/{}", t.trigger_id));
        v["secret_set"] = json!(t.secret_hash.is_some());
    }
    v
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
/// may not run it now. `event_paths` (the paths in the last event a run
/// kept) are for its `owner` only.
async fn routine_json(m: &Master, row: &RoutineRow, unseen: i64, owner: bool) -> ApiResult<Value> {
    let id = row.routine_id.clone();
    let (triggers, last, event) = m
        .inner
        .mirror
        .call(move |s| {
            let triggers = s.triggers(&id)?;
            let last = s.runs_page(&id, None, 1)?.into_iter().next();
            let event = if owner { s.latest_event(&id)? } else { None };
            Ok::<_, harness_store::StoreError>((triggers, last, event))
        })
        .await?;
    let event_paths = event
        .and_then(|e| serde_json::from_str::<Value>(&e).ok())
        .map(|e| events::paths(&e))
        .unwrap_or_default();
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
        "event_paths": event_paths,
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

async fn one_json(m: &Master, who: &Principal, row: &RoutineRow) -> ApiResult<Value> {
    let unseen = unseen_of(m, &row.owner_id).await?;
    let n = unseen.get(&row.routine_id).copied().unwrap_or(0);
    let owner = who.user_id.as_deref() == Some(row.owner_id.as_str());
    routine_json(m, row, n, owner).await
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
    // A user lists only their own routines.
    let owner = who.user_id.is_some();
    for row in &rows {
        let n = unseen.get(&row.routine_id).copied().unwrap_or(0);
        routines.push(routine_json(&m, row, n, owner).await?);
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
    let sources: Vec<Value> = if cfg.enabled && cfg.webhooks {
        vec![source_json(&events::Webhook)]
    } else {
        Vec::new()
    };
    Ok(Json(json!({
        "routines": routines,
        "workspace_id": ws_id,
        "can_create": why.is_none(),
        "can_agentic": can_agentic,
        "why": why,
        "limits": limits(&cfg),
        "variables": template::namespaces(None),
        "sources": sources,
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
    agentic_allowed(&m, &who, &a, &spec)?;
    rspec::check(&m, &a, &spec).await.map_err(bad)?;
    let name = rspec::clean_name(&b.name).map_err(bad)?;
    let now = now(&m);
    let zone = default_zone(&m, &who).await;
    let mut triggers = parse_triggers(&cfg, &zone, &b.triggers, &[], now)?;
    source_allowed(&who, &a, triggers.source)?;
    check_prompt(&spec, triggers.source)?;
    let secrets = new_secrets(&mut triggers.rows, &[]);
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
    let routine = routine_json(&m, &row, 0, true).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "routine": routine, "secrets": secrets })),
    ))
}

async fn detail(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let row = visible(&m, &who, &id).await?;
    Ok(Json(one_json(&m, &who, &row).await?))
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
    agentic_allowed(&m, &who, &a, &spec)?;
    if changed_spec {
        rspec::check(&m, &a, &spec).await.map_err(bad)?;
    }
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
    let mut secrets = HashMap::new();
    let (triggers, source) = match &b.triggers {
        Some(list) => {
            let zone = default_zone(&m, &who).await;
            let mut t = parse_triggers(&cfg, &zone, list, &existing, now)?;
            source_allowed(&who, &a, t.source)?;
            secrets = new_secrets(&mut t.rows, &existing);
            (Some(t.rows), t.source)
        }
        None if resume => (
            Some(runner::restarted(&existing, now)),
            source_of(&existing),
        ),
        None => (None, source_of(&existing)),
    };
    check_prompt(&spec, source)?;
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
    let routine = one_json(&m, &who, &row).await?;
    Ok(Json(json!({ "routine": routine, "secrets": secrets })))
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
    let sample = match b.event {
        Some(v) => Some(sample_event(&m, &row, v).await?),
        None => None,
    };
    let spec = RoutineSpec::read(&row.spec).unwrap_or_default();
    agentic_allowed(&m, &who, &a, &spec)?;
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
    let answer = match runner::run_now(&m, &row, sample).await {
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

/// The routine's event source, from its stored triggers.
async fn stored_source(
    m: &Master,
    row: &RoutineRow,
) -> ApiResult<Option<&'static dyn EventSource>> {
    let rid = row.routine_id.clone();
    let triggers = m.inner.mirror.call(move |s| s.triggers(&rid)).await?;
    Ok(source_of(&triggers))
}

/// A sample event in an event's place, checked like a webhook body: at
/// most 64 KiB of JSON, nested at most 32 deep. Returns it as JSON text.
fn check_sample(sample: &Value) -> ApiResult<String> {
    let text = sample.to_string();
    if text.len() > events::MAX_BODY {
        return Err(bad("a sample event has at most 64 KiB"));
    }
    if events::depth(sample) > events::MAX_DEPTH {
        return Err(bad(format!(
            "a sample event is nested at most {} deep",
            events::MAX_DEPTH
        )));
    }
    Ok(text)
}

/// Run now's sample event for routine `row`, which must be one an event
/// starts.
async fn sample_event(m: &Master, row: &RoutineRow, sample: Value) -> ApiResult<Event> {
    let Some(src) = stored_source(m, row).await? else {
        return Err(bad("a sample event is for routines a webhook starts"));
    };
    let text = check_sample(&sample)?;
    let received_at = runner::local_now(m, &row.owner_id).await;
    Ok(Event {
        kind: src.kind(),
        data: Event::sample(sample, received_at),
        dedupe_key: None,
        size: text.len(),
        sha256: auth::sha256_hex(&text),
    })
}

/// A new secret for webhook trigger `tid`, shown once; the old one stops
/// working at once.
async fn rotate_secret(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, tid)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let (row, a) = owned(&m, &who, &id).await?;
    agentic_allowed(
        &m,
        &who,
        &a,
        &RoutineSpec::read(&row.spec).unwrap_or_default(),
    )?;
    let t = tid.clone();
    let trigger = m.inner.mirror.call(move |s| s.trigger(&t)).await?;
    let Some(trigger) = trigger.filter(|t| t.routine_id == row.routine_id) else {
        return Err(ApiError(StatusCode::NOT_FOUND, "unknown trigger".into()));
    };
    if events::source(&trigger.kind).is_none() {
        return Err(bad("only a webhook trigger has a secret"));
    }
    let secret = auth::random_hex(32);
    let hash = auth::sha256_hex(&secret);
    m.inner
        .mirror
        .call(move |s| s.trigger_set_secret(&tid, &hash))
        .await?;
    r::audit(
        &m,
        "hook.secret_rotated",
        json!({ "routine_id": row.routine_id, "trigger_id": trigger.trigger_id, "owner": row.owner_id }),
    )
    .await;
    r::changed(&m, &row.owner_id);
    Ok(Json(json!({ "secret": secret })))
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
    /// What a webhook call might bring (`Event::sample`).
    #[serde(default)]
    sample_event: Option<Value>,
}

/// One problem for the editor, at `offset` in the prompt or about the
/// routine as a whole.
fn problem(offset: Option<usize>, message: impl Into<String>) -> Value {
    json!({ "offset": offset, "message": message.into() })
}

/// The triggers a preview is about: the ones being edited, else the
/// routine's own. Problems are collected, not returned.
struct Previewed {
    schedules: Vec<Schedule>,
    source: Option<&'static dyn EventSource>,
    /// The first schedule's zone, for `now.*`.
    zone: Option<String>,
}

async fn previewed_triggers(
    m: &Master,
    cfg: &RoutinesConfig,
    zone: &str,
    b: &PreviewBody,
    routine: Option<&RoutineRow>,
    now: i64,
    errors: &mut Vec<Value>,
) -> ApiResult<Previewed> {
    let mut out = Previewed {
        schedules: Vec::new(),
        source: None,
        zone: None,
    };
    if let Some(list) = &b.triggers {
        for (i, t) in list.iter().enumerate() {
            // Checked one by one, so each problem names its trigger.
            let one = [TriggerBody {
                trigger_id: None,
                kind: t.kind.clone(),
                config: t.config.clone(),
            }];
            match parse_triggers(cfg, zone, &one, &[], now) {
                Ok(mut p) => {
                    out.zone = out.zone.or_else(|| tz_of(&p.rows[0].config));
                    out.source = out.source.or(p.source);
                    out.schedules.append(&mut p.schedules);
                }
                Err(ApiError(_, msg)) => {
                    errors.push(problem(None, format!("{} {}: {msg}", t.kind, i + 1)))
                }
            }
        }
        if list.len() > 1
            && let Err(ApiError(_, msg)) = check_set(cfg, list)
        {
            errors.push(problem(None, msg));
        }
        return Ok(out);
    }
    if let Some(r) = routine {
        let rid = r.routine_id.clone();
        let rows = m.inner.mirror.call(move |s| s.triggers(&rid)).await?;
        out.source = source_of(&rows);
        for t in &rows {
            out.zone = out.zone.clone().or_else(|| tz_of(&t.config));
            if let Some(s) = ScheduleConfig::read(&t.config).and_then(|c| c.schedule().ok()) {
                out.schedules.push(s);
            }
        }
    }
    Ok(out)
}

/// What a routine being edited would do: its next runs, about how many
/// chats a day, and its prompt rendered with this moment's values (and a
/// sample event's, for a webhook routine). Problems come back in `errors`,
/// not as a failure, so the editor can show them as the user types.
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
    let p = previewed_triggers(&m, &cfg, &zone, &b, routine.as_ref(), now, &mut errors).await?;
    let next_runs = schedule::next_runs(&p.schedules, now, 5);
    let chats_per_day =
        (!p.schedules.is_empty()).then(|| schedule::chats_per_day(&p.schedules, now));
    let event = match (&b.sample_event, p.source) {
        (Some(sample), Some(_)) => match check_sample(sample) {
            Ok(_) => Some(Event::sample(
                sample.clone(),
                local(now, p.zone.as_deref().unwrap_or(&zone)),
            )),
            Err(ApiError(_, msg)) => {
                errors.push(problem(None, msg));
                None
            }
        },
        (Some(_), None) => {
            errors.push(problem(
                None,
                "a sample event is for routines a webhook starts",
            ));
            None
        }
        (None, _) => None,
    };
    let event_paths = event.as_ref().map(events::paths).unwrap_or_default();
    let stored = routine.as_ref().and_then(|r| RoutineSpec::read(&r.spec));
    let prompt = b
        .prompt
        .clone()
        .or_else(|| stored.as_ref().map(|s| s.prompt.clone()))
        .unwrap_or_default();
    let mut warnings: Vec<String> = Vec::new();
    let mut rendered = None;
    let mut untrusted = false;
    match template::parse(&prompt).and_then(|t| t.check(p.source).map(|_| t)) {
        Err(e) => errors.push(problem(Some(e.offset), e.message)),
        Ok(t) => {
            let on_missing = stored.as_ref().map(|s| s.on_missing).unwrap_or_default();
            let ctx = SampleCtx {
                scope: scope.as_ref(),
                routine: routine.as_ref(),
                zone: p.zone.as_deref().unwrap_or(&zone),
                next_run: next_runs.first().copied(),
                fired_by: p.source.map(|s| s.kind()),
                event,
            };
            let vars = sample_vars(&m, &who, ctx, now).await?;
            match t.render(&vars, on_missing) {
                Ok(r) => {
                    warnings = r.warnings;
                    untrusted = r.untrusted;
                    rendered = Some(r.text);
                }
                Err(e) => errors.push(problem(None, e.to_string())),
            }
        }
    }
    Ok(Json(json!({
        "next_runs": next_runs,
        "chats_per_day": chats_per_day,
        "rendered": rendered,
        "untrusted": untrusted,
        "warnings": warnings,
        "errors": errors,
        "event_paths": event_paths,
    })))
}

fn tz_of(config: &str) -> Option<String> {
    ScheduleConfig::read(config).and_then(|c| c.tz().map(str::to_string))
}

/// "YYYY-MM-DD HH:MM" of `ms` in `zone` (UTC if unknown).
fn local(ms: i64, zone: &str) -> String {
    let tz = jiff::tz::TimeZone::get(zone).unwrap_or(jiff::tz::TimeZone::UTC);
    jiff::Timestamp::from_millisecond(ms)
        .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
        .to_zoned(tz)
        .strftime("%Y-%m-%d %H:%M")
        .to_string()
}

/// What a preview renders with besides this moment.
struct SampleCtx<'a> {
    scope: Option<&'a Access>,
    routine: Option<&'a RoutineRow>,
    zone: &'a str,
    /// The next scheduled run, if the routine has schedules.
    next_run: Option<i64>,
    /// The kind of its event source, if an event starts it.
    fired_by: Option<&'static str>,
    event: Option<Value>,
}

/// The values a preview renders with: this moment, the routine's own (or
/// a new routine's) names and last run, and the sample event.
async fn sample_vars(m: &Master, who: &Principal, c: SampleCtx<'_>, now: i64) -> ApiResult<Vars> {
    let tz = jiff::tz::TimeZone::get(c.zone).unwrap_or(jiff::tz::TimeZone::UTC);
    let zoned = |ms: i64| {
        jiff::Timestamp::from_millisecond(ms)
            .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
            .to_zoned(tz.clone())
    };
    let (number, last) = match c.routine {
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
    let trigger = match (c.fired_by, c.next_run) {
        (Some(kind), _) => kind,
        (None, Some(_)) => "schedule",
        (None, None) => "manual",
    };
    Ok(Vars {
        now: zoned(now),
        routine_name: c
            .routine
            .map_or_else(|| "New routine".into(), |r| r.name.clone()),
        routine_id: c.routine.map(|r| r.routine_id.clone()).unwrap_or_default(),
        run_trigger: trigger.into(),
        run_number: number,
        run_scheduled: c.next_run.map(zoned),
        user_name: if display.is_empty() {
            who.username.clone()
        } else {
            display.to_string()
        },
        workspace_name: c
            .scope
            .map(|a| a.workspace.name.clone())
            .unwrap_or_default(),
        last_run: last.map(|l| (l.status, zoned(l.started_ms))),
        event: c.event,
    })
}
