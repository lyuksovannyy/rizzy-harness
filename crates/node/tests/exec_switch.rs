//! The owner's switch of a chat's execution mode (N.10b; docs/execution-modes.md,
//! PLAN.md T39) on a node, over the link as the master drives it, with the
//! scripted agent: an idle chat switches both ways and the log says so; a
//! switch is refused while anything of the chat runs, for a sub-agent's,
//! a routine's and a chat's session, and for a mode the node does not offer;
//! the chat's MCP servers do not survive the switch; a chat grant does not
//! carry across it; the model is told in the next turn's opening; a restart
//! and a fork read the switched mode from the log; a log entry naming an
//! unknown mode runs nothing; nothing a model writes switches anything.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::event::{EventKind, Profile};
use harness_core::exec_mode::{InForce, in_force};
use harness_core::policy::{ApprovalMode, Decision, ExecMode};
use harness_core::{RawEvent, SessionId};
use harness_node::exec::ExecOffers;
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster, PromptMsg};
use harness_store::{Durability, Store};
use serde_json::{Value, json};

fn offers(sandbox: bool, host: bool) -> ExecOffers {
    let off = |what: &str| Err(format!("{what} is off in this test"));
    ExecOffers::new(
        if sandbox { Ok(()) } else { off("sandbox") },
        if host { Ok(()) } else { off("host") },
        vec![],
    )
}

fn policy(root: &Path, o: ExecOffers) -> NodePolicy {
    NodePolicy {
        workspace_roots: vec![root.into()],
        max_approval_mode: ApprovalMode::Auto,
        exec: o,
        ..Default::default()
    }
}

/// The mock, keeping what the model was sent of every request.
struct Spy(Mock, Arc<std::sync::Mutex<Vec<String>>>);

#[async_trait::async_trait]
impl harness_agent_native::Provider for Spy {
    fn name(&self) -> &'static str {
        "spy"
    }
    async fn complete(
        &self,
        req: &harness_agent_native::ChatRequest,
        deltas: tokio::sync::mpsc::UnboundedSender<String>,
    ) -> Result<harness_agent_native::Completion, harness_agent_native::ProviderError> {
        self.1.lock().unwrap().push(format!("{:?}", req.messages));
        self.0.complete(req, deltas).await
    }
}

struct N {
    node: NodeRuntime,
    link: NodeLink,
    sent: Arc<std::sync::Mutex<Vec<String>>>,
    capabilities: Vec<String>,
}

async fn start(db: &Path, policy: NodePolicy, profile: Profile) -> N {
    let sent: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
    let node = NodeRuntime::start(NodeConfig {
        node_id: "n".into(),
        hostname: "test".into(),
        policy,
        store: Store::open(db, Durability::Full).unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Spy(
                Mock {
                    delay: Duration::from_millis(5),
                },
                sent.clone(),
            )),
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile,
        incognito_ttl: None,
        checkpoints_dir: Some(db.with_file_name("checkpoints")),
        git_sandbox: None,
        artifacts: None,
    })
    .await
    .unwrap();
    let mut link = node.attach().await;
    let capabilities = match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { capabilities, .. } => capabilities,
        other => panic!("expected hello, got {other:?}"),
    };
    N {
        node,
        link,
        sent,
        capabilities,
    }
}

impl N {
    fn send(&self, m: MasterToNode) {
        self.link.to_node.try_send(m).unwrap();
    }

    async fn next(&mut self) -> NodeToMaster {
        tokio::time::timeout(Duration::from_secs(20), self.link.from_node.recv())
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

    async fn create(
        &mut self,
        cwd: &Path,
        mode: ApprovalMode,
        exec: Option<ExecMode>,
    ) -> SessionId {
        self.create_as(cwd, mode, exec, false).await
    }

    async fn create_as(
        &mut self,
        cwd: &Path,
        mode: ApprovalMode,
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
            approval_mode: Some(mode),
            profile: None,
            agent: None,
            incognito: false,
            no_callbacks,
            exec_mode: exec,
        });
        self.ack().await.unwrap();
        sid
    }

    async fn switch(&mut self, sid: SessionId, mode: ExecMode) -> Result<(), String> {
        self.send(MasterToNode::SetExecMode {
            req_id: format!("x-{}", harness_core::new_id()),
            session_id: sid,
            mode,
        });
        self.ack().await
    }

    fn log(&self, sid: SessionId) -> Vec<RawEvent> {
        self.node.store().events_after(sid, 0, 100_000).unwrap()
    }

    fn mode(&self, sid: SessionId) -> InForce {
        in_force(&self.log(sid))
    }

    /// The ids of the turns of `sid` that ended, in order.
    fn turn_ids(&self, sid: SessionId) -> Vec<String> {
        self.log(sid)
            .iter()
            .filter_map(|e| match e.typed() {
                Ok(EventKind::TurnEnded(t)) => Some(t.turn_id),
                _ => None,
            })
            .collect()
    }

    /// A fork of `parent` at the end of `turn`: the new chat.
    async fn fork(&mut self, parent: SessionId, turn: &str) -> SessionId {
        let child = SessionId::new();
        self.send(MasterToNode::Fork {
            req_id: format!("f-{child}"),
            session_id: child,
            parent,
            turn_id: turn.into(),
            title: "t (fork)".into(),
        });
        self.ack().await.unwrap();
        child
    }

    fn changes(&self, sid: SessionId) -> Vec<(ExecMode, ExecMode)> {
        self.log(sid)
            .iter()
            .filter_map(|e| match e.typed() {
                Ok(EventKind::ExecModeChanged(c)) => Some((c.mode, c.from)),
                _ => None,
            })
            .collect()
    }

    /// One turn of `sid` with `text`, answering every card with `answer`;
    /// the cards in order, until its turn ends.
    async fn turn(
        &mut self,
        sid: SessionId,
        text: &str,
        answer: impl Fn(&harness_core::event::ApprovalRequested) -> Decision,
    ) -> Vec<harness_core::event::ApprovalRequested> {
        self.send(prompt(sid, text));
        let mut asked = vec![];
        loop {
            let NodeToMaster::Event { event } = self.next().await else {
                continue;
            };
            match event.typed().unwrap() {
                EventKind::ApprovalRequested(a) => {
                    self.send(MasterToNode::ApprovalRespond {
                        req_id: format!("a-{}", a.approval_id),
                        session_id: event.session_id,
                        approval_id: a.approval_id.clone(),
                        decision: answer(&a),
                        note: None,
                    });
                    asked.push(a);
                }
                EventKind::TurnEnded(_) if event.session_id == sid => return asked,
                _ => {}
            }
        }
    }
}

