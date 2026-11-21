//! Projects whose folder a node is cloning (ADOPTION_PLAN X.27 step 3,
//! PLAN.md T86 and T77): the `project_clones` table. A row is not a
//! project: no lookup of a project by id reads it.

use rusqlite::{OptionalExtension, params};

use crate::projects::insert_project;
use crate::{NewProject, ProjectRow, Result, Store, StoreError};

/// Schema version 2 (the first migration after the schema was frozen,
/// PLAN.md §0): a database of version 1 gets this table on its next open.
pub const SCHEMA: &str = r#"
-- A project whose folder a node is cloning now, or whose clone failed
-- (ADOPTION_PLAN X.27 step 3, PLAN.md T86 and T77). Not a project: no
-- route that takes a project id reads this table, so nothing can start a
-- chat in it, link it, give it a folder or write its memory or board. When
-- the clone ends well the row is deleted and the project written, with the
-- same id, in one transaction; when it fails the row stays `failed` with
-- the reason for its starter to read and dismiss. At the master's start
-- every `cloning` row becomes `failed` (no clone outlives the master's
-- wait for it).
CREATE TABLE IF NOT EXISTS project_clones (
    project_id   TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    node_id      TEXT NOT NULL,
    host         TEXT NOT NULL,
    full_name    TEXT NOT NULL,
    created_by   TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    started_ms   INTEGER NOT NULL,
    state        TEXT NOT NULL CHECK (state IN ('cloning', 'failed')),
    reason       TEXT,
    ended_ms     INTEGER,
    CHECK ((state = 'failed') = (reason IS NOT NULL))
);
CREATE INDEX IF NOT EXISTS project_clones_ws ON project_clones(workspace_id);
-- One clone of a repository per person and node at a time.
CREATE UNIQUE INDEX IF NOT EXISTS project_clones_one
    ON project_clones(created_by, node_id, host, lower(full_name)) WHERE state = 'cloning';
"#;

/// Failed clones kept per person; older ones go when a new one fails.
pub const FAILED_KEPT: usize = 10;

/// A clone, running or failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneRow {
    /// The id the project gets when the clone ends well.
    pub project_id: String,
    pub workspace_id: String,
    pub name: String,
    pub node_id: String,
    pub host: String,
    pub full_name: String,
    pub created_by: String,
    pub started_ms: i64,
    /// `None` while cloning; the reason once failed.
    pub failed: Option<String>,
}

/// A clone to record as running.
#[derive(Debug, Clone)]
pub struct NewClone {
    pub project_id: String,
    pub workspace_id: String,
    pub name: String,
    pub node_id: String,
    pub host: String,
    pub full_name: String,
    pub created_by: String,
}

const COLS: &str =
    "project_id, workspace_id, name, node_id, host, full_name, created_by, started_ms, reason";

fn clone_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<CloneRow> {
    Ok(CloneRow {
        project_id: r.get(0)?,
        workspace_id: r.get(1)?,
        name: r.get(2)?,
        node_id: r.get(3)?,
        host: r.get(4)?,
        full_name: r.get(5)?,
        created_by: r.get(6)?,
        started_ms: r.get(7)?,
        failed: r.get(8)?,
    })
}

/// Refused while `user` clones `host`'s `full_name` (any case) onto `node`.
fn check_not_cloning(
    conn: &rusqlite::Connection,
    user: &str,
    node: &str,
    host: &str,
    full_name: &str,
) -> Result<()> {
    let busy: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM project_clones WHERE state = 'cloning' AND created_by = ?1 \
         AND node_id = ?2 AND host = ?3 AND lower(full_name) = lower(?4))",
        params![user, node, host, full_name],
        |r| r.get(0),
    )?;
    if busy {
        return Err(StoreError::Refused(format!(
            "{host}/{full_name} is being cloned onto this node for you already; it shows as \
             cloning in your projects until it ends"
        )));
    }
    Ok(())
}

impl Store {
    /// `Refused` (said for the person) while `user` clones `full_name` of
    /// `host` onto `node` already: the read-only form of the check
    /// [`Store::start_clone`] makes, for the request to make before it takes
    /// anything else.
    pub fn check_not_cloning(
        &self,
        user: &str,
        node: &str,
        host: &str,
        full_name: &str,
    ) -> Result<()> {
        check_not_cloning(&self.lock(), user, node, host, full_name)
    }

