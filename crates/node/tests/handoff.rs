//! Hand-off notes on a real node (ADOPTION_PLAN X.16, PLAN.md T60): the
//! note is the session's own event, written once per stretch of work by
//! the small model or, when this node does not know it, the chat's own;
//! a person's message while it is written goes first and the note is
//! dropped; a node that was down writes it when asked after its restart;
//! a sub-agent's or a routine's chat never gets one.

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::llm::{ModelRouter, Route};
use harness_agent_native::{
    ChatMsg, ChatRequest, Completion, HANDOFF_PROMPT, NativeBackend, NativeConfig, Provider,
    ProviderError, mock::Mock,
};
use harness_core::agent::{AgentSpec, ToolSelection};
use harness_core::event::*;
use harness_core::policy::ApprovalMode;
use harness_core::view::{Item, SessionView};
use harness_core::{RawEvent, SessionId};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{HANDOFF_BUSY, HANDOFF_NOTHING, MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};
use tokio::sync::mpsc;

/// The mock, slow on hand-off requests when `slow`.
struct Slow {
    slow: bool,
}

#[async_trait::async_trait]
impl Provider for Slow {
    fn name(&self) -> &'static str {
        "mock"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        d: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        if self.slow
            && matches!(req.messages.first(), Some(ChatMsg::System(p)) if p == HANDOFF_PROMPT)
        {
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
        Mock {
            delay: Duration::from_millis(1),
        }
        .complete(req, d)
        .await
    }
}

/// A node whose only provider is `demo` (so `gone::x` is a model it does
/// not know), its chats on `demo::mock`.
fn config(dir: &std::path::Path, slow: bool) -> NodeConfig {
    config_with(dir, slow, harness_node::policy::DEFAULT_HANDOFF_PER_HOUR)
}

fn config_with(dir: &std::path::Path, slow: bool, per_hour: usize) -> NodeConfig {
    let router = ModelRouter::new(vec![Route {
        prefix: "demo".into(),
        provider: Arc::new(Slow { slow }),
    }]);
    NodeConfig {
        node_id: "local".into(),
        hostname: "test".into(),
        policy: NodePolicy {
            workspace_roots: vec![dir.into()],
            max_approval_mode: ApprovalMode::Auto,
            limits: harness_node::policy::LimitsPolicy {
                handoff_notes_per_hour: per_hour,
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

async fn attach(node: &NodeRuntime) -> NodeLink {
    let mut link = node.attach().await;
    match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { capabilities, .. } => {
            assert!(
                capabilities
                    .iter()
                    .any(|c| c == harness_protocol::CAP_HANDOFF)
            );
        }
        other => panic!("expected hello, got {other:?}"),
    }
    link
}

/// Events until one for `sid` matches; acks must be ok.
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
            NodeToMaster::Ack { ok, error, .. } => assert!(ok, "nack: {error:?}"),
            _ => {}
        }
    }
}

/// The answer to the command `req_id`, events skipped.
async fn ack(link: &mut NodeLink, req_id: &str) -> Result<(), String> {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(10), link.from_node.recv())
            .await
            .expect("timed out waiting for an ack")
            .expect("link closed");
        if let NodeToMaster::Ack {
            req_id: r,
            ok,
            error,
        } = msg
            && r == req_id
        {
            return if ok {
                Ok(())
            } else {
                Err(error.unwrap_or_default())
            };
        }
    }
}

