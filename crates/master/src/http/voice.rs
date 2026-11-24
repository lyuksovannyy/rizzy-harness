//! Dictation: the audio upload that comes back as text (the quota and the
//! provider call are `crate::voice`). Voice routes go here.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::json;

use super::caller::User;
use super::{ApiError, ApiResult};
use crate::Master;

pub(super) fn routes() -> Router<Master> {
    Router::new().route(
        "/api/transcribe",
        post(transcribe).layer(axum::extract::DefaultBodyLimit::max(
            crate::voice::MAX_AUDIO,
        )),
    )
}

async fn transcribe(
    User(who): User,
    State(m): State<Master>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> ApiResult<Json<serde_json::Value>> {
    let t = m.transcriber().ok_or_else(|| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "voice is not set up on this server".into(),
        )
    })?;
    if body.is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "no audio".into()));
    }
    // Held until the answer (or the dropped request) ends.
    let _slot = m
        .voice_usage()
        .begin(
            who.user_id.as_deref().unwrap_or(""),
            body.len() as u64,
            t.limits(),
            harness_core::now_ms(),
        )
        .map_err(|e| ApiError(StatusCode::TOO_MANY_REQUESTS, e.to_string()))?;
    let mime = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .filter(|v| v.starts_with("audio/"))
        .unwrap_or("audio/webm")
        .to_string();
    let text = t
        .transcribe(body.to_vec(), &mime)
        .await
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(json!({ "text": text })))
}
