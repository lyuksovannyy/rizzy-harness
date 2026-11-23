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
use harness_docs::edit::preview::{Preview, Why};
use harness_docs::edit::serve::{Request, frame_request, parse_reply};
use harness_protocol::PreviewRefusal;
use serde::{Deserialize, Serialize};

use super::{ApiError, ApiResult, require, visible_session};
use crate::pdf::DocError;
use crate::{Master, attached};

/// The largest answer the route forwards, in bytes: the helper's own bound
/// is under 256 KB, so this is a helper that misbehaves.
pub(super) const MAX_ANSWER_BYTES: usize = 512 * 1024;
/// The most the master reads from the helper for a preview: that, a header
/// and room for the framing. More is a failed helper.
pub(super) const MAX_HELPER_BYTES: usize = MAX_ANSWER_BYTES + 128 * 1024;

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/sessions/{id}/files/{file_id}/preview", get(preview))
}

#[derive(Deserialize)]
struct PreviewQuery {
    #[serde(default)]
    sheet: Option<u32>,
}

#[derive(Serialize)]
pub(super) struct Answer {
    /// The file's name as the chat has it (text for the page).
    pub(super) name: String,
    pub(super) preview: Preview,
}

fn unknown() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "unknown file".into())
}

/// A Word or Excel name: the only ones the preview spends a helper on.
pub(super) fn office_name(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, e)| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "docx" | "docm" | "xlsx" | "xlsm"
        )
    })
}

pub(super) fn sentence(e: DocError) -> ApiError {
    match e {
        DocError::NotSetUp | DocError::Unconfined | DocError::Busy | DocError::Timeout => {
            ApiError(StatusCode::SERVICE_UNAVAILABLE, e.to_string())
        }
        // The helper's own words may hold a name from the file.
        DocError::Unreadable(_) | DocError::Crashed | DocError::NoText => why(Why::Damaged),
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
    if file.size == 0 {
        return Err(refusal(PreviewRefusal::Empty, false));
    }
    if file.size > attached::MAX_FILE {
        return Err(refusal(PreviewRefusal::FileTooLarge, false));
    }
    let reader = m.pdf_reader().ok_or_else(|| sentence(DocError::NotSetUp))?;
    // The caller's place, as attachments take it: the API token has one of
    // its own, whatever chat it asks about.
    let account = who.user_id.clone().unwrap_or_else(|| "api-token".into());
    // One helper at a time per account: a second preview waits here, at most
    // the queue bound, then is told to try again.
    let _mine = reader
        .account_slot(&account)
        .await
        .map_err(|e| sentence(e.into()))?;
    let bytes = attached::bytes_of(&m, &file).await.map_err(|_| unknown())?;
    let out = reader
        .edit_capped(
            frame_request(&Request::Preview { sheet: q.sheet }, &bytes),
            MAX_HELPER_BYTES,
        )
        .await
        .map_err(sentence)?;
    drop(bytes);
    let (reply, _) = parse_reply(&out).ok_or_else(|| {
        ApiError(
            StatusCode::BAD_GATEWAY,
            "the document reader's answer was not understood".into(),
        )
    })?;
    let preview = match (reply.preview, reply.why) {
        (Some(p), _) => p,
        (None, Some(w)) => return Err(why(w)),
        (None, None) => return Err(not_office()),
    };
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

pub(super) fn not_office() -> ApiError {
    ApiError(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "this is not a Word or Excel document that can be previewed".into(),
    )
}

/// What the helper said it could not do, as one sentence of ours each.
pub(super) fn why(w: Why) -> ApiError {
    let (code, text) = match w {
        Why::NotOffice => return not_office(),
        Why::Protected => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "this file is protected with a password, so it cannot be previewed",
        ),
        Why::TooLarge => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "this file is too large to preview; download it to open it",
        ),
        Why::Damaged => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "this file could not be previewed: it is damaged or signed in a way the reader will not keep",
        ),
        Why::NoSheet => (
            StatusCode::BAD_REQUEST,
            "there is no such sheet in this file",
        ),
        Why::Locked => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "this file is protected against editing, so a copy cannot be changed here",
        ),
    };
    ApiError(code, text.into())
}

/// What a node said it could not show (the folder route), in the sentences
/// of this route, so a person meets the same words in both kinds of chat.
/// `folder`: the file is in a chat's folder, where the panel has no
/// download, so the two "too large" sentences send the person to Word or
/// Excel instead.
pub(super) fn refusal(r: PreviewRefusal, folder: bool) -> ApiError {
    if folder && matches!(r, PreviewRefusal::FileTooLarge | PreviewRefusal::TooLarge) {
        let code = if r == PreviewRefusal::FileTooLarge {
            StatusCode::PAYLOAD_TOO_LARGE
        } else {
            StatusCode::UNPROCESSABLE_ENTITY
        };
        return ApiError(
            code,
            "this file is too large to preview; open it in Word or Excel".into(),
        );
    }
    match r {
        PreviewRefusal::NotFound => unknown(),
        PreviewRefusal::FileTooLarge => ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "this file is too large to preview; download it to open it".into(),
        ),
        PreviewRefusal::Empty => ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "this file is empty".into(),
        ),
        PreviewRefusal::TooLarge => why(Why::TooLarge),
        PreviewRefusal::NotOffice => why(Why::NotOffice),
        PreviewRefusal::Protected => why(Why::Protected),
        PreviewRefusal::Damaged => why(Why::Damaged),
        PreviewRefusal::NoSheet => why(Why::NoSheet),
        PreviewRefusal::Locked => why(Why::Locked),
        PreviewRefusal::NoWrite => ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "this chat's node does not let a copy be saved here: it does not allow editing documents, or the folder cannot be written to".into(),
        ),
        // A word of a newer node: not understood, not a broken link.
        PreviewRefusal::Unknown => ApiError(
            StatusCode::BAD_GATEWAY,
            "the node's answer was not understood".into(),
        ),
        PreviewRefusal::Busy => ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "the node is busy reading other files; try again in a minute".into(),
        ),
        PreviewRefusal::NotAvailable => ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "this chat's node cannot read documents safely: its reader needs Linux 6.7 or newer with Landlock enabled".into(),
        ),
    }
}
