//! A person's rewind of a chat's conversation to an earlier message of
//! theirs, and putting it back (ADOPTION_PLAN X.46, docs/rewind.md, README
//! "Rewind"):
//!
//! * `POST /api/sessions/{id}/rewind` with `{message_seq, files, force}`;
//! * `POST /api/sessions/{id}/rewind/{rewind_seq}/undo`.
//!
//! Only the chat's owner (or the API token, for a chat nobody owns): anyone
//! else, admins and workspace admins included, gets the 404 of a chat that
//! does not exist (T10). The same gates as for a prompt: a settled password
//! and the owner's level for agentic work (`files` writes into the folder).
//!
//! The master answers what it can from its mirror
//! (`harness_core::rewind::check` and `check_undo`, the node's own rule),
//! refuses a sub-agent's chat and a routine's own first message, and relays
//! `session.rewind` or `session.rewind_undo`; the node decides again against
//! its own log under the turn slot (a turn, `/compact` or background work
//! running, a files part that is refused). Nothing a model, a tool or a
//! routine sends reaches these routes: a rewind is a person's click, never
//! automatic.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, acting_session, agentic_gate, require, visible_session};
use crate::{CommandError, Master};

pub(super) fn routes() -> Router<Master> {
    Router::new()
        .route("/api/sessions/{id}/rewind", post(rewind))
        .route("/api/sessions/{id}/rewind/{rewind_seq}/undo", post(undo))
}

fn refused(why: impl std::fmt::Display) -> ApiError {
    ApiError(
        StatusCode::CONFLICT,
        format!("{}: {why}", harness_protocol::REWIND_REFUSED),
    )
}

#[derive(Deserialize)]
struct Body {
    message_seq: u64,
    /// Put the folder back to before the rewound turns too.
    #[serde(default)]
    files: bool,
    /// Overwrite files edited after those turns too (as "Undo anyway").
    #[serde(default)]
    force: bool,
}

/// The chat's own rows, or the 404 of one that is not the caller's: this
/// is stricter than a prompt, which the API token may send to any chat.
pub(super) async fn owned(
    m: &Master,
    who: &super::auth::Principal,
    id: &str,
) -> ApiResult<harness_store::SessionRow> {
    let row = visible_session(m, who, id).await?;
    let mine = match &who.user_id {
        Some(me) => row.owner_id.as_deref() == Some(me.as_str()),
        None => row.owner_id.is_none(),
    };
    if !mine {
        return Err(ApiError(StatusCode::NOT_FOUND, "unknown session".into()));
    }
    // The same resolution as every request that changes a chat.
    acting_session(m, who, id).await
}

fn needs_update() -> ApiError {
    CommandError::Rejected(
        "this chat's node cannot rewind a conversation (a node older than this server, or one \
         that is away)"
            .into(),
    )
    .into()
}

async fn rewind(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<Body>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let row = owned(&m, &who, &id).await?;
    agentic_gate(&m, &who, &row).await?;
    if row.parent_id.is_some() {
        return Err(refused(
            "a sub-agent's chat is rewound by the agent that delegated it",
        ));
    }
    if !m.node_has(&row.node_id, harness_protocol::CAP_REWIND) {
        return Err(needs_update());
    }
    let sid = row.session_id;
    let log = m
        .inner
        .mirror
        .call(move |s| s.events_since_turn_at(sid, i64::MIN, harness_core::rewind::KINDS))
        .await?;
    let target = harness_core::rewind::check(&log, b.message_seq).map_err(refused)?;
    // A routine's run: its first message is the routine's prompt, not the
    // person's to take back (a later message of the owner's is theirs).
    let run = m.inner.mirror.call(move |s| s.run_by_session(sid)).await?;
    if run.is_some() {
        let first = m
            .inner
            .mirror
            .call(move |s| s.first_event(sid, "turn_started"))
            .await?;
        if first.and_then(|f| {
            f.get("turn_id")
                .and_then(|t| t.as_str())
                .map(str::to_string)
        }) == Some(target.turn_id.clone())
        {
            return Err(refused(
                "it is the routine's own run; run the routine again instead",
            ));
        }
    }
    tracing::info!(session = %sid, message_seq = b.message_seq, files = b.files,
        "a person rewinds a chat to an earlier message");
    m.rewind(sid, b.message_seq, b.files, b.force).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn undo(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, rewind_seq)): Path<(String, u64)>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let row = owned(&m, &who, &id).await?;
    agentic_gate(&m, &who, &row).await?;
    if !m.node_has(&row.node_id, harness_protocol::CAP_REWIND) {
        return Err(needs_update());
    }
    let sid = row.session_id;
    let log = m
        .inner
        .mirror
        .call(move |s| s.events_since_turn_at(sid, i64::MIN, harness_core::rewind::KINDS))
        .await?;
    harness_core::rewind::check_undo(&log, rewind_seq).map_err(refused)?;
    tracing::info!(session = %sid, rewind_seq, "a person puts a rewind back");
    m.rewind_undo(sid, rewind_seq).await?;
    Ok(Json(json!({ "ok": true })))
}
