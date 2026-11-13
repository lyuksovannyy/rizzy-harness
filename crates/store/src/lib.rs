//! SQLite event log. The same schema serves the node's authoritative log and
//! the master's mirror; only durability differs. See PLAN.md §2–3.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use harness_core::{RawEvent, SessionId};
use rusqlite::{Connection, OptionalExtension, params};

// Each feature keeps its tables in its own file; their types are reached by
// module path (`routines::RoutineMark`), so no `pub use` lines are added.
pub mod artifacts;
pub mod blobs;
pub mod media;
mod projects;
pub mod routines;
pub mod session_images;
pub use projects::{
    CardPatch, CardRow, ColumnRow, NewCard, NewColumn, NewProject, ProjectLink, ProjectPatch,
    ProjectRow, TemplateRow,
};

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
    /// A change refused because it would break an invariant (a workspace
    /// left without an owner, ...). The message is written for users.
    #[error("{0}")]
    Refused(String),
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
    /// A second writer: `seq` already holds other content, now or earlier.
    /// Nothing is stored, and nothing will be for this session until it is
    /// deleted. `first` is true for the append that found it.
    Diverged {
        seq: u64,
        first: bool,
    },
    /// The writer is a revoked node (refused even for frames queued before
    /// the revoke).
    Revoked,
    /// The next event in order, but not one the log takes (`reason` says
    /// which rule it breaks; it names no content). Nothing is stored. An
    /// honest node's own log refuses it first, so on a mirror it means a
    /// broken or hostile node: the master freezes the session
    /// (`Store::freeze`) as it does a diverged one.
    Refused {
        seq: u64,
        reason: &'static str,
    },
}

/// A session a node lists in its hello, as the mirror sees it
/// (`Store::hello_states`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelloState {
    /// Mirrored up to this `seq` (0: not mirrored yet).
    Head(u64),
    /// Deleted here: the node is told again.
    Deleted,
    /// Diverged or refused: nothing is asked for.
    Frozen,
    /// Another node's session: nothing is revealed or sent.
    Foreign,
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
    /// A sub-agent's session: the session that delegated to it.
    pub parent_id: Option<String>,
    /// The effective workspace (see `EFFECTIVE_WS`); `None` only for a chat
    /// without an owner and without a mapping. Master only.
    pub workspace_id: Option<String>,
    /// Background commands the log shows started and not ended (T.8d): the
    /// revocation sweep stops them although no turn runs.
    pub background: u32,
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

/// A chat waiting for the user.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Attention {
    pub session_id: SessionId,
    pub title: String,
    /// `approval` or `question`.
    pub reason: &'static str,
    /// The event that started needing attention (for "seen").
    pub seq: u64,
    pub parent_id: Option<String>,
    pub updated_ms: i64,
    /// The chat's effective workspace, so the UI can switch to it.
    pub workspace_id: Option<String>,
}

/// A workspace (master only). Personal ones have `personal_owner` set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRow {
    pub workspace_id: String,
    pub name: String,
    pub personal_owner: Option<String>,
    pub created_ms: i64,
    /// Workspace-tier settings, a JSON object the master parses.
    pub settings: String,
}

impl WorkspaceRow {
    pub fn personal(&self) -> bool {
        self.personal_owner.is_some()
    }
}

/// One member of a workspace, with the account fields the UI shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberRow {
    pub user_id: String,
    pub username: String,
    pub disabled: bool,
    pub role_id: String,
    pub joined_ms: i64,
    /// The member's own choice to use their personal memory here.
    pub use_memory: bool,
}

/// A workspace a user belongs to, with their role there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Membership {
    pub workspace: WorkspaceRow,
    pub role_id: String,
    pub use_memory: bool,
    /// How many members the workspace has.
    pub members: u64,
}

/// A remote node the master trusts (pinned key).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRow {
    pub node_id: String,
    pub hostname: String,
    pub public_key: String,
    pub created_ms: i64,
    pub revoked_ms: Option<i64>,
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
    profile    TEXT,
    parent_id  TEXT,
    -- Attention: approvals waiting, or the last turn ended with a question.
    pending_approvals INTEGER NOT NULL DEFAULT 0,
    -- Background commands started and not ended (T.8d).
    background INTEGER NOT NULL DEFAULT 0,
    last_asks  INTEGER NOT NULL DEFAULT 0,
    asks_user  INTEGER NOT NULL DEFAULT 0,
    attention_seq INTEGER NOT NULL DEFAULT 0,
    -- Set when a second writer sent other content under this seq (PLAN.md
    -- §3): every later append is refused until the session is deleted.
    diverged_seq INTEGER
);
-- Listings are per owner (`Store::sessions_of`).
CREATE INDEX IF NOT EXISTS sessions_owner ON sessions(owner_id, updated_ms);
-- How far each user has looked at each chat (questions stop needing
-- attention once seen).
CREATE TABLE IF NOT EXISTS seen (
    user_id    TEXT NOT NULL,
    session_id TEXT NOT NULL,
    seq        INTEGER NOT NULL,
    PRIMARY KEY (user_id, session_id)
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
    ts_ms      INTEGER NOT NULL,
    -- The node that owned it, so only that node is told it was deleted
    -- (NULL when the mirror never had a row for it).
    node_id    TEXT
);
-- Uploaded documents waiting to be sent with a message (text extracted).
-- Consumed (deleted) when the message is sent: the text then lives in the
-- message's event. Unsent uploads expire.
CREATE TABLE IF NOT EXISTS attachments (
    attachment_id TEXT PRIMARY KEY,
    owner_id      TEXT,
    name          TEXT NOT NULL,
    mime          TEXT NOT NULL,
    text          TEXT NOT NULL,
    truncated     INTEGER NOT NULL,
    created_ms    INTEGER NOT NULL
);
-- Remote nodes the master trusts: pinned Ed25519 public keys.
CREATE TABLE IF NOT EXISTS nodes (
    node_id    TEXT PRIMARY KEY,
    public_key TEXT NOT NULL UNIQUE,
    hostname   TEXT NOT NULL,
    created_ms INTEGER NOT NULL,
    revoked_ms INTEGER
);
-- Single-use join tokens (SHA-256 only).
CREATE TABLE IF NOT EXISTS join_tokens (
    token_hash TEXT PRIMARY KEY,
    expires_ms INTEGER NOT NULL,
    created_by TEXT
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
-- Browser logins. `token_hash` is the SHA-256 of the cookie value and
-- `bind_hash` the SHA-256 of the login key the page keeps in its own
-- origin's storage and sends as a header (Gate 3 P-1: the cookie alone
-- reaches every port of the host), so a leaked database leaks neither.
-- `expires_ms` slides forward on use.
CREATE TABLE IF NOT EXISTS ui_sessions (
    token_hash   TEXT PRIMARY KEY,
    bind_hash    TEXT NOT NULL,
    user_id      TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    created_ms   INTEGER NOT NULL,
    last_seen_ms INTEGER NOT NULL,
    expires_ms   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS ui_sessions_user ON ui_sessions(user_id);
CREATE INDEX IF NOT EXISTS ui_sessions_expires ON ui_sessions(expires_ms);
-- Full-text index over chats (derived from events; rebuildable). Title and
-- message text are separate columns so ranking can weight titles higher.
CREATE VIRTUAL TABLE IF NOT EXISTS chat_fts USING fts5(
    session_id UNINDEXED,
    title,
    body,
    tokenize = 'unicode61 remove_diacritics 2'
);
-- Workspaces (master only). A Personal workspace has personal_owner set.
CREATE TABLE IF NOT EXISTS workspaces (
    workspace_id   TEXT PRIMARY KEY,
    name           TEXT NOT NULL,
    personal_owner TEXT UNIQUE REFERENCES users(user_id) ON DELETE CASCADE,
    created_ms     INTEGER NOT NULL,
    -- Workspace-tier settings (JSON object; WorkspaceSettings in the master).
    settings       TEXT NOT NULL DEFAULT '{}'
);
CREATE TABLE IF NOT EXISTS workspace_members (
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    user_id      TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    role_id      TEXT NOT NULL,          -- owner | admin | member | guest
    joined_ms    INTEGER NOT NULL,
    -- The member's own choice to share their facts (personal memory) with this
    -- workspace's chats: recall and proposals. Every insert says it (1 in
    -- Personal, 0 elsewhere); the default only covers an insert that forgets,
    -- and then it shares nothing.
    use_memory   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (workspace_id, user_id)
);
CREATE INDEX IF NOT EXISTS workspace_members_user ON workspace_members(user_id);
-- Which team workspace a chat is in (master only). No row: the owner's
-- Personal workspace. No foreign key on session_id: the mapping may be
-- written before the mirror has the session.
CREATE TABLE IF NOT EXISTS session_workspaces (
    session_id   TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS session_workspaces_ws ON session_workspaces(workspace_id);
"#;

/// A session's effective workspace: its `session_workspaces` row while the
/// owner is still a member of that workspace (or the chat has no owner),
/// else the owner's Personal workspace, else NULL. Checking membership here
/// instead of trusting the row makes it self-healing: however an owner left
/// a workspace, their chats fall back to Personal. Needs `sessions s` with
/// `WS_JOINS`; every column is qualified because the joined tables also have
/// `session_id` and `created_ms`.
const EFFECTIVE_WS: &str = "COALESCE(
    CASE WHEN s.owner_id IS NULL
           OR EXISTS (SELECT 1 FROM workspace_members m
                      WHERE m.workspace_id = sw.workspace_id AND m.user_id = s.owner_id)
         THEN sw.workspace_id END,
    pw.workspace_id)";
