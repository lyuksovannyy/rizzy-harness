//! Custom connectors (ADOPTION_PLAN X.35 step 1, PLAN.md T84, README
//! "Custom connectors"): a workspace's admins add a remote MCP server by its
//! address; each member connects their own account to it (a token they
//! paste, for a connector that signs in with one) and sees what it offers.
//! Agents call its tools through the master with the chat owner's own
//! account ([`call`], step 2, PLAN.md T85); its token never reaches a node.
//!
//! * **The connector** ([`check_name`], [`check_url`], [`check_header`]): a
//!   name by the node's MCP server rule (so step 2 can name its tools
//!   `mcp__<name>__<tool>`), an `https` URL on a public name with no port,
//!   user, query or fragment, and how a person signs in: `none`, or
//!   `token` in `Authorization: Bearer …` or a header the admin names.
//! * **The call** ([`check`]): the rules of `crate::remote` (public
//!   addresses only on every connection, no redirect followed, no proxy,
//!   nothing compressed, connect and total time limits, no idle
//!   connection), through our own HTTP client for rmcp's transport
//!   (`client`): one [`CHECK_BYTES`] budget for the whole check, error
//!   bodies never read, a value count before any parse, our headers only,
//!   no standalone stream. It runs on the one long-lived connector thread
//!   ([`pool`], two check slots) whose log output goes nowhere (rmcp logs
//!   what servers send: names, instructions, results); a check that finds
//!   no slot free, or whose person has one running, is refused at once. No
//!   task or socket of a check outlives it ([`Checked::tasks_left`] counts
//!   what was still there; tests assert none), and only the cleaned offer
//!   is kept.
//! * **What it offers** ([`Offer`]): the server's name and version, its
//!   instructions and its tools (name, description), cleaned and capped,
//!   kept only in a small per-person cache in memory ([`Cache`]), bounded
//!   by entries and by bytes.
//! * **Abuse**: checks are limited per person and (failed ones) per
//!   connector (`crate::remote::Attempts`).

pub mod call;
pub mod client;
pub mod grant;
pub mod pool;
pub mod tools;

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
pub use pool::{Busy, Pool};

/// The bytes one check may read, all its requests together (initialize,
/// the initialized notification, every `tools/list` page), counted on the
/// raw bodies as they come over the wire (nothing compressed is taken),
/// before any parsing; no message bigger than this reaches the JSON
/// parser, and none with more than `client::MAX_JSON_VALUES` values. 512
/// KiB (it was 1 MiB): parsing costs tens of times the bytes, and a check
/// shows at most [`MAX_TOOLS`] tools, which with long descriptions and
/// schemas fit many times over. Step 2 (calling a connector's tools) sets
/// its own budget per call, sized by what a tool's result may be and what
/// the model is shown of it: not decided here.
pub const CHECK_BYTES: usize = 512 * 1024;
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
/// The bytes of every offer held, all people together ([`Offer::weight`]).
/// One offer at every cap weighs about 0.2 MiB, so at least 20 of those fit.
pub const MAX_CACHED_BYTES: usize = 4 * 1024 * 1024;

const LIMITS: remote::Limits = remote::Limits {
    per_user: PER_USER,
    per_target: PER_CONNECTOR,
    window: WINDOW,
    max_tracked: 10_000,
    busy: "too many connector checks on this server right now; try again in a few minutes",
    user_over: "too many connector checks from your account; try again in a few minutes",
    target_over: "too many failed checks against this connector; try again in a few minutes",
};

