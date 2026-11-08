//! Media: each account's private library of images, video and audio
//! (docs/media.md). An item names one blob; the bytes, the sniffed kind and
//! the display hints are the blob's. Items are private to their owner: the
//! workspace an item was added in filters lists, it never shares them.

use rusqlite::types::Value;
use rusqlite::{OptionalExtension, Transaction, params, params_from_iter};

use crate::blobs::{BlobInsert, BlobRow, Limits, NewBlob, OverQuota, Preview};
use crate::{Result, Store};

/// Run by `Store::init`, after the blob tables it references. The table of
/// chats that carry an item (`media_uses`) ships with images in chats.
pub(crate) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS media_items (
    media_id     TEXT PRIMARY KEY,
    blob_id      TEXT NOT NULL REFERENCES blobs(blob_id) ON DELETE CASCADE,
    owner_id     TEXT REFERENCES users(user_id) ON DELETE CASCADE,
    workspace_id TEXT REFERENCES workspaces(workspace_id) ON DELETE SET NULL,
    name         TEXT NOT NULL,
    origin       TEXT NOT NULL,
    session_id   TEXT,
    artifact_id  TEXT,
    version_id   TEXT,
    created_ms   INTEGER NOT NULL,
    updated_ms   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS media_items_owner ON media_items(owner_id, created_ms DESC);
CREATE INDEX IF NOT EXISTS media_items_session ON media_items(session_id);
CREATE UNIQUE INDEX IF NOT EXISTS media_items_artifact ON media_items(artifact_id) WHERE artifact_id IS NOT NULL;
CREATE VIRTUAL TABLE IF NOT EXISTS media_fts USING fts5(media_id UNINDEXED, name,
    tokenize = 'unicode61 remove_diacritics 2');
"#;

/// An item's effective workspace: the one it was added in while its owner
/// is still a member there (or it has no owner: the API token), else the
/// owner's Personal workspace. Self-healing like the chats' `EFFECTIVE_WS`:
/// however an owner left, or the workspace went (`SET NULL`), their items
/// fall back to Personal. Needs `media_items mi`.
const MEDIA_WS: &str = "COALESCE(
    CASE WHEN mi.owner_id IS NULL
           OR EXISTS (SELECT 1 FROM workspace_members m
                      WHERE m.workspace_id = mi.workspace_id AND m.user_id = mi.owner_id)
         THEN mi.workspace_id END,
    (SELECT pw.workspace_id FROM workspaces pw WHERE pw.personal_owner = mi.owner_id))";

/// What a Media item shows as its kind: the blob's, or `other` for a media
/// file the browser cannot show (`display = download`). Needs `blobs b`.
const ITEM_KIND: &str = "CASE WHEN b.display = 'download' THEN 'other' ELSE b.kind END";

/// The columns `MediaRow::from_row` reads. The source chat joins only when
/// it is the item owner's own chat.
fn item_cols() -> String {
    format!(
        "mi.media_id, mi.owner_id, {MEDIA_WS}, mi.name, mi.origin, mi.session_id, mi.artifact_id,
         mi.version_id, mi.created_ms, mi.updated_ms, s.title,
         b.blob_id, b.owner_id, b.sha256, b.size, b.kind, b.format, b.mime, b.display, b.created_ms,
         p.width, p.height, p.duration_ms, p.thumb IS NOT NULL, p.no_thumb, av.seq, aa.name"
    )
}

/// The marked name `media_search` adds after `item_cols`.
const MARKED_AT: usize = 27;

const ITEM_JOINS: &str = "JOIN blobs b ON b.blob_id = mi.blob_id
     LEFT JOIN blob_previews p ON p.blob_id = mi.blob_id
     LEFT JOIN sessions s ON s.session_id = mi.session_id AND s.owner_id IS mi.owner_id
     LEFT JOIN artifact_versions av ON av.version_id = mi.version_id AND av.artifact_id = mi.artifact_id
     LEFT JOIN artifacts aa ON aa.artifact_id = mi.artifact_id";

