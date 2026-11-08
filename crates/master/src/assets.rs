//! The SPA, embedded from `web/dist` at compile time (release) or read from
//! disk (debug builds, via rust-embed's default behaviour).
//!
//! Text files go gzipped to browsers that take it: the JS alone is about a
//! third of its size that way, which is most of a first visit's download
//! (hashed assets are cached for a year after it).

use std::borrow::Cow;
use std::collections::HashMap;
use std::io::Write;
use std::sync::{LazyLock, Mutex};

use axum::body::Bytes;
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Response};
use flate2::Compression;
use flate2::write::GzEncoder;
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
struct Dist;

/// Smaller files go as they are: gzip's header and trailer eat most of what
/// it would save.
const MIN_GZIP: usize = 1024;

/// Gzipped copies by content hash, made on first request. Keyed by hash, not
/// name, so a debug build reading `web/dist` from disk never serves a stale
/// copy after a rebuild. `None`: gzip did not make the file smaller.
static GZIPPED: LazyLock<Mutex<HashMap<[u8; 32], Option<Bytes>>>> = LazyLock::new(Default::default);

/// A strong validator from the file's content hash. The gzipped copy is a
/// different representation of the same file, so it gets its own tag.
fn etag(hash: &[u8; 32], gzip: bool) -> String {
    let hex: String = hash[..16].iter().map(|b| format!("{b:02x}")).collect();
    let suffix = if gzip { "-gz" } else { "" };
    format!("\"{hex}{suffix}\"")
}

/// Whether an `If-None-Match` header names `etag` (weak comparison, as for GET).
fn none_match(header: Option<&str>, etag: &str) -> bool {
    header.is_some_and(|h| {
        h.split(',')
            .map(str::trim)
            .any(|t| t == "*" || t.trim_start_matches("W/") == etag)
    })
}

/// Text formats; fonts, images and media are compressed already.
fn compressible(mime: &str) -> bool {
    mime.starts_with("text/")
        || matches!(
            mime,
            "application/javascript"
                | "application/json"
                | "application/manifest+json"
                | "application/wasm"
                | "application/xml"
                | "image/svg+xml"
        )
}

/// Whether `Accept-Encoding` takes gzip. `q=0` refuses it, and `*` stands for
/// any coding the header does not name.
fn accepts_gzip(header: Option<&str>) -> bool {
    let mut any = false;
    for item in header.unwrap_or("").split(',') {
        let mut parts = item.split(';');
        let coding = parts.next().unwrap_or("").trim();
        let q = parts
            .filter_map(|p| p.split_once('='))
            .find(|(k, _)| k.trim().eq_ignore_ascii_case("q"))
            .map_or(1.0, |(_, v)| v.trim().parse::<f32>().unwrap_or(0.0));
        if coding.eq_ignore_ascii_case("gzip") || coding.eq_ignore_ascii_case("x-gzip") {
            return q > 0.0;
        }
        if coding == "*" {
            any = q > 0.0;
        }
    }
    any
}

/// `data` gzipped at the best level, if that makes it smaller.
fn gzip(data: &[u8]) -> Option<Bytes> {
    let mut enc = GzEncoder::new(Vec::with_capacity(data.len() / 3), Compression::best());
    enc.write_all(data).ok()?;
    let out = enc.finish().ok()?;
    (out.len() < data.len()).then(|| Bytes::from(out))
}

/// The cached gzipped copy of the file with `hash`, made now if missing.
async fn gzipped(hash: [u8; 32], data: Bytes) -> Option<Bytes> {
    let cache = || GZIPPED.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(hit) = cache().get(&hash) {
        return hit.clone();
    }
    // Off the async threads: the largest chunk takes milliseconds to compress.
    // Two first requests at once may both compress; the copies are equal.
    let out = tokio::task::spawn_blocking(move || gzip(&data))
        .await
        .ok()?;
    cache().insert(hash, out.clone());
    out
}

/// What goes on the wire for one file.
struct Body {
    bytes: Bytes,
    gzip: bool,
    /// The response depends on `Accept-Encoding`, so caches must key on it.
    varies: bool,
}