/// Background reads of a person's tools (X.35 step 2): their own budget,
/// smaller than a person's checks, so their failures never use up the
/// budget a person's Connect and an admin's Check need.
const FILL_LIMITS: remote::Limits = remote::Limits {
    per_user: PER_USER,
    per_target: 10,
    window: WINDOW,
    max_tracked: 10_000,
    busy: "too many background reads",
    user_over: "too many background reads",
    target_over: "too many failed background reads",
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

/// `X-` headers that proxies, load balancers and tracing add, forward or
/// log: a token never goes in one, whatever it is called.
const X_REFUSED: &[&str] = &[
    "x-forwarded-",
    "x-real-ip",
    "x-original-",
    "x-request-",
    "x-correlation-",
    "x-envoy-",
    "x-amzn-",
    "x-cloud-trace-",
];

const HEADER_RULE: &str = "the token goes in Authorization, or in a header whose name ends in -Key, \
                           -Token or -Auth (such as X-API-Key or Private-Token), or one that starts \
                           with X- and is not a forwarding or tracing header; 1 to 64 letters, digits \
                           and -";

/// The header an admin names for the token, stored lower-case. An allow
/// rule (the header decides who on the way reads the token: proxies and
/// access logs redact `Authorization` and pass or log others): exactly
/// `authorization`; or a name ending in `-key`, `-token` or `-auth`; or
/// one starting `x-` that is not in [`X_REFUSED`]. Never `sec-*`,
/// `proxy-*` or `mcp-*`, and only letters, digits and `-`.
pub fn check_header(raw: &str) -> Result<String, String> {
    let s = raw.trim().to_ascii_lowercase();
    let shape = (1..=64).contains(&s.len())
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !s.starts_with('-')
        && !s.ends_with('-');
    if !shape || HeaderName::from_bytes(s.as_bytes()).is_err() {
        return Err(HEADER_RULE.into());
    }
    let never = ["sec-", "proxy-", "mcp-"].iter().any(|p| s.starts_with(p))
        || X_REFUSED.iter().any(|p| s.starts_with(p));
    let allowed = s == "authorization"
        || ["-key", "-token", "-auth"].iter().any(|e| s.ends_with(e))
        || s.starts_with("x-");
    if never || !allowed {
        return Err(HEADER_RULE.into());
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
    /// Tools left out because the name an agent would see is another of
    /// its tools'.
    pub left_out: u32,
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
    /// The tools agents may be offered (X.35 step 2), with their schemas,
    /// as the same check read them ([`tools::AgentTool`]); empty when the
    /// check failed.
    pub tools: Vec<tools::AgentTool>,
    /// Tasks still alive on the worker's runtime when the check ended.
    pub tasks_left: usize,
    /// Bytes of the budget used.
    pub bytes_read: usize,
}

/// Run one check of the connector `name` at `url` with `token` (a header,
/// or none) for `who`, on a worker of `pool` ([`pool`]). Never logs.
pub async fn check(
    pool: &Pool,
    who: &str,
    name: &str,
    url: &str,
    token: Option<TokenHeader>,
    net: &Net,
) -> Result<Checked, Busy> {
    let (name, url, net) = (name.to_string(), url.to_string(), net.clone());
    let thread = pool.thread().clone();
    let out = pool
        .run(who, move || async move {
            let budget = Budget::new(CHECK_BYTES);
            let timeout = net.timeout;
            let (result, tools) =
                match tokio::time::timeout(timeout, run(&name, &url, token, &net, budget.clone()))
                    .await
                {
                    Ok(Ok((offer, tools))) => (Ok(offer), tools),
                    Ok(Err(f)) => (Err(f), Vec::new()),
                    Err(_) => (Err(Failure::Timeout), Vec::new()),
                };
            // Let what was closed finish closing, then count what is left.
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
            Checked {
                result,
                tools,
                tasks_left: thread.tasks_beyond_jobs(),
                bytes_read: CHECK_BYTES - budget.left(),
            }
        })
        .await?;
    Ok(out.unwrap_or(Checked {
        result: Err(Failure::Protocol),
        tools: Vec::new(),
        tasks_left: 0,
        bytes_read: 0,
    }))
}

async fn run(
    name: &str,
    url: &str,
    token: Option<TokenHeader>,
    net: &Net,
    budget: Arc<Budget>,
) -> Result<(Offer, Vec<tools::AgentTool>), Failure> {
    use rmcp::model::PaginatedRequestParams;

    let (service, http) = session(url, token, net, budget).await?;
    let failed = |http: &Http| http.first_failure().unwrap_or(Failure::Protocol);
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
        left_out: 0,
        checked_ms: harness_core::now_ms(),
    };
    let mut cursor = None;
    let mut seen = std::collections::HashSet::new();
    let mut agents = std::collections::HashSet::new();
    let mut text = 0usize;
    let mut for_agents = tools::Collect::new(name);
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
            // One raw name once; one agent name once (two names can clean
            // to the same one: the first keeps it, the other is counted).
            if !seen.insert(t.name.to_string()) {
                continue;
            }
            let agent_name = harness_tools::mcp::public_name(name, &t.name);
            if !agents.insert(agent_name.clone()) {
                offer.left_out += 1;
                continue;
            }
            for_agents.take(&t, &agent_name);
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
                agent_name,
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
    Ok((offer, for_agents.done()))
}

/// The client side of one MCP session with the connector at `url`: our
/// HTTP client on the rules of `crate::remote` (public addresses only, no
/// redirect, no proxy, nothing compressed), `token` in its header to that
/// URL only, every body counted against `budget`; `initialize` done.
pub(crate) async fn session(
    url: &str,
    token: Option<TokenHeader>,
    net: &Net,
    budget: Arc<Budget>,
) -> Result<
    (
        rmcp::service::RunningService<rmcp::RoleClient, rmcp::model::ClientConfig>,
        Http,
    ),
    Failure,
> {
    use rmcp::model::{ClientCapabilities, ClientConfig, Implementation};
    use rmcp::service::{ClientLifecycleMode, ClientServiceExt as _};
    use rmcp::transport::StreamableHttpClientTransport;
    use rmcp::transport::common::client_side_sse::NeverRetry;
    use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;

    // The rules once more, on what is about to be connected to.
    check_url(url).map_err(|_| Failure::Connect)?;
    let c = remote::client(
        net.timeout,
        &net.lookup,
        !net.allow_private,
        remote::Redirects::None,
        true,
        &net.roots,
    )
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
    let service = info
        .serve_with_lifecycle(transport, ClientLifecycleMode::Initialize)
        .await
        .map_err(|_| http.first_failure().unwrap_or(Failure::Protocol))?;
    Ok((service, http))
}

/// What a person saw of a connector before sending a token or checking,
/// and what a turn's list of its tools was made for: changes with its URL,
/// its sign-in and its header.
pub fn stamp(r: &harness_store::custom_connectors::ConnectorRow) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for part in [
        r.url.as_str(),
        r.auth.as_str(),
        r.header.as_deref().unwrap_or(""),
    ] {
        h.update((part.len() as u32).to_be_bytes());
        h.update(part.as_bytes());
    }
    hex::encode(h.finalize())[..16].to_string()
}

