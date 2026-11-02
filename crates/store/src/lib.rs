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
    /// The user who created the session; `None` for sessions predating
    /// accounts or created directly on a node.
    pub owner_id: Option<String>,
    /// The session's active agent (from `session_created` / `agent_changed`).
    pub agent_id: Option<String>,
    /// `chat` or `agentic`; `None` for sessions predating profiles.
    pub profile: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    /// Reserved for teams; unused in v1.
    Member,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Member => "member",
        }
    }

    fn parse(s: &str) -> Result<Self> {
        match s {
            "owner" => Ok(Role::Owner),
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
CREATE TABLE IF NOT EXISTS users (
    user_id       TEXT PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role          TEXT NOT NULL,
    created_ms    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS ui_sessions (
    token_hash TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    expires_ms INTEGER NOT NULL
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
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // Additive migrations for databases created by older versions.
        let has_owner: bool = conn
            .prepare("SELECT 1 FROM pragma_table_info('sessions') WHERE name = 'owner_id'")?
            .exists([])?;
        if !has_owner {
            conn.execute_batch("ALTER TABLE sessions ADD COLUMN owner_id TEXT")?;
        }
        for (col, ddl) in [
            ("agent_id", "ALTER TABLE sessions ADD COLUMN agent_id TEXT"),
            ("profile", "ALTER TABLE sessions ADD COLUMN profile TEXT"),
        ] {
            let exists: bool = conn
                .prepare("SELECT 1 FROM pragma_table_info('sessions') WHERE name = ?1")?
                .exists([col])?;
            if !exists {
                conn.execute_batch(ddl)?;
            }
        }
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
                let opt = |k: &str| {
                    e.payload
                        .get(k)
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                };
                tx.execute(
                    "UPDATE sessions SET title = ?2, cwd = ?3, model = ?4, agent_id = ?5, profile = ?6 WHERE session_id = ?1",
                    params![sid, s("title"), s("cwd"), s("model"), opt("agent_id"), opt("profile")],
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
            "SELECT session_id, node_id, title, cwd, model, created_ms, updated_ms, head_seq, running, owner_id, agent_id, profile FROM sessions ORDER BY updated_ms DESC",
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

    pub fn create_user(
        &self,
        user_id: &str,
        username: &str,
        password_hash: &str,
        role: Role,
    ) -> Result<()> {
        let conn = self.lock();
        let res = conn.execute(
            "INSERT INTO users (user_id, username, password_hash, role, created_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![user_id, username, password_hash, role.as_str(), harness_core::now_ms()],
        );
        match res {
            Ok(_) => Ok(()),
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Err(StoreError::Conflict(format!("user {username}")))
            }
            Err(e) => Err(e.into()),
        }
    }

    fn user_where(&self, clause: &str, arg: &str) -> Result<Option<UserRow>> {
        let conn = self.lock();
        let sql = format!(
            "SELECT user_id, username, password_hash, role, created_ms FROM users WHERE {clause}"
        );
        let row = conn
            .query_row(&sql, params![arg], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            })
            .optional()?;
        row.map(|(user_id, username, password_hash, role, created_ms)| {
            Ok(UserRow {
                user_id,
                username,
                password_hash,
                role: Role::parse(&role)?,
                created_ms,
            })
        })
        .transpose()
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
            Some(id) => self.user_by_id(&id),
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
