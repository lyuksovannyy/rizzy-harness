//! Files waiting to be sent with a message that the model cannot read as
//! text (PLAN.md T94): the `chat_uploads` table. The bytes are a blob of
//! the owner's (`blobs`); this row is the reference that keeps the blob
//! until the file is sent or the upload is abandoned.
//!
//! Lifetime: [`Store::chat_upload_put`] adds the row; a send
//! [takes](Store::chat_uploads_take) it (marks it `sending`, so it is
//! taken once), and then the master either [finishes](Store::chat_uploads_done)
//! it (the node took the message: the row goes) or
//! [gives it back](Store::chat_uploads_give_back) (the node certainly did
//! not). A send whose answer was lost leaves the row marked: the node may
//! have the file already, and it is not offered again. Rows older than the
//! incognito TTL are [purged](Store::chat_uploads_purge) like unsent text
//! attachments, and the blob sweep then deletes their blobs.

use rusqlite::{OptionalExtension, params};

use crate::blobs::{BlobInsert, Limits, NewBlob, OverQuota};
use crate::{Result, Store};

/// Schema version 4 (the third migration after the schema was frozen,
/// PLAN.md §0): a database of an older version gets this table on its
/// next open.
pub const SCHEMA: &str = r#"
-- Files attached to a message not yet sent (PLAN.md T94), on the owner's
-- blobs. `sending` is set while a send holds the row; the row is deleted
-- when its message was taken, and purged by age when never sent.
CREATE TABLE IF NOT EXISTS chat_uploads (
    upload_id  TEXT PRIMARY KEY,
    owner_id   TEXT REFERENCES users(user_id) ON DELETE CASCADE,
    blob_id    TEXT NOT NULL REFERENCES blobs(blob_id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    mime       TEXT NOT NULL,
    size       INTEGER NOT NULL,
    sha256     TEXT NOT NULL,
    session_id TEXT,
    sending    INTEGER NOT NULL DEFAULT 0 CHECK (sending IN (0, 1)),
    created_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS chat_uploads_owner ON chat_uploads(owner_id);
CREATE INDEX IF NOT EXISTS chat_uploads_blob ON chat_uploads(blob_id);
"#;

/// An upload the master stored and is about to record.
#[derive(Debug, Clone)]
pub struct NewUpload {
    pub upload_id: String,
    /// `None`: the API token.
    pub owner_id: Option<String>,
    pub name: String,
    pub mime: String,
    /// The chat it was attached in, when it exists already.
    pub session_id: Option<String>,
}

/// What [`Store::chat_upload_put`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Put {
    /// Recorded. `new_blob`: the file written for it is needed (false: the
    /// owner already held these bytes, and the new file is not).
    Added {
        new_blob: bool,
    },
    /// The account already holds as many unsent uploads as it may.
    TooMany,
    OverQuota(OverQuota),
}

impl Put {
    pub fn added(&self) -> bool {
        matches!(self, Put::Added { .. })
    }
}

/// An upload a send holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakenUpload {
    pub upload_id: String,
    pub owner_id: Option<String>,
    pub blob_id: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
    pub sha256: String,
}

impl Store {
    /// Record an upload and its blob in one transaction, with the count
    /// and quota checks. The same bytes the owner already holds share
    /// their blob and cost nothing.
    pub fn chat_upload_put(
        &self,
        u: &NewUpload,
        blob: &NewBlob,
        limits: Limits,
        max_unsent: usize,
    ) -> Result<Put> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let n: i64 = tx.query_row(
            "SELECT COUNT(*) FROM chat_uploads WHERE owner_id IS ?1",
            params![u.owner_id],
            |r| r.get(0),
        )?;
        if n as usize >= max_unsent {
            return Ok(Put::TooMany);
        }
        let (blob_id, new_blob) = match crate::blobs::blob_insert(&tx, blob, limits)? {
            BlobInsert::OverQuota(q) => return Ok(Put::OverQuota(q)),
            BlobInsert::Inserted => (blob.blob_id.clone(), true),
            BlobInsert::Existing(id) => (id, false),
        };
        tx.execute(
            "INSERT INTO chat_uploads
               (upload_id, owner_id, blob_id, name, mime, size, sha256, session_id, created_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                u.upload_id,
                u.owner_id,
                blob_id,
                u.name,
                u.mime,
                blob.size as i64,
                blob.sha256,
                u.session_id,
                harness_core::now_ms()
            ],
        )?;
        tx.commit()?;
        Ok(Put::Added { new_blob })
    }

    /// Hold the given uploads for a send, all or nothing: each must exist,
    /// not be held already, and belong to `owner` (`None`, the API token,
    /// may take any). `Ok(None)` when any does not.
    pub fn chat_uploads_take(
        &self,
        ids: &[String],
        owner: Option<&str>,
    ) -> Result<Option<Vec<TakenUpload>>> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let row = tx
                .query_row(
                    "SELECT owner_id, blob_id, name, mime, size, sha256, sending
                     FROM chat_uploads WHERE upload_id = ?1",
                    params![id],
                    |r| {
                        Ok((
                            TakenUpload {
                                upload_id: id.clone(),
                                owner_id: r.get(0)?,
                                blob_id: r.get(1)?,
                                name: r.get(2)?,
                                mime: r.get(3)?,
                                size: r.get::<_, i64>(4)?.max(0) as u64,
                                sha256: r.get(5)?,
                            },
                            r.get::<_, i64>(6)? != 0,
                        ))
                    },
                )
                .optional()?;
            match row {
                Some((t, false)) if owner.is_none() || t.owner_id.as_deref() == owner => {
                    out.push(t)
                }
                _ => return Ok(None),
            }
            tx.execute(
                "UPDATE chat_uploads SET sending = 1 WHERE upload_id = ?1",
                params![id],
            )?;
        }
        tx.commit()?;
        Ok(Some(out))
    }

    /// The node certainly did not take the message: the uploads can be
    /// taken again.
    pub fn chat_uploads_give_back(&self, ids: &[String]) -> Result<()> {
        let conn = self.lock();
        for id in ids {
            conn.execute(
                "UPDATE chat_uploads SET sending = 0 WHERE upload_id = ?1",
                params![id],
            )?;
        }
        Ok(())
    }

    /// The node took the message: the rows go (their blobs, now
    /// unreferenced, are swept by the hourly sweep).
    pub fn chat_uploads_done(&self, ids: &[String]) -> Result<()> {
        let conn = self.lock();
        for id in ids {
            conn.execute("DELETE FROM chat_uploads WHERE upload_id = ?1", params![id])?;
        }
        Ok(())
    }

    /// Drop uploads created before `before_ms` (never sent, or a send
    /// whose answer was lost long ago). Returns how many.
    pub fn chat_uploads_purge(&self, before_ms: i64) -> Result<usize> {
        Ok(self.lock().execute(
            "DELETE FROM chat_uploads WHERE created_ms < ?1",
            params![before_ms],
        )?)
    }
}
