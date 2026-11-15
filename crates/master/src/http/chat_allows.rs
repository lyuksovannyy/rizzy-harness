//! The person's removal of a call they allowed for the rest of a chat
//! ("Allow this exact call for this chat"). Owner only, as every per-chat
//! route: anyone else, admins and workspace admins included, gets the 404
//! a chat that does not exist gets (T10). Like a denial it takes power
//! away, so it needs no settled password and no agentic grant (Gate 3
//! review P-7). The node records `chat_allow_removed` or refuses an id it
//! does not hold.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::delete;
use axum::{Json, Router};
use serde_json::{Value, json};

use super::{ApiError, ApiResult, require_any, visible_session};
use crate::Master;

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/sessions/{id}/allows/{grant}", delete(revoke))
}

/// A grant id as the node makes them (`new_id()`: hex).
fn is_grant_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

async fn revoke(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, grant)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let who = require_any(&m, &headers).await?;
    let row = visible_session(&m, &who, &id).await?;
    if !is_grant_id(&grant) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "bad grant id".into()));
    }
    m.revoke_allow(row.session_id, grant).await?;
    Ok(Json(json!({ "ok": true })))
}
