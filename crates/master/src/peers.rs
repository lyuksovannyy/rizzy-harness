//! A person's message that calls several agents by mention (ADOPTION_PLAN
//! X.37, README "Calling several agents"): the master reads the rule
//! (`harness_core::mention`), resolves every agent called in the chat's own
//! workspace set, and sends the node one prompt that carries the first
//! agent as `agent` and the others as `peers`; the node asks them in order.
//!
//! Only a message the chat's owner wrote, signed in as themselves, calls
//! more than the one agent a leading `@agent` always called: the API token's
//! and a routine's (and a webhook's) keep today's single leading mention, so
//! text nobody typed here cannot start several turns. An answer to a card,
//! a message sent with the agent picker's override, a Retry, a sub-agent's
//! chat and parallel answers call nobody by the rule.

use harness_core::mention::{MAX_CALLED, Who, scan};

use crate::resolver::Resolver;
use crate::{CommandError, Master};

/// The sentence for a message that calls too many agents.
pub fn too_many() -> CommandError {
    CommandError::Rejected(format!(
        "One message can call at most {MAX_CALLED} agents; this one names more. Mention fewer \
         of them (an @name in the middle of a sentence calls nobody)."
    ))
}

/// The sentence for a node that cannot ask several agents in turn.
pub const OLD_NODE: &str = "This node is too old to ask several agents in one message; update \
    it, or call one agent per message";

impl Master {
    /// The ids of the agents `text` calls in `row`'s chat, in the order
    /// they answer (at most [`MAX_CALLED`] + 1: more is the caller's
    /// refusal). `r` is the chat's own resolver (its workspace's agents);
    /// the chat owner's own login is the person, never resolved to anyone
    /// else's. Empty when the text has no `@`.
    pub(crate) async fn called_agents(
        &self,
        row: &harness_store::SessionRow,
        r: &Resolver,
        text: &str,
    ) -> Vec<String> {
        if !text.contains('@') {
            return Vec::new();
        }
        let login = match row.owner_id.clone() {
            Some(id) => self
                .inner
                .mirror
                .call(move |s| s.user_by_id(&id))
                .await
                .ok()
                .flatten()
                .map(|u| u.username),
            None => None,
        };
        scan(text, &|name| match r.top_level(name) {
            Ok(a) => Some(Who::Agent(a.id.clone())),
            Err(_) => (login.as_deref() == Some(name)).then_some(Who::Person),
        })
        .called
    }
}

/// A peer's spec as its turn runs it: what a turn nobody sent from the
/// master's side has (as a callback's): no recall, no board, no `remember`,
/// no connector. Its own definition otherwise (model, tools, switches).
pub fn for_peer(spec: &mut harness_core::agent::AgentSpec) {
    crate::deny_remember(spec);
    crate::recall::deny(spec);
    crate::board::deny(spec);
    spec.connector_tools.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::SessionId;
    use harness_core::agent::{AgentSpec, ToolSelection};
    use harness_protocol::MasterToNode;

    fn spec(id: &str, prompt: usize) -> AgentSpec {
        AgentSpec {
            agent_id: id.into(),
            name: id.into(),
            system_prompt: "x".repeat(prompt),
            tools: ToolSelection::List(vec![]),
            model: None,
            fallbacks: vec![],
            unavailable: vec![],
            no_effort: vec![],
            vision: vec![],
            context_windows: Default::default(),
            effort: None,
            ui: Default::default(),
            prompt_hash: "h".into(),
            deny_tools: vec![],
            deny_skills: vec![],
            deny_mcp_servers: vec![],
            stored_skills: vec![],
            connector_tools: vec![],
            delegates: vec![],
            caps: None,
            no_personalisation: false,
        }
    }

    fn prompt(peers: Vec<AgentSpec>) -> MasterToNode {
        MasterToNode::Prompt {
            req_id: "r".into(),
            session_id: SessionId::new(),
            text: "hi".into(),
            agent: Some(spec("a", 20_000)),
            attachments: vec![],
            compare: vec![],
            savings: None,
            context: None,
            reply: None,
            images: vec![],
            agent_named: true,
            files: vec![],
            owner: None,
            peers,
        }
    }

    /// The link's guard, with four specs: an agent may be at most 64 KiB
    /// stored (its prompt 20,000 characters), so four of them and their
    /// delegates are far below it; the guard still refuses, before
    /// anything is sent, a command whose specs together pass it.
    #[test]
    fn four_specs_fit_the_link_and_oversize_ones_are_refused() {
        let at_caps = prompt((0..3).map(|i| spec(&format!("p{i}"), 64 * 1024)).collect());
        assert!(crate::too_large_for_link(&at_caps).is_none());
        let huge = prompt((0..3).map(|i| spec(&format!("p{i}"), 2_500_000)).collect());
        let e = crate::too_large_for_link(&huge).expect("refused");
        assert!(e.to_string().contains("too large"), "{e}");
    }

    /// The pure part of what a peer's turn is given.
    #[test]
    fn a_peers_spec_has_no_memory_recall_board_or_connector() {
        let mut p = spec("p", 10);
        p.connector_tools = vec![harness_core::connectors::ConnectorToolEntry {
            connector: "c".into(),
            name: "mcp__c__t".into(),
            tool: "t".into(),
            description: String::new(),
            schema: "{}".into(),
        }];
        for_peer(&mut p);
        assert!(p.connector_tools.is_empty());
        for t in ["remember", "recall"] {
            assert!(p.deny_tools.iter().any(|d| d == t), "{t}");
        }
    }
}
