//! Artifacts: named lineages of files agents publish from agentic chats
//! (docs/artifacts.md). A lineage is keyed by (owner, effective workspace,
//! project, name); each version is one immutable file held by a blob. The
//! lineage lookup, the sequence number and the caps run in one transaction
//! (the connection mutex serialises them), and so do a version's commit,
//! retention, the name index and the lineage's Media item.

use rusqlite::types::Value;
use rusqlite::{OptionalExtension, Transaction, params, params_from_iter};

use crate::blobs::{BlobInsert, BlobRow, Limits, NewBlob, OverQuota, Preview};
use crate::{Result, Store, StoreError};

/// Run by `Store::init`, after the blob and media tables.
pub(crate) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS artifacts (
    artifact_id  TEXT PRIMARY KEY,
    workspace_id TEXT REFERENCES workspaces(workspace_id) ON DELETE SET NULL,
    owner_id     TEXT REFERENCES users(user_id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    visibility   TEXT NOT NULL DEFAULT 'private',
    created_ms   INTEGER NOT NULL,
    updated_ms   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS artifacts_ws ON artifacts(workspace_id, updated_ms DESC);
CREATE INDEX IF NOT EXISTS artifacts_owner ON artifacts(owner_id, workspace_id);
CREATE TABLE IF NOT EXISTS artifact_versions (
    version_id  TEXT PRIMARY KEY,
    artifact_id TEXT NOT NULL REFERENCES artifacts(artifact_id) ON DELETE CASCADE,
    seq         INTEGER NOT NULL,
    status      TEXT NOT NULL,
    error       TEXT,
    blob_id     TEXT REFERENCES blobs(blob_id) ON DELETE CASCADE,
    size        INTEGER NOT NULL,
    sha256      TEXT NOT NULL,
    filename    TEXT NOT NULL,
    packed      TEXT,
    by_user     INTEGER NOT NULL DEFAULT 0,
    session_id  TEXT, call_id TEXT,
    stage_id    TEXT NOT NULL,
    node_id     TEXT NOT NULL,
    meta        TEXT NOT NULL DEFAULT '{}',
    notes       TEXT NOT NULL DEFAULT '',
    pinned      INTEGER NOT NULL DEFAULT 0,
    created_ms  INTEGER NOT NULL,
    ended_ms    INTEGER,
    UNIQUE (artifact_id, seq), UNIQUE (node_id, stage_id)
);
CREATE INDEX IF NOT EXISTS artifact_versions_session ON artifact_versions(session_id);
CREATE INDEX IF NOT EXISTS artifact_versions_blob ON artifact_versions(blob_id);
CREATE INDEX IF NOT EXISTS artifact_versions_pending ON artifact_versions(status) WHERE status = 'pending';
CREATE TABLE IF NOT EXISTS artifact_projects (
    artifact_id TEXT PRIMARY KEY REFERENCES artifacts(artifact_id) ON DELETE CASCADE,
    project_id  TEXT NOT NULL REFERENCES projects(project_id) ON DELETE CASCADE);
CREATE VIRTUAL TABLE IF NOT EXISTS artifact_fts USING fts5(artifact_id UNINDEXED, name, filenames, notes,
    tokenize = 'unicode61 remove_diacritics 2');
"#;

/// An artifact's effective workspace: the one it was published in while
/// its owner is still a member there, else the owner's Personal workspace
/// (a workspace deleted under it is `SET NULL`). Needs `artifacts a`.
const ARTIFACT_WS: &str = "COALESCE(
    CASE WHEN a.workspace_id IS NOT NULL
          AND (a.owner_id IS NULL
               OR EXISTS (SELECT 1 FROM workspace_members m
                          WHERE m.workspace_id = a.workspace_id AND m.user_id = a.owner_id))
         THEN a.workspace_id END,
    (SELECT pw.workspace_id FROM workspaces pw WHERE pw.personal_owner = a.owner_id))";

/// Who sees an artifact, the list's and the search provider's rule: a
/// member of its effective workspace who owns it, or any member when it is
/// shared and still in the workspace it was shared in (one that fell back
/// to Personal is private again). `{v}` is the viewer's placeholder.
fn visible(v: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM workspace_members vm WHERE vm.workspace_id = {ARTIFACT_WS} AND vm.user_id = {v})
         AND (a.owner_id = {v} OR (a.visibility = 'workspace' AND a.workspace_id = {ARTIFACT_WS}))"
    )
}

/// A lineage that has something to show (a ready or pending version);
/// lineages whose versions all failed wait for the hourly sweep.
const LISTED: &str = "EXISTS (SELECT 1 FROM artifact_versions lv
     WHERE lv.artifact_id = a.artifact_id AND lv.status IN ('ready', 'pending'))";

const HAS_READY: &str = "EXISTS (SELECT 1 FROM artifact_versions rv
     WHERE rv.artifact_id = a.artifact_id AND rv.status = 'ready')";

/// The newest ready version's blob kind. Needs `artifacts a`.
const LATEST_KIND: &str =
    "(SELECT kb.kind FROM artifact_versions kv JOIN blobs kb ON kb.blob_id = kv.blob_id
     WHERE kv.artifact_id = a.artifact_id AND kv.status = 'ready' ORDER BY kv.seq DESC LIMIT 1)";

