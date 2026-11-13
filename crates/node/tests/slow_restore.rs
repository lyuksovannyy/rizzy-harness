//! A slow command for one session must not hold up the node (AUDIT S10):
//! `Cancel` and `ApprovalRespond`, for that session or any other, are
//! answered at once while a restore runs, and each session's own commands
//! still run in the order they were sent.
//!
//! The restore is made slow for real: the test runs itself again with a
//! `git` first on `PATH` that sleeps on `cat-file --batch` (only restore uses
//! it).
//! A child process because the environment of this one cannot be changed
//! without `unsafe`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::SessionId;
use harness_core::event::*;
use harness_core::policy::{ApprovalMode, Decision};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};

const CHILD: &str = "HARNESS_TEST_SLOW_GIT_CHILD";
const SLOW: Duration = Duration::from_secs(4);

#[test]
fn cancel_during_a_slow_restore_is_answered_within_a_second() {
    if std::env::var_os(CHILD).is_some() {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(scenario());
        return;
    }
    let path = std::env::var("PATH").unwrap_or_default();
    let Some(real) = std::env::split_paths(&path)
        .map(|d| d.join("git"))
        .find(|g| g.is_file())
    else {
        panic!("git is needed for this test");
    };
    let bin = tempfile::tempdir().unwrap();
    let wrapper = bin.path().join("git");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do [ \"$a\" = --batch ] && sleep {}; done\nexec '{}' \"$@\"\n",
            SLOW.as_secs(),
            real.display()
        ),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "cancel_during_a_slow_restore_is_answered_within_a_second",
            "--exact",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env(
            "PATH",
            std::env::join_paths(
                std::iter::once(bin.path().to_path_buf()).chain(std::env::split_paths(&path)),
            )
            .unwrap(),
        )
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("the scenario hung");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(status.success(), "the scenario failed: {status}");
}

struct Link {
    link: NodeLink,
    /// Acks seen, by req_id, with when they arrived.
    acks: Vec<(String, bool, Option<String>, Instant)>,
    events: Vec<EventKind>,
}

impl Link {
    fn send(&self, m: MasterToNode) {
        self.link.to_node.try_send(m).unwrap();
    }

    async fn pump_until(&mut self, done: impl Fn(&Self) -> bool, within: Duration) {
        let end = tokio::time::Instant::now() + within;
        while !done(self) {
            let msg = tokio::time::timeout_at(end, self.link.from_node.recv())
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "timed out; acks so far: {:?}",
                        self.acks.iter().map(|a| &a.0).collect::<Vec<_>>()
                    )
                })
                .expect("link closed");
            match msg {
                NodeToMaster::Event { event } => self.events.push(event.typed().unwrap()),
                NodeToMaster::Ack { req_id, ok, error } => {
                    self.acks.push((req_id, ok, error, Instant::now()))
                }
                _ => {}
            }
        }
    }

    fn ack(&self, req: &str) -> Option<&(String, bool, Option<String>, Instant)> {
        self.acks.iter().find(|a| a.0 == req)
    }

    async fn create(&mut self, cwd: &std::path::Path) -> SessionId {
        let sid = SessionId::new();
        let req = format!("create-{sid}");
        self.send(MasterToNode::Create {
            no_callbacks: false,
            profile: None,
            incognito: false,
            agent: None,
            req_id: req.clone(),
            session_id: sid,
            title: "t".into(),
            cwd: cwd.display().to_string(),
            model: None,
            approval_mode: Some(ApprovalMode::Auto),
        });
        self.pump_until(|l| l.ack(&req).is_some(), Duration::from_secs(10))
            .await;
        assert!(self.ack(&req).unwrap().1, "{:?}", self.ack(&req));
        sid
    }

    fn prompt(&self, sid: SessionId, req: &str, text: &str) {
        self.send(MasterToNode::Prompt {
            savings: None,
            context: None,
            reply: None,
            agent: None,
            attachments: vec![],
            compare: vec![],
            req_id: req.into(),
            session_id: sid,
            text: text.into(),
            images: vec![],
        });
    }
}

