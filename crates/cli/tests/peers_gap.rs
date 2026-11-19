//! The instant between two turns of a message that called several agents
//! (ADOPTION_PLAN X.37): the slot is free before the next agent's turn
//! claims it. A message sent then is refused (the next agent comes first,
//! never a turn in between), and a Stop sent then ends the list: nothing
//! runs after it and the agents not asked are named. Here because the hold
//! that keeps the gap open is a `harness-node` test hook (`testing`), which
//! this crate's tests enable.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_agent_native::{
    ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError, mock::Mock,
};
use harness_core::agent::{AgentSpec, ToolSelection};
use harness_core::event::*;
use harness_core::policy::ApprovalMode;
use harness_core::{RawEvent, SessionId};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};
use tokio::sync::mpsc;

struct Model(Arc<Mutex<Vec<String>>>);

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
        self.0.lock().unwrap().push(req.model.clone());
        if req.model == "fail" {
            return Err(ProviderError::Status {
                status: 400,
                body: "refused".into(),
                hint: Default::default(),
            });
        }
        Mock {
            delay: Duration::from_millis(1),
        }
        .complete(req, d)
        .await
    }
}

fn agent(id: &str) -> AgentSpec {
    AgentSpec {
        agent_id: id.into(),
        name: id.into(),
        system_prompt: format!("You are @{id}."),
        tools: ToolSelection::List(vec![]),
        model: Some(id.into()),
        fallbacks: vec![],
        unavailable: vec![],
        no_effort: vec![],
        vision: vec![],
        context_windows: Default::default(),
        effort: None,
        ui: Default::default(),
        prompt_hash: format!("h-{id}"),
        deny_tools: vec![],
        deny_skills: vec![],
        deny_mcp_servers: vec![],
        stored_skills: vec![],
        connector_tools: vec![],
        delegates: vec![],
        caps: None,
        no_personalisation: false,
    }
}

struct N {
    _node: NodeRuntime,
    link: NodeLink,
    dir: tempfile::TempDir,
    n: u32,
}

impl N {
    async fn start() -> N {
        let dir = tempfile::tempdir().unwrap();
        let cfg = NodeConfig {
            node_id: "n".into(),
            hostname: "test".into(),
            policy: NodePolicy {
                workspace_roots: vec![dir.path().into()],
                max_approval_mode: ApprovalMode::Auto,
                ..Default::default()
            },
            store: Store::open(&dir.path().join("node.db"), Durability::Full).unwrap(),
            backend: Arc::new(NativeBackend::new(
                Arc::new(Model(Arc::default())),
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
        };
        let node = NodeRuntime::start(cfg).await.unwrap();
        let mut link = node.attach().await;
        assert!(matches!(
            link.from_node.recv().await.unwrap(),
            NodeToMaster::Hello { .. }
        ));
        N {
            _node: node,
            link,
            dir,
            n: 0,
        }
    }

    fn send(&self, m: MasterToNode) {
        self.link.to_node.try_send(m).unwrap();
    }

    async fn ack(&mut self, id: &str) -> Result<(), String> {
        loop {
            let m = tokio::time::timeout(Duration::from_secs(30), self.link.from_node.recv())
                .await
                .expect("timed out")
                .expect("link closed");
            if let NodeToMaster::Ack {
                req_id, ok, error, ..
            } = m
                && req_id == id
            {
                return if ok {
                    Ok(())
                } else {
                    Err(error.unwrap_or_default())
                };
            }
        }
    }

    fn req(&mut self) -> String {
        self.n += 1;
        format!("r{}", self.n)
    }

    async fn create(&mut self, a: &AgentSpec) -> SessionId {
        let sid = SessionId::new();
        let id = self.req();
        self.send(MasterToNode::Create {
            req_id: id.clone(),
            session_id: sid,
            title: "t".into(),
            cwd: self.dir.path().display().to_string(),
            model: a.model.clone(),
            approval_mode: Some(ApprovalMode::Auto),
            profile: None,
            agent: Some(a.clone()),
            incognito: false,
            no_callbacks: false,
            exec_mode: None,
        });
        self.ack(&id).await.unwrap();
        sid
    }

    async fn say(
        &mut self,
        sid: SessionId,
        text: &str,
        first: &AgentSpec,
        peers: Vec<AgentSpec>,
    ) -> Result<(), String> {
        let id = self.req();
        self.send(MasterToNode::Prompt {
            req_id: id.clone(),
            session_id: sid,
            text: text.into(),
            agent: Some(first.clone()),
            attachments: vec![],
            compare: vec![],
            reply: None,
            savings: None,
            context: None,
            images: vec![],
            agent_named: true,
            files: vec![],
            owner: None,
            identity: None,
            peers,
            tags: vec![],
        });
        self.ack(&id).await
    }

    fn log(&self, sid: SessionId) -> Vec<RawEvent> {
        Store::open(&self.dir.path().join("node.db"), Durability::Full)
            .unwrap()
            .events_after(sid, 0, 100_000)
            .unwrap()
    }
}

fn started(log: &[RawEvent]) -> Vec<String> {
    log.iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::TurnStarted(t)) => t.agent.map(|a| a.agent_id),
            _ => None,
        })
        .collect()
}

