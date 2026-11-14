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
        .route("/api/sessions/{id}/memory/{call_id}/keep", post(keep))
        .route(
            "/api/sessions/{id}/memory/{call_id}/dismiss",
            post(dismiss),
        )
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

/// The caller's own chat `sid` and its `remember` call `call_id` (404 for
/// anyone else's chat, admins and the API token included, and for any
/// other tool's call). Returns the chat and the call's input.
async fn own_call(
    m: &Master,
    who: &Principal,
    sid: &str,
    call_id: &str,
) -> ApiResult<(SessionRow, Value)> {
    let row = visible_session(m, who, sid).await?;
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
    Ok((row, input))
}

/// What the chat line shows for a call: its recorded state and the facts
/// it names as they are now. `waiting` when nothing is recorded yet (the
/// master files a call when the node reports it; the line asks again).
async fn state_json(m: &Master, who: &Principal, row: &SessionRow, call_id: &str, input: &Value) -> ApiResult<Value> {
    if input.get("scope").and_then(|v| v.as_str()) == Some("project") {
        return Ok(json!({ "state": "project" }));
    }
    let Some(r) = crate::memory::call_record(m, row.session_id, call_id).await else {
        return Ok(json!({ "state": "waiting" }));
    };
    let mem = memory_store(m)?;
    let uid = who.user_id.as_deref();
    let get = async |id: &Option<String>| -> ApiResult<Option<Fact>> {
        Ok(match id {
            Some(id) => mem.get(id).await.map_err(memory_err)?.filter(|f| is_own(uid, f)),
            None => None,
        })
    };
    let fact = get(&r.added).await?;
    let target = get(&r.target).await?;
    use crate::memory::CallState as S;
    let state = match r.state {
        S::Saved | S::Kept if r.added.is_some() && fact.is_none() => "gone",
        S::Saved if r.added.is_none() => "forgot",
        S::Saved => "saved",
        S::Kept => "kept",
        // Kept or discarded on the Memory page meanwhile.
        S::Pending if r.added.is_some() => match fact.as_ref().map(|f| f.status) {
            Some(Status::Proposed) => "pending",
            Some(_) => "kept",
            None => "dismissed",
        },
        S::Pending => "pending",
        S::Dismissed => "dismissed",
        S::Undone => "undone",
        S::Refused => "refused",
    };
    let mut v = json!({
        "state": state,
        "fact": fact,
        "target": target,
        "forget": r.added.is_none() && r.target.is_some(),
        "reason": r.why,
    });
    if let Some(f) = &fact {
        v["place"] = json!(if kept_in(f).is_some() { "workspace" } else { "user" });
    }
    Ok(v)
}

async fn call_state(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, call_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let (row, input) = own_call(&m, &who, &id, &call_id).await?;
    Ok(Json(state_json(&m, &who, &row, &call_id, &input).await?))
}

async fn decide(
    m: Master,
    headers: HeaderMap,
    id: String,
    call_id: String,
    d: crate::memory::Decision,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let (row, input) = own_call(&m, &who, &id, &call_id).await?;
    let uid = who.user_id.clone().unwrap_or_default();
    crate::memory::decide(&m, &uid, row.session_id, &call_id, d)
        .await
        .map_err(|why| ApiError(StatusCode::CONFLICT, why.into()))?;
    Ok(Json(state_json(&m, &who, &row, &call_id, &input).await?))
}

/// The person keeps what the call asked for (a save, a replacement, a
/// forgetting).
async fn keep(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, call_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    decide(m, headers, id, call_id, crate::memory::Decision::Keep).await
}

async fn dismiss(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, call_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    decide(m, headers, id, call_id, crate::memory::Decision::Dismiss).await
}

/// Undo what the call did, as recorded: delete the fact it saved, restore
/// the one it retired; nothing else.
async fn undo(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, call_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    decide(m, headers, id, call_id, crate::memory::Decision::Undo).await
}
