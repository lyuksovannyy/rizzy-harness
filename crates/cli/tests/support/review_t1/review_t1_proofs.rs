//! T.1 review proofs (the repeated-call guard). Not part of the suite:
//! `run_rust_proofs.sh` copies this into crates/agent-native/tests/ for one
//! run and removes the copy again.
//!
//! Each test PASSES when the weakness it names is present, and prints what
//! the loop did. They drive the real loop (`run_turn`) with the real `bash`
//! tool, uncontained, in a temporary folder.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use harness_agent_native::provider::ChatMsg;
use harness_agent_native::{
    ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError,
};
use harness_core::backend::{AgentBackend, TurnCtx, TurnHost, TurnOutcome};
use harness_core::event::{EventKind, NOTE_LEAD, ToolCallRef, TurnEndReason};
use harness_core::policy::{Decision, ToolRisk};
use harness_tools::{ToolCtx, ToolOutcome};
use serde_json::{Value, json};
use tokio::sync::mpsc;

#[derive(Default)]
struct Host(Mutex<Vec<EventKind>>);

impl Host {
    /// Every finished call: (ok, output, the harness's note).
    fn finished(&self) -> Vec<(bool, String, Option<String>)> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                EventKind::ToolCallFinished(f) => Some((f.ok, f.output.clone(), f.note.clone())),
                _ => None,
            })
            .collect()
    }
}

#[async_trait::async_trait]
impl TurnHost for Host {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        self.0.lock().unwrap().push(kind);
        Ok(())
    }
    async fn approve(&self, _: &str, _: &str, _: &Value, _: ToolRisk) -> Decision {
        Decision::Allow
    }
}

