//! The chat's git branch and changed-file count for its header
//! (ADOPTION_PLAN X.64, PLAN.md T19): `GET /api/sessions/{id}/git`. Owner
//! only, through the same check as opening the chat: anyone else, admins and
//! workspace admins included, gets the 404 a chat that does not exist gets
//! (T10), and the node is not asked. So does a plain chat (it has no folder).
//! A temporary coding chat is served like any other. A read: no agentic
//! grant is needed, as for the chat's diff.
//!
//! The master asks the owning node once (`crate::git_state`) and passes on
//! what it sends, bounded again; it reads no folder and runs nothing. The
//! answer is `null` for everything the header shows nothing for: a folder
//! that is no repository the node reads, a node that is away or too old or
//! slow, a node that denies `read`. That is on purpose: the chip is a hint,
//! so no case of it is an error the page must word. Only "too many asks
//! already" is a 503, and the page keeps the chip it has.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use harness_protocol::GitState;

use super::caller::User;
use super::{ApiError, ApiResult, visible_session};
use crate::Master;
use crate::git_state::GitError;

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/sessions/{id}/git", get(git))
}

async fn git(
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
) -> ApiResult<Json<Option<GitState>>> {
    let row = visible_session(&m, &who, &id).await?;
    if row.profile.as_deref() == Some("chat") {
        return Err(ApiError(StatusCode::NOT_FOUND, "unknown session".into()));
    }
    let user = who.user_id.clone().unwrap_or_else(|| "(api token)".into());
    match crate::git_state::ask(&m, &user, row.session_id).await {
        Ok(s) => Ok(Json(s)),
        Err(GitError::Busy) => Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "the branch is being read already; try again in a moment".into(),
        )),
        Err(GitError::Unknown) => Err(ApiError(StatusCode::NOT_FOUND, "unknown session".into())),
        Err(GitError::Away | GitError::TooOld | GitError::Timeout) => Ok(Json(None)),
    }
}
