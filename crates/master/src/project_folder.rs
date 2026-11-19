//! A new project's folder on a node (ADOPTION_PLAN X.26, PLAN.md T77). The
//! master relays one `project.create` with a display name and hands the
//! one `project.created` that answers it, from that node only, to the
//! waiting request, as `files.rs` does for `@` completion. It never sends
//! a path; the path it gets back is the node's answer, stored in the
//! project's settings (`cwd`) and checked by the node again for every chat
//! started there. Who may ask is the caller's check
//! (`http/projects.rs`).
//!
//! A clone (`project.clone`, X.27 step 3, PLAN.md T86) goes the same way
//! and is answered the same way; the master waits longer than the clone
//! time limit the node announced, and the credential it sends (a private
//! repository's only) is in a `Secret` from the sealed row to the link.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use harness_core::Secret;
use harness_protocol::{CAP_PROJECTS, MasterToNode};
use tokio::sync::oneshot;

use crate::{Master, lock};

/// How long a request waits for its node: `git init` and one empty commit
/// take well under a second; a node far away or busy gets some room.
const WAIT: Duration = Duration::from_secs(30);
/// Folder requests waiting for nodes at once, all nodes together, and per
/// account. A clone waits up to the node's limit plus 60 s, so clones have a
/// budget of their own (below): a few people's clones never refuse a plain
/// "New project" folder.
const MAX_WAITING: usize = 16;
const PER_USER: usize = 2;
/// Clones waiting for nodes at once, all nodes together, and per account.
const MAX_CLONING: usize = 8;
const PER_USER_CLONING: usize = 2;
/// The longest refusal of a node shown for a plain folder request, in
/// characters; the longest path taken (the master stores it as a project's
/// `cwd`). A clone's refusal is passed on whole up to [`MAX_RAW`] and cut by
/// whoever stores it, after the credential was taken out of it.
const MAX_ERROR: usize = 300;
const MAX_RAW: usize = 2000;
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
    /// A `project.clone`, counted against the clone budget.
    clone: bool,
    tx: oneshot::Sender<Answer>,
}

/// `text` without control characters and without Unicode format characters
/// (bidirectional overrides and isolates, zero-width characters, the byte
/// order mark, tag characters): text a node or a forge wrote, shown to a
/// person, must not reorder or hide what is around it.
pub fn clean_text(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() && !is_format(*c))
        .collect()
}

/// Unicode category Cf, as far as it can hide or reorder text.
fn is_format(c: char) -> bool {
    matches!(c,
        '\u{ad}' | '\u{600}'..='\u{605}' | '\u{61c}' | '\u{6dd}' | '\u{70f}' | '\u{8e2}'
        | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}'
        | '\u{2060}'..='\u{206f}' | '\u{feff}' | '\u{fff9}'..='\u{fffb}'
        | '\u{e0000}'..='\u{e007f}')
}

