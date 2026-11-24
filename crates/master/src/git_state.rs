//! The git branch and changed-file count of a chat's folder for its header,
//! asked of the node that owns the chat (ADOPTION_PLAN X.64, PLAN.md T19).
//! The master relays one `session.git_state` and hands the one
//! `session.git_state_result` that answers it, from that node only, to the
//! waiting request. It reads no folder and runs no program; it keeps
//! nothing: no cache, no log line, nothing in its store. What a node sends is
//! text from a repository someone else controls, so it is bounded again here
//! before it goes on to a page. The visibility check is the caller's
//! (`http/git_state.rs`), the same as opening the chat.

use std::collections::HashMap;
use std::time::Duration;

use harness_core::SessionId;
use harness_protocol::{
    CAP_GIT_STATE, GitState, MAX_GIT_BRANCH_BYTES, MAX_GIT_CHANGED, MasterToNode,
};
use tokio::sync::oneshot;

use crate::{Master, lock};

/// How long a request waits for its node: over the node's own bound on the
/// work (12 s), under the route's limit (`limits.rs`).
const WAIT: Duration = Duration::from_secs(16);
/// Requests waiting at once, all nodes together, and per account.
const MAX_WAITING: usize = 32;
const PER_USER: usize = 2;

/// What a node answered.
enum Answer {
    State(Option<GitState>),
    Busy,
}

/// Requests waiting for nodes' answers, by request id.
#[derive(Default)]
pub struct Waiting(std::sync::Mutex<HashMap<String, Wait>>);

struct Wait {
    node: String,
    user: String,
    tx: oneshot::Sender<Answer>,
}

/// Why there is nothing to show, for the route to say or to say nothing of.
#[derive(Debug, PartialEq, Eq)]
pub enum GitError {
    /// The node is not connected (or went while the request waited).
    Away,
    /// The node is older than this server.
    TooOld,
    /// This server, or the node, answers other requests already; the page
    /// keeps what it shows.
    Busy,
    /// No answer in time.
    Timeout,
    Unknown,
}

impl From<crate::CommandError> for GitError {
    fn from(_: crate::CommandError) -> Self {
        GitError::Unknown
    }
}

/// Removes a request's entry however its wait ends.
struct Entry<'a> {
    m: &'a Master,
    req_id: &'a str,
}

impl Drop for Entry<'_> {
    fn drop(&mut self) {
        lock(&self.m.inner.git_states.0).remove(self.req_id);
    }
}

/// The git state of `sid`'s folder, from the node that owns the chat;
/// `user` is who asks (the API token counts as one). `Ok(None)`: the node
/// reads no repository there.
pub async fn ask(m: &Master, user: &str, sid: SessionId) -> Result<Option<GitState>, GitError> {
    let node = m.owner(sid).await?;
    if !m.is_connected(&node) {
        return Err(GitError::Away);
    }
    if !m.node_has(&node, CAP_GIT_STATE) {
        return Err(GitError::TooOld);
    }
    let req_id = harness_core::new_id();
    let (tx, rx) = oneshot::channel();
    {
        let mut w = lock(&m.inner.git_states.0);
        if w.len() >= MAX_WAITING || w.values().filter(|x| x.user == user).count() >= PER_USER {
            return Err(GitError::Busy);
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
        MasterToNode::GitState {
            req_id: req_id.clone(),
            session_id: sid,
        },
    );
    if !sent {
        return Err(GitError::Away);
    }
    match tokio::time::timeout(WAIT, rx).await {
        Ok(Ok(Answer::State(s))) => Ok(s.and_then(bounded)),
        Ok(Ok(Answer::Busy)) => Err(GitError::Busy),
        // The node's link closed while it worked (`node_down`).
        Ok(Err(_)) => Err(GitError::Away),
        Err(_) => Err(GitError::Timeout),
    }
}

/// What a node sent, held to the shape this page shows: a branch of at most
/// [`MAX_GIT_BRANCH_BYTES`] without control or line-separator characters, a
/// commit of seven to sixty-four lowercase hex digits, a count within the
/// cap. A state with nothing in it is none.
fn bounded(s: GitState) -> Option<GitState> {
    let branch = s.branch.map(|b| {
        let mut out: String = b
            .chars()
            .map(|c| {
                if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') {
                    '\u{FFFD}'
                } else {
                    c
                }
            })
            .collect();
        if out.len() > MAX_GIT_BRANCH_BYTES {
            let mut end = MAX_GIT_BRANCH_BYTES;
            while !out.is_char_boundary(end) {
                end -= 1;
            }
            out.truncate(end);
        }
        out
    });
    let commit = s.commit.filter(|c| {
        (7..=64).contains(&c.len()) && c.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    });
    let changed = s.changed.map(|n| n.min(MAX_GIT_CHANGED));
    (branch.is_some() || commit.is_some()).then_some(GitState {
        branch,
        commit,
        changed,
    })
}

/// A `session.git_state_result` from `node_id`: to the request it answers, if
/// that request went to this node; dropped otherwise.
pub fn on_answer(m: &Master, node_id: &str, req_id: String, state: Option<GitState>, busy: bool) {
    let tx = {
        let mut w = lock(&m.inner.git_states.0);
        match w.get(&req_id) {
            Some(x) if x.node == node_id => w.remove(&req_id).map(|x| x.tx),
            Some(_) => {
                tracing::warn!(node = %node_id, "git state answer for another node's request; ignored");
                None
            }
            None => None,
        }
    };
    if let Some(tx) = tx {
        let _ = tx.send(if busy {
            Answer::Busy
        } else {
            Answer::State(state)
        });
    }
}

/// The node's link closed: its waiting requests end now, as "away".
pub fn node_down(m: &Master, node_id: &str) {
    lock(&m.inner.git_states.0).retain(|_, x| x.node != node_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(branch: Option<&str>, commit: Option<&str>, changed: Option<u32>) -> GitState {
        GitState {
            branch: branch.map(Into::into),
            commit: commit.map(Into::into),
            changed,
        }
    }

    /// A node that is wrong or hostile cannot hand the page more than the
    /// shape it shows.
    #[test]
    fn what_a_node_sends_is_held_to_the_shape() {
        let long = "é".repeat(500);
        let s = bounded(state(Some(&long), Some("abcdef0"), Some(u32::MAX))).unwrap();
        assert!(s.branch.as_ref().unwrap().len() <= MAX_GIT_BRANCH_BYTES);
        assert_eq!(s.changed, Some(MAX_GIT_CHANGED));
        let s = bounded(state(Some("a\u{1b}[0m\nb\u{2028}c"), Some("ABCDEF0"), None)).unwrap();
        assert_eq!(s.branch.as_deref(), Some("a\u{FFFD}[0m\u{FFFD}b\u{FFFD}c"));
        assert_eq!(s.commit, None, "upper case is not a hex id here");
        assert_eq!(bounded(state(None, Some("<img src=x>"), Some(1))), None);
        assert_eq!(bounded(state(None, None, Some(1))), None);
        assert!(bounded(state(None, Some(&"a".repeat(65)), None)).is_none());
    }
}
