//! A new project's folder on a node (ADOPTION_PLAN X.26, PLAN.md T77). The
//! master relays one `project.create` with a display name and hands the
//! one `project.created` that answers it, from that node only, to the
//! waiting request, as `files.rs` does for `@` completion. It never sends
//! a path; the path it gets back is the node's answer, stored in the
//! project's settings (`cwd`) and checked by the node again for every chat
//! started there. Who may ask is the caller's check
//! (`http/projects.rs`).

use std::collections::HashMap;
use std::time::Duration;

use harness_protocol::{CAP_PROJECTS, MasterToNode};
use tokio::sync::oneshot;

use crate::{Master, lock};

/// How long a request waits for its node: `git init` and one empty commit
/// take well under a second; a node far away or busy gets some room.
const WAIT: Duration = Duration::from_secs(30);
/// Requests waiting for nodes at once, all nodes together, and per account.
const MAX_WAITING: usize = 16;
const PER_USER: usize = 2;
/// The longest refusal of a node passed on, in characters; the longest
/// path taken (the master stores it as a project's `cwd`).
const MAX_ERROR: usize = 300;
pub const MAX_PATH: usize = 1024;

type Answer = Result<String, String>;

/// Requests waiting for their node's answer, by request id.
#[derive(Default)]
pub struct Waiting(std::sync::Mutex<HashMap<String, Wait>>);

struct Wait {
    node: String,
    user: String,
    tx: oneshot::Sender<Answer>,
}

/// Why no folder was made, each in words for the person.
#[derive(Debug)]
pub enum FolderError {
    /// The node is not connected now.
    Away(String),
    /// It did not announce `projects` (its node.toml turns them off, or it
    /// is older).
    NotOffered(String),
    Busy,
    Timeout(String),
    Refused(String),
}

impl std::fmt::Display for FolderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FolderError::Away(n) => write!(
                f,
                "node {n} is not connected: start it, or choose another node"
            ),
            FolderError::NotOffered(n) => write!(
                f,
                "node {n} makes no project folders: its node.toml turns them off ([projects] \
                 dir), or it is an older version. Choose an existing folder instead"
            ),
            FolderError::Busy => write!(f, "too many folders are being made; try again"),
            FolderError::Timeout(n) => write!(f, "node {n} did not answer in time; try again"),
            FolderError::Refused(why) => write!(f, "{why}"),
        }
    }
}

struct Entry<'a> {
    m: &'a Master,
    req_id: &'a str,
}

impl Drop for Entry<'_> {
    fn drop(&mut self) {
        lock(&self.m.inner.project_folders.0).remove(self.req_id);
    }
}

/// Ask `node` to make a new project's folder for `name`; `user` is who
/// asks. The folder's absolute path on that node.
pub async fn make(m: &Master, user: &str, node: &str, name: &str) -> Result<String, FolderError> {
    if !m.is_connected(node) {
        return Err(FolderError::Away(node.to_string()));
    }
    if !m.node_has(node, CAP_PROJECTS) {
        return Err(FolderError::NotOffered(node.to_string()));
    }
    let req_id = harness_core::new_id();
    let (tx, rx) = oneshot::channel();
    {
        let mut w = lock(&m.inner.project_folders.0);
        if w.len() >= MAX_WAITING || w.values().filter(|x| x.user == user).count() >= PER_USER {
            return Err(FolderError::Busy);
        }
        w.insert(
            req_id.clone(),
            Wait {
                node: node.to_string(),
                user: user.to_string(),
                tx,
            },
        );
    }
    let _entry = Entry { m, req_id: &req_id };
    let sent = m.send_raw(
        node,
        MasterToNode::ProjectCreate {
            req_id: req_id.clone(),
            name: name.to_string(),
        },
    );
    if !sent {
        return Err(FolderError::Away(node.to_string()));
    }
    match tokio::time::timeout(WAIT, rx).await {
        Ok(Ok(Ok(p))) => Ok(p),
        Ok(Ok(Err(e))) => Err(FolderError::Refused(e)),
        Ok(Err(_)) => Err(FolderError::Away(node.to_string())),
        Err(_) => Err(FolderError::Timeout(node.to_string())),
    }
}

/// A `project.created` from `node_id`: to the request it answers, if that
/// request went to this node; dropped otherwise. A path that is not one
/// line of at most [`MAX_PATH`] characters, or not absolute, is taken as
/// a refusal: the node said nothing usable.
pub fn on_answer(m: &Master, node_id: &str, req_id: String, answer: Answer) {
    let tx = {
        let mut w = lock(&m.inner.project_folders.0);
        match w.get(&req_id) {
            Some(x) if x.node == node_id => w.remove(&req_id).map(|x| x.tx),
            Some(_) => {
                tracing::warn!(node = %node_id, "project folder answer for another node's request; ignored");
                None
            }
            None => None,
        }
    };
    if let Some(tx) = tx {
        let answer = match answer {
            Ok(p)
                if p.starts_with('/')
                    && p.chars().count() <= MAX_PATH
                    && !p.chars().any(char::is_control) =>
            {
                Ok(p)
            }
            Ok(_) => Err("the node answered with an unusable path".to_string()),
            Err(e) => Err(e
                .chars()
                .filter(|c| !c.is_control())
                .take(MAX_ERROR)
                .collect()),
        };
        let _ = tx.send(answer);
    }
}

/// The node's link closed: its waiting requests end now, as "away".
pub fn node_down(m: &Master, node_id: &str) {
    lock(&m.inner.project_folders.0).retain(|_, x| x.node != node_id);
}
