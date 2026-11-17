//! A new project's folder on a node (ADOPTION_PLAN X.26, PLAN.md T77). The
//! master relays one `project.create` with a display name and hands the
//! one `project.created` that answers it, from that node only, to the
//! waiting request, as `files.rs` does for `@` completion. It never sends
//! a path; the path it gets back is the node's answer, stored in the
//! project's settings (`cwd`) and checked by the node again for every chat
//! started there. Who may ask is the caller's check
//! (`http/projects.rs`).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
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

/// Requests waiting for their node's answer, by request id; the projects
/// a folder is being made for now (one request per project at a time);
/// the wait (tests shorten it).
#[derive(Default)]
pub struct Waiting {
    pending: std::sync::Mutex<HashMap<String, Wait>>,
    claims: std::sync::Mutex<HashSet<String>>,
    wait_ms: AtomicU64,
}

impl Waiting {
    fn wait(&self) -> Duration {
        match self.wait_ms.load(Ordering::Relaxed) {
            0 => WAIT,
            ms => Duration::from_millis(ms),
        }
    }
}

impl Master {
    /// Tests: how long a new folder is waited for (0: the default, 30 s).
    #[doc(hidden)]
    pub fn set_project_folder_wait(&self, d: Duration) {
        self.inner
            .project_folders
            .wait_ms
            .store(d.as_millis() as u64, Ordering::Relaxed);
    }
}

/// A project a folder is being made for: held from before the node is
/// asked until the project's settings are written, so a second request
/// for the same project is refused before any node is asked (review of
/// PR 143: two at once each made a folder and the project kept one).
pub struct Claim {
    m: Master,
    project: String,
}

impl Drop for Claim {
    fn drop(&mut self) {
        lock(&self.m.inner.project_folders.claims).remove(&self.project);
    }
}

/// Claim `project` for one folder request, or `None` while another holds it.
pub fn claim(m: &Master, project: &str) -> Option<Claim> {
    lock(&m.inner.project_folders.claims)
        .insert(project.to_string())
        .then(|| Claim {
            m: m.clone(),
            project: project.to_string(),
        })
}

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
    /// No answer in time: the node may still make the folder. The node,
    /// and the folder name's first part (the slug of the name).
    Timeout(String, String),
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
            FolderError::Timeout(n, slug) => write!(
                f,
                "node {n} did not answer in time; it may still make the folder: if a folder \
                 named {slug}-… appears in its projects folder, remove it, or open it with \
                 Existing folder"
            ),
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
        lock(&self.m.inner.project_folders.pending).remove(self.req_id);
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
        let mut w = lock(&m.inner.project_folders.pending);
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
    match tokio::time::timeout(m.inner.project_folders.wait(), rx).await {
        Ok(Ok(Ok(p))) => Ok(p),
        Ok(Ok(Err(e))) => Err(FolderError::Refused(e)),
        Ok(Err(_)) => Err(FolderError::Away(node.to_string())),
        Err(_) => Err(FolderError::Timeout(
            node.to_string(),
            harness_node::projects::slug(name),
        )),
    }
}

/// A `project.created` from `node_id`: to the request it answers, if that
/// request went to this node; dropped otherwise. A path that is not one
/// line of at most [`MAX_PATH`] characters, or not absolute, is taken as
/// a refusal: the node said nothing usable.
pub fn on_answer(m: &Master, node_id: &str, req_id: String, answer: Answer) {
    let tx = {
        let mut w = lock(&m.inner.project_folders.pending);
        match w.get(&req_id) {
            Some(x) if x.node == node_id => w.remove(&req_id).map(|x| x.tx),
            Some(_) => {
                tracing::warn!(node = %node_id, "project folder answer for another node's request; ignored");
                None
            }
            None => {
                // Its request timed out (the person was told the node may
                // still make it): the master's own log says where it is.
                if let Ok(p) = &answer {
                    tracing::warn!(
                        node = %node_id,
                        folder = %p.chars().take(MAX_PATH).collect::<String>().escape_debug(),
                        "a project folder was made after its request gave up; no project names it"
                    );
                }
                None
            }
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
    lock(&m.inner.project_folders.pending).retain(|_, x| x.node != node_id);
}
