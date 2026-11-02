use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::event::*;
use harness_core::policy::Decision;
use harness_core::view::{Item, SessionView};
use harness_core::{RawEvent, SessionId};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};

struct Harness {
    _dir: tempfile::TempDir,
    ws: std::path::PathBuf,
    node: NodeRuntime,
    link: NodeLink,
    seen: Vec<RawEvent>,
}

fn config(store: Store, policy: NodePolicy) -> NodeConfig {
    NodeConfig {
        node_id: "local".into(),
        hostname: "test".into(),
        policy,
        store,
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(5),
            }),
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: vec![harness_core::Secret::new("sk-node-secret-1".into())],
        profile: harness_core::event::Profile::Agentic,
    }
}

impl Harness {
    async fn new(policy: impl FnOnce(&std::path::Path) -> NodePolicy) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir(&ws).unwrap();
        let store = Store::open(&dir.path().join("node.db"), Durability::Full).unwrap();
        let node = NodeRuntime::start(config(store, policy(dir.path())))
            .await
            .unwrap();
        let mut link = node.attach().await;
        match link.from_node.recv().await.unwrap() {
            NodeToMaster::Hello { node_id, .. } => assert_eq!(node_id, "local"),
            other => panic!("expected hello, got {other:?}"),
        }
        Self {
            _dir: dir,
            ws,
            node,
            link,
            seen: vec![],
        }
    }

    fn send(&self, m: MasterToNode) {
        self.link.to_node.send(m).unwrap();
    }

    /// Receive until `pred` matches an event; returns it. Acks must be ok.
    async fn until(&mut self, pred: impl Fn(&EventKind) -> bool) -> EventKind {
        loop {
            let msg = tokio::time::timeout(Duration::from_secs(10), self.link.from_node.recv())
                .await
                .expect("timed out waiting for event")
                .expect("link closed");
            match msg {
                NodeToMaster::Event { event } => {
                    let k = event.typed().unwrap();
                    self.seen.push(event);
                    if pred(&k) {
                        return k;
                    }
                }
                NodeToMaster::Ack { ok, error, .. } => assert!(ok, "nack: {error:?}"),
                _ => {}
            }
        }
    }

    async fn create(&mut self) -> SessionId {
        let sid = SessionId::new();
        self.send(MasterToNode::Create {
            profile: None,
            agent: None,
            req_id: "c".into(),
            session_id: sid,
            title: "t".into(),
            cwd: self.ws.display().to_string(),
            model: None,
            approval_mode: Some(harness_core::policy::ApprovalMode::Auto),
        });
        self.until(|k| matches!(k, EventKind::SessionCreated(_)))
            .await;
        sid
    }
}

fn default_policy(root: &std::path::Path) -> NodePolicy {
    NodePolicy {
        workspace_roots: vec![root.into()],
        ..Default::default()
    }
}

#[tokio::test]
async fn approval_roundtrip_and_auto_request_is_clamped() {
    let mut h = Harness::new(default_policy).await;
    let sid = h.create().await;
    // The master asked for `auto`, the node policy max is `ask`.
    let created = h.seen[0].typed().unwrap();
    let EventKind::SessionCreated(c) = created else {
        panic!()
    };
    assert_eq!(c.approval_mode, harness_core::policy::ApprovalMode::Ask);

    h.send(MasterToNode::Prompt {
        agent: None,
        req_id: "p".into(),
        session_id: sid,
        text: "run: echo sk-node-secret-1 > out.txt; echo written".into(),
    });
    let EventKind::ApprovalRequested(a) = h
        .until(|k| matches!(k, EventKind::ApprovalRequested(_)))
        .await
    else {
        unreachable!()
    };
    assert!(!h.ws.join("out.txt").exists(), "ran before approval");
    h.send(MasterToNode::ApprovalRespond {
        req_id: "a".into(),
        session_id: sid,
        approval_id: a.approval_id,
        decision: Decision::Allow,
    });
    h.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
    assert!(h.ws.join("out.txt").exists());
    // Secret scrubbed from every stored payload, including the tool input.
    let stored = h.node.store().events_after(sid, 0, 1000).unwrap();
    let dump = serde_json::to_string(&stored).unwrap();
    assert!(!dump.contains("sk-node-secret-1"));
    // Stored log == what the link delivered, contiguous.
    assert_eq!(stored, h.seen);
    let v = SessionView::fold(&stored).unwrap();
    assert!(v.running_turn.is_none());
    assert!(v.pending_approvals.is_empty());
}

