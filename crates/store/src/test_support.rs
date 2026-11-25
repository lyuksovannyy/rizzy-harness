//! Test-only hooks (feature `test-support`, which only the dev-dependencies
//! of other crates turn on; never in a shipped build): statement counts,
//! and raw SQL for rows no writer makes.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rusqlite::trace::{TraceEvent, TraceEventCodes};

use crate::Store;

/// Counts the statements run on a store's connection while it lives: a
/// request's statement count, to show it does not grow with the data
/// (FIX_PLAN 4.5 slice 4). Only a number is kept; no statement text, no
/// bound value. Tracing stops when it is dropped.
pub struct StatementCounter {
    store: Store,
    n: Arc<AtomicUsize>,
}

impl StatementCounter {
    /// Statements since the counter started (or since the last `reset`).
    pub fn count(&self) -> usize {
        self.n.load(Ordering::SeqCst)
    }

    pub fn reset(&self) {
        self.n.store(0, Ordering::SeqCst);
    }
}

impl Store {
    /// Run `sql` on the store's connection as it is: for tests that need
    /// data no writer makes (a corrupt or hand-edited row).
    pub fn execute_batch_for_test(&self, sql: &str) -> rusqlite::Result<()> {
        self.lock().execute_batch(sql)
    }

    /// The first column of the first row of `sql`, as a number.
    pub fn query_i64_for_test(&self, sql: &str) -> rusqlite::Result<i64> {
        self.lock().query_row(sql, [], |r| r.get(0))
    }

    /// Start counting the statements run on this store's connection.
    #[expect(
        clippy::disallowed_methods,
        reason = "counts statements in tests; nothing is logged"
    )]
    pub fn count_statements(&self) -> StatementCounter {
        let n = Arc::new(AtomicUsize::new(0));
        let seen = n.clone();
        self.lock()
            .trace_v2(
                TraceEventCodes::SQLITE_TRACE_STMT,
                Some(move |e: TraceEvent<'_>| {
                    // Trigger bodies are reported as "-- ..." lines.
                    if let TraceEvent::Stmt(_, sql) = e
                        && !sql.starts_with("--")
                    {
                        seen.fetch_add(1, Ordering::SeqCst);
                    }
                }),
            )
            .expect("statement tracing");
        StatementCounter {
            store: self.clone(),
            n,
        }
    }
}

impl Drop for StatementCounter {
    #[expect(
        clippy::disallowed_methods,
        reason = "turns the test's statement count off"
    )]
    fn drop(&mut self) {
        let _ = self
            .store
            .lock()
            .trace_v2(TraceEventCodes::empty(), None::<fn(TraceEvent<'_>)>);
    }
}
