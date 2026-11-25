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
    pub(crate) fn bind(self) -> Option<&'a str> {
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
    /// continued: a run chat stays listed while its run is going or
    /// waiting, while its result is unseen, and once the owner continued it.
    pub hide_seen_runs: bool,
    /// Only the chats whose effective project is this one (linked to it,
    /// and in the project's workspace: what the master's
    /// `projects::effective` says). The caller checks that it may see the
    /// project; the owner term is unchanged, so a project never widens
    /// whose chats are listed.
    pub project: Option<&'a str>,
    /// Start after this row; `None`: from the newest.
    pub after: Option<Cursor>,
    /// Clamped to `1..=MAX_PAGE`.
    pub limit: usize,
}

/// The owner term: `?1` is bound either way, so both texts take the same
/// parameters. A separate text, not `?1 IS NULL OR …`, which would keep
/// SQLite off the `sessions_owner` index.
pub(crate) fn owner_term(owner: Owner<'_>) -> &'static str {
    match owner {
        Owner::User(_) => "s.owner_id = ?1",
        Owner::Everyone => "?1 IS NULL",
    }
}

/// `col` is stored as text in exactly `SessionId::to_string()`'s
/// spelling (lower-case hex, hyphenated, 36 characters). Every writer
/// stores that; a row that fails this (another spelling, NULL, a BLOB,
/// garbage) is corrupt data, and the chat list leaves it out in SQL: the
/// keyset compares stored text, so another spelling as a cursor would page
/// in a loop, and a value that is not text cannot be read as an id at all.
/// A residual filter: it does not change which index a query uses.
pub(crate) fn canonical_id(col: &str) -> String {
    const HEX: &str = "[0-9a-f]";
    let group = |n: usize| HEX.repeat(n);
    let pattern = [group(8), group(4), group(4), group(4), group(12)].join("-");
    // GLOB stops at a NUL, so text that goes on after one would pass on its
    // first 36 characters: the byte length closes that.
    format!(
        "(typeof({col}) = 'text' AND length(CAST({col} AS BLOB)) = 36 AND {col} GLOB '{pattern}')"
    )
}

/// Which chats a page lists by project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ByProject {
    /// `PageQuery.project` (`?8`): one project's, or (`None`) any.
    Asked,
    /// Only chats in no project: no link whose project is in the chat's
    /// effective workspace (the master's `projects::effective` is `None`).
    Loose,
}

fn page_sql(owner: Owner<'_>, after: bool, project: bool, by: ByProject) -> String {
    // A project's page is driven by the project's links, the owner a filter
    // only (`+`): as a plain term SQLite walks the owner's chats through
    // `sessions_owner` and checks each against the links, which for a
    // project holding few of them reads all of them (see the bench).
    let (owner, link_owner) = if project {
        (
            owner_filter(owner),
            match owner {
                Owner::User(_) => "o.owner_id = ?1",
                Owner::Everyone => "?1 IS NULL",
            },
        )
    } else {
        (owner_term(owner), "")
    };
    let canonical = canonical_id("s.session_id");
    // SQLite seeks `sessions_owner` to `updated_ms <= ?2` from this OR
    // (the plan test checks it); the id breaks ties.
    let after = if after {
        "(s.updated_ms < ?2 OR (s.updated_ms = ?2 AND s.session_id < ?3))"
    } else {
        "?2 IS NULL AND ?3 IS NULL"
    };
    // A project's chats: its links (`session_projects_project`), while the
    // project is in the chat's effective workspace (a chat that moved
    // workspace keeps an inert link). A separate text, not `?8 IS NULL OR`,
    // so that the links can drive the query (the plan test checks it). The
    // owner is checked inside the links' subquery too, so another member's
    // chat in a shared project costs one key lookup and nothing more (a
    // team project of 40,000 others' chats: 9 ms instead of 21, bench).
    // Only speed depends on that inner check: the outer owner filter alone
    // keeps every other owner's chat out, so no result changes without it
    // (`the_owner_check_inside_the_links_saves_work` counts the work).
    let project = if project {
        format!(
            "s.session_id IN (SELECT sp.session_id FROM session_projects sp
                 JOIN sessions o ON o.session_id = sp.session_id
                 WHERE sp.project_id = ?8 AND {link_owner})
           AND {EFFECTIVE_WS} = (SELECT p.workspace_id FROM projects p WHERE p.project_id = ?8)"
        )
    } else if by == ByProject::Loose {
        format!(
            "?8 IS NULL AND NOT EXISTS (
                 SELECT 1 FROM session_projects sp JOIN projects p ON p.project_id = sp.project_id
                  WHERE sp.session_id = s.session_id AND p.workspace_id = {EFFECTIVE_WS})"
        )
    } else {
        "?8 IS NULL".to_string()
    };
    format!(
        "SELECT {SESSION_COLS}, {EFFECTIVE_WS} FROM sessions s {WS_JOINS}
         WHERE {owner} AND {after} AND {canonical}
           AND (?4 IS NULL OR {EFFECTIVE_WS} = ?4)
           AND {project}
           AND (?5 = 0 OR (s.parent_id IS NULL AND s.incognito = 0))
           AND (?6 = 0 OR NOT EXISTS (
                 SELECT 1 FROM routine_runs r JOIN routines o ON o.routine_id = r.routine_id
                 WHERE r.session_id = s.session_id AND r.seen <> 0 AND r.promoted = 0
                   AND r.status NOT IN {ACTIVE}))
         ORDER BY s.updated_ms DESC, s.session_id DESC LIMIT ?7"
    )
}