fn artifact_cols() -> String {
    format!(
        "a.artifact_id, a.owner_id, u.username, a.workspace_id, {ARTIFACT_WS}, a.name, a.visibility,
         ap.project_id, a.created_ms, a.updated_ms,
         (SELECT COUNT(*) FROM artifact_versions cv WHERE cv.artifact_id = a.artifact_id AND cv.status = 'ready'),
         (SELECT mi.media_id FROM media_items mi WHERE mi.artifact_id = a.artifact_id)"
    )
}

const ARTIFACT_JOINS: &str = "LEFT JOIN users u ON u.user_id = a.owner_id
     LEFT JOIN artifact_projects ap ON ap.artifact_id = a.artifact_id";

const VERSION_COLS: &str =
    "v.version_id, v.artifact_id, v.seq, v.status, v.error, v.blob_id, v.size,
     v.sha256, v.filename, v.packed, v.by_user, v.session_id, v.call_id, v.stage_id, v.node_id,
     v.meta, v.notes, v.pinned, v.created_ms, v.ended_ms,
     b.blob_id, b.owner_id, b.sha256, b.size, b.kind, b.format, b.mime, b.display, b.created_ms,
     p.width, p.height, p.duration_ms, p.thumb IS NOT NULL, p.no_thumb";

const VERSION_JOINS: &str = "LEFT JOIN blobs b ON b.blob_id = v.blob_id
     LEFT JOIN blob_previews p ON p.blob_id = v.blob_id";

/// Failed versions kept per artifact, and for how long at most.
const KEEP_FAILED: usize = 3;
const FAILED_TTL_MS: i64 = 7 * 24 * 3600 * 1000;

/// One lineage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactRow {
    pub artifact_id: String,
    pub owner_id: Option<String>,
    pub owner_name: Option<String>,
    /// Where it was published (`None` once that workspace was deleted).
    pub workspace_id: Option<String>,
    /// `ARTIFACT_WS`.
    pub effective_ws: Option<String>,
    pub name: String,
    /// `private` | `workspace`.
    pub visibility: String,
    pub project_id: Option<String>,
    pub created_ms: i64,
    pub updated_ms: i64,
    /// Ready versions.
    pub ready: u64,
    /// The lineage's Media item, if any.
    pub media_id: Option<String>,
}

impl ArtifactRow {
    fn from_row(r: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            artifact_id: r.get(0)?,
            owner_id: r.get(1)?,
            owner_name: r.get(2)?,
            workspace_id: r.get(3)?,
            effective_ws: r.get(4)?,
            name: r.get(5)?,
            visibility: r.get(6)?,
            project_id: r.get(7)?,
            created_ms: r.get(8)?,
            updated_ms: r.get(9)?,
            ready: r.get::<_, i64>(10)?.max(0) as u64,
            media_id: r.get(11)?,
        })
    }

    /// Shared with its workspace, and still in it.
    pub fn shared(&self) -> bool {
        self.visibility == "workspace" && self.workspace_id == self.effective_ws
    }
}

/// One version, with its blob once it is ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRow {
    pub version_id: String,
    pub artifact_id: String,
    pub seq: u64,
    /// `pending` | `ready` | `failed`.
    pub status: String,
    pub error: Option<String>,
    pub size: u64,
    pub sha256: String,
    pub filename: String,
    pub packed: Option<String>,
    pub by_user: bool,
    pub session_id: Option<String>,
    pub call_id: Option<String>,
    pub stage_id: String,
    pub node_id: String,
    /// JSON, cleaned at ingest.
    pub meta: String,
    pub notes: String,
    pub pinned: bool,
    pub created_ms: i64,
    pub ended_ms: Option<i64>,
    pub blob: Option<BlobRow>,
    pub preview: Preview,
}

impl VersionRow {
    fn from_row(r: &rusqlite::Row) -> rusqlite::Result<Self> {
        let has_blob = r.get::<_, Option<String>>(20)?.is_some();
        Ok(Self {
            version_id: r.get(0)?,
            artifact_id: r.get(1)?,
            seq: r.get::<_, i64>(2)?.max(0) as u64,
            status: r.get(3)?,
            error: r.get(4)?,
            size: r.get::<_, i64>(6)?.max(0) as u64,
            sha256: r.get(7)?,
            filename: r.get(8)?,
            packed: r.get(9)?,
            by_user: r.get(10)?,
            session_id: r.get(11)?,
            call_id: r.get(12)?,
            stage_id: r.get(13)?,
            node_id: r.get(14)?,
            meta: r.get(15)?,
            notes: r.get(16)?,
            pinned: r.get(17)?,
            created_ms: r.get(18)?,
            ended_ms: r.get(19)?,
            blob: if has_blob {
                Some(BlobRow::from_row(r, 20)?)
            } else {
                None
            },
            preview: crate::blobs::preview_at(r, 29)?,
        })
    }
}

