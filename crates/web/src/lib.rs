//! Web tools for chats: `web_search` (SearXNG or Brave) and `web_fetch`.
//!
//! `web_fetch` is the dangerous one: the model chooses the URL. Two guards:
//! * **No private targets.** Every hostname is resolved by our resolver,
//!   which drops loopback, private, link-local, CGNAT, multicast and other
//!   non-public addresses (also for each redirect); IP-literal URLs are
//!   checked the same way. Environment proxies are ignored so the check
//!   cannot be bypassed by one.
//! * **URL provenance.** A URL is fetched only if it appears in text the
//!   user or a tool supplied (`ToolCtx::observed`). A prompt-injected page
//!   cannot make the model build `https://evil/?q=<your data>`.
//!
//! Every page in a result gets a session-stable citation label (`[S4]`, see
//! `sources`), recorded with its URL on the tool result so the UI resolves
//! the model's citations to links it did not write.

pub mod favicon;
mod sources;

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use harness_core::policy::ToolRisk;
use harness_tools::{Tool, ToolCtx, ToolOutcome};
use serde::Serialize;
use serde_json::{Value, json};
use sources::cite_hint;

/// Largest response body read, in bytes.
pub const MAX_BODY: usize = 2 * 1024 * 1024;
/// Largest text handed to the model from one page.
pub const MAX_PAGE_CHARS: usize = 20_000;
const TIMEOUT: Duration = Duration::from_secs(20);
const USER_AGENT: &str = concat!("harness/", env!("CARGO_PKG_VERSION"), " (web_fetch)");

/// True for addresses on the public internet.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
                || o[0] == 0
                || (o[0] == 100 && (64..128).contains(&o[1])) // CGNAT, tailnets
                || (o[0] == 192 && o[1] == 0 && o[2] == 0) // IETF protocol assignments
                || (o[0] == 198 && (o[1] == 18 || o[1] == 19)) // benchmarking
                || o[0] >= 240) // reserved
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let s = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // unique local
                || (s[0] & 0xffc0) == 0xfe80 // link local
                || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
                || (s[0] == 0x64 && s[1] == 0xff9b)) // NAT64 can reach private v4
        }
    }
}

/// DNS resolver that refuses names resolving only to non-public addresses.
struct PublicOnly;

impl reqwest::dns::Resolve for PublicOnly {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|a| is_public(a.ip()))
                .collect();
            if addrs.is_empty() {
                return Err(format!("{host} does not resolve to a public address").into());
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

fn check_url(u: &url::Url, allow_private: bool) -> Result<(), String> {
    if !matches!(u.scheme(), "http" | "https") {
        return Err(format!(
            "only http and https URLs can be fetched, not {}",
            u.scheme()
        ));
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err("URLs with credentials are not fetched".into());
    }
    match u.host() {
        None => Err("URL has no host".into()),
        Some(url::Host::Ipv4(ip)) if !allow_private && !is_public(IpAddr::V4(ip)) => {
            Err("private and local addresses are not fetched".into())
        }
        Some(url::Host::Ipv6(ip)) if !allow_private && !is_public(IpAddr::V6(ip)) => {
            Err("private and local addresses are not fetched".into())
        }
        Some(url::Host::Domain(d))
            if !allow_private && (d == "localhost" || d.ends_with(".localhost")) =>
        {
            Err("private and local addresses are not fetched".into())
        }
        _ => Ok(()),
    }
}

/// The resolver behind `web_fetch`, for the server's other clients that
/// must reach public addresses only (provider and model tests, AUDIT L3).
/// It checks the address that is connected to, so DNS rebinding does not
/// get past it; IP-literal hosts never reach a resolver, so check those
/// with [`check_public_url`].
pub fn public_only_resolver() -> Arc<dyn reqwest::dns::Resolve> {
    Arc::new(PublicOnly)
}

/// The URL check behind `web_fetch`: http(s), no credentials, and no
/// private, loopback or `localhost` host when it is a literal.
pub fn check_public_url(u: &url::Url) -> Result<(), String> {
    check_url(u, false)
}

/// HTTP client for model-chosen URLs. `allow_private` exists for tests only;
/// `private_redirects` separately controls whether redirects may go inward.
fn fetch_client(allow_private: bool, private_redirects: bool) -> reqwest::Client {
    fetch_client_with(allow_private, private_redirects, false)
}

/// [`fetch_client`], optionally refusing any redirect to a non-https URL.
fn fetch_client_with(
    allow_private: bool,
    private_redirects: bool,
    https_only: bool,
) -> reqwest::Client {
    let policy = reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= 5 {
            return attempt.error("too many redirects");
        }
        if https_only && attempt.url().scheme() != "https" {
            return attempt.error("redirect to non-https refused");
        }
        match check_url(attempt.url(), private_redirects) {
            Ok(()) => attempt.follow(),
            Err(e) => attempt.error(e),
        }
    });
    let mut b = reqwest::Client::builder()
        .no_proxy()
        .redirect(policy)
        .timeout(TIMEOUT)
        .user_agent(USER_AGENT);
    if !allow_private {
        b = b.dns_resolver(Arc::new(PublicOnly));
    }
    b.build().expect("static client config")
}

