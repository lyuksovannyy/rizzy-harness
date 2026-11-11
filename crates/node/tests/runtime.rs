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
        context_windows: Default::default(),
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

/// `git -C dir args...`, with an identity; panics unless it succeeds.
fn git_in(dir: &std::path::Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A repository at `dir` with one committed file, `keep.txt`.
fn init_repo(dir: &std::path::Path) {
    git_in(dir, &["init", "-q"]);
    std::fs::write(dir.join("keep.txt"), "v1\n").unwrap();
    git_in(dir, &["add", "."]);
    git_in(dir, &["commit", "-qm", "init"]);
}

fn agent_with(
    id: &str,
    tools: &[&str],
    delegates: Vec<harness_core::agent::AgentSpec>,
) -> harness_core::agent::AgentSpec {
    harness_core::agent::AgentSpec {
        tools: harness_core::agent::ToolSelection::List(
            tools.iter().map(|t| t.to_string()).collect(),
        ),
        ..agent(id, delegates)
    }
}

/// Prompt `sid` as `lead` (who may delegate to `delegates`) and wait for
/// the parent's turn to end; returns the outputs of its `delegate` calls.
async fn delegate_as_lead(
    h: &mut Harness,
    sid: SessionId,
    delegates: Vec<harness_core::agent::AgentSpec>,
    text: &str,
) -> Vec<String> {
    h.send(MasterToNode::Prompt {
        savings: None,
        context: None,
        reply: None,
        agent: Some(agent("lead", delegates)),
        attachments: vec![],
        compare: vec![],
        req_id: "p".into(),
        session_id: sid,
        text: text.into(),
        images: vec![],
    });
    loop {
        h.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
        if h.seen.last().is_some_and(|e| e.session_id == sid) {
            break;
        }
    }
    let stored = h.node.store().events_after(sid, 0, 10_000).unwrap();
    let calls: Vec<String> = stored
        .iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::ToolCallStarted(c)) if c.tool == "delegate" => Some(c.call_id),
            _ => None,
        })
        .collect();
    stored
        .iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::ToolCallFinished(f)) if calls.contains(&f.call_id) => Some(f.output),
            _ => None,
        })
        .collect()
}

/// A sub-agent that can change files gets its own worktree or is not
/// started: in a folder that is not a repository the delegation is refused
/// with the reason, no child session exists and nothing in the folder moved.
#[tokio::test]
async fn a_writing_sub_agent_is_refused_outside_a_repository() {
    let mut h = Harness::new(auto_policy).await;
    std::fs::write(h.ws.join("keep.txt"), "v1\n").unwrap();
    let sid = h.create().await;
    let before = tree(&h.ws);
    for tools in [&["bash"][..], &["write"], &["edit"]] {
        let out = delegate_as_lead(
            &mut h,
            sid,
            vec![agent_with("dev", tools, vec![])],
            "delegate: dev=run: echo v2 > keep.txt; echo new > made.txt",
        )
        .await;
        let out = out.last().expect("a delegate call");
        assert!(
            out.contains("not started") && out.contains("not a git repository"),
            "{tools:?}: {out}"
        );
        assert!(out.contains("git init"), "{out}");
    }
    assert_eq!(tree(&h.ws), before, "the folder changed");
    let sessions = h.node.store().sessions().unwrap();
    assert_eq!(sessions.len(), 1, "a child session was created");
}

