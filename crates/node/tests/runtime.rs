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
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir(&ws).unwrap();
        let store = Store::open(&dir.path().join("node.db"), Durability::Full).unwrap();
        let mut cfg = config(store, policy(dir.path()));
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

/// "Allow for this turn" (`allow_session` on the wire) is honoured only
/// after the tool's first approval in the turn (audit L10): whatever a
/// client sends on the first request is a single allow, so the first command
/// is always seen; the next request offers it, and then it holds for the
/// rest of the turn only.
#[tokio::test]
async fn allow_for_this_turn_is_refused_on_the_first_approval() {
    let mut h = Harness::new(default_policy).await;
    let sid = h.create().await;
    let prompt = |h: &Harness, id: &str| {
        h.send(MasterToNode::Prompt {
            savings: None,
            context: None,
            reply: None,
            agent: None,
            attachments: vec![],
            compare: vec![],
            req_id: id.into(),
            session_id: sid,
            // Three `bash true` calls in one turn.
            text: "progress: 4".into(),
            images: vec![],
        })
    };
    prompt(&h, "p1");
    let mut asked = vec![];
    for _ in 0..2 {
        let EventKind::ApprovalRequested(a) = h
            .until(|k| matches!(k, EventKind::ApprovalRequested(_)))
            .await
        else {
            unreachable!()
        };
        asked.push(a.clone());
        // A scripted or stale client sends "for this turn" every time.
        h.send(MasterToNode::ApprovalRespond {
            req_id: "a".into(),
            session_id: sid,
            approval_id: a.approval_id,
            decision: Decision::AllowSession,
        });
    }
    h.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
    let events: Vec<EventKind> = h.seen.iter().map(|e| e.typed().unwrap()).collect();
    let requests = events
        .iter()
        .filter(|k| matches!(k, EventKind::ApprovalRequested(_)))
        .count();
    let resolved: Vec<Decision> = events
        .iter()
        .filter_map(|k| match k {
            EventKind::ApprovalResolved(r) => Some(r.decision),
            _ => None,
        })
        .collect();
    // The first answer was cut down to a single allow; the second request
    // came because of it; the third call ran without asking.
    assert_eq!(requests, 2, "{resolved:?}");
    assert_eq!(resolved, [Decision::Allow, Decision::AllowSession]);
    assert!(!asked[0].turn_allow, "first request must not offer it");
    assert!(asked[1].turn_allow, "second request offers it");

    // A new turn starts over: the first approval again cannot be "for the turn".
    prompt(&h, "p2");
    let EventKind::ApprovalRequested(a) = h
        .until(|k| matches!(k, EventKind::ApprovalRequested(_)))
        .await
    else {
        unreachable!()
    };
    assert!(!a.turn_allow);
    // Deny everything until the turn ends.
    let mut next = Some(a);
    while let Some(a) = next.take() {
        h.send(MasterToNode::ApprovalRespond {
            req_id: "a".into(),
            session_id: sid,
            approval_id: a.approval_id,
            decision: Decision::Deny,
        });
        if let EventKind::ApprovalRequested(a) = h
            .until(|k| matches!(k, EventKind::ApprovalRequested(_) | EventKind::TurnEnded(_)))
            .await
        {
            next = Some(a);
        }
    }
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
            .send(MasterToNode::Create {
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
        .send(MasterToNode::Prompt {
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
        .send(MasterToNode::Prompt {
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
        .send(MasterToNode::Prompt {
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
