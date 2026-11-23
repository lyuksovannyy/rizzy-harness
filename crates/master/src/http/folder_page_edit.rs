//! `POST /api/sessions/{id}/folder-edit`: a person's text changes to a Word
//! or Excel file of a coding chat's folder, saved by the node as a new file
//! beside it (PLAN.md T100, ADOPTION_PLAN X.60). The master runs and writes
//! nothing here: it asks the node that owns the chat (`crate::folder_preview`,
//! one `session.doc_page_edit`), which resolves the path under the rules of
//! its `read` tool, runs its confined helper and writes the copy with the
//! create-only write of `doc_edit`; and only if node.toml lets the agent's own
//! editing tools run. The checks are those of `folder_preview.rs`, in the
//! same order, plus the rule of every POST (the guard of `auth.rs`) and
//! `acting_session`. The body is read after the caller is known.
//!
//! What the node answers is text from a node: the path it names is bounded and
//! must be plain (no control character, not absolute, no `..`), the results
//! must be one per change, and anything else is a 502 that is not forwarded.

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use harness_protocol::{MAX_PREVIEW_PATH_BYTES, MAX_SAVED_PATH_BYTES, PageChange};
use serde::Deserialize;
use serde_json::json;

use super::doc_page_edit::{MAX_BODY_BYTES, check_changes, parse, refused_changes, results_json};
use super::doc_preview::refusal;
use super::files::away;
use super::{ApiError, ApiResult, acting_session, agentic_gate, require};
use crate::Master;
use crate::folder_preview::PreviewError;

pub(super) fn routes() -> Router<Master> {
    Router::new().route(
        "/api/sessions/{id}/folder-edit",
        post(edit).layer(DefaultBodyLimit::max(MAX_BODY_BYTES)),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Body {
    #[serde(default)]
    path: String,
    #[serde(default)]
    sheet: Option<u32>,
    changes: Vec<PageChange>,
}

fn unknown() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "unknown file".into())
}

fn bad() -> ApiError {
    ApiError(
        StatusCode::BAD_GATEWAY,
        "the node's answer was not understood".into(),
    )
}

/// A path a node names for the copy: text of bounded length that is relative
/// and plain.
fn plain_path(p: &str) -> bool {
    !p.is_empty()
        && p.len() <= MAX_SAVED_PATH_BYTES
        && !p.starts_with('/')
        && !p.chars().any(char::is_control)
        && !p.split('/').any(|c| c == ".." || c.is_empty())
}

async fn edit(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> ApiResult<Response> {
    let who = require(&m, &headers).await?;
    let row = acting_session(&m, &who, &id).await?;
    agentic_gate(&m, &who, &row).await?;
    // Only a plain top-level coding chat with a folder: a chat without one
    // has the other route; a temporary chat shows no files.
    if row.profile.as_deref() == Some("chat") || row.incognito {
        return Err(unknown());
    }
    let Body {
        path,
        sheet,
        changes,
    } = parse(&body)?;
    if path.is_empty() || path.len() > MAX_PREVIEW_PATH_BYTES {
        return Err(unknown());
    }
    check_changes(&changes)?;
    let n = changes.len();
    let user = who.user_id.clone().unwrap_or_else(|| "(api token)".into());
    let node = row.node_id.clone();
    let done = crate::folder_preview::ask_edit(&m, &user, row.session_id, path, sheet, changes)
        .await
        .map_err(|e| match e {
            PreviewError::Away(_) => away(&node),
            PreviewError::TooOld => ApiError(
                StatusCode::CONFLICT,
                "this chat's node cannot save changes to files; update the node".into(),
            ),
            PreviewError::Busy => ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                "another file is being read; try again in a moment".into(),
            ),
            PreviewError::Timeout => ApiError(
                StatusCode::GATEWAY_TIMEOUT,
                "the node did not finish in time; nothing is known to be saved, look in the folder"
                    .into(),
            ),
            PreviewError::Refused(r) => refusal(r, true),
            PreviewError::Unknown => unknown(),
        })?;
    if done.results.len() != n {
        return Err(bad());
    }
    let all_ok = done.results.iter().all(|r| r.ok);
    match done.saved {
        Some(saved) if all_ok && plain_path(&saved) => {
            let name = saved.rsplit('/').next().unwrap_or(&saved).to_string();
            Ok(Json(json!({
                "saved": saved,
                "name": name,
                "results": results_json(&done.results),
            }))
            .into_response())
        }
        None if !all_ok => Ok(refused_changes(&done.results)),
        _ => Err(bad()),
    }
}
