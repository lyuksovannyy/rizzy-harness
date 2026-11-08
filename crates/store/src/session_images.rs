//! Images sent with a chat's messages, kept on the node that runs it
//! (docs/media.md). Owned by the files backend; Phase 0 only registers the
//! schema and the chat-deletion hook.

/// Run by `Store::init` on every store (node.db and chat.db use it).
pub(crate) const SCHEMA: &str = "";

/// Called by `Store::delete_session` inside its transaction, so the image
/// bytes go with the chat (under `secure_delete`).
pub(crate) fn on_session_deleted(_tx: &rusqlite::Transaction, _sid: &str) -> crate::Result<()> {
    Ok(())
}