async fn create(link: &mut NodeLink, ws: &std::path::Path, no_callbacks: bool) -> SessionId {
    let sid = SessionId::new();
    link.to_node
        .try_send(MasterToNode::Create {
            exec_mode: None,
            no_callbacks,
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
    until(link, sid, |k| matches!(k, EventKind::SessionCreated(_))).await;
    sid
}

fn prompt(link: &NodeLink, sid: SessionId, agent: Option<AgentSpec>, text: String) {
    link.to_node
        .try_send(MasterToNode::Prompt {
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
            agent_named: false,
            files: vec![],
            owner: None,
            peers: vec![],
            tags: vec![],
        })
        .unwrap();
}

fn handoff(link: &NodeLink, sid: SessionId, req_id: &str, small: Option<&str>) {
    link.to_node
        .try_send(MasterToNode::Handoff {
            req_id: req_id.into(),
            session_id: sid,
            small_model: small.map(str::to_string),
            windows: Default::default(),
            low_effort: vec![],
        })
        .unwrap();
}

/// A turn that sets the task list: something to hand off.
fn work(step: &str) -> String {
    format!(
        "script: {}",
        serde_json::json!([{"tool": "todo", "input": {"items": [
            {"content": step, "status": "in_progress"},
            {"content": "Ship it", "status": "pending"},
        ]}}])
    )
}

fn log(store: &Store, sid: SessionId) -> Vec<RawEvent> {
    store.events_after(sid, 0, 10_000).unwrap()
}

fn notes(store: &Store, sid: SessionId) -> Vec<(u64, HandoffWritten)> {
    log(store, sid)
        .iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::HandoffWritten(h)) => Some((e.seq, h)),
            _ => None,
        })
        .collect()
}

/// Until the store holds `n` notes of `sid` (the event may reach the link
/// before or after the command's ack).
async fn wait_notes(store: &Store, sid: SessionId, n: usize) {
    for _ in 0..500 {
        if notes(store, sid).len() >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("no note written");
}

async fn turn(link: &mut NodeLink, sid: SessionId, text: String) {
    prompt(link, sid, None, text);
    until(link, sid, |k| matches!(k, EventKind::TurnEnded(_))).await;
}

/// The small model this node does not know fails as such, the chat's own
/// writes it, once: a second ask finds nothing due; the next stretch of
/// work gets a new note, which replaces it in the view.
#[tokio::test]
async fn a_note_falls_back_is_written_once_and_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let node = NodeRuntime::start(config(dir.path(), false)).await.unwrap();
    let mut link = attach(&node).await;
    let sid = create(&mut link, &ws, false).await;
    // Nothing to hand off yet: a question.
    turn(&mut link, sid, "what is this?".into()).await;
    handoff(&link, sid, "h0", None);
    assert_eq!(ack(&mut link, "h0").await, Err(HANDOFF_NOTHING.into()));

    turn(&mut link, sid, work("Write the parser")).await;
    let head = log(node.store(), sid).last().unwrap().seq;
    handoff(&link, sid, "h1", Some("gone::x"));
    assert_eq!(ack(&mut link, "h1").await, Ok(()));
    wait_notes(node.store(), sid, 1).await;
    let n = notes(node.store(), sid);
    assert_eq!(n.len(), 1);
    let (seq, h) = &n[0];
    assert_eq!((*seq, h.through_seq), (head + 1, head));
    assert_eq!(h.model.as_deref(), Some("demo::mock"));
    assert_eq!(
        h.failed,
        vec![NoteFailure {
            model: "gone::x".into(),
            reason: "unknown to this node".into()
        }]
    );
    assert!(h.text.as_deref().unwrap().starts_with("Status:"), "{h:?}");
    assert!(h.usage.is_some(), "its cost is the chat's");

    // Once per stretch: nothing new, nothing due.
    handoff(&link, sid, "h2", Some("demo::mock"));
    assert_eq!(ack(&mut link, "h2").await, Err(HANDOFF_NOTHING.into()));
    // A turn that changes nothing: still nothing.
    turn(&mut link, sid, "thanks".into()).await;
    handoff(&link, sid, "h3", None);
    assert_eq!(ack(&mut link, "h3").await, Err(HANDOFF_NOTHING.into()));
    // New work: a new note, the latest is the one shown.
    turn(&mut link, sid, work("Write the tests")).await;
    handoff(&link, sid, "h4", None);
    assert_eq!(ack(&mut link, "h4").await, Ok(()));
    wait_notes(node.store(), sid, 2).await;
    assert_eq!(notes(node.store(), sid).len(), 2);
    let v = SessionView::fold(&log(node.store(), sid)).unwrap();
    let shown: Vec<&Item> = v
        .items
        .iter()
        .filter(|i| matches!(i, Item::Handoff { .. }))
        .collect();
    assert_eq!(shown.len(), 2);
    // A later turn reads its history as before: the note is not sent.
    turn(&mut link, sid, "carry on".into()).await;
    let evs = log(node.store(), sid);
    assert_eq!(evs.last().unwrap().kind, "turn_ended");
}

