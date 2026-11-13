//! T.1 delta re-check proofs (the repeated-call guard after the fixes for
//! T1-R1 to R6). Not part of the suite: `run_rust_proofs.sh` copies this
//! into crates/agent-native/tests/ for one run and removes the copy again.
//!
//! Each test PASSES when the weakness it names is present, and prints what
//! the loop did. They drive the real loop (`run_turn`) with the real `bash`
//! tool, uncontained, in a temporary folder under target/tmp.

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

/// Denies the tool `change` (and counts how often the person was asked for
/// it); allows everything else.
#[derive(Default)]
struct Host {
    events: Mutex<Vec<EventKind>>,
    asked_change: AtomicUsize,
}

impl Host {
    fn finished(&self) -> Vec<(bool, String, Option<String>)> {
        self.events
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
        self.events.lock().unwrap().push(kind);
        Ok(())
    }
    async fn approve(&self, _: &str, tool: &str, _: &Value, _: ToolRisk) -> Decision {
        if tool == "change" {
            self.asked_change.fetch_add(1, Ordering::SeqCst);
            return Decision::Deny;
        }
        Decision::Allow
    }
}

/// One message per request: the calls of step `n`, then a plain answer.
/// Keeps every request it was sent.
struct Script {
    steps: Vec<Vec<(&'static str, Value)>>,
    asked: AtomicUsize,
    seen: Mutex<Vec<Vec<ChatMsg>>>,
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
        self.seen.lock().unwrap().push(req.messages.clone());
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

/// A read-only tool with one unchanging answer; counts its runs.
struct Look(Arc<AtomicUsize>);

#[async_trait::async_trait]
impl harness_tools::Tool for Look {
    fn name(&self) -> &'static str {
        "look"
    }
    fn description(&self) -> &'static str {
        "look"
    }
    fn schema(&self) -> Value {
        json!({"type": "object"})
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }
    async fn run(&self, _: &ToolCtx, _: Value) -> ToolOutcome {
        self.0.fetch_add(1, Ordering::SeqCst);
        ToolOutcome::ok("fn main() {}")
    }
}

/// A read-only poll whose answer moves every time.
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
        ToolOutcome::ok(format!("bg-1 (running for {} s)", 30 * (n + 1)))
    }
}

/// A tool that changes something (the person is asked, and says no).
struct Change(Arc<AtomicUsize>);

#[async_trait::async_trait]
impl harness_tools::Tool for Change {
    fn name(&self) -> &'static str {
        "change"
    }
    fn description(&self) -> &'static str {
        "change"
    }
    fn schema(&self) -> Value {
        json!({"type": "object"})
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::Mutating
    }
    async fn run(&self, _: &ToolCtx, _: Value) -> ToolOutcome {
        self.0.fetch_add(1, Ordering::SeqCst);
        ToolOutcome::ok("changed")
    }
}

struct Ran {
    out: TurnOutcome,
    host: Host,
    script: Arc<Script>,
    looks: usize,
    changes: usize,
    dir: tempfile::TempDir,
}

async fn run(steps: Vec<Vec<(&'static str, Value)>>) -> Ran {
    let dir = tempfile::tempdir().unwrap();
    let script = Arc::new(Script {
        steps,
        asked: AtomicUsize::new(0),
        seen: Mutex::default(),
    });
    let (looks, changes) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let mut tools = harness_tools::builtin();
    tools.push(Arc::new(Look(looks.clone())));
    tools.push(Arc::new(Poll(AtomicUsize::new(0))));
    tools.push(Arc::new(Change(changes.clone())));
    let b = NativeBackend::new(script.clone(), tools, NativeConfig::default());
    let host = Host::default();
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
            extra: Default::default(),
            cancel: Default::default(),
            host: &host,
        })
        .await;
    Ran {
        out,
        host,
        script,
        looks: looks.load(Ordering::SeqCst),
        changes: changes.load(Ordering::SeqCst),
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
            &out.replace('\n', " / ")[..out.len().min(70)],
            note.as_ref()
                .map(|n| format!("\n           NOTE {}", &n[..n.len().min(100)]))
                .unwrap_or_default()
        );
    }
    println!("  turn: {:?} {:?}", r.out.reason, r.out.error);
}

