//! The skills of a chat for the composer's `/` menu (ADOPTION_PLAN X.65,
//! PLAN.md T46): `GET /api/sessions/{id}/skills-menu` answers
//! `{"skills": [{"name", "description"}]}`. Owner only, through the same
//! check as opening the chat: anyone else, admins and workspace admins
//! included, gets the 404 a chat that does not exist gets (T10), and the
//! node is not asked. Plain chats are served too (the master's stored skills
//! for the chat's agent), and so is a temporary chat. A read: no agentic
//! grant is needed.
//!
//! The master asks the owning node once (`crate::skills_menu`) and passes on
//! what it sends, bounded again; it reads no folder and runs nothing, and
//! the page's choosing an entry only completes text: no skill's text is read
//! for the menu. The list is empty for everything the menu shows nothing
//! more for: a node that is away, too old or slow, an agent without the
//! skill tool, a node that denies it. That is on purpose: the built-in
//! commands are always there, so no case of it is an error the page must
//! word. Only "too many asks already" is a 503, and the page keeps the menu
//! it has.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};

use super::{ApiError, ApiResult, require, visible_session};
use crate::Master;
use crate::skills_menu::MenuError;

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/sessions/{id}/skills-menu", get(menu))
}

async fn menu(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let row = visible_session(&m, &who, &id).await?;
    let user = who.user_id.clone().unwrap_or_else(|| "(api token)".into());
    match crate::skills_menu::ask(&m, &user, &row).await {
        Ok(skills) => Ok(Json(json!({ "skills": skills }))),
        Err(MenuError::Busy) => Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "the skills are being read already; try again in a moment".into(),
        )),
        Err(MenuError::Unknown) => Err(ApiError(StatusCode::NOT_FOUND, "unknown session".into())),
        Err(MenuError::Away | MenuError::TooOld | MenuError::Timeout | MenuError::NoAgent) => {
            Ok(Json(json!({ "skills": [] })))
        }
    }
}
