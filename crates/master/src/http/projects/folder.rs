//! A project's folder on a node (ADOPTION_PLAN X.26, PLAN.md T77): "New
//! project" in the start screen's Project popup, and "Create its folder on
//! <node>" for a project that has none. The master asks the node with the
//! project's name and nothing else (`crate::project_folder`); the node
//! makes the folder in the projects folder its node.toml names and answers
//! with the path, which becomes the project's `node_id` and `cwd` settings
//! (the defaults every chat started in the project takes).

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use harness_store::ProjectPatch;
use serde::Deserialize;
use serde_json::Value;

use super::{ApiError, ApiResult, bad, changed, detail_json, managed, member_of, not_found};
use crate::auth::Principal;
use crate::project_folder::{self, FolderError};
use crate::projects::ProjectSettings;
use crate::workspaces::{self as ws, Access};
use crate::{Audience, Master};

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/projects/{id}/folder", post(give_folder))
}

/// Whether `who`, in workspace `a`, may have a folder made on `node`: they
/// may run agent chats there at all (their level is at least `sandbox`),
/// they may use that node (the embedded node is for admins by default),
/// and it is a worker node the master knows. The same rules as starting an
/// agentic chat there (`create_checked`).
pub(super) fn may_make_on(m: &Master, who: &Principal, a: &Access, node: &str) -> ApiResult<()> {
    if let Some(why) = crate::levels::no_work(who, Some(a)) {
        return Err(ApiError(StatusCode::FORBIDDEN, why));
    }
    if let Some(why) = ws::node_refusal(m, who, node) {
        return Err(ApiError(StatusCode::FORBIDDEN, why));
    }
    match m.nodes().into_iter().find(|n| n.node_id == node) {
        Some(n) if n.profile == harness_core::event::Profile::Agentic => Ok(()),
        Some(_) => Err(bad(format!("node {node} runs no agent work"))),
        None => Err(not_found("node")),
    }
}

/// Ask `node` for a new folder named after `name`; the settings that record
/// it. Call after [`may_make_on`].
pub(super) async fn make_on(
    m: &Master,
    who: &Principal,
    node: &str,
    name: &str,
    mut settings: ProjectSettings,
) -> ApiResult<ProjectSettings> {
    let user = who.user_id.as_deref().unwrap_or("api token");
    let path = project_folder::make(m, user, node, name)
        .await
        .map_err(|e| {
            let code = match e {
                FolderError::Away(_) | FolderError::Timeout(_) => StatusCode::SERVICE_UNAVAILABLE,
                FolderError::Busy => StatusCode::TOO_MANY_REQUESTS,
                FolderError::NotOffered(_) | FolderError::Refused(_) => StatusCode::CONFLICT,
            };
            ApiError(code, e.to_string())
        })?;
    settings.node_id = Some(node.to_string());
    settings.cwd = Some(path);
    Ok(settings)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GiveFolder {
    node_id: String,
}

/// `POST /api/projects/{id}/folder` `{ node_id }`: a project without a
/// folder gets a new one on that node. Managers only, as every change to a
/// project; refused when it has a folder already.
async fn give_folder(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<GiveFolder>,
) -> ApiResult<Json<Value>> {
    let (who, row) = managed(&m, &headers, &id).await?;
    let a = member_of(&m, &who, &row.workspace_id, "project").await?;
    let settings = ProjectSettings::read(&row.settings);
    if settings.cwd.is_some() {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "this project has a folder already; change it in the project's settings".into(),
        ));
    }
    may_make_on(&m, &who, &a, &b.node_id)?;
    let settings = make_on(&m, &who, &b.node_id, &row.name, settings).await?;
    let patch = ProjectPatch {
        settings: Some(
            serde_json::to_string(&settings)
                .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
        ),
        ..Default::default()
    };
    let pid = row.project_id.clone();
    let row = m
        .inner
        .mirror
        .call(move |s| s.update_project(&pid, &patch))
        .await?
        .ok_or_else(|| not_found("project"))?;
    changed(&m, Audience::Workspace(row.workspace_id.clone()));
    Ok(Json(detail_json(&m, &who, &a, &row).await?))
}
