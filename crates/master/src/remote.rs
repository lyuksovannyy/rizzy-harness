//! The rules for the master's calls to a remote service on a person's
//! behalf with a token they pasted (PLAN.md T22, T83, T84): forge
//! connections (`crate::forge`) and custom connectors
//! (`crate::connectors`) both use them, so the two cannot drift apart.
//!
//! * **Where.** Every connection goes through [`RemoteResolver`]: the name
//!   is looked up and, unless the caller allows private addresses, only
//!   public addresses are kept (`harness_web::is_public`), for every
//!   connection, so a name that later resolves inward (DNS rebinding) is
//!   refused too. An IP-literal host never reaches a resolver: callers
//!   check those with `harness_web::check_public_url` first.
//! * **How.** Redirects as the caller says ([`Redirects`]: forges stay on
//!   the same origin, `harness_agent_native::models::same_origin_redirects`,
//!   where another origin is not followed and its 3xx comes back as the
//!   answer; connectors follow none); no environment proxy; a connect and a
//!   total time limit; no idle connection is kept
//!   (`pool_max_idle_per_host(0)`); for connectors no content encoding is
//!   asked for or decoded, so what is counted is what came over the wire.
//! * **What comes back** is untrusted: [`clean_text`] drops controls,
//!   bidi controls and invisible characters and caps the length;
//!   callers read at most a fixed number of bytes and never an error body.
//! * **How often.** [`Attempts`]: per person (every check) and per target
//!   (failed checks only), in a window, with a bounded table.

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use harness_core::Secret;

use crate::lock;

/// The longest connect, within the call's own limit.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// How names are looked up: the system's resolver, or (tests) a fixed
/// table; the names asked are kept for tests. A table entry's port, when
/// not 0, is the port connected to for a URL without one (tests reach a
/// fake on 127.0.0.1 for `https://name/`).
#[derive(Clone, Default)]
pub struct Lookup {
    pub(crate) fixed: Option<Arc<HashMap<String, Vec<SocketAddr>>>>,
    pub(crate) asked: Arc<Mutex<Vec<String>>>,
    /// Tests: this name's lookup blocks a blocking thread for this long,
    /// as a system resolver that does not answer does, then fails.
    pub(crate) stall: Option<(String, Duration)>,
    /// Lookups that may block a thread at once through this lookup (its
    /// permit is held by the blocking thread itself, until the system
    /// resolver returns): connector checks and calls each have one, so
    /// names that never resolve for one never starve the other.
    pub(crate) gate: Option<Arc<tokio::sync::Semaphore>>,
}

impl Lookup {
    /// Look names up in `table` only.
    pub fn table(table: HashMap<String, Vec<SocketAddr>>) -> Self {
        Self {
            fixed: Some(Arc::new(table)),
            asked: Arc::default(),
            stall: None,
            gate: None,
        }
    }

    /// Tests: like [`Lookup::table`], and `name` stalls for `d` on a
    /// blocking thread.
    pub fn stalling(table: HashMap<String, Vec<SocketAddr>>, name: &str, d: Duration) -> Self {
        Self {
            stall: Some((name.to_string(), d)),
            ..Self::table(table)
        }
    }

    /// This lookup, at most `gate`'s permits of it blocking at once.
    pub fn gated(mut self, gate: Arc<tokio::sync::Semaphore>) -> Self {
        self.gate = Some(gate);
        self
    }

    /// The names looked up so far.
    pub fn asked(&self) -> Vec<String> {
        lock(&self.asked).clone()
    }
}

/// The resolver of every remote call: it looks a name up and, when
/// `public_only`, keeps only public addresses, refusing the name when none
/// is left.
pub struct RemoteResolver {
    pub lookup: Lookup,
    pub public_only: bool,
}

