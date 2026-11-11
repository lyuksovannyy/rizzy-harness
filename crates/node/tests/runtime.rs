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
    config_with(
        store,
        policy,
        Arc::new(Mock {
            delay: Duration::from_millis(5),
        }),
    )
}

fn config_with(
    store: Store,
    policy: NodePolicy,
    provider: Arc<dyn harness_agent_native::Provider>,
) -> NodeConfig {
    NodeConfig {
        node_id: "local".into(),
        hostname: "test".into(),
        policy,
        store,
        backend: Arc::new(NativeBackend::new(
            provider,
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: harness_core::SecretSet::new(vec![harness_core::Secret::new(
            "sk-node-secret-1".into(),
        )]),
        profile: harness_core::event::Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: None,
        artifacts: None,
    }
}

impl Harness {
    async fn new(policy: impl FnOnce(&std::path::Path) -> NodePolicy) -> Self {
        Self::with_provider(
            policy,
            Arc::new(Mock {
                delay: Duration::from_millis(5),
            }),
        )
        .await
    }

    async fn with_provider(
        policy: impl FnOnce(&std::path::Path) -> NodePolicy,
        provider: Arc<dyn harness_agent_native::Provider>,
    ) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir(&ws).unwrap();
        let store = Store::open(&dir.path().join("node.db"), Durability::Full).unwrap();
        let mut cfg = config_with(store, policy(dir.path()), provider);
        cfg.checkpoints_dir = Some(dir.path().join("checkpoints"));
        let node = NodeRuntime::start(cfg).await.unwrap();
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
        self.link.to_node.try_send(m).unwrap();
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
            incognito: false,
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
        savings: None,
        context: None,
        reply: None,
        agent: None,
        attachments: vec![],
        compare: vec![],
        req_id: "p".into(),
        session_id: sid,
        text: "run: echo sk-node-secret-1 > out.txt; echo written".into(),
        images: vec![],
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
        savings: None,
        context: None,
        reply: None,
        agent: None,
        attachments: vec![],
        compare: vec![],
        req_id: "p".into(),
        session_id: sid,
        text: "run: sleep 100".into(),
        images: vec![],
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
        savings: None,
        context: None,
        reply: None,
        agent: None,
        attachments: vec![],
        compare: vec![],
        req_id: "p2".into(),
        session_id: sid,
        text: format!("run: (sleep 1000) & echo $! > {}; wait", pidfile.display()),
        images: vec![],
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

/// A cancel sent right after a prompt stops that prompt's turn, even
/// though cancels are answered ahead of the session's queued commands.
#[tokio::test]
async fn cancel_right_after_prompt_stops_that_turn() {
    let mut h = Harness::new(default_policy).await;
    let sid = h.create().await;
    for round in 0..5 {
        h.send(MasterToNode::Prompt {
            savings: None,
            context: None,
            reply: None,
            agent: None,
            attachments: vec![],
            compare: vec![],
            req_id: format!("p{round}"),
            session_id: sid,
            text: "slow: 400".into(),
            images: vec![],
        });
        h.send(MasterToNode::Cancel {
            req_id: format!("x{round}"),
            session_id: sid,
        });
        let EventKind::TurnEnded(t) = h.until(|k| matches!(k, EventKind::TurnEnded(_))).await
        else {
            unreachable!()
        };
        assert_eq!(t.reason, TurnEndReason::Cancelled, "round {round}");
    }
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
        savings: None,
        context: None,
        reply: None,
        agent: None,
        attachments: vec![],
        compare: vec![],
        req_id: "p".into(),
        session_id: sid,
        text: "run: touch nope".into(),
        images: vec![],
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
        incognito: false,
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
            let error = error.unwrap();
            assert!(
                error.contains("outside the folders this node allows"),
                "{error}"
            );
        }
        other => panic!("{other:?}"),
    }
}

/// `[sandbox] mode = "required"` on a node where the sandbox does not work:
/// no agentic session is created, with the setting named (FIX_PLAN 2.7).
#[tokio::test]
async fn a_node_without_its_required_sandbox_refuses_sessions() {
    let mut h = Harness::new(|root| {
        let mut p = default_policy(root);
        p.sandbox_refusal = Some("set [sandbox] mode in node.toml".into());
        p
    })
    .await;
    h.send(MasterToNode::Create {
        profile: None,
        incognito: false,
        agent: None,
        req_id: "c".into(),
        session_id: SessionId::new(),
        title: "t".into(),
        cwd: h.ws.display().to_string(),
        model: None,
        approval_mode: None,
    });
    match h.link.from_node.recv().await.unwrap() {
        NodeToMaster::Ack { ok, error, .. } => {
            assert!(!ok);
            assert!(error.unwrap().contains("[sandbox] mode"));
        }
        other => panic!("{other:?}"),
    }
}

/// A session's sandbox temp folder goes when the session is deleted.
#[tokio::test]
async fn deleting_a_session_removes_its_temp_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let mut h = Harness::new(|r| {
        let mut p = default_policy(r);
        p.sandbox_tmp = Some(root.clone());
        p
    })
    .await;
    let sid = h.create().await;
    let other = SessionId::new();
    for s in [sid, other] {
        std::fs::create_dir_all(tmp.path().join(s.to_string()).join("x")).unwrap();
    }
    h.send(MasterToNode::Delete {
        req_id: "d".into(),
        session_id: sid,
    });
    loop {
        match tokio::time::timeout(Duration::from_secs(10), h.link.from_node.recv())
            .await
            .unwrap()
            .unwrap()
        {
            NodeToMaster::Deleted { session_id } if session_id == sid => break,
            _ => {}
        }
    }
    assert!(!tmp.path().join(sid.to_string()).exists());
    assert!(tmp.path().join(other.to_string()).exists(), "only its own");
}

