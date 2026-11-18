//! Forge connections (ADOPTION_PLAN X.27 step 1, PLAN.md T83, README
//! "Connections"): a person pastes an access token for a forge host the
//! server allows; the master asks that forge who the token belongs to and
//! keeps the account with the token sealed (`crate::sealed`).
//!
//! * **Hosts.** `github.com` and `gitlab.com` are built in. More come only
//!   from the server's `config.toml` (`[[forge.hosts]]`: `host`, `kind`,
//!   `api_base`, `private_address`); no HTTP route adds one. A person names
//!   a host by its exact listed name; the request goes to that entry's
//!   `api_base`, never to anything the person typed.
//! * **The call.** https (plain http only to this machine or a host in
//!   `HARNESS_PLAIN_HTTP_HOSTS`, `config::url_allowed`), to public
//!   addresses only through `web_fetch`'s resolver (checked on every
//!   connection) unless the entry says `private_address = true`; same-origin
//!   redirects; no environment proxy; the token in a header only; a few
//!   seconds and a few KiB at most; the forge's error body is never read.
//! * **The answer** is untrusted: the id must be a number, the login the
//!   characters forges allow, the display name and e-mail are cleaned and
//!   capped, and the UI renders them as text.
//! * **Abuse.** Attempts are limited per person and per host; a person has
//!   at most [`MAX_PER_USER`] connections.
//!
//! In this version a connection is stored and checked only: nothing sends a
//! token to a node or a model (cloning and pushing are X.27 steps 2 to 4).

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use harness_core::Secret;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};

use crate::sealed::{Sample, Sealer, Stored};
use crate::{Master, lock};

/// Connections one person may have.
pub const MAX_PER_USER: u64 = 20;
/// Token checks one person may start per [`WINDOW`].
pub const PER_USER: usize = 10;
/// Token checks against one host per [`WINDOW`], everyone together.
pub const PER_HOST: usize = 60;
pub const WINDOW: Duration = Duration::from_secs(600);
/// People and hosts the limiter tracks at once; past it, attempts are
/// refused until old ones expire (fails closed).
const MAX_TRACKED: usize = 10_000;
/// The longest token taken.
pub const MAX_TOKEN: usize = 512;
/// The most of a forge's answer read.
pub const MAX_BODY: usize = 64 * 1024;
/// The whole check, both calls of GitLab's included.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_LOGIN: usize = 100;
const MAX_DISPLAY: usize = 128;
const MAX_EMAIL: usize = 254;
/// Scopes kept, by count and by the bytes of their names together: 32
/// names with 2,048 bytes between them are at most 2,146 bytes as JSON,
/// under the column's 4,096.
pub const MAX_SCOPES: usize = 32;
pub const MAX_SCOPE_BYTES: usize = 2048;

/// Which API a forge speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ForgeKind {
    Github,
    Gitlab,
    Gitea,
    Forgejo,
}

impl ForgeKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Gitlab => "gitlab",
            Self::Gitea => "gitea",
            Self::Forgejo => "forgejo",
        }
    }
}

/// One host the server allows (`[[forge.hosts]]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForgeHost {
    /// The forge's name as people know it: lower-case, no port, no dot at
    /// the end (`git.example.com`).
    pub host: String,
    pub kind: ForgeKind,
    /// Where its API is: `https://git.example.com/api/v4` (GitLab),
    /// `…/api/v3` (GitHub Enterprise Server), `…/api/v1` (Gitea, Forgejo).
    /// Its host must be `host`, or `api_host` when that is given; a port is
    /// allowed.
    pub api_base: String,
    /// The API's own host, where it is not the forge's: `api.github.com`
    /// for github.com, `api.<name>.ghe.com` for GitHub Enterprise Cloud with
    /// data residency. It goes through the same address rules as `host`.
    /// Unset: the API is on `host` itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_host: Option<String>,
    /// The forge is on a private address (a LAN, a VPN): the admin allows
    /// the server to connect there for this host only.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub private_address: bool,
}

/// `[forge]`: written only when changed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ForgeConfig {
    /// Hosts besides the built-in ones.
    pub hosts: Vec<ForgeHost>,
}

impl ForgeConfig {
    pub fn is_default(&self) -> bool {
        self.hosts.is_empty()
    }

    /// Every entry well-formed, none twice, none a built-in one.
    pub fn validate(&self) -> Result<(), String> {
        let builtin = builtins();
        for (i, h) in self.hosts.iter().enumerate() {
            let at = format!("[[forge.hosts]] {:?}", h.host);
            if !valid_host(&h.host) {
                return Err(format!(
                    "{at}: host must be a lower-case name or address with no port, path or dot at the end"
                ));
            }
            if builtin.iter().any(|b| b.host == h.host) {
                return Err(format!("{at}: this host is built in; leave it out"));
            }
            if self.hosts[..i].iter().any(|o| o.host == h.host) {
                return Err(format!("{at}: listed twice"));
            }
            if let Some(a) = &h.api_host
                && !valid_host(a)
            {
                return Err(format!(
                    "{at}: api_host must be a lower-case name or address with no port, path or dot at the end"
                ));
            }
            check_api_base(h).map_err(|e| format!("{at}: {e}"))?;
        }
        Ok(())
    }
}

