//! A person's Retry of the chat's failed last turn (ADOPTION_PLAN X.21,
//! README "Retry"): `POST /api/sessions/{id}/turns/{turn}/retry`. The same
//! checks as sending the chat a prompt: its owner (or the API token) only,
//! anyone else, admins and workspace admins included, gets the 404 of a
//! chat that does not exist (T10); a settled password and the owner's
//! level for agentic work; the chat's current agent, model, settings and
//! context, resolved as for a prompt.
//!
//! The master answers what it can from its mirror (only the last turn, a
//! person's, ended `error`, `interrupted` or `stopped`, nothing after it:
//! `harness_core::retry::check`, the node's own rule), refuses a sub-agent's
//! chat and a routine's own run (the routine retries its runs by its own
//! rules), and relays `session.retry`; the node decides again against its
//! log under the turn slot (a turn or `/compact` running, a second click).
//! Nothing a model, a tool or a routine sends reaches this route: it is a
//! person's click, never automatic (the loop's own `model_retry` is).

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{Value, json};

use super::{ApiError, ApiResult, agentic_gate, require, visible_session};
use crate::{CommandError, Master};

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/sessions/{id}/turns/{turn}/retry", post(retry))
}

/// What the check and the message lookup read of the log.
const KINDS: &[&str] = &[
    "turn_started",
    "turn_ended",
    "user_message",
    "queued_delivered",
    "error",
    "checkpoint",
];

fn refused(why: impl std::fmt::Display) -> ApiError {
    ApiError(
        StatusCode::CONFLICT,
        format!("{}: {why}", harness_protocol::RETRY_REFUSED),
    )
}

async fn retry(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, turn)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let row = visible_session(&m, &who, &id).await?;
    agentic_gate(&m, &who, &row).await?;
    if row.parent_id.is_some() {
        return Err(refused(
            "a sub-agent's turn is retried by the agent that delegated it",
        ));
    }
    if !m.node_has(&row.node_id, harness_protocol::CAP_RETRY) {
        return Err(CommandError::Rejected(
            "this chat's node cannot retry a turn (a node older than this server, or one that \
             is away)"
                .into(),
        )
        .into());
    }
    let sid = row.session_id;
    // The last turn's events: what the node's own rule reads.
    let last = m
        .inner
        .mirror
        .call(move |s| s.events_since_turn_at(sid, i64::MAX, KINDS))
        .await?;
    harness_core::retry::check(&last, &turn).map_err(refused)?;
    // A routine's run is its chat's first turn: the routine retries it by
    // its own rules (a node away), the owner by running the routine again.
    let run = m.inner.mirror.call(move |s| s.run_by_session(sid)).await?;
    if run.is_some() {
        let first = m
            .inner
            .mirror
            .call(move |s| s.first_event(sid, "turn_started"))
            .await?;
        if first.and_then(|f| {
            f.get("turn_id")
                .and_then(|t| t.as_str())
                .map(str::to_string)
        }) == Some(turn.clone())
        {
            return Err(refused(
                "it is the routine's own run; run the routine again instead",
            ));
        }
    }
    // The message's text, for what the turn looks up by it (recall,
    // project facts) and a leading `@agent`: never sent, the node has it.
    let text = match harness_core::retry::message(&last, &turn) {
        Some(t) => t,
        None => {
            let all = m
                .inner
                .mirror
                .call(move |s| {
                    s.events_since_turn_at(
                        sid,
                        i64::MIN,
                        &["turn_started", "user_message", "queued_delivered"],
                    )
                })
                .await?;
            harness_core::retry::message(&all, &turn).unwrap_or_default()
        }
    };
    // A chat a routine started from an external event stays untrusted for
    // every turn the owner runs there, a retry too.
    let flags = crate::routines::on_owner_prompt(&m, sid).await;
    tracing::info!(session = %sid, turn = %turn, "a person retries a failed turn");
    m.prompt(
        sid,
        text,
        crate::PromptOpts {
            context: Some(
                crate::prefs::Prefs::load(&m, who.user_id.as_deref())
                    .await
                    .model_context(
                        &who.username,
                        jiff::Timestamp::now(),
                        row.profile.as_deref() == Some("chat"),
                    ),
            ),
            untrusted: flags.untrusted,
            by_owner: who.user_id.is_some() && who.user_id == row.owner_id,
            retry: Some(turn),
            ..Default::default()
        },
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}
