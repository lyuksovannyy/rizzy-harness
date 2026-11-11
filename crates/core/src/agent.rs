//! What a node needs to run a turn as a given agent. The master owns agent
//! definitions and sends this spec with each command, so nodes stay stateless
//! about agents. Node policy still filters the tools (agents request, nodes decide).

use serde::{Deserialize, Serialize};

use crate::event::AgentRef;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Budget {
    /// Short prompts, fewer steps: chat-style and throwaway work.
    #[default]
    Lean,
    Standard,
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
    pub budget: Budget,
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
    pub fn allows_tool(&self, tool: &str) -> bool {
        self.tools.allows(tool) && !self.deny_tools.iter().any(|t| t == tool)
    }

    pub fn agent_ref(&self) -> AgentRef {
        AgentRef {
            agent_id: self.agent_id.clone(),
            name: self.name.clone(),
            prompt_hash: self.prompt_hash.clone(),
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
    fn ui_level_caps() {
        use UiLevel::*;
        assert_eq!(Rich.at_most(Display), Display);
        assert_eq!(Display.at_most(Rich), Display);
        assert_eq!(Off.at_most(Display), Off);
        assert_eq!(serde_json::to_string(&Display).unwrap(), "\"display\"");
        assert_eq!(UiLevel::default(), Rich);
    }
}
