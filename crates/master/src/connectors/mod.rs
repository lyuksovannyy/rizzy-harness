//! Custom connectors (ADOPTION_PLAN X.35 step 1, PLAN.md T84, README
//! "Custom connectors"): a workspace's admins add a remote MCP server by its
//! address; each member connects their own account to it (a token they
//! paste, for a connector that signs in with one) and sees what it offers.
//! In this version no agent uses a connector and nothing reaches a node.
//!
//! * **The connector** ([`check_name`], [`check_url`], [`check_header`]): a
//!   name by the node's MCP server rule (so step 2 can name its tools
//!   `mcp__<name>__<tool>`), an `https` URL on a public name with no port,
//!   user, query or fragment, and how a person signs in: `none`, or
//!   `token` in `Authorization: Bearer …` or a header the admin names.
//! * **The call** ([`check`]): the rules of `crate::remote` (public
//!   addresses only on every connection, same-origin redirects not
//!   followed, no proxy, connect and total time limits, no idle
//!   connection), through our own HTTP client for rmcp's transport
//!   (`client`): one [`CHECK_BYTES`] budget for the whole check, error
//!   bodies never read, our headers only, no standalone stream. It runs on
//!   a runtime of its own on a thread of its own whose log output goes
//!   nowhere (rmcp logs what servers send: names, instructions, results),
//!   and that runtime is dropped when the check ends, so no task, socket
//!   or buffer of it outlives it ([`Checked::tasks_left`] says how many
//!   were still there when it ended, for tests).
//! * **What it offers** ([`Offer`]): the server's name and version, its
//!   instructions and its tools (name, description), cleaned and capped,
//!   kept only in a small per-person cache in memory ([`Cache`]).
//! * **Abuse**: checks are limited per person and (failed ones) per
//!   connector (`crate::remote::Attempts`).

pub mod client;

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use harness_core::Secret;
use reqwest::header::{HeaderName, HeaderValue};
use serde::Serialize;

use crate::remote::{self, Attempts, Lookup};
use crate::{Master, lock};
use client::{Budget, Failure, Http, TokenHeader};

/// The bytes one check may read, all its requests together (initialize,
/// the initialized notification, every `tools/list` page), counted on the
/// raw bodies before any parsing. A tool list of [`MAX_TOOLS`] with long
/// descriptions and schemas fits many times over. Step 2 (calling a
/// connector's tools) will need its own budget per call, sized by what a
/// tool's result may be and what the model is shown of it: not decided
/// here.
pub const CHECK_BYTES: usize = 1024 * 1024;
/// The time of each request, its answer's body included (the forge's), and
/// of the whole check; connecting within `remote::CONNECT_TIMEOUT`.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
/// `tools/list` pages read; past them, "and more".
pub const MAX_PAGES: usize = 8;
/// Tools shown; past them, "and more".
pub const MAX_TOOLS: usize = 64;
const MAX_TOOL_NAME: usize = 128;
const MAX_DESCRIPTION: usize = 512;
/// All descriptions of one offer together, in characters.
const MAX_TEXT: usize = 32 * 1024;
const MAX_SERVER_TEXT: usize = 128;
const MAX_INSTRUCTIONS: usize = 1024;
/// Connectors one workspace may have.
pub const MAX_PER_WORKSPACE: u64 = 50;
/// Accounts one person may have, all workspaces together.
pub const MAX_ACCOUNTS: u64 = 100;
/// The longest token taken.
pub const MAX_TOKEN: usize = 2048;
/// The longest URL taken.
pub const MAX_URL: usize = 512;
/// Checks one person may start per window.
pub const PER_USER: usize = 20;
/// Failed checks against one connector per window, everyone together.
pub const PER_CONNECTOR: usize = 60;
pub const WINDOW: Duration = Duration::from_secs(600);
/// How long a check's offer is kept for its person, and how many.
pub const CACHE_TTL: Duration = Duration::from_secs(600);
pub const MAX_CACHED: usize = 256;

