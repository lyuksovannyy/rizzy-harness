//! Routines: saved instructions that start a chat on a schedule or a
//! webhook, and their runs (docs/routines.md). Owned by the routines
//! backend; Phase 0 registers the schema, the chat-deletion hook and
//! `routine_marks`, which search calls.

use std::collections::HashMap;

use harness_core::SessionId;

use crate::{Result, Store};

/// Run by `Store::init`, after the workspace and user tables it references.
pub(crate) const SCHEMA: &str = "";

/// What a chat started by a routine run shows: which routine, which run and
/// how that run stands (`SessionRow.routine` on the wire).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RoutineMark {
    pub routine_id: String,
    pub name: String,
    pub run_id: String,
    pub status: String,
    pub seen: bool,
    pub promoted: bool,
    pub untrusted: bool,
}

impl Store {
    /// The routine runs behind `sids`, by chat; chats no run started are
    /// left out.
    pub fn routine_marks(&self, _sids: &[SessionId]) -> Result<HashMap<SessionId, RoutineMark>> {
        Ok(HashMap::new())
    }
}

/// Called by `Store::delete_session` inside its transaction: the run keeps
/// its status, times and usage, but no text of the deleted chat.
pub(crate) fn on_session_deleted(_tx: &rusqlite::Transaction, _sid: &str) -> crate::Result<()> {
    Ok(())
}
