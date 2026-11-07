//! Site icons for the search engine picker, fetched by the server so the
//! browser never contacts the site. Same guards as `web_fetch` (public
//! addresses only, every redirect re-checked), small body caps, and only
//! raster images recognised by their magic bytes: never SVG (active content)
//! and never HTML, whatever the server claims the type is.

use std::time::Duration;

use futures::StreamExt;

use crate::{check_url, fetch_client_with, sources::decode_entities};

/// Largest page read while looking for `<link rel=icon>`.
pub const MAX_PAGE: usize = 256 * 1024;
/// Largest icon accepted; a bigger one is not truncated but refused.
pub const MAX_ICON: usize = 100 * 1024;
const TIMEOUT: Duration = Duration::from_secs(5);
const USER_AGENT: &str = concat!("harness/", env!("CARGO_PKG_VERSION"), " (favicon)");
/// `<link>` icons tried before falling back to `/favicon.ico`.
const MAX_CANDIDATES: usize = 3;

/// A validated icon: the bytes and the type their magic bytes say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Favicon {
    pub bytes: Vec<u8>,
    pub content_type: &'static str,
}

/// The origin of a user-supplied https URL (`https://host[:port]/`), or why
/// it is not fetched.
pub fn origin(raw: &str) -> Result<url::Url, String> {
    let u = url::Url::parse(raw.trim()).map_err(|e| format!("invalid URL: {e}"))?;
    if u.scheme() != "https" {
        return Err("only https URLs are accepted".into());
    }
    check_url(&u, false)?;
    url::Url::parse(&format!("{}/", u.origin().ascii_serialization()))
        .map_err(|e| format!("invalid URL: {e}"))
}

/// The image type of `bytes` by magic number, for the raster formats served.
pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0, 0, 1, 0, ..] => Some("image/x-icon"),
        [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', ..] => Some("image/png"),
        [b'G', b'I', b'F', b'8', b'7' | b'9', b'a', ..] => Some("image/gif"),
        [0xff, 0xd8, 0xff, ..] => Some("image/jpeg"),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Some("image/webp"),
        _ => None,
    }
}

/// Icon URLs from the `<link rel=…icon…>` tags in `<head>`, best first: the
/// smallest square at least 16px, then the rest in document order. SVG and
/// mask icons are skipped, and so is an http icon on an https page.
pub fn icon_links(html: &str, base: &url::Url) -> Vec<url::Url> {
    // ASCII lowercasing keeps every byte offset, so `lower` indexes `html`.
    let lower = html.to_ascii_lowercase();
    let mut found: Vec<(Option<u32>, url::Url)> = Vec::new();
    let mut i = 0;
    while let Some(off) = lower[i..].find('<') {
        let at = i + off;
        let rest = &lower[at..];
        if rest.starts_with("</head") || rest.starts_with("<body") {
            break;
        }
        let skip_to = |end: &str| lower[at..].find(end).map(|j| at + j + end.len());
        let next = if rest.starts_with("<!--") {
            skip_to("-->")
        } else if rest.starts_with("<script") {
            skip_to("</script")
        } else if rest.starts_with("<style") {
            skip_to("</style")
        } else if rest.starts_with("<link")
            && rest[5..].starts_with(|c: char| c.is_ascii_whitespace() || c == '/')
        {
            // A tag cut off by the end of the input is dropped, as browsers do.
            attributes(html, at + 5).map(|(attrs, end)| {
                found.extend(icon_link(&attrs, base));
                end
            })
        } else {
            Some(at + 1)
        };
        match next {
            Some(n) => i = n,
            None => break,
        }
    }
    let best = found
        .iter()
        .enumerate()
        .filter_map(|(n, (size, _))| size.map(|s| (s, n)))
        .min()
        .map(|(_, n)| n);
    if let Some(n) = best {
        let link = found.remove(n);
        found.insert(0, link);
    }
    found.into_iter().map(|(_, u)| u).collect()
}

/// The URL and preferred size of one `<link>`, if it names a usable icon.
fn icon_link(attrs: &[(String, String)], base: &url::Url) -> Option<(Option<u32>, url::Url)> {
    let get = |name: &str| {
        attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };
    let rel = get("rel")?.to_ascii_lowercase();
    let is_icon = rel.split_ascii_whitespace().any(|t| {
        matches!(
            t,
            "icon" | "apple-touch-icon" | "apple-touch-icon-precomposed"
        )
    });
    if !is_icon || get("type").is_some_and(|t| t.to_ascii_lowercase().contains("svg")) {
        return None;
    }
    let href = decode_entities(get("href")?.trim());
    let u = base.join(&href).ok()?;
    let downgrade = u.scheme() != base.scheme() && u.scheme() != "https";
    if !matches!(u.scheme(), "http" | "https")
        || downgrade
        || u.path().to_ascii_lowercase().ends_with(".svg")
    {
        return None;
    }
    // `sizes="16x16 32x32"`: the smallest square that is at least 16px.
    let size = get("sizes").and_then(|s| {
        s.to_ascii_lowercase()
            .split_ascii_whitespace()
            .filter_map(|wh| {
                let (w, h) = wh.split_once('x')?;
                let (w, h) = (w.parse::<u32>().ok()?, h.parse::<u32>().ok()?);
                (w == h && w >= 16).then_some(w)
            })
            .min()
    });
    Some((size, u))
}

