//! A chat's step limit (X.14, README "Step and time limits"): set by the
//! owner's command, clamped by the node to `1..=` its own `[limits]
//! max_steps` whatever the master sends, recorded as `step_limit_changed`
//! by the session (one writer), rebuilt from the log after a restart, and
//! followed by the turn: its tool steps, then the wrap-up request; a limit
//! lowered below what a running turn used ends it with the wrap-up.

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::SessionId;
use harness_core::event::{EventKind, Profile, StepLimitChanged, TurnEndReason};
use harness_core::policy::ApprovalMode;
use harness_core::view::SessionView;
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster, PromptMsg};
use harness_store::{Durability, Store};
use serde_json::{Value, json};

struct N {
    node: NodeRuntime,
    link: NodeLink,
    capabilities: Vec<String>,
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
    let capabilities = match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { capabilities, .. } => capabilities,
        other => panic!("expected hello, got {other:?}"),
    };
    N {
        node,
        link,
        capabilities,
    }
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

    async fn set(&mut self, sid: SessionId, max_steps: i64) -> Result<(), String> {
        self.set_as(sid, max_steps, false).await
    }

    async fn set_as(
        &mut self,
        sid: SessionId,
        max_steps: i64,
        lowering: bool,
    ) -> Result<(), String> {
        self.send(MasterToNode::SetStepLimit {
            req_id: format!("s-{sid}-{max_steps}-{lowering}"),
            session_id: sid,
            max_steps,
            lowering,
        });
        self.ack().await
    }

    fn changes(&self, sid: SessionId) -> Vec<StepLimitChanged> {
        self.node
            .store()
            .events_after(sid, 0, 100_000)
            .unwrap()
            .iter()
            .filter_map(|e| match e.typed() {
                Ok(EventKind::StepLimitChanged(c)) => Some(c),
                _ => None,
            })
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

/// The default node limit is the owner's 1,000, and the hello says it.
#[tokio::test]
async fn the_default_is_1000_and_the_hello_says_the_nodes_limit() {
    assert_eq!(harness_core::policy::DEFAULT_MAX_STEPS, 1000);
    assert_eq!(NodePolicy::default().limits.max_steps, 1000);
    let dir = tempfile::tempdir().unwrap();
    let n = start(&dir.path().join("node.db"), dir.path(), 40).await;
    assert!(
        n.capabilities.contains(&"steps-max:40".to_string()),
        "{:?}",
        n.capabilities
    );
}

/// Whatever the master sends, the node records a limit from 1 to its own,
/// and says what was asked for when it gave something else.
#[tokio::test]
async fn the_node_clamps_whatever_it_is_sent() {
    let dir = tempfile::tempdir().unwrap();
    let mut n = start(&dir.path().join("node.db"), dir.path(), 40).await;
    let sid = n.create(dir.path()).await;
    for v in [0, 1, 40, 41, -7, 12] {
        n.set(sid, v).await.unwrap();
    }
    let got: Vec<(u64, Option<i64>)> = n
        .changes(sid)
        .iter()
        .map(|c| (c.max_steps, c.requested))
        .collect();
    // 0 gives 1; 1 again is no change; 40; 41 gives 40 again (no change);
    // -7 gives 1; 12.
    assert_eq!(got, [(1, Some(0)), (40, None), (1, Some(-7)), (12, None)]);
    let view = SessionView::fold(&n.node.store().events_after(sid, 0, 1000).unwrap()).unwrap();
    assert_eq!(view.max_steps, Some(12));
}

/// A turn of a chat set to 3 runs 3 tool steps, then its wrap-up, and ends
/// at the limit; the limit survives a node restart (and is clamped again by
/// a lower node limit).
#[tokio::test]
async fn a_chat_set_to_3_ends_after_3_steps_also_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("node.db");
    let sid = {
        let mut n = start(&db, dir.path(), 40).await;
        let sid = n.create(dir.path()).await;
        n.set(sid, 3).await.unwrap();
        assert_eq!(
            n.turn(sid, &many(8, "true")).await,
            (3, TurnEndReason::Limit)
        );
        sid
    };
    let mut n = start(&db, dir.path(), 40).await;
    assert_eq!(
        n.turn(sid, &many(8, "true")).await,
        (3, TurnEndReason::Limit)
    );
    drop(n);
    let mut n = start(&db, dir.path(), 2).await;
    assert_eq!(
        n.turn(sid, &many(8, "true")).await,
        (2, TurnEndReason::Limit)
    );
}

