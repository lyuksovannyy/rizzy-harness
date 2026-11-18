//! The project board tools on the node (`board`, `board_edit`; PLAN.md
//! T78): a call becomes one `board.request` for the session it runs in, in
//! order after the events before it (so the master has seen the turn
//! start), and the answer comes back as `board.result`. A slow or absent
//! master is a plain tool error after [`WAIT`]; nothing is retried, so a
//! change is asked for once (the master also drops a repeated request id).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::SessionId;
use harness_core::board::{BoardAnswer, BoardRequest, BoardSource};
use harness_protocol::NodeToMaster;
use tokio::sync::oneshot;

use crate::Inner;

/// How long a call waits for the master.
pub const WAIT: Duration = Duration::from_secs(20);
/// Calls waiting for the master at once, on the whole node.
const MAX_PENDING: usize = 32;

type Reply = oneshot::Sender<Result<BoardAnswer, String>>;

/// The calls waiting for their `board.result`, by request id.
#[derive(Default)]
pub(crate) struct Pending(Mutex<HashMap<String, Reply>>);

impl Pending {
    /// A `board.result` arrived: hand it to its waiting call. An unknown id
    /// (late, after the wait gave up, or never asked) is dropped.
    pub(crate) fn answer(&self, req_id: &str, answer: Option<BoardAnswer>, error: Option<String>) {
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
pub(crate) struct LinkBoard {
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
impl BoardSource for LinkBoard {
    async fn board(&self, request: BoardRequest) -> Result<BoardAnswer, String> {
        if self.node.outbox().is_none() {
            return Err("this node is not connected to the server".into());
        }
        let writes = request.writes();
        let req_id = harness_core::new_id();
        let (tx, rx) = oneshot::channel();
        {
            let mut p = self.node.boards.0.lock().unwrap_or_else(|p| p.into_inner());
            if p.len() >= MAX_PENDING {
                return Err("too many board calls are waiting for the server; try again".into());
            }
            p.insert(req_id.clone(), tx);
        }
        let _waiting = Waiting(&self.node.boards, req_id.clone());
        let send = self.node.publish_wait(NodeToMaster::Board {
            req_id,
            session_id: self.session,
            request,
        });
        let asked = async {
            send.await;
            rx.await
        };
        // A change the server made but could not confirm must not be made
        // twice by a model that retries blindly.
        let maybe = if writes {
            " The change may have been made: list the board before trying again."
        } else {
            ""
        };
        match tokio::time::timeout(WAIT, asked).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err(format!("the server did not answer.{maybe}")),
            Err(_) => Err(format!(
                "the server did not answer within {} seconds.{maybe}",
                WAIT.as_secs()
            )),
        }
    }
}
