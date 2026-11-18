//! A person's own forge accounts (ADOPTION_PLAN X.27 step 1, PLAN.md T83,
//! docs/nodes-credentials-environments.md §4.1 `user_connections`).
//!
//! The token is stored sealed (the master's `sealed` module, bound to the
//! row's user, host and account); this crate never sees it in the clear and
//! [`ConnectionRow`] has no field for it: only [`Store::connection_sealed`]
//! returns the sealed bytes. Rows go with their user (foreign key cascade).
//! Every read and write names the user, so one person's id never reaches
//! another person's row.

use rusqlite::{OptionalExtension, params};

use crate::{Result, Store, StoreError};

/// Run by `Store::init`, after the users.
pub(crate) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS user_connections (
    connection_id TEXT PRIMARY KEY,
    user_id       TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    host          TEXT NOT NULL CHECK (length(host) BETWEEN 1 AND 253),
    account_id    TEXT NOT NULL CHECK (length(account_id) BETWEEN 1 AND 32),
    login         TEXT NOT NULL CHECK (length(login) BETWEEN 1 AND 255),
    display_name  TEXT CHECK (length(display_name) <= 1024),
    -- The e-mail the forge links to the account, when it gives one.
    commit_email  TEXT CHECK (length(commit_email) <= 1024),
    -- The token's scopes as the forge reported them (a JSON array), NULL
    -- when the forge does not report them.
    scopes        TEXT CHECK (length(scopes) <= 4096),
    -- Scopes the forge reported beyond those kept (the list is capped).
    scopes_more   INTEGER NOT NULL DEFAULT 0 CHECK (scopes_more >= 0),
    sealed_token  BLOB NOT NULL CHECK (length(sealed_token) <= 4096),
    created_ms    INTEGER NOT NULL,
    updated_ms    INTEGER NOT NULL,
    UNIQUE (user_id, host, account_id)
);
"#;

/// One connection as listed: never the token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionRow {
    pub connection_id: String,
    pub user_id: String,
    pub host: String,
    pub account_id: String,
    pub login: String,
    pub display_name: Option<String>,
    pub commit_email: Option<String>,
    /// `None`: the forge does not report scopes.
    pub scopes: Option<Vec<String>>,
    /// Scopes reported beyond `scopes` (not kept).
    pub scopes_more: u32,
    pub created_ms: i64,
    pub updated_ms: i64,
}

/// What to store: the account the forge answered for, and the sealed token.
#[derive(Debug, Clone)]
pub struct NewConnection {
    /// Used when the row is new; an existing row of the same user, host
    /// and account keeps its id.
    pub connection_id: String,
    pub user_id: String,
    pub host: String,
    pub account_id: String,
    pub login: String,
    pub display_name: Option<String>,
    pub commit_email: Option<String>,
    pub scopes: Option<Vec<String>>,
    pub scopes_more: u32,
    pub sealed_token: Vec<u8>,
}

/// A sealed token with its row's user, host and account.
pub type SealedSample = (String, String, String, Vec<u8>);

/// What [`Store::connection_put`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionPut {
    /// A new row.
    Added(String),
    /// The same user, host and account had a row: its token and details
    /// were replaced.
    Replaced(String),
    /// The user already has `cap` connections.
    OverCap,
}

const COLS: &str = "connection_id, user_id, host, account_id, login, display_name, commit_email, \
                    scopes, created_ms, updated_ms, scopes_more";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ConnectionRow> {
    let scopes: Option<String> = r.get(7)?;
    Ok(ConnectionRow {
        connection_id: r.get(0)?,
        user_id: r.get(1)?,
        host: r.get(2)?,
        account_id: r.get(3)?,
        login: r.get(4)?,
        display_name: r.get(5)?,
        commit_email: r.get(6)?,
        // A list that does not read is shown as "not reported".
        scopes: scopes.and_then(|s| serde_json::from_str(&s).ok()),
        created_ms: r.get(8)?,
        updated_ms: r.get(9)?,
        scopes_more: r.get(10)?,
    })
}

