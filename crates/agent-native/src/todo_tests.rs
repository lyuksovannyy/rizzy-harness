//! The `todo` tool through the loop: a valid call is recorded as one
//! `todo_updated` event (the latest is the list), before the call's result;
//! a refused call records nothing and tells the model why.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::backend::{AgentBackend, TurnCtx, TurnHost};
use harness_core::event::*;
use harness_core::policy::{Decision, ToolRisk};
use harness_core::todo::{TodoItem, TodoStatus};
use serde_json::{Value, json};

use crate::{NativeBackend, NativeConfig, mock};

#[derive(Default)]
struct Host(Mutex<Vec<EventKind>>);

#[async_trait::async_trait]
impl TurnHost for Host {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        self.0.lock().unwrap().push(kind);
        Ok(())
    }
    async fn approve(&self, _: &str, _: &str, _: &Value, risk: ToolRisk) -> Decision {
        // Read-only, as the node's policy treats it under `ask`.
        if risk == ToolRisk::ReadOnly {
            Decision::Allow
        } else {
            Decision::Deny
        }
    }
}

async fn turn(script: Value) -> Vec<EventKind> {
    let dir = tempfile::tempdir().unwrap();
    let host = Host::default();
    let b = NativeBackend::new(
        Arc::new(mock::Mock {
            delay: Duration::from_millis(1),
        }),
        harness_tools::builtin(),
        NativeConfig::default(),
    );
    b.run_turn(TurnCtx {
        session_id: harness_core::SessionId::new(),
        turn_id: "t".into(),
        history: vec![],
        user_text: format!("script: {script}"),
        cwd: dir.path().to_path_buf(),
        model: "mock".into(),
        agent: None,
        compare: vec![],
        parent_session: None,
        extra: Default::default(),
        cancel: Default::default(),
        host: &host,
    })
    .await;
    host.0.into_inner().unwrap()
}

fn call(items: Value) -> Value {
    json!({"tool": "todo", "input": {"items": items}})
}

#[tokio::test]
async fn each_call_replaces_the_list_in_the_log() {
    let ev = turn(json!([
        call(json!([
            {"content": "Read the code", "status": "in_progress"},
            {"content": "Write the tests", "status": "pending"},
        ])),
        call(json!([
            {"content": "Read the code", "status": "done"},
            {"content": "Write the tests", "status": "in_progress"},
        ])),
    ]))
    .await;
    let lists: Vec<&TodoUpdated> = ev
        .iter()
        .filter_map(|e| match e {
            EventKind::TodoUpdated(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(lists.len(), 2, "{ev:?}");
    assert_eq!(
        lists[1].items,
        vec![
            TodoItem {
                content: "Read the code".into(),
                status: TodoStatus::Done
            },
            TodoItem {
                content: "Write the tests".into(),
                status: TodoStatus::InProgress
            },
        ]
    );
    // Recorded with its call, before that call's result.
    for t in &lists {
        assert_eq!(t.turn_id, "t");
        let at = |want: &dyn Fn(&EventKind) -> bool| ev.iter().position(want).unwrap();
        let rec = at(&|e| matches!(e, EventKind::TodoUpdated(x) if x.call_id == t.call_id));
        let fin = at(&|e| matches!(e, EventKind::ToolCallFinished(f) if f.call_id == t.call_id));
        assert!(rec < fin, "{ev:?}");
    }
    // The latest wins in the fold too.
    let sid = harness_core::SessionId::new();
    let raw: Vec<harness_core::RawEvent> = ev
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, k)| {
            let (kind, payload) = k.into_parts();
            harness_core::RawEvent {
                session_id: sid,
                seq: i as u64 + 1,
                ts_ms: 0,
                kind,
                payload,
            }
        })
        .collect();
    assert_eq!(
        harness_core::todo::latest(&raw),
        Some(lists[1].items.clone())
    );
}

#[tokio::test]
async fn a_refused_list_is_not_recorded_and_the_model_is_told_why() {
    let ev = turn(json!([call(json!([
        {"content": "a", "status": "in_progress"},
        {"content": "b", "status": "in_progress"},
    ]))]))
    .await;
    assert!(
        !ev.iter().any(|e| matches!(e, EventKind::TodoUpdated(_))),
        "{ev:?}"
    );
    assert!(
        ev.iter().any(|e| matches!(e,
            EventKind::ToolCallFinished(f) if !f.ok && f.output.contains("in_progress"))),
        "{ev:?}"
    );
}