async fn body(data: Cow<'static, [u8]>, hash: [u8; 32], mime: &str, headers: &HeaderMap) -> Body {
    let bytes = match data {
        Cow::Borrowed(b) => Bytes::from_static(b),
        Cow::Owned(v) => Bytes::from(v),
    };
    let varies = compressible(mime) && bytes.len() >= MIN_GZIP;
    let accept = headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok());
    if varies
        && accepts_gzip(accept)
        && let Some(gz) = gzipped(hash, bytes.clone()).await
    {
        return Body {
            bytes: gz,
            gzip: true,
            varies,
        };
    }
    Body {
        bytes,
        gzip: false,
        varies,
    }
}

/// A response with `b` (none for a 304) and the headers that describe it.
fn reply(
    status: StatusCode,
    b: Body,
    mime: &str,
    cache: &'static str,
    tag: Option<&str>,
) -> Response {
    let modified = status != StatusCode::NOT_MODIFIED;
    let mut res = if modified {
        (status, b.bytes).into_response()
    } else {
        status.into_response()
    };
    let h = res.headers_mut();
    if modified {
        let mime = HeaderValue::from_str(mime)
            .unwrap_or(HeaderValue::from_static("application/octet-stream"));
        h.insert(header::CONTENT_TYPE, mime);
        if b.gzip {
            h.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        }
    }
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    if b.varies {
        h.insert(header::VARY, HeaderValue::from_static("accept-encoding"));
    }
    if let Some(tag) = tag.and_then(|t| HeaderValue::from_str(t).ok()) {
        h.insert(header::ETAG, tag);
    }
    res
}