/// The same when the repository is there but its worktree cannot be made:
/// here the folder for worktrees (`.git/harness-worktrees`) is a file.
#[tokio::test]
async fn a_writing_sub_agent_is_refused_when_its_worktree_cannot_be_made() {
    let mut h = Harness::new(auto_policy).await;
    init_repo(&h.ws);
    std::fs::write(h.ws.join(".git/harness-worktrees"), "in the way\n").unwrap();
    let sid = h.create().await;
    let before = tree(&h.ws);
    let out = delegate_as_lead(
        &mut h,
        sid,
        vec![agent("dev", vec![])],
        "delegate: dev=run: echo v2 > keep.txt; echo new > made.txt",
    )
    .await;
    let out = out.last().expect("a delegate call");
    assert!(
        out.contains("not started") && out.contains("worktree could not be made"),
        "{out}"
    );
    assert_eq!(tree(&h.ws), before, "the folder changed");
    assert_eq!(h.node.store().sessions().unwrap().len(), 1);
    assert_eq!(
        git_in(&h.ws, &["for-each-ref", "refs/harness"]),
        "",
        "a ref was left"
    );
    assert_eq!(
        git_in(&h.ws, &["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        1
    );
}

/// A sub-agent that cannot change files needs no worktree: it still runs
/// in a folder that is not a repository.
#[tokio::test]
async fn a_read_only_sub_agent_runs_outside_a_repository() {
    let mut h = Harness::new(auto_policy).await;
    std::fs::write(h.ws.join("keep.txt"), "hello from the folder\n").unwrap();
    let sid = h.create().await;
    let out = delegate_as_lead(
        &mut h,
        sid,
        vec![agent_with("scout", &["read", "grep", "glob"], vec![])],
        "delegate: scout=read: keep.txt",
    )
    .await;
    let out = out.last().expect("a delegate call");
    assert!(out.contains("@scout ended (completed)"), "{out}");
    assert!(out.contains("hello from the folder"), "{out}");
    assert_eq!(h.node.store().sessions().unwrap().len(), 2);
}

/// In a repository a writing sub-agent works in its worktree and its
/// changes are applied to the folder afterwards.
#[tokio::test]
async fn a_writing_sub_agent_changes_are_applied_from_its_worktree() {
    let mut h = Harness::new(auto_policy).await;
    init_repo(&h.ws);
    let sid = h.create().await;
    let out = delegate_as_lead(
        &mut h,
        sid,
        vec![agent("dev", vec![])],
        "delegate: dev=run: pwd; echo v2 > keep.txt; echo new > made.txt",
    )
    .await;
    let out = out.last().expect("a delegate call");
    assert!(
        out.contains("harness-worktrees"),
        "not in a worktree: {out}"
    );
    assert!(out.contains("2 changed file(s) applied"), "{out}");
    assert_eq!(
        std::fs::read_to_string(h.ws.join("keep.txt")).unwrap(),
        "v2\n"
    );
    assert_eq!(
        std::fs::read_to_string(h.ws.join("made.txt")).unwrap(),
        "new\n"
    );
    assert_eq!(git_in(&h.ws, &["worktree", "list"]).lines().count(), 1);
}

/// Two writing sub-agents delegated in one step do not see each other's
/// uncommitted files while they run; both results are applied.
#[tokio::test]
async fn parallel_writing_sub_agents_do_not_see_each_other() {
    let mut h = Harness::new(auto_policy).await;
    init_repo(&h.ws);
    let sid = h.create().await;
    let task = |me: &str, other: &str| {
        format!(
            "{me}=run: echo {me} > {me}.txt; sleep 1; \
             if test -e {other}.txt; then echo SAW-{other}; else echo ALONE-{me}; fi"
        )
    };
    let out = delegate_as_lead(
        &mut h,
        sid,
        vec![agent("a", vec![]), agent("b", vec![])],
        &format!("delegate: {} | {}", task("a", "b"), task("b", "a")),
    )
    .await;
    assert_eq!(out.len(), 2, "{out:?}");
    assert!(out.iter().any(|o| o.contains("ALONE-a")), "{out:?}");
    assert!(out.iter().any(|o| o.contains("ALONE-b")), "{out:?}");
    assert!(out.iter().all(|o| !o.contains("SAW-")), "{out:?}");
    assert_eq!(std::fs::read_to_string(h.ws.join("a.txt")).unwrap(), "a\n");
    assert_eq!(std::fs::read_to_string(h.ws.join("b.txt")).unwrap(), "b\n");
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
            stop: Default::default(),
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

// ---- A restore first saves the folder as it is (PLAN.md T33) ----

/// A checkpoint's phase as the log spells it.
fn phase(c: &Checkpoint) -> String {
    serde_json::to_value(c.phase)
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned()
}

fn run_git(dir: &std::path::Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8(out.stdout).unwrap()
}

/// The ack for `id`, collecting the events on the way.
async fn ack_for(h: &mut Harness, id: &str) -> Result<(), String> {
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

async fn restore(
    h: &mut Harness,
    sid: SessionId,
    id: &str,
    turn: &str,
    force: bool,
) -> Result<(), String> {
    h.send(MasterToNode::Restore {
        req_id: id.into(),
        session_id: sid,
        turn_id: turn.into(),
        force,
        rewind: false,
    });
    ack_for(h, id).await
}

async fn prompt_turn(h: &mut Harness, sid: SessionId, text: &str) -> String {
    h.send(MasterToNode::Prompt {
        savings: None,
        context: None,
        reply: None,
        agent: None,
        attachments: vec![],
        compare: vec![],
        req_id: "p".into(),
        session_id: sid,
        text: text.into(),
        images: vec![],
    });
    let EventKind::Checkpoint(after) = h
        .until(|k| matches!(k, EventKind::Checkpoint(c) if c.phase == CheckpointPhase::After))
        .await
    else {
        unreachable!()
    };
    h.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
    after.turn_id
}

/// A git repository, one turn that edits `keep.txt` and creates `made.txt`,
/// then the user's own work between turns: an edit to the tracked file,
/// their own `made.txt` and a new untracked binary file.
async fn turn_then_user_work() -> (Harness, SessionId, String) {
    let mut h = Harness::new(auto_policy).await;
    run_git(&h.ws, &["init", "-q"]);
    std::fs::write(h.ws.join("keep.txt"), "v1\n").unwrap();
    run_git(&h.ws, &["add", "."]);
    run_git(
        &h.ws,
        &["-c", "commit.gpgSign=false", "commit", "-qm", "init"],
    );
    let sid = h.create().await;
    let turn = prompt_turn(&mut h, sid, "run: echo v2 > keep.txt; echo new > made.txt").await;
    std::fs::write(h.ws.join("keep.txt"), "the user's edit\n").unwrap();
    std::fs::write(h.ws.join("made.txt"), "the user's own version\n").unwrap();
    std::fs::write(h.ws.join("extra.bin"), [0u8, 159, 255, 10, 13]).unwrap();
    (h, sid, turn)
}

fn pre_restore_snapshots(seen: &[RawEvent]) -> Vec<Checkpoint> {
    seen.iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::Checkpoint(c)) if phase(&c) == "before_restore" => Some(c),
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
    // A sub-agent that can change files needs a repository for its worktree.
    init_repo(&h.ws);
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

/// A forced restore overwrites work no turn checkpoint holds; it saves the
/// folder first, and restoring to that snapshot brings the work back.
#[tokio::test]
async fn a_forced_restore_can_be_undone() {
    let (mut h, sid, turn) = turn_then_user_work().await;
    let mine = tree(&h.ws);
    let mark = h.seen.len();
    restore(&mut h, sid, "r1", &turn, false)
        .await
        .expect_err("an unforced restore over the user's edits is refused");
    assert_eq!(tree(&h.ws), mine);
    assert!(
        pre_restore_snapshots(&h.seen[mark..]).is_empty(),
        "a refused restore records nothing"
    );

    restore(&mut h, sid, "r2", &turn, true).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(h.ws.join("keep.txt")).unwrap(),
        "v1\n"
    );
    assert!(!h.ws.join("made.txt").exists());
    let pre = pre_restore_snapshots(&h.seen[mark..]);
    assert_eq!(pre.len(), 1, "one snapshot before the restore");
    // It is in the log before the restore's own events.
    let phases: Vec<String> = h.seen[mark..]
        .iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::Checkpoint(c)) => Some(phase(&c)),
            _ => None,
        })
        .collect();
    assert_eq!(
        phases.first().map(String::as_str),
        Some("before_restore"),
        "{phases:?}"
    );

    // Undo the restore: an ordinary restore of the snapshot's checkpoint.
    restore(&mut h, sid, "r3", &pre[0].turn_id, false)
        .await
        .unwrap();
    assert_eq!(tree(&h.ws), mine, "the user's work is back, byte for byte");
}