/// One Media item with its blob and display hints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaRow {
    pub media_id: String,
    /// `None`: the API token.
    pub owner_id: Option<String>,
    /// The effective workspace (`MEDIA_WS`).
    pub workspace_id: Option<String>,
    pub name: String,
    /// `upload` | `chat` | `agent`.
    pub origin: String,
    pub session_id: Option<String>,
    /// The source chat's title, while it exists and is the owner's.
    pub session_title: Option<String>,
    pub artifact_id: Option<String>,
    pub version_id: Option<String>,
    /// The version it shows and its lineage's name, while both exist.
    pub artifact_seq: Option<u64>,
    pub artifact_name: Option<String>,
    pub created_ms: i64,
    pub updated_ms: i64,
    pub blob: BlobRow,
    pub preview: Preview,
}

impl MediaRow {
    fn from_row(r: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            media_id: r.get(0)?,
            owner_id: r.get(1)?,
            workspace_id: r.get(2)?,
            name: r.get(3)?,
            origin: r.get(4)?,
            session_id: r.get(5)?,
            artifact_id: r.get(6)?,
            version_id: r.get(7)?,
            created_ms: r.get(8)?,
            updated_ms: r.get(9)?,
            session_title: r.get(10)?,
            blob: BlobRow::from_row(r, 11)?,
            preview: crate::blobs::preview_at(r, 20)?,
            artifact_seq: r.get::<_, Option<i64>>(25)?.map(|v| v.max(0) as u64),
            artifact_name: r.get(26)?,
        })
    }

    /// `image` | `video` | `audio`, or `other` for a media file the browser
    /// cannot show.
    pub fn kind(&self) -> &str {
        if self.blob.display == "download" {
            "other"
        } else {
            &self.blob.kind
        }
    }
}

/// A Media item to add (its blob comes with it).
#[derive(Debug, Clone)]
pub struct NewItem {
    pub media_id: String,
    pub owner_id: Option<String>,
    /// Where it was added (`None`: the API token without a workspace).
    pub workspace_id: Option<String>,
    pub name: String,
    pub origin: String,
    pub session_id: Option<String>,
}

/// What `media_insert` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaInsert {
    /// A new item; `new_blob` is false when it reuses a blob the owner
    /// already had (the file just written is then not needed).
    Added {
        media_id: String,
        new_blob: bool,
    },
    /// The same bytes are already an item in the same effective workspace:
    /// that item (nothing was added; the new file is not needed).
    Duplicate {
        media_id: String,
    },
    OverQuota(OverQuota),
}

/// What `media_delete` removed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaDeleted {
    pub items: usize,
    /// Blobs nothing references any more: their files are the master's to
    /// unlink.
    pub blobs: Vec<String>,
}

/// A list call's filters. `owner` and `workspace` are the caller's
/// visibility (`None`: the API token / every workspace).
#[derive(Debug, Clone, Default)]
pub struct MediaQuery {
    pub owner: Option<String>,
    pub workspace: Option<String>,
    /// `image` | `video` | `audio` | `other`.
    pub kind: Option<String>,
    pub origin: Option<String>,
    pub session: Option<String>,
    /// A prepared FTS5 query over names (quoted tokens only).
    pub fts: Option<String>,
    /// Items older than this `(created_ms, media_id)`, for paging.
    pub before: Option<(i64, String)>,
    pub limit: usize,
}

/// SQL built from optional parts, with its positional arguments.
#[derive(Default)]
struct Sql {
    clauses: Vec<String>,
    args: Vec<Value>,
}

impl Sql {
    /// The placeholder for one more argument.
    fn arg(&mut self, v: impl Into<Value>) -> String {
        self.args.push(v.into());
        format!("?{}", self.args.len())
    }