/// How deep [`Store::children_of`] follows sub-agents of sub-agents. The
/// node has no depth limit of its own; 16 levels is far past any real
/// delegation, and it is what ends a corrupt `parent_id` loop.
pub const MAX_DEPTH: usize = 16;
/// The most rows [`Store::children_of`] returns. A page has at most
/// [`MAX_PAGE`] parents; 2,000 is ten sub-agent chats under each of them,
/// a response of a few hundred KB. Past it the newest are kept and the
/// caller is told.
pub const MAX_DESCENDANTS: usize = 2_000;
/// The most rows the walk below a page reads. Each chat appears at most
/// once per depth (it has one parent, which appears at most once at the
/// depth above, by induction from the distinct seeds), so a walk of
/// fewer than this many rows is complete, and one that reaches it has
/// found more than [`MAX_DESCENDANTS`] distinct chats: `truncated` is then
/// true either way. Only past it are the kept rows the newest of those
/// found rather than of all. Measured worst case (a walk cut here): see
/// `docs/store-readers.md`.
pub const MAX_WALK: usize = MAX_DEPTH * (MAX_DESCENDANTS + 1);

/// Every sub-agent chat below the parents, at any depth, through
/// [`PARENT_SCHEMA`]'s index. The owner term is checked at every step, so
/// a chain stops at a chat of somebody else. A loop of `parent_id`s ends
/// at [`MAX_DEPTH`] (each chat once per depth, see [`MAX_WALK`]), so
/// `UNION ALL`: a `UNION` would drop nothing. The final `IN` lists each
/// chat once; it is sorted newest first and cut to one more than
/// [`MAX_DESCENDANTS`], so the newest survive whatever order the walk
/// finds them in.
///
/// `INDEXED BY`, not `+s.owner_id`: with an owner term SQLite prefers
/// `sessions_owner` and walks all of the owner's chats (5.5 ms at 10,000,
/// measured), and an unusable index is an error here instead of a silent
/// walk.
fn children_sql(owner: Owner<'_>) -> String {
    let owner = owner_term(owner);
    let canonical = canonical_id("s.session_id");
    format!(
        "WITH RECURSIVE tree(id, depth) AS (
             SELECT s.session_id, 1 FROM sessions s INDEXED BY sessions_parent
              WHERE s.parent_id IN (SELECT value FROM json_each(?2)) AND {owner}
                AND {canonical}
             UNION ALL
             SELECT s.session_id, t.depth + 1 FROM tree t
               JOIN sessions s INDEXED BY sessions_parent ON s.parent_id = t.id
              WHERE t.depth < {MAX_DEPTH} AND {owner} AND {canonical}
             LIMIT ?3)
         SELECT {SESSION_COLS}, {EFFECTIVE_WS} FROM sessions s {WS_JOINS}
          WHERE s.session_id IN (SELECT id FROM tree)
          ORDER BY s.updated_ms DESC, s.session_id DESC LIMIT ?4"
    )
}

