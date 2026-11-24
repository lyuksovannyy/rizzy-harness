//! Previewing a Word or Excel file of a chat's folder (ADOPTION_PLAN X.59,
//! PLAN.md T99): the master relays one `session.doc_preview` to the node
//! that owns the chat and hands the one `session.doc_preview_result` that
//! answers it, from that node only, to the waiting request, as `files.rs`
//! does for the `@` completion. It reads, resolves and parses nothing: the
//! path is text for the node's own rules, the file is read by the node's
//! confined helper. It keeps nothing and writes no path to a log. The owner
//! check is the caller's (`http/folder_preview.rs`).
//!
//! A person's text changes to such a file (X.60, T100), and the view of a text
//! file (X.63), wait the same way, in the same map (so one account's preview and edit share its one place),
//! each answer taken only by the kind of request that is waiting for it.

use std::collections::HashMap;
use std::time::Duration;

use harness_core::SessionId;
use harness_protocol::{
    CAP_DOC_PAGE_EDIT, CAP_DOC_PREVIEW, CAP_FOLDER_FILE, ChangeResult, FileRefusal, FolderFileText,
    MasterToNode, PageChange, PreviewRefusal,
};
use serde_json::Value;
use tokio::sync::oneshot;

use crate::{Master, lock};

/// How long a request waits for its node: the helper's queue (10 s) and run
/// (20 s) bounds and the file's read.
const WAIT: Duration = Duration::from_secs(40);
/// A text file's read has no helper to queue for: the node answers at once or
/// not at all.
const WAIT_FILE: Duration = Duration::from_secs(10);
/// Requests waiting at once, all nodes together, and per account: one
/// person's previews run one at a time, so they cannot fill the node's
/// helper places that the agent's own tool calls use.
const MAX_WAITING: usize = 16;
const PER_USER: usize = 1;

/// What a node answered.
#[derive(Debug)]
pub enum Got {
    Preview(Value),
    Edit {
        saved: Option<String>,
        results: Vec<ChangeResult>,
    },
    File(Result<FolderFileText, FileRefusal>),
}

type Answer = Result<Got, PreviewRefusal>;

/// What a waiting request is for: an answer of the other kind is not taken.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Preview,
    Edit,
    File,
}

/// What a page edit got: the new file's path (none when a change was
/// refused) and one result per change.
#[derive(Debug)]
pub struct Edited {
    pub saved: Option<String>,
    pub results: Vec<ChangeResult>,
}

/// Requests waiting for nodes' answers, by request id.
#[derive(Default)]
pub struct Waiting(std::sync::Mutex<HashMap<String, Wait>>);

