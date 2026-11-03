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
    #[error("already exists: {0}")]
    Conflict(String),
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
    /// The session was deleted here; its events are not accepted again.
    Deleted,
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
    /// The user who created the session; `None` when created with the API
    /// token or directly on a node.
    pub owner_id: Option<String>,
    /// The session's active agent (from `session_created` / `agent_changed`).
    pub agent_id: Option<String>,
    /// `chat` or `agentic`; `None` until `session_created` has been mirrored.
    pub profile: Option<String>,
    /// Temporary chat: never indexed, purged after the configured TTL.
    pub incognito: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Manages accounts; always allowed agentic sessions.
    Admin,
    Member,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Member => "member",
        }
    }

    fn parse(s: &str) -> Result<Self> {
        match s {
            "admin" => Ok(Role::Admin),
            "member" => Ok(Role::Member),
            other => Err(StoreError::Corrupt(format!("role {other}"))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRow {
    pub user_id: String,
    pub username: String,
    /// PHC string (`$argon2id$...`).
    pub password_hash: String,
    pub role: Role,
    pub created_ms: i64,
    pub disabled: bool,
    /// Raw grant; admins are allowed agentic regardless.
    pub can_agentic: bool,
    pub must_change_password: bool,
}

/// A user to create.
#[derive(Debug, Clone)]
pub struct NewUser {
    pub user_id: String,
    pub username: String,
    pub password_hash: String,
    pub role: Role,
    pub can_agentic: bool,
    pub must_change_password: bool,
}

/// Fields to change on a user; `None` leaves a field as is.
#[derive(Debug, Clone, Default)]
pub struct UserPatch {
    pub role: Option<Role>,
    pub disabled: Option<bool>,
    pub can_agentic: Option<bool>,
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
    running    INTEGER NOT NULL DEFAULT 0,
    incognito  INTEGER NOT NULL DEFAULT 0,
    owner_id   TEXT,
    agent_id   TEXT,
    profile    TEXT
);
CREATE TABLE IF NOT EXISTS events (
    session_id TEXT NOT NULL,
    seq        INTEGER NOT NULL,
    ts_ms      INTEGER NOT NULL,
    kind       TEXT NOT NULL,
    payload    TEXT NOT NULL,
    PRIMARY KEY (session_id, seq)
) WITHOUT ROWID;
-- Sessions deleted here. A mirror refuses their events afterwards, so a node
-- that was offline during the delete cannot resurrect them.
CREATE TABLE IF NOT EXISTS deleted_sessions (
    session_id TEXT PRIMARY KEY,
    ts_ms      INTEGER NOT NULL
);
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
CREATE TABLE IF NOT EXISTS users (
    user_id       TEXT PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role          TEXT NOT NULL,
    created_ms    INTEGER NOT NULL,
    disabled      INTEGER NOT NULL DEFAULT 0,
    can_agentic   INTEGER NOT NULL DEFAULT 0,
    must_change_password INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS ui_sessions (
    token_hash TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    expires_ms INTEGER NOT NULL
);
-- Full-text index over chats (derived from events; rebuildable). Title and
-- message text are separate columns so ranking can weight titles higher.
CREATE VIRTUAL TABLE IF NOT EXISTS chat_fts USING fts5(
    session_id UNINDEXED,
    title,
    body,
    tokenize = 'unicode61 remove_diacritics 2'
);
"#;

/// Marks the start/end of a matched term in search snippets. Control
/// characters, so they can never collide with chat text or HTML.
pub const HIT_START: char = '\u{2}';
pub const HIT_END: char = '\u{3}';

/// One chat matching a search.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub session_id: SessionId,
    pub title: String,
    /// Best matching excerpt, terms wrapped in `HIT_START` / `HIT_END`.
    pub snippet: String,
    /// bm25: lower is better.
    pub score: f64,
    pub updated_ms: i64,
}

/// What a newly appended event contributes to the search index.
fn index_text(kind: &str, payload: &serde_json::Value) -> Option<(&'static str, String)> {
    let text = |k: &str| payload.get(k).and_then(|v| v.as_str()).map(str::to_string);
    match kind {
        "session_created" => text("title").map(|t| ("title", t)),
        "user_message" | "assistant_message" => text("text")
            .filter(|t| !t.trim().is_empty())
            .map(|t| ("body", t)),
        _ => None,
    }
}

fn is_incognito(payload: &serde_json::Value) -> bool {
    payload.get("incognito").and_then(|v| v.as_bool()) == Some(true)
}

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
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // Deleted chats (incognito, user deletes) are overwritten, not just
        // unlinked, so their text does not linger in free pages.
        conn.pragma_update(None, "secure_delete", "ON")?;
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
        if existing.is_none() {
            let deleted: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM deleted_sessions WHERE session_id = ?1)",
                params![sid],
                |r| r.get(0),
            )?;
            if deleted {
                return Ok(AppendOutcome::Deleted);
            }
        }
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
                let opt = |k: &str| {
                    e.payload
                        .get(k)
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                };
                tx.execute(
                    "UPDATE sessions SET title = ?2, cwd = ?3, model = ?4, agent_id = ?5, profile = ?6, incognito = ?7 WHERE session_id = ?1",
                    params![sid, s("title"), s("cwd"), s("model"), opt("agent_id"), opt("profile"), is_incognito(&e.payload)],
                )?;
            }
            "agent_changed" => {
                let agent = e
                    .payload
                    .pointer("/agent/agent_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                tx.execute(
                    "UPDATE sessions SET agent_id = ?2 WHERE session_id = ?1",
                    params![sid, agent],
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
        if let Some((col, text)) = index_text(&e.kind, &e.payload) {
            let incognito: bool = tx.query_row(
                "SELECT incognito FROM sessions WHERE session_id = ?1",
                params![sid],
                |r| r.get::<_, i64>(0).map(|v| v != 0),
            )?;
            if !incognito {
                Self::index_row(&tx, &sid, col, &text)?;
            }
        }
        tx.commit()?;
        Ok(AppendOutcome::Applied)
    }

    fn index_row(conn: &Connection, sid: &str, col: &str, text: &str) -> Result<()> {
        let (title, body) = if col == "title" {
            (text, "")
        } else {
            ("", text)
        };
        conn.execute(
            "INSERT INTO chat_fts (session_id, title, body) VALUES (?1, ?2, ?3)",
            params![sid, title, body],
        )?;
        Ok(())
    }

    /// Drop and rebuild the search index from the event log.
    pub fn rebuild_search_index(&self) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM chat_fts", [])?;
        let rows: Vec<(String, String, String)> = {
            let mut stmt = tx.prepare(
                "SELECT e.session_id, e.kind, e.payload FROM events e
                 JOIN sessions s ON s.session_id = e.session_id
                 WHERE s.incognito = 0
                   AND e.kind IN ('session_created', 'user_message', 'assistant_message')
                 ORDER BY e.session_id, e.seq",
            )?;
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<_>>()?
        };
        for (sid, kind, payload) in rows {
            let payload: serde_json::Value = serde_json::from_str(&payload)?;
            if let Some((col, text)) = index_text(&kind, &payload) {
                Self::index_row(&tx, &sid, col, &text)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Run a prepared FTS5 query (build it with care: it is FTS5 syntax).
    /// `owner`: `Some(id)` limits to that user's sessions; `None` searches all.
    /// Returns the best hit per session, best first.
    pub fn search(
        &self,
        fts_query: &str,
        owner: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT f.session_id, s.title,
                    snippet(chat_fts, -1, char(2), char(3), '…', 14),
                    bm25(chat_fts, 0.0, 3.0, 1.0) AS score, s.updated_ms
             FROM chat_fts f JOIN sessions s ON s.session_id = f.session_id
             WHERE chat_fts MATCH ?1 AND s.incognito = 0
               AND (?2 IS NULL OR s.owner_id = ?2)
             ORDER BY score LIMIT 200",
        )?;
        let rows = stmt.query_map(params![fts_query, owner], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, f64>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?;
        let mut out: Vec<SearchHit> = Vec::new();
        for row in rows {
            let (sid, title, snippet, score, updated_ms) = row?;
            let session_id: SessionId = sid
                .parse()
                .map_err(|_| StoreError::Corrupt(format!("session id {sid}")))?;
            // Rows arrive best-first; keep the first (best) per session.
            if out.iter().any(|h| h.session_id == session_id) {
                continue;
            }
            out.push(SearchHit {
                session_id,
                title,
                snippet,
                score,
                updated_ms,
            });
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    /// The indexed text of one session (titles and messages), for scoring.
    pub fn session_search_text(&self, sid: SessionId) -> Result<String> {
        let conn = self.lock();
        let mut stmt =
            conn.prepare_cached("SELECT title || ' ' || body FROM chat_fts WHERE session_id = ?1")?;
        let parts = stmt
            .query_map(params![sid.to_string()], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(parts.join("\n"))
    }

    /// Remove a session and everything derived from it. With `tombstone`, the
    /// id is remembered so later events for it are refused (mirrors).
    /// Returns whether the session existed.
    pub fn delete_session(&self, sid: SessionId, tombstone: bool) -> Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let id = sid.to_string();
        tx.execute("DELETE FROM events WHERE session_id = ?1", params![id])?;
        let indexed = tx.execute("DELETE FROM chat_fts WHERE session_id = ?1", params![id])?;
        if indexed > 0 {
            // FTS5 deletes only add tombstones; merging drops the old terms.
            tx.execute("INSERT INTO chat_fts(chat_fts) VALUES('optimize')", [])?;
        }
        let n = tx.execute("DELETE FROM sessions WHERE session_id = ?1", params![id])?;
        if tombstone {
            tx.execute(
                "INSERT OR REPLACE INTO deleted_sessions (session_id, ts_ms) VALUES (?1, ?2)",
                params![id, harness_core::now_ms()],
            )?;
        }
        tx.commit()?;
        Ok(n > 0)
    }

    pub fn is_deleted(&self, sid: SessionId) -> Result<bool> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM deleted_sessions WHERE session_id = ?1)",
            params![sid.to_string()],
            |r| r.get(0),
        )?)
    }

    /// Idle incognito sessions last touched before `before_ms`.
    pub fn expired_incognito(&self, before_ms: i64) -> Result<Vec<SessionId>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT session_id FROM sessions WHERE incognito = 1 AND running = 0 AND updated_ms < ?1",
        )?;
        let ids = stmt
            .query_map(params![before_ms], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids.into_iter()
            .map(|s| {
                s.parse()
                    .map_err(|_| StoreError::Corrupt(format!("session id {s}")))
            })
            .collect()
    }

    /// Fold the WAL back into the database file and truncate it, so deleted
    /// rows do not survive in the log file either.
    pub fn checkpoint(&self) -> Result<()> {
        let conn = self.lock();
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        Ok(())
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
            "SELECT session_id, node_id, title, cwd, model, created_ms, updated_ms, head_seq, running, owner_id, agent_id, profile, incognito FROM sessions ORDER BY updated_ms DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                SessionRow {
                    // Placeholder; parsed below where errors can be typed.
                    session_id: SessionId(Default::default()),
                    node_id: r.get(1)?,
                    title: r.get(2)?,
                    cwd: r.get(3)?,
                    model: r.get(4)?,
                    created_ms: r.get(5)?,
                    updated_ms: r.get(6)?,
                    head_seq: r.get::<_, i64>(7)? as u64,
                    running: r.get::<_, i64>(8)? != 0,
                    owner_id: r.get(9)?,
                    agent_id: r.get(10)?,
                    profile: r.get(11)?,
                    incognito: r.get::<_, i64>(12)? != 0,
                },
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (sid, mut session) = row?;
            session.session_id = sid
                .parse()
                .map_err(|_| StoreError::Corrupt(format!("session id {sid}")))?;
            out.push(session);
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

    pub fn set_session_owner(&self, sid: SessionId, owner_id: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE sessions SET owner_id = ?2 WHERE session_id = ?1",
            params![sid.to_string(), owner_id],
        )?;
        Ok(())
    }

    // ---- accounts ----

    pub fn user_count(&self) -> Result<u64> {
        let conn = self.lock();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
        Ok(n as u64)
    }

    pub fn create_user(&self, u: &NewUser) -> Result<()> {
        let conn = self.lock();
        let res = conn.execute(
            "INSERT INTO users (user_id, username, password_hash, role, created_ms, can_agentic, must_change_password)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                u.user_id,
                u.username,
                u.password_hash,
                u.role.as_str(),
                harness_core::now_ms(),
                u.can_agentic,
                u.must_change_password
            ],
        );
        match res {
            Ok(_) => Ok(()),
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Err(StoreError::Conflict(format!("user {}", u.username)))
            }
            Err(e) => Err(e.into()),
        }
    }

    const USER_COLS: &str = "user_id, username, password_hash, role, created_ms, disabled, can_agentic, must_change_password";

    fn user_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<(UserRow, String)> {
        Ok((
            UserRow {
                user_id: r.get(0)?,
                username: r.get(1)?,
                password_hash: r.get(2)?,
                // Parsed by the caller so errors stay typed.
                role: Role::Member,
                created_ms: r.get(4)?,
                disabled: r.get::<_, i64>(5)? != 0,
                can_agentic: r.get::<_, i64>(6)? != 0,
                must_change_password: r.get::<_, i64>(7)? != 0,
            },
            r.get(3)?,
        ))
    }

    fn finish_user((mut u, role): (UserRow, String)) -> Result<UserRow> {
        u.role = Role::parse(&role)?;
        Ok(u)
    }

    fn user_where(&self, clause: &str, arg: &str) -> Result<Option<UserRow>> {
        let conn = self.lock();
        let sql = format!("SELECT {} FROM users WHERE {clause}", Self::USER_COLS);
        conn.query_row(&sql, params![arg], Self::user_from_row)
            .optional()?
            .map(Self::finish_user)
            .transpose()
    }

    /// All users, by username.
    pub fn list_users(&self) -> Result<Vec<UserRow>> {
        let conn = self.lock();
        let sql = format!("SELECT {} FROM users ORDER BY username", Self::USER_COLS);
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], Self::user_from_row)?;
        rows.map(|r| Self::finish_user(r?)).collect()
    }

    /// Apply a patch. Errors with `Corrupt` if the user does not exist.
    pub fn update_user(&self, user_id: &str, p: &UserPatch) -> Result<()> {
        let conn = self.lock();
        let n = conn.execute(
            "UPDATE users SET
               role = COALESCE(?2, role),
               disabled = COALESCE(?3, disabled),
               can_agentic = COALESCE(?4, can_agentic)
             WHERE user_id = ?1",
            params![user_id, p.role.map(Role::as_str), p.disabled, p.can_agentic],
        )?;
        if n == 0 {
            return Err(StoreError::Corrupt(format!("no user {user_id}")));
        }
        Ok(())
    }

    pub fn set_password_hash(
        &self,
        user_id: &str,
        password_hash: &str,
        must_change: bool,
    ) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE users SET password_hash = ?2, must_change_password = ?3 WHERE user_id = ?1",
            params![user_id, password_hash, must_change],
        )?;
        Ok(())
    }

    /// Delete a user; their UI sessions go with them (foreign key cascade).
    pub fn delete_user(&self, user_id: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute("DELETE FROM users WHERE user_id = ?1", params![user_id])?;
        Ok(())
    }

    /// Admins that can still log in.
    pub fn active_admin_count(&self) -> Result<u64> {
        let conn = self.lock();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM users WHERE role = 'admin' AND disabled = 0",
            [],
            |r| r.get(0),
        )?;
        Ok(n as u64)
    }

    /// Log a user out everywhere. Returns how many sessions were removed.
    pub fn ui_sessions_delete_for_user(&self, user_id: &str) -> Result<usize> {
        let conn = self.lock();
        Ok(conn.execute(
            "DELETE FROM ui_sessions WHERE user_id = ?1",
            params![user_id],
        )?)
    }

    pub fn user_by_username(&self, username: &str) -> Result<Option<UserRow>> {
        self.user_where("username = ?1", username)
    }

    pub fn user_by_id(&self, user_id: &str) -> Result<Option<UserRow>> {
        self.user_where("user_id = ?1", user_id)
    }

    /// Store a UI session. Only a hash of the cookie value is kept.
    pub fn ui_session_create(
        &self,
        token_hash: &str,
        user_id: &str,
        expires_ms: i64,
    ) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO ui_sessions (token_hash, user_id, expires_ms) VALUES (?1, ?2, ?3)",
            params![token_hash, user_id, expires_ms],
        )?;
        Ok(())
    }

    /// The user behind a UI session, if it exists and `now_ms < expires_ms`.
    pub fn ui_session_user(&self, token_hash: &str, now_ms: i64) -> Result<Option<UserRow>> {
        let user_id: Option<String> = {
            let conn = self.lock();
            conn.query_row(
                "SELECT user_id FROM ui_sessions WHERE token_hash = ?1 AND expires_ms > ?2",
                params![token_hash, now_ms],
                |r| r.get(0),
            )
            .optional()?
        };
        match user_id {
            // A disabled user's sessions are dead even before cleanup.
            Some(id) => Ok(self.user_by_id(&id)?.filter(|u| !u.disabled)),
            None => Ok(None),
        }
    }

    pub fn ui_session_delete(&self, token_hash: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "DELETE FROM ui_sessions WHERE token_hash = ?1",
            params![token_hash],
        )?;
        Ok(())
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
