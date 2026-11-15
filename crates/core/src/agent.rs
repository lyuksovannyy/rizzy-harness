//! What a node needs to run a turn as a given agent. The master owns agent
//! definitions and sends this spec with each command, so nodes stay stateless
//! about agents. Node policy still filters the tools (agents request, nodes decide).

use serde::{Deserialize, Serialize};

use crate::event::AgentRef;

/// Every MCP tool's name starts with this (`harness_tools::mcp::PREFIX`).
pub const MCP_PREFIX: &str = "mcp__";

/// The `tools` entry that stands for every MCP tool.
pub const MCP_ALL: &str = "mcp";

/// **The tools that cannot change anything** (ADOPTION_PLAN T.7, PLAN.md
/// T70): not a file, not a process outside their own call, not a setting,
/// nothing on another machine through the node. The one allow-list every
/// "is this agent read-only" question reads ([`AgentSpec::changes_things`],
/// [`tool_changes`]): the master's small-model setting, the node's
/// sub-agent folders, plan mode, the parallel batch and the repeated-call
/// guard. A tool added later changes things until someone puts it here;
/// an MCP tool is never here (its effects are its server's word).
///
/// - `read`, `grep`, `glob`, `outline`, `graph_query`: read files and the
///   repository's map.
/// - `tool_output`: reads a command's kept output; `bash_output` reads, and
///   `bash_stop` stops, a background command of the session's own.
/// - `todo`: writes only the chat's own task list (its own log).
/// - `skill`: loads a skill's instructions.
/// - `recall`: searches the person's earlier chats and memory.
/// - `web_search`, `web_fetch`: a search and a page read through the
///   node's web tools (they send a query or a URL out: read-only is not
///   secrecy, as with the read-only shell's network).
/// - `remember`: changes nothing itself; the master files the proposal, and
///   a sub-agent's call always waits for a person's Keep
///   (`master/src/memory.rs`, `saves_without_a_click`). Plan mode refuses
///   it all the same ([`crate::policy::PLAN_REFUSED`]): it promises that
///   nothing changes, memory included.
///
/// `bash` is not here: it changes things in a writable shell. An agent
/// whose other tools are all here gets the read-only shell instead
/// ([`AgentSpec::changes_things`]).
pub const READ_ONLY_TOOLS: &[&str] = &[
    "read",
    "grep",
    "glob",
    "outline",
    "graph_query",
    "tool_output",
    "bash_output",
    "bash_stop",
    "todo",
    "skill",
    "recall",
    "web_search",
    "web_fetch",
    "remember",
];

/// The shell: the one tool that changes things or not by where it runs.
pub const SHELL: &str = "bash";

