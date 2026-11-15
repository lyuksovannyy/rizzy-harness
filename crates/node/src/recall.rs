//! `recall` on the node (PLAN.md T55): the tool's query goes to the master
//! as a `recall.request` for the session the call runs in, in order after
//! the events before it (so the master has seen the turn start), and the
//! answer comes back as `recall.result`. A slow or absent master is a plain
//! tool error after [`WAIT`]; nothing waits for longer, nothing is retried.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::SessionId;
use harness_core::recall::{RecallHit, RecallQuery, RecallSource};
use harness_protocol::NodeToMaster;
use tokio::sync::oneshot;

use crate::Inner;

/// How long a search waits for the master.
pub const WAIT: Duration = Duration::from_secs(20);
/// Searches waiting for the master at once, on the whole node.
const MAX_PENDING: usize = 32;

type Reply = oneshot::Sender<Result<Vec<RecallHit>, String>>;

/// The searches waiting for their `recall.result`, by request id.
#[derive(Default)]
pub(crate) struct Pending(Mutex<HashMap<String, Reply>>);

impl Pending {
    /// A `recall.result` arrived: hand it to its waiting call. An unknown
    /// id (late, after the wait gave up, or never asked) is dropped.
    pub(crate) fn answer(&self, req_id: &str, hits: Vec<RecallHit>, error: Option<String>) {
        let tx = self
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(req_id);
        if let Some(tx) = tx {
            let _ = tx.send(match error {
                Some(e) => Err(e),
                None => Ok(hits),
            });
        }
    }
}

/// One session's way to the master, handed to its tool calls.
pub(crate) struct LinkRecall {
    pub(crate) node: Arc<Inner>,
    pub(crate) session: SessionId,
}

/// Removes a waiting call however it ends.
struct Waiting<'a>(&'a Pending, String);

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.1);
    }
}

#[async_trait::async_trait]
impl RecallSource for LinkRecall {
    async fn recall(&self, query: RecallQuery) -> Result<Vec<RecallHit>, String> {
        if self.node.outbox().is_none() {
            return Err("this node is not connected to the server".into());
        }
        let req_id = harness_core::new_id();
        let (tx, rx) = oneshot::channel();
        {
            let mut p = self
                .node
                .recalls
                .0
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if p.len() >= MAX_PENDING {
                return Err("too many searches are waiting for the server; try again".into());
            }
            p.insert(req_id.clone(), tx);
        }
        let _waiting = Waiting(&self.node.recalls, req_id.clone());
        let send = self.node.publish_wait(NodeToMaster::Recall {
            req_id,
            session_id: self.session,
            query,
        });
        let asked = async {
            send.await;
            rx.await
        };
        match tokio::time::timeout(WAIT, asked).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err("the server did not answer".into()),
            Err(_) => Err(format!(
                "the server did not answer within {} seconds",
                WAIT.as_secs()
            )),
        }
    }
}
