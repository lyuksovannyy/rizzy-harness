//! Built-in memory: SQLite + FTS5, plus vectors when an embedder is set.

use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

use harness_core::SessionId;
use harness_store::migrate::migrate;
use rusqlite::{Connection, OptionalExtension, params};

use crate::embed::{cosine, from_blob, to_blob};
use crate::{
    Embedder, Fact, FactKind, MemoryError, MemoryStore, NewFact, QdrantIndex, Result, Status,
};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS facts (
    id             TEXT PRIMARY KEY,
    owner_id       TEXT,
    text           TEXT NOT NULL,
    kind           TEXT NOT NULL,
    status         TEXT NOT NULL,
    source_session TEXT,
    dedupe_key     TEXT UNIQUE,
    created_ms     INTEGER NOT NULL,
    updated_ms     INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS facts_owner ON facts(owner_id);
CREATE VIRTUAL TABLE IF NOT EXISTS fact_fts USING fts5(
    id UNINDEXED, text, tokenize = 'unicode61 remove_diacritics 2'
);
-- One vector per fact and embedding model; other models' vectors are ignored.
CREATE TABLE IF NOT EXISTS fact_vec (
    id    TEXT NOT NULL,
    model TEXT NOT NULL,
    vec   BLOB NOT NULL,
    PRIMARY KEY (id, model)
);
-- Facts whose vector for a model is in the external index (Qdrant).
CREATE TABLE IF NOT EXISTS fact_indexed (
    id    TEXT NOT NULL,
    model TEXT NOT NULL,
    PRIMARY KEY (id, model)
);
"#;

/// The schema of `memory.db`, one entry per version (`harness_store::migrate`).
pub const MIGRATIONS: &[harness_store::migrate::Migration] = &[&[SCHEMA]];

pub struct SqliteMemory {
    conn: Arc<Mutex<Connection>>,
    embedder: RwLock<Option<Arc<Embedder>>>,
    /// Vectors in Qdrant instead of `fact_vec`.
    index: RwLock<Option<Arc<QdrantIndex>>>,
}

fn kind_str(k: FactKind) -> &'static str {
    match k {
        FactKind::Fact => "fact",
        FactKind::Preference => "preference",
        FactKind::Location => "location",
    }
}

fn parse_kind(s: &str) -> FactKind {
    match s {
        "preference" => FactKind::Preference,
        "location" => FactKind::Location,
        _ => FactKind::Fact,
    }
}

fn status_str(s: Status) -> &'static str {
    match s {
        Status::Proposed => "proposed",
        Status::Active => "active",
    }
}

const COLS: &str = "id, owner_id, text, kind, status, source_session, created_ms, updated_ms";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Fact> {
    let status: String = r.get(4)?;
    let source: Option<String> = r.get(5)?;
    Ok(Fact {
        id: r.get(0)?,
        owner_id: r.get(1)?,
        text: r.get(2)?,
        kind: parse_kind(&r.get::<_, String>(3)?),
        status: if status == "active" {
            Status::Active
        } else {
            Status::Proposed
        },
        source_session: source.and_then(|s| s.parse::<SessionId>().ok()),
        created_ms: r.get(6)?,
        updated_ms: r.get(7)?,
    })
}

/// Quoted OR query over the words of `q` (FTS5 syntax in `q` is inert).
fn fts_query(q: &str) -> Option<String> {
    let toks: Vec<String> = q
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.chars().count() > 2)
        .map(|t| format!("\"{}\"", t.to_lowercase()))
        .collect();
    (!toks.is_empty()).then(|| toks.join(" OR "))
}

impl SqliteMemory {
    pub fn open(path: &Path) -> Result<Self> {
        let mut conn = Connection::open(path)?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        // Before anything else writes: a newer build's file is refused as is.
        migrate(&mut conn, &path.display().to_string(), MIGRATIONS)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        migrate(&mut conn, ":memory:", MIGRATIONS)?;
        Self::init(conn)
    }

