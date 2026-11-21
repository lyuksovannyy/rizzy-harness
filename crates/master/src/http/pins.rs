//! The owner's bookmark on a message (docs/pins.md, README "Pinned
//! messages"): `POST /api/sessions/{id}/pins` with `{message_seq, pinned}`.
//!
//! Only the chat's owner: anyone else, admins and workspace admins and the
//! API token on a chat somebody owns included, gets the 404 of a chat that
//! does not exist (T10), the very same answer. A sub-agent's chat and a
//! routine's chat the caller owns can be pinned in: a pin is a bookmark for
//! the person, it changes nothing the model reads and nothing a chat does.
//!
//! The master answers what it can from its mirror (`harness_core::pins::check`,
//! the node's own rule) and relays `session.pin`; the node decides again
//! against its own log. Nothing a model, a tool or a routine sends reaches
//! this route: a pin is a person's click, never automatic.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, require};
use crate::{CommandError, Master};

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/sessions/{id}/pins", post(pin))
}

/// How a refusal starts (the node's own starts the same way).
const REFUSED: &str = "This message can't be pinned";

#[derive(Deserialize)]
struct Body {
    message_seq: u64,
    pinned: bool,
}

async fn pin(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<Body>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let row = super::rewind::owned(&m, &who, &id).await?;
    if !m.node_has(&row.node_id, harness_protocol::CAP_PIN) {
        return Err(CommandError::Rejected(
            "this chat's node cannot pin messages (a node older than this server, or one that \
             is away)"
                .into(),
        )
        .into());
    }
    let sid = row.session_id;
    let log = m
        .inner
        .mirror
        .call(move |s| s.events_since_turn_at(sid, i64::MIN, harness_core::pins::KINDS))
        .await?;
    harness_core::pins::check(&log, b.message_seq, b.pinned)
        .map_err(|e| ApiError(StatusCode::CONFLICT, format!("{REFUSED}: {e}")))?;
    tracing::info!(session = %sid, message_seq = b.message_seq, pinned = b.pinned,
        "a person bookmarks a message");
    m.pin(sid, b.message_seq, b.pinned).await?;
    Ok(Json(json!({ "ok": true })))
}