/// A version an ingest records, from the publishing session's own row.
#[derive(Debug, Clone, Default)]
pub struct NewVersion {
    pub owner_id: String,
    pub workspace_id: String,
    pub project_id: Option<String>,
    pub name: String,
    pub node_id: String,
    pub stage_id: String,
    pub size: u64,
    pub sha256: String,
    pub filename: String,
    pub packed: Option<String>,
    pub by_user: bool,
    pub session_id: Option<String>,
    pub call_id: Option<String>,
    pub meta: String,
    pub notes: String,
}

/// Caps an ingest checks (lineages count only those with a ready version).
#[derive(Debug, Clone, Copy)]
pub struct IngestCaps {
    pub pending_per_node: u64,
    pub per_owner: u64,
    pub per_workspace: u64,
}

/// What `artifact_ingest` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ingested {
    /// This node's stage is already recorded (a replayed event).
    Duplicate,
    Added {
        artifact_id: String,
        version_id: String,
        seq: u64,
        /// `Some(reason)` when it was recorded as failed.
        failed: Option<String>,
    },
}

/// What `artifact_commit` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Committed {
    /// The version is no longer pending (failed, deleted): the file is not
    /// needed.
    NotPending,
    /// It did not fit; nothing changed (the caller fails the version).
    OverQuota(OverQuota),
    Ready {
        /// The written file became a new blob (false: a dedupe hit).
        new_blob: bool,
        /// Blobs nothing references any more: the master unlinks them.
        gone: Vec<String>,
        /// The lineage's Media item was added or now shows this version.
        media: bool,
    },
}

/// A pending version, for the transfer that pulls it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub version_id: String,
    pub artifact_id: String,
    pub owner_id: Option<String>,
    pub node_id: String,
    pub session_id: Option<String>,
    pub stage_id: String,
    pub size: u64,
    pub sha256: String,
    pub filename: String,
    pub created_ms: i64,
}

/// A list call's filters; `viewer` and `workspace` are the caller's
/// visibility (`None`: the API token / every workspace).
#[derive(Debug, Clone, Default)]
pub struct ArtifactQuery {
    pub viewer: Option<String>,
    pub workspace: Option<String>,
    pub project: Option<String>,
    /// Blob kinds of the newest ready version.
    pub kinds: Option<Vec<String>>,
    pub mine: bool,
    /// A prepared FTS5 query (quoted tokens only).
    pub fts: Option<String>,
    /// Lineages older than this `(updated_ms, artifact_id)`.
    pub before: Option<(i64, String)>,
    pub limit: usize,
}

/// What the hourly sweep did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Swept {
    /// Pending versions that ran out of time, now failed: `(node_id,
    /// session_id, stage_id)` for the stage release.
    pub expired: Vec<(String, Option<String>, String)>,
    pub rows: usize,
    pub gone: Vec<String>,
}

#[derive(Default)]
struct Sql {
    clauses: Vec<String>,
    args: Vec<Value>,
}

impl Sql {
    fn arg(&mut self, v: impl Into<Value>) -> String {
        self.args.push(v.into());
        format!("?{}", self.args.len())
    }

