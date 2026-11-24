//! A person's message that called several agents (ADOPTION_PLAN X.37): the
//! node asks them one after another in the same chat and log, each in a
//! turn of its own under its own spec, never two at once; Stop, a failed
//! turn, a queued message, a restart and a replay.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_agent_native::provider::{ChatMsg, ChatRequest, Completion, Provider, ProviderError};
use harness_agent_native::{NativeBackend, NativeConfig};
use harness_core::agent::{AgentSpec, ToolSelection};
use harness_core::event::*;
use harness_core::policy::ApprovalMode;
use harness_core::view::{Item, SessionView};
use harness_core::{RawEvent, SessionId};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster, PromptMsg};
use harness_store::{Durability, Store};
use tokio::sync::{Notify, mpsc};

/// A model whose answer is `answer of <model>`; a model named in `hold`
/// waits for its gate before it answers, one in `fail` is refused.
struct Scripted {
    seen: Mutex<Vec<ChatRequest>>,
    hold: Mutex<HashMap<String, Arc<Notify>>>,
    entered: Notify,
    fail: Vec<&'static str>,
    delay: Duration,
}

impl Scripted {
    fn new(fail: Vec<&'static str>) -> Arc<Self> {
        Arc::new(Self {
            seen: Mutex::default(),
            hold: Mutex::default(),
            entered: Notify::new(),
            fail,
            delay: Duration::from_millis(15),
        })
    }

    /// Make `model`'s request wait; the returned gate lets it go.
    fn hold(&self, model: &str) -> Arc<Notify> {
        let g = Arc::new(Notify::new());
        self.hold
            .lock()
            .unwrap()
            .insert(model.to_string(), g.clone());
        g
    }

    fn requests(&self, model: &str) -> Vec<ChatRequest> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.model == model)
            .cloned()
            .collect()
    }
}

#[async_trait::async_trait]
impl Provider for Scripted {
    fn name(&self) -> &'static str {
        "scripted"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        _deltas: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        self.seen.lock().unwrap().push(req.clone());
        let gate = self.hold.lock().unwrap().get(&req.model).cloned();
        if let Some(g) = gate {
            self.entered.notify_one();
            g.notified().await;
        }
        tokio::time::sleep(self.delay).await;
        if self.fail.contains(&req.model.as_str()) {
            return Err(ProviderError::Status {
                status: 400,
                body: "refused".into(),
                hint: Default::default(),
            });
        }
        Ok(Completion {
            text: format!("answer of {}", req.model),
            tool_calls: vec![],
            usage: None,
            stop: Default::default(),
            thinking: None,
        })
    }
}

