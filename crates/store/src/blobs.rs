//! The blob store's tables: one row per stored file and its display hints
//! (docs/media.md). Owned by the files backend; Phase 0 only registers the
//! schema and the account-deletion hook.

/// Run by `Store::init`, after the core schema.
pub(crate) const SCHEMA: &str = "";

/// Called by `Store::delete_user` inside its transaction, before the
/// account's row goes: the FK cascade then removes its blob rows, and what
/// a cascade cannot reach (FTS rows) must be deleted here first.
pub(crate) fn on_user_deleted(_tx: &rusqlite::Transaction, _user_id: &str) -> crate::Result<()> {
    Ok(())
}
