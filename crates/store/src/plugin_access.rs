//! Which agents of a workspace may use which skill or MCP server
//! (ADOPTION_PLAN X.34, PLAN.md T80). An item without a row is Shared:
//! every agent of the workspace may use it. A row makes it Assigned: only
//! the agents it lists (ids of the workspace's set, as a JSON array), none
//! when the list is empty. The master checks names and ids; this keeps
//! them, bounds them and lets them go with their workspace (foreign key
//! cascade).

use rusqlite::params;

use crate::{Result, Store, StoreError};

/// Run by `Store::init`, after the workspaces.
pub(crate) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS plugin_access (
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    kind         TEXT NOT NULL CHECK (kind IN ('skill', 'mcp')),
    name         TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 64),
    -- The agent ids it is assigned to: a JSON array, at most 16 KiB.
    agents       TEXT NOT NULL CHECK (length(CAST(agents AS BLOB)) <= 16384),
    updated_ms   INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, kind, name)
);
"#;

/// The largest list of agents, as stored (the table's CHECK says the same).
pub const MAX_AGENTS_BYTES: usize = 16 * 1024;

/// One Assigned item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginAccessRow {
    /// `skill` or `mcp`.
    pub kind: String,
    pub name: String,
    pub agents: Vec<String>,
    pub updated_ms: i64,
}

impl Store {
    /// Every Assigned item of workspace `ws`, by kind and name. A list that
    /// does not read is an empty one: nobody but the agents named may use
    /// the item, so a damaged row fails closed.
    pub fn plugin_access(&self, ws: &str) -> Result<Vec<PluginAccessRow>> {
        let conn = self.lock();
        let mut st = conn.prepare(
            "SELECT kind, name, agents, updated_ms FROM plugin_access
             WHERE workspace_id = ?1 ORDER BY kind, name",
        )?;
        let rows = st.query_map(params![ws], |r| {
            let agents: String = r.get(2)?;
            Ok(PluginAccessRow {
                kind: r.get(0)?,
                name: r.get(1)?,
                agents: serde_json::from_str(&agents).unwrap_or_default(),
                updated_ms: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Make item (`kind`, `name`) of `ws` Assigned to `agents`, replacing
    /// what it had. One that would make more than `max_rows` Assigned
    /// items in the workspace is `Refused` (checked in the same
    /// transaction); `false` when the workspace does not exist.
    pub fn plugin_access_put(
        &self,
        ws: &str,
        kind: &str,
        name: &str,
        agents: &[String],
        max_rows: usize,
    ) -> Result<bool> {
        let json = serde_json::to_string(agents).expect("strings serialize");
        if json.len() > MAX_AGENTS_BYTES {
            return Err(StoreError::Refused(format!(
                "an item's agents are stored in at most {MAX_AGENTS_BYTES} bytes"
            )));
        }
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        if Self::workspace_in(&tx, ws)?.is_none() {
            return Ok(false);
        }
        let others: i64 = tx.query_row(
            "SELECT count(*) FROM plugin_access
             WHERE workspace_id = ?1 AND NOT (kind = ?2 AND name = ?3)",
            params![ws, kind, name],
            |r| r.get(0),
        )?;
        if others as usize >= max_rows {
            return Err(StoreError::Refused(format!(
                "a workspace has at most {max_rows} Assigned skills and connectors: make one \
                 Shared first"
            )));
        }
        tx.execute(
            "INSERT INTO plugin_access (workspace_id, kind, name, agents, updated_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (workspace_id, kind, name) DO UPDATE SET
                 agents = excluded.agents, updated_ms = excluded.updated_ms",
            params![ws, kind, name, json, harness_core::now_ms()],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Make item (`kind`, `name`) of `ws` Shared again; whether it was
    /// Assigned.
    pub fn plugin_access_delete(&self, ws: &str, kind: &str, name: &str) -> Result<bool> {
        Ok(self.lock().execute(
            "DELETE FROM plugin_access WHERE workspace_id = ?1 AND kind = ?2 AND name = ?3",
            params![ws, kind, name],
        )? > 0)
    }

    /// Take agent `agent_id` out of every Assigned list of `ws` (`None`:
    /// of every workspace): the agent is gone, so a later agent made with
    /// its id must not inherit what it was given (PLAN.md T80). The item
    /// stays Assigned, to the others (to nobody, if it was the last). How
    /// many lists changed.
    pub fn plugin_access_forget_agent(&self, ws: Option<&str>, agent_id: &str) -> Result<usize> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let rows: Vec<(String, String, String, String)> = {
            let mut st = tx.prepare(
                "SELECT workspace_id, kind, name, agents FROM plugin_access
                 WHERE ?1 IS NULL OR workspace_id = ?1",
            )?;
            st.query_map(params![ws], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<rusqlite::Result<_>>()?
        };
        let mut changed = 0;
        for (w, kind, name, agents) in rows {
            let mut list: Vec<String> = serde_json::from_str(&agents).unwrap_or_default();
            let before = list.len();
            list.retain(|a| a != agent_id);
            if list.len() == before {
                continue;
            }
            tx.execute(
                "UPDATE plugin_access SET agents = ?4, updated_ms = ?5
                 WHERE workspace_id = ?1 AND kind = ?2 AND name = ?3",
                params![
                    w,
                    kind,
                    name,
                    serde_json::to_string(&list).expect("strings serialize"),
                    harness_core::now_ms()
                ],
            )?;
            changed += 1;
        }
        tx.commit()?;
        Ok(changed)
    }
}
