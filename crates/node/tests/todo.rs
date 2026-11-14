//! The agent's task list on a real node: the `todo` call is in the
//! session's log, the list is folded from that log after the node restarts,
//! and a sub-agent's list is in its own session's log, not its parent's.

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::agent::{AgentSpec, ToolSelection};
use harness_core::event::*;
use harness_core::policy::ApprovalMode;
use harness_core::todo::{TodoItem, TodoStatus};
use harness_core::view::SessionView;
use harness_core::{RawEvent, SessionId};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};

fn start_config(dir: &std::path::Path) -> NodeConfig {
    start_with(dir, vec![])
}

fn start_with(dir: &std::path::Path, denied_tools: Vec<String>) -> NodeConfig {
    NodeConfig {
        node_id: "local".into(),
        hostname: "test".into(),
        policy: NodePolicy {
            workspace_roots: vec![dir.into()],
            max_approval_mode: ApprovalMode::Auto,
            denied_tools,
            ..Default::default()
        },
        store: Store::open(&dir.join("node.db"), Durability::Full).unwrap(),
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
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    }
}

async fn attach(node: &NodeRuntime) -> NodeLink {
    let mut link = node.attach().await;
    match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { .. } => {}
        other => panic!("expected hello, got {other:?}"),
    }
    link
}

/// Events until one for `sid` matches.
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

async fn create(link: &mut NodeLink, ws: &std::path::Path) -> SessionId {
    create_in(link, ws, ApprovalMode::Auto).await
}

async fn create_in(link: &mut NodeLink, ws: &std::path::Path, mode: ApprovalMode) -> SessionId {
    let sid = SessionId::new();
    link.to_node
        .try_send(MasterToNode::Create {
            exec_mode: None,
            no_callbacks: false,
            profile: None,
            incognito: false,
            agent: None,
            req_id: "c".into(),
            session_id: sid,
            title: "t".into(),
            cwd: ws.display().to_string(),
            model: None,
            approval_mode: Some(mode),
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
        })
        .unwrap();
}

/// `script:` text for one `todo` call setting `items`.
fn todo_script(items: serde_json::Value) -> String {
    format!(
        "script: {}",
        serde_json::json!([{"tool": "todo", "input": {"items": items}}])
    )
}

fn plan() -> serde_json::Value {
    serde_json::json!([
        {"content": "Read the code", "status": "done"},
        {"content": "Write the tests", "status": "in_progress"},
        {"content": "Implement", "status": "pending"},
    ])
}

fn want() -> Vec<TodoItem> {
    serde_json::from_value(plan()).unwrap()
}

fn log(store: &Store, sid: SessionId) -> Vec<RawEvent> {
    store.events_after(sid, 0, 10_000).unwrap()
}

#[tokio::test]
async fn the_list_is_in_the_log_and_folds_back_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let sid = {
        let node = NodeRuntime::start(start_config(dir.path())).await.unwrap();
        let mut link = attach(&node).await;
        let sid = create(&mut link, &ws).await;
        prompt(&link, sid, None, todo_script(plan()));
        until(&mut link, sid, |k| matches!(k, EventKind::TurnEnded(_))).await;
        let evs = log(node.store(), sid);
        assert!(evs.iter().any(|e| e.kind == "todo_updated"), "{evs:?}");
        assert_eq!(SessionView::fold(&evs).unwrap().todo, want());
        assert!(node.stop(Duration::from_secs(5)).await.is_empty());
        sid
    };
    let node = NodeRuntime::start(start_config(dir.path())).await.unwrap();
    let evs = log(node.store(), sid);
    let v = SessionView::fold(&evs).unwrap();
    assert_eq!(v.todo, want());
    assert_eq!(v.todo[1].status, TodoStatus::InProgress);
    // The next call replaces it, on the restarted node.
    let mut link = attach(&node).await;
    prompt(
        &link,
        sid,
        None,
        todo_script(serde_json::json!([{"content": "Ship it", "status": "pending"}])),
    );
    until(&mut link, sid, |k| matches!(k, EventKind::TurnEnded(_))).await;
    let v = SessionView::fold(&log(node.store(), sid)).unwrap();
    assert_eq!(
        v.todo,
        vec![TodoItem {
            content: "Ship it".into(),
            status: TodoStatus::Pending
        }]
    );
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

/// A sub-agent keeps its own list: its call is recorded in its own
/// session's log, the parent's log and fold have none.
#[tokio::test]
async fn a_sub_agents_list_is_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let node = NodeRuntime::start(start_config(dir.path())).await.unwrap();
    let mut link = attach(&node).await;
    let sid = create(&mut link, &ws).await;
    let lead = agent("lead", &["todo"], vec![agent("planner", &["todo"], vec![])]);
    prompt(
        &link,
        sid,
        Some(lead),
        format!("delegate: planner={}", todo_script(plan())),
    );
    until(&mut link, sid, |k| matches!(k, EventKind::TurnEnded(_))).await;
    let parent = log(node.store(), sid);
    assert!(
        !parent.iter().any(|e| e.kind == "todo_updated"),
        "{parent:?}"
    );
    assert!(SessionView::fold(&parent).unwrap().todo.is_empty());
    // The child: the session the delegate call names.
    let child: SessionId = parent
        .iter()
        .find_map(|e| match e.typed() {
            Ok(EventKind::ToolCallFinished(f)) => f
                .output
                .strip_prefix("session: ")
                .and_then(|r| r.lines().next())
                .and_then(|id| id.parse().ok()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no child session in {parent:?}"));
    assert_ne!(child, sid);
    let evs = log(node.store(), child);
    let v = SessionView::fold(&evs).unwrap();
    assert_eq!(
        v.meta.as_ref().and_then(|m| m.parent_session),
        Some(sid),
        "{evs:?}"
    );
    assert_eq!(v.todo, want());
}

/// The list writes only the chat's own log: no approval is asked for it,
/// even in `ask_all` (TD-6); a node that denies `todo` still refuses it.
#[tokio::test]
async fn the_list_is_never_asked_about_but_a_node_may_deny_it() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let node = NodeRuntime::start(start_config(dir.path())).await.unwrap();
    let mut link = attach(&node).await;
    let sid = create_in(&mut link, &ws, ApprovalMode::AskAll).await;
    prompt(&link, sid, None, todo_script(plan()));
    until(&mut link, sid, |k| matches!(k, EventKind::TurnEnded(_))).await;
    let evs = log(node.store(), sid);
    assert!(
        !evs.iter().any(|e| e.kind == "approval_requested"),
        "{evs:?}"
    );
    assert_eq!(SessionView::fold(&evs).unwrap().todo, want());
    assert!(node.stop(Duration::from_secs(5)).await.is_empty());
    drop(node);

    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let node = NodeRuntime::start(start_with(dir.path(), vec!["todo".into()]))
        .await
        .unwrap();
    let mut link = attach(&node).await;
    let sid = create_in(&mut link, &ws, ApprovalMode::Auto).await;
    prompt(&link, sid, None, todo_script(plan()));
    until(&mut link, sid, |k| matches!(k, EventKind::TurnEnded(_))).await;
    let evs = log(node.store(), sid);
    assert!(!evs.iter().any(|e| e.kind == "todo_updated"), "{evs:?}");
    assert!(SessionView::fold(&evs).unwrap().todo.is_empty());
}
