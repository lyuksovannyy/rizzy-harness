//! The skills store (ADOPTION_PLAN X.34 step B, PLAN.md T81): skills kept
//! as text on the master, a name, a description and a body, either the
//! server's own (`workspace_id` NULL, offered in every workspace) or one
//! workspace's (gone with it: foreign key cascade). The master checks the
//! text (`harness_tools::skills::check_stored`) and who may write; this
//! keeps one skill per name in a scope, refuses a workspace skill under a
//! server skill's name, and bounds each scope's count and bytes, all in the
//! write's own transaction, so concurrent writes cannot pass a cap.

use rusqlite::{OptionalExtension, params};

use crate::{Result, Store, StoreError};

/// Run by `Store::init`, after the workspaces.
pub(crate) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS stored_skills (
    -- NULL: the server's own skill; else the workspace's.
    workspace_id TEXT REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    name         TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 64),
    description  TEXT NOT NULL CHECK (length(description) BETWEEN 1 AND 1024),
    -- At most 24 KiB (the skill tool's body cap); the master refuses more first.
    body         TEXT NOT NULL CHECK (length(CAST(body AS BLOB)) BETWEEN 1 AND 24576),
    updated_ms   INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS stored_skills_name
    ON stored_skills(coalesce(workspace_id, ''), name);
"#;

/// Where a stored skill lives: the server's scope or one workspace's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillScope<'a> {
    Server,
    Workspace(&'a str),
}

impl SkillScope<'_> {
    fn ws(&self) -> Option<&str> {
        match self {
            SkillScope::Server => None,
            SkillScope::Workspace(w) => Some(w),
        }
    }
}

/// Bounds of one scope, checked in each write's transaction.
#[derive(Debug, Clone, Copy)]
pub struct SkillCaps {
    /// Skills one scope may hold.
    pub count: usize,
    /// Bytes of every name, description and body of one scope together.
    pub bytes: usize,
}

/// One stored skill. `body` is empty in a listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSkillRow {
    /// `None`: the server's.
    pub workspace_id: Option<String>,
    pub name: String,
    pub description: String,
    pub body: String,
    pub updated_ms: i64,
}

/// Why a write was refused: said to the person as it stands.
fn refused(s: impl Into<String>) -> StoreError {
    StoreError::Refused(s.into())
}

impl Store {
    /// The skills of `scope`, by name, without their bodies.
    pub fn stored_skills(&self, scope: SkillScope<'_>) -> Result<Vec<StoredSkillRow>> {
        let conn = self.lock();
        let mut st = conn.prepare(
            "SELECT workspace_id, name, description, updated_ms FROM stored_skills
             WHERE coalesce(workspace_id, '') = coalesce(?1, '') ORDER BY name",
        )?;
        let rows = st.query_map(params![scope.ws()], |r| {
            Ok(StoredSkillRow {
                workspace_id: r.get(0)?,
                name: r.get(1)?,
                description: r.get(2)?,
                body: String::new(),
                updated_ms: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// One skill of `scope`, with its body.
    pub fn stored_skill(
        &self,
        scope: SkillScope<'_>,
        name: &str,
    ) -> Result<Option<StoredSkillRow>> {
        Ok(self
            .lock()
            .query_row(
                "SELECT workspace_id, name, description, body, updated_ms FROM stored_skills
                 WHERE coalesce(workspace_id, '') = coalesce(?1, '') AND name = ?2",
                params![scope.ws(), name],
                |r| {
                    Ok(StoredSkillRow {
                        workspace_id: r.get(0)?,
                        name: r.get(1)?,
                        description: r.get(2)?,
                        body: r.get(3)?,
                        updated_ms: r.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    /// Store a new skill in `scope`. Refused: a skill of that name in the
    /// scope; in a workspace, a server skill of that name; past `caps`.
    /// `false` when the workspace does not exist.
    pub fn stored_skill_create(
        &self,
        scope: SkillScope<'_>,
        name: &str,
        description: &str,
        body: &str,
        caps: SkillCaps,
    ) -> Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        if let Some(ws) = scope.ws()
            && Self::workspace_in(&tx, ws)?.is_none()
        {
            return Ok(false);
        }
        let taken = |ws: Option<&str>| -> rusqlite::Result<bool> {
            tx.query_row(
                "SELECT count(*) FROM stored_skills
                 WHERE coalesce(workspace_id, '') = coalesce(?1, '') AND name = ?2",
                params![ws, name],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n > 0)
        };
        if taken(scope.ws())? {
            return Err(refused(format!(
                "there is already a skill `{name}` here: open it to change it"
            )));
        }
        if scope.ws().is_some() && taken(None)? {
            return Err(refused(format!(
                "`{name}` is the name of one of the server's own skills, which every workspace \
                 has: choose another name"
            )));
        }
        Self::within_caps(&tx, scope, None, name, description, body, caps)?;
        tx.execute(
            "INSERT INTO stored_skills (workspace_id, name, description, body, updated_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![scope.ws(), name, description, body, harness_core::now_ms()],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Replace the description and body of skill `name` of `scope`;
    /// `false` when there is none. Refused past `caps`.
    pub fn stored_skill_update(
        &self,
        scope: SkillScope<'_>,
        name: &str,
        description: &str,
        body: &str,
        caps: SkillCaps,
    ) -> Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        Self::within_caps(&tx, scope, Some(name), name, description, body, caps)?;
        let n = tx.execute(
            "UPDATE stored_skills SET description = ?3, body = ?4, updated_ms = ?5
             WHERE coalesce(workspace_id, '') = coalesce(?1, '') AND name = ?2",
            params![scope.ws(), name, description, body, harness_core::now_ms()],
        )?;
        tx.commit()?;
        Ok(n > 0)
    }

    /// Remove skill `name` of `scope`; whether there was one.
    pub fn stored_skill_delete(&self, scope: SkillScope<'_>, name: &str) -> Result<bool> {
        Ok(self.lock().execute(
            "DELETE FROM stored_skills
             WHERE coalesce(workspace_id, '') = coalesce(?1, '') AND name = ?2",
            params![scope.ws(), name],
        )? > 0)
    }

    /// Whether `scope` stays within `caps` with this skill written (`except`:
    /// the one it replaces).
    fn within_caps(
        tx: &rusqlite::Transaction<'_>,
        scope: SkillScope<'_>,
        except: Option<&str>,
        name: &str,
        description: &str,
        body: &str,
        caps: SkillCaps,
    ) -> Result<()> {
        let (count, bytes): (i64, i64) = tx.query_row(
            "SELECT count(*), coalesce(sum(length(CAST(name AS BLOB))
                     + length(CAST(description AS BLOB)) + length(CAST(body AS BLOB))), 0)
             FROM stored_skills
             WHERE coalesce(workspace_id, '') = coalesce(?1, '')
               AND (?2 IS NULL OR name <> ?2)",
            params![scope.ws(), except],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if except.is_none() && count as usize >= caps.count {
            return Err(refused(format!(
                "a scope holds at most {} skills: delete one first",
                caps.count
            )));
        }
        let size = name.len() + description.len() + body.len();
        if bytes as usize + size > caps.bytes {
            return Err(refused(format!(
                "the skills here hold at most {} KiB of text together: shorten or delete one \
                 first",
                caps.bytes / 1024
            )));
        }
        Ok(())
    }
}