fn prompt(sid: SessionId, text: &str) -> MasterToNode {
    MasterToNode::Prompt(PromptMsg {
        req_id: format!("p-{sid}-{}", harness_core::new_id()),
        ..PromptMsg::test(sid, text)
    })
}

fn script(calls: &[(&str, Value)]) -> String {
    let steps: Vec<Value> = calls
        .iter()
        .map(|(tool, input)| json!({"tool": tool, "input": input}))
        .collect();
    format!("script:{}", Value::Array(steps))
}

fn ws(dir: &Path) -> PathBuf {
    let ws = dir.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    ws
}

/// An idle chat switches both ways; the event says the mode and where it
/// came from; asking for the mode it is in records nothing.
#[tokio::test]
async fn an_idle_chat_switches_both_ways_and_the_log_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let mut n = start(
        &dir.path().join("node.db"),
        policy(dir.path(), offers(true, true)),
        Profile::Agentic,
    )
    .await;
    assert!(
        n.capabilities
            .contains(&harness_protocol::CAP_EXEC_SWITCH.to_string()),
        "the hello announces the switch"
    );
    let sid = n.create(&ws(dir.path()), ApprovalMode::Ask, None).await;
    assert_eq!(n.mode(sid), InForce::Mode(ExecMode::Sandbox));
    n.switch(sid, ExecMode::Host).await.unwrap();
    assert_eq!(n.mode(sid), InForce::Mode(ExecMode::Host));
    n.switch(sid, ExecMode::Host).await.unwrap();
    assert_eq!(n.changes(sid).len(), 1, "no change, no event");
    n.switch(sid, ExecMode::Sandbox).await.unwrap();
    assert_eq!(
        n.changes(sid),
        [
            (ExecMode::Host, ExecMode::Sandbox),
            (ExecMode::Sandbox, ExecMode::Host)
        ]
    );
    // The creation's event is untouched: the mode is the latest change.
    let first = n.log(sid)[0].payload["exec_mode"].clone();
    assert_eq!(first, json!("sandbox"));
    // And the chat still works in it.
    n.turn(sid, "hello", |_| Decision::Allow).await;
}

/// Nothing of the chat may run: a turn (with its approval waiting) refuses
/// the switch and records nothing; once the turn has ended it goes through.
#[tokio::test]
async fn a_switch_is_refused_while_a_turn_runs_and_while_an_approval_waits() {
    let dir = tempfile::tempdir().unwrap();
    let mut n = start(
        &dir.path().join("node.db"),
        policy(dir.path(), offers(true, true)),
        Profile::Agentic,
    )
    .await;
    let sid = n.create(&ws(dir.path()), ApprovalMode::Ask, None).await;
    n.send(prompt(
        sid,
        &script(&[("bash", json!({"command": "echo waiting"}))]),
    ));
    let card = loop {
        if let NodeToMaster::Event { event } = n.next().await
            && let Ok(EventKind::ApprovalRequested(a)) = event.typed()
        {
            break a;
        }
    };
    let e = n.switch(sid, ExecMode::Host).await.unwrap_err();
    assert!(e.contains("while the chat works"), "{e}");
    assert!(n.changes(sid).is_empty(), "nothing recorded");
    assert_eq!(n.mode(sid), InForce::Mode(ExecMode::Sandbox));
    // The card is still there to answer; after the turn it goes through.
    n.send(MasterToNode::ApprovalRespond {
        req_id: "a".into(),
        session_id: sid,
        approval_id: card.approval_id,
        decision: Decision::Allow,
        note: None,
    });
    loop {
        if let NodeToMaster::Event { event } = n.next().await
            && event.kind == "turn_ended"
        {
            break;
        }
    }
    n.switch(sid, ExecMode::Host).await.unwrap();
    assert_eq!(n.mode(sid), InForce::Mode(ExecMode::Host));
}