    /// Per-connection settings, on a database already at `MIGRATIONS`.
    fn init(conn: Connection) -> Result<Self> {
        // Deleted memories are overwritten, not just unlinked.
        conn.pragma_update(None, "secure_delete", "ON")?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            embedder: RwLock::new(None),
            index: RwLock::new(None),
        })
    }

    /// Keep vectors in `index` (Qdrant) rather than in `memory.db`.
    pub fn set_vector_index(&self, index: Option<Arc<QdrantIndex>>) {
        *self.index.write().unwrap_or_else(|p| p.into_inner()) = index;
    }

    fn vector_index(&self) -> Option<Arc<QdrantIndex>> {
        self.index.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Drop `id`'s vectors from the external index (best effort: recall
    /// checks every hit against `facts`, so a leftover never surfaces).
    async fn unindex(&self, id: &str) {
        if let Some(ix) = self.vector_index()
            && let Err(e) = ix.delete(&[id.to_string()]).await
        {
            tracing::warn!(error = %e, "could not remove a memory's vector from qdrant");
        }
    }

    /// `vector_recall` with the vectors in Qdrant.
    async fn indexed_recall(
        &self,
        ix: Arc<QdrantIndex>,
        emb: Arc<Embedder>,
        owner: Option<String>,
        query: &str,
    ) -> Result<Vec<(String, f32)>> {
        let model = emb.model.clone();
        let (o, m) = (owner.clone(), model.clone());
        let missing: Vec<(String, String)> = self
            .db(move |c| {
                let mut st = c.prepare(
                    "SELECT f.id, f.text FROM facts f
                     WHERE f.status = 'active' AND f.owner_id IS ?1
                       AND NOT EXISTS (SELECT 1 FROM fact_indexed v WHERE v.id = f.id AND v.model = ?2)
                     LIMIT 256",
                )?;
                let rows = st
                    .query_map(params![o, m], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?;
        for chunk in missing.chunks(64) {
            let texts: Vec<String> = chunk.iter().map(|(_, t)| t.clone()).collect();
            let vecs = emb.embed(&texts).await?;
            let points = chunk
                .iter()
                .zip(vecs)
                .map(|((id, _), v)| (id.clone(), owner.clone(), v))
                .collect();
            ix.upsert(&model, points).await?;
            let (ids, m) = (
                chunk.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(),
                model.clone(),
            );
            self.db(move |c| {
                let tx = c.transaction()?;
                for id in ids {
                    tx.execute(
                        "INSERT OR IGNORE INTO fact_indexed (id, model) VALUES (?1, ?2)",
                        params![id, m],
                    )?;
                }
                tx.commit()?;
                Ok(())
            })
            .await?;
        }
        let q = emb
            .embed(&[query.to_string()])
            .await?
            .pop()
            .unwrap_or_default();
        let hits = ix.search(owner.as_deref(), &model, &q, 100).await?;
        // Only facts that are still there, active and this owner's.
        let ids: Vec<String> = hits.iter().map(|(id, _)| id.clone()).collect();
        let live: std::collections::HashSet<String> = self
            .db(move |c| {
                let mut st = c.prepare(
                    "SELECT id FROM facts WHERE id = ?1 AND status = 'active' AND owner_id IS ?2",
                )?;
                let mut out = std::collections::HashSet::new();
                for id in ids {
                    if st.exists(params![id, owner])? {
                        out.insert(id);
                    }
                }
                Ok(out)
            })
            .await?;
        Ok(hits
            .into_iter()
            .filter(|(id, _)| live.contains(id))
            .collect())
    }

    /// Turn vector recall on (Some) or off (None). Takes effect immediately;
    /// vectors for the new model are computed lazily on the next recall.
    pub fn set_embedder(&self, e: Option<Embedder>) {
        *self.embedder.write().unwrap_or_else(|p| p.into_inner()) = e.map(Arc::new);
    }

    pub fn embedder_model(&self) -> Option<String> {
        self.embedder
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|e| e.model.clone())
    }

    async fn db<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<R> + Send + 'static,
    ) -> Result<R> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut c = conn.lock().unwrap_or_else(|p| p.into_inner());
            f(&mut c)
        })
        .await
        .map_err(|e| MemoryError::Invalid(format!("memory task failed: {e}")))?
    }

    async fn vector_recall(
        &self,
        emb: Arc<Embedder>,
        owner: Option<String>,
        query: &str,
    ) -> Result<Vec<(String, f32)>> {
        let model = emb.model.clone();
        // Facts that still need a vector for this model (backfill, batched).
        let (o, m) = (owner.clone(), model.clone());
        let missing: Vec<(String, String)> = self
            .db(move |c| {
                let mut st = c.prepare(
                    "SELECT f.id, f.text FROM facts f
                     WHERE f.status = 'active' AND f.owner_id IS ?1
                       AND NOT EXISTS (SELECT 1 FROM fact_vec v WHERE v.id = f.id AND v.model = ?2)
                     LIMIT 256",
                )?;
                let rows = st
                    .query_map(params![o, m], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?;
        for chunk in missing.chunks(64) {
            let texts: Vec<String> = chunk.iter().map(|(_, t)| t.clone()).collect();
            let vecs = emb.embed(&texts).await?;
            let rows: Vec<(String, Vec<u8>)> = chunk
                .iter()
                .zip(vecs)
                .map(|((id, _), v)| (id.clone(), to_blob(&v)))
                .collect();
            let m = model.clone();
            self.db(move |c| {
                let tx = c.transaction()?;
                for (id, blob) in rows {
                    tx.execute(
                        "INSERT OR REPLACE INTO fact_vec (id, model, vec) VALUES (?1, ?2, ?3)",
                        params![id, m, blob],
                    )?;
                }
                tx.commit()?;
                Ok(())
            })
            .await?;
        }
        let q = emb
            .embed(&[query.to_string()])
            .await?
            .pop()
            .unwrap_or_default();
        let m = model.clone();
        let all: Vec<(String, Vec<u8>)> = self
            .db(move |c| {
                let mut st = c.prepare(
                    "SELECT f.id, v.vec FROM facts f JOIN fact_vec v ON v.id = f.id AND v.model = ?2
                     WHERE f.status = 'active' AND f.owner_id IS ?1",
                )?;
                let rows = st
                    .query_map(params![owner, m], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?;
        let mut scored: Vec<(String, f32)> = all
            .into_iter()
            .map(|(id, b)| {
                let s = cosine(&q, &from_blob(&b));
                (id, s)
            })
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        Ok(scored)
    }
}

#[async_trait::async_trait]
impl MemoryStore for SqliteMemory {
    fn name(&self) -> &'static str {
        "builtin"
    }

    async fn add(&self, f: NewFact) -> Result<Option<Fact>> {
        let text = crate::clean_text(&f.text)?;
        let id = harness_core::new_id();
        let now = harness_core::now_ms();
        let fact = Fact {
            id: id.clone(),
            owner_id: f.owner_id.clone(),
            text: text.clone(),
            kind: f.kind,
            status: f.status,
            source_session: f.source_session,
            created_ms: now,
            updated_ms: now,
        };
        let out = fact.clone();
        let inserted = self
            .db(move |c| {
                let tx = c.transaction()?;
                let n = tx.execute(
                    "INSERT OR IGNORE INTO facts (id, owner_id, text, kind, status, source_session, dedupe_key, created_ms, updated_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                    params![
                        id,
                        f.owner_id,
                        text,
                        kind_str(f.kind),
                        status_str(f.status),
                        f.source_session.map(|s| s.to_string()),
                        f.dedupe_key,
                        now
                    ],
                )?;
                if n > 0 {
                    tx.execute(
                        "INSERT INTO fact_fts (id, text) VALUES (?1, ?2)",
                        params![id, text],
                    )?;
                }
                tx.commit()?;
                Ok(n > 0)
            })
            .await?;
        Ok(inserted.then_some(out))
    }

    async fn get(&self, id: &str) -> Result<Option<Fact>> {
        let id = id.to_string();
        self.db(move |c| {
            Ok(c.query_row(
                &format!("SELECT {COLS} FROM facts WHERE id = ?1"),
                params![id],
                row,
            )
            .optional()?)
        })
        .await
    }

    async fn list(&self, owner: Option<&str>) -> Result<Vec<Fact>> {
        let owner = owner.map(str::to_string);
        self.db(move |c| {
            let mut st = c.prepare(&format!(
                "SELECT {COLS} FROM facts WHERE owner_id IS ?1 ORDER BY updated_ms DESC"
            ))?;
            let rows = st
                .query_map(params![owner], row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
    }

    async fn update(
        &self,
        id: &str,
        text: Option<String>,
        status: Option<Status>,
    ) -> Result<Option<Fact>> {
        let text = text.map(|t| crate::clean_text(&t)).transpose()?;
        let changed_text = text.is_some();
        let id2 = id.to_string();
        self.db(move |c| {
            let tx = c.transaction()?;
            let now = harness_core::now_ms();
            if let Some(t) = &text {
                let n = tx.execute(
                    "UPDATE facts SET text = ?2, updated_ms = ?3 WHERE id = ?1",
                    params![id2, t, now],
                )?;
                if n > 0 {
                    tx.execute("DELETE FROM fact_fts WHERE id = ?1", params![id2])?;
                    tx.execute(
                        "INSERT INTO fact_fts (id, text) VALUES (?1, ?2)",
                        params![id2, t],
                    )?;
                    // The old vector no longer matches the text.
                    tx.execute("DELETE FROM fact_vec WHERE id = ?1", params![id2])?;
                    tx.execute("DELETE FROM fact_indexed WHERE id = ?1", params![id2])?;
                }
            }
            if let Some(s) = status {
                tx.execute(
                    "UPDATE facts SET status = ?2, updated_ms = ?3 WHERE id = ?1",
                    params![id2, status_str(s), now],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
        .await?;
        if changed_text {
            self.unindex(id).await;
        }
        self.get(id).await
    }

    async fn delete(&self, id: &str) -> Result<bool> {
        let gone = id.to_string();
        let id = id.to_string();
        let deleted = self
            .db(move |c| {
                let tx = c.transaction()?;
                let n = tx.execute("DELETE FROM facts WHERE id = ?1", params![id])?;
                tx.execute("DELETE FROM fact_fts WHERE id = ?1", params![id])?;
                tx.execute("DELETE FROM fact_vec WHERE id = ?1", params![id])?;
                tx.execute("DELETE FROM fact_indexed WHERE id = ?1", params![id])?;
                if n > 0 {
                    tx.execute("INSERT INTO fact_fts(fact_fts) VALUES('optimize')", [])?;
                }
                tx.commit()?;
                Ok(n > 0)
            })
            .await?;
        if deleted {
            self.unindex(&gone).await;
        }
        Ok(deleted)
    }

    async fn recall(&self, owner: Option<&str>, query: &str, k: usize) -> Result<Vec<Fact>> {
        let o = owner.map(str::to_string);
        // Keyword candidates (always available).
        let fts: Vec<String> = match fts_query(query) {
            None => vec![],
            Some(q) => {
                let o = o.clone();
                self.db(move |c| {
                    let mut st = c.prepare(
                        "SELECT f.id FROM fact_fts x JOIN facts f ON f.id = x.id
                         WHERE fact_fts MATCH ?1 AND f.status = 'active' AND f.owner_id IS ?2
                         ORDER BY bm25(fact_fts) LIMIT 50",
                    )?;
                    let rows = st
                        .query_map(params![q, o], |r| r.get(0))?
                        .collect::<rusqlite::Result<Vec<String>>>()?;
                    Ok(rows)
                })
                .await?
            }
        };
        let emb = self
            .embedder
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        // Vector candidates when an embedder is configured; a failing
        // endpoint degrades to keyword recall rather than failing the chat.
        let found = match (emb, self.vector_index()) {
            (None, _) => None,
            (Some(e), Some(ix)) => Some(self.indexed_recall(ix, e, o.clone(), query).await),
            (Some(e), None) => Some(self.vector_recall(e, o.clone(), query).await),
        };
        let vec: Vec<String> = match found {
            None => vec![],
            Some(found) => match found {
                Ok(v) => v
                    .into_iter()
                    .filter(|(_, s)| *s >= 0.35)
                    .take(50)
                    .map(|(id, _)| id)
                    .collect(),
                Err(err) => {
                    tracing::warn!(error = %err, "embedding recall failed; using keywords only");
                    vec![]
                }
            },
        };
        // Reciprocal-rank fusion of the two lists.
        let mut score: Vec<(String, f64)> = Vec::new();
        for list in [&fts, &vec] {
            for (rank, id) in list.iter().enumerate() {
                let add = 1.0 / (60.0 + rank as f64);
                match score.iter_mut().find(|(i, _)| i == id) {
                    Some((_, s)) => *s += add,
                    None => score.push((id.clone(), add)),
                }
            }
        }
        score.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut out = Vec::new();
        for (id, _) in score.into_iter().take(k) {
            if let Some(f) = self.get(&id).await? {
                out.push(f);
            }
        }
        Ok(out)
    }
}
