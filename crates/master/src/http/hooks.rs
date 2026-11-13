//! Incoming webhooks that fire routines (docs/routines.md, Webhooks):
//! `POST /api/hooks/{trigger_id}` with `Authorization: Bearer <secret>`.
//!
//! The checks run in a fixed order, so nothing about a routine shows before
//! the caller proved its secret, and no throttle lets someone without the
//! secret lock out the sender who has it (`routines::buckets`):
//!
//! 1. the body limit, 64 KiB (413, by the extractor);
//! 2. the trigger id: unknown → a comparison against a dummy hash, then the
//!    unknown-id bucket (429 when empty, else 401);
//! 3. the secret, in constant time: wrong → the trigger's bad-secret bucket
//!    (429 when empty, else the same 401);
//! 4. the trigger's call rate (429);
//! 5. the routine disabled or paused, or webhooks off (409);
//! 6. the content type: JSON → `event.body`, text → `event.text`, else 415;
//! 7. the `Idempotency-Key` (a repeat answers the first call's run);
//! 8. overlap and caps (202 `{skipped}`), else 202 `{run_id}`.
//!
//! The handler never calls `require`: the API token is no credential here,
//! and no secret is ever read from the URL. Headers and the query string
//! never reach the event.

use axum::Router;
use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::post;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::auth::sha256_hex;
use crate::routines::buckets::UNKNOWN_ID;
use crate::routines::events::{self, Event};
use crate::routines::runner::{self, Fired};
use crate::routines::{self, RoutinesHandle};
use crate::{Master, lock};

/// The longest `Idempotency-Key`.
const MAX_KEY: usize = 200;
/// What a secret is compared with when the trigger id is unknown, so that a
/// miss costs what a hit does. No secret hashes to it.
const DUMMY_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";
/// The one answer for an unknown id and a wrong secret.
const UNAUTHORIZED: &str = "unknown webhook or wrong secret";
const PAUSED: &str = "this routine is turned off or paused";

/// Merged into the API router by `http::router`.
pub(super) fn routes() -> Router<Master> {
    Router::new().route(
        "/api/hooks/{trigger_id}",
        post(hook).layer(DefaultBodyLimit::max(events::MAX_BODY)),
    )
}

fn answer(code: StatusCode, body: Value) -> Response {
    (code, Json(body)).into_response()
}

fn error(code: StatusCode, msg: &str) -> Response {
    answer(code, json!({ "error": msg }))
}

/// 429, saying when to come back (whole seconds, at least one).
fn too_many(wait_ms: i64) -> Response {
    let secs = ((wait_ms + 999) / 1000).max(1);
    let mut r = error(
        StatusCode::TOO_MANY_REQUESTS,
        "too many calls; try again later",
    );
    r.headers_mut()
        .insert(header::RETRY_AFTER, HeaderValue::from(secs));
    r
}

/// The bearer secret, or nothing.
fn bearer(headers: &HeaderMap) -> &str {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map_or("", str::trim)
}

