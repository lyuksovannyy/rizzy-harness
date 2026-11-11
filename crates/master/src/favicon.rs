//! `GET /api/favicon?url=`: a site's icon for the search engine picker,
//! fetched here (see `harness_web::favicon`) so the browser never contacts
//! the site. Results, misses included, are kept in a small process-wide
//! cache keyed by origin.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use harness_web::favicon::FaviconFetcher;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Semaphore;

use crate::{Master, auth};

const MAX_ORIGINS: usize = 512;
/// Total icon bytes kept; older entries go first when it is exceeded.
const MAX_BYTES: usize = 8 * 1024 * 1024;
const HIT_TTL: Duration = Duration::from_secs(24 * 3600);
const MISS_TTL: Duration = Duration::from_secs(3600);
/// Covers the page, up to three `<link>` icons and `/favicon.ico`.
const FETCH_BUDGET: Duration = Duration::from_secs(15);

static FETCHER: LazyLock<FaviconFetcher> = LazyLock::new(FaviconFetcher::new);
/// Outbound fetches at once; a burst of new origins queues here, briefly.
static SLOTS: Semaphore = Semaphore::const_new(8);
const SLOT_WAIT: Duration = Duration::from_secs(2);
/// Uncached fetches one user may have running, so nobody holds every slot.
const PER_USER: usize = 2;
static IN_FLIGHT: LazyLock<Mutex<HashMap<String, usize>>> = LazyLock::new(Default::default);
static CACHE: LazyLock<Mutex<HashMap<String, Entry>>> = LazyLock::new(Default::default);

#[derive(Clone)]
struct Icon {
    bytes: Bytes,
    content_type: &'static str,
}

struct Entry {
    icon: Option<Icon>,
    fetched: Instant,
    used: Instant,
}

#[derive(Deserialize)]
pub struct FaviconQuery {
    #[serde(default)]
    url: String,
}

pub async fn favicon(
    State(m): State<Master>,
    headers: HeaderMap,
    uri: Uri,
    Query(q): Query<FaviconQuery>,
) -> Response {
    // The login key, or (for the `<img>` that draws it, which cannot send
    // one) a link minted for this login and this exact URL, like every
    // other byte route. The cookie alone used to do: it made the server
    // fetch from the network for whoever held it, and told them whether
    // the login was live (Gate 3 fix review 2, R2-1).
    let who = match auth::authenticate(&m, &headers).await {
        Some(who) => who,
        None => match auth::link_principal(&m, &headers, &uri).await {
            Some(who) => who,
            None => return error(StatusCode::UNAUTHORIZED, "not logged in"),
        },
    };
    if who.must_change_password {
        return error(StatusCode::FORBIDDEN, "change your password first");
    }
    let origin = match harness_web::favicon::origin(&q.url) {
        Ok(o) => o,
        Err(e) => return error(StatusCode::BAD_REQUEST, &e),
    };
    let key = origin.as_str().to_string();
    let icon = match lookup(&key) {
        Some(icon) => icon,
        None => {
            let user = who.user_id.unwrap_or(who.username);
            let Some(_mine) = UserSlot::take(user) else {
                return error(StatusCode::TOO_MANY_REQUESTS, "too many icon requests");
            };
            // Busy: say so, and remember nothing (it is not a miss).
            let Ok(Ok(_slot)) = tokio::time::timeout(SLOT_WAIT, SLOTS.acquire()).await else {
                return error(StatusCode::SERVICE_UNAVAILABLE, "busy, try again");
            };
            let fetched = tokio::time::timeout(FETCH_BUDGET, FETCHER.fetch(&origin))
                .await
                .ok()
                .flatten();
            let icon = fetched.map(|f| Icon {
                bytes: Bytes::from(f.bytes),
                content_type: f.content_type,
            });
            remember(key, icon.clone());
            icon
        }
    };
    match icon {
        Some(icon) => (
            [
                (header::CONTENT_TYPE, icon.content_type),
                (header::CACHE_CONTROL, "public, max-age=86400"),
            ],
            icon.bytes,
        )
            .into_response(),
        None => error(StatusCode::NOT_FOUND, "no icon"),
    }
}

/// One of a user's [`PER_USER`] uncached fetches, released on drop.
struct UserSlot(String);

impl UserSlot {
    fn take(user: String) -> Option<Self> {
        let mut m = IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner());
        let n = m.entry(user.clone()).or_insert(0);
        if *n >= PER_USER {
            return None;
        }
        *n += 1;
        Some(Self(user))
    }
}

impl Drop for UserSlot {
    fn drop(&mut self) {
        let mut m = IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(n) = m.get_mut(&self.0) {
            *n -= 1;
            if *n == 0 {
                m.remove(&self.0);
            }
        }
    }
}

fn error(code: StatusCode, msg: &str) -> Response {
    (code, Json(json!({ "error": msg }))).into_response()
}

/// The cached answer for an origin: `Some(None)` is a remembered miss.
fn lookup(key: &str) -> Option<Option<Icon>> {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let e = cache.get_mut(key)?;
    let ttl = if e.icon.is_some() { HIT_TTL } else { MISS_TTL };
    if e.fetched.elapsed() > ttl {
        cache.remove(key);
        return None;
    }
    e.used = Instant::now();
    Some(e.icon.clone())
}

fn remember(key: String, icon: Option<Icon>) {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.remove(&key);
    let size = |i: &Option<Icon>| i.as_ref().map_or(0, |i| i.bytes.len());
    let new = size(&icon);
    loop {
        let total: usize = cache.values().map(|e| size(&e.icon)).sum();
        if cache.len() < MAX_ORIGINS && total + new <= MAX_BYTES {
            break;
        }
        // A miss only displaces another miss: a run of made-up origins
        // cannot flush the icons everyone uses.
        let Some(oldest) = cache
            .iter()
            .filter(|(_, e)| icon.is_some() || e.icon.is_none())
            .min_by_key(|(_, e)| e.used)
            .map(|(k, _)| k.clone())
        else {
            if icon.is_none() {
                return;
            }
            break;
        };
        cache.remove(&oldest);
    }
    let now = Instant::now();
    cache.insert(
        key,
        Entry {
            icon,
            fetched: now,
            used: now,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_user_slots_are_released() {
        let a = UserSlot::take("u-slots".into()).unwrap();
        let b = UserSlot::take("u-slots".into()).unwrap();
        assert!(UserSlot::take("u-slots".into()).is_none(), "third refused");
        assert!(UserSlot::take("u-other".into()).is_some(), "per user");
        drop(a);
        let c = UserSlot::take("u-slots".into()).unwrap();
        drop((b, c));
        let in_flight = IN_FLIGHT.lock().unwrap();
        assert!(!in_flight.contains_key("u-slots"));
    }

    /// Made-up origins fill the cache with misses, never pushing out icons.
    #[test]
    fn misses_never_evict_icons() {
        let icon = || {
            Some(Icon {
                bytes: Bytes::from_static(b"x"),
                content_type: "image/png",
            })
        };
        for i in 0..MAX_ORIGINS {
            remember(format!("hit-{i}"), icon());
        }
        remember("miss".into(), None);
        assert!(lookup("miss").is_none(), "the miss was not kept");
        assert!((0..MAX_ORIGINS).all(|i| lookup(&format!("hit-{i}")).is_some()));
        remember("hit-new".into(), icon());
        assert!(lookup("hit-new").is_some());
    }
}