/// The attributes of a tag whose name ends at `from`, names lowercased,
/// and the offset just past the tag's closing `>` (`None` if it never closes).
fn attributes(html: &str, from: usize) -> Option<(Vec<(String, String)>, usize)> {
    let b = html.as_bytes();
    let mut out = Vec::new();
    let mut i = from;
    loop {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        if i >= b.len() {
            return None;
        }
        if b[i] == b'>' {
            return Some((out, i + 1));
        }
        let start = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'>' | b'/') {
            i += 1;
        }
        let name = html[start..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && matches!(b[i], b'"' | b'\'') {
                let q = b[i];
                let s = i + 1;
                i = s;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                value = html[s..i].to_string();
                i = (i + 1).min(b.len());
            } else {
                let s = i;
                while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' {
                    i += 1;
                }
                value = html[s..i].to_string();
            }
        }
        if !name.is_empty() {
            out.push((name, value));
        }
    }
}

pub struct FaviconFetcher {
    client: reqwest::Client,
    allow_private: bool,
    /// Every request and redirect is https (off only for local test servers).
    https_only: bool,
}

impl FaviconFetcher {
    pub fn new() -> Self {
        Self {
            client: fetch_client_with(false, false, true),
            allow_private: false,
            https_only: true,
        }
    }

    /// For tests against local (plain http) servers only.
    #[doc(hidden)]
    pub fn allowing_private() -> Self {
        Self {
            client: fetch_client_with(true, true, false),
            allow_private: true,
            https_only: false,
        }
    }

    /// Whether `u` may be fetched at all.
    fn allowed(&self, u: &url::Url) -> bool {
        (!self.https_only || u.scheme() == "https") && check_url(u, self.allow_private).is_ok()
    }

    /// The icon of the site at `origin`: the best `<link>` icon on its page,
    /// else `/favicon.ico`. `None` when there is none we would serve.
    pub async fn fetch(&self, origin: &url::Url) -> Option<Favicon> {
        if !self.allowed(origin) {
            return None;
        }
        let resp = self.get(origin.clone()).await.ok()?;
        let base = resp.url().clone();
        // (Redirects are refused already; this keeps `/favicon.ico` https.)
        if !self.allowed(&base) {
            return None;
        }
        let mut tried = Vec::new();
        if let Some(html) = page_html(resp).await {
            for u in icon_links(&html, &base).into_iter().take(MAX_CANDIDATES) {
                if let Some(icon) = self.icon(&u).await {
                    return Some(icon);
                }
                tried.push(u);
            }
        }
        let ico = base.join("/favicon.ico").ok()?;
        if tried.contains(&ico) {
            return None;
        }
        self.icon(&ico).await
    }

    async fn get(&self, u: url::Url) -> reqwest::Result<reqwest::Response> {
        self.client
            .get(u)
            .timeout(TIMEOUT)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .send()
            .await
    }

    async fn icon(&self, u: &url::Url) -> Option<Favicon> {
        if !self.allowed(u) {
            return None;
        }
        let resp = self.get(u.clone()).await.ok()?;
        if !resp.status().is_success() || resp.content_length().unwrap_or(0) > MAX_ICON as u64 {
            return None;
        }
        let (bytes, truncated) = read_capped(resp, MAX_ICON).await?;
        if truncated {
            return None;
        }
        let content_type = sniff(&bytes)?;
        Some(Favicon {
            bytes,
            content_type,
        })
    }
}

impl Default for FaviconFetcher {
    fn default() -> Self {
        Self::new()
    }
}

/// The page as text when it is a successful HTML response.
async fn page_html(resp: reqwest::Response) -> Option<String> {
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !resp.status().is_success() || !(ctype.is_empty() || ctype.contains("html")) {
        return None;
    }
    let (body, _) = read_capped(resp, MAX_PAGE).await?;
    Some(String::from_utf8_lossy(&body).into_owned())
}