/// Not for a routine's chat, a chat without tools, and not for a mode the
/// node does not offer: the node decides again whatever the master checked.
#[tokio::test]
async fn the_node_refuses_a_routine_a_chat_and_a_mode_it_does_not_offer() {
    let dir = tempfile::tempdir().unwrap();
    // A node that offers only the sandbox: `host` is refused with its reason.
    let mut n = start(
        &dir.path().join("a.db"),
        policy(dir.path(), offers(true, false)),
        Profile::Agentic,
    )
    .await;
    let sid = n.create(&ws(dir.path()), ApprovalMode::Ask, None).await;
    let e = n.switch(sid, ExecMode::Host).await.unwrap_err();
    assert!(e.contains("does not offer the host mode"), "{e}");
    assert!(n.changes(sid).is_empty());
    // A routine's chat runs in the sandbox for good.
    let routine = n
        .create_as(&ws(dir.path()), ApprovalMode::Ask, None, true)
        .await;
    let e = n.switch(routine, ExecMode::Host).await.unwrap_err();
    assert!(e.contains("routine"), "{e}");
    assert_eq!(n.mode(routine), InForce::Mode(ExecMode::Sandbox));
    // An unknown chat.
    assert!(n.switch(SessionId::new(), ExecMode::Host).await.is_err());
    drop(n);
    // A node whose host mode is off because the sandbox is required and
    // broken says so, and a host chat cannot leave to a mode it lost.
    let required = ExecOffers::new(
        Ok(()),
        Err("[sandbox] mode = \"required\" and the Landlock sandbox does not work".into()),
        vec![],
    );
    let mut n = start(
        &dir.path().join("b.db"),
        policy(dir.path(), required),
        Profile::Agentic,
    )
    .await;
    let sid = n.create(&ws(dir.path()), ApprovalMode::Ask, None).await;
    let e = n.switch(sid, ExecMode::Host).await.unwrap_err();
    assert!(e.contains("\"required\""), "{e}");
    drop(n);
    // A chat session (no tools) has no mode to switch.
    let mut n = start(
        &dir.path().join("c.db"),
        policy(dir.path(), offers(true, true)),
        Profile::Chat,
    )
    .await;
    let sid = SessionId::new();
    n.send(MasterToNode::Create {
        req_id: "c".into(),
        session_id: sid,
        title: "t".into(),
        cwd: String::new(),
        model: None,
        approval_mode: None,
        profile: Some(Profile::Chat),
        agent: None,
        incognito: false,
        no_callbacks: false,
        exec_mode: None,
    });
    n.ack().await.unwrap();
    let e = n.switch(sid, ExecMode::Host).await.unwrap_err();
    assert!(e.contains("no commands"), "{e}");
}

/// A call allowed "for this chat" in one mode is asked about again in the
/// other, and again when the chat comes back: the person decided about that
/// shell. Each grant is removed by an event, so the list shows it.
#[tokio::test]
async fn a_chat_grant_does_not_carry_across_a_switch() {
    let dir = tempfile::tempdir().unwrap();
    let ws = ws(dir.path());
    let call = script(&[("bash", json!({"command": "echo granted"}))]);
    let deny = |_: &harness_core::event::ApprovalRequested| Decision::Deny;
    let mut n = start(
        &dir.path().join("node.db"),
        policy(dir.path(), offers(true, true)),
        Profile::Agentic,
    )
    .await;
    let sid = n.create(&ws, ApprovalMode::Ask, None).await;
    let asked = n.turn(sid, &call, |_| Decision::AllowChat).await;
    assert_eq!(asked.len(), 1);
    assert!(n.turn(sid, &call, deny).await.is_empty(), "covered");
    n.switch(sid, ExecMode::Host).await.unwrap();
    let removed = n
        .log(sid)
        .iter()
        .filter(|e| e.kind == "chat_allow_removed")
        .count();
    assert_eq!(removed, 1, "the grant is removed by an event");
    // Before the mode event: a restart between them leaves fewer grants.
    let kinds: Vec<String> = n.log(sid).iter().map(|e| e.kind.clone()).collect();
    let (r, c) = (
        kinds
            .iter()
            .position(|k| k == "chat_allow_removed")
            .unwrap(),
        kinds.iter().position(|k| k == "exec_mode_changed").unwrap(),
    );
    assert!(r < c, "{kinds:?}");
    // Asked again on the host shell, and its card says so.
    let asked = n.turn(sid, &call, |_| Decision::AllowChat).await;
    assert_eq!(asked.len(), 1, "a grant of the sandbox covers no host call");
    assert!(
        asked[0].reason.contains("host shell"),
        "{}",
        asked[0].reason
    );
    assert!(n.turn(sid, &call, deny).await.is_empty(), "the host grant");
    // And back: the host grant covers nothing in the sandbox.
    n.switch(sid, ExecMode::Sandbox).await.unwrap();
    assert_eq!(n.turn(sid, &call, deny).await.len(), 1);
}

