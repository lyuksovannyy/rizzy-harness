//! `attached_file` on the node (PLAN.md T95): a call becomes one
//! `attached.request` for the session it runs in, in order after the events
//! before it (so the master has seen the message that names the file), and
//! the answer comes back as `attached.result`. A slow or absent master is a
//! plain tool error after [`WAIT`]; nothing is retried. The node holds and
//! writes nothing of the file: the bytes stay on the master.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::SessionId;
use harness_core::attached::{AttachedAnswer, AttachedRequest, AttachedSource};
use harness_protocol::NodeToMaster;
use tokio::sync::oneshot;

use crate::Inner;

/// How long a call waits for the master (it answers within its own time
/// limit; this is the outer one).
pub const WAIT: Duration = Duration::from_secs(25);
/// Calls waiting for the master at once, on the whole node.
const MAX_PENDING: usize = 32;

type Reply = oneshot::Sender<Result<AttachedAnswer, String>>;

/// The calls waiting for their `attached.result`, by request id.
#[derive(Default)]
pub(crate) struct Pending(Mutex<HashMap<String, Reply>>);

impl Pending {
    /// An `attached.result` arrived: hand it to its waiting call. An unknown
    /// id (late, after the wait gave up, or never asked) is dropped.
    pub(crate) fn answer(
        &self,
        req_id: &str,
        answer: Option<AttachedAnswer>,
        error: Option<String>,
    ) {
        let tx = self
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(req_id);
        if let Some(tx) = tx {
            let _ = tx.send(match (answer, error) {
                (_, Some(e)) => Err(e),
                (Some(a), None) => Ok(a),
                (None, None) => Err("the server sent an empty answer".into()),
            });
        }
    }
}

/// One session's way to the master, handed to its tool calls.
pub(crate) struct LinkAttached {
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
impl AttachedSource for LinkAttached {
    async fn attached(&self, request: AttachedRequest) -> Result<AttachedAnswer, String> {
        if self.node.outbox().is_none() {
            return Err("this node is not connected to the server".into());
        }
        let req_id = harness_core::new_id();
        let (tx, rx) = oneshot::channel();
        {
            let mut p = self
                .node
                .attacheds
                .0
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if p.len() >= MAX_PENDING {
                return Err("too many file reads are waiting for the server; try again".into());
            }
            p.insert(req_id.clone(), tx);
        }
        let _waiting = Waiting(&self.node.attacheds, req_id.clone());
        let send = self.node.publish_wait(NodeToMaster::Attached {
            req_id,
            session_id: self.session,
            request,
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