impl Store {
    /// Add `c`, or replace the row of the same user, host and account; a
    /// new row only while the user has fewer than `cap`.
    pub fn connection_put(&self, c: &NewConnection, cap: u64) -> Result<ConnectionPut> {
        let scopes = match &c.scopes {
            Some(s) => {
                Some(serde_json::to_string(s).map_err(|e| StoreError::Refused(e.to_string()))?)
            }
            None => None,
        };
        let now = harness_core::now_ms();
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT connection_id FROM user_connections
                 WHERE user_id = ?1 AND host = ?2 AND account_id = ?3",
                params![c.user_id, c.host, c.account_id],
                |r| r.get(0),
            )
            .optional()?;
        let out = match existing {
            Some(id) => {
                tx.execute(
                    "UPDATE user_connections SET login = ?2, display_name = ?3, commit_email = ?4,
                         scopes = ?5, sealed_token = ?6, updated_ms = ?7, scopes_more = ?8
                     WHERE connection_id = ?1",
                    params![
                        id,
                        c.login,
                        c.display_name,
                        c.commit_email,
                        scopes,
                        c.sealed_token,
                        now,
                        c.scopes_more
                    ],
                )?;
                ConnectionPut::Replaced(id)
            }
            None => {
                let n: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM user_connections WHERE user_id = ?1",
                    params![c.user_id],
                    |r| r.get(0),
                )?;
                if n as u64 >= cap {
                    return Ok(ConnectionPut::OverCap);
                }
                tx.execute(
                    "INSERT INTO user_connections (connection_id, user_id, host, account_id, login,
                         display_name, commit_email, scopes, sealed_token, created_ms, updated_ms,
                         scopes_more)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10, ?11)",
                    params![
                        c.connection_id,
                        c.user_id,
                        c.host,
                        c.account_id,
                        c.login,
                        c.display_name,
                        c.commit_email,
                        scopes,
                        c.sealed_token,
                        now,
                        c.scopes_more
                    ],
                )?;
                ConnectionPut::Added(c.connection_id.clone())
            }
        };
        tx.commit()?;
        Ok(out)
    }

    /// `user`'s connections, by host and login.
    pub fn connections_of(&self, user: &str) -> Result<Vec<ConnectionRow>> {
        let conn = self.lock();
        let mut st = conn.prepare(&format!(
            "SELECT {COLS} FROM user_connections WHERE user_id = ?1 ORDER BY host, login, connection_id"
        ))?;
        let rows = st.query_map(params![user], row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// `user`'s connection `id`, if it is theirs.
    pub fn connection(&self, user: &str, id: &str) -> Result<Option<ConnectionRow>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                &format!(
                    "SELECT {COLS} FROM user_connections WHERE user_id = ?1 AND connection_id = ?2"
                ),
                params![user, id],
                row,
            )
            .optional()?)
    }

    /// The sealed token of `user`'s connection `id`, if it is theirs.
    pub fn connection_sealed(&self, user: &str, id: &str) -> Result<Option<Vec<u8>>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                "SELECT sealed_token FROM user_connections WHERE user_id = ?1 AND connection_id = ?2",
                params![user, id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Remove `user`'s connection `id`; false when there was none of theirs.
    pub fn connection_delete(&self, user: &str, id: &str) -> Result<bool> {
        let conn = self.lock();
        Ok(conn.execute(
            "DELETE FROM user_connections WHERE user_id = ?1 AND connection_id = ?2",
            params![user, id],
        )? > 0)
    }

    /// How many sealed values the database holds (a missing key file is
    /// made again only at 0).
    pub fn sealed_count(&self) -> Result<u64> {
        let conn = self.lock();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM user_connections", [], |r| r.get(0))?;
        Ok(n as u64)
    }

    /// Up to `n` sealed values with their rows (user, host, account), to
    /// prove a key opens what is stored before it is used.
    pub fn sealed_samples(&self, n: usize) -> Result<Vec<SealedSample>> {
        let conn = self.lock();
        let mut st = conn.prepare(
            "SELECT user_id, host, account_id, sealed_token FROM user_connections
             ORDER BY created_ms, connection_id LIMIT ?1",
        )?;
        let rows = st.query_map(params![n as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}