const WS_JOINS: &str = "LEFT JOIN session_workspaces sw ON sw.session_id = s.session_id
     LEFT JOIN workspaces pw ON pw.personal_owner = s.owner_id";

const SESSION_COLS: &str = "s.session_id, s.node_id, s.title, s.cwd, s.model, s.created_ms, s.updated_ms, s.head_seq, s.running, s.owner_id, s.agent_id, s.profile, s.incognito, s.parent_id, s.background";

/// The one role the store knows: the master defines the roles and their
/// permissions; the store needs owners for the last-owner rule.
const OWNER: &str = "owner";
/// Stored name of every Personal workspace (the UI translates it).
const PERSONAL_NAME: &str = "Personal";

const WORKSPACE_COLS: &str = "w.workspace_id, w.name, w.personal_owner, w.created_ms, w.settings";

/// An owner of workspace `?1` whose account is enabled (disabled owners
/// cannot act, so they never keep a workspace owned).
const ENABLED_OWNER: &str =
    "SELECT 1 FROM workspace_members m JOIN users u ON u.user_id = m.user_id
     WHERE m.workspace_id = ?1 AND m.role_id = 'owner' AND u.disabled = 0";

/// Why a member change conditional on the role the caller's guards checked
/// was refused: someone changed that role in between.
const ROLE_CHANGED: &str = "this member's role just changed; reload and try again";

fn no_owner_left(workspace: &str) -> StoreError {
    StoreError::Refused(format!(
        "{workspace} would be left without an owner; make another member an owner first"
    ))
}

/// Marks the start/end of a matched term in search snippets. Control
/// characters, so they can never collide with chat text or HTML.
pub const HIT_START: char = '\u{2}';
pub const HIT_END: char = '\u{3}';

/// How many index rows a search ranks before keeping the best per chat:
/// the deepest a search panel tab can page.
pub const SEARCH_WINDOW: usize = 500;

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

/// The largest event the log takes: its payload as JSON plus its kind
/// (Gate 3 review L-3). Half the link's 8 MiB frame, so one event always
/// fits a frame with its envelope. The largest legitimate event today is a
/// `user_message`: the prompt text (the HTTP body limit is 2 MB) plus up to
/// five attachments of 100,000 characters, about 2.5 MB of JSON in a
/// realistic worst case; `Master::prompt` refuses a message that would not
/// fit. Everything else is far smaller (tool output is cut to 30,000
/// characters, model output by its token limit).
pub const MAX_EVENT_BYTES: usize = 4 << 20;
/// A session title in the row (and the search index) is cut to this many
/// characters; the event keeps it whole. Sub-agent titles are under 100.
pub const MAX_TITLE_CHARS: usize = 200;
/// Identifiers and paths a node supplies for the row are refused past
/// these lengths rather than cut: a cut one would name something else.
/// `PATH_MAX` on Linux.
pub const MAX_CWD_BYTES: usize = 4096;
pub const MAX_MODEL_BYTES: usize = 256;
/// Agent ids are at most 32 characters (`harness_agents::valid_id`).
pub const MAX_AGENT_ID_BYTES: usize = 64;
const MAX_KIND_BYTES: usize = 64;

/// A title as the row stores it: at most `MAX_TITLE_CHARS` characters, cut
/// on a character boundary.
fn row_title(t: &str) -> String {
    match t.char_indices().nth(MAX_TITLE_CHARS) {
        Some((at, _)) => t[..at].to_string(),
        None => t.to_string(),
    }
}

/// Why the log does not take `e`, the next event in order with a payload of
/// `payload_len` bytes of JSON; `None` when it does. The reasons name the
/// rule only, never content.
fn refusal(e: &RawEvent, payload_len: usize) -> Option<&'static str> {
    if e.kind.len() > MAX_KIND_BYTES {
        return Some("event kind too long");
    }
    if payload_len + e.kind.len() > MAX_EVENT_BYTES {
        return Some("event too large");
    }
    let longer = |v: Option<&serde_json::Value>, max: usize| {
        v.and_then(|v| v.as_str()).is_some_and(|s| s.len() > max)
    };
    let p = &e.payload;
    match e.kind.as_str() {
        // The row is a session's creation record: set once, by its first
        // event. A later `session_created` would rewrite the owner (through
        // its parent), `incognito` and `profile` on a node's word alone
        // (Gate 3 review L-2), and no node writes one.
        "session_created" if e.seq != 1 => Some("session_created after the session's first event"),
        "session_created" if longer(p.get("cwd"), MAX_CWD_BYTES) => Some("cwd too long"),
        "session_created" if longer(p.get("model"), MAX_MODEL_BYTES) => Some("model too long"),
        "session_created" if longer(p.get("agent_id"), MAX_AGENT_ID_BYTES) => {
            Some("agent id too long")
        }
        "session_created"
            if !matches!(p.get("profile"), None | Some(serde_json::Value::Null))
                && !matches!(
                    p.get("profile").and_then(|v| v.as_str()),
                    Some("chat" | "agentic")
                ) =>
        {
            Some("unknown profile")
        }
        "agent_changed" if longer(p.pointer("/agent/agent_id"), MAX_AGENT_ID_BYTES) => {
            Some("agent id too long")
        }
        _ => None,
    }
}

/// What a newly appended event contributes to the search index.
fn index_text(kind: &str, payload: &serde_json::Value) -> Option<(&'static str, String)> {
    let text = |k: &str| payload.get(k).and_then(|v| v.as_str()).map(str::to_string);
    match kind {
        "session_created" => text("title").map(|t| ("title", row_title(&t))),
        "user_message" | "assistant_message" => text("text")
            .filter(|t| !t.trim().is_empty())
            .map(|t| ("body", t)),
        _ => None,
    }
}

/// Does an assistant message hold a question or form the UI shows? Its
/// turn then waits for the user like one that ends with a question mark.
fn asks_with_a_block(payload: &serde_json::Value) -> bool {
    let Some(blocks) = payload.get("blocks").and_then(|b| b.as_array()) else {
        return false;
    };
    blocks.iter().any(|b| {
        matches!(
            b.get("type").and_then(|t| t.as_str()),
            Some("choices" | "form")
        ) && b.get("error").is_none()
            && b.get("spec").is_some_and(|s| s.is_object())
    })
}

/// How often a UI session in use has its expiry moved forward.
pub const UI_SESSION_TOUCH_MS: i64 = 60 * 1000;