/// The owner as a filter only (`+` keeps it off `sessions_owner`): the
/// id-list annotations look each id up by key and drop chats of somebody
/// else. With a plain owner term SQLite walks all of the owner's chats
/// through `sessions_owner` instead (16 ms at 10,000, measured).
pub(crate) fn owner_filter(owner: Owner<'_>) -> &'static str {
    match owner {
        Owner::User(_) => "+s.owner_id = ?1",
        Owner::Everyone => "?1 IS NULL",
    }
}

/// The id-list annotations answer only for chats of `owner`, whatever
/// ids a caller passes.
fn unread_sql(owner: Owner<'_>) -> String {
    let owner = owner_filter(owner);
    format!(
        "SELECT s.session_id FROM sessions s
         LEFT JOIN seen v ON v.session_id = s.session_id AND v.user_id = ?2
         WHERE s.session_id IN (SELECT value FROM json_each(?3)) AND {owner}
           AND s.finished_seq > COALESCE(v.seq, 0)"
    )
}

fn links_sql(owner: Owner<'_>) -> String {
    let owner = owner_filter(owner);
    format!(
        "SELECT sp.session_id, sp.project_id, p.workspace_id FROM session_projects sp
         JOIN sessions s ON s.session_id = sp.session_id
         JOIN projects p ON p.project_id = sp.project_id
         WHERE sp.session_id IN (SELECT value FROM json_each(?2)) AND {owner}"
    )
}

fn marks_sql(owner: Owner<'_>) -> String {
    let owner = owner_filter(owner);
    format!(
        "SELECT r.session_id, r.routine_id, o.name, r.run_id, r.status, r.seen, r.promoted, r.untrusted
         FROM routine_runs r JOIN routines o ON o.routine_id = r.routine_id
         JOIN sessions s ON s.session_id = r.session_id
         WHERE r.session_id IN (SELECT value FROM json_each(?2)) AND {owner}"
    )
}

/// What [`Store::children_of`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Descendants {
    /// Newest first.
    pub rows: Vec<SessionRow>,
    /// More than [`MAX_DESCENDANTS`] distinct chats: only the newest are in
    /// `rows`.
    pub truncated: bool,
}

/// `ids` as one JSON array, the single parameter of an `IN` list.
pub(crate) fn id_list(ids: &[SessionId]) -> Result<String> {
    let ids: Vec<String> = ids.iter().map(ToString::to_string).collect();
    Ok(serde_json::to_string(&ids)?)
}

/// A row that passed `canonical_id`, with its id parsed. Failing here is a
/// bug in the predicate, not data (the predicate's unit test covers every
/// odd value found so far, an embedded NUL included): the error gives the length only, never
/// the stored value.
pub(crate) fn listed((text, mut row): (String, SessionRow)) -> Result<SessionRow> {
    row.session_id = text.parse().map_err(|_| {
        StoreError::Corrupt(format!(
            "a chat id of {} bytes passed the canonical check but is not a session id",
            text.len()
        ))
    })?;
    Ok(row)
}

