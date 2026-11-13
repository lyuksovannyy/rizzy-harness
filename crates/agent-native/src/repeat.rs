//! The repeated-call guard (T.1): a model that asks for the same call and
//! gets the same result again is told so, and after four identical results
//! with nothing changing in between the call is not run.
//!
//! - **The same call** is [`harness_tools::call_key`]: the tool, the
//!   session's folder and the input as canonical JSON (object keys sorted,
//!   nothing else changed; `write` and `edit` by the real path). One more
//!   space is another call: the guard stops a stuck model, not a determined
//!   one (`max_steps` is the bound for that).
//! - **The same result** is a digest of `ok` and the output as logged, less
//!   the harness's own tokens that differ on every run ([`stable`]: the
//!   kept output's random id, a stop's measured time). A poll
//!   whose answer changes (a running background command, a file that
//!   changed) is a new result and starts the count again, so it is never
//!   refused.
//! - **A change** starts every call's count again ([`RepeatGuard::restart`];
//!   each keeps its last result, so a call that only repeats itself after
//!   it is no change): a new result of a call made before in this turn (a
//!   poll whose answer moved, so the wait before the next poll is not
//!   counted on), the first successful run of a tool that changes things
//!   (an edit, then the test run again), a sub-agent that completed, and a
//!   background command's end told at the step boundary. Calls in between
//!   that change nothing do not break a count: it is not "in a row".
//! - **The window** is one turn: a new guard for every turn, every callback
//!   turn and every sub-agent's turn. A restart ends the turn.
//!
//! It is fed only what the loop logged and asked before approval and
//! dispatch, so a refused call is never offered to the person and never
//! starts. Nothing the model writes reads as a switch: the counts live
//! here, and its notes hold no text of the call or its result. Only digests
//! are kept, never an argument or an output. The result's digest is 64 bits
//! (std's SipHash): two different results that collide only count as one,
//! which can at worst refuse a call after four runs, never run one.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use harness_core::event::NOTE_LEAD;
use harness_tools::CallKey;

/// From this many identical results the result carries a warning.
pub const WARN_AT: u32 = 2;
/// The call that would be the this-many-th is not run.
pub const REFUSE_AT: u32 = 5;

/// What the person sees as the result of a refused call.
pub const REFUSED_OUTPUT: &str = "Not run: this exact call gave the same result several \
     times in this turn, with nothing changing in between (the harness's repeated-call guard).";

/// The result of a call after the refusal that ended the turn, in the same
/// message: not run, and the person is not asked about it.
pub const ENDED_OUTPUT: &str = "Not run: the turn was ended by the harness's repeated-call guard \
     at an earlier call of this message.";

/// Why the turn ended, for the person.
pub const STOP_ERROR: &str = "stopped: the model asked again for a call the repeated-call guard \
     had refused (the same call with the same result, again and again, in one turn)";

struct Streak {
    outcome: u64,
    /// Identical results since the last change, refused asks included.
    runs: u32,
}

/// The guard of one turn.
#[derive(Default)]
pub struct RepeatGuard {
    calls: HashMap<CallKey, Streak>,
    /// A refusal ended the turn: nothing more runs in it.
    ended: bool,
}

/// The guard's answer before a call runs.
#[derive(Debug, PartialEq)]
pub enum Admission {
    Run,
    /// Not run: a refusal earlier in this step ended the turn;
    /// [`ENDED_OUTPUT`] is its result.
    Ended,
    /// Not run: [`REFUSED_OUTPUT`] is its result, `note` the model's line;
    /// `stop` when it was refused before and the turn ends after this step.
    Refuse {
        note: String,
        stop: bool,
    },
}

impl RepeatGuard {
    /// Before approval and dispatch.
    pub fn admit(&mut self, key: CallKey) -> Admission {
        if self.ended {
            return Admission::Ended;
        }
        let Some(s) = self.calls.get_mut(&key) else {
            return Admission::Run;
        };
        if s.runs + 1 < REFUSE_AT {
            return Admission::Run;
        }
        s.runs += 1;
        let stop = s.runs > REFUSE_AT;
        self.ended = stop;
        let note = if stop {
            format!(
                "{NOTE_LEAD} This call and the calls after it in this message were not run, \
                 and the turn ends here: it was refused before and you asked for it again."
            )
        } else {
            format!(
                "{NOTE_LEAD} This call was not run: the same call gave the same result {} \
                 times in this turn, with nothing changing in between that the harness can \
                 see, so it would again. Try a different approach, or end your turn and tell the person what \
                 blocks you. Asking for it again ends the turn.",
                REFUSE_AT - 1
            )
        };
        Admission::Refuse { note, stop }
    }

