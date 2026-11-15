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
//!   it is no change): a new successful result of a call made before in
//!   this turn (a poll whose answer moved, so the wait before the next poll
//!   is not counted on; a failure in new words restarts only its own
//!   count), the first successful run of a tool that changes things
//!   (an edit, then the test run again), a sub-agent that completed, and a
//!   background command's end told at the step boundary. Calls in between
//!   that change nothing do not break a count: it is not "in a row".
//! - **The window** is one turn: a new guard for every turn, every callback
//!   turn and every sub-agent's turn. A restart ends the turn.
//! - **The second rule: one tool failing the same way** whatever its input
//!   (live test 2026-11-15: 28 `bash` calls, each adding one flag to the
//!   same command, each failing with the same error; the first rule never
//!   fired, the turn burned 139k tokens). Per tool, a streak of failures
//!   whose results are identical (after [`stable`]); the third is warned
//!   ([`TOOL_WARN_AT`]), and after seven the next call to that tool is not
//!   run ([`TOOL_REFUSE_AT`]): that refusal is the strong notice, and it
//!   says the tool's next call will be run once. Other calls of the tool in
//!   the refusing message are refused too. The tool's next call in a later
//!   message (whenever it comes) **is run**, as a trial: the refusal is of
//!   the tool, not of one call, so a sound call (`cargo build --offline`
//!   after seven identical resolver errors) must get its chance. If the
//!   trial fails with the same output again, the turn ends after that step
//!   as the first rule ends it; any other result resets the rule. It cannot
//!   loop: each cycle costs seven identical failures, one refusal and one
//!   trial, a trial that repeats the failure ends the turn, and one that
//!   does not starts the count from one, so the next refusal needs seven
//!   new identical failures (with `max_steps` as the outer bound). Any
//!   other result of the tool (a success, a failure in other words) ends
//!   the streak, and so does a change ([`RepeatGuard::restart`]); calls of
//!   other tools in between do not. Only failures count: a poll that
//!   succeeds with the same answer is the first rule's (same call) or no
//!   loop at all. A call the person denied is neither counted nor a reset
//!   ([`RepeatGuard::record_denied`]): every denial reads the same, and
//!   seven "no"s to seven different commands are the person's answers, not
//!   a stuck model (the first rule still counts the same call denied
//!   again). The loop marks the denial on the outcome
//!   (`ToolOutcome::denied`), never from the text. The thresholds are Hermes's same-tool failure counts
//!   (warn at 3, halt at 8; `agent/tool_guardrails.py`, research round 2
//!   synth 01 §2), which also need no identical result; with one required
//!   here the rule is stricter, and it never refuses on argument churn
//!   alone, which is why OpenClaw keeps its rolling detector off.
//!
//! It is fed only what the loop logged and asked before approval and
//! dispatch, so a refused call is never offered to the person and never
//! starts. Nothing the model writes reads as a switch: the counts live
//! here, and its notes hold no text of the call or its result. Only digests
//! are kept, never an argument or an output. The result's digest is 64 bits
//! (std's SipHash): two different results that collide only count as one,
//! which can at worst refuse a call after four runs, never run one.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use harness_core::event::NOTE_LEAD;
use harness_tools::CallKey;

/// From this many identical results the result carries a warning.
pub const WARN_AT: u32 = 2;
/// The call that would be the this-many-th is not run.
pub const REFUSE_AT: u32 = 5;
/// The second rule: from this many identical failures of one tool in a row
/// (any input) the result carries a warning.
pub const TOOL_WARN_AT: u32 = 3;
/// The second rule: the call of that tool that would be the this-many-th
/// such failure is not run.
pub const TOOL_REFUSE_AT: u32 = 8;

/// What the person sees as the result of a refused call.
pub const REFUSED_OUTPUT: &str = "Not run: this exact call gave the same result several \
     times in this turn, with nothing changing in between (the harness's repeated-call guard).";

/// What the person sees as the result of a call the second rule refused.
pub const TOOL_REFUSED_OUTPUT: &str = "Not run: the calls to this tool before it failed with exactly \
     the same output several times in a row in this turn, whatever their input, with nothing \
     changing in between (the harness's repeated-call guard).";

