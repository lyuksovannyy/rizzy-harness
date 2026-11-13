//! Callback turns (T.8d step 3; docs/background-commands.md §4.4 and §13):
//! when a background command ends while its chat is idle, the master
//! resumes the agent once, within bounds. This module is the watcher, the
//! admission and what the owner sees of it afterwards; the node decides
//! whether the turn runs (`Session::callback` on the node).
//!
//! * **The watcher** is told of every event the mirror takes
//!   ([`on_applied`], called from `Master::apply`, so it never falls behind
//!   the broadcast). An end that may call back starts one check of its chat;
//!   ends that come while a check runs make it look once more, so several
//!   ends become one turn.
//! * **Its position is the log itself.** A check reads the mirror: the ends
//!   after the chat's last `turn_started` and last `callback_refused`, less
//!   those a refusal of the master's covers (its attention mark). Nothing
//!   else is kept, so a master that restarts reads the same thing again
//!   (`start` checks every chat updated in the last hour); a second send of
//!   the same callback is refused by the node, which finds the end covered
//!   by the first turn. No end is lost but by the 60-minute rule.
//! * **Admission** is a routine fire's (docs/routines.md), less the
//!   `routines` permission: the owner's account exists, is enabled and owes
//!   no password change; the agentic grant and permission in the chat's
//!   workspace, and the node, as for the owner's own prompt
//!   (`workspaces::agentic_blocked`); not a routine's chat, not an untrusted
//!   one, not a sub-agent's. A refusal starts nothing and leaves an
//!   attention item.
//! * **The turn** goes through `Master::prompt` with `PromptOpts.callback`
//!   (its third caller): the chat's own agent, no text, no context, no
//!   memory.
//! * **Afterwards** the owner's attention list holds one row per chat: its
//!   callback turns not seen yet, or why the agent was not resumed (a kv
//!   row per chat, `callback-attention:<sid>`, no table). The chat list
//!   says which chats have a background command running (`background` on
//!   each row, from the mirror's events, kept in memory here).

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::time::Duration;

use harness_core::event::{BackgroundEnded, EventKind};
use harness_core::{RawEvent, SessionId};
use harness_store::SessionRow;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{CommandError, Live, Master, PromptOpts, lock};

/// An end recorded longer ago than this when the master would resume its
/// chat starts no turn (the master was down, or the node away): the agent
/// learns of it at the person's next message.
pub const MAX_AGE_MS: i64 = 60 * 60 * 1000;
/// Ends this close together become one turn.
const SETTLE: Duration = Duration::from_millis(300);
/// How soon a chat whose node is away, busy or stopping is tried again.
const RETRY: Duration = Duration::from_secs(15);
/// The kv prefix of a chat's attention mark.
const KEY: &str = "callback-attention:";
/// The longest any command runs (`bash_timeout_max_s` at most 3600 s) and
/// a margin: a command started before this cannot be running any more.
const RUNNING_WINDOW_MS: i64 = 65 * 60 * 1000;

fn key(sid: SessionId) -> String {
    format!("{KEY}{sid}")
}

/// The kv key of the workspace a chat's newest background command started
/// in (as the mirror saw the chat then).
fn ws_key(sid: SessionId) -> String {
    format!("callback-workspace:{sid}")
}

/// Record where `sid`'s newest background command started.
async fn started_in(m: &Master, sid: SessionId) {
    let res = m
        .inner
        .mirror
        .call(move |s| {
            let k = ws_key(sid);
            match s.session(sid)?.and_then(|r| r.workspace_id) {
                Some(ws) => s.kv_set(&k, &ws),
                None => s.kv_delete(&k).map(|_| ()),
            }
        })
        .await;
    if let Err(e) = res {
        tracing::warn!(session = %sid, error = %e, "callbacks: could not record a start's workspace");
    }
}

#[derive(Default)]
pub(crate) struct Callbacks {
    chats: Mutex<HashMap<SessionId, Chat>>,
}

#[derive(Default)]
struct Chat {
    /// Background commands the mirror shows running: id to its deadline
    /// (node clock).
    running: BTreeMap<String, i64>,
    /// A check of this chat runs; `again`: an end came meanwhile.
    busy: bool,
    again: bool,
}