/// Whether a call of `tool` can change anything: every tool not on
/// [`READ_ONLY_TOOLS`] (MCP tools always), and [`SHELL`] unless this call
/// runs in the read-only shell (`read_only_shell`, the node's choice).
pub fn tool_changes(tool: &str, read_only_shell: bool) -> bool {
    if tool == SHELL {
        return !read_only_shell;
    }
    tool.starts_with(MCP_PREFIX) || !READ_ONLY_TOOLS.contains(&tool)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolSelection {
    /// The literal string `"all"`: every tool the runtime offers.
    All(AllTools),
    List(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllTools {
    All,
}

impl ToolSelection {
    pub fn allows(&self, tool: &str) -> bool {
        match self {
            ToolSelection::All(_) => true,
            ToolSelection::List(l) => l.iter().any(|t| t == tool),
        }
    }
}

/// Which `ui:` blocks an agent's replies may use (`ui:` in its frontmatter).
/// The agent loop enforces it when it records a message; the prompt only
/// tells the model what is available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiLevel {
    /// Every block: questions and forms as well as progress and charts.
    #[default]
    Rich,
    /// Progress and charts only; a question or form shows as an error.
    Display,
    /// No blocks: `ui:` fences stay code.
    Off,
}

impl UiLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            UiLevel::Rich => "rich",
            UiLevel::Display => "display",
            UiLevel::Off => "off",
        }
    }

    /// The lower of the two levels (`Off` < `Display` < `Rich`).
    pub fn at_most(self, cap: UiLevel) -> UiLevel {
        let rank = |l: UiLevel| match l {
            UiLevel::Off => 0,
            UiLevel::Display => 1,
            UiLevel::Rich => 2,
        };
        if rank(self) <= rank(cap) { self } else { cap }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSpec {
    pub agent_id: String,
    pub name: String,
    pub system_prompt: String,
    pub tools: ToolSelection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Models to try, in order, when the turn's model is unavailable or
    /// fails before any output (prefixed ids). The master leaves out the
    /// unavailable ones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallbacks: Vec<String>,
    /// Models the master knows are unavailable (missing from their
    /// provider's latest listing): a turn on one of these starts with the
    /// fallbacks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unavailable: Vec<String>,
    /// Models of the chain (the turn's own and the fallbacks) that take no
    /// reasoning effort: a request to one of these goes without `effort`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub no_effort: Vec<String>,
    /// Context windows (tokens) of the chain's models, where the master
    /// knows them from the provider's listing or the admin. Compaction is
    /// keyed to these; a model missing here falls back to the configured
    /// trigger.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub context_windows: std::collections::BTreeMap<String, u64>,
    /// `low` | `medium` | `high` | `max`; only applied if the model supports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default)]
    pub ui: UiLevel,
    /// Hash of everything above that changes behaviour.
    pub prompt_hash: String,
    /// Tools withheld for this turn whatever `tools` says (e.g. `remember`
    /// in an incognito chat). Set by the master per session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny_tools: Vec<String>,
    /// Agents this one may hand sub-tasks to (`delegate` tool). Resolved by
    /// the master; their own `delegates` are empty, so depth is one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub delegates: Vec<AgentSpec>,
    /// Limits on delegation per turn, shown to the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caps: Option<DelegateCaps>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegateCaps {
    /// Sub-agent runs per turn.
    pub max_subagents: u32,
    /// Tokens (input + output) all sub-agents of one turn may use together.
    pub max_tokens: u64,
}

impl Default for DelegateCaps {
    fn default() -> Self {
        Self {
            max_subagents: 4,
            max_tokens: 200_000,
        }
    }
}

impl AgentSpec {
    /// Whether this agent may use `tool`. An entry `mcp` in `tools` (or in
    /// `deny_tools`) stands for every MCP tool (`mcp__<server>__<tool>`,
    /// whose names come from the node's servers and cannot be listed ahead).
    pub fn allows_tool(&self, tool: &str) -> bool {
        let mcp = tool.starts_with(MCP_PREFIX);
        (self.tools.allows(tool) || (mcp && self.tools.allows(MCP_ALL)))
            && !self
                .deny_tools
                .iter()
                .any(|t| t == tool || (mcp && t == MCP_ALL))
    }

    /// Whether this agent may use any MCP tool: the node starts a chat's
    /// servers only for a turn whose agent may.
    pub fn allows_mcp(&self) -> bool {
        let listed = match &self.tools {
            ToolSelection::All(_) => true,
            ToolSelection::List(l) => l.iter().any(|t| t == MCP_ALL || t.starts_with(MCP_PREFIX)),
        };
        listed && !self.deny_tools.iter().any(|t| t == MCP_ALL)
    }

    /// **Whether this agent can change things here** (PLAN.md T70): it may
    /// use every tool (`all`), an MCP tool, or any tool that is not on
    /// [`READ_ONLY_TOOLS`], counting only the tools it may call (its own
    /// list, less its denials) that `offered` keeps (the node's
    /// `denied_tools`; the master passes `|_| true`). Its shell does not
    /// count: an agent that changes nothing else runs `bash` in the
    /// read-only shell, which the node picks from this same answer, so a
    /// spec cannot ask for a writable shell for it. Read-only agents work
    /// in their parent's folder (no copy to merge back), may run on the
    /// small model and may be started in plan mode.
    pub fn changes_things(&self, offered: impl Fn(&str) -> bool) -> bool {
        let ToolSelection::List(tools) = &self.tools else {
            return true;
        };
        tools
            .iter()
            .filter(|t| self.allows_tool(t) && offered(t))
            .any(|t| t == MCP_ALL || tool_changes(t, true))
    }

    pub fn agent_ref(&self) -> AgentRef {
        AgentRef {
            agent_id: self.agent_id.clone(),
            name: self.name.clone(),
            prompt_hash: self.prompt_hash.clone(),
            withheld: self.deny_tools.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_selection_serde() {
        let all: ToolSelection = serde_json::from_str("\"all\"").unwrap();
        assert!(all.allows("bash"));
        let some: ToolSelection = serde_json::from_str("[\"read\"]").unwrap();
        assert!(some.allows("read") && !some.allows("bash"));
        assert_eq!(serde_json::to_string(&all).unwrap(), "\"all\"");
    }

    #[test]
    fn mcp_stands_for_every_mcp_tool() {
        let spec = |tools: ToolSelection, deny: &[&str]| {
            AgentSpec {
            tools,
            deny_tools: deny.iter().map(|d| d.to_string()).collect(),
            ..serde_json::from_value(serde_json::json!({
                "agent_id": "a", "name": "A", "system_prompt": "", "tools": "all", "prompt_hash": "h"
            }))
            .unwrap()
        }
        };
        let list = |l: &[&str]| ToolSelection::List(l.iter().map(|t| t.to_string()).collect());
        let a = spec(list(&["bash", "mcp"]), &[]);
        assert!(a.allows_tool("mcp__github__create_issue") && a.allows_mcp());
        assert!(!a.allows_tool("read"));
        let b = spec(list(&["bash"]), &[]);
        assert!(!b.allows_tool("mcp__github__create_issue") && !b.allows_mcp());
        let c = spec(list(&["mcp__github__list"]), &[]);
        assert!(c.allows_tool("mcp__github__list") && !c.allows_tool("mcp__github__drop"));
        assert!(c.allows_mcp());
        let d = spec(ToolSelection::All(AllTools::All), &["mcp"]);
        assert!(!d.allows_tool("mcp__x__y") && !d.allows_mcp() && d.allows_tool("bash"));
        // `mcp` is not a tool of its own name only.
        assert!(!spec(list(&["mcp"]), &[]).allows_tool("mcpx"));
    }

    fn listed(tools: &[&str], deny: &[&str]) -> AgentSpec {
        AgentSpec {
            tools: ToolSelection::List(tools.iter().map(|t| t.to_string()).collect()),
            deny_tools: deny.iter().map(|d| d.to_string()).collect(),
            ..serde_json::from_value(serde_json::json!({
                "agent_id": "a", "name": "A", "system_prompt": "", "tools": "all", "prompt_hash": "h"
            }))
            .unwrap()
        }
    }

    /// The shared rule's truth table (PLAN.md T70): odd agents a person can
    /// write, judged on the master (everything offered) and on a node.
    #[test]
    fn an_agent_is_read_only_only_when_every_tool_it_may_call_is_listed() {
        let all = |_: &str| true;
        for (tools, changes) in [
            (&[][..], false),
            (&["read"][..], false),
            (
                &["read", "grep", "glob", "outline", "graph_query", "skill"],
                false,
            ),
            (&["read", "remember"], false),
            (&["read", "recall", "todo", "tool_output"], false),
            (&["web_search", "web_fetch", "remember"], false),
            // The shell: read-only for such an agent, so no writer.
            (&["read", "bash"], false),
            (&["read", "bash", "bash_output", "bash_stop"], false),
            (&["bash"], false),
            (&["read", "publish"], true),
            (&["read", "write"], true),
            (&["read", "edit", "bash"], true),
            (&["read", "mcp"], true),
            (&["read", "mcp__github__list_issues"], true),
            (&["read", "bash", "mcp__x__y"], true),
            // Not listed yet, so it changes things.
            (&["read", "notebook_edit"], true),
            (&["read", "delegate"], true),
        ] {
            assert_eq!(listed(tools, &[]).changes_things(all), changes, "{tools:?}");
        }
        let every = AgentSpec {
            tools: ToolSelection::All(AllTools::All),
            ..listed(&[], &[])
        };
        assert!(every.changes_things(all), "all tools");
        assert!(
            every.changes_things(|t| t == "read"),
            "all tools, whatever is offered"
        );
        // The agent's own denials and the node's narrow it, never widen it.
        assert!(!listed(&["read", "write"], &["write"]).changes_things(all));
        assert!(!listed(&["read", "mcp"], &["mcp"]).changes_things(all));
        assert!(!listed(&["read", "write", "bash"], &[]).changes_things(|t| t != "write"));
        assert!(listed(&["read", "write"], &[]).changes_things(|t| t != "bash"));
    }

    #[test]
    fn a_call_changes_things_unless_listed_or_in_the_read_only_shell() {
        for t in READ_ONLY_TOOLS {
            assert!(!tool_changes(t, false), "{t}");
        }
        for t in [
            "write",
            "edit",
            "publish",
            "delegate",
            "mcp",
            "mcp__s__read",
            "anything",
        ] {
            assert!(tool_changes(t, false) && tool_changes(t, true), "{t}");
        }
        assert!(tool_changes(SHELL, false));
        assert!(!tool_changes(SHELL, true));
        // An MCP tool named like a listed one is still an MCP tool.
        assert!(tool_changes("mcp__fs__read", true));
    }

    #[test]
    fn ui_level_caps() {
        use UiLevel::*;
        assert_eq!(Rich.at_most(Display), Display);
        assert_eq!(Display.at_most(Rich), Display);
        assert_eq!(Off.at_most(Display), Off);
        assert_eq!(serde_json::to_string(&Display).unwrap(), "\"display\"");
        assert_eq!(UiLevel::default(), Rich);
    }
}
