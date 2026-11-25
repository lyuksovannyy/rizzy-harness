//! Chats read as a set, scoped in SQL (FIX_PLAN 4.5 slice 4,
//! `docs/store-readers.md` §5): what the request paths that once read every
//! chat (`sessions()`) or one account's every chat per member or per hit
//! actually need, one statement each.
//!
//! The owner (or the owners) and the workspace are terms of the `WHERE`;
//! an id list is bound as one JSON array (`json_each`), so its size never
//! changes the statement. A chat whose stored id is not canonical is left
//! out, as in the paged list (`session_pages::canonical_id`).

use std::collections::HashMap;

use harness_core::SessionId;
use rusqlite::params;

use crate::session_pages::{Owner, canonical_id, id_list, listed, owner_filter, owner_term, sid};
use crate::{EFFECTIVE_WS, Result, SESSION_COLS, SessionRow, Store, WS_JOINS};

/// `owners` as one JSON array.
fn owner_list(owners: &[String]) -> Result<String> {
    Ok(serde_json::to_string(owners)?)
}

/// Whether the chat is `owner`'s, as 0 or 1: a chat nobody owns is
/// nobody's but the API token's (`Principal::can_see`).
fn mine(owner: Owner<'_>) -> &'static str {
    match owner {
        Owner::User(_) => "COALESCE(s.owner_id = ?1, 0)",
        Owner::Everyone => "?1 IS NULL",
    }
}

fn titles_sql(owner: Owner<'_>) -> String {
    let mine = mine(owner);
    let canonical = canonical_id("s.session_id");
    format!(
        "SELECT s.session_id, CASE WHEN {mine} THEN s.title END FROM sessions s
         WHERE s.session_id IN (SELECT value FROM json_each(?2)) AND {canonical}"
    )
}

fn counts_sql(owner: Owner<'_>) -> String {
    let owner = owner_term(owner);
    format!(
        "SELECT sp.project_id, COUNT(*) FROM sessions s {WS_JOINS}
           JOIN session_projects sp ON sp.session_id = s.session_id
           JOIN projects p ON p.project_id = sp.project_id
         WHERE {owner} AND s.parent_id IS NULL AND s.incognito = 0
           AND (?2 IS NULL OR p.workspace_id = ?2)
           AND p.workspace_id = {EFFECTIVE_WS}
         GROUP BY sp.project_id"
    )
}

fn recent_sql(owner: Owner<'_>) -> String {
    let owner = owner_term(owner);
    let canonical = canonical_id("s.session_id");
    format!(
        "SELECT {SESSION_COLS}, {EFFECTIVE_WS} FROM sessions s {WS_JOINS}
         WHERE {owner} AND {canonical}
           AND (?2 IS NULL OR {EFFECTIVE_WS} = ?2)
           AND s.parent_id IS NULL AND s.incognito = 0
           AND NOT EXISTS (SELECT 1 FROM routine_runs r
                           JOIN routines o ON o.routine_id = r.routine_id
                           WHERE r.session_id = s.session_id)
         ORDER BY s.updated_ms DESC, s.session_id DESC LIMIT ?3 OFFSET ?4"
    )
}

fn by_ids_sql(owner: Owner<'_>) -> String {
    let owner = owner_filter(owner);
    let canonical = canonical_id("s.session_id");
    format!(
        "SELECT {SESSION_COLS}, {EFFECTIVE_WS} FROM sessions s {WS_JOINS}
         WHERE s.session_id IN (SELECT value FROM json_each(?2)) AND {owner} AND {canonical}"
    )
}

/// No canonical-id filter here (review F1 of slice 4): these chats are
/// stopped, cancelled or deleted, and a chat whose stored id is in another
/// spelling must go with its owner like any other. Its stored text comes
/// back beside the row ([`OwnedChat::stored_id`]).
///
/// The last column says, for a row not in the canonical spelling only,
/// whether another row's id is a spelling of the same id (review R2-1):
/// every command, tombstone and cleanup keyed by that id would reach the
/// other chat, so such a row is never acted on. Decided in this statement,
/// so no write can come between the check and the answer. A canonical row
/// is never marked (the `CASE` answers 0 before the search runs), and the
/// search, a walk of every row, runs only for odd rows, which no writer
/// makes.
fn owned_sql() -> String {
    let canonical = canonical_id("s.session_id");
    let norm = |c: &str| {
        format!(
            "replace(replace(replace(replace(lower({c}), 'urn:uuid:', ''), '{{', ''), '}}', ''), '-', '')"
        )
    };
    let (mine, other) = (norm("s.session_id"), norm("o.session_id"));
    format!(
        "SELECT {SESSION_COLS}, {EFFECTIVE_WS},
                CASE WHEN {canonical} THEN 0 ELSE EXISTS (
                    SELECT 1 FROM sessions o
                     WHERE o.rowid <> s.rowid AND {other} = {mine}) END
         FROM sessions s {WS_JOINS}
         WHERE s.owner_id IN (SELECT value FROM json_each(?1))
           AND (?2 IS NULL OR {EFFECTIVE_WS} = ?2)
         ORDER BY s.updated_ms DESC, s.session_id DESC"
    )
}

/// A chat [`Store::sessions_owned_by`] found: its row (the id parsed), and
/// the id exactly as stored, which a destructive write must key on when it
/// is not the canonical spelling (no writer stores one; a hand edit or a
/// corrupt file can).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedChat {
    pub row: SessionRow,
    pub stored_id: String,
}

