//! A person's memory over HTTP (README, Memory): their facts in both places
//! (their own, and those kept in one of their workspaces), moving one
//! between them, and what one `remember` call of their chat did, with undo.
//! Every route reaches only the caller's own facts and chats: another
//! member, a workspace admin and a server admin get 404 (PLAN.md T49).

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use harness_memory::{Fact, MemoryStore, NewFact, Status};
use harness_store::{Membership, SessionRow};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, require, visible_session};
use crate::auth::Principal;
use crate::memory::{is_own, kept_in, member_owner};
use crate::{Live, Master};

type Store = std::sync::Arc<dyn MemoryStore>;

pub(super) fn routes() -> Router<Master> {
    Router::new()
        .route("/api/memory", get(list).post(add))
        .route(
            "/api/memory/{id}",
            axum::routing::patch(update).delete(remove),
        )
        .route("/api/sessions/{id}/memory/{call_id}", get(call_state))
        .route("/api/sessions/{id}/memory/{call_id}/undo", post(undo))
}

pub(super) fn memory_store(m: &Master) -> ApiResult<Store> {
    m.memory().ok_or_else(|| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "memory is not set up on this server".into(),
        )
    })
}

pub(super) fn memory_err(e: harness_memory::MemoryError) -> ApiError {
    match e {
        harness_memory::MemoryError::Invalid(msg) => ApiError(StatusCode::BAD_REQUEST, msg),
        other => ApiError(StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
    }
}

fn not_found() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "no such memory".into())
}

async fn memberships(m: &Master, who: &Principal) -> ApiResult<Vec<Membership>> {
    let Some(uid) = who.user_id.clone() else {
        return Ok(Vec::new());
    };
    Ok(m.inner.mirror.call(move |s| s.memberships(&uid)).await?)
}

/// A fact as the Memory page shows it: where it lives too.
fn fact_json(f: &Fact, ms: &[Membership]) -> Value {
    let mut v = json!(f);
    match kept_in(f) {
        Some(ws) => {
            let w = ms.iter().find(|x| x.workspace.workspace_id == ws);
            v["place"] = json!("workspace");
            v["workspace_id"] = json!(ws);
            v["workspace_name"] = json!(w.map(|x| x.workspace.name.clone()));
            v["workspace_personal"] = json!(w.is_some_and(|x| x.workspace.personal()));
        }
        None => v["place"] = json!("user"),
    }
    v
}

/// Every fact of the caller: their own, then those kept in each workspace
/// they are in; newest first.
async fn list(State(m): State<Master>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let mem = memory_store(&m)?;
    let ms = memberships(&m, &who).await?;
    let mut facts = mem.list(who.user_id.as_deref()).await.map_err(memory_err)?;
    if let Some(uid) = &who.user_id {
        for w in &ms {
            let key = member_owner(&w.workspace.workspace_id, uid);
            facts.extend(mem.list(Some(&key)).await.map_err(memory_err)?);
        }
    }
    facts.sort_by_key(|f| std::cmp::Reverse(f.updated_ms));
    let out: Vec<Value> = facts.iter().map(|f| fact_json(f, &ms)).collect();
    Ok(Json(json!({ "memories": out, "backend": mem.name() })))
}

/// The owner key for a fact of the caller kept in `workspace` (one they
/// are in), or their own facts' key for `None`.
async fn owner_key(m: &Master, who: &Principal, workspace: Option<&str>) -> ApiResult<String> {
    let Some(uid) = who.user_id.clone() else {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "the API token has no workspaces".into(),
        ));
    };
    match workspace {
        None => Ok(uid),
        Some(ws) => {
            let ms = memberships(m, who).await?;
            if !ms.iter().any(|x| x.workspace.workspace_id == ws) {
                return Err(ApiError(StatusCode::NOT_FOUND, "no such workspace".into()));
            }
            Ok(member_owner(ws, &uid))
        }
    }
}

#[derive(Deserialize)]
struct NewMemoryBody {
    text: String,
    #[serde(default)]
    kind: harness_memory::FactKind,
    /// Keep it in this workspace (one the caller is in) instead of their
    /// own facts.
    #[serde(default)]
    workspace_id: Option<String>,
}

/// Added by the person: active at once.
async fn add(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<NewMemoryBody>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let mem = memory_store(&m)?;
    let owner = match (&who.user_id, b.workspace_id.as_deref()) {
        (None, None) => None,
        (_, ws) => Some(owner_key(&m, &who, ws).await?),
    };
    let f = mem
        .add(NewFact {
            owner_id: owner,
            text: b.text,
            kind: b.kind,
            status: Status::Active,
            source_session: None,
            dedupe_key: None,
        })
        .await
        .map_err(memory_err)?;
    let _ = m.inner.live.send(Live::MemoryChanged);
    let ms = memberships(&m, &who).await?;
    Ok(Json(f.map(|f| fact_json(&f, &ms)).unwrap_or(Value::Null)))
}

/// The fact, if it is the caller's (404 otherwise, never 403).
async fn own_fact(m: &Master, who: &Principal, id: &str) -> ApiResult<(Store, Fact)> {
    let mem = memory_store(m)?;
    match mem.get(id).await.map_err(memory_err)? {
        Some(f)
            if is_own(who.user_id.as_deref(), &f)
                || (who.user_id.is_none() && f.owner_id.is_none()) =>
        {
            Ok((mem, f))
        }
        _ => Err(not_found()),
    }
}

#[derive(Deserialize)]
struct MemoryPatch {
    #[serde(default)]
    text: Option<String>,
    /// `active` confirms a proposal or restores a retired fact.
    #[serde(default)]
    status: Option<Status>,
    /// `user` (their own facts) or `workspace` (with `workspace_id`): moves it.
    #[serde(default)]
    place: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
}

