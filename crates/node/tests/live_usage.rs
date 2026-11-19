//! Usage during a turn (X.39): the node's session actor writes a
//! `turn_usage` after each model call that reported usage, a running total
//! that the turn's `turn_ended.usage` ends equal to; a turn stopped midway
//! keeps what it had reached in the chat's figures.

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::SessionId;
use harness_core::event::{EventKind, Profile, TurnEndReason, Usage};
use harness_core::policy::ApprovalMode;
use harness_core::view::SessionView;
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster, PromptMsg};
use harness_store::{Durability, Store};
use serde_json::{Value, json};

struct N {
    node: NodeRuntime,
    link: NodeLink,
}

async fn start(db: &std::path::Path, root: &std::path::Path, max_steps: usize) -> N {
    let mut policy = NodePolicy {
        workspace_roots: vec![root.into()],
        max_approval_mode: ApprovalMode::Auto,
        ..Default::default()
    };
    policy.limits.max_steps = max_steps;
    let cfg = NodeConfig {
        node_id: "n".into(),
        hostname: "test".into(),
        policy,
        store: Store::open(db, Durability::Full).unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(2),
            }),
            harness_tools::builtin(),
            NativeConfig {
                max_steps,
                ..Default::default()
            },
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    };
    let node = NodeRuntime::start(cfg).await.unwrap();
    let mut link = node.attach().await;
    match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { .. } => {}
        other => panic!("expected hello, got {other:?}"),
    }
    N { node, link }
}

impl N {
    fn send(&self, m: MasterToNode) {
        self.link.to_node.try_send(m).unwrap();
    }

    async fn next(&mut self) -> NodeToMaster {
        tokio::time::timeout(Duration::from_secs(30), self.link.from_node.recv())
            .await
            .expect("timed out")
            .expect("link closed")
    }

    async fn ack(&mut self) -> Result<(), String> {
        loop {
            match self.next().await {
                NodeToMaster::Ack { ok: true, .. } => return Ok(()),
                NodeToMaster::Ack { error, .. } => return Err(error.unwrap_or_default()),
                _ => {}
            }
        }
    }

    async fn create(&mut self, cwd: &std::path::Path) -> SessionId {
        let sid = SessionId::new();
        self.send(MasterToNode::Create {
            req_id: format!("c-{sid}"),
            session_id: sid,
            title: "t".into(),
            cwd: cwd.display().to_string(),
            model: None,
            approval_mode: Some(ApprovalMode::Auto),
            profile: None,
            agent: None,
            incognito: false,
            no_callbacks: false,
            exec_mode: None,
        });
        self.ack().await.unwrap();
        sid
    }

    fn events(&self, sid: SessionId) -> Vec<EventKind> {
        self.node
            .store()
            .events_after(sid, 0, 100_000)
            .unwrap()
            .iter()
            .filter_map(|e| e.typed().ok())
            .collect()
    }

    fn prompt(&self, sid: SessionId, text: &str) {
        self.send(MasterToNode::Prompt(PromptMsg {
            req_id: format!("p-{sid}-{}", harness_core::new_id()),
            ..PromptMsg::test(sid, text)
        }));
    }

    /// Run one turn: the tool calls that finished ok, and how it ended.
    async fn turn(&mut self, sid: SessionId, text: &str) -> (usize, TurnEndReason) {
        self.prompt(sid, text);
        self.until_end(sid).await
    }

    async fn until_end(&mut self, sid: SessionId) -> (usize, TurnEndReason) {
        let mut ran = 0;
        loop {
            let NodeToMaster::Event { event } = self.next().await else {
                continue;
            };
            if event.session_id != sid {
                continue;
            }
            match event.typed().unwrap() {
                EventKind::ToolCallFinished(f) if f.ok => ran += 1,
                EventKind::TurnEnded(e) => return (ran, e.reason),
                _ => {}
            }
        }
    }
}

/// The mock's `script:` of `n` copies of `command`, one per step.
fn many(n: usize, command: &str) -> String {
    let steps: Vec<Value> = (0..n)
        .map(|i| json!({"tool": "bash", "input": {"command": format!("{command} # {i}")}}))
        .collect();
    format!("script:{}", Value::Array(steps))
}