/// The node's data directory (keys, the API token, every user's chats)
/// can never be inside a session (AUDIT S1): not its folder, not above it.
#[tokio::test]
async fn cwd_containing_the_data_dir_is_refused() {
    let mut h = Harness::new(|root| {
        let mut p = default_policy(root);
        std::fs::create_dir_all(root.join("ws/.data")).unwrap();
        p.protect(&root.join("ws/.data"));
        p
    })
    .await;
    for cwd in [h.ws.clone(), h.ws.join(".data")] {
        h.send(MasterToNode::Create {
            profile: None,
            incognito: false,
            agent: None,
            req_id: "c".into(),
            session_id: SessionId::new(),
            title: "t".into(),
            cwd: cwd.display().to_string(),
            model: None,
            approval_mode: None,
        });
        match h.link.from_node.recv().await.unwrap() {
            NodeToMaster::Ack { ok, error, .. } => {
                assert!(!ok);
                let error = error.unwrap();
                assert!(error.contains("data directory"), "{error}");
                assert!(error.contains("project folder"), "{error}");
            }
            other => panic!("{other:?}"),
        }
    }
}

/// A session from the log whose folder contains the data directory (made
/// before the rule, or the folder changed since) does no more work there.
#[tokio::test]
async fn a_loaded_session_over_the_data_dir_gets_no_turn() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    let data = ws.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let path = dir.path().join("node.db");
    let sid = SessionId::new();
    {
        let node = NodeRuntime::start(config(
            Store::open(&path, Durability::Full).unwrap(),
            default_policy(dir.path()),
        ))
        .await
        .unwrap();
        let link = node.attach().await;
        link.to_node
            .try_send(MasterToNode::Create {
                profile: None,
                incognito: false,
                agent: None,
                req_id: "c".into(),
                session_id: sid,
                title: "t".into(),
                cwd: ws.display().to_string(),
                model: None,
                approval_mode: None,
            })
            .unwrap();
        let mut from = link.from_node;
        loop {
            if let Some(NodeToMaster::Ack { ok, .. }) = from.recv().await {
                assert!(ok);
                break;
            }
        }
    }
    let mut policy = default_policy(dir.path());
    policy.protect(&data);
    let node = NodeRuntime::start(config(
        Store::open(&path, Durability::Full).unwrap(),
        policy,
    ))
    .await
    .unwrap();
    let mut link = node.attach().await;
    link.to_node
        .try_send(MasterToNode::Prompt {
            savings: None,
            context: None,
            reply: None,
            agent: None,
            attachments: vec![],
            compare: vec![],
            req_id: "p".into(),
            session_id: sid,
            text: "run: echo hi > out.txt".into(),
            images: vec![],
        })
        .unwrap();
    loop {
        match link.from_node.recv().await.unwrap() {
            NodeToMaster::Ack { ok, error, .. } => {
                assert!(!ok);
                assert!(error.unwrap().contains("data directory"));
                break;
            }
            NodeToMaster::Event { event } => {
                assert_ne!(event.kind, "turn_started", "a turn started");
            }
            _ => {}
        }
    }
    assert!(!ws.join("out.txt").exists());
}