const LIMITS: remote::Limits = remote::Limits {
    per_user: PER_USER,
    per_target: PER_CONNECTOR,
    window: WINDOW,
    max_tracked: 10_000,
    busy: "too many connector checks on this server right now; try again in a few minutes",
    user_over: "too many connector checks from your account; try again in a few minutes",
    target_over: "too many failed checks against this connector; try again in a few minutes",
};

/// Log lines of rmcp's HTTP client transport never reach the server's log
/// (they can carry a server's session id or a remote error's text). A
/// backstop: the check already runs where nothing is logged
/// ([`check`]). Only this target: the node's MCP client (stdio, in the
/// same process when a node is embedded) logs as before.
pub const QUIET_TARGET: &str = "rmcp::transport::streamable_http_client";

/// Whether a log line of `target` may be written (see [`QUIET_TARGET`]).
pub fn log_allowed(target: &str) -> bool {
    !target.starts_with(QUIET_TARGET)
}

/// A connector's name: the node's MCP server rule.
pub fn check_name(name: &str) -> Result<(), String> {
    if harness_core::agent::valid_mcp_server_name(name) {
        Ok(())
    } else {
        Err(
            "a connector's name is 1 to 32 of a-z, 0-9, '-' and '_', starting and ending with a \
             letter or digit, without '__'"
                .into(),
        )
    }
}

/// A connector's URL, as stored: `https://` on a lower-case public DNS
/// name (at least one dot, no `xn--` label, not `localhost`), no port, no
/// user or password, no query, no fragment, at most [`MAX_URL`] bytes.
pub fn check_url(raw: &str) -> Result<String, String> {
    const RULE: &str = "the address is https://, a public host name (no IP address, no \
                        localhost, no port), no user name or password, no ? query and no # part";
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > MAX_URL {
        return Err(format!("the address is 1 to {MAX_URL} characters"));
    }
    let u = reqwest::Url::parse(raw).map_err(|_| RULE.to_string())?;
    let host = match u.host_str() {
        Some(h) if !h.starts_with('[') && h.parse::<std::net::IpAddr>().is_err() => h.to_string(),
        _ => return Err(RULE.into()),
    };
    let lower = raw.to_ascii_lowercase();
    // The address must start `https://` as written (so any other scheme is
    // refused here), and a port, even `:443` (which the parser drops), too.
    if !u.username().is_empty()
        || u.password().is_some()
        || lower
            .strip_prefix("https://")
            .and_then(|r| r.split('/').next())
            .is_none_or(|authority| authority.contains(':'))
        || u.query().is_some()
        || u.fragment().is_some()
        || !crate::forge::valid_host(&host)
        // `localhost` has no dot.
        || !host.contains('.')
        || host.ends_with(".localhost")
        || host.split('.').any(|l| l.starts_with("xn--"))
    {
        return Err(RULE.into());
    }
    let out = u.to_string();
    if out.len() > MAX_URL {
        return Err(format!("the address is 1 to {MAX_URL} characters"));
    }
    Ok(out)
}

/// Headers a token may not go in: ones the HTTP layer or the transport
/// sets, and hop-by-hop ones.
const REFUSED_HEADERS: &[&str] = &[
    "host",
    "content-length",
    "content-type",
    "cookie",
    "transfer-encoding",
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "trailer",
    "upgrade",
    "accept",
    "user-agent",
    "mcp-session-id",
    "mcp-protocol-version",
    "last-event-id",
];

/// The header an admin names for the token: a header name of at most 64
/// characters, not one of [`REFUSED_HEADERS`]; stored lower-case.
pub fn check_header(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    let name = HeaderName::from_bytes(raw.as_bytes())
        .ok()
        .filter(|_| (1..=64).contains(&raw.len()))
        .ok_or_else(|| {
            "a header name is 1 to 64 of letters, digits and - (for example X-API-Key)".to_string()
        })?;
    let s = name.as_str().to_string();
    if REFUSED_HEADERS.contains(&s.as_str()) || s.starts_with("mcp-") || s.starts_with("sec-") {
        return Err(format!("the token cannot go in the {raw} header"));
    }
    Ok(s)
}