/// The result of a call after the refusal that ended the turn, in the same
/// message: not run, and the person is not asked about it.
pub const ENDED_OUTPUT: &str = "Not run: the turn was ended by the harness's repeated-call guard \
     at an earlier call of this message.";

/// Why the turn ended, for the person.
pub const STOP_ERROR: &str = "stopped: the model asked again for a call the repeated-call guard \
     had refused (the same call with the same result, or one tool failing the same way, again \
     and again, in one turn)";

struct Streak {
    outcome: u64,
    /// Identical results since the last change.
    runs: u32,
    /// The step (assistant message) of its first refusal since the last
    /// change.
    refused: Option<u32>,
}

/// The second rule's state of one tool.
struct ToolStreak {
    outcome: u64,
    /// Identical failures in a row since the last change.
    runs: u32,
    /// The step (assistant message) of its refusal.
    refused: Option<u32>,
    /// The call after the refusal is running: if it fails the same way, the
    /// turn ends.
    trial: bool,
}

/// The guard of one turn.
#[derive(Default)]
pub struct RepeatGuard {
    calls: HashMap<CallKey, Streak>,
    /// The second rule: per tool (a digest of its name), its identical
    /// failures in a row.
    tools: HashMap<u64, ToolStreak>,
    /// The model's message whose calls are being admitted.
    step: u32,
    /// A refusal ended the turn: nothing more runs in it.
    ended: bool,
    /// Calls whose count was set back once because their result left
    /// what the model is sent ([`RepeatGuard::forgive`]).
    forgiven: HashSet<CallKey>,
}

/// The guard's answer before a call runs.
#[derive(Debug, PartialEq)]
pub enum Admission {
    Run,
    /// Not run: a refusal earlier in this step ended the turn;
    /// [`ENDED_OUTPUT`] is its result.
    Ended,
    /// Not run: `output` is its result ([`REFUSED_OUTPUT`] or
    /// [`TOOL_REFUSED_OUTPUT`]), `note` the model's line; `stop` when the
    /// model was sent its refusal and asked for it again, and the turn ends
    /// after this step.
    Refuse {
        output: &'static str,
        note: String,
        stop: bool,
    },
}

impl RepeatGuard {
    /// The model's next message: its calls are admitted from here on. A
    /// refusal made before this was sent to the model with the results.
    pub fn next_step(&mut self) {
        self.step += 1;
    }

    /// Before approval and dispatch: `key` is the call, `tool` its tool.
    pub fn admit(&mut self, key: CallKey, tool: &str) -> Admission {
        if self.ended {
            return Admission::Ended;
        }
        let step = self.step;
        // Only a refusal the model has been sent ends the turn: the same
        // call again in the message that was refused is refused again,
        // as the first time (review T1-D2).
        let refuse = |s: &mut Streak| {
            let stop = s.refused.is_some_and(|at| at < step);
            s.refused.get_or_insert(step);
            stop
        };
        if let Some(s) = self.calls.get_mut(&key)
            && s.runs + 1 >= REFUSE_AT
        {
            let stop = refuse(s);
            self.ended = stop;
            let note = if stop {
                stop_note()
            } else {
                format!(
                    "{NOTE_LEAD} This call was not run: the same call gave the same result {} \
                     times in this turn, with nothing changing in between that the harness can \
                     see, so it would again. Try a different approach, or end your turn and tell the person what \
                     blocks you. Asking for it again ends the turn.",
                    REFUSE_AT - 1
                )
            };
            return Admission::Refuse {
                output: REFUSED_OUTPUT,
                note,
                stop,
            };
        }
        if let Some(s) = self.tools.get_mut(&tool_digest(tool))
            && s.runs + 1 >= TOOL_REFUSE_AT
            && !s.trial
        {
            match s.refused {
                // The model was sent the refusal: this call is the trial.
                Some(at) if at < step => s.trial = true,
                _ => {
                    s.refused.get_or_insert(step);
                    return Admission::Refuse {
                        output: TOOL_REFUSED_OUTPUT,
                        note: format!(
                            "{NOTE_LEAD} This call was not run: the last {} calls to this tool \
                             failed with exactly the same output, whatever their input, with \
                             nothing changing in between that the harness can see. Changing the \
                             input this way does not change the result: read what the error says \
                             and try a different approach, or end your turn and tell the person \
                             what blocks you. Your next call of this tool, in a later message, \
                             is run once; if it fails with the same output again, the turn ends.",
                            TOOL_REFUSE_AT - 1
                        ),
                        stop: false,
                    };
                }
            }
        }
        Admission::Run
    }