#[tokio::test]
async fn resume_replays_in_batches() {
    let mut h = Harness::new(default_policy).await;
    let sid = h.create().await;
    h.send(MasterToNode::Prompt {
        savings: None,
        context: None,
        reply: None,
        agent: None,
        attachments: vec![],
        compare: vec![],
        req_id: "p".into(),
        session_id: sid,
        text: "hello world".into(),
        images: vec![],
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

/// `session.snapshot` batches are cut by bytes as well as by count, so a
/// replay never builds a frame the link refuses (and then retries forever).
/// An event bigger than the budget goes alone in its batch.
#[tokio::test]
async fn resume_batches_are_limited_by_bytes() {
    let mut h = Harness::new(default_policy).await;
    let sid = h.create().await;
    let budget = harness_node::SNAPSHOT_BYTES;
    // Eight events of a quarter budget each, then one bigger than it.
    let sizes = [budget / 4; 8]
        .into_iter()
        .chain([budget + budget / 2])
        .collect::<Vec<_>>();
    for (i, n) in sizes.iter().enumerate() {
        h.send(MasterToNode::Prompt {
            savings: None,
            context: None,
            reply: None,
            agent: None,
            attachments: vec![],
            compare: vec![],
            req_id: format!("p{i}"),
            session_id: sid,
            text: "x".repeat(*n),
            images: vec![],
        });
        h.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
    }
    let head = h.seen.last().unwrap().seq;
    h.send(MasterToNode::Resume {
        session_id: sid,
        last_seq: 0,
    });
    let mut got: Vec<RawEvent> = vec![];
    let mut batches = 0;
    while got.last().map(|e| e.seq) != Some(head) {
        let msg = tokio::time::timeout(Duration::from_secs(10), h.link.from_node.recv())
            .await
            .expect("replay stalled")
            .unwrap();
        if let NodeToMaster::Snapshot { events, .. } = &msg {
            let bytes = serde_json::to_vec(&msg).unwrap().len();
            assert!(
                bytes <= budget || events.len() == 1,
                "a {bytes}-byte snapshot of {} events (budget {budget})",
                events.len()
            );
            batches += 1;
            got.extend(events.iter().cloned());
        }
    }
    assert!(batches > 2, "only {batches} batches");
    // Complete and in order.
    let seqs: Vec<u64> = got.iter().map(|e| e.seq).collect();
    assert_eq!(seqs, (1..=head).collect::<Vec<_>>());
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
                profile: harness_core::event::Profile::Agentic,
                agent_id: None,
                incognito: false,
                parent_session: None,
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
                turn_allow: false,
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
        .try_send(MasterToNode::Prompt {
            savings: None,
            context: None,
            reply: None,
            agent: None,
            attachments: vec![],
            compare: vec![],
            req_id: "p".into(),
            session_id: sid,
            text: "continue".into(),
            images: vec![],
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

/// A delegated sub-task's answer goes to its parent agent, so its turns may
/// not ask the user anything; the cap comes from the session itself, here
/// one the node loads from its log on start.
#[tokio::test]
async fn sub_task_sessions_ask_no_questions() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let store = Store::open(&dir.path().join("node.db"), Durability::Full).unwrap();
    let sid = SessionId::new();
    let (kind, payload) = EventKind::SessionCreated(SessionCreated {
        profile: harness_core::event::Profile::Agentic,
        agent_id: None,
        incognito: false,
        parent_session: Some(SessionId::new()),
        title: "t".into(),
        cwd: ws.display().to_string(),
        model: "mock".into(),
        backend: "native".into(),
        node_id: "local".into(),
        approval_mode: Default::default(),
    })
    .into_parts();
    let created = RawEvent {
        session_id: sid,
        seq: 1,
        ts_ms: 0,
        kind,
        payload,
    };
    store.append("local", &created).unwrap();
    let policy = NodePolicy {
        workspace_roots: vec![dir.path().into()],
        ..Default::default()
    };
    let node = NodeRuntime::start(config(store, policy)).await.unwrap();
    let mut link = node.attach().await;
    link.to_node
        .try_send(MasterToNode::Prompt {
            savings: None,
            context: None,
            reply: None,
            agent: None,
            attachments: vec![],
            compare: vec![],
            req_id: "p".into(),
            session_id: sid,
            text: "ui: choices".into(),
            images: vec![],
        })
        .unwrap();
    let blocks = loop {
        if let Some(NodeToMaster::Event { event }) = link.from_node.recv().await
            && let Ok(EventKind::AssistantMessage(m)) = event.typed()
        {
            break m.blocks;
        }
    };
    assert_eq!(blocks.len(), 1);
    assert_eq!(
        blocks[0].error.as_deref(),
        Some(harness_core::ui::OFF_FOR_AGENT)
    );
}

/// Agentic turns in a git repo get before/after snapshots, and a turn's
/// file changes can be undone over the protocol.
#[tokio::test]
async fn git_checkpoints_and_undo() {
    let mut h = Harness::new(|root| NodePolicy {
        workspace_roots: vec![root.into()],
        max_approval_mode: harness_core::policy::ApprovalMode::Auto,
        ..Default::default()
    })
    .await;
    let git = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&h.ws)
                .args(args)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .status()
                .unwrap()
                .success()
        )
    };
    git(&["init", "-q"]);
    std::fs::write(h.ws.join("keep.txt"), "v1\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "init"]);
    let sid = h.create().await;
    h.send(MasterToNode::Prompt {
        savings: None,
        context: None,
        reply: None,
        agent: None,
        attachments: vec![],
        compare: vec![],
        req_id: "p".into(),
        session_id: sid,
        text: "run: echo v2 > keep.txt; echo new > made.txt".into(),
        images: vec![],
    });
    let EventKind::Checkpoint(after) = h
        .until(|k| matches!(k, EventKind::Checkpoint(c) if c.phase == CheckpointPhase::After))
        .await
    else {
        unreachable!()
    };
    let mut files = after.files.clone();
    files.sort();
    assert_eq!(files, ["keep.txt", "made.txt"]);
    let turn = after.turn_id.clone();
    h.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
    assert_eq!(
        std::fs::read_to_string(h.ws.join("keep.txt")).unwrap(),
        "v2\n"
    );

    h.send(MasterToNode::Restore {
        req_id: "r".into(),
        session_id: sid,
        turn_id: turn,
        force: false,
        rewind: false,
    });
    h.until(|k| matches!(k, EventKind::Checkpoint(c) if c.phase == CheckpointPhase::Restored))
        .await;
    assert_eq!(
        std::fs::read_to_string(h.ws.join("keep.txt")).unwrap(),
        "v1\n"
    );
    assert!(!h.ws.join("made.txt").exists());
}

/// A working copy that arrives with a hostile `.git/config` (fsmonitor,
/// hooks, a filter) runs none of it when a prompt is checkpointed and
/// undone, before anything was approved.
#[cfg(unix)]
#[tokio::test]
async fn hostile_repo_config_does_not_run_on_checkpoint_or_undo() {
    use std::os::unix::fs::PermissionsExt;
    let mut h = Harness::new(|root| NodePolicy {
        workspace_roots: vec![root.into()],
        max_approval_mode: harness_core::policy::ApprovalMode::Auto,
        ..Default::default()
    })
    .await;
    let git = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&h.ws)
                .args(args)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .status()
                .unwrap()
                .success()
        )
    };
    git(&["init", "-q"]);
    std::fs::write(h.ws.join("keep.txt"), "v1\n").unwrap();
    std::fs::write(h.ws.join(".gitattributes"), "* filter=evil\n").unwrap();
    git(&["add", "."]);
    git(&["-c", "commit.gpgSign=false", "commit", "-qm", "init"]);
    let marks = h.ws.parent().unwrap().join("marks");
    std::fs::create_dir(&marks).unwrap();
    let script = |name: &str| {
        let p = h.ws.parent().unwrap().join(format!("{name}.sh"));
        std::fs::write(
            &p,
            format!(
                "#!/bin/sh\ntouch '{}'\n{}\n",
                marks.join(name).display(),
                // Filters pass the content through; the others read nothing.
                if name.starts_with("clean") || name.starts_with("smudge") {
                    "cat"
                } else {
                    "exit 0"
                }
            ),
        )
        .unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p.display().to_string()
    };
    git(&["config", "core.fsmonitor", &script("fsmonitor")]);
    git(&["config", "filter.evil.clean", &script("clean")]);
    git(&["config", "filter.evil.smudge", &script("smudge")]);
    let hooks = h.ws.join(".git/hooks");
    for n in ["post-index-change", "pre-commit", "post-checkout"] {
        let p = hooks.join(n);
        std::fs::copy(script(n), &p).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let sid = h.create().await;
    h.send(MasterToNode::Prompt {
        savings: None,
        context: None,
        reply: None,
        agent: None,
        attachments: vec![],
        compare: vec![],
        req_id: "p".into(),
        session_id: sid,
        text: "run: echo v2 > keep.txt".into(),
        images: vec![],
    });
    let EventKind::Checkpoint(after) = h
        .until(|k| matches!(k, EventKind::Checkpoint(c) if c.phase == CheckpointPhase::After))
        .await
    else {
        unreachable!()
    };
    assert_eq!(after.files, ["keep.txt"]);
    h.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
    h.send(MasterToNode::Restore {
        req_id: "r".into(),
        session_id: sid,
        turn_id: after.turn_id.clone(),
        force: false,
        rewind: false,
    });
    h.until(|k| matches!(k, EventKind::Checkpoint(c) if c.phase == CheckpointPhase::Restored))
        .await;
    assert_eq!(
        std::fs::read_to_string(h.ws.join("keep.txt")).unwrap(),
        "v1\n"
    );
    let ran: Vec<_> = std::fs::read_dir(&marks)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(ran.is_empty(), "repository config ran: {ran:?}");
}

/// A folder without git gets a shadow repository outside it; only the
/// latest N checkpoints are kept; rewind undoes a turn and all later ones.
#[tokio::test]
async fn shadow_checkpoints_keep_latest_and_rewind() {
    let mut h = Harness::new(|root| NodePolicy {
        workspace_roots: vec![root.into()],
        max_approval_mode: harness_core::policy::ApprovalMode::Auto,
        checkpoints_keep: 2,
        ..Default::default()
    })
    .await;
    std::fs::write(h.ws.join("notes.txt"), "v0\n").unwrap();
    let sid = h.create().await;
    let mut turns = vec![];
    for v in 1..=3 {
        h.send(MasterToNode::Prompt {
            savings: None,
            context: None,
            reply: None,
            agent: None,
            attachments: vec![],
            compare: vec![],
            req_id: "p".into(),
            session_id: sid,
            text: format!("run: echo v{v} > notes.txt; echo {v} > f{v}.txt"),
            images: vec![],
        });
        let EventKind::Checkpoint(after) = h
            .until(|k| matches!(k, EventKind::Checkpoint(c) if c.phase == CheckpointPhase::After))
            .await
        else {
            unreachable!()
        };
        turns.push(after.turn_id);
        h.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
    }
    assert!(
        !h.ws.join(".git").exists(),
        "nothing is written into the folder"
    );
    // Keep 2: the first turn's checkpoint is gone.
    h.send(MasterToNode::Restore {
        req_id: "r1".into(),
        session_id: sid,
        turn_id: turns[0].clone(),
        force: false,
        rewind: false,
    });
    let err = loop {
        match tokio::time::timeout(Duration::from_secs(10), h.link.from_node.recv())
            .await
            .unwrap()
            .unwrap()
        {
            NodeToMaster::Ack {
                req_id,
                ok: false,
                error,
            } if req_id == "r1" => break error.unwrap(),
            NodeToMaster::Ack {
                req_id, ok: true, ..
            } if req_id == "r1" => panic!("should be pruned"),
            _ => {}
        }
    };
    assert!(err.contains("no longer kept"), "{err}");
    // Rewind to before turn 2: turns 2 and 3 are undone together.
    h.send(MasterToNode::Restore {
        req_id: "r2".into(),
        session_id: sid,
        turn_id: turns[1].clone(),
        force: false,
        rewind: true,
    });
    let mut restored = vec![];
    while restored.len() < 2 {
        if let EventKind::Checkpoint(c) = h
            .until(
                |k| matches!(k, EventKind::Checkpoint(c) if c.phase == CheckpointPhase::Restored),
            )
            .await
        {
            restored.push(c.turn_id);
        }
    }
    assert_eq!(restored, turns[1..]);
    assert_eq!(
        std::fs::read_to_string(h.ws.join("notes.txt")).unwrap(),
        "v1\n"
    );
    assert!(h.ws.join("f1.txt").exists());
    assert!(!h.ws.join("f2.txt").exists() && !h.ws.join("f3.txt").exists());
}

/// The log is written first (PLAN.md §2): when the store cannot append, the
/// tool does not run and the turn ends, even when the events that end it
/// cannot be written either. The store is broken from outside, as a full
/// disk would break it: a trigger on the real database refuses every event
/// from the tool's start on. Once it is fixed, the session works again.
#[tokio::test]
async fn a_tool_does_not_run_when_its_event_cannot_be_written() {
    let mut h = Harness::new(|root| NodePolicy {
        workspace_roots: vec![root.into()],
        max_approval_mode: harness_core::policy::ApprovalMode::Auto,
        ..Default::default()
    })
    .await;
    let sid = h.create().await;
    let db = rusqlite::Connection::open(h._dir.path().join("node.db")).unwrap();
    db.busy_timeout(Duration::from_secs(5)).unwrap();
    db.execute_batch(
        "CREATE TRIGGER break_log BEFORE INSERT ON events
         WHEN NEW.kind NOT IN ('turn_started', 'user_message', 'checkpoint', 'assistant_delta', 'assistant_message')
         BEGIN SELECT RAISE(ABORT, 'database or disk is full'); END;",
    )
    .unwrap();
    let marker = h.ws.join("marker");
    let prompt = |h: &Harness, id: &str, text: String| {
        h.send(MasterToNode::Prompt {
            savings: None,
            context: None,
            reply: None,
            agent: None,
            attachments: vec![],
            compare: vec![],
            req_id: id.into(),
            session_id: sid,
            text,
            images: vec![],
        })
    };
    // The ack for `id`, collecting events on the way.
    async fn ack(h: &mut Harness, id: &str) -> Result<(), String> {
        loop {
            let msg = tokio::time::timeout(Duration::from_secs(10), h.link.from_node.recv())
                .await
                .expect("timed out waiting for ack")
                .expect("link closed");
            match msg {
                NodeToMaster::Event { event } => h.seen.push(event),
                NodeToMaster::Ack {
                    req_id, ok, error, ..
                } if req_id == id => {
                    return if ok {
                        Ok(())
                    } else {
                        Err(error.unwrap_or_default())
                    };
                }
                _ => {}
            }
        }
    }
    prompt(&h, "p1", format!("run: touch {}", marker.display()));
    ack(&mut h, "p1")
        .await
        .expect("the turn starts: its first events are written");
    h.until(|k| matches!(k, EventKind::AssistantMessage(_)))
        .await;
    // The turn is over once the session takes a prompt again; while the log
    // is broken that prompt is refused for the log, not for a running turn.
    let mut refused = Ok(());
    for i in 0..100 {
        let id = format!("q{i}");
        prompt(&h, &id, "hello".into());
        match ack(&mut h, &id).await {
            Err(e) if e.contains("already running") => {
                tokio::time::sleep(Duration::from_millis(50)).await
            }
            other => {
                refused = other;
                break;
            }
        }
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !marker.exists(),
        "the tool ran although its start was not logged"
    );
    let refused = refused.expect_err("a broken log must refuse the prompt");
    assert!(refused.contains("log could not be written"), "{refused}");

    // Fixed: the session closes the stopped turn and runs the next one.
    db.execute_batch("DROP TRIGGER break_log;").unwrap();
    let mark = h.seen.len();
    prompt(&h, "p2", "hello".into());
    ack(&mut h, "p2")
        .await
        .expect("works again once the store does");
    h.until(|k| matches!(k, EventKind::TurnEnded(t) if t.reason == TurnEndReason::Completed))
        .await;
    let ends: Vec<TurnEndReason> = h.seen[mark..]
        .iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::TurnEnded(t)) => Some(t.reason),
            _ => None,
        })
        .collect();
    assert_eq!(
        ends,
        [TurnEndReason::Error, TurnEndReason::Completed],
        "the stopped turn is closed first"
    );
    assert!(!marker.exists());
    let stored = h.node.store().events_after(sid, 0, 1000).unwrap();
    let v = SessionView::fold(&stored).unwrap();
    assert!(v.running_turn.is_none());
    assert!(stored.iter().all(|e| e.kind != "tool_call_started"));
    assert!(stored.iter().any(|e| matches!(e.typed(),
        Ok(EventKind::Error(m)) if m.message.contains("log could not be written"))));
}

