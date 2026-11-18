//! A project folder's own MCP servers (ADOPTION_PLAN X.36, PLAN.md T82):
//! the project page's "MCP servers from the repository". Each call asks
//! the project's node (`crate::project_mcp`), which reads the folder's
//! `.mcp.json` once and answers with the list for the caller:
//!
//! * `GET /api/projects/{id}/mcp`: the list (opening it, and Reload);
//! * `POST /api/projects/{id}/mcp/approve` `{ name, digest }`: approve the
//!   file's entry if it still has the digest the caller was shown;
//! * `POST /api/projects/{id}/mcp/remove` `{ name }`: remove the caller's
//!   approval.
//!
//! An approval is the caller's own (their user id), for their own chats in
//! the project's folder. Who may: a member of the project's workspace who
//! may run agent chats on its node (the rule of a project's folder,
//! `folder::may_make_on`); the API token has no approvals.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use harness_protocol::{ProjectMcpList, ProjectMcpRequest, ProjectMcpState};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, bad, visible};
use crate::Master;
use crate::project_mcp::{self, McpError};
use crate::projects::ProjectSettings;

pub(super) fn routes() -> Router<Master> {
    Router::new()
        .route("/api/projects/{id}/mcp", get(list))
        .route("/api/projects/{id}/mcp/approve", post(approve))
        .route("/api/projects/{id}/mcp/remove", post(remove))
}

/// Who asks, the project's node and folder, once the rules allow it.
struct Asker {
    user: String,
    project: String,
    node: String,
    folder: String,
}

async fn asker(m: &Master, headers: &HeaderMap, id: &str) -> ApiResult<Asker> {
    let (who, row, a) = visible(m, headers, id).await?;
    let Some(user) = who.user_id.clone() else {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "a repository's MCP servers are approved by a person, for their own chats; the API \
             token has none"
                .into(),
        ));
    };
    let settings = ProjectSettings::read(&row.settings);
    let (Some(node), Some(folder)) = (settings.node_id, settings.cwd) else {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "this project has no folder on a node, so it has no .mcp.json".into(),
        ));
    };
    super::folder::may_make_on(m, &who, &a, &node)?;
    Ok(Asker {
        user,
        project: row.project_id,
        node,
        folder,
    })
}

async fn ask(m: &Master, who: &Asker, req: ProjectMcpRequest) -> ApiResult<ProjectMcpList> {
    project_mcp::ask(m, &who.user, &who.node, &who.folder, req)
        .await
        .map_err(|e| {
            let code = match e {
                McpError::Away(_) | McpError::Timeout(_) => StatusCode::SERVICE_UNAVAILABLE,
                McpError::Busy => StatusCode::TOO_MANY_REQUESTS,
                McpError::NotOffered(_) | McpError::Refused(_) => StatusCode::CONFLICT,
            };
            ApiError(code, e.to_string())
        })
}

fn answer(who: &Asker, l: &ProjectMcpList) -> Json<Value> {
    let mut v = serde_json::to_value(l).unwrap_or_else(|_| json!({}));
    v["node_id"] = json!(who.node);
    Json(v)
}

/// One audit line: who, which project, node and server, and the outcome;
/// never the entry's command or environment.
async fn audit(m: &Master, kind: &'static str, who: &Asker, server: &str, outcome: &str) {
    let detail = json!({
        "user": who.user,
        "project": who.project,
        "node": who.node,
        "server": server,
        "outcome": outcome,
    })
    .to_string();
    if let Err(e) = m.inner.mirror.call(move |s| s.audit(kind, &detail)).await {
        tracing::warn!(error = %e, kind, "could not write a project MCP audit line");
    }
}

async fn list(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let who = asker(&m, &headers, &id).await?;
    let l = ask(&m, &who, ProjectMcpRequest::List).await?;
    Ok(answer(&who, &l))
}

fn check_name(name: &str) -> ApiResult<()> {
    if harness_core::agent::valid_mcp_server_name(name) {
        Ok(())
    } else {
        Err(bad("not a server name this node could have approved"))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApproveBody {
    name: String,
    digest: String,
}

async fn approve(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<ApproveBody>,
) -> ApiResult<Json<Value>> {
    check_name(&b.name)?;
    if b.digest.len() != 64 || !b.digest.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(bad("digest: the 64 hex characters the list gave"));
    }
    let who = asker(&m, &headers, &id).await?;
    let l = ask(
        &m,
        &who,
        ProjectMcpRequest::Approve {
            name: b.name.clone(),
            digest: b.digest,
        },
    )
    .await?;
    let taken = l.note.is_none()
        && l.entries
            .iter()
            .any(|e| e.name == b.name && e.state == ProjectMcpState::Approved);
    let outcome = if taken { "approved" } else { "refused" };
    audit(&m, "project_mcp.approve", &who, &b.name, outcome).await;
    Ok(answer(&who, &l))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoveBody {
    name: String,
}

async fn remove(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<RemoveBody>,
) -> ApiResult<Json<Value>> {
    check_name(&b.name)?;
    let who = asker(&m, &headers, &id).await?;
    let l = ask(
        &m,
        &who,
        ProjectMcpRequest::Remove {
            name: b.name.clone(),
        },
    )
    .await?;
    audit(&m, "project_mcp.remove", &who, &b.name, "removed").await;
    Ok(answer(&who, &l))
}