    /// Who may see an item: its owner (`owner` None: the API token, all),
    /// in workspace `ws` (None: any). The list and search share it.
    fn visible(&mut self, owner: Option<&str>, ws: Option<&str>) {
        if let Some(o) = owner {
            let p = self.arg(o.to_string());
            self.clauses.push(format!("mi.owner_id = {p}"));
        }
        if let Some(w) = ws {
            let p = self.arg(w.to_string());
            self.clauses.push(format!("{MEDIA_WS} = {p}"));
        }
    }

    fn where_(&self) -> String {
        if self.clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", self.clauses.join(" AND "))
        }
    }
}

/// The effective workspace an item added by `owner` in `ws` gets (the
/// same rule as `MEDIA_WS`).
fn effective_ws(tx: &Transaction, owner: Option<&str>, ws: Option<&str>) -> Result<Option<String>> {
    let Some(owner) = owner else {
        return Ok(ws.map(str::to_string));
    };
    if let Some(ws) = ws {
        let member: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM workspace_members WHERE workspace_id = ?1 AND user_id = ?2)",
            params![ws, owner],
            |r| r.get(0),
        )?;
        if member {
            return Ok(Some(ws.to_string()));
        }
    }
    Ok(tx
        .query_row(
            "SELECT workspace_id FROM workspaces WHERE personal_owner = ?1",
            params![owner],
            |r| r.get(0),
        )
        .optional()?)
}

fn insert_item(tx: &Transaction, item: &NewItem, blob_id: &str) -> Result<()> {
    let now = harness_core::now_ms();
    tx.execute(
        "INSERT INTO media_items (media_id, blob_id, owner_id, workspace_id, name, origin,
                                  session_id, created_ms, updated_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
        params![
            item.media_id,
            blob_id,
            item.owner_id,
            item.workspace_id,
            item.name,
            item.origin,
            item.session_id,
            now
        ],
    )?;
    tx.execute(
        "INSERT INTO media_fts (media_id, name) VALUES (?1, ?2)",
        params![item.media_id, item.name],
    )?;
    Ok(())
}

/// A ready artifact version of a media kind, for its lineage's item.
pub(crate) struct AgentItem<'a> {
    pub artifact_id: &'a str,
    pub version_id: &'a str,
    pub blob_id: &'a str,
    pub session_id: Option<&'a str>,
}

