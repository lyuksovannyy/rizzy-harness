//! Where connector checks (PLAN.md T84) and agents' connector calls (X.35
//! step 2, T85) run: on ONE long-lived thread, made on the first job, with
//! the no-op log subscriber installed once (rmcp logs what servers send,
//! at every level, from tasks of its own; they all run on this thread, so
//! nothing of theirs is written anywhere) and a one-thread runtime with at
//! most two blocking threads (name lookups). Jobs are local tasks there.
//!
//! * **Memory** (review of PR 157, M5): a thread per check grew the
//!   process by hundreds of MB (review of PR 155); a fixed set of worker
//!   threads (two for checks, four for calls) still kept one allocator
//!   arena each, at its own peak (+18.8 MB after heavy use, release); one
//!   thread keeps one (+8.2 MB, of which +5.2 MB is the first use of all:
//!   the TLS stack and the runtime). `tests/connector_call_memory.rs`.
//! * **Slots, not workers.** Each pool has its own number of slots (two
//!   checks, four calls) and its own per-key limit (a person; a
//!   connector), so calls never take a check's slot. A job goes to a free
//!   slot or is refused at once ([`Busy`]): nothing queues.
//! * **Time.** Every job is bounded by its own time limit, enforced inside
//!   it; one whose caller stops waiting is dropped at once. A name lookup
//!   stuck in the system resolver holds one of the two blocking threads
//!   until the resolver gives up; later lookups wait for a free one (and
//!   fail at their own limit) instead of starting more threads. Threads in
//!   all: one, and at most two blocking threads, ever.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use futures::FutureExt as _;

use crate::lock;

/// Check slots: checks that may run at once.
pub const WORKERS: usize = 2;

type Job = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()>>> + Send>;

/// Why a job did not start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Busy {
    /// The first key (a person) has as many running as it may.
    #[error("you have a connector check running already; wait for it to end")]
    Person,
    /// Another key (a connector) has as many running as it may.
    #[error("this connector is as busy as it may be on this server; try again in a moment")]
    Other,
    #[error(
        "this server is running as many connector checks as it does at once; try again in a moment"
    )]
    Full,
}

/// The keys with a job running (how many each) and how many run, of this
/// pool; its jobs run on the one shared connector thread ([`shared`]).
pub struct Pool {
    thread: Arc<Thread>,
    size: usize,
    per_key: usize,
    keys: Arc<Mutex<HashMap<String, usize>>>,
    running: Arc<AtomicUsize>,
    most: Arc<AtomicUsize>,
}

/// The check slots: [`WORKERS`], one check per person.
impl Default for Pool {
    fn default() -> Self {
        Self::new("connector-check", WORKERS, 1)
    }
}

/// Frees the slot and the keys when the job ends, however it ends.
struct Done {
    keys: Arc<Mutex<HashMap<String, usize>>>,
    held: Vec<String>,
    running: Arc<AtomicUsize>,
}

impl Drop for Done {
    fn drop(&mut self) {
        release(&self.keys, &self.held);
        self.running.fetch_sub(1, Ordering::SeqCst);
    }
}

fn release(keys: &Mutex<HashMap<String, usize>>, held: &[String]) {
    let mut k = lock(keys);
    for h in held {
        if let Some(n) = k.get_mut(h) {
            *n -= 1;
            if *n == 0 {
                k.remove(h);
            }
        }
    }
}

/// The connector thread: every check and call of the pools that share it
/// runs here (review of PR 157, M5: one allocator arena for all of them,
/// instead of one per worker thread, each keeping its own peak). The no-op
/// log subscriber installed once, a one-thread runtime with at most two
/// blocking threads (name lookups), its jobs as local tasks. Made on the
/// first job.
#[derive(Default)]
pub struct Thread {
    tx: OnceLock<Option<tokio::sync::mpsc::UnboundedSender<Job>>>,
    /// Jobs alive on it now (each is one task of its runtime).
    live: Arc<AtomicUsize>,
}

impl Thread {
    fn sender(&self) -> Option<&tokio::sync::mpsc::UnboundedSender<Job>> {
        self.tx
            .get_or_init(|| {
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Job>();
                let live = self.live.clone();
                std::thread::Builder::new()
                    .name("connectors".into())
                    .spawn(move || {
                        let _quiet = tracing::subscriber::set_default(
                            tracing::subscriber::NoSubscriber::default(),
                        );
                        let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .max_blocking_threads(2)
                            .build()
                        else {
                            return;
                        };
                        let local = tokio::task::LocalSet::new();
                        local.block_on(&rt, async {
                            while let Some(job) = rx.recv().await {
                                let live = live.clone();
                                live.fetch_add(1, Ordering::SeqCst);
                                // A panic in a job ends that job, not the
                                // thread.
                                tokio::task::spawn_local(async move {
                                    let _ =
                                        std::panic::AssertUnwindSafe(job()).catch_unwind().await;
                                    live.fetch_sub(1, Ordering::SeqCst);
                                });
                            }
                        });
                    })
                    .ok()
                    .map(|_| tx)
            })
            .as_ref()
    }

    /// Tasks alive on this thread's runtime now, less the jobs themselves:
    /// what jobs started and left (read from a job, tests assert none).
    pub fn tasks_beyond_jobs(&self) -> usize {
        tokio::runtime::Handle::try_current()
            .map(|h| h.metrics().num_alive_tasks())
            .unwrap_or(0)
            .saturating_sub(self.live.load(Ordering::SeqCst))
    }
}