/// The snapshot's event is written before any file changes: when it
/// cannot be appended, nothing is restored and the reason comes back.
#[tokio::test]
async fn a_restore_whose_snapshot_cannot_be_logged_changes_nothing() {
    let (mut h, sid, turn) = turn_then_user_work().await;
    let mine = tree(&h.ws);
    let db = rusqlite::Connection::open(h._dir.path().join("node.db")).unwrap();
    db.busy_timeout(Duration::from_secs(5)).unwrap();
    db.execute_batch(
        "CREATE TRIGGER break_log BEFORE INSERT ON events
         WHEN NEW.kind = 'checkpoint' AND json_extract(NEW.payload, '$.phase') = 'before_restore'
         BEGIN SELECT RAISE(ABORT, 'database or disk is full'); END;",
    )
    .unwrap();
    let err = restore(&mut h, sid, "r1", &turn, true)
        .await
        .expect_err("a restore whose snapshot is not in the log must not run");
    assert!(err.contains("log could not be written"), "{err}");
    assert!(err.contains("nothing was restored"), "{err}");
    assert_eq!(tree(&h.ws), mine, "the folder changed");
    let stored = h.node.store().events_after(sid, 0, 1000).unwrap();
    assert!(
        !stored.iter().any(|e| matches!(e.typed(),
            Ok(EventKind::Checkpoint(c)) if c.phase == CheckpointPhase::Restored)),
        "a restore was recorded"
    );

    // Fixed: the same restore runs.
    db.execute_batch("DROP TRIGGER break_log;").unwrap();
    restore(&mut h, sid, "r2", &turn, true).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(h.ws.join("keep.txt")).unwrap(),
        "v1\n"
    );
}

/// When the folder cannot be saved (here a file the node cannot read, so
/// `git add -A` fails), the restore is refused with the reason and the
/// folder is left as it is.
#[cfg(unix)]
#[tokio::test]
async fn a_restore_whose_snapshot_fails_changes_nothing() {
    use std::os::unix::fs::PermissionsExt;
    let (mut h, sid, turn) = turn_then_user_work().await;
    let locked = h.ws.join("locked.txt");
    std::fs::write(&locked, "private\n").unwrap();
    let mine = tree(&h.ws);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&locked).is_ok() {
        eprintln!("skipped: this user can read a mode 000 file (root?)");
        return;
    }
    let res = restore(&mut h, sid, "r1", &turn, true).await;
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
    let err = res.expect_err("a restore that could not save the folder must not run");
    assert!(err.contains("nothing was restored"), "{err}");
    assert!(
        err.contains("locked.txt"),
        "the reason names the file: {err}"
    );
    assert_eq!(tree(&h.ws), mine, "the folder changed");
}