/// The built-in hosts.
pub fn builtins() -> Vec<ForgeHost> {
    vec![
        ForgeHost {
            host: "github.com".into(),
            kind: ForgeKind::Github,
            api_base: "https://api.github.com".into(),
            api_host: Some("api.github.com".into()),
            private_address: false,
        },
        ForgeHost {
            host: "gitlab.com".into(),
            kind: ForgeKind::Gitlab,
            api_base: "https://gitlab.com/api/v4".into(),
            api_host: None,
            private_address: false,
        },
    ]
}

/// A lower-case DNS name of ASCII letters, digits and hyphens, or an IPv4
/// address; no port, no path, no trailing dot, nothing else.
pub fn valid_host(h: &str) -> bool {
    if h.parse::<std::net::Ipv4Addr>().is_ok() {
        return true;
    }
    (1..=253).contains(&h.len())
        && h.split('.').all(|l| {
            (1..=63).contains(&l.len())
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

/// `api_base` is allowed by `url_allowed` and on the entry's API host
/// (`api_host`, else `host`), with no query, fragment or trailing `/`.
pub fn check_api_base(h: &ForgeHost) -> Result<(), String> {
    if !crate::config::url_allowed(&h.api_base) {
        return Err(
            "api_base must be https:// (plain http only for localhost or a host in HARNESS_PLAIN_HTTP_HOSTS), with no user name or password"
                .into(),
        );
    }
    let u = reqwest::Url::parse(&h.api_base).map_err(|e| format!("api_base: {e}"))?;
    let api_host = h.api_host.as_deref().unwrap_or(&h.host);
    if u.host_str() != Some(api_host) {
        return Err(match &h.api_host {
            Some(a) => format!("api_base must be on its api_host {a}"),
            None => format!(
                "api_base must be on {} itself (an API on another host needs api_host naming it)",
                h.host
            ),
        });
    }
    if u.query().is_some() || u.fragment().is_some() || h.api_base.ends_with('/') {
        return Err("api_base must have no query, no fragment and no / at the end".into());
    }
    Ok(())
}

/// What the forge says the token belongs to, cleaned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub account_id: String,
    pub login: String,
    pub display_name: Option<String>,
    pub commit_email: Option<String>,
    /// `None`: this forge (or this kind of token) does not report scopes.
    pub scopes: Option<Vec<String>>,
    /// Scopes reported beyond those kept ([`MAX_SCOPES`], [`MAX_SCOPE_BYTES`]).
    pub scopes_more: u32,
}

/// Why a check failed, in words that name no token and quote nothing the
/// forge sent.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CheckError {
    #[error(
        "{0} refused this token (HTTP {1}): it is wrong, expired or revoked, or lacks the scope to read your account"
    )]
    Refused(String, u16),
    #[error("{0} answered HTTP {1}")]
    Status(String, u16),
    #[error("{0} could not be reached: {1}")]
    Unreachable(String, &'static str),
    #[error("{0} sent an answer this server does not accept: {1}")]
    BadAnswer(String, &'static str),
}

/// Master-side state: the sealer, the attempt limits and test hooks.
#[derive(Default)]
pub struct State {
    sealer: OnceLock<Sealer>,
    hosts: Mutex<Option<Vec<ForgeHost>>>,
    attempts: Attempts,
    /// Tests: a shorter call timeout, in ms (0: [`CALL_TIMEOUT`]).
    timeout_ms: AtomicU64,
    /// How forge names are looked up (tests: a fixed table).
    lookup: Mutex<Lookup>,
}

/// What the database holds sealed, with a few values to prove a key on.
pub(crate) fn stored_now(store: &harness_store::Store) -> harness_store::Result<Stored> {
    let count = store.sealed_count()?;
    let samples = store
        .sealed_samples(crate::sealed::SAMPLES)?
        .into_iter()
        .map(|(user, host, account, sealed)| Sample {
            table: "user_connections".into(),
            owner: user,
            fields: vec![host, account],
            sealed,
        })
        .collect();
    Ok(Stored { count, samples })
}

/// [`stored_now`] off the async runtime.
pub(crate) async fn stored(m: &Master) -> harness_store::Result<Stored> {
    m.inner.mirror.call(stored_now).await
}

/// Open sealed storage under `data_dir` (`<data>/keys/secrets.key`) and
/// say in the log when the key file needs the admin. A bad file does not
/// stop the server: connections fail with a pointer to the log.
pub fn open(m: &Master, data_dir: &std::path::Path) {
    let s = Sealer::new(data_dir);
    // The key must open what is stored (a backup's key of another server
    // is refused here, and again at every use until it is fixed).
    match stored_now(&m.inner.mirror) {
        Ok(st) => {
            if let Err(e) = s.check(&st) {
                tracing::error!(error = %e, "sealed secrets are unavailable");
            }
        }
        Err(e) => tracing::error!(error = %e, "could not read the sealed secrets"),
    }
    let _ = m.inner.forge.sealer.set(s);
}

impl Master {
    /// The sealer, once `forge::open` ran.
    pub fn sealer(&self) -> Option<&Sealer> {
        self.inner.forge.sealer.get()
    }

    /// Tests: use `s` (a temporary folder's key).
    pub fn set_sealer(&self, s: Sealer) {
        let _ = self.inner.forge.sealer.set(s);
    }

    /// The forge hosts people may connect: the built-in ones, then the
    /// config's.
    pub fn forge_hosts(&self) -> Vec<ForgeHost> {
        if let Some(h) = lock(&self.inner.forge.hosts).clone() {
            return h;
        }
        let mut out = builtins();
        if let Some(rt) = self.provider_runtime() {
            out.extend(rt.config().forge.hosts);
        }
        out
    }

    /// Tests: allow exactly `hosts` (built-ins included only if listed).
    pub fn set_forge_hosts(&self, hosts: Vec<ForgeHost>) {
        *lock(&self.inner.forge.hosts) = Some(hosts);
    }

    /// Tests: look forge names up in `table` (name to addresses) instead of
    /// the system's resolver; the address rules apply as in use.
    pub fn set_forge_lookup(&self, table: HashMap<String, Vec<IpAddr>>) {
        *lock(&self.inner.forge.lookup) = Lookup {
            fixed: Some(Arc::new(table)),
            asked: Arc::default(),
        };
    }

    /// Tests: the names forge calls looked up since `set_forge_lookup`.
    pub fn forge_names_asked(&self) -> Vec<String> {
        lock(&lock(&self.inner.forge.lookup).asked).clone()
    }

    pub(crate) fn forge_lookup(&self) -> Lookup {
        lock(&self.inner.forge.lookup).clone()
    }

    /// Tests: cut forge calls after `d`.
    pub fn set_forge_timeout(&self, d: Duration) {
        self.inner
            .forge
            .timeout_ms
            .store(d.as_millis() as u64, Ordering::Relaxed);
    }

    pub(crate) fn forge_timeout(&self) -> Duration {
        match self.inner.forge.timeout_ms.load(Ordering::Relaxed) {
            0 => CALL_TIMEOUT,
            ms => Duration::from_millis(ms),
        }
    }

    /// The token of `user`'s connection `id`, opened: the one way a stored
    /// token comes back, for the later steps that clone and push (nothing
    /// calls it over HTTP). `Ok(None)`: no such connection of theirs.
    pub async fn connection_token(
        &self,
        user: &str,
        id: &str,
    ) -> Result<Option<Secret<String>>, crate::sealed::SealError> {
        let (u, i) = (user.to_string(), id.to_string());
        let found = self
            .inner
            .mirror
            .call(move |s| {
                Ok::<_, harness_store::StoreError>(match s.connection(&u, &i)? {
                    Some(r) => s.connection_sealed(&u, &i)?.map(|b| (r, b)),
                    None => None,
                })
            })
            .await
            .map_err(|e| crate::sealed::SealError::Io {
                path: "master.db".into(),
                source: std::io::Error::other(e.to_string()),
            })?;
        let Some((row, blob)) = found else {
            return Ok(None);
        };
        let sealer = self
            .sealer()
            .ok_or_else(|| crate::sealed::SealError::KeyBad {
                path: "keys/secrets.key".into(),
                why: "sealed storage is not set up".into(),
            })?;
        let st = stored(self)
            .await
            .map_err(|e| crate::sealed::SealError::Io {
                path: "master.db".into(),
                source: std::io::Error::other(e.to_string()),
            })?;
        let fields = [row.host.as_str(), row.account_id.as_str()];
        sealer
            .open(
                &crate::sealed::Binding {
                    table: "user_connections",
                    owner: &row.user_id,
                    fields: &fields,
                },
                &blob,
                &st,
            )
            .map(Some)
    }

    /// Count one token check by `user` against `host`; `Err` says which
    /// limit it is over.
    pub(crate) fn forge_attempt(&self, user: &str, host: &str) -> Result<(), &'static str> {
        self.inner.forge.attempts.admit(user, host)
    }

    /// A check against `host` failed: it counts against the host's budget.
    pub(crate) fn forge_failed(&self, host: &str) {
        self.inner.forge.attempts.failed(host)
    }
}