    /// Something changed: every call's count starts again. Each keeps its
    /// last result, so a call that then only repeats it is judged against
    /// it (and is no progress itself).
    pub fn restart(&mut self) {
        for s in self.calls.values_mut() {
            s.runs = 0;
        }
    }

    /// After a call ran: its result as logged. Returns the model's warning
    /// line when this result repeats the last one.
    pub fn record(
        &mut self,
        key: CallKey,
        changes: bool,
        ok: bool,
        output: &str,
    ) -> Option<String> {
        if self.ended {
            return None;
        }
        let mut h = std::hash::DefaultHasher::new();
        (ok, stable(output).as_ref()).hash(&mut h);
        let outcome = h.finish();
        let (runs, change) = match self.calls.get(&key) {
            Some(s) if s.outcome == outcome => (s.runs + 1, false),
            // A new result of a call made before: something moved.
            Some(_) => (1, true),
            None => (1, changes && ok),
        };
        if change {
            self.restart();
        }
        self.calls.insert(key, Streak { outcome, runs });
        (runs >= WARN_AT).then(|| {
            format!(
                "{NOTE_LEAD} This exact call has now given the same result {runs} times in \
                 this turn, with nothing changing in between that the harness can see; asking \
                 again will not change it. If you are waiting for something outside to \
                 change, wait inside one command (a loop with a sleep and a deadline) or run \
                 it in the background. Call number {REFUSE_AT} of it will not be run."
            )
        })
    }
}

/// Tokens the harness itself writes into a result that differ on every
/// run of the same command (`harness_tools::bash`): the random id of the
/// kept full output after filtering, and a stop's measured time. Each is
/// masked from the lead to the end of its digits (or before the tail).
const VOLATILE_AFTER: [&str; 2] = ["tool_output id=", "its guard confirmed the end ("];
const VOLATILE_BEFORE: [&str; 1] = ["s after the stop was ordered"];