/// What the owner's attention list holds for a chat (kv, one per chat).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Mark {
    owner: Option<String>,
    /// `resumed` (callback turns ran) or `not_resumed`.
    kind: String,
    bg_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    /// The event it is about: the newest callback turn's `turn_started`,
    /// or the newest end that was not resumed (`seen` past it clears it).
    seq: u64,
    ts_ms: i64,
    /// Callback turns since the person's last turn.
    #[serde(default)]
    turns: u32,
}

/// The mirror took `e` (`Master::apply`, after the append).
pub(crate) fn on_applied(m: &Master, e: &RawEvent) {
    let sid = e.session_id;
    match e.typed() {
        Ok(EventKind::BackgroundStarted(b)) => {
            lock(&m.inner.callbacks.chats)
                .entry(sid)
                .or_default()
                .running
                .insert(b.bg_id, b.ends_by_ms);
            let _ = m.inner.live.send(Live::SessionsChanged);
            let m = m.clone();
            tokio::spawn(async move { started_in(&m, sid).await });
        }
        Ok(EventKind::BackgroundEnded(b)) => {
            if let Some(c) = lock(&m.inner.callbacks.chats).get_mut(&sid) {
                c.running.remove(&b.bg_id);
            }
            let _ = m.inner.live.send(Live::SessionsChanged);
            if b.calls_back() {
                trigger(m, sid);
            }
        }
        Ok(EventKind::TurnStarted(t)) => {
            let (m, seq, ts) = (m.clone(), e.seq, e.ts_ms);
            tokio::spawn(async move {
                match t.cause {
                    Some(c) => resumed(&m, sid, c.callback_ids().to_vec(), seq, ts).await,
                    // The person's own turn: they are here; what ran without
                    // them is in the chat above it.
                    None => {
                        let k = key(sid);
                        let _ = m.inner.mirror.call(move |s| s.kv_delete(&k)).await;
                    }
                }
            });
        }
        Ok(EventKind::CallbackRefused(r)) => {
            let (m, seq, ts) = (m.clone(), e.seq, e.ts_ms);
            tokio::spawn(async move {
                not_resumed(&m, sid, r.bg_ids, r.reason, seq, ts).await;
            });
        }
        _ => {}
    }
}

/// The chat was deleted from the mirror.
pub(crate) async fn forget(m: &Master, sid: SessionId) {
    lock(&m.inner.callbacks.chats).remove(&sid);
    let (k, w) = (key(sid), ws_key(sid));
    let _ = m
        .inner
        .mirror
        .call(move |s| {
            s.kv_delete(&k)?;
            s.kv_delete(&w)
        })
        .await;
}

/// Check `sid` now, or once more after the check that runs.
fn trigger(m: &Master, sid: SessionId) {
    {
        let mut chats = lock(&m.inner.callbacks.chats);
        let c = chats.entry(sid).or_default();
        if c.busy {
            c.again = true;
            return;
        }
        c.busy = true;
    }
    let m = m.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(SETTLE).await;
            if let Some(c) = lock(&m.inner.callbacks.chats).get_mut(&sid) {
                c.again = false;
            }
            let retry = !m.is_stopping() && check(&m, sid).await == Next::Retry;
            if retry {
                tokio::time::sleep(RETRY).await;
                continue;
            }
            let mut chats = lock(&m.inner.callbacks.chats);
            let Some(c) = chats.get_mut(&sid) else { return };
            if c.again && !m.is_stopping() {
                c.again = false;
                continue;
            }
            c.busy = false;
            return;
        }
    });
}

#[derive(Debug, PartialEq, Eq)]
enum Next {
    Done,
    Retry,
}

