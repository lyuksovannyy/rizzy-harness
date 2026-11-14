//! The owner's change of a chat's approval mode (T48, README "Approval
//! modes"): `ask_all`, `ask`, `accept_edits` or `auto`. Owner only, as every
//! per-chat route: anyone else, admins and workspace admins included, gets
//! the 404 a chat that does not exist gets (T10). It goes through
//! `Master::send_command`, which checks the owner's execution level now, as
//! for every command that makes a chat work.
//!
//! The master decides nothing about the mode itself: it relays
//! `session.set_approval_mode` to the owning node, which clamps it by its
//! own `max_approval_mode` and the chat's execution mode, refuses it for a
//! sub-agent's or a routine's chat, and records `approval_mode_changed`;
//! the chat shows the mode from that event.

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use harness_core::policy::ApprovalMode;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiResult, agentic_gate, require, visible_session};
use crate::Master;

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/sessions/{id}/approval-mode", post(set_mode))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModeBody {
    mode: ApprovalMode,
}

async fn set_mode(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<ModeBody>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let row = visible_session(&m, &who, &id).await?;
    agentic_gate(&m, &who, &row).await?;
    tracing::info!(session = %row.session_id, mode = b.mode.id(), "the owner changes the approval mode");
    m.set_approval_mode(row.session_id, b.mode).await?;
    Ok(Json(json!({ "ok": true })))
}
