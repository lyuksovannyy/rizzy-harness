//! A chat's agent switch (README "Agents", PLAN.md "Agents"). The agent a
//! chat runs is the master's state of the chat (its row), sent with every
//! message; picking one writes nothing here. The node refuses a pick while
//! the chat works, and records a switch when the next turn under the
//! chat's own agent (a message, a Retry, a callback turn: the first that
//! runs under the pick) starts on another agent or model than the chat's
//! last recorded one: `agent_changed`, right before that turn's
//! `turn_started`, so the notice sits above it.

use harness_core::RawEvent;
use harness_core::agent::AgentSpec;
use harness_core::event::{AgentChanged, EventKind};
use harness_protocol::AGENT_WORKING;

use super::Session;

impl Session {
    /// The person picked another agent for this chat: allowed only while
    /// it does not work, and recorded nowhere (the master keeps the pick).
    /// "Works": the turn slot is held (a person's turn, one being stopped
    /// until its end is written, a callback turn, a retry, a turn reading
    /// queued messages) or a `/compact` runs. A background command alone
    /// is no work: its callback turn takes the agent the chat has then.
    pub(crate) fn set_agent(&self) -> Result<(), String> {
        if self.parent_session.is_some() {
            return Err("a sub-agent's chat runs the agent its parent named".into());
        }
        let turn = self
            .turn
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some();
        let compacting = self
            .compacting
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some();
        if turn || compacting {
            return Err(AGENT_WORKING.into());
        }
        Ok(())
    }

    /// The `agent_changed` a turn under `agent` writes first, read from
    /// the log the turn joins (`history`). The one rule, for every turn:
    /// only a turn under the chat's own agent records a switch, so none
    /// when the agent was `named` for this turn (an @mention, a routine's
    /// explicit agent, the asker an answer goes to, a Retry of such a
    /// message, the agent a queued message's turn inherits) or the chat is
    /// a sub-agent's; and only when its agent or model differs from the
    /// last recorded one (the latest `agent_changed`, else
    /// `session_created`). The model compared is the one asked for, never a
    /// fallback that answered. A chat that recorded no agent (made on a
    /// node directly, never by the master) gets none.
    pub(super) fn switch_of(
        &self,
        history: &[RawEvent],
        agent: Option<&AgentSpec>,
        named: bool,
    ) -> Option<AgentChanged> {
        if named || self.parent_session.is_some() {
            return None;
        }
        let agent = agent?;
        let model = agent.model.clone().unwrap_or_else(|| self.model.clone());
        let (was, was_model) = history.iter().rev().find_map(|e| {
            match e.kind.as_str() {
                "agent_changed" | "session_created" => {}
                _ => return None,
            }
            match e.typed() {
                Ok(EventKind::AgentChanged(a)) => Some((Some(a.agent.agent_id), a.model)),
                Ok(EventKind::SessionCreated(c)) => Some((c.agent_id, Some(c.model))),
                _ => None,
            }
        })?;
        let was = was?;
        if was == agent.agent_id && was_model.is_none_or(|m| m == model) {
            return None;
        }
        Some(AgentChanged {
            agent: agent.agent_ref(),
            model: Some(model),
        })
    }
}