fn config(dir: &std::path::Path, provider: Arc<dyn Provider>) -> NodeConfig {
    let mut policy = NodePolicy {
        workspace_roots: vec![dir.into()],
        max_approval_mode: ApprovalMode::Auto,
        ..Default::default()
    };
    policy.skills.roots.clear();
    NodeConfig {
        node_id: "n".into(),
        hostname: "test".into(),
        policy,
        store: Store::open(&dir.join("node.db"), Durability::Full).unwrap(),
        backend: Arc::new(NativeBackend::new(
            provider,
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

/// An agent as the master resolves it, on a model of its own name.
fn agent(id: &str) -> AgentSpec {
    AgentSpec {
        agent_id: id.into(),
        name: format!("Agent {id}"),
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
    node: Option<NodeRuntime>,
    link: NodeLink,
    dir: tempfile::TempDir,
    n: u32,
}

async fn attach(node: &NodeRuntime) -> NodeLink {
    let mut link = node.attach().await;
    match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { capabilities, .. } => {
            assert!(
                capabilities
                    .iter()
                    .any(|c| c == harness_protocol::CAP_PEERS),
                "{capabilities:?}"
            );
            link
        }
        other => panic!("expected hello, got {other:?}"),
    }
}

impl N {
    async fn start(p: Arc<dyn Provider>) -> N {
        let dir = tempfile::tempdir().unwrap();
        let node = NodeRuntime::start(config(dir.path(), p)).await.unwrap();
        let link = attach(&node).await;
        N {
            node: Some(node),
            link,
            dir,
            n: 0,
        }
    }

    async fn restart(&mut self, p: Arc<dyn Provider>) {
        drop(self.node.take());
        tokio::time::sleep(Duration::from_millis(200)).await;
        let node = NodeRuntime::start(config(self.dir.path(), p))
            .await
            .unwrap();
        self.link = attach(&node).await;
        self.node = Some(node);
    }

    fn send(&self, m: MasterToNode) {
        self.link.to_node.try_send(m).unwrap();
    }

    fn req(&mut self) -> String {
        self.n += 1;
        format!("r{}", self.n)
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

    /// A message that called `first` and then `peers`.
    async fn say(
        &mut self,
        sid: SessionId,
        text: &str,
        first: &AgentSpec,
        peers: &[&AgentSpec],
    ) -> Result<(), String> {
        let req_id = self.req();
        self.send(MasterToNode::Prompt(PromptMsg {
            req_id: req_id.clone(),
            agent: Some(first.clone()),
            agent_named: true,
            peers: peers.iter().map(|a| (*a).clone()).collect(),
            ..PromptMsg::test(sid, text)
        }));
        self.ack(&req_id).await
    }

    /// [`Self::say`] for a message the master gave `context` (its standing
    /// parts), as a person's signed-in message gets.
    async fn say_with_context(
        &mut self,
        sid: SessionId,
        text: &str,
        first: &AgentSpec,
        peers: &[&AgentSpec],
        context: harness_core::backend::MasterContext,
    ) -> Result<(), String> {
        let req_id = self.req();
        self.send(MasterToNode::Prompt(PromptMsg {
            req_id: req_id.clone(),
            agent: Some(first.clone()),
            context: Some(Box::new(context)),
            agent_named: true,
            peers: peers.iter().map(|a| (*a).clone()).collect(),
            ..PromptMsg::test(sid, text)
        }));
        self.ack(&req_id).await
    }

    /// [`Self::say`] for a message that also mentioned `tags` (sub-agent-only
    /// agents).
    async fn say_tagged(
        &mut self,
        sid: SessionId,
        text: &str,
        first: &AgentSpec,
        peers: &[&AgentSpec],
        tags: &[&str],
    ) -> Result<(), String> {
        let req_id = self.req();
        self.send(MasterToNode::Prompt(PromptMsg {
            req_id: req_id.clone(),
            agent: Some(first.clone()),
            agent_named: true,
            peers: peers.iter().map(|a| (*a).clone()).collect(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            ..PromptMsg::test(sid, text)
        }));
        self.ack(&req_id).await
    }

    fn log(&self, sid: SessionId) -> Vec<RawEvent> {
        Store::open(&self.dir.path().join("node.db"), Durability::Full)
            .unwrap()
            .events_after(sid, 0, 100_000)
            .unwrap()
    }

    /// Poll the log until `done` holds (the node writes it on its own).
    async fn until(&self, sid: SessionId, done: impl Fn(&[RawEvent]) -> bool) -> Vec<RawEvent> {
        for _ in 0..1000 {
            let log = self.log(sid);
            if done(&log) {
                return log;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("never: {:?}", kinds(&self.log(sid)));
    }
}

fn kinds(log: &[RawEvent]) -> Vec<String> {
    log.iter().map(|e| e.kind.clone()).collect()
}

fn ended(log: &[RawEvent]) -> usize {
    log.iter().filter(|e| e.kind == "turn_ended").count()
}

/// The turns of the log in order: (agent id, cause is a mention, ended).
fn turns(log: &[RawEvent]) -> Vec<(String, bool, Option<TurnEndReason>)> {
    let mut out: Vec<(String, bool, Option<TurnEndReason>)> = Vec::new();
    let mut ids: Vec<String> = Vec::new();
    for e in log {
        match e.typed() {
            Ok(EventKind::TurnStarted(t)) => {
                ids.push(t.turn_id);
                out.push((
                    t.agent.map(|a| a.agent_id).unwrap_or_default(),
                    matches!(t.cause, Some(TurnCause::Mention(_))),
                    None,
                ));
            }
            Ok(EventKind::TurnEnded(t)) => {
                let i = ids.iter().position(|x| *x == t.turn_id).unwrap();
                out[i].2 = Some(t.reason);
            }
            _ => {}
        }
    }
    out
}

/// The one-writer rule: a turn starts only after the one before it ended.
fn never_two_open(log: &[RawEvent]) {
    let mut open: Option<String> = None;
    for e in log {
        match e.typed() {
            Ok(EventKind::TurnStarted(t)) => {
                assert!(open.is_none(), "{open:?} was open when {} began", t.turn_id);
                open = Some(t.turn_id);
            }
            Ok(EventKind::TurnEnded(t)) => {
                assert_eq!(open.as_deref(), Some(t.turn_id.as_str()));
                open = None;
            }
            _ => {}
        }
    }
}

fn skipped(log: &[RawEvent]) -> Vec<PeersSkipped> {
    log.iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::PeersSkipped(p)) => Some(p),
            _ => None,
        })
        .collect()
}

fn user_text(m: &ChatMsg) -> Option<&str> {
    m.user_text()
}

/// Four agents answer one message one after another, in order, in one log:
/// each in its own turn under its own model, each reading the earlier
/// answers; the message is recorded once with who it called; a message sent
/// while the second agent works is read, once, at the next request of the
/// next turn (the queue's rule: the model reads it before its next request),
/// and no two turns are ever open together.
#[tokio::test]
async fn four_agents_answer_one_after_another_and_a_queued_message_is_read_once() {
    let model = Scripted::new(vec![]);
    let gate = model.hold("b");
    let mut n = N::start(model.clone()).await;
    let (a, b, c, d) = (agent("a"), agent("b"), agent("c"), agent("d"));
    let sid = n.create(&a).await;
    n.say(sid, "@a @b @c @d, yo how it is goin?", &a, &[&b, &c, &d])
        .await
        .unwrap();
    // The first agent has answered; the second is in the model call.
    model.entered.notified().await;
    let log = n.log(sid);
    assert_eq!(turns(&log).len(), 2, "{:?}", kinds(&log));
    // A message while the list runs: queued, as for any running turn.
    n.say(sid, "and one more thing", &b, &[]).await.unwrap();
    gate.notify_one();
    let log = n
        .until(sid, |l| {
            ended(l) >= 4 && turns(l).iter().all(|t| t.2.is_some())
        })
        .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let log2 = n.log(sid);
    assert_eq!(log.len(), log2.len(), "nothing more starts after the list");
    never_two_open(&log2);
    let t = turns(&log2);
    let order: Vec<&str> = t.iter().map(|t| t.0.as_str()).collect();
    assert_eq!(order, ["a", "b", "c", "d"], "{:?}", kinds(&log2));
    assert_eq!(
        t.iter().map(|t| t.1).collect::<Vec<_>>(),
        [false, true, true, true]
    );
    let log = log2;
    // The message was recorded once, with who it called, in order.
    let msgs: Vec<UserMessage> = log
        .iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::UserMessage(m)) if !m.queued => Some(m),
            _ => None,
        })
        .collect();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].triggers, ["a", "b", "c", "d"]);
    // Each mention turn names the message that called it.
    let seq = log.iter().find(|e| e.kind == "user_message").unwrap().seq;
    for e in &log {
        if let Ok(EventKind::TurnStarted(TurnStarted {
            cause: Some(TurnCause::Mention(m)),
            ..
        })) = e.typed()
        {
            assert_eq!((m.message_seq, m.by), (seq, CalledBy::Person));
        }
    }
    // The queued message is read exactly once, by the turn of `c`.
    let delivered: Vec<(String, Vec<u64>)> = log
        .iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::QueuedDelivered(d)) => Some((d.turn_id, d.seqs)),
            _ => None,
        })
        .collect();
    assert_eq!(delivered.len(), 1, "{delivered:?}");
    let c_turn = log
        .iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::TurnStarted(t))
                if t.agent.as_ref().is_some_and(|a| a.agent_id == "c") =>
            {
                Some(t.turn_id)
            }
            _ => None,
        })
        .next()
        .unwrap();
    assert_eq!(delivered[0].0, c_turn);
    let holding = |who: &str| {
        model
            .requests(who)
            .iter()
            .map(|r| {
                r.messages
                    .iter()
                    .filter(|m| user_text(m).is_some_and(|t| t.contains("and one more thing")))
                    .count()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(holding("b"), [0], "b had passed its last step");
    assert_eq!(holding("c"), [1]);
    assert_eq!(holding("d"), [1], "rebuilt from the log, once");
    // Each later agent read the earlier agents' answers and its own opening.
    let rc = model.requests("c");
    let flat = format!("{:?}", rc[0].messages);
    assert!(
        flat.contains("answer of a") && flat.contains("answer of b"),
        "{flat}"
    );
    assert!(
        flat.contains(CALLED_LEAD) && flat.contains("You are @c."),
        "{flat}"
    );
    assert!(!flat.contains("answer of d"));
    // The fold shows who was called and where each agent's turn begins.
    let view = SessionView::fold(&log).unwrap();
    let called: Vec<&str> = view
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Called { agent, .. } => agent.as_ref().map(|a| a.agent_id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(called, ["b", "c", "d"]);
}

/// A later request is the earlier one with more after it: the mention
/// turn's opening is rebuilt from the log byte for byte (the provider's
/// prompt cache), also by a turn of a later message.
#[tokio::test]
async fn a_later_turn_rebuilds_a_mention_turns_messages_byte_for_byte() {
    let model = Scripted::new(vec![]);
    let mut n = N::start(model.clone()).await;
    let (a, b, c) = (agent("a"), agent("b"), agent("c"));
    let sid = n.create(&a).await;
    n.say(sid, "@a @b @c hi", &a, &[&b, &c]).await.unwrap();
    n.until(sid, |l| ended(l) == 3).await;
    let sent = model.requests("c").remove(0).messages;
    // The same agent, in a later message of its own.
    n.say(sid, "@c once more", &c, &[]).await.unwrap();
    n.until(sid, |l| ended(l) == 4).await;
    let later = model.requests("c").remove(1).messages;
    // Everything c was sent the first time is, unchanged, the start of what
    // it is sent now (the system prompt, the history, its own opening).
    assert_eq!(later[..sent.len()], sent[..]);
    assert!(later.len() > sent.len());
}

/// Stop ends the running turn and the rest of the list; one note names the
/// agents that were not asked, and nothing runs after it.
#[tokio::test]
async fn stop_ends_the_turn_and_the_rest_and_names_who_was_not_asked() {
    let model = Scripted::new(vec![]);
    let _gate = model.hold("b");
    let mut n = N::start(model.clone()).await;
    let (a, b, c, d) = (agent("a"), agent("b"), agent("c"), agent("d"));
    let sid = n.create(&a).await;
    n.say(sid, "@a @b @c @d go", &a, &[&b, &c, &d])
        .await
        .unwrap();
    model.entered.notified().await;
    let req_id = n.req();
    n.send(MasterToNode::Cancel {
        req_id: req_id.clone(),
        session_id: sid,
    });
    n.ack(&req_id).await.unwrap();
    let log = n.until(sid, |l| !skipped(l).is_empty()).await;
    never_two_open(&log);
    let t = turns(&log);
    assert_eq!(
        t.iter().map(|t| t.0.as_str()).collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert_eq!(t[1].2, Some(TurnEndReason::Cancelled));
    let s = skipped(&log);
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].agents, ["c", "d"]);
    assert_eq!(s[0].reason, PeersSkipReason::Stopped);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(turns(&n.log(sid)).len(), 2, "nothing runs after it");
    assert!(model.requests("c").is_empty());
}

/// A turn that fails does not stop the list: the next agent still runs.
#[tokio::test]
async fn a_failed_turn_does_not_stop_the_next_agent() {
    let model = Scripted::new(vec!["b"]);
    let mut n = N::start(model.clone()).await;
    let (a, b, c) = (agent("a"), agent("b"), agent("c"));
    let sid = n.create(&a).await;
    n.say(sid, "@a @b @c go", &a, &[&b, &c]).await.unwrap();
    let log = n.until(sid, |l| ended(l) == 3).await;
    never_two_open(&log);
    let t = turns(&log);
    assert_eq!(t[1].2, Some(TurnEndReason::Error));
    assert_eq!(t[2].2, Some(TurnEndReason::Completed));
    assert!(skipped(&log).is_empty());
    // The failed turn is not retried (it has no message of its own).
    let e = harness_core::retry::check(&log, &last_turn(&log)).unwrap_err();
    assert!(!e.is_empty());
}

fn last_turn(log: &[RawEvent]) -> String {
    log.iter()
        .rev()
        .find_map(|e| match e.typed() {
            Ok(EventKind::TurnStarted(t)) => Some(t.turn_id),
            _ => None,
        })
        .unwrap()
}

/// A restart in the middle of the list: the turn that was running is closed
/// as interrupted, one note says which agents were not asked, nothing runs,
/// and a second restart writes no second note.
#[tokio::test]
async fn a_restart_mid_list_says_who_was_not_asked_once() {
    let model = Scripted::new(vec![]);
    let _gate = model.hold("b");
    let mut n = N::start(model.clone()).await;
    let (a, b, c, d) = (agent("a"), agent("b"), agent("c"), agent("d"));
    let sid = n.create(&a).await;
    n.say(sid, "@a @b @c @d go", &a, &[&b, &c, &d])
        .await
        .unwrap();
    model.entered.notified().await;
    n.restart(Scripted::new(vec![])).await;
    let log = n.log(sid);
    never_two_open(&log);
    let s = skipped(&log);
    assert_eq!(s.len(), 1, "{:?}", kinds(&log));
    assert_eq!(s[0].agents, ["c", "d"]);
    assert_eq!(s[0].reason, PeersSkipReason::Restarted);
    assert_eq!(turns(&log)[1].2, Some(TurnEndReason::Interrupted));
    tokio::time::sleep(Duration::from_millis(200)).await;
    n.restart(Scripted::new(vec![])).await;
    let log = n.log(sid);
    assert_eq!(skipped(&log).len(), 1, "said once");
    assert_eq!(turns(&log).len(), 2, "nothing was asked after the restart");
}

/// What the node refuses: more agents than the limit, an agent twice, a
/// message that calls several agents while a turn runs, one that answers a
/// card or asks for parallel answers.
#[tokio::test]
async fn the_node_refuses_what_it_cannot_run_in_order() {
    let model = Scripted::new(vec![]);
    let gate = model.hold("a");
    let mut n = N::start(model.clone()).await;
    let (a, b, c, d, e) = (agent("a"), agent("b"), agent("c"), agent("d"), agent("e"));
    let sid = n.create(&a).await;
    let r = n.say(sid, "x", &a, &[&b, &c, &d, &e]).await.unwrap_err();
    assert!(r.contains("at most 4"), "{r}");
    let r = n.say(sid, "x", &a, &[&b, &b]).await.unwrap_err();
    assert!(r.contains("once"), "{r}");
    let r = n.say(sid, "x", &a, &[&a]).await.unwrap_err();
    assert!(r.contains("once"), "{r}");
    assert!(n.log(sid).iter().all(|e| e.kind != "user_message"));
    // While a turn runs, a list message waits (refused, nothing recorded).
    n.say(sid, "first", &a, &[]).await.unwrap();
    model.entered.notified().await;
    let r = n.say(sid, "@a @b hi", &a, &[&b]).await.unwrap_err();
    assert!(r.contains("a turn is already running"), "{r}");
    gate.notify_one();
    n.until(sid, |l| ended(l) == 1).await;
    // Nothing of the refused ones is in the log.
    let msgs = n
        .log(sid)
        .iter()
        .filter(|e| e.kind == "user_message")
        .count();
    assert_eq!(msgs, 1);
}

/// X.37: sub-agent-only names a message mentions (tags) are recorded on the
/// message, never start a turn, and the answering agent's turn says, in one
/// line, which of them it has among its own delegates (and nothing about
/// the others); a replay rebuilds the same line.
#[tokio::test]
async fn tags_are_recorded_and_named_only_to_agents_that_may_delegate_to_them() {
    let model = Scripted::new(vec![]);
    let mut n = N::start(model.clone()).await;
    let mut a = agent("a");
    a.delegates = vec![agent("advisor")];
    let mut b = agent("b");
    b.delegates = vec![agent("advisor"), agent("scout")];
    let c = agent("c");
    let sid = n.create(&a).await;
    n.say_tagged(
        sid,
        "@a @b @c @advisor @scout go",
        &a,
        &[&b, &c],
        &["advisor", "scout"],
    )
    .await
    .unwrap();
    let log = n.until(sid, |l| ended(l) == 3).await;
    never_two_open(&log);
    assert_eq!(
        turns(&log).iter().map(|t| t.0.as_str()).collect::<Vec<_>>(),
        ["a", "b", "c"],
        "a tag starts no turn"
    );
    let m = log
        .iter()
        .find_map(|e| match e.typed() {
            Ok(EventKind::UserMessage(m)) => Some(m),
            _ => None,
        })
        .unwrap();
    assert_eq!(m.tags, ["advisor", "scout"]);
    let one = harness_core::mention::tags_line(&["advisor".into()]).unwrap();
    let two = harness_core::mention::tags_line(&["advisor".into(), "scout".into()]).unwrap();
    let flat = |who: &str| format!("{:?}", model.requests(who)[0].messages);
    assert_eq!(
        flat("a").matches(&one).count(),
        1,
        "the first agent: in its context, once"
    );
    assert!(
        !flat("a").contains(&two),
        "a has no delegate scout: not offered"
    );
    assert_eq!(
        flat("b").matches(&two).count(),
        1,
        "a later agent: in its opening, once"
    );
    // c's history holds a's line and b's (one each); c's own names nothing.
    assert_eq!(flat("c").matches(&two).count(), 1);
    assert_eq!(
        flat("c")
            .matches("as agents you may run as sub-agents")
            .count(),
        2
    );
    // The cause records what each later agent was told.
    let causes: Vec<Vec<String>> = log
        .iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::TurnStarted(TurnStarted {
                cause: Some(TurnCause::Mention(m)),
                ..
            })) => Some(m.tags),
            _ => None,
        })
        .collect();
    assert_eq!(
        causes,
        [vec!["advisor".to_string(), "scout".to_string()], vec![]]
    );
    // A later message rebuilds the same lines from the log.
    n.say(sid, "@b again", &b, &[]).await.unwrap();
    n.until(sid, |l| ended(l) == 4).await;
    let later = format!("{:?}", model.requests("b")[1].messages);
    assert_eq!(later.matches(&two).count(), 1, "b's earlier opening, once");
    let first_b = &model.requests("b")[0].messages;
    assert_eq!(
        model.requests("b")[1].messages[..first_b.len()],
        first_b[..]
    );
    // A hostile or confused master: more than 16, a bad handle, a repeat.
    for bad in [vec!["Bad Name"], vec!["x", "x"]] {
        assert!(n.say_tagged(sid, "x", &a, &[], &bad).await.is_err());
    }
    let many: Vec<String> = (0..17).map(|i| format!("t{i}")).collect();
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    assert!(n.say_tagged(sid, "x", &a, &[], &refs).await.is_err());
}

