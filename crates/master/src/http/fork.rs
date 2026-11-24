//! A person's fork of a chat at the end of a completed turn (ADOPTION_PLAN
//! C.2, docs/fork.md, README "Fork a chat"):
//! `POST /api/sessions/{id}/fork` with `{turn_id}`; the answer is the new
//! chat's id, which the page opens as it opens a new chat.
//!
//! Only the chat's owner (or the API token, for a chat nobody owns): anyone
//! else, admins and workspace admins and the token on an owned chat
//! included, gets the 404 of a chat that does not exist (T10). The same
//! gates as for a prompt: a settled password and the owner's level for
//! agentic work (the fork can run commands in the same folder).
//!
//! The master answers what it can from its mirror
//! (`harness_core::fork::check`, the node's own rule), refuses a
//! sub-agent's chat and a routine's run, and sends `session.fork` to the
//! chat's node, which decides again against its own log and policy and
//! writes the new chat. Nothing a model, a tool or a routine sends reaches
//! this route: a fork is a person's click, never automatic.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use super::caller::User;
use super::{ApiError, ApiResult, agentic_gate};
use crate::{CommandError, Master};

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/sessions/{id}/fork", post(fork))
}

fn refused(why: impl std::fmt::Display) -> ApiError {
    ApiError(
        StatusCode::CONFLICT,
        format!("{}: {why}", harness_protocol::FORK_REFUSED),
    )
}

#[derive(Deserialize)]
struct Body {
    /// The turn whose end the new chat's copy stops at.
    turn_id: String,
}

async fn fork(
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
    Json(b): Json<Body>,
) -> ApiResult<Json<Value>> {
    let row = super::rewind::owned(&m, &who, &id).await?;
    agentic_gate(&m, &who, &row).await?;
    if row.parent_id.is_some() {
        return Err(refused(
            "a sub-agent's chat belongs to the agent that delegated it",
        ));
    }
    if !m.node_has(&row.node_id, harness_protocol::CAP_FORK) {
        return Err(CommandError::Rejected(
            "this chat's node cannot fork a chat (a node older than this server, or one that is \
             away)"
                .into(),
        )
        .into());
    }
    let sid = row.session_id;
    // A frozen mirror is not the chat: its events stop where the two
    // writers parted.
    if m.inner
        .mirror
        .call(move |s| s.diverged_seq(sid))
        .await?
        .is_some()
    {
        return Err(refused("this chat's log diverged; it cannot be copied"));
    }
    let log = m
        .inner
        .mirror
        .call(move |s| s.events_since_turn_at(sid, i64::MIN, harness_core::fork::KINDS))
        .await?;
    harness_core::fork::check(&log, &b.turn_id).map_err(refused)?;
    // A routine's run: its prompt is the routine's, not a conversation to
    // take elsewhere.
    if m.inner
        .mirror
        .call(move |s| s.run_by_session(sid))
        .await?
        .is_some()
    {
        return Err(refused(
            "a routine's own run is not forked; run the routine again",
        ));
    }
    tracing::info!(session = %sid, "a person forks a chat at a finished turn");
    let new = crate::forks::fork_session(&m, &row, &b.turn_id).await?;
    Ok(Json(json!({ "session_id": new })))
}
