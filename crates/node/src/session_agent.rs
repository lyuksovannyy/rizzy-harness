//! A chat's agent switch (README "Agents", PLAN.md "Agents"). The agent a
//! chat runs is the master's state of the chat (its row), sent with every
//! message; picking one writes nothing here. The node refuses a pick while
//! the chat works, and records a switch only when the person's next turn
//! under the chat's own agent starts on another agent or model than the
//! chat's last recorded one: `agent_changed`, right before that turn's
//! `turn_started`, so the notice sits above the message that caused it.

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

    /// The `agent_changed` a person's turn under the chat's own `agent`
    /// writes first, read from the log the turn joins (`history`): when
    /// its agent or model differs from the last recorded one (the latest
    /// `agent_changed`, else `session_created`). The model compared is the
    /// one asked for, never a fallback that answered. A chat that recorded
    /// no agent (made on a node directly, never by the master) gets none.
    pub(super) fn switch_of(
        &self,
        history: &[RawEvent],
        agent: Option<&AgentSpec>,
    ) -> Option<AgentChanged> {
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
