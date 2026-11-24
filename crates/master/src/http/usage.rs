//! Token totals for server admins (ADOPTION_PLAN X.18, X.39): per person and
//! model, and per workspace, for a period. Numbers only: nothing about a
//! single chat leaves the store (chats are private to their owner, admins
//! too; see `harness_store::usage` for what is counted and why). Prices are
//! not applied here: the page prices the token sums with the same functions
//! as the chat header.

use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use harness_store::usage::{Bucket, UsageTotals};
use serde::Deserialize;
use serde_json::{Value, json};

use super::caller::Admin;
use super::{ApiError, ApiResult};
use crate::Master;

/// Merged into the API router by `http::router`.
pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/admin/usage", get(usage))
}

/// The longest period one request may ask for.
const MAX_DAYS: i64 = 366;
const MAX_PERIOD_MS: i64 = MAX_DAYS * 86_400_000;
/// How long the scan may run before the answer is cut (shorter than the
/// route's own 30 s timeout).
pub(crate) const TIME_LIMIT: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
struct Period {
    from: Option<i64>,
    to: Option<i64>,
}

fn bucket(b: &Bucket) -> Value {
    json!({
        "input_tokens": b.tally.input_tokens,
        "output_tokens": b.tally.output_tokens,
        "cache_read_tokens": b.tally.cache_read_tokens,
        "cache_write_tokens": b.tally.cache_write_tokens,
        "turns": b.turns,
        "models": b.models.iter().map(|(m, u)| json!({
            "model": m,
            "input_tokens": u.tally.input_tokens,
            "output_tokens": u.tally.output_tokens,
            "cache_read_tokens": u.tally.cache_read_tokens,
            "cache_write_tokens": u.tally.cache_write_tokens,
            "turns": u.turns,
        })).collect::<Vec<_>>(),
    })
}

/// The answer's JSON; every field is a number, an account's id and name, a
/// workspace's id and name, or a model id.
fn body(from: i64, to: i64, t: &UsageTotals) -> Value {
    let merge = |mut head: Value, b: &Bucket| {
        if let (Some(h), Value::Object(rest)) = (head.as_object_mut(), bucket(b)) {
            h.extend(rest);
        }
        head
    };
    json!({
        "from": from,
        "to": to,
        "cut": t.covered_percent.map(|p| json!({"covered_percent": p})),
        "people": t.people.iter()
            .map(|p| merge(json!({"user_id": p.user_id, "username": p.username}), &p.bucket))
            .collect::<Vec<_>>(),
        "workspaces": t.workspaces.iter()
            .map(|w| merge(json!({"workspace_id": w.workspace_id, "name": w.name}), &w.bucket))
            .collect::<Vec<_>>(),
    })
}

async fn usage(
    _: Admin,
    State(m): State<Master>,
    Query(q): Query<Period>,
) -> ApiResult<Json<Value>> {
    let bad = |e: &str| ApiError(StatusCode::BAD_REQUEST, e.into());
    let (Some(from), Some(to)) = (q.from, q.to) else {
        return Err(bad("from and to (ms since 1970) are required"));
    };
    if from < 0 || to <= from {
        return Err(bad("the period must end after it starts"));
    }
    if to - from > MAX_PERIOD_MS {
        return Err(bad("the period is at most 366 days"));
    }
    let limit = m.usage_time_limit();
    let totals = m
        .inner
        .mirror
        .call(move |s| s.usage_totals(from, to, limit))
        .await?;
    Ok(Json(body(from, to, &totals)))
}