/// One look at `sid`: resume it for the ends that wait, refuse with the
/// reason, or try again later.
async fn check(m: &Master, sid: SessionId) -> Next {
    let loaded = m
        .inner
        .mirror
        .call(move |s| {
            let Some(row) = s.session(sid)? else {
                return Ok(None);
            };
            let ends = s.events_since_last_turn(sid, &["background_ended"])?;
            let mark = s.kv_get(&key(sid))?;
            Ok::<_, harness_store::StoreError>(Some((row, ends, mark)))
        })
        .await;
    let (row, ends, mark) = match loaded {
        // The chat is gone: nothing starts.
        Ok(None) => return Next::Done,
        Ok(Some(x)) => x,
        Err(e) => {
            tracing::warn!(session = %sid, error = %e, "callbacks: could not read a chat");
            return Next::Retry;
        }
    };
    // What a refusal of the master's already covered stays refused.
    let covered = mark
        .and_then(|v| serde_json::from_str::<Mark>(&v).ok())
        .filter(|k| k.kind == "not_resumed")
        .map_or(0, |k| k.seq);
    let waiting: Vec<(u64, i64, BackgroundEnded)> = ends
        .into_iter()
        .filter(|e| e.seq > covered)
        .filter_map(|e| match e.typed() {
            Ok(EventKind::BackgroundEnded(b)) if b.calls_back() => Some((e.seq, e.ts_ms, b)),
            _ => None,
        })
        .collect();
    let Some(newest) = waiting.iter().map(|w| w.0).max() else {
        return Next::Done;
    };
    let all: Vec<String> = waiting.iter().map(|w| w.2.bg_id.clone()).collect();
    let now = harness_core::now_ms();
    let fresh: Vec<String> = waiting
        .iter()
        .filter(|w| now - w.1 <= MAX_AGE_MS)
        .map(|w| w.2.bg_id.clone())
        .collect();
    let ts = waiting.iter().map(|w| w.1).max().unwrap_or(now);
    if fresh.is_empty() {
        let why = "it ended more than 60 minutes before the agent could be resumed".to_string();
        refuse(m, &row, all, why, newest, ts).await;
        return Next::Done;
    }
    if let Err(why) = admission(m, &row).await {
        refuse(m, &row, all, why, newest, ts).await;
        return Next::Done;
    }
    // A node that does not resume chats: nothing to say, the end waits for
    // the person as before step 3.
    if !m.node_has(&row.node_id, harness_protocol::CAP_CALLBACK) {
        return if m.is_connected(&row.node_id) {
            Next::Done
        } else {
            Next::Retry
        };
    }
    let sent = m
        .prompt(
            sid,
            String::new(),
            PromptOpts {
                callback: fresh.clone(),
                ..Default::default()
            },
        )
        .await;
    match sent {
        Ok(()) => Next::Done,
        Err(CommandError::Rejected(e)) if e.starts_with(harness_protocol::CALLBACK_NOTHING) => {
            Next::Done
        }
        // The node recorded `callback_refused`; its mark comes from there.
        Err(CommandError::Rejected(e)) if e.starts_with(harness_protocol::CALLBACK_REFUSED) => {
            Next::Done
        }
        Err(CommandError::NodeOffline(_) | CommandError::Timeout | CommandError::NodeStopping) => {
            Next::Retry
        }
        Err(CommandError::UnknownSession) => Next::Done,
        Err(e) => {
            refuse(m, &row, fresh, e.to_string(), newest, ts).await;
            Next::Done
        }
    }
}

