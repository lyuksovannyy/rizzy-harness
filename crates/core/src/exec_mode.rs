//! The execution mode a chat is in now, as its log says it (N.10a, N.10b):
//! `session_created.exec_mode`, then every `exec_mode_changed` after it,
//! the latest winning. The ONE rule: the node's session at load, the Rust
//! view, the master's mirror column and (in `web/src/fold.ts`) the browser's
//! fold all read it through here or copy it from here.
//!
//! The mode is state, not conversation. A rewind never moves it back (as it
//! never moves the approval mode back), a fork's copy carries the changes
//! up to the cut, and nothing here looks at which events a rewind hides.
//!
//! It fails closed. A mode that is written but not one this build knows
//! (a hostile or corrupted log, a node from the future) is
//! [`InForce::Unreadable`], never "the previous one": whoever acts on it
//! runs nothing, as for a `session_created` that names no mode.

use serde_json::Value;

use crate::RawEvent;
use crate::policy::ExecMode;

/// The event kind that changes the mode.
pub const CHANGED: &str = "exec_mode_changed";

/// What the log says the mode is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InForce {
    /// None recorded: a chat session (no commands), or a log with no
    /// `session_created` yet.
    #[default]
    Unset,
    Mode(ExecMode),
    /// Recorded, but not a mode this build knows.
    Unreadable,
}

impl InForce {
    /// The mode, when there is a readable one.
    pub fn mode(self) -> Option<ExecMode> {
        match self {
            InForce::Mode(m) => Some(m),
            InForce::Unset | InForce::Unreadable => None,
        }
    }
}

/// What the event `kind` with `payload` records as the chat's mode: `None`
/// for any other kind; `Some(None)` for a creation that names none; else
/// the text written, which may or may not be a mode (the master's mirror
/// keeps it as written, so a mode it cannot read stays unreadable there).
pub fn recorded(kind: &str, payload: &Value) -> Option<Option<String>> {
    let field = match kind {
        "session_created" => "exec_mode",
        CHANGED => "mode",
        _ => return None,
    };
    let text = match payload.get(field) {
        None | Some(Value::Null) if kind == "session_created" => None,
        // Anything else that is not text is "written, and not a mode".
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) | None => Some(String::new()),
    };
    Some(text)
}

/// `now` after `e`: the one step of the fold.
pub fn step(now: InForce, e: &RawEvent) -> InForce {
    match recorded(&e.kind, &e.payload) {
        None => now,
        Some(None) => InForce::Unset,
        Some(Some(t)) => ExecMode::parse(&t).map_or(InForce::Unreadable, InForce::Mode),
    }
}

/// The mode `events` (a chat's log, in order) put it in.
pub fn in_force<'a>(events: impl IntoIterator<Item = &'a RawEvent>) -> InForce {
    events.into_iter().fold(InForce::Unset, step)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionId;
    use serde_json::json;

    fn ev(seq: u64, kind: &str, payload: Value) -> RawEvent {
        RawEvent {
            session_id: SessionId::new(),
            seq,
            ts_ms: 0,
            kind: kind.into(),
            payload,
        }
    }

    fn created(mode: Value) -> RawEvent {
        ev(1, "session_created", json!({ "exec_mode": mode }))
    }

    fn changed(seq: u64, mode: Value) -> RawEvent {
        ev(seq, CHANGED, json!({ "mode": mode, "from": "sandbox" }))
    }

    #[test]
    fn the_latest_change_wins_over_the_creation() {
        let log = [
            created(json!("sandbox")),
            ev(2, "turn_started", json!({})),
            changed(3, json!("host")),
            changed(4, json!("sandbox")),
            changed(5, json!("host")),
        ];
        assert_eq!(in_force(&log[..1]), InForce::Mode(ExecMode::Sandbox));
        assert_eq!(in_force(&log[..3]), InForce::Mode(ExecMode::Host));
        assert_eq!(in_force(&log[..4]), InForce::Mode(ExecMode::Sandbox));
        assert_eq!(in_force(&log), InForce::Mode(ExecMode::Host));
    }

    #[test]
    fn a_chat_session_has_none() {
        let log = [ev(1, "session_created", json!({}))];
        assert_eq!(in_force(&log), InForce::Unset);
        assert_eq!(InForce::Unset.mode(), None);
    }

    /// A mode this build does not know, or one that is not even text, is
    /// never "the previous mode": it is unreadable, and stays so until a
    /// later change names one.
    #[test]
    fn an_unknown_mode_fails_closed() {
        for bad in [json!("quantum"), json!(7), json!(null), json!("")] {
            let log = [created(json!("host")), changed(2, bad.clone())];
            assert_eq!(in_force(&log), InForce::Unreadable, "{bad}");
            assert_eq!(in_force(&log).mode(), None);
        }
        let no_field = [created(json!("host")), ev(2, CHANGED, json!({}))];
        assert_eq!(in_force(&no_field), InForce::Unreadable);
        let unknown_creation = [created(json!("quantum"))];
        assert_eq!(in_force(&unknown_creation), InForce::Unreadable);
    }

    #[test]
    fn the_mirror_keeps_the_text_as_written() {
        assert_eq!(
            recorded(CHANGED, &json!({"mode": "host"})),
            Some(Some("host".into()))
        );
        assert_eq!(
            recorded("session_created", &json!({})),
            Some(None),
            "a creation that names none"
        );
        assert_eq!(recorded("turn_started", &json!({"mode": "host"})), None);
    }
}
