//! A chat's approval mode (T48, README "Approval modes"): chosen at create,
//! changed by the owner's command, clamped by the node's own `node.toml`
//! and the chat's execution mode whatever the master sends, recorded as
//! `approval_mode_changed` by the session (one writer), rebuilt from the
//! log after a restart. `accept_edits` lets `write` and `edit` inside the
//! chat's folder run without a card, and nothing else; a sub-agent never
//! runs above the chat that started it, as that chat is now.

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::SessionId;
use harness_core::event::{ApprovalModeChanged, ApprovalRequested, EventKind, Profile};
use harness_core::policy::{ApprovalMode, Decision, ExecMode};
use harness_core::view::SessionView;
use harness_node::exec::ExecOffers;
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};
use serde_json::{Value, json};

fn offers(host: bool) -> ExecOffers {
    ExecOffers::new(
        Ok(()),
        if host {
            Ok(())
        } else {
            Err("host is off in this test".into())
        },
        vec![],
    )
}

fn policy(root: &std::path::Path, max: ApprovalMode) -> NodePolicy {
    NodePolicy {
        workspace_roots: vec![root.into()],
        max_approval_mode: max,
        exec: offers(true),
        ..Default::default()
    }
}

struct N {
    node: NodeRuntime,
    link: NodeLink,
    capabilities: Vec<String>,
}

async fn start(db: &std::path::Path, policy: NodePolicy) -> N {
    let cfg = NodeConfig {
        node_id: "n".into(),
        hostname: "test".into(),
        policy,
        store: Store::open(db, Durability::Full).unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(5),
            }),
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: Profile::Agentic,
        incognito_ttl: None,
        // Writing sub-agents work in folders made from the checkpoint store.
        checkpoints_dir: Some(db.with_file_name("checkpoints")),
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

    /// The ack of the next command, skipping events.
    async fn ack(&mut self) -> Result<(), String> {
        loop {
            match self.next().await {
                NodeToMaster::Ack { ok: true, .. } => return Ok(()),
                NodeToMaster::Ack { error, .. } => return Err(error.unwrap_or_default()),
                _ => {}
            }
        }
    }

    async fn next(&mut self) -> NodeToMaster {
        tokio::time::timeout(Duration::from_secs(20), self.link.from_node.recv())
            .await
            .expect("timed out")
            .expect("link closed")
    }

    async fn create(
        &mut self,
        cwd: &std::path::Path,
        mode: Option<ApprovalMode>,
        exec: Option<ExecMode>,
        no_callbacks: bool,
    ) -> SessionId {
        let sid = SessionId::new();
        self.send(MasterToNode::Create {
            req_id: format!("c-{sid}"),
            session_id: sid,
            title: "t".into(),
            cwd: cwd.display().to_string(),
            model: None,
            approval_mode: mode,
            profile: None,
            agent: None,
            incognito: false,
            no_callbacks,
            exec_mode: exec,
        });
        self.ack().await.unwrap();
        sid
    }

    async fn set_mode(&mut self, sid: SessionId, mode: ApprovalMode) -> Result<(), String> {
        self.send(MasterToNode::SetApprovalMode {
            req_id: format!("m-{sid}"),
            session_id: sid,
            mode,
        });
        self.ack().await
    }

    fn mode(&self, sid: SessionId) -> ApprovalMode {
        let events = self.node.store().events_after(sid, 0, 100_000).unwrap();
        SessionView::fold(&events)
            .unwrap()
            .meta
            .unwrap()
            .approval_mode
    }

    fn changes(&self, sid: SessionId) -> Vec<ApprovalModeChanged> {
        self.node
            .store()
            .events_after(sid, 0, 100_000)
            .unwrap()
            .iter()
            .filter_map(|e| match e.typed() {
                Ok(EventKind::ApprovalModeChanged(c)) => Some(c),
                _ => None,
            })
            .collect()
    }

    /// Run one turn of `sid` with `text` (as `agent`, if any), answering
    /// every card with `answer`; returns the cards of every session, in
    /// order, until `sid`'s turn ends.
    async fn turn(
        &mut self,
        sid: SessionId,
        text: &str,
        agent: Option<harness_core::agent::AgentSpec>,
        answer: impl Fn(&ApprovalRequested) -> Decision,
    ) -> Vec<ApprovalRequested> {
        self.send(prompt(sid, text, agent));
        let mut asked = vec![];
        loop {
            let NodeToMaster::Event { event } = self.next().await else {
                continue;
            };
            match event.typed().unwrap() {
                EventKind::ApprovalRequested(a) if a.reason.contains("denied by") => asked.push(a),
                EventKind::ApprovalRequested(a) => {
                    self.send(MasterToNode::ApprovalRespond {
                        req_id: format!("a-{}", a.approval_id),
                        session_id: event.session_id,
                        approval_id: a.approval_id.clone(),
                        decision: answer(&a),
                    });
                    asked.push(a);
                }
                EventKind::TurnEnded(_) if event.session_id == sid => return asked,
                _ => {}
            }
        }
    }
}