/// The person's message while a note is written goes first: the note is
/// cancelled and nothing lands in the log; the next ask writes it after
/// the turn.
#[tokio::test]
async fn a_message_during_the_write_goes_first() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let node = NodeRuntime::start(config(dir.path(), true)).await.unwrap();
    let mut link = attach(&node).await;
    let sid = create(&mut link, &ws, false).await;
    turn(&mut link, sid, work("Write the parser")).await;
    handoff(&link, sid, "h1", None);
    assert_eq!(ack(&mut link, "h1").await, Ok(()));
    // A second ask while it is written: busy.
    handoff(&link, sid, "h1b", None);
    assert_eq!(ack(&mut link, "h1b").await, Err(HANDOFF_BUSY.into()));
    let started = std::time::Instant::now();
    turn(&mut link, sid, "next step please".into()).await;
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "the turn did not wait for the note"
    );
    // Past the slow note's time: it never landed.
    tokio::time::sleep(Duration::from_millis(3500)).await;
    assert!(notes(node.store(), sid).is_empty());
    let evs = log(node.store(), sid);
    assert_eq!(evs.last().unwrap().kind, "turn_ended");
    // Asked again (the master's next sweep): written after the turn.
    handoff(&link, sid, "h2", None);
    assert_eq!(ack(&mut link, "h2").await, Ok(()));
    wait_notes(node.store(), sid, 1).await;
    let n = notes(node.store(), sid);
    assert_eq!(n[0].1.through_seq, evs.last().unwrap().seq);
}

/// A Stop while a note is written cancels it too.
#[tokio::test]
async fn a_stop_cancels_the_note() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let node = NodeRuntime::start(config(dir.path(), true)).await.unwrap();
    let mut link = attach(&node).await;
    let sid = create(&mut link, &ws, false).await;
    turn(&mut link, sid, work("Write the parser")).await;
    handoff(&link, sid, "h1", None);
    assert_eq!(ack(&mut link, "h1").await, Ok(()));
    link.to_node
        .try_send(MasterToNode::Cancel {
            req_id: "x".into(),
            session_id: sid,
        })
        .unwrap();
    tokio::time::sleep(Duration::from_millis(3500)).await;
    assert!(notes(node.store(), sid).is_empty());
}

/// A node that was down when the chat went idle writes the note when the
/// master asks after its restart: the log alone says it is due.
#[tokio::test]
async fn a_restarted_node_writes_the_note_it_missed() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let sid = {
        let node = NodeRuntime::start(config(dir.path(), false)).await.unwrap();
        let mut link = attach(&node).await;
        let sid = create(&mut link, &ws, false).await;
        turn(&mut link, sid, work("Write the parser")).await;
        assert!(node.stop(Duration::from_secs(5)).await.is_empty());
        sid
    };
    let node = NodeRuntime::start(config(dir.path(), false)).await.unwrap();
    let mut link = attach(&node).await;
    handoff(&link, sid, "h1", None);
    assert_eq!(ack(&mut link, "h1").await, Ok(()));
    wait_notes(node.store(), sid, 1).await;
    assert_eq!(notes(node.store(), sid).len(), 1);
}

