//! Changes made on the Agents page (docs/agent-pages.md): a summary and
//! additional instructions for built-in and file agents, whose definitions
//! stay as they are, and the agents made there. The master stores them;
//! [`AgentSet::with_edits`] applies them over the loaded definitions.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Agent, AgentError, AgentScope, AgentSet, Features};

/// The heading that introduces additional instructions after a prompt.
pub const ADDITIONAL: &str = "# Additional instructions";
/// The same for a workspace's own (X.33), after the server's.
pub const ADDITIONAL_WORKSPACE: &str = "# Additional instructions for this workspace";

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
    /// More `@handles`; optional, so agents saved before them still read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// The UI's grouping; none is [`crate::DEFAULT_CATEGORY`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

/// Every edit of one scope (the server, or one workspace), by agent id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Edits {
    pub overlays: BTreeMap<String, Overlay>,
    pub custom: BTreeMap<String, Custom>,
}

impl Agent {
    /// Apply an overlay. The prompt hash changes with the instructions, so
    /// turns stay explainable after an edit.
    pub fn apply(&mut self, o: &Overlay) {
        self.apply_under(o, ADDITIONAL);
    }

    /// Apply an overlay whose instructions go under `heading`.
    fn apply_under(&mut self, o: &Overlay, heading: &str) {
        let summary = o.summary.trim();
        if !summary.is_empty() {
            self.description = summary.to_string();
        }
        let extra = o.instructions.trim();
        if extra.is_empty() {
            return;
        }
        self.spec.system_prompt = format!("{}\n\n{heading}\n\n{extra}", self.spec.system_prompt);
        let mut h = Sha256::new();
        h.update(self.spec.prompt_hash.as_bytes());
        h.update([0u8]);
        if heading != ADDITIONAL {
            h.update(heading.as_bytes());
            h.update([0u8]);
        }
        h.update(extra.as_bytes());
        self.spec.prompt_hash = hex::encode(h.finalize())[..16].to_string();
    }
}

impl AgentSet {
    /// This set with the server's `edits` applied (see [`Self::with_layers`]).
    pub fn with_edits(&self, edits: &Edits) -> (AgentSet, Vec<AgentError>) {
        self.with_layers(&[edits])
    }

    /// This set with `layers` of edits applied in order: the server's
    /// first, then at most one workspace's (X.33). Overlays go on the
    /// agents they name that a wider layer made (a workspace may add
    /// instructions to a built-in, a file or a server agent, under its own
    /// heading after the server's), then the custom agents (each layer's by
    /// name, the server's first). A custom agent whose id an agent of a
    /// wider layer has, or that does not build, is skipped and reported;
    /// one of its aliases another agent has is dropped and reported (the
    /// earlier claim wins). An agent not available as a sub-agent leaves
    /// every `delegates`. **Delegates come from the agent's own layer or a
    /// wider one**: a built-in, a file or a server agent never gets a
    /// workspace's agent as a sub-agent, even one with a name it lists, and
    /// a custom agent that delegates may call all the sub-agents it can
    /// see from its own layer.
    pub fn with_layers(&self, layers: &[&Edits]) -> (AgentSet, Vec<AgentError>) {
        let mut agents = self.agents.clone();
        let mut errors = Vec::new();
        // (layer, id, definition)
        let mut customs: Vec<(usize, &String, &Custom)> = Vec::new();
        for (i, l) in layers.iter().enumerate() {
            for (id, c) in &l.custom {
                if agents.iter().any(|a| &a.id == id) || customs.iter().any(|(_, x, _)| *x == id) {
                    errors.push(AgentError::Shadowed(id.clone()));
                } else {
                    customs.push((i, id, c));
                }
            }
        }
        let base_subs: Vec<String> = agents
            .iter()
            .filter(|a| a.features.as_subagent)
            .map(|a| a.id.clone())
            .collect();
        // The sub-agents an agent of layer `upto` may see.
        let subagents = |upto: usize| -> Vec<String> {
            base_subs
                .iter()
                .cloned()
                .chain(
                    customs
                        .iter()
                        .filter(|(l, _, c)| *l <= upto && c.features.as_subagent)
                        .map(|(_, id, _)| (*id).clone()),
                )
                .collect()
        };
        let wide = subagents(0);
        for a in &mut agents {
            a.delegates.retain(|d| wide.contains(d));
        }
        // (layer of the agent, agent); the definitions count as layer 0.
        let mut made: Vec<(usize, Agent)> = Vec::new();
        for (layer, id, c) in &customs {
            let delegates = subagents(*layer).into_iter().filter(|d| d != *id).collect();
            let built = crate::custom(id, &c.name, &c.summary, &c.prompt, c.features, delegates)
                .and_then(|mut a| {
                    (a.aliases, a.category) = crate::labels(id, &c.aliases, c.category.as_deref())?;
                    if *layer > 0 {
                        a.scope = AgentScope::Workspace;
                    }
                    Ok(a)
                });
            match built {
                Ok(a) => made.push((*layer, a)),
                Err(e) => errors.push(e),
            }
        }
        made.sort_by_key(|(l, a)| (*l, a.name.to_lowercase()));
        let mut levels: Vec<Option<usize>> = vec![None; agents.len()];
        for (l, a) in made {
            levels.push(Some(l));
            agents.push(a);
        }
        // Overlay `i` goes on the definitions and on custom agents of a
        // wider layer, never on its own layer's.
        for (a, made_in) in agents.iter_mut().zip(&levels) {
            for (i, l) in layers.iter().enumerate() {
                if made_in.is_some_and(|m| m >= i) {
                    continue;
                }
                if let Some(o) = l.overlays.get(&a.id) {
                    a.apply_under(
                        o,
                        if i == 0 {
                            ADDITIONAL
                        } else {
                            ADDITIONAL_WORKSPACE
                        },
                    );
                }
            }
        }
        errors.extend(crate::settle(&mut agents));
        (AgentSet::from_agents(agents, &self.default_id), errors)
    }
}
