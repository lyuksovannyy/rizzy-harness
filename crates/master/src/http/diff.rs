//! A turn's changes (X.12, PLAN.md T53): `GET
//! /api/sessions/{id}/turns/{turn}/diff[?rewind=1]`. Owner only, through
//! the same check as opening the chat: anyone else, admins and workspace
//! admins included, gets the 404 a chat that does not exist gets (T10).
//! A read: no agentic grant is needed, as for reading the chat's events.
//!
//! The master asks the owning node (`crate::diff`) and passes its answer on
//! as it came; nothing is cached or stored (every `/api` answer is
//! `no-store`, `response.rs`). When there is nothing to show, the answer
//! is a sentence with a status that says whose it is: 503 the node is
//! away, 409 the node's own reason (pruned, unknown turn, busy) or a node
//! too old, 504 no answer in time. Never a 500 for any of those.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use harness_protocol::TurnDiff;
use serde::Deserialize;

use super::caller::User;
use super::{ApiError, ApiResult, visible_session};
use crate::Master;
use crate::diff::DiffError;

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/sessions/{id}/turns/{turn}/diff", get(turn_diff))
}

#[derive(Deserialize)]
struct DiffQuery {
    #[serde(default)]
    rewind: Option<String>,
}

/// A turn id as nodes make them (`new_id`, `restore-<id>`): short, plain.
fn is_turn_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 80
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

async fn turn_diff(
    User(who): User,
    State(m): State<Master>,
    Path((id, turn)): Path<(String, String)>,
    Query(q): Query<DiffQuery>,
) -> ApiResult<Json<TurnDiff>> {
    let row = visible_session(&m, &who, &id).await?;
    if !is_turn_id(&turn) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "bad turn id".into()));
    }
    let rewind = matches!(q.rewind.as_deref(), Some("1" | "true"));
    let node = row.node_id.clone();
    let user = who.user_id.clone().unwrap_or_else(|| "(api token)".into());
    match crate::diff::turn_diff(&m, &user, row.session_id, turn, rewind).await {
        Ok(d) => Ok(Json(*d)),
        Err(e) => Err(error(
            &node,
            e,
            "this chat's node is older than this server and cannot show a turn's changes; \
             update the node",
        )),
    }
}

/// The words and status of a refused diff, for the turn's route and the
/// whole chat's (`chat_diff.rs`); `too_old` is the sentence for a node
/// without the capability.
pub(super) fn error(node: &str, e: DiffError, too_old: &str) -> ApiError {
    match e {
        DiffError::Away(_) => ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "the node {node} that holds this chat's folder is away, so its changes \
                 cannot be shown now; they can once it is back"
            ),
        ),
        DiffError::TooOld => ApiError(StatusCode::CONFLICT, too_old.into()),
        DiffError::Busy => ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "the server is already waiting for several changes (yours or others'); try again in \
             a moment"
                .into(),
        ),
        DiffError::Timeout => ApiError(
            StatusCode::GATEWAY_TIMEOUT,
            "the node did not send the changes in time".into(),
        ),
        DiffError::Refused(why) => ApiError(StatusCode::CONFLICT, why),
        DiffError::Unknown => ApiError(StatusCode::NOT_FOUND, "unknown session".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_ids_are_the_nodes_shape_only() {
        for ok in ["0193f2a0-0000-7000-8000-000000000001", "restore-abc_1"] {
            assert!(is_turn_id(ok), "{ok}");
        }
        for bad in ["", "../x", "a b", "a/b", "%2e%2e", &"x".repeat(81)] {
            assert!(!is_turn_id(bad), "{bad:?}");
        }
    }
}