fn agent(id: &str, tools: &[&str], delegates: Vec<AgentSpec>) -> AgentSpec {
    AgentSpec {
        agent_id: id.into(),
        name: id.into(),
        system_prompt: "work".into(),
        tools: ToolSelection::List(tools.iter().map(|t| t.to_string()).collect()),
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

/// Never for a routine's chat (`no_callbacks`, held for good) nor a
/// sub-agent's, whatever the master asks.
#[tokio::test]
async fn a_routines_or_a_sub_agents_chat_gets_none() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let node = NodeRuntime::start(config(dir.path(), false)).await.unwrap();
    let mut link = attach(&node).await;
    let routine = create(&mut link, &ws, true).await;
    turn(&mut link, routine, work("Write the parser")).await;
    handoff(&link, routine, "r", None);
    let e = ack(&mut link, "r").await.unwrap_err();
    assert!(e.starts_with(HANDOFF_NOTHING), "{e}");

    let sid = create(&mut link, &ws, false).await;
    let lead = agent("lead", &["todo"], vec![agent("planner", &["todo"], vec![])]);
    prompt(
        &link,
        sid,
        Some(lead),
        format!("delegate: planner={}", work("Plan")),
    );
    until(&mut link, sid, |k| matches!(k, EventKind::TurnEnded(_))).await;
    let child: SessionId = log(node.store(), sid)
        .iter()
        .find_map(|e| match e.typed() {
            Ok(EventKind::ToolCallFinished(f)) => f
                .output
                .strip_prefix("session: ")
                .and_then(|r| r.lines().next())
                .and_then(|id| id.parse().ok()),
            _ => None,
        })
        .expect("a child session");
    assert!(
        log(node.store(), child)
            .iter()
            .any(|e| e.kind == "todo_updated")
    );
    handoff(&link, child, "c", None);
    let e = ack(&mut link, "c").await.unwrap_err();
    assert!(e.starts_with(HANDOFF_NOTHING), "{e}");
    assert!(notes(node.store(), child).is_empty());
}

/// The node's owner turns notes off (`handoff_notes_per_hour = 0`): the
/// node does not announce them and refuses the command.
#[tokio::test]
async fn a_node_with_notes_off_neither_offers_nor_writes_them() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let node = NodeRuntime::start(config_with(dir.path(), false, 0))
        .await
        .unwrap();
    let mut link = node.attach().await;
    match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { capabilities, .. } => {
            assert!(
                !capabilities
                    .iter()
                    .any(|c| c == harness_protocol::CAP_HANDOFF)
            );
        }
        other => panic!("expected hello, got {other:?}"),
    }
    let sid = create(&mut link, &ws, false).await;
    turn(&mut link, sid, work("Write the parser")).await;
    handoff(&link, sid, "h", None);
    let e = ack(&mut link, "h").await.unwrap_err();
    assert!(e.contains("handoff_notes_per_hour = 0"), "{e}");
    assert!(notes(node.store(), sid).is_empty());
}

/// A master that asks and cancels in a loop (a cancelled note leaves no
/// event, so the chat stays due) gets at most `handoff_notes_per_hour`
/// note calls per chat.
#[tokio::test]
async fn asking_and_cancelling_in_a_loop_is_bounded_per_hour() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let node = NodeRuntime::start(config_with(dir.path(), true, 2))
        .await
        .unwrap();
    let mut link = attach(&node).await;
    let sid = create(&mut link, &ws, false).await;
    turn(&mut link, sid, work("Write the parser")).await;
    for i in 0..2 {
        handoff(&link, sid, &format!("h{i}"), None);
        assert_eq!(ack(&mut link, &format!("h{i}")).await, Ok(()));
        link.to_node
            .try_send(MasterToNode::Cancel {
                req_id: format!("x{i}"),
                session_id: sid,
            })
            .unwrap();
        ack(&mut link, &format!("x{i}")).await.unwrap();
    }
    handoff(&link, sid, "h2", None);
    let e = ack(&mut link, "h2").await.unwrap_err();
    assert!(e.contains("in the last hour"), "{e}");
    assert!(notes(node.store(), sid).is_empty());
}