/// An approval whose answer could not be written is closed with its turn:
/// the turn's end denies it, so the log never shows it pending after the
/// turn ended (a transient failure: only the person's answer is refused).
#[tokio::test]
async fn an_approval_whose_answer_was_not_written_is_closed_with_the_turn() {
    let mut h = Harness::new(default_policy).await;
    let sid = h.create().await;
    let db = rusqlite::Connection::open(h._dir.path().join("node.db")).unwrap();
    db.busy_timeout(Duration::from_secs(5)).unwrap();
    db.execute_batch(
        "CREATE TRIGGER break_log BEFORE INSERT ON events
         WHEN NEW.kind = 'approval_resolved' AND json_extract(NEW.payload, '$.by') = 'user'
         BEGIN SELECT RAISE(ABORT, 'database or disk is full'); END;",
    )
    .unwrap();
    h.send(MasterToNode::Prompt {
        savings: None,
        context: None,
        reply: None,
        agent: None,
        attachments: vec![],
        compare: vec![],
        req_id: "p".into(),
        session_id: sid,
        text: "run: touch out.txt".into(),
        images: vec![],
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
    h.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
    assert!(
        !h.ws.join("out.txt").exists(),
        "an approval the log does not hold allows nothing"
    );
    let stored = h.node.store().events_after(sid, 0, 1000).unwrap();
    let v = SessionView::fold(&stored).unwrap();
    assert!(v.running_turn.is_none());
    assert!(
        v.pending_approvals.is_empty(),
        "turn ended but the fold still shows a pending approval: {:?}",
        v.pending_approvals
    );
}

fn auto_policy(root: &std::path::Path) -> NodePolicy {
    NodePolicy {
        workspace_roots: vec![root.into()],
        max_approval_mode: harness_core::policy::ApprovalMode::Auto,
        ..Default::default()
    }
}

/// Every file under `dir` but `.git`, with its bytes.
fn tree(dir: &std::path::Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut out: Vec<_> = walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| e.file_name() != ".git")
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| (e.path().to_path_buf(), std::fs::read(e.path()).unwrap()))
        .collect();
    out.sort();
    out
}

