//! All of a chat's changes in one diff (ADOPTION_PLAN X.61, PLAN.md T53):
//! `GET /api/sessions/{id}/diff`. Owner only, through the same check as
//! opening the chat: anyone else, admins and workspace admins included, gets
//! the 404 a chat that does not exist gets (T10). So does a chat without a
//! folder (a plain chat) and a temporary one: they have no checkpoints, and
//! the node is not asked. A read: no agentic grant is needed, as for a
//! turn's diff.
//!
//! The same request to the owning node as a turn's diff (`crate::diff`,
//! with `all`): the same waiting places and bounds, the same statuses (503
//! away, 409 the node's reason or a node too old, 504 no answer in time),
//! the node's answer passed on as it came, nothing kept.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use harness_protocol::TurnDiff;

use super::diff::error;
use super::{ApiError, ApiResult, require, visible_session};
use crate::Master;

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/sessions/{id}/diff", get(chat_diff))
}

async fn chat_diff(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<TurnDiff>> {
    let who = require(&m, &headers).await?;
    let row = visible_session(&m, &who, &id).await?;
    if row.profile.as_deref() == Some("chat") || row.incognito {
        return Err(ApiError(StatusCode::NOT_FOUND, "unknown session".into()));
    }
    let node = row.node_id.clone();
    let user = who.user_id.clone().unwrap_or_else(|| "(api token)".into());
    crate::diff::chat_diff(&m, &user, row.session_id)
        .await
        .map(|d| Json(*d))
        .map_err(|e| {
            error(
                &node,
                e,
                "this chat's node is older than this server and cannot show all of a chat's \
                 changes; update the node",
            )
        })
}