/// What an agent called after the first knows (review of PR 166, M2): the
/// first message's context as the first agent got it (your personalisation,
/// the project's brief and board), without the instructions for a tool it
/// does not have (`remember`: its own spec denies it) and without memory.
/// The first agent's own request is unchanged, and the mention turn's is
/// rebuilt byte for byte by a later turn.
#[tokio::test]
async fn a_later_agent_is_not_told_how_to_remember() {
    use harness_core::backend::{MasterContext, StandingKey as K, StandingPart};
    let part = |part, text: &str| StandingPart {
        part,
        text: text.into(),
    };
    let ctx = MasterContext {
        turn: Some("TURN-LINE".into()),
        standing: vec![
            part(K::Personal, "PERSONAL-X"),
            part(K::Memory, "MEMORY-FACTS"),
            part(K::Remember, "REMEMBER-RULES"),
            part(K::Project, "BRIEF-Z"),
            part(K::Board, "BOARD-CARDS"),
        ],
        unattended: false,
        push: None,
    };
    let model = Scripted::new(vec![]);
    let mut n = N::start(model.clone()).await;
    let (a, b) = (agent("a"), agent("b"));
    let sid = n.create(&a).await;
    n.say_with_context(sid, "@a @b hi", &a, &[&b], ctx)
        .await
        .unwrap();
    n.until(sid, |l| ended(l) == 2).await;
    let flat = |who: &str, i: usize| format!("{:?}", model.requests(who)[i].messages);
    // The first agent: everything it was given.
    for t in [
        "TURN-LINE",
        "PERSONAL-X",
        "MEMORY-FACTS",
        "REMEMBER-RULES",
        "BRIEF-Z",
        "BOARD-CARDS",
    ] {
        assert!(flat("a", 0).contains(t), "a lacks {t}");
    }
    // The later agent: personalisation, brief, board and the turn's line;
    // no memory, and no `remember` instructions.
    for t in ["TURN-LINE", "PERSONAL-X", "BRIEF-Z", "BOARD-CARDS"] {
        assert!(flat("b", 0).contains(t), "b lacks {t}");
    }
    for t in ["MEMORY-FACTS", "REMEMBER-RULES"] {
        assert!(!flat("b", 0).contains(t), "b was given {t}");
    }
    // Replay: a later message to b rebuilds b's request byte for byte.
    n.say(sid, "@b again", &b, &[]).await.unwrap();
    n.until(sid, |l| ended(l) == 3).await;
    let first_b = &model.requests("b")[0].messages;
    let later = &model.requests("b")[1].messages;
    assert_eq!(later[..first_b.len()], first_b[..]);
}