/// The header the token is sent in: `authorization` takes `Bearer <token>`,
/// any other the token as it is. Marked sensitive.
pub fn token_header(header: Option<&str>, token: &Secret<String>) -> Result<TokenHeader, Failure> {
    let name = header.unwrap_or("authorization");
    let value = if name == "authorization" {
        format!("Bearer {}", token.expose())
    } else {
        token.expose().clone()
    };
    let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| Failure::BadToken)?;
    let mut value = HeaderValue::from_str(&value).map_err(|_| Failure::BadToken)?;
    value.set_sensitive(true);
    Ok(TokenHeader { name, value })
}

/// A pasted token: 1 to [`MAX_TOKEN`] visible ASCII characters.
pub fn clean_token(t: &str) -> Result<Secret<String>, String> {
    remote::clean_token(t, MAX_TOKEN)
}

/// One tool, as a person sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OfferedTool {
    /// The server's name for it, cleaned and capped.
    pub name: String,
    /// The name an agent would see it under (step 2), always
    /// `mcp__<connector>__…`, whatever the server calls it.
    pub agent_name: String,
    pub description: Option<String>,
}

/// What a connector offers, as one check found it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Offer {
    pub server_name: Option<String>,
    pub server_version: Option<String>,
    /// The server's instructions, as text; stored nowhere.
    pub instructions: Option<String>,
    pub tools: Vec<OfferedTool>,
    /// The server has more than was read or shown.
    pub more: bool,
    pub checked_ms: i64,
}

/// Where and how a check connects.
#[derive(Clone)]
pub struct Net {
    pub lookup: Lookup,
    /// Tests only: connect to private addresses too.
    pub allow_private: bool,
    /// Tests only: trust only these certificates (DER).
    pub roots: Vec<Vec<u8>>,
    pub timeout: Duration,
}

impl Default for Net {
    fn default() -> Self {
        Self {
            lookup: Lookup::default(),
            allow_private: false,
            roots: Vec::new(),
            timeout: CHECK_TIMEOUT,
        }
    }
}

/// A check's outcome and what it left behind.
#[derive(Debug)]
pub struct Checked {
    pub result: Result<Offer, Failure>,
    /// Tasks still alive on the check's runtime when it ended (before the
    /// runtime, and they with it, was dropped).
    pub tasks_left: usize,
    /// Bytes of the budget used.
    pub bytes_read: usize,
}

