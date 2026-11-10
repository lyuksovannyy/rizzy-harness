//! Streamed tool output has a total budget per call (PLAN.md T9): a flood
//! produces deltas totalling at most the budget plus one marker, the model is
//! told, and cancel and timeout still end the command promptly.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use harness_agent_native::{NativeBackend, NativeConfig, mock};
use harness_core::backend::{AgentBackend, TurnCtx, TurnHost};
use harness_core::event::{EventKind, ToolCallFinished, TurnEndReason};
use harness_core::policy::{Decision, ToolRisk};
use harness_tools::stream::TRUNCATED_MARKER;
use serde_json::Value;

#[derive(Default)]
struct Host(Mutex<Vec<EventKind>>);

#[async_trait::async_trait]
impl TurnHost for Host {
    async fn emit(&self, kind: EventKind) {
        self.0.lock().unwrap().push(kind);
    }
    async fn approve(&self, _: &str, _: &str, _: &Value, _: ToolRisk) -> Decision {
        Decision::Allow
    }
}

impl Host {
    fn deltas(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                EventKind::ToolOutputDelta(d) => Some(d.text.clone()),
                _ => None,
            })
            .collect()
    }

    fn finished(&self) -> ToolCallFinished {
        self.0
            .lock()
            .unwrap()
            .iter()
            .find_map(|e| match e {
                EventKind::ToolCallFinished(f) => Some(f.clone()),
                _ => None,
            })
            .expect("tool_call_finished")
    }
}

fn backend(budget: usize, timeout: Duration) -> NativeBackend {
    NativeBackend::new(
        Arc::new(mock::Mock {
            delay: Duration::from_millis(1),
        }),
        harness_tools::builtin(),
        NativeConfig {
            tool_stream_bytes: budget,
            tool_timeout: timeout,
            ..Default::default()
        },
    )
}

fn turn<'a>(host: &'a Host, text: &str, cwd: &std::path::Path) -> TurnCtx<'a> {
    TurnCtx {
        session_id: harness_core::SessionId::new(),
        turn_id: "t".into(),
        history: vec![],
        user_text: text.into(),
        cwd: cwd.to_path_buf(),
        model: "mock".into(),
        agent: None,
        compare: vec![],
        parent_session: None,
        extra: Default::default(),
        cancel: Default::default(),
        host,
    }
}

#[tokio::test]
async fn flood_streams_at_most_the_budget_and_one_marker() {
    let dir = tempfile::tempdir().unwrap();
    let host = Host::default();
    let budget = 32 * 1024;
    let out = backend(budget, Duration::from_secs(60))
        .run_turn(turn(
            &host,
            "run: head -c 20000000 /dev/zero | tr '\\0' a; exit 3",
            dir.path(),
        ))
        .await;
    assert_eq!(out.reason, TurnEndReason::Completed);
    let deltas = host.deltas();
    let all = deltas.concat();
    assert_eq!(all.len(), budget + TRUNCATED_MARKER.len());
    assert_eq!(all.matches(TRUNCATED_MARKER).count(), 1);
    assert!(all.ends_with(TRUNCATED_MARKER));
    // Coalesced into ~2 KiB events: the count is bounded by the budget,
    // not by the 20 MB the command printed.
    assert!(deltas.len() <= budget / 1024 + 2, "{} deltas", deltas.len());
    let f = host.finished();
    assert_eq!(f.exit_code, Some(3));
    assert!(f.output.contains("live output stopped"), "{}", f.output);
}

#[tokio::test]
async fn output_under_the_budget_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let host = Host::default();
    backend(1 << 20, Duration::from_secs(60))
        .run_turn(turn(&host, "run: seq 1 30000", dir.path()))
        .await;
    let want: String = (1..=30000).map(|n| format!("{n}\n")).collect();
    assert!(host.deltas().concat() == want, "stream differs");
    assert!(!host.finished().output.contains("live output stopped"));
}

#[tokio::test]
async fn cancel_during_a_flood_returns_promptly() {
    let dir = tempfile::tempdir().unwrap();
    let host = Host::default();
    let c = turn(&host, "run: yes", dir.path());
    let cancel = c.cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        cancel.cancel();
    });
    let b = backend(1 << 20, Duration::from_secs(600));
    let started = Instant::now();
    let out = tokio::time::timeout(Duration::from_secs(10), b.run_turn(c))
        .await
        .expect("cancel hung during a flood");
    assert!(started.elapsed() < Duration::from_secs(4));
    assert_eq!(out.reason, TurnEndReason::Cancelled);
    assert!(host.deltas().concat().len() <= (1 << 20) + TRUNCATED_MARKER.len());
}

#[tokio::test]
async fn timeout_during_a_flood_still_fires() {
    let dir = tempfile::tempdir().unwrap();
    let host = Host::default();
    let started = Instant::now();
    backend(64 * 1024, Duration::from_secs(1))
        .run_turn(turn(&host, "run: yes", dir.path()))
        .await;
    assert!(started.elapsed() < Duration::from_secs(8));
    let f = host.finished();
    assert!(
        f.output.contains("timed out"),
        "{}",
        &f.output[f.output.len() - 300..]
    );
    assert_eq!(
        host.deltas().concat().len(),
        64 * 1024 + TRUNCATED_MARKER.len()
    );
}
