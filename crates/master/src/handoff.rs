//! Hand-off notes (ADOPTION_PLAN X.16, docs/handoff-notes.md): the
//! master's side. Every [`SWEEP`] the master looks for coding chats idle
//! at least `harness_core::handoff::IDLE_MS` whose log says a note is due
//! (`harness_core::handoff::due`), checks the owner as a callback turn's
//! admission does, and asks the chat's node to write it
//! (`session.handoff`) with the small model the admin chose. The node
//! decides whether it runs, which models it knows, and writes the event;
//! the master runs no model and keeps no state that matters: its position
//! is the log itself (the latest note covers the chat), so a master or a
//! node that restarts simply asks again at the next sweep. Chats it found
//! not due, or refused, are remembered by their head seq until they move.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use harness_core::SessionId;
use harness_core::savings::NoteWriter;

use crate::{CommandError, Master, lock};

/// How often the master looks for chats due a note.
pub const SWEEP: Duration = Duration::from_secs(60);
/// A chat last active longer ago than this is not asked any more (its node
/// was away all that time; the next chat reads the fixed part alone).
pub const MAX_AGE_MS: i64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Default)]
pub(crate) struct Handoffs {
    /// Chats looked at and settled (not due, refused, asked), by the head
    /// seq they had then: looked at again only once it moved.
    settled: Mutex<HashMap<SessionId, u64>>,
}

/// Start the sweep (the binary calls this once the master serves).
pub(crate) fn start(m: &Master) {
    let m = m.clone();
    tokio::spawn(async move {
        // A first look soon after the start: notes a down master missed.
        tokio::time::sleep(Duration::from_secs(5)).await;
        loop {
            if m.is_stopping() {
                return;
            }
            sweep(&m).await;
            tokio::time::sleep(SWEEP).await;
        }
    });
}

#[derive(Debug, PartialEq, Eq)]
enum Next {
    Settled,
    Retry,
}

/// One look at every chat that may be due a note; returns how many were
/// asked for one.
pub(crate) async fn sweep(m: &Master) -> usize {
    let savings = m.token_savings().unwrap_or_default();
    if savings.handoff_notes == NoteWriter::Off {
        return 0;
    }
    let now = harness_core::now_ms();
    let until = now - harness_core::handoff::IDLE_MS;
    let found = m
        .inner
        .mirror
        .call(move |s| s.handoff_candidates(now - MAX_AGE_MS, until))
        .await;
    let chats = match found {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "hand-off notes: could not read the idle chats");
            return 0;
        }
    };
    let mut asked = 0;
    for (sid, head) in chats {
        if m.is_stopping() {
            break;
        }
        if lock(&m.inner.handoffs.settled).get(&sid) == Some(&head) {
            continue;
        }
        let (next, sent) = check(m, sid, &savings).await;
        asked += usize::from(sent);
        if next == Next::Settled {
            lock(&m.inner.handoffs.settled).insert(sid, head);
        }
    }
    asked
}

/// The chat was deleted from the mirror.
pub(crate) fn forget(m: &Master, sid: SessionId) {
    lock(&m.inner.handoffs.settled).remove(&sid);
}

/// One chat: ask its node for a note when one is due and the owner may
/// have one. Returns what to do next time and whether it was asked.
async fn check(
    m: &Master,
    sid: SessionId,
    savings: &harness_core::savings::TokenSavings,
) -> (Next, bool) {
    let loaded = m
        .inner
        .mirror
        .call(move |s| {
            let Some(row) = s.session(sid)? else {
                return Ok(None);
            };
            Ok::<_, harness_store::StoreError>(Some((row, s.handoff_log(sid)?)))
        })
        .await;
    let (row, log) = match loaded {
        Ok(Some(x)) => x,
        Ok(None) => return (Next::Settled, false),
        Err(e) => {
            tracing::warn!(session = %sid, error = %e, "hand-off notes: could not read a chat");
            return (Next::Retry, false);
        }
    };
    if row.incognito || row.parent_id.is_some() || !harness_core::handoff::due(&log) {
        return (Next::Settled, false);
    }
    if !m.node_has(&row.node_id, harness_protocol::CAP_HANDOFF) {
        // An older node, or one away: asked again once it says hello.
        let away = !m.is_connected(&row.node_id);
        return (if away { Next::Retry } else { Next::Settled }, false);
    }
    // Who may have a model run for them unattended: exactly a callback
    // turn's admission (owner enabled, agentic access, not a routine's,
    // not an untrusted chat, not a sub-agent's).
    if let Err(why) = crate::callbacks::admission(m, &row).await {
        tracing::debug!(session = %sid, reason = %why, "hand-off notes: not asked");
        return (Next::Settled, false);
    }
    // The small model only when it is chosen and still configured; the
    // node falls back to the chat's own model either way.
    let small = match savings.handoff_notes {
        NoteWriter::SmallModel => savings
            .small_model
            .clone()
            .filter(|s| m.find_model(s).is_some()),
        NoteWriter::ChatModel | NoteWriter::Off => None,
    };
    let last_model = log.iter().rev().find_map(|e| match e.typed() {
        Ok(harness_core::event::EventKind::TurnEnded(t)) => t.usage.and_then(|u| u.model),
        _ => None,
    });
    let windows = small
        .iter()
        .chain(last_model.iter())
        .chain(std::iter::once(&row.model))
        .filter_map(|id| Some((id.clone(), m.find_model(id)?.context?)))
        .collect();
    let sent = m
        .send_command(&row.node_id, move |req_id| {
            harness_protocol::MasterToNode::Handoff {
                req_id,
                session_id: sid,
                small_model: small,
                windows,
            }
        })
        .await;
    match sent {
        Ok(()) => (Next::Settled, true),
        Err(CommandError::Rejected(e)) if e == harness_protocol::HANDOFF_BUSY => {
            (Next::Retry, false)
        }
        Err(CommandError::NodeOffline(_) | CommandError::Timeout | CommandError::NodeStopping) => {
            (Next::Retry, false)
        }
        Err(e) => {
            tracing::debug!(session = %sid, error = %e, "hand-off notes: the node wrote none");
            (Next::Settled, false)
        }
    }
}
