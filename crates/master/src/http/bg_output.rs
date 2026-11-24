//! What a background command is printing (ADOPTION_PLAN X.62, PLAN.md T8D):
//! `GET /api/sessions/{id}/background/{bg}/output`. Owner only, through the
//! same check as opening the chat: anyone else, admins and workspace admins
//! included, gets the 404 a chat that does not exist gets (T10), with the
//! node never asked. A read: no agentic grant is needed, as for reading the
//! chat's events, which hold the same output (scrubbed the same way).
//!
//! The master asks the owning node (`crate::bg_output`) and passes its
//! answer on as it came; nothing is cached or stored (every `/api` answer
//! is `no-store`). The node answers from its own table for that chat, so an
//! id of another chat is not found there. When there is nothing to show the
//! answer is a sentence with a status that says whose it is: 503 the node is
//! away or the server is busy, 409 the node's reason (forgotten, a node
//! older than this server), 504 no answer in time. Never a 500.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use harness_protocol::BackgroundOutput;

use super::background::is_bg_id;
use super::{ApiError, ApiResult, require, visible_session};
use crate::Master;
use crate::bg_output::BgOutputError;

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/sessions/{id}/background/{bg}/output", get(output))
}

async fn output(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, bg)): Path<(String, String)>,
) -> ApiResult<Json<BackgroundOutput>> {
    let who = require(&m, &headers).await?;
    let row = visible_session(&m, &who, &id).await?;
    if !is_bg_id(&bg) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "bad background command id (bg-<number>)".into(),
        ));
    }
    let node = row.node_id.clone();
    let user = who.user_id.clone().unwrap_or_else(|| "(api token)".into());
    crate::bg_output::output(&m, &user, row.session_id, bg)
        .await
        .map(Json)
        .map_err(|e| match e {
            BgOutputError::Away(_) => ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                format!(
                    "the node {node} that runs this chat is away, so its output cannot be read now"
                ),
            ),
            BgOutputError::TooOld => ApiError(
                StatusCode::CONFLICT,
                "this chat's node is older than this server and cannot show a command's output; \
                 update the node"
                    .into(),
            ),
            BgOutputError::Busy => ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                "the server is already waiting for several outputs (yours or others'); try again \
                 in a moment"
                    .into(),
            ),
            BgOutputError::Timeout => ApiError(
                StatusCode::GATEWAY_TIMEOUT,
                "the node did not send the output in time".into(),
            ),
            BgOutputError::Refused(why) => ApiError(StatusCode::CONFLICT, why),
            BgOutputError::Unknown => ApiError(StatusCode::NOT_FOUND, "unknown session".into()),
        })
}
