//! Switching a chat's agent (README "Agents", PLAN.md "Agents"): picking
//! one is state of the chat, not history. `session.set_agent` writes
//! nothing to the log; the node refuses it while the chat works (a turn,
//! a callback turn, a retry, a stop still ending, a `/compact`). The switch
//! is recorded once, as `agent_changed` naming the agent and its model,
//! when the next person's turn starts under an agent or model other than
//! the chat's last recorded one, right before that turn's `turn_started`
//! (so above its message). An @mention is not a switch; neither is a
//! restart. A Retry after a switch runs, and notes, the new agent.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_agent_native::RetryHint;
use harness_agent_native::provider::{ChatMsg, ChatRequest, Completion, Provider, ProviderError};
use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::SessionId;
use harness_core::agent::{AgentSpec, ToolSelection};
use harness_core::event::{EventKind, Profile, TurnEndReason};
use harness_core::policy::ApprovalMode;
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster, PromptMsg, RetryMsg};
use harness_store::{Durability, Store};
use serde_json::Value;

type Requests = Arc<Mutex<Vec<ChatRequest>>>;

struct Recording {
    inner: Mock,
    seen: Requests,
}

#[async_trait::async_trait]
impl Provider for Recording {
    fn name(&self) -> &'static str {
        "mock"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        d: tokio::sync::mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        self.seen.lock().unwrap().push(req.clone());
        // An agent on the model `fail`: the provider refuses (a 400 is
        // never retried), so the turn ends in an error a Retry may run.
        if req.model == "fail" {
            return Err(ProviderError::Status {
                status: 400,
                body: "refused".into(),
                hint: RetryHint::default(),
            });
        }
        self.inner.complete(req, d).await
    }
}

struct N {
    node: Option<NodeRuntime>,
    link: NodeLink,
    dir: tempfile::TempDir,
    seen: Requests,
    n: u32,
}

