//! Media: each account's private library of images, video and audio
//! (docs/media.md). Owned by the files backend; Phase 0 only registers the
//! schema and the chat-deletion hook.

/// Run by `Store::init`, after the blob tables it references.
pub(crate) const SCHEMA: &str = "";

/// Called by `Store::delete_session` inside its transaction, so a deleted
/// chat leaves no rows that name it.
pub(crate) fn on_session_deleted(_tx: &rusqlite::Transaction, _sid: &str) -> crate::Result<()> {
    Ok(())
}
