//! A person's `/compact` against the turn that reads queued messages
//! (ADOPTION_PLAN X.20, review M1): when a turn ends with a message the
//! person sent during it, the slot is free for a moment before the turn
//! that reads it claims it. A `/compact` in that window is refused, so the
//! message is read and never stranded. Here because the hold that keeps
//! the window open is a `harness-node` test hook (`testing`), which this
//! crate's tests enable.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_agent_native::llm::{ModelRouter, Route};
use harness_agent_native::{
    ChatMsg, ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError,
    RetryHint, SUMMARY_PROMPT, mock::Mock,
};
use harness_core::event::*;
use harness_core::policy::ApprovalMode;
use harness_core::{RawEvent, SessionId};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};
use tokio::sync::mpsc;

/// How the stand-in model answers.
#[derive(Default)]
struct Behave {
    /// Summary requests wait this long.
    slow_summary: Option<Duration>,
    /// Summary requests fail.
    fail_summary: bool,
    /// Every other request waits this long.
    slow_turn: Option<Duration>,
}

/// The mock, as `Behave` says; keeps every request.
struct Model {
    behave: Behave,
    seen: Arc<Mutex<Vec<ChatRequest>>>,
}

fn is_summary(req: &ChatRequest) -> bool {
    matches!(req.messages.first(), Some(ChatMsg::System(p)) if p == SUMMARY_PROMPT)
}

#[async_trait::async_trait]
impl Provider for Model {
    fn name(&self) -> &'static str {
        "mock"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        d: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        self.seen.lock().unwrap().push(req.clone());
        let summary = is_summary(req);
        let wait = if summary {
            self.behave.slow_summary
        } else {
            self.behave.slow_turn
        };
        if let Some(w) = wait {
            tokio::time::sleep(w).await;
        }
        let slow = req
            .messages
            .iter()
            .any(|m| matches!(m, ChatMsg::User(t) if t.contains("SLOW")));
        if slow && !summary {
            tokio::time::sleep(Duration::from_millis(800)).await;
        }
        if summary && self.behave.fail_summary {
            return Err(ProviderError::Status {
                status: 400,
                body: "SECRET-PROVIDER-BODY".into(),
                hint: RetryHint::default(),
            });
        }
        Mock {
            delay: Duration::from_millis(1),
        }
        .complete(req, d)
        .await
    }
}

struct Node {
    node: NodeRuntime,
    link: NodeLink,
    seen: Arc<Mutex<Vec<ChatRequest>>>,
    _dir: tempfile::TempDir,
    ws: std::path::PathBuf,
}

async fn start(behave: Behave) -> Node {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let seen: Arc<Mutex<Vec<ChatRequest>>> = Arc::default();
    let router = ModelRouter::new(vec![Route {
        prefix: "demo".into(),
        provider: Arc::new(Model {
            behave,
            seen: seen.clone(),
        }),
    }]);
    let cfg = NodeConfig {
        node_id: "local".into(),
        hostname: "test".into(),
        policy: NodePolicy {
            workspace_roots: vec![dir.path().into()],
            max_approval_mode: ApprovalMode::Auto,
            ..Default::default()
        },
        store: Store::open(&dir.path().join("node.db"), Durability::Full).unwrap(),
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
    };
    let node = NodeRuntime::start(cfg).await.unwrap();
    let mut link = node.attach().await;
    match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { capabilities, .. } => {
            assert!(
                capabilities
                    .iter()
                    .any(|c| c == harness_protocol::CAP_COMPACT)
            );
        }
        other => panic!("expected hello, got {other:?}"),
    }
    Node {
        node,
        link,
        seen,
        _dir: dir,
        ws,
    }
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