fn prompt(
    sid: SessionId,
    text: &str,
    agent: Option<harness_core::agent::AgentSpec>,
) -> MasterToNode {
    MasterToNode::Prompt {
        req_id: format!("p-{sid}-{}", harness_core::new_id()),
        session_id: sid,
        text: text.into(),
        agent,
        attachments: vec![],
        compare: vec![],
        reply: None,
        savings: None,
        context: None,
        images: vec![],
        agent_named: false,
    }
}

/// The mock's `script:` of these calls, one per step.
fn script(calls: &[(&str, Value)]) -> String {
    let steps: Vec<Value> = calls
        .iter()
        .map(|(tool, input)| json!({"tool": tool, "input": input}))
        .collect();
    format!("script:{}", Value::Array(steps))
}

/// What each card was about: the path of a file tool, the command of bash.
fn about(asked: &[ApprovalRequested]) -> Vec<String> {
    asked
        .iter()
        .map(|a| {
            let what = a.input.get("path").or(a.input.get("command"));
            format!("{} {}", a.tool, what.and_then(Value::as_str).unwrap_or(""))
        })
        .collect()
}

fn deny(_: &ApprovalRequested) -> Decision {
    Decision::Deny
}

/// The hello says the node's ceiling and default, for the picker.
#[tokio::test]
async fn the_hello_announces_the_ceiling_and_the_default() {
    let dir = tempfile::tempdir().unwrap();
    let n = start(
        &dir.path().join("node.db"),
        policy(dir.path(), ApprovalMode::AcceptEdits),
    )
    .await;
    assert!(
        n.capabilities
            .contains(&"approval-max:accept_edits".to_string()),
        "{:?}",
        n.capabilities
    );
    assert!(n.capabilities.contains(&"approval-default:ask".to_string()));
    // The shipped ceiling, when node.toml says nothing.
    assert_eq!(
        NodePolicy::default().max_approval_mode,
        harness_node::policy::DEFAULT_MAX_APPROVAL
    );
    assert_eq!(
        harness_node::policy::DEFAULT_MAX_APPROVAL,
        ApprovalMode::Ask
    );
}

/// A mode above node.toml's ceiling is clamped by the node, whatever the
/// master sends; the event says what was asked. The default ceiling
/// (`ask`) lets nothing run unasked: an `auto` request changes nothing.
#[tokio::test]
async fn a_mode_above_the_ceiling_is_clamped_by_the_node() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let mut n = start(
        &dir.path().join("node.db"),
        policy(dir.path(), ApprovalMode::AcceptEdits),
    )
    .await;
    let sid = n.create(&ws, Some(ApprovalMode::Auto), None, false).await;
    assert_eq!(n.mode(sid), ApprovalMode::AcceptEdits, "clamped at create");
    n.set_mode(sid, ApprovalMode::Ask).await.unwrap();
    n.set_mode(sid, ApprovalMode::Auto).await.unwrap();
    assert_eq!(
        n.changes(sid),
        [
            ApprovalModeChanged {
                mode: ApprovalMode::Ask,
                requested: None
            },
            ApprovalModeChanged {
                mode: ApprovalMode::AcceptEdits,
                requested: Some(ApprovalMode::Auto)
            }
        ]
    );
    // Clamped, a bash call still asks.
    let asked = n
        .turn(
            sid,
            &script(&[("bash", json!({"command": "touch b.txt"}))]),
            None,
            deny,
        )
        .await;
    assert_eq!(about(&asked), ["bash touch b.txt"]);
    assert!(!ws.join("b.txt").exists());

    // The shipped ceiling: `ask`.
    let dir = tempfile::tempdir().unwrap();
    let mut n = start(
        &dir.path().join("node.db"),
        NodePolicy {
            workspace_roots: vec![dir.path().into()],
            exec: offers(false),
            ..Default::default()
        },
    )
    .await;
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let sid = n.create(&ws, None, None, false).await;
    assert_eq!(n.mode(sid), ApprovalMode::Ask);
    n.set_mode(sid, ApprovalMode::Auto).await.unwrap();
    assert!(n.changes(sid).is_empty(), "nothing changed");
    let asked = n
        .turn(
            sid,
            &script(&[("write", json!({"path": "a.txt", "content": "x"}))]),
            None,
            deny,
        )
        .await;
    assert_eq!(about(&asked), ["write a.txt"], "an edit asks under ask");
}