pub async fn serve(uri: Uri, headers: HeaderMap) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let candidate = if path.is_empty() { "index.html" } else { path };
    if let Some(f) = Dist::get(candidate) {
        let mime = mime_guess::from_path(candidate).first_or_octet_stream();
        let hash = f.metadata.sha256_hash();
        let b = body(f.data, hash, mime.as_ref(), &headers).await;
        if candidate.starts_with("assets/") {
            let cache = "public, max-age=31536000, immutable";
            return reply(StatusCode::OK, b, mime.as_ref(), cache, None);
        }
        // The rest keep their names across builds (index.html, theme.js, the
        // wall lists), so the browser asks every time; the ETag lets it keep
        // its copy (304) until the file changes.
        let tag = etag(&hash, b.gzip);
        let cached = headers
            .get(header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok());
        let status = if none_match(cached, &tag) {
            StatusCode::NOT_MODIFIED
        } else {
            StatusCode::OK
        };
        return reply(status, b, mime.as_ref(), "no-cache", Some(&tag));
    }
    // SPA fallback for client-side routes.
    match Dist::get("index.html") {
        Some(f) => {
            let mime = "text/html; charset=utf-8";
            let b = body(f.data, f.metadata.sha256_hash(), mime, &headers).await;
            reply(StatusCode::OK, b, mime, "no-cache", None)
        }
        None => Html(format!(
            "<!doctype html><title>{0}</title><body style=\"font-family:sans-serif\"><h1>{0}</h1>\
             <p>The web UI is not built. Run <code>pnpm install &amp;&amp; pnpm build</code> in <code>web/</code> and rebuild.</p>\
             <p>The API is available under <code>/api</code>.</p></body>",
            harness_core::APP_NAME
        ))
        .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;
    use std::io::Read;

    fn accept(v: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::ACCEPT_ENCODING, HeaderValue::from_str(v).unwrap());
        h
    }

    fn gunzip(b: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        GzDecoder::new(b).read_to_end(&mut out).unwrap();
        out
    }

    /// Script-like text that compresses, as a chunk does.
    fn script() -> Vec<u8> {
        "export const f = (a) => a + 1;\n".repeat(200).into_bytes()
    }

    #[test]
    fn etag_is_quoted_hex_of_the_hash() {
        let t = etag(&[0xab; 32], false);
        assert_eq!(t, format!("\"{}\"", "ab".repeat(16)));
        assert_eq!(
            etag(&[0xab; 32], true),
            format!("\"{}-gz\"", "ab".repeat(16))
        );
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
        // The plain copy's tag does not validate the gzipped one.
        assert!(!none_match(Some("\"abc\""), "\"abc-gz\""));
    }

    #[test]
    fn accept_encoding_takes_gzip_unless_refused() {
        assert!(accepts_gzip(Some("gzip, deflate, br, zstd")));
        assert!(accepts_gzip(Some("br;q=1.0, GZIP;q=0.5")));
        assert!(accepts_gzip(Some("x-gzip")));
        assert!(accepts_gzip(Some("*")));
        assert!(!accepts_gzip(Some("gzip;q=0")));
        assert!(!accepts_gzip(Some("gzip; Q=0.000, *")));
        assert!(!accepts_gzip(Some("*;q=0")));
        assert!(!accepts_gzip(Some("br, zstd")));
        assert!(!accepts_gzip(Some("identity")));
        assert!(!accepts_gzip(Some("")));
        assert!(!accepts_gzip(None));
    }

    #[test]
    fn only_text_formats_are_compressed() {
        for m in [
            "application/javascript",
            "text/javascript",
            "text/css",
            "text/html",
            "image/svg+xml",
            "application/json",
        ] {
            assert!(compressible(m), "{m}");
        }
        for m in [
            "font/woff2",
            "image/png",
            "image/webp",
            "video/mp4",
            "application/octet-stream",
        ] {
            assert!(!compressible(m), "{m}");
        }
    }

    #[tokio::test]
    async fn scripts_go_gzipped_to_browsers_that_take_it() {
        let data = script();
        let b = body(
            Cow::Owned(data.clone()),
            [1; 32],
            "text/javascript",
            &accept("gzip, br"),
        )
        .await;
        assert!(b.gzip && b.varies);
        assert!(
            b.bytes.len() < data.len() / 4,
            "{} of {}",
            b.bytes.len(),
            data.len()
        );
        assert_eq!(gunzip(&b.bytes), data);

        let res = reply(StatusCode::OK, b, "text/javascript", "no-cache", None);
        assert_eq!(res.headers()[header::CONTENT_ENCODING], "gzip");
        assert_eq!(res.headers()[header::VARY], "accept-encoding");
        assert_eq!(res.headers()[header::CONTENT_TYPE], "text/javascript");
    }

    #[tokio::test]
    async fn others_get_the_file_as_it_is() {
        let data = script();
        // A browser that refuses gzip still learns the response varies.
        let b = body(
            Cow::Owned(data.clone()),
            [2; 32],
            "text/javascript",
            &accept("br;q=1, gzip;q=0"),
        )
        .await;
        assert!(!b.gzip && b.varies);
        assert_eq!(b.bytes, data);
        let res = reply(StatusCode::OK, b, "text/javascript", "no-cache", None);
        assert!(res.headers().get(header::CONTENT_ENCODING).is_none());
        assert_eq!(res.headers()[header::VARY], "accept-encoding");

        // Fonts and tiny files never vary.
        let b = body(
            Cow::Owned(data.clone()),
            [3; 32],
            "font/woff2",
            &accept("gzip"),
        )
        .await;
        assert!(!b.gzip && !b.varies);
        let b = body(
            Cow::Owned(b"x".repeat(MIN_GZIP - 1)),
            [4; 32],
            "text/css",
            &accept("gzip"),
        )
        .await;
        assert!(!b.gzip && !b.varies);
        let res = reply(StatusCode::OK, b, "text/css", "no-cache", None);
        assert!(res.headers().get(header::VARY).is_none());
    }

    #[tokio::test]
    async fn text_that_does_not_shrink_goes_as_it_is() {
        // Random bytes labelled as text: gzip would only add its framing.
        let mut noise = vec![0u8; 4096];
        getrandom::fill(&mut noise).unwrap();
        let b = body(
            Cow::Owned(noise.clone()),
            [5; 32],
            "text/plain",
            &accept("gzip"),
        )
        .await;
        assert!(!b.gzip && b.varies);
        assert_eq!(b.bytes, noise);
    }

    #[tokio::test]
    async fn a_not_modified_reply_has_no_body_or_encoding() {
        let b = body(Cow::Owned(script()), [6; 32], "text/html", &accept("gzip")).await;
        let tag = etag(&[6; 32], b.gzip);
        let res = reply(
            StatusCode::NOT_MODIFIED,
            b,
            "text/html",
            "no-cache",
            Some(&tag),
        );
        assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
        assert!(res.headers().get(header::CONTENT_ENCODING).is_none());
        assert!(res.headers().get(header::CONTENT_TYPE).is_none());
        assert_eq!(res.headers()[header::ETAG], tag.as_str());
        assert_eq!(res.headers()[header::VARY], "accept-encoding");
        let body = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
        assert!(body.is_empty());
    }
}