impl Offer {
    /// What it takes in memory, near enough: its text's bytes and a fixed
    /// amount per tool and per offer.
    pub fn weight(&self) -> usize {
        let opt = |s: &Option<String>| s.as_ref().map_or(0, String::len);
        256 + opt(&self.server_name)
            + opt(&self.server_version)
            + opt(&self.instructions)
            + self
                .tools
                .iter()
                .map(|t| 96 + t.name.len() + t.agent_name.len() + opt(&t.description))
                .sum::<usize>()
    }
}

/// Offers by (person, connector), for a short while, in memory only, at
/// most [`MAX_CACHED`] and [`MAX_CACHED_BYTES`]; the oldest go first.
#[derive(Default)]
pub struct Cache {
    map: HashMap<(String, String), (Instant, Offer)>,
    order: VecDeque<(String, String)>,
    bytes: usize,
}

impl Cache {
    pub fn put(&mut self, user: &str, connector: &str, offer: Offer) {
        let k = (user.to_string(), connector.to_string());
        let w = offer.weight();
        if let Some((_, old)) = self.map.insert(k.clone(), (Instant::now(), offer)) {
            self.bytes -= old.weight();
            self.order.retain(|o| o != &k);
        }
        self.order.push_back(k);
        self.bytes += w;
        while self.map.len() > MAX_CACHED || self.bytes > MAX_CACHED_BYTES {
            match self.order.pop_front() {
                Some(old) => {
                    if let Some((_, o)) = self.map.remove(&old) {
                        self.bytes -= o.weight();
                    }
                }
                None => break,
            }
        }
    }

