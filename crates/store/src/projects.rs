//! Projects (master only; docs/projects.md): a project groups chats inside a
//! workspace and has a task board. Its memory lives in the memory store, its
//! rules in the master; this is storage only. node.db and chat.db get the
//! empty tables with the shared schema and never write them.

use harness_core::SessionId;
use rusqlite::{OptionalExtension, Transaction, params};

use crate::{EFFECTIVE_WS, Result, Store, StoreError, WS_JOINS};

pub(crate) const SCHEMA: &str = r#"
-- A project is in one workspace (Personal included) and goes with it: its
-- board and chat links follow by cascade, its facts (memory store) are the
-- master's to forget.
CREATE TABLE IF NOT EXISTS projects (
    project_id   TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    icon         TEXT NOT NULL,
    accent       TEXT NOT NULL,
    instructions TEXT NOT NULL DEFAULT '',
    -- JSON object; ProjectSettings in the master.
    settings     TEXT NOT NULL DEFAULT '{}',
    created_by   TEXT,
    created_ms   INTEGER NOT NULL,
    updated_ms   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS projects_ws ON projects(workspace_id);
-- Which project a chat is in (at most one). No foreign key on session_id: a
-- link may be written right after the create, and delete_session drops it.
CREATE TABLE IF NOT EXISTS session_projects (
    session_id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(project_id) ON DELETE CASCADE,
    added_ms   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS session_projects_project ON session_projects(project_id);
CREATE TABLE IF NOT EXISTS board_columns (
    column_id  TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(project_id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    position   INTEGER NOT NULL,
    -- Cards here are finished: left out of the model's context.
    done       INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS board_columns_project ON board_columns(project_id);
CREATE TABLE IF NOT EXISTS board_cards (
    card_id    TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(project_id) ON DELETE CASCADE,
    column_id  TEXT NOT NULL REFERENCES board_columns(column_id) ON DELETE CASCADE,
    title      TEXT NOT NULL,
    notes      TEXT NOT NULL DEFAULT '',
    position   INTEGER NOT NULL,
    session_id TEXT,
    created_by TEXT,
    created_ms INTEGER NOT NULL,
    updated_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS board_cards_column ON board_cards(column_id);
-- Board templates people saved (built-in ones live in the master's code).
CREATE TABLE IF NOT EXISTS board_templates (
    template_id  TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    -- JSON array of {"name", "done"}.
    columns      TEXT NOT NULL,
    created_by   TEXT,
    created_ms   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS board_templates_ws ON board_templates(workspace_id);
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRow {
    pub project_id: String,
    pub workspace_id: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub accent: String,
    pub instructions: String,
    /// JSON object, read by the master.
    pub settings: String,
    pub created_by: Option<String>,
    pub created_ms: i64,
    pub updated_ms: i64,
}

/// A board column to create.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NewColumn {
    pub name: String,
    #[serde(default)]
    pub done: bool,
}

/// A project to create, with its board's first columns.
#[derive(Debug, Clone)]
pub struct NewProject {
    pub project_id: String,
    pub workspace_id: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub accent: String,
    pub instructions: String,
    pub settings: String,
    pub created_by: Option<String>,
    pub columns: Vec<NewColumn>,
}

/// Fields to change; `None` leaves a field as is.
#[derive(Debug, Clone, Default)]
pub struct ProjectPatch {
    pub name: Option<String>,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub accent: Option<String>,
    pub instructions: Option<String>,
    pub settings: Option<String>,
}

/// One chat's link: the chat, its project and that project's workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectLink {
    pub session_id: SessionId,
    pub project_id: String,
    pub workspace_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnRow {
    pub column_id: String,
    pub name: String,
    pub position: i64,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardRow {
    pub card_id: String,
    pub column_id: String,
    pub title: String,
    pub notes: String,
    pub position: i64,
    pub session_id: Option<String>,
    pub created_by: Option<String>,
    pub created_ms: i64,
    pub updated_ms: i64,
}

#[derive(Debug, Clone)]
pub struct NewCard {
    pub card_id: String,
    pub column_id: String,
    pub title: String,
    pub notes: String,
    pub session_id: Option<String>,
    pub created_by: Option<String>,
}

/// Changes to a card. `column_id`/`position` move it (position = index in
/// the target column, clamped); `session_id: Some(None)` unlinks its chat.
#[derive(Debug, Clone, Default)]
pub struct CardPatch {
    pub title: Option<String>,
    pub notes: Option<String>,
    pub column_id: Option<String>,
    pub position: Option<usize>,
    pub session_id: Option<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateRow {
    pub template_id: String,
    pub workspace_id: String,
    pub name: String,
    /// JSON array of `NewColumn`.
    pub columns: String,
    pub created_by: Option<String>,
    pub created_ms: i64,
}

const PROJECT_COLS: &str = "project_id, workspace_id, name, description, icon, accent, \
     instructions, settings, created_by, created_ms, updated_ms";

fn project_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectRow> {
    Ok(ProjectRow {
        project_id: r.get(0)?,
        workspace_id: r.get(1)?,
        name: r.get(2)?,
        description: r.get(3)?,
        icon: r.get(4)?,
        accent: r.get(5)?,
        instructions: r.get(6)?,
        settings: r.get(7)?,
        created_by: r.get(8)?,
        created_ms: r.get(9)?,
        updated_ms: r.get(10)?,
    })
}

const CARD_COLS: &str =
    "card_id, column_id, title, notes, position, session_id, created_by, created_ms, updated_ms";

fn card_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<CardRow> {
    Ok(CardRow {
        card_id: r.get(0)?,
        column_id: r.get(1)?,
        title: r.get(2)?,
        notes: r.get(3)?,
        position: r.get(4)?,
        session_id: r.get(5)?,
        created_by: r.get(6)?,
        created_ms: r.get(7)?,
        updated_ms: r.get(8)?,
    })
}

fn count(tx: &Transaction<'_>, sql: &str, key: &str) -> Result<usize> {
    let n: i64 = tx.query_row(sql, params![key], |r| r.get(0))?;
    Ok(n as usize)
}

fn insert_columns(tx: &Transaction<'_>, project: &str, cols: &[NewColumn]) -> Result<()> {
    for (i, c) in cols.iter().enumerate() {
        tx.execute(
            "INSERT INTO board_columns (column_id, project_id, name, position, done) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![harness_core::new_id(), project, c.name, i as i64, c.done],
        )?;
    }
    Ok(())
}

/// Ids in `sql`'s order (one `?1` parameter).
fn ids(tx: &Transaction<'_>, sql: &str, key: &str) -> Result<Vec<String>> {
    let mut stmt = tx.prepare(sql)?;
    let rows = stmt.query_map(params![key], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Write positions 0.. in the order given.
fn renumber(tx: &Transaction<'_>, table: &str, id_col: &str, order: &[String]) -> Result<()> {
    let sql = format!("UPDATE {table} SET position = ?2 WHERE {id_col} = ?1");
    for (i, id) in order.iter().enumerate() {
        tx.execute(&sql, params![id, i as i64])?;
    }
    Ok(())
}

fn card_ids_in(tx: &Transaction<'_>, column: &str) -> Result<Vec<String>> {
    ids(
        tx,
        "SELECT card_id FROM board_cards WHERE column_id = ?1 ORDER BY position, created_ms",
        column,
    )
}

fn column_in(tx: &Transaction<'_>, project: &str, column: &str) -> Result<bool> {
    Ok(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM board_columns WHERE column_id = ?1 AND project_id = ?2)",
        params![column, project],
        |r| r.get(0),
    )?)
}

impl Store {
    // ---- projects ----

    /// Create a project and its board's columns. Refused when the workspace
    /// already has `max` projects.
    pub fn create_project(&self, p: &NewProject, max: usize) -> Result<ProjectRow> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let n = count(
            &tx,
            "SELECT COUNT(*) FROM projects WHERE workspace_id = ?1",
            &p.workspace_id,
        )?;
        if n >= max {
            return Err(StoreError::Refused(format!(
                "a workspace has at most {max} projects"
            )));
        }
        let now = harness_core::now_ms();
        tx.execute(
            &format!("INSERT INTO projects ({PROJECT_COLS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)"),
            params![
                p.project_id,
                p.workspace_id,
                p.name,
                p.description,
                p.icon,
                p.accent,
                p.instructions,
                p.settings,
                p.created_by,
                now
            ],
        )?;
        insert_columns(&tx, &p.project_id, &p.columns)?;
        let row = tx.query_row(
            &format!("SELECT {PROJECT_COLS} FROM projects WHERE project_id = ?1"),
            params![p.project_id],
            project_row,
        )?;
        tx.commit()?;
        Ok(row)
    }

    pub fn project(&self, id: &str) -> Result<Option<ProjectRow>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                &format!("SELECT {PROJECT_COLS} FROM projects WHERE project_id = ?1"),
                params![id],
                project_row,
            )
            .optional()?)
    }

    /// The projects of `workspace` (all of them for `None`), by name.
    pub fn projects(&self, workspace: Option<&str>) -> Result<Vec<ProjectRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {PROJECT_COLS} FROM projects WHERE ?1 IS NULL OR workspace_id = ?1 \
             ORDER BY name COLLATE NOCASE, created_ms"
        ))?;
        let rows = stmt.query_map(params![workspace], project_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Apply `patch`; `None` when there is no such project.
    pub fn update_project(&self, id: &str, patch: &ProjectPatch) -> Result<Option<ProjectRow>> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let fields = [
            ("name", &patch.name),
            ("description", &patch.description),
            ("icon", &patch.icon),
            ("accent", &patch.accent),
            ("instructions", &patch.instructions),
            ("settings", &patch.settings),
        ];
        for (col, v) in fields {
            if let Some(v) = v {
                tx.execute(
                    &format!("UPDATE projects SET {col} = ?2 WHERE project_id = ?1"),
                    params![id, v],
                )?;
            }
        }
        tx.execute(
            "UPDATE projects SET updated_ms = ?2 WHERE project_id = ?1",
            params![id, harness_core::now_ms()],
        )?;
        let row = tx
            .query_row(
                &format!("SELECT {PROJECT_COLS} FROM projects WHERE project_id = ?1"),
                params![id],
                project_row,
            )
            .optional()?;
        tx.commit()?;
        Ok(row)
    }

    /// Delete a project, its board and its chat links (the chats stay).
    pub fn delete_project(&self, id: &str) -> Result<bool> {
        let conn = self.lock();
        Ok(conn.execute("DELETE FROM projects WHERE project_id = ?1", params![id])? > 0)
    }

    // ---- chats in projects ----

    /// Put a chat into `project`, or take it out (`None`).
    pub fn set_session_project(&self, sid: SessionId, project: Option<&str>) -> Result<()> {
        let conn = self.lock();
        match project {
            Some(p) => conn.execute(
                "INSERT OR REPLACE INTO session_projects (session_id, project_id, added_ms) VALUES (?1, ?2, ?3)",
                params![sid.to_string(), p, harness_core::now_ms()],
            )?,
            None => conn.execute(
                "DELETE FROM session_projects WHERE session_id = ?1",
                params![sid.to_string()],
            )?,
        };
        Ok(())
    }

    /// Link a chat just stamped by `claim_session` to `project`, only when the
    /// chat's effective workspace is the project's (a removal racing the
    /// create leaves it in Personal, outside a team's project). Returns
    /// whether it was linked.
    pub fn link_new_session(&self, sid: SessionId, project: &str) -> Result<bool> {
        let conn = self.lock();
        let sql = format!(
            "INSERT OR REPLACE INTO session_projects (session_id, project_id, added_ms)
             SELECT s.session_id, p.project_id, ?3 FROM sessions s {WS_JOINS}
             JOIN projects p ON p.project_id = ?2
             WHERE s.session_id = ?1 AND p.workspace_id = {EFFECTIVE_WS}"
        );
        Ok(conn.execute(
            &sql,
            params![sid.to_string(), project, harness_core::now_ms()],
        )? > 0)
    }

    /// The chat's link, if it has one (whether it still applies is the
    /// master's call: the project must be in the chat's workspace).
    pub fn session_project(&self, sid: SessionId) -> Result<Option<ProjectLink>> {
        Ok(self.project_links(Some(sid))?.into_iter().next())
    }

    /// Every chat's link (or one chat's).
    pub fn project_links(&self, sid: Option<SessionId>) -> Result<Vec<ProjectLink>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT sp.session_id, sp.project_id, p.workspace_id FROM session_projects sp \
             JOIN projects p ON p.project_id = sp.project_id \
             WHERE ?1 IS NULL OR sp.session_id = ?1",
        )?;
        let rows = stmt.query_map(params![sid.map(|s| s.to_string())], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (sid, project_id, workspace_id) = row?;
            let session_id = sid
                .parse()
                .map_err(|_| StoreError::Corrupt(format!("session id {sid}")))?;
            out.push(ProjectLink {
                session_id,
                project_id,
                workspace_id,
            });
        }
        Ok(out)
    }

    // ---- board ----

    /// The project's columns and cards, each in board order.
    pub fn board(&self, project: &str) -> Result<(Vec<ColumnRow>, Vec<CardRow>)> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT column_id, name, position, done FROM board_columns WHERE project_id = ?1 ORDER BY position",
        )?;
        let cols = stmt
            .query_map(params![project], |r| {
                Ok(ColumnRow {
                    column_id: r.get(0)?,
                    name: r.get(1)?,
                    position: r.get(2)?,
                    done: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {CARD_COLS} FROM board_cards WHERE project_id = ?1 ORDER BY position, created_ms"
        ))?;
        let cards = stmt
            .query_map(params![project], card_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok((cols, cards))
    }

    /// Add a column at the end. Refused past `max` columns.
    pub fn add_column(&self, project: &str, col: &NewColumn, max: usize) -> Result<String> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let n = count(
            &tx,
            "SELECT COUNT(*) FROM board_columns WHERE project_id = ?1",
            project,
        )?;
        if n >= max {
            return Err(StoreError::Refused(format!(
                "a board has at most {max} columns"
            )));
        }
        let id = harness_core::new_id();
        tx.execute(
            "INSERT INTO board_columns (column_id, project_id, name, position, done) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, project, col.name, n as i64, col.done],
        )?;
        tx.commit()?;
        Ok(id)
    }

    /// Rename, mark done, or move a column (`position` = its new index).
    /// `false` when the project has no such column.
    pub fn update_column(
        &self,
        project: &str,
        column: &str,
        name: Option<&str>,
        done: Option<bool>,
        position: Option<usize>,
    ) -> Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        if !column_in(&tx, project, column)? {
            return Ok(false);
        }
        if let Some(name) = name {
            tx.execute(
                "UPDATE board_columns SET name = ?2 WHERE column_id = ?1",
                params![column, name],
            )?;
        }
        if let Some(done) = done {
            tx.execute(
                "UPDATE board_columns SET done = ?2 WHERE column_id = ?1",
                params![column, done],
            )?;
        }
        if let Some(to) = position {
            let mut order = ids(
                &tx,
                "SELECT column_id FROM board_columns WHERE project_id = ?1 ORDER BY position",
                project,
            )?;
            order.retain(|c| c != column);
            order.insert(to.min(order.len()), column.to_string());
            renumber(&tx, "board_columns", "column_id", &order)?;
        }
        tx.commit()?;
        Ok(true)
    }

    /// Delete an empty column. Refused while it holds cards or is the last.
    pub fn delete_column(&self, project: &str, column: &str) -> Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        if !column_in(&tx, project, column)? {
            return Ok(false);
        }
        if count(
            &tx,
            "SELECT COUNT(*) FROM board_cards WHERE column_id = ?1",
            column,
        )? > 0
        {
            return Err(StoreError::Refused(
                "move or delete this column's cards first".into(),
            ));
        }
        let left = count(
            &tx,
            "SELECT COUNT(*) FROM board_columns WHERE project_id = ?1",
            project,
        )?;
        if left <= 1 {
            return Err(StoreError::Refused(
                "a board keeps at least one column".into(),
            ));
        }
        tx.execute(
            "DELETE FROM board_columns WHERE column_id = ?1",
            params![column],
        )?;
        let order = ids(
            &tx,
            "SELECT column_id FROM board_columns WHERE project_id = ?1 ORDER BY position",
            project,
        )?;
        renumber(&tx, "board_columns", "column_id", &order)?;
        tx.commit()?;
        Ok(true)
    }

    /// Replace the board's columns. Refused while the board has cards.
    pub fn reset_columns(&self, project: &str, cols: &[NewColumn]) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        if count(
            &tx,
            "SELECT COUNT(*) FROM board_cards WHERE project_id = ?1",
            project,
        )? > 0
        {
            return Err(StoreError::Refused(
                "a template can only replace the columns of a board without cards".into(),
            ));
        }
        tx.execute(
            "DELETE FROM board_columns WHERE project_id = ?1",
            params![project],
        )?;
        insert_columns(&tx, project, cols)?;
        tx.commit()?;
        Ok(())
    }

    /// Add a card at the bottom of its column. `false` when the project has
    /// no such column; refused past `max` cards.
    pub fn add_card(&self, project: &str, c: &NewCard, max: usize) -> Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        if !column_in(&tx, project, &c.column_id)? {
            return Ok(false);
        }
        if count(
            &tx,
            "SELECT COUNT(*) FROM board_cards WHERE project_id = ?1",
            project,
        )? >= max
        {
            return Err(StoreError::Refused(format!(
                "a board has at most {max} cards"
            )));
        }
        let pos = count(
            &tx,
            "SELECT COUNT(*) FROM board_cards WHERE column_id = ?1",
            &c.column_id,
        )?;
        let now = harness_core::now_ms();
        tx.execute(
            &format!("INSERT INTO board_cards ({CARD_COLS}, project_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9)"),
            params![
                c.card_id,
                c.column_id,
                c.title,
                c.notes,
                pos as i64,
                c.session_id,
                c.created_by,
                now,
                project
            ],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn card(&self, project: &str, card: &str) -> Result<Option<CardRow>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                &format!(
                    "SELECT {CARD_COLS} FROM board_cards WHERE card_id = ?1 AND project_id = ?2"
                ),
                params![card, project],
                card_row,
            )
            .optional()?)
    }

    /// Edit or move a card. `false` when the project has no such card or
    /// the target column is not on its board.
    pub fn update_card(&self, project: &str, card: &str, patch: &CardPatch) -> Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let from: Option<String> = tx
            .query_row(
                "SELECT column_id FROM board_cards WHERE card_id = ?1 AND project_id = ?2",
                params![card, project],
                |r| r.get(0),
            )
            .optional()?;
        let Some(from) = from else {
            return Ok(false);
        };
        let to = patch.column_id.clone().unwrap_or_else(|| from.clone());
        if to != from && !column_in(&tx, project, &to)? {
            return Ok(false);
        }
        if let Some(t) = &patch.title {
            tx.execute(
                "UPDATE board_cards SET title = ?2 WHERE card_id = ?1",
                params![card, t],
            )?;
        }
        if let Some(n) = &patch.notes {
            tx.execute(
                "UPDATE board_cards SET notes = ?2 WHERE card_id = ?1",
                params![card, n],
            )?;
        }
        if let Some(s) = &patch.session_id {
            tx.execute(
                "UPDATE board_cards SET session_id = ?2 WHERE card_id = ?1",
                params![card, s],
            )?;
        }
        if to != from || patch.position.is_some() {
            let mut order = card_ids_in(&tx, &to)?;
            order.retain(|c| c != card);
            let at = patch.position.unwrap_or(order.len()).min(order.len());
            order.insert(at, card.to_string());
            tx.execute(
                "UPDATE board_cards SET column_id = ?2 WHERE card_id = ?1",
                params![card, to],
            )?;
            renumber(&tx, "board_cards", "card_id", &order)?;
            if to != from {
                let rest = card_ids_in(&tx, &from)?;
                renumber(&tx, "board_cards", "card_id", &rest)?;
            }
        }
        tx.execute(
            "UPDATE board_cards SET updated_ms = ?2 WHERE card_id = ?1",
            params![card, harness_core::now_ms()],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn delete_card(&self, project: &str, card: &str) -> Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let column: Option<String> = tx
            .query_row(
                "SELECT column_id FROM board_cards WHERE card_id = ?1 AND project_id = ?2",
                params![card, project],
                |r| r.get(0),
            )
            .optional()?;
        let Some(column) = column else {
            return Ok(false);
        };
        tx.execute("DELETE FROM board_cards WHERE card_id = ?1", params![card])?;
        let rest = card_ids_in(&tx, &column)?;
        renumber(&tx, "board_cards", "card_id", &rest)?;
        tx.commit()?;
        Ok(true)
    }

    // ---- board templates ----

    /// Saved templates of `workspace`, by name.
    pub fn board_templates(&self, workspace: &str) -> Result<Vec<TemplateRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT template_id, workspace_id, name, columns, created_by, created_ms FROM board_templates \
             WHERE workspace_id = ?1 ORDER BY name COLLATE NOCASE, created_ms",
        )?;
        let rows = stmt.query_map(params![workspace], |r| {
            Ok(TemplateRow {
                template_id: r.get(0)?,
                workspace_id: r.get(1)?,
                name: r.get(2)?,
                columns: r.get(3)?,
                created_by: r.get(4)?,
                created_ms: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn board_template(&self, id: &str) -> Result<Option<TemplateRow>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                "SELECT template_id, workspace_id, name, columns, created_by, created_ms FROM board_templates WHERE template_id = ?1",
                params![id],
                |r| {
                    Ok(TemplateRow {
                        template_id: r.get(0)?,
                        workspace_id: r.get(1)?,
                        name: r.get(2)?,
                        columns: r.get(3)?,
                        created_by: r.get(4)?,
                        created_ms: r.get(5)?,
                    })
                },
            )
            .optional()?)
    }

    /// Save a template. Refused past `max` templates in its workspace.
    pub fn create_board_template(&self, t: &TemplateRow, max: usize) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        if count(
            &tx,
            "SELECT COUNT(*) FROM board_templates WHERE workspace_id = ?1",
            &t.workspace_id,
        )? >= max
        {
            return Err(StoreError::Refused(format!(
                "a workspace has at most {max} board templates"
            )));
        }
        tx.execute(
            "INSERT INTO board_templates (template_id, workspace_id, name, columns, created_by, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![t.template_id, t.workspace_id, t.name, t.columns, t.created_by, t.created_ms],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_board_template(&self, id: &str) -> Result<bool> {
        let conn = self.lock();
        Ok(conn.execute(
            "DELETE FROM board_templates WHERE template_id = ?1",
            params![id],
        )? > 0)
    }
}