    /// Record `c` as cloning. Refused (`StoreError::Refused`, said for the
    /// person) while the same person clones the same repository (host,
    /// name in any case) onto the same node, or when the workspace has
    /// `max` projects counting running clones.
    pub fn start_clone(&self, c: &NewClone, max: usize) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        check_not_cloning(&tx, &c.created_by, &c.node_id, &c.host, &c.full_name)?;
        let n: i64 = tx.query_row(
            "SELECT (SELECT COUNT(*) FROM projects WHERE workspace_id = ?1) + \
             (SELECT COUNT(*) FROM project_clones WHERE workspace_id = ?1 AND state = 'cloning')",
            params![c.workspace_id],
            |r| r.get(0),
        )?;
        if n as usize >= max {
            return Err(StoreError::Refused(format!(
                "a workspace has at most {max} projects"
            )));
        }
        tx.execute(
            "INSERT INTO project_clones (project_id, workspace_id, name, node_id, host, full_name, \
             created_by, started_ms, state) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'cloning')",
            params![
                c.project_id,
                c.workspace_id,
                c.name,
                c.node_id,
                c.host,
                c.full_name,
                c.created_by,
                harness_core::now_ms()
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The forge's own spelling of a running clone's repository name.
    pub fn set_clone_name(&self, id: &str, full_name: &str) -> Result<()> {
        self.lock().execute(
            "UPDATE project_clones SET full_name = ?2 WHERE project_id = ?1 AND state = 'cloning'",
            params![id, full_name],
        )?;
        Ok(())
    }

    /// Forget a clone that never reached its node (a refusal the request
    /// answered itself).
    pub fn drop_clone(&self, id: &str) -> Result<()> {
        self.lock().execute(
            "DELETE FROM project_clones WHERE project_id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// A running clone failed: kept with `reason` for its starter, whose
    /// failed clones beyond the newest [`FAILED_KEPT`] go. `false` when it
    /// was not running (gone with its workspace or its person).
    pub fn fail_clone(&self, id: &str, reason: &str) -> Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let who: Option<String> = tx
            .query_row(
                "UPDATE project_clones SET state = 'failed', reason = ?2, ended_ms = ?3 \
                 WHERE project_id = ?1 AND state = 'cloning' RETURNING created_by",
                params![id, reason, harness_core::now_ms()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(who) = &who {
            tx.execute(
                "DELETE FROM project_clones WHERE state = 'failed' AND created_by = ?1 AND \
                 project_id NOT IN (SELECT project_id FROM project_clones WHERE state = 'failed' \
                 AND created_by = ?1 ORDER BY ended_ms DESC, rowid DESC LIMIT ?2)",
                params![who, FAILED_KEPT as i64],
            )?;
        }
        tx.commit()?;
        Ok(who.is_some())
    }

    /// A running clone ended well: its row goes and the project `p` (the
    /// same id) is written, in one transaction. Refused when the clone is
    /// not running any more (its workspace or person was deleted) or by
    /// the project limit.
    pub fn finish_clone(&self, p: &NewProject, max: usize) -> Result<ProjectRow> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let gone = tx.execute(
            "DELETE FROM project_clones WHERE project_id = ?1 AND state = 'cloning'",
            params![p.project_id],
        )? == 0;
        if gone {
            return Err(StoreError::Refused(
                "the project's workspace or its starter was removed while it was cloning".into(),
            ));
        }
        let row = insert_project(&tx, p, max)?;
        tx.commit()?;
        Ok(row)
    }

    /// The clones of `workspace` (all of them for `None`), oldest first.
    pub fn clones(&self, workspace: Option<&str>) -> Result<Vec<CloneRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {COLS} FROM project_clones WHERE ?1 IS NULL OR workspace_id = ?1 \
             ORDER BY started_ms, rowid"
        ))?;
        let rows = stmt.query_map(params![workspace], clone_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn clone_of(&self, id: &str) -> Result<Option<CloneRow>> {
        Ok(self
            .lock()
            .query_row(
                &format!("SELECT {COLS} FROM project_clones WHERE project_id = ?1"),
                params![id],
                clone_row,
            )
            .optional()?)
    }

    /// Dismiss `user`'s failed clone `id`; `false` when there is none
    /// (running, someone else's, or no such row).
    pub fn dismiss_clone(&self, id: &str, user: &str) -> Result<bool> {
        Ok(self.lock().execute(
            "DELETE FROM project_clones WHERE project_id = ?1 AND created_by = ?2 AND state = 'failed'",
            params![id, user],
        )? > 0)
    }

    /// At the master's start: every running clone failed with `reason` (the
    /// master that waited for it is gone). How many.
    pub fn fail_running_clones(&self, reason: &str) -> Result<usize> {
        Ok(self.lock().execute(
            "UPDATE project_clones SET state = 'failed', reason = ?1, ended_ms = ?2 \
             WHERE state = 'cloning'",
            params![reason, harness_core::now_ms()],
        )?)
    }
}
