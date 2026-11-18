//! Which agents of a workspace may use which skill or MCP server
//! (ADOPTION_PLAN X.34 step A, PLAN.md T80).
//!
//! **What there is to choose from** is what the nodes announce
//! (`node.plugins`, [`node_announced`]): the skills of each node's own
//! skill folders and the names of its node.toml servers. The master keeps
//! the latest list per node, bounded and cleaned
//! ([`harness_protocol::NodePlugins::bounded`]), one per link. A
//! repository's skills are not announced; they are the repository's text.
//!
//! **What a workspace chose** is a row per Assigned item (`plugin_access`
//! in the store): the agents of that workspace's set that may use it. No
//! row is Shared: every agent may. A setting is by name: the same name on
//! two nodes is one item, whatever each node's skill says.
//!
//! **How it is enforced is deny-only** ([`Assignments::deny`]): every spec
//! the master sends for a chat of the workspace (a turn's agent and each of
//! its sub-agents, each by its own id) carries the names it may NOT use,
//! `deny_skills` and `deny_mcp_servers`. Nothing here names a skill to load
//! or a server to start: the node still decides what exists. A node that
//! does not announce [`harness_protocol::CAP_PLUGIN_ACCESS`] would ignore
//! those lists, so [`fail_closed`] withholds the whole `skill` tool or
//! every MCP tool from such an agent instead.

use std::collections::BTreeMap;

use harness_core::agent::AgentSpec;
use harness_protocol::{MasterToNode, NodePlugins};
use sha2::{Digest, Sha256};

use crate::{Master, lock};

/// The two kinds of item, as stored and as the API names them.
pub const SKILL: &str = "skill";
pub const MCP: &str = "mcp";

/// Assigned items one workspace may have.
pub const MAX_ASSIGNED: usize = 200;

/// Agents one item may be assigned to.
pub const MAX_AGENTS: usize = 100;

/// Whether `name` is a valid name of an item of `kind` (the node's own
/// rules: ASCII only, so no look-alike and no hidden character).
pub fn valid(kind: &str, name: &str) -> bool {
    match kind {
        SKILL => harness_core::skills::valid_skill_name(name),
        MCP => harness_core::agent::valid_mcp_server_name(name),
        _ => false,
    }
}

/// One workspace's Assigned items: (kind, name) to the agent ids that may
/// use it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Assignments {
    items: BTreeMap<(String, String), Vec<String>>,
    /// The settings could not be read: nobody knows what is Assigned, so
    /// every agent loses the `skill` tool and every MCP tool for the turn
    /// (fails closed).
    unreadable: bool,
}

impl Assignments {
    /// From the store's rows; unknown kinds and bad names are left out
    /// (the store's CHECK and the API refuse them first).
    pub fn from_rows(rows: Vec<harness_store::plugin_access::PluginAccessRow>) -> Self {
        Self {
            items: rows
                .into_iter()
                .filter(|r| valid(&r.kind, &r.name))
                .map(|r| ((r.kind, r.name), r.agents))
                .collect(),
            unreadable: false,
        }
    }

    /// Settings that could not be read (see `unreadable`).
    pub fn unreadable() -> Self {
        Self {
            unreadable: true,
            ..Self::default()
        }
    }

    /// The agents item (`kind`, `name`) is assigned to; `None`: Shared.
    pub fn assigned(&self, kind: &str, name: &str) -> Option<&[String]> {
        self.items
            .get(&(kind.to_string(), name.to_string()))
            .map(Vec::as_slice)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str, &[String])> {
        self.items
            .iter()
            .map(|((k, n), a)| (k.as_str(), n.as_str(), a.as_slice()))
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && !self.unreadable
    }

    /// Put on `spec` (and on each of its sub-agents, by its own id) the
    /// items of this workspace it may not use: the Assigned ones that do
    /// not name it. A spec's own lists are only added to, never cleared,
    /// and its prompt hash changes with them, so a turn's log says which
    /// lists it ran with.
    pub fn deny(&self, spec: &mut AgentSpec) {
        self.deny_one(spec);
        for d in &mut spec.delegates {
            self.deny_one(d);
        }
    }

    fn deny_one(&self, spec: &mut AgentSpec) {
        if self.unreadable {
            for t in [SKILL_TOOL, harness_core::agent::MCP_ALL] {
                if !spec.deny_tools.iter().any(|d| d == t) {
                    spec.deny_tools.push(t.into());
                }
            }
            return;
        }
        let mut added = false;
        for ((kind, name), agents) in &self.items {
            if agents.contains(&spec.agent_id) {
                continue;
            }
            let list = match kind.as_str() {
                SKILL => &mut spec.deny_skills,
                MCP => &mut spec.deny_mcp_servers,
                _ => continue,
            };
            if !list.contains(name) {
                list.push(name.clone());
                added = true;
            }
        }
        if added {
            let mut h = Sha256::new();
            h.update(spec.prompt_hash.as_bytes());
            for (tag, list) in [
                ("skills", &spec.deny_skills),
                ("mcp", &spec.deny_mcp_servers),
            ] {
                h.update([0u8]);
                h.update(tag.as_bytes());
                for n in list {
                    h.update([0u8]);
                    h.update(n.as_bytes());
                }
            }
            spec.prompt_hash = hex::encode(h.finalize())[..16].to_string();
        }
    }
}