// The chat's MCP servers at a switch run under the real confinements:
// `crates/cli/tests/mcp_sandbox.rs` `a_switch_stops_the_servers_and_they_start_under_the_new_mode`
// (this rig's host confinement refuses commands).

/// The model is told in the opening of the next turn (the environment
/// line the log keeps), once: a turn after that, with no further switch,
/// carries no "until this turn".
#[tokio::test]
async fn the_next_turn_tells_the_model_the_mode_changed() {
    let dir = tempfile::tempdir().unwrap();
    let mut n = start(
        &dir.path().join("node.db"),
        policy(dir.path(), offers(true, true)),
        Profile::Agentic,
    )
    .await;
    let sid = n.create(&ws(dir.path()), ApprovalMode::Ask, None).await;
    n.turn(sid, "hello", |_| Decision::Allow).await;
    n.switch(sid, ExecMode::Host).await.unwrap();
    n.turn(sid, "again", |_| Decision::Allow).await;
    n.turn(sid, "and again", |_| Decision::Allow).await;
    let envs: Vec<String> = n
        .log(sid)
        .iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::TurnStarted(t)) => t.environment,
            _ => None,
        })
        .collect();
    assert_eq!(envs.len(), 3, "{envs:?}");
    assert!(envs[0].contains("with bash in the sandbox,"), "{}", envs[0]);
    assert!(!envs[0].contains("until this turn"));
    assert!(
        envs[1].contains("ran in the sandbox until this turn and now run in the host shell"),
        "{}",
        envs[1]
    );
    assert!(
        envs[2].contains("with bash in the host shell"),
        "{}",
        envs[2]
    );
    assert!(!envs[2].contains("until this turn"), "{}", envs[2]);
    // What the model was sent is that line, from the log.
    let sent = n.sent.lock().unwrap().clone();
    assert!(
        sent.iter()
            .any(|m| m.contains("ran in the sandbox until this turn")),
        "the model was never told"
    );
}

/// After a restart the mode is the switched one (the log's latest), a
/// node that no longer offers it runs nothing, and a fork carries the mode
/// of its cut: the change copied with the events, not the parent's mode now.
#[tokio::test]
async fn a_restart_and_a_fork_read_the_switched_mode_from_the_log() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("node.db");
    let ws = ws(dir.path());
    let (sid, first_turn, second_turn) = {
        let mut n = start(
            &db,
            policy(dir.path(), offers(true, true)),
            Profile::Agentic,
        )
        .await;
        let sid = n.create(&ws, ApprovalMode::Ask, None).await;
        n.turn(sid, "one", |_| Decision::Allow).await;
        n.switch(sid, ExecMode::Host).await.unwrap();
        n.turn(sid, "two", |_| Decision::Allow).await;
        let turns: Vec<String> = n
            .log(sid)
            .iter()
            .filter_map(|e| match e.typed() {
                Ok(EventKind::TurnEnded(t)) => Some(t.turn_id),
                _ => None,
            })
            .collect();
        // A fork starts in the mode its parent has NOW, whatever its cut:
        // the fork of the turn before the switch says so in an event of its
        // own (after `session_forked`); the one after it needs none.
        for (i, turn) in turns.iter().enumerate() {
            let child = n.fork(sid, turn).await;
            assert_eq!(n.mode(child), InForce::Mode(ExecMode::Host), "fork {i}");
            let own: Vec<(ExecMode, ExecMode)> = n.changes(child);
            assert_eq!(own, [(ExecMode::Host, ExecMode::Sandbox)], "fork {i}");
            let forked_at = n
                .log(child)
                .iter()
                .position(|e| e.kind == "session_forked")
                .unwrap();
            let change_at = n
                .log(child)
                .iter()
                .rposition(|e| e.kind == "exec_mode_changed")
                .unwrap();
            assert_eq!(
                change_at > forked_at,
                i == 0,
                "fork {i}: the fork's own event comes after its divider only for the earlier cut"
            );
        }
        (sid, turns[0].clone(), turns[1].clone())
    };
    let _ = (first_turn, second_turn);
    // Restarted on a node that offers only the sandbox: the chat is on the
    // host, so it runs nothing and says so; the switched mode was read.
    {
        let mut n = start(
            &db,
            policy(dir.path(), offers(true, false)),
            Profile::Agentic,
        )
        .await;
        n.send(prompt(sid, "hello"));
        let e = n.ack().await.unwrap_err();
        assert!(e.contains("runs in the host mode"), "{e}");
        // ...and its owner can bring it back to a mode this node offers.
        n.switch(sid, ExecMode::Sandbox).await.unwrap();
        n.turn(sid, "hello", |_| Decision::Allow).await;
    }
    // On a node offering both, a card of the host chat says the host shell.
    let mut n = start(
        &db,
        policy(dir.path(), offers(true, true)),
        Profile::Agentic,
    )
    .await;
    n.switch(sid, ExecMode::Host).await.unwrap();
    let cards = n
        .turn(
            sid,
            &script(&[("bash", json!({"command": "true"}))]),
            |_| Decision::Allow,
        )
        .await;
    assert!(
        cards[0].reason.contains("host shell"),
        "{}",
        cards[0].reason
    );
}