impl OwnedChat {
    /// Stored as `SessionId::to_string()` writes it: every lookup by id
    /// finds it.
    pub fn canonical(&self) -> bool {
        self.stored_id == self.row.session_id.to_string()
    }
}

impl Store {
    /// For each chat among `ids` that exists: `Some(title)` when it is
    /// `owner`'s, `None` when it is somebody else's. Another account's
    /// title never leaves the store; that the chat exists does (a board
    /// card shows "private"). Ids that are no chat are left out.
    pub fn chat_titles_of(
        &self,
        owner: Owner<'_>,
        ids: &[SessionId],
    ) -> Result<HashMap<SessionId, Option<String>>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let list = id_list(ids)?;
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&titles_sql(owner))?;
        let rows = stmt.query_map(params![owner.bind(), list], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })?;
        rows.map(|r| {
            let (id, title) = r?;
            Ok((sid(id)?, title))
        })
        .collect()
    }

    /// `owner`'s top-level chats (no sub-agent, no incognito chat) per
    /// effective project: linked, and the project in the chat's effective
    /// workspace (the master's `projects::effective`). With `workspace`,
    /// only that workspace's projects are counted.
    pub fn project_chat_counts(
        &self,
        owner: Owner<'_>,
        workspace: Option<&str>,
    ) -> Result<HashMap<String, usize>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&counts_sql(owner))?;
        let rows = stmt.query_map(params![owner.bind(), workspace], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        rows.map(|r| {
            let (p, n) = r?;
            Ok((p, n as usize))
        })
        .collect()
    }

    /// The empty search's chats: `owner`'s top-level chats in `workspace`
    /// (effective), newest first, leaving out every chat a routine run
    /// started (they would crowd out the rest), `offset` rows skipped.
    pub fn recent_chats(
        &self,
        owner: Owner<'_>,
        workspace: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<SessionRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&recent_sql(owner))?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let offset = i64::try_from(offset).unwrap_or(i64::MAX);
        let rows = stmt.query_map(
            params![owner.bind(), workspace, limit, offset],
            Self::session_from_row,
        )?;
        rows.map(|r| listed(r?)).collect()
    }

    /// `owner`'s chats among `ids`, by id (others, and ids that are no
    /// chat, are left out).
    pub fn sessions_by_ids(
        &self,
        owner: Owner<'_>,
        ids: &[SessionId],
    ) -> Result<HashMap<SessionId, SessionRow>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let list = id_list(ids)?;
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&by_ids_sql(owner))?;
        let rows = stmt.query_map(params![owner.bind(), list], Self::session_from_row)?;
        rows.map(|r| {
            let row = listed(r?)?;
            Ok((row.session_id, row))
        })
        .collect()
    }

    /// Every chat owned by one of `owners` (in `workspace`, effective, when
    /// given), whatever kind and however its id is spelled: what a member's
    /// removal, a workspace's or an account's deletion and the revocation
    /// sweep act on. A row whose id does not parse as a session id at all
    /// cannot be acted on, nor can an odd spelling of an id another row
    /// also holds (see `owned_sql`): both are skipped, and one warning per
    /// call gives how many (never the stored text), so one corrupt row does
    /// not stop the operation for every other chat.
    pub fn sessions_owned_by(
        &self,
        owners: &[String],
        workspace: Option<&str>,
    ) -> Result<Vec<OwnedChat>> {
        if owners.is_empty() {
            return Ok(Vec::new());
        }
        let list = owner_list(owners)?;
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&owned_sql())?;
        let rows = stmt.query_map(params![list, workspace], |r| {
            // A stored id that is not text (NULL, a BLOB) reads as None.
            Ok((
                r.get::<_, Option<String>>(0).ok().flatten(),
                Self::session_from_row_after_id(r)?,
                r.get::<_, bool>(16)?,
            ))
        })?;
        let mut out = Vec::new();
        let (mut skipped, mut shared) = (0usize, 0usize);
        for r in rows {
            let (stored, mut row, twin) = r?;
            if twin {
                shared += 1;
                continue;
            }
            match stored.as_deref().map(str::parse::<SessionId>) {
                Some(Ok(id)) => {
                    row.session_id = id;
                    out.push(OwnedChat {
                        row,
                        stored_id: stored.unwrap_or_default(),
                    });
                }
                _ => skipped += 1,
            }
        }
        if skipped + shared > 0 {
            tracing::warn!(
                skipped,
                shared,
                "chats left out, not stopped or deleted: stored ids that are no session id (skipped), \
                 or another spelling of another row's id (shared; acting on it would reach that chat)"
            );
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests;
