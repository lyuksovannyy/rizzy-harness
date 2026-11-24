//! What a background command is printing (ADOPTION_PLAN X.62, PLAN.md T53
//! and T8D): the tail of its output, asked of the node that owns the chat.
//! The master relays one `session.background_output` and hands the one
//! `session.background_output_result` that answers it, from that node
//! only, to the waiting request, as `files.rs` does for the composer's
//! completion. It keeps nothing: no cache, no copy, no log line with a
//! command or its text, nothing in its store. The owner check is the
//! caller's (`http/bg_output.rs`).

use std::collections::HashMap;
use std::time::Duration;

use harness_core::SessionId;
use harness_protocol::{BackgroundOutput, CAP_BG_OUTPUT, MAX_BG_OUTPUT_BYTES, MasterToNode};
use tokio::sync::oneshot;

use crate::{Master, lock};

/// How long a request waits for its node (a read of memory; a node that
/// does not answer in this time is of no use to a strip that polls).
const WAIT: Duration = Duration::from_secs(5);
/// Requests waiting for nodes at once, all nodes together, and per
/// account: a person with many rows open, or a script, cannot take every
/// place.
const MAX_WAITING: usize = 32;
const PER_USER: usize = 4;
/// The longest refusal of a node passed on, in characters.
const MAX_ERROR: usize = 300;

type Answer = Result<BackgroundOutput, String>;

/// Requests waiting for their node's answer, by request id.
#[derive(Default)]
pub struct Waiting(std::sync::Mutex<HashMap<String, Wait>>);

struct Wait {
    node: String,
    user: String,
    tx: oneshot::Sender<Answer>,
}

/// Why there is no output to show, each in words for the person.
#[derive(Debug)]
pub enum BgOutputError {
    Away(String),
    TooOld,
    Busy,
    Timeout,
    /// The node's own words (forgotten, not this chat's, a sub-agent...).
    Refused(String),
    Unknown,
}

impl From<crate::CommandError> for BgOutputError {
    fn from(e: crate::CommandError) -> Self {
        match e {
            crate::CommandError::UnknownSession => BgOutputError::Unknown,
            other => BgOutputError::Refused(other.to_string()),
        }
    }
}

struct Entry<'a> {
    m: &'a Master,
    req_id: &'a str,
}

impl Drop for Entry<'_> {
    fn drop(&mut self) {
        lock(&self.m.inner.bg_outputs.0).remove(self.req_id);
    }
}

/// The tail of `bg_id`'s output in chat `sid`, from the node that owns the
/// chat; `user` is who asks.
pub async fn output(
    m: &Master,
    user: &str,
    sid: SessionId,
    bg_id: String,
) -> Result<BackgroundOutput, BgOutputError> {
    let node = m.owner(sid).await?;
    if !m.is_connected(&node) {
        return Err(BgOutputError::Away(node));
    }
    if !m.node_has(&node, CAP_BG_OUTPUT) {
        return Err(BgOutputError::TooOld);
    }
    let req_id = harness_core::new_id();
    let (tx, rx) = oneshot::channel();
    {
        let mut w = lock(&m.inner.bg_outputs.0);
        if w.len() >= MAX_WAITING || w.values().filter(|x| x.user == user).count() >= PER_USER {
            return Err(BgOutputError::Busy);
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
        MasterToNode::BackgroundOutput {
            req_id: req_id.clone(),
            session_id: sid,
            bg_id,
        },
    );
    if !sent {
        return Err(BgOutputError::Away(node));
    }
    match tokio::time::timeout(WAIT, rx).await {
        Ok(Ok(Ok(o))) => Ok(o),
        Ok(Ok(Err(e))) => Err(BgOutputError::Refused(e)),
        Ok(Err(_)) => Err(BgOutputError::Away(node)),
        Err(_) => Err(BgOutputError::Timeout),
    }
}

/// `text` cut from the front to at most `max` bytes, on a character
/// boundary: what a node that sent more than it may is held to.
fn tail(text: String, max: usize) -> (String, bool) {
    if text.len() <= max {
        return (text, false);
    }
    let mut from = text.len() - max;
    while !text.is_char_boundary(from) {
        from += 1;
    }
    (text[from..].to_string(), true)
}

/// A `session.background_output_result` from `node_id`: to the request it
/// answers, if that request went to this node; dropped otherwise. The text
/// is held to [`MAX_BG_OUTPUT_BYTES`] whatever the node sent.
pub fn on_answer(m: &Master, node_id: &str, req_id: String, answer: Answer) {
    let tx = {
        let mut w = lock(&m.inner.bg_outputs.0);
        match w.get(&req_id) {
            Some(x) if x.node == node_id => w.remove(&req_id).map(|x| x.tx),
            Some(_) => {
                tracing::warn!(node = %node_id, "output answer for another node's request; ignored");
                None
            }
            None => None,
        }
    };
    if let Some(tx) = tx {
        let _ = tx.send(
            answer
                .map(|mut o| {
                    let (text, cut) = tail(std::mem::take(&mut o.text), MAX_BG_OUTPUT_BYTES);
                    o.text = text;
                    o.cut |= cut;
                    o
                })
                .map_err(|e| e.chars().take(MAX_ERROR).collect()),
        );
    }
}

/// The node's link closed: its waiting requests end now, as "away".
pub fn node_down(m: &Master, node_id: &str) {
    lock(&m.inner.bg_outputs.0).retain(|_, x| x.node != node_id);
}

#[cfg(test)]
mod tests {
    use super::tail;

    #[test]
    fn a_long_text_is_cut_from_the_front_on_a_boundary() {
        let (t, cut) = tail("é".repeat(10), 7);
        assert!(
            cut && t.len() <= 7 && t.chars().all(|c| c == 'é'),
            "{}",
            t.len()
        );
        let (t, cut) = tail("abc".into(), 3);
        assert_eq!((t.as_str(), cut), ("abc", false));
    }
}