    /// Whether one more call of `tool`, after `already` calls of it in the
    /// same parallel batch, may join the batch. A batch's calls are all
    /// admitted before any of them is recorded, so this keeps out every
    /// call whose admission could depend on an earlier call of the batch:
    /// the second rule near its refusal (identical failures of the calls
    /// before it would refuse it, run one after another), or a tool whose
    /// refusal was sent or whose trial runs (a failed trial ends the turn
    /// and the calls after it are not run). The loop also keeps one call
    /// key once per batch, so the first rule counts every repeat exactly
    /// as it does in order. What remains can differ from the order only by
    /// being stricter: a change an earlier call of the batch makes
    /// ([`Self::restart`]) does not reset a later one's count until the
    /// next step.
    pub fn batchable(&self, tool: &str, already: u32) -> bool {
        if self.ended {
            return false;
        }
        match self.tools.get(&tool_digest(tool)) {
            None => already + 1 < TOOL_REFUSE_AT,
            Some(s) => s.refused.is_none() && !s.trial && s.runs + already + 1 < TOOL_REFUSE_AT,
        }
    }

    /// Something changed: every call's count starts again. Each keeps its
    /// last result, so a call that then only repeats it is judged against
    /// it (and is no progress itself).
    pub fn restart(&mut self) {
        for s in self.calls.values_mut() {
            s.runs = 0;
            s.refused = None;
        }
        for s in self.tools.values_mut() {
            s.runs = 0;
            s.refused = None;
            s.trial = false;
        }
    }

    /// The call's result is no longer in what the model is sent (cleared
    /// or summarised, X.15): its next identical result is not counted as a
    /// repeat, since a model reads again what it can no longer see. Once
    /// per call per turn: shortening (which only size triggers) cannot set
    /// a loop's count back again and again, so a stuck model is refused
    /// after at most `REFUSE_AT - 1` more runs.
    pub fn forgive(&mut self, key: CallKey) {
        if self.forgiven.insert(key)
            && let Some(s) = self.calls.get_mut(&key)
        {
            s.runs = 0;
        }
    }

    /// A refusal or a failed trial ended the turn: the loop ends it after
    /// this step.
    pub fn ended(&self) -> bool {
        self.ended
    }

    /// After a call the person denied: the first rule counts it as any
    /// result (the same call denied again is refused the fifth time); the
    /// second rule leaves the tool's streak as it was, neither counted nor
    /// reset.
    pub fn record_denied(&mut self, key: CallKey, output: &str) -> Option<String> {
        if self.ended {
            return None;
        }
        let runs = self.record_call(key, false, false, outcome_of(false, output));
        (runs >= WARN_AT).then(|| same_call_warning(runs))
    }

    /// The first rule's count of `key` after a result: its identical
    /// results since the last change.
    fn record_call(&mut self, key: CallKey, changes: bool, ok: bool, outcome: u64) -> u32 {
        let (runs, change) = match self.calls.get(&key) {
            Some(s) if s.outcome == outcome => (s.runs + 1, false),
            // A new result of a call made before: something moved, if it
            // succeeded (a poll that answers). A failure in new words (a
            // test runner's run time) is no change: it restarts its own
            // count only, never another call's, so a denied call between
            // two runs of it is still counted (review T1-D1).
            Some(_) => (1, ok),
            None => (1, changes && ok),
        };
        if change {
            self.restart();
        }
        self.calls.insert(
            key,
            Streak {
                outcome,
                runs,
                refused: None,
            },
        );
        runs
    }

