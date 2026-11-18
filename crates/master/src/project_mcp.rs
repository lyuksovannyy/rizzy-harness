//! A project folder's own MCP servers (ADOPTION_PLAN X.36, PLAN.md T82):
//! the master relays a person's request (`project.mcp`: their user id, the
//! project's folder and what they ask) to the project's node and hands the
//! one `project.mcp_result` that answers it, from that node only, to the
//! waiting request, as `project_folder.rs` does. The node decides
//! everything (its node.toml switch, the folder, the file, the digest);
//! who may ask is the caller's check (`http/projects/mcp.rs`). Nothing a
//! turn, a tool or a model does reaches this: only a person's HTTP call.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use harness_protocol::{
    CAP_PROJECT_MCP, MasterToNode, ProjectMcpList, ProjectMcpRequest, ProjectMcpState,
};
use tokio::sync::oneshot;

use crate::{Master, lock};

/// How long a request waits for its node (the node gives itself 10 s).
const WAIT: Duration = Duration::from_secs(15);
/// Requests waiting for nodes at once, all together, and per account.
const MAX_WAITING: usize = 16;
const PER_USER: usize = 2;
/// Requests one account may send one node per minute.
pub const PER_MINUTE: usize = 30;
/// The longest refusal passed on, in characters.
const MAX_ERROR: usize = 300;

type Answer = Result<ProjectMcpList, String>;

struct Wait {
    node: String,
    user: String,
    tx: oneshot::Sender<Answer>,
}

/// Requests waiting for their node's answer, and each (account, node)'s
/// recent requests.
#[derive(Default)]
pub struct Waiting {
    pending: std::sync::Mutex<HashMap<String, Wait>>,
    recent: std::sync::Mutex<HashMap<(String, String), VecDeque<Instant>>>,
}

/// Why there is no list, each in words for the person.
#[derive(Debug)]
pub enum McpError {
    Away(String),
    NotOffered(String),
    Busy,
    Timeout(String),
    Refused(String),
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpError::Away(n) => write!(f, "node {n} is not connected: start it and try again"),
            McpError::NotOffered(n) => write!(
                f,
                "node {n} does not run a repository's MCP servers: its node.toml has no [mcp] \
                 project_servers = \"ask\" (or it is an older version)"
            ),
            McpError::Busy => write!(f, "too many requests about MCP servers; try again shortly"),
            McpError::Timeout(n) => write!(f, "node {n} did not answer in time; try again"),
            McpError::Refused(why) => write!(f, "{why}"),
        }
    }
}

struct Entry<'a> {
    m: &'a Master,
    req_id: &'a str,
}

impl Drop for Entry<'_> {
    fn drop(&mut self) {
        lock(&self.m.inner.project_mcp.pending).remove(self.req_id);
    }
}

/// Ask `node` about `folder`'s `.mcp.json` for `user`.
pub async fn ask(
    m: &Master,
    user: &str,
    node: &str,
    folder: &str,
    request: ProjectMcpRequest,
) -> Result<ProjectMcpList, McpError> {
    if !m.is_connected(node) {
        return Err(McpError::Away(node.to_string()));
    }
    if !m.node_has(node, CAP_PROJECT_MCP) {
        return Err(McpError::NotOffered(node.to_string()));
    }
    {
        let mut recent = lock(&m.inner.project_mcp.recent);
        let now = Instant::now();
        // Forget accounts that went quiet, so the map stays small.
        if recent.len() > 4096 {
            recent.retain(|_, q| q.back().is_some_and(|t| now - *t < Duration::from_secs(60)));
        }
        let q = recent
            .entry((user.to_string(), node.to_string()))
            .or_default();
        while q
            .front()
            .is_some_and(|t| now - *t >= Duration::from_secs(60))
        {
            q.pop_front();
        }
        if q.len() >= PER_MINUTE {
            return Err(McpError::Busy);
        }
        q.push_back(now);
    }
    let req_id = harness_core::new_id();
    let (tx, rx) = oneshot::channel();
    {
        let mut w = lock(&m.inner.project_mcp.pending);
        if w.len() >= MAX_WAITING || w.values().filter(|x| x.user == user).count() >= PER_USER {
            return Err(McpError::Busy);
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
        MasterToNode::ProjectMcp {
            req_id: req_id.clone(),
            user_id: user.to_string(),
            folder: folder.to_string(),
            request,
        },
    );
    if !sent {
        return Err(McpError::Away(node.to_string()));
    }
    match tokio::time::timeout(WAIT, rx).await {
        Ok(Ok(Ok(l))) => Ok(l),
        Ok(Ok(Err(e))) => Err(McpError::Refused(e)),
        Ok(Err(_)) => Err(McpError::Away(node.to_string())),
        Err(_) => Err(McpError::Timeout(node.to_string())),
    }
}

/// A list as the master passes it on, whatever a node sent: at most 64
/// entries, every string one bounded line (a node's text is shown as text
/// anyway), no state a list cannot have.
fn bounded(mut l: ProjectMcpList) -> ProjectMcpList {
    let line = |s: &str, n: usize| -> String {
        s.chars()
            .map(|c| if c.is_control() { '\u{FFFD}' } else { c })
            .take(n)
            .collect()
    };
    let opt = |s: &Option<String>, n: usize| s.as_deref().map(|s| line(s, n));
    let list = |v: &[String]| -> Vec<String> { v.iter().take(64).map(|a| line(a, 2048)).collect() };
    l.entries.truncate(64);
    for e in &mut l.entries {
        e.name = line(&e.name, 64);
        e.command = line(&e.command, 2048);
        e.args = list(&e.args);
        e.env_keys = list(&e.env_keys);
        e.digest = opt(&e.digest, 64);
        e.reason = opt(&e.reason, MAX_ERROR);
        e.approved_command = opt(&e.approved_command, 2048);
        e.approved_args = e.approved_args.as_deref().map(list);
        e.approved_env_keys = e.approved_env_keys.as_deref().map(list);
        if !matches!(e.state, ProjectMcpState::New | ProjectMcpState::Changed) {
            e.digest = None;
        }
    }
    l.unreadable = opt(&l.unreadable, MAX_ERROR);
    l.note = opt(&l.note, MAX_ERROR);
    l
}

/// A `project.mcp_result` from `node_id`: to the request it answers, if
/// that request went to this node; dropped otherwise.
pub fn on_answer(m: &Master, node_id: &str, req_id: String, answer: Answer) {
    let tx = {
        let mut w = lock(&m.inner.project_mcp.pending);
        match w.get(&req_id) {
            Some(x) if x.node == node_id => w.remove(&req_id).map(|x| x.tx),
            Some(_) => {
                tracing::warn!(node = %node_id, "project MCP answer for another node's request; ignored");
                None
            }
            None => None,
        }
    };
    if let Some(tx) = tx {
        let _ = tx.send(match answer {
            Ok(l) => Ok(bounded(l)),
            Err(e) => Err(e
                .chars()
                .filter(|c| !c.is_control())
                .take(MAX_ERROR)
                .collect()),
        });
    }
}

/// The node's link closed: its waiting requests end now, as "away".
pub fn node_down(m: &Master, node_id: &str) {
    lock(&m.inner.project_mcp.pending).retain(|_, x| x.node != node_id);
}
