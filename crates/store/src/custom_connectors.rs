//! A workspace's custom connectors and each person's own account for one
//! (ADOPTION_PLAN X.35 step 1, PLAN.md T84).
//!
//! `workspace_connectors`: a remote MCP server by its address, added by the
//! workspace's admins; it goes with its workspace. `connector_accounts`: a
//! person's token for one connector, sealed by the master (bound to the
//! table, the person, the workspace, the connector and its URL as stored);
//! this crate never sees it in the clear and no row type here has a field
//! for it ([`Store::connector_account_sealed`] returns the bytes). An
//! account goes with its connector, its workspace membership (so with the
//! person and the workspace too), by foreign key cascade; editing a
//! connector's URL or sign-in removes every account of it in the same
//! transaction ([`Store::connector_update`]). Every read names the
//! workspace, and an account's reads the person.

use rusqlite::{OptionalExtension, params};

use crate::{Result, Store};

/// Run by `Store::init`, after the workspaces and their members.
pub(crate) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS workspace_connectors (
    connector_id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    name         TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 32),
    url          TEXT NOT NULL CHECK (length(url) BETWEEN 9 AND 512),
    -- How a person signs in: none, or a token they paste.
    auth         TEXT NOT NULL CHECK (auth IN ('none', 'token')),
    -- The header the token goes in; NULL: Authorization (Bearer).
    header       TEXT CHECK (header IS NULL OR length(header) BETWEEN 1 AND 64),
    created_ms   INTEGER NOT NULL,
    updated_ms   INTEGER NOT NULL,
    UNIQUE (workspace_id, name),
    UNIQUE (workspace_id, connector_id)
);
CREATE TABLE IF NOT EXISTS connector_accounts (
    user_id      TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    connector_id TEXT NOT NULL,
    sealed_token BLOB NOT NULL CHECK (length(sealed_token) <= 4096),
    created_ms   INTEGER NOT NULL,
    updated_ms   INTEGER NOT NULL,
    PRIMARY KEY (user_id, connector_id),
    FOREIGN KEY (workspace_id, user_id)
        REFERENCES workspace_members(workspace_id, user_id) ON DELETE CASCADE,
    FOREIGN KEY (workspace_id, connector_id)
        REFERENCES workspace_connectors(workspace_id, connector_id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS connector_accounts_connector ON connector_accounts(connector_id);
"#;

/// One connector as listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorRow {
    pub connector_id: String,
    pub workspace_id: String,
    pub name: String,
    pub url: String,
    /// `none` or `token`.
    pub auth: String,
    pub header: Option<String>,
    pub created_ms: i64,
    pub updated_ms: i64,
}

/// One person's account for a connector: never the token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRow {
    pub connector_id: String,
    pub created_ms: i64,
    pub updated_ms: i64,
}

/// What [`Store::connector_add`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectorAdd {
    Added,
    NameTaken,
    OverCap,
    NoWorkspace,
}

/// What [`Store::connector_account_put`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountPut {
    Added,
    Replaced,
    OverCap,
    /// The connector is gone, is not the person's workspace's, takes no
    /// token, or its URL is no longer the one the token was checked with:
    /// nothing is stored.
    Changed,
    /// The person is not a member of the workspace.
    NotMember,
}

const COLS: &str = "connector_id, workspace_id, name, url, auth, header, created_ms, updated_ms";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ConnectorRow> {
    Ok(ConnectorRow {
        connector_id: r.get(0)?,
        workspace_id: r.get(1)?,
        name: r.get(2)?,
        url: r.get(3)?,
        auth: r.get(4)?,
        header: r.get(5)?,
        created_ms: r.get(6)?,
        updated_ms: r.get(7)?,
    })
}

impl Store {
    /// Add `c` to its workspace: while it has fewer than `cap`, under a
    /// name it does not have yet.
    pub fn connector_add(&self, c: &ConnectorRow, cap: u64) -> Result<ConnectorAdd> {
        let now = harness_core::now_ms();
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let ws: Option<i64> = tx
            .query_row(
                "SELECT 1 FROM workspaces WHERE workspace_id = ?1",
                params![c.workspace_id],
                |r| r.get(0),
            )
            .optional()?;
        if ws.is_none() {
            return Ok(ConnectorAdd::NoWorkspace);
        }
        let taken: Option<i64> = tx
            .query_row(
                "SELECT 1 FROM workspace_connectors WHERE workspace_id = ?1 AND name = ?2",
                params![c.workspace_id, c.name],
                |r| r.get(0),
            )
            .optional()?;
        if taken.is_some() {
            return Ok(ConnectorAdd::NameTaken);
        }
        let n: i64 = tx.query_row(
            "SELECT COUNT(*) FROM workspace_connectors WHERE workspace_id = ?1",
            params![c.workspace_id],
            |r| r.get(0),
        )?;
        if n as u64 >= cap {
            return Ok(ConnectorAdd::OverCap);
        }
        tx.execute(
            "INSERT INTO workspace_connectors (connector_id, workspace_id, name, url, auth, header,
                 created_ms, updated_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![
                c.connector_id,
                c.workspace_id,
                c.name,
                c.url,
                c.auth,
                c.header,
                now
            ],
        )?;
        tx.commit()?;
        Ok(ConnectorAdd::Added)
    }