/// D2: a fork made after the parent came back to the sandbox, at a cut that
/// was in the host shell, is a sandbox chat (log and fold say so, with its
/// own `exec_mode_changed`); the reverse likewise; a fork of a chat that was
/// never switched has none.
#[tokio::test]
async fn a_fork_starts_in_the_mode_its_parent_has_now() {
    let dir = tempfile::tempdir().unwrap();
    let mut n = start(
        &dir.path().join("node.db"),
        policy(dir.path(), offers(true, true)),
        Profile::Agentic,
    )
    .await;
    let sid = n.create(&ws(dir.path()), ApprovalMode::Ask, None).await;
    n.turn(sid, "one", |_| Decision::Allow).await;
    let plain = n.fork(sid, &n.turn_ids(sid)[0]).await;
    assert_eq!(n.mode(plain), InForce::Mode(ExecMode::Sandbox));
    assert!(n.changes(plain).is_empty(), "no switch, no event");
    n.switch(sid, ExecMode::Host).await.unwrap();
    n.turn(sid, "two", |_| Decision::Allow).await;
    n.switch(sid, ExecMode::Sandbox).await.unwrap();
    let turns = n.turn_ids(sid);
    // The cut in the host shell, the parent in the sandbox now.
    let f = n.fork(sid, &turns[1]).await;
    assert_eq!(n.mode(f), InForce::Mode(ExecMode::Sandbox));
    assert_eq!(
        n.changes(f).last(),
        Some(&(ExecMode::Sandbox, ExecMode::Host))
    );
    // Its first command asks as a sandbox chat's, not as the host's.
    let cards = n
        .turn(f, &script(&[("bash", json!({"command": "true"}))]), |_| {
            Decision::Allow
        })
        .await;
    assert!(
        !cards[0].reason.contains("host shell"),
        "{}",
        cards[0].reason
    );
    // And the reverse: the parent on the host, the cut in the sandbox.
    n.switch(sid, ExecMode::Host).await.unwrap();
    let g = n.fork(sid, &turns[0]).await;
    assert_eq!(n.mode(g), InForce::Mode(ExecMode::Host));
    assert_eq!(
        n.changes(g).last(),
        Some(&(ExecMode::Host, ExecMode::Sandbox))
    );
}

/// A log entry that says an unknown mode (a hostile or corrupted log, a
/// node from the future) is "no mode known": nothing runs, and it is not
/// repaired by a switch either.
#[tokio::test]
async fn an_exec_mode_changed_with_an_unknown_mode_runs_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("node.db");
    let sid = {
        let mut n = start(
            &db,
            policy(dir.path(), offers(true, true)),
            Profile::Agentic,
        )
        .await;
        let sid = n.create(&ws(dir.path()), ApprovalMode::Ask, None).await;
        n.turn(sid, "hello", |_| Decision::Allow).await;
        sid
    };
    {
        let store = Store::open(&db, Durability::Full).unwrap();
        let head = store.events_after(sid, 0, 100_000).unwrap().len() as u64;
        store
            .append(
                "n",
                &RawEvent {
                    session_id: sid,
                    seq: head + 1,
                    ts_ms: harness_core::now_ms(),
                    kind: "exec_mode_changed".into(),
                    payload: json!({"mode": "quantum", "from": "sandbox"}),
                },
            )
            .unwrap();
    }
    let mut n = start(
        &db,
        policy(dir.path(), offers(true, true)),
        Profile::Agentic,
    )
    .await;
    let marker = dir.path().join("ran.txt");
    n.send(prompt(
        sid,
        &format!("run: echo ran > {}", marker.display()),
    ));
    let e = n.ack().await.unwrap_err();
    assert!(e.contains("no execution mode"), "{e}");
    assert!(!marker.exists());
    let e = n.switch(sid, ExecMode::Sandbox).await.unwrap_err();
    assert!(e.contains("not known here"), "{e}");
}

