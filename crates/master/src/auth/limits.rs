//! Limits that hold before anyone is authenticated (AUDIT S4, PLAN.md T12).
//!
//! * [`FailureMap`]: failures per key inside a sliding window, with a fixed
//!   number of keys. Checking and counting happen under one lock
//!   ([`FailureMap::try_reserve`]), so a burst of parallel guesses cannot all
//!   pass the check before the first failure is recorded.
//! * [`Hasher`]: every Argon2 run in the process takes one of
//!   [`HASH_PERMITS`] permits, waiting at most [`HASH_WAIT`]; past that the
//!   caller answers 429. Argon2id with default parameters takes 19 MiB, so
//!   this also bounds memory.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Argon2 runs allowed at once, across login, setup, account creation and
/// password changes.
pub const HASH_PERMITS: usize = 4;
/// How long a request waits for a permit before it is refused.
pub const HASH_WAIT: Duration = Duration::from_secs(2);
/// `Retry-After` (seconds) when no permit came free in time.
pub const BUSY_RETRY_AFTER_S: u64 = 1;
/// Keys the login failure map holds. When it is full, expired entries go
/// first, then the entry whose latest failure is oldest (PLAN.md T12 has
/// what that costs an attacker). About 150 bytes per key: under 10 MiB.
pub const FAILURE_MAP_CAPACITY: usize = 65_536;

/// One failure, or a reservation that becomes one unless given back.
#[derive(Clone, Copy)]
struct Slot {
    at_ms: i64,
    id: u64,
}

struct Inner {
    map: HashMap<Box<str>, Vec<Slot>>,
    next_id: u64,
}

/// Failures per key in a sliding window, bounded in keys and in failures
/// per key.
pub struct FailureMap {
    inner: Mutex<Inner>,
    capacity: usize,
    max: usize,
    window_ms: i64,
}