    /// After a call ran: its result as logged. Returns the model's warning
    /// line when this result repeats the last one.
    pub fn record(
        &mut self,
        key: CallKey,
        tool: &str,
        changes: bool,
        ok: bool,
        output: &str,
    ) -> Option<String> {
        if self.ended {
            return None;
        }
        let outcome = outcome_of(ok, output);
        let runs = self.record_call(key, changes, ok, outcome);
        // The second rule: this tool's identical failures in a row; any
        // other result of it ends the streak.
        let t = tool_digest(tool);
        let failed = if ok {
            self.tools.remove(&t);
            0
        } else {
            match self.tools.get_mut(&t) {
                Some(s) if s.outcome == outcome && s.trial => {
                    // The trial after the refusal failed the same way.
                    self.ended = true;
                    return Some(format!(
                        "{NOTE_LEAD} This call failed with exactly the same output as the \
                         calls to this tool before its refusal, so the turn ends here; the \
                         calls after it in this message are not run."
                    ));
                }
                Some(s) if s.outcome == outcome => {
                    s.runs += 1;
                    s.runs
                }
                _ => {
                    self.tools.insert(
                        t,
                        ToolStreak {
                            outcome,
                            runs: 1,
                            refused: None,
                            trial: false,
                        },
                    );
                    1
                }
            }
        };
        if runs >= WARN_AT {
            return Some(same_call_warning(runs));
        }
        (failed >= TOOL_WARN_AT).then(|| {
            format!(
                "{NOTE_LEAD} This call and the {} calls to the same tool before it in this turn \
                 all failed with exactly the same output, though their input differed; changing \
                 the input this way does not change the result. Read what the error says and \
                 try a different approach, or end your turn and tell the person what blocks \
                 you. After {} such failures in a row the next call to this tool is not run.",
                failed - 1,
                TOOL_REFUSE_AT - 1
            )
        })
    }
}

/// The digest of a result as the guard compares it.
fn outcome_of(ok: bool, output: &str) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    (ok, stable(output).as_ref()).hash(&mut h);
    h.finish()
}

/// The first rule's warning after `runs` identical results.
fn same_call_warning(runs: u32) -> String {
    format!(
        "{NOTE_LEAD} This exact call has now given the same result {runs} times in this turn, \
         with nothing changing in between that the harness can see; asking again will not \
         change it. If you are waiting for something outside to change, wait inside one \
         command (a loop with a sleep and a deadline) or run it in the background. Call number \
         {REFUSE_AT} of it will not be run."
    )
}

/// The note of a refusal that ends the turn.
fn stop_note() -> String {
    format!(
        "{NOTE_LEAD} This call and the calls after it in this message were not run, and the \
         turn ends here: it was refused before and you asked for it again."
    )
}

/// A tool's name as the second rule keys it: a digest, like every key here.
fn tool_digest(tool: &str) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    tool.hash(&mut h);
    h.finish()
}