async fn update(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<MemoryPatch>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let (mem, cur) = own_fact(&m, &who, &id).await?;
    let mut f = mem
        .update(&id, b.text, b.status)
        .await
        .map_err(memory_err)?
        .ok_or_else(not_found)?;
    let to = match b.place.as_deref() {
        None => None,
        Some("user") => Some(owner_key(&m, &who, None).await?),
        Some("workspace") => {
            let ws = b.workspace_id.as_deref().ok_or_else(|| {
                ApiError(
                    StatusCode::BAD_REQUEST,
                    "place workspace needs workspace_id".into(),
                )
            })?;
            Some(owner_key(&m, &who, Some(ws)).await?)
        }
        Some(_) => {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "place must be user or workspace".into(),
            ));
        }
    };
    // A move is a copy in the new place and the old one deleted: the
    // back ends key facts by owner and none moves one.
    if let Some(owner) = to.filter(|o| cur.owner_id.as_deref() != Some(o.as_str())) {
        let moved = mem
            .add(NewFact {
                owner_id: Some(owner),
                text: f.text.clone(),
                kind: f.kind,
                status: f.status,
                source_session: f.source_session,
                dedupe_key: None,
            })
            .await
            .map_err(memory_err)?
            .ok_or_else(not_found)?;
        mem.delete(&id).await.map_err(memory_err)?;
        f = moved;
    }
    let _ = m.inner.live.send(Live::MemoryChanged);
    let ms = memberships(&m, &who).await?;
    Ok(Json(fact_json(&f, &ms)))
}

async fn remove(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let (mem, _) = own_fact(&m, &who, &id).await?;
    mem.delete(&id).await.map_err(memory_err)?;
    let _ = m.inner.live.send(Live::MemoryChanged);
    Ok(Json(json!({ "ok": true })))
}

/// What one `remember` call did, read from the log and the store.
struct CallFacts {
    input: Value,
    /// The fact it added, while it is still there.
    added: Option<Fact>,
    /// The fact it replaced or forgot, while it is still retired.
    retired: Option<Fact>,
}

/// The call `call_id` of the caller's own chat `sid` (404 for anyone
/// else's chat, admins included, and for any other tool's call).
async fn call_facts(
    m: &Master,
    who: &Principal,
    sid: &str,
    call_id: &str,
) -> ApiResult<(Store, SessionRow, CallFacts)> {
    let row = visible_session(m, who, sid).await?;
    let mem = memory_store(m)?;
    let none = || ApiError(StatusCode::NOT_FOUND, "no such call".into());
    if who.user_id.is_none() || row.owner_id != who.user_id {
        return Err(none());
    }
    let (s, c) = (row.session_id, call_id.to_string());
    let (tool, input) = m
        .inner
        .mirror
        .call(move |st| st.tool_call(s, &c))
        .await?
        .ok_or_else(none)?;
    if tool != "remember" {
        return Err(none());
    }
    let key = format!("{}:{call_id}", row.session_id);
    let uid = who.user_id.as_deref().unwrap_or_default();
    let mut owners = vec![uid.to_string()];
    if let Some(ws) = &row.workspace_id {
        owners.push(member_owner(ws, uid));
    }
    let mut added = None;
    for o in &owners {
        if let Some(f) = mem.by_key(Some(o), &key).await.map_err(memory_err)? {
            added = Some(f);
            break;
        }
    }
    let target = input
        .get("replaces")
        .or_else(|| input.get("forget"))
        .and_then(|v| v.as_str());
    let retired = match target {
        Some(id) => mem
            .get(id)
            .await
            .map_err(memory_err)?
            .filter(|f| f.status == Status::Retired && is_own(Some(uid), f)),
        None => None,
    };
    Ok((
        mem,
        row,
        CallFacts {
            input,
            added,
            retired,
        },
    ))
}

fn state_json(m: &Master, row: &SessionRow, call_id: &str, c: &CallFacts) -> Value {
    if c.input.get("scope").and_then(|v| v.as_str()) == Some("project") {
        return json!({ "state": "project" });
    }
    let refusal = m.inner.refusals.get(row.session_id, call_id);
    let forget = c.input.get("forget").is_some() && c.input.get("text").is_none();
    let state = match (&c.added, &refusal) {
        (Some(f), _) => match f.status {
            Status::Active => "saved",
            Status::Proposed => "proposed",
            Status::Retired => "retired",
        },
        (None, Some(_)) => "refused",
        (None, None) if forget && c.retired.is_some() => "forgot",
        (None, None) => "none",
    };
    let mut v = json!({
        "state": state,
        "fact": c.added,
        "retired": c.retired,
        "reason": refusal.map(|r| r.why),
    });
    if let Some(f) = &c.added {
        v["place"] = json!(if kept_in(f).is_some() {
            "workspace"
        } else {
            "user"
        });
    }
    v
}

async fn call_state(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, call_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let (_, row, c) = call_facts(&m, &who, &id, &call_id).await?;
    Ok(Json(state_json(&m, &row, &call_id, &c)))
}

/// Undo what the call did: delete the fact it saved, restore the one it
/// replaced or forgot.
async fn undo(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, call_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let (mem, row, mut c) = call_facts(&m, &who, &id, &call_id).await?;
    if let Some(f) = c.added.take() {
        mem.delete(&f.id).await.map_err(memory_err)?;
    }
    if let Some(f) = c.retired.take() {
        mem.update(&f.id, None, Some(Status::Active))
            .await
            .map_err(memory_err)?;
    }
    let _ = m.inner.live.send(Live::MemoryChanged);
    let mut v = state_json(&m, &row, &call_id, &c);
    v["state"] = json!("undone");
    Ok(Json(v))
}
