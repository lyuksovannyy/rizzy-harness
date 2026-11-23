//! `GET /api/sessions/{id}/files/{file_id}/preview?sheet=`: a Word or Excel
//! file of a chat without a folder, read as data for the page's side panel
//! (PLAN.md T99). Owner only, through the same check as opening the chat
//! (anyone else, admins included, gets the 404 a chat that does not exist
//! gets, T10); the file must be one the chat's own log names
//! (`session_files`, `logged`), made by the agent or attached by the owner,
//! and the blob must be the chat owner's: the checks of `made_files`.
//! The file is parsed only by the confined `doc-edit` helper (T98), one at a
//! time per account; the page gets what it sends back (strings, never HTML)
//! and, whatever the helper failed with, a sentence of ours.
//!
//! The login is the normal session, not a link: this is JSON for the page,
//! read by script, and a link (`POST /api/links`) is for bytes a browser
//! navigates to. A page of another origin cannot read it.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use harness_docs::edit::preview::Preview;
use harness_docs::edit::serve::{Request, frame_request, parse_reply};
use serde::{Deserialize, Serialize};

use super::{ApiError, ApiResult, require, visible_session};
use crate::pdf::DocError;
use crate::{Master, attached};

/// The longest `sheet` the route passes on, in bytes (a sheet's name is at
/// most 31 characters; this leaves room for any of them).
const MAX_SHEET_BYTES: usize = 200;
/// The largest answer the route forwards, in bytes: the helper's own bound
/// is under 256 KB, so this is a helper that misbehaves.
const MAX_ANSWER_BYTES: usize = 512 * 1024;

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/sessions/{id}/files/{file_id}/preview", get(preview))
}

#[derive(Deserialize)]
struct PreviewQuery {
    #[serde(default)]
    sheet: Option<String>,
}

#[derive(Serialize)]
struct Answer {
    /// The file's name as the chat has it (text for the page).
    name: String,
    preview: Preview,
}

fn unknown() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "unknown file".into())
}

/// A Word or Excel name: the only ones the preview spends a helper on.
fn office_name(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, e)| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "docx" | "docm" | "xlsx" | "xlsm"
        )
    })
}

fn sentence(e: DocError) -> ApiError {
    match e {
        DocError::NotSetUp | DocError::Unconfined | DocError::Busy | DocError::Timeout => {
            ApiError(StatusCode::SERVICE_UNAVAILABLE, e.to_string())
        }
        // The helper's own words may hold a name from the file.
        DocError::Unreadable(_) | DocError::Crashed | DocError::NoText => ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "this file could not be previewed: it is damaged, protected or too large".into(),
        ),
    }
}

async fn preview(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, file_id)): Path<(String, String)>,
    Query(q): Query<PreviewQuery>,
) -> ApiResult<Json<Answer>> {
    let who = require(&m, &headers).await?;
    let row = visible_session(&m, &who, &id).await?;
    // Only a plain chat keeps files: a coding chat's are in its folder.
    if row.profile.as_deref() != Some("chat") || row.parent_id.is_some() || row.incognito {
        return Err(unknown());
    }
    if file_id.is_empty()
        || file_id.len() > harness_core::attached::MAX_ID_CHARS
        || !file_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err(unknown());
    }
    if q.sheet.as_ref().is_some_and(|s| s.len() > MAX_SHEET_BYTES) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "that is not the name of a sheet".into(),
        ));
    }
    let sid = row.session_id.to_string();
    let fid = file_id.clone();
    let file = m
        .inner
        .mirror
        .call(move |s| s.session_file(&sid, &fid))
        .await?
        .ok_or_else(unknown)?;
    // The blob is the chat owner's, whatever a row says.
    if file.owner_id != row.owner_id {
        return Err(unknown());
    }
    if !office_name(&file.name) {
        return Err(not_office());
    }
    if file.size == 0 || file.size > attached::MAX_FILE {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "this file is too large to preview".into(),
        ));
    }
    let reader = m.pdf_reader().ok_or_else(|| sentence(DocError::NotSetUp))?;
    let account = row
        .owner_id
        .clone()
        .unwrap_or_else(|| format!("chat:{}", row.session_id));
    // One helper at a time per account: a second preview waits here, at most
    // the queue bound, then is told to try again.
    let _mine = reader
        .account_slot(&account)
        .await
        .map_err(|e| sentence(e.into()))?;
    let bytes = attached::bytes_of(&m, &file).await.map_err(|_| unknown())?;
    let out = reader
        .edit(frame_request(&Request::Preview { sheet: q.sheet }, &bytes))
        .await
        .map_err(sentence)?;
    drop(bytes);
    let (reply, _) = parse_reply(&out).ok_or_else(|| {
        ApiError(
            StatusCode::BAD_GATEWAY,
            "the document reader's answer was not understood".into(),
        )
    })?;
    let preview = reply.preview.ok_or_else(not_office)?;
    let answer = Answer {
        name: file.name,
        preview,
    };
    if serde_json::to_vec(&answer).map_or(usize::MAX, |v| v.len()) > MAX_ANSWER_BYTES {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "the document reader's answer was too large".into(),
        ));
    }
    Ok(Json(answer))
}

fn not_office() -> ApiError {
    ApiError(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "this is not a Word or Excel document that can be previewed".into(),
    )
}
