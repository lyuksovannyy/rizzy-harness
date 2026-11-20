//! A workspace's own agents and its edits of the server's (ADOPTION_PLAN
//! X.33, PLAN.md T79): one JSON document per agent id, `custom` for an
//! agent made in the workspace, `overlay` for its summary and additional
//! instructions on a server agent. The master parses the documents; this
//! keeps them, bounds them and lets them go with their workspace (foreign
//! key cascade). The server's own edits stay in `kv`.

use rusqlite::params;

use crate::{Result, Store, StoreError};

/// Run by `Store::init`, after the workspaces.
pub(crate) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS workspace_agents (
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    agent_id     TEXT NOT NULL,
    kind         TEXT NOT NULL CHECK (kind IN ('custom', 'overlay')),
    -- At most 64 KiB (MAX_DOC_BYTES); the master refuses more first.
    doc          TEXT NOT NULL CHECK (length(CAST(doc AS BLOB)) <= 65536),
    updated_ms   INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, agent_id)
);
CREATE INDEX IF NOT EXISTS workspace_agents_agent ON workspace_agents(agent_id);
"#;

/// The largest document, in bytes (the table's CHECK says the same).
pub const MAX_DOC_BYTES: usize = 64 * 1024;

/// One stored document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceAgentRow {
    pub agent_id: String,
    /// `custom` or `overlay`.
    pub kind: String,
    pub doc: String,
    pub updated_ms: i64,
}

impl Store {
    /// Every document of workspace `ws`, by agent id.
    pub fn workspace_agents(&self, ws: &str) -> Result<Vec<WorkspaceAgentRow>> {
        let conn = self.lock();
        let mut st = conn.prepare(
            "SELECT agent_id, kind, doc, updated_ms FROM workspace_agents
             WHERE workspace_id = ?1 ORDER BY agent_id",
        )?;
        let rows = st.query_map(params![ws], |r| {
            Ok(WorkspaceAgentRow {
                agent_id: r.get(0)?,
                kind: r.get(1)?,
                doc: r.get(2)?,
                updated_ms: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Store `doc` as agent `agent_id`'s `kind` document in `ws`, replacing
    /// what it had. A `custom` one that would make more than `max_custom`
    /// in the workspace is `Refused` (checked in the same transaction);
    /// `false` when the workspace does not exist.
    pub fn workspace_agent_put(
        &self,
        ws: &str,
        agent_id: &str,
        kind: &str,
        doc: &str,
        max_custom: usize,
    ) -> Result<bool> {
        if doc.len() > MAX_DOC_BYTES {
            return Err(StoreError::Refused(format!(
                "an agent is stored in at most {MAX_DOC_BYTES} bytes"
            )));
        }
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        if Self::workspace_in(&tx, ws)?.is_none() {
            return Ok(false);
        }
        if kind == "custom" {
            let others: i64 = tx.query_row(
                "SELECT count(*) FROM workspace_agents
                 WHERE workspace_id = ?1 AND kind = 'custom' AND agent_id <> ?2",
                params![ws, agent_id],
                |r| r.get(0),
            )?;
            if others as usize >= max_custom {
                return Err(StoreError::Refused(format!(
                    "a workspace has at most {max_custom} agents of its own: delete one first"
                )));
            }
        }
        tx.execute(
            "INSERT INTO workspace_agents (workspace_id, agent_id, kind, doc, updated_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (workspace_id, agent_id) DO UPDATE SET
                 kind = excluded.kind, doc = excluded.doc, updated_ms = excluded.updated_ms",
            params![ws, agent_id, kind, doc, harness_core::now_ms()],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Remove agent `agent_id`'s `kind` document from `ws`; whether there
    /// was one.
    pub fn workspace_agent_delete(&self, ws: &str, agent_id: &str, kind: &str) -> Result<bool> {
        Ok(self.lock().execute(
            "DELETE FROM workspace_agents WHERE workspace_id = ?1 AND agent_id = ?2 AND kind = ?3",
            params![ws, agent_id, kind],
        )? > 0)
    }

    /// Every workspace's `custom` document that names `agent_id` in quotes
    /// (a cheap pre-filter: the master parses what comes back), as
    /// (workspace, agent, doc). `agent_id` is a handle, so it needs no
    /// escaping.
    pub fn workspace_custom_docs_naming(
        &self,
        agent_id: &str,
    ) -> Result<Vec<(String, String, String)>> {
        let conn = self.lock();
        let mut st = conn.prepare(
            "SELECT workspace_id, agent_id, doc FROM workspace_agents
             WHERE kind = 'custom' AND instr(doc, ?1) > 0",
        )?;
        let rows = st.query_map(params![format!("\"{agent_id}\"")], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Remove every workspace's overlay on agent `agent_id`: the server
    /// agent it was written for is gone, so it must not land on a later,
    /// unrelated agent with that id (PLAN.md T79). How many went.
    pub fn delete_workspace_overlays(&self, agent_id: &str) -> Result<usize> {
        Ok(self.lock().execute(
            "DELETE FROM workspace_agents WHERE agent_id = ?1 AND kind = 'overlay'",
            params![agent_id],
        )?)
    }
}