/// The URL forms we look for in observed text: no scheme, no fragment, no
/// trailing slash. `https://a.com/x/#y` -> `a.com/x`.
fn url_key(u: &url::Url) -> String {
    let mut s = u.as_str().to_string();
    if let Some(i) = s.find('#') {
        s.truncate(i);
    }
    let s = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .unwrap_or(&s)
        .trim_end_matches('/')
        .to_string();
    s.strip_prefix("www.").map(str::to_string).unwrap_or(s)
}

/// Was this URL given by the user or a tool (not invented by the model)?
pub fn provenance_ok(u: &url::Url, observed: &[String]) -> bool {
    let key = url_key(u);
    if key.is_empty() {
        return false;
    }
    observed.iter().any(|t| {
        // Compare against each URL-ish token in the text, normalised the same way.
        t.split(|c: char| c.is_whitespace() || "<>\"'`()[]{}|".contains(c))
            .map(|tok| tok.trim_end_matches(['.', ',', ';', ':', '!', '?']))
            .filter(|tok| tok.contains('.'))
            .any(|tok| {
                let with_scheme = if tok.starts_with("http://") || tok.starts_with("https://") {
                    tok.to_string()
                } else {
                    format!("https://{tok}")
                };
                url::Url::parse(&with_scheme)
                    .is_ok_and(|v| !shorthand_address(&v, tok) && url_key(&v) == key)
            })
    })
}

