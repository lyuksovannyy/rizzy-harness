//! What the owner sees of runs: the `routine` mark on chat rows (and which
//! run chats the chat list leaves out), the attention rows, the continued
//! flag, and the hourly retention of run chats and purge of old runs.

use std::collections::HashMap;

use harness_core::SessionId;
use harness_store::routines::{AttentionRow, RoutineMark};
use serde_json::{Value, json};

use super::spec::{Notify, RoutineSpec};
use super::{PromptFlags, config, handle};
use crate::Master;
use crate::auth::Principal;

/// Runs older than this are forgotten (once their chat is gone).
const KEEP_RUNS_DAYS: i64 = 90;
/// Attention rows sent, newest first.
const ATTENTION_ROWS: usize = 20;
/// Run chats retention deletes per hour, server-wide: a backlog drains over
/// hours instead of stalling the maintenance loop.
const RETENTION_PER_HOUR: usize = 100;

/// The marks of the chats in `rows` (their `session_id`).
async fn marks(
    m: &Master,
    rows: &[Value],
) -> harness_store::Result<HashMap<SessionId, RoutineMark>> {
    let sids: Vec<SessionId> = rows
        .iter()
        .filter_map(|r| r.get("session_id")?.as_str()?.parse().ok())
        .collect();
    if sids.is_empty() {
        return Ok(HashMap::new());
    }
    m.inner.mirror.call(move |s| s.routine_marks(&sids)).await
}

fn mark_of(marks: &HashMap<SessionId, RoutineMark>, row: &Value) -> Option<RoutineMark> {
    let sid: SessionId = row.get("session_id")?.as_str()?.parse().ok()?;
    marks.get(&sid).cloned()
}

/// A run chat stays in the chat list while its run is going or waiting,
/// while its result is unseen, and once the owner continued it.
fn listed(mark: &RoutineMark) -> bool {
    !mark.seen || mark.promoted || matches!(mark.status.as_str(), "pending" | "running" | "waiting")
}

// A `Vec`, not a slice: the filter removes rows.
#[allow(clippy::ptr_arg)]
pub(super) async fn annotate(
    m: &Master,
    rows: &mut Vec<Value>,
    include_runs: bool,
) -> harness_store::Result<()> {
    let marks = marks(m, rows).await?;
    rows.retain_mut(|row| {
        let mark = mark_of(&marks, row);
        let keep = include_runs || mark.as_ref().is_none_or(listed);
        row["routine"] = json!(mark);
        keep
    });
    Ok(())
}

pub(super) async fn annotate_one(m: &Master, row: &mut Value) -> harness_store::Result<()> {
    let marks = marks(m, std::slice::from_ref(row)).await?;
    row["routine"] = json!(mark_of(&marks, row));
    Ok(())
}

pub(super) async fn on_owner_prompt(m: &Master, sid: SessionId) -> PromptFlags {
    match m.inner.mirror.call(move |s| s.run_promote(sid)).await {
        Ok(untrusted) => PromptFlags { untrusted },
        Err(e) => {
            // Fail closed: a chat that may hold event data stays untrusted.
            tracing::warn!(session = %sid, error = %e, "routines: could not read a run chat's flags");
            PromptFlags { untrusted: true }
        }
    }
}

fn notify(spec: &str) -> Notify {
    RoutineSpec::read(spec).unwrap_or_default().notify
}

/// Why a routine is paused, for its attention row.
fn pause_reason(row: &AttentionRow) -> Option<String> {
    match row.paused_reason.as_deref() {
        Some("failures") => Some("its runs failed several times in a row".into()),
        Some("cap") => Some("the daily limit of routine runs was reached".into()),
        // Blocked: the newest run says why.
        _ => row.reason.clone(),
    }
}

fn attention_json(row: &AttentionRow) -> Value {
    let (kind, status, reason) = match &row.paused_reason {
        Some(_) => ("paused", "paused".to_string(), pause_reason(row)),
        None => {
            let status = row.status.clone().unwrap_or_default();
            let kind = if status == "completed" {
                "result"
            } else {
                "failed"
            };
            (kind, status, row.reason.clone())
        }
    };
    json!({
        "kind": kind,
        "routine_id": row.routine_id,
        "name": row.name,
        "workspace_id": row.workspace_id,
        "run_id": row.run_id,
        "session_id": row.session_id,
        "status": status,
        "reason": reason,
        "unseen": row.unseen,
        "notify": notify(&row.spec),
        "ts_ms": row.ts_ms,
    })
}

/// The caller's routine results across all their workspaces, newest 20.
/// The API token owns no routines.
pub(super) async fn attention(m: &Master, who: &Principal) -> harness_store::Result<Value> {
    let Some(uid) = who.user_id.clone() else {
        return Ok(json!([]));
    };
    let mut rows = m
        .inner
        .mirror
        .call(move |s| s.routine_attention(&uid))
        .await?;
    rows.sort_by_key(|r| std::cmp::Reverse(r.ts_ms));
    rows.truncate(ATTENTION_ROWS);
    Ok(Value::Array(rows.iter().map(attention_json).collect()))
}

/// Retention first (its deleted chats free their runs for the purge), then
/// old runs, expired `Idempotency-Key`s and idle webhook throttles.
pub(super) async fn hourly(m: &Master) {
    let now = handle(m).map_or_else(harness_core::now_ms, |h| h.clock.now());
    let cfg = config(m);
    delete_old_run_chats(m, cfg.keep_run_chats as usize).await;
    let keep = cfg.keep_runs as usize;
    let before = now - KEEP_RUNS_DAYS * 24 * 3600 * 1000;
    match m
        .inner
        .mirror
        .call(move |s| {
            Ok::<_, harness_store::StoreError>((s.runs_purge(keep, before)?, s.dedupe_purge(now)?))
        })
        .await
    {
        Ok((0, _)) => {}
        Ok((n, _)) => tracing::info!(runs = n, "routines: forgot old runs"),
        Err(e) => tracing::warn!(error = %e, "routines: could not purge old runs"),
    }
    if let Some(h) = handle(m) {
        crate::lock(&h.hooks).prune(cfg.hook_rate_per_minute, now);
    }
}

/// Per routine, keep the newest `keep` run chats the owner has seen and not
/// continued, and delete older ones like the owner would (their runs keep
/// status, times and usage; the store hook clears their text).
async fn delete_old_run_chats(m: &Master, keep: usize) {
    let old = m
        .inner
        .mirror
        .call(move |s| s.run_chats_beyond(keep, RETENTION_PER_HOUR))
        .await;
    let old = match old {
        Ok(old) => old,
        Err(e) => {
            tracing::warn!(error = %e, "routines: could not list old run chats");
            return;
        }
    };
    if old.is_empty() {
        return;
    }
    let mut deleted = 0;
    for sid in old {
        let res = match m.delete_session(sid).await {
            // Gone from the mirror already: clear the run's side of it, or
            // the chat would be listed again every hour.
            Err(crate::CommandError::UnknownSession) => m
                .inner
                .mirror
                .call(move |s| s.delete_session(sid, true).map(|_| ()))
                .await
                .map_err(|e| e.to_string()),
            other => other.map_err(|e| e.to_string()),
        };
        match res {
            Ok(()) => deleted += 1,
            Err(e) => {
                tracing::warn!(session = %sid, error = %e, "routines: could not delete an old run chat")
            }
        }
    }
    tracing::info!(chats = deleted, "routines: deleted old run chats");
}