async fn scenario() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let git = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&ws)
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
    std::fs::write(ws.join("keep.txt"), "v1\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "init"]);

    let store = Store::open(&dir.path().join("node.db"), Durability::Full).unwrap();
    let node = NodeRuntime::start(NodeConfig {
        node_id: "local".into(),
        hostname: "test".into(),
        policy: NodePolicy {
            workspace_roots: vec![dir.path().into()],
            max_approval_mode: ApprovalMode::Auto,
            ..Default::default()
        },
        store,
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(1),
            }),
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: Some(dir.path().join("checkpoints")),
        git_sandbox: None,
        artifacts: None,
    })
    .await
    .unwrap();
    let mut l = Link {
        link: node.attach().await,
        acks: vec![],
        events: vec![],
    };
    let a = l.create(&ws).await;
    l.prompt(a, "p1", "run: echo v2 > keep.txt");
    l.pump_until(
        |l| {
            l.events
                .iter()
                .any(|e| matches!(e, EventKind::TurnEnded(_)))
        },
        Duration::from_secs(20),
    )
    .await;
    let turn = l
        .events
        .iter()
        .find_map(|e| match e {
            EventKind::Checkpoint(c) if c.phase == CheckpointPhase::After => {
                Some(c.turn_id.clone())
            }
            _ => None,
        })
        .expect("a checkpoint");
    let b = l.create(&ws).await;
    // B gets a turn of its own to undo later (another file).
    l.prompt(b, "pb", "run: echo b > b.txt");
    l.pump_until(
        |l| {
            l.events
                .iter()
                .filter(|e| matches!(e, EventKind::TurnEnded(_)))
                .count()
                == 2
        },
        Duration::from_secs(20),
    )
    .await;
    let b_turn = l
        .events
        .iter()
        .rev()
        .find_map(|e| match e {
            EventKind::Checkpoint(c) if c.phase == CheckpointPhase::After => {
                Some(c.turn_id.clone())
            }
            _ => None,
        })
        .expect("b's checkpoint");

    let started = Instant::now();
    l.send(MasterToNode::Restore {
        req_id: "restore".into(),
        session_id: a,
        turn_id: turn,
        force: false,
        rewind: false,
    });
    // Sent after the restore, for the same session: must wait for it.
    l.prompt(a, "after-restore", "hello");
    tokio::time::sleep(Duration::from_millis(300)).await;
    let sent = Instant::now();
    for (req, sid) in [("cancel-a", a), ("cancel-b", b)] {
        l.send(MasterToNode::Cancel {
            req_id: req.into(),
            session_id: sid,
        });
    }
    l.send(MasterToNode::ApprovalRespond {
        req_id: "approve-b".into(),
        session_id: b,
        approval_id: "no-such-approval".into(),
        decision: Decision::Allow,
    });
    l.pump_until(
        |l| {
            ["cancel-a", "cancel-b", "approve-b"]
                .iter()
                .all(|r| l.ack(r).is_some())
        },
        Duration::from_secs(20),
    )
    .await;
    for r in ["cancel-a", "cancel-b", "approve-b"] {
        let waited = l.ack(r).unwrap().3 - sent;
        assert!(
            waited < Duration::from_secs(1),
            "{r} was answered after {waited:?}, behind the restore"
        );
    }
    assert!(l.ack("cancel-a").unwrap().1 && l.ack("cancel-b").unwrap().1);
    // While A's restore runs, B can neither start a turn in the folder nor
    // restore it (A.15).
    l.prompt(b, "turn-during-restore", "run: echo late > late.txt");
    l.send(MasterToNode::Restore {
        req_id: "second-restore".into(),
        session_id: b,
        turn_id: b_turn,
        force: false,
        rewind: false,
    });
    l.pump_until(
        |l| l.ack("turn-during-restore").is_some() && l.ack("second-restore").is_some(),
        Duration::from_secs(20),
    )
    .await;
    for r in ["turn-during-restore", "second-restore"] {
        let a = l.ack(r).unwrap();
        assert!(
            !a.1 && a
                .2
                .as_deref()
                .is_some_and(|e| e.contains("an undo is running in this folder")),
            "{r}: {a:?}"
        );
    }
    assert!(!ws.join("late.txt").exists());
    assert!(
        l.ack("restore").is_none(),
        "the restore was not slow; the test proves nothing"
    );
    l.pump_until(
        |l| l.ack("restore").is_some() && l.ack("after-restore").is_some(),
        Duration::from_secs(30),
    )
    .await;
    let r = l.ack("restore").unwrap();
    assert!(r.1, "restore failed: {:?}", r.2);
    assert!(r.3 - started >= SLOW, "restore took {:?}", r.3 - started);
    let p = l.ack("after-restore").unwrap();
    assert!(p.1, "{:?}", p.2);
    let pos = |req: &str| l.acks.iter().position(|a| a.0 == req).unwrap();
    assert!(
        pos("restore") < pos("after-restore"),
        "a session's commands ran out of order"
    );
    assert_eq!(
        std::fs::read_to_string(ws.join("keep.txt")).unwrap(),
        "v1\n"
    );
}
