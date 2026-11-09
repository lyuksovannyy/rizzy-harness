//! Routines: saved instructions that start a chat on a schedule or a
//! webhook, and their runs (docs/routines.md). Storage only: what a routine
//! may do, when it fires and how a run goes are the master's. Every time
//! comes from the caller (the master's routine clock), never from here.
//! node.db and chat.db get the empty tables with the shared schema.

use std::collections::HashMap;

use harness_core::SessionId;
use rusqlite::types::Value as Sql;
use rusqlite::{OptionalExtension, Transaction, params, params_from_iter};

use crate::{Result, Store, StoreError};

/// Run by `Store::init`, after the workspace and user tables it references.
pub(crate) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS routines (
    routine_id   TEXT PRIMARY KEY,                                                     -- new_id()
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    owner_id     TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    enabled      INTEGER NOT NULL DEFAULT 1,
    paused_reason TEXT,                       -- set by the master: blocked | failures | cap
    spec         TEXT NOT NULL,               -- JSON RoutineSpec (8.3)
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    created_ms   INTEGER NOT NULL,
    updated_ms   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS routines_owner ON routines(owner_id, workspace_id);
CREATE TABLE IF NOT EXISTS routine_triggers (
    trigger_id  TEXT PRIMARY KEY,             -- random_hex(16); also a webhook's public id
    routine_id  TEXT NOT NULL REFERENCES routines(routine_id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,                -- schedule | webhook (later sources add names, §15.2)
    config      TEXT NOT NULL,                -- JSON (8.3)
    next_run_ms INTEGER,                      -- schedules only; NULL = nothing due
    secret_hash TEXT,                         -- webhook: SHA-256 hex of the bearer secret
    last_fired_ms INTEGER
);
CREATE INDEX IF NOT EXISTS routine_triggers_due ON routine_triggers(next_run_ms) WHERE next_run_ms IS NOT NULL;
CREATE INDEX IF NOT EXISTS routine_triggers_routine ON routine_triggers(routine_id);
CREATE TABLE IF NOT EXISTS routine_runs (
    run_id       TEXT PRIMARY KEY,            -- new_id()
    routine_id   TEXT NOT NULL REFERENCES routines(routine_id) ON DELETE CASCADE,
    trigger_id   TEXT,
    trigger_kind TEXT NOT NULL,               -- schedule | webhook | manual
    scheduled_ms INTEGER,
    started_ms   INTEGER NOT NULL,
    ended_ms     INTEGER,
    status       TEXT NOT NULL,   -- pending|running|waiting|completed|error|cancelled|interrupted|timed_out|failed|skipped|blocked|missed
    reason       TEXT,
    session_id   TEXT UNIQUE,                 -- no FK: the mirror may lag; cleared by delete_session
    chat_deleted INTEGER NOT NULL DEFAULT 0,  -- 1: its chat was deleted (by the owner or by retention)
    turn_id      TEXT,
    waiting_since_ms INTEGER,
    prompt       TEXT,                        -- rendered, <= 32 KiB; cleared when the chat is deleted
    event        TEXT,                        -- JSON, <= 16 KiB; cleared when the chat is deleted
    untrusted    INTEGER NOT NULL DEFAULT 0,
    warnings     TEXT,                        -- cleared when the chat is deleted
    usage        TEXT,
    attempt      INTEGER NOT NULL DEFAULT 1,
    repeats      INTEGER NOT NULL DEFAULT 1,  -- coalesced skipped/missed
    seen         INTEGER NOT NULL DEFAULT 0,  -- the owner viewed the run's chat or its attention row
    promoted     INTEGER NOT NULL DEFAULT 0   -- the owner prompted in the run's chat
);
CREATE INDEX IF NOT EXISTS routine_runs_routine ON routine_runs(routine_id, started_ms DESC);
CREATE INDEX IF NOT EXISTS routine_runs_active ON routine_runs(status)
    WHERE status IN ('pending','running','waiting');
-- Webhook Idempotency-Key, remembered for 24 h (purged hourly).
CREATE TABLE IF NOT EXISTS routine_dedupe (
    routine_id TEXT NOT NULL REFERENCES routines(routine_id) ON DELETE CASCADE,
    key        TEXT NOT NULL,                 -- SHA-256 hex of the header value
    run_id     TEXT NOT NULL,
    expires_ms INTEGER NOT NULL,
    PRIMARY KEY (routine_id, key)
);
-- Added in phase B (a new table; the four above are frozen). Chats that hold
-- data from an external event: written with the run that made them untrusted,
-- so the flag outlives the run row when its routine or workspace is deleted
-- (both cascade the runs), and removed only with the chat.
CREATE TABLE IF NOT EXISTS routine_untrusted_chats (
    session_id TEXT PRIMARY KEY
) WITHOUT ROWID;
"#;

/// How long a webhook's `Idempotency-Key` is remembered.
pub const DEDUPE_MS: i64 = 24 * 3600 * 1000;

/// Runs that have not finished: the overlap rule, the watcher and the
/// restart reconcile work on these.
const ACTIVE: &str = "('pending','running','waiting')";
/// Runs with a result the owner should see: these count as unseen.
const FINISHED: &str =
    "('completed','error','failed','timed_out','interrupted','cancelled','blocked')";

/// What a chat started by a routine run shows: which routine, which run and
/// how that run stands (`SessionRow.routine` on the wire).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RoutineMark {
    pub routine_id: String,
    pub name: String,
    pub run_id: String,
    pub status: String,
    pub seen: bool,
    pub promoted: bool,
    pub untrusted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutineRow {
    pub routine_id: String,
    pub workspace_id: String,
    pub owner_id: String,
    pub name: String,
    pub enabled: bool,
    /// `blocked`, `failures` or `cap`; the routine does not fire while set.
    pub paused_reason: Option<String>,
    /// JSON `RoutineSpec`, read by the master.
    pub spec: String,
    pub consecutive_failures: i64,
    pub created_ms: i64,
    pub updated_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerRow {
    pub trigger_id: String,
    pub routine_id: String,
    pub kind: String,
    /// JSON, read by the master.
    pub config: String,
    pub next_run_ms: Option<i64>,
    pub secret_hash: Option<String>,
    pub last_fired_ms: Option<i64>,
}

/// A trigger to save with a routine. An id the routine already has keeps
/// that trigger's secret (unless one is given) and its last fire.
#[derive(Debug, Clone)]
pub struct NewTrigger {
    pub trigger_id: String,
    pub kind: String,
    pub config: String,
    pub next_run_ms: Option<i64>,
    pub secret_hash: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewRoutine {
    pub routine_id: String,
    pub workspace_id: String,
    pub owner_id: String,
    pub name: String,
    pub enabled: bool,
    pub spec: String,
    pub triggers: Vec<NewTrigger>,
}

/// How many routines one account, and one workspace, may hold.
#[derive(Debug, Clone, Copy)]
pub struct RoutineCaps {
    pub per_user: usize,
    pub per_workspace: usize,
}

/// Changes to a routine; `None` leaves a field as is.
#[derive(Debug, Clone, Default)]
pub struct RoutinePatch {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub spec: Option<String>,
    /// The whole new trigger set (ids the routine has are kept).
    pub triggers: Option<Vec<NewTrigger>>,
    /// Clear `paused_reason` and `consecutive_failures` (enabling).
    pub resume: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRow {
    pub run_id: String,
    pub routine_id: String,
    pub trigger_id: Option<String>,
    pub trigger_kind: String,
    pub scheduled_ms: Option<i64>,
    pub started_ms: i64,
    pub ended_ms: Option<i64>,
    pub status: String,
    pub reason: Option<String>,
    pub session_id: Option<String>,
    pub chat_deleted: bool,
    pub turn_id: Option<String>,
    pub waiting_since_ms: Option<i64>,
    pub prompt: Option<String>,
    pub event: Option<String>,
    pub untrusted: bool,
    /// JSON array of strings.
    pub warnings: Option<String>,
    /// JSON `Usage`.
    pub usage: Option<String>,
    pub attempt: i64,
    pub repeats: i64,
    pub seen: bool,
    pub promoted: bool,
}

impl RunRow {
    pub fn active(&self) -> bool {
        matches!(self.status.as_str(), "pending" | "running" | "waiting")
    }
}

/// A run to record. `skipped` and `missed` runs fold into the routine's
/// newest run when it has the same status and reason (`repeats` adds up),
/// so a stalled routine leaves one row, not hundreds.
#[derive(Debug, Clone)]
pub struct NewRun {
    pub run_id: String,
    pub routine_id: String,
    pub trigger_id: Option<String>,
    pub trigger_kind: String,
    pub scheduled_ms: Option<i64>,
    pub started_ms: i64,
    pub status: String,
    pub reason: Option<String>,
    pub repeats: i64,
    /// The event that fired it (JSON, already cut to fit 16 KiB).
    pub event: Option<String>,
}

/// What recording a fire did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claimed {
    /// A `pending` run was inserted: the caller executes it.
    Started,
    /// Another run of the routine had not finished: a `skipped` run was
    /// recorded instead (overlap is always "skip" in v1).
    Overlap,
    /// The caller's own `skipped` or `missed` run was recorded.
    Recorded,
}

/// The reason an overlapping fire is skipped with.
pub const OVERLAP: &str = "the previous run had not finished";

/// What recording a webhook call did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookClaim {
    /// Recorded as `claimed`; the run id is the new run's.
    Claimed(Claimed, String),
    /// The `Idempotency-Key` came with an earlier call that started this
    /// run, within the last 24 hours: nothing new was recorded.
    Duplicate(String),
}

/// Changes to a run; `None` leaves a field as is.
#[derive(Debug, Clone, Default)]
pub struct RunPatch {
    pub status: Option<String>,
    pub reason: Option<Option<String>>,
    pub ended_ms: Option<i64>,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub waiting_since_ms: Option<Option<i64>>,
    pub prompt: Option<String>,
    pub untrusted: Option<bool>,
    pub warnings: Option<String>,
    pub usage: Option<String>,
    pub attempt: Option<i64>,
    /// Only while the run has not finished: a late event never rewrites a
    /// result (a timeout recorded before the cancelled turn ends).
    pub only_active: bool,
}

/// One routine's line in the owner's attention list: its newest unseen
/// finished run and how many there are, or its pause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionRow {
    pub routine_id: String,
    pub name: String,
    pub workspace_id: String,
    /// JSON `RoutineSpec` (the master reads `notify`).
    pub spec: String,
    /// `None` for a pause row.
    pub paused_reason: Option<String>,
    pub run_id: Option<String>,
    pub session_id: Option<String>,
    pub status: Option<String>,
    pub reason: Option<String>,
    pub unseen: i64,
    pub ts_ms: i64,
}

const ROUTINE_COLS: &str = "routine_id, workspace_id, owner_id, name, enabled, paused_reason, \
     spec, consecutive_failures, created_ms, updated_ms";

fn routine_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RoutineRow> {
    Ok(RoutineRow {
        routine_id: r.get(0)?,
        workspace_id: r.get(1)?,
        owner_id: r.get(2)?,
        name: r.get(3)?,
        enabled: r.get(4)?,
        paused_reason: r.get(5)?,
        spec: r.get(6)?,
        consecutive_failures: r.get(7)?,
        created_ms: r.get(8)?,
        updated_ms: r.get(9)?,
    })
}

const TRIGGER_COLS: &str =
    "trigger_id, routine_id, kind, config, next_run_ms, secret_hash, last_fired_ms";

fn trigger_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<TriggerRow> {
    Ok(TriggerRow {
        trigger_id: r.get(0)?,
        routine_id: r.get(1)?,
        kind: r.get(2)?,
        config: r.get(3)?,
        next_run_ms: r.get(4)?,
        secret_hash: r.get(5)?,
        last_fired_ms: r.get(6)?,
    })
}

const RUN_COLS: &str = "r.run_id, r.routine_id, r.trigger_id, r.trigger_kind, r.scheduled_ms, \
     r.started_ms, r.ended_ms, r.status, r.reason, r.session_id, r.chat_deleted, r.turn_id, \
     r.waiting_since_ms, r.prompt, r.event, r.untrusted, r.warnings, r.usage, r.attempt, \
     r.repeats, r.seen, r.promoted";

fn run_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RunRow> {
    Ok(RunRow {
        run_id: r.get(0)?,
        routine_id: r.get(1)?,
        trigger_id: r.get(2)?,
        trigger_kind: r.get(3)?,
        scheduled_ms: r.get(4)?,
        started_ms: r.get(5)?,
        ended_ms: r.get(6)?,
        status: r.get(7)?,
        reason: r.get(8)?,
        session_id: r.get(9)?,
        chat_deleted: r.get(10)?,
        turn_id: r.get(11)?,
        waiting_since_ms: r.get(12)?,
        prompt: r.get(13)?,
        event: r.get(14)?,
        untrusted: r.get(15)?,
        warnings: r.get(16)?,
        usage: r.get(17)?,
        attempt: r.get(18)?,
        repeats: r.get(19)?,
        seen: r.get(20)?,
        promoted: r.get(21)?,
    })
}

fn count(tx: &Transaction<'_>, sql: &str, key: &str) -> Result<usize> {
    let n: i64 = tx.query_row(sql, params![key], |r| r.get(0))?;
    Ok(n as usize)
}

/// A name the owner already uses in this workspace (case-insensitive) is
/// refused: their list would show two routines they cannot tell apart.
fn check_name(tx: &Transaction<'_>, owner: &str, ws: &str, name: &str, id: &str) -> Result<()> {
    let taken: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM routines WHERE owner_id = ?1 AND workspace_id = ?2
             AND name = ?3 COLLATE NOCASE AND routine_id != ?4)",
        params![owner, ws, name, id],
        |r| r.get(0),
    )?;
    if taken {
        return Err(StoreError::Conflict(format!(
            "you already have a routine named {name:?} here"
        )));
    }
    Ok(())
}