pub(crate) fn sid(text: String) -> Result<SessionId> {
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
    ///
    /// A chat whose stored id is not in canonical form is left out in SQL
    /// (see `canonical_id`); `rizzy-harness doctor` is to report such rows.
    pub fn sessions_page(&self, q: &PageQuery<'_>) -> Result<(Vec<SessionRow>, Option<Cursor>)> {
        self.page(q, ByProject::Asked)
    }

    /// [`Store::sessions_page`] of the chats in no project (the project
    /// page's "Add a chat" list, review F5 of slice 2+3): `q.project` must
    /// be `None`; the owner, workspace and cursor rules are the same.
    pub fn loose_sessions_page(
        &self,
        q: &PageQuery<'_>,
    ) -> Result<(Vec<SessionRow>, Option<Cursor>)> {
        if q.project.is_some() {
            return Err(StoreError::Refused(
                "a page of chats in no project names no project".into(),
            ));
        }
        self.page(q, ByProject::Loose)
    }

    fn page(&self, q: &PageQuery<'_>, by: ByProject) -> Result<(Vec<SessionRow>, Option<Cursor>)> {
        let limit = q.limit.clamp(1, MAX_PAGE);
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&page_sql(
            q.owner,
            q.after.is_some(),
            q.project.is_some(),
            by,
        ))?;
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
                q.project,
            ],
            Self::session_from_row,
        )?;
        let mut out = rows.map(|r| listed(r?)).collect::<Result<Vec<_>>>()?;
        let next = if out.len() > limit {
            out.truncate(limit);
            out.last().map(Cursor::of)
        } else {
            None
        };
        Ok((out, next))
    }

    /// Every sub-agent chat of `owner` below `parents`, at any depth up to
    /// [`MAX_DEPTH`], newest first; a chain stops at a chat that is not
    /// `owner`'s. A child whose stored id is not in canonical form (see
    /// `canonical_id`) is not returned and its subtree is not walked.
    ///
    /// Deliberately unlike today's list, a child is returned whatever its
    /// own effective workspace: it is shown nested under its parent, so it
    /// follows the parent's workspace (a child created before its parent
    /// was moved keeps its own mapping, and would otherwise vanish).
    pub fn children_of(&self, owner: Owner<'_>, parents: &[SessionId]) -> Result<Descendants> {
        if parents.is_empty() {
            return Ok(Descendants {
                rows: Vec::new(),
                truncated: false,
            });
        }
        let list = id_list(parents)?;
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&children_sql(owner))?;
        let rows = stmt.query_map(
            params![
                owner.bind(),
                list,
                MAX_WALK as i64,
                (MAX_DESCENDANTS + 1) as i64
            ],
            Self::session_from_row,
        )?;
        let mut rows = rows.map(|r| listed(r?)).collect::<Result<Vec<_>>>()?;
        let truncated = rows.len() > MAX_DESCENDANTS;
        rows.truncate(MAX_DESCENDANTS);
        Ok(Descendants { rows, truncated })
    }

    /// Which of `owner`'s chats among `ids` finished after `viewer` last
    /// looked at them: the same rule as [`Store::unread`].
    pub fn unread_of(
        &self,
        owner: Owner<'_>,
        viewer: &str,
        ids: &[SessionId],
    ) -> Result<HashSet<SessionId>> {
        if ids.is_empty() {
            return Ok(HashSet::new());
        }
        let list = id_list(ids)?;
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&unread_sql(owner))?;
        let rows = stmt.query_map(params![owner.bind(), viewer, list], |r| {
            r.get::<_, String>(0)
        })?;
        rows.map(|r| sid(r?)).collect()
    }

    /// The project links of `owner`'s chats among `ids` (chats without one
    /// are left out).
    pub fn project_links_of(
        &self,
        owner: Owner<'_>,
        ids: &[SessionId],
    ) -> Result<Vec<ProjectLink>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let list = id_list(ids)?;
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&links_sql(owner))?;
        let rows = stmt.query_map(params![owner.bind(), list], |r| {
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

    /// [`Store::routine_marks`] of `owner`'s chats among `ids`, in one
    /// statement.
    pub fn routine_marks_of(
        &self,
        owner: Owner<'_>,
        ids: &[SessionId],
    ) -> Result<HashMap<SessionId, RoutineMark>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let list = id_list(ids)?;
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&marks_sql(owner))?;
        let rows = stmt.query_map(params![owner.bind(), list], |r| {
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
pub(crate) mod tests;
