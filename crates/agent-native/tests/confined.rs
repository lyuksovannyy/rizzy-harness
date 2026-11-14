//! A call the node's Accept edits rule let through runs with
//! `allow_outside_workspace` off (PLAN.md T48, review F4): the tool walks
//! its path the way the rule did, so a path out of the folder is refused
//! even on a node that lets file tools leave it. A call a person allowed
//! keeps the node's setting.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock};
use harness_core::backend::{AgentBackend, TurnCtx, TurnHost};
use harness_core::event::{EventKind, ToolCallFinished};
use harness_core::policy::{Decision, ToolRisk};
use serde_json::{Value, json};

struct Host {
    events: Mutex<Vec<EventKind>>,
    /// What `confined` answers: the Accept edits rule allowed the call.
    by_rule: bool,
}

#[async_trait::async_trait]
impl TurnHost for Host {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        self.events.lock().unwrap().push(kind);
        Ok(())
    }
    async fn approve(&self, _: &str, _: &str, _: &Value, _: ToolRisk) -> Decision {
        Decision::Allow
    }
    fn confined(&self, _call_id: &str) -> bool {
        self.by_rule
    }
}

async fn write_outside(by_rule: bool) -> (bool, ToolCallFinished) {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    std::fs::create_dir(dir.path().join("out")).unwrap();
    let host = Host {
        events: Mutex::default(),
        by_rule,
    };
    let backend = NativeBackend::new(
        Arc::new(mock::Mock {
            delay: Duration::from_millis(1),
        }),
        harness_tools::builtin(),
        NativeConfig {
            allow_outside_workspace: true,
            ..Default::default()
        },
    );
    let steps = json!([{"tool": "write", "input": {"path": "../out/x.txt", "content": "x"}}]);
    backend
        .run_turn(TurnCtx {
            session_id: harness_core::SessionId::new(),
            turn_id: "t".into(),
            history: vec![],
            user_text: format!("script:{steps}"),
            cwd: ws.clone(),
            model: "mock".into(),
            agent: None,
            compare: vec![],
            parent_session: None,
            extra: Default::default(),
            cancel: Default::default(),
            host: &host,
        })
        .await;
    let finished = host
        .events
        .lock()
        .unwrap()
        .iter()
        .find_map(|e| match e {
            EventKind::ToolCallFinished(f) => Some(f.clone()),
            _ => None,
        })
        .expect("tool_call_finished");
    (dir.path().join("out/x.txt").exists(), finished)
}

#[tokio::test]
async fn a_call_the_rule_allowed_stays_in_the_folder() {
    let (written, f) = write_outside(true).await;
    assert!(!written, "the rule's call left the folder");
    assert!(
        !f.ok && f.output.contains("outside the session workspace"),
        "{}",
        f.output
    );
}

#[tokio::test]
async fn a_call_a_person_allowed_keeps_the_nodes_setting() {
    let (written, f) = write_outside(false).await;
    assert!(written && f.ok, "{}", f.output);
}
