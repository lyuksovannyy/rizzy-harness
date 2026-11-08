//! Storage usage and settings over HTTP (docs/media.md): each account's own
//! meter, and the server admin's numbers and limits. Numbers only: no file
//! names or contents ever reach the admin.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};

use super::{ApiError, ApiResult, require, require_admin, runtime};
use crate::Master;
use crate::storage::{self, StorageConfig};

/// Merged into the API router by `http::router`.
pub(super) fn routes() -> Router<Master> {
    Router::new()
        .route("/api/me/storage", get(mine))
        .route("/api/admin/storage", get(admin).put(put_settings))
}

/// The caller's usage across Media and Artifacts. `quota` is 0 when
/// unlimited (the API token, or `account_quota_mb = 0`).
async fn mine(State(m): State<Master>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let owner = who.user_id.clone();
    let u = m
        .inner
        .mirror
        .call(move |s| s.blob_usage(owner.as_deref()))
        .await?;
    let quota = storage::config(&m).limits(who.user_id.as_deref()).account;
    Ok(Json(json!({
        "used": u.used,
        "quota": quota,
        "media": { "bytes": u.media_bytes, "items": u.media_items },
        "artifacts": { "bytes": u.artifact_bytes, "versions": u.artifact_versions },
    })))
}

fn settings_json(c: &StorageConfig) -> Value {
    json!({
        "max_file_mb": c.max_file_mb,
        "account_quota_mb": c.account_quota_mb,
        "server_quota_mb": c.server_quota_mb,
        "min_free_mb": c.min_free_mb,
    })
}

async fn admin(State(m): State<Master>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    require_admin(&m, &headers).await?;
    let (rows, total) = m.inner.mirror.call(|s| s.storage_usage_all()).await?;
    let disk = storage::disk(&m);
    Ok(Json(json!({
        "settings": settings_json(&storage::config(&m)),
        "usage": rows.iter().map(|r| json!({
            "user_id": r.user_id,
            "username": r.username,
            "used": r.used,
            "media_items": r.media_items,
            "artifact_versions": r.artifact_versions,
        })).collect::<Vec<_>>(),
        "total": total,
        "disk": {
            "free": disk.map(|d| d.0),
            "total": disk.map(|d| d.1),
        },
    })))
}

/// Replace the limits; the next upload or publish uses them. Lowering a
/// quota never deletes anything: files over it stay, new ones are refused.
async fn put_settings(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<Value>,
) -> ApiResult<Json<Value>> {
    require_admin(&m, &headers).await?;
    let bad = |e: String| ApiError(StatusCode::BAD_REQUEST, e);
    let b: StorageConfig = serde_json::from_value(b).map_err(|e| bad(e.to_string()))?;
    b.check().map_err(bad)?;
    let rt = runtime(&m)?;
    let _edits = rt.lock_edits().await;
    let mut cfg = rt.config();
    cfg.storage = b;
    rt.save(cfg.clone())
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(settings_json(&cfg.storage)))
}