    /// The weight of everything held.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn get(&self, user: &str, connector: &str) -> Option<Offer> {
        self.map
            .get(&(user.to_string(), connector.to_string()))
            .filter(|(at, _)| at.elapsed() < CACHE_TTL)
            .map(|(_, o)| o.clone())
    }

    /// Forget one person's offer of `connector`, or everyone's (`None`).
    pub fn forget(&mut self, user: Option<&str>, connector: &str) {
        let bytes = &mut self.bytes;
        self.map.retain(|(u, c), (_, o)| {
            let keep = !(c == connector && user.is_none_or(|x| x == u));
            if !keep {
                *bytes -= o.weight();
            }
            keep
        });
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

/// The table a person's sealed token is bound to (`crate::sealed`).
pub const ACCOUNTS_TABLE: &str = "connector_accounts";

/// Chats told once that connectors are off in them (bounded: past it,
/// told again).
#[derive(Default)]
pub struct Told(std::collections::HashSet<harness_core::SessionId>);

impl Told {
    /// True the first time for `sid`.
    pub fn told(&mut self, sid: harness_core::SessionId) -> bool {
        if self.0.len() >= 4096 {
            self.0.clear();
        }
        self.0.insert(sid)
    }
}

/// Master-side state: the limits, the caches, the call workers and the
/// test hooks.
pub struct State {
    attempts: Attempts,
    pool: Pool,
    cache: Mutex<Cache>,
    /// Each person's tools of each connector, with schemas (step 2).
    pub(crate) tools: Mutex<tools::Cache>,
    /// Agents' calls (step 2): their own workers, counts and grants.
    pub(crate) calls: call::State,
    /// Background reads of tools (step 2): a budget of their own.
    pub(crate) fill_attempts: Attempts,
    /// Marked chats already told that connectors are off there.
    pub(crate) told: Mutex<Told>,
    /// Name lookups that may block a thread at once: one for checks, one
    /// for calls (the connector thread has two blocking threads), so names
    /// that never resolve for calls never starve a check (review 3, L8).
    pub(crate) check_lookups: std::sync::Arc<tokio::sync::Semaphore>,
    pub(crate) call_lookups: std::sync::Arc<tokio::sync::Semaphore>,
    /// And one for background reads of tools, so a name that hangs for one
    /// never delays a person's Check (review 4, L5).
    pub(crate) fill_lookups: std::sync::Arc<tokio::sync::Semaphore>,
    net: Mutex<Net>,
    tasks_left: AtomicUsize,
}

impl Default for State {
    fn default() -> Self {
        // One connector thread for checks and calls (review of PR 157, M5).
        let thread = std::sync::Arc::new(pool::Thread::default());
        Self {
            attempts: Attempts::new(LIMITS),
            pool: Pool::on(thread.clone(), pool::WORKERS, 1),
            cache: Mutex::default(),
            tools: Mutex::default(),
            calls: call::State::on(thread),
            fill_attempts: Attempts::new(FILL_LIMITS),
            told: Mutex::default(),
            check_lookups: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
            call_lookups: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
            fill_lookups: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
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

    /// The check slots (`pool`).
    pub fn connector_pool(&self) -> &Pool {
        &self.inner.connectors.pool
    }

    /// Tests: the bytes of the offers held.
    pub fn connector_offer_bytes(&self) -> usize {
        lock(&self.inner.connectors.cache).bytes()
    }

    /// The net of a check: its lookups under the checks' gate.
    pub(crate) fn connector_net(&self) -> Net {
        let mut n = lock(&self.inner.connectors.net).clone();
        n.lookup = n.lookup.gated(self.inner.connectors.check_lookups.clone());
        n
    }

    /// The net of a background read: its lookups under their own gate.
    pub(crate) fn connector_fill_net(&self) -> Net {
        let mut n = lock(&self.inner.connectors.net).clone();
        n.lookup = n.lookup.gated(self.inner.connectors.fill_lookups.clone());
        n
    }

    /// The net of a call: its lookups under the calls' gate.
    pub(crate) fn connector_call_net(&self) -> Net {
        let mut n = lock(&self.inner.connectors.net).clone();
        n.lookup = n.lookup.gated(self.inner.connectors.call_lookups.clone());
        n
    }

    /// Tests: lookups of `name` block a thread for `d`, as a system
    /// resolver that never answers does.
    pub fn set_connector_test_stall(&self, name: &str, d: Duration) {
        let mut n = lock(&self.inner.connectors.net);
        n.lookup.stall = Some((name.to_string(), d));
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

    /// Forget what a check found of connector `id` (one person's, or
    /// everyone's): the offer shown and the tools agents are offered.
    pub(crate) fn connector_offer_forget(&self, user: Option<&str>, id: &str) {
        lock(&self.inner.connectors.cache).forget(user, id);
        self.connector_tools_forget(user, id);
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
        for (ok, stored) in [
            ("Authorization", "authorization"),
            ("X-API-Key", "x-api-key"),
            ("Api-Key", "api-key"),
            ("Private-Token", "private-token"),
            ("X-Auth", "x-auth"),
            ("Some-Auth", "some-auth"),
            ("X-Linear-Token", "x-linear-token"),
            ("x-custom", "x-custom"),
        ] {
            assert_eq!(check_header(ok).as_deref(), Ok(stored), "{ok}");
        }
        for bad in [
            "Referer",
            "Origin",
            "X-Forwarded-For",
            "X-Forwarded-Host",
            "Forwarded",
            "Via",
            "X-Real-IP",
            "X-Original-URL",
            "X-Request-Id",
            "X-Correlation-Id",
            "X-Envoy-Peer",
            "X-Amzn-Trace-Id",
            "X-Cloud-Trace-Context",
            "Location",
            "Set-Cookie",
            "Cookie",
            "Accept-Encoding",
            "Content-Encoding",
            "Expect",
            "Range",
            "Date",
            "Host",
            "Content-Length",
            "Transfer-Encoding",
            "Connection",
            "Upgrade",
            "Proxy-Authorization",
            "Proxy-Key",
            "Sec-WebSocket-Key",
            "Mcp-Session-Id",
            "mcp-token",
            "-key",
            "X-Key\r\nEvil: 1",
            "X-Key: 1",
            "X Key",
            "X_Key",
            "X-Key_1",
            "X-Key!",
            "X-Api.Key",
            "",
            &format!("x-{}", "k".repeat(63)),
        ] {
            let e = check_header(bad).unwrap_err();
            assert!(e.contains("-Key, -Token or -Auth"), "{bad:?}: {e}");
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

    /// Offers at every cap: the cache holds at most MAX_CACHED_BYTES of
    /// them, by its own count, and the count is what is held.
    #[test]
    fn the_cache_is_bounded_in_bytes_at_the_worst() {
        let big = |c: char, n: usize| Some(c.to_string().repeat(n));
        let worst = Offer {
            server_name: big('é', MAX_SERVER_TEXT),
            server_version: big('é', MAX_SERVER_TEXT),
            instructions: big('é', MAX_INSTRUCTIONS),
            tools: (0..MAX_TOOLS)
                .map(|i| OfferedTool {
                    name: format!("{i}{}", "é".repeat(MAX_TOOL_NAME - 3)),
                    agent_name: "a".repeat(64),
                    description: big('é', MAX_TEXT / MAX_TOOLS),
                })
                .collect(),
            more: true,
            left_out: 0,
            checked_ms: 0,
        };
        let w = worst.weight();
        assert!(w < 300 * 1024, "{w}");
        let mut c = Cache::default();
        for i in 0..MAX_CACHED {
            c.put(&format!("u{i}"), "k", worst.clone());
            assert!(c.bytes() <= MAX_CACHED_BYTES, "{}", c.bytes());
        }
        assert_eq!(c.len(), MAX_CACHED_BYTES / w);
        let held: usize = c.map.values().map(|(_, o)| o.weight()).sum();
        assert_eq!(held, c.bytes());
        // Replacing and forgetting keep the count right.
        c.put("u255", "k", worst.clone());
        c.forget(None, "k");
        assert_eq!((c.len(), c.bytes()), (0, 0));
    }

    #[test]
    fn the_cache_is_bounded_and_forgets() {
        let o = Offer {
            server_name: None,
            server_version: None,
            instructions: None,
            tools: vec![],
            more: false,
            left_out: 0,
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