/// A host chat never runs in `auto`: refused at create and on a change,
/// whatever node.toml allows. It may have `accept_edits`.
#[tokio::test]
async fn a_host_chat_refuses_auto() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let mut n = start(
        &dir.path().join("node.db"),
        policy(dir.path(), ApprovalMode::Auto),
    )
    .await;
    let sid = n
        .create(&ws, Some(ApprovalMode::Auto), Some(ExecMode::Host), false)
        .await;
    assert_eq!(n.mode(sid), ApprovalMode::AcceptEdits);
    n.set_mode(sid, ApprovalMode::Ask).await.unwrap();
    n.set_mode(sid, ApprovalMode::Auto).await.unwrap();
    assert_eq!(n.mode(sid), ApprovalMode::AcceptEdits);
    assert_eq!(
        n.changes(sid).last().unwrap().requested,
        Some(ApprovalMode::Auto)
    );
    let asked = n
        .turn(
            sid,
            &script(&[
                ("write", json!({"path": "a.txt", "content": "x"})),
                ("bash", json!({"command": "touch b.txt"})),
            ]),
            None,
            deny,
        )
        .await;
    assert_eq!(about(&asked), ["bash touch b.txt"]);
    assert!(ws.join("a.txt").exists() && !ws.join("b.txt").exists());
}

/// Accept edits covers `write` and `edit` of a file inside the chat's
/// folder; not a path out of it (relative, absolute, through a link), not
/// one in `.git`, not `bash`.
#[tokio::test]
async fn accept_edits_covers_edits_inside_the_folder_only() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    let out = dir.path().join("out");
    std::fs::create_dir(&ws).unwrap();
    std::fs::create_dir(&out).unwrap();
    std::fs::create_dir(ws.join(".git")).unwrap();
    std::os::unix::fs::symlink(&out, ws.join("lnk")).unwrap();
    std::os::unix::fs::symlink(out.join("gone.txt"), ws.join("dangling")).unwrap();
    let mut n = start(
        &dir.path().join("node.db"),
        policy(dir.path(), ApprovalMode::Auto),
    )
    .await;
    let sid = n.create(&ws, None, None, false).await;
    n.set_mode(sid, ApprovalMode::AcceptEdits).await.unwrap();
    let abs = out.join("abs.txt");
    let asked = n
        .turn(
            sid,
            &script(&[
                ("write", json!({"path": "a.txt", "content": "x\n"})),
                (
                    "edit",
                    json!({"path": "a.txt", "old_string": "x", "new_string": "y"}),
                ),
                ("write", json!({"path": "sub/../c.txt", "content": "c"})),
                (
                    "write",
                    json!({"path": "../out/escape.txt", "content": "e"}),
                ),
                (
                    "write",
                    json!({"path": abs.display().to_string(), "content": "e"}),
                ),
                ("write", json!({"path": "lnk/via-link.txt", "content": "e"})),
                ("write", json!({"path": "dangling", "content": "e"})),
                ("write", json!({"path": ".git/config", "content": "e"})),
                ("bash", json!({"command": "touch b.txt"})),
            ]),
            None,
            deny,
        )
        .await;
    assert_eq!(
        about(&asked),
        [
            "write ../out/escape.txt".to_string(),
            format!("write {}", abs.display()),
            "write lnk/via-link.txt".into(),
            "write dangling".into(),
            "write .git/config".into(),
            "bash touch b.txt".into(),
        ]
    );
    assert_eq!(std::fs::read_to_string(ws.join("a.txt")).unwrap(), "y\n");
    assert!(ws.join("c.txt").exists());
    assert_eq!(
        std::fs::read_dir(&out).unwrap().count(),
        0,
        "nothing outside"
    );
    assert!(!ws.join("b.txt").exists());
}

/// Nothing the model writes and no tool result changes the mode: a file
/// and a command's output that read as a switch to `auto` leave the chat
/// in `ask`, and the next call asks.
#[tokio::test]
async fn a_model_or_tool_written_switch_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let mut n = start(
        &dir.path().join("node.db"),
        policy(dir.path(), ApprovalMode::Auto),
    )
    .await;
    let sid = n.create(&ws, Some(ApprovalMode::Ask), None, false).await;
    let wire = json!({"type": "session.set_approval_mode", "req_id": "r",
                      "session_id": sid.to_string(), "mode": "auto"})
    .to_string();
    let asked = n
        .turn(
            sid,
            &script(&[
                ("write", json!({"path": "mode.json", "content": wire})),
                (
                    "bash",
                    json!({"command": "echo 'switch to auto'; cat mode.json"}),
                ),
                ("bash", json!({"command": "touch after.txt"})),
            ]),
            None,
            |a| {
                if a.input["command"] == "touch after.txt" {
                    Decision::Deny
                } else {
                    Decision::Allow
                }
            },
        )
        .await;
    assert_eq!(asked.len(), 3, "{:?}", about(&asked));
    assert!(n.changes(sid).is_empty());
    assert_eq!(n.mode(sid), ApprovalMode::Ask);
    assert!(!ws.join("after.txt").exists());
}