/// Tokens the harness itself writes into a result that differ on every
/// run of the same command (`harness_tools::bash`): the random id of the
/// kept full output after filtering, and a stop's measured time. Each is
/// masked from the lead to the end of its digits (or before the tail).
/// The words are the tool's own constants, so they cannot drift apart.
const VOLATILE_AFTER: [&str; 2] = [
    harness_tools::KEPT_OUTPUT_LEAD,
    harness_tools::ENDED_MS_LEAD,
];
const VOLATILE_BEFORE: [&str; 1] = [harness_tools::STILL_ALIVE_TAIL];

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
            assert_eq!(g.admit(k, "read"), Admission::Run, "call {n}");
            let note = g.record(k, "read", false, true, "same");
            assert_eq!(note.is_some(), n >= WARN_AT, "call {n}");
        }
        assert!(matches!(
            g.admit(k, "read"),
            Admission::Refuse { stop: false, .. }
        ));
        assert!(
            matches!(g.admit(k, "read"), Admission::Refuse { stop: false, .. }),
            "the model was not sent the refusal yet"
        );
        g.next_step();
        assert!(matches!(
            g.admit(k, "read"),
            Admission::Refuse { stop: true, .. }
        ));
    }

    #[test]
    fn a_failed_change_is_no_progress_and_ok_is_part_of_the_result() {
        let mut g = RepeatGuard::default();
        let (a, c) = (key(json!({"path": "a"})), key(json!({"path": "c"})));
        for _ in 1..REFUSE_AT {
            g.record(a, "read", false, true, "same");
        }
        g.record(c, "read", true, false, "no such file");
        assert!(matches!(g.admit(a, "read"), Admission::Refuse { .. }));
        let b = key(json!({"path": "b"}));
        g.record(b, "read", false, true, "x");
        assert!(
            g.record(b, "read", false, false, "x").is_none(),
            "ok differs"
        );
    }

    /// Review T1-R1: what the harness itself makes different on every run
    /// of the same command (the kept output's random id, a stop's measured
    /// time) is not part of the result; a program's own changing text is.
    /// The texts are what `bash` writes (`tools/src/bash.rs`, through the
    /// same constants); the filtered one also comes from a real run in
    /// `repeat_tests::a_failing_build_the_filter_shortens_is_refused_the_fifth_time`.
    #[test]
    fn the_harness_own_volatile_tokens_are_not_the_result() {
        use harness_tools::{ENDED_MS_LEAD, KEPT_OUTPUT_LEAD, STILL_ALIVE_TAIL};
        let how = "timed out after 5s (state `timeout_secs`, up to 600 on this node, or run \
                   it with `background: true`)";
        let kept = |id: &str| {
            format!(
                "error[E0308]: mismatched types\n[filtered: 1 of 2 lines not shown; full \
                 output: {KEPT_OUTPUT_LEAD}{id}]\n[exit code: 101]"
            )
        };
        let ended = |ms: u32| {
            format!(
                "(no output)\n[{how}: the command and everything it started were killed; \
                 {ENDED_MS_LEAD}{ms} ms)]"
            )
        };
        let alive = |s: f64| {
            format!(
                "(no output)\n[{how} and killed, but NOT ended: a process of this command is \
                 still alive {s:.1}{STILL_ALIVE_TAIL}; the kernel cannot end it yet (it waits \
                 on I/O that does not return, as on a stalled network or FUSE file system). \
                 Its guard keeps killing it; it ends when that I/O returns]"
            )
        };
        let pairs = [
            (kept("e7b5872348defe99"), kept("247eae90ecb7cbcf")),
            (ended(12), ended(347)),
            (alive(2.0), alive(12.34)),
        ];
        for (i, (a, b)) in pairs.into_iter().enumerate() {
            let mut g = RepeatGuard::default();
            let k = key(json!({ "path": i }));
            assert_ne!(a, b);
            assert!(g.record(k, "read", true, false, &a).is_none());
            assert!(
                g.record(k, "read", true, false, &b).is_some(),
                "pair {i}: {b}"
            );
        }
        let mut g = RepeatGuard::default();
        let k = key(json!({"path": "poll"}));
        g.record(k, "read", false, true, "bg-1 (running for 30 s)");
        assert!(
            g.record(k, "read", false, true, "bg-1 (running for 60 s)")
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
            g.record(a, "read", true, true, "a");
            g.record(b, "read", true, true, "b");
            g.record(x, "read", false, false, "error");
        }
        assert!(matches!(g.admit(x, "read"), Admission::Refuse { .. }));
    }

    #[test]
    fn notes_hold_no_text_of_the_call_and_lead_as_the_harness() {
        let mut g = RepeatGuard::default();
        let k = key(json!({"path": "SECRET-ARG"}));
        let mut notes = vec![];
        for _ in 1..REFUSE_AT {
            notes.extend(g.record(k, "read", false, true, "SECRET-OUT"));
        }
        for _ in 0..2 {
            if let Admission::Refuse { note, .. } = g.admit(k, "read") {
                notes.push(note);
            }
            g.next_step();
        }
        assert_eq!(notes.len(), 5);
        for n in notes {
            assert!(n.starts_with(NOTE_LEAD) && !n.contains('\n'), "{n}");
            assert!(!n.contains("SECRET"), "{n}");
        }
    }

    /// The second rule alone: one tool, a new input every time, the same
    /// failure. Warned from the third, refused at the eighth; any other
    /// result of that tool, or a change, starts it again; a success is
    /// never counted.
    #[test]
    fn one_tool_failing_the_same_way_is_warned_then_refused() {
        let mut g = RepeatGuard::default();
        let k =
            |i: u32| harness_tools::call_key(std::path::Path::new("/w"), "bash", &json!({"c": i}));
        for i in 1..TOOL_REFUSE_AT {
            assert_eq!(g.admit(k(i), "bash"), Admission::Run, "call {i}");
            let note = g.record(k(i), "bash", false, false, "error: read-only");
            assert_eq!(note.is_some(), i >= TOOL_WARN_AT, "call {i}");
        }
        match g.admit(k(99), "bash") {
            Admission::Refuse { output, stop, .. } => {
                assert_eq!(output, TOOL_REFUSED_OUTPUT);
                assert!(!stop);
            }
            a => panic!("{a:?}"),
        }
        assert_eq!(g.admit(k(1), "read"), Admission::Run, "another tool runs");
        assert!(
            matches!(
                g.admit(k(98), "bash"),
                Admission::Refuse { stop: false, .. }
            ),
            "the same message: refused again"
        );
        // Review F1: the next call of the tool, in a later message, is the
        // trial and runs; failing the same way again ends the turn.
        g.next_step();
        assert_eq!(g.admit(k(100), "bash"), Admission::Run);
        assert!(
            g.record(k(100), "bash", false, false, "error: read-only")
                .is_some()
        );
        assert!(g.ended());
        assert_eq!(g.admit(k(101), "bash"), Admission::Ended);

        // A trial with another result resets the rule.
        let mut g = RepeatGuard::default();
        for i in 1..TOOL_REFUSE_AT {
            g.record(k(i), "bash", false, false, "error");
        }
        assert!(matches!(g.admit(k(60), "bash"), Admission::Refuse { .. }));
        g.next_step();
        assert_eq!(g.admit(k(61), "bash"), Admission::Run);
        assert!(g.record(k(61), "bash", false, true, "built").is_none());
        assert!(!g.ended());
        for i in 62..62 + TOOL_REFUSE_AT - 2 {
            assert_eq!(g.admit(k(i), "bash"), Admission::Run, "call {i}");
            g.record(k(i), "bash", false, false, "error");
        }
        assert_eq!(
            g.admit(k(90), "bash"),
            Admission::Run,
            "counting from one again"
        );

        // Review F2: denials are neither counted nor a reset.
        let mut g = RepeatGuard::default();
        let denied = "The user denied this tool call. Ask how to proceed or try something else.";
        for i in 1..3 * TOOL_REFUSE_AT {
            assert_eq!(g.admit(k(i), "bash"), Admission::Run, "denial {i}");
            g.record_denied(k(i), denied);
        }
        for i in 1..TOOL_REFUSE_AT {
            g.record(k(200 + i), "bash", false, false, "error");
            g.record_denied(k(300 + i), denied);
        }
        assert!(
            matches!(g.admit(k(400), "bash"), Admission::Refuse { .. }),
            "denials between the failures did not reset them"
        );

        // A success in between, or a change, starts the streak again.
        let mut g = RepeatGuard::default();
        for i in 1..TOOL_REFUSE_AT {
            g.record(k(i), "bash", false, false, "error");
        }
        g.record(k(50), "bash", false, true, "fine");
        assert_eq!(g.admit(k(51), "bash"), Admission::Run);
        let mut g = RepeatGuard::default();
        for i in 1..TOOL_REFUSE_AT {
            g.record(k(i), "bash", false, false, "error");
        }
        g.restart();
        assert_eq!(g.admit(k(51), "bash"), Admission::Run);

        // Identical successes through new inputs never count.
        let mut g = RepeatGuard::default();
        for i in 1..3 * TOOL_REFUSE_AT {
            assert!(g.record(k(i), "bash", false, true, "pending").is_none());
            assert_eq!(g.admit(k(i + 1), "bash"), Admission::Run);
        }
    }
}
