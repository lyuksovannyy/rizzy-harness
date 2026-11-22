//! The owner's switch of an existing agentic chat between the sandbox and
//! the host shell (N.10b, PLAN.md T39). The master checks what it can from
//! its own state, in the order a create checks it, through the same
//! functions, and relays `session.set_exec_mode`; the node decides again
//! (what it offers, whether anything of the chat runs) and records
//! `exec_mode_changed`. The master's mirror (`sessions.exec_mode`) follows
//! that event.

use harness_core::policy::ExecMode;
use harness_protocol::MasterToNode;
use harness_store::SessionRow;

use crate::{CommandError, DIVERGED, Master, workspaces};

impl Master {
    /// Switch the commands of chat `row` to `mode`. The caller has settled
    /// who may ask (the chat's owner only: `http/exec_mode.rs`); this is
    /// every other check:
    ///
    /// 1. the chat is an agentic chat of a person: not a chat without tools,
    ///    a sub-agent's (it follows its parent), a routine's (routines run
    ///    in the sandbox for good) or one nobody owns;
    /// 2. its log is not diverged (the node would write an event nobody
    ///    sees);
    /// 3. the owner's effective level in its workspace allows `mode`, and
    ///    the node rule allows them the node (`workspaces::switch_refusal`);
    /// 4. a connected node announced `mode` (`mode_refusal`), and a switch
    ///    (`CAP_EXEC_SWITCH`: an older node is told to update).
    pub async fn set_exec_mode(
        &self,
        row: &SessionRow,
        mode: ExecMode,
    ) -> Result<(), CommandError> {
        let sid = row.session_id;
        if row.profile.as_deref() == Some("chat") {
            return Err(CommandError::Rejected(
                "a chat without tools runs no commands and has no execution mode".into(),
            ));
        }
        if row.parent_id.is_some() {
            return Err(CommandError::Rejected(
                "a sub-agent runs in the execution mode of the chat that started it; switch \
                 that chat"
                    .into(),
            ));
        }
        if self
            .inner
            .mirror
            .call(move |s| s.run_by_session(sid))
            .await?
            .is_some()
        {
            return Err(CommandError::Rejected(
                "a routine's chat always runs in the sandbox".into(),
            ));
        }
        if self
            .inner
            .mirror
            .call(move |s| s.diverged_seq(sid))
            .await?
            .is_some()
        {
            return Err(CommandError::Rejected(DIVERGED.into()));
        }
        if let Some(why) = workspaces::switch_refusal(self, row, mode).await {
            return Err(CommandError::Forbidden(why));
        }
        if let Some(why) = self.mode_refusal(&row.node_id, mode) {
            return Err(CommandError::Rejected(why));
        }
        if !self.node_has(&row.node_id, harness_protocol::CAP_EXEC_SWITCH) {
            return Err(CommandError::Rejected(
                "this chat's node cannot switch a chat's commands (a node older than this \
                 server, or one that is away): update the node"
                    .into(),
            ));
        }
        self.send_command(&row.node_id, |req_id| MasterToNode::SetExecMode {
            req_id,
            session_id: sid,
            mode,
        })
        .await?;
        // The level may have been lowered between the check and the ack: a
        // chat now in a mode above its owner's level is stopped, as for any
        // command that made a node work.
        if mode == ExecMode::Host {
            self.recheck_after_ack(sid);
        }
        Ok(())
    }
}
