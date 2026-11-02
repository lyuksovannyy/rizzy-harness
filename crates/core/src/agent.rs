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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSpec {
    pub agent_id: String,
    pub name: String,
    pub system_prompt: String,
    pub tools: ToolSelection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// `low` | `medium` | `high` | `max`; only applied if the model supports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default)]
    pub budget: Budget,
    /// Hash of everything above that changes behaviour.
    pub prompt_hash: String,
}

impl AgentSpec {
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
}
