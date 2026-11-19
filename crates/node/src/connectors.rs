//! Custom connectors' tools on the node (X.35 step 2, PLAN.md T85): a call
//! becomes one `connector.request` for the session the call runs in (the
//! public tool name and its arguments, nothing else), in order after the
//! events before it, so the master has seen the turn start; the answer
//! comes back as `connector.result`. Nothing runs here and nothing is read
//! here: the master makes the call with the chat owner's account, whose
//! token never reaches this node. A slow or absent master is a plain tool
//! error after [`WAIT`]; nothing is retried. A call whose turn stops is
//! dropped here and the master is told (`connector.cancel`), so it drops
//! the remote call too.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::SessionId;
use harness_core::connectors::{ConnectorAnswer, ConnectorSource, MAX_ARGUMENT_BYTES};
use harness_protocol::NodeToMaster;
use tokio::sync::oneshot;

use crate::Inner;

/// How long a call waits for the master: the master's own limit for one
/// call (`harness_master::connectors::call::CALL_TIMEOUT`, 120 s) and a
/// margin for the link.
pub const WAIT: Duration = Duration::from_secs(135);
/// Calls waiting for the master at once, on the whole node.
const MAX_PENDING: usize = 32;
/// The longest answer taken from the master (bytes of text): the loop
/// shows at most `harness_tools::MAX_OUTPUT_CHARS` of it; a longer one is
/// refused, not cut.
const MAX_ANSWER: usize = 512 * 1024;

type Reply = oneshot::Sender<Result<ConnectorAnswer, String>>;

/// The calls waiting for their `connector.result`, by request id.
#[derive(Default)]
pub(crate) struct Pending(Mutex<HashMap<String, Reply>>);

impl Pending {
    /// A `connector.result` arrived: hand it to its waiting call. An
    /// unknown id (late, after the wait gave up or the turn stopped,
    /// answered already, or never asked) is dropped.
    pub(crate) fn answer(
        &self,
        req_id: &str,
        answer: Option<ConnectorAnswer>,
        error: Option<String>,
    ) {
        let tx = self
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(req_id);
        if let Some(tx) = tx {
            let _ = tx.send(match (answer, error) {
                (_, Some(e)) => Err(harness_core::text::clean_line(&e, 300)),
                (Some(a), None) if a.text.len() > MAX_ANSWER => {
                    Err("the server sent more than one tool result may hold".into())
                }
                (Some(a), None) => Ok(a),
                (None, None) => Err("the server sent an empty answer".into()),
            });
        }
    }

    #[cfg(test)]
    fn waiting(&self) -> usize {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).len()
    }
}

/// One session's way to the master, handed to its turn's connector tools.
pub(crate) struct LinkConnectors {
    pub(crate) node: Arc<Inner>,
    pub(crate) session: SessionId,
}

/// Removes a waiting call however it ends; one still waiting when dropped
/// (its turn stopped) is cancelled on the master too.
struct Waiting<'a> {
    node: &'a Inner,
    req_id: String,
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        let was = self
            .node
            .connectors
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.req_id);
        if was.is_some() {
            self.node.publish(NodeToMaster::ConnectorCancel {
                req_id: self.req_id.clone(),
            });
        }
    }
}

#[async_trait::async_trait]
impl ConnectorSource for LinkConnectors {
    async fn call(
        &self,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<ConnectorAnswer, String> {
        if !self.node.cfg.policy.mcp.master_connectors {
            return Err("this node does not take the server's connectors".into());
        }
        let size = serde_json::to_string(&arguments).map_or(usize::MAX, |s| s.len());
        if size > MAX_ARGUMENT_BYTES {
            return Err(format!(
                "the arguments are {size} bytes; a connector call takes at most \
                 {MAX_ARGUMENT_BYTES}"
            ));
        }
        if self.node.outbox().is_none() {
            return Err("this node is not connected to the server".into());
        }
        let req_id = harness_core::new_id();
        let (tx, rx) = oneshot::channel();
        {
            let mut p = self
                .node
                .connectors
                .0
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if p.len() >= MAX_PENDING {
                return Err(
                    "too many connector calls are waiting for the server; try again".into(),
                );
            }
            p.insert(req_id.clone(), tx);
        }
        let _waiting = Waiting {
            node: &self.node,
            req_id: req_id.clone(),
        };
        let send = self.node.publish_wait(NodeToMaster::Connector {
            req_id,
            session_id: self.session,
            tool: tool.to_string(),
            arguments,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Only the first answer to a waiting call counts; an answer nobody
    /// waits for changes nothing; an oversized answer is refused, not cut;
    /// an error is one clean line.
    #[tokio::test]
    async fn only_the_first_bounded_answer_counts() {
        let p = Pending::default();
        let (tx, rx) = oneshot::channel();
        p.0.lock().unwrap().insert("r".into(), tx);
        let a = |t: &str| ConnectorAnswer {
            text: t.into(),
            is_error: false,
        };
        p.answer("r", Some(a("first")), None);
        p.answer("r", Some(a("second")), None);
        p.answer("other", Some(a("x")), None);
        assert_eq!(rx.await.unwrap(), Ok(a("first")));
        assert_eq!(p.waiting(), 0);
        let (tx, rx) = oneshot::channel();
        p.0.lock().unwrap().insert("big".into(), tx);
        p.answer("big", Some(a(&"x".repeat(MAX_ANSWER + 1))), None);
        assert!(rx.await.unwrap().unwrap_err().contains("more than"));
        let (tx, rx) = oneshot::channel();
        p.0.lock().unwrap().insert("e".into(), tx);
        p.answer("e", None, Some("no\n[harness] forged".into()));
        assert_eq!(rx.await.unwrap(), Err("no [harness] forged".into()));
    }
}