impl Pool {
    /// `size` slots (jobs at once) on a connector thread of its own; at
    /// most `per_key` jobs at once of any one key. `_name` is for reading
    /// only.
    pub fn new(_name: &'static str, size: usize, per_key: usize) -> Self {
        Self::on(Arc::default(), size, per_key)
    }

    /// `size` slots on `thread`, which other pools share (the master's
    /// checks and calls: one thread, one arena).
    pub fn on(thread: Arc<Thread>, size: usize, per_key: usize) -> Self {
        Self {
            thread,
            size,
            per_key,
            keys: Arc::default(),
            running: Arc::default(),
            most: Arc::default(),
        }
    }

    /// The thread its jobs run on.
    pub fn thread(&self) -> &Arc<Thread> {
        &self.thread
    }

    /// The most jobs that ran at once so far (tests).
    pub fn most_at_once(&self) -> usize {
        self.most.load(Ordering::SeqCst)
    }

    /// Jobs running now.
    pub fn running(&self) -> usize {
        self.running.load(Ordering::SeqCst)
    }

    /// Run `make()`'s future for `who`; refused at once when `who` has one
    /// running or no slot is free. The future is made on the connector
    /// thread, so it need not be `Send`.
    pub async fn run<T, F, Fut>(&self, who: &str, make: F) -> Result<Option<T>, Busy>
    where
        T: Send + 'static,
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = T> + 'static,
    {
        self.run_keyed(&[who], make).await
    }

    /// [`Self::run`] under every key of `keys` (a person first, then, say,
    /// a connector): refused at once when any of them has `per_key` jobs
    /// running or every slot is taken; nothing waits in a queue. A job
    /// whose caller stops waiting (its future dropped: a stopped turn, a
    /// closed request) is dropped at once.
    pub async fn run_keyed<T, F, Fut>(&self, keys: &[&str], make: F) -> Result<Option<T>, Busy>
    where
        T: Send + 'static,
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = T> + 'static,
    {
        let held: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
        {
            let mut k = lock(&self.keys);
            if let Some(i) = held
                .iter()
                .position(|h| k.get(h).copied().unwrap_or(0) >= self.per_key)
            {
                return Err(if i == 0 { Busy::Person } else { Busy::Other });
            }
            // A slot, taken under the same lock as the keys (a slot is only
            // ever freed meanwhile, never taken).
            if self.running.load(Ordering::SeqCst) >= self.size {
                return Err(Busy::Full);
            }
            let before = self.running.fetch_add(1, Ordering::SeqCst);
            self.most.fetch_max(before + 1, Ordering::SeqCst);
            for h in &held {
                *k.entry(h.clone()).or_default() += 1;
            }
        }
        let done = Done {
            keys: self.keys.clone(),
            held,
            running: self.running.clone(),
        };
        let Some(tx) = self.thread.sender() else {
            drop(done);
            return Err(Busy::Full);
        };
        let (mut otx, rx) = tokio::sync::oneshot::channel();
        let job: Job = Box::new(move || {
            Box::pin(async move {
                let work = make();
                tokio::select! {
                    v = work => {
                        // The slot and the keys are free before the caller
                        // hears: its next job is never refused for this one.
                        drop(done);
                        let _ = otx.send(v);
                    }
                    // Nobody waits any more: the work is dropped here.
                    _ = otx.closed() => drop(done),
                }
            })
        });
        if tx.send(job).is_err() {
            // The thread is gone: dropping the job freed the slot and keys.
            return Err(Busy::Full);
        }
        Ok(rx.await.ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// The keys' limits and the workers' are refusals, never a queue; a
    /// job whose caller stops waiting is dropped on its worker.
    #[tokio::test]
    async fn keyed_limits_refuse_and_a_dropped_caller_drops_the_job() {
        let pool = Arc::new(Pool::new("test-pool", 3, 2));
        let hold = || async {
            tokio::time::sleep(Duration::from_secs(30)).await;
        };
        let p = pool.clone();
        let a1 = tokio::spawn(async move { p.run_keyed(&["u:a", "c:x"], hold).await });
        let p = pool.clone();
        let a2 = tokio::spawn(async move { p.run_keyed(&["u:a", "c:y"], hold).await });
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(pool.running(), 2);
        // The person's third: refused at once, by the first key.
        assert_eq!(
            pool.run_keyed(&["u:a", "c:z"], hold).await.unwrap_err(),
            Busy::Person
        );
        let p = pool.clone();
        let b1 = tokio::spawn(async move { p.run_keyed(&["u:b", "c:x"], hold).await });
        tokio::time::sleep(Duration::from_millis(100)).await;
        // Connector x has two now (a's and b's): another person's third
        // there is refused by the second key.
        assert_eq!(
            pool.run_keyed(&["u:c", "c:x"], hold).await.unwrap_err(),
            Busy::Other
        );
        // Three workers busy: anyone else is refused.
        assert_eq!(
            pool.run_keyed(&["u:d", "c:w"], hold).await.unwrap_err(),
            Busy::Full
        );
        // Callers that stop waiting: their jobs end on the worker.
        a1.abort();
        a2.abort();
        b1.abort();
        for _ in 0..100 {
            if pool.running() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(pool.running(), 0);
        assert_eq!(pool.most_at_once(), 3);
        // And the keys are free again.
        assert_eq!(
            pool.run_keyed(&["u:a", "c:x"], || async { 7 }).await,
            Ok(Some(7))
        );
    }
}