/// A sub-agent started after a switch runs in the chat's new mode.
#[tokio::test]
async fn a_sub_agent_started_after_a_switch_takes_the_new_mode() {
    let dir = tempfile::tempdir().unwrap();
    let ws = ws(dir.path());
    std::fs::write(ws.join("keep.txt"), "hello\n").unwrap();
    let mut n = start(
        &dir.path().join("node.db"),
        policy(dir.path(), offers(true, true)),
        Profile::Agentic,
    )
    .await;
    let parent = n
        .create(&ws, ApprovalMode::Auto, Some(ExecMode::Sandbox))
        .await;
    n.switch(parent, ExecMode::Host).await.unwrap();
    let spec = |id: &str, tools: Vec<String>, delegates| harness_core::agent::AgentSpec {
        agent_id: id.into(),
        name: id.into(),
        system_prompt: "work".into(),
        tools: harness_core::agent::ToolSelection::List(tools),
        model: None,
        fallbacks: vec![],
        unavailable: vec![],
        no_effort: vec![],
        vision: vec![],
        context_windows: Default::default(),
        effort: None,
        ui: Default::default(),
        no_personalisation: false,
        prompt_hash: "h".into(),
        deny_tools: vec![],
        deny_skills: vec![],
        deny_mcp_servers: vec![],
        stored_skills: vec![],
        connector_tools: vec![],
        delegates,
        caps: None,
    };
    let scout = spec("scout", vec!["read".into()], vec![]);
    let lead = spec("lead", vec!["bash".into()], vec![scout]);
    n.send(MasterToNode::Prompt(PromptMsg {
        req_id: "p".into(),
        agent: Some(lead),
        ..PromptMsg::test(parent, "delegate: scout=read: keep.txt")
    }));
    n.ack().await.unwrap();
    let by = std::time::Instant::now() + Duration::from_secs(20);
    while !n.log(parent).iter().any(|e| e.kind == "turn_ended") {
        assert!(std::time::Instant::now() < by, "the turn did not end");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let sessions = n.node.store().sessions().unwrap();
    assert_eq!(sessions.len(), 2, "{sessions:?}");
    for s in sessions {
        let log = n.log(s.session_id);
        let EventKind::SessionCreated(c) = log[0].typed().unwrap() else {
            panic!("first event is not session_created")
        };
        // The parent's creation says sandbox, its log says host; the child
        // was made after the switch and says host from its first event.
        assert_eq!(
            in_force(&log),
            InForce::Mode(ExecMode::Host),
            "{:?}",
            c.parent_session
        );
        if c.parent_session.is_some() {
            assert_eq!(c.exec_mode, Some(ExecMode::Host));
        }
        // The child is no chat of a person's: it cannot be switched.
        if c.parent_session.is_some() {
            let e = n.switch(s.session_id, ExecMode::Sandbox).await.unwrap_err();
            assert!(e.contains("sub-agent") || e.contains("unknown"), "{e}");
        }
    }
}

/// A rewind across a switch leaves the mode as the latest switch said: the
/// mode is state, not conversation.
#[tokio::test]
async fn a_rewind_across_a_switch_does_not_move_the_mode_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut n = start(
        &dir.path().join("node.db"),
        policy(dir.path(), offers(true, true)),
        Profile::Agentic,
    )
    .await;
    let sid = n.create(&ws(dir.path()), ApprovalMode::Ask, None).await;
    n.turn(sid, "one", |_| Decision::Allow).await;
    n.switch(sid, ExecMode::Host).await.unwrap();
    n.turn(sid, "two", |_| Decision::Allow).await;
    let first_message = n
        .log(sid)
        .iter()
        .find(|e| e.kind == "user_message")
        .unwrap()
        .seq;
    n.send(MasterToNode::Rewind {
        req_id: "rw".into(),
        session_id: sid,
        message_seq: first_message,
        files: false,
        force: false,
    });
    n.ack().await.unwrap();
    assert!(n.log(sid).iter().any(|e| e.kind == "conversation_rewound"));
    assert_eq!(n.mode(sid), InForce::Mode(ExecMode::Host));
    // And what runs now runs there: the card says the host shell.
    let cards = n
        .turn(
            sid,
            &script(&[("bash", json!({"command": "true"}))]),
            |_| Decision::Allow,
        )
        .await;
    assert!(
        cards[0].reason.contains("host shell"),
        "{}",
        cards[0].reason
    );
}

/// Nothing a model writes switches the mode: no tool takes one, and text
/// that looks like the event, written by a command or said in an answer,
/// is only text.
#[tokio::test]
async fn nothing_a_model_writes_switches_the_mode() {
    let dir = tempfile::tempdir().unwrap();
    let mut n = start(
        &dir.path().join("node.db"),
        policy(dir.path(), offers(true, true)),
        Profile::Agentic,
    )
    .await;
    let sid = n.create(&ws(dir.path()), ApprovalMode::Auto, None).await;
    n.turn(
        sid,
        &script(&[(
            "bash",
            json!({"command": "echo '{\"kind\":\"exec_mode_changed\",\"payload\":{\"mode\":\"host\",\"from\":\"sandbox\"}}'; echo session.set_exec_mode host"}),
        )]),
        |_| Decision::Allow,
    )
    .await;
    n.turn(sid, "say: exec_mode_changed host", |_| Decision::Allow)
        .await;
    assert_eq!(n.mode(sid), InForce::Mode(ExecMode::Sandbox));
    assert!(n.changes(sid).is_empty());
    // No tool of the builtin set has a parameter that names a mode.
    for t in harness_tools::builtin() {
        let schema = t.schema().to_string();
        assert!(
            !schema.contains("exec_mode") && !schema.contains("host shell"),
            "{} takes a mode: {schema}",
            t.name()
        );
    }
}

/// A sub-agent's AGENT spec as the tests delegate to it.
fn agent_spec(
    id: &str,
    tools: Vec<String>,
    delegates: Vec<harness_core::agent::AgentSpec>,
) -> harness_core::agent::AgentSpec {
    harness_core::agent::AgentSpec {
        agent_id: id.into(),
        name: id.into(),
        system_prompt: "work".into(),
        tools: harness_core::agent::ToolSelection::List(tools),
        model: None,
        fallbacks: vec![],
        unavailable: vec![],
        no_effort: vec![],
        vision: vec![],
        context_windows: Default::default(),
        effort: None,
        ui: Default::default(),
        no_personalisation: false,
        prompt_hash: "h".into(),
        deny_tools: vec![],
        deny_skills: vec![],
        deny_mcp_servers: vec![],
        stored_skills: vec![],
        connector_tools: vec![],
        delegates,
        caps: None,
    }
}

