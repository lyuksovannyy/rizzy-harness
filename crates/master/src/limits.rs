//! Per-route request timeouts (audit S6).
//!
//! One middleware, applied with `route_layer` so it sees the matched route
//! template. Every route is cut after `Timeouts::short` unless the table
//! below says otherwise, so a new route is limited by default and a forgotten
//! entry fails safe. The cut drops the handler's future (its work stops at
//! the next await) and answers 504 with a fixed message.
//!
//! The header-read and handshake limits live in `serve.rs`; they cover the
//! time before a request exists. This covers the time after.

use std::time::Duration;

use axum::Json;
use axum::extract::{MatchedPath, Request, State};
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::json;

#[derive(Clone, Copy, Debug)]
pub struct Timeouts {
    /// Ordinary handlers: a database read, a command acknowledged by a node
    /// (15 s), a favicon fetch (15 s).
    pub short: Duration,
    /// Handlers that call a provider.
    pub long: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            short: Duration::from_secs(30),
            long: Duration::from_secs(120),
        }
    }
}

enum Limit {
    Short,
    Long,
    /// Bounded by something else (see the table), not by wall time.
    None,
}

/// Which limit a route gets. Keys are route templates as registered.
fn limit(method: &Method, template: &str) -> Limit {
    match (method.as_str(), template) {
        // Long-lived: the upgrade itself answers at once, but the socket is
        // the point of the route and must never be on a timer.
        ("GET", "/api/ws")
        // Uploads: the client's link speed sets the time; size limits and the
        // storage quota bound them. /api/transcribe also waits for the
        // transcription (its own 120 s client timeout).
        | ("POST", "/api/attachments" | "/api/transcribe" | "/api/media")
        // Blob downloads: file size over the client's link.
        | ("GET", "/api/media/{id}/content")
        | ("GET", "/api/artifacts/{id}/latest/download")
        | ("GET", "/api/artifacts/{id}/versions/{vid}/download")
        | ("GET", "/api/artifacts/{id}/versions/{vid}/preview")
        // Pulls the file from the node (idle 60 s, hard cap 1 h in artifact_pull).
        | ("POST", "/api/sessions/{id}/artifacts") => Limit::None,
        // Provider calls with their own budgets of 20-60 s (agent-native
        // `models.rs`, `agent_edits.rs`); 30 s would cut them mid-answer.
        ("POST", "/api/admin/providers/test" | "/api/admin/providers/models/test")
        | ("POST", "/api/admin/providers/{id}/models/refresh")
        | ("GET", "/api/admin/providers/{id}/models")
        | ("POST", "/api/agent-summary") => Limit::Long,
        _ => Limit::Short,
    }
}

pub async fn enforce(State(t): State<Timeouts>, req: Request, next: Next) -> Response {
    let limit = match req.extensions().get::<MatchedPath>() {
        Some(p) => limit(req.method(), p.as_str()),
        None => Limit::Short,
    };
    let after = match limit {
        Limit::None => return next.run(req).await,
        Limit::Short => t.short,
        Limit::Long => t.long,
    };
    let (method, path) = (req.method().clone(), req.uri().path().to_string());
    match tokio::time::timeout(after, next.run(req)).await {
        Ok(r) => r,
        Err(_) => {
            tracing::warn!(%method, %path, secs = after.as_secs_f32(), "request timed out");
            (
                StatusCode::GATEWAY_TIMEOUT,
                Json(json!({ "error": "request timed out" })),
            )
                .into_response()
        }
    }
}
