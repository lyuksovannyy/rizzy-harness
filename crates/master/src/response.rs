//! What every response gets on its way out (AUDIT L4, L6; PLAN.md T24).
//!
//! The outermost layer, so it covers the `Host`/`Origin` refusals, request
//! timeouts and the embedded UI files as well as every handler:
//!
//! - **A request id**, 64 random bits as 16 hex characters, generated here
//!   for every request (never taken from the client, never derived from
//!   anything), in `x-request-id` on every response.
//! - **No detail in a 500.** Whatever a handler put in a 500's body (a
//!   store error with SQL, a file path, a library's message) is logged with
//!   the request id and replaced by `{"error":"internal error",
//!   "request_id":"…"}`. This is done to the response, not at each call
//!   site, so a new handler cannot leak by building its own 500.
//! - **`Cache-Control: no-store`** on every `/api/*` response, errors and
//!   304s included, replacing what a handler set: API answers and stored
//!   files are private, and a copy in the browser's disk cache would outlive
//!   a logout and a delete. The UI's own files keep their caching (hashed
//!   names, `assets.rs`).
//! - **`Strict-Transport-Security`** when the server itself serves TLS
//!   (`--tls`), never over plain HTTP, and not for `localhost` or an IP
//!   address (see [`hsts`]).

use axum::extract::{Request, State};
use axum::http::{HeaderName, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::Master;

/// The response header carrying the request id.
pub const REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// The only body a 500 has.
pub const INTERNAL: &str = "internal error";

/// A 500's original body is read for the log up to this many bytes.
const LOGGED_BODY: usize = 4096;

/// A year or more would be wrong for a self-hosted
/// box whose owner may turn `--tls` off again: 180 days, this host only.
/// Not `includeSubDomains`: the harness runs on a name it does not own
/// (`nas.home.arpa`, a tailnet name) whose sibling hosts may serve plain
/// HTTP. Not `preload`.
pub const HSTS: &str = "max-age=15552000";

/// This request's id, for handlers that log.
#[derive(Clone, Debug)]
pub struct RequestId(pub String);

pub async fn finish(State(master): State<Master>, mut req: Request, next: Next) -> Response {
    let id = crate::auth::random_hex(8);
    let path = req.uri().path().to_string();
    let api = path == "/api" || path.starts_with("/api/");
    let hsts = master.inner.auth.serves_tls()
        && req
            .headers()
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .is_some_and(hsts_applies);
    let method = req.method().clone();
    req.extensions_mut().insert(RequestId(id.clone()));
    let mut resp = next.run(req).await;
    if resp.status() == StatusCode::INTERNAL_SERVER_ERROR {
        resp = internal(resp, &id, &method, &path).await;
    }
    let h = resp.headers_mut();
    h.insert(
        REQUEST_ID,
        HeaderValue::from_str(&id).expect("hex is a header value"),
    );
    if api {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    if hsts {
        h.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static(HSTS),
        );
    }
    resp
}

/// Log a 500's body with the request id; answer with the fixed one.
async fn internal(resp: Response, id: &str, method: &axum::http::Method, path: &str) -> Response {
    let (parts, body) = resp.into_parts();
    let detail = match axum::body::to_bytes(body, LOGGED_BODY).await {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(_) => format!("(body over {LOGGED_BODY} bytes or unreadable)"),
    };
    tracing::error!(request_id = %id, %method, path, %detail, "internal error");
    let mut out = (
        StatusCode::INTERNAL_SERVER_ERROR,
        axum::Json(serde_json::json!({ "error": INTERNAL, "request_id": id })),
    )
        .into_response();
    // Keep what the guard and the handler said about the response itself
    // (CSP, Retry-After), not what described the old body.
    for (k, v) in &parts.headers {
        if k != header::CONTENT_TYPE
            && k != header::CONTENT_LENGTH
            && k != header::CONTENT_DISPOSITION
            && k != header::CONTENT_ENCODING
        {
            out.headers_mut().append(k.clone(), v.clone());
        }
    }
    out
}

/// Whether a response for `host` (the `Host` header) gets HSTS. Not for
/// `localhost`: HSTS is per host name, not per port, so it would switch
/// every other development server on `http://localhost` to https in that
/// browser. Not for IP addresses, which browsers ignore it for (RFC 6797
/// §8.1.1).
pub fn hsts_applies(host: &str) -> bool {
    if host.starts_with('[') {
        return false; // an IPv6 literal
    }
    let name = host.rsplit_once(':').map_or(host, |(h, _)| h);
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    !(name.is_empty()
        || name == "localhost"
        || name.ends_with(".localhost")
        || name.parse::<std::net::IpAddr>().is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsts_hosts() {
        for yes in ["harness.example", "nas.home.arpa:7777", "box:443"] {
            assert!(hsts_applies(yes), "{yes}");
        }
        for no in [
            "localhost:7777",
            "LOCALHOST",
            "app.localhost:7777",
            "127.0.0.1:7777",
            "10.8.0.1",
            "[::1]:7777",
            "[fd00::1]",
            "",
        ] {
            assert!(!hsts_applies(no), "{no}");
        }
    }
}
