//! The blob store's tables: one row per stored file and its display hints
//! (docs/media.md). The files live under the master's data dir, encrypted
//! with the key in their row; this module only keeps the rows. A row goes
//! when the last row that references it goes (`release_unreferenced`, in
//! the same transaction), and the master unlinks the file after the commit.

use std::collections::HashSet;

use rusqlite::{OptionalExtension, Transaction, params};

use crate::{Result, Store};

/// Run by `Store::init`, after the core schema.
pub(crate) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS blobs (
    blob_id    TEXT PRIMARY KEY,
    owner_id   TEXT REFERENCES users(user_id) ON DELETE CASCADE,
    sha256     TEXT NOT NULL,
    size       INTEGER NOT NULL,
    kind       TEXT NOT NULL,
    format     TEXT NOT NULL,
    mime       TEXT NOT NULL,
    display    TEXT NOT NULL,
    key        BLOB NOT NULL,
    created_ms INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS blobs_owner_sha ON blobs(COALESCE(owner_id, ''), sha256);
CREATE INDEX IF NOT EXISTS blobs_owner ON blobs(owner_id);
-- Display hints and the browser-made thumbnail, per blob.
CREATE TABLE IF NOT EXISTS blob_previews (
    blob_id     TEXT PRIMARY KEY REFERENCES blobs(blob_id) ON DELETE CASCADE,
    width       INTEGER, height INTEGER,
    duration_ms INTEGER,
    thumb_mime  TEXT,
    thumb       BLOB,
    no_thumb    INTEGER NOT NULL DEFAULT 0,
    updated_ms  INTEGER NOT NULL
);
"#;

/// A blob nothing references any more: no Media item and no artifact
/// version. Needs `blobs b`.
const UNREFERENCED: &str = "NOT EXISTS (SELECT 1 FROM media_items r WHERE r.blob_id = b.blob_id)
     AND NOT EXISTS (SELECT 1 FROM artifact_versions v WHERE v.blob_id = b.blob_id)";

const BLOB_COLS: &str =
    "b.blob_id, b.owner_id, b.sha256, b.size, b.kind, b.format, b.mime, b.display, b.created_ms";

/// A file the master wrote and is about to record.
#[derive(Debug, Clone)]
pub struct NewBlob {
    pub blob_id: String,
    /// `None`: the API token.
    pub owner_id: Option<String>,
    /// Of the plaintext, computed by the master while it wrote the file.
    pub sha256: String,
    pub size: u64,
    pub kind: String,
    pub format: String,
    pub mime: String,
    /// `inline` | `img` | `download`.
    pub display: String,
    pub key: Vec<u8>,
    /// Image dimensions read from the file header (never decoded).
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// A stored file's row, without its key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobRow {
    pub blob_id: String,
    pub owner_id: Option<String>,
    pub sha256: String,
    pub size: u64,
    pub kind: String,
    pub format: String,
    pub mime: String,
    pub display: String,
    pub created_ms: i64,
}

impl BlobRow {
    pub(crate) fn from_row(r: &rusqlite::Row, at: usize) -> rusqlite::Result<Self> {
        Ok(Self {
            blob_id: r.get(at)?,
            owner_id: r.get(at + 1)?,
            sha256: r.get(at + 2)?,
            size: r.get::<_, i64>(at + 3)?.max(0) as u64,
            kind: r.get(at + 4)?,
            format: r.get(at + 5)?,
            mime: r.get(at + 6)?,
            display: r.get(at + 7)?,
            created_ms: r.get(at + 8)?,
        })
    }
}

/// A blob's display hints, as the UI gets them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Preview {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
    pub has_thumb: bool,
    /// The owner's browser could not or would not make a thumbnail.
    pub no_thumb: bool,
}

/// Byte limits a new blob must fit; 0 is no limit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Limits {
    /// The owner's blobs together (the API token is never limited here).
    pub account: u64,
    /// Every blob together.
    pub server: u64,
}

/// Why a new blob did not fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverQuota {
    pub used: u64,
    pub limit: u64,
    /// The server's limit, not the account's.
    pub server: bool,
}