/// Replace a routine's triggers with `set`. Ids it already has keep their
/// row, so a webhook URL survives an edit; ids it does not have are new.
fn save_triggers(tx: &Transaction<'_>, routine: &str, set: &[NewTrigger]) -> Result<()> {
    let keep: Vec<&str> = set.iter().map(|t| t.trigger_id.as_str()).collect();
    let mut old = tx.prepare("SELECT trigger_id FROM routine_triggers WHERE routine_id = ?1")?;
    let existing: Vec<String> = old
        .query_map(params![routine], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    drop(old);
    for id in existing.iter().filter(|id| !keep.contains(&id.as_str())) {
        tx.execute(
            "DELETE FROM routine_triggers WHERE trigger_id = ?1",
            params![id],
        )?;
    }
    for t in set {
        if existing.contains(&t.trigger_id) {
            tx.execute(
                "UPDATE routine_triggers SET kind = ?2, config = ?3, next_run_ms = ?4,
                     secret_hash = COALESCE(?5, secret_hash)
                 WHERE trigger_id = ?1",
                params![t.trigger_id, t.kind, t.config, t.next_run_ms, t.secret_hash],
            )?;
        } else {
            tx.execute(
                "INSERT INTO routine_triggers (trigger_id, routine_id, kind, config, next_run_ms, secret_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    t.trigger_id,
                    routine,
                    t.kind,
                    t.config,
                    t.next_run_ms,
                    t.secret_hash
                ],
            )?;
        }
    }
    Ok(())
}