/// The result as the guard compares it: without [`VOLATILE_AFTER`] and
/// [`VOLATILE_BEFORE`]'s values. Only the harness's own tokens: a
/// program's changing text (a poll's "running for 30 s") is the result.
/// A program that prints the same words is masked the same; that can only
/// make two results of the same call alike, never run a refused one.
fn stable(output: &str) -> std::borrow::Cow<'_, str> {
    let volatile = VOLATILE_AFTER
        .iter()
        .chain(VOLATILE_BEFORE.iter())
        .any(|l| output.contains(l));
    if !volatile {
        return output.into();
    }
    let mut s = output.to_string();
    for lead in VOLATILE_AFTER {
        let mut out = String::with_capacity(s.len());
        let mut rest = s.as_str();
        while let Some(i) = rest.find(lead) {
            let (head, tail) = rest.split_at(i + lead.len());
            out.push_str(head);
            out.push('#');
            let n = tail.bytes().take_while(u8::is_ascii_alphanumeric).count();
            rest = &tail[n..];
        }
        out.push_str(rest);
        s = out;
    }
    for tail_lit in VOLATILE_BEFORE {
        let mut out = String::with_capacity(s.len());
        let mut rest = s.as_str();
        while let Some(i) = rest.find(tail_lit) {
            let head = &rest[..i];
            let n = head
                .bytes()
                .rev()
                .take_while(|b| b.is_ascii_digit() || *b == b'.')
                .count();
            out.push_str(&head[..head.len() - n]);
            out.push('#');
            out.push_str(tail_lit);
            rest = &rest[i + tail_lit.len()..];
        }
        out.push_str(rest);
        s = out;
    }
    s.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn key(input: serde_json::Value) -> CallKey {
        harness_tools::call_key(std::path::Path::new("/w"), "read", &input)
    }

    #[test]
    fn counts_identical_results_and_refuses_the_fifth() {
        let mut g = RepeatGuard::default();
        let k = key(json!({"path": "a"}));
        for n in 1..REFUSE_AT {
            assert_eq!(g.admit(k), Admission::Run, "call {n}");
            let note = g.record(k, false, true, "same");
            assert_eq!(note.is_some(), n >= WARN_AT, "call {n}");
        }
        assert!(matches!(g.admit(k), Admission::Refuse { stop: false, .. }));
        assert!(matches!(g.admit(k), Admission::Refuse { stop: true, .. }));
    }

    #[test]
    fn a_failed_change_is_no_progress_and_ok_is_part_of_the_result() {
        let mut g = RepeatGuard::default();
        let (a, c) = (key(json!({"path": "a"})), key(json!({"path": "c"})));
        for _ in 1..REFUSE_AT {
            g.record(a, false, true, "same");
        }
        g.record(c, true, false, "no such file");
        assert!(matches!(g.admit(a), Admission::Refuse { .. }));
        let b = key(json!({"path": "b"}));
        g.record(b, false, true, "x");
        assert!(g.record(b, false, false, "x").is_none(), "ok differs");
    }

    /// Review T1-R1: what the harness itself makes different on every run
    /// of the same command (the kept output's random id, a stop's measured
    /// time) is not part of the result; a program's own changing text is.
    #[test]
    fn the_harness_own_volatile_tokens_are_not_the_result() {
        let pairs = [
            (
                "error[E0308]: mismatched types\n[filtered: 1 of 2 lines not shown; full \
                 output: tool_output id=e7b5872348defe99]\n[exit code: 101]",
                "error[E0308]: mismatched types\n[filtered: 1 of 2 lines not shown; full \
                 output: tool_output id=247eae90ecb7cbcf]\n[exit code: 101]",
            ),
            (
                "[timed out after 5 s: the command and everything it started were killed; \
                 its guard confirmed the end (12 ms)]",
                "[timed out after 5 s: the command and everything it started were killed; \
                 its guard confirmed the end (347 ms)]",
            ),
            (
                "[timed out after 5 s and killed, but NOT ended: a process of this command \
                 is still alive 2.0s after the stop was ordered; the kernel cannot end it]",
                "[timed out after 5 s and killed, but NOT ended: a process of this command \
                 is still alive 12.3s after the stop was ordered; the kernel cannot end it]",
            ),
        ];
        for (i, (a, b)) in pairs.into_iter().enumerate() {
            let mut g = RepeatGuard::default();
            let k = key(json!({ "path": i }));
            assert!(g.record(k, true, false, a).is_none());
            assert!(g.record(k, true, false, b).is_some(), "pair {i}");
        }
        let mut g = RepeatGuard::default();
        let k = key(json!({"path": "poll"}));
        g.record(k, false, true, "bg-1 (running for 30 s)");
        assert!(
            g.record(k, false, true, "bg-1 (running for 60 s)")
                .is_none()
        );
    }

    /// Review T1-R2: a restart keeps each call's last result, so a call
    /// that only repeats itself after it is no progress.
    #[test]
    fn a_restart_keeps_the_last_results() {
        let mut g = RepeatGuard::default();
        let (a, b, x) = (
            key(json!({"path": "a"})),
            key(json!({"path": "b"})),
            key(json!({"path": "x"})),
        );
        for _ in 1..REFUSE_AT {
            g.record(a, true, true, "a");
            g.record(b, true, true, "b");
            g.record(x, false, false, "error");
        }
        assert!(matches!(g.admit(x), Admission::Refuse { .. }));
    }

    #[test]
    fn notes_hold_no_text_of_the_call_and_lead_as_the_harness() {
        let mut g = RepeatGuard::default();
        let k = key(json!({"path": "SECRET-ARG"}));
        let mut notes = vec![];
        for _ in 1..REFUSE_AT {
            notes.extend(g.record(k, false, true, "SECRET-OUT"));
        }
        for _ in 0..2 {
            if let Admission::Refuse { note, .. } = g.admit(k) {
                notes.push(note);
            }
        }
        assert_eq!(notes.len(), 5);
        for n in notes {
            assert!(n.starts_with(NOTE_LEAD) && !n.contains('\n'), "{n}");
            assert!(!n.contains("SECRET"), "{n}");
        }
    }
}