fn agent(
    id: &str,
    delegates: Vec<harness_core::agent::AgentSpec>,
) -> harness_core::agent::AgentSpec {
    harness_core::agent::AgentSpec {
        agent_id: id.into(),
        name: id.into(),
        system_prompt: "work".into(),
        tools: harness_core::agent::ToolSelection::List(vec!["bash".into()]),
        model: None,
        fallbacks: vec![],
        unavailable: vec![],
        no_effort: vec![],
        effort: None,
        budget: Default::default(),
        ui: Default::default(),
        prompt_hash: "h".into(),
        deny_tools: vec![],
        delegates,
        caps: None,
    }
}

/// A writing sub-agent works in its own worktree; when its log fails, its
/// changes are not merged into the user's folder (nothing would record the
/// merge), and the parent is told why the sub-agent stopped.
#[tokio::test]
async fn a_sub_agent_whose_log_failed_changes_nothing_in_the_folder() {
    let mut h = Harness::new(auto_policy).await;
    let git = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&h.ws)
                .args(args)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .status()
                .unwrap()
                .success()
        )
    };
    git(&["init", "-q"]);
    std::fs::write(h.ws.join("keep.txt"), "v1\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "init"]);
    let sid = h.create().await;
    let before = tree(&h.ws);
    // Only the parent's log works: the child's tool result is refused.
    let db = rusqlite::Connection::open(h._dir.path().join("node.db")).unwrap();
    db.busy_timeout(Duration::from_secs(5)).unwrap();
    db.execute_batch(&format!(
        "CREATE TRIGGER break_log BEFORE INSERT ON events
         WHEN NEW.kind = 'tool_call_finished' AND NEW.session_id <> '{sid}'
         BEGIN SELECT RAISE(ABORT, 'database or disk is full'); END;"
    ))
    .unwrap();
    h.send(MasterToNode::Prompt {
        savings: None,
        context: None,
        reply: None,
        agent: Some(agent("lead", vec![agent("dev", vec![])])),
        attachments: vec![],
        compare: vec![],
        req_id: "p".into(),
        session_id: sid,
        text: "delegate: dev=run: echo v2 > keep.txt; echo new > made.txt".into(),
        images: vec![],
    });
    let EventKind::ToolCallFinished(result) = h
        .until(|k| matches!(k, EventKind::ToolCallFinished(f) if f.output.contains("@dev")))
        .await
    else {
        unreachable!()
    };
    h.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
    assert_eq!(tree(&h.ws), before, "the folder changed");
    assert!(
        result.output.contains("log could not be written") && result.output.contains("not applied"),
        "{}",
        result.output
    );
}