#[tokio::test]
async fn cancel_while_waiting_for_approval_and_while_running() {
    let mut h = Harness::new(default_policy).await;
    let sid = h.create().await;
    h.send(MasterToNode::Prompt {
        agent: None,
        req_id: "p".into(),
        session_id: sid,
        text: "run: sleep 100".into(),
    });
    h.until(|k| matches!(k, EventKind::ApprovalRequested(_)))
        .await;
    h.send(MasterToNode::Cancel {
        req_id: "x".into(),
        session_id: sid,
    });
    let EventKind::ApprovalResolved(r) = h
        .until(|k| matches!(k, EventKind::ApprovalResolved(_)))
        .await
    else {
        unreachable!()
    };
    assert_eq!(r.by, ResolvedBy::Cancelled);
    let EventKind::TurnEnded(t) = h.until(|k| matches!(k, EventKind::TurnEnded(_))).await else {
        unreachable!()
    };
    assert_eq!(t.reason, TurnEndReason::Cancelled);

    // Now approve a long-running command and cancel it while it runs.
    let pidfile = h.ws.join("pid");
    h.send(MasterToNode::Prompt {
        agent: None,
        req_id: "p2".into(),
        session_id: sid,
        text: format!("run: (sleep 1000) & echo $! > {}; wait", pidfile.display()),
    });
    let EventKind::ApprovalRequested(a) = h
        .until(|k| matches!(k, EventKind::ApprovalRequested(_)))
        .await
    else {
        unreachable!()
    };
    h.send(MasterToNode::ApprovalRespond {
        req_id: "a".into(),
        session_id: sid,
        approval_id: a.approval_id,
        decision: Decision::Allow,
    });
    let mut pid = None;
    for _ in 0..200 {
        if let Some(p) = std::fs::read_to_string(&pidfile)
            .ok()
            .and_then(|s| s.trim().parse::<i32>().ok())
        {
            pid = Some(p);
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let pid = pid.expect("pid");
    h.send(MasterToNode::Cancel {
        req_id: "x2".into(),
        session_id: sid,
    });
    let EventKind::TurnEnded(t) = h.until(|k| matches!(k, EventKind::TurnEnded(_))).await else {
        unreachable!()
    };
    assert_eq!(t.reason, TurnEndReason::Cancelled);
    let alive = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .map(|s| {
            !s.rsplit(')')
                .next()
                .unwrap_or("")
                .trim_start()
                .starts_with('Z')
        })
        .unwrap_or(false);
    assert!(!alive, "child {pid} survived cancel");
}

#[tokio::test]
async fn policy_denied_tool_never_runs() {
    let mut h = Harness::new(|root| NodePolicy {
        workspace_roots: vec![root.into()],
        denied_tools: vec!["bash".into()],
        ..Default::default()
    })
    .await;
    let sid = h.create().await;
    h.send(MasterToNode::Prompt {
        agent: None,
        req_id: "p".into(),
        session_id: sid,
        text: "run: touch nope".into(),
    });
    let EventKind::ApprovalResolved(r) = h
        .until(|k| matches!(k, EventKind::ApprovalResolved(_)))
        .await
    else {
        unreachable!()
    };
    assert_eq!(r.by, ResolvedBy::Policy);
    h.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
    assert!(!h.ws.join("nope").exists());
}

#[tokio::test]
async fn cwd_outside_roots_is_refused() {
    let mut h = Harness::new(|root| NodePolicy {
        workspace_roots: vec![root.join("ws").join("sub")],
        ..Default::default()
    })
    .await;
    h.send(MasterToNode::Create {
        profile: None,
        agent: None,
        req_id: "c".into(),
        session_id: SessionId::new(),
        title: "t".into(),
        cwd: "/".into(),
        model: None,
        approval_mode: None,
    });
    match h.link.from_node.recv().await.unwrap() {
        NodeToMaster::Ack { ok, error, .. } => {
            assert!(!ok);
            assert!(error.unwrap().contains("workspace_roots"));
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn resume_replays_in_batches() {
    let mut h = Harness::new(default_policy).await;
    let sid = h.create().await;
    h.send(MasterToNode::Prompt {
        agent: None,
        req_id: "p".into(),
        session_id: sid,
        text: "hello world".into(),
    });
    h.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
    h.send(MasterToNode::Resume {
        session_id: sid,
        last_seq: 2,
    });
    loop {
        match h.link.from_node.recv().await.unwrap() {
            NodeToMaster::Snapshot {
                head_seq, events, ..
            } => {
                assert_eq!(events.first().unwrap().seq, 3);
                assert_eq!(events.last().unwrap().seq, head_seq);
                assert_eq!(events, h.seen[2..].to_vec());
                break;
            }
            NodeToMaster::Ack { .. } => {}
            other => panic!("{other:?}"),
        }
    }
}

/// A log left mid-turn by a crash is closed out on start.
#[tokio::test]
async fn recovers_interrupted_turn_on_start() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let path = dir.path().join("node.db");
    let sid = SessionId::new();
    {
        let store = Store::open(&path, Durability::Full).unwrap();
        let kinds = vec![
            EventKind::SessionCreated(SessionCreated {
                profile: None,
                agent_id: None,
                incognito: false,
                title: "t".into(),
                cwd: ws.display().to_string(),
                model: "mock".into(),
                backend: "native".into(),
                node_id: "local".into(),
                approval_mode: Default::default(),
            }),
            EventKind::TurnStarted(TurnStarted {
                agent: None,
                turn_id: "t1".into(),
            }),
            EventKind::ToolCallStarted(ToolCallStarted {
                author: None,
                turn_id: "t1".into(),
                call_id: "c1".into(),
                tool: "bash".into(),
                input: serde_json::json!({"command": "ls"}),
            }),
            EventKind::ApprovalRequested(ApprovalRequested {
                approval_id: "a1".into(),
                call_id: "c1".into(),
                tool: "bash".into(),
                input: serde_json::json!({}),
                reason: "r".into(),
            }),
        ];
        for (i, k) in kinds.into_iter().enumerate() {
            let (kind, payload) = k.into_parts();
            store
                .append(
                    "local",
                    &RawEvent {
                        session_id: sid,
                        seq: i as u64 + 1,
                        ts_ms: 0,
                        kind,
                        payload,
                    },
                )
                .unwrap();
        }
    }
    let store = Store::open(&path, Durability::Full).unwrap();
    let node = NodeRuntime::start(config(
        store.clone(),
        NodePolicy {
            workspace_roots: vec![dir.path().into()],
            ..Default::default()
        },
    ))
    .await
    .unwrap();
    let evs = store.events_after(sid, 0, 100).unwrap();
    let v = SessionView::fold(&evs).unwrap();
    assert!(v.running_turn.is_none());
    assert!(v.pending_approvals.is_empty());
    assert!(matches!(
        v.items.last(),
        Some(Item::TurnEnd {
            reason: TurnEndReason::Interrupted,
            ..
        })
    ));
    // And the session accepts a new prompt afterwards.
    let mut link = node.attach().await;
    link.to_node
        .send(MasterToNode::Prompt {
            agent: None,
            req_id: "p".into(),
            session_id: sid,
            text: "continue".into(),
        })
        .unwrap();
    loop {
        if let Some(NodeToMaster::Event { event }) = link.from_node.recv().await
            && event.kind == "turn_ended"
        {
            break;
        }
    }
}