/// Recent attempts by key (a person's id or a host).
type Recent = HashMap<String, VecDeque<Instant>>;

/// The per-person budget counts every check (it bounds what one person
/// makes this server send); the per-host one counts failed checks only (it
/// keeps this server from hammering a forge with bad tokens), so people
/// whose tokens work are not locked out by others' junk, though failures
/// alone can still use it up.
#[derive(Default)]
struct Attempts {
    /// (every check per person, failed checks per host)
    seen: Mutex<(Recent, Recent)>,
}

fn prune(q: &mut VecDeque<Instant>, now: Instant) {
    while q.front().is_some_and(|t| now.duration_since(*t) >= WINDOW) {
        q.pop_front();
    }
}

impl Attempts {
    fn admit(&self, user: &str, host: &str) -> Result<(), &'static str> {
        let now = Instant::now();
        let mut g = lock(&self.seen);
        let (users, hosts) = &mut *g;
        for map in [&mut *users, &mut *hosts] {
            if map.len() >= MAX_TRACKED {
                map.retain(|_, q| {
                    prune(q, now);
                    !q.is_empty()
                });
            }
        }
        if (!users.contains_key(user) && users.len() >= MAX_TRACKED)
            || (!hosts.contains_key(host) && hosts.len() >= MAX_TRACKED)
        {
            return Err(
                "too many token checks on this server right now; try again in a few minutes",
            );
        }
        let u = users.entry(user.to_string()).or_default();
        prune(u, now);
        if u.len() >= PER_USER {
            return Err("too many token checks from your account; try again in a few minutes");
        }
        if let Some(h) = hosts.get_mut(host) {
            prune(h, now);
            if h.len() >= PER_HOST {
                return Err(
                    "too many failed token checks against this forge; try again in a few minutes",
                );
            }
        }
        u.push_back(now);
        Ok(())
    }

    fn failed(&self, host: &str) {
        let now = Instant::now();
        let mut g = lock(&self.seen);
        let hosts = &mut g.1;
        if !hosts.contains_key(host) && hosts.len() >= MAX_TRACKED {
            hosts.retain(|_, q| {
                prune(q, now);
                !q.is_empty()
            });
            if hosts.len() >= MAX_TRACKED {
                return;
            }
        }
        let h = hosts.entry(host.to_string()).or_default();
        prune(h, now);
        h.push_back(now);
    }
}