/// For a node that does not enforce the lists (no
/// [`harness_protocol::CAP_PLUGIN_ACCESS`]): every spec in `msg` that
/// denies a skill loses the whole `skill` tool, and one that denies a
/// server every MCP tool, through `deny_tools`, which every node reads. An
/// Assigned item so never reaches another agent there; the agents it is
/// assigned to lose the tool on that node too, until it is updated.
pub fn fail_closed(msg: &mut MasterToNode) {
    let spec = match msg {
        MasterToNode::Create { agent, .. }
        | MasterToNode::Prompt { agent, .. }
        | MasterToNode::Callback { agent, .. }
        | MasterToNode::Retry { agent, .. } => agent.as_mut(),
        MasterToNode::SetAgent { agent, .. } => Some(agent),
        _ => None,
    };
    let Some(spec) = spec else { return };
    withhold_whole(spec);
    for d in &mut spec.delegates {
        withhold_whole(d);
    }
}

fn withhold_whole(spec: &mut AgentSpec) {
    let mut deny = |on: bool, tool: &str| {
        if on && !spec.deny_tools.iter().any(|t| t == tool) {
            spec.deny_tools.push(tool.into());
        }
    };
    deny(!spec.deny_skills.is_empty(), SKILL_TOOL);
    deny(
        !spec.deny_mcp_servers.is_empty(),
        harness_core::agent::MCP_ALL,
    );
}

const SKILL_TOOL: &str = harness_tools::skills::NAME;

/// Take what node `node_id` announced on its current link: the first
/// `node.plugins` of each link, bounded; a later one on the same link is
/// ignored, so a node cannot make the list change faster than it
/// reconnects.
pub(crate) fn node_announced(m: &Master, node_id: &str, plugins: NodePlugins) {
    let plugins = plugins.bounded();
    let mut nodes = lock(&m.inner.nodes);
    let Some(n) = nodes.get_mut(node_id) else {
        return;
    };
    if n.plugins_taken {
        tracing::warn!(node = %node_id, "a second node.plugins on one link; ignored");
        return;
    }
    n.plugins_taken = true;
    n.plugins = plugins;
}

/// One announcing node, as the Plugins screen lists it.
#[derive(Debug, Clone)]
pub struct Announcer {
    pub node_id: String,
    pub hostname: String,
    pub connected: bool,
    /// It enforces `deny_skills` and `deny_mcp_servers`.
    pub enforces: bool,
    pub embedded: bool,
    pub plugins: NodePlugins,
}

impl Master {
    /// Every node's latest announcement (nodes that never sent one have an
    /// empty one), with whether it is connected and enforces the lists.
    pub fn announcers(&self) -> Vec<Announcer> {
        let nodes = lock(&self.inner.nodes);
        let mut v: Vec<Announcer> = nodes
            .values()
            .map(|n| Announcer {
                node_id: n.info.node_id.clone(),
                hostname: n.info.hostname.clone(),
                connected: n.info.connected && n.tx.is_some(),
                enforces: n
                    .capabilities
                    .iter()
                    .any(|c| c == harness_protocol::CAP_PLUGIN_ACCESS),
                embedded: n.info.embedded,
                plugins: n.plugins.clone(),
            })
            .collect();
        v.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        v
    }

    /// Workspace `ws`'s Assigned items, read now. A store error is logged
    /// and fails closed ([`Assignments::unreadable`]).
    pub async fn assignments(&self, ws: &str) -> Assignments {
        let id = ws.to_string();
        match self.inner.mirror.call(move |s| s.plugin_access(&id)).await {
            Ok(rows) => Assignments::from_rows(rows),
            Err(e) => {
                tracing::warn!(error = %e, workspace = %ws, "could not read the Plugins settings");
                Assignments::unreadable()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(id: &str) -> AgentSpec {
        serde_json::from_value(serde_json::json!({
            "agent_id": id, "name": id, "system_prompt": "", "tools": "all", "prompt_hash": "h"
        }))
        .unwrap()
    }

    /// Settings that cannot be read fail closed: every agent loses the
    /// `skill` tool and every MCP tool for the turn, sub-agents too.
    #[test]
    fn unreadable_settings_withhold_the_whole_tools() {
        let mut s = spec("coder");
        s.delegates = vec![spec("scout")];
        Assignments::unreadable().deny(&mut s);
        for a in [&s, &s.delegates[0]] {
            assert!(!a.allows_tool("skill") && !a.allows_mcp(), "{}", a.agent_id);
        }
        let mut none = spec("coder");
        Assignments::default().deny(&mut none);
        assert_eq!(none, spec("coder"), "nothing chosen: nothing changes");
    }

    /// The fallback for a node without the capability touches only specs
    /// with a denial, and keeps their own denials.
    #[test]
    fn fail_closed_adds_only_where_something_is_denied() {
        let mut a = spec("a");
        a.deny_skills = vec!["x".into()];
        a.deny_tools = vec!["remember".into()];
        let mut d = spec("d");
        d.deny_mcp_servers = vec!["m".into()];
        a.delegates = vec![d, spec("e")];
        let mut msg = MasterToNode::SetAgent {
            req_id: "r".into(),
            session_id: harness_core::SessionId::new(),
            agent: a,
        };
        fail_closed(&mut msg);
        let MasterToNode::SetAgent { agent, .. } = msg else {
            unreachable!()
        };
        assert_eq!(agent.deny_tools, ["remember", "skill"]);
        assert_eq!(agent.delegates[0].deny_tools, ["mcp"]);
        assert!(agent.delegates[1].deny_tools.is_empty());
    }
}
