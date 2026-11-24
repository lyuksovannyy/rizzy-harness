//! Where a chat's push goes, as the master knows it (ADOPTION_PLAN X.27
//! step 4, PLAN.md T101, review F2): `GET /api/sessions/{id}/push-target`.
//! The page draws a push card's destination from this, never from the
//! node's text: the host and `owner/name` of the project's recorded source
//! and the login of the owner's connection that the master named for the
//! chat's latest turn. Owner only: anyone else, admins included, gets the
//! 404 a chat that does not exist gets (T10). 404 too when the chat's
//! latest turn named no target. A read; nothing secret.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};

use super::{ApiError, ApiResult, require, visible_session};
use crate::Master;

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/sessions/{id}/push-target", get(target))
}

async fn target(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let row = visible_session(&m, &who, &id).await?;
    let none = || ApiError(StatusCode::NOT_FOUND, "no push target".into());
    if who.user_id.is_none() || row.owner_id != who.user_id {
        return Err(ApiError(StatusCode::NOT_FOUND, "unknown session".into()));
    }
    let t = m.inner.pushes.offer_of(row.session_id).ok_or_else(none)?;
    Ok(Json(
        json!({"host": t.host, "full_name": t.full_name, "login": t.login}),
    ))
}