/// Is `tok` an IPv4 address in a shorthand the url crate expands (`1.5`
/// is 1.0.0.5, `0x7f.1` is 127.0.0.1)? Such a token is a number, not an
/// address someone gave: a form answer carries numbers the model chose.
fn shorthand_address(v: &url::Url, tok: &str) -> bool {
    let Some(url::Host::Ipv4(ip)) = v.host() else {
        return false;
    };
    let rest = tok.split_once("://").map_or(tok, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = host.split(':').next().unwrap_or("");
    host != ip.to_string()
}

/// The page's main content as Markdown (readability extraction) and its
/// title, or `None` when extraction finds too little (then the caller falls
/// back to all text). The title is not repeated in the Markdown: the tool
/// output's label line carries it.
pub fn readable_markdown(html: &str, url: &str) -> Option<(String, String)> {
    let mut r = dom_smoothie::Readability::new(html, Some(url), None).ok()?;
    let article = r.parse().ok()?;
    let md = htmd::convert(&article.content).ok()?;
    let md = md.trim();
    if md.chars().count() < 200 {
        return None;
    }
    Some((md.to_string(), article.title.trim().to_string()))
}

/// A page as text, with the title the page gives itself (if any).
struct Page {
    text: String,
    title: Option<String>,
    truncated: bool,
}

/// Read at most `MAX_BODY` bytes and turn the page into text: the main
/// article as Markdown when `readable`, else all of the page's text.
async fn read_page(resp: reqwest::Response, readable: bool) -> Result<Page, String> {
    let page_url = resp.url().to_string();
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let html = ctype.contains("html") || ctype.is_empty();
    let textual =
        html || ctype.starts_with("text/") || ctype.contains("json") || ctype.contains("xml");
    if !textual {
        return Err(format!("unsupported content type `{ctype}`"));
    }
    let mut body = Vec::new();
    let mut truncated = false;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        let room = MAX_BODY - body.len();
        if chunk.len() > room {
            body.extend_from_slice(&chunk[..room]);
            truncated = true;
            break;
        }
        body.extend_from_slice(&chunk);
    }
    let (text, title) = if html {
        let extracted = if readable {
            let (b, u) = (
                String::from_utf8_lossy(&body).into_owned(),
                page_url.clone(),
            );
            tokio::task::spawn_blocking(move || readable_markdown(&b, &u))
                .await
                .ok()
                .flatten()
        } else {
            None
        };
        match extracted {
            Some((md, title)) => (md, Some(title)),
            None => (
                html2text::from_read(&body[..], 100).map_err(|e| e.to_string())?,
                sources::html_title(&body),
            ),
        }
    } else {
        (String::from_utf8_lossy(&body).into_owned(), None)
    };
    let chars = text.chars().count();
    if chars > MAX_PAGE_CHARS {
        let cut: String = text.chars().take(MAX_PAGE_CHARS).collect();
        return Ok(Page {
            text: format!("{cut}\n\n[... page truncated ...]"),
            title,
            truncated: true,
        });
    }
    Ok(Page {
        text,
        title,
        truncated,
    })
}

pub struct WebFetch {
    client: reqwest::Client,
    allow_private: bool,
}

impl WebFetch {
    pub fn new() -> Self {
        Self {
            client: fetch_client(false, false),
            allow_private: false,
        }
    }

    /// For tests against local servers only.
    #[doc(hidden)]
    pub fn allowing_private() -> Self {
        Self {
            client: fetch_client(true, true),
            allow_private: true,
        }
    }

    /// For tests: a local start URL, but redirects held to the real rules.
    #[doc(hidden)]
    pub fn allowing_private_start() -> Self {
        Self {
            client: fetch_client(true, false),
            allow_private: true,
        }
    }
}

impl Default for WebFetch {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for WebFetch {
    fn name(&self) -> &'static str {
        "web_fetch"
    }
    fn description(&self) -> &'static str {
        concat!(
            "Fetch a web page and return its text, labelled like [S5]. Only URLs that the \
             user gave or that a previous tool result (e.g. web_search) contained can be \
             fetched.",
            cite_hint!()
        )
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "url": { "type": "string", "description": "http(s) URL" } },
            "required": ["url"]
        })
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }
    async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome {
        let Some(raw) = input.get("url").and_then(|v| v.as_str()) else {
            return ToolOutcome::err("missing string argument `url`");
        };
        let u = match url::Url::parse(raw.trim()) {
            Ok(u) => u,
            Err(e) => return ToolOutcome::err(format!("invalid URL: {e}")),
        };
        if let Err(e) = check_url(&u, self.allow_private) {
            return ToolOutcome::err(e);
        }
        if !provenance_ok(&u, &ctx.observed) {
            return ToolOutcome::err(
                "this URL did not come from the user or a search result, so it is not fetched. \
                 Search for it first or ask the user for the link.",
            );
        }
        let req = self.client.get(u.clone()).send();
        let resp = tokio::select! {
            r = req => r,
            _ = ctx.cancel.cancelled() => return ToolOutcome::err("[cancelled]"),
        };
        let resp = match resp {
            Ok(r) => r,
            Err(e) => return ToolOutcome::err(format!("fetch failed: {}", root_cause(&e))),
        };
        let status = resp.status();
        let mut final_url = resp.url().clone();
        if !status.is_success() {
            return ToolOutcome::err(format!("{final_url} answered {status}"));
        }
        // The fragment never reached the server: the page is the URL without it.
        final_url.set_fragment(None);
        match read_page(resp, ctx.savings.web_readability).await {
            Ok(page) => {
                let (output, source) =
                    sources::fetch_output(ctx, &final_url, page.title.as_deref(), &page.text);
                ToolOutcome {
                    ok: true,
                    output,
                    truncated: page.truncated,
                    sources: vec![source],
                    ..Default::default()
                }
            }
            Err(e) => ToolOutcome::err(e),
        }
    }
}