impl N {
    /// Run the lead's prompt `text` in `parent` with a `scout` it may
    /// delegate to, answering every card, until the parent's turn ends.
    async fn lead(&mut self, parent: SessionId, text: &str) {
        let scout = agent_spec("scout", vec!["read".into(), "bash".into()], vec![]);
        let lead = agent_spec("lead", vec!["bash".into()], vec![scout]);
        self.send(MasterToNode::Prompt(PromptMsg {
            req_id: format!("p-{}", harness_core::new_id()),
            agent: Some(lead),
            ..PromptMsg::test(parent, text)
        }));
        loop {
            let NodeToMaster::Event { event } = self.next().await else {
                continue;
            };
            match event.typed().unwrap() {
                EventKind::ApprovalRequested(a) => self.send(MasterToNode::ApprovalRespond {
                    req_id: format!("a-{}", a.approval_id),
                    session_id: event.session_id,
                    approval_id: a.approval_id.clone(),
                    decision: Decision::Allow,
                    note: None,
                }),
                EventKind::TurnEnded(_) if event.session_id == parent => return,
                _ => {}
            }
        }
    }

    fn child_of(&self, parent: SessionId) -> SessionId {
        self.node
            .store()
            .sessions()
            .unwrap()
            .into_iter()
            .map(|s| s.session_id)
            .find(|s| *s != parent)
            .expect("a child")
    }
}

/// B1: a sub-agent's chat runs in the mode its own log says, for good (it
/// has no switch). One made BEFORE its chat was switched takes no turn from
/// anyone afterwards, in either direction: the model's follow-up through
/// `delegate` and the person's message are refused with a sentence that says
/// what to do, and nothing runs; once the chat is back in the child's mode
/// it works again.
#[tokio::test]
async fn a_sub_agent_made_before_a_switch_takes_no_turn_after_it() {
    for (start_mode, then) in [
        (ExecMode::Sandbox, ExecMode::Host),
        (ExecMode::Host, ExecMode::Sandbox),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let ws = ws(dir.path());
        std::fs::write(ws.join("keep.txt"), "hello\n").unwrap();
        let mut n = start(
            &dir.path().join("node.db"),
            policy(dir.path(), offers(true, true)),
            Profile::Agentic,
        )
        .await;
        let parent = n.create(&ws, ApprovalMode::Auto, Some(start_mode)).await;
        n.lead(parent, "delegate: scout=read: keep.txt").await;
        let child = n.child_of(parent);
        assert_eq!(n.mode(child), InForce::Mode(start_mode));
        let turns_before = n
            .log(child)
            .iter()
            .filter(|e| e.kind == "turn_started")
            .count();
        n.switch(parent, then).await.unwrap();
        // The child's own mode did not move.
        assert_eq!(n.mode(child), InForce::Mode(start_mode));
        let words = |m: ExecMode| {
            if m == ExecMode::Host {
                "the host shell"
            } else {
                "the sandbox"
            }
        };
        let want = format!(
            "ran in {} and this chat's commands now run in {}: start a new sub-agent",
            words(start_mode),
            words(then)
        );
        // The person's message.
        let marker = dir.path().join("ran.txt");
        n.send(prompt(
            child,
            &format!("run: echo ran > {}", marker.display()),
        ));
        let e = n.ack().await.unwrap_err();
        assert!(e.contains(&want), "{start_mode:?}: {e}");
        // The model's follow-up.
        n.lead(
            parent,
            &script(&[(
                "delegate",
                json!({"agent": "scout", "session": child.to_string(), "task": "read: keep.txt"}),
            )]),
        )
        .await;
        let outs: Vec<String> = n
            .log(parent)
            .iter()
            .filter_map(|e| match e.typed() {
                Ok(EventKind::ToolCallFinished(f)) => Some(f.output),
                _ => None,
            })
            .collect();
        assert!(
            outs.last().unwrap().contains(&want),
            "{start_mode:?}: {outs:?}"
        );
        assert!(!marker.exists(), "something ran");
        assert_eq!(
            n.log(child)
                .iter()
                .filter(|e| e.kind == "turn_started")
                .count(),
            turns_before,
            "the child took a turn"
        );
        // Back in its mode, the child takes a message again.
        n.switch(parent, start_mode).await.unwrap();
        n.send(prompt(child, "hello again"));
        n.ack().await.unwrap();
    }
}