impl reqwest::dns::Resolve for RemoteResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        let lookup = self.lookup.clone();
        let public_only = self.public_only;
        Box::pin(async move {
            lock(&lookup.asked).push(host.clone());
            let stalls = lookup.stall.as_ref().is_some_and(|(n, _)| *n == host);
            let blocks = stalls || lookup.fixed.is_none();
            // A permit for the thread a lookup may block, held by that
            // thread: a lookup the caller gave up on keeps it until the
            // resolver returns.
            let permit = match (&lookup.gate, blocks) {
                (Some(g), true) => Some(g.clone().acquire_owned().await?),
                _ => None,
            };
            if stalls {
                let d = lookup.stall.as_ref().map(|(_, d)| *d).unwrap_or_default();
                let _ = tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    std::thread::sleep(d)
                })
                .await;
                return Err(format!("{host} did not resolve").into());
            }
            let all: Vec<SocketAddr> = match &lookup.fixed {
                Some(t) => t.get(&host).cloned().unwrap_or_default(),
                None => {
                    let h = host.clone();
                    tokio::task::spawn_blocking(move || {
                        let _permit = permit;
                        use std::net::ToSocketAddrs as _;
                        (h.as_str(), 0)
                            .to_socket_addrs()
                            .map(|a| a.collect::<Vec<_>>())
                    })
                    .await??
                }
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

/// Which redirects a client follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Redirects {
    /// Same origin only (forges).
    SameOrigin,
    /// None: every 3xx is the answer (connectors).
    None,
}

/// A client with the rules above: `timeout` for the whole of each request
/// (its answer's body included), connect within [`CONNECT_TIMEOUT`],
/// `redirects`, no proxy, no idle connection kept, the resolver
/// (`public_only` unless the caller's entry allows a private address).
/// With `identity`, no content encoding is asked for or decoded (gzip is
/// the one decoder this build of reqwest has; a test asserts that no
/// `Accept-Encoding` is sent, so one added later shows). `extra_roots`:
/// tests only (a fake server's own certificate).
pub fn client(
    timeout: Duration,
    lookup: &Lookup,
    public_only: bool,
    redirects: Redirects,
    identity: bool,
    extra_roots: &[Vec<u8>],
) -> Result<reqwest::Client, reqwest::Error> {
    let mut b = reqwest::Client::builder()
        .connect_timeout(timeout.min(CONNECT_TIMEOUT))
        .timeout(timeout)
        .redirect(match redirects {
            Redirects::SameOrigin => harness_agent_native::models::same_origin_redirects(),
            Redirects::None => reqwest::redirect::Policy::none(),
        })
        .no_proxy()
        .pool_max_idle_per_host(0)
        .user_agent(concat!("rizzy-harness/", env!("CARGO_PKG_VERSION")))
        .dns_resolver(Arc::new(RemoteResolver {
            lookup: lookup.clone(),
            public_only,
        }));
    if identity {
        b = b.no_gzip();
    }
    if !extra_roots.is_empty() {
        let mut certs = Vec::new();
        for der in extra_roots {
            certs.push(reqwest::Certificate::from_der(der)?);
        }
        b = b.tls_certs_only(certs);
    }
    b.build()
}

/// Characters that change how text around them reads: controls, bidi
/// controls, zero-width and invisible ones.
pub fn hidden(c: char) -> bool {
    c.is_control()
        || matches!(c,
            '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}' | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}')
}

/// Remote text shown to a person: one line (every line break and tab a
/// space, so "a\nb" stays two words), hidden characters gone, spaces
/// folded, at most `max` characters; `None` when nothing is left.
pub fn clean_text(s: &str, max: usize) -> Option<String> {
    let s: String = s
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| !hidden(*c))
        .collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s: String = s.chars().take(max).collect();
    (!s.is_empty()).then_some(s)
}

/// A pasted token, trimmed: 1 to `max` visible ASCII characters.
pub fn clean_token(t: &str, max: usize) -> Result<Secret<String>, String> {
    let t = t.trim();
    if t.is_empty() || t.len() > max || !t.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
        return Err(format!(
            "a token is 1 to {max} visible ASCII characters with no spaces"
        ));
    }
    Ok(Secret::new(t.to_string()))
}

/// Recent attempts by key (a person's id or a target).
type Recent = HashMap<String, VecDeque<Instant>>;

/// The limits of one kind of check, and what a refusal says.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub per_user: usize,
    pub per_target: usize,
    pub window: Duration,
    /// Keys tracked at once; past it, attempts are refused until old ones
    /// expire (fails closed).
    pub max_tracked: usize,
    pub busy: &'static str,
    pub user_over: &'static str,
    pub target_over: &'static str,
}

