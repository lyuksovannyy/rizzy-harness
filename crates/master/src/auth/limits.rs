//! Limits that hold before anyone is authenticated (AUDIT S4, PLAN.md T12).
//!
//! * The login failure map is a [`crate::keyed_limit::FailureMap`]; this file
//!   holds only its capacity ([`FAILURE_MAP_CAPACITY`]).
//! * [`Hasher`]: every Argon2 run in the process takes one of
//!   [`HASH_PERMITS`] permits, waiting at most [`HASH_WAIT`]; past that the
//!   caller answers 429. Argon2id with default parameters takes 19 MiB, so
//!   this also bounds memory.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
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