/// Whether `e` is the event already stored at its `seq`: same kind, time
/// and payload, the payload compared as JSON values (member order does not
/// count; floats survive the text round trip with `float_roundtrip`).
fn same_as_stored(tx: &rusqlite::Transaction<'_>, sid: &str, e: &RawEvent) -> Result<bool> {
    let stored: Option<(i64, String, String)> = tx
        .query_row(
            "SELECT ts_ms, kind, payload FROM events WHERE session_id = ?1 AND seq = ?2",
            params![sid, e.seq as i64],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((ts_ms, kind, payload)) = stored else {
        // Below head with no row: nothing to compare against (not reachable
        // today: a session's events and row are deleted together). Treated
        // as a duplicate, as before, rather than freezing the session.
        return Ok(true);
    };
    Ok(ts_ms == e.ts_ms
        && kind == e.kind
        && serde_json::from_str::<serde_json::Value>(&payload)? == e.payload)
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
        conn.execute_batch(projects::SCHEMA)?;
        // In this order: each one's tables reference the ones before.
        conn.execute_batch(blobs::SCHEMA)?;
        conn.execute_batch(media::SCHEMA)?;
        conn.execute_batch(artifacts::SCHEMA)?;
        conn.execute_batch(routines::SCHEMA)?;
        conn.execute_batch(session_images::SCHEMA)?;
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
        // Checked here, in the transaction that would store the frame, so
        // frames queued before a revoke are refused too. Only the mirror has
        // node rows; the embedded node and a node's own log have none.
        let revoked: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE node_id = ?1 AND revoked_ms IS NOT NULL)",
            params![owner],
            |r| r.get(0),
        )?;
        if revoked {
            return Ok(AppendOutcome::Revoked);
        }
        let existing: Option<(String, i64, Option<i64>)> = tx
            .query_row(
                "SELECT node_id, head_seq, diverged_seq FROM sessions WHERE session_id = ?1",
                params![sid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if existing.is_none() {
            let deleted: Option<Option<String>> = tx
                .query_row(
                    "SELECT node_id FROM deleted_sessions WHERE session_id = ?1",
                    params![sid],
                    |r| r.get(0),
                )
                .optional()?;
            match deleted {
                // Another node's deleted session: refused as for a live one,
                // without saying it was deleted (Gate 3 review L-6).
                Some(Some(node)) if node != owner => {
                    return Err(StoreError::WrongOwner {
                        session_id: e.session_id,
                        owner: node,
                        claimed: owner.to_string(),
                    });
                }
                Some(_) => return Ok(AppendOutcome::Deleted),
                None => {}
            }
        }
        let head = match &existing {
            Some((node, head, diverged)) => {
                if node != owner {
                    return Err(StoreError::WrongOwner {
                        session_id: e.session_id,
                        owner: node.clone(),
                        claimed: owner.to_string(),
                    });
                }
                if let Some(at) = diverged {
                    return Ok(AppendOutcome::Diverged {
                        seq: *at as u64,
                        first: false,
                    });
                }
                *head as u64
            }
            None => 0,
        };
        if e.seq <= head {
            // Only a resend reaches this (the owning node's own appends are
            // always head + 1), so the extra read is off the hot path.
            if same_as_stored(&tx, &sid, e)? {
                return Ok(AppendOutcome::Duplicate);
            }
            tx.execute(
                "UPDATE sessions SET diverged_seq = ?2 WHERE session_id = ?1",
                params![sid, e.seq as i64],
            )?;
            tx.commit()?;
            return Ok(AppendOutcome::Diverged {
                seq: e.seq,
                first: true,
            });
        }
        if e.seq != head + 1 {
            return Ok(AppendOutcome::Gap { head });
        }
        let payload = serde_json::to_string(&e.payload)?;
        if let Some(reason) = refusal(e, payload.len()) {
            return Ok(AppendOutcome::Refused { seq: e.seq, reason });
        }
        tx.execute(
            "INSERT INTO events (session_id, seq, ts_ms, kind, payload) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![sid, e.seq as i64, e.ts_ms, e.kind, payload],
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
                // A parent is kept only when it is another session of the
                // same node (Gate 3 review L-1): a node cannot attach its
                // session to another node's chat and so reach that chat's
                // project. Anything else is dropped and the session is an
                // ordinary top-level one.
                let parent = match opt("parent_session") {
                    Some(p) => tx
                        .query_row(
                            "SELECT p.session_id FROM sessions p
                             WHERE p.session_id = ?1 AND p.node_id = ?2 AND p.session_id != ?3",
                            params![p, owner, sid],
                            |r| r.get::<_, String>(0),
                        )
                        .optional()?,
                    None => None,
                };
                // Every listing loads the title: cut, never refused (it is
                // free text; the event keeps the whole of it).
                let title = row_title(&s("title"));
                tx.execute(
                    "UPDATE sessions SET title = ?2, cwd = ?3, model = ?4, agent_id = ?5, profile = ?6, incognito = ?7, parent_id = ?8 WHERE session_id = ?1",
                    params![sid, title, s("cwd"), s("model"), opt("agent_id"), opt("profile"), is_incognito(&e.payload), parent],
                )?;
                // A sub-agent's session belongs to whoever owns its parent,
                // so the two have the same owner by construction; only into
                // an empty owner: no event changes one once set.
                if let Some(parent) = &parent {
                    tx.execute(
                        "UPDATE sessions SET owner_id = (SELECT p.owner_id FROM sessions p WHERE p.session_id = ?2) WHERE session_id = ?1 AND owner_id IS NULL",
                        params![sid, parent],
                    )?;
                    // ...and lives in its parent's workspace (no rows on a
                    // node: it never maps chats).
                    tx.execute(
                        "INSERT OR IGNORE INTO session_workspaces (session_id, workspace_id)
                         SELECT ?1, sw.workspace_id FROM session_workspaces sw
                         WHERE sw.session_id = ?2",
                        params![sid, parent],
                    )?;
                }
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
                    "UPDATE sessions SET running = 1, asks_user = 0, last_asks = 0 WHERE session_id = ?1",
                    params![sid],
                )?;
            }
            "turn_ended" => {
                let completed =
                    e.payload.get("reason").and_then(|v| v.as_str()) == Some("completed");
                tx.execute(
                    "UPDATE sessions SET running = 0,
                        asks_user = CASE WHEN ?2 AND last_asks = 1 THEN 1 ELSE 0 END,
                        attention_seq = CASE WHEN ?2 AND last_asks = 1 THEN ?3 ELSE attention_seq END
                     WHERE session_id = ?1",
                    params![sid, completed, e.seq as i64],
                )?;
            }
            "assistant_message" => {
                let text = e
                    .payload
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim_end();
                if !text.is_empty() {
                    tx.execute(
                        "UPDATE sessions SET last_asks = ?2 WHERE session_id = ?1",
                        params![sid, text.ends_with('?') || asks_with_a_block(&e.payload)],
                    )?;
                }
            }
            "approval_requested" => {
                tx.execute(
                    "UPDATE sessions SET pending_approvals = pending_approvals + 1, attention_seq = ?2 WHERE session_id = ?1",
                    params![sid, e.seq as i64],
                )?;
            }
            "background_started" => {
                tx.execute(
                    "UPDATE sessions SET background = background + 1 WHERE session_id = ?1",
                    params![sid],
                )?;
            }
            "background_ended" => {
                tx.execute(
                    "UPDATE sessions SET background = MAX(0, background - 1) WHERE session_id = ?1",
                    params![sid],
                )?;
            }
            "approval_resolved" => {
                tx.execute(
                    "UPDATE sessions SET pending_approvals = MAX(0, pending_approvals - 1) WHERE session_id = ?1",
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
    /// `workspace`: `Some(id)` also limits to sessions whose effective
    /// workspace is `id`; `None` does not filter (only the API token searches
    /// across workspaces). Sub-agent sessions are never hits: no list shows
    /// them, their parent chat does. Returns the best hit per session, best
    /// first, from the best `SEARCH_WINDOW` matching rows.
    pub fn search(
        &self,
        fts_query: &str,
        owner: Option<&str>,
        workspace: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT f.session_id, s.title,
                    snippet(chat_fts, -1, char(2), char(3), '…', 14),
                    bm25(chat_fts, 0.0, 3.0, 1.0) AS score, s.updated_ms
             FROM chat_fts f JOIN sessions s ON s.session_id = f.session_id
             {WS_JOINS}
             WHERE chat_fts MATCH ?1 AND s.incognito = 0 AND s.parent_id IS NULL
               AND (?2 IS NULL OR s.owner_id = ?2)
               AND (?3 IS NULL OR {EFFECTIVE_WS} = ?3)
             ORDER BY score LIMIT {SEARCH_WINDOW}"
        ))?;
        let rows = stmt.query_map(params![fts_query, owner, workspace], |r| {
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
        let node: Option<String> = tx
            .query_row(
                "SELECT node_id FROM sessions WHERE session_id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        tx.execute("DELETE FROM events WHERE session_id = ?1", params![id])?;
        let indexed = tx.execute("DELETE FROM chat_fts WHERE session_id = ?1", params![id])?;
        if indexed > 0 {
            // FTS5 deletes only add tombstones; merging drops the old terms.
            tx.execute("INSERT INTO chat_fts(chat_fts) VALUES('optimize')", [])?;
        }
        let n = tx.execute("DELETE FROM sessions WHERE session_id = ?1", params![id])?;
        tx.execute(
            "DELETE FROM session_workspaces WHERE session_id = ?1",
            params![id],
        )?;
        tx.execute(
            "DELETE FROM session_projects WHERE session_id = ?1",
            params![id],
        )?;
        tx.execute(
            "UPDATE board_cards SET session_id = NULL WHERE session_id = ?1",
            params![id],
        )?;
        if tombstone {
            tx.execute(
                "INSERT INTO deleted_sessions (session_id, ts_ms, node_id) VALUES (?1, ?2, ?3)
                 ON CONFLICT(session_id) DO UPDATE SET ts_ms = ?2,
                     node_id = COALESCE(?3, deleted_sessions.node_id)",
                params![id, harness_core::now_ms(), node],
            )?;
        }
        // Rows that name the chat without a foreign key (it is deleted on
        // its own) go in the same transaction.
        routines::on_session_deleted(&tx, &id)?;
        media::on_session_deleted(&tx, &id)?;
        session_images::on_session_deleted(&tx, &id)?;
        tx.commit()?;
        Ok(n > 0)
    }

    /// The `seq` at which a second writer was caught, if one was.
    pub fn diverged_seq(&self, sid: SessionId) -> Result<Option<u64>> {
        let conn = self.lock();
        let v: Option<Option<i64>> = conn
            .query_row(
                "SELECT diverged_seq FROM sessions WHERE session_id = ?1",
                params![sid.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        Ok(v.flatten().map(|s| s as u64))
    }

    /// Freeze `node`'s session `sid` at `seq` after a `Refused` append: from
    /// now on every append is refused as for a diverged session (PLAN.md
    /// §3), since the mirror can no longer hold a contiguous copy. A session
    /// refused at its first event gets an empty frozen row, so its later
    /// frames are refused too instead of asking for it again and again.
    /// Another node's session is left alone. Returns whether this call froze it.
    pub fn freeze(&self, sid: SessionId, node: &str, seq: u64) -> Result<bool> {
        let conn = self.lock();
        let id = sid.to_string();
        let now = harness_core::now_ms();
        let n = conn.execute(
            "INSERT INTO sessions (session_id, node_id, created_ms, updated_ms, head_seq, diverged_seq)
             VALUES (?1, ?2, ?3, ?3, 0, ?4)
             ON CONFLICT(session_id) DO UPDATE SET diverged_seq = ?4
             WHERE sessions.node_id = ?2 AND sessions.diverged_seq IS NULL",
            params![id, node, now, seq as i64],
        )?;
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

    pub fn join_token_create(
        &self,
        token_hash: &str,
        expires_ms: i64,
        by: Option<&str>,
    ) -> Result<()> {
        self.lock().execute(
            "INSERT INTO join_tokens (token_hash, expires_ms, created_by) VALUES (?1, ?2, ?3)",
            params![token_hash, expires_ms, by],
        )?;
        Ok(())
    }

    /// Whether a join token is known and unexpired, without spending it
    /// (a join of another protocol is refused before the token is spent,
    /// and only to whoever holds a valid one).
    pub fn join_token_valid(&self, token_hash: &str, now_ms: i64) -> Result<bool> {
        Ok(self.lock().query_row(
            "SELECT EXISTS(SELECT 1 FROM join_tokens WHERE token_hash = ?1 AND expires_ms > ?2)",
            params![token_hash, now_ms],
            |r| r.get(0),
        )?)
    }

    /// Consume a join token and register the node, atomically. Returns the
    /// assigned node id, or `None` if the token is unknown, used or expired,
    /// or the key is already registered.
    pub fn node_join(
        &self,
        token_hash: &str,
        now_ms: i64,
        public_key: &str,
        hostname: &str,
    ) -> Result<Option<String>> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let ok = tx.execute(
            "DELETE FROM join_tokens WHERE token_hash = ?1 AND expires_ms > ?2",
            params![token_hash, now_ms],
        )? == 1;
        tx.execute(
            "DELETE FROM join_tokens WHERE expires_ms <= ?1",
            params![now_ms],
        )?;
        let known: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE public_key = ?1)",
            params![public_key],
            |r| r.get(0),
        )?;
        if !ok || known {
            tx.commit()?;
            return Ok(None);
        }
        let base: String = hostname
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .take(40)
            .collect::<String>()
            .trim_matches('-')
            .to_string();
        let base = if base.is_empty() || base == "local" || base == "master" {
            format!("node-{base}")
        } else {
            base
        };
        let mut id = base.clone();
        let mut n = 2;
        while tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE node_id = ?1)",
            params![id],
            |r| r.get::<_, bool>(0),
        )? {
            id = format!("{base}-{n}");
            n += 1;
        }
        tx.execute(
            "INSERT INTO nodes (node_id, public_key, hostname, created_ms) VALUES (?1, ?2, ?3, ?4)",
            params![id, public_key, hostname, now_ms],
        )?;
        tx.commit()?;
        Ok(Some(id))
    }

    /// The pinned key of a node that is not revoked.
    /// Whether `node_id` is a registered node that was revoked (one indexed
    /// read; an id with no row, such as the embedded node, is not).
    pub fn node_is_revoked(&self, node_id: &str) -> Result<bool> {
        Ok(self.lock().query_row(
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE node_id = ?1 AND revoked_ms IS NOT NULL)",
            params![node_id],
            |r| r.get(0),
        )?)
    }

    pub fn node_key(&self, node_id: &str) -> Result<Option<String>> {
        Ok(self
            .lock()
            .query_row(
                "SELECT public_key FROM nodes WHERE node_id = ?1 AND revoked_ms IS NULL",
                params![node_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Every registered remote node.
    pub fn registered_nodes(&self) -> Result<Vec<NodeRow>> {
        let conn = self.lock();
        let mut st = conn.prepare(
            "SELECT node_id, hostname, public_key, created_ms, revoked_ms FROM nodes ORDER BY created_ms",
        )?;
        let rows = st
            .query_map([], |r| {
                Ok(NodeRow {
                    node_id: r.get(0)?,
                    hostname: r.get(1)?,
                    public_key: r.get(2)?,
                    created_ms: r.get(3)?,
                    revoked_ms: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn node_revoke(&self, node_id: &str, now_ms: i64) -> Result<bool> {
        Ok(self.lock().execute(
            "UPDATE nodes SET revoked_ms = ?2 WHERE node_id = ?1 AND revoked_ms IS NULL",
            params![node_id, now_ms],
        )? > 0)
    }

    /// Record that `user` has looked at a chat up to `seq`.
    pub fn mark_seen(&self, user: &str, sid: SessionId, seq: u64) -> Result<()> {
        self.lock().execute(
            "INSERT INTO seen (user_id, session_id, seq) VALUES (?1, ?2, ?3)
             ON CONFLICT(user_id, session_id) DO UPDATE SET seq = MAX(seq, excluded.seq)",
            params![user, sid.to_string(), seq as i64],
        )?;
        Ok(())
    }

    /// Chats that need something from the user: a pending approval, or a
    /// finished turn that ended with a question the user has not seen.
    /// `owner`: `Some` limits to that user's chats (across all their
    /// workspaces; each item says which). Newest first.
    pub fn attention(&self, owner: Option<&str>, viewer: &str) -> Result<Vec<Attention>> {
        let conn = self.lock();
        let mut st = conn.prepare_cached(&format!(
            "SELECT s.session_id, s.title, s.pending_approvals, s.attention_seq, s.parent_id, s.updated_ms,
                    {EFFECTIVE_WS}
             FROM sessions s
             LEFT JOIN seen v ON v.session_id = s.session_id AND v.user_id = ?2
             {WS_JOINS}
             WHERE (?1 IS NULL OR s.owner_id = ?1)
               AND (s.pending_approvals > 0
                    OR (s.asks_user = 1 AND s.attention_seq > COALESCE(v.seq, 0)))
             ORDER BY s.updated_ms DESC LIMIT 20"
        ))?;
        let rows = st.query_map(params![owner, viewer], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (sid, title, approvals, seq, parent_id, updated_ms, workspace_id) = row?;
            out.push(Attention {
                session_id: sid
                    .parse()
                    .map_err(|_| StoreError::Corrupt(format!("session id {sid}")))?,
                title,
                reason: if approvals > 0 {
                    "approval"
                } else {
                    "question"
                },
                seq: seq as u64,
                parent_id,
                updated_ms,
                workspace_id,
            });
        }
        Ok(out)
    }

    /// Store an unsent upload unless its owner's unsent uploads would then
    /// pass `max_count` or `max_bytes` (bytes of name and text): `false`,
    /// nothing stored. Counted and inserted under one lock, so two uploads
    /// at once cannot both slip under the cap. `owner` `None` (the API
    /// token) counts against its own rows.
    pub fn attachment_put(
        &self,
        id: &str,
        owner: Option<&str>,
        a: &harness_core::event::Attachment,
        max_count: usize,
        max_bytes: usize,
    ) -> Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let (n, bytes): (i64, i64) = tx.query_row(
            "SELECT COUNT(*), COALESCE(SUM(LENGTH(CAST(name AS BLOB)) + LENGTH(CAST(text AS BLOB))), 0) FROM attachments WHERE owner_id IS ?1",
            params![owner],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let size = (a.name.len() + a.text.len()) as i64;
        if n as usize >= max_count || (bytes + size) as usize > max_bytes {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO attachments (attachment_id, owner_id, name, mime, text, truncated, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, owner, a.name, a.mime, a.text, a.truncated, harness_core::now_ms()],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Remove and return the given uploads, all or nothing. `owner` must
    /// match (`None` = the API token, which may use any upload).
    pub fn attachments_take(
        &self,
        ids: &[String],
        owner: Option<&str>,
    ) -> Result<Option<Vec<harness_core::event::Attachment>>> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let mut out = Vec::new();
        for id in ids {
            let row = tx
                .query_row(
                    "SELECT name, mime, text, truncated, owner_id FROM attachments WHERE attachment_id = ?1",
                    params![id],
                    |r| {
                        Ok((
                            harness_core::event::Attachment {
                                name: r.get(0)?,
                                mime: r.get(1)?,
                                text: r.get(2)?,
                                truncated: r.get::<_, i64>(3)? != 0,
                            },
                            r.get::<_, Option<String>>(4)?,
                        ))
                    },
                )
                .optional()?;
            match row {
                Some((a, o)) if owner.is_none() || o.as_deref() == owner => out.push(a),
                _ => return Ok(None),
            }
            tx.execute(
                "DELETE FROM attachments WHERE attachment_id = ?1",
                params![id],
            )?;
        }
        tx.commit()?;
        Ok(Some(out))
    }

    /// Put back uploads `attachments_take` returned for a message that was
    /// not sent, under their own ids (an id taken again since is left).
    pub fn attachments_give_back(
        &self,
        ids: &[String],
        owner: Option<&str>,
        atts: &[harness_core::event::Attachment],
    ) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        for (id, a) in ids.iter().zip(atts) {
            tx.execute(
                "INSERT OR IGNORE INTO attachments (attachment_id, owner_id, name, mime, text, truncated, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![id, owner, a.name, a.mime, a.text, a.truncated, harness_core::now_ms()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Drop uploads never sent, created before `before_ms`.
    pub fn purge_attachments(&self, before_ms: i64) -> Result<usize> {
        Ok(self.lock().execute(
            "DELETE FROM attachments WHERE created_ms < ?1",
            params![before_ms],
        )?)
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

    /// The latest message with text in each of `sids`, the user's or the
    /// assistant's, for search previews and routine results. Only a
    /// session's last four messages are read (a backward scan of its key
    /// range); a session whose last four have no text, or that has none, is
    /// left out.
    pub fn latest_message_texts(&self, sids: &[SessionId]) -> Result<HashMap<SessionId, String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT payload FROM events WHERE session_id = ?1 AND kind IN ('user_message', 'assistant_message') ORDER BY seq DESC LIMIT 4",
        )?;
        let mut out = HashMap::new();
        for sid in sids {
            let mut rows = stmt.query(params![sid.to_string()])?;
            while let Some(row) = rows.next()? {
                let payload: serde_json::Value = serde_json::from_str(&row.get::<_, String>(0)?)?;
                let text = payload.get("text").and_then(|v| v.as_str());
                if let Some(text) = text.filter(|t| !t.trim().is_empty()) {
                    out.insert(*sid, text.to_string());
                    break;
                }
            }
        }
        Ok(out)
    }

    /// The agent that wrote assistant message `message_id`, looked for only
    /// after the session's last user message: only a message there can
    /// still take an answer (`ui::check_reply`), so older turns are never
    /// read. `None` when it is not there or names no agent.
    pub fn open_message_author(&self, sid: SessionId, message_id: &str) -> Result<Option<String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT kind, payload FROM events WHERE session_id = ?1 AND kind IN ('user_message', 'assistant_message') ORDER BY seq DESC",
        )?;
        let mut rows = stmt.query(params![sid.to_string()])?;
        while let Some(row) = rows.next()? {
            if row.get::<_, String>(0)? == "user_message" {
                break;
            }
            let payload: serde_json::Value = serde_json::from_str(&row.get::<_, String>(1)?)?;
            if payload.get("message_id").and_then(|v| v.as_str()) == Some(message_id) {
                return Ok(payload
                    .get("author")
                    .and_then(|v| v.as_str())
                    .map(str::to_string));
            }
        }
        Ok(None)
    }

    /// The session query, with `tail` (WHERE / ORDER BY) appended.
    fn session_query(tail: &str) -> String {
        format!("SELECT {SESSION_COLS}, {EFFECTIVE_WS} FROM sessions s {WS_JOINS} {tail}")
    }

    fn session_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<(String, SessionRow)> {
        Ok((
            r.get::<_, String>(0)?,
            SessionRow {
                // Placeholder; parsed by the caller so errors stay typed.
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
                parent_id: r.get(13)?,
                background: r.get::<_, i64>(14)?.max(0) as u32,
                workspace_id: r.get(15)?,
            },
        ))
    }

    fn finish_session((sid, mut session): (String, SessionRow)) -> Result<SessionRow> {
        session.session_id = sid
            .parse()
            .map_err(|_| StoreError::Corrupt(format!("session id {sid}")))?;
        Ok(session)
    }

    pub fn sessions(&self) -> Result<Vec<SessionRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&Self::session_query("ORDER BY s.updated_ms DESC"))?;
        let rows = stmt.query_map([], Self::session_from_row)?;
        rows.map(|r| Self::finish_session(r?)).collect()
    }

    /// `owner`'s sessions, newest first (`None`: everyone's, for the API
    /// token). Filtered in SQL, so listing one account's chats never loads
    /// every other account's rows (Gate 3 review L-3).
    pub fn sessions_of(&self, owner: Option<&str>) -> Result<Vec<SessionRow>> {
        let Some(owner) = owner else {
            return self.sessions();
        };
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&Self::session_query(
            "WHERE s.owner_id = ?1 ORDER BY s.updated_ms DESC",
        ))?;
        let rows = stmt.query_map(params![owner], Self::session_from_row)?;
        rows.map(|r| Self::finish_session(r?)).collect()
    }

    /// The sessions of `node_id` the mirror shows running (a turn, or a
    /// background command): what the
    /// revocation sweep looks at when the node's link comes up (Gate 3
    /// review P-3). Filtered in SQL.
    pub fn running_on(&self, node_id: &str) -> Result<Vec<SessionRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&Self::session_query(
            "WHERE s.node_id = ?1 AND (s.running = 1 OR s.background > 0) ORDER BY s.updated_ms DESC",
        ))?;
        let rows = stmt.query_map(params![node_id], Self::session_from_row)?;
        rows.map(|r| Self::finish_session(r?)).collect()
    }

    pub fn session(&self, sid: SessionId) -> Result<Option<SessionRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&Self::session_query("WHERE s.session_id = ?1"))?;
        stmt.query_row(params![sid.to_string()], Self::session_from_row)
            .optional()?
            .map(Self::finish_session)
            .transpose()
    }

    /// What the mirror knows of each of `sids`, as `node` should see it in
    /// its hello (one read for the batch). Another node's session, live or
    /// deleted, is `Foreign`: nothing about it is revealed or sent
    /// (Gate 3 review L-6).
    pub fn hello_states(&self, node: &str, sids: &[SessionId]) -> Result<Vec<HelloState>> {
        let conn = self.lock();
        let mut live = conn.prepare_cached(
            "SELECT node_id, head_seq, diverged_seq IS NOT NULL FROM sessions WHERE session_id = ?1",
        )?;
        let mut gone =
            conn.prepare_cached("SELECT node_id FROM deleted_sessions WHERE session_id = ?1")?;
        let mut out = Vec::with_capacity(sids.len());
        for sid in sids {
            let id = sid.to_string();
            let row: Option<(String, i64, bool)> = live
                .query_row(params![id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .optional()?;
            let state = match row {
                Some((n, _, _)) if n != node => HelloState::Foreign,
                Some((_, _, true)) => HelloState::Frozen,
                Some((_, head, false)) => HelloState::Head(head as u64),
                None => match gone
                    .query_row(params![id], |r| r.get::<_, Option<String>>(0))
                    .optional()?
                {
                    Some(Some(n)) if n != node => HelloState::Foreign,
                    Some(_) => HelloState::Deleted,
                    None => HelloState::Head(0),
                },
            };
            out.push(state);
        }
        Ok(out)
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

    /// Stamp a new chat once its node acknowledged the create: set its owner
    /// (when given) and map it into `workspace`, in one transaction. A
    /// mapping is written only for an existing team workspace that the owner
    /// (the given one, else the chat's current one) belongs to, or for a
    /// chat without an owner; checking membership here means a removal
    /// racing the create cannot leave the chat in a workspace its owner has
    /// left. A Personal workspace never gets a row: its chats are found
    /// through their owner. Returns whether a mapping row was written.
    pub fn claim_session(
        &self,
        sid: SessionId,
        owner: Option<&str>,
        workspace: Option<&str>,
    ) -> Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let id = sid.to_string();
        let owner: Option<String> = match owner {
            Some(o) => {
                tx.execute(
                    "UPDATE sessions SET owner_id = ?2 WHERE session_id = ?1",
                    params![id, o],
                )?;
                Some(o.to_string())
            }
            None => tx
                .query_row(
                    "SELECT owner_id FROM sessions WHERE session_id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .optional()?
                .flatten(),
        };
        // A parent is kept only under the same node and the same owner
        // (Gate 3 review L-1). `append` checked the node; the owner is known
        // only now, as the master recorded it for the create, and a node
        // that named another user's chat on itself as parent loses that
        // parent here, with the workspace mapping it inherited from it.
        let dropped = tx.execute(
            "UPDATE sessions SET parent_id = NULL
             WHERE session_id = ?1 AND parent_id IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM sessions p
                               WHERE p.session_id = sessions.parent_id
                                 AND p.node_id = sessions.node_id
                                 AND p.owner_id IS sessions.owner_id)",
            params![id],
        )?;
        if dropped > 0 {
            tx.execute(
                "DELETE FROM session_workspaces WHERE session_id = ?1",
                params![id],
            )?;
        }
        let mapped = match workspace {
            Some(ws) => {
                tx.execute(
                    "INSERT OR REPLACE INTO session_workspaces (session_id, workspace_id)
                     SELECT ?1, w.workspace_id FROM workspaces w
                     WHERE w.workspace_id = ?2 AND w.personal_owner IS NULL
                       AND (?3 IS NULL OR EXISTS (SELECT 1 FROM workspace_members m
                                                  WHERE m.workspace_id = w.workspace_id
                                                    AND m.user_id = ?3))",
                    params![id, ws, owner],
                )? > 0
            }
            None => false,
        };
        tx.commit()?;
        Ok(mapped)
    }

    // ---- accounts ----

    pub fn user_count(&self) -> Result<u64> {
        let conn = self.lock();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
        Ok(n as u64)
    }

    /// Create an account together with its Personal workspace (one
    /// transaction, so no account is ever without one).
    pub fn create_user(&self, u: &NewUser) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let now = harness_core::now_ms();
        let res = tx.execute(
            "INSERT INTO users (user_id, username, password_hash, role, created_ms, can_agentic, must_change_password)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                u.user_id,
                u.username,
                u.password_hash,
                u.role.as_str(),
                now,
                u.can_agentic,
                u.must_change_password
            ],
        );
        match res {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                return Err(StoreError::Conflict(format!("user {}", u.username)));
            }
            Err(e) => return Err(e.into()),
        }
        Self::insert_personal(&tx, &u.user_id, now)?;
        tx.commit()?;
        Ok(())
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

    /// Delete a user, in one transaction. Refused while they are the only
    /// enabled owner of a team workspace that has other members. Team
    /// workspaces where they are the only member are deleted, and their
    /// chats leave every team workspace. Their UI sessions, Personal
    /// workspace and memberships go with them (foreign key cascade).
    pub fn delete_user(&self, user_id: &str) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        // (workspace, name, whether anyone else is a member)
        let teams: Vec<(String, String, bool)> = {
            let mut st = tx.prepare(
                "SELECT w.workspace_id, w.name,
                        EXISTS (SELECT 1 FROM workspace_members o
                                WHERE o.workspace_id = w.workspace_id AND o.user_id != ?1)
                 FROM workspace_members m JOIN workspaces w ON w.workspace_id = m.workspace_id
                 WHERE m.user_id = ?1 AND w.personal_owner IS NULL",
            )?;
            st.query_map(params![user_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0))
            })?
            .collect::<rusqlite::Result<_>>()?
        };
        for (ws, name, others) in &teams {
            if *others && Self::only_enabled_owner(&tx, ws, user_id)? {
                let username: String = tx.query_row(
                    "SELECT username FROM users WHERE user_id = ?1",
                    params![user_id],
                    |r| r.get(0),
                )?;
                return Err(StoreError::Refused(format!(
                    "{username} is the only owner of {name}; make another member an owner first"
                )));
            }
        }
        for (ws, _, others) in &teams {
            if !others {
                tx.execute(
                    "DELETE FROM workspaces WHERE workspace_id = ?1",
                    params![ws],
                )?;
            }
        }
        tx.execute(
            "DELETE FROM session_workspaces
             WHERE session_id IN (SELECT session_id FROM sessions WHERE owner_id = ?1)",
            params![user_id],
        )?;
        // What the cascade below cannot reach (the account's FTS rows).
        blobs::on_user_deleted(&tx, user_id)?;
        tx.execute("DELETE FROM users WHERE user_id = ?1", params![user_id])?;
        tx.commit()?;
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

    /// Log a user out everywhere. Returns how many logins were removed.
    pub fn ui_sessions_delete_for_user(&self, user_id: &str) -> Result<usize> {
        Ok(self.lock().execute(
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

    /// Store a UI session that lasts until `expires_ms`. Only hashes of
    /// the cookie value and of the login key are kept. Expired sessions
    /// (anyone's) are removed here, so the table does not grow without a
    /// timer.
    pub fn ui_session_create(
        &self,
        token_hash: &str,
        bind_hash: &str,
        user_id: &str,
        now_ms: i64,
        expires_ms: i64,
    ) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "DELETE FROM ui_sessions WHERE expires_ms <= ?1",
            params![now_ms],
        )?;
        conn.execute(
            "INSERT INTO ui_sessions (token_hash, bind_hash, user_id, created_ms, last_seen_ms, expires_ms)
             VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
            params![token_hash, bind_hash, user_id, now_ms, expires_ms],
        )?;
        Ok(())
    }

    /// The user behind a UI session, if it exists, `now_ms < expires_ms`,
    /// the user is not disabled and `bound` accepts the stored `bind_hash`
    /// (the caller compares it in constant time): one indexed lookup. Only
    /// a session that passes all of that is used: it slides, its expiry
    /// moving to `now_ms + ttl_ms`, written at most once per
    /// `UI_SESSION_TOUCH_MS`, not on every request.
    pub fn ui_session_user(
        &self,
        token_hash: &str,
        now_ms: i64,
        ttl_ms: i64,
        bound: impl FnOnce(&str) -> bool,
    ) -> Result<Option<UserRow>> {
        let conn = self.lock();
        let Some((user, last_seen_ms, bind_hash)) =
            Self::ui_session_lookup(&conn, token_hash, now_ms)?
        else {
            return Ok(None);
        };
        if !bound(&bind_hash) {
            return Ok(None);
        }
        if now_ms - last_seen_ms >= UI_SESSION_TOUCH_MS {
            conn.execute(
                "UPDATE ui_sessions SET last_seen_ms = ?2, expires_ms = ?3 WHERE token_hash = ?1",
                params![token_hash, now_ms, now_ms.saturating_add(ttl_ms)],
            )?;
        }
        Ok(Some(user))
    }

    /// Whether a UI session is live, by its cookie alone, without using
    /// it: nothing is written, so the expiry stays where it is. Never an
    /// authentication by itself, since the login key is not checked: for a
    /// WebSocket that has yet to present its key or re-checks a login it
    /// already verified (which must not keep an idle tab's login alive),
    /// and for the login a download link is bound to.
    pub fn ui_session_check(&self, token_hash: &str, now_ms: i64) -> Result<Option<UserRow>> {
        let conn = self.lock();
        match Self::ui_session_lookup(&conn, token_hash, now_ms)? {
            Some((user, _, _)) => Ok(Some(user)),
            None => Ok(None),
        }
    }

    /// The live, enabled session's user, when it was last used, and the
    /// hash of its login key.
    fn ui_session_lookup(
        conn: &Connection,
        token_hash: &str,
        now_ms: i64,
    ) -> Result<Option<(UserRow, i64, String)>> {
        let sql = format!(
            "SELECT u.{}, s.last_seen_ms, s.bind_hash FROM ui_sessions s
             JOIN users u ON u.user_id = s.user_id
             WHERE s.token_hash = ?1 AND s.expires_ms > ?2 AND u.disabled = 0",
            Self::USER_COLS.replace(", ", ", u.")
        );
        let found = conn
            .query_row(&sql, params![token_hash, now_ms], |r| {
                Ok((
                    Self::user_from_row(r)?,
                    r.get::<_, i64>(8)?,
                    r.get::<_, String>(9)?,
                ))
            })
            .optional()?;
        found
            .map(|(user, seen, bind)| Ok((Self::finish_user(user)?, seen, bind)))
            .transpose()
    }

    /// How many UI sessions are stored, expired ones included.
    pub fn ui_session_count(&self) -> Result<u64> {
        let n: i64 = self
            .lock()
            .query_row("SELECT COUNT(*) FROM ui_sessions", [], |r| r.get(0))?;
        Ok(n as u64)
    }

    /// End one UI session (logout). `true` when it existed.
    pub fn ui_session_delete(&self, token_hash: &str) -> Result<bool> {
        Ok(self.lock().execute(
            "DELETE FROM ui_sessions WHERE token_hash = ?1",
            params![token_hash],
        )? > 0)
    }

    /// Every `(key, value)` whose key starts with `prefix`.
    pub fn kv_prefix(&self, prefix: &str) -> Result<Vec<(String, String)>> {
        let conn = self.lock();
        let mut st =
            conn.prepare("SELECT k, v FROM kv WHERE substr(k, 1, length(?1)) = ?1 ORDER BY k")?;
        let rows = st
            .query_map(params![prefix], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn kv_delete(&self, k: &str) -> Result<bool> {
        Ok(self
            .lock()
            .execute("DELETE FROM kv WHERE k = ?1", params![k])?
            > 0)
    }

    pub fn kv_set(&self, k: &str, v: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO kv (k, v) VALUES (?1, ?2) ON CONFLICT(k) DO UPDATE SET v = excluded.v",
            params![k, v],
        )?;
        Ok(())
    }

    // ---- workspaces (master only) ----

    /// A membership row. `use_memory` is whether the member shares their
    /// facts with the workspace: on in Personal, off in a team workspace
    /// (the creator's own membership included) until they turn it on.
    fn insert_member(
        conn: &Connection,
        ws: &str,
        user_id: &str,
        role_id: &str,
        now: i64,
        use_memory: bool,
    ) -> rusqlite::Result<usize> {
        conn.execute(
            "INSERT INTO workspace_members (workspace_id, user_id, role_id, joined_ms, use_memory) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![ws, user_id, role_id, now, use_memory],
        )
    }

    /// Create `user_id`'s Personal workspace with them as its owner.
    fn insert_personal(conn: &Connection, user_id: &str, now: i64) -> Result<String> {
        let ws = harness_core::new_id();
        conn.execute(
            "INSERT INTO workspaces (workspace_id, name, personal_owner, created_ms) VALUES (?1, ?2, ?3, ?4)",
            params![ws, PERSONAL_NAME, user_id, now],
        )?;
        Self::insert_member(conn, &ws, user_id, OWNER, now, true)?;
        Ok(ws)
    }

    /// Give every account a Personal workspace (accounts that predate them
    /// have none) and restore a Personal owner's missing membership. Called
    /// when the master opens its store. Returns how many workspaces and
    /// memberships it had to add.
    pub fn ensure_personal_workspaces(&self) -> Result<usize> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        // NOT EXISTS, not NOT IN: team rows have a NULL personal_owner,
        // which would make NOT IN match nothing.
        let missing: Vec<String> = {
            let mut st = tx.prepare(
                "SELECT u.user_id FROM users u
                 WHERE NOT EXISTS (SELECT 1 FROM workspaces w WHERE w.personal_owner = u.user_id)",
            )?;
            st.query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        let now = harness_core::now_ms();
        for user_id in &missing {
            Self::insert_personal(&tx, user_id, now)?;
        }
        let healed = tx.execute(
            "INSERT OR IGNORE INTO workspace_members (workspace_id, user_id, role_id, joined_ms, use_memory)
             SELECT w.workspace_id, w.personal_owner, ?1, ?2, 1 FROM workspaces w
             WHERE w.personal_owner IS NOT NULL",
            params![OWNER, now],
        )?;
        tx.commit()?;
        Ok(missing.len() + healed)
    }

    /// `user_id`'s Personal workspace, created (or its owner membership
    /// restored) when missing.
    pub fn personal_workspace(&self, user_id: &str) -> Result<String> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT workspace_id FROM workspaces WHERE personal_owner = ?1",
                params![user_id],
                |r| r.get(0),
            )
            .optional()?;
        let now = harness_core::now_ms();
        let ws = match existing {
            Some(ws) => {
                tx.execute(
                    "INSERT OR IGNORE INTO workspace_members (workspace_id, user_id, role_id, joined_ms, use_memory) VALUES (?1, ?2, ?3, ?4, 1)",
                    params![ws, user_id, OWNER, now],
                )?;
                ws
            }
            None => Self::insert_personal(&tx, user_id, now)?,
        };
        tx.commit()?;
        Ok(ws)
    }

    fn workspace_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<WorkspaceRow> {
        Ok(WorkspaceRow {
            workspace_id: r.get(0)?,
            name: r.get(1)?,
            personal_owner: r.get(2)?,
            created_ms: r.get(3)?,
            settings: r.get(4)?,
        })
    }

    fn workspace_in(conn: &Connection, ws: &str) -> Result<Option<WorkspaceRow>> {
        Ok(conn
            .query_row(
                &format!("SELECT {WORKSPACE_COLS} FROM workspaces w WHERE w.workspace_id = ?1"),
                params![ws],
                Self::workspace_from_row,
            )
            .optional()?)
    }

    /// A workspace whose members may change; `None` when unknown. A Personal
    /// workspace is refused: its only member is its owner.
    fn team_workspace(conn: &Connection, ws: &str) -> Result<Option<WorkspaceRow>> {
        match Self::workspace_in(conn, ws)? {
            Some(w) if w.personal() => Err(StoreError::Refused(
                "a Personal workspace has only you".into(),
            )),
            other => Ok(other),
        }
    }

    /// Is `user_id` the only enabled owner of `ws`? Asked inside the
    /// transaction that takes their ownership away, so two concurrent
    /// changes cannot both pass. Taking away a disabled owner changes
    /// nothing, so members can still leave a workspace whose owners are all
    /// disabled.
    fn only_enabled_owner(conn: &Connection, ws: &str, user_id: &str) -> Result<bool> {
        Ok(conn.query_row(
            &format!(
                "SELECT EXISTS ({ENABLED_OWNER} AND m.user_id = ?2)
                    AND NOT EXISTS ({ENABLED_OWNER} AND m.user_id != ?2)"
            ),
            params![ws, user_id],
            |r| r.get::<_, i64>(0),
        )? != 0)
    }

    pub fn workspace(&self, ws: &str) -> Result<Option<WorkspaceRow>> {
        Self::workspace_in(&self.lock(), ws)
    }

    /// Every workspace: team ones by name, then the Personal ones.
    pub fn workspaces(&self) -> Result<Vec<WorkspaceRow>> {
        let conn = self.lock();
        let mut st = conn.prepare(&format!("SELECT {WORKSPACE_COLS} FROM workspaces w"))?;
        let mut rows = st
            .query_map([], Self::workspace_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // Sorted here: SQLite's NOCASE folds ASCII letters only.
        rows.sort_by_cached_key(|w| (w.personal(), w.name.to_lowercase(), w.created_ms));
        Ok(rows)
    }

    /// The workspaces `user_id` belongs to: Personal first, then by name.
    pub fn memberships(&self, user_id: &str) -> Result<Vec<Membership>> {
        let conn = self.lock();
        let mut st = conn.prepare_cached(&format!(
            "SELECT {WORKSPACE_COLS}, m.role_id, m.use_memory,
                    (SELECT COUNT(*) FROM workspace_members c WHERE c.workspace_id = w.workspace_id)
             FROM workspace_members m JOIN workspaces w ON w.workspace_id = m.workspace_id
             WHERE m.user_id = ?1"
        ))?;
        let mut rows = st
            .query_map(params![user_id], |r| {
                Ok(Membership {
                    workspace: Self::workspace_from_row(r)?,
                    role_id: r.get(5)?,
                    use_memory: r.get::<_, i64>(6)? != 0,
                    members: r.get::<_, i64>(7)? as u64,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.sort_by_cached_key(|m| {
            (
                !m.workspace.personal(),
                m.workspace.name.to_lowercase(),
                m.workspace.created_ms,
            )
        });
        Ok(rows)
    }

    const MEMBER_SELECT: &str =
        "SELECT m.user_id, u.username, u.disabled, m.role_id, m.joined_ms, m.use_memory
         FROM workspace_members m JOIN users u ON u.user_id = m.user_id";

    fn member_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<MemberRow> {
        Ok(MemberRow {
            user_id: r.get(0)?,
            username: r.get(1)?,
            disabled: r.get::<_, i64>(2)? != 0,
            role_id: r.get(3)?,
            joined_ms: r.get(4)?,
            use_memory: r.get::<_, i64>(5)? != 0,
        })
    }

    pub fn member(&self, ws: &str, user_id: &str) -> Result<Option<MemberRow>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                &format!(
                    "{} WHERE m.workspace_id = ?1 AND m.user_id = ?2",
                    Self::MEMBER_SELECT
                ),
                params![ws, user_id],
                Self::member_from_row,
            )
            .optional()?)
    }

    /// A workspace's members, in the order they joined.
    pub fn members(&self, ws: &str) -> Result<Vec<MemberRow>> {
        let conn = self.lock();
        let mut st = conn.prepare(&format!(
            "{} WHERE m.workspace_id = ?1 ORDER BY m.joined_ms, u.username",
            Self::MEMBER_SELECT
        ))?;
        let rows = st
            .query_map(params![ws], Self::member_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn member_count(&self, ws: &str) -> Result<u64> {
        let conn = self.lock();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM workspace_members WHERE workspace_id = ?1",
            params![ws],
            |r| r.get(0),
        )?;
        Ok(n as u64)
    }

    /// Usernames of a workspace's owners (disabled ones included), in the
    /// order they joined.
    pub fn owners(&self, ws: &str) -> Result<Vec<String>> {
        let conn = self.lock();
        let mut st = conn.prepare(
            "SELECT u.username FROM workspace_members m JOIN users u ON u.user_id = m.user_id
             WHERE m.workspace_id = ?1 AND m.role_id = ?2 ORDER BY m.joined_ms, u.username",
        )?;
        let rows = st
            .query_map(params![ws, OWNER], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Create a team workspace owned by `owner`.
    pub fn create_workspace(&self, name: &str, owner: &str) -> Result<WorkspaceRow> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let w = WorkspaceRow {
            workspace_id: harness_core::new_id(),
            name: name.to_string(),
            personal_owner: None,
            created_ms: harness_core::now_ms(),
            settings: "{}".into(),
        };
        tx.execute(
            "INSERT INTO workspaces (workspace_id, name, created_ms, settings) VALUES (?1, ?2, ?3, ?4)",
            params![w.workspace_id, w.name, w.created_ms, w.settings],
        )?;
        Self::insert_member(&tx, &w.workspace_id, owner, OWNER, w.created_ms, false)?;
        tx.commit()?;
        Ok(w)
    }

    /// Returns whether the workspace exists.
    pub fn rename_workspace(&self, ws: &str, name: &str) -> Result<bool> {
        Ok(self.lock().execute(
            "UPDATE workspaces SET name = ?2 WHERE workspace_id = ?1",
            params![ws, name],
        )? > 0)
    }

    /// Replace the workspace-tier settings (a JSON object the master
    /// validated). Returns whether the workspace exists.
    pub fn set_workspace_settings(&self, ws: &str, settings: &str) -> Result<bool> {
        Ok(self.lock().execute(
            "UPDATE workspaces SET settings = ?2 WHERE workspace_id = ?1",
            params![ws, settings],
        )? > 0)
    }

    /// Delete a team workspace. Its members and chat mappings go with it
    /// (foreign key cascade), so its chats return to their owners' Personal
    /// workspaces; no chat is deleted. Returns whether it existed.
    pub fn delete_workspace(&self, ws: &str) -> Result<bool> {
        let conn = self.lock();
        match Self::workspace_in(&conn, ws)? {
            None => Ok(false),
            Some(w) if w.personal() => Err(StoreError::Refused(
                "a Personal workspace cannot be deleted".into(),
            )),
            Some(_) => Ok(conn.execute(
                "DELETE FROM workspaces WHERE workspace_id = ?1",
                params![ws],
            )? > 0),
        }
    }

    /// Add a member. `Conflict` when they already are one; `false` when the
    /// workspace or the account does not exist.
    pub fn add_member(&self, ws: &str, user_id: &str, role_id: &str) -> Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        if Self::team_workspace(&tx, ws)?.is_none() {
            return Ok(false);
        }
        let username: Option<String> = tx
            .query_row(
                "SELECT username FROM users WHERE user_id = ?1",
                params![user_id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(username) = username else {
            return Ok(false);
        };
        match Self::insert_member(&tx, ws, user_id, role_id, harness_core::now_ms(), false) {
            Ok(_) => {}
            Err(e)
                if e.sqlite_error().map(|e| e.extended_code)
                    == Some(rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY) =>
            {
                return Err(StoreError::Conflict(format!("member {username}")));
            }
            Err(e) => return Err(e.into()),
        }
        tx.commit()?;
        Ok(true)
    }

    /// After a write conditional on the member's role changed no row: their
    /// role is no longer the one the caller's guards checked (refused, so a
    /// concurrent change cannot slip past those guards), or they are no
    /// member (`false`).
    fn role_unchanged(conn: &Connection, ws: &str, user_id: &str) -> Result<bool> {
        let member: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM workspace_members WHERE workspace_id = ?1 AND user_id = ?2)",
            params![ws, user_id],
            |r| r.get::<_, i64>(0).map(|v| v != 0),
        )?;
        if member {
            return Err(StoreError::Refused(ROLE_CHANGED.into()));
        }
        Ok(false)
    }

    /// Change a member's role, refusing to take the last enabled owner
    /// away. With `expected_role`, only while they still hold it (what the
    /// caller's guards checked). Returns whether they are a member.
    pub fn set_member_role(
        &self,
        ws: &str,
        user_id: &str,
        expected_role: Option<&str>,
        role_id: &str,
    ) -> Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let Some(w) = Self::team_workspace(&tx, ws)? else {
            return Ok(false);
        };
        // They stay a member, so the workspace keeps members and needs an owner.
        if role_id != OWNER && Self::only_enabled_owner(&tx, ws, user_id)? {
            return Err(no_owner_left(&w.name));
        }
        let n = tx.execute(
            "UPDATE workspace_members SET role_id = ?3
             WHERE workspace_id = ?1 AND user_id = ?2 AND (?4 IS NULL OR role_id = ?4)",
            params![ws, user_id, role_id, expected_role],
        )?;
        if n == 0 {
            return Self::role_unchanged(&tx, ws, user_id);
        }
        tx.commit()?;
        Ok(true)
    }

    /// Remove a member (or let them leave), refusing to take the last
    /// enabled owner away from the others. With `expected_role`, only while
    /// they still hold it (what the caller's guards checked). Their chats
    /// here return to their Personal workspace. Returns whether they were a
    /// member.
    pub fn remove_member(
        &self,
        ws: &str,
        user_id: &str,
        expected_role: Option<&str>,
    ) -> Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let Some(w) = Self::team_workspace(&tx, ws)? else {
            return Ok(false);
        };
        let others: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM workspace_members WHERE workspace_id = ?1 AND user_id != ?2)",
            params![ws, user_id],
            |r| r.get::<_, i64>(0).map(|v| v != 0),
        )?;
        if others && Self::only_enabled_owner(&tx, ws, user_id)? {
            return Err(no_owner_left(&w.name));
        }
        let n = tx.execute(
            "DELETE FROM workspace_members
             WHERE workspace_id = ?1 AND user_id = ?2 AND (?3 IS NULL OR role_id = ?3)",
            params![ws, user_id, expected_role],
        )?;
        if n == 0 {
            return Self::role_unchanged(&tx, ws, user_id);
        }
        tx.execute(
            "DELETE FROM session_workspaces WHERE workspace_id = ?1
               AND session_id IN (SELECT session_id FROM sessions WHERE owner_id = ?2)",
            params![ws, user_id],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// A member's own switch for using their personal memory in this
    /// workspace. Returns whether they are a member.
    pub fn set_use_memory(&self, ws: &str, user_id: &str, on: bool) -> Result<bool> {
        Ok(self.lock().execute(
            "UPDATE workspace_members SET use_memory = ?3 WHERE workspace_id = ?1 AND user_id = ?2",
            params![ws, user_id, on],
        )? > 0)
    }
}
