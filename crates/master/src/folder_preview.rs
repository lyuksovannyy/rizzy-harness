//! Previewing a Word or Excel file of a chat's folder (ADOPTION_PLAN X.59,
//! PLAN.md T99): the master relays one `session.doc_preview` to the node
//! that owns the chat and hands the one `session.doc_preview_result` that
//! answers it, from that node only, to the waiting request, as `files.rs`
//! does for the `@` completion. It reads, resolves and parses nothing: the
//! path is text for the node's own rules, the file is read by the node's
//! confined helper. It keeps nothing and writes no path to a log. The owner
//! check is the caller's (`http/folder_preview.rs`).
//!
//! A later "edit" request takes the same target and waits the same way: add
//! a sibling message and a sibling `ask`; nothing here changes.

use std::collections::HashMap;
use std::time::Duration;

use harness_core::SessionId;
use harness_protocol::{CAP_DOC_PREVIEW, MasterToNode, PreviewRefusal};
use serde_json::Value;
use tokio::sync::oneshot;

use crate::{Master, lock};

/// How long a request waits for its node: the helper's queue (10 s) and run
/// (20 s) bounds and the file's read.
const WAIT: Duration = Duration::from_secs(40);
/// Requests waiting at once, all nodes together, and per account: one
/// person's previews run one at a time, so they cannot fill the node's
/// helper places that the agent's own tool calls use.
const MAX_WAITING: usize = 16;
const PER_USER: usize = 1;

type Answer = Result<Value, PreviewRefusal>;

/// Requests waiting for nodes' answers, by request id.
#[derive(Default)]
pub struct Waiting(std::sync::Mutex<HashMap<String, Wait>>);

struct Wait {
    node: String,
    user: String,
    tx: oneshot::Sender<Answer>,
}

/// Why there is no preview to give, each in words by the route.
#[derive(Debug)]
pub enum PreviewError {
    Away(String),
    TooOld,
    Busy,
    Timeout,
    Refused(PreviewRefusal),
    Unknown,
}

impl From<crate::CommandError> for PreviewError {
    fn from(e: crate::CommandError) -> Self {
        match e {
            crate::CommandError::UnknownSession => PreviewError::Unknown,
            _ => PreviewError::Away(String::new()),
        }
    }
}

struct Entry<'a> {
    m: &'a Master,
    req_id: &'a str,
}

impl Drop for Entry<'_> {
    fn drop(&mut self) {
        lock(&self.m.inner.previews.0).remove(self.req_id);
    }
}

/// The preview of `path` in `sid`'s folder (and `sheet`, the place among the
/// visible sheets), from the node that owns the chat; `user` is who asks.
pub async fn ask(
    m: &Master,
    user: &str,
    sid: SessionId,
    path: String,
    sheet: Option<u32>,
) -> Result<Value, PreviewError> {
    let node = m.owner(sid).await?;
    if !m.is_connected(&node) {
        return Err(PreviewError::Away(node));
    }
    if !m.node_has(&node, CAP_DOC_PREVIEW) {
        return Err(PreviewError::TooOld);
    }
    let req_id = harness_core::new_id();
    let (tx, rx) = oneshot::channel();
    {
        let mut w = lock(&m.inner.previews.0);
        if w.len() >= MAX_WAITING || w.values().filter(|x| x.user == user).count() >= PER_USER {
            return Err(PreviewError::Busy);
        }
        w.insert(
            req_id.clone(),
            Wait {
                node: node.clone(),
                user: user.to_string(),
                tx,
            },
        );
    }
    let _entry = Entry { m, req_id: &req_id };
    let sent = m.send_raw(
        &node,
        MasterToNode::DocPreview {
            req_id: req_id.clone(),
            session_id: sid,
            path,
            sheet,
        },
    );
    if !sent {
        return Err(PreviewError::Away(node));
    }
    match tokio::time::timeout(WAIT, rx).await {
        Ok(Ok(Ok(p))) => Ok(p),
        Ok(Ok(Err(r))) => Err(PreviewError::Refused(r)),
        Ok(Err(_)) => Err(PreviewError::Away(node)),
        Err(_) => Err(PreviewError::Timeout),
    }
}

/// A `session.doc_preview_result` from `node_id`: to the request it answers,
/// if that request went to this node; dropped otherwise. The size is the
/// route's to check (it parses the value once).
pub fn on_answer(m: &Master, node_id: &str, req_id: String, answer: Answer) {
    let tx = {
        let mut w = lock(&m.inner.previews.0);
        match w.get(&req_id) {
            Some(x) if x.node == node_id => w.remove(&req_id).map(|x| x.tx),
            Some(_) => {
                tracing::warn!(node = %node_id, "preview answer for another node's request; ignored");
                None
            }
            None => None,
        }
    };
    if let Some(tx) = tx {
        let _ = tx.send(answer);
    }
}

/// The node's link closed: its waiting requests end now, as "away".
pub fn node_down(m: &Master, node_id: &str) {
    lock(&m.inner.previews.0).retain(|_, x| x.node != node_id);
}