/// A failed append while bash runs stops it: its process tree is gone
/// within the grace period.
#[tokio::test]
async fn a_failed_append_kills_the_running_command() {
    let mut h = Harness::new(auto_policy).await;
    let sid = h.create().await;
    let db = rusqlite::Connection::open(h._dir.path().join("node.db")).unwrap();
    db.busy_timeout(Duration::from_secs(5)).unwrap();
    db.execute_batch(
        "CREATE TRIGGER break_log BEFORE INSERT ON events
         WHEN NEW.kind = 'tool_output_delta'
         BEGIN SELECT RAISE(ABORT, 'database or disk is full'); END;",
    )
    .unwrap();
    let pidfile = h.ws.join("pid");
    h.send(MasterToNode::Prompt {
        savings: None,
        context: None,
        reply: None,
        agent: None,
        attachments: vec![],
        compare: vec![],
        req_id: "p".into(),
        session_id: sid,
        // Output only once the pid is written, so the failing delta comes
        // while the child runs.
        text: format!(
            "run: (sleep 1000) & echo $! > {}; sleep 0.2; echo hi; wait",
            pidfile.display()
        ),
        images: vec![],
    });
    let EventKind::TurnEnded(t) = h.until(|k| matches!(k, EventKind::TurnEnded(_))).await else {
        unreachable!()
    };
    assert_eq!(t.reason, TurnEndReason::Error);
    let pid: i32 = std::fs::read_to_string(&pidfile)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let alive = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .map(|s| {
            !s.rsplit(')')
                .next()
                .unwrap_or("")
                .trim_start()
                .starts_with('Z')
        })
        .unwrap_or(false);
    assert!(!alive, "child {pid} survived the failed append");
}