fn insert_run(tx: &Transaction<'_>, run: &NewRun) -> Result<()> {
    // Skipped and missed runs are over the moment they are recorded.
    let ended = (run.status != "pending").then_some(run.started_ms);
    tx.execute(
        "INSERT INTO routine_runs (run_id, routine_id, trigger_id, trigger_kind, scheduled_ms,
             started_ms, ended_ms, status, reason, repeats, event)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            run.run_id,
            run.routine_id,
            run.trigger_id,
            run.trigger_kind,
            run.scheduled_ms,
            run.started_ms,
            ended,
            run.status,
            run.reason,
            run.repeats,
            run.event
        ],
    )?;
    Ok(())
}

/// Mark the chat of untrusted run `run_id` as holding event data, for good:
/// the mark stays when the run row goes with its routine.
fn mark_untrusted_chat(tx: &Transaction<'_>, run_id: &str) -> Result<()> {
    tx.execute(
        "INSERT OR IGNORE INTO routine_untrusted_chats (session_id)
         SELECT session_id FROM routine_runs
         WHERE run_id = ?1 AND untrusted = 1 AND session_id IS NOT NULL",
        params![run_id],
    )?;
    Ok(())
}

/// `Store::chat_untrusted` on an open connection. One hop up is the whole
/// chain: delegation is one level deep (a delegate's spec has no delegates
/// of its own), and a deleted parent hands its mark down
/// (`on_session_deleted`).
fn untrusted(conn: &rusqlite::Connection, sid: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM routine_untrusted_chats WHERE session_id = ?1
                OR session_id = (SELECT parent_id FROM sessions WHERE session_id = ?1))",
        params![sid],
        |r| r.get(0),
    )?)
}

