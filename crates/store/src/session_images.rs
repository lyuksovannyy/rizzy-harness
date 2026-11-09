//! Images sent with a chat's messages, kept on the node that runs it
//! (docs/media.md). The log records only references (`user_message.images`);
//! the bytes live here, keyed by their sha256, so a later turn can show them
//! to the model again. They go with the chat.

use rusqlite::params;

use crate::{Result, Store};

/// Run by `Store::init` on every store (node.db and chat.db use it).
pub(crate) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS session_images (
    session_id TEXT NOT NULL, sha256 TEXT NOT NULL, mime TEXT NOT NULL, name TEXT NOT NULL,
    bytes BLOB NOT NULL, PRIMARY KEY (session_id, sha256));
"#;

/// One stored image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionImage {
    pub sha256: String,
    pub mime: String,
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Called by `Store::delete_session` inside its transaction, so the image
/// bytes go with the chat (under `secure_delete`).
pub(crate) fn on_session_deleted(tx: &rusqlite::Transaction, sid: &str) -> crate::Result<()> {
    tx.execute(
        "DELETE FROM session_images WHERE session_id = ?1",
        params![sid],
    )?;
    Ok(())
}

impl Store {
    /// Keep `images` for chat `sid`. The same bytes sent again are stored
    /// once (the key is their sha256).
    pub fn session_images_put(&self, sid: &str, images: &[SessionImage]) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        for i in images {
            tx.execute(
                "INSERT OR IGNORE INTO session_images (session_id, sha256, mime, name, bytes)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![sid, i.sha256, i.mime, i.name, i.bytes],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Chat `sid`'s images among `sha256` (missing ones are left out).
    pub fn session_images(&self, sid: &str, sha256: &[String]) -> Result<Vec<SessionImage>> {
        let conn = self.lock();
        let mut st = conn.prepare(
            "SELECT sha256, mime, name, bytes FROM session_images
             WHERE session_id = ?1 AND sha256 = ?2",
        )?;
        let mut out = Vec::new();
        for sha in sha256 {
            let mut rows = st.query_map(params![sid, sha], |r| {
                Ok(SessionImage {
                    sha256: r.get(0)?,
                    mime: r.get(1)?,
                    name: r.get(2)?,
                    bytes: r.get(3)?,
                })
            })?;
            if let Some(row) = rows.next() {
                out.push(row?);
            }
        }
        Ok(out)
    }
}