fn tokens(u: &Usage) -> u64 {
    u.input_tokens + u.output_tokens
}

/// A scripted turn of three tool steps is four model calls: each one that
/// reported usage is followed by a `turn_usage` with the turn's running
/// total, rising, in the place between the call's reply and its tool; the
/// last equals what `turn_ended` reports.
#[tokio::test]
async fn each_model_call_writes_the_running_total() {
    let dir = tempfile::tempdir().unwrap();
    let mut n = start(&dir.path().join("node.db"), dir.path(), 40).await;
    let sid = n.create(dir.path()).await;
    let (ran, reason) = n.turn(sid, &many(3, "true")).await;
    assert_eq!((ran, reason), (3, TurnEndReason::Completed));
    let log = n.events(sid);
    let shape: Vec<&str> = log
        .iter()
        .map(|k| k.name())
        .filter(|k| {
            matches!(
                *k,
                "turn_usage" | "assistant_message" | "tool_call_finished" | "turn_ended"
            )
        })
        .collect();
    let step = ["turn_usage", "assistant_message", "tool_call_finished"];
    let mut want: Vec<&str> = step.repeat(3);
    want.extend(["turn_usage", "assistant_message", "turn_ended"]);
    assert_eq!(shape, want);
    let totals: Vec<u64> = log
        .iter()
        .filter_map(|k| match k {
            EventKind::TurnUsage(t) => Some(tokens(&t.usage)),
            _ => None,
        })
        .collect();
    assert_eq!(totals.len(), 4);
    assert!(totals.windows(2).all(|w| w[0] < w[1]), "{totals:?}");
    let Some(EventKind::TurnEnded(end)) = log.last() else {
        panic!("the turn's last event is its end");
    };
    let Some(EventKind::TurnUsage(last)) = log
        .iter()
        .rev()
        .find(|k| matches!(k, EventKind::TurnUsage(_)))
    else {
        unreachable!()
    };
    assert_eq!(Some(&last.usage), end.usage.as_ref());
    // The events carry contiguous seqs, as every event of the one writer.
    let seqs: Vec<u64> = n
        .node
        .store()
        .events_after(sid, 0, 100_000)
        .unwrap()
        .iter()
        .map(|e| e.seq)
        .collect();
    assert!(seqs.windows(2).all(|w| w[1] == w[0] + 1));
}

/// A turn the person stops after two steps: its running figures are in the
/// log, so the chat's total is what it had reached even if the end carried
/// none.
#[tokio::test]
async fn a_stopped_turn_keeps_its_last_running_figure() {
    let dir = tempfile::tempdir().unwrap();
    let mut n = start(&dir.path().join("node.db"), dir.path(), 40).await;
    let sid = n.create(dir.path()).await;
    n.prompt(sid, &many(10, "sleep 0.4"));
    let mut started = 0;
    while started < 2 {
        if let NodeToMaster::Event { event } = n.next().await
            && matches!(event.typed(), Ok(EventKind::ToolCallStarted(_)))
        {
            started += 1;
        }
    }
    n.send(MasterToNode::Cancel {
        req_id: "stop".into(),
        session_id: sid,
    });
    let (_, reason) = n.until_end(sid).await;
    assert_eq!(reason, TurnEndReason::Cancelled);
    let log = n.events(sid);
    let running: Vec<&Usage> = log
        .iter()
        .filter_map(|k| match k {
            EventKind::TurnUsage(t) => Some(&t.usage),
            _ => None,
        })
        .collect();
    assert!(running.len() >= 2, "{} running figures", running.len());
    let rows = n.node.store().events_after(sid, 0, 100_000).unwrap();
    let mut view = SessionView::default();
    // The chat as it stood with the end taken away: the last running figure.
    let (end, before) = rows.split_last().unwrap();
    assert_eq!(end.kind, "turn_ended");
    for e in before {
        view.apply(e).unwrap();
    }
    assert_eq!(tokens(&view.usage), tokens(running.last().unwrap()));
    assert!(view.running_turn.is_some());
}
