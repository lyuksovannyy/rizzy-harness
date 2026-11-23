//! `@` file mentions (PLAN.md T63): `GET /api/sessions/{id}/files?q=` for
//! the composer's completion, and the checks on a prompt's `files`. Owner
//! only, through the same check as opening the chat (anyone else, admins
//! included, gets the 404 a chat that does not exist gets, T10), and the
//! same agentic grant as sending to it: the paths are for a message only
//! the owner may send. The master asks the owning node (`crate::files`)
//! and keeps nothing.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use harness_protocol::{CAP_FILES, MAX_PROMPT_FILE_BYTES, MAX_PROMPT_FILES};
use serde::Deserialize;
use serde_json::json;

use super::{ApiError, ApiResult, agentic_gate, require, visible_session};
use crate::Master;
use crate::files::FilesError;

/// The longest query passed to the node, in bytes.
const MAX_QUERY: usize = 256;

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/sessions/{id}/files", get(complete))
}

#[derive(Deserialize)]
struct FilesQuery {
    #[serde(default)]
    q: String,
}

/// A chat with no folder (the chat profile) has no files to mention.
fn has_folder(row: &harness_store::SessionRow) -> bool {
    row.profile.as_deref() != Some("chat")
}

async fn complete(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<FilesQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let row = visible_session(&m, &who, &id).await?;
    agentic_gate(&m, &who, &row).await?;
    if !has_folder(&row) || q.q.len() > MAX_QUERY {
        return Ok(Json(json!({ "paths": [] })));
    }
    let node = row.node_id.clone();
    let user = who.user_id.clone().unwrap_or_else(|| "(api token)".into());
    match crate::files::complete(&m, &user, row.session_id, q.q).await {
        Ok(paths) => Ok(Json(json!({ "paths": paths }))),
        Err(e) => Err(match e {
            FilesError::Away(_) => away(&node),
            FilesError::TooOld => ApiError(
                StatusCode::CONFLICT,
                "this chat's node is older than this server and cannot list its files; update \
                 the node"
                    .into(),
            ),
            FilesError::Busy => ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                "too many file lists are being looked up; try again in a moment".into(),
            ),
            FilesError::Timeout => ApiError(
                StatusCode::GATEWAY_TIMEOUT,
                "the node did not list the files in time".into(),
            ),
            FilesError::Refused(why) => ApiError(StatusCode::CONFLICT, why),
            FilesError::Unknown => ApiError(StatusCode::NOT_FOUND, "unknown session".into()),
        }),
    }
}

/// The answer for a node that is not connected, in words for the person.
pub(super) fn away(node: &str) -> ApiError {
    ApiError(
        StatusCode::SERVICE_UNAVAILABLE,
        format!("the node {node} that holds this chat's folder is away"),
    )
}

/// A prompt's `files`: at most [`MAX_PROMPT_FILES`] paths of at most
/// [`MAX_PROMPT_FILE_BYTES`] each, only in a chat with a folder, only to a
/// node that reads them. The node reads each under its own rules.
pub(super) fn check_mentions(
    m: &Master,
    row: &harness_store::SessionRow,
    files: &[String],
) -> ApiResult<()> {
    if files.is_empty() {
        return Ok(());
    }
    let bad = |s: String| Err(ApiError(StatusCode::BAD_REQUEST, s));
    if !has_folder(row) {
        return bad("this chat has no folder, so it has no files to mention".into());
    }
    if files.len() > MAX_PROMPT_FILES {
        return bad(format!(
            "at most {MAX_PROMPT_FILES} mentioned files per message"
        ));
    }
    if files.iter().any(|f| f.len() > MAX_PROMPT_FILE_BYTES) {
        return bad(format!(
            "a mentioned path is longer than {MAX_PROMPT_FILE_BYTES} bytes"
        ));
    }
    if !m.node_has(&row.node_id, CAP_FILES) {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "this chat's node is older than this server and cannot read mentioned files; update \
             the node, or send the message without them"
                .into(),
        ));
    }
    Ok(())
}