/// Why no folder was made, each in words for the person.
#[derive(Debug)]
pub enum FolderError {
    /// The node is not connected now.
    Away(String),
    /// It did not announce `projects` (its node.toml turns them off, or it
    /// is older).
    NotOffered(String),
    /// It did not announce `forge-clone` (its node.toml turns cloning off,
    /// or it is older): never sent a `project.clone`.
    NoClone(String),
    Busy,
    /// The clone budget (the server's, or the person's) is used up.
    CloneBusy,
    /// The node's link closed while it was cloning: the node's own clone
    /// task goes on. The node, and the folder name's first part.
    LinkLost(String, String),
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
            FolderError::NoClone(n) => write!(
                f,
                "node {n} cannot clone repositories: its node.toml turns cloning off ([forge] \
                 clone or hosts, [projects] dir), or it is an older version"
            ),
            FolderError::Busy => write!(f, "too many folders are being made; try again"),
            FolderError::CloneBusy => write!(
                f,
                "too many repositories are being cloned right now (you may have two at a time); \
                 try again when one has finished"
            ),
            FolderError::LinkLost(n, slug) => write!(
                f,
                "node {n}'s link closed while it was cloning. The clone may still finish there: \
                 if a folder named {slug}-… appears in its projects folder, open it with \
                 Existing folder, or remove it"
            ),
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

struct Entry {
    m: Master,
    req_id: String,
}

impl Drop for Entry {
    fn drop(&mut self) {
        lock(&self.m.inner.project_folders.pending).remove(&self.req_id);
    }
}

/// A request sent to its node: [`Sent::answer`] waits for the answer. Its
/// place among the waiting requests goes when it is dropped.
pub struct Sent {
    entry: Entry,
    rx: oneshot::Receiver<Answer>,
    wait: Duration,
    node: String,
    slug: String,
    clone: bool,
}

impl Sent {
    /// The folder's absolute path on the node, or why there is none. A
    /// clone's refusal is the node's text cleaned of hidden characters but
    /// not yet cut (the caller takes the credential out first).
    pub async fn answer(self) -> Result<String, FolderError> {
        let _entry = self.entry;
        match tokio::time::timeout(self.wait, self.rx).await {
            Ok(Ok(Ok(p))) => Ok(p),
            Ok(Ok(Err(e))) if self.clone => Err(FolderError::Refused(e)),
            Ok(Ok(Err(e))) => Err(FolderError::Refused(e.chars().take(MAX_ERROR).collect())),
            // The sender went with the link ([`node_down`]). A folder
            // request had nothing started that the person could trip over;
            // a clone goes on at the node.
            Ok(Err(_)) if self.clone => Err(FolderError::LinkLost(self.node, self.slug)),
            Ok(Err(_)) => Err(FolderError::Away(self.node)),
            Err(_) => Err(FolderError::Timeout(self.node, self.slug)),
        }
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
    let wait = m.inner.project_folders.wait();
    ask(m, user, node, name, wait, false, |req_id| {
        MasterToNode::ProjectCreate {
            req_id,
            name: name.to_string(),
        }
    })
    .await
}

/// A forge repository to clone (T86): checked by the caller against the
/// person's own connection and the forge.
pub struct CloneSource {
    pub host: String,
    pub full_name: String,
    /// Both for a private repository only.
    pub username: Option<String>,
    pub token: Option<Secret<String>>,
}

/// Why a clone that was running when the master stopped failed.
pub const RESTARTED: &str = "the server restarted while this repository was being cloned. The \
     node may still have made its folder: if a new folder for it appears in that node's projects \
     folder, remove it, or open it with Existing folder. Then try again";

/// How much longer than the node's own clone limit the master waits: the
/// node ends git at its limit, then removes what it fetched and answers.
pub const CLONE_GRACE: Duration = Duration::from_secs(60);

/// Whether `node` is connected and clones (an older node, or one whose
/// node.toml turns cloning off, is never sent a `project.clone`).
pub fn may_clone_on(m: &Master, node: &str) -> Result<u64, FolderError> {
    if !m.is_connected(node) {
        return Err(FolderError::Away(node.to_string()));
    }
    m.node_clone_timeout(node)
        .ok_or_else(|| FolderError::NoClone(node.to_string()))
}

/// Ask `node` to clone `src` into a new project's folder named after
/// `name`; `user` is who asks. The folder's absolute path on that node.
///
/// Sent at once, answered later: the request is among the waiting ones
/// (and counts against their limits) from the moment this returns, so a
/// refusal (away, busy, no clone) comes before anything is promised to
/// the person, and the answer is waited for by whoever holds [`Sent`].
pub fn clone(
    m: &Master,
    user: &str,
    node: &str,
    name: &str,
    src: CloneSource,
) -> Result<Sent, FolderError> {
    let limit = may_clone_on(m, node)?;
    let wait = match m.inner.project_folders.wait_ms.load(Ordering::Relaxed) {
        0 => Duration::from_secs(limit) + CLONE_GRACE,
        ms => Duration::from_millis(ms),
    };
    send(m, user, node, name, wait, true, move |req_id| {
        MasterToNode::ProjectClone {
            req_id,
            name: name.to_string(),
            host: src.host,
            full_name: src.full_name,
            username: src.username,
            token: src.token,
        }
    })
}

/// Send the message `build` makes for a new request id to `node` and wait
/// up to `wait` for its `project.created`.
async fn ask(
    m: &Master,
    user: &str,
    node: &str,
    name: &str,
    wait: Duration,
    clone: bool,
    build: impl FnOnce(String) -> MasterToNode,
) -> Result<String, FolderError> {
    send(m, user, node, name, wait, clone, build)?
        .answer()
        .await
}

/// Register a new request for `node` among the waiting ones and send it
/// the message `build` makes for its id.
fn send(
    m: &Master,
    user: &str,
    node: &str,
    name: &str,
    wait: Duration,
    clone: bool,
    build: impl FnOnce(String) -> MasterToNode,
) -> Result<Sent, FolderError> {
    let req_id = harness_core::new_id();
    let (tx, rx) = oneshot::channel();
    {
        // Folder requests and clones are counted apart.
        let mut w = lock(&m.inner.project_folders.pending);
        let (all, mine) = w
            .values()
            .filter(|x| x.clone == clone)
            .fold((0, 0), |(a, p), x| (a + 1, p + usize::from(x.user == user)));
        let (max, per_user) = if clone {
            (MAX_CLONING, PER_USER_CLONING)
        } else {
            (MAX_WAITING, PER_USER)
        };
        if all >= max || mine >= per_user {
            return Err(if clone {
                FolderError::CloneBusy
            } else {
                FolderError::Busy
            });
        }
        w.insert(
            req_id.clone(),
            Wait {
                node: node.to_string(),
                user: user.to_string(),
                clone,
                tx,
            },
        );
    }
    let entry = Entry {
        m: m.clone(),
        req_id: req_id.clone(),
    };
    if !m.send_raw(node, build(req_id)) {
        return Err(FolderError::Away(node.to_string()));
    }
    Ok(Sent {
        entry,
        rx,
        wait,
        node: node.to_string(),
        slug: harness_node::projects::slug(name),
        clone,
    })
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
            Err(e) => Err(clean_text(&e).chars().take(MAX_RAW).collect()),
        };
        let _ = tx.send(answer);
    }
}

/// The node's link closed: its waiting requests end now, a folder request as
/// "away", a clone as "the clone may still finish" ([`Sent::answer`]).
pub fn node_down(m: &Master, node_id: &str) {
    lock(&m.inner.project_folders.pending).retain(|_, x| x.node != node_id);
}
