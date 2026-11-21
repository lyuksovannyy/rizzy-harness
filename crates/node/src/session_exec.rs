//! The owner's switch of a chat's execution mode (N.10b, README "Execution
//! modes", docs/execution-modes.md, PLAN.md T39): `sandbox` to `host` or
//! back, for an existing agentic chat, only while nothing of the chat runs.
//! The node decides, whatever the master checked first, under the turn
//! slot (the rewind's rule): the chat is a person's agentic chat (not a
//! sub-agent's, a routine's or a chat without tools); this node offers the
//! mode (`ExecOffers::choose`, as at create: `host = true`, the sandbox
//! rules of `[execution]` and `[sandbox] mode = "required"`); no turn runs
//! or is being stopped, no `/compact`, no background command or
//! sub-agent runs or has an end not recorded, no approval waits.
//!
//! What it does, in this order, all before the ack:
//!
//! 1. stops the chat's MCP servers and waits until they are gone (they
//!    were started under the old confinement; the next turn starts them
//!    under the new one). A server that does not die in time refuses the
//!    switch, with nothing recorded;
//! 2. drops every chat grant (`chat_allow_removed` each): a call allowed
//!    in one mode must be asked again in the other, and when the chat
//!    returns, too (the person decided about that shell, not this one);
//! 3. records `exec_mode_changed`, then applies it.
//!
//! The order is the safe one at every crash: a restart between 2 and 3
//! leaves the old mode with fewer grants. Nothing a model writes reaches
//! this: only `session.set_exec_mode`, which the master sends for the
//! chat's owner.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use harness_core::event::{EventKind, ExecModeChanged, Profile};
use harness_core::policy::ExecMode;

use super::Session;

/// How long a stopped MCP server may take to be gone before the switch is
/// refused.
pub(crate) const MCP_STOP_WAIT: Duration = Duration::from_secs(10);

/// Why a switch is refused (the master shows these to the owner).
pub(crate) const EXEC_CHAT: &str =
    "a chat without tools runs no commands and has no execution mode";
pub(crate) const EXEC_SUB_AGENT: &str =
    "a sub-agent runs in the execution mode of the chat that started it; switch that chat";
pub(crate) const EXEC_ROUTINE: &str =
    "a routine's chat always runs in the sandbox; the mode is not switchable";
pub(crate) const EXEC_UNREADABLE: &str = "this chat's execution mode is not known here (its log \
     names none this node reads), so it runs nothing and cannot be switched: start a new chat";

impl Session {
    /// Switch this chat's commands to `to` (`session.set_exec_mode`).
    /// Acked once `exec_mode_changed` is written and everything built under
    /// the old mode is dropped. `Ok` also when the chat is in `to` already
    /// (nothing recorded). Refusals say what to stop.
    pub(crate) async fn set_exec_mode(self: &Arc<Self>, to: ExecMode) -> Result<(), String> {
        if self.profile != Profile::Agentic {
            return Err(EXEC_CHAT.into());
        }
        if self.parent_session.is_some() {
            return Err(EXEC_SUB_AGENT.into());
        }
        if self.no_callbacks {
            return Err(EXEC_ROUTINE.into());
        }
        // The same slot as a rewind: a message, a callback, a retry, a
        // `/compact` or an undo of files cannot start meanwhile, and none
        // is running.
        let queue = self.queue.lock().await;
        let slot = self.take_slot().map_err(|e| {
            format!("the commands cannot be switched while the chat works ({e}): stop it or wait for it to end, then switch")
        })?;
        if self.node.is_stopping() {
            return Err(harness_protocol::NODE_STOPPING.into());
        }
        if self.peers_pending() {
            return Err(format!(
                "the commands cannot be switched: {}",
                super::peers::PEERS_PENDING
            ));
        }
        if let Some(what) = self.background_work() {
            return Err(format!(
                "the commands cannot be switched while this chat has background work running \
                 or an end not recorded yet ({what}): stop it or wait for it to end, then switch"
            ));
        }
        if !self
            .approvals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_empty()
        {
            return Err(
                "the commands cannot be switched while an approval waits: answer it, then switch"
                    .into(),
            );
        }
        if self.unclosed.load(Ordering::SeqCst) {
            return Err(
                "the commands cannot be switched: the last turn's end is not in the log yet; \
                 send a message first"
                    .into(),
            );
        }
        let Some(from) = *self.exec_mode.lock().unwrap_or_else(|p| p.into_inner()) else {
            return Err(EXEC_UNREADABLE.into());
        };
        if from == to {
            return Ok(());
        }
        // The node decides again, from its own start-time offers, as at
        // create: what the master asks never widens them.
        self.node.cfg.policy.exec.choose(Some(to))?;
        // 1. The servers started under the old confinement.
        if let Some(mcp) = &self.mcp {
            mcp.stop_all();
            if !mcp.wait_gone(MCP_STOP_WAIT).await {
                return Err(
                    "an MCP server of this chat did not stop in time, so the commands were not \
                     switched; try again"
                        .into(),
                );
            }
        }
        // 2. The grants given under the old mode.
        self.drop_grants().await?;
        // 3. The log first: a switch it does not hold is not made.
        self.append(EventKind::ExecModeChanged(ExecModeChanged {
            mode: to,
            from,
        }))
        .await
        .map_err(|e| e.to_string())?;
        *self.exec_mode.lock().unwrap_or_else(|p| p.into_inner()) = Some(to);
        tracing::info!(session = %self.id, from = from.id(), to = to.id(), "the chat's commands were switched");
        drop(slot);
        drop(queue);
        Ok(())
    }

    /// Remove every chat grant, recorded one by one. If the log refuses one,
    /// the ones not yet removed are put back and the switch is refused: the
    /// table and the log agree at every point.
    async fn drop_grants(&self) -> Result<(), String> {
        let ids: Vec<String> = self
            .grants
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .keys()
            .cloned()
            .collect();
        for id in ids {
            // `revoke_allow` takes it out first and puts it back if the log
            // does not take its removal.
            self.revoke_allow(&id).await?;
        }
        Ok(())
    }
}