/// The per-person budget counts every check (it bounds what one person
/// makes this server send); the per-target one counts failed checks only
/// (it keeps this server from hammering a service with bad tokens), so
/// people whose tokens work are not locked out by others' junk, though
/// failures alone can still use it up.
pub struct Attempts {
    limits: Limits,
    /// (every check per person, failed checks per target)
    seen: Mutex<(Recent, Recent)>,
}

fn prune(q: &mut VecDeque<Instant>, now: Instant, window: Duration) {
    while q.front().is_some_and(|t| now.duration_since(*t) >= window) {
        q.pop_front();
    }
}

impl Attempts {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            seen: Mutex::default(),
        }
    }

    /// Count one check by `user` against `target`; `Err` says which limit
    /// it is over.
    pub fn admit(&self, user: &str, target: &str) -> Result<(), &'static str> {
        let l = self.limits;
        let now = Instant::now();
        let mut g = lock(&self.seen);
        let (users, targets) = &mut *g;
        for map in [&mut *users, &mut *targets] {
            if map.len() >= l.max_tracked {
                map.retain(|_, q| {
                    prune(q, now, l.window);
                    !q.is_empty()
                });
            }
        }
        if (!users.contains_key(user) && users.len() >= l.max_tracked)
            || (!targets.contains_key(target) && targets.len() >= l.max_tracked)
        {
            return Err(l.busy);
        }
        let u = users.entry(user.to_string()).or_default();
        prune(u, now, l.window);
        if u.len() >= l.per_user {
            return Err(l.user_over);
        }
        if let Some(h) = targets.get_mut(target) {
            prune(h, now, l.window);
            if h.len() >= l.per_target {
                return Err(l.target_over);
            }
        }
        u.push_back(now);
        Ok(())
    }

    /// A check against `target` failed: it counts against its budget.
    pub fn failed(&self, target: &str) {
        let l = self.limits;
        let now = Instant::now();
        let mut g = lock(&self.seen);
        let targets = &mut g.1;
        if !targets.contains_key(target) && targets.len() >= l.max_tracked {
            targets.retain(|_, q| {
                prune(q, now, l.window);
                !q.is_empty()
            });
            if targets.len() >= l.max_tracked {
                return;
            }
        }
        let h = targets.entry(target.to_string()).or_default();
        prune(h, now, l.window);
        h.push_back(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::dns::Resolve as _;

    /// Review 3 of PR 157, L8: names that never resolve for one gate (the
    /// calls') hold at most its permits of blocking threads; another gate's
    /// lookup (a check's) still gets a thread and answers.
    #[test]
    fn a_gate_keeps_hung_lookups_from_starving_another() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(2)
            .build()
            .unwrap();
        rt.block_on(async {
            let calls = Arc::new(tokio::sync::Semaphore::new(1));
            let checks = Arc::new(tokio::sync::Semaphore::new(1));
            let hung = RemoteResolver {
                lookup: Lookup::stalling(HashMap::new(), "hang.test", Duration::from_secs(3))
                    .gated(calls),
                public_only: false,
            };
            let a: SocketAddr = "127.0.0.1:1".parse().unwrap();
            let slow = RemoteResolver {
                lookup: Lookup::stalling(
                    HashMap::from([("ok.test".to_string(), vec![a])]),
                    "slow.test",
                    Duration::from_millis(100),
                )
                .gated(checks),
                public_only: false,
            };
            // Four lookups of a name that hangs, each given up after 200 ms.
            for _ in 0..4 {
                let f = hung.resolve("hang.test".parse().unwrap());
                let _ = tokio::time::timeout(Duration::from_millis(200), f).await;
            }
            // A check's lookup that blocks a thread for a moment answers.
            let at = std::time::Instant::now();
            let r = tokio::time::timeout(
                Duration::from_secs(2),
                slow.resolve("slow.test".parse().unwrap()),
            )
            .await;
            assert!(r.is_ok(), "the check's lookup got no thread");
            assert!(at.elapsed() < Duration::from_secs(1), "{:?}", at.elapsed());
        });
    }
}
