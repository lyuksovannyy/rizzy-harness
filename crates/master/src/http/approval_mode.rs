//! The change of a chat's approval mode (T50, README "Approval modes"):
//! `ask_all`, `ask`, `accept_edits` or `auto`. Only for whoever may see the
//! chat: its owner, or the API token (which can already prompt and approve
//! it); anyone else, admins and workspace admins included, gets the 404 a
//! chat that does not exist gets (T10). A raise (a looser mode than the
//! chat has now) needs a settled password and passes the owner's
//! execution-level check, as every command that makes a chat work; a
//! lowering, like a denial, needs neither (review F6). A raise to `auto` is
//! written to the audit log (ids and the two modes).
//!
//! The master decides nothing about the mode itself: it relays
//! `session.set_approval_mode` to the owning node, which clamps it by its
//! own `max_approval_mode`, refuses it for a sub-agent's or a routine's
//! chat, and records `approval_mode_changed`; the chat shows the mode from
//! that event.

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use harness_core::policy::ApprovalMode;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiResult, acting_session, agentic_gate, require, require_any};
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
    let who = require_any(&m, &headers).await?;
    let row = acting_session(&m, &who, &id).await?;
    let now = m.approval_mode_of(row.session_id).await?;
    // Unknown: treated as a raise (checked), never as a lowering.
    let raise = now.is_none_or(|n| b.mode > n);
    if raise {
        let who = require(&m, &headers).await?;
        agentic_gate(&m, &who, &row).await?;
    }
    tracing::info!(session = %row.session_id, mode = b.mode.id(), raise, "the approval mode changes");
    m.set_approval_mode(row.session_id, b.mode, raise).await?;
    if raise && b.mode == ApprovalMode::Auto {
        crate::levels::audit(
            &m,
            "approval_mode.auto",
            json!({
                "session_id": row.session_id.to_string(),
                "user_id": who.user_id,
                "from": now.map(|n| n.id()),
                "to": b.mode.id(),
            }),
        )
        .await;
    }
    Ok(Json(json!({ "ok": true })))
}