/// A turn whose end could not be written, then a node restart: the
/// restart closes it, exactly once.
#[tokio::test]
async fn an_unclosed_turn_is_closed_once_by_a_restart() {
    let mut h = Harness::new(auto_policy).await;
    let sid = h.create().await;
    let path = h._dir.path().join("node.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.busy_timeout(Duration::from_secs(5)).unwrap();
    db.execute_batch(
        "CREATE TRIGGER break_log BEFORE INSERT ON events
         WHEN NEW.kind NOT IN ('turn_started', 'user_message', 'checkpoint', 'assistant_delta', 'assistant_message')
         BEGIN SELECT RAISE(ABORT, 'database or disk is full'); END;",
    )
    .unwrap();
    h.send(MasterToNode::Prompt {
        savings: None,
        context: None,
        reply: None,
        agent: None,
        attachments: vec![],
        compare: vec![],
        req_id: "p".into(),
        session_id: sid,
        text: "run: true".into(),
        images: vec![],
    });
    h.until(|k| matches!(k, EventKind::AssistantMessage(_)))
        .await;
    // Let the turn give up on its end.
    tokio::time::sleep(Duration::from_millis(500)).await;
    db.execute_batch("DROP TRIGGER break_log;").unwrap();
    let Harness {
        node, link, _dir, ..
    } = h;
    drop(link);
    drop(node);
    let store = Store::open(&path, Durability::Full).unwrap();
    let _node = NodeRuntime::start(config(store.clone(), auto_policy(_dir.path())))
        .await
        .unwrap();
    let stored = store.events_after(sid, 0, 1000).unwrap();
    let ends: Vec<&str> = stored.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(
        ends.iter().filter(|k| **k == "turn_ended").count(),
        1,
        "{ends:?}"
    );
    assert!(SessionView::fold(&stored).unwrap().running_turn.is_none());
}

/// How long a prompt's ack takes on a long session: it reads the log and
/// writes two fsynced events first. A measurement, not a check:
/// `cargo test -p harness-node --test runtime ack_time -- --ignored --nocapture`.
#[tokio::test]
#[ignore]
async fn ack_time_on_a_long_session() {
    for n in [10_000u64, 100_000] {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir(&ws).unwrap();
        let path = dir.path().join("node.db");
        let sid = SessionId::new();
        {
            let store = Store::open(&path, Durability::Normal).unwrap();
            let created = EventKind::SessionCreated(SessionCreated {
                profile: harness_core::event::Profile::Agentic,
                agent_id: None,
                incognito: false,
                parent_session: None,
                title: "t".into(),
                cwd: ws.display().to_string(),
                model: "mock".into(),
                backend: "native".into(),
                node_id: "local".into(),
                approval_mode: Default::default(),
            });
            for seq in 1..=n {
                let kind = if seq == 1 {
                    created.clone()
                } else {
                    EventKind::Error(ErrorEvent {
                        turn_id: None,
                        message: "x".repeat(200),
                    })
                };
                let (kind, payload) = kind.into_parts();
                store
                    .append(
                        "local",
                        &RawEvent {
                            session_id: sid,
                            seq,
                            ts_ms: 0,
                            kind,
                            payload,
                        },
                    )
                    .unwrap();
            }
        }
        let store = Store::open(&path, Durability::Full).unwrap();
        let node = NodeRuntime::start(config(store, auto_policy(dir.path())))
            .await
            .unwrap();
        let mut link = node.attach().await;
        let _hello = link.from_node.recv().await;
        let t = std::time::Instant::now();
        link.to_node
            .try_send(MasterToNode::Prompt {
                savings: None,
                context: None,
                reply: None,
                agent: None,
                attachments: vec![],
                compare: vec![],
                req_id: "p".into(),
                session_id: sid,
                text: "hello".into(),
                images: vec![],
            })
            .unwrap();
        loop {
            if let Some(NodeToMaster::Ack { req_id, ok, .. }) = link.from_node.recv().await
                && req_id == "p"
            {
                assert!(ok);
                break;
            }
        }
        println!("ack after {n} events: {:?}", t.elapsed());
    }
}

// ---- "allow for this turn" covers the exact call (ADOPTION_PLAN A.2) ------

/// Calls what the user (or a parent's task) lists, one per request: the
/// message holds a JSON array of `[tool, input]`; then it answers "done".
struct Script;

#[async_trait::async_trait]
impl harness_agent_native::Provider for Script {
    fn name(&self) -> &'static str {
        "script"
    }
    async fn complete(
        &self,
        req: &harness_agent_native::ChatRequest,
        _deltas: tokio::sync::mpsc::UnboundedSender<String>,
    ) -> Result<harness_agent_native::Completion, harness_agent_native::ProviderError> {
        use harness_agent_native::ChatMsg;
        let since = req
            .messages
            .iter()
            .rposition(|m| m.user_text().is_some())
            .unwrap_or(0);
        let user = req.messages[since].user_text().unwrap_or("");
        let calls: Vec<(String, serde_json::Value)> = user
            .find('[')
            .zip(user.rfind(']'))
            .and_then(|(a, b)| serde_json::from_str(&user[a..=b]).ok())
            .unwrap_or_default();
        let done = req.messages[since + 1..]
            .iter()
            .filter(|m| matches!(m, ChatMsg::Tool { .. }))
            .count();
        let tool_calls = calls
            .get(done)
            .map(|(tool, input)| ToolCallRef {
                call_id: harness_core::new_id(),
                tool: tool.clone(),
                input: input.clone(),
            })
            .into_iter()
            .collect();
        Ok(harness_agent_native::Completion {
            text: "done".into(),
            tool_calls,
            usage: None,
        })
    }
}

impl Harness {
    /// Send `text` as a prompt to `sid` (as `agent`, if given), answer every
    /// approval of any session with `answer(n)` (n counts from 0), until
    /// `sid`'s turn ends. Returns each request with its session.
    async fn turn_answering(
        &mut self,
        sid: SessionId,
        agent: Option<harness_core::agent::AgentSpec>,
        text: String,
        answer: impl Fn(usize) -> Decision,
    ) -> Vec<(SessionId, ApprovalRequested)> {
        self.send(MasterToNode::Prompt {
            savings: None,
            context: None,
            reply: None,
            agent,
            attachments: vec![],
            compare: vec![],
            req_id: "p".into(),
            session_id: sid,
            text,
            images: vec![],
        });
        let mut asked = vec![];
        loop {
            let k = self
                .until(|k| matches!(k, EventKind::ApprovalRequested(_) | EventKind::TurnEnded(_)))
                .await;
            let session = self.seen.last().unwrap().session_id;
            match k {
                EventKind::ApprovalRequested(a) => {
                    // A policy denial is resolved by the node at once; an
                    // answer to it would only be refused.
                    if !a.reason.contains("denied by this node's policy") {
                        self.send(MasterToNode::ApprovalRespond {
                            req_id: "a".into(),
                            session_id: session,
                            approval_id: a.approval_id.clone(),
                            decision: answer(asked.len()),
                        });
                    }
                    asked.push((session, a));
                }
                EventKind::TurnEnded(_) if session == sid => return asked,
                _ => {}
            }
        }
    }
}