fn config(dir: &std::path::Path, seen: &Requests) -> NodeConfig {
    let mut policy = NodePolicy {
        workspace_roots: vec![dir.into()],
        max_approval_mode: ApprovalMode::Auto,
        ..Default::default()
    };
    // Not about skills: the developer's own stay out.
    policy.skills.roots.clear();
    NodeConfig {
        node_id: "n".into(),
        hostname: "test".into(),
        policy,
        store: Store::open(&dir.join("node.db"), Durability::Full).unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Recording {
                inner: Mock {
                    delay: Duration::from_millis(5),
                },
                seen: seen.clone(),
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
        NodeToMaster::Hello { .. } => link,
        other => panic!("expected hello, got {other:?}"),
    }
}

async fn start() -> N {
    let dir = tempfile::tempdir().unwrap();
    let seen: Requests = Arc::default();
    let node = NodeRuntime::start(config(dir.path(), &seen)).await.unwrap();
    let link = attach(&node).await;
    N {
        node: Some(node),
        link,
        dir,
        seen,
        n: 0,
    }
}

/// An agent as the master resolves it: `id`, named `Id`, on `model`.
fn agent(id: &str, model: &str) -> AgentSpec {
    AgentSpec {
        agent_id: id.into(),
        name: format!("Agent {id}"),
        system_prompt: format!("You are @{id}."),
        tools: ToolSelection::List(vec![]),
        model: Some(model.into()),
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

impl N {
    fn send(&self, m: MasterToNode) {
        self.link.to_node.try_send(m).unwrap();
    }

    fn req(&mut self) -> String {
        self.n += 1;
        format!("r{}", self.n)
    }

    async fn next(&mut self) -> NodeToMaster {
        tokio::time::timeout(Duration::from_secs(30), self.link.from_node.recv())
            .await
            .expect("timed out")
            .expect("link closed")
    }

    /// The answer to request `id` (others, a prompt's say, are skipped).
    async fn ack(&mut self, id: &str) -> Result<(), String> {
        loop {
            match self.next().await {
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

    fn store(&self) -> &Store {
        self.node.as_ref().unwrap().store()
    }

    async fn restart(&mut self) {
        drop(self.node.take());
        tokio::time::sleep(Duration::from_millis(200)).await;
        let node = NodeRuntime::start(config(self.dir.path(), &self.seen))
            .await
            .unwrap();
        self.link = attach(&node).await;
        self.node = Some(node);
    }

    /// A chat created with agent `a`, as the master creates one.
    async fn create(&mut self, a: &AgentSpec) -> SessionId {
        let sid = SessionId::new();
        let req_id = self.req();
        self.send(MasterToNode::Create {
            req_id: req_id.clone(),
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
        self.ack(&req_id).await.unwrap();
        sid
    }

    async fn set_agent(&mut self, sid: SessionId, a: &AgentSpec) -> Result<(), String> {
        let req_id = self.req();
        self.send(MasterToNode::SetAgent {
            req_id: req_id.clone(),
            session_id: sid,
            agent: a.clone(),
        });
        self.ack(&req_id).await
    }

    fn prompt(&mut self, sid: SessionId, a: &AgentSpec, named: bool, text: &str) {
        let req_id = self.req();
        self.send(MasterToNode::Prompt(PromptMsg {
            req_id,
            session_id: sid,
            text: text.into(),
            agent: Some(a.clone()),
            attachments: vec![],
            compare: vec![],
            reply: None,
            savings: None,
            context: None,
            images: vec![],
            agent_named: named,
            files: vec![],
            owner: None,
            identity: None,
            peers: vec![],
            tags: vec![],
        }));
    }

    /// One turn under `a`, to its end.
    async fn turn(&mut self, sid: SessionId, a: &AgentSpec, named: bool, text: &str) {
        self.prompt(sid, a, named, text);
        self.until_end(sid).await;
    }

    async fn until_end(&mut self, sid: SessionId) -> TurnEndReason {
        loop {
            match self.next().await {
                NodeToMaster::Event { event } if event.session_id == sid => {
                    if let Ok(EventKind::TurnEnded(e)) = event.typed() {
                        return e.reason;
                    }
                }
                NodeToMaster::Ack {
                    ok: false, error, ..
                } => panic!("refused: {error:?}"),
                _ => {}
            }
        }
    }

    async fn until_started(&mut self, sid: SessionId) -> String {
        loop {
            match self.next().await {
                NodeToMaster::Event { event } if event.session_id == sid => {
                    if let Ok(EventKind::TurnStarted(t)) = event.typed() {
                        return t.turn_id;
                    }
                }
                NodeToMaster::Ack {
                    ok: false, error, ..
                } => panic!("refused: {error:?}"),
                _ => {}
            }
        }
    }

    fn kinds(&self, sid: SessionId) -> Vec<String> {
        self.store()
            .events_after(sid, 0, 100_000)
            .unwrap()
            .into_iter()
            .map(|e| e.kind)
            .collect()
    }

    fn last_seq(&self, sid: SessionId) -> u64 {
        self.store()
            .events_after(sid, 0, 100_000)
            .unwrap()
            .last()
            .unwrap()
            .seq
    }

    /// Every `agent_changed` payload of the chat.
    fn switches(&self, sid: SessionId) -> Vec<Value> {
        self.store()
            .events_after(sid, 0, 100_000)
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == "agent_changed")
            .map(|e| e.payload)
            .collect()
    }
}

/// The owner's report: A → B → C → B with no message sent leaves the log
/// exactly as it was.
#[tokio::test]
async fn picking_agents_without_a_message_writes_nothing() {
    let mut n = start().await;
    let a = agent("a", "mock-a");
    let sid = n.create(&a).await;
    n.turn(sid, &a, false, "hello").await;
    let before = n.last_seq(sid);
    for id in ["b", "c", "b"] {
        n.set_agent(sid, &agent(id, &format!("mock-{id}")))
            .await
            .unwrap();
    }
    assert_eq!(n.last_seq(sid), before, "{:?}", n.kinds(sid));
    assert!(n.switches(sid).is_empty());
}

/// A switch followed by a message: one `agent_changed`, right before that
/// turn's `turn_started` and so above its message, naming the agent and
/// the model it runs on. The next message under the same agent adds none.
#[tokio::test]
async fn a_switch_is_recorded_once_at_the_next_message_with_its_model() {
    let mut n = start().await;
    let a = agent("a", "mock-a");
    let b = agent("b", "mock-b");
    let sid = n.create(&a).await;
    n.turn(sid, &a, false, "one").await;
    n.set_agent(sid, &b).await.unwrap();
    n.set_agent(sid, &agent("c", "mock-c")).await.unwrap();
    n.set_agent(sid, &b).await.unwrap();
    n.turn(sid, &b, false, "two").await;
    n.turn(sid, &b, false, "three").await;
    let switches = n.switches(sid);
    assert_eq!(switches.len(), 1, "{switches:?}");
    assert_eq!(switches[0]["agent"]["agent_id"], "b");
    assert_eq!(switches[0]["agent"]["name"], "Agent b");
    assert_eq!(switches[0]["model"], "mock-b");
    let kinds = n.kinds(sid);
    let at = kinds.iter().position(|k| k == "agent_changed").unwrap();
    assert_eq!(kinds[at + 1], "turn_started", "{kinds:?}");
    assert_eq!(kinds[at + 2], "user_message", "{kinds:?}");
    // The message it sits on is "two".
    let evs = n.store().events_after(sid, 0, 100_000).unwrap();
    assert_eq!(evs[at + 2].payload["text"], "two");
}

/// The same agent on another model is a switch too (the person sees which
/// model answers); the same agent and model picked again is none.
#[tokio::test]
async fn the_model_counts_and_picking_the_same_again_is_no_switch() {
    let mut n = start().await;
    let a = agent("a", "mock-a");
    let sid = n.create(&a).await;
    n.turn(sid, &a, false, "one").await;
    n.set_agent(sid, &a).await.unwrap();
    n.turn(sid, &a, false, "two").await;
    assert!(n.switches(sid).is_empty());
    let a2 = agent("a", "mock-a2");
    n.turn(sid, &a2, false, "three").await;
    let switches = n.switches(sid);
    assert_eq!(switches.len(), 1, "{switches:?}");
    assert_eq!(switches[0]["model"], "mock-a2");
}

/// The first message of a chat, under the agent it was created with,
/// shows no notice; neither does an @mention (one message's agent), nor
/// the chat's own agent's message after it.
#[tokio::test]
async fn no_notice_for_the_first_message_or_a_mention() {
    let mut n = start().await;
    let a = agent("a", "mock-a");
    let sid = n.create(&a).await;
    n.turn(sid, &a, false, "first").await;
    n.turn(sid, &agent("x", "mock-x"), true, "@x look").await;
    n.turn(sid, &a, false, "back").await;
    assert!(n.switches(sid).is_empty(), "{:?}", n.kinds(sid));
}

/// While a turn runs (and while it is being stopped) the agent may not
/// change: refused, nothing written; after Stop it may, and the next
/// message notes it.
#[tokio::test]
async fn a_switch_while_working_is_refused_until_stopped() {
    let mut n = start().await;
    let a = agent("a", "mock-a");
    let b = agent("b", "mock-b");
    let sid = n.create(&a).await;
    n.prompt(sid, &a, false, "slow: 4000");
    n.until_started(sid).await;
    let before = n.kinds(sid).len();
    let refused = n.set_agent(sid, &b).await;
    let e = refused.expect_err("a switch while the turn runs is refused");
    assert!(e.contains("working"), "{e}");
    let req_id = n.req();
    n.send(MasterToNode::Cancel {
        req_id: req_id.clone(),
        session_id: sid,
    });
    assert_eq!(n.until_end(sid).await, TurnEndReason::Cancelled);
    assert!(n.switches(sid).is_empty());
    assert!(
        !n.kinds(sid)[before..].iter().any(|k| k == "agent_changed"),
        "{:?}",
        n.kinds(sid)
    );
    n.set_agent(sid, &b).await.unwrap();
    n.turn(sid, &b, false, "now").await;
    assert_eq!(n.switches(sid).len(), 1);
}

/// A restart between the pick and the message neither loses the switch
/// (the master holds the pick and sends it with the message) nor invents
/// one.
#[tokio::test]
async fn a_restart_between_pick_and_message_keeps_one_notice() {
    let mut n = start().await;
    let a = agent("a", "mock-a");
    let b = agent("b", "mock-b");
    let sid = n.create(&a).await;
    n.turn(sid, &a, false, "one").await;
    n.restart().await;
    n.turn(sid, &a, false, "two").await;
    assert!(n.switches(sid).is_empty(), "a restart is no switch");
    n.set_agent(sid, &b).await.unwrap();
    n.restart().await;
    n.turn(sid, &b, false, "three").await;
    n.restart().await;
    n.turn(sid, &b, false, "four").await;
    let switches = n.switches(sid);
    assert_eq!(switches.len(), 1, "{switches:?}");
    assert_eq!(switches[0]["agent"]["agent_id"], "b");
}

/// Retry after a switch runs under the agent the chat has now (T69) and
/// notes the switch once, above the retried turn.
#[tokio::test]
async fn retry_after_a_switch_runs_the_new_agent_and_notes_it_once() {
    let mut n = start().await;
    let a = agent("a", "fail");
    let b = agent("b", "mock-b");
    let sid = n.create(&a).await;
    n.prompt(sid, &a, false, "one");
    let failed = n.until_started(sid).await;
    assert_eq!(n.until_end(sid).await, TurnEndReason::Error);
    n.set_agent(sid, &b).await.unwrap();
    let req_id = n.req();
    n.send(MasterToNode::Retry(RetryMsg {
        req_id,
        session_id: sid,
        turn_id: failed,
        agent: Some(b.clone()),
        savings: None,
        context: None,
        agent_named: false,
        owner: None,
        identity: None,
    }));
    n.until_end(sid).await;
    let switches = n.switches(sid);
    assert_eq!(switches.len(), 1, "{switches:?}");
    assert_eq!(switches[0]["agent"]["agent_id"], "b");
    let evs = n.store().events_after(sid, 0, 100_000).unwrap();
    let at = evs.iter().position(|e| e.kind == "agent_changed").unwrap();
    assert_eq!(evs[at + 1].kind, "turn_started");
    assert!(evs[at + 1].payload["retry"].is_object());
    assert_eq!(evs[at + 1].payload["agent"]["agent_id"], "b");
}

/// T59: the record is not sent to the model. The first request after a
/// switch extends the last request before it, message for message, and
/// the turn after it extends that one: the log rebuilds what was sent.
#[tokio::test]
async fn a_request_after_a_switch_equals_its_rebuild() {
    let mut n = start().await;
    let a = agent("a", "mock-a");
    let b = agent("b", "mock-b");
    let sid = n.create(&a).await;
    n.turn(sid, &a, false, "one").await;
    let before = n.seen.lock().unwrap().last().unwrap().clone();
    n.set_agent(sid, &b).await.unwrap();
    let mark = n.seen.lock().unwrap().len();
    n.turn(sid, &b, false, "two").await;
    let after = n.seen.lock().unwrap()[mark].clone();
    let last = n.seen.lock().unwrap().last().unwrap().clone();
    assert_eq!(n.switches(sid).len(), 1);
    // The system prompt is the new agent's; the history after it is the
    // same messages, with nothing added for the switch.
    assert!(matches!(&after.messages[0], ChatMsg::System(s) if s.contains("@b")));
    assert!(
        after.messages[1..].starts_with(&before.messages[1..]),
        "after: {:#?}\nbefore: {:#?}",
        after.messages,
        before.messages
    );
    assert_eq!(after.model, "mock-b");
    let mark = n.seen.lock().unwrap().len();
    n.turn(sid, &b, false, "three").await;
    let next = n.seen.lock().unwrap()[mark].clone();
    assert!(
        next.messages.starts_with(&last.messages),
        "next: {:#?}\nlast: {:#?}",
        next.messages,
        last.messages
    );
}

/// Review of PR 135 (F2): a Retry of a failed @mention turn runs the
/// mentioned agent again (the master resolves the message's `@x` and says
/// so, `agent_named`); nobody picked an agent, so nothing is recorded, at
/// the retry or at the next plain message.
#[tokio::test]
async fn a_retry_of_a_mention_turn_writes_no_switch() {
    let mut n = start().await;
    let a = agent("a", "mock-a");
    let x = agent("x", "fail");
    let sid = n.create(&a).await;
    n.turn(sid, &a, false, "first").await;
    n.prompt(sid, &x, true, "@x look");
    let failed = n.until_started(sid).await;
    assert_eq!(n.until_end(sid).await, TurnEndReason::Error);
    let req_id = n.req();
    n.send(MasterToNode::Retry(RetryMsg {
        req_id,
        session_id: sid,
        turn_id: failed,
        agent: Some(x.clone()),
        savings: None,
        context: None,
        agent_named: true,
        owner: None,
        identity: None,
    }));
    n.until_end(sid).await;
    n.turn(sid, &a, false, "back to my chat").await;
    let switches = n.switches(sid);
    assert!(switches.is_empty(), "no pick was made, yet: {switches:#?}");
}

/// A chat created on a model of its own whose agent names its model: the
/// first message under that agent is no switch (the node keeps the
/// agent's model as the chat's).
#[tokio::test]
async fn a_chat_created_on_a_chosen_model_shows_no_notice_at_its_first_message() {
    let mut n = start().await;
    let a = agent("a", "mock-a");
    let sid = SessionId::new();
    let req_id = n.req();
    n.send(MasterToNode::Create {
        req_id: req_id.clone(),
        session_id: sid,
        title: "t".into(),
        cwd: n.dir.path().display().to_string(),
        model: Some("chosen-at-home".into()),
        approval_mode: Some(ApprovalMode::Auto),
        profile: None,
        agent: Some(a.clone()),
        incognito: false,
        no_callbacks: false,
        exec_mode: None,
    });
    n.ack(&req_id).await.unwrap();
    n.turn(sid, &a, false, "first").await;
    let switches = n.switches(sid);
    assert!(switches.is_empty(), "{switches:#?}");
}

/// A sub-agent's chat has no picker and takes no pick: the node refuses
/// it (the person cannot prompt it either; its parent's turn drives it).
#[tokio::test]
async fn a_sub_agents_chat_takes_no_pick() {
    let mut n = start().await;
    let mut lead = agent("lead", "mock");
    lead.delegates = vec![agent("look", "mock")];
    let sid = n.create(&lead).await;
    n.prompt(sid, &lead, false, "delegate: look=hello");
    // The child's `session_created` names the parent.
    let child = loop {
        if let NodeToMaster::Event { event } = n.next().await
            && let Ok(EventKind::SessionCreated(c)) = event.typed()
            && c.parent_session == Some(sid)
        {
            break event.session_id;
        }
    };
    n.until_end(sid).await;
    let e = n
        .set_agent(child, &agent("b", "mock-b"))
        .await
        .expect_err("a sub-agent's chat takes no pick");
    assert!(e.contains("sub-agent"), "{e}");
}
