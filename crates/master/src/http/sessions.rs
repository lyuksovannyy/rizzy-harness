//! Renaming a chat. Owner only, as every per-chat route: anyone else,
//! admins included, gets the 404 a chat that does not exist gets (T10).
//!
//! The title the lists show is the master's row (`sessions.title`), filled
//! from `session_created` and read by every list, search hit and page; the
//! node and the model never read it. So a rename is a master-side update of
//! that row and its search entry, and appends no event: the log keeps the
//! title the chat was created with, and its one writer stays the node.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::put;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, require, visible_session};
use crate::{Live, Master};

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/sessions/{id}/title", put(rename))
}

#[derive(Deserialize)]
struct RenameBody {
    title: String,
}

/// Control and hidden characters are dropped, surrounding blanks trimmed;
/// empty or over `MAX_TITLE_CHARS` characters is refused.
async fn rename(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<RenameBody>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let sid = visible_session(&m, &who, &id).await?.session_id;
    let title = crate::projects::clean_required("title", &b.title, harness_store::MAX_TITLE_CHARS)
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))?;
    let t2 = title.clone();
    if !m
        .inner
        .mirror
        .call(move |s| s.rename_session(sid, &t2))
        .await?
    {
        return Err(ApiError(StatusCode::NOT_FOUND, "unknown session".into()));
    }
    let _ = m.inner.live.send(Live::SessionsChanged);
    Ok(Json(json!({ "ok": true, "title": title })))
}