async fn create(n: &mut Node) -> SessionId {
    let sid = SessionId::new();
    n.link
        .to_node
        .try_send(MasterToNode::Create {
            exec_mode: None,
            no_callbacks: false,
            profile: None,
            incognito: false,
            agent: None,
            req_id: "c".into(),
            session_id: sid,
            title: "t".into(),
            cwd: n.ws.display().to_string(),
            model: None,
            approval_mode: Some(ApprovalMode::Auto),
        })
        .unwrap();
    until(&mut n.link, sid, |k| {
        matches!(k, EventKind::SessionCreated(_))
    })
    .await;
    sid
}

fn prompt(link: &NodeLink, sid: SessionId, req_id: &str, text: String) {
    link.to_node
        .try_send(MasterToNode::Prompt {
            savings: None,
            context: None,
            reply: None,
            agent: None,
            attachments: vec![],
            compare: vec![],
            req_id: req_id.into(),
            session_id: sid,
            text,
            images: vec![],
            files: vec![],
            agent_named: false,
            owner: None,
        })
        .unwrap();
}

async fn turn(n: &mut Node, sid: SessionId, text: String) {
    prompt(&n.link, sid, "p", text);
    until(&mut n.link, sid, |k| matches!(k, EventKind::TurnEnded(_))).await;
}

fn compact(link: &NodeLink, sid: SessionId, req_id: &str, focus: Option<&str>) {
    link.to_node
        .try_send(MasterToNode::Compact {
            req_id: req_id.into(),
            session_id: sid,
            focus: focus.map(str::to_string),
            windows: Default::default(),
        })
        .unwrap();
}

fn log(store: &Store, sid: SessionId) -> Vec<RawEvent> {
    store.events_after(sid, 0, 10_000).unwrap()
}

/// A long message: three of them make a chat worth summarising.
fn long(tag: &str) -> String {
    format!(
        "{tag} {}",
        "words about the database migration ".repeat(120)
    )
}

/// Three turns of `long` text: two to summarise, the last one kept.
async fn three_turns(n: &mut Node) -> SessionId {
    let sid = create(n).await;
    for tag in ["FIRST-TURN", "SECOND-TURN", "THIRD-TURN"] {
        turn(n, sid, long(tag)).await;
    }
    sid
}

/// The ended turn freed the slot; the turn for the queued message waits at
/// the hold. `/compact` now is refused (on the old code it was accepted,
/// that turn's claim then failed and the message stayed unread); once the
/// hold is released the message is read.
#[tokio::test]
async fn compact_never_strands_a_queued_message() {
    let mut n = start(Behave::default()).await;
    let sid = three_turns(&mut n).await;
    // Past the third turn's own look for queued messages.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let hold = harness_node::testing::hold_next_queued_start(sid);
    let asked = n.seen.lock().unwrap().len();
    prompt(&n.link, sid, "pa", "SLOW turn A".into());
    // B is sent while A's last (and only) request runs: A ends without
    // another request, so B waits for the turn that reads it.
    for _ in 0..500 {
        if n.seen.lock().unwrap().len() > asked {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    prompt(&n.link, sid, "pb", "QUEUED-B read me".into());
    assert_eq!(ack(&mut n.link, "pb").await, Ok(()));
    hold.held().await;
    compact(&n.link, sid, "k1", None);
    assert_eq!(
        ack(&mut n.link, "k1").await,
        Err(harness_protocol::COMPACT_PENDING.into())
    );
    hold.release();
    // The turn that reads B runs and ends.
    for _ in 0..500 {
        let evs = log(n.node.store(), sid);
        if evs.iter().filter(|e| e.kind == "queued_delivered").count() >= 1
            && evs.last().is_some_and(|e| e.kind == "turn_ended")
            && evs.iter().filter(|e| e.kind == "turn_ended").count() >= 5
        {
            assert!(!evs.iter().any(|e| e.kind == "history_summarized"));
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!(
        "the queued message was never read: {:?}",
        log(n.node.store(), sid)
            .iter()
            .map(|e| &e.kind)
            .collect::<Vec<_>>()
    );
}
