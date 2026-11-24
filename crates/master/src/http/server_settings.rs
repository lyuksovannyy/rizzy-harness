//! What the server tells every signed-in page about itself, and the admin's
//! server-wide settings that have no area of their own: notices, the
//! feature flags, token savings and the graphify status.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

use super::agent_edits::{ScopeQuery, settings_scope};
use super::caller::{Admin, User};
use super::search::kinds_shown;
use super::{ApiError, ApiResult, WorkspaceQuery, runtime};
use crate::Master;
use crate::workspaces as ws;

pub(super) fn routes() -> Router<Master> {
    Router::new()
        .route("/api/notices", get(notices))
        .route("/api/features", get(features))
        .route("/api/admin/savings", get(get_savings).put(put_savings))
        .route("/api/admin/graphify", get(get_graphify))
}

async fn notices(
    User(who): User,
    State(m): State<Master>,
    Query(q): Query<ScopeQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let scope = settings_scope(&m, &who, &q).await?;
    Ok(Json(
        json!({ "notices": crate::notices::for_viewer(&m, &who, scope.as_ref()).await }),
    ))
}

/// What optional services this server has, and what the caller may use in
/// a workspace (the sidebar shows an entry only when its `show` is true).
async fn features(
    User(who): User,
    State(m): State<Master>,
    Query(q): Query<WorkspaceQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let scope = ws::scope(&m, &who, q.workspace.as_deref()).await?;
    let scope = scope.as_ref();
    let routines = crate::routines::features(&m, &who, scope).await;
    let media = crate::media::features(&m, &who, scope).await;
    let artifacts = crate::artifacts::features(&m, &who, scope).await;
    let kinds = kinds_shown(&media, &artifacts);
    Ok(Json(json!({
        "voice": m.transcriber().is_some(),
        "memory": m.memory().is_some(),
        "routines": routines,
        "media": media,
        "artifacts": artifacts,
        "search": { "kinds": kinds },
    })))
}

// ---- token savings -------------------------------------------------------

async fn get_savings(
    _: Admin,
    State(m): State<Master>,
) -> ApiResult<Json<harness_core::savings::TokenSavings>> {
    Ok(Json(runtime(&m)?.config().savings))
}

/// Replace the token-saving settings; the next turn uses them.
async fn put_savings(
    _: Admin,
    State(m): State<Master>,
    Json(b): Json<harness_core::savings::TokenSavings>,
) -> ApiResult<Json<harness_core::savings::TokenSavings>> {
    let rt = runtime(&m)?;
    let _edits = rt.lock_edits().await;
    let mut cfg = rt.config();
    cfg.savings = b;
    rt.save(cfg.clone())
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(cfg.savings))
}

/// Whether graphify is installed and allowed on the built-in node, and what to
/// do on a node to turn it on. Read-only: node.toml is the node's, so no HTTP
/// call installs graphify or edits it (FIX_PLAN 1.2, PLAN T5).
async fn get_graphify(_: Admin, State(m): State<Master>) -> ApiResult<Json<serde_json::Value>> {
    let local = m.graphify();
    Ok(Json(json!({
        "local_node": local.is_some(),
        "allowed": local.as_ref().is_some_and(|(s, _)| s.allowed()),
        "node_toml": local.as_ref().map(|(_, p)| p.display().to_string()),
        "installed": harness_tools::graphify::installed_version().await,
        "pinned_version": harness_tools::graphify::VERSION,
        "steps": [
            "rizzy-harness graphify install",
            "graphify = true    # add to node.toml, then restart the node",
        ],
    })))
}
