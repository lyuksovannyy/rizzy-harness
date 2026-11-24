//! `GET /api/sessions/{id}/folder-preview?path=&sheet=`: a Word or Excel
//! file of a coding chat's folder, read as data for the page's side panel
//! (PLAN.md T99, ADOPTION_PLAN X.59). The same answer as the preview of a
//! chat without a folder (`doc_preview.rs`: same JSON, same sentences), but
//! the master runs nothing and reads nothing here: it asks the node that
//! owns the chat (`crate::folder_preview`), and the node resolves the path
//! under the rules its `read` tool has, reads the file and runs its confined
//! helper. Owner only, through the check of opening the chat (anyone else,
//! admins included, gets the 404 a chat that does not exist gets, T10), and
//! the agentic grant that reaching the folder needs (`files.rs`).
//!
//! The path is in the query: it is not a secret, and no log line holds a
//! query (the request log and the timeout log write the path of the URL
//! only). The master does not interpret it beyond its length.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use harness_docs::edit::preview::Preview;
use harness_protocol::MAX_PREVIEW_PATH_BYTES;
use serde::Deserialize;

use super::caller::User;
use super::doc_preview::{Answer, MAX_ANSWER_BYTES, refusal};
use super::files::away;
use super::{ApiError, ApiResult, agentic_gate, visible_session};
use crate::Master;
use crate::folder_preview::PreviewError;

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/sessions/{id}/folder-preview", get(preview))
}

#[derive(Deserialize)]
struct PreviewQuery {
    #[serde(default)]
    path: String,
    #[serde(default)]
    sheet: Option<u32>,
}

pub(super) fn unknown() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "unknown file".into())
}

/// The last component of a path, for the panel's title: the file's name as
/// the page's own text (the node never sends it).
pub(super) fn base_name(path: &str) -> String {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .chars()
        .filter(|c| !c.is_control())
        .take(255)
        .collect()
}

async fn preview(
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
    Query(q): Query<PreviewQuery>,
) -> ApiResult<Json<Answer>> {
    let row = visible_session(&m, &who, &id).await?;
    agentic_gate(&m, &who, &row).await?;
    // Only a coding chat with a folder: a chat without one has the other
    // route; a temporary chat shows no files. A sub-agent's chat of the owner
    // is asked as any other: it shares its parent's folder and the node reads
    // under the session's own rules.
    if row.profile.as_deref() == Some("chat") || row.incognito {
        return Err(unknown());
    }
    if q.path.is_empty() || q.path.len() > MAX_PREVIEW_PATH_BYTES {
        return Err(unknown());
    }
    let user = who.user_id.clone().unwrap_or_else(|| "(api token)".into());
    let node = row.node_id.clone();
    let value = crate::folder_preview::ask(&m, &user, row.session_id, q.path.clone(), q.sheet)
        .await
        .map_err(|e| match e {
            PreviewError::Away(_) => away(&node),
            PreviewError::TooOld => ApiError(
                StatusCode::CONFLICT,
                "this chat's node cannot preview files; update the node".into(),
            ),
            PreviewError::Busy => ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                "another preview is being read; try again in a moment".into(),
            ),
            PreviewError::Timeout => ApiError(
                StatusCode::GATEWAY_TIMEOUT,
                "the node did not read the file in time".into(),
            ),
            PreviewError::Refused(r) => refusal(r, true),
            PreviewError::File(_) | PreviewError::Unknown => unknown(),
        })?;
    // The node's answer is read once into the typed preview: anything else
    // is a node that misbehaves, and is not forwarded.
    let bad = || {
        ApiError(
            StatusCode::BAD_GATEWAY,
            "the node's answer was not understood".into(),
        )
    };
    let preview: Preview = serde_json::from_value(value).map_err(|_| bad())?;
    let answer = Answer {
        name: base_name(&q.path),
        preview,
    };
    if serde_json::to_vec(&answer).map_or(usize::MAX, |v| v.len()) > MAX_ANSWER_BYTES {
        return Err(bad());
    }
    Ok(Json(answer))
}