fn root_cause(e: &dyn std::error::Error) -> String {
    let mut msg = e.to_string();
    let mut src = e.source();
    while let Some(s) = src {
        msg = format!("{msg}: {s}");
        src = s.source();
    }
    msg
}

/// Where `web_search` sends queries. Configured by the admin, so it may be a
/// LAN address (a self-hosted SearXNG).
#[derive(Debug, Clone)]
pub enum SearchBackend {
    /// SearXNG JSON API (`format=json` must be enabled on the instance).
    Searxng { base_url: String },
    /// Brave Search API. The key is redacted in `Debug`.
    Brave {
        api_key: harness_core::Secret<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

pub struct WebSearch {
    backend: SearchBackend,
    client: reqwest::Client,
    /// Brave's endpoint; overridable for tests.
    brave_url: String,
}

impl WebSearch {
    pub fn new(backend: SearchBackend) -> Self {
        Self {
            backend,
            client: reqwest::Client::builder()
                .timeout(TIMEOUT)
                .user_agent(USER_AGENT)
                .build()
                .expect("static client config"),
            brave_url: "https://api.search.brave.com/res/v1/web/search".into(),
        }
    }

    pub fn with_brave_url(mut self, url: &str) -> Self {
        self.brave_url = url.into();
        self
    }

    pub async fn search(&self, q: &str, count: usize) -> Result<Vec<SearchResult>, String> {
        // Engines pass on whatever the pages say: titles and snippets are
        // cleaned to plain lines, and a hit without an http(s) URL is dropped.
        let take = |v: &Value, title: &str, url: &str, snip: &str| -> Vec<SearchResult> {
            v.as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|r| {
                            let text = |k: &str| r.get(k).and_then(|s| s.as_str()).map(strip_tags);
                            sources::search_result(
                                &text(title)?,
                                r.get(url)?.as_str()?,
                                &text(snip).unwrap_or_default(),
                            )
                        })
                        .take(count)
                        .collect()
                })
                .unwrap_or_default()
        };
        let n = count.to_string();
        let resp = match &self.backend {
            SearchBackend::Searxng { base_url } => {
                let u = url::Url::parse_with_params(
                    &format!("{}/search", base_url.trim_end_matches('/')),
                    &[("q", q), ("format", "json")],
                )
                .map_err(|e| format!("search URL: {e}"))?;
                self.client.get(u)
            }
            SearchBackend::Brave { api_key } => {
                let u = url::Url::parse_with_params(&self.brave_url, &[("q", q), ("count", &n)])
                    .map_err(|e| format!("search URL: {e}"))?;
                self.client
                    .get(u)
                    .header("X-Subscription-Token", api_key.expose().as_str())
                    .header("Accept", "application/json")
            }
        }
        .send()
        .await
        .map_err(|e| format!("search failed: {}", root_cause(&e)))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("search engine answered {status}"));
        }
        let v: Value = resp.json().await.map_err(|e| format!("search: {e}"))?;
        Ok(match &self.backend {
            SearchBackend::Searxng { .. } => take(&v["results"], "title", "url", "content"),
            SearchBackend::Brave { .. } => {
                take(&v["web"]["results"], "title", "url", "description")
            }
        })
    }
}

/// Brave wraps matches in `<strong>`; keep the text only.
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    sources::decode_entities(&out)
}