/// At most `cap` bytes of the body, and whether there was more.
async fn read_capped(resp: reqwest::Response, cap: usize) -> Option<(Vec<u8>, bool)> {
    let mut body = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.ok()?;
        let room = cap - body.len();
        if chunk.len() > room {
            body.extend_from_slice(&chunk[..room]);
            return Some((body, true));
        }
        body.extend_from_slice(&chunk);
    }
    Some((body, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// https on every hop: no plain-http fetch, and no redirect to http even
    /// when the start was allowed.
    #[tokio::test]
    async fn https_only_refuses_http_and_downgrade_redirects() {
        use axum::{Router, response::Redirect, routing::get};
        let f = FaviconFetcher {
            client: fetch_client_with(true, true, true),
            allow_private: true,
            https_only: true,
        };
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = url::Url::parse(&format!("http://{}/", l.local_addr().unwrap())).unwrap();
        let app = Router::new()
            .route("/", get(|| async { Redirect::temporary("/favicon.ico") }))
            .route("/favicon.ico", get(|| async { &[0u8, 0, 1, 0, 1, 0][..] }));
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        assert!(!f.allowed(&base));
        assert!(f.allowed(&url::Url::parse("https://example.com/").unwrap()));
        assert_eq!(f.fetch(&base).await, None);
        assert_eq!(f.icon(&base.join("/favicon.ico").unwrap()).await, None);
        assert!(
            f.get(base.clone()).await.is_err(),
            "redirect to http followed"
        );
    }

    fn links(html: &str) -> Vec<String> {
        let base = url::Url::parse("https://example.com/dir/page").unwrap();
        icon_links(html, &base)
            .into_iter()
            .map(|u| u.to_string())
            .collect()
    }

    #[test]
    fn origin_keeps_only_https_origin() {
        assert_eq!(
            origin("https://Example.com:8443/a/b?q=1#x")
                .unwrap()
                .as_str(),
            "https://example.com:8443/"
        );
        assert_eq!(
            origin(" https://example.com ").unwrap().as_str(),
            "https://example.com/"
        );
        for bad in [
            "http://example.com/",
            "ftp://example.com/",
            "javascript:alert(1)",
            "https://user:pw@example.com/",
            "https://127.0.0.1/",
            "https://localhost/",
            "https://[::1]/",
            "example.com",
            "",
        ] {
            assert!(origin(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn link_parsing() {
        let html = r#"<!doctype html><html><head>
            <!-- <link rel="icon" href="/commented.png"> -->
            <script>document.write('<link rel="icon" href="/scripted.png">')</script>
            <LINK REL="Shortcut Icon" HREF="/favicon.ico">
            <link rel=stylesheet href=/style.css>
            <link rel="icon" type="image/svg+xml" href="/icon.svg">
            <link rel="mask-icon" href="/mask.png">
            <link rel='apple-touch-icon' sizes='180x180' href='apple.png'>
            <link rel="icon" sizes="32x32 16x16" href="/i.png?a=1&amp;b=2">
            <link rel="icon" sizes="8x8" href="/tiny.png">
            <link rel="icon" href="http://example.com/plain.png">
            <link rel="icon" href="data:image/png;base64,AAAA">
            <link rel="icon" href="https://cdn.example.net/x.gif"/>
        </head><body><link rel="icon" href="/body.png"></body></html>"#;
        assert_eq!(
            links(html),
            [
                "https://example.com/i.png?a=1&b=2",
                "https://example.com/favicon.ico",
                "https://example.com/dir/apple.png",
                "https://example.com/tiny.png",
                "https://cdn.example.net/x.gif",
            ]
        );
    }

    #[test]
    fn link_parsing_edge_cases() {
        assert!(links("").is_empty());
        assert!(links("<html><head><link rel=icon").is_empty());
        assert!(links("<link rel=\"icon\" href=\"/unterminated").is_empty());
        assert_eq!(
            links("<link rel=icon href=/a.png><linkfoo rel=icon href=/b.png>"),
            ["https://example.com/a.png"]
        );
        // A `>` inside a quoted value does not end the tag; non-ASCII survives.
        assert_eq!(
            links("<link title=\"a>b\" rel=\"icon\" href=\"/ä.png\">"),
            ["https://example.com/%C3%A4.png"]
        );
    }

    #[test]
    fn magic_bytes() {
        assert_eq!(sniff(&[0, 0, 1, 0, 1, 0]), Some("image/x-icon"));
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n\0\0"), Some("image/png"));
        assert_eq!(sniff(b"GIF89a.."), Some("image/gif"));
        assert_eq!(sniff(b"GIF87a.."), Some("image/gif"));
        assert_eq!(sniff(&[0xff, 0xd8, 0xff, 0xe0]), Some("image/jpeg"));
        assert_eq!(sniff(b"RIFF\x10\0\0\0WEBPVP8 "), Some("image/webp"));
        for bad in [
            &b""[..],
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>",
            b"<?xml version=\"1.0\"?><svg/>",
            b"<!doctype html><html>",
            b"RIFF\x10\0\0\0WAVE",
            &[0, 0, 2, 0],
            b"\x89PNG",
        ] {
            assert_eq!(sniff(bad), None, "{bad:?}");
        }
    }
}