fn bash(cmd: &str) -> serde_json::Value {
    serde_json::json!(["bash", {"command": cmd}])
}

fn commands(asked: &[(SessionId, ApprovalRequested)]) -> Vec<&str> {
    asked
        .iter()
        .map(|(_, a)| a.input["command"].as_str().unwrap_or(""))
        .collect()
}

fn resolved(h: &Harness) -> Vec<(Decision, ResolvedBy)> {
    h.seen
        .iter()
        .filter_map(|e| match e.typed().unwrap() {
            EventKind::ApprovalResolved(r) => Some((r.decision, r.by)),
            _ => None,
        })
        .collect()
}

/// "Allow for this turn" (`allow_session` on the wire) covers the exact
/// call on the card the person answers, so it is offered and honoured on
/// the first request for that call (ADOPTION_PLAN A.2).
#[tokio::test]
async fn allow_for_this_turn_is_honoured_on_the_first_approval() {
    let mut h = Harness::with_provider(default_policy, Arc::new(Script)).await;
    let sid = h.create().await;
    let calls = serde_json::json!([bash("true"), bash("true"), bash("true")]);
    let asked = h
        .turn_answering(sid, None, calls.to_string(), |_| Decision::AllowSession)
        .await;
    assert_eq!(commands(&asked), ["true"]);
    assert!(asked[0].1.turn_allow, "the first request offers it");
    assert_eq!(resolved(&h), [(Decision::AllowSession, ResolvedBy::User)]);
}

/// A single "allow" covers that one call only.
#[tokio::test]
async fn a_single_allow_does_not_cover_the_next_identical_call() {
    let mut h = Harness::with_provider(default_policy, Arc::new(Script)).await;
    let sid = h.create().await;
    let calls = serde_json::json!([bash("ls"), bash("ls")]);
    let asked = h
        .turn_answering(sid, None, calls.to_string(), |_| Decision::Allow)
        .await;
    assert_eq!(commands(&asked), ["ls", "ls"]);
}

/// Approving `ls` "for this turn" approves `ls`, not `bash`: a later
/// `rm -rf x` in the same turn is asked about.
#[tokio::test]
async fn allow_for_this_turn_does_not_cover_another_command() {
    let mut h = Harness::with_provider(default_policy, Arc::new(Script)).await;
    std::fs::create_dir(h.ws.join("x")).unwrap();
    let sid = h.create().await;
    let calls = serde_json::json!([bash("ls"), bash("ls"), bash("rm -rf x")]);
    let asked = h
        .turn_answering(sid, None, calls.to_string(), |n| {
            if n == 0 {
                Decision::AllowSession
            } else {
                Decision::Deny
            }
        })
        .await;
    assert_eq!(commands(&asked), ["ls", "rm -rf x"]);
    assert!(h.ws.join("x").exists(), "rm -rf x ran");
}

/// The same command twice after "for this turn" is asked once; the next
/// turn asks again.
#[tokio::test]
async fn allow_for_this_turn_covers_the_same_command_until_the_turn_ends() {
    let mut h = Harness::with_provider(default_policy, Arc::new(Script)).await;
    let sid = h.create().await;
    let calls = serde_json::json!([bash("ls"), bash("ls")]);
    let asked = h
        .turn_answering(sid, None, calls.to_string(), |_| Decision::AllowSession)
        .await;
    assert_eq!(commands(&asked), ["ls"]);

    let asked = h
        .turn_answering(sid, None, calls.to_string(), |_| Decision::AllowSession)
        .await;
    assert_eq!(commands(&asked), ["ls"], "the allowance outlived its turn");
}

/// A call `node.toml` denies is refused every time, whatever a client
/// answers: no allowance can cover it.
#[tokio::test]
async fn a_policy_denied_call_cannot_be_allowed_for_the_turn() {
    let mut h = Harness::with_provider(
        |root| NodePolicy {
            workspace_roots: vec![root.into()],
            denied_tools: vec!["bash".into()],
            ..Default::default()
        },
        Arc::new(Script),
    )
    .await;
    let sid = h.create().await;
    let calls = serde_json::json!([bash("touch nope"), bash("touch nope")]);
    let asked = h
        .turn_answering(sid, None, calls.to_string(), |_| Decision::AllowSession)
        .await;
    assert_eq!(commands(&asked), ["touch nope", "touch nope"]);
    assert!(asked.iter().all(|(_, a)| !a.turn_allow));
    assert_eq!(
        resolved(&h),
        [
            (Decision::Deny, ResolvedBy::Policy),
            (Decision::Deny, ResolvedBy::Policy)
        ]
    );
    assert!(!h.ws.join("nope").exists());
}

/// A sub-agent started in the turn does not inherit the parent's allowance.
#[tokio::test]
async fn a_sub_agent_does_not_inherit_allow_for_this_turn() {
    let mut h = Harness::with_provider(default_policy, Arc::new(Script)).await;
    let sid = h.create().await;
    let task = serde_json::json!([bash("ls")]).to_string();
    let calls = serde_json::json!([
        bash("ls"),
        bash("ls"),
        ["delegate", {"agent": "dev", "task": task}],
    ]);
    // The parent's second `ls` is covered; the sub-agent's is asked.
    let asked = h
        .turn_answering(
            sid,
            Some(agent("lead", vec![agent("dev", vec![])])),
            calls.to_string(),
            |_| Decision::AllowSession,
        )
        .await;
    assert_eq!(commands(&asked), ["ls", "ls"], "{asked:?}");
    assert_eq!(asked[0].0, sid);
    assert_ne!(asked[1].0, sid, "the second request is the sub-agent's");
}
