//! The chat list one page at a time (FIX_PLAN 4.5, `docs/store-readers.md`
//! §3 "A"): a keyset page of one owner's chats, and the page's
//! annotations (unread, project links, routine marks, sub-agent children)
//! as one statement each over the page's ids.
//!
//! Every query here is a constant text with bound parameters; an id list
//! is bound as one JSON array (`json_each`), so its size never changes the
//! statement and never meets SQLite's variable limit.

use std::collections::{HashMap, HashSet};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use harness_core::SessionId;
use rusqlite::params;

use crate::routines::{ACTIVE, RoutineMark};
use crate::{
    EFFECTIVE_WS, ProjectLink, Result, SESSION_COLS, SessionRow, Store, StoreError, WS_JOINS,
};

/// Schema version 7: the sub-agent chats of a page's parents are found by
/// parent, not by walking the owner's chats. Partial: top-level chats,
/// most of them, are not in it.
pub const PARENT_SCHEMA: &str = r#"
CREATE INDEX IF NOT EXISTS sessions_parent ON sessions(parent_id) WHERE parent_id IS NOT NULL;
"#;

/// The largest page; a larger `limit` is clamped to it, `0` reads 1.
pub const MAX_PAGE: usize = 200;

/// Whose chats a page lists. Spelled out instead of an `Option` so that
/// "every account's chats" is never what a forgotten `None` means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner<'a> {
    /// One account's chats: the caller's, from its session, never from
    /// the request.
    User(&'a str),
    /// Every chat, whoever owns it: the API token only.
    Everyone,
}

impl<'a> Owner<'a> {
    fn bind(self) -> Option<&'a str> {
        match self {
            Owner::User(u) => Some(u),
            Owner::Everyone => None,
        }
    }
}

/// A page's start: the last row of the previous page. The order is
/// `(updated_ms, session_id)` descending; the session id breaks ties, so
/// rows with equal times are neither repeated nor lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub updated_ms: i64,
    pub session_id: SessionId,
}

/// Longest cursor text [`Cursor::parse`] looks at (checked before decoding).
pub const CURSOR_MAX_LEN: usize = 64;
const CURSOR_VERSION: u8 = 1;
/// Version byte, `updated_ms` (8 bytes, big-endian), the session id (16).
const CURSOR_BYTES: usize = 1 + 8 + 16;

impl Cursor {
    fn of(row: &SessionRow) -> Self {
        Self {
            updated_ms: row.updated_ms,
            session_id: row.session_id,
        }
    }

    /// The wire form: unpadded base64url of 25 bytes (34 characters).
    /// Opaque to clients; not signed, since it only says where to start
    /// inside a query whose owner and workspace the server sets.
    pub fn encode(&self) -> String {
        let mut b = Vec::with_capacity(CURSOR_BYTES);
        b.push(CURSOR_VERSION);
        b.extend_from_slice(&self.updated_ms.to_be_bytes());
        b.extend_from_slice(self.session_id.0.as_bytes());
        URL_SAFE_NO_PAD.encode(b)
    }

    /// Strict: exactly what [`Cursor::encode`] writes, else `None`
    /// (the HTTP layer answers 400). Longer than [`CURSOR_MAX_LEN`] is
    /// refused before it is decoded.
    pub fn parse(text: &str) -> Option<Self> {
        if text.len() > CURSOR_MAX_LEN {
            return None;
        }
        // The engine refuses padding and non-zero trailing bits, so one
        // cursor has one spelling.
        let b = URL_SAFE_NO_PAD.decode(text).ok()?;
        if b.len() != CURSOR_BYTES || b[0] != CURSOR_VERSION {
            return None;
        }
        let updated_ms = i64::from_be_bytes(b[1..9].try_into().ok()?);
        let hex: String = b[9..].iter().map(|x| format!("{x:02x}")).collect();
        Some(Self {
            updated_ms,
            session_id: hex.parse().ok()?,
        })
    }
}