/// What `blob_insert` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlobInsert {
    /// The owner already holds these bytes: this blob (the new file is not
    /// needed).
    Existing(String),
    Inserted,
    OverQuota(OverQuota),
}

/// One account's stored bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Usage {
    /// Every blob the account owns.
    pub used: u64,
    /// Blobs its Media items show, and how many items.
    pub media_bytes: u64,
    pub media_items: u64,
    /// Blobs its artifact versions hold, and how many ready versions.
    pub artifact_bytes: u64,
    pub artifact_versions: u64,
}

/// One account's line on the admin's storage page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountUsage {
    pub user_id: String,
    pub username: String,
    pub used: u64,
    pub media_items: u64,
    pub artifact_versions: u64,
}

/// Record a blob in `tx`, with the quota check: a blob the owner already
/// holds (same sha256) is reused and costs nothing; a new one must fit the
/// account's and the server's limits. An image's header dimensions go into
/// its preview row in the same transaction.
pub(crate) fn blob_insert(tx: &Transaction, b: &NewBlob, limits: Limits) -> Result<BlobInsert> {
    let existing: Option<String> = tx
        .query_row(
            "SELECT blob_id FROM blobs WHERE COALESCE(owner_id, '') = COALESCE(?1, '') AND sha256 = ?2",
            params![b.owner_id, b.sha256],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = existing {
        return Ok(BlobInsert::Existing(id));
    }
    if limits.account > 0
        && let Some(owner) = &b.owner_id
    {
        let used = sum(tx, "WHERE owner_id = ?1", params![owner])?;
        if used.saturating_add(b.size) > limits.account {
            return Ok(BlobInsert::OverQuota(OverQuota {
                used,
                limit: limits.account,
                server: false,
            }));
        }
    }
    if limits.server > 0 {
        let used = sum(tx, "", params![])?;
        if used.saturating_add(b.size) > limits.server {
            return Ok(BlobInsert::OverQuota(OverQuota {
                used,
                limit: limits.server,
                server: true,
            }));
        }
    }
    let now = harness_core::now_ms();
    tx.execute(
        "INSERT INTO blobs (blob_id, owner_id, sha256, size, kind, format, mime, display, key, created_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            b.blob_id,
            b.owner_id,
            b.sha256,
            b.size as i64,
            b.kind,
            b.format,
            b.mime,
            b.display,
            b.key,
            now
        ],
    )?;
    if b.width.is_some() || b.height.is_some() {
        tx.execute(
            "INSERT INTO blob_previews (blob_id, width, height, updated_ms) VALUES (?1, ?2, ?3, ?4)",
            params![b.blob_id, b.width, b.height, now],
        )?;
    }
    Ok(BlobInsert::Inserted)
}

fn sum(tx: &Transaction, filter: &str, p: impl rusqlite::Params) -> Result<u64> {
    let n: i64 = tx.query_row(
        &format!("SELECT COALESCE(SUM(size), 0) FROM blobs {filter}"),
        p,
        |r| r.get(0),
    )?;
    Ok(n.max(0) as u64)
}

/// Delete those of `candidates` nothing references any more; returns the
/// ones deleted, whose files the master unlinks after the commit.
pub(crate) fn release_unreferenced(tx: &Transaction, candidates: &[String]) -> Result<Vec<String>> {
    let mut gone = Vec::new();
    let mut st = tx.prepare_cached(&format!(
        "DELETE FROM blobs AS b WHERE b.blob_id = ?1 AND {UNREFERENCED}"
    ))?;
    for id in candidates {
        if !gone.contains(id) && st.execute(params![id])? > 0 {
            gone.push(id.clone());
        }
    }
    Ok(gone)
}

/// Called by `Store::delete_user` inside its transaction, before the
/// account's row goes: the FK cascade then removes its blob and item rows,
/// and what a cascade cannot reach (FTS rows) must be deleted here first.
pub(crate) fn on_user_deleted(tx: &rusqlite::Transaction, user_id: &str) -> crate::Result<()> {
    crate::media::forget_owner(tx, user_id)?;
    crate::artifacts::forget_owner(tx, user_id)
}

