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

fn owned_sql() -> String {
    let canonical = canonical_id("s.session_id");
    format!(
        "SELECT {SESSION_COLS}, {EFFECTIVE_WS} FROM sessions s {WS_JOINS}
         WHERE s.owner_id IN (SELECT value FROM json_each(?1)) AND {canonical}
           AND (?2 IS NULL OR {EFFECTIVE_WS} = ?2)
         ORDER BY s.updated_ms DESC, s.session_id DESC"
    )
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
    /// given), whatever kind: what a member's removal, a workspace's or an
    /// account's deletion and the revocation sweep act on.
    pub fn sessions_owned_by(
        &self,
        owners: &[String],
        workspace: Option<&str>,
    ) -> Result<Vec<SessionRow>> {
        if owners.is_empty() {
            return Ok(Vec::new());
        }
        let list = owner_list(owners)?;
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&owned_sql())?;
        let rows = stmt.query_map(params![list, workspace], Self::session_from_row)?;
        rows.map(|r| listed(r?)).collect()
    }
}

#[cfg(test)]
mod tests;