/// One message per request: the calls of step `n`, then a plain answer.
struct Script {
    steps: Vec<Vec<(&'static str, Value)>>,
    asked: AtomicUsize,
    last: Mutex<Vec<ChatMsg>>,
}

#[async_trait::async_trait]
impl Provider for Script {
    fn name(&self) -> &'static str {
        "script"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        _: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        *self.last.lock().unwrap() = req.messages.clone();
        let n = self.asked.fetch_add(1, Ordering::SeqCst);
        let tool_calls = self
            .steps
            .get(n)
            .map(|calls| {
                calls
                    .iter()
                    .map(|(t, i)| ToolCallRef {
                        call_id: harness_core::new_id(),
                        tool: (*t).into(),
                        input: i.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Completion {
            text: "done".into(),
            tool_calls,
            usage: None,
            stop: Default::default(),
        })
    }
}

/// A read-only poll whose answer changes every time, as `bash_output` of a
/// running command does ("running for 12 s") or `read` of a growing log.
struct Poll(AtomicUsize);

#[async_trait::async_trait]
impl harness_tools::Tool for Poll {
    fn name(&self) -> &'static str {
        "poll"
    }
    fn description(&self) -> &'static str {
        "poll"
    }
    fn schema(&self) -> Value {
        json!({"type": "object"})
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }
    async fn run(&self, _: &ToolCtx, _: Value) -> ToolOutcome {
        let n = self.0.fetch_add(1, Ordering::SeqCst);
        ToolOutcome::ok(format!("bg-1 (running for {} s). 40 % built", 30 * (n + 1)))
    }
}

struct Ran {
    out: TurnOutcome,
    host: Host,
    script: Arc<Script>,
    dir: tempfile::TempDir,
}

async fn run(
    steps: Vec<Vec<(&'static str, Value)>>,
    savings: harness_core::savings::Savings,
) -> Ran {
    let dir = tempfile::tempdir().unwrap();
    let script = Arc::new(Script {
        steps,
        asked: AtomicUsize::new(0),
        last: Mutex::default(),
    });
    let mut tools = harness_tools::builtin();
    tools.push(Arc::new(Poll(AtomicUsize::new(0))));
    let b = NativeBackend::new(script.clone(), tools, NativeConfig::default());
    let host = Host::default();
    let mut extra: harness_core::backend::TurnExtra = Default::default();
    extra.savings = savings;
    let out = b
        .run_turn(TurnCtx {
            session_id: harness_core::SessionId::new(),
            turn_id: "t".into(),
            history: vec![],
            user_text: "go".into(),
            cwd: dir.path().to_path_buf(),
            model: "x".into(),
            agent: None,
            compare: vec![],
            parent_session: None,
            extra,
            cancel: Default::default(),
            host: &host,
        })
        .await;
    Ran {
        out,
        host,
        script,
        dir,
    }
}

fn bash(cmd: &str) -> (&'static str, Value) {
    ("bash", json!({"command": cmd}))
}

fn lines_of(dir: &std::path::Path, name: &str) -> usize {
    std::fs::read_to_string(dir.join(name))
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

fn show(r: &Ran) {
    for (i, (ok, out, note)) in r.host.finished().iter().enumerate() {
        println!(
            "  call {:>2}: ok={ok:<5} {:?}{}",
            i + 1,
            out.replace('\n', " / "),
            note.as_ref()
                .map(|n| format!("\n           NOTE {}", &n[..n.len().min(90)]))
                .unwrap_or_default()
        );
    }
    println!("  turn: {:?} {:?}", r.out.reason, r.out.error);
}

/// Control: the guard does stop the plain case with the real `bash`.
#[tokio::test]
async fn control_a_plain_failing_command_is_refused_the_fifth_time() {
    let x = bash("echo run >> runs; echo 'error: nope'; exit 1");
    let r = run(vec![vec![x]; 5], Default::default()).await;
    show(&r);
    assert_eq!(lines_of(r.dir.path(), "runs"), 4);
    assert!(r.host.finished()[4].1.contains("Not run"));
}

/// F1. With shell-output filtering on (the default of an agentic session,
/// `TokenSavings::default().for_turn(false)`), a failing command whose
/// output the filter shortens ("Compiling …", as every failing `cargo
/// build` or `cargo test` prints) gets a new random `tool_output id=` in
/// its result each time, so the guard never sees the same result twice: no
/// warning, no refusal, eight runs of eight.
#[tokio::test]
async fn f1_a_filtered_failing_build_is_never_the_same_result() {
    let savings = harness_core::savings::TokenSavings::default().for_turn(false);
    assert!(savings.filter_shell_output, "the default of an agentic turn");
    let x = bash(
        "echo run >> runs; echo '   Compiling app v0.1.0 (/w)'; \
         echo 'error[E0308]: mismatched types'; exit 101",
    );
    let r = run(vec![vec![x]; 8], savings).await;
    show(&r);
    assert_eq!(
        lines_of(r.dir.path(), "runs"),
        8,
        "the guard stopped it after all"
    );
    let fin = r.host.finished();
    assert!(fin.iter().all(|(_, _, note)| note.is_none()), "no warning");
    assert!(fin[0].1.contains("tool_output id="), "{}", fin[0].1);
    assert_ne!(fin[0].1, fin[1].1, "the two results differ only by the id");
    assert_eq!(r.out.reason, TurnEndReason::Completed);
}

/// F2. Two successful commands with unchanging results around a failing
/// one: each of the two is "a change with a new result" every time, because
/// the other one's reset has just forgotten it, so the count of the failing
/// command never passes 1. Ten rounds, thirty calls, nothing said.
#[tokio::test]
async fn f2_two_unchanging_commands_hide_the_loop_between_them() {
    let (a, b) = (bash("git status 2>&1 | head -1; true"), bash("ls"));
    let x = bash("echo run >> runs; echo 'error: nope'; exit 1");
    let mut steps = vec![];
    for _ in 0..10 {
        steps.extend([vec![a.clone()], vec![b.clone()], vec![x.clone()]]);
    }
    let r = run(steps, Default::default()).await;
    show(&r);
    assert_eq!(lines_of(r.dir.path(), "runs"), 10, "the guard stopped it");
    assert!(r.host.finished().iter().all(|(_, _, n)| n.is_none()));
    assert_eq!(r.out.reason, TurnEndReason::Completed);
}

/// F3. Wait, then look: `sleep` between polls whose answer changes every
/// time. The poll shows the world moving, but it is read-only, so the wait
/// is counted on: the fifth `sleep` is refused and the sixth ends the turn
/// with an error.
#[tokio::test]
async fn f3_a_wait_between_changing_polls_ends_the_turn() {
    let (wait, poll) = (bash("sleep 0.05"), ("poll", json!({"id": "bg-1"})));
    let mut steps = vec![];
    for _ in 0..8 {
        steps.extend([vec![wait.clone()], vec![poll.clone()]]);
    }
    let r = run(steps, Default::default()).await;
    show(&r);
    let fin = r.host.finished();
    let polls: Vec<&String> = fin
        .iter()
        .filter(|f| f.1.starts_with("bg-1"))
        .map(|f| &f.1)
        .collect();
    let distinct: std::collections::HashSet<_> = polls.iter().collect();
    assert_eq!(distinct.len(), polls.len(), "every poll's answer was new");
    assert_eq!(
        fin.iter().filter(|f| f.1.contains("Not run")).count(),
        2,
        "the fifth and the sixth wait"
    );
    assert_eq!(r.out.reason, TurnEndReason::Error);
    assert!(r.out.error.as_deref().unwrap().contains("repeated-call guard"));
}

/// F4. The note of the second refusal says "the turn ends here", and the
/// calls after it in the same message still run.
#[tokio::test]
async fn f4_calls_after_the_final_refusal_still_run() {
    let x = bash("echo 'error: nope'; exit 1");
    let mut steps = vec![vec![x.clone()]; 5];
    steps.push(vec![x.clone(), bash("echo ran > after.txt")]);
    let r = run(steps, Default::default()).await;
    show(&r);
    let fin = r.host.finished();
    let last_refusal = fin[5].2.clone().unwrap_or_default();
    assert!(last_refusal.starts_with(NOTE_LEAD) && last_refusal.contains("the turn ends here"));
    assert!(
        r.dir.path().join("after.txt").exists(),
        "the call after it did not run"
    );
    assert_eq!(r.out.reason, TurnEndReason::Error);
    let _ = &r.script;
}

/// F5. A turn the guard ended leaves its last note at the end of the log's
/// messages. When a background command of the chat then ends and starts a
/// callback turn, `open_callback` takes every trailing note into the new
/// turn's opening: the opening of the callback turn says "the turn ends
/// here", and the refused result no longer carries its note.
#[tokio::test]
async fn f5_the_guards_last_note_moves_into_a_callback_turns_opening() {
    use harness_agent_native::history_to_messages;
    use harness_agent_native::provider::place_notes;
    use harness_core::RawEvent;
    use harness_core::event::*;

    let x = bash("echo 'error: nope'; exit 1");
    let r = run(vec![vec![x]; 6], Default::default()).await;
    assert_eq!(r.out.reason, TurnEndReason::Error);
    let sid = harness_core::SessionId::new();
    let mut kinds = vec![EventKind::UserMessage(UserMessage {
        turn_id: "t".into(),
        text: "go".into(),
        attachments: vec![],
        reply: None,
        images: vec![],
    })];
    kinds.extend(r.host.0.lock().unwrap().iter().cloned());
    // The chat is idle; a background command it started earlier ends, and
    // the node starts a callback turn for it.
    kinds.push(EventKind::BackgroundEnded(BackgroundEnded {
        bg_id: "bg-1".into(),
        reason: BackgroundReason::Exited,
        exit_code: Some(0),
        signal: None,
        confirmed: BackgroundConfirmed::Ended,
        ran_ms: 90_000,
        output_bytes: 5,
        tail: "built".into(),
        tail_cut: false,
        delivered: false,
    }));
    kinds.push(EventKind::TurnStarted(TurnStarted {
        turn_id: "t2".into(),
        agent: None,
        cause: Some(TurnCause::Callback(vec!["bg-1".into()])),
    }));
    let log: Vec<RawEvent> = kinds
        .into_iter()
        .enumerate()
        .map(|(i, k)| {
            let (kind, payload) = k.into_parts();
            RawEvent {
                session_id: sid,
                seq: i as u64 + 1,
                ts_ms: 0,
                kind,
                payload,
            }
        })
        .collect();
    let sent = place_notes(history_to_messages(&log));
    let last_result = sent
        .iter()
        .rev()
        .find_map(|m| match m {
            ChatMsg::Tool { content, .. } => Some(content.clone()),
            _ => None,
        })
        .unwrap();
    let opening = sent.last().and_then(|m| m.user_text()).unwrap().to_string();
    println!("  last result of the ended turn: {last_result:?}");
    println!("  the callback turn's opening:\n{opening}");
    assert!(opening.contains("the turn ends here"), "{opening}");
    assert!(!last_result.contains(NOTE_LEAD), "{last_result}");
}
