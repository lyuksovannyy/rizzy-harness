//! What hand-off notes read from the mirror (ADOPTION_PLAN X.16): the
//! chats that may be due one, and the part of a chat's log a note's fixed
//! part and `harness_core::handoff::due` read.

use harness_core::{RawEvent, SessionId};
use rusqlite::params;

use crate::{Result, Store, StoreError};

impl Store {
    /// Chats that may be due a hand-off note: last active between
    /// `from_ms` and `until_ms` (their node's clock), no turn running, not
    /// a chat-profile one, not incognito, not a sub-agent's, in a project
    /// (the only place a note is read: X.16), and whose
    /// newest event is not a note already. The reader still checks each
    /// with `harness_core::handoff::due` and its own admission.
    pub fn handoff_candidates(
        &self,
        from_ms: i64,
        until_ms: i64,
    ) -> Result<Vec<(SessionId, u64, String)>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT s.session_id, s.head_seq, s.node_id FROM sessions s
             WHERE s.updated_ms >= ?1 AND s.updated_ms <= ?2 AND s.running = 0
               AND s.incognito = 0 AND s.parent_id IS NULL
               AND COALESCE(s.profile, 'agentic') <> 'chat'
               AND EXISTS (SELECT 1 FROM session_projects sp WHERE sp.session_id = s.session_id)
               AND COALESCE((SELECT e.kind FROM events e WHERE e.session_id = s.session_id
                             ORDER BY e.seq DESC LIMIT 1), '') <> 'handoff_written'",
        )?;
        let rows = stmt.query_map(params![from_ms, until_ms], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for r in rows {
            let (id, head, node) = r?;
            out.push((
                id.parse()
                    .map_err(|_| StoreError::Corrupt(format!("session id {id}")))?,
                head.max(0) as u64,
                node,
            ));
        }
        Ok(out)
    }

    /// The events of `sid` a note reads (`harness_core::handoff::KINDS`),
    /// ascending: only the first `user_message` (the person's first words;
    /// later ones, attachments and all, are never read) and the ones a
    /// rewind went back to (the rule that counts a rewind needs them).
    /// A fork's log is its own events only (`harness_core::fork::own`): what
    /// it copied is its parent's, which has its own note.
    pub fn handoff_log(&self, sid: SessionId) -> Result<Vec<RawEvent>> {
        handoff_log_in(&self.lock(), sid)
    }
}

/// [`Store::handoff_log`] on a connection already held.
pub(crate) fn handoff_log_in(conn: &rusqlite::Connection, sid: SessionId) -> Result<Vec<RawEvent>> {
    // How much of the chat is a copy (0: none). The query below only needs
    // the number, to find the first message of the chat's OWN; which events
    // are copies is `harness_core::fork::own`'s to say.
    let through = crate::session_forks::through(conn, &sid.to_string())?;
    {
        let mut stmt = conn.prepare_cached(
            "SELECT seq, ts_ms, kind, payload FROM events WHERE session_id = ?1
               AND (kind IN ('turn_started', 'turn_ended', 'checkpoint', 'todo_updated',
                             'handoff_written', 'conversation_rewound',
                             'conversation_rewind_undone')
                    OR seq = (SELECT MIN(seq) FROM events
                              WHERE session_id = ?1 AND kind = 'user_message'
                                AND seq > ?2)
                    -- The message a rewind went back to: `rewind::rewound`
                    -- counts a rewind only when it finds that message.
                    OR (kind = 'user_message' AND seq IN (
                          SELECT json_extract(payload, '$.message_seq') FROM events
                          WHERE session_id = ?1 AND kind = 'conversation_rewound')))
             ORDER BY seq",
        )?;
        let rows = stmt.query_map(params![sid.to_string(), through as i64], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (seq, ts_ms, kind, payload) = row?;
            out.push(RawEvent {
                session_id: sid,
                seq: seq as u64,
                ts_ms,
                kind,
                payload: serde_json::from_str(&payload)?,
            });
        }
        Ok(harness_core::fork::own(&out, through).into_owned())
    }
}