/// Pre-restore snapshots are checkpoints like a turn's: each counts as one
/// against `checkpoints_keep`, the oldest go, the newest stays usable.
#[tokio::test]
async fn pre_restore_snapshots_obey_checkpoints_keep() {
    let mut h = Harness::new(|root| NodePolicy {
        workspace_roots: vec![root.into()],
        max_approval_mode: harness_core::policy::ApprovalMode::Auto,
        checkpoints_keep: 2,
        ..Default::default()
    })
    .await;
    std::fs::write(h.ws.join("notes.txt"), "v0\n").unwrap();
    let sid = h.create().await;
    let turn = prompt_turn(&mut h, sid, "run: echo v1 > notes.txt").await;
    // Undo the turn, undo that undo, and so on: three restores.
    let mut target = turn;
    let mut snaps = vec![];
    for i in 0..3 {
        let mark = h.seen.len();
        restore(&mut h, sid, &format!("r{i}"), &target, false)
            .await
            .unwrap();
        let pre = pre_restore_snapshots(&h.seen[mark..]);
        assert_eq!(pre.len(), 1);
        target = pre[0].turn_id.clone();
        snaps.push(pre[0].clone());
    }
    assert_eq!(
        std::fs::read_to_string(h.ws.join("notes.txt")).unwrap(),
        "v0\n"
    );
    let shadow = std::fs::read_dir(h._dir.path().join("checkpoints"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|x| x == "git"))
        .expect("a shadow repository");
    let refs = run_git(
        &h.ws,
        &[
            "--git-dir",
            &shadow.display().to_string(),
            "for-each-ref",
            "--format=%(refname)",
            "refs/harness/",
        ],
    );
    let groups: std::collections::BTreeSet<String> = refs
        .lines()
        .map(|r| r.rsplit_once('/').unwrap().0.to_string())
        .collect();
    let want: std::collections::BTreeSet<String> = snaps[1..]
        .iter()
        .map(|c| format!("refs/harness/{sid}/{}", c.turn_id))
        .collect();
    assert_eq!(groups, want, "only the two newest checkpoints are kept");
    // The newest still undoes.
    restore(&mut h, sid, "last", &snaps[2].turn_id, false)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(h.ws.join("notes.txt")).unwrap(),
        "v1\n"
    );
}

// ---- Nested repositories are outside checkpoints (PLAN.md T33, A.15) ----

/// A committed repository at `dir` (`x.txt`), or with no commit yet.
fn nested_repo(dir: &std::path::Path, commit: bool) {
    std::fs::create_dir_all(dir).unwrap();
    git_in(dir, &["init", "-q"]);
    std::fs::write(dir.join("x.txt"), "x-1\n").unwrap();
    if commit {
        git_in(dir, &["add", "."]);
        git_in(dir, &["-c", "commit.gpgSign=false", "commit", "-qm", "n"]);
    }
}

/// Every entry under `dir`, `.git` included, with its bytes.
fn whole(dir: &std::path::Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut out: Vec<_> = walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .map(|e| {
            let bytes = if e.file_type().is_file() {
                std::fs::read(e.path()).unwrap()
            } else {
                vec![]
            };
            (e.path().to_path_buf(), bytes)
        })
        .collect();
    out.sort();
    out
}

fn checkpoint_events(seen: &[RawEvent]) -> Vec<Checkpoint> {
    seen.iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::Checkpoint(c)) => Some(c),
            _ => None,
        })
        .collect()
}