/// Why the master will not resume `row` now (`Err`), as a routine fire
/// checks its owner (docs/routines.md, "Admission") without the `routines`
/// permission, and as the owner's own prompt passes `agentic_gate`.
async fn admission(m: &Master, row: &SessionRow) -> Result<(), String> {
    if row.parent_id.is_some() {
        return Err("a sub-agent's chat gets no callback turn".into());
    }
    let Some(owner) = row.owner_id.clone() else {
        return Err("the chat has no owner account".into());
    };
    let user = match m.inner.mirror.call(move |s| s.user_by_id(&owner)).await {
        Ok(Some(u)) => u,
        Ok(None) => return Err("the owner's account is gone".into()),
        Err(e) => {
            tracing::warn!(session = %row.session_id, error = %e, "callbacks: could not load an owner");
            return Err("the owner's access could not be checked".into());
        }
    };
    if user.disabled {
        return Err("the owner's account is disabled".into());
    }
    if user.must_change_password {
        return Err("the owner must change their password first".into());
    }
    let who = crate::auth::Principal::from_user(user);
    let sid = row.session_id;
    // An owner who left the chat's team workspace still reads the chat (it
    // falls back to their Personal one) and may go on in it themselves;
    // nothing goes on there without them. The workspace the chat's newest
    // background command started in is kept for this (`started_in`): a
    // chat that is not there any more is not resumed until its owner
    // starts something again (stricter than the owner's own prompt).
    let k = ws_key(sid);
    match m.inner.mirror.call(move |s| s.kv_get(&k)).await {
        Ok(Some(ws)) if row.workspace_id.as_deref() != Some(ws.as_str()) => {
            return Err(
                "the chat is no longer in the workspace its command started in (its owner left it)"
                    .into(),
            );
        }
        Ok(_) => {}
        Err(_) => return Err("the owner's access could not be checked".into()),
    }
    if let Some(why) = crate::workspaces::agentic_blocked(m, &who, row).await {
        return Err(why);
    }
    // Fails closed: a mark that cannot be read counts.
    let routine = m
        .inner
        .mirror
        .call(move |s| s.routine_marks(&[sid]))
        .await
        .map_or(true, |marks| !marks.is_empty());
    if routine {
        return Err("a routine's chat gets no callback turn".into());
    }
    // The untrusted mark as `routines::on_owner_prompt` reads it, without
    // marking a run's chat continued (nobody continued it).
    if crate::routines::chat_untrusted(m, sid).await {
        return Err(
            "this chat holds data from an external event, so nothing resumes it \
                    unattended"
                .into(),
        );
    }
    Ok(())
}

async fn write_mark(m: &Master, sid: SessionId, mark: Mark) {
    let k = key(sid);
    let v = serde_json::to_string(&mark).unwrap_or_default();
    if let Err(e) = m.inner.mirror.call(move |s| s.kv_set(&k, &v)).await {
        tracing::warn!(session = %sid, error = %e, "callbacks: could not record an attention mark");
    }
    let _ = m.inner.live.send(Live::SessionsChanged);
}

async fn read_mark(m: &Master, sid: SessionId) -> Option<Mark> {
    let k = key(sid);
    let v = m.inner.mirror.call(move |s| s.kv_get(&k)).await.ok()??;
    serde_json::from_str(&v).ok()
}

async fn owner_of(m: &Master, sid: SessionId) -> Option<String> {
    m.inner
        .mirror
        .call(move |s| s.session(sid))
        .await
        .ok()
        .flatten()
        .and_then(|r| r.owner_id)
}

/// A refusal of the master's: an attention mark, an audit line (ids and
/// the reason only), nothing on the node.
async fn refuse(m: &Master, row: &SessionRow, bg_ids: Vec<String>, why: String, seq: u64, ts: i64) {
    tracing::info!(session = %row.session_id, reason = %why, "callbacks: the agent was not resumed");
    let detail = json!({"session": row.session_id, "bg_ids": bg_ids, "reason": why}).to_string();
    let _ = m
        .inner
        .mirror
        .call(move |s| s.audit("callback.refused", &detail))
        .await;
    not_resumed(m, row.session_id, bg_ids, why, seq, ts).await;
}

async fn not_resumed(
    m: &Master,
    sid: SessionId,
    bg_ids: Vec<String>,
    why: String,
    seq: u64,
    ts: i64,
) {
    let turns = read_mark(m, sid).await.map_or(0, |k| k.turns);
    let owner = owner_of(m, sid).await;
    let mark = Mark {
        owner,
        kind: "not_resumed".into(),
        bg_ids,
        reason: Some(why),
        seq,
        ts_ms: ts,
        turns,
    };
    write_mark(m, sid, mark).await;
}

async fn resumed(m: &Master, sid: SessionId, bg_ids: Vec<String>, seq: u64, ts: i64) {
    let turns = read_mark(m, sid).await.map_or(0, |k| k.turns) + 1;
    let owner = owner_of(m, sid).await;
    let mark = Mark {
        owner,
        kind: "resumed".into(),
        bg_ids,
        reason: None,
        seq,
        ts_ms: ts,
        turns,
    };
    write_mark(m, sid, mark).await;
}

