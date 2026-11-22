//! Where a fork came from (ADOPTION_PLAN C.2, docs/fork.md): the
//! `session_forks` table, and writing a fork's events and images in one
//! transaction.
//!
//! The row is written in the transaction that stores the fork's first event
//! (`session_created.forked`), so the chat and its lineage appear together;
//! it goes with the fork. It has no foreign key to `sessions`: the parent
//! may be deleted and the fork lives on, and no row is deleted because its
//! parent was. `through_seq` is what every reader of a fork's log needs to
//! know which events are copies (`harness_core::fork::inherited`): the
//! search index and the usage sums here, the master's mirror when it
//! applies an event.

use harness_core::event::EventKind;
use harness_core::{RawEvent, SessionId};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{AppendOutcome, Result, Store, StoreError};

/// Schema version 3 (the second migration after the schema was frozen,
/// PLAN.md §0): a database of version 1 or 2 gets this table on its next
/// open.
pub const SCHEMA: &str = r#"
-- A chat that is a fork of another (ADOPTION_PLAN C.2): its first
-- `through_seq` events are copies of the parent's, at the same seqs. Written
-- with the fork's first event, from `session_created.forked`. No foreign
-- key: the parent may be deleted, the fork lives on; deleting the parent
-- deletes no fork.
CREATE TABLE IF NOT EXISTS session_forks (
    session_id  TEXT PRIMARY KEY,
    parent_id   TEXT NOT NULL,
    through_seq INTEGER NOT NULL,
    created_ms  INTEGER NOT NULL
);
"#;

/// A fork's lineage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForkRow {
    /// The chat it was forked from (it may be gone).
    pub parent: SessionId,
    /// The seq of the parent's `turn_ended` the copy stops at.
    pub through_seq: u64,
    pub created_ms: i64,
}

/// Called by `Store::append` for a `session_created`: the lineage row, when
/// the event says the chat is a fork. A claim that cannot be true (a chat
/// that is its own parent, a copy of fewer than two events, a parent that is
/// not an id) is not recorded: the chat is then an ordinary one. A node that
/// lies about it can only hide its own chat's effects from the master.
pub(crate) fn on_created(tx: &Connection, sid: &str, e: &RawEvent) -> Result<()> {
    let Some(f) = e.payload.get("forked") else {
        return Ok(());
    };
    let parent = f.get("parent").and_then(|v| v.as_str());
    let through = f.get("through_seq").and_then(|v| v.as_u64());
    let (Some(parent), Some(through)) = (parent, through) else {
        return Ok(());
    };
    if parent == sid || through < 2 || parent.parse::<SessionId>().is_err() {
        return Ok(());
    }
    tx.execute(
        "INSERT OR IGNORE INTO session_forks (session_id, parent_id, through_seq, created_ms)
         VALUES (?1, ?2, ?3, ?4)",
        params![sid, parent, through as i64, e.ts_ms],
    )?;
    Ok(())
}

/// Called by `Store::delete_session` inside its transaction: the fork's own
/// row goes with it (never the rows of its forks).
pub(crate) fn on_session_deleted(tx: &rusqlite::Transaction, sid: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM session_forks WHERE session_id = ?1",
        params![sid],
    )?;
    Ok(())
}

/// The chat `sid` was forked from, as its lineage row says.
pub(crate) fn parent(conn: &Connection, sid: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT parent_id FROM session_forks WHERE session_id = ?1",
            params![sid],
            |r| r.get::<_, String>(0),
        )
        .optional()?)
}