async fn hook(
    State(m): State<Master>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let body = match body {
        Ok(b) => b,
        Err(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return error(e.status(), "the body is larger than 64 KiB");
        }
        Err(e) => return error(e.status(), &e.body_text()),
    };
    let Some(h) = routines::handle(&m) else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "routines are not running");
    };
    let now = h.clock.now();
    let (t, r) = match authenticate(&m, h, &id, bearer(&headers), now).await {
        Ok(target) => target,
        Err(refused) => return refused.into_response(),
    };
    let cfg = routines::config(&m);
    let rate = lock(&h.hooks).call(&t.trigger_id, cfg.hook_rate_per_minute, now);
    if let Err(wait) = rate {
        return too_many(wait);
    }
    if !cfg.enabled || !cfg.webhooks {
        return error(
            StatusCode::CONFLICT,
            "webhooks are turned off on this server",
        );
    }
    if !r.enabled || r.paused_reason.is_some() {
        return error(StatusCode::CONFLICT, PAUSED);
    }
    let received_at = runner::local_now(&m, &r.owner_id).await;
    let data = match event_data(&headers, &body, received_at) {
        Ok(d) => d,
        Err((code, msg)) => return error(code, msg),
    };
    let dedupe_key = match idempotency_key(&headers) {
        Ok(k) => k,
        Err(msg) => return error(StatusCode::BAD_REQUEST, msg),
    };
    let event = Event {
        kind: "webhook",
        data,
        dedupe_key,
        size: body.len(),
        sha256: hex::encode(Sha256::digest(&body)),
    };
    match routines::fire_event(&m, &t, &r, event).await {
        Fired::Started(run_id) => answer(StatusCode::ACCEPTED, json!({ "run_id": run_id })),
        Fired::Duplicate(run_id) => answer(
            StatusCode::ACCEPTED,
            json!({ "run_id": run_id, "duplicate": true }),
        ),
        Fired::Skipped(why) => answer(StatusCode::ACCEPTED, json!({ "skipped": why })),
        Fired::Paused => error(StatusCode::CONFLICT, PAUSED),
        Fired::Stopping => error(StatusCode::SERVICE_UNAVAILABLE, routines::STOPPING),
        Fired::Failed => error(StatusCode::INTERNAL_SERVER_ERROR, "could not start the run"),
    }
}

/// Why a call did not get past its secret.
enum Refused {
    /// An unknown id or a wrong secret (the same answer for both).
    Unauthorized,
    /// A bucket of step 2 or 3 is empty; a token comes in that many ms.
    Throttled(i64),
    /// The trigger could not be looked up.
    Broken,
}

impl IntoResponse for Refused {
    fn into_response(self) -> Response {
        match self {
            Refused::Unauthorized => error(StatusCode::UNAUTHORIZED, UNAUTHORIZED),
            Refused::Throttled(wait) => too_many(wait),
            Refused::Broken => error(StatusCode::INTERNAL_SERVER_ERROR, "try again"),
        }
    }
}

type Target = (
    harness_store::routines::TriggerRow,
    harness_store::routines::RoutineRow,
);

/// Steps 2 and 3: the webhook trigger `id` and its routine, once `secret`
/// matches; else why not.
async fn authenticate(
    m: &Master,
    h: &RoutinesHandle,
    id: &str,
    secret: &str,
    now: i64,
) -> Result<Target, Refused> {
    // Ids are 32 hex characters; anything else is unknown without a lookup.
    let target = if id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
        let tid = id.to_string();
        match m.inner.mirror.call(move |s| s.hook_target(&tid)).await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(error = %e, "hooks: could not look up a trigger");
                return Err(Refused::Broken);
            }
        }
    } else {
        None
    };
    let stored = target
        .as_ref()
        .and_then(|(t, _)| t.secret_hash.as_deref())
        .unwrap_or(DUMMY_HASH);
    // Always compared, the unknown id against the dummy.
    let matches = bool::from(sha256_hex(secret).as_bytes().ct_eq(stored.as_bytes()));
    match target {
        Some(target) if matches => Ok(target),
        Some((t, _)) => {
            let verdict = lock(&h.hooks).bad_secret(&t.trigger_id, now);
            Err(rejected(m, h, &t.trigger_id, verdict, now).await)
        }
        None => {
            let verdict = lock(&h.hooks).unknown_id(now);
            Err(rejected(m, h, UNKNOWN_ID, verdict, now).await)
        }
    }
}

/// Refuse a call that did not prove its secret, and write the throttled
/// `hook.rejected` line (ids and counts only).
async fn rejected(
    m: &Master,
    h: &RoutinesHandle,
    key: &str,
    verdict: Result<(), i64>,
    now: i64,
) -> Refused {
    let due = lock(&h.hooks).rejected(key, now);
    if let Some(n) = due {
        let trigger = (key != UNKNOWN_ID).then_some(key);
        routines::audit(
            m,
            "hook.rejected",
            json!({ "trigger_id": trigger, "rejected": n }),
        )
        .await;
    }
    match verdict {
        Ok(()) => Refused::Unauthorized,
        Err(wait) => Refused::Throttled(wait),
    }
}

