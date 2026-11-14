//! The audit lines of the commands the node's command loop answers inline
//! (cancel, approval answer, `stop_background`; review A21-1).
//!
//! The command loop must never wait for the store: it acts, queues the
//! ack, and hands the line to [`AuditLog::record`], which never waits. One
//! writer task takes what is queued and writes it in one transaction. A
//! line that finds the queue full (`AUDIT_QUEUE`), and the lines of a write
//! the store refused, are counted; the count is written as one line
//! (`audit.not_recorded`, `{"count": N}`) as soon as the store takes a
//! write again, so every command is either recorded or counted in the log
//! (PLAN.md T5). The work a flood of inline commands causes is then one
//! queue of bounded length and one task, whatever the store's speed.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use harness_store::Store;
use tokio::sync::mpsc;

/// Lines waiting for the writer; each is a few hundred bytes.
pub(crate) const AUDIT_QUEUE: usize = 1024;
/// While lines are counted and not recorded, the writer tries again this
/// often even when no new line comes.
const RETRY: Duration = Duration::from_secs(1);

struct Line {
    ts_ms: i64,
    kind: &'static str,
    detail: String,
}

#[derive(Clone)]
pub(crate) struct AuditLog {
    tx: mpsc::Sender<Line>,
    lost: Arc<AtomicU64>,
}

impl AuditLog {
    /// Start the writer for `store`; it ends when the last `AuditLog` is
    /// dropped and the queue is empty.
    pub(crate) fn start(store: Store) -> Self {
        let (tx, rx) = mpsc::channel(AUDIT_QUEUE);
        let lost = Arc::new(AtomicU64::new(0));
        tokio::spawn(write(store, rx, lost.clone()));
        Self { tx, lost }
    }

    /// Queue one line, never waiting: when the queue is full the line is
    /// counted instead.
    pub(crate) fn record(&self, kind: &'static str, detail: String) {
        let line = Line {
            ts_ms: harness_core::now_ms(),
            kind,
            detail,
        };
        if self.tx.try_send(line).is_err() {
            self.lost.fetch_add(1, Ordering::SeqCst);
        }
    }
}

async fn write(store: Store, mut rx: mpsc::Receiver<Line>, lost: Arc<AtomicU64>) {
    loop {
        let first = if lost.load(Ordering::SeqCst) > 0 {
            match tokio::time::timeout(RETRY, rx.recv()).await {
                Ok(Some(l)) => Some(l),
                Ok(None) => return,
                Err(_) => None,
            }
        } else {
            match rx.recv().await {
                Some(l) => Some(l),
                None => return,
            }
        };
        let mut batch: Vec<Line> = first.into_iter().collect();
        while batch.len() < AUDIT_QUEUE {
            match rx.try_recv() {
                Ok(l) => batch.push(l),
                Err(_) => break,
            }
        }
        let counted = lost.swap(0, Ordering::SeqCst);
        let note = (counted > 0).then(|| serde_json::json!({ "count": counted }).to_string());
        let n = batch.len() as u64;
        let res = store
            .call(move |s| {
                let now = harness_core::now_ms();
                let mut lines: Vec<(i64, &str, &str)> = batch
                    .iter()
                    .map(|l| (l.ts_ms, l.kind, l.detail.as_str()))
                    .collect();
                if let Some(d) = &note {
                    lines.push((now, "audit.not_recorded", d.as_str()));
                }
                s.audit_many(&lines)
            })
            .await;
        if let Err(e) = res {
            tracing::warn!(error = %e, lines = n, "could not write audit lines; counting them");
            lost.fetch_add(n + counted, Ordering::SeqCst);
        }
    }
}
