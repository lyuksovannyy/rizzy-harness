//! Where connector checks run (PLAN.md T84; review of PR 155, HIGH 1): a
//! fixed pool of [`WORKERS`] long-lived threads, made on the first check.
//! Agents' connector calls (X.35 step 2, T85) run on a pool of their own
//! made the same way ([`Pool::new`]: `super::call::WORKERS` workers, two
//! calls per person and per connector), so a slow call never holds a
//! person's check and checks never starve turns.
//! A thread per check grew the process by hundreds of MB and kept most of
//! it (each new thread got a new allocator arena); a fixed pool bounds the
//! threads, the arenas and how many checks run at once.
//!
//! * Each worker is one thread with the no-op log subscriber installed once
//!   (rmcp logs what servers send, at every level, from tasks of its own;
//!   they all run on this thread, so nothing of theirs is written
//!   anywhere), and a one-thread runtime with at most one blocking thread
//!   (for name lookups).
//! * A check goes to an idle worker or is refused at once ([`Busy::Full`]):
//!   nothing queues behind another check. A person has at most one check
//!   running ([`Busy::Person`]).
//! * A stuck worker is bounded by the check's own time limit
//!   (`Net::timeout`, enforced inside the worker): its future is dropped
//!   at the limit, the person is told it did not answer in time, and the
//!   worker is free again. A name lookup stuck in the system resolver
//!   holds that worker's one blocking thread until the resolver gives up;
//!   later lookups on that worker wait for it (and fail at their own
//!   limit) instead of starting more threads. Threads in all: 2 workers and
//!   at most 2 blocking threads, ever.
//! * After each check the worker lets closed connections finish closing
//!   and counts the tasks still alive (tests assert none). The runtime is
//!   kept, not remade: a new one would bring a new blocking thread while a
//!   stuck lookup still holds the old one. Anything left runs to its end on
//!   the worker, which keeps driving the runtime while it waits.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use futures::FutureExt as _;

use crate::lock;

/// Workers, so checks that may run at once.
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

struct Worker {
    busy: Arc<AtomicBool>,
    tx: tokio::sync::mpsc::Sender<Job>,
}

/// The workers, the keys with a job running (how many each) and how many
/// run.
pub struct Pool {
    name: &'static str,
    size: usize,
    per_key: usize,
    workers: OnceLock<Vec<Worker>>,
    keys: Arc<Mutex<HashMap<String, usize>>>,
    running: Arc<AtomicUsize>,
    most: Arc<AtomicUsize>,
}

/// The check workers: [`WORKERS`], one check per person.
impl Default for Pool {
    fn default() -> Self {
        Self::new("connector-check", WORKERS, 1)
    }
}

/// Frees the worker and the keys when the job ends, however it ends.
struct Done {
    busy: Arc<AtomicBool>,
    keys: Arc<Mutex<HashMap<String, usize>>>,
    held: Vec<String>,
    running: Arc<AtomicUsize>,
}

impl Drop for Done {
    fn drop(&mut self) {
        release(&self.keys, &self.held);
        self.running.fetch_sub(1, Ordering::SeqCst);
        self.busy.store(false, Ordering::SeqCst);
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

fn worker(name: &str, i: usize) -> Option<Worker> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Job>(1);
    std::thread::Builder::new()
        .name(format!("{name}-{i}"))
        .spawn(move || {
            let _quiet =
                tracing::subscriber::set_default(tracing::subscriber::NoSubscriber::default());
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .max_blocking_threads(1)
                .build()
            else {
                return;
            };
            rt.block_on(async {
                while let Some(job) = rx.recv().await {
                    // A panic in a check ends that check, not the worker.
                    let _ = std::panic::AssertUnwindSafe(job()).catch_unwind().await;
                }
            });
        })
        .ok()?;
    Some(Worker {
        busy: Arc::default(),
        tx,
    })
}

impl Pool {
    /// `size` workers named `name-<i>`, made on the first job; at most
    /// `per_key` jobs at once of any one key.
    pub fn new(name: &'static str, size: usize, per_key: usize) -> Self {
        Self {
            name,
            size,
            per_key,
            workers: OnceLock::new(),
            keys: Arc::default(),
            running: Arc::default(),
            most: Arc::default(),
        }
    }

    /// The most jobs that ran at once so far (tests).
    pub fn most_at_once(&self) -> usize {
        self.most.load(Ordering::SeqCst)
    }

    /// Jobs running now.
    pub fn running(&self) -> usize {
        self.running.load(Ordering::SeqCst)
    }

    /// Run `make()`'s future on an idle worker for `who`; refused at once
    /// when `who` has one running or no worker is idle. The future is made
    /// on the worker, so it need not be `Send`.
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
    /// running or no worker is idle; nothing waits in a queue. A job whose
    /// caller stops waiting (its future dropped: a stopped turn, a closed
    /// request) is dropped on the worker at once.
    pub async fn run_keyed<T, F, Fut>(&self, keys: &[&str], make: F) -> Result<Option<T>, Busy>
    where
        T: Send + 'static,
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = T> + 'static,
    {
        let workers = self.workers.get_or_init(|| {
            (0..self.size)
                .filter_map(|i| worker(self.name, i))
                .collect()
        });
        let held: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
        {
            let mut k = lock(&self.keys);
            if let Some(i) = held
                .iter()
                .position(|h| k.get(h).copied().unwrap_or(0) >= self.per_key)
            {
                return Err(if i == 0 { Busy::Person } else { Busy::Other });
            }
            for h in &held {
                *k.entry(h.clone()).or_default() += 1;
            }
        }
        let Some(w) = workers.iter().find(|w| {
            w.busy
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        }) else {
            release(&self.keys, &held);
            return Err(Busy::Full);
        };
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.most.fetch_max(now, Ordering::SeqCst);
        let done = Done {
            busy: w.busy.clone(),
            keys: self.keys.clone(),
            held,
            running: self.running.clone(),
        };
        let (mut tx, rx) = tokio::sync::oneshot::channel();
        let job: Job = Box::new(move || {
            Box::pin(async move {
                let work = make();
                tokio::select! {
                    v = work => {
                        // The worker and the keys are free before the
                        // caller hears: its next job is never refused for
                        // this one.
                        drop(done);
                        let _ = tx.send(v);
                    }
                    // Nobody waits any more: the work is dropped here.
                    _ = tx.closed() => drop(done),
                }
            })
        });
        if let Err(e) = w.tx.try_send(job) {
            // The worker is gone (its thread could not start or ended):
            // dropping the job frees the slot and the keys.
            drop(e);
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
