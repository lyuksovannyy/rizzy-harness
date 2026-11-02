//! The SPA, embedded from `web/dist` at compile time (release) or read from
//! disk (debug builds, via rust-embed's default behaviour).

use axum::http::{StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
struct Dist;

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let candidate = if path.is_empty() { "index.html" } else { path };
    if let Some(f) = Dist::get(candidate) {
        let mime = mime_guess::from_path(candidate).first_or_octet_stream();
        let cache = if candidate.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        return (
            [
                (header::CONTENT_TYPE, mime.as_ref().to_string()),
                (header::CACHE_CONTROL, cache.to_string()),
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
