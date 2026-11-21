//! The owner's bookmark on a message (docs/pins.md, README "Pinned
//! messages"). The node decides, against its own log: the rule is
//! `harness_core::pins::check` (shared with the master, which answers the
//! click from its mirror first). A pin is one event, `message_pinned`,
//! written by this session's one writer through the same append every
//! other event takes, so `seq` stays contiguous; nothing the model reads
//! changes (the rebuild and every other reader ignore the kind), so, unlike
//! a rewind, a running turn does not stop it and it does not claim the turn
//! slot. Nothing here can be started by a model, a tool or a routine: only
//! `session.pin`, which the master sends for the chat's owner.

use harness_core::event::{EventKind, MessagePinned};
use harness_core::pins;

use super::Session;

/// How the node's refusal of a `session.pin` starts.
pub const PIN_REFUSED: &str = "This message can't be pinned";

impl Session {
    /// Pin (`pinned`) or unpin the message at `message_seq` (`session.pin`).
    /// Refused on a stopping node and for anything `pins::check` refuses.
    /// Acked once `message_pinned` is written.
    pub(crate) async fn pin_message(&self, message_seq: u64, pinned: bool) -> Result<(), String> {
        if self.node.is_stopping() {
            return Err(harness_protocol::NODE_STOPPING.into());
        }
        // One pin at a time: the check and the write see the same pins, so
        // two clicks cannot take the chat past the limit. (The gate is the
        // one a rewind's check and write hold; a running turn never holds
        // it across a model call.)
        let _gate = self.bg_gate.lock().await;
        let id = self.id;
        let log = self
            .node
            .cfg
            .store
            .call(move |s| s.events_since_turn_at(id, i64::MIN, pins::KINDS))
            .await
            .map_err(|e| e.to_string())?;
        pins::check(&log, message_seq, pinned).map_err(|e| format!("{PIN_REFUSED}: {e}"))?;
        self.append(EventKind::MessagePinned(MessagePinned {
            message_seq,
            pinned,
        }))
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }
}
