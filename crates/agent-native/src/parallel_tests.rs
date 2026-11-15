//! Read-only calls of one step run at the same time (X.19): which calls
//! join a batch, that the log and the next request keep call order
//! whichever call finishes first, and that every guard of a call still
//! sees it once, in call order.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use harness_core::backend::{TurnCtx, TurnHost};
use harness_core::event::*;
use harness_core::policy::{Decision, ToolRisk};
use harness_core::{RawEvent, SessionId};
use harness_tools::{ToolCtx, ToolOutcome};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::provider::*;
use crate::*;

struct Host {
    events: Mutex<Vec<EventKind>>,
    /// Read-only calls run with no card (the chat's mode is not `ask_all`).
    unasked: bool,
    /// This tool is in the node's `denied_tools`.
    denied: Option<&'static str>,
    /// The tools the host was asked to approve, in order.
    asked: Mutex<Vec<String>>,
}

impl Host {
    fn new() -> Self {
        Self {
            events: Mutex::default(),
            unasked: true,
            denied: None,
            asked: Mutex::default(),
        }
    }

    /// The tool events, as `S:<call>` and `F:<call>`, in log order.
    fn order(&self) -> Vec<String> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                EventKind::ToolCallStarted(s) => Some(format!("S:{}", s.call_id)),
                EventKind::ToolCallFinished(f) => Some(format!("F:{}", f.call_id)),
                _ => None,
            })
            .collect()
    }

    fn finished(&self) -> Vec<(String, bool, String)> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                EventKind::ToolCallFinished(f) => Some((f.call_id.clone(), f.ok, f.output.clone())),
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
    async fn approve(&self, _c: &str, tool: &str, _i: &Value, r: ToolRisk) -> Decision {
        self.asked.lock().unwrap().push(tool.into());
        let _ = r;
        if self.denied == Some(tool) {
            Decision::Deny
        } else {
            Decision::Allow
        }
    }
    fn tool_denied(&self, tool: &str) -> bool {
        self.denied == Some(tool)
    }
    fn runs_unasked(&self, tool: &str, risk: ToolRisk) -> bool {
        self.unasked && self.denied != Some(tool) && risk == ToolRisk::ReadOnly
    }
}