/// How many of the chat's first events are its parent's copies; 0 for a
/// chat that is no fork.
pub(crate) fn through(conn: &Connection, sid: &str) -> Result<u64> {
    Ok(conn
        .query_row(
            "SELECT through_seq FROM session_forks WHERE session_id = ?1",
            params![sid],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        .map_or(0, |n| n.max(0) as u64))
}

/// The hashes of the images `events` refer to: a person's message's and a
/// `read` result's.
fn image_hashes(events: &[RawEvent]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in events {
        let mut take = |v: Option<&serde_json::Value>| {
            if let Some(s) = v.and_then(|v| v.as_str())
                && !out.iter().any(|o| o == s)
            {
                out.push(s.to_string());
            }
        };
        match e.kind.as_str() {
            "user_message" => {
                for i in e
                    .payload
                    .get("images")
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flatten()
                {
                    take(i.get("sha256"));
                }
            }
            "tool_call_finished" => take(e.payload.get("image").and_then(|i| i.get("sha256"))),
            _ => {}
        }
    }
    out
}

impl Store {
    /// The lineage of `sid`, when it is a fork.
    pub fn fork_of(&self, sid: SessionId) -> Result<Option<ForkRow>> {
        let conn = self.lock();
        let row: Option<(String, i64, i64)> = conn
            .query_row(
                "SELECT parent_id, through_seq, created_ms FROM session_forks WHERE session_id = ?1",
                params![sid.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        row.map(|(parent, through, created_ms)| {
            Ok(ForkRow {
                parent: parent
                    .parse()
                    .map_err(|_| StoreError::Corrupt(format!("session id {parent}")))?,
                through_seq: through.max(0) as u64,
                created_ms,
            })
        })
        .transpose()
    }

    /// How many chats (forks) descend from the original chat `sid` belongs
    /// to: walk up the lineage to the oldest chat it names (a deleted parent
    /// still names its own), count everything below it. What a new fork's
    /// title number is counted from (`harness_core::fork::title_nth`). Both
    /// walks are bounded (a node that lies in `forked` can make a cycle).
    pub fn forks_of_original(&self, sid: SessionId) -> Result<u64> {
        const WALK: usize = 10_000;
        let conn = self.lock();
        let parent_of = |id: &str| -> Result<Option<String>> {
            Ok(conn
                .query_row(
                    "SELECT parent_id FROM session_forks WHERE session_id = ?1",
                    params![id],
                    |r| r.get::<_, String>(0),
                )
                .optional()?)
        };
        let mut root = sid.to_string();
        let mut above = std::collections::HashSet::from([root.clone()]);
        while above.len() < WALK {
            match parent_of(&root)? {
                // A parent seen before is a cycle: the walk ends there.
                Some(p) if above.insert(p.clone()) => root = p,
                _ => break,
            }
        }
        let mut seen = std::collections::HashSet::from([root.clone()]);
        let mut queue = vec![root];
        let mut stmt = conn.prepare("SELECT session_id FROM session_forks WHERE parent_id = ?1")?;
        while let Some(at) = queue.pop() {
            let kids: Vec<String> = stmt
                .query_map(params![at], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?;
            for k in kids {
                if seen.len() < WALK && seen.insert(k.clone()) {
                    queue.push(k);
                }
            }
        }
        // The original itself is not a fork.
        Ok(seen.len() as u64 - 1)
    }

    /// Write a fork: all its `events` (`harness_core::fork::build`) and the
    /// `session_images` rows of `parent` that its copied events refer to,
    /// under the fork's id, in ONE transaction. Either the whole fork is
    /// there or nothing is: a crash in between leaves no half chat. Refused
    /// (and nothing written) when an event is not taken as the next of a new
    /// chat, when the first is not a `session_created` that names `parent`,
    /// and when the fork exists.
    pub fn write_fork(&self, owner: &str, parent: SessionId, events: &[RawEvent]) -> Result<()> {
        let first = events
            .first()
            .ok_or_else(|| StoreError::Refused("a fork has no events".into()))?;
        let named = matches!(first.typed(), Ok(EventKind::SessionCreated(c))
            if c.forked.is_some_and(|f| f.parent == parent));
        if !named || events.iter().any(|e| e.session_id != first.session_id) {
            return Err(StoreError::Refused(
                "a fork starts with the creation that names its parent".into(),
            ));
        }
        let new = first.session_id.to_string();
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        for e in events {
            match Self::append_in(&tx, owner, e)? {
                AppendOutcome::Applied => {}
                other => {
                    return Err(StoreError::Refused(format!(
                        "event {} of a fork was not taken ({other:?})",
                        e.seq
                    )));
                }
            }
        }
        for sha in image_hashes(events) {
            tx.execute(
                "INSERT OR IGNORE INTO session_images (session_id, sha256, mime, name, bytes)
                 SELECT ?1, sha256, mime, name, bytes FROM session_images
                 WHERE session_id = ?2 AND sha256 = ?3",
                params![new, parent.to_string(), sha],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}
