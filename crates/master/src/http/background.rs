//! The person's stop of a chat's background commands (T.8d step 2,
//! docs/background-commands.md §4.7, §4.9): one command (`bg_id`) or all
//! of them. Owner only, as every per-chat route: anyone else, admins and
//! workspace admins included, gets the 404 a chat that does not exist gets
//! (T10). Like the turn's cancel it needs no settled password and no
//! agentic grant: stopping work is always allowed (Gate 3 review P-7).
//!
//! The master runs nothing. It reads what the log shows (an end already
//! recorded is answered here, the same every time, without asking the
//! node), then relays `session.stop_background { by: user }` to the owning
//! node, which alone decides: it stops the command and records
//! `background_ended { reason: stopped_by_user }`, or refuses with its
//! reason, which is passed on. The answer says what was asked of the node
//! (`stopping`); the end itself arrives as the event, as every end does.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use harness_core::RawEvent;
use harness_core::event::{BackgroundReason, EventKind};
use harness_protocol::{CAP_BACKGROUND, StopBackgroundBy};
use serde_json::{Value, json};

use super::{ApiError, ApiResult, require_any, visible_session};
use crate::{CommandError, Master};

pub(super) fn routes() -> Router<Master> {
    Router::new()
        .route("/api/sessions/{id}/background/stop", post(stop_all))
        .route("/api/sessions/{id}/background/{bg}/stop", post(stop_one))
}

async fn stop_all(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    stop(&m, &headers, &id, None).await
}

async fn stop_one(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, bg)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    stop(&m, &headers, &id, Some(bg)).await
}

/// `bg-<n>` as the node numbers them (at most nine digits).
fn is_bg_id(s: &str) -> bool {
    s.strip_prefix("bg-")
        .is_some_and(|n| !n.is_empty() && n.len() <= 9 && n.bytes().all(|b| b.is_ascii_digit()))
}

/// What the log shows: the commands started and not ended, in the order
/// they started, and the ended ones with their reason.
fn shown(events: &[RawEvent]) -> (Vec<String>, Vec<(String, BackgroundReason)>) {
    let mut running: Vec<String> = Vec::new();
    let mut ended: Vec<(String, BackgroundReason)> = Vec::new();
    for e in events {
        match e.typed() {
            Ok(EventKind::BackgroundStarted(b)) => {
                if !running.contains(&b.bg_id) && !ended.iter().any(|(id, _)| *id == b.bg_id) {
                    running.push(b.bg_id);
                }
            }
            Ok(EventKind::BackgroundEnded(b)) => {
                running.retain(|id| *id != b.bg_id);
                if !ended.iter().any(|(id, _)| *id == b.bg_id) {
                    ended.push((b.bg_id, b.reason));
                }
            }
            _ => {}
        }
    }
    (running, ended)
}

/// The node is not connected: the stop cannot reach it. Said plainly, with
/// what happens to the commands instead.
fn away(node: &str) -> ApiError {
    ApiError(
        StatusCode::SERVICE_UNAVAILABLE,
        format!(
            "the node {node} that runs this chat is away, so the stop cannot reach it; its \
             background commands end with that node or at their timeout, and the chat shows \
             the end when the node is back"
        ),
    )
}

async fn stop(
    m: &Master,
    headers: &HeaderMap,
    id: &str,
    bg: Option<String>,
) -> ApiResult<Json<Value>> {
    let who = require_any(m, headers).await?;
    let row = visible_session(m, &who, id).await?;
    if let Some(b) = &bg
        && !is_bg_id(b)
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "bad background command id (bg-<number>)".into(),
        ));
    }
    let sid = row.session_id;
    let log = m
        .inner
        .mirror
        .call(move |s| s.background_events(sid))
        .await?;
    let (running, ended) = shown(&log);
    let stopping = match &bg {
        Some(b) => {
            if let Some((_, reason)) = ended.iter().find(|(id, _)| id == b) {
                return Ok(Json(json!({
                    "ok": true,
                    "stopping": [],
                    "ended": [{"bg_id": b, "reason": reason}],
                    "message": format!("{b} had already ended: it {}", reason.words()),
                })));
            }
            // An id the log does not show running is still the node's to
            // judge (a start whose event is on its way, or a refusal).
            vec![b.clone()]
        }
        None if running.is_empty() => {
            return Ok(Json(json!({
                "ok": true,
                "stopping": [],
                "ended": [],
                "message": "no background command runs in this chat",
            })));
        }
        None => running,
    };
    let node = row.node_id.clone();
    if !m.is_connected(&node) {
        return Err(away(&node));
    }
    if !m.node_has(&node, CAP_BACKGROUND) {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "this chat's node does not run background commands (a chat node, or one older \
             than this server)"
                .into(),
        ));
    }
    tracing::info!(session = %sid, bg = bg.as_deref().unwrap_or("all"), "the owner stops background commands");
    match m.stop_background(sid, bg, StopBackgroundBy::User).await {
        Ok(()) => Ok(Json(
            json!({ "ok": true, "stopping": stopping, "ended": [] }),
        )),
        // The link went between the check and the send.
        Err(CommandError::NodeOffline(_)) => Err(away(&node)),
        // The node's refusal (409, its words), no answer in time (504), a
        // node that is stopping (503: it stops them itself).
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_the_nodes_shape_only() {
        for ok in ["bg-1", "bg-42", "bg-999999999"] {
            assert!(is_bg_id(ok), "{ok}");
        }
        for bad in [
            "",
            "bg-",
            "bg-1x",
            "bg--1",
            "BG-1",
            "bg-1234567890",
            "../bg-1",
            "bg-1 ",
        ] {
            assert!(!is_bg_id(bad), "{bad:?}");
        }
    }
}
