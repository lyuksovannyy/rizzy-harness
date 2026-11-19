//! The independent review of PR 117 (kept as a regression test): a note racing a person's turn. A note is
//! asked right after each turn, then a prompt follows after 0..8 ms, so
//! the note's append lands before, during and after the turn's claim.
//! Invariants checked on the log: seq contiguous from 1; every note's
//! `through_seq` is `seq - 1`; no note inside a turn (between a
//! `turn_started` and its `turn_ended`); no prompt refused ("a turn is
//! already running": `until` asserts every ack ok).

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::llm::{ModelRouter, Route};
use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::event::*;
use harness_core::policy::ApprovalMode;
use harness_core::{RawEvent, SessionId};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};

fn config(dir: &std::path::Path) -> NodeConfig {
    let router = ModelRouter::new(vec![Route {
        prefix: "demo".into(),
        provider: Arc::new(Mock {
            delay: Duration::from_millis(1),
        }),
    }]);
    NodeConfig {
        node_id: "local".into(),
        hostname: "test".into(),
        policy: NodePolicy {
            workspace_roots: vec![dir.into()],
            max_approval_mode: ApprovalMode::Auto,
            // Every one of the 40 asks may start a note.
            limits: harness_node::policy::LimitsPolicy {
                handoff_notes_per_hour: 60,
                ..Default::default()
            },
            ..Default::default()
        },
        store: Store::open(&dir.join("node.db"), Durability::Full).unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(router),
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        default_model: "demo::mock".into(),
        secrets: Default::default(),
        profile: Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    }
}

/// Events until one for `sid` matches; every ack (prompts, notes) must be
/// ok except a note's "nothing due" / "busy".
async fn until(link: &mut NodeLink, sid: SessionId, pred: impl Fn(&EventKind) -> bool) {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(10), link.from_node.recv())
            .await
            .expect("timed out waiting for event")
            .expect("link closed");
        match msg {
            NodeToMaster::Event { event } if event.session_id == sid => {
                if pred(&event.typed().unwrap()) {
                    return;
                }
            }
            NodeToMaster::Ack {
                ok, error, req_id, ..
            } => {
                if req_id.starts_with('h') {
                    continue;
                }
                assert!(ok, "nack of {req_id}: {error:?}");
            }
            _ => {}
        }
    }
}

fn work(step: &str) -> String {
    format!(
        "script: {}",
        serde_json::json!([{"tool": "todo", "input": {"items": [
            {"content": step, "status": "in_progress"},
        ]}}])
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_note_never_lands_inside_a_turn_and_never_refuses_a_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let node = NodeRuntime::start(config(dir.path())).await.unwrap();
    let mut link = node.attach().await;
    let _hello = link.from_node.recv().await.unwrap();
    let sid = SessionId::new();
    link.to_node
        .try_send(MasterToNode::Create {
            exec_mode: None,
            no_callbacks: false,
            profile: None,
            incognito: false,
            agent: None,
            req_id: "c".into(),
            session_id: sid,
            title: "t".into(),
            cwd: ws.display().to_string(),
            model: None,
            approval_mode: Some(ApprovalMode::Auto),
        })
        .unwrap();
    until(&mut link, sid, |k| {
        matches!(k, EventKind::SessionCreated(_))
    })
    .await;
    let prompt = |link: &NodeLink, i: usize, text: String| {
        link.to_node
            .try_send(MasterToNode::Prompt {
                savings: None,
                context: None,
                reply: None,
                agent: None,
                attachments: vec![],
                compare: vec![],
                req_id: format!("p{i}"),
                session_id: sid,
                text,
                images: vec![],
                agent_named: false,
                files: vec![],
                owner: None,
                peers: vec![],
            })
            .unwrap();
    };
    prompt(&link, 0, work("step 0"));
    until(&mut link, sid, |k| matches!(k, EventKind::TurnEnded(_))).await;
    for i in 1..=40usize {
        link.to_node
            .try_send(MasterToNode::Handoff {
                req_id: format!("h{i}"),
                session_id: sid,
                small_model: None,
                windows: Default::default(),
                low_effort: vec![],
            })
            .unwrap();
        // 0..8 ms: the note's append before, at and after the claim.
        let wait = (i % 9) as u64;
        if wait > 0 {
            tokio::time::sleep(Duration::from_millis(wait)).await;
        }
        prompt(&link, i, work(&format!("step {i}")));
        until(&mut link, sid, |k| matches!(k, EventKind::TurnEnded(_))).await;
    }
    // Let any last note land.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let log: Vec<RawEvent> = node.store().events_after(sid, 0, 100_000).unwrap();
    let mut open = false;
    let mut notes = 0;
    let mut before_turn = 0;
    for (i, e) in log.iter().enumerate() {
        assert_eq!(e.seq, i as u64 + 1, "seq not contiguous at {i}");
        match e.kind.as_str() {
            "turn_started" => {
                assert!(!open, "turn inside a turn at {}", e.seq);
                open = true;
            }
            "turn_ended" => open = false,
            "handoff_written" => {
                notes += 1;
                assert!(!open, "note inside a turn at seq {}", e.seq);
                let h = match e.typed().unwrap() {
                    EventKind::HandoffWritten(h) => h,
                    _ => unreachable!(),
                };
                assert_eq!(
                    h.through_seq,
                    e.seq - 1,
                    "note does not follow what it read"
                );
                if log.get(i + 1).is_some_and(|n| n.kind == "turn_started") {
                    before_turn += 1;
                }
            }
            _ => {}
        }
    }
    println!(
        "events {}, notes landed {notes} of 40 asks, notes directly before a turn_started {before_turn}",
        log.len()
    );
    // Some raced in before the turn's claim; the rest were cancelled.
    assert!(notes > 0, "no note landed: the race was not exercised");
}
