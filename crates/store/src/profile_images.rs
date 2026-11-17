//! A person's profile pictures (PLAN.md T74): one avatar and one banner per
//! account, each a blob of the blob store (`blobs`), so they are encrypted,
//! count against the account's storage and go with the account (both
//! foreign keys cascade). A new one replaces the old in one transaction; the
//! old blob goes when nothing else references it.

use rusqlite::{OptionalExtension, params};

use crate::blobs::{BlobInsert, BlobRow, Limits, NewBlob, OverQuota};
use crate::{Result, Store};

/// Run by `Store::init`, after `blobs`.
pub(crate) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS profile_images (
    user_id    TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    slot       TEXT NOT NULL CHECK (slot IN ('avatar', 'banner')),
    blob_id    TEXT NOT NULL REFERENCES blobs(blob_id) ON DELETE CASCADE,
    updated_ms INTEGER NOT NULL,
    PRIMARY KEY (user_id, slot)
);
CREATE INDEX IF NOT EXISTS profile_images_blob ON profile_images(blob_id);
"#;

/// What `profile_image_set` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileImageSet {
    /// Stored; `new_blob` says whether the written file is needed (false:
    /// the account already held these bytes). `gone`: blobs the
    /// replaced picture left unreferenced, whose files the master unlinks.
    Stored {
        sha256: String,
        new_blob: bool,
        gone: Vec<String>,
    },
    OverQuota(OverQuota),
}

impl Store {
    /// Make `blob` the account's `slot` picture, replacing the one before.
    pub fn profile_image_set(
        &self,
        user_id: &str,
        slot: &str,
        blob: &NewBlob,
        limits: Limits,
    ) -> Result<ProfileImageSet> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let (blob_id, new_blob) = match crate::blobs::blob_insert(&tx, blob, limits)? {
            BlobInsert::OverQuota(q) => return Ok(ProfileImageSet::OverQuota(q)),
            BlobInsert::Inserted => (blob.blob_id.clone(), true),
            BlobInsert::Existing(id) => (id, false),
        };
        let old: Option<String> = tx
            .query_row(
                "SELECT blob_id FROM profile_images WHERE user_id = ?1 AND slot = ?2",
                params![user_id, slot],
                |r| r.get(0),
            )
            .optional()?;
        tx.execute(
            "INSERT INTO profile_images (user_id, slot, blob_id, updated_ms) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (user_id, slot) DO UPDATE SET blob_id = excluded.blob_id,
                                                       updated_ms = excluded.updated_ms",
            params![user_id, slot, blob_id, harness_core::now_ms()],
        )?;
        let gone = match old {
            Some(o) if o != blob_id => crate::blobs::release_unreferenced(&tx, &[o])?,
            _ => Vec::new(),
        };
        let sha256: String = tx.query_row(
            "SELECT sha256 FROM blobs WHERE blob_id = ?1",
            params![blob_id],
            |r| r.get(0),
        )?;
        tx.commit()?;
        Ok(ProfileImageSet::Stored {
            sha256,
            new_blob,
            gone,
        })
    }

    /// The account's `slot` picture, if it has one.
    pub fn profile_image(&self, user_id: &str, slot: &str) -> Result<Option<BlobRow>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                "SELECT b.blob_id, b.owner_id, b.sha256, b.size, b.kind, b.format, b.mime,
                        b.display, b.created_ms
                 FROM profile_images p JOIN blobs b ON b.blob_id = p.blob_id
                 WHERE p.user_id = ?1 AND p.slot = ?2",
                params![user_id, slot],
                |r| BlobRow::from_row(r, 0),
            )
            .optional()?)
    }

    /// Remove the account's `slot` picture; returns the blobs it left
    /// unreferenced.
    pub fn profile_image_delete(&self, user_id: &str, slot: &str) -> Result<Vec<String>> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let old: Option<String> = tx
            .query_row(
                "DELETE FROM profile_images WHERE user_id = ?1 AND slot = ?2 RETURNING blob_id",
                params![user_id, slot],
                |r| r.get(0),
            )
            .optional()?;
        let gone = match old {
            Some(o) => crate::blobs::release_unreferenced(&tx, &[o])?,
            None => Vec::new(),
        };
        tx.commit()?;
        Ok(gone)
    }

    /// Whether `a` and `b` are members of one workspace together (where
    /// each sees the other's name in the members list).
    pub fn share_a_workspace(&self, a: &str, b: &str) -> Result<bool> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM workspace_members x
                            JOIN workspace_members y ON y.workspace_id = x.workspace_id
                            WHERE x.user_id = ?1 AND y.user_id = ?2)",
            params![a, b],
            |r| r.get::<_, i64>(0),
        )? != 0)
    }
}