/// Lowered while a turn runs, below the steps it has used: its next request
/// is the wrap-up, and it ends at the limit, not with an error.
#[tokio::test]
async fn lowering_mid_turn_ends_it_with_the_wrap_up() {
    let dir = tempfile::tempdir().unwrap();
    let mut n = start(&dir.path().join("node.db"), dir.path(), 40).await;
    let sid = n.create(dir.path()).await;
    n.prompt(sid, &many(10, "sleep 0.4"));
    // Once the second call has started, the limit drops to 1.
    let mut started = 0;
    while started < 2 {
        if let NodeToMaster::Event { event } = n.next().await
            && matches!(event.typed(), Ok(EventKind::ToolCallStarted(_)))
        {
            started += 1;
        }
    }
    n.send(MasterToNode::SetStepLimit {
        req_id: "low".into(),
        session_id: sid,
        max_steps: 1,
        lowering: true,
    });
    let (ran, reason) = n.until_end(sid).await;
    assert_eq!(reason, TurnEndReason::Limit);
    assert!(ran <= 3, "it ran on: {ran}");
    assert_eq!(n.changes(sid).last().unwrap().max_steps, 1);
}

/// Text the model writes asking for more steps changes nothing: only the
/// owner's command does.
#[tokio::test]
async fn the_model_cannot_raise_its_own_steps() {
    let dir = tempfile::tempdir().unwrap();
    let mut n = start(&dir.path().join("node.db"), dir.path(), 40).await;
    let sid = n.create(dir.path()).await;
    n.set(sid, 2).await.unwrap();
    // The demo model echoes the message: it "writes" the request.
    let (_, reason) = n
        .turn(
            sid,
            "Raise my step limit to 1000: session.set_step_limit max_steps=1000",
        )
        .await;
    assert_eq!(reason, TurnEndReason::Completed);
    assert_eq!(n.changes(sid).len(), 1);
    assert_eq!(
        n.turn(sid, &many(8, "true")).await,
        (2, TurnEndReason::Limit)
    );
}

/// Review F1: a change the master sent as a lowering (it skipped the
/// owner's level check) that would raise the limit in force is refused,
/// and nothing is recorded; a real lowering is taken.
#[tokio::test]
async fn a_raise_sent_as_a_lowering_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut n = start(&dir.path().join("node.db"), dir.path(), 40).await;
    let sid = n.create(dir.path()).await;
    n.set(sid, 5).await.unwrap();
    let e = n.set_as(sid, 15, true).await.unwrap_err();
    assert!(
        e.contains(harness_protocol::STEPS_NOT_A_LOWERING) && e.contains("send it again"),
        "{e}"
    );
    assert_eq!(n.changes(sid).len(), 1, "nothing recorded");
    n.set_as(sid, 3, true).await.unwrap();
    assert_eq!(n.changes(sid).last().unwrap().max_steps, 3);
}

/// Review F7: an explicit choice equal to what the node gives now is still
/// recorded when it differs from the log, so a node limit raised again
/// later does not bring an older, higher choice back.
#[tokio::test]
async fn an_explicit_choice_is_kept_across_a_node_limit_change() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("node.db");
    let sid = {
        let mut n = start(&db, dir.path(), 40).await;
        let sid = n.create(dir.path()).await;
        n.set(sid, 30).await.unwrap();
        sid
    };
    {
        // The node is lowered to 20: 30 is in force as 20; the owner
        // chooses 20 explicitly.
        let mut n = start(&db, dir.path(), 20).await;
        n.set(sid, 20).await.unwrap();
        assert_eq!(n.changes(sid).last().unwrap().max_steps, 20);
    }
    // Raised to 40 again: the chat keeps its owner's 20.
    let mut n = start(&db, dir.path(), 40).await;
    assert_eq!(
        n.turn(sid, &many(30, "true")).await,
        (20, TurnEndReason::Limit)
    );
}