#[async_trait::async_trait]
impl Tool for WebSearch {
    fn name(&self) -> &'static str {
        "web_search"
    }
    fn description(&self) -> &'static str {
        concat!(
            "Search the web. Returns a label like [S2], the title, URL and snippet per \
             result; use web_fetch on a URL to read the page. Prefer this for current events \
             and facts you are unsure of.",
            cite_hint!()
        )
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "count": { "type": "integer", "minimum": 1, "maximum": 10 }
            },
            "required": ["query"]
        })
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }
    async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome {
        let Some(q) = input.get("query").and_then(|v| v.as_str()) else {
            return ToolOutcome::err("missing string argument `query`");
        };
        let count = input
            .get("count")
            .and_then(|v| v.as_u64())
            .unwrap_or(6)
            .clamp(1, 10) as usize;
        let res = tokio::select! {
            r = self.search(q, count) => r,
            _ = ctx.cancel.cancelled() => return ToolOutcome::err("[cancelled]"),
        };
        match res {
            Ok(rs) if rs.is_empty() => ToolOutcome::ok("No results."),
            Ok(rs) => {
                let (output, sources) = sources::search_output(ctx, &rs);
                ToolOutcome {
                    sources,
                    ..ToolOutcome::ok(output)
                }
            }
            Err(e) => ToolOutcome::err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_addresses_are_not_public() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fe80::1",
            "fd00::1",
            "::ffff:127.0.0.1",
            "64:ff9b::a00:1",
            "224.0.0.1",
            "255.255.255.255",
        ] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["1.1.1.1", "93.184.216.34", "2606:4700:4700::1111"] {
            assert!(is_public(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn url_checks() {
        let ok = |s: &str| check_url(&url::Url::parse(s).unwrap(), false).is_ok();
        assert!(ok("https://example.com/a"));
        assert!(!ok("file:///etc/passwd"));
        assert!(!ok("http://127.0.0.1:7777/api"));
        assert!(!ok("http://[::1]/"));
        assert!(!ok("http://169.254.169.254/latest/meta-data"));
        assert!(!ok("http://localhost:8080/"));
        assert!(!ok("http://user:pw@example.com/"));
        assert!(!ok("http://2130706433/")); // decimal 127.0.0.1, normalised by url
    }

    #[test]
    fn provenance() {
        let obs = vec![
            "look at https://docs.rs/tokio/latest/tokio/ please".to_string(),
            "1. Result\n   https://example.org/page?id=7\n   snippet".to_string(),
            "and example.net/guide.".to_string(),
        ];
        let ok = |s: &str| provenance_ok(&url::Url::parse(s).unwrap(), &obs);
        assert!(ok("https://docs.rs/tokio/latest/tokio"));
        assert!(ok("http://docs.rs/tokio/latest/tokio/#x"));
        assert!(ok("https://example.org/page?id=7"));
        assert!(ok("https://example.net/guide"));
        // Built by the model: not in anything the user or a tool said.
        assert!(!ok("https://example.org/page?id=7&leak=secret"));
        assert!(!ok("https://evil.com/?q=example.org/page"));
        assert!(!ok("https://docs.rs/"));
    }

    /// The url crate reads a number as a shorthand IPv4 address (`1.5` is
    /// 1.0.0.5), and a form answer the user sends carries numbers the model
    /// picked (a fixed default, min == max), so a number must not vouch for
    /// an address. One written out in full still does.
    #[test]
    fn numbers_are_not_addresses() {
        let obs = vec![
            "Form answer · trip\n- people: 1.5\n- budget: 3.75, 203.0113 or 0.0000001".to_string(),
            "version 2.5/x, then fetch 93.184.216.34/page".to_string(),
        ];
        let ok = |s: &str| provenance_ok(&url::Url::parse(s).unwrap(), &obs);
        for invented in [
            "https://1.0.0.5/",
            "https://3.0.0.75/",
            "https://203.0.0.75/",
            "https://0.0.0.1/",
            "https://2.0.0.5/x",
        ] {
            assert!(!ok(invented), "{invented}");
        }
        assert!(ok("https://93.184.216.34/page"));
    }
}