/// The routine's newest run other than `exclude`, when it is a `skipped`
/// or `missed` run with this status and reason: the one to fold into.
fn foldable(
    tx: &Transaction<'_>,
    routine: &str,
    status: &str,
    reason: Option<&str>,
    exclude: &str,
) -> Result<Option<String>> {
    if !matches!(status, "skipped" | "missed") {
        return Ok(None);
    }
    let newest: Option<(String, String, Option<String>)> = tx
        .query_row(
            "SELECT run_id, status, reason FROM routine_runs
             WHERE routine_id = ?1 AND run_id != ?2
             ORDER BY started_ms DESC, run_id DESC LIMIT 1",
            params![routine, exclude],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    Ok(newest
        .filter(|(_, s, r)| s == status && r.as_deref() == reason)
        .map(|(id, _, _)| id))
}

/// Insert `run`, or fold a skipped or missed one into the newest run.
fn record(tx: &Transaction<'_>, run: &NewRun) -> Result<()> {
    match foldable(
        tx,
        &run.routine_id,
        &run.status,
        run.reason.as_deref(),
        &run.run_id,
    )? {
        Some(id) => {
            tx.execute(
                "UPDATE routine_runs SET repeats = repeats + ?2, ended_ms = ?3 WHERE run_id = ?1",
                params![id, run.repeats, run.started_ms],
            )?;
            Ok(())
        }
        None => insert_run(tx, run),
    }
}

/// Start `run` unless the routine has one that has not finished (then a
/// skipped run is recorded); record a skipped or missed one as it is.
fn start_or_record(tx: &Transaction<'_>, run: &NewRun) -> Result<Claimed> {
    if run.status != "pending" {
        record(tx, run)?;
        return Ok(Claimed::Recorded);
    }
    let busy: bool = tx.query_row(
        &format!(
            "SELECT EXISTS(SELECT 1 FROM routine_runs WHERE routine_id = ?1 AND status IN {ACTIVE})"
        ),
        params![run.routine_id],
        |r| r.get(0),
    )?;
    if busy {
        let skipped = NewRun {
            status: "skipped".into(),
            reason: Some(OVERLAP.into()),
            ..run.clone()
        };
        record(tx, &skipped)?;
        return Ok(Claimed::Overlap);
    }
    insert_run(tx, run)?;
    Ok(Claimed::Started)
}

impl Store {
    // ---- routines ----

    /// Create a routine with its triggers. Refused (409) over the caps or
    /// when the owner already has one of that name in the workspace.
    pub fn routine_create(
        &self,
        r: &NewRoutine,
        caps: RoutineCaps,
        now: i64,
    ) -> Result<RoutineRow> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let mine = count(
            &tx,
            "SELECT COUNT(*) FROM routines WHERE owner_id = ?1",
            &r.owner_id,
        )?;
        if mine >= caps.per_user {
            return Err(StoreError::Refused(format!(
                "an account has at most {} routines",
                caps.per_user
            )));
        }
        let here = count(
            &tx,
            "SELECT COUNT(*) FROM routines WHERE workspace_id = ?1",
            &r.workspace_id,
        )?;
        if here >= caps.per_workspace {
            return Err(StoreError::Refused(format!(
                "a workspace has at most {} routines",
                caps.per_workspace
            )));
        }
        check_name(&tx, &r.owner_id, &r.workspace_id, &r.name, &r.routine_id)?;
        tx.execute(
            &format!(
                "INSERT INTO routines ({ROUTINE_COLS}) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, 0, ?7, ?7)"
            ),
            params![
                r.routine_id,
                r.workspace_id,
                r.owner_id,
                r.name,
                r.enabled,
                r.spec,
                now
            ],
        )?;
        save_triggers(&tx, &r.routine_id, &r.triggers)?;
        let row = tx.query_row(
            &format!("SELECT {ROUTINE_COLS} FROM routines WHERE routine_id = ?1"),
            params![r.routine_id],
            routine_row,
        )?;
        tx.commit()?;
        Ok(row)
    }

    /// Apply `p`. Returns the routine as it is now, `None` if it is gone.
    pub fn routine_update(
        &self,
        id: &str,
        p: &RoutinePatch,
        now: i64,
    ) -> Result<Option<RoutineRow>> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let Some(cur) = tx
            .query_row(
                &format!("SELECT {ROUTINE_COLS} FROM routines WHERE routine_id = ?1"),
                params![id],
                routine_row,
            )
            .optional()?
        else {
            return Ok(None);
        };
        if let Some(name) = &p.name {
            check_name(&tx, &cur.owner_id, &cur.workspace_id, name, id)?;
        }
        tx.execute(
            "UPDATE routines SET name = COALESCE(?2, name), enabled = COALESCE(?3, enabled),
                 spec = COALESCE(?4, spec), updated_ms = ?5,
                 paused_reason = CASE WHEN ?6 THEN NULL ELSE paused_reason END,
                 consecutive_failures = CASE WHEN ?6 THEN 0 ELSE consecutive_failures END
             WHERE routine_id = ?1",
            params![id, p.name, p.enabled, p.spec, now, p.resume],
        )?;
        if let Some(set) = &p.triggers {
            save_triggers(&tx, id, set)?;
        }
        let row = tx.query_row(
            &format!("SELECT {ROUTINE_COLS} FROM routines WHERE routine_id = ?1"),
            params![id],
            routine_row,
        )?;
        tx.commit()?;
        Ok(Some(row))
    }

    /// Delete a routine; its triggers and runs go with it. The chats its
    /// runs started stay. Returns whether it existed.
    pub fn routine_delete(&self, id: &str) -> Result<bool> {
        Ok(self
            .lock()
            .execute("DELETE FROM routines WHERE routine_id = ?1", params![id])?
            > 0)
    }

    pub fn routine(&self, id: &str) -> Result<Option<RoutineRow>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                &format!("SELECT {ROUTINE_COLS} FROM routines WHERE routine_id = ?1"),
                params![id],
                routine_row,
            )
            .optional()?)
    }

    /// Routines of `owner` (anyone's for `None`) in `workspace` (any for
    /// `None`), oldest first.
    pub fn routines_for(
        &self,
        owner: Option<&str>,
        workspace: Option<&str>,
    ) -> Result<Vec<RoutineRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {ROUTINE_COLS} FROM routines
             WHERE (?1 IS NULL OR owner_id = ?1) AND (?2 IS NULL OR workspace_id = ?2)
             ORDER BY created_ms, routine_id"
        ))?;
        let rows = stmt.query_map(params![owner, workspace], routine_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Routines paused with `reason`, oldest first.
    pub fn routines_paused(&self, reason: &str) -> Result<Vec<RoutineRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {ROUTINE_COLS} FROM routines WHERE paused_reason = ?1 ORDER BY created_ms"
        ))?;
        let rows = stmt.query_map(params![reason], routine_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Pause a routine with `reason` unless it is paused already. Returns
    /// whether this paused it.
    pub fn routine_pause(&self, id: &str, reason: &str, now: i64) -> Result<bool> {
        Ok(self.lock().execute(
            "UPDATE routines SET paused_reason = ?2, updated_ms = ?3
             WHERE routine_id = ?1 AND paused_reason IS NULL",
            params![id, reason, now],
        )? > 0)
    }

    /// Count a finished run against the routine: a failure adds one to its
    /// failures in a row, anything else resets them. Returns the count.
    pub fn routine_count_failure(&self, id: &str, failed: bool) -> Result<i64> {
        let conn = self.lock();
        conn.execute(
            "UPDATE routines SET consecutive_failures =
                 CASE WHEN ?2 THEN consecutive_failures + 1 ELSE 0 END
             WHERE routine_id = ?1",
            params![id, failed],
        )?;
        Ok(conn
            .query_row(
                "SELECT consecutive_failures FROM routines WHERE routine_id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0))
    }

    // ---- triggers ----

    pub fn trigger(&self, id: &str) -> Result<Option<TriggerRow>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                &format!("SELECT {TRIGGER_COLS} FROM routine_triggers WHERE trigger_id = ?1"),
                params![id],
                trigger_row,
            )
            .optional()?)
    }

    /// A routine's triggers, in the order they were added.
    pub fn triggers(&self, routine: &str) -> Result<Vec<TriggerRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {TRIGGER_COLS} FROM routine_triggers WHERE routine_id = ?1 ORDER BY rowid"
        ))?;
        let rows = stmt.query_map(params![routine], trigger_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Triggers due at `now` (earliest first) of routines that are enabled
    /// and not paused, with their routine.
    pub fn routine_triggers_due(
        &self,
        now: i64,
        limit: usize,
    ) -> Result<Vec<(TriggerRow, RoutineRow)>> {
        let conn = self.lock();
        let cols: Vec<String> = TRIGGER_COLS
            .split(", ")
            .map(|c| format!("t.{c}"))
            .chain(ROUTINE_COLS.split(", ").map(|c| format!("o.{}", c.trim())))
            .collect();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {} FROM routine_triggers t JOIN routines o ON o.routine_id = t.routine_id
             WHERE t.next_run_ms IS NOT NULL AND t.next_run_ms <= ?1
               AND o.enabled = 1 AND o.paused_reason IS NULL
             ORDER BY t.next_run_ms LIMIT ?2",
            cols.join(", ")
        ))?;
        let rows = stmt.query_map(params![now, limit as i64], |r| {
            let t = trigger_row(r)?;
            let o = RoutineRow {
                routine_id: r.get(7)?,
                workspace_id: r.get(8)?,
                owner_id: r.get(9)?,
                name: r.get(10)?,
                enabled: r.get(11)?,
                paused_reason: r.get(12)?,
                spec: r.get(13)?,
                consecutive_failures: r.get(14)?,
                created_ms: r.get(15)?,
                updated_ms: r.get(16)?,
            };
            Ok((t, o))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// When the next trigger of an enabled, unpaused routine is due.
    pub fn routine_next_due(&self) -> Result<Option<i64>> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT MIN(t.next_run_ms) FROM routine_triggers t
             JOIN routines o ON o.routine_id = t.routine_id
             WHERE t.next_run_ms IS NOT NULL AND o.enabled = 1 AND o.paused_reason IS NULL",
            [],
            |r| r.get(0),
        )?)
    }

    /// Fire a trigger's occurrence once: in one transaction, move the
    /// trigger from `expected_next` to `new_next` and record `run`, but
    /// only if the trigger still stood at `expected_next`. `None`: someone
    /// else fired it (a double fire is impossible).
    pub fn claim_occurrence(
        &self,
        trigger_id: &str,
        expected_next: i64,
        new_next: Option<i64>,
        run: &NewRun,
    ) -> Result<Option<Claimed>> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let moved = tx.execute(
            "UPDATE routine_triggers SET next_run_ms = ?3 WHERE trigger_id = ?1 AND next_run_ms = ?2",
            params![trigger_id, expected_next, new_next],
        )?;
        if moved != 1 {
            return Ok(None);
        }
        let claimed = start_or_record(&tx, run)?;
        if claimed == Claimed::Started {
            tx.execute(
                "UPDATE routine_triggers SET last_fired_ms = ?2 WHERE trigger_id = ?1",
                params![trigger_id, run.started_ms],
            )?;
        }
        tx.commit()?;
        Ok(Some(claimed))
    }

    /// Record a webhook call as `run`, in one transaction. With a `key`
    /// (the SHA-256 hex of its `Idempotency-Key`) that an earlier call of
    /// the last 24 hours brought, nothing is recorded: `Duplicate` names
    /// that call's run. A key is remembered once its call started a run;
    /// a call skipped for overlap left nothing to repeat, so a retry of it
    /// is a new call.
    pub fn webhook_claim(
        &self,
        routine_id: &str,
        trigger_id: &str,
        key: Option<&str>,
        run: &NewRun,
        now: i64,
    ) -> Result<HookClaim> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        if let Some(key) = key {
            let first: Option<String> = tx
                .query_row(
                    "SELECT run_id FROM routine_dedupe
                     WHERE routine_id = ?1 AND key = ?2 AND expires_ms > ?3",
                    params![routine_id, key, now],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(first) = first {
                return Ok(HookClaim::Duplicate(first));
            }
        }
        let claimed = start_or_record(&tx, run)?;
        if claimed == Claimed::Started {
            if let Some(key) = key {
                // An expired row for the key is replaced.
                tx.execute(
                    "INSERT OR REPLACE INTO routine_dedupe (routine_id, key, run_id, expires_ms)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![routine_id, key, run.run_id, now + DEDUPE_MS],
                )?;
            }
            tx.execute(
                "UPDATE routine_triggers SET last_fired_ms = ?2 WHERE trigger_id = ?1",
                params![trigger_id, run.started_ms],
            )?;
        }
        tx.commit()?;
        Ok(HookClaim::Claimed(claimed, run.run_id.clone()))
    }

    /// Forget `Idempotency-Key`s whose 24 hours are over. Returns how many.
    pub fn dedupe_purge(&self, now: i64) -> Result<usize> {
        Ok(self.lock().execute(
            "DELETE FROM routine_dedupe WHERE expires_ms <= ?1",
            params![now],
        )?)
    }

    /// The webhook trigger `id` with its routine, if there is one.
    pub fn hook_target(&self, id: &str) -> Result<Option<(TriggerRow, RoutineRow)>> {
        let Some(t) = self.trigger(id)?.filter(|t| t.kind == "webhook") else {
            return Ok(None);
        };
        Ok(self.routine(&t.routine_id)?.map(|r| (t, r)))
    }

    /// Give trigger `id` a new secret (its SHA-256 hex). Returns whether
    /// the trigger exists.
    pub fn trigger_set_secret(&self, id: &str, secret_hash: &str) -> Result<bool> {
        Ok(self.lock().execute(
            "UPDATE routine_triggers SET secret_hash = ?2 WHERE trigger_id = ?1",
            params![id, secret_hash],
        )? > 0)
    }

    // ---- runs ----

    /// Start a run that no trigger owns (Run now), unless the routine has
    /// one that has not finished.
    pub fn run_start(&self, run: &NewRun) -> Result<Claimed> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let claimed = start_or_record(&tx, run)?;
        tx.commit()?;
        Ok(claimed)
    }

    /// Apply `p` to a run. Returns whether a row changed.
    pub fn run_update(&self, run_id: &str, p: &RunPatch) -> Result<bool> {
        let mut sets: Vec<&str> = Vec::new();
        let mut vals: Vec<Sql> = vec![Sql::Text(run_id.into())];
        let text = |s: &Option<String>| s.clone().map_or(Sql::Null, Sql::Text);
        let int = |n: Option<i64>| n.map_or(Sql::Null, Sql::Integer);
        let mut set = |col: &'static str, v: Sql| {
            sets.push(col);
            vals.push(v);
        };
        if let Some(s) = &p.status {
            set("status", Sql::Text(s.clone()));
        }
        if let Some(r) = &p.reason {
            set("reason", text(r));
        }
        if let Some(t) = p.ended_ms {
            set("ended_ms", Sql::Integer(t));
        }
        if let Some(s) = &p.session_id {
            set("session_id", Sql::Text(s.clone()));
        }
        if let Some(t) = &p.turn_id {
            set("turn_id", Sql::Text(t.clone()));
        }
        if let Some(w) = p.waiting_since_ms {
            set("waiting_since_ms", int(w));
        }
        if let Some(t) = &p.prompt {
            set("prompt", Sql::Text(t.clone()));
        }
        if let Some(u) = p.untrusted {
            set("untrusted", Sql::Integer(u.into()));
        }
        if let Some(w) = &p.warnings {
            set("warnings", Sql::Text(w.clone()));
        }
        if let Some(u) = &p.usage {
            set("usage", Sql::Text(u.clone()));
        }
        if let Some(a) = p.attempt {
            set("attempt", Sql::Integer(a));
        }
        if sets.is_empty() {
            return Ok(false);
        }
        let assignments: Vec<String> = sets
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{c} = ?{}", i + 2))
            .collect();
        let guard = if p.only_active {
            format!(" AND status IN {ACTIVE}")
        } else {
            String::new()
        };
        let sql = format!(
            "UPDATE routine_runs SET {} WHERE run_id = ?1{guard}",
            assignments.join(", ")
        );
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let changed = tx.execute(&sql, params_from_iter(vals))? > 0;
        // In the same transaction: no moment where the run is untrusted
        // and its chat is not.
        if changed && p.untrusted == Some(true) {
            mark_untrusted_chat(&tx, run_id)?;
        }
        tx.commit()?;
        Ok(changed)
    }

    /// End a run that has not finished as `status` (`skipped` folds into
    /// the newest run when that one was skipped for the same reason).
    /// Returns whether it was still running.
    pub fn run_finish(
        &self,
        run_id: &str,
        status: &str,
        reason: Option<&str>,
        now: i64,
    ) -> Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let Some(routine) = tx
            .query_row(
                &format!(
                    "SELECT routine_id FROM routine_runs WHERE run_id = ?1 AND status IN {ACTIVE}"
                ),
                params![run_id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        else {
            return Ok(false);
        };
        match foldable(&tx, &routine, status, reason, run_id)? {
            Some(into) => {
                tx.execute(
                    "UPDATE routine_runs SET repeats = repeats + 1, ended_ms = ?2 WHERE run_id = ?1",
                    params![into, now],
                )?;
                tx.execute(
                    "DELETE FROM routine_runs WHERE run_id = ?1",
                    params![run_id],
                )?;
            }
            None => {
                tx.execute(
                    "UPDATE routine_runs SET status = ?2, reason = ?3, ended_ms = ?4,
                         waiting_since_ms = NULL
                     WHERE run_id = ?1",
                    params![run_id, status, reason, now],
                )?;
            }
        }
        tx.commit()?;
        Ok(true)
    }

    pub fn run(&self, run_id: &str) -> Result<Option<RunRow>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                &format!("SELECT {RUN_COLS} FROM routine_runs r WHERE r.run_id = ?1"),
                params![run_id],
                run_row,
            )
            .optional()?)
    }

    /// Runs that have not finished, oldest first.
    pub fn active_runs(&self) -> Result<Vec<RunRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {RUN_COLS} FROM routine_runs r WHERE r.status IN {ACTIVE}
             ORDER BY r.started_ms, r.run_id"
        ))?;
        let rows = stmt.query_map([], run_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// A routine's runs, newest first: `limit` of them, after (older than)
    /// run `before` when given.
    pub fn runs_page(
        &self,
        routine: &str,
        before: Option<&str>,
        limit: usize,
    ) -> Result<Vec<RunRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {RUN_COLS} FROM routine_runs r
             WHERE r.routine_id = ?1
               AND (?2 IS NULL OR (r.started_ms, r.run_id) <
                    (SELECT b.started_ms, b.run_id FROM routine_runs b WHERE b.run_id = ?2))
             ORDER BY r.started_ms DESC, r.run_id DESC LIMIT ?3"
        ))?;
        let rows = stmt.query_map(params![routine, before, limit as i64], run_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// How many runs a routine has had (coalesced rows count once).
    pub fn run_count(&self, routine: &str) -> Result<i64> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM routine_runs WHERE routine_id = ?1",
            params![routine],
            |r| r.get(0),
        )?)
    }

    /// The run that started chat `sid`.
    pub fn run_by_session(&self, sid: SessionId) -> Result<Option<RunRow>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                &format!("SELECT {RUN_COLS} FROM routine_runs r WHERE r.session_id = ?1"),
                params![sid.to_string()],
                run_row,
            )
            .optional()?)
    }

    /// The run whose chat delegated to sub-agent session `child`.
    pub fn run_by_child_session(&self, child: SessionId) -> Result<Option<RunRow>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                &format!(
                    "SELECT {RUN_COLS} FROM routine_runs r
                     JOIN sessions s ON s.parent_id = r.session_id
                     WHERE s.session_id = ?1"
                ),
                params![child.to_string()],
                run_row,
            )
            .optional()?)
    }

    /// Approvals waiting in chat `sid` and in the sub-agent sessions it
    /// delegated to.
    pub fn pending_approvals_with_children(&self, sid: SessionId) -> Result<i64> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT COALESCE(SUM(pending_approvals), 0) FROM sessions
             WHERE session_id = ?1 OR parent_id = ?1",
            params![sid.to_string()],
            |r| r.get(0),
        )?)
    }

    /// The payload of chat `sid`'s first event of `kind`. A run's turn is
    /// the first turn of the chat it started, so its `turn_started` and
    /// `turn_ended` are the first ones.
    pub fn first_event(&self, sid: SessionId, kind: &str) -> Result<Option<serde_json::Value>> {
        let conn = self.lock();
        let payload: Option<String> = conn
            .query_row(
                "SELECT payload FROM events WHERE session_id = ?1 AND kind = ?2 ORDER BY seq LIMIT 1",
                params![sid.to_string(), kind],
                |r| r.get(0),
            )
            .optional()?;
        Ok(payload.map(|p| serde_json::from_str(&p)).transpose()?)
    }

    /// `(runs since `since` that started or tried to, runs pending or
    /// running now)` over every routine of `owner`: the daily and the
    /// concurrency caps.
    pub fn owner_run_counts(&self, owner: &str, since: i64) -> Result<(i64, i64)> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT
                 COALESCE(SUM(r.started_ms >= ?2 AND r.status NOT IN ('skipped','missed','blocked')), 0),
                 COALESCE(SUM(r.status IN ('pending','running')), 0)
             FROM routine_runs r JOIN routines o ON o.routine_id = r.routine_id
             WHERE o.owner_id = ?1",
            params![owner, since],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }

    /// The owner looked at a finished run (its chat or its attention row).
    /// Runs still going stay unseen: their result is still to come.
    pub fn mark_run_seen(&self, routine: &str, run_id: &str) -> Result<bool> {
        Ok(self.lock().execute(
            &format!(
                "UPDATE routine_runs SET seen = 1
                 WHERE run_id = ?1 AND routine_id = ?2 AND seen = 0 AND status IN {FINISHED}"
            ),
            params![run_id, routine],
        )? > 0)
    }

    /// The owner prompted in chat `sid`: mark its run continued. Returns
    /// whether the chat holds data from an external event (`chat_untrusted`),
    /// which stays true after its run is gone (its routine deleted).
    pub fn run_promote(&self, sid: SessionId) -> Result<bool> {
        let conn = self.lock();
        let id = sid.to_string();
        conn.execute(
            "UPDATE routine_runs SET promoted = 1 WHERE session_id = ?1 AND promoted = 0",
            params![id],
        )?;
        untrusted(&conn, &id)
    }

    /// Whether chat `sid` holds data from an external event: it is marked,
    /// or it is a sub-agent's chat whose parent is (the parent handed it a
    /// task written from that data, and the owner can prompt it directly).
    pub fn chat_untrusted(&self, sid: SessionId) -> Result<bool> {
        untrusted(&self.lock(), &sid.to_string())
    }

    /// Unseen finished runs per routine of `owner`.
    pub fn routine_unseen(&self, owner: Option<&str>) -> Result<HashMap<String, i64>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT r.routine_id, COUNT(*) FROM routine_runs r
             JOIN routines o ON o.routine_id = r.routine_id
             WHERE (?1 IS NULL OR o.owner_id = ?1) AND r.seen = 0 AND r.status IN {FINISHED}
             GROUP BY r.routine_id"
        ))?;
        let rows = stmt.query_map(params![owner], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The owner's routine results: per routine, its newest unseen finished
    /// run with the count of unseen ones, and a row for each paused
    /// routine. Skipped and missed runs never appear.
    pub fn routine_attention(&self, owner: &str) -> Result<Vec<AttentionRow>> {
        let conn = self.lock();
        let mut out = Vec::new();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT o.routine_id, o.name, o.workspace_id, o.spec, r.run_id, r.session_id,
                    r.status, r.reason, COALESCE(r.ended_ms, r.started_ms),
                    (SELECT COUNT(*) FROM routine_runs u WHERE u.routine_id = o.routine_id
                         AND u.seen = 0 AND u.status IN {FINISHED})
             FROM routines o JOIN routine_runs r ON r.run_id =
                 (SELECT u.run_id FROM routine_runs u WHERE u.routine_id = o.routine_id
                      AND u.seen = 0 AND u.status IN {FINISHED}
                  ORDER BY u.started_ms DESC, u.run_id DESC LIMIT 1)
             WHERE o.owner_id = ?1"
        ))?;
        let rows = stmt.query_map(params![owner], |r| {
            Ok(AttentionRow {
                routine_id: r.get(0)?,
                name: r.get(1)?,
                workspace_id: r.get(2)?,
                spec: r.get(3)?,
                paused_reason: None,
                run_id: r.get(4)?,
                session_id: r.get(5)?,
                status: r.get(6)?,
                reason: r.get(7)?,
                ts_ms: r.get(8)?,
                unseen: r.get(9)?,
            })
        })?;
        for row in rows {
            out.push(row?);
        }
        // A pause row names the routine's newest run, which says why.
        let mut stmt = conn.prepare_cached(
            "SELECT o.routine_id, o.name, o.workspace_id, o.spec, o.paused_reason, o.updated_ms,
                    r.run_id, r.session_id, r.status, r.reason
             FROM routines o LEFT JOIN routine_runs r ON r.run_id =
                 (SELECT u.run_id FROM routine_runs u WHERE u.routine_id = o.routine_id
                  ORDER BY u.started_ms DESC, u.run_id DESC LIMIT 1)
             WHERE o.owner_id = ?1 AND o.paused_reason IS NOT NULL",
        )?;
        let rows = stmt.query_map(params![owner], |r| {
            Ok(AttentionRow {
                routine_id: r.get(0)?,
                name: r.get(1)?,
                workspace_id: r.get(2)?,
                spec: r.get(3)?,
                paused_reason: r.get(4)?,
                ts_ms: r.get(5)?,
                run_id: r.get(6)?,
                session_id: r.get(7)?,
                status: r.get(8)?,
                reason: r.get(9)?,
                unseen: 0,
            })
        })?;
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// The routine runs behind `sids`, by chat; chats no run started are
    /// left out.
    pub fn routine_marks(&self, sids: &[SessionId]) -> Result<HashMap<SessionId, RoutineMark>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT r.routine_id, o.name, r.run_id, r.status, r.seen, r.promoted, r.untrusted
             FROM routine_runs r JOIN routines o ON o.routine_id = r.routine_id
             WHERE r.session_id = ?1",
        )?;
        let mut out = HashMap::new();
        for sid in sids {
            let mark = stmt
                .query_row(params![sid.to_string()], |r| {
                    Ok(RoutineMark {
                        routine_id: r.get(0)?,
                        name: r.get(1)?,
                        run_id: r.get(2)?,
                        status: r.get(3)?,
                        seen: r.get(4)?,
                        promoted: r.get(5)?,
                        untrusted: r.get(6)?,
                    })
                })
                .optional()?;
            if let Some(mark) = mark {
                out.insert(*sid, mark);
            }
        }
        Ok(out)
    }

    /// Forget finished runs beyond the newest `keep` of each routine and
    /// runs that started before `before`. A run whose chat still exists is
    /// kept: the chat's routine mark (and its untrusted flag) live on it.
    pub fn runs_purge(&self, keep: usize, before: i64) -> Result<usize> {
        let conn = self.lock();
        Ok(conn.execute(
            &format!(
                "DELETE FROM routine_runs WHERE status NOT IN {ACTIVE} AND session_id IS NULL
                   AND (started_ms < ?2 OR run_id IN (
                        SELECT run_id FROM (
                            SELECT run_id, ROW_NUMBER() OVER (
                                PARTITION BY routine_id ORDER BY started_ms DESC, run_id DESC) AS n
                            FROM routine_runs)
                        WHERE n > ?1))"
            ),
            params![keep as i64, before],
        )?)
    }

    /// Chats of runs to delete for retention, at most `limit`, oldest
    /// first: per routine, the run chats beyond the newest `keep` that the
    /// owner has seen and never continued. Chats of runs still going,
    /// unseen or continued are never among them.
    pub fn run_chats_beyond(&self, keep: usize, limit: usize) -> Result<Vec<SessionId>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT session_id FROM (
                 SELECT session_id, started_ms, run_id, ROW_NUMBER() OVER (
                     PARTITION BY routine_id ORDER BY started_ms DESC, run_id DESC) AS n
                 FROM routine_runs
                 WHERE session_id IS NOT NULL AND seen = 1 AND promoted = 0
                   AND status NOT IN {ACTIVE})
             WHERE n > ?1 ORDER BY started_ms, run_id LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![keep as i64, limit as i64], |r| {
            r.get::<_, String>(0)
        })?;
        let mut out = Vec::new();
        for sid in rows {
            // A run's chat id is always a session id; skip one that is not.
            if let Ok(sid) = sid?.parse() {
                out.push(sid);
            }
        }
        Ok(out)
    }

    /// The event of a routine's newest run that kept one (JSON).
    pub fn latest_event(&self, routine: &str) -> Result<Option<String>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                "SELECT event FROM routine_runs WHERE routine_id = ?1 AND event IS NOT NULL
                 ORDER BY started_ms DESC, run_id DESC LIMIT 1",
                params![routine],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Audit lines of routines and their hooks, oldest first, as
    /// `(kind, detail)`: tests check that none carries content.
    pub fn routine_audit_lines(&self) -> Result<Vec<(String, String)>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT kind, detail FROM audit
             WHERE kind LIKE 'routine.%' OR kind LIKE 'hook.%' ORDER BY rowid",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

/// Called by `Store::delete_session` inside its transaction: the run keeps
/// its status, times, trigger and usage, but no text of the deleted chat;
/// the chat's untrusted mark goes with it. Its sub-agents' chats are not
/// deleted with it, and they hold what it handed them, so they take the
/// mark over first.
pub(crate) fn on_session_deleted(tx: &rusqlite::Transaction, sid: &str) -> crate::Result<()> {
    tx.execute(
        "UPDATE routine_runs SET session_id = NULL, chat_deleted = 1, prompt = NULL,
             event = NULL, warnings = NULL
         WHERE session_id = ?1",
        params![sid],
    )?;
    tx.execute(
        "INSERT OR IGNORE INTO routine_untrusted_chats (session_id)
         SELECT session_id FROM sessions
         WHERE parent_id = ?1
           AND EXISTS(SELECT 1 FROM routine_untrusted_chats WHERE session_id = ?1)",
        params![sid],
    )?;
    tx.execute(
        "DELETE FROM routine_untrusted_chats WHERE session_id = ?1",
        params![sid],
    )?;
    Ok(())
}
