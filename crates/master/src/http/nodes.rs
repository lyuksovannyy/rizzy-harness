//! The nodes: the list a caller may pick from, and the admin's list, join
//! token and revocation. New node routes of the master's HTTP API go here.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::json;

use super::caller::{Admin, User};
use super::{ApiError, ApiResult};
use crate::Master;
use crate::workspaces as ws;

pub(super) fn routes() -> Router<Master> {
    Router::new()
        .route("/api/nodes", get(nodes))
        .route("/api/admin/nodes", get(admin_nodes))
        .route("/api/admin/nodes/join", post(node_join_token))
        .route("/api/admin/nodes/{id}", delete(revoke_node))
}

/// The nodes the caller may pick from: without the right to use the
/// embedded agentic node (`[nodes] local_node_users`), it is left out.
async fn nodes(User(who): User, State(m): State<Master>) -> ApiResult<Json<serde_json::Value>> {
    let local_ok = ws::may_use_local_node(&m, &who);
    let nodes: Vec<_> = m
        .nodes()
        .into_iter()
        .filter(|n| local_ok || !m.is_local_node(&n.node_id))
        // What a node can contain says how to get out of it: only for those
        // whose account may run agent chats at all (N.10a).
        .map(|mut n| {
            if who.exec_ceiling < harness_core::policy::ExecLevel::Sandbox {
                (n.exec_modes, n.exec_gaps) = Default::default();
                n.exec_switch = false;
            }
            n
        })
        .collect();
    Ok(Json(json!({ "nodes": nodes })))
}

async fn admin_nodes(_: Admin, State(m): State<Master>) -> ApiResult<Json<serde_json::Value>> {
    let listener = m.node_listener();
    Ok(Json(json!({
        "nodes": m.registered_nodes().await?,
        "listener": listener.map(|(port, fp)| json!({ "port": port, "fingerprint": fp })),
    })))
}

/// A join token plus everything the node needs, except the host name the
/// node should dial (the browser knows that better than the server).
async fn node_join_token(
    Admin(who): Admin,
    State(m): State<Master>,
) -> ApiResult<Json<serde_json::Value>> {
    let Some((port, fingerprint)) = m.node_listener() else {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "remote nodes are off: start the server with --node-listen 0.0.0.0:7443".into(),
        ));
    };
    let token = m.create_join_token(who.user_id.clone()).await?;
    Ok(Json(json!({
        "token": token,
        "port": port,
        "fingerprint": fingerprint,
        "expires_in_s": crate::nodes::JOIN_TOKEN_TTL.as_secs(),
    })))
}

async fn revoke_node(
    _: Admin,
    State(m): State<Master>,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    if !m.revoke_node(&id).await? {
        return Err(ApiError(StatusCode::NOT_FOUND, "no such node".into()));
    }
    crate::artifacts::on_node_revoked(&m, &id).await;
    Ok(Json(json!({ "ok": true })))
}
