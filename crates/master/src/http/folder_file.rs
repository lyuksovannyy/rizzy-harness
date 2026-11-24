//! `GET /api/sessions/{id}/folder-file?path=`: a text file of a coding chat's
//! folder, read as data for the page's side panel (PLAN.md T99, ADOPTION_PLAN
//! X.63). Read-only. The master runs nothing and reads nothing here: it asks
//! the node that owns the chat (`crate::folder_preview`), and the node
//! resolves the path under the rules the document preview has (pinned folder,
//! links judged, never outside the folder, never `.git`, protected places,
//! `read` not denied by node.toml), reads at most 256 KiB of it and scrubs
//! the text of the node's secrets. The checks are those of
//! `folder_preview.rs`, in the same order: owner only through the check of
//! opening the chat (anyone else, admins included, gets the 404 a chat that
//! does not exist gets, T10), the agentic grant (`files.rs`), a coding chat
//! with a folder that is not temporary.
//!
//! The path is in the query: it is not a secret, and no log line holds a
//! query (the request log and the timeout log write the path of the URL
//! only). The master does not interpret it beyond its length.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use harness_protocol::{
    FileRefusal, FolderFileText, MAX_FOLDER_FILE_TEXT_BYTES, MAX_PREVIEW_PATH_BYTES,
};
use serde::{Deserialize, Serialize};

use super::caller::User;
use super::files::away;
use super::folder_preview::{base_name, unknown};
use super::{ApiError, ApiResult, agentic_gate, visible_session};
use crate::Master;
use crate::folder_preview::PreviewError;

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/sessions/{id}/folder-file", get(file))
}

#[derive(Deserialize)]
struct FileQuery {
    #[serde(default)]
    path: String,
}

/// What the page gets: the file's name (from the path the page asked for, not
/// from the node), and what the node read.
#[derive(Serialize)]
struct Answer {
    name: String,
    #[serde(flatten)]
    file: FolderFileText,
}

fn refusal(r: FileRefusal) -> ApiError {
    match r {
        FileRefusal::NotFound => unknown(),
        FileRefusal::Binary => ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "this file is not text, so it is not shown here".into(),
        ),
        FileRefusal::Denied => ApiError(
            StatusCode::FORBIDDEN,
            "this chat's node does not let files be read".into(),
        ),
        FileRefusal::Busy => ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "another file is being read; try again in a moment".into(),
        ),
        FileRefusal::Unknown => ApiError(
            StatusCode::BAD_GATEWAY,
            "the node's answer was not understood".into(),
        ),
    }
}

async fn file(
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
    Query(q): Query<FileQuery>,
) -> ApiResult<Json<Answer>> {
    let row = visible_session(&m, &who, &id).await?;
    agentic_gate(&m, &who, &row).await?;
    // Only a coding chat with a folder: a chat without one has no files; a
    // temporary chat shows none. A sub-agent's chat of the owner is asked as
    // any other: it shares its parent's folder and the node reads under the
    // session's own rules.
    if row.profile.as_deref() == Some("chat") || row.incognito {
        return Err(unknown());
    }
    if q.path.is_empty() || q.path.len() > MAX_PREVIEW_PATH_BYTES {
        return Err(unknown());
    }
    let user = who.user_id.clone().unwrap_or_else(|| "(api token)".into());
    let node = row.node_id.clone();
    let file = crate::folder_preview::ask_file(&m, &user, row.session_id, q.path.clone())
        .await
        .map_err(|e| match e {
            PreviewError::Away(_) => away(&node),
            PreviewError::TooOld => ApiError(
                StatusCode::CONFLICT,
                "this chat's node cannot show files; update the node".into(),
            ),
            PreviewError::Busy => ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                "another file is being read; try again in a moment".into(),
            ),
            PreviewError::Timeout => ApiError(
                StatusCode::GATEWAY_TIMEOUT,
                "the node did not read the file in time".into(),
            ),
            PreviewError::File(r) => refusal(r),
            PreviewError::Refused(_) | PreviewError::Unknown => unknown(),
        })?;
    // A node that sends more than it was asked for is not forwarded.
    if file.text.len() > MAX_FOLDER_FILE_TEXT_BYTES {
        return Err(refusal(FileRefusal::Unknown));
    }
    Ok(Json(Answer {
        name: base_name(&q.path),
        file,
    }))
}
