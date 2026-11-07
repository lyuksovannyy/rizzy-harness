//! The SPA, embedded from `web/dist` at compile time (release) or read from
//! disk (debug builds, via rust-embed's default behaviour).

use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
struct Dist;

/// A strong validator from the file's content hash.
fn etag(hash: &[u8; 32]) -> String {
    let hex: String = hash[..16].iter().map(|b| format!("{b:02x}")).collect();
    format!("\"{hex}\"")
}

/// Whether an `If-None-Match` header names `etag` (weak comparison, as for GET).
fn none_match(header: Option<&str>, etag: &str) -> bool {
    header.is_some_and(|h| {
        h.split(',')
            .map(str::trim)
            .any(|t| t == "*" || t.trim_start_matches("W/") == etag)
    })
}

pub async fn serve(uri: Uri, headers: HeaderMap) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let candidate = if path.is_empty() { "index.html" } else { path };
    if let Some(f) = Dist::get(candidate) {
        let mime = mime_guess::from_path(candidate).first_or_octet_stream();
        if candidate.starts_with("assets/") {
            return (
                [
                    (header::CONTENT_TYPE, mime.as_ref().to_string()),
                    (
                        header::CACHE_CONTROL,
                        "public, max-age=31536000, immutable".to_string(),
                    ),
                ],
                f.data.into_owned(),
            )
                .into_response();
        }
        // The rest keep their names across builds (index.html, theme.js, the
        // wall lists), so the browser asks every time; the ETag lets it keep
        // its copy (304) until the file changes.
        let tag = etag(&f.metadata.sha256_hash());
        let cached = headers
            .get(header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok());
        if none_match(cached, &tag) {
            return (
                StatusCode::NOT_MODIFIED,
                [
                    (header::ETAG, tag),
                    (header::CACHE_CONTROL, "no-cache".to_string()),
                ],
            )
                .into_response();
        }
        return (
            [
                (header::CONTENT_TYPE, mime.as_ref().to_string()),
                (header::CACHE_CONTROL, "no-cache".to_string()),
                (header::ETAG, tag),
            ],
            f.data.into_owned(),
        )
            .into_response();
    }
    // SPA fallback for client-side routes.
    match Dist::get("index.html") {
        Some(f) => (
            [(header::CACHE_CONTROL, "no-cache")],
            Html(f.data.into_owned()),
        )
            .into_response(),
        None => Html(format!(
            "<!doctype html><title>{0}</title><body style=\"font-family:sans-serif\"><h1>{0}</h1>\
             <p>The web UI is not built. Run <code>npm ci &amp;&amp; npm run build</code> in <code>web/</code> and rebuild.</p>\
             <p>The API is available under <code>/api</code>.</p></body>",
            harness_core::APP_NAME
        ))
        .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etag_is_quoted_hex_of_the_hash() {
        let t = etag(&[0xab; 32]);
        assert_eq!(t, format!("\"{}\"", "ab".repeat(16)));
    }

    #[test]
    fn if_none_match_lists_and_weak_tags() {
        let t = "\"abc\"";
        assert!(none_match(Some("\"abc\""), t));
        assert!(none_match(Some("W/\"abc\""), t));
        assert!(none_match(Some("\"x\", \"abc\""), t));
        assert!(none_match(Some("*"), t));
        assert!(!none_match(Some("\"abd\""), t));
        assert!(!none_match(None, t));
    }
}