    fn visible(&mut self, viewer: Option<&str>, ws: Option<&str>) {
        if let Some(w) = ws {
            let p = self.arg(w.to_string());
            self.clauses.push(format!("{ARTIFACT_WS} = {p}"));
        }
        if let Some(v) = viewer {
            let p = self.arg(v.to_string());
            self.clauses.push(visible(&p));
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

fn now() -> i64 {
    harness_core::now_ms()
}

/// The lineage `(owner, workspace, project, name)`, by effective workspace.
fn lineage(tx: &Transaction, v: &NewVersion) -> Result<Option<String>> {
    Ok(tx
        .query_row(
            &format!(
                "SELECT a.artifact_id FROM artifacts a
                 LEFT JOIN artifact_projects ap ON ap.artifact_id = a.artifact_id
                 WHERE a.owner_id = ?1 AND {ARTIFACT_WS} = ?2 AND a.name = ?3 AND ap.project_id IS ?4
                 ORDER BY a.created_ms LIMIT 1"
            ),
            params![v.owner_id, v.workspace_id, v.name, v.project_id],
            |r| r.get(0),
        )
        .optional()?)
}

fn count(tx: &Transaction, sql: &str, p: impl rusqlite::Params) -> Result<u64> {
    let n: i64 = tx.query_row(sql, p, |r| r.get(0))?;
    Ok(n.max(0) as u64)
}

/// Why a new lineage may not be started, if it may not.
fn lineage_cap(tx: &Transaction, v: &NewVersion, caps: IngestCaps) -> Result<Option<String>> {
    let mine = count(
        tx,
        &format!(
            "SELECT COUNT(*) FROM artifacts a WHERE a.owner_id = ?1 AND {ARTIFACT_WS} = ?2 AND {HAS_READY}"
        ),
        params![v.owner_id, v.workspace_id],
    )?;
    if mine >= caps.per_owner {
        return Ok(Some(format!(
            "you have {mine} artifacts in this workspace; delete some to publish new ones"
        )));
    }
    let all = count(
        tx,
        &format!("SELECT COUNT(*) FROM artifacts a WHERE {ARTIFACT_WS} = ?1 AND {HAS_READY}"),
        params![v.workspace_id],
    )?;
    if all >= caps.per_workspace {
        return Ok(Some(format!(
            "this workspace holds {all} artifacts; delete some to publish new ones"
        )));
    }
    Ok(None)
}

fn create_lineage(tx: &Transaction, v: &NewVersion) -> Result<String> {
    let id = harness_core::new_id();
    let t = now();
    tx.execute(
        "INSERT INTO artifacts (artifact_id, workspace_id, owner_id, name, visibility, created_ms, updated_ms)
         VALUES (?1, ?2, ?3, ?4, 'private', ?5, ?5)",
        params![id, v.workspace_id, v.owner_id, v.name, t],
    )?;
    if let Some(p) = &v.project_id {
        tx.execute(
            "INSERT INTO artifact_projects (artifact_id, project_id) VALUES (?1, ?2)",
            params![id, p],
        )?;
    }
    Ok(id)
}

/// Rewrite an artifact's name index row from its live versions (deleted
/// versions' file names and notes leave the index with them).
fn sync_fts(tx: &Transaction, artifact_id: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM artifact_fts WHERE artifact_id = ?1",
        params![artifact_id],
    )?;
    tx.execute(
        "INSERT INTO artifact_fts (artifact_id, name, filenames, notes)
         SELECT a.artifact_id, a.name,
                COALESCE((SELECT group_concat(filename, ' ') FROM (SELECT DISTINCT filename FROM artifact_versions
                          WHERE artifact_id = a.artifact_id AND status IN ('ready', 'pending'))), ''),
                COALESCE((SELECT group_concat(notes, ' ') FROM artifact_versions
                          WHERE artifact_id = a.artifact_id AND status IN ('ready', 'pending') AND notes != ''), '')
         FROM artifacts a WHERE a.artifact_id = ?1",
        params![artifact_id],
    )?;
    Ok(())
}

fn fts_optimize(tx: &Transaction) -> Result<()> {
    tx.execute(
        "INSERT INTO artifact_fts(artifact_fts) VALUES('optimize')",
        [],
    )?;
    Ok(())
}

/// Delete versions by id; returns their blobs (candidates to release).
fn delete_versions(tx: &Transaction, ids: &[(String, Option<String>)]) -> Result<Vec<String>> {
    let mut blobs = Vec::new();
    for (id, blob) in ids {
        tx.execute(
            "DELETE FROM artifact_versions WHERE version_id = ?1",
            params![id],
        )?;
        blobs.extend(blob.clone());
    }
    Ok(blobs)
}

/// Keep the newest `keep` ready unpinned versions (pinned ones always),
/// and the newest `KEEP_FAILED` failed ones younger than a week. Returns
/// the deleted versions' blobs.
fn retain(tx: &Transaction, artifact_id: &str, keep: usize) -> Result<Vec<String>> {
    let rows = |sql: &str| -> Result<Vec<(String, Option<String>, i64)>> {
        let mut st = tx.prepare(sql)?;
        let v = st
            .query_map(params![artifact_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    };
    let ready = rows(
        "SELECT version_id, blob_id, created_ms FROM artifact_versions
         WHERE artifact_id = ?1 AND status = 'ready' AND pinned = 0 ORDER BY seq DESC",
    )?;
    let failed = rows(
        "SELECT version_id, blob_id, created_ms FROM artifact_versions
         WHERE artifact_id = ?1 AND status = 'failed' ORDER BY seq DESC",
    )?;
    let old = now() - FAILED_TTL_MS;
    let gone: Vec<(String, Option<String>)> = ready
        .into_iter()
        .skip(keep)
        .chain(
            failed
                .into_iter()
                .enumerate()
                .filter(|(i, v)| *i >= KEEP_FAILED || v.2 < old)
                .map(|(_, v)| v),
        )
        .map(|(id, blob, _)| (id, blob))
        .collect();
    delete_versions(tx, &gone)
}

/// Delete a lineage and its name index row; returns its versions' blobs.
fn drop_lineage(tx: &Transaction, artifact_id: &str) -> Result<Vec<String>> {
    let blobs: Vec<String> = {
        let mut st = tx.prepare(
            "SELECT blob_id FROM artifact_versions WHERE artifact_id = ?1 AND blob_id IS NOT NULL",
        )?;
        st.query_map(params![artifact_id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    tx.execute(
        "DELETE FROM artifacts WHERE artifact_id = ?1",
        params![artifact_id],
    )?;
    tx.execute(
        "DELETE FROM artifact_fts WHERE artifact_id = ?1",
        params![artifact_id],
    )?;
    Ok(blobs)
}

/// Does the lineage still have a ready or pending version?
fn alive(tx: &Transaction, artifact_id: &str) -> Result<bool> {
    Ok(tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM artifact_versions
                        WHERE artifact_id = ?1 AND status IN ('ready', 'pending'))",
        params![artifact_id],
        |r| r.get(0),
    )?)
}

/// Before an account goes (`blobs::on_user_deleted`): its artifacts' name
/// index rows, which no foreign key reaches.
pub(crate) fn forget_owner(tx: &Transaction, user_id: &str) -> Result<()> {
    let n = tx.execute(
        "DELETE FROM artifact_fts WHERE artifact_id IN
             (SELECT artifact_id FROM artifacts WHERE owner_id = ?1)",
        params![user_id],
    )?;
    if n > 0 {
        fts_optimize(tx)?;
    }
    Ok(())
}

impl Store {
    /// Record a staged file as a pending version of its lineage (created
    /// if needed), or as a failed one when `fail` says why or a cap is
    /// reached. Idempotent per `(node_id, stage_id)`.
    pub fn artifact_ingest(
        &self,
        v: &NewVersion,
        fail: Option<&str>,
        caps: IngestCaps,
    ) -> Result<Ingested> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let dup: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM artifact_versions WHERE node_id = ?1 AND stage_id = ?2)",
            params![v.node_id, v.stage_id],
            |r| r.get(0),
        )?;
        if dup {
            return Ok(Ingested::Duplicate);
        }
        let mut failed = fail.map(str::to_string);
        let artifact_id = match lineage(&tx, v)? {
            Some(id) => id,
            None => {
                if failed.is_none() {
                    failed = lineage_cap(&tx, v, caps)?;
                }
                create_lineage(&tx, v)?
            }
        };
        if failed.is_none() {
            let pending = count(
                &tx,
                "SELECT COUNT(*) FROM artifact_versions WHERE node_id = ?1 AND status = 'pending'",
                params![v.node_id],
            )?;
            if pending >= caps.pending_per_node {
                failed = Some(format!(
                    "the worker node already has {pending} files waiting to upload; try again later"
                ));
            }
        }
        let seq = count(
            &tx,
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM artifact_versions WHERE artifact_id = ?1",
            params![artifact_id],
        )?;
        let version_id = harness_core::new_id();
        let t = now();
        tx.execute(
            "INSERT INTO artifact_versions (version_id, artifact_id, seq, status, error, size, sha256,
                 filename, packed, by_user, session_id, call_id, stage_id, node_id, meta, notes,
                 created_ms, ended_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                version_id,
                artifact_id,
                seq as i64,
                if failed.is_some() { "failed" } else { "pending" },
                failed,
                v.size as i64,
                v.sha256,
                v.filename,
                v.packed,
                v.by_user,
                v.session_id,
                v.call_id,
                v.stage_id,
                v.node_id,
                v.meta,
                v.notes,
                t,
                failed.as_ref().map(|_| t),
            ],
        )?;
        tx.execute(
            "UPDATE artifacts SET updated_ms = ?2 WHERE artifact_id = ?1",
            params![artifact_id, t],
        )?;
        sync_fts(&tx, &artifact_id)?;
        tx.commit()?;
        Ok(Ingested::Added {
            artifact_id,
            version_id,
            seq,
            failed,
        })
    }

    /// A pulled file arrived whole and verified: record its blob (quota
    /// checked here, authoritatively), make the version ready, apply
    /// retention, rewrite the name index, and point the lineage's Media
    /// item at it for image, video and audio, all in one transaction.
    pub fn artifact_commit(
        &self,
        version_id: &str,
        blob: &NewBlob,
        limits: Limits,
        keep: usize,
    ) -> Result<Committed> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let row: Option<(String, Option<String>)> = tx
            .query_row(
                "SELECT artifact_id, session_id FROM artifact_versions
                 WHERE version_id = ?1 AND status = 'pending'",
                params![version_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((artifact_id, session_id)) = row else {
            return Ok(Committed::NotPending);
        };
        let (blob_id, new_blob) = match crate::blobs::blob_insert(&tx, blob, limits)? {
            // Nothing written; the master fails the version with its
            // own words for the limit.
            BlobInsert::OverQuota(q) => return Ok(Committed::OverQuota(q)),
            BlobInsert::Inserted => (blob.blob_id.clone(), true),
            BlobInsert::Existing(id) => (id, false),
        };
        let t = now();
        tx.execute(
            "UPDATE artifact_versions SET status = 'ready', error = NULL, blob_id = ?2, ended_ms = ?3
             WHERE version_id = ?1",
            params![version_id, blob_id, t],
        )?;
        tx.execute(
            "UPDATE artifacts SET updated_ms = ?2 WHERE artifact_id = ?1",
            params![artifact_id, t],
        )?;
        let mut candidates = retain(&tx, &artifact_id, keep)?;
        let kind: String = tx.query_row(
            "SELECT kind FROM blobs WHERE blob_id = ?1",
            params![blob_id],
            |r| r.get(0),
        )?;
        let media = if matches!(kind.as_str(), "image" | "video" | "audio") {
            let old = crate::media::upsert_agent(
                &tx,
                &crate::media::AgentItem {
                    artifact_id: &artifact_id,
                    version_id,
                    blob_id: &blob_id,
                    session_id: session_id.as_deref(),
                },
            )?;
            candidates.extend(old);
            true
        } else {
            false
        };
        sync_fts(&tx, &artifact_id)?;
        let gone = crate::blobs::release_unreferenced(&tx, &candidates)?;
        tx.commit()?;
        Ok(Committed::Ready {
            new_blob,
            gone,
            media,
        })
    }

    /// Fail a pending version with `reason`. Returns whether it was
    /// pending.
    pub fn artifact_fail(&self, version_id: &str, reason: &str) -> Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let artifact: Option<String> = tx
            .query_row(
                "SELECT artifact_id FROM artifact_versions WHERE version_id = ?1 AND status = 'pending'",
                params![version_id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(artifact_id) = artifact else {
            return Ok(false);
        };
        fail_in(&tx, version_id, reason)?;
        sync_fts(&tx, &artifact_id)?;
        tx.commit()?;
        Ok(true)
    }

    /// Pending versions (of one node, or all).
    pub fn artifact_pending(&self, node_id: Option<&str>) -> Result<Vec<Pending>> {
        let conn = self.lock();
        let mut st = conn.prepare(
            "SELECT v.version_id, v.artifact_id, a.owner_id, v.node_id, v.session_id, v.stage_id,
                    v.size, v.sha256, v.filename, v.created_ms
             FROM artifact_versions v JOIN artifacts a ON a.artifact_id = v.artifact_id
             WHERE v.status = 'pending' AND (?1 IS NULL OR v.node_id = ?1)
             ORDER BY v.created_ms",
        )?;
        let rows = st
            .query_map(params![node_id], |r| {
                Ok(Pending {
                    version_id: r.get(0)?,
                    artifact_id: r.get(1)?,
                    owner_id: r.get(2)?,
                    node_id: r.get(3)?,
                    session_id: r.get(4)?,
                    stage_id: r.get(5)?,
                    size: r.get::<_, i64>(6)?.max(0) as u64,
                    sha256: r.get(7)?,
                    filename: r.get(8)?,
                    created_ms: r.get(9)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn artifact_get(&self, artifact_id: &str) -> Result<Option<ArtifactRow>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                &format!(
                    "SELECT {} FROM artifacts a {ARTIFACT_JOINS} WHERE a.artifact_id = ?1",
                    artifact_cols()
                ),
                params![artifact_id],
                ArtifactRow::from_row,
            )
            .optional()?)
    }

    /// A lineage's versions, newest first.
    pub fn artifact_versions(&self, artifact_id: &str) -> Result<Vec<VersionRow>> {
        self.versions_where("v.artifact_id = ?1 ORDER BY v.seq DESC", artifact_id)
    }

    /// The versions a chat staged, oldest first (its cards and failed
    /// "Save as artifact" items).
    pub fn artifact_session_versions(&self, session_id: &str) -> Result<Vec<VersionRow>> {
        self.versions_where("v.session_id = ?1 ORDER BY v.created_ms", session_id)
    }

    pub fn artifact_version(&self, version_id: &str) -> Result<Option<VersionRow>> {
        Ok(self
            .versions_where("v.version_id = ?1", version_id)?
            .into_iter()
            .next())
    }

    fn versions_where(&self, tail: &str, arg: &str) -> Result<Vec<VersionRow>> {
        let conn = self.lock();
        let mut st = conn.prepare(&format!(
            "SELECT {VERSION_COLS} FROM artifact_versions v {VERSION_JOINS} WHERE {tail}"
        ))?;
        let rows = st
            .query_map(params![arg], VersionRow::from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// The version a lineage shows: its newest ready one, else its newest
    /// of any status (a first upload still pending).
    pub fn artifact_latest(&self, artifact_id: &str) -> Result<Option<VersionRow>> {
        let v = self.artifact_versions(artifact_id)?;
        let ready = v.iter().find(|x| x.status == "ready").cloned();
        Ok(ready.or_else(|| v.into_iter().next()))
    }

    /// A page of lineages, most recently changed first.
    pub fn artifact_list(&self, q: &ArtifactQuery) -> Result<Vec<ArtifactRow>> {
        let mut sql = Sql::default();
        sql.clauses.push(LISTED.into());
        sql.visible(q.viewer.as_deref(), q.workspace.as_deref());
        if q.mine
            && let Some(v) = &q.viewer
        {
            let p = sql.arg(v.clone());
            sql.clauses.push(format!("a.owner_id = {p}"));
        }
        if let Some(pr) = &q.project {
            let p = sql.arg(pr.clone());
            sql.clauses.push(format!("ap.project_id = {p}"));
        }
        if let Some(kinds) = &q.kinds {
            let ps: Vec<String> = kinds.iter().map(|k| sql.arg(k.clone())).collect();
            sql.clauses
                .push(format!("{LATEST_KIND} IN ({})", ps.join(", ")));
        }
        if let Some(f) = &q.fts {
            let p = sql.arg(f.clone());
            sql.clauses.push(format!(
                "a.artifact_id IN (SELECT artifact_id FROM artifact_fts WHERE artifact_fts MATCH {p})"
            ));
        }
        if let Some((ms, id)) = &q.before {
            let (pm, pi) = (sql.arg(*ms), sql.arg(id.clone()));
            sql.clauses.push(format!(
                "(a.updated_ms < {pm} OR (a.updated_ms = {pm} AND a.artifact_id < {pi}))"
            ));
        }
        let limit = sql.arg(q.limit as i64);
        let text = format!(
            "SELECT {} FROM artifacts a {ARTIFACT_JOINS} {}
             ORDER BY a.updated_ms DESC, a.artifact_id DESC LIMIT {limit}",
            artifact_cols(),
            sql.where_()
        );
        let conn = self.lock();
        let mut st = conn.prepare(&text)?;
        let rows = st
            .query_map(params_from_iter(sql.args), ArtifactRow::from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// The search panel's rows: lineages whose name, file names or notes
    /// match `fts` (best first), or the newest without it, with the name
    /// marked (`HIT_START` / `HIT_END`). Same visibility as the list.
    pub fn artifact_search(
        &self,
        viewer: Option<&str>,
        workspace: Option<&str>,
        fts: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(ArtifactRow, String)>> {
        let mut sql = Sql::default();
        sql.clauses.push(LISTED.into());
        sql.visible(viewer, workspace);
        let (from, marked, order) = match fts {
            Some(f) => {
                let p = sql.arg(f.to_string());
                sql.clauses.push(format!("artifact_fts MATCH {p}"));
                (
                    "artifact_fts f JOIN artifacts a ON a.artifact_id = f.artifact_id",
                    "highlight(artifact_fts, 1, char(2), char(3))",
                    "bm25(artifact_fts), a.updated_ms DESC",
                )
            }
            None => (
                "artifacts a",
                "a.name",
                "a.updated_ms DESC, a.artifact_id DESC",
            ),
        };
        let (pl, po) = (sql.arg(limit as i64), sql.arg(offset as i64));
        let text = format!(
            "SELECT {}, {marked} FROM {from} {ARTIFACT_JOINS} {} ORDER BY {order} LIMIT {pl} OFFSET {po}",
            artifact_cols(),
            sql.where_()
        );
        let conn = self.lock();
        let mut st = conn.prepare(&text)?;
        let rows = st
            .query_map(params_from_iter(sql.args), |r| {
                Ok((ArtifactRow::from_row(r)?, r.get::<_, String>(12)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// How many lineages `viewer` sees in `workspace` (for `features`).
    pub fn artifact_count_visible(
        &self,
        viewer: Option<&str>,
        workspace: Option<&str>,
    ) -> Result<u64> {
        let mut sql = Sql::default();
        sql.clauses.push(LISTED.into());
        sql.visible(viewer, workspace);
        let conn = self.lock();
        let n: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM artifacts a {}", sql.where_()),
            params_from_iter(sql.args),
            |r| r.get(0),
        )?;
        Ok(n.max(0) as u64)
    }

    /// Rename and/or share a lineage. A name another lineage of the same
    /// owner, workspace and project has is a `Conflict`.
    pub fn artifact_update(
        &self,
        artifact_id: &str,
        name: Option<&str>,
        visibility: Option<&str>,
    ) -> Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let t = now();
        if let Some(n) = name {
            let clash: bool = tx.query_row(
                &format!(
                    "SELECT EXISTS (SELECT 1 FROM artifacts a
                         LEFT JOIN artifact_projects ap ON ap.artifact_id = a.artifact_id
                         JOIN artifacts me ON me.artifact_id = ?1
                         LEFT JOIN artifact_projects mp ON mp.artifact_id = me.artifact_id
                         WHERE a.artifact_id != me.artifact_id AND a.owner_id IS me.owner_id
                           AND a.name = ?2 AND ap.project_id IS mp.project_id
                           AND {ARTIFACT_WS} IS (SELECT {} FROM artifacts a WHERE a.artifact_id = ?1))",
                    ARTIFACT_WS
                ),
                params![artifact_id, n],
                |r| r.get(0),
            )?;
            if clash {
                return Err(StoreError::Conflict(format!("an artifact named {n:?}")));
            }
            tx.execute(
                "UPDATE artifacts SET name = ?2, updated_ms = ?3 WHERE artifact_id = ?1",
                params![artifact_id, n, t],
            )?;
        }
        if let Some(v) = visibility {
            tx.execute(
                "UPDATE artifacts SET visibility = ?2 WHERE artifact_id = ?1",
                params![artifact_id, v],
            )?;
        }
        let exists: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM artifacts WHERE artifact_id = ?1)",
            params![artifact_id],
            |r| r.get(0),
        )?;
        if exists && name.is_some() {
            sync_fts(&tx, artifact_id)?;
        }
        tx.commit()?;
        Ok(exists)
    }

    /// Delete a lineage with its versions; returns the blobs nothing
    /// references any more (its Media item, if any, keeps its own).
    pub fn artifact_delete(&self, artifact_id: &str) -> Result<Vec<String>> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let blobs = drop_lineage(&tx, artifact_id)?;
        fts_optimize(&tx)?;
        let gone = crate::blobs::release_unreferenced(&tx, &blobs)?;
        tx.commit()?;
        Ok(gone)
    }

    /// Delete one version; a lineage left with no ready and no pending
    /// version goes with it. `None` when there is no such version.
    pub fn artifact_version_delete(
        &self,
        artifact_id: &str,
        version_id: &str,
    ) -> Result<Option<Vec<String>>> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let blob: Option<Option<String>> = tx
            .query_row(
                "SELECT blob_id FROM artifact_versions WHERE version_id = ?1 AND artifact_id = ?2",
                params![version_id, artifact_id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(blob) = blob else {
            return Ok(None);
        };
        let mut candidates = delete_versions(&tx, &[(version_id.to_string(), blob)])?;
        if alive(&tx, artifact_id)? {
            sync_fts(&tx, artifact_id)?;
        } else {
            candidates.extend(drop_lineage(&tx, artifact_id)?);
            fts_optimize(&tx)?;
        }
        let gone = crate::blobs::release_unreferenced(&tx, &candidates)?;
        tx.commit()?;
        Ok(Some(gone))
    }

    /// Pin or unpin a ready version; more than `max` pinned per artifact
    /// is a `Conflict`. Returns whether the version exists.
    pub fn artifact_pin(
        &self,
        artifact_id: &str,
        version_id: &str,
        pinned: bool,
        max: u64,
    ) -> Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        if pinned {
            let n = count(
                &tx,
                "SELECT COUNT(*) FROM artifact_versions
                 WHERE artifact_id = ?1 AND pinned = 1 AND version_id != ?2",
                params![artifact_id, version_id],
            )?;
            if n >= max {
                return Err(StoreError::Conflict(format!(
                    "at most {max} pinned versions"
                )));
            }
        }
        let n = tx.execute(
            "UPDATE artifact_versions SET pinned = ?3 WHERE version_id = ?1 AND artifact_id = ?2",
            params![version_id, artifact_id, pinned],
        )?;
        tx.commit()?;
        Ok(n > 0)
    }

    /// Fail every pending version of `node_id` (revoked, or too old to
    /// publish). Returns the versions failed.
    pub fn artifact_fail_node(&self, node_id: &str, reason: &str) -> Result<Vec<String>> {
        let ids: Vec<String> = self
            .artifact_pending(Some(node_id))?
            .into_iter()
            .map(|p| p.version_id)
            .collect();
        let mut failed = Vec::new();
        for id in ids {
            if self.artifact_fail(&id, reason)? {
                failed.push(id);
            }
        }
        Ok(failed)
    }

    /// Blobs ready versions hold, for the start-up check of their files.
    pub fn artifact_ready_blobs(&self) -> Result<Vec<String>> {
        let conn = self.lock();
        let mut st = conn.prepare(
            "SELECT DISTINCT blob_id FROM artifact_versions WHERE status = 'ready' AND blob_id IS NOT NULL",
        )?;
        let ids = st
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(ids)
    }

    /// Ready versions whose file is gone become failed; returns how many.
    pub fn artifact_fail_missing(&self, blob_ids: &[String]) -> Result<usize> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let mut n = 0;
        for id in blob_ids {
            n += tx.execute(
                "UPDATE artifact_versions SET status = 'failed', error = 'the stored file is missing',
                     blob_id = NULL, ended_ms = ?2
                 WHERE blob_id = ?1 AND status = 'ready'",
                params![id, now()],
            )?;
        }
        tx.commit()?;
        Ok(n)
    }

    /// The hourly part of Artifacts: pending versions older than
    /// `stage_ttl_ms` fail, failed versions beyond retention go, lineages
    /// with no ready and no pending version go, then the unreferenced
    /// blobs.
    pub fn artifacts_hourly(&self, stage_ttl_ms: i64) -> Result<Swept> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let t = now();
        let mut out = Swept::default();
        {
            let mut st = tx.prepare(
                "SELECT version_id, node_id, session_id, stage_id FROM artifact_versions
                 WHERE status = 'pending' AND created_ms < ?1",
            )?;
            let stale = st
                .query_map(params![t - stage_ttl_ms], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (vid, node, sid, stage) in stale {
                fail_in(&tx, &vid, "the node never sent the file")?;
                out.expired.push((node, sid, stage));
            }
        }
        let ids: Vec<String> = {
            let mut st = tx.prepare("SELECT artifact_id FROM artifacts")?;
            st.query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        let mut candidates = Vec::new();
        for id in ids {
            let before = count(
                &tx,
                "SELECT COUNT(*) FROM artifact_versions WHERE artifact_id = ?1",
                params![id],
            )?;
            // Failed retention only: ready versions keep their own rule,
            // applied when one becomes ready.
            candidates.extend(retain(&tx, &id, usize::MAX)?);
            if alive(&tx, &id)? {
                let after = count(
                    &tx,
                    "SELECT COUNT(*) FROM artifact_versions WHERE artifact_id = ?1",
                    params![id],
                )?;
                if after != before {
                    sync_fts(&tx, &id)?;
                }
                out.rows += (before - after) as usize;
            } else {
                candidates.extend(drop_lineage(&tx, &id)?);
                out.rows += before as usize + 1;
            }
        }
        if out.rows > 0 || !out.expired.is_empty() {
            fts_optimize(&tx)?;
        }
        out.gone = crate::blobs::release_unreferenced(&tx, &candidates)?;
        tx.commit()?;
        Ok(out)
    }
}

fn fail_in(tx: &Transaction, version_id: &str, reason: &str) -> Result<()> {
    tx.execute(
        "UPDATE artifact_versions SET status = 'failed', error = ?2, ended_ms = ?3
         WHERE version_id = ?1 AND status = 'pending'",
        params![version_id, reason, now()],
    )?;
    Ok(())
}