/// Sends the calls of step `n` in its `n`-th message, then answers.
struct Rec {
    steps: Vec<Vec<(&'static str, Value)>>,
    seen: Mutex<Vec<ChatRequest>>,
}

#[async_trait::async_trait]
impl Provider for Rec {
    fn name(&self) -> &'static str {
        "rec"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        _deltas: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        self.seen.lock().unwrap().push(req.clone());
        let since = req
            .messages
            .iter()
            .rposition(|m| m.user_text().is_some())
            .map_or(0, |i| i + 1);
        let step = req.messages[since..]
            .iter()
            .filter(|m| matches!(m, ChatMsg::Assistant { .. }))
            .count();
        let tool_calls = self
            .steps
            .get(step)
            .map(|s| {
                s.iter()
                    .map(|(t, i)| ToolCallRef {
                        call_id: i["id"]
                            .as_str()
                            .map_or_else(harness_core::new_id, String::from),
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

/// What the stand-in tools saw: how many ran at once, at most, and when
/// each started and ended.
#[derive(Default)]
struct Probe {
    running: AtomicUsize,
    most: AtomicUsize,
    runs: AtomicUsize,
    spans: Mutex<Vec<(String, Instant, Instant)>>,
}

impl Probe {
    fn span(&self, id: &str) -> (Instant, Instant) {
        let s = self.spans.lock().unwrap();
        let (_, a, b) = s.iter().find(|(i, ..)| i == id).expect("ran");
        (*a, *b)
    }
}

/// A read-only, parallel-safe tool: sleeps `ms`, then answers its `id`;
/// `fail` fails it with `fail`'s text, `panic` panics, `wait` waits for
/// the cancel. `parallel` false: the same tool, not declared safe.
struct Slow {
    name: &'static str,
    parallel: bool,
    risk: ToolRisk,
    probe: Arc<Probe>,
}

#[async_trait::async_trait]
impl harness_tools::Tool for Slow {
    fn name(&self) -> &'static str {
        self.name
    }
    fn description(&self) -> &'static str {
        "slow"
    }
    fn schema(&self) -> Value {
        json!({"type": "object"})
    }
    fn risk(&self) -> ToolRisk {
        self.risk
    }
    fn parallel_safe(&self) -> bool {
        self.parallel
    }
    async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome {
        let p = &self.probe;
        let now = p.running.fetch_add(1, Ordering::SeqCst) + 1;
        p.most.fetch_max(now, Ordering::SeqCst);
        p.runs.fetch_add(1, Ordering::SeqCst);
        let start = Instant::now();
        let ms = input["ms"].as_u64().unwrap_or(0);
        if input["wait"] == json!(true) {
            ctx.cancel.cancelled().await;
        } else {
            tokio::time::sleep(Duration::from_millis(ms)).await;
        }
        p.running.fetch_sub(1, Ordering::SeqCst);
        let id = input["id"].as_str().unwrap_or("?").to_string();
        p.spans
            .lock()
            .unwrap()
            .push((id.clone(), start, Instant::now()));
        if input["panic"] == json!(true) {
            panic!("the stand-in tool panics");
        }
        if let Some(f) = input["fail"].as_str() {
            return ToolOutcome::err(f.to_string());
        }
        if ctx.cancel.is_cancelled() {
            return ToolOutcome::err(format!("{id} cancelled"));
        }
        ToolOutcome::ok(format!("result of {id}"))
    }
}

struct Run {
    out: TurnOutcome,
    rec: Arc<Rec>,
    probe: Arc<Probe>,
    took: Duration,
}

#[derive(Default)]
struct Opts {
    parent: Option<SessionId>,
    cancel: Option<tokio_util::sync::CancellationToken>,
    callback: bool,
}

async fn run_opts(host: &Host, steps: Vec<Vec<(&'static str, Value)>>, o: Opts) -> Run {
    let rec = Arc::new(Rec {
        steps,
        seen: Mutex::default(),
    });
    let probe = Arc::new(Probe::default());
    let tool = |name, parallel, risk| -> Arc<dyn harness_tools::Tool> {
        Arc::new(Slow {
            name,
            parallel,
            risk,
            probe: probe.clone(),
        })
    };
    let tools = vec![
        tool("look", true, ToolRisk::ReadOnly),
        tool("find", true, ToolRisk::ReadOnly),
        tool("peek", false, ToolRisk::ReadOnly),
        tool("change", true, ToolRisk::Mutating),
    ];
    let b = NativeBackend::new(rec.clone(), tools, NativeConfig::default());
    let dir = tempfile::tempdir().unwrap();
    let started = Instant::now();
    let out = b
        .run_turn(TurnCtx {
            session_id: SessionId::new(),
            turn_id: "t".into(),
            history: Vec::<RawEvent>::new(),
            user_text: "go".into(),
            cwd: dir.path().to_path_buf(),
            model: "rec".into(),
            agent: None,
            compare: vec![],
            parent_session: o.parent,
            extra: harness_core::backend::TurnExtra {
                cause: o.callback.then(|| TurnCause::Callback(vec!["bg-1".into()])),
                ..Default::default()
            },
            cancel: o.cancel.unwrap_or_default(),
            host,
        })
        .await;
    Run {
        out,
        rec,
        probe,
        took: started.elapsed(),
    }
}

async fn run(host: &Host, steps: Vec<Vec<(&'static str, Value)>>) -> Run {
    run_opts(host, steps, Opts::default()).await
}

fn look(id: &str, ms: u64) -> (&'static str, Value) {
    ("look", json!({"id": id, "ms": ms}))
}

/// The tool results of the request after the first step, as sent.
fn sent_results(r: &Run) -> Vec<(String, String)> {
    let reqs = r.rec.seen.lock().unwrap();
    reqs[1]
        .messages
        .iter()
        .filter_map(|m| match m {
            ChatMsg::Tool { call_id, content } => Some((call_id.clone(), content.clone())),
            _ => None,
        })
        .collect()
}

const DELAY: u64 = 300;

/// Four reads with a delay each take about one delay, not four.
#[tokio::test]
async fn four_reads_run_together() {
    let host = Host::new();
    let r = run(
        &host,
        vec![(1..=4).map(|i| look(&format!("r{i}"), DELAY)).collect()],
    )
    .await;
    assert_eq!(r.out.reason, TurnEndReason::Completed, "{:?}", r.out);
    assert_eq!(r.probe.most.load(Ordering::SeqCst), 4, "all four at once");
    assert!(
        r.took < Duration::from_millis(DELAY * 3),
        "four reads of {DELAY} ms took {:?}",
        r.took
    );
    assert!(host.finished().iter().all(|(_, ok, _)| *ok));
}

/// Call 3 finishes first, call 1 last: the log has every start in call
/// order, then every result in call order, and the next request sends the
/// results in call order.
#[tokio::test]
async fn events_and_the_next_request_keep_call_order() {
    let host = Host::new();
    let r = run(
        &host,
        vec![vec![look("c1", 400), look("c2", 200), look("c3", 10)]],
    )
    .await;
    let (_, end1) = r.probe.span("c1");
    let (_, end3) = r.probe.span("c3");
    assert!(end3 < end1, "the test needs call 3 to finish first");
    assert_eq!(
        host.order(),
        ["S:c1", "S:c2", "S:c3", "F:c1", "F:c2", "F:c3"],
        "starts, then results, in call order"
    );
    let sent = sent_results(&r);
    assert_eq!(
        sent,
        [
            ("c1".to_string(), "result of c1".to_string()),
            ("c2".into(), "result of c2".into()),
            ("c3".into(), "result of c3".into())
        ]
    );
}

/// A call that changes something splits the step: the reads before it
/// run together and end before it starts; it runs alone; the reads after
/// it run together after it.
#[tokio::test]
async fn a_change_between_reads_runs_alone_in_its_place() {
    let host = Host::new();
    let r = run(
        &host,
        vec![vec![
            look("a", 100),
            look("b", 100),
            ("change", json!({"id": "w", "ms": 10})),
            look("c", 100),
            look("d", 100),
        ]],
    )
    .await;
    assert_eq!(
        host.order(),
        [
            "S:a", "S:b", "F:a", "F:b", "S:w", "F:w", "S:c", "S:d", "F:c", "F:d"
        ]
    );
    let (w0, w1) = r.probe.span("w");
    assert!(r.probe.span("a").1 <= w0 && r.probe.span("b").1 <= w0);
    assert!(w1 <= r.probe.span("c").0 && w1 <= r.probe.span("d").0);
    assert_eq!(r.probe.most.load(Ordering::SeqCst), 2);
}

/// A read-only tool that does not declare itself parallel safe runs in
/// order, as every tool did before (an allow-list).
#[tokio::test]
async fn an_undeclared_read_only_tool_stays_in_order() {
    let host = Host::new();
    let r = run(
        &host,
        vec![vec![
            ("peek", json!({"id": "p1", "ms": 50})),
            ("peek", json!({"id": "p2", "ms": 50})),
        ]],
    )
    .await;
    assert_eq!(host.order(), ["S:p1", "F:p1", "S:p2", "F:p2"]);
    assert_eq!(r.probe.most.load(Ordering::SeqCst), 1);
}

/// Where read-only calls need a card (`ask_all`), nothing runs in
/// parallel and each call is asked about alone, in order.
#[tokio::test]
async fn a_call_that_needs_a_card_keeps_the_step_in_order() {
    let mut host = Host::new();
    host.unasked = false;
    let r = run(&host, vec![vec![look("a", 50), look("b", 50)]]).await;
    assert_eq!(host.order(), ["S:a", "F:a", "S:b", "F:b"]);
    assert_eq!(r.probe.most.load(Ordering::SeqCst), 1);
    assert_eq!(*host.asked.lock().unwrap(), ["look", "look"]);
}

/// A tool in the node's `denied_tools` is never part of a batch: it is
/// refused by the policy, in order, and runs nowhere.
#[tokio::test]
async fn a_denied_tool_is_refused_in_order() {
    let mut host = Host::new();
    host.denied = Some("look");
    let r = run(&host, vec![vec![look("a", 10), look("b", 10)]]).await;
    assert_eq!(host.order(), ["S:a", "F:a", "S:b", "F:b"]);
    assert_eq!(r.probe.runs.load(Ordering::SeqCst), 0);
    assert!(host.finished().iter().all(|(_, ok, _)| !ok));
}

/// The repeated-call guard counts a batch's calls as in order: six
/// identical reads in one step run four times and the fifth and sixth are
/// refused, as before.
#[tokio::test]
async fn identical_reads_in_one_step_are_counted_as_in_order() {
    let host = Host::new();
    let same = ("look", json!({"id": "same", "ms": 5}));
    let r = run(&host, vec![vec![same; 6]]).await;
    assert_eq!(r.probe.runs.load(Ordering::SeqCst), 4);
    assert_eq!(r.probe.most.load(Ordering::SeqCst), 1, "never together");
    let fin = host.finished();
    assert!(fin[..4].iter().all(|(_, ok, _)| *ok), "{fin:?}");
    assert!(
        fin[4..]
            .iter()
            .all(|(_, ok, o)| !ok && o.contains("Not run")),
        "{fin:?}"
    );
}

/// The guard's second rule (one tool failing the same way, whatever its
/// input) refuses the eighth such call; a batch cannot run more of them.
#[tokio::test]
async fn a_batch_runs_no_more_failing_calls_than_in_order() {
    let host = Host::new();
    let calls: Vec<_> = (0..10)
        .map(|i| ("look", json!({"id": format!("f{i}"), "fail": "same error"})))
        .collect();
    let r = run(&host, vec![calls]).await;
    assert_eq!(
        r.probe.runs.load(Ordering::SeqCst),
        (repeat::TOOL_REFUSE_AT - 1) as usize,
        "{:?}",
        host.finished()
    );
}

/// A tool that panics fails its own call; the others of the batch
/// complete and every call has its result.
#[tokio::test]
async fn a_panicking_call_fails_alone() {
    let host = Host::new();
    let r = run(
        &host,
        vec![vec![
            look("a", 50),
            ("look", json!({"id": "b", "panic": true})),
            look("c", 50),
        ]],
    )
    .await;
    assert_eq!(r.out.reason, TurnEndReason::Completed, "{:?}", r.out);
    let fin = host.finished();
    assert_eq!(fin.len(), 3, "{fin:?}");
    assert!(fin[0].1 && fin[2].1, "{fin:?}");
    assert!(!fin[1].1 && fin[1].2 == TOOL_PANICKED, "{fin:?}");
}

/// A failing call is that call's result; the others are unaffected.
#[tokio::test]
async fn a_failing_call_fails_alone() {
    let host = Host::new();
    run(
        &host,
        vec![vec![
            look("a", 30),
            ("look", json!({"id": "b", "fail": "no such file"})),
            look("c", 10),
        ]],
    )
    .await;
    let fin = host.finished();
    assert_eq!(
        fin.iter().map(|(_, ok, _)| *ok).collect::<Vec<_>>(),
        [true, false, true]
    );
    assert_eq!(fin[1].2, "no such file");
}

/// Stop during a batch: every running call is cancelled, every call of
/// the batch gets its result (the ones that never started too), and the
/// turn ends as cancelled with no further request.
#[tokio::test]
async fn stop_cancels_every_call_of_a_batch() {
    let host = Host::new();
    let cancel = tokio_util::sync::CancellationToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        c2.cancel();
    });
    let calls: Vec<_> = (0..MAX_PARALLEL_CALLS + 2)
        .map(|i| {
            (
                if i % 2 == 0 { "look" } else { "find" },
                json!({"id": format!("w{i}"), "wait": true}),
            )
        })
        .collect();
    let r = run_opts(
        &host,
        vec![calls],
        Opts {
            cancel: Some(cancel),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(r.out.reason, TurnEndReason::Cancelled, "{:?}", r.out);
    assert!(r.took < Duration::from_secs(3), "{:?}", r.took);
    let fin = host.finished();
    assert_eq!(fin.len(), MAX_PARALLEL_CALLS + 2, "{fin:?}");
    assert_eq!(r.probe.most.load(Ordering::SeqCst), MAX_PARALLEL_CALLS);
    assert!(
        fin[..MAX_PARALLEL_CALLS]
            .iter()
            .all(|(id, ok, o)| !ok && *o == format!("{id} cancelled")),
        "{fin:?}"
    );
    assert!(
        fin[MAX_PARALLEL_CALLS..]
            .iter()
            .all(|(_, ok, o)| !ok && o == NOT_RUN_STOPPED),
        "{fin:?}"
    );
    assert_eq!(r.rec.seen.lock().unwrap().len(), 1, "no request after Stop");
}

/// No more than `MAX_PARALLEL_CALLS` run at once, however many the step has.
#[tokio::test]
async fn a_batch_runs_at_most_the_cap_at_once() {
    let host = Host::new();
    // Two tools: the guard keeps at most `TOOL_REFUSE_AT - 1` calls of one
    // tool in a batch (`RepeatGuard::batchable`).
    let n = 2 * (repeat::TOOL_REFUSE_AT as usize - 1);
    let r = run(
        &host,
        vec![
            (0..n)
                .map(|i| {
                    let tool = if i % 2 == 0 { "look" } else { "find" };
                    (tool, json!({"id": format!("r{i}"), "ms": 50}))
                })
                .collect(),
        ],
    )
    .await;
    assert_eq!(r.probe.most.load(Ordering::SeqCst), MAX_PARALLEL_CALLS);
    assert_eq!(host.finished().len(), n);
    assert_eq!(sent_results(&r).len(), n);
}

/// A sub-agent's turn runs the same loop: its reads run together too.
#[tokio::test]
async fn a_sub_agent_turn_runs_reads_together() {
    let host = Host::new();
    let r = run_opts(
        &host,
        vec![(1..=4).map(|i| look(&format!("r{i}"), DELAY)).collect()],
        Opts {
            parent: Some(SessionId::new()),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(r.probe.most.load(Ordering::SeqCst), 4);
    assert_eq!(
        host.order(),
        [
            "S:r1", "S:r2", "S:r3", "S:r4", "F:r1", "F:r2", "F:r3", "F:r4"
        ]
    );
}

/// A callback turn (no person's message) runs the same loop: the same.
#[tokio::test]
async fn a_callback_turn_runs_reads_together() {
    let host = Host::new();
    let r = run_opts(
        &host,
        vec![vec![look("c1", 200), look("c2", 10)]],
        Opts {
            callback: true,
            ..Default::default()
        },
    )
    .await;
    assert_eq!(r.probe.most.load(Ordering::SeqCst), 2);
    assert_eq!(host.order(), ["S:c1", "S:c2", "F:c1", "F:c2"]);
}