/// A message in the gap is refused, not run between two agents; a Stop in
/// the gap is not lost: the list ends and the agents not asked are named.
#[tokio::test]
async fn a_message_or_a_stop_between_two_turns_of_the_list() {
    let mut n = N::start().await;
    let (a, b, c) = (agent("a"), agent("b"), agent("c"));
    let sid = n.create(&a).await;
    let hold = harness_node::testing::hold_next_peer_start(sid);
    n.say(sid, "@a @b @c go", &a, vec![b.clone(), c.clone()])
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), hold.held())
        .await
        .expect("the gap was reached");
    // The first turn ended; nothing else runs yet.
    let log = n.log(sid);
    assert_eq!(started(&log), ["a"]);
    assert_eq!(log.iter().filter(|e| e.kind == "turn_ended").count(), 1);
    // A message now is refused with the queue's sentence; nothing recorded.
    let r = n.say(sid, "in between", &a, vec![]).await.unwrap_err();
    assert!(r.contains("a turn is already running"), "{r}");
    assert!(r.contains("still taking their turns"), "{r}");
    assert_eq!(
        n.log(sid)
            .iter()
            .filter(|e| e.kind == "user_message")
            .count(),
        1
    );
    // Stop with no turn running: the list is over.
    let id = n.req();
    n.send(MasterToNode::Cancel {
        req_id: id.clone(),
        session_id: sid,
    });
    n.ack(&id).await.unwrap();
    hold.release();
    for _ in 0..500 {
        if n.log(sid).iter().any(|e| e.kind == "peers_skipped") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let log = n.log(sid);
    assert_eq!(started(&log), ["a"], "no agent was asked after the Stop");
    let skipped: Vec<PeersSkipped> = log
        .iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::PeersSkipped(p)) => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].agents, ["b", "c"]);
    assert_eq!(skipped[0].reason, PeersSkipReason::Stopped);
}

fn turn_of(log: &[RawEvent], agent: &str) -> String {
    log.iter()
        .find_map(|e| match e.typed() {
            Ok(EventKind::TurnStarted(t))
                if t.agent.as_ref().is_some_and(|a| a.agent_id == agent) =>
            {
                Some(t.turn_id)
            }
            _ => None,
        })
        .unwrap()
}

/// A Retry of the failed first turn, sent in the gap, is refused like a new
/// message (review of PR 166, L6): the next agent comes first, and nothing
/// is skipped.
#[tokio::test]
async fn a_retry_between_two_turns_of_the_list_is_refused() {
    let mut n = N::start().await;
    let mut a = agent("a");
    a.model = Some("fail".into());
    let b = agent("b");
    let sid = n.create(&a).await;
    let hold = harness_node::testing::hold_next_peer_start(sid);
    n.say(sid, "@a @b go", &a, vec![b.clone()]).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), hold.held())
        .await
        .expect("the gap was reached");
    let failed = turn_of(&n.log(sid), "a");
    let id = n.req();
    n.send(MasterToNode::Retry {
        req_id: id.clone(),
        session_id: sid,
        turn_id: failed,
        agent: Some(a.clone()),
        savings: None,
        context: None,
        agent_named: false,
        owner: None,
        identity: None,
    });
    let r = n.ack(&id).await.unwrap_err();
    assert!(r.contains("still taking their turns"), "{r}");
    hold.release();
    for _ in 0..500 {
        if n.log(sid).iter().filter(|e| e.kind == "turn_ended").count() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let log = n.log(sid);
    assert_eq!(started(&log), ["a", "b"], "b was asked, once");
    assert!(log.iter().all(|e| e.kind != "peers_skipped"));
}

/// In the instant after the last agent is taken off the list and before its
/// turn claims the slot, a new message is refused: it cannot win the slot and
/// make that agent skipped.
#[tokio::test]
async fn a_message_after_the_last_agent_is_chosen_and_before_it_starts_is_refused() {
    let mut n = N::start().await;
    let (a, b) = (agent("a"), agent("b"));
    let sid = n.create(&a).await;
    let hold = harness_node::testing::hold_next_peer_claim(sid);
    n.say(sid, "@a @b go", &a, vec![b.clone()]).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), hold.held())
        .await
        .expect("the instant was reached");
    let r = n.say(sid, "me first", &a, vec![]).await.unwrap_err();
    assert!(r.contains("still taking their turns"), "{r}");
    hold.release();
    for _ in 0..500 {
        if n.log(sid).iter().filter(|e| e.kind == "turn_ended").count() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let log = n.log(sid);
    assert_eq!(started(&log), ["a", "b"]);
    assert!(log.iter().all(|e| e.kind != "peers_skipped"));
    assert_eq!(
        log.iter().filter(|e| e.kind == "user_message").count(),
        1,
        "the refused message was never recorded"
    );
}