/// A pasted token, trimmed: 1 to [`MAX_TOKEN`] visible ASCII characters.
pub fn clean_token(t: &str) -> Result<Secret<String>, String> {
    let t = t.trim();
    if t.is_empty() || t.len() > MAX_TOKEN || !t.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
        return Err(format!(
            "a token is 1 to {MAX_TOKEN} visible ASCII characters with no spaces"
        ));
    }
    Ok(Secret::new(t.to_string()))
}

/// How forge names are looked up: the system's resolver, or (tests) a
/// fixed table; the names asked are kept for tests.
#[derive(Clone, Default)]
pub struct Lookup {
    fixed: Option<Arc<HashMap<String, Vec<IpAddr>>>>,
    asked: Arc<Mutex<Vec<String>>>,
}

/// The resolver of every forge call: it looks a name up and, unless the
/// entry says `private_address`, keeps only public addresses (refusing the
/// name when none is left). It runs for every connection, so a name that
/// later resolves inward (DNS rebinding) is refused too.
struct ForgeResolver {
    lookup: Lookup,
    public_only: bool,
}

impl reqwest::dns::Resolve for ForgeResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        let lookup = self.lookup.clone();
        let public_only = self.public_only;
        Box::pin(async move {
            lock(&lookup.asked).push(host.clone());
            let all: Vec<SocketAddr> = match &lookup.fixed {
                Some(t) => t
                    .get(&host)
                    .map(|v| v.iter().map(|ip| SocketAddr::new(*ip, 0)).collect())
                    .unwrap_or_default(),
                None => tokio::net::lookup_host((host.as_str(), 0)).await?.collect(),
            };
            let addrs: Vec<SocketAddr> = all
                .into_iter()
                .filter(|a| !public_only || harness_web::is_public(a.ip()))
                .collect();
            if addrs.is_empty() {
                return Err(format!("{host} does not resolve to an allowed address").into());
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// The client for `h`: its resolver (public addresses only unless the
/// entry allows a private one), same-origin redirects, no proxy.
fn client(
    h: &ForgeHost,
    timeout: Duration,
    lookup: &Lookup,
) -> Result<reqwest::Client, CheckError> {
    let unreachable = |why| CheckError::Unreachable(h.host.clone(), why);
    check_api_base(h).map_err(|_| unreachable("its api_base is not allowed"))?;
    let mut b = reqwest::Client::builder()
        .connect_timeout(timeout.min(Duration::from_secs(5)))
        .timeout(timeout)
        .redirect(harness_agent_native::models::same_origin_redirects())
        .no_proxy()
        .user_agent(concat!("rizzy-harness/", env!("CARGO_PKG_VERSION")));
    if !h.private_address {
        let u = reqwest::Url::parse(&h.api_base).map_err(|_| unreachable("bad api_base"))?;
        harness_web::check_public_url(&u).map_err(|_| {
            unreachable(
                "it is on a private address, and its entry does not say private_address = true",
            )
        })?;
    }
    b = b.dns_resolver(Arc::new(ForgeResolver {
        lookup: lookup.clone(),
        public_only: !h.private_address,
    }));
    b.build().map_err(|_| unreachable("no HTTP client"))
}

fn auth_headers(kind: ForgeKind, token: &Secret<String>) -> Result<HeaderMap, ()> {
    let mut hm = HeaderMap::new();
    let (name, value) = match kind {
        ForgeKind::Github => ("authorization", format!("Bearer {}", token.expose())),
        ForgeKind::Gitlab => ("private-token", token.expose().clone()),
        ForgeKind::Gitea | ForgeKind::Forgejo => {
            ("authorization", format!("token {}", token.expose()))
        }
    };
    let mut v = HeaderValue::from_str(&value).map_err(|_| ())?;
    v.set_sensitive(true);
    hm.insert(name, v);
    if kind == ForgeKind::Github {
        hm.insert(
            "accept",
            HeaderValue::from_static("application/vnd.github+json"),
        );
        hm.insert(
            "x-github-api-version",
            HeaderValue::from_static("2022-11-28"),
        );
    } else {
        hm.insert("accept", HeaderValue::from_static("application/json"));
    }
    Ok(hm)
}

/// GET `path` under `h`'s API: the status, the headers and, on success,
/// at most [`MAX_BODY`] bytes of JSON. An error body is never read.
async fn get(
    c: &reqwest::Client,
    h: &ForgeHost,
    path: &str,
    headers: HeaderMap,
) -> Result<(HeaderMap, serde_json::Value), CheckError> {
    let unreachable = |why| CheckError::Unreachable(h.host.clone(), why);
    let r = c
        .get(format!("{}{path}", h.api_base))
        .headers(headers)
        .send()
        .await
        .map_err(|e| {
            unreachable(if e.is_timeout() {
                "it did not answer in time"
            } else if e.is_redirect() {
                "it redirected too often"
            } else {
                "the connection failed (or the address is not allowed)"
            })
        })?;
    let status = r.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(CheckError::Refused(h.host.clone(), status.as_u16()));
    }
    if !status.is_success() {
        return Err(CheckError::Status(h.host.clone(), status.as_u16()));
    }
    let too_large = || CheckError::BadAnswer(h.host.clone(), "it is too large");
    if r.content_length().is_some_and(|n| n > MAX_BODY as u64) {
        return Err(too_large());
    }
    let headers = r.headers().clone();
    let mut body = Vec::new();
    let mut s = r.bytes_stream();
    while let Some(chunk) = s.next().await {
        let chunk = chunk.map_err(|e| {
            unreachable(if e.is_timeout() {
                "it did not finish its answer in time"
            } else {
                "its answer broke off"
            })
        })?;
        if body.len() + chunk.len() > MAX_BODY {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    let v = serde_json::from_slice(&body)
        .map_err(|_| CheckError::BadAnswer(h.host.clone(), "it is not JSON"))?;
    Ok((headers, v))
}

/// Ask `h` whom `token` belongs to.
pub async fn who_am_i(
    h: &ForgeHost,
    token: &Secret<String>,
    timeout: Duration,
    lookup: &Lookup,
) -> Result<Account, CheckError> {
    match tokio::time::timeout(timeout, ask(h, token, timeout, lookup)).await {
        Ok(r) => r,
        Err(_) => Err(CheckError::Unreachable(
            h.host.clone(),
            "it did not answer in time",
        )),
    }
}

async fn ask(
    h: &ForgeHost,
    token: &Secret<String>,
    timeout: Duration,
    lookup: &Lookup,
) -> Result<Account, CheckError> {
    let c = client(h, timeout, lookup)?;
    let headers = auth_headers(h.kind, token)
        .map_err(|_| CheckError::BadAnswer(h.host.clone(), "the token cannot be sent"))?;
    let (resp_headers, v) = get(&c, h, "/user", headers.clone()).await?;
    let bad = |what| CheckError::BadAnswer(h.host.clone(), what);
    let account_id = v
        .get("id")
        .and_then(|x| x.as_u64())
        .ok_or_else(|| bad("no numeric account id"))?
        .to_string();
    let (login_key, name_key) = match h.kind {
        ForgeKind::Gitlab => ("username", "name"),
        ForgeKind::Github => ("login", "name"),
        ForgeKind::Gitea | ForgeKind::Forgejo => ("login", "full_name"),
    };
    let login = v
        .get(login_key)
        .and_then(|x| x.as_str())
        .filter(|l| valid_login(l))
        .ok_or_else(|| bad("no account name, or one with characters forges do not use"))?
        .to_string();
    let display_name = v
        .get(name_key)
        .and_then(|x| x.as_str())
        .and_then(clean_text);
    let email = match h.kind {
        // GitLab: the commit e-mail the account chose, else its e-mail.
        ForgeKind::Gitlab => ["commit_email", "email"]
            .iter()
            .find_map(|k| v.get(*k).and_then(|x| x.as_str()).and_then(clean_email)),
        _ => v
            .get("email")
            .and_then(|x| x.as_str())
            .and_then(clean_email),
    };
    let reported: Option<Vec<String>> = match h.kind {
        // Classic tokens report theirs; fine-grained ones send no header.
        ForgeKind::Github => resp_headers
            .get("x-oauth-scopes")
            .and_then(|x| x.to_str().ok())
            .map(|s| s.split(',').map(str::to_string).collect()),
        // A second call; its failure only leaves the scopes unknown.
        ForgeKind::Gitlab => match get(&c, h, "/personal_access_tokens/self", headers).await {
            Ok((_, t)) => t.get("scopes").and_then(|s| s.as_array()).map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str())
                    .take(10_000)
                    .map(str::to_string)
                    .collect()
            }),
            Err(_) => None,
        },
        ForgeKind::Gitea | ForgeKind::Forgejo => None,
    };
    let (scopes, scopes_more) = match reported {
        Some(r) => {
            let (kept, more) = clean_scopes(r.iter().map(String::as_str));
            (Some(kept), more)
        }
        None => (None, 0),
    };
    Ok(Account {
        account_id,
        login,
        display_name,
        commit_email: email,
        scopes,
        scopes_more,
    })
}

/// A login as forges allow them: ASCII letters, digits, `-`, `_`, `.`.
pub fn valid_login(l: &str) -> bool {
    (1..=MAX_LOGIN).contains(&l.len())
        && l.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Characters that change how text around them reads: controls, bidi
/// controls, zero-width and invisible ones.
fn hidden(c: char) -> bool {
    c.is_control()
        || matches!(c,
            '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}' | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}')
}

/// Forge text shown to a person: hidden characters gone, spaces folded,
/// at most [`MAX_DISPLAY`] characters; `None` when nothing is left.
pub fn clean_text(s: &str) -> Option<String> {
    let s: String = s.chars().filter(|c| !hidden(*c)).collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s: String = s.chars().take(MAX_DISPLAY).collect();
    (!s.is_empty()).then_some(s)
}

/// An e-mail address of visible ASCII with one `@` inside it, or `None`.
pub fn clean_email(s: &str) -> Option<String> {
    let s = s.trim();
    let ok = (3..=MAX_EMAIL).contains(&s.len())
        && s.bytes()
            .all(|b| (0x21..=0x7e).contains(&b) && !matches!(b, b'<' | b'>' | b'"' | b'\\' | b','))
        && s.matches('@').count() == 1
        && !s.starts_with('@')
        && !s.ends_with('@');
    ok.then(|| s.to_string())
}

/// The scopes kept: well-formed ones, at most [`MAX_SCOPES`] of them and
/// [`MAX_SCOPE_BYTES`] of names together (so what is stored always fits
/// its column), and how many well-formed ones were left over.
fn clean_scopes<'a>(it: impl Iterator<Item = &'a str>) -> (Vec<String>, u32) {
    let mut kept = Vec::new();
    let (mut bytes, mut more) = (0usize, 0u32);
    for s in it.map(str::trim).filter(|s| {
        (1..=64).contains(&s.len())
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b':' | b'-' | b'.'))
    }) {
        if kept.len() < MAX_SCOPES && bytes + s.len() <= MAX_SCOPE_BYTES {
            bytes += s.len();
            kept.push(s.to_string());
        } else {
            more = more.saturating_add(1);
        }
    }
    (kept, more)
}

