//! Shared reducer fixture: `web/src/fold.test.ts` folds the same events with
//! the TypeScript port and must produce exactly `expected`.
//! Regenerate with `UPDATE_FIXTURES=1 cargo test -p harness-core --test fold_fixture`.

use harness_core::event::*;
use harness_core::policy::{ApprovalMode, Decision};
use harness_core::view::SessionView;
use harness_core::{RawEvent, SessionId};
use serde_json::json;

fn events() -> Vec<RawEvent> {
    let sid: SessionId = "01a19bf2-0000-7000-8000-00000000f1f0".parse().unwrap();
    let t = || "t1".to_string();
    let kinds = vec![
        EventKind::SessionCreated(SessionCreated {
            title: "fixture".into(),
            cwd: "/w".into(),
            model: "mock".into(),
            backend: "native".into(),
            node_id: "local".into(),
            approval_mode: ApprovalMode::Ask,
        }),
        EventKind::TurnStarted(TurnStarted { turn_id: t() }),
        EventKind::UserMessage(UserMessage {
            turn_id: t(),
            text: "fix the bug".into(),
        }),
        EventKind::AssistantDelta(AssistantDelta {
            turn_id: t(),
            message_id: "m1".into(),
            text: "Let me ".into(),
        }),
        EventKind::AssistantDelta(AssistantDelta {
            turn_id: t(),
            message_id: "m1".into(),
            text: "look.".into(),
        }),
        EventKind::AssistantMessage(AssistantMessage {
            turn_id: t(),
            message_id: "m1".into(),
            text: "Let me look.".into(),
            tool_calls: vec![ToolCallRef {
                call_id: "c1".into(),
                tool: "bash".into(),
                input: json!({"command": "cargo test"}),
            }],
        }),
        EventKind::ToolCallStarted(ToolCallStarted {
            turn_id: t(),
            call_id: "c1".into(),
            tool: "bash".into(),
            input: json!({"command": "cargo test"}),
        }),
        EventKind::ApprovalRequested(ApprovalRequested {
            approval_id: "a1".into(),
            call_id: "c1".into(),
            tool: "bash".into(),
            input: json!({"command": "cargo test"}),
            reason: "runs programs".into(),
        }),
        EventKind::ApprovalResolved(ApprovalResolved {
            approval_id: "a1".into(),
            decision: Decision::AllowSession,
            by: ResolvedBy::User,
        }),
        EventKind::ToolOutputDelta(ToolOutputDelta {
            call_id: "c1".into(),
            stream: OutputStream::Stdout,
            text: "running 3 tests\n".into(),
        }),
        EventKind::ToolCallFinished(ToolCallFinished {
            call_id: "c1".into(),
            ok: false,
            output: "1 failed".into(),
            exit_code: Some(101),
            truncated: false,
        }),
        EventKind::ToolCallStarted(ToolCallStarted {
            turn_id: t(),
            call_id: "c2".into(),
            tool: "read".into(),
            input: json!({"path": "src/lib.rs"}),
        }),
        EventKind::ToolCallFinished(ToolCallFinished {
            call_id: "c2".into(),
            ok: true,
            output: "     1\tfn main() {}".into(),
            exit_code: None,
            truncated: false,
        }),
        // A message with no preceding deltas.
        EventKind::AssistantMessage(AssistantMessage {
            turn_id: t(),
            message_id: "m2".into(),
            text: "Found it.".into(),
            tool_calls: vec![],
        }),
        EventKind::ToolCallStarted(ToolCallStarted {
            turn_id: t(),
            call_id: "c3".into(),
            tool: "edit".into(),
            input: json!({"path": "a", "old_string": "x", "new_string": "y"}),
        }),
        EventKind::ApprovalRequested(ApprovalRequested {
            approval_id: "a2".into(),
            call_id: "c3".into(),
            tool: "edit".into(),
            input: json!({}),
            reason: "changes files".into(),
        }),
        EventKind::Checkpoint(Checkpoint {
            turn_id: t(),
            phase: CheckpointPhase::Before,
            git_ref: "refs/x".into(),
        }),
        EventKind::Error(ErrorEvent {
            turn_id: Some(t()),
            message: "node restarted".into(),
        }),
        EventKind::TurnEnded(TurnEnded {
            turn_id: t(),
            reason: TurnEndReason::Interrupted,
            usage: Some(Usage {
                input_tokens: 1,
                output_tokens: 2,
            }),
        }),
    ];
    let mut out: Vec<RawEvent> = kinds
        .into_iter()
        .enumerate()
        .map(|(i, k)| {
            let (kind, payload) = k.into_parts();
            RawEvent {
                session_id: sid,
                seq: i as u64 + 1,
                ts_ms: 1_700_000_000_000 + i as i64,
                kind,
                payload,
            }
        })
        .collect();
    let n = out.len() as u64;
    out.push(RawEvent {
        session_id: sid,
        seq: n + 1,
        ts_ms: 0,
        kind: "from_a_newer_node".into(),
        payload: json!({"anything": true}),
    });
    out
}

#[test]
fn fixture_is_current() {
    let evs = events();
    let expected = SessionView::fold(&evs).unwrap();
    let doc = json!({ "events": evs, "expected": expected });
    let text = serde_json::to_string_pretty(&doc).unwrap() + "\n";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../web/src/__fixtures__/session.json");
    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    let on_disk =
        std::fs::read_to_string(&path).expect("fixture missing; run with UPDATE_FIXTURES=1");
    assert_eq!(
        on_disk, text,
        "fixture is stale; run with UPDATE_FIXTURES=1"
    );
}