/// What one page of the chat list asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageQuery<'a> {
    pub owner: Owner<'a>,
    /// The effective workspace (`EFFECTIVE_WS`); `None`: every workspace.
    /// The caller checks membership before it asks.
    pub workspace: Option<&'a str>,
    /// Only what the sidebar shows: no sub-agent chats, no incognito ones.
    pub top_level: bool,
    /// Leave out a routine run's chat once its run is over, seen and not
    /// continued (what the master's `routines::annotate` drops today).
    pub hide_seen_runs: bool,
    /// Start after this row; `None`: from the newest.
    pub after: Option<Cursor>,
    /// Clamped to `1..=MAX_PAGE`.
    pub limit: usize,
}

/// The owner term: `?1` is bound either way, so both texts take the same
/// parameters. A separate text, not `?1 IS NULL OR …`, which would keep
/// SQLite off the `sessions_owner` index.
fn owner_term(owner: Owner<'_>) -> &'static str {
    match owner {
        Owner::User(_) => "s.owner_id = ?1",
        Owner::Everyone => "?1 IS NULL",
    }
}

fn page_sql(owner: Owner<'_>, after: bool) -> String {
    let owner = owner_term(owner);
    // SQLite seeks `sessions_owner` to `updated_ms <= ?2` from this OR
    // (the plan test checks it); the id breaks ties.
    let after = if after {
        "(s.updated_ms < ?2 OR (s.updated_ms = ?2 AND s.session_id < ?3))"
    } else {
        "?2 IS NULL AND ?3 IS NULL"
    };
    format!(
        "SELECT {SESSION_COLS}, {EFFECTIVE_WS} FROM sessions s {WS_JOINS}
         WHERE {owner} AND {after}
           AND (?4 IS NULL OR {EFFECTIVE_WS} = ?4)
           AND (?5 = 0 OR (s.parent_id IS NULL AND s.incognito = 0))
           AND (?6 = 0 OR NOT EXISTS (
                 SELECT 1 FROM routine_runs r JOIN routines o ON o.routine_id = r.routine_id
                 WHERE r.session_id = s.session_id AND r.seen <> 0 AND r.promoted = 0
                   AND r.status NOT IN {ACTIVE}))
         ORDER BY s.updated_ms DESC, s.session_id DESC LIMIT ?7"
    )
}

/// Driven from the parent ids into [`PARENT_SCHEMA`]'s index; the owner
/// is then a filter on a handful of rows. `INDEXED BY`, not `+s.owner_id`:
/// with an owner term SQLite prefers `sessions_owner` and walks all of
/// the owner's chats (5.5 ms at 10,000, measured), and an unusable index
/// is an error here instead of a silent walk.
fn children_sql(owner: Owner<'_>) -> String {
    let owner = owner_term(owner);
    format!(
        "SELECT {SESSION_COLS}, {EFFECTIVE_WS} FROM sessions s INDEXED BY sessions_parent {WS_JOINS}
         WHERE s.parent_id IN (SELECT value FROM json_each(?2)) AND {owner}
         ORDER BY s.updated_ms DESC, s.session_id DESC"
    )
}

const UNREAD_OF: &str = "SELECT s.session_id FROM sessions s
     LEFT JOIN seen v ON v.session_id = s.session_id AND v.user_id = ?1
     WHERE s.session_id IN (SELECT value FROM json_each(?2))
       AND s.finished_seq > COALESCE(v.seq, 0)";

const LINKS_OF: &str =
    "SELECT sp.session_id, sp.project_id, p.workspace_id FROM session_projects sp
     JOIN projects p ON p.project_id = sp.project_id
     WHERE sp.session_id IN (SELECT value FROM json_each(?1))";

const MARKS_OF: &str =
    "SELECT r.session_id, r.routine_id, o.name, r.run_id, r.status, r.seen, r.promoted, r.untrusted
     FROM routine_runs r JOIN routines o ON o.routine_id = r.routine_id
     WHERE r.session_id IN (SELECT value FROM json_each(?1))";

/// `ids` as one JSON array, the single parameter of an `IN` list.
fn id_list(ids: &[SessionId]) -> Result<String> {
    let ids: Vec<String> = ids.iter().map(ToString::to_string).collect();
    Ok(serde_json::to_string(&ids)?)
}

fn sid(text: String) -> Result<SessionId> {
    text.parse()
        .map_err(|_| StoreError::Corrupt(format!("session id {text}")))
}

impl Store {
    /// One page of the chat list, newest first, and where the next page
    /// starts (`None`: this was the last).
    ///
    /// The order is `(updated_ms, session_id)` descending. A chat whose
    /// `updated_ms` changes while someone pages may be listed twice (its
    /// time went down past the cursor: node clocks are not monotonic) or
    /// be passed over in this walk (its time went up past the cursor; the
    /// next read from the top lists it). No other chat is repeated or lost.
    pub fn sessions_page(&self, q: &PageQuery<'_>) -> Result<(Vec<SessionRow>, Option<Cursor>)> {
        let limit = q.limit.clamp(1, MAX_PAGE);
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&page_sql(q.owner, q.after.is_some()))?;
        let rows = stmt.query_map(
            params![
                q.owner.bind(),
                q.after.map(|c| c.updated_ms),
                q.after.map(|c| c.session_id.to_string()),
                q.workspace,
                q.top_level,
                q.hide_seen_runs,
                // One more than asked: whether there is a next page.
                (limit + 1) as i64,
            ],
            Self::session_from_row,
        )?;
        let mut out = rows
            .map(|r| Self::finish_session(r?))
            .collect::<Result<Vec<_>>>()?;
        let next = if out.len() > limit {
            out.truncate(limit);
            out.last().map(Cursor::of)
        } else {
            None
        };
        Ok((out, next))
    }

    /// The sub-agent chats of `owner` whose parent is one of `parents`,
    /// newest first.
    pub fn children_of(&self, owner: Owner<'_>, parents: &[SessionId]) -> Result<Vec<SessionRow>> {
        if parents.is_empty() {
            return Ok(Vec::new());
        }
        let list = id_list(parents)?;
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&children_sql(owner))?;
        let rows = stmt.query_map(params![owner.bind(), list], Self::session_from_row)?;
        rows.map(|r| Self::finish_session(r?)).collect()
    }

    /// Which of `ids` finished after `viewer` last looked at them: the
    /// same rule as [`Store::unread`], for these chats only.
    pub fn unread_of(&self, viewer: &str, ids: &[SessionId]) -> Result<HashSet<SessionId>> {
        if ids.is_empty() {
            return Ok(HashSet::new());
        }
        let list = id_list(ids)?;
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(UNREAD_OF)?;
        let rows = stmt.query_map(params![viewer, list], |r| r.get::<_, String>(0))?;
        rows.map(|r| sid(r?)).collect()
    }

    /// The project links of `ids` (chats without one are left out).
    pub fn project_links_of(&self, ids: &[SessionId]) -> Result<Vec<ProjectLink>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let list = id_list(ids)?;
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(LINKS_OF)?;
        let rows = stmt.query_map(params![list], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        rows.map(|r| {
            let (s, project_id, workspace_id) = r?;
            Ok(ProjectLink {
                session_id: sid(s)?,
                project_id,
                workspace_id,
            })
        })
        .collect()
    }

    /// [`Store::routine_marks`] in one statement.
    pub fn routine_marks_of(&self, ids: &[SessionId]) -> Result<HashMap<SessionId, RoutineMark>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let list = id_list(ids)?;
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(MARKS_OF)?;
        let rows = stmt.query_map(params![list], |r| {
            Ok((
                r.get::<_, String>(0)?,
                RoutineMark {
                    routine_id: r.get(1)?,
                    name: r.get(2)?,
                    run_id: r.get(3)?,
                    status: r.get(4)?,
                    seen: r.get(5)?,
                    promoted: r.get(6)?,
                    untrusted: r.get(7)?,
                },
            ))
        })?;
        rows.map(|r| {
            let (s, mark) = r?;
            Ok((sid(s)?, mark))
        })
        .collect()
    }
}

#[cfg(test)]
mod bench;
#[cfg(test)]
mod tests;
