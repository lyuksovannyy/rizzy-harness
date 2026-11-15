//! A turn's changes, asked of the node that owns the chat (X.12, PLAN.md
//! T53). The master relays one `session.diff` and hands the one
//! `session.diff_result` that answers it, from that node only, to the
//! waiting request. It keeps nothing: no cache, no log line with a path or
//! a line, nothing in its store. The visibility check is the caller's
//! (`http/diff.rs`), the same as opening the chat.

use std::collections::HashMap;
use std::time::Duration;

use harness_core::SessionId;
use harness_protocol::{CAP_DIFF, MasterToNode, TurnDiff};
use tokio::sync::oneshot;

use crate::{Master, lock};

/// How long a request waits for its node: under the route's own limit
/// (`limits.rs`, 30 s), over the node's bound on the work (20 s).
const WAIT: Duration = Duration::from_secs(25);
/// Requests waiting for nodes at once, all nodes together; past this one
/// is refused at once (a node also refuses past its own two).
const MAX_WAITING: usize = 32;
/// ...and at most this many for one account (review D9), so one person
/// with a slow node cannot take every place.
const PER_USER: usize = 4;

/// The longest refusal of a node passed on, in characters.
const MAX_ERROR: usize = 1000;

type Answer = Result<Box<TurnDiff>, String>;

/// Requests waiting for their node's answer, by request id.
#[derive(Default)]
pub struct Waiting(std::sync::Mutex<HashMap<String, Wait>>);

/// One waiting request: the node it went to, who asked, where the answer
/// goes.
struct Wait {
    node: String,
    user: String,
    tx: oneshot::Sender<Answer>,
}

/// Why there is no diff to show, each in words for the person.
#[derive(Debug)]
pub enum DiffError {
    /// The node is not connected (or went while the request waited).
    Away(String),
    /// The node is older than this server.
    TooOld,
    /// The server waits for too many diffs already.
    Busy,
    /// No answer in time.
    Timeout,
    /// The node's own words (pruned, unknown turn, busy, too large...).
    Refused(String),
    Unknown,
}

impl From<crate::CommandError> for DiffError {
    fn from(e: crate::CommandError) -> Self {
        match e {
            crate::CommandError::UnknownSession => DiffError::Unknown,
            other => DiffError::Refused(other.to_string()),
        }
    }
}

/// Removes a request's entry however its wait ends.
struct Entry<'a> {
    m: &'a Master,
    req_id: &'a str,
}

impl Drop for Entry<'_> {
    fn drop(&mut self) {
        lock(&self.m.inner.diffs.0).remove(self.req_id);
    }
}

/// What turn `turn_id` of `sid` changed (with `rewind`, it and every later
/// turn), from the node that owns the chat; `user` is who asks (the API
/// token counts as one).
pub async fn turn_diff(
    m: &Master,
    user: &str,
    sid: SessionId,
    turn_id: String,
    rewind: bool,
) -> Result<Box<TurnDiff>, DiffError> {
    let node = m.owner(sid).await?;
    if !m.is_connected(&node) {
        return Err(DiffError::Away(node));
    }
    if !m.node_has(&node, CAP_DIFF) {
        return Err(DiffError::TooOld);
    }
    let req_id = harness_core::new_id();
    let (tx, rx) = oneshot::channel();
    {
        let mut w = lock(&m.inner.diffs.0);
        if w.len() >= MAX_WAITING || w.values().filter(|x| x.user == user).count() >= PER_USER {
            return Err(DiffError::Busy);
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
        MasterToNode::Diff {
            req_id: req_id.clone(),
            session_id: sid,
            turn_id,
            rewind,
        },
    );
    if !sent {
        return Err(DiffError::Away(node));
    }
    match tokio::time::timeout(WAIT, rx).await {
        Ok(Ok(Ok(d))) => Ok(d),
        Ok(Ok(Err(e))) => Err(DiffError::Refused(e)),
        // The node's link closed while it worked (`node_down`).
        Ok(Err(_)) => Err(DiffError::Away(node)),
        Err(_) => Err(DiffError::Timeout),
    }
}

/// A `session.diff_result` from `node_id`: to the request it answers, if
/// that request went to this node; dropped otherwise.
pub fn on_answer(m: &Master, node_id: &str, req_id: String, answer: Answer) {
    let tx = {
        let mut w = lock(&m.inner.diffs.0);
        match w.get(&req_id) {
            Some(x) if x.node == node_id => w.remove(&req_id).map(|x| x.tx),
            Some(_) => {
                tracing::warn!(node = %node_id, "diff answer for another node's request; ignored");
                None
            }
            None => None,
        }
    };
    if let Some(tx) = tx {
        // The node's words, shown to the owner as text; a bounded length.
        let _ = tx.send(answer.map_err(|e| e.chars().take(MAX_ERROR).collect()));
    }
}

/// The node's link closed: its waiting requests end now, as "away".
pub fn node_down(m: &Master, node_id: &str) {
    lock(&m.inner.diffs.0).retain(|_, x| x.node != node_id);
}