/// Where on `h` a person makes a token, and the scopes later steps (listing
/// repositories, clone, push) will need.
pub fn token_help(h: &ForgeHost) -> (String, &'static str) {
    match h.kind {
        ForgeKind::Github => (
            format!("https://{}/settings/tokens", h.host),
            "a classic token with the repo scope, or a fine-grained token with Contents: read and write and Metadata: read on the repositories you will use",
        ),
        ForgeKind::Gitlab => (
            format!("https://{}/-/user_settings/personal_access_tokens", h.host),
            "read_user (to check the account), read_api, read_repository and write_repository",
        ),
        ForgeKind::Gitea | ForgeKind::Forgejo => (
            format!("https://{}/user/settings/applications", h.host),
            "read:user (to check the account), and read and write on repository",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(h: &str, base: &str) -> ForgeHost {
        ForgeHost {
            host: h.into(),
            kind: ForgeKind::Gitea,
            api_base: base.into(),
            api_host: None,
            private_address: false,
        }
    }

    #[test]
    fn config_hosts_are_checked() {
        let ok = ForgeConfig {
            hosts: vec![
                host("git.example.com", "https://git.example.com/api/v1"),
                host("10.0.0.5", "https://10.0.0.5:3000/api/v1"),
            ],
        };
        assert!(ok.validate().is_ok(), "{:?}", ok.validate());
        for (h, base, needle) in [
            (
                "Git.Example.com",
                "https://git.example.com/api/v1",
                "lower-case",
            ),
            (
                "git.example.com.",
                "https://git.example.com/api/v1",
                "lower-case",
            ),
            (
                "git.example.com:3000",
                "https://git.example.com/api/v1",
                "lower-case",
            ),
            (
                "https://git.example.com",
                "https://git.example.com/api/v1",
                "lower-case",
            ),
            (
                "user@git.example.com",
                "https://git.example.com/api/v1",
                "lower-case",
            ),
            (
                "git.example.com/x",
                "https://git.example.com/api/v1",
                "lower-case",
            ),
            (
                "gіthub.com",
                "https://xn--gthub-n4d.com/api/v1",
                "lower-case",
            ),
            ("github.com", "https://github.com/api/v1", "built in"),
            (
                "git.example.com",
                "http://git.example.com/api/v1",
                "https://",
            ),
            (
                "git.example.com",
                "https://u:p@git.example.com/api/v1",
                "https://",
            ),
            (
                "git.example.com",
                "https://evil.example.net/api/v1",
                "on git.example.com",
            ),
            ("git.example.com", "https://git.example.com/api/v1/", "no /"),
            (
                "git.example.com",
                "https://git.example.com/api?x=1",
                "no query",
            ),
        ] {
            let e = ForgeConfig {
                hosts: vec![host(h, base)],
            }
            .validate()
            .unwrap_err();
            assert!(e.contains(needle), "{h} {base}: {e}");
        }
        let twice = ForgeConfig {
            hosts: vec![
                host("a.example.com", "https://a.example.com/api/v1"),
                host("a.example.com", "https://a.example.com/api/v1"),
            ],
        };
        assert!(twice.validate().unwrap_err().contains("twice"));
        // An unknown key or kind in the file is an error, not ignored.
        assert!(
            toml::from_str::<ForgeConfig>(
                "[[hosts]]\nhost='a'\nkind='svn'\napi_base='https://a/x'"
            )
            .is_err()
        );
        assert!(
            toml::from_str::<ForgeConfig>(
                "[[hosts]]\nhost='a'\nkind='gitea'\napi_base='https://a/x'\nopen=true"
            )
            .is_err()
        );
    }

    /// The admin's file is the only way in: config.toml's `[[forge.hosts]]`
    /// is read, checked at load and written back unchanged.
    #[test]
    fn config_toml_adds_hosts_and_bad_ones_stop_the_load() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.toml");
        std::fs::write(
            &p,
            "[[forge.hosts]]\nhost = \"git.example.com\"\nkind = \"forgejo\"\napi_base = \"https://git.example.com/api/v1\"\nprivate_address = true\n",
        )
        .unwrap();
        let c = crate::config::Config::load(&p).unwrap();
        assert_eq!(c.forge.hosts.len(), 1);
        assert!(c.forge.hosts[0].private_address);
        let text = toml::to_string(&c).unwrap();
        assert!(
            text.contains("[[forge.hosts]]") && text.contains("private_address = true"),
            "{text}"
        );
        std::fs::write(
            &p,
            "[[forge.hosts]]\nhost = \"git.example.com:3000\"\nkind = \"gitea\"\napi_base = \"https://git.example.com:3000/api/v1\"\n",
        )
        .unwrap();
        assert!(crate::config::Config::load(&p).is_err());
        assert!(
            !toml::to_string(&crate::config::Config::default())
                .unwrap()
                .contains("forge")
        );
    }

    /// Review HIGH 1: the built-in list must work as shipped.
    #[test]
    fn every_built_in_host_passes_its_own_rules_and_builds_a_client() {
        for h in builtins() {
            check_api_base(&h).unwrap_or_else(|e| panic!("{}: {e}", h.host));
            assert!(!h.private_address, "{}", h.host);
            client(&h, CALL_TIMEOUT, &Lookup::default())
                .unwrap_or_else(|e| panic!("{}: {e}", h.host));
        }
        let gh = builtins()
            .into_iter()
            .find(|h| h.host == "github.com")
            .unwrap();
        assert_eq!(gh.api_host.as_deref(), Some("api.github.com"));
    }

    #[test]
    fn an_api_on_another_host_must_be_named() {
        let mut h = host("ghe.example.com", "https://api.ghe.example.com");
        h.kind = ForgeKind::Github;
        let e = ForgeConfig {
            hosts: vec![h.clone()],
        }
        .validate()
        .unwrap_err();
        assert!(e.contains("needs api_host"), "{e}");
        h.api_host = Some("api.ghe.example.com".into());
        ForgeConfig {
            hosts: vec![h.clone()],
        }
        .validate()
        .unwrap();
        // Named, it must still be where api_base points, and well-formed.
        h.api_host = Some("api.other.example.com".into());
        let e = ForgeConfig {
            hosts: vec![h.clone()],
        }
        .validate()
        .unwrap_err();
        assert!(e.contains("its api_host api.other.example.com"), "{e}");
        h.api_host = Some("API.ghe.example.com".into());
        assert!(
            ForgeConfig { hosts: vec![h] }
                .validate()
                .unwrap_err()
                .contains("api_host must be")
        );
    }

    /// Review L1: what is kept always fits the column.
    #[test]
    fn scopes_are_capped_by_count_and_bytes() {
        let long: Vec<String> = (0..64)
            .map(|i| format!("{i:02}{}", "s".repeat(62)))
            .collect();
        let (kept, more) = clean_scopes(long.iter().map(String::as_str));
        assert_eq!((kept.len(), more), (MAX_SCOPES, 32));
        assert!(serde_json::to_string(&kept).unwrap().len() <= 4096);
        let short: Vec<String> = (0..100).map(|i| format!("s{i}")).collect();
        let (kept, more) = clean_scopes(short.iter().map(String::as_str));
        assert_eq!((kept.len(), more), (MAX_SCOPES, 68));
    }

    #[test]
    fn forge_text_is_cleaned() {
        assert_eq!(
            clean_text("  Ada\u{202E}  <b>Lovelace</b>\n\t"),
            Some("Ada <b>Lovelace</b>".into())
        );
        assert_eq!(clean_text("\u{200B}\u{0007}"), None);
        assert_eq!(
            clean_text(&"x".repeat(500)).unwrap().chars().count(),
            MAX_DISPLAY
        );
        assert!(valid_login("ada-l_0.v"));
        for bad in ["", "a b", "<script>", "ada\n", "адa", &"a".repeat(101)] {
            assert!(!valid_login(bad), "{bad:?}");
        }
        assert_eq!(clean_email(" a@b.c "), Some("a@b.c".into()));
        for bad in [
            "a",
            "@b",
            "a@",
            "a@@b",
            "a b@c",
            "a@b\r\nX: y",
            "<a@b>",
            "a,b@c",
        ] {
            assert_eq!(clean_email(bad), None, "{bad:?}");
        }
        assert_eq!(
            clean_scopes(" repo, read:org ,<x>, ".split(',')),
            (vec!["repo".to_string(), "read:org".to_string()], 0)
        );
    }

    #[test]
    fn tokens_are_visible_ascii_only() {
        assert_eq!(clean_token("  ghp_abc\n").unwrap().expose(), "ghp_abc");
        for bad in [
            "",
            "   ",
            "a b",
            "a\r\nX-Evil: 1",
            "tökén",
            &"a".repeat(MAX_TOKEN + 1),
        ] {
            let e = clean_token(bad).unwrap_err();
            assert!(!e.contains("X-Evil") && !e.contains("tök"), "{e}");
        }
    }

    #[test]
    fn attempts_are_limited_per_person_and_per_host() {
        let a = Attempts::default();
        for _ in 0..PER_USER {
            a.admit("alice", "github.com").unwrap();
        }
        assert!(
            a.admit("alice", "gitlab.com")
                .unwrap_err()
                .contains("your account")
        );
        // Checks that work never use up the host's budget (review L2) ...
        for i in 0..(3 * PER_HOST) {
            a.admit(&format!("ok{i}"), "github.com").unwrap();
        }
        // ... failed ones do, for everyone.
        for _ in 0..PER_HOST {
            a.failed("github.com");
        }
        assert!(
            a.admit("zed", "github.com")
                .unwrap_err()
                .contains("failed token checks against this forge")
        );
        assert!(a.admit("zed", "gitlab.com").is_ok());
    }

    #[test]
    fn the_token_goes_in_a_sensitive_header_per_kind() {
        let t = Secret::new("tok".into());
        let gh = auth_headers(ForgeKind::Github, &t).unwrap();
        assert_eq!(gh["authorization"], "Bearer tok");
        assert!(gh["authorization"].is_sensitive());
        let gl = auth_headers(ForgeKind::Gitlab, &t).unwrap();
        assert_eq!(gl["private-token"], "tok");
        assert!(gl["private-token"].is_sensitive());
        assert!(!gl.contains_key("authorization"));
        for k in [ForgeKind::Gitea, ForgeKind::Forgejo] {
            assert_eq!(auth_headers(k, &t).unwrap()["authorization"], "token tok");
        }
    }
}
