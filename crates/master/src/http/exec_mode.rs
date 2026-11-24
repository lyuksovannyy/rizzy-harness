//! The switch of an existing chat's execution mode (N.10b, README
//! "Execution modes"): `POST /api/sessions/{id}/exec-mode` with
//! `{ "mode": "sandbox" | "host" }`.
//!
//! Only the chat's OWNER: anyone else, admins, workspace admins and the API
//! token on a person's chat included, gets the 404 a chat that does not
//! exist gets (T10); a stricter rule than a prompt's, because the mode
//! decides whether the chat's commands are contained. A settled password,
//! as for every action that changes a chat. The rest is
//! `Master::set_exec_mode` (the checks of a create for the target mode, in
//! the same order) and the node's own decision (what it offers, nothing of
//! the chat running). Every request for `host` is audited, whatever refuses
//! it, as a host create is; a granted switch to `sandbox` too.

use axum::extract::{Path, State};

use axum::routing::post;
use axum::{Json, Router};
use harness_core::policy::ExecMode;
use serde::Deserialize;
use serde_json::{Value, json};

use super::ApiResult;
use super::caller::User;
use super::rewind::owned;
use crate::Master;

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/sessions/{id}/exec-mode", post(set_mode))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModeBody {
    mode: ExecMode,
}

async fn set_mode(
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
    Json(b): Json<ModeBody>,
) -> ApiResult<Json<Value>> {
    let row = owned(&m, &who, &id).await?;
    tracing::info!(session = %row.session_id, mode = b.mode.id(), "the owner switches a chat's commands");
    let out = m.set_exec_mode(&row, b.mode).await;
    if b.mode == ExecMode::Host || out.is_ok() {
        audit(
            &m,
            &who,
            &row,
            b.mode,
            out.as_ref().err().map(|e| e.to_string()),
        )
        .await;
    }
    out?;
    Ok(Json(json!({ "ok": true })))
}

/// One audit line: who, the chat, its workspace and node, from and to, and
/// the outcome ("granted", or "refused: " and the refusal's first words). No
/// chat content.
async fn audit(
    m: &Master,
    who: &crate::auth::Principal,
    row: &harness_store::SessionRow,
    to: ExecMode,
    refusal: Option<String>,
) {
    let outcome = match refusal {
        None => "granted".to_string(),
        Some(why) => format!("refused: {}", why.chars().take(80).collect::<String>()),
    };
    let detail = json!({
        "user": who.user_id.clone().unwrap_or_else(|| "api token".into()),
        "session": row.session_id.to_string(),
        "workspace": row.workspace_id,
        "node": row.node_id,
        "from": row.exec_mode,
        "to": to.id(),
        "outcome": outcome,
    })
    .to_string();
    let kind = match to {
        ExecMode::Host => "session.exec_mode.host",
        ExecMode::Sandbox => "session.exec_mode.sandbox",
    };
    if let Err(e) = m.inner.mirror.call(move |s| s.audit(kind, &detail)).await {
        tracing::warn!(error = %e, "could not write the execution-mode audit line");
    }
}