/// The attention rows of `owner`'s chats (`None`: the API token, every
/// chat) not seen by `viewer`: one per chat, in the shape of the store's
/// `Attention` with `reason` `resumed` or `not_resumed` and a `detail`.
pub(crate) async fn attention(
    m: &Master,
    owner: Option<String>,
    viewer: String,
) -> harness_store::Result<Vec<Value>> {
    m.inner
        .mirror
        .call(move |s| {
            let mut out = Vec::new();
            for (k, v) in s.kv_prefix(KEY)? {
                let Some(sid) = k
                    .strip_prefix(KEY)
                    .and_then(|x| x.parse::<SessionId>().ok())
                else {
                    continue;
                };
                let Ok(mark) = serde_json::from_str::<Mark>(&v) else {
                    continue;
                };
                if owner.is_some() && mark.owner != owner {
                    continue;
                }
                if s.seen_seq(&viewer, sid)? >= mark.seq {
                    continue;
                }
                let Some(row) = s.session(sid)? else { continue };
                let ids = mark.bg_ids.join(", ");
                let detail = match (mark.kind.as_str(), &mark.reason) {
                    ("resumed", _) if mark.turns > 1 => format!(
                        "{ids} ended: the agent was resumed without you ({} turns)",
                        mark.turns
                    ),
                    ("resumed", _) => format!("{ids} ended: the agent was resumed without you"),
                    (_, reason) => format!(
                        "{ids} ended; the agent was not resumed: {}",
                        reason.as_deref().unwrap_or("refused")
                    ),
                };
                out.push(json!({
                    "session_id": sid,
                    "title": row.title,
                    "reason": mark.kind,
                    "detail": detail,
                    "seq": mark.seq,
                    "parent_id": row.parent_id,
                    "updated_ms": mark.ts_ms,
                    "workspace_id": row.workspace_id,
                }));
            }
            out.sort_by_key(|v| std::cmp::Reverse(v["updated_ms"].as_i64().unwrap_or(0)));
            out.truncate(20);
            Ok(out)
        })
        .await
}

/// Give each chat row `background`: how many background commands the
/// mirror shows running there (0 when none, or once each one's deadline
/// has passed, whatever the log says: a node that is away cannot report).
pub(crate) fn annotate(m: &Master, rows: &mut [Value]) {
    let now = harness_core::now_ms();
    let chats = lock(&m.inner.callbacks.chats);
    for row in rows {
        let n = row
            .get("session_id")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<SessionId>().ok())
            .and_then(|sid| chats.get(&sid))
            .map_or(0, |c| c.running.values().filter(|&&by| by > now).count());
        row["background"] = json!(n);
    }
}

/// At the master's start: what runs, from the mirror, and one check of
/// every chat whose log moved in the last hour (their ends may still wait;
/// older ones are past the 60-minute rule).
pub(crate) fn start(m: &Master) {
    let m = m.clone();
    tokio::spawn(async move {
        let now = harness_core::now_ms();
        let since = now - RUNNING_WINDOW_MS;
        let loaded = m
            .inner
            .mirror
            .call(move |s| {
                let mut out = Vec::new();
                for sid in s.sessions_updated_since(since)? {
                    out.push((sid, s.background_events_since(sid, since)?));
                }
                Ok::<_, harness_store::StoreError>(out)
            })
            .await;
        let chats = match loaded {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %e, "callbacks: could not read the recent chats");
                return;
            }
        };
        for (sid, events) in chats {
            {
                let mut all = lock(&m.inner.callbacks.chats);
                let c = all.entry(sid).or_default();
                for e in &events {
                    match e.typed() {
                        Ok(EventKind::BackgroundStarted(b)) => {
                            c.running.insert(b.bg_id, b.ends_by_ms);
                        }
                        Ok(EventKind::BackgroundEnded(b)) => {
                            c.running.remove(&b.bg_id);
                        }
                        _ => {}
                    }
                }
            }
            // Only a chat whose log has an end in the window can have one
            // waiting (S3-3).
            if events.iter().any(|e| e.kind == "background_ended") {
                trigger(&m, sid);
            }
        }
    });
}
