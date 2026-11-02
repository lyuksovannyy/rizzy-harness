//! SQLite event log. The same schema serves the node's authoritative log and
//! the master's mirror; only durability differs. See PLAN.md §2–3.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use harness_core::{RawEvent, SessionId};
use rusqlite::{Connection, OptionalExtension, params};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("session {session_id} is owned by {owner}, not {claimed}")]
    WrongOwner {
        session_id: SessionId,
        owner: String,
        claimed: String,
    },
    #[error("corrupt row: {0}")]
    Corrupt(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Durability {
    /// fsync every commit. Required for the authoritative (node) log.
    Full,
    /// WAL + NORMAL. Fine for a mirror that can re-fetch.
    Normal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendOutcome {
    Applied,
    /// `seq <= head`: already stored. Idempotent no-op.
    Duplicate,
    /// `seq > head + 1`: not stored; the caller should resume from `head`.
    Gap {
        head: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    pub session_id: SessionId,
    pub node_id: String,
    pub title: String,
    pub cwd: String,
    pub model: String,
    pub created_ms: i64,
    pub updated_ms: i64,
    pub head_seq: u64,
    pub running: bool,
}

#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    session_id TEXT PRIMARY KEY,
    node_id    TEXT NOT NULL,
    title      TEXT NOT NULL DEFAULT '',
    cwd        TEXT NOT NULL DEFAULT '',
    model      TEXT NOT NULL DEFAULT '',
    created_ms INTEGER NOT NULL,
    updated_ms INTEGER NOT NULL,
    head_seq   INTEGER NOT NULL,
    running    INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS events (
    session_id TEXT NOT NULL,
    seq        INTEGER NOT NULL,
    ts_ms      INTEGER NOT NULL,
    kind       TEXT NOT NULL,
    payload    TEXT NOT NULL,
    PRIMARY KEY (session_id, seq)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS audit (
    id     INTEGER PRIMARY KEY AUTOINCREMENT,
    ts_ms  INTEGER NOT NULL,
    kind   TEXT NOT NULL,
    detail TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS kv (
    k TEXT PRIMARY KEY,
    v TEXT NOT NULL
);
"#;

impl Store {
    pub fn open(path: &Path, durability: Durability) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let sync = match durability {
            Durability::Full => "FULL",
            Durability::Normal => "NORMAL",
        };
        conn.pragma_update(None, "synchronous", sync)?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        // A poisoned lock means a panic mid-call; SQLite's transaction already
        // rolled back, so the connection is still consistent.
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Run a blocking store operation off the async runtime.
    pub async fn call<R, F>(&self, f: F) -> R
    where
        F: FnOnce(&Store) -> R + Send + 'static,
        R: Send + 'static,
    {
        let this = self.clone();
        match tokio::task::spawn_blocking(move || f(&this)).await {
            Ok(r) => r,
            Err(e) => std::panic::resume_unwind(e.into_panic()),
        }
    }

    /// Append one event following the replication rule (PLAN.md §3).
    /// `owner` is the node that is allowed to write this session.
    pub fn append(&self, owner: &str, e: &RawEvent) -> Result<AppendOutcome> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let sid = e.session_id.to_string();
        let existing: Option<(String, i64)> = tx
            .query_row(
                "SELECT node_id, head_seq FROM sessions WHERE session_id = ?1",
                params![sid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let head = match &existing {
            Some((node, head)) => {
                if node != owner {
                    return Err(StoreError::WrongOwner {
                        session_id: e.session_id,
                        owner: node.clone(),
                        claimed: owner.to_string(),
                    });
                }
                *head as u64
            }
            None => 0,
        };
        if e.seq <= head {
            return Ok(AppendOutcome::Duplicate);
        }
        if e.seq != head + 1 {
            return Ok(AppendOutcome::Gap { head });
        }
        tx.execute(
            "INSERT INTO events (session_id, seq, ts_ms, kind, payload) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![sid, e.seq as i64, e.ts_ms, e.kind, serde_json::to_string(&e.payload)?],
        )?;
        if existing.is_none() {
            tx.execute(
                "INSERT INTO sessions (session_id, node_id, created_ms, updated_ms, head_seq) VALUES (?1, ?2, ?3, ?3, 0)",
                params![sid, owner, e.ts_ms],
            )?;
        }
        tx.execute(
            "UPDATE sessions SET head_seq = ?2, updated_ms = ?3 WHERE session_id = ?1",
            params![sid, e.seq as i64, e.ts_ms],
        )?;
        match e.kind.as_str() {
            "session_created" => {
                let s = |k: &str| {
                    e.payload
                        .get(k)
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string()
                };
                tx.execute(
                    "UPDATE sessions SET title = ?2, cwd = ?3, model = ?4 WHERE session_id = ?1",
                    params![sid, s("title"), s("cwd"), s("model")],
                )?;
            }
            "turn_started" => {
                tx.execute(
                    "UPDATE sessions SET running = 1 WHERE session_id = ?1",
                    params![sid],
                )?;
            }
            "turn_ended" => {
                tx.execute(
                    "UPDATE sessions SET running = 0 WHERE session_id = ?1",
                    params![sid],
                )?;
            }
            _ => {}
        }
        tx.commit()?;
        Ok(AppendOutcome::Applied)
    }

    pub fn head_seq(&self, sid: SessionId) -> Result<u64> {
        let conn = self.lock();
        let h: Option<i64> = conn
            .query_row(
                "SELECT head_seq FROM sessions WHERE session_id = ?1",
                params![sid.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        Ok(h.unwrap_or(0) as u64)
    }

    pub fn owner(&self, sid: SessionId) -> Result<Option<String>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                "SELECT node_id FROM sessions WHERE session_id = ?1",
                params![sid.to_string()],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Events with `seq > after`, ascending, at most `limit`.
    pub fn events_after(&self, sid: SessionId, after: u64, limit: usize) -> Result<Vec<RawEvent>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT seq, ts_ms, kind, payload FROM events WHERE session_id = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![sid.to_string(), after as i64, limit as i64], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (seq, ts_ms, kind, payload) = row?;
            out.push(RawEvent {
                session_id: sid,
                seq: seq as u64,
                ts_ms,
                kind,
                payload: serde_json::from_str(&payload)?,
            });
        }
        Ok(out)
    }

    pub fn sessions(&self) -> Result<Vec<SessionRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT session_id, node_id, title, cwd, model, created_ms, updated_ms, head_seq, running FROM sessions ORDER BY updated_ms DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, i64>(8)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (sid, node_id, title, cwd, model, created_ms, updated_ms, head, running) = row?;
            out.push(SessionRow {
                session_id: sid
                    .parse()
                    .map_err(|_| StoreError::Corrupt(format!("session id {sid}")))?,
                node_id,
                title,
                cwd,
                model,
                created_ms,
                updated_ms,
                head_seq: head as u64,
                running: running != 0,
            });
        }
        Ok(out)
    }

    pub fn session(&self, sid: SessionId) -> Result<Option<SessionRow>> {
        Ok(self.sessions()?.into_iter().find(|r| r.session_id == sid))
    }

    /// `(session_id, head_seq)` for every session, ordered by id.
    pub fn heads(&self) -> Result<Vec<(SessionId, u64)>> {
        let conn = self.lock();
        let mut stmt =
            conn.prepare_cached("SELECT session_id, head_seq FROM sessions ORDER BY session_id")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (sid, head) = row?;
            let sid = sid
                .parse()
                .map_err(|_| StoreError::Corrupt(format!("session id {sid}")))?;
            out.push((sid, head as u64));
        }
        Ok(out)
    }

    pub fn audit(&self, kind: &str, detail: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO audit (ts_ms, kind, detail) VALUES (?1, ?2, ?3)",
            params![harness_core::now_ms(), kind, detail],
        )?;
        Ok(())
    }

    pub fn audit_count(&self) -> Result<u64> {
        let conn = self.lock();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM audit", [], |r| r.get(0))?;
        Ok(n as u64)
    }

    pub fn kv_get(&self, k: &str) -> Result<Option<String>> {
        let conn = self.lock();
        Ok(conn
            .query_row("SELECT v FROM kv WHERE k = ?1", params![k], |r| r.get(0))
            .optional()?)
    }

    pub fn kv_set(&self, k: &str, v: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO kv (k, v) VALUES (?1, ?2) ON CONFLICT(k) DO UPDATE SET v = excluded.v",
            params![k, v],
        )?;
        Ok(())
    }
}
