//! Files a person attached to a chat that has no folder, kept on the
//! owner's blobs for the agent to read through `attached_file` (PLAN.md
//! T95): the `session_files` table. A row says "this chat may read this
//! blob under this id"; it is the master's record of what it sent the chat,
//! and it is served only once the chat's own log has the message that names
//! the file (`logged`).
//!
//! Lifetime: a send [holds](Store::session_files_hold) a row (not logged)
//! when it takes the upload and [releases](Store::session_files_release) it
//! when the node certainly did not take the message; the mirror marks it
//! `logged` when it applies the `user_message` whose `attached` names it
//! (`on_message`); a fork's copy of that message gives the fork a row of its
//! own for the same blob (the parent's, when the same node and the same
//! account hold it). The rows go with the chat (`on_session_deleted`), and a
//! blob is swept once no row, upload, Media item, artifact or picture
//! refers to it (`blobs::UNREFERENCED`): deleting the last chat that
//! refers to a file frees it.

use rusqlite::{OptionalExtension, Transaction, params};

use crate::chat_uploads::TakenUpload;
use crate::{Result, Store};

/// Schema version 5 (PLAN.md §0): a database of an older version gets this
/// table on its next open.
pub const SCHEMA: &str = r#"
-- Files a chat may read through `attached_file` (PLAN.md T95), on the
-- owner's blobs. `logged` is set when the chat's own log has the message
-- that names the file; only a logged row is served. Rows go with the chat,
-- and a blob with no row left is swept.
CREATE TABLE IF NOT EXISTS session_files (
    session_id TEXT NOT NULL,
    file_id    TEXT NOT NULL,
    blob_id    TEXT NOT NULL REFERENCES blobs(blob_id) ON DELETE CASCADE,
    owner_id   TEXT,
    name       TEXT NOT NULL,
    mime       TEXT NOT NULL,
    size       INTEGER NOT NULL,
    sha256     TEXT NOT NULL,
    logged     INTEGER NOT NULL DEFAULT 0 CHECK (logged IN (0, 1)),
    created_ms INTEGER NOT NULL,
    PRIMARY KEY (session_id, file_id)
);
CREATE INDEX IF NOT EXISTS session_files_blob ON session_files(blob_id);
"#;

/// A file a chat may read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFile {
    pub file_id: String,
    pub blob_id: String,
    /// The account whose blob it is (`None`: the API token's).
    pub owner_id: Option<String>,
    pub name: String,
    pub mime: String,
    pub size: u64,
    pub sha256: String,
}

/// Called by `Store::delete_session` inside its transaction: the chat's
/// rows go, and the blobs they alone held are swept by the hourly sweep.
pub(crate) fn on_session_deleted(tx: &Transaction, sid: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM session_files WHERE session_id = ?1",
        params![sid],
    )?;
    Ok(())
}

/// Called by `Store::append` for a `user_message`: the files its `attached`
/// names become readable in this chat. A normal message marks the rows its
/// send held (the id and the hash must both match, so a node cannot name a
/// file the master did not send this chat). A fork's copy of a message
/// (`copy`) gives the fork a row of its own for each file, from the same
/// row of the parent, only when the parent is a chat of the same node (the
/// one appending) and the row is logged there.
pub(crate) fn on_message(
    tx: &Transaction,
    sid: &str,
    node: &str,
    e: &harness_core::RawEvent,
    copy: bool,
) -> Result<()> {
    let Some(list) = e.payload.get("attached").and_then(|v| v.as_array()) else {
        return Ok(());
    };
    let parent: Option<String> = if copy {
        crate::session_forks::parent(tx, sid)?
    } else {
        None
    };
    for f in list.iter().take(crate::MAX_ATTACHED_PER_MESSAGE) {
        let (Some(id), Some(sha)) = (
            f.get("file_id").and_then(|v| v.as_str()),
            f.get("sha256").and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        if copy {
            let Some(p) = &parent else { continue };
            tx.execute(
                "INSERT OR IGNORE INTO session_files
                   (session_id, file_id, blob_id, owner_id, name, mime, size, sha256, logged, created_ms)
                 SELECT ?1, f.file_id, f.blob_id, f.owner_id, f.name, f.mime, f.size, f.sha256, 1, ?5
                 FROM session_files f JOIN sessions ps ON ps.session_id = f.session_id
                 WHERE f.session_id = ?2 AND f.file_id = ?3 AND f.sha256 = ?4
                   AND f.logged = 1 AND ps.node_id = ?6",
                params![sid, p, id, sha, harness_core::now_ms(), node],
            )?;
        } else {
            tx.execute(
                "UPDATE session_files SET logged = 1
                 WHERE session_id = ?1 AND file_id = ?2 AND sha256 = ?3",
                params![sid, id, sha],
            )?;
        }
    }
    Ok(())
}

impl Store {
    /// Record the files a send of chat `sid` carries, not yet logged: the
    /// blobs they name are referenced from now on, whatever becomes of the
    /// unsent upload rows.
    pub fn session_files_hold(&self, sid: &str, files: &[TakenUpload]) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        for f in files {
            tx.execute(
                "INSERT OR REPLACE INTO session_files
                   (session_id, file_id, blob_id, owner_id, name, mime, size, sha256, logged, created_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9)",
                params![
                    sid,
                    f.upload_id,
                    f.blob_id,
                    f.owner_id,
                    f.name,
                    f.mime,
                    f.size as i64,
                    f.sha256,
                    harness_core::now_ms()
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The node certainly did not take the message: its rows that no log
    /// names go (a logged one stays: the chat has the message).
    pub fn session_files_release(&self, sid: &str, ids: &[String]) -> Result<()> {
        let conn = self.lock();
        for id in ids {
            conn.execute(
                "DELETE FROM session_files WHERE session_id = ?1 AND file_id = ?2 AND logged = 0",
                params![sid, id],
            )?;
        }
        Ok(())
    }

    /// Whether chat `sid` has a file its own log names: the tool is offered
    /// to a turn only then (a row held by a send whose answer was lost, and
    /// never logged, offers nothing: the chat can never read it).
    pub fn session_files_any(&self, sid: &str) -> Result<bool> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM session_files WHERE session_id = ?1 AND logged = 1)",
            params![sid],
            |r| r.get(0),
        )?)
    }

    /// The file `file_id` of chat `sid`, when the chat's own log has the
    /// message that names it. Never by owner, never by another chat.
    pub fn session_file(&self, sid: &str, file_id: &str) -> Result<Option<SessionFile>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                "SELECT file_id, blob_id, owner_id, name, mime, size, sha256
                 FROM session_files WHERE session_id = ?1 AND file_id = ?2 AND logged = 1",
                params![sid, file_id],
                |r| {
                    Ok(SessionFile {
                        file_id: r.get(0)?,
                        blob_id: r.get(1)?,
                        owner_id: r.get(2)?,
                        name: r.get(3)?,
                        mime: r.get(4)?,
                        size: r.get::<_, i64>(5)?.max(0) as u64,
                        sha256: r.get(6)?,
                    })
                },
            )
            .optional()?)
    }
}