/// D1: the chat's switch is refused while a turn of a sub-agent of it runs
/// or one of its approvals waits (its calls would change confinement
/// between two of them); the card it showed still said what it ran in.
#[tokio::test]
async fn a_switch_is_refused_while_a_sub_agents_turn_waits() {
    let dir = tempfile::tempdir().unwrap();
    let ws = ws(dir.path());
    std::fs::write(ws.join("keep.txt"), "hello\n").unwrap();
    let mut n = start(
        &dir.path().join("node.db"),
        policy(dir.path(), offers(true, true)),
        Profile::Agentic,
    )
    .await;
    let parent = n
        .create(&ws, ApprovalMode::Ask, Some(ExecMode::Sandbox))
        .await;
    n.lead(parent, "delegate: scout=read: keep.txt").await;
    let child = n.child_of(parent);
    n.send(prompt(
        child,
        &script(&[
            ("bash", json!({"command": "echo first"})),
            ("bash", json!({"command": "echo second"})),
        ]),
    ));
    let first = loop {
        let NodeToMaster::Event { event } = n.next().await else {
            continue;
        };
        if let Ok(EventKind::ApprovalRequested(a)) = event.typed() {
            break a;
        }
    };
    let e = n.switch(parent, ExecMode::Host).await.unwrap_err();
    assert!(e.contains("a sub-agent of this chat"), "{e}");
    assert!(e.contains(&child.to_string()), "{e}");
    assert!(n.changes(parent).is_empty());
    // Both calls of the child's turn run in the mode they started in.
    let mut reasons = vec![first.reason.clone()];
    n.send(MasterToNode::ApprovalRespond {
        req_id: "a1".into(),
        session_id: child,
        approval_id: first.approval_id,
        decision: Decision::Allow,
        note: None,
    });
    loop {
        let NodeToMaster::Event { event } = n.next().await else {
            continue;
        };
        if event.session_id != child {
            continue;
        }
        if let Ok(EventKind::ApprovalRequested(a)) = event.typed() {
            reasons.push(a.reason.clone());
            n.send(MasterToNode::ApprovalRespond {
                req_id: "a2".into(),
                session_id: child,
                approval_id: a.approval_id,
                decision: Decision::Allow,
                note: None,
            });
        }
        if event.kind == "turn_ended" {
            break;
        }
    }
    assert!(
        reasons.iter().all(|r| !r.contains("host shell")),
        "{reasons:?}"
    );
    // Its turn over, the switch goes through.
    n.switch(parent, ExecMode::Host).await.unwrap();
}

/// While the chat above it is being switched, a sub-agent's chat takes no
/// turn (second review, N1): a message that arrived in that window would
/// run a whole turn in the mode the chat is leaving. The window is raised
/// by a test hook, so this runs only where the hook is built.
#[cfg(feature = "testing")]
#[tokio::test]
async fn a_sub_agents_chat_takes_no_turn_while_its_chat_is_being_switched() {
    let dir = tempfile::tempdir().unwrap();
    let ws = ws(dir.path());
    std::fs::write(ws.join("keep.txt"), "hello\n").unwrap();
    let mut n = start(
        &dir.path().join("node.db"),
        policy(dir.path(), offers(true, true)),
        Profile::Agentic,
    )
    .await;
    let parent = n
        .create(&ws, ApprovalMode::Auto, Some(ExecMode::Host))
        .await;
    n.lead(parent, "delegate: scout=read: keep.txt").await;
    let child = n.child_of(parent);
    let turns = |n: &N| {
        n.log(child)
            .iter()
            .filter(|e| e.kind == "turn_started")
            .count()
    };
    let before = turns(&n);
    n.node.hold_exec_switch(parent, true);
    n.send(prompt(child, "read: keep.txt"));
    let e = n.ack().await.unwrap_err();
    assert!(e.contains("being switched right now"), "{e}");
    assert_eq!(turns(&n), before, "nothing started in the child's chat");
    // The mark is the chat's own: once it is lowered the same message runs.
    n.node.hold_exec_switch(parent, false);
    n.send(prompt(child, "read: keep.txt"));
    n.ack().await.unwrap();
}

/// A chat whose mode is not known runs nothing, and neither do its
/// sub-agents' chats, whatever mode they were made in (second review, N2).
#[tokio::test]
async fn a_sub_agent_of_a_chat_with_an_unknown_mode_runs_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("node.db");
    let ws = ws(dir.path());
    std::fs::write(ws.join("keep.txt"), "hello\n").unwrap();
    let (parent, child) = {
        let mut n = start(
            &db,
            policy(dir.path(), offers(true, true)),
            Profile::Agentic,
        )
        .await;
        let parent = n
            .create(&ws, ApprovalMode::Auto, Some(ExecMode::Sandbox))
            .await;
        n.lead(parent, "delegate: scout=read: keep.txt").await;
        (parent, n.child_of(parent))
    };
    {
        let store = Store::open(&db, Durability::Full).unwrap();
        let head = store.events_after(parent, 0, 100_000).unwrap().len() as u64;
        store
            .append(
                "n",
                &RawEvent {
                    session_id: parent,
                    seq: head + 1,
                    ts_ms: harness_core::now_ms(),
                    kind: "exec_mode_changed".into(),
                    payload: json!({"mode": "quantum", "from": "sandbox"}),
                },
            )
            .unwrap();
    }
    let mut n = start(
        &db,
        policy(dir.path(), offers(true, true)),
        Profile::Agentic,
    )
    .await;
    assert_eq!(n.mode(child), InForce::Mode(ExecMode::Sandbox));
    let marker = dir.path().join("ran.txt");
    n.send(prompt(
        child,
        &format!("run: echo ran > {}", marker.display()),
    ));
    let e = n.ack().await.unwrap_err();
    assert!(e.contains("not known here"), "{e}");
    assert!(!marker.exists());
}
