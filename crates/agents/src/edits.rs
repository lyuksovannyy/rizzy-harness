//! Changes made on the Agents page (docs/agent-pages.md): a summary and
//! additional instructions for built-in and file agents, whose definitions
//! stay as they are, and the agents made there. The master stores them;
//! [`AgentSet::with_edits`] applies them over the loaded definitions.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Agent, AgentError, AgentSet, Features};

/// The heading that introduces additional instructions after a prompt.
pub const ADDITIONAL: &str = "# Additional instructions";

/// What may change on a built-in or file agent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Overlay {
    /// Replaces the definition's description; empty keeps it.
    pub summary: String,
    /// Follows the definition's prompt, under [`ADDITIONAL`].
    pub instructions: String,
}

/// An agent made on the Agents page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Custom {
    pub name: String,
    #[serde(default)]
    pub summary: String,
    pub prompt: String,
    pub features: Features,
}

/// Every edit of one scope (today the whole server), by agent id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Edits {
    pub overlays: BTreeMap<String, Overlay>,
    pub custom: BTreeMap<String, Custom>,
}

impl Agent {
    /// Apply an overlay. The prompt hash changes with the instructions, so
    /// turns stay explainable after an edit.
    pub fn apply(&mut self, o: &Overlay) {
        let summary = o.summary.trim();
        if !summary.is_empty() {
            self.description = summary.to_string();
        }
        let extra = o.instructions.trim();
        if extra.is_empty() {
            return;
        }
        self.spec.system_prompt = format!("{}\n\n{ADDITIONAL}\n\n{extra}", self.spec.system_prompt);
        let mut h = Sha256::new();
        h.update(self.spec.prompt_hash.as_bytes());
        h.update([0u8]);
        h.update(extra.as_bytes());
        self.spec.prompt_hash = hex::encode(h.finalize())[..16].to_string();
    }
}

impl AgentSet {
    /// This set with `edits` applied: overlays on the agents they name,
    /// then the custom agents (by name). An agent not available as a
    /// sub-agent leaves every `delegates`; a custom agent that delegates
    /// may call all the others that are. A custom agent whose id a built-in
    /// or file agent has, or that does not build, is skipped and reported.
    pub fn with_edits(&self, edits: &Edits) -> (AgentSet, Vec<AgentError>) {
        let mut agents = self.agents.clone();
        for a in &mut agents {
            if let Some(o) = edits.overlays.get(&a.id) {
                a.apply(o);
            }
        }
        let mut errors = Vec::new();
        let mut customs = Vec::new();
        for (id, c) in &edits.custom {
            if agents.iter().any(|a| &a.id == id) {
                errors.push(AgentError::Shadowed(id.clone()));
            } else {
                customs.push((id, c));
            }
        }
        let subagents: Vec<String> = agents
            .iter()
            .filter(|a| a.features.as_subagent)
            .map(|a| a.id.clone())
            .chain(
                customs
                    .iter()
                    .filter(|(_, c)| c.features.as_subagent)
                    .map(|(id, _)| (*id).clone()),
            )
            .collect();
        for a in &mut agents {
            a.delegates.retain(|d| subagents.contains(d));
        }
        let mut made = Vec::new();
        for (id, c) in customs {
            let delegates = subagents.iter().filter(|d| *d != id).cloned().collect();
            match crate::custom(id, &c.name, &c.summary, &c.prompt, c.features, delegates) {
                Ok(a) => made.push(a),
                Err(e) => errors.push(e),
            }
        }
        made.sort_by_key(|a| a.name.to_lowercase());
        agents.extend(made);
        (AgentSet::from_agents(agents, &self.default_id), errors)
    }
}