/// The event's data from the body by its content type (step 6).
fn event_data(
    headers: &HeaderMap,
    body: &[u8],
    received_at: String,
) -> Result<Value, (StatusCode, &'static str)> {
    let ct = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let essence = ct
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match essence.as_str() {
        "application/json" => {
            let bad = (StatusCode::BAD_REQUEST, "the body is not valid JSON");
            let v: Value = serde_json::from_slice(body).map_err(|_| bad)?;
            if events::depth(&v) > events::MAX_DEPTH {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "the JSON body is nested more than 32 deep",
                ));
            }
            Ok(Event::json(v, received_at))
        }
        "text/plain" => {
            let bad = (StatusCode::BAD_REQUEST, "the body is not UTF-8 text");
            let text = std::str::from_utf8(body).map_err(|_| bad)?;
            Ok(Event::text(text.to_string(), received_at))
        }
        _ => Err((
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "send application/json or text/plain",
        )),
    }
}

/// The `Idempotency-Key` as its SHA-256 hex (never kept as sent).
fn idempotency_key(headers: &HeaderMap) -> Result<Option<String>, &'static str> {
    let Some(v) = headers.get("idempotency-key") else {
        return Ok(None);
    };
    let v = v
        .to_str()
        .map_err(|_| "the Idempotency-Key must be plain text")?
        .trim();
    if v.is_empty() {
        return Ok(None);
    }
    if v.chars().count() > MAX_KEY {
        return Err("the Idempotency-Key has at most 200 characters");
    }
    Ok(Some(sha256_hex(v)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(*k, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn bodies_by_content_type() {
        let json_ct = headers(&[("content-type", "application/json; charset=utf-8")]);
        let e = event_data(&json_ct, br#"{"a":[1]}"#, "t".into()).unwrap();
        assert_eq!(e["body"]["a"][0], 1);
        assert_eq!(e["received_at"], "t");
        assert_eq!(
            event_data(&json_ct, b"{", "t".into()).unwrap_err().0,
            StatusCode::BAD_REQUEST
        );
        let deep = format!("{}{}", "[".repeat(33), "]".repeat(33));
        assert_eq!(
            event_data(&json_ct, deep.as_bytes(), "t".into())
                .unwrap_err()
                .0,
            StatusCode::BAD_REQUEST
        );
        let ok = format!("{}{}", "[".repeat(32), "]".repeat(32));
        assert!(event_data(&json_ct, ok.as_bytes(), "t".into()).is_ok());
        let text = headers(&[("content-type", "Text/Plain")]);
        let e = event_data(&text, "héllo".as_bytes(), "t".into()).unwrap();
        assert_eq!(e["text"], "héllo");
        assert_eq!(
            event_data(&text, &[0xff], "t".into()).unwrap_err().0,
            StatusCode::BAD_REQUEST
        );
        for ct in [
            &headers(&[("content-type", "text/html")]),
            &HeaderMap::new(),
        ] {
            assert_eq!(
                event_data(ct, b"x", "t".into()).unwrap_err().0,
                StatusCode::UNSUPPORTED_MEDIA_TYPE
            );
        }
    }

    #[test]
    fn idempotency_keys_are_hashed_and_bounded() {
        assert_eq!(idempotency_key(&HeaderMap::new()), Ok(None));
        assert_eq!(
            idempotency_key(&headers(&[("idempotency-key", " ")])),
            Ok(None)
        );
        let k = idempotency_key(&headers(&[("idempotency-key", "build-812")]))
            .unwrap()
            .unwrap();
        assert_eq!(k, sha256_hex("build-812"));
        let long = "k".repeat(MAX_KEY + 1);
        assert!(idempotency_key(&headers(&[("idempotency-key", &long)])).is_err());
    }

    #[test]
    fn the_secret_comes_only_from_the_bearer_header() {
        assert_eq!(bearer(&headers(&[("authorization", "Bearer abc ")])), "abc");
        assert_eq!(bearer(&headers(&[("authorization", "Basic abc")])), "");
        assert_eq!(bearer(&HeaderMap::new()), "");
    }
}