/// D1 (opened by the fix for T1-R3). "A new result of a call made before"
/// is a change whatever the call and whether it failed. A failing test
/// runner that prints its own run time is therefore a change every run, and
/// starts every other count again: an unchanging read and a call the person
/// denies, both repeated each round, are never warned about or refused. The
/// person is asked for the same denied call eight times of eight. On
/// 22818fe a failed call never started a count again (`changes && ok`), so
/// the fifth identical denied call was not offered.
#[tokio::test]
async fn d1_a_failing_runner_that_prints_its_time_restarts_a_denied_approval() {
    let test = bash(
        "echo run >> runs; echo \"test result: FAILED. 1 failed; finished in 0.$(date +%N)s\"; exit 101",
    );
    let look = ("look", json!({"path": "src/main.rs"}));
    let change = ("change", json!({"path": "src/main.rs", "new": "x"}));
    let mut steps = vec![];
    for _ in 0..8 {
        steps.extend([vec![test.clone()], vec![look.clone()], vec![change.clone()]]);
    }
    let r = run(steps).await;
    show(&r);
    println!(
        "  failing test ran {} times, the read {} times, the person was asked {} times",
        lines_of(r.dir.path(), "runs"),
        r.looks,
        r.host.asked_change.load(Ordering::SeqCst)
    );
    assert_eq!(lines_of(r.dir.path(), "runs"), 8);
    assert_eq!(r.looks, 8, "the unchanging read was refused after all");
    assert_eq!(
        r.host.asked_change.load(Ordering::SeqCst),
        8,
        "the person was spared after all"
    );
    assert_eq!(r.changes, 0);
    assert!(
        r.host.finished().iter().all(|(_, _, n)| n.is_none()),
        "no warning on any of the 24 results"
    );
    assert_eq!(r.out.reason, TurnEndReason::Completed);
}

/// D1 control: the same loop with a failing command whose text does not
/// change is stopped (everything is refused the fifth time).
#[tokio::test]
async fn d1_control_with_an_unchanging_failure_the_person_is_asked_four_times() {
    let test = bash("echo run >> runs; echo 'test result: FAILED. 1 failed'; exit 101");
    let look = ("look", json!({"path": "src/main.rs"}));
    let change = ("change", json!({"path": "src/main.rs", "new": "x"}));
    let mut steps = vec![];
    for _ in 0..8 {
        steps.extend([vec![test.clone()], vec![look.clone()], vec![change.clone()]]);
    }
    let r = run(steps).await;
    show(&r);
    assert_eq!(r.host.asked_change.load(Ordering::SeqCst), 4);
    assert_eq!(lines_of(r.dir.path(), "runs"), 4);
    assert_eq!(r.out.reason, TurnEndReason::Error);
}

/// D2 (not new: in 22818fe too, the first review missed it). Two of the
/// same call in one message: the first is refused, the second "was refused
/// before and you asked for it again" and ends the turn, although the model
/// was never sent a refusal. Its last request holds warnings only.
#[tokio::test]
async fn d2_the_same_call_twice_in_one_message_ends_the_turn_on_an_unseen_refusal() {
    let a = ("look", json!({"path": "a"}));
    let steps = vec![
        vec![a.clone(), a.clone()],
        vec![a.clone(), a.clone()],
        vec![a.clone(), a.clone(), bash("echo ran > after.txt")],
    ];
    let r = run(steps).await;
    show(&r);
    let fin = r.host.finished();
    assert_eq!(r.looks, 4);
    assert_eq!(r.out.reason, TurnEndReason::Error);
    assert!(
        fin[5]
            .2
            .as_deref()
            .unwrap()
            .contains("it was refused before and you asked for it again"),
        "{fin:?}"
    );
    assert!(!r.dir.path().join("after.txt").exists());
    // What the model had been sent when it wrote that message.
    let seen = r.script.seen.lock().unwrap();
    assert_eq!(seen.len(), 3, "no request after the refusals");
    let told_refused = seen.last().unwrap().iter().any(|m| match m {
        ChatMsg::Tool { content, .. } => content
            .lines()
            .any(|l| l.starts_with(NOTE_LEAD) && l.contains("was not run")),
        _ => false,
    });
    assert!(!told_refused, "the model had seen a refusal");
}

/// D3 (a nit of the fix for T1-R3 (a)). In wait, poll, wait, poll the
/// second wait is warned ("nothing changing in between ... wait inside one
/// command"), because the first poll is not yet "a new result of a call made
/// before". From the third wait on nothing is said.
#[tokio::test]
async fn d3_the_second_wait_between_polls_is_warned_once() {
    let (wait, poll) = (bash("sleep 0.05"), ("poll", json!({"id": "bg-1"})));
    let mut steps = vec![];
    for _ in 0..6 {
        steps.extend([vec![wait.clone()], vec![poll.clone()]]);
    }
    let r = run(steps).await;
    show(&r);
    let fin = r.host.finished();
    let warned: Vec<usize> = fin
        .iter()
        .enumerate()
        .filter(|(_, f)| f.2.is_some())
        .map(|(i, _)| i + 1)
        .collect();
    assert_eq!(warned, vec![3], "calls with a note");
    assert_eq!(r.out.reason, TurnEndReason::Completed);
}
