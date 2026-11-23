//! `doc_create` and `sheet_create` on the node, in a chat without a folder
//! (PLAN.md T97): a call becomes one `docs.request` for the session it runs
//! in, in order after the events before it, and the answer comes back as
//! `docs.result`. A slow or absent master is a plain tool error after
//! [`WAIT`]; nothing is retried. The node holds and writes nothing of the
//! file: the master builds it and keeps it, and the node only names it in the
//! call's `tool_call_finished.made`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::SessionId;
use harness_core::docs::{DocCreated, DocRequest, DocSource};
use harness_protocol::NodeToMaster;
use tokio::sync::oneshot;

use crate::Inner;

/// How long a call waits for the master (it answers within its own time
/// limit; this is the outer one).
pub const WAIT: Duration = Duration::from_secs(25);
/// Calls waiting for the master at once, on the whole node.
const MAX_PENDING: usize = 32;

type Reply = oneshot::Sender<Result<DocCreated, String>>;

/// The calls waiting for their `docs.result`, by request id.
#[derive(Default)]
pub(crate) struct Pending(Mutex<HashMap<String, Reply>>);

impl Pending {
    /// A `docs.result` arrived: hand it to its waiting call. An unknown
    /// id (late, after the wait gave up, or never asked) is dropped.
    pub(crate) fn answer(&self, req_id: &str, answer: Option<DocCreated>, error: Option<String>) {
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
pub(crate) struct LinkDocs {
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
impl DocSource for LinkDocs {
    async fn create(&self, request: DocRequest) -> Result<DocCreated, String> {
        if self.node.outbox().is_none() {
            return Err("this node is not connected to the server".into());
        }
        let req_id = harness_core::new_id();
        let (tx, rx) = oneshot::channel();
        {
            let mut p = self
                .node
                .docs_pending
                .0
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if p.len() >= MAX_PENDING {
                return Err(
                    "too many documents are waiting to be made by the server; try again".into(),
                );
            }
            p.insert(req_id.clone(), tx);
        }
        let _waiting = Waiting(&self.node.docs_pending, req_id.clone());
        let send = self.node.publish_wait(NodeToMaster::Docs {
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

/// This node's one way to run the `doc-edit` helper: its own binary, at most
/// two at once on the whole node (`None` when the binary cannot be found).
pub(crate) fn shared_helper() -> Option<Arc<dyn harness_core::docs::EditHelper>> {
    static HELPER: std::sync::OnceLock<Option<Arc<harness_tools::doc_helper::ExeHelper>>> =
        std::sync::OnceLock::new();
    HELPER
        .get_or_init(|| {
            std::env::current_exe()
                .and_then(|e| e.canonicalize())
                .ok()
                .map(|e| Arc::new(harness_tools::doc_helper::ExeHelper::new(e)))
        })
        .clone()
        .map(|h| h as Arc<dyn harness_core::docs::EditHelper>)
}