/// Point the lineage's one Media item at `v` (origin `agent`), adding it
/// when there is none (also after the owner deleted it). Returns the blob
/// it showed before, now perhaps unreferenced.
pub(crate) fn upsert_agent(tx: &Transaction, v: &AgentItem) -> Result<Option<String>> {
    let found: Option<(String, String)> = tx
        .query_row(
            "SELECT media_id, blob_id FROM media_items WHERE artifact_id = ?1",
            params![v.artifact_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let now = harness_core::now_ms();
    if let Some((media_id, old)) = found {
        tx.execute(
            "UPDATE media_items SET blob_id = ?2, version_id = ?3, updated_ms = ?4 WHERE media_id = ?1",
            params![media_id, v.blob_id, v.version_id, now],
        )?;
        return Ok((old != v.blob_id).then_some(old));
    }
    let (owner, ws, name): (Option<String>, Option<String>, String) = tx.query_row(
        "SELECT owner_id, workspace_id, name FROM artifacts WHERE artifact_id = ?1",
        params![v.artifact_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let item = NewItem {
        media_id: harness_core::new_id(),
        owner_id: owner,
        workspace_id: ws,
        name,
        origin: "agent".into(),
        session_id: v.session_id.map(str::to_string),
    };
    insert_item(tx, &item, v.blob_id)?;
    tx.execute(
        "UPDATE media_items SET artifact_id = ?2, version_id = ?3 WHERE media_id = ?1",
        params![item.media_id, v.artifact_id, v.version_id],
    )?;
    Ok(None)
}

/// Drop `ids`' name index rows; FTS5 deletes only add tombstones, so a
/// merge follows when any went.
fn forget_fts(tx: &Transaction, ids: &[String]) -> Result<()> {
    let mut n = 0;
    for id in ids {
        n += tx.execute("DELETE FROM media_fts WHERE media_id = ?1", params![id])?;
    }
    if n > 0 {
        tx.execute("INSERT INTO media_fts(media_fts) VALUES('optimize')", [])?;
    }
    Ok(())
}

/// Before an account goes (`blobs::on_user_deleted`): its items' name index
/// rows, which no foreign key reaches.
pub(crate) fn forget_owner(tx: &Transaction, user_id: &str) -> Result<()> {
    let ids: Vec<String> = {
        let mut st = tx.prepare("SELECT media_id FROM media_items WHERE owner_id = ?1")?;
        st.query_map(params![user_id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    forget_fts(tx, &ids)
}

/// Called by `Store::delete_session` inside its transaction, so a deleted
/// chat leaves no rows that name it. Nothing yet: the chats that carry an
/// item (`media_uses`) ship with images in chats.
pub(crate) fn on_session_deleted(_tx: &rusqlite::Transaction, _sid: &str) -> crate::Result<()> {
    Ok(())
}

impl Store {
    /// Add an item and its blob in one transaction, with the quota check
    /// (`blobs::blob_insert`). The same bytes already in Media in the same
    /// effective workspace are that item again; in another workspace they
    /// become a new item on the same blob.
    pub fn media_insert(
        &self,
        item: &NewItem,
        blob: &NewBlob,
        limits: Limits,
    ) -> Result<MediaInsert> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let (blob_id, new_blob) = match crate::blobs::blob_insert(&tx, blob, limits)? {
            BlobInsert::OverQuota(q) => return Ok(MediaInsert::OverQuota(q)),
            BlobInsert::Inserted => (blob.blob_id.clone(), true),
            BlobInsert::Existing(id) => (id, false),
        };
        if !new_blob {
            let ws = effective_ws(&tx, item.owner_id.as_deref(), item.workspace_id.as_deref())?;
            let same: Option<String> = tx
                .query_row(
                    &format!(
                        "SELECT mi.media_id FROM media_items mi
                         WHERE mi.blob_id = ?1 AND mi.owner_id IS ?2 AND {MEDIA_WS} IS ?3
                         ORDER BY mi.created_ms LIMIT 1"
                    ),
                    params![blob_id, item.owner_id, ws],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(media_id) = same {
                return Ok(MediaInsert::Duplicate { media_id });
            }
        }
        insert_item(&tx, item, &blob_id)?;
        tx.commit()?;
        Ok(MediaInsert::Added {
            media_id: item.media_id.clone(),
            new_blob,
        })
    }

    pub fn media_get(&self, media_id: &str) -> Result<Option<MediaRow>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                &format!(
                    "SELECT {} FROM media_items mi {ITEM_JOINS} WHERE mi.media_id = ?1",
                    item_cols()
                ),
                params![media_id],
                MediaRow::from_row,
            )
            .optional()?)
    }

    /// A page of items, newest first.
    pub fn media_list(&self, q: &MediaQuery) -> Result<Vec<MediaRow>> {
        let mut sql = Sql::default();
        sql.visible(q.owner.as_deref(), q.workspace.as_deref());
        if let Some(k) = &q.kind {
            let p = sql.arg(k.clone());
            sql.clauses.push(format!("{ITEM_KIND} = {p}"));
        }
        if let Some(o) = &q.origin {
            let p = sql.arg(o.clone());
            sql.clauses.push(format!("mi.origin = {p}"));
        }
        if let Some(s) = &q.session {
            let p = sql.arg(s.clone());
            sql.clauses.push(format!("mi.session_id = {p}"));
        }
        if let Some(f) = &q.fts {
            let p = sql.arg(f.clone());
            sql.clauses.push(format!(
                "mi.media_id IN (SELECT media_id FROM media_fts WHERE media_fts MATCH {p})"
            ));
        }
        if let Some((ms, id)) = &q.before {
            let (pm, pi) = (sql.arg(*ms), sql.arg(id.clone()));
            sql.clauses.push(format!(
                "(mi.created_ms < {pm} OR (mi.created_ms = {pm} AND mi.media_id < {pi}))"
            ));
        }
        let limit = sql.arg(q.limit as i64);
        let text = format!(
            "SELECT {} FROM media_items mi {ITEM_JOINS} {}
             ORDER BY mi.created_ms DESC, mi.media_id DESC LIMIT {limit}",
            item_cols(),
            sql.where_()
        );
        let conn = self.lock();
        let mut st = conn.prepare(&text)?;
        let rows = st
            .query_map(params_from_iter(sql.args), MediaRow::from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// The search panel's rows: items whose name matches `fts` (best
    /// first), or the newest ones without it. Each comes with its name,
    /// matched terms marked with `HIT_START` / `HIT_END`.
    pub fn media_search(
        &self,
        owner: Option<&str>,
        workspace: Option<&str>,
        fts: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(MediaRow, String)>> {
        let mut sql = Sql::default();
        sql.visible(owner, workspace);
        let (from, marked, order) = match fts {
            Some(f) => {
                let p = sql.arg(f.to_string());
                sql.clauses.push(format!("media_fts MATCH {p}"));
                (
                    "media_fts f JOIN media_items mi ON mi.media_id = f.media_id",
                    "highlight(media_fts, 1, char(2), char(3))",
                    "bm25(media_fts), mi.updated_ms DESC",
                )
            }
            None => (
                "media_items mi",
                "mi.name",
                "mi.updated_ms DESC, mi.media_id DESC",
            ),
        };
        let (pl, po) = (sql.arg(limit as i64), sql.arg(offset as i64));
        let text = format!(
            "SELECT {}, {marked} FROM {from} {ITEM_JOINS} {} ORDER BY {order} LIMIT {pl} OFFSET {po}",
            item_cols(),
            sql.where_()
        );
        let conn = self.lock();
        let mut st = conn.prepare(&text)?;
        let rows = st
            .query_map(params_from_iter(sql.args), |r| {
                Ok((MediaRow::from_row(r)?, r.get::<_, String>(MARKED_AT)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Rename an item (its name index follows). Returns whether it exists.
    pub fn media_rename(&self, media_id: &str, name: &str) -> Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let n = tx.execute(
            "UPDATE media_items SET name = ?2, updated_ms = ?3 WHERE media_id = ?1",
            params![media_id, name, harness_core::now_ms()],
        )?;
        if n > 0 {
            forget_fts(&tx, &[media_id.to_string()])?;
            tx.execute(
                "INSERT INTO media_fts (media_id, name) VALUES (?1, ?2)",
                params![media_id, name],
            )?;
        }
        tx.commit()?;
        Ok(n > 0)
    }

    /// Delete `owner`'s items among `ids` (`owner` None: the API token, any
    /// item), their name index rows, and the blobs nothing references any
    /// more. Never touches an artifact version an item shows.
    pub fn media_delete(&self, owner: Option<&str>, ids: &[String]) -> Result<MediaDeleted> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let mut gone = Vec::new();
        let mut blobs = Vec::new();
        for id in ids {
            let blob: Option<String> = tx
                .query_row(
                    "SELECT blob_id FROM media_items
                     WHERE media_id = ?1 AND (?2 IS NULL OR owner_id = ?2)",
                    params![id, owner],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(blob) = blob else { continue };
            tx.execute("DELETE FROM media_items WHERE media_id = ?1", params![id])?;
            gone.push(id.clone());
            blobs.push(blob);
        }
        forget_fts(&tx, &gone)?;
        let blobs = crate::blobs::release_unreferenced(&tx, &blobs)?;
        tx.commit()?;
        Ok(MediaDeleted {
            items: gone.len(),
            blobs,
        })
    }
}