    /// Workspace `ws`'s connectors, by name.
    pub fn connectors_of(&self, ws: &str) -> Result<Vec<ConnectorRow>> {
        let conn = self.lock();
        let mut st = conn.prepare(&format!(
            "SELECT {COLS} FROM workspace_connectors WHERE workspace_id = ?1 ORDER BY name"
        ))?;
        let rows = st.query_map(params![ws], row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Connector `id`, if it is workspace `ws`'s.
    pub fn connector(&self, ws: &str, id: &str) -> Result<Option<ConnectorRow>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                &format!(
                    "SELECT {COLS} FROM workspace_connectors WHERE workspace_id = ?1 AND connector_id = ?2"
                ),
                params![ws, id],
                row,
            )
            .optional()?)
    }

    /// Change connector `id` of `ws` to `url`, `auth` and `header`. When the
    /// URL or the sign-in changes, every account of it is removed first, in
    /// the same transaction. `None`: no such connector of `ws`; else the
    /// row as it is now and how many accounts were removed.
    pub fn connector_update(
        &self,
        ws: &str,
        id: &str,
        url: &str,
        auth: &str,
        header: Option<&str>,
    ) -> Result<Option<(ConnectorRow, u64)>> {
        let now = harness_core::now_ms();
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let old = tx
            .query_row(
                &format!(
                    "SELECT {COLS} FROM workspace_connectors WHERE workspace_id = ?1 AND connector_id = ?2"
                ),
                params![ws, id],
                row,
            )
            .optional()?;
        let Some(old) = old else {
            return Ok(None);
        };
        let removed = if old.url != url || old.auth != auth {
            tx.execute(
                "DELETE FROM connector_accounts WHERE workspace_id = ?1 AND connector_id = ?2",
                params![ws, id],
            )? as u64
        } else {
            0
        };
        tx.execute(
            "UPDATE workspace_connectors SET url = ?3, auth = ?4, header = ?5, updated_ms = ?6
             WHERE workspace_id = ?1 AND connector_id = ?2",
            params![ws, id, url, auth, header, now],
        )?;
        let new = tx.query_row(
            &format!(
                "SELECT {COLS} FROM workspace_connectors WHERE workspace_id = ?1 AND connector_id = ?2"
            ),
            params![ws, id],
            row,
        )?;
        tx.commit()?;
        Ok(Some((new, removed)))
    }

    /// Remove connector `id` of `ws`, and every account of it; false when
    /// there was none.
    pub fn connector_delete(&self, ws: &str, id: &str) -> Result<bool> {
        let conn = self.lock();
        Ok(conn.execute(
            "DELETE FROM workspace_connectors WHERE workspace_id = ?1 AND connector_id = ?2",
            params![ws, id],
        )? > 0)
    }

    /// Store `user`'s sealed token for connector `id` of `ws`, checked
    /// against `url`: only while the connector is still there with that URL
    /// and a token sign-in, the person a member, and (for a new row) the
    /// person has fewer than `cap` accounts.
    pub fn connector_account_put(
        &self,
        user: &str,
        ws: &str,
        id: &str,
        url: &str,
        sealed: &[u8],
        cap: u64,
    ) -> Result<AccountPut> {
        let now = harness_core::now_ms();
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let member: Option<i64> = tx
            .query_row(
                "SELECT 1 FROM workspace_members WHERE workspace_id = ?1 AND user_id = ?2",
                params![ws, user],
                |r| r.get(0),
            )
            .optional()?;
        if member.is_none() {
            return Ok(AccountPut::NotMember);
        }
        let same: Option<i64> = tx
            .query_row(
                "SELECT 1 FROM workspace_connectors
                 WHERE workspace_id = ?1 AND connector_id = ?2 AND url = ?3 AND auth = 'token'",
                params![ws, id, url],
                |r| r.get(0),
            )
            .optional()?;
        if same.is_none() {
            return Ok(AccountPut::Changed);
        }
        let had = tx.execute(
            "UPDATE connector_accounts SET sealed_token = ?4, updated_ms = ?5
             WHERE user_id = ?1 AND workspace_id = ?2 AND connector_id = ?3",
            params![user, ws, id, sealed, now],
        )?;
        let out = if had > 0 {
            AccountPut::Replaced
        } else {
            let n: i64 = tx.query_row(
                "SELECT COUNT(*) FROM connector_accounts WHERE user_id = ?1",
                params![user],
                |r| r.get(0),
            )?;
            if n as u64 >= cap {
                return Ok(AccountPut::OverCap);
            }
            tx.execute(
                "INSERT INTO connector_accounts (user_id, workspace_id, connector_id, sealed_token,
                     created_ms, updated_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![user, ws, id, sealed, now],
            )?;
            AccountPut::Added
        };
        tx.commit()?;
        Ok(out)
    }

    /// `user`'s accounts in `ws`.
    pub fn connector_accounts_of(&self, user: &str, ws: &str) -> Result<Vec<AccountRow>> {
        let conn = self.lock();
        let mut st = conn.prepare(
            "SELECT connector_id, created_ms, updated_ms FROM connector_accounts
             WHERE user_id = ?1 AND workspace_id = ?2 ORDER BY connector_id",
        )?;
        let rows = st.query_map(params![user, ws], |r| {
            Ok(AccountRow {
                connector_id: r.get(0)?,
                created_ms: r.get(1)?,
                updated_ms: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The sealed token of `user`'s account for connector `id` of `ws`.
    pub fn connector_account_sealed(
        &self,
        user: &str,
        ws: &str,
        id: &str,
    ) -> Result<Option<Vec<u8>>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                "SELECT sealed_token FROM connector_accounts
                 WHERE user_id = ?1 AND workspace_id = ?2 AND connector_id = ?3",
                params![user, ws, id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Remove `user`'s account for connector `id` of `ws`; false when there
    /// was none of theirs.
    pub fn connector_account_delete(&self, user: &str, ws: &str, id: &str) -> Result<bool> {
        let conn = self.lock();
        Ok(conn.execute(
            "DELETE FROM connector_accounts
             WHERE user_id = ?1 AND workspace_id = ?2 AND connector_id = ?3",
            params![user, ws, id],
        )? > 0)
    }
}