/// Run one check of the connector `name` at `url` with `token` (a header,
/// or none). Never logs; see the module comment.
pub async fn check(name: &str, url: &str, token: Option<TokenHeader>, net: &Net) -> Checked {
    let (name, url, net) = (name.to_string(), url.to_string(), net.clone());
    let (tx, rx) = tokio::sync::oneshot::channel();
    let spawned = std::thread::Builder::new()
        .name("connector-check".into())
        .spawn(move || {
            let _quiet =
                tracing::subscriber::set_default(tracing::subscriber::NoSubscriber::default());
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            let budget = Budget::new(CHECK_BYTES);
            let b2 = budget.clone();
            let timeout = net.timeout;
            let result = rt.block_on(async move {
                match tokio::time::timeout(timeout, run(&name, &url, token, &net, b2)).await {
                    Ok(r) => r,
                    Err(_) => Err(Failure::Timeout),
                }
            });
            // Let what was closed finish closing, then count what is left.
            rt.block_on(async {
                for _ in 0..20 {
                    tokio::task::yield_now().await;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            });
            let tasks_left = rt.metrics().num_alive_tasks();
            rt.shutdown_timeout(Duration::from_secs(1));
            let _ = tx.send(Checked {
                result,
                tasks_left,
                bytes_read: CHECK_BYTES - budget.left(),
            });
        });
    if spawned.is_err() {
        return Checked {
            result: Err(Failure::Connect),
            tasks_left: 0,
            bytes_read: 0,
        };
    }
    rx.await.unwrap_or(Checked {
        result: Err(Failure::Protocol),
        tasks_left: 0,
        bytes_read: 0,
    })
}

async fn run(
    name: &str,
    url: &str,
    token: Option<TokenHeader>,
    net: &Net,
    budget: Arc<Budget>,
) -> Result<Offer, Failure> {
    use rmcp::model::{ClientCapabilities, ClientConfig, Implementation, PaginatedRequestParams};
    use rmcp::service::{ClientLifecycleMode, ClientServiceExt as _};
    use rmcp::transport::StreamableHttpClientTransport;
    use rmcp::transport::common::client_side_sse::NeverRetry;
    use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;

    // The rules once more, on what is about to be connected to.
    check_url(url).map_err(|_| Failure::Connect)?;
    let c = remote::client(net.timeout, &net.lookup, !net.allow_private, &net.roots)
        .map_err(|_| Failure::Connect)?;
    let http = Http {
        client: c,
        url: url.into(),
        token,
        budget,
        failure: Arc::default(),
    };
    let mut cfg = StreamableHttpClientTransportConfig::with_uri(url.to_string());
    cfg.retry_config = Arc::new(NeverRetry::default());
    cfg.reinit_on_expired_session = false;
    cfg.max_concurrent_requests = 1;
    cfg.channel_buffer_capacity = 4;
    let transport = StreamableHttpClientTransport::with_client(http.clone(), cfg);
    // No capability at all: no sampling, no roots, no elicitation. The
    // newest version that has the initialize handshake: a later one asks
    // for per-request headers (Mcp-Method, Mcp-Name, Mcp-Param-*) that the
    // client does not send.
    let info = ClientConfig::new(
        ClientCapabilities::default(),
        Implementation::new(harness_core::APP_NAME, env!("CARGO_PKG_VERSION")),
    )
    .with_protocol_version(rmcp::model::ProtocolVersion::LATEST_WITH_INITIALIZE);
    let failed = |http: &Http| http.first_failure().unwrap_or(Failure::Protocol);
    let service = info
        .serve_with_lifecycle(transport, ClientLifecycleMode::Initialize)
        .await
        .map_err(|_| failed(&http))?;
    let peer = service.peer_info();
    let server = peer.as_ref().and_then(|p| p.server_info.as_ref());
    let mut offer = Offer {
        server_name: server.and_then(|s| remote::clean_text(&s.name, MAX_SERVER_TEXT)),
        server_version: server.and_then(|s| remote::clean_text(&s.version, MAX_SERVER_TEXT)),
        instructions: peer
            .as_ref()
            .and_then(|p| p.instructions.as_deref())
            .and_then(|s| remote::clean_text(s, MAX_INSTRUCTIONS)),
        tools: Vec::new(),
        more: false,
        checked_ms: harness_core::now_ms(),
    };
    let mut cursor = None;
    let mut seen = std::collections::HashSet::new();
    let mut text = 0usize;
    for page in 0.. {
        if page == MAX_PAGES {
            offer.more = true;
            break;
        }
        let params = PaginatedRequestParams::default().with_cursor(cursor);
        let r = match service.list_tools(Some(params)).await {
            Ok(r) => r,
            // Pages already read stand; the budget running out is "and
            // more", any other failure of a later page too.
            Err(_) if page > 0 => {
                offer.more = true;
                break;
            }
            Err(_) => {
                let f = failed(&http);
                let _ = service.cancel().await;
                return Err(f);
            }
        };
        for t in r.tools {
            if offer.tools.len() >= MAX_TOOLS {
                offer.more = true;
                break;
            }
            if !seen.insert(t.name.to_string()) {
                continue;
            }
            let Some(shown) = remote::clean_text(&t.name, MAX_TOOL_NAME) else {
                continue;
            };
            let description = t
                .description
                .as_deref()
                .and_then(|d| remote::clean_text(d, MAX_DESCRIPTION))
                .filter(|d| {
                    let fits = text + d.chars().count() <= MAX_TEXT;
                    if fits {
                        text += d.chars().count();
                    } else {
                        offer.more = true;
                    }
                    fits
                });
            offer.tools.push(OfferedTool {
                agent_name: harness_tools::mcp::public_name(name, &t.name),
                name: shown,
                description,
            });
        }
        cursor = r.next_cursor;
        if cursor.is_none() || offer.more {
            break;
        }
    }
    let _ = service.cancel().await;
    Ok(offer)
}

/// Offers by (person, connector), for a short while, in memory only.
#[derive(Default)]
pub struct Cache {
    map: HashMap<(String, String), (Instant, Offer)>,
    order: VecDeque<(String, String)>,
}

impl Cache {
    pub fn put(&mut self, user: &str, connector: &str, offer: Offer) {
        let k = (user.to_string(), connector.to_string());
        if self
            .map
            .insert(k.clone(), (Instant::now(), offer))
            .is_none()
        {
            self.order.push_back(k);
        }
        while self.map.len() > MAX_CACHED {
            match self.order.pop_front() {
                Some(old) => {
                    self.map.remove(&old);
                }
                None => break,
            }
        }
    }

    pub fn get(&self, user: &str, connector: &str) -> Option<Offer> {
        self.map
            .get(&(user.to_string(), connector.to_string()))
            .filter(|(at, _)| at.elapsed() < CACHE_TTL)
            .map(|(_, o)| o.clone())
    }

    /// Forget one person's offer of `connector`, or everyone's (`None`).
    pub fn forget(&mut self, user: Option<&str>, connector: &str) {
        self.map
            .retain(|(u, c), _| !(c == connector && user.is_none_or(|x| x == u)));
        self.order
            .retain(|(u, c)| !(c == connector && user.is_none_or(|x| x == u)));
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// Master-side state: the limits, the cache and the test hooks.
pub struct State {
    attempts: Attempts,
    cache: Mutex<Cache>,
    net: Mutex<Net>,
    tasks_left: AtomicUsize,
}

impl Default for State {
    fn default() -> Self {
        Self {
            attempts: Attempts::new(LIMITS),
            cache: Mutex::default(),
            net: Mutex::default(),
            tasks_left: AtomicUsize::new(0),
        }
    }
}

impl Master {
    /// Tests: connect to a fake server (`table`: names to addresses with
    /// their ports), allowing private addresses when `allow_private`,
    /// trusting only `root` (DER) when given, cutting checks after
    /// `timeout`. Rust only: no route or setting reaches it.
    pub fn set_connector_test_net(
        &self,
        table: HashMap<String, Vec<SocketAddr>>,
        allow_private: bool,
        root: Option<Vec<u8>>,
        timeout: Duration,
    ) {
        *lock(&self.inner.connectors.net) = Net {
            lookup: Lookup::table(table),
            allow_private,
            roots: root.into_iter().collect(),
            timeout,
        };
    }

    /// Tests: the names connector checks looked up.
    pub fn connector_names_asked(&self) -> Vec<String> {
        lock(&self.inner.connectors.net).lookup.asked()
    }

    /// Tests: tasks the last check left when it ended.
    pub fn connector_tasks_left(&self) -> usize {
        self.inner.connectors.tasks_left.load(Ordering::SeqCst)
    }

    /// Tests: offers held in memory.
    pub fn connector_offers_cached(&self) -> usize {
        lock(&self.inner.connectors.cache).len()
    }

    pub(crate) fn connector_net(&self) -> Net {
        lock(&self.inner.connectors.net).clone()
    }

    pub(crate) fn connector_attempt(&self, user: &str, id: &str) -> Result<(), &'static str> {
        self.inner.connectors.attempts.admit(user, id)
    }

    pub(crate) fn connector_failed(&self, id: &str) {
        self.inner.connectors.attempts.failed(id)
    }

    pub(crate) fn connector_checked(&self, c: &Checked) {
        self.inner
            .connectors
            .tasks_left
            .store(c.tasks_left, Ordering::SeqCst);
    }

    pub(crate) fn connector_offer(&self, user: &str, id: &str) -> Option<Offer> {
        lock(&self.inner.connectors.cache).get(user, id)
    }

    pub(crate) fn connector_offer_put(&self, user: &str, id: &str, o: Offer) {
        lock(&self.inner.connectors.cache).put(user, id, o)
    }

    pub(crate) fn connector_offer_forget(&self, user: Option<&str>, id: &str) {
        lock(&self.inner.connectors.cache).forget(user, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_https_public_names_without_extras() {
        assert_eq!(
            check_url(" https://MCP.Example.com/mcp ").unwrap(),
            "https://mcp.example.com/mcp"
        );
        assert_eq!(
            check_url("https://mcp.example.com").unwrap(),
            "https://mcp.example.com/"
        );
        for bad in [
            "http://mcp.example.com/mcp",
            "https://203.0.113.5/mcp",
            "https://8.8.8.8/mcp",
            "https://[2606:4700::1111]/mcp",
            "https://localhost/mcp",
            "https://mcp.localhost/mcp",
            "https://intranet/mcp",
            "https://user:pw@mcp.example.com/mcp",
            "https://tok@mcp.example.com/mcp",
            "https://mcp.example.com:8443/mcp",
            "https://mcp.example.com:443/mcp",
            "https://mcp.example.com/mcp#x",
            "https://mcp.example.com/mcp?key=1",
            "https://xn--mcp-dqa.example.com/mcp",
            "https://a_b.example.com/mcp",
            "https://mcp.example.com/mcp?",
            "https://mcp.example.com/mcp#",
            "https://mсp.example.com/mcp",
            "ftp://mcp.example.com/",
            "mcp.example.com/mcp",
            "",
            &format!("https://mcp.example.com/{}", "a".repeat(MAX_URL)),
            "https://2130706433/mcp",
            "https://0x7f.1/mcp",
        ] {
            assert!(check_url(bad).is_err(), "{bad} was taken");
        }
    }

    #[test]
    fn token_headers_are_checked_names() {
        assert_eq!(check_header("X-API-Key").unwrap(), "x-api-key");
        assert_eq!(check_header("Authorization").unwrap(), "authorization");
        for bad in [
            "Host",
            "content-length",
            "Cookie",
            "Transfer-Encoding",
            "Connection",
            "Upgrade",
            "Proxy-Authorization",
            "Mcp-Session-Id",
            "mcp-anything",
            "Sec-Fetch-Site",
            "X-Key\r\nEvil: 1",
            "X-Key: 1",
            "X Key",
            "",
            &"x".repeat(65),
        ] {
            assert!(check_header(bad).is_err(), "{bad:?} was taken");
        }
        let t = Secret::new("abc".into());
        let h = token_header(None, &t).unwrap();
        assert_eq!(
            (h.name.as_str(), h.value.to_str().unwrap()),
            ("authorization", "Bearer abc")
        );
        assert!(h.value.is_sensitive());
        let h = token_header(Some("x-api-key"), &t).unwrap();
        assert_eq!(
            (h.name.as_str(), h.value.to_str().unwrap()),
            ("x-api-key", "abc")
        );
        assert!(h.value.is_sensitive());
        for bad in [
            "",
            "a b",
            "a\r\nX-Evil: 1",
            "tökén",
            &"a".repeat(MAX_TOKEN + 1),
        ] {
            assert!(clean_token(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn names_follow_the_node_rule() {
        assert!(check_name("linear").is_ok());
        for bad in [
            "Linear",
            "a__b",
            "-a",
            "mcp__x",
            "",
            &"a".repeat(33),
            "lineаr",
        ] {
            assert!(check_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_log_backstop_is_this_transport_only() {
        assert!(!log_allowed("rmcp::transport::streamable_http_client"));
        assert!(!log_allowed(
            "rmcp::transport::streamable_http_client::something"
        ));
        assert!(log_allowed("rmcp::service"));
        assert!(log_allowed("rmcp::transport::async_rw"));
        assert!(log_allowed("harness_master::connectors"));
    }

    #[test]
    fn the_cache_is_bounded_and_forgets() {
        let o = Offer {
            server_name: None,
            server_version: None,
            instructions: None,
            tools: vec![],
            more: false,
            checked_ms: 0,
        };
        let mut c = Cache::default();
        for i in 0..(MAX_CACHED + 10) {
            c.put(&format!("u{i}"), "k", o.clone());
        }
        assert_eq!(c.len(), MAX_CACHED);
        assert!(c.get("u0", "k").is_none());
        assert!(c.get(&format!("u{}", MAX_CACHED + 9), "k").is_some());
        c.put("alice", "k2", o.clone());
        c.put("bob", "k2", o.clone());
        c.forget(Some("alice"), "k2");
        assert!(c.get("alice", "k2").is_none() && c.get("bob", "k2").is_some());
        c.forget(None, "k2");
        assert!(c.get("bob", "k2").is_none());
    }
}
