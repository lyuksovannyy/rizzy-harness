//! `GET /api/sessions/{id}/made/{file_id}`: the Word or Excel file an agent
//! made in a chat without a folder (PLAN.md T97). Owner only, through the
//! same check as opening the chat (anyone else, admins included, gets the
//! 404 a chat that does not exist gets, T10); the file must be one the
//! chat's own log names (`session_files`, `logged`), and the blob must be
//! the chat owner's. Sent as an attachment of unknown type
//! (`blobs::serve` with `inline: false`): never rendered by the browser.

use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::get;

use super::caller::ByteCaller;
use super::{ApiError, ApiResult, visible_session};
use crate::{Master, storage};

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/sessions/{id}/made/{file_id}", get(download))
}

fn unknown() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "unknown file".into())
}

/// The caller: a download button's link (`POST /api/links`) or the
/// login's own key.
async fn download(
    ByteCaller(who): ByteCaller,
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, file_id)): Path<(String, String)>,
) -> ApiResult<Response> {
    let row = visible_session(&m, &who, &id).await?;
    // Only a plain chat keeps made files: a coding chat's are in its folder.
    if row.profile.as_deref() != Some("chat") || row.parent_id.is_some() || row.incognito {
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
    let blob_id = file.blob_id.clone();
    let blob = m
        .inner
        .mirror
        .call(move |s| s.blob(&blob_id))
        .await?
        .ok_or_else(unknown)?;
    let st = storage::get(&m).ok_or_else(unknown)?;
    let path = st.path(&file.blob_id).ok_or_else(unknown)?;
    let bid = file.blob_id.clone();
    let key = m
        .inner
        .mirror
        .call(move |s| s.blob_key(&bid))
        .await?
        .ok_or_else(unknown)?;
    Ok(crate::blobs::serve(
        &headers,
        crate::blobs::Serve {
            blob: &blob,
            path,
            key,
            name: &file.name,
            inline: false,
        },
    )
    .await)
}
