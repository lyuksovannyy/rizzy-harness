//! The change of a chat's step limit (X.14, README "Step and time limits"):
//! how many model calls with tools one turn of that chat may make, from 1
//! up to its node's `[limits] max_steps`. Only for whoever may see the
//! chat: its owner, or the API token (which can already prompt it); anyone
//! else, admins and workspace admins included, gets the 404 a chat that
//! does not exist gets (T10). A raise (more steps than the chat has now)
//! needs a settled password and passes the owner's execution-level check,
//! as a raise of the approval mode does; a lowering needs neither.
//!
//! The master decides nothing about the limit itself: it relays
//! `session.set_step_limit` to the owning node, which clamps it to
//! `1..=` its own limit, refuses it for a sub-agent's or a routine's chat,
//! and records `step_limit_changed`; the chat shows the limit from that
//! event.

use axum::extract::{Path, State};

use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use super::caller::{self, Caller};
use super::{ApiResult, acting_session, agentic_gate};
use crate::Master;

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/sessions/{id}/step-limit", post(set_limit))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LimitBody {
    /// An integer; the node clamps it to `1..=` its limit.
    max_steps: i64,
}

async fn set_limit(
    Caller(who): Caller,
    State(m): State<Master>,
    Path(id): Path<String>,
    Json(b): Json<LimitBody>,
) -> ApiResult<Json<Value>> {
    let row = acting_session(&m, &who, &id).await?;
    let (own, max) = m.step_limit_of(row.session_id).await?;
    // Unknown: treated as a raise (checked), never as a lowering.
    let now = own.or(max);
    let raise = now.is_none_or(|n| b.max_steps > n as i64);
    if raise {
        caller::settled(&who)?;
        agentic_gate(&m, &who, &row).await?;
    }
    tracing::info!(session = %row.session_id, max_steps = b.max_steps, raise, "the step limit changes");
    m.set_step_limit(row.session_id, b.max_steps, raise).await?;
    Ok(Json(json!({ "ok": true })))
}