impl FailureMap {
    pub fn new(capacity: usize, max: usize, window_ms: i64) -> Self {
        Self {
            inner: Mutex::new(Inner {
                map: HashMap::new(),
                next_id: 0,
            }),
            capacity,
            max,
            window_ms,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Milliseconds until `slots` (pruned) has room again.
    fn retry_ms(&self, slots: &[Slot], now_ms: i64) -> i64 {
        let oldest = slots.iter().map(|s| s.at_ms).min().unwrap_or(now_ms);
        (oldest + self.window_ms - now_ms).max(1)
    }

    /// Is `key` at its limit? Never adds a key, so it is safe to call before
    /// anything is paid for. `Err` carries the milliseconds to wait.
    pub fn check(&self, key: &str, now_ms: i64) -> Result<(), i64> {
        let mut inner = self.lock();
        let Some(slots) = inner.map.get_mut(key) else {
            return Ok(());
        };
        slots.retain(|s| now_ms - s.at_ms < self.window_ms);
        if slots.is_empty() {
            inner.map.remove(key);
            return Ok(());
        }
        if slots.len() >= self.max {
            return Err(self.retry_ms(slots, now_ms));
        }
        Ok(())
    }

    /// Take a slot for `key`, or `Err(ms to wait)` when its failures are
    /// used up. The slot counts as a failure until the returned
    /// [`Reservation`] is settled; dropping it unsettled gives it back.
    pub fn try_reserve<'a>(&'a self, key: &'a str, now_ms: i64) -> Result<Reservation<'a>, i64> {
        let mut inner = self.lock();
        let id = inner.next_id;
        inner.next_id += 1;
        if let Some(slots) = inner.map.get_mut(key) {
            slots.retain(|s| now_ms - s.at_ms < self.window_ms);
            if slots.len() >= self.max {
                return Err(self.retry_ms(slots, now_ms));
            }
            slots.push(Slot { at_ms: now_ms, id });
        } else {
            self.make_room(&mut inner.map, now_ms);
            let mut slots = Vec::with_capacity(self.max);
            slots.push(Slot { at_ms: now_ms, id });
            inner.map.insert(key.into(), slots);
        }
        Ok(Reservation {
            map: self,
            key,
            id,
            at_ms: now_ms,
            settled: false,
        })
    }

    /// Before a new key goes in: drop expired keys, then, if still full,
    /// the key whose latest failure is oldest. Linear, but only when full,
    /// and inserts are paced by the hash permits.
    fn make_room(&self, map: &mut HashMap<Box<str>, Vec<Slot>>, now_ms: i64) {
        if map.len() < self.capacity {
            return;
        }
        map.retain(|_, slots| slots.iter().any(|s| now_ms - s.at_ms < self.window_ms));
        while map.len() >= self.capacity {
            let oldest = map
                .iter()
                .min_by_key(|(_, slots)| slots.iter().map(|s| s.at_ms).max())
                .map(|(k, _)| k.clone());
            match oldest {
                Some(k) => map.remove(&k),
                None => break,
            };
        }
    }

    fn release(&self, key: &str, id: u64) {
        let mut inner = self.lock();
        if let Some(slots) = inner.map.get_mut(key) {
            slots.retain(|s| s.id != id);
            if slots.is_empty() {
                inner.map.remove(key);
            }
        }
    }

    /// Make sure the failure `id` is counted: it may have been cleared by a
    /// parallel success (or evicted) while its hash ran.
    fn keep(&self, key: &str, id: u64, at_ms: i64) {
        let mut inner = self.lock();
        if inner
            .map
            .get(key)
            .is_some_and(|slots| slots.iter().any(|s| s.id == id))
        {
            return;
        }
        if !inner.map.contains_key(key) {
            self.make_room(&mut inner.map, at_ms);
        }
        let slots = inner
            .map
            .entry(key.into())
            .or_insert_with(|| Vec::with_capacity(self.max));
        if slots.len() < self.max {
            slots.push(Slot { at_ms, id });
        }
    }

    pub fn clear(&self, key: &str) {
        self.lock().map.remove(key);
    }

    /// Keys held (for tests and diagnostics).
    pub fn len(&self) -> usize {
        self.lock().map.len()
    }

    /// Keys the map has room for without growing.
    #[cfg(test)]
    pub fn capacity(&self) -> usize {
        self.lock().map.capacity()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A taken slot. Settle it with [`Reservation::failed`] or
/// [`Reservation::succeeded`]; dropped unsettled (store error, cancelled
/// request, refused permit) it is given back.
pub struct Reservation<'a> {
    map: &'a FailureMap,
    key: &'a str,
    id: u64,
    at_ms: i64,
    settled: bool,
}

impl Reservation<'_> {
    /// The attempt failed: the slot stays counted.
    pub fn failed(mut self) {
        self.settled = true;
        self.map.keep(self.key, self.id, self.at_ms);
    }

    /// The attempt succeeded: the key's failures are forgotten.
    pub fn succeeded(mut self) {
        self.settled = true;
        self.map.clear(self.key);
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if !self.settled {
            self.map.release(self.key, self.id);
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HashError {
    /// No permit came free within [`HASH_WAIT`]: answer 429.
    #[error("{BUSY}")]
    Busy,
    /// The blocking task panicked or the pool is gone.
    #[error("password hashing failed")]
    Failed,
}

/// The message of every "no hash permit" 429.
pub const BUSY: &str = "the server is busy; try again in a moment";

/// Runs password hashes, at most [`HASH_PERMITS`] at once.
#[derive(Clone)]
pub struct Hasher {
    permits: Arc<Semaphore>,
    wait: Duration,
    running: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
    started: Arc<AtomicUsize>,
}

/// The right to run one hash.
pub struct HashPermit {
    _permit: OwnedSemaphorePermit,
}

impl Default for Hasher {
    fn default() -> Self {
        Self::new(HASH_PERMITS, HASH_WAIT)
    }
}

impl Hasher {
    pub fn new(permits: usize, wait: Duration) -> Self {
        Self {
            permits: Arc::new(Semaphore::new(permits)),
            wait,
            running: Arc::default(),
            peak: Arc::default(),
            started: Arc::default(),
        }
    }

    /// Wait (bounded) for a permit.
    pub async fn acquire(&self) -> Result<HashPermit, HashError> {
        match tokio::time::timeout(self.wait, self.permits.clone().acquire_owned()).await {
            Ok(Ok(p)) => Ok(HashPermit { _permit: p }),
            Ok(Err(_)) => Err(HashError::Failed),
            Err(_) => Err(HashError::Busy),
        }
    }

    /// Run `f` on the blocking pool under `permit`. The permit moves into
    /// the task: if the caller goes away the hash still finishes and only
    /// then frees it (so the cap counts real work), and a panic in `f`
    /// frees it while unwinding.
    pub async fn run_with<T: Send + 'static>(
        &self,
        permit: HashPermit,
        f: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, HashError> {
        let (running, peak, started) = (
            self.running.clone(),
            self.peak.clone(),
            self.started.clone(),
        );
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            started.fetch_add(1, Ordering::SeqCst);
            let now = running.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            let _running = Running(running);
            f()
        })
        .await
        .map_err(|_| HashError::Failed)
    }

    pub async fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, HashError> {
        let permit = self.acquire().await?;
        self.run_with(permit, f).await
    }

    /// Most hashes seen running at once.
    pub fn peak(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }

    /// Hashes started so far.
    pub fn started(&self) -> usize {
        self.started.load(Ordering::SeqCst)
    }

    pub fn available(&self) -> usize {
        self.permits.available_permits()
    }
}

struct Running(Arc<AtomicUsize>);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: i64 = 1000;

    #[test]
    fn reservations_count_until_given_back() {
        let m = FailureMap::new(10, 3, W);
        let a = m.try_reserve("bob", 0).unwrap();
        let b = m.try_reserve("bob", 0).unwrap();
        let c = m.try_reserve("bob", 0).unwrap();
        assert_eq!(
            m.try_reserve("bob", 1).err(),
            Some(999),
            "full while pending"
        );
        assert!(m.check("bob", 1).is_err());
        drop(c); // cancelled: given back
        a.failed();
        b.failed();
        let d = m.try_reserve("bob", 2).unwrap();
        d.failed();
        assert!(m.try_reserve("bob", 3).is_err());
        assert!(m.check("bob", W + 2).is_ok(), "window over");
        assert!(m.is_empty(), "expired keys go");
    }

    #[test]
    fn a_dropped_reservation_leaves_no_key() {
        let m = FailureMap::new(10, 5, W);
        drop(m.try_reserve("x", 0).unwrap());
        assert!(m.is_empty());
        assert!(m.check("never", 0).is_ok());
        assert!(m.is_empty(), "check adds nothing");
    }

    #[test]
    fn a_failure_cleared_by_a_parallel_success_still_counts() {
        let m = FailureMap::new(10, 5, W);
        let guess = m.try_reserve("bob", 0).unwrap();
        let owner = m.try_reserve("bob", 0).unwrap();
        owner.succeeded();
        guess.failed();
        assert_eq!(m.len(), 1);
        for t in 1..5 {
            m.try_reserve("bob", t).unwrap().failed();
        }
        assert!(m.check("bob", 5).is_err());
    }

    #[test]
    fn the_map_is_bounded_and_drops_expired_then_oldest() {
        let m = FailureMap::new(3, 5, W);
        m.try_reserve("a", 0).unwrap().failed();
        m.try_reserve("b", 10).unwrap().failed();
        m.try_reserve("c", 20).unwrap().failed();
        m.try_reserve("a", 30).unwrap().failed(); // a is now the newest
        m.try_reserve("d", 40).unwrap().failed();
        assert_eq!(m.len(), 3);
        assert!(m.lock().map.contains_key("a"));
        assert!(
            !m.lock().map.contains_key("b"),
            "oldest latest-failure goes"
        );
        // Expired keys go before any live one.
        m.try_reserve("e", W + 25).unwrap().failed();
        let inner = m.lock();
        assert!(!inner.map.contains_key("c"));
        assert!(inner.map.contains_key("a") && inner.map.contains_key("d"));
    }

    #[tokio::test]
    async fn no_permit_in_time_is_busy() {
        let h = Hasher::new(1, Duration::from_millis(50));
        let gate = Arc::new(std::sync::Barrier::new(2));
        let g = gate.clone();
        let h2 = h.clone();
        let slow = tokio::spawn(async move { h2.run(move || g.wait()).await });
        while h.available() == 1 {
            tokio::task::yield_now().await;
        }
        assert!(matches!(h.run(|| ()).await, Err(HashError::Busy)));
        gate.wait();
        slow.await.unwrap().unwrap();
        assert_eq!(h.available(), 1);
    }

    #[tokio::test]
    async fn a_panicking_hash_frees_its_permit() {
        let h = Hasher::new(1, Duration::from_secs(5));
        let r = h.run(|| panic!("boom")).await;
        assert!(matches!(r, Err(HashError::Failed)));
        assert_eq!(h.available(), 1);
        h.run(|| ()).await.unwrap();
    }

    #[tokio::test]
    async fn a_cancelled_request_frees_its_permit_when_the_hash_ends() {
        let h = Hasher::new(1, Duration::from_secs(5));
        // Cancelled while waiting for a permit: nothing is held.
        let p = h.acquire().await.unwrap();
        let h2 = h.clone();
        let waiter = tokio::spawn(async move { h2.run(|| ()).await });
        tokio::task::yield_now().await;
        waiter.abort();
        let _ = waiter.await;
        drop(p);
        assert_eq!(h.available(), 1);
        // Cancelled while hashing: the hash finishes, then frees it.
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let h2 = h.clone();
        let job = tokio::spawn(async move { h2.run(move || rx.recv().ok()).await });
        while h.available() == 1 {
            tokio::task::yield_now().await;
        }
        job.abort();
        let _ = job.await;
        assert_eq!(h.available(), 0, "still hashing");
        tx.send(()).unwrap();
        let p = h.acquire().await.expect("freed after the hash");
        drop(p);
        assert_eq!(h.available(), 1);
    }
}
