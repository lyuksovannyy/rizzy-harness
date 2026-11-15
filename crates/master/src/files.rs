//! The composer's `@` completion (PLAN.md T63): paths of a chat's folder,
//! asked of the node that owns the chat. The master relays one
//! `session.files` and hands the one `session.files_result` that answers
//! it, from that node only, to the waiting request, as `diff.rs` does for a
//! turn's changes. It keeps nothing: no index, no cache, no log line with
//! a path. The owner check is the caller's (`http/files.rs`).

use std::collections::HashMap;
use std::time::Duration;

use harness_core::SessionId;
use harness_protocol::{CAP_FILES, MAX_FILES_RESULT, MasterToNode};
use tokio::sync::oneshot;

use crate::{Master, lock};

/// How long a request waits for its node (the node bounds its walk at
/// about 1.5 s; a completion later than this is of no use to a person
/// typing).
const WAIT: Duration = Duration::from_secs(5);
/// Requests waiting for nodes at once, all nodes together, and per
/// account: one person typing fast cannot take every place.
const MAX_WAITING: usize = 32;
const PER_USER: usize = 4;
/// The longest refusal of a node passed on, in characters.
const MAX_ERROR: usize = 300;

type Answer = Result<Vec<String>, String>;

/// Requests waiting for their node's answer, by request id.
#[derive(Default)]
pub struct Waiting(std::sync::Mutex<HashMap<String, Wait>>);

struct Wait {
    node: String,
    user: String,
    tx: oneshot::Sender<Answer>,
}

/// Why there are no paths to offer, each in words for the person.
#[derive(Debug)]
pub enum FilesError {
    Away(String),
    TooOld,
    Busy,
    Timeout,
    Refused(String),
    Unknown,
}

impl From<crate::CommandError> for FilesError {
    fn from(e: crate::CommandError) -> Self {
        match e {
            crate::CommandError::UnknownSession => FilesError::Unknown,
            other => FilesError::Refused(other.to_string()),
        }
    }
}

struct Entry<'a> {
    m: &'a Master,
    req_id: &'a str,
}

impl Drop for Entry<'_> {
    fn drop(&mut self) {
        lock(&self.m.inner.files.0).remove(self.req_id);
    }
}

/// Paths of `sid`'s folder for `query`, from the node that owns the chat;
/// `user` is who asks.
pub async fn complete(
    m: &Master,
    user: &str,
    sid: SessionId,
    query: String,
) -> Result<Vec<String>, FilesError> {
    let node = m.owner(sid).await?;
    if !m.is_connected(&node) {
        return Err(FilesError::Away(node));
    }
    if !m.node_has(&node, CAP_FILES) {
        return Err(FilesError::TooOld);
    }
    let req_id = harness_core::new_id();
    let (tx, rx) = oneshot::channel();
    {
        let mut w = lock(&m.inner.files.0);
        if w.len() >= MAX_WAITING || w.values().filter(|x| x.user == user).count() >= PER_USER {
            return Err(FilesError::Busy);
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
        MasterToNode::Files {
            req_id: req_id.clone(),
            session_id: sid,
            query,
        },
    );
    if !sent {
        return Err(FilesError::Away(node));
    }
    match tokio::time::timeout(WAIT, rx).await {
        Ok(Ok(Ok(p))) => Ok(p),
        Ok(Ok(Err(e))) => Err(FilesError::Refused(e)),
        Ok(Err(_)) => Err(FilesError::Away(node)),
        Err(_) => Err(FilesError::Timeout),
    }
}

/// A `session.files_result` from `node_id`: to the request it answers, if
/// that request went to this node; dropped otherwise. At most
/// [`MAX_FILES_RESULT`] paths are passed on.
pub fn on_answer(m: &Master, node_id: &str, req_id: String, answer: Answer) {
    let tx = {
        let mut w = lock(&m.inner.files.0);
        match w.get(&req_id) {
            Some(x) if x.node == node_id => w.remove(&req_id).map(|x| x.tx),
            Some(_) => {
                tracing::warn!(node = %node_id, "files answer for another node's request; ignored");
                None
            }
            None => None,
        }
    };
    if let Some(tx) = tx {
        let _ = tx.send(
            answer
                .map(|mut p| {
                    p.truncate(MAX_FILES_RESULT);
                    p
                })
                .map_err(|e| e.chars().take(MAX_ERROR).collect()),
        );
    }
}

/// The node's link closed: its waiting requests end now, as "away".
pub fn node_down(m: &Master, node_id: &str) {
    lock(&m.inner.files.0).retain(|_, x| x.node != node_id);
}