/// The person is told what undo does not cover: each checkpoint names the
/// nested repositories its snapshot left out, the undo's `restored` record
/// names the ones it left alone and lists none of them among its files,
/// and the chat gets a note naming them.
#[tokio::test]
async fn nested_repositories_are_named_in_checkpoints_and_after_an_undo() {
    let mut h = Harness::new(auto_policy).await;
    init_repo(&h.ws);
    nested_repo(&h.ws.join("vendor lib"), true);
    nested_repo(&h.ws.join("a/b/scratch"), false);
    let sid = h.create().await;
    let mark = h.seen.len();
    let turn = prompt_turn(
        &mut h,
        sid,
        "run: echo v2 > keep.txt; echo agent > 'vendor lib/x.txt'; echo agent > a/b/scratch/x.txt",
    )
    .await;
    let want = ["a/b/scratch", "vendor lib"];
    let cps = checkpoint_events(&h.seen[mark..]);
    assert_eq!(cps.len(), 2, "{cps:?}");
    for c in &cps {
        assert_eq!(c.excluded, want, "{c:?}");
    }
    assert_eq!(cps[1].files, ["keep.txt"], "no nested path among the files");
    let nested_before = (
        whole(&h.ws.join("vendor lib")),
        whole(&h.ws.join("a/b/scratch")),
    );

    let mark = h.seen.len();
    restore(&mut h, sid, "r", &turn, false).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(h.ws.join("keep.txt")).unwrap(),
        "v1\n"
    );
    assert_eq!(
        (
            whole(&h.ws.join("vendor lib")),
            whole(&h.ws.join("a/b/scratch"))
        ),
        nested_before,
        "undo touched a nested repository"
    );
    let restored: Vec<Checkpoint> = checkpoint_events(&h.seen[mark..])
        .into_iter()
        .filter(|c| c.phase == CheckpointPhase::Restored)
        .collect();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].files, ["keep.txt"]);
    assert_eq!(restored[0].excluded, want);
    // Repositories that were there all along: named in `excluded` only,
    // no note in the chat.
    assert_eq!(notes(&h.seen[mark..]), Vec::<String>::new());
    // The fold keeps them with the checkpoint.
    let view = SessionView::fold(&h.seen).unwrap();
    assert_eq!(view.checkpoints[&turn].excluded, want);

    // A turn that makes a repository: its undo leaves it and says so,
    // starting with what it restored.
    let turn = prompt_turn(
        &mut h,
        sid,
        "run: echo v3 > keep.txt; mkdir made && cd made && git init -q && echo m > m.txt",
    )
    .await;
    let mark = h.seen.len();
    restore(&mut h, sid, "r2", &turn, false).await.unwrap();
    assert!(h.ws.join("made/.git").is_dir());
    let notes = notes(&h.seen[mark..]);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].starts_with("Undo restored 1 file.") && notes[0].contains("`made`"),
        "{}",
        notes[0]
    );
}

/// The chat notes (`error` events) among `seen`.
fn notes(seen: &[RawEvent]) -> Vec<String> {
    seen.iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::Error(e)) => Some(e.message),
            _ => None,
        })
        .collect()
}

/// A restore that fails after it registered its folder unregisters it: a
/// turn and a restore of the same folder run afterwards.
#[tokio::test]
async fn a_failed_restore_does_not_keep_the_folder_busy() {
    let (mut h, sid, turn) = turn_then_user_work().await;
    restore(&mut h, sid, "r1", &turn, false)
        .await
        .expect_err("refused: the user's edits");
    let ws = h.ws.clone();
    let other = create_in(&mut h, &ws).await;
    prompt_turn(&mut h, other, "run: echo hi > other.txt").await;
    restore(&mut h, sid, "r2", &turn, true).await.unwrap();
}

/// A sub-agent's base snapshot leaves nested repositories out too: one
/// without a commit no longer stops a delegation.
#[tokio::test]
async fn a_writing_sub_agent_runs_beside_a_nested_repository_without_a_commit() {
    let mut h = Harness::new(auto_policy).await;
    init_repo(&h.ws);
    nested_repo(&h.ws.join("scratch"), false);
    let sid = h.create().await;
    let out = delegate_as_lead(
        &mut h,
        sid,
        vec![agent("dev", vec![])],
        "delegate: dev=run: echo v2 > keep.txt",
    )
    .await;
    let out = out.last().expect("a delegate call");
    assert!(out.contains("1 changed file(s) applied"), "{out}");
    assert_eq!(
        std::fs::read_to_string(h.ws.join("keep.txt")).unwrap(),
        "v2\n"
    );
}

