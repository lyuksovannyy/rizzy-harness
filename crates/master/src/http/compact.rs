//! A person's `/compact` (ADOPTION_PLAN X.20, PLAN.md T64, README "Context
//! and summaries"): the chat's history summarised now, with optional words
//! on what to keep in focus. The same checks as sending the chat a prompt:
//! its owner (or the API token) only; anyone else, admins and workspace
//! admins included, gets the 404 a chat that does not exist gets (T10);
//! a settled password and the owner's level for agentic work.
//!
//! The master decides nothing about the summary: it relays
//! `session.compact` to the owning node, which refuses it while a turn
//! runs or for a chat too short to summarise (the answer is this request's
//! 409), else starts it and records the outcome in the chat's log
//! (`history_summarized` with `manual`, or `compact_failed`). Nothing a
//! model or a routine sends reaches this route: it is a person's command,
//! never a tool.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, agentic_gate, require, visible_session};
use crate::{CommandError, Master};

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/sessions/{id}/compact", post(compact))
}

/// The most bytes of focus text the master forwards; the node cuts what
/// it passes on to `COMPACT_FOCUS_MAX_CHARS` characters.
const FOCUS_MAX_BYTES: usize = 64 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompactBody {
    #[serde(default)]
    focus: Option<String>,
}

async fn compact(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<CompactBody>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let row = visible_session(&m, &who, &id).await?;
    agentic_gate(&m, &who, &row).await?;
    if b.focus.as_ref().is_some_and(|f| f.len() > FOCUS_MAX_BYTES) {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "the focus text is too long".into(),
        ));
    }
    if !m.node_has(&row.node_id, harness_protocol::CAP_COMPACT) {
        return Err(CommandError::Rejected(
            "this chat's node cannot summarise on request (a node older than this server, or \
             one that is away)"
                .into(),
        )
        .into());
    }
    let sid = row.session_id;
    // The windows the master knows for the models the node may summarise
    // with: the chat's own and the one its last turn ended on.
    let log = m.inner.mirror.call(move |s| s.handoff_log(sid)).await?;
    let last_model = log.iter().rev().find_map(|e| match e.typed() {
        Ok(harness_core::event::EventKind::TurnEnded(t)) => t.usage.and_then(|u| u.model),
        _ => None,
    });
    let windows = last_model
        .iter()
        .chain(std::iter::once(&row.model))
        .filter_map(|id| Some((id.clone(), m.find_model(id)?.context?)))
        .collect();
    let focus = b.focus.filter(|f| !f.trim().is_empty());
    tracing::info!(session = %sid, focus = focus.is_some(), "a person asks for a summary");
    m.send_command(&row.node_id, move |req_id| {
        harness_protocol::MasterToNode::Compact {
            req_id,
            session_id: sid,
            focus,
            windows,
        }
    })
    .await?;
    Ok(Json(json!({ "ok": true })))
}