struct Wait {
    kind: Kind,
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
    /// A text file's refusal (X.63).
    File(FileRefusal),
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

/// No room for `user`'s request: too many waiting in all, or this account's
/// own place is taken.
fn full(w: &HashMap<String, Wait>, user: &str) -> bool {
    w.len() >= MAX_WAITING || w.values().filter(|x| x.user == user).count() >= PER_USER
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
    check_node(m, &node, CAP_DOC_PREVIEW)?;
    let req_id = harness_core::new_id();
    let msg = MasterToNode::DocPreview {
        req_id: req_id.clone(),
        session_id: sid,
        path,
        sheet,
    };
    match wait(m, user, node, req_id, Kind::Preview, WAIT, msg).await? {
        Got::Preview(v) => Ok(v),
        _ => Err(PreviewError::Unknown),
    }
}

/// `changes` to the file at `path` in `sid`'s folder, saved by the node that
/// owns the chat as a new file beside it.
pub async fn ask_edit(
    m: &Master,
    user: &str,
    sid: SessionId,
    path: String,
    sheet: Option<u32>,
    changes: Vec<PageChange>,
) -> Result<Edited, PreviewError> {
    let node = m.owner(sid).await?;
    check_node(m, &node, CAP_DOC_PAGE_EDIT)?;
    let req_id = harness_core::new_id();
    let msg = MasterToNode::DocPageEdit {
        req_id: req_id.clone(),
        session_id: sid,
        path,
        sheet,
        changes,
    };
    match wait(m, user, node, req_id, Kind::Edit, WAIT, msg).await? {
        Got::Edit { saved, results } => Ok(Edited { saved, results }),
        _ => Err(PreviewError::Unknown),
    }
}

/// The text of `path` in `sid`'s folder (X.63), read by the node that owns
/// the chat; `user` is who asks.
pub async fn ask_file(
    m: &Master,
    user: &str,
    sid: SessionId,
    path: String,
) -> Result<FolderFileText, PreviewError> {
    let node = m.owner(sid).await?;
    check_node(m, &node, CAP_FOLDER_FILE)?;
    let req_id = harness_core::new_id();
    let msg = MasterToNode::FolderFile {
        req_id: req_id.clone(),
        session_id: sid,
        path,
    };
    match wait(m, user, node, req_id, Kind::File, WAIT_FILE, msg).await? {
        Got::File(Ok(f)) => Ok(f),
        Got::File(Err(r)) => Err(PreviewError::File(r)),
        _ => Err(PreviewError::Unknown),
    }
}

fn check_node(m: &Master, node: &str, cap: &str) -> Result<(), PreviewError> {
    if !m.is_connected(node) {
        return Err(PreviewError::Away(node.to_string()));
    }
    if !m.node_has(node, cap) {
        return Err(PreviewError::TooOld);
    }
    Ok(())
}

async fn wait(
    m: &Master,
    user: &str,
    node: String,
    req_id: String,
    kind: Kind,
    bound: Duration,
    msg: MasterToNode,
) -> Result<Got, PreviewError> {
    let (tx, rx) = oneshot::channel();
    {
        let mut w = lock(&m.inner.previews.0);
        if full(&w, user) {
            return Err(PreviewError::Busy);
        }
        w.insert(
            req_id.clone(),
            Wait {
                kind,
                node: node.clone(),
                user: user.to_string(),
                tx,
            },
        );
    }
    let _entry = Entry { m, req_id: &req_id };
    if !m.send_raw(&node, msg) {
        return Err(PreviewError::Away(node));
    }
    match tokio::time::timeout(bound, rx).await {
        Ok(Ok(Ok(p))) => Ok(p),
        Ok(Ok(Err(r))) => Err(PreviewError::Refused(r)),
        Ok(Err(_)) => Err(PreviewError::Away(node)),
        Err(_) => Err(PreviewError::Timeout),
    }
}

/// A `session.doc_preview_result` from `node_id`: to the request it answers,
/// if that request went to this node; dropped otherwise. The size is the
/// route's to check (it parses the value once).
pub fn on_answer(m: &Master, node_id: &str, req_id: String, answer: Result<Value, PreviewRefusal>) {
    deliver(m, node_id, req_id, Kind::Preview, answer.map(Got::Preview));
}

/// A `session.folder_file_result` from `node_id`, likewise.
pub fn on_file_answer(
    m: &Master,
    node_id: &str,
    req_id: String,
    answer: Result<FolderFileText, FileRefusal>,
) {
    deliver(m, node_id, req_id, Kind::File, Ok(Got::File(answer)));
}

/// A `session.doc_page_edit_result` from `node_id`, likewise.
pub fn on_edit_answer(
    m: &Master,
    node_id: &str,
    req_id: String,
    answer: Result<(Option<String>, Vec<ChangeResult>), PreviewRefusal>,
) {
    deliver(
        m,
        node_id,
        req_id,
        Kind::Edit,
        answer.map(|(saved, results)| Got::Edit { saved, results }),
    );
}

fn deliver(m: &Master, node_id: &str, req_id: String, kind: Kind, answer: Answer) {
    let tx = {
        let mut w = lock(&m.inner.previews.0);
        match w.get(&req_id) {
            Some(x) if x.node == node_id && x.kind == kind => w.remove(&req_id).map(|x| x.tx),
            Some(_) => {
                tracing::warn!(node = %node_id, "answer for another node's or another kind of request; ignored");
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

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(user: &str) -> Wait {
        Wait {
            kind: Kind::Preview,
            node: "n".into(),
            user: user.into(),
            tx: oneshot::channel().0,
        }
    }

    /// One place per account, and a bound on all of them together.
    #[test]
    fn the_places_are_bounded_per_account_and_in_all() {
        let mut w = HashMap::new();
        assert!(!full(&w, "a"));
        w.insert("r-a".to_string(), wait("a"));
        assert!(full(&w, "a"), "an account's second request waits");
        assert!(!full(&w, "b"), "another account is not held up");
        for i in 1..MAX_WAITING {
            assert!(!full(&w, &format!("u{i}")), "{i}");
            w.insert(format!("r{i}"), wait(&format!("u{i}")));
        }
        assert_eq!(w.len(), MAX_WAITING);
        assert!(
            full(&w, "newcomer"),
            "no room for anybody once all are taken"
        );
    }
}