/// A nested repository arriving with a hostile `.git/config` (fsmonitor,
/// hooks, a filter its own `.gitattributes` asks for), with and without a
/// commit, runs none of it when a turn is checkpointed and undone.
#[cfg(unix)]
#[tokio::test]
async fn hostile_nested_repo_config_does_not_run_on_checkpoint_or_undo() {
    use std::os::unix::fs::PermissionsExt;
    let mut h = Harness::new(auto_policy).await;
    init_repo(&h.ws);
    let marks = h.ws.parent().unwrap().join("marks");
    std::fs::create_dir(&marks).unwrap();
    let script = |name: &str| {
        let p = h.ws.parent().unwrap().join(format!("{name}.sh"));
        std::fs::write(
            &p,
            format!(
                "#!/bin/sh\ntouch '{}'\n{}\n",
                marks.join(name).display(),
                if name.contains("clean") || name.contains("smudge") {
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
    for (dep, commit) in [("dep", true), ("unborn", false)] {
        let d = h.ws.join(dep);
        nested_repo(&d, commit);
        std::fs::write(d.join(".gitattributes"), "* filter=evil\n").unwrap();
        let cfg = |k: &str, v: &str| git_in(&d, &["config", k, v]);
        cfg("core.fsmonitor", &script(&format!("{dep}-fsmonitor")));
        cfg("filter.evil.clean", &script(&format!("{dep}-clean")));
        cfg("filter.evil.smudge", &script(&format!("{dep}-smudge")));
        cfg("filter.evil.required", "true");
        let hooks = d.join(".git/hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        for n in ["post-index-change", "pre-commit", "post-checkout"] {
            let p = hooks.join(n);
            std::fs::copy(script(&format!("{dep}-{n}")), &p).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let sid = h.create().await;
    let turn = prompt_turn(
        &mut h,
        sid,
        "run: echo v2 > keep.txt; echo agent > dep/x.txt; echo agent > unborn/x.txt",
    )
    .await;
    restore(&mut h, sid, "r", &turn, false).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(h.ws.join("keep.txt")).unwrap(),
        "v1\n"
    );
    let ran: Vec<_> = std::fs::read_dir(&marks)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(ran.is_empty(), "nested repository config ran: {ran:?}");
}

// ---- A restore waits for other chats in the folder (PLAN.md T33, A.15) ----

const BUSY: &str = "another chat is working in this folder right now";

async fn create_in(h: &mut Harness, cwd: &std::path::Path) -> SessionId {
    let sid = SessionId::new();
    h.send(MasterToNode::Create {
        profile: None,
        incognito: false,
        agent: None,
        req_id: format!("c-{sid}"),
        session_id: sid,
        title: "t".into(),
        cwd: cwd.display().to_string(),
        model: None,
        approval_mode: Some(harness_core::policy::ApprovalMode::Auto),
    });
    until_in(h, sid, |k| matches!(k, EventKind::SessionCreated(_))).await;
    sid
}

/// Receive until `pred` matches an event of session `sid`.
async fn until_in(h: &mut Harness, sid: SessionId, pred: impl Fn(&EventKind) -> bool) {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(20), h.link.from_node.recv())
            .await
            .expect("timed out waiting for event")
            .expect("link closed");
        if let NodeToMaster::Event { event } = msg {
            let k = event.typed().unwrap();
            let mine = event.session_id == sid;
            h.seen.push(event);
            if mine && pred(&k) {
                return;
            }
        }
    }
}

/// Start a turn in `sid` that keeps running (a long command).
async fn start_long_turn(h: &mut Harness, sid: SessionId) {
    h.send(MasterToNode::Prompt {
        savings: None,
        context: None,
        reply: None,
        agent: None,
        attachments: vec![],
        compare: vec![],
        req_id: format!("p-{sid}"),
        session_id: sid,
        text: "run: sleep 30".into(),
        images: vec![],
    });
    until_in(h, sid, |k| matches!(k, EventKind::ToolCallStarted(_))).await;
}

async fn stop_turn(h: &mut Harness, sid: SessionId) {
    h.send(MasterToNode::Cancel {
        req_id: format!("x-{sid}"),
        session_id: sid,
    });
    until_in(h, sid, |k| matches!(k, EventKind::TurnEnded(_))).await;
}

/// An undo is refused while another chat has a turn running in the same
/// folder, in a folder containing it or in one inside it: it would write
/// files under that turn's feet. The refusal records nothing and changes
/// nothing. Once that chat is idle the undo runs, and a chat busy in an
/// unrelated folder never blocks it.
#[tokio::test]
async fn a_restore_is_refused_while_another_chat_works_in_the_folder() {
    let mut h = Harness::new(auto_policy).await;
    let ws = h.ws.clone();
    let proj = ws.join("proj");
    std::fs::create_dir_all(proj.join("sub")).unwrap();
    std::fs::create_dir_all(ws.join("other")).unwrap();
    std::fs::write(proj.join("notes.txt"), "v0\n").unwrap();
    let a = create_in(&mut h, &proj).await;
    let turn = prompt_turn(&mut h, a, "run: echo v1 > notes.txt").await;
    let mut idle = vec![];
    for (name, cwd) in [
        ("same folder", proj.clone()),
        ("parent folder", ws.clone()),
        ("sub-folder", proj.join("sub")),
    ] {
        let b = create_in(&mut h, &cwd).await;
        start_long_turn(&mut h, b).await;
        let mark = h.seen.len();
        let err = restore(&mut h, a, &format!("r-{name}"), &turn, false)
            .await
            .expect_err(name);
        assert!(err.contains(BUSY), "{name}: {err}");
        assert!(
            pre_restore_snapshots(&h.seen[mark..]).is_empty(),
            "{name}: a refused restore records nothing"
        );
        assert_eq!(
            std::fs::read_to_string(proj.join("notes.txt")).unwrap(),
            "v1\n",
            "{name}"
        );
        stop_turn(&mut h, b).await;
        idle.push(b);
    }
    // Those chats are idle now; one is busy elsewhere.
    let c = create_in(&mut h, &ws.join("other")).await;
    start_long_turn(&mut h, c).await;
    restore(&mut h, a, "r-ok", &turn, false).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(proj.join("notes.txt")).unwrap(),
        "v0\n"
    );
    stop_turn(&mut h, c).await;
}

/// Folders are compared by where they really are: a chat whose folder is
/// a symlink to this one is working in this folder.
#[cfg(unix)]
#[tokio::test]
async fn a_restore_is_refused_when_the_busy_chat_reaches_the_folder_through_a_symlink() {
    let mut h = Harness::new(auto_policy).await;
    let real = h.ws.join("real");
    std::fs::create_dir_all(&real).unwrap();
    let b = create_in(&mut h, &real).await;
    // B's recorded folder becomes a link to where the files now are.
    let moved = h.ws.join("moved");
    std::fs::rename(&real, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, &real).unwrap();
    std::fs::write(moved.join("notes.txt"), "v0\n").unwrap();
    let a = create_in(&mut h, &moved).await;
    let turn = prompt_turn(&mut h, a, "run: echo v1 > notes.txt").await;
    // Since R.1 (S-2) a folder is pinned when its chat is made: B's folder
    // was replaced by a link, so B cannot start a turn at all, and so is
    // never busy in A's folder through it.
    h.send(MasterToNode::Prompt {
        savings: None,
        context: None,
        reply: None,
        agent: None,
        attachments: vec![],
        compare: vec![],
        req_id: format!("p-{b}"),
        session_id: b,
        text: "run: sleep 30".into(),
        images: vec![],
    });
    let err = ack_for(&mut h, &format!("p-{b}")).await.unwrap_err();
    assert!(err.contains("folder was replaced"), "{err}");
    restore(&mut h, a, "r2", &turn, false).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(moved.join("notes.txt")).unwrap(),
        "v0\n"
    );
}

// ---- The node's own git never runs what a session wrote (R.1, S-1, S-2) ----

/// S-1: a sub-agent's shell defines a clean filter in the shared
/// `.git/config` and asks for it in its worktree's `.gitattributes`. The
/// node's own git (the result snapshot, applying it to the folder) is not
/// sandboxed, and it never runs that filter; the change still arrives.
#[tokio::test]
async fn a_filter_a_sub_agent_defines_never_runs_in_the_nodes_git() {
    let mut h = Harness::new(auto_policy).await;
    init_repo(&h.ws);
    let sid = h.create().await;
    let marker = h._dir.path().join("ran-outside-the-sandbox");
    let cmd = format!(
        "pwd; G=$(git rev-parse --path-format=absolute --git-common-dir); \
         printf '#!/bin/sh\\ntr \"\\\\0\" \" \" < /proc/$PPID/cmdline > {m}\\ncat\\n' > $G/f.sh; \
         chmod +x $G/f.sh; git config filter.x.clean $G/f.sh; \
         echo '* filter=x' > .gitattributes; echo hi > new.txt",
        m = marker.display()
    );
    let out = delegate_as_lead(
        &mut h,
        sid,
        vec![agent("dev", vec![])],
        &format!("delegate: dev=run: {cmd}"),
    )
    .await;
    let out = out.last().expect("a delegate call");
    assert!(
        out.contains("harness-worktrees"),
        "not in a worktree: {out}"
    );
    assert!(
        !marker.exists(),
        "the node's own git ran the sub-agent's filter: {}",
        std::fs::read_to_string(&marker).unwrap_or_default()
    );
    assert!(out.contains("2 changed file(s) applied"), "{out}");
    assert_eq!(
        std::fs::read_to_string(h.ws.join("new.txt")).unwrap(),
        "hi\n"
    );
}

/// S-1: hostile configuration in the main repository's `.git/config` (what
/// the session's own shell may write there, PLAN.md R2) and attributes that
/// ask for it: a clean/smudge filter, `core.fsmonitor`, `core.hooksPath`
/// with hooks, `core.sshCommand`, an `include.path` that defines another
/// filter and a pager, an alias, `diff.external`, a pager, an editor and a
/// signing program. None of it runs in the node's own git through a turn's
/// checkpoints, a whole delegation (worktree made, the child changes files,
/// its changes applied to the folder) and an undo.
#[cfg(unix)]
#[tokio::test]
async fn hostile_config_in_the_main_repository_never_runs_in_the_nodes_git() {
    use std::os::unix::fs::PermissionsExt;
    let mut h = Harness::new(auto_policy).await;
    init_repo(&h.ws);
    let top = h._dir.path().to_path_buf();
    let marks = top.join("marks");
    std::fs::create_dir(&marks).unwrap();
    let script = |name: &str| {
        let p = top.join(format!("{name}.sh"));
        std::fs::write(
            &p,
            format!("#!/bin/sh\ntouch '{}'\ncat\n", marks.join(name).display()),
        )
        .unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p.display().to_string()
    };
    let hooks = top.join("hooks");
    std::fs::create_dir(&hooks).unwrap();
    for n in [
        "post-checkout",
        "pre-commit",
        "post-commit",
        "post-index-change",
        "reference-transaction",
    ] {
        let p = hooks.join(n);
        std::fs::copy(script(n), &p).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let included = top.join("included.cfg");
    std::fs::write(
        &included,
        format!(
            "[filter \"inc\"]\n\tclean = {}\n\tsmudge = {}\n[core]\n\tpager = {}\n",
            script("inc-clean"),
            script("inc-smudge"),
            script("inc-pager")
        ),
    )
    .unwrap();
    for (k, v) in [
        ("filter.evil.clean", script("clean")),
        ("filter.evil.smudge", script("smudge")),
        ("core.fsmonitor", script("fsmonitor")),
        ("core.hooksPath", hooks.display().to_string()),
        ("core.sshCommand", script("ssh")),
        ("include.path", included.display().to_string()),
        ("alias.st", format!("!{}", script("alias"))),
        ("diff.external", script("diff-external")),
        ("core.pager", script("pager")),
        ("core.editor", script("editor")),
        ("gpg.program", script("gpg")),
        ("commit.gpgSign", "true".into()),
    ] {
        git_in(&h.ws, &["config", k, &v]);
    }
    std::fs::write(
        h.ws.join(".gitattributes"),
        "* filter=evil\n*.md filter=inc\n",
    )
    .unwrap();
    let sid = h.create().await;
    let turn = prompt_turn(&mut h, sid, "run: echo v2 > keep.txt; echo doc > a.md").await;
    let out = delegate_as_lead(
        &mut h,
        sid,
        vec![agent("dev", vec![])],
        "delegate: dev=run: echo '* filter=evil' > .gitattributes; echo child > child.txt; \
         echo m > b.md",
    )
    .await;
    let out = out.last().expect("a delegate call");
    assert!(out.contains("3 changed file(s) applied"), "{out}");
    assert_eq!(
        std::fs::read_to_string(h.ws.join("child.txt")).unwrap(),
        "child\n"
    );
    restore(&mut h, sid, "r", &turn, false).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(h.ws.join("keep.txt")).unwrap(),
        "v1\n"
    );
    assert!(!h.ws.join("a.md").exists());
    let ran: Vec<_> = std::fs::read_dir(&marks)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(
        ran.is_empty(),
        "repository config ran in the node's git: {ran:?}"
    );
}

/// S-2: a session's folder is pinned when the session is created (its real
/// path, device and inode). Replaced afterwards (by a link to a folder
/// outside the workspace roots, by a link to another folder inside them, by
/// a new directory of the same name), the session refuses its next turn and
/// its undo, and nothing of the other folder reaches the chat.
#[cfg(unix)]
#[tokio::test]
async fn a_session_whose_folder_was_replaced_refuses_to_work() {
    let mut h = Harness::new(auto_policy).await;
    std::fs::write(h.ws.join("keep.txt"), "mine\n").unwrap();
    let sid = h.create().await;
    let turn = prompt_turn(&mut h, sid, "run: echo v2 > keep.txt").await;
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("id_ed25519"), "SSH-KEY-CANARY\n").unwrap();
    let inside = h._dir.path().join("other");
    std::fs::create_dir(&inside).unwrap();
    std::fs::write(inside.join("id_ed25519"), "SSH-KEY-CANARY\n").unwrap();
    std::fs::rename(&h.ws, h._dir.path().join("ws.moved")).unwrap();
    for (i, target) in [
        Some(outside.path().to_path_buf()),
        Some(inside.clone()),
        None,
    ]
    .into_iter()
    .enumerate()
    {
        let _ = std::fs::remove_file(&h.ws);
        let _ = std::fs::remove_dir_all(&h.ws);
        match &target {
            Some(t) => std::os::unix::fs::symlink(t, &h.ws).unwrap(),
            None => {
                std::fs::create_dir(&h.ws).unwrap();
                std::fs::write(h.ws.join("id_ed25519"), "SSH-KEY-CANARY\n").unwrap();
            }
        }
        h.send(MasterToNode::Prompt {
            savings: None,
            context: None,
            reply: None,
            agent: None,
            attachments: vec![],
            compare: vec![],
            req_id: format!("p{i}"),
            session_id: sid,
            text: "read: id_ed25519".into(),
            images: vec![],
        });
        let e = ack_for(&mut h, &format!("p{i}")).await.unwrap_err();
        assert!(e.contains("folder was replaced"), "{target:?}: {e}");
        let e = restore(&mut h, sid, &format!("r{i}"), &turn, false)
            .await
            .unwrap_err();
        assert!(e.contains("folder was replaced"), "{target:?}: {e}");
    }
    let leaked = h
        .seen
        .iter()
        .any(|e| e.payload.to_string().contains("SSH-KEY-CANARY"));
    assert!(!leaked, "the other folder's file reached the chat");
}