impl Store {
    pub fn blob(&self, blob_id: &str) -> Result<Option<BlobRow>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                &format!("SELECT {BLOB_COLS} FROM blobs b WHERE b.blob_id = ?1"),
                params![blob_id],
                |r| BlobRow::from_row(r, 0),
            )
            .optional()?)
    }

    /// The key a blob's file is encrypted with.
    pub fn blob_key(&self, blob_id: &str) -> Result<Option<Vec<u8>>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                "SELECT key FROM blobs WHERE blob_id = ?1",
                params![blob_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// What `owner` (`None`: the API token) stores.
    pub fn blob_usage(&self, owner: Option<&str>) -> Result<Usage> {
        let conn = self.lock();
        let n: [i64; 5] = conn.query_row(
            "SELECT
                (SELECT COALESCE(SUM(size), 0) FROM blobs WHERE owner_id IS ?1),
                (SELECT COALESCE(SUM(b.size), 0) FROM blobs b
                 WHERE b.owner_id IS ?1
                   AND EXISTS (SELECT 1 FROM media_items mi WHERE mi.blob_id = b.blob_id)),
                (SELECT COUNT(*) FROM media_items WHERE owner_id IS ?1),
                (SELECT COALESCE(SUM(b.size), 0) FROM blobs b
                 WHERE b.owner_id IS ?1
                   AND EXISTS (SELECT 1 FROM artifact_versions v WHERE v.blob_id = b.blob_id)),
                (SELECT COUNT(*) FROM artifact_versions v JOIN artifacts a ON a.artifact_id = v.artifact_id
                 WHERE a.owner_id IS ?1 AND v.status = 'ready')",
            params![owner],
            |r| Ok([r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?]),
        )?;
        let [
            used,
            media_bytes,
            media_items,
            artifact_bytes,
            artifact_versions,
        ] = n.map(|v| v.max(0) as u64);
        Ok(Usage {
            used,
            media_bytes,
            media_items,
            artifact_bytes,
            artifact_versions,
        })
    }

    /// Every blob's bytes together.
    pub fn blob_total(&self) -> Result<u64> {
        let conn = self.lock();
        let n: i64 =
            conn.query_row("SELECT COALESCE(SUM(size), 0) FROM blobs", [], |r| r.get(0))?;
        Ok(n.max(0) as u64)
    }

    /// Each account's usage (numbers only), largest first, and every
    /// blob's bytes together (the API token's included).
    pub fn storage_usage_all(&self) -> Result<(Vec<AccountUsage>, u64)> {
        let conn = self.lock();
        let mut st = conn.prepare(
            "SELECT u.user_id, u.username,
                    (SELECT COALESCE(SUM(size), 0) FROM blobs b WHERE b.owner_id = u.user_id),
                    (SELECT COUNT(*) FROM media_items mi WHERE mi.owner_id = u.user_id),
                    (SELECT COUNT(*) FROM artifact_versions v JOIN artifacts a ON a.artifact_id = v.artifact_id
                     WHERE a.owner_id = u.user_id AND v.status = 'ready')
             FROM users u ORDER BY 3 DESC, u.username",
        )?;
        let rows = st
            .query_map([], |r| {
                Ok(AccountUsage {
                    user_id: r.get(0)?,
                    username: r.get(1)?,
                    used: r.get::<_, i64>(2)?.max(0) as u64,
                    media_items: r.get::<_, i64>(3)?.max(0) as u64,
                    artifact_versions: r.get::<_, i64>(4)?.max(0) as u64,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let total: i64 =
            conn.query_row("SELECT COALESCE(SUM(size), 0) FROM blobs", [], |r| r.get(0))?;
        Ok((rows, total.max(0) as u64))
    }

    /// Blobs nothing references, created before `before_ms` (a blob is
    /// always inserted with its first reference, so these are leftovers).
    pub fn blobs_unreferenced(&self, before_ms: i64) -> Result<Vec<String>> {
        let conn = self.lock();
        let mut st = conn.prepare(&format!(
            "SELECT b.blob_id FROM blobs b WHERE b.created_ms < ?1 AND {UNREFERENCED}"
        ))?;
        let ids = st
            .query_map(params![before_ms], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(ids)
    }

    /// Delete those of `ids` that are still unreferenced; returns the ones
    /// deleted.
    pub fn blob_delete(&self, ids: &[String]) -> Result<Vec<String>> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let gone = release_unreferenced(&tx, ids)?;
        tx.commit()?;
        Ok(gone)
    }

    /// Every blob id with a row, for the sweeps that delete files without
    /// one.
    pub fn known_blob_ids(&self) -> Result<HashSet<String>> {
        let conn = self.lock();
        let mut st = conn.prepare("SELECT blob_id FROM blobs")?;
        let ids = st
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<HashSet<String>>>()?;
        Ok(ids)
    }

    pub fn preview_get(&self, blob_id: &str) -> Result<Preview> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                "SELECT width, height, duration_ms, thumb IS NOT NULL, no_thumb
                 FROM blob_previews WHERE blob_id = ?1",
                params![blob_id],
                preview_from_row,
            )
            .optional()?
            .unwrap_or_default())
    }

    /// The thumbnail's type and bytes, if the owner's browser made one.
    pub fn preview_thumb(&self, blob_id: &str) -> Result<Option<(String, Vec<u8>)>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                "SELECT thumb_mime, thumb FROM blob_previews
                 WHERE blob_id = ?1 AND thumb IS NOT NULL AND thumb_mime IS NOT NULL",
                params![blob_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    /// Store a thumbnail (already sniffed and size-checked by the master);
    /// it clears `no_thumb`.
    pub fn preview_put_thumb(&self, blob_id: &str, mime: &str, bytes: &[u8]) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO blob_previews (blob_id, thumb_mime, thumb, no_thumb, updated_ms)
             VALUES (?1, ?2, ?3, 0, ?4)
             ON CONFLICT(blob_id) DO UPDATE SET thumb_mime = ?2, thumb = ?3, no_thumb = 0,
                                                updated_ms = ?4",
            params![blob_id, mime, bytes, harness_core::now_ms()],
        )?;
        Ok(())
    }

    /// The owner's browser hints (already clamped); a `None` leaves that
    /// hint as it is.
    pub fn preview_set_hints(
        &self,
        blob_id: &str,
        width: Option<u32>,
        height: Option<u32>,
        duration_ms: Option<u64>,
    ) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO blob_previews (blob_id, width, height, duration_ms, updated_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(blob_id) DO UPDATE SET width = COALESCE(?2, width),
                 height = COALESCE(?3, height), duration_ms = COALESCE(?4, duration_ms),
                 updated_ms = ?5",
            params![
                blob_id,
                width,
                height,
                duration_ms.map(|d| d as i64),
                harness_core::now_ms()
            ],
        )?;
        Ok(())
    }

    pub fn preview_set_no_thumb(&self, blob_id: &str, no_thumb: bool) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO blob_previews (blob_id, no_thumb, updated_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT(blob_id) DO UPDATE SET no_thumb = ?2, updated_ms = ?3",
            params![blob_id, no_thumb, harness_core::now_ms()],
        )?;
        Ok(())
    }
}

/// `width, height, duration_ms, has_thumb, no_thumb` from `at` on; every
/// column may be NULL (a LEFT JOIN without a preview row).
pub(crate) fn preview_at(r: &rusqlite::Row, at: usize) -> rusqlite::Result<Preview> {
    let dim = |v: Option<i64>| v.and_then(|v| u32::try_from(v).ok());
    Ok(Preview {
        width: dim(r.get(at)?),
        height: dim(r.get(at + 1)?),
        duration_ms: r
            .get::<_, Option<i64>>(at + 2)?
            .and_then(|v| u64::try_from(v).ok()),
        has_thumb: r.get::<_, Option<bool>>(at + 3)?.unwrap_or(false),
        no_thumb: r.get::<_, Option<bool>>(at + 4)?.unwrap_or(false),
    })
}

fn preview_from_row(r: &rusqlite::Row) -> rusqlite::Result<Preview> {
    preview_at(r, 0)
}
