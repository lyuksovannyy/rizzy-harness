//! Where connector checks run (PLAN.md T84; review of PR 155, HIGH 1): a
//! fixed pool of [`WORKERS`] long-lived threads, made on the first check.
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

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use futures::FutureExt as _;

use crate::lock;

/// Workers, so checks that may run at once.
pub const WORKERS: usize = 2;

type Job = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()>>> + Send>;

/// Why a check did not start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Busy {
    #[error("you have a connector check running already; wait for it to end")]
    Person,
    #[error(
        "this server is running as many connector checks as it does at once; try again in a moment"
    )]
    Full,
}

struct Worker {
    busy: Arc<AtomicBool>,
    tx: tokio::sync::mpsc::Sender<Job>,
}

/// The workers, the people with a check running and how many run.
#[derive(Default)]
pub struct Pool {
    workers: OnceLock<Vec<Worker>>,
    people: Arc<Mutex<HashSet<String>>>,
    running: Arc<AtomicUsize>,
    most: Arc<AtomicUsize>,
}

/// Frees the worker and the person when the job ends, however it ends.
struct Done {
    busy: Arc<AtomicBool>,
    people: Arc<Mutex<HashSet<String>>>,
    who: String,
    running: Arc<AtomicUsize>,
}

impl Drop for Done {
    fn drop(&mut self) {
        lock(&self.people).remove(&self.who);
        self.running.fetch_sub(1, Ordering::SeqCst);
        self.busy.store(false, Ordering::SeqCst);
    }
}

fn worker(i: usize) -> Option<Worker> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Job>(1);
    std::thread::Builder::new()
        .name(format!("connector-check-{i}"))
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
    /// The most checks that ran at once so far (tests).
    pub fn most_at_once(&self) -> usize {
        self.most.load(Ordering::SeqCst)
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
        let workers = self
            .workers
            .get_or_init(|| (0..WORKERS).filter_map(worker).collect());
        if !lock(&self.people).insert(who.to_string()) {
            return Err(Busy::Person);
        }
        let Some(w) = workers.iter().find(|w| {
            w.busy
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        }) else {
            lock(&self.people).remove(who);
            return Err(Busy::Full);
        };
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.most.fetch_max(now, Ordering::SeqCst);
        let done = Done {
            busy: w.busy.clone(),
            people: self.people.clone(),
            who: who.to_string(),
            running: self.running.clone(),
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        let job: Job = Box::new(move || {
            Box::pin(async move {
                let _done = done;
                let _ = tx.send(make().await);
            })
        });
        if let Err(e) = w.tx.try_send(job) {
            // The worker is gone (its thread could not start or ended):
            // dropping the job frees the slot and the person.
            drop(e);
            return Err(Busy::Full);
        }
        Ok(rx.await.ok())
    }
}