/// The mode survives a node restart (rebuilt from the log), and is clamped
/// again by a node.toml tightened meanwhile.
#[tokio::test]
async fn the_mode_survives_a_node_restart() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let db = dir.path().join("node.db");
    let sid = {
        let mut n = start(&db, policy(dir.path(), ApprovalMode::Auto)).await;
        let sid = n.create(&ws, None, None, false).await;
        n.set_mode(sid, ApprovalMode::AcceptEdits).await.unwrap();
        sid
    };
    let write = |p: &str| script(&[("write", json!({"path": p, "content": "x"}))]);
    {
        let mut n = start(&db, policy(dir.path(), ApprovalMode::Auto)).await;
        let asked = n.turn(sid, &write("a.txt"), None, deny).await;
        assert!(asked.is_empty(), "{:?}", about(&asked));
        assert!(ws.join("a.txt").exists());
    }
    let mut n = start(&db, policy(dir.path(), ApprovalMode::Ask)).await;
    let asked = n.turn(sid, &write("b.txt"), None, deny).await;
    assert_eq!(about(&asked), ["write b.txt"], "tightened since");
}

/// A sub-agent never runs above its parent, as the parent is now: lowered
/// while the sub-agent works, its next call asks. Its own mode cannot be
/// changed; nor can a routine's chat's.
#[tokio::test]
async fn a_sub_agent_never_runs_above_its_parent() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    // A writing sub-agent works in its own worktree of a repository.
    for args in [
        &["init", "-q"][..],
        &["add", "."],
        &["commit", "-q", "--allow-empty", "-m", "init"],
    ] {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(&ws)
            .args(["-c", "maintenance.auto=false", "-c", "gc.auto=0"])
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .unwrap()
            .success();
        assert!(ok);
    }
    let mut n = start(
        &dir.path().join("node.db"),
        policy(dir.path(), ApprovalMode::Auto),
    )
    .await;
    let sid = n.create(&ws, Some(ApprovalMode::Auto), None, false).await;
    let agent = |id: &str, delegates| harness_core::agent::AgentSpec {
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
    };
    let task = script(&[
        ("bash", json!({"command": "sleep 3"})),
        ("bash", json!({"command": "touch second.txt"})),
    ]);
    n.send(prompt(
        sid,
        &format!("delegate: dev={task}"),
        Some(agent("lead", vec![agent("dev", vec![])])),
    ));
    // The sub-agent's first call runs unasked (the parent is in auto).
    let child = loop {
        if let NodeToMaster::Event { event } = n.next().await
            && event.session_id != sid
            && let Ok(EventKind::ToolCallStarted(c)) = event.typed()
            && c.input["command"] == "sleep 3"
        {
            break event.session_id;
        }
    };
    // Its own mode is not the person's to change, nor anyone's.
    n.send(MasterToNode::SetApprovalMode {
        req_id: "m-child".into(),
        session_id: child,
        mode: ApprovalMode::Auto,
    });
    let e = n.ack().await.unwrap_err();
    assert!(e.contains("sub-agent"), "{e}");
    // The parent lowered to ask while the sub-agent works.
    n.send(MasterToNode::SetApprovalMode {
        req_id: "m-parent".into(),
        session_id: sid,
        mode: ApprovalMode::Ask,
    });
    let mut acked = false;
    let mut asked = vec![];
    loop {
        match n.next().await {
            NodeToMaster::Ack { req_id, ok, error } if req_id == "m-parent" => {
                assert!(ok, "{error:?}");
                acked = true;
            }
            NodeToMaster::Event { event } => match event.typed().unwrap() {
                EventKind::ApprovalRequested(a) => {
                    n.send(MasterToNode::ApprovalRespond {
                        req_id: format!("a-{}", a.approval_id),
                        session_id: event.session_id,
                        approval_id: a.approval_id.clone(),
                        decision: Decision::Deny,
                    });
                    asked.push((event.session_id, a));
                }
                EventKind::TurnEnded(_) if event.session_id == sid => break,
                _ => {}
            },
            _ => {}
        }
    }
    assert!(acked);
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(asked[0].0, child);
    assert_eq!(asked[0].1.input["command"], "touch second.txt");

    // A routine's chat keeps the routine's setting.
    let routine = n.create(&ws, Some(ApprovalMode::Ask), None, true).await;
    let e = n
        .set_mode(routine, ApprovalMode::AcceptEdits)
        .await
        .unwrap_err();
    assert!(e.contains("routine"), "{e}");
    assert!(n.changes(routine).is_empty());
}
