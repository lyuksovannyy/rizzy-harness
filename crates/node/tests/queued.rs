//! A message the person sends while a turn runs (X.5): stored at once and
//! acknowledged, read by the model before its next request, exactly once;
//! one that comes after the model's last step starts the next turn. Stop,
//! the cap, an approval card and a node restart.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_agent_native::provider::{ChatMsg, ChatRequest, Completion, Provider, ProviderError};
use harness_agent_native::{NativeBackend, NativeConfig};
use harness_core::event::*;
use harness_core::policy::{ApprovalMode, Decision};
use harness_core::view::{Item, SessionView};
use harness_core::{RawEvent, SessionId};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};
use serde_json::{Value, json};
use tokio::sync::{Notify, mpsc};

/// What the model answers to the n-th request (0-based).
type Script = dyn Fn(usize, &ChatRequest) -> Completion + Send + Sync;

/// A model whose first request waits until the test opens the gate, so a
/// message can be sent while the turn is known to be running. Records
/// every request.
struct Gated {
    script: Box<Script>,
    entered: Notify,
    gate: Notify,
    seen: Mutex<Vec<ChatRequest>>,
    /// The request that waits for the gate (0: the first).
    hold: usize,
    /// A request answered with a provider error instead.
    fail: Option<usize>,
}

impl Gated {
    fn new(
        script: impl Fn(usize, &ChatRequest) -> Completion + Send + Sync + 'static,
    ) -> Arc<Self> {
        Self::with(script, 0, None)
    }

    fn with(
        script: impl Fn(usize, &ChatRequest) -> Completion + Send + Sync + 'static,
        hold: usize,
        fail: Option<usize>,
    ) -> Arc<Self> {
        Arc::new(Self {
            script: Box::new(script),
            entered: Notify::new(),
            gate: Notify::new(),
            seen: Mutex::new(vec![]),
            hold,
            fail,
        })
    }

    fn requests(&self) -> Vec<ChatRequest> {
        self.seen.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl Provider for Gated {
    fn name(&self) -> &'static str {
        "gated"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        _deltas: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        let n = {
            let mut s = self.seen.lock().unwrap();
            s.push(req.clone());
            s.len() - 1
        };
        if n == self.hold {
            self.entered.notify_one();
            self.gate.notified().await;
        }
        if self.fail == Some(n) {
            return Err(ProviderError::Status {
                status: 400,
                body: "bad request".into(),
                hint: Default::default(),
            });
        }
        Ok((self.script)(n, req))
    }
}

fn answer(text: &str) -> Completion {
    Completion {
        text: text.into(),
        tool_calls: vec![],
        usage: None,
        stop: Default::default(),
    }
}

fn call(tool: &str, input: Value) -> Completion {
    Completion {
        text: String::new(),
        tool_calls: vec![ToolCallRef {
            call_id: harness_core::new_id(),
            tool: tool.into(),
            input,
        }],
        usage: None,
        stop: Default::default(),
    }
}

/// Step 1 lists the folder, step 2 answers.
fn one_step(n: usize, _: &ChatRequest) -> Completion {
    match n {
        0 => call("glob", json!({"pattern": "*"})),
        _ => answer("done"),
    }
}

/// The user-role texts of a request that hold `needle`.
fn holding(req: &ChatRequest, needle: &str) -> usize {
    req.messages
        .iter()
        .filter(|m| m.user_text().is_some_and(|t| t.contains(needle)))
        .count()
}

struct Node {
    dir: tempfile::TempDir,
    node: Option<NodeRuntime>,
    link: NodeLink,
    seen: Vec<RawEvent>,
}

fn config_as(dir: &std::path::Path, provider: Arc<dyn Provider>, profile: Profile) -> NodeConfig {
    NodeConfig {
        node_id: "local".into(),
        hostname: "test".into(),
        policy: NodePolicy {
            workspace_roots: vec![dir.into()],
            max_approval_mode: ApprovalMode::Auto,
            ..Default::default()
        },
        store: Store::open(&dir.join("node.db"), Durability::Full).unwrap(),
        backend: Arc::new(NativeBackend::new(
            provider,
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    }
}

impl Node {
    async fn start(provider: Arc<dyn Provider>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("ws")).unwrap();
        Self::on(dir, provider).await
    }

    /// A node of `profile` (a chat node takes parallel answers).
    async fn start_as(provider: Arc<dyn Provider>, profile: Profile) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("ws")).unwrap();
        Self::on_as(dir, provider, profile).await
    }

    async fn on(dir: tempfile::TempDir, provider: Arc<dyn Provider>) -> Self {
        Self::on_as(dir, provider, Profile::Agentic).await
    }

    async fn on_as(dir: tempfile::TempDir, provider: Arc<dyn Provider>, profile: Profile) -> Self {
        let node = NodeRuntime::start(config_as(dir.path(), provider, profile))
            .await
            .unwrap();
        let mut link = node.attach().await;
        assert!(matches!(
            link.from_node.recv().await.unwrap(),
            NodeToMaster::Hello { .. }
        ));
        Self {
            dir,
            node: Some(node),
            link,
            seen: vec![],
        }
    }

    /// The node goes away with its turn still running; another starts on
    /// the same data directory with `provider`.
    async fn restart(self, provider: Arc<dyn Provider>) -> Self {
        let Node {
            dir, node, link, ..
        } = self;
        drop(link);
        drop(node);
        Self::on(dir, provider).await
    }

    fn send(&self, m: MasterToNode) {
        self.link.to_node.try_send(m).unwrap();
    }

    async fn next(&mut self) -> NodeToMaster {
        let msg = tokio::time::timeout(Duration::from_secs(10), self.link.from_node.recv())
            .await
            .expect("timed out")
            .expect("link closed");
        if let NodeToMaster::Event { event } = &msg {
            self.seen.push(event.clone());
        }
        msg
    }

    async fn ack(&mut self, req_id: &str) -> Result<(), String> {
        loop {
            if let NodeToMaster::Ack {
                req_id: r,
                ok,
                error,
            } = self.next().await
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

    async fn until(&mut self, pred: impl Fn(&EventKind) -> bool) {
        if self.seen.iter().any(|e| e.typed().is_ok_and(|k| pred(&k))) {
            return;
        }
        loop {
            if let NodeToMaster::Event { event } = self.next().await
                && pred(&event.typed().unwrap())
            {
                return;
            }
        }
    }

    /// Wait for the `n`-th `turn_ended` of the log (1-based).
    async fn ended(&mut self, n: usize) {
        loop {
            let ends = self.seen.iter().filter(|e| e.kind == "turn_ended").count();
            if ends >= n {
                return;
            }
            self.next().await;
        }
    }

    /// Wait for a `turn_ended` received after the first `mark` events.
    async fn ended_after(&mut self, mark: usize) {
        while !self.seen[mark.min(self.seen.len())..]
            .iter()
            .any(|e| e.kind == "turn_ended")
        {
            self.next().await;
        }
    }

    async fn create(&mut self, mode: ApprovalMode) -> SessionId {
        let sid = SessionId::new();
        self.send(MasterToNode::Create {
            exec_mode: None,
            no_callbacks: false,
            profile: None,
            incognito: false,
            agent: None,
            req_id: "c".into(),
            session_id: sid,
            title: "t".into(),
            cwd: self.dir.path().join("ws").display().to_string(),
            model: None,
            approval_mode: Some(mode),
        });
        assert_eq!(self.ack("c").await, Ok(()));
        sid
    }

    /// A command written as JSON, as the master sends it.
    fn send_json(&self, m: Value) {
        self.send(serde_json::from_value(m).unwrap());
    }

    fn prompt(&self, sid: SessionId, req_id: &str, text: &str) {
        self.send(MasterToNode::Prompt {
            req_id: req_id.into(),
            session_id: sid,
            text: text.into(),
            agent: None,
            attachments: vec![],
            compare: vec![],
            savings: None,
            context: None,
            reply: None,
            images: vec![],
            agent_named: false,
        });
    }

    fn log(&self, sid: SessionId) -> Vec<RawEvent> {
        Store::open(&self.dir.path().join("node.db"), Durability::Full)
            .unwrap()
            .events_after(sid, 0, 10_000)
            .unwrap()
    }
}

fn kinds(log: &[RawEvent]) -> Vec<String> {
    log.iter().map(|e| e.kind.clone()).collect()
}

/// The queued `user_message`s of a log, with their seqs.
fn queued(log: &[RawEvent]) -> Vec<(u64, UserMessage)> {
    log.iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::UserMessage(m)) if m.queued => Some((e.seq, m)),
            _ => None,
        })
        .collect()
}

fn delivered(log: &[RawEvent]) -> Vec<u64> {
    log.iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::QueuedDelivered(d)) => Some(d.seqs),
            _ => None,
        })
        .flatten()
        .collect()
}

/// A prompt during a running turn is stored and acknowledged (no refusal),
/// marked as sent while the turn ran, and the model reads it before its
/// next request, after the step's tool result, exactly once: in that
/// request, and rebuilt from the log in the next turn's.
#[tokio::test]
async fn a_message_during_a_turn_reaches_the_next_step_once() {
    let model = Gated::new(one_step);
    let mut n = Node::start(model.clone()).await;
    let sid = n.create(ApprovalMode::Auto).await;
    n.prompt(sid, "p1", "list the files");
    assert_eq!(n.ack("p1").await, Ok(()));
    model.entered.notified().await;
    n.prompt(sid, "p2", "also check the README");
    assert_eq!(
        n.ack("p2").await,
        Ok(()),
        "a running turn takes the message"
    );
    // Stored before the ack, in the running turn.
    let log = n.log(sid);
    let q = queued(&log);
    assert_eq!(q.len(), 1, "{:?}", kinds(&log));
    let turn = log
        .iter()
        .find_map(|e| match e.typed() {
            Ok(EventKind::TurnStarted(t)) => Some(t.turn_id),
            _ => None,
        })
        .unwrap();
    assert_eq!(q[0].1.turn_id, turn);
    assert_eq!(q[0].1.text, "also check the README");
    model.gate.notify_one();
    n.ended(1).await;
    let reqs = model.requests();
    assert_eq!(reqs.len(), 2, "one turn, two steps");
    assert_eq!(holding(&reqs[0], "also check the README"), 0);
    assert_eq!(holding(&reqs[1], "also check the README"), 1);
    // After the step's tool result, marked as written while it worked.
    let last = reqs[1].messages.last().unwrap();
    let text = last.user_text().expect("the person's message comes last");
    assert!(text.starts_with("also check the README\n\n"), "{text}");
    assert!(text.contains(QUEUED_LEAD), "{text}");
    assert!(matches!(
        reqs[1].messages[reqs[1].messages.len() - 2],
        ChatMsg::Tool { .. }
    ));
    // Recorded as read in the same turn, after the tool's result.
    let log = n.log(sid);
    assert_eq!(delivered(&log), vec![q[0].0]);
    let ks = kinds(&log);
    let at = ks.iter().position(|k| k == "queued_delivered").unwrap();
    assert_eq!(ks[at - 1], "tool_call_finished", "{ks:?}");
    assert_eq!(ks.iter().filter(|k| *k == "turn_started").count(), 1);
    // The fold: shown where it was sent, read.
    let view = SessionView::fold(&log).unwrap();
    let mark = view.items.iter().find_map(|i| match i {
        Item::User { queued, .. } => *queued,
        _ => None,
    });
    assert_eq!(mark.map(|q| q.read), Some(true));
    // The next turn rebuilds it from the log, once.
    n.prompt(sid, "p3", "thanks");
    assert_eq!(n.ack("p3").await, Ok(()));
    n.ended(2).await;
    let reqs = model.requests();
    assert_eq!(holding(&reqs[2], "also check the README"), 1);
}

/// A message that comes after the model's last step (it answered without
/// tools) starts the next turn: the model reads it there, once, and no
/// third turn follows.
#[tokio::test]
async fn a_message_after_the_last_step_starts_the_next_turn() {
    let model = Gated::new(|n, _| answer(&format!("answer {n}")));
    let mut n = Node::start(model.clone()).await;
    let sid = n.create(ApprovalMode::Auto).await;
    n.prompt(sid, "p1", "hello");
    assert_eq!(n.ack("p1").await, Ok(()));
    model.entered.notified().await;
    n.prompt(sid, "p2", "and one more thing");
    assert_eq!(n.ack("p2").await, Ok(()));
    model.gate.notify_one();
    n.ended(2).await;
    let reqs = model.requests();
    assert_eq!(reqs.len(), 2, "{reqs:?}");
    assert_eq!(holding(&reqs[0], "and one more thing"), 0);
    let text = reqs[1].messages.last().unwrap().user_text().unwrap();
    // After the node's environment line, which opens every turn.
    let text = harness_agent_native::provider::without_environment(text);
    assert!(text.starts_with("and one more thing"), "{text}");
    assert_eq!(holding(&reqs[1], "and one more thing"), 1);
    let log = n.log(sid);
    let q = queued(&log);
    assert_eq!(delivered(&log), vec![q[0].0]);
    // The second turn: started, then the message read, and no own message.
    let ks = kinds(&log);
    let second = ks.iter().rposition(|k| k == "turn_started").unwrap();
    assert_eq!(ks[second + 1], "queued_delivered", "{ks:?}");
    let users = ks.iter().filter(|k| *k == "user_message").count();
    assert_eq!(users, 2, "nothing is recorded twice: {ks:?}");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(model.requests().len(), 2, "no turn after that");
}

/// Stop works as before with a message waiting: the turn ends cancelled
/// and no turn starts by itself; the person's next message takes the
/// waiting one with it, once.
#[tokio::test]
async fn stop_with_a_waiting_message_starts_nothing() {
    let model = Gated::new(|_, _| answer("ok"));
    let mut n = Node::start(model.clone()).await;
    let sid = n.create(ApprovalMode::Auto).await;
    n.prompt(sid, "p1", "slow work");
    assert_eq!(n.ack("p1").await, Ok(()));
    model.entered.notified().await;
    n.prompt(sid, "p2", "waiting words");
    assert_eq!(n.ack("p2").await, Ok(()));
    n.send(MasterToNode::Cancel {
        req_id: "x".into(),
        session_id: sid,
    });
    assert_eq!(n.ack("x").await, Ok(()));
    n.ended(1).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let log = n.log(sid);
    let ks = kinds(&log);
    assert_eq!(
        ks.iter().filter(|k| *k == "turn_started").count(),
        1,
        "{ks:?}"
    );
    assert!(delivered(&log).is_empty());
    assert!(log.iter().any(|e| matches!(
        e.typed(),
        Ok(EventKind::TurnEnded(TurnEnded {
            reason: TurnEndReason::Cancelled,
            ..
        }))
    )));
    // Still unread in the fold.
    let view = SessionView::fold(&log).unwrap();
    assert!(view.items.iter().any(|i| matches!(
        i,
        Item::User {
            queued: Some(q),
            ..
        } if !q.read
    )));
    n.prompt(sid, "p3", "carry forward");
    assert_eq!(n.ack("p3").await, Ok(()));
    n.ended(2).await;
    let reqs = model.requests();
    let last = reqs.last().unwrap();
    assert_eq!(holding(last, "waiting words"), 1);
    // In the order they were sent: the waiting one, then the new one.
    let texts: Vec<&str> = last.messages.iter().filter_map(|m| m.user_text()).collect();
    let (w, g) = (
        texts
            .iter()
            .position(|t| t.contains("waiting words"))
            .unwrap(),
        texts
            .iter()
            .position(|t| t.contains("carry forward"))
            .unwrap(),
    );
    assert!(w < g, "{texts:?}");
    let log = n.log(sid);
    assert_eq!(delivered(&log).len(), 1);
}

/// At most 8 messages wait for the running turn's next step; the ninth is
/// refused with the old reason and is not stored.
#[tokio::test]
async fn at_most_eight_messages_wait() {
    let model = Gated::new(|_, _| answer("ok"));
    let mut n = Node::start(model.clone()).await;
    let sid = n.create(ApprovalMode::Auto).await;
    n.prompt(sid, "p", "work");
    assert_eq!(n.ack("p").await, Ok(()));
    model.entered.notified().await;
    for i in 0..8 {
        n.prompt(sid, &format!("q{i}"), &format!("note {i}"));
        assert_eq!(n.ack(&format!("q{i}")).await, Ok(()), "message {i}");
    }
    n.prompt(sid, "q8", "note 8");
    let err = n.ack("q8").await.unwrap_err();
    assert!(err.starts_with("a turn is already running"), "{err}");
    assert!(err.contains('8'), "{err}");
    assert_eq!(queued(&n.log(sid)).len(), 8);
    model.gate.notify_one();
    n.ended(2).await;
}

/// A message while the turn waits on an approval card is queued like any
/// other; the card still needs its answer, and the model reads the
/// message after the tool's result.
#[tokio::test]
async fn a_message_while_an_approval_waits_is_queued() {
    let model = Gated::new(|n, _| match n {
        0 => call("write", json!({"path": "a.txt", "content": "x"})),
        _ => answer("written"),
    });
    let mut n = Node::start(model.clone()).await;
    let sid = n.create(ApprovalMode::Ask).await;
    n.prompt(sid, "p", "write a file");
    assert_eq!(n.ack("p").await, Ok(()));
    model.entered.notified().await;
    model.gate.notify_one();
    n.until(|k| matches!(k, EventKind::ApprovalRequested(_)))
        .await;
    n.prompt(sid, "q", "name it b.txt next time");
    assert_eq!(n.ack("q").await, Ok(()));
    let approval_id = n
        .seen
        .iter()
        .find_map(|e| match e.typed() {
            Ok(EventKind::ApprovalRequested(a)) => Some(a.approval_id),
            _ => None,
        })
        .unwrap();
    // The card is still open: nothing ran.
    assert!(!n.seen.iter().any(|e| e.kind == "approval_resolved"));
    n.send(MasterToNode::ApprovalRespond {
        req_id: "a".into(),
        session_id: sid,
        approval_id,
        decision: Decision::Allow,
    });
    assert_eq!(n.ack("a").await, Ok(()));
    n.ended(1).await;
    let reqs = model.requests();
    assert_eq!(holding(&reqs[1], "name it b.txt next time"), 1);
}

/// A restart with a stored message the model never read: the turn is
/// closed as interrupted, and the person's next turn delivers the message
/// from the log, exactly once.
#[tokio::test]
async fn a_restart_delivers_a_waiting_message_once() {
    let model = Gated::new(|_, _| answer("ok"));
    let mut n = Node::start(model.clone()).await;
    let sid = n.create(ApprovalMode::Auto).await;
    n.prompt(sid, "p1", "long job");
    assert_eq!(n.ack("p1").await, Ok(()));
    model.entered.notified().await;
    n.prompt(sid, "p2", "before the restart");
    assert_eq!(n.ack("p2").await, Ok(()));
    // The old model stays blocked: its turn never ends in this process.
    let after = Gated::new(|_, _| answer("ok"));
    after.gate.notify_one();
    let mut n = n.restart(after.clone()).await;
    let log = n.log(sid);
    assert!(log.iter().any(|e| matches!(
        e.typed(),
        Ok(EventKind::TurnEnded(TurnEnded {
            reason: TurnEndReason::Interrupted,
            ..
        }))
    )));
    assert!(delivered(&log).is_empty());
    n.prompt(sid, "p3", "after the restart");
    assert_eq!(n.ack("p3").await, Ok(()));
    let mark = n.seen.len();
    n.ended_after(mark).await;
    let reqs = after.requests();
    assert_eq!(holding(&reqs[0], "before the restart"), 1);
    let log = n.log(sid);
    assert_eq!(delivered(&log), vec![queued(&log)[0].0]);
    // And once more: a later turn reads it from the log, still once.
    n.prompt(sid, "p4", "again");
    assert_eq!(n.ack("p4").await, Ok(()));
    let mark = n.seen.len();
    n.ended_after(mark).await;
    let reqs = after.requests();
    assert_eq!(holding(reqs.last().unwrap(), "before the restart"), 1);
    assert_eq!(delivered(&n.log(sid)).len(), 1);
}

/// An agent the master resolved, as JSON.
fn agent(id: &str) -> Value {
    json!({"agent_id": id, "name": id, "system_prompt": "work", "tools": "all",
           "prompt_hash": format!("h-{id}")})
}

/// Review ST-3: a turn that ends with an error (here the provider's)
/// leaves the person's message for a turn the node starts at once; the
/// message is often the fix.
#[tokio::test]
async fn a_message_after_an_error_starts_the_next_turn() {
    let model = Gated::with(|_, _| answer("ok"), 0, Some(0));
    let mut n = Node::start(model.clone()).await;
    let sid = n.create(ApprovalMode::Auto).await;
    n.prompt(sid, "p1", "do it");
    assert_eq!(n.ack("p1").await, Ok(()));
    model.entered.notified().await;
    n.prompt(sid, "p2", "use the other endpoint");
    assert_eq!(n.ack("p2").await, Ok(()));
    model.gate.notify_one();
    n.ended(2).await;
    let reqs = model.requests();
    assert_eq!(reqs.len(), 2, "the failed request, then the next turn's");
    assert_eq!(holding(&reqs[1], "use the other endpoint"), 1);
    let log = n.log(sid);
    let ks = kinds(&log);
    let second = ks.iter().rposition(|k| k == "turn_started").unwrap();
    assert_eq!(ks[second + 1], "queued_delivered", "{ks:?}");
    assert!(log.iter().any(|e| matches!(
        e.typed(),
        Ok(EventKind::TurnEnded(TurnEnded {
            reason: TurnEndReason::Error,
            ..
        }))
    )));
}

/// Review ST-8: a turn the node starts for waiting messages takes the
/// savings, not the last turn's per-turn context (a routine's note, a
/// recall for other words).
#[tokio::test]
async fn the_next_turn_does_not_take_the_last_turns_context() {
    let model = Gated::new(|_, _| answer("ok"));
    let mut n = Node::start(model.clone()).await;
    let sid = n.create(ApprovalMode::Auto).await;
    n.send_json(
        json!({"type": "session.prompt", "req_id": "p1", "session_id": sid,
        "text": "hello", "context": "CTX-ONLY-FOR-THE-FIRST-TURN"}),
    );
    assert_eq!(n.ack("p1").await, Ok(()));
    model.entered.notified().await;
    n.prompt(sid, "p2", "and then");
    assert_eq!(n.ack("p2").await, Ok(()));
    model.gate.notify_one();
    n.ended(2).await;
    let reqs = model.requests();
    assert_eq!(holding(&reqs[0], "CTX-ONLY-FOR-THE-FIRST-TURN"), 1);
    assert_eq!(holding(&reqs[1], "and then"), 1);
    assert_eq!(holding(&reqs[1], "CTX-ONLY-FOR-THE-FIRST-TURN"), 0);
}

/// Review ST-5: a message that names another agent (the picker, or an
/// @mention the master resolved) while a turn of this one runs is
/// refused with the old reason and why; one that names none waits, as
/// does one naming the running turn's own agent.
#[tokio::test]
async fn a_message_naming_another_agent_is_refused() {
    let model = Gated::new(|_, _| answer("ok"));
    let mut n = Node::start(model.clone()).await;
    let sid = n.create(ApprovalMode::Auto).await;
    n.send_json(
        json!({"type": "session.prompt", "req_id": "p1", "session_id": sid,
        "text": "work", "agent": agent("coder")}),
    );
    assert_eq!(n.ack("p1").await, Ok(()));
    model.entered.notified().await;
    n.send_json(
        json!({"type": "session.prompt", "req_id": "p2", "session_id": sid,
        "text": "@writer draft it", "agent": agent("writer"), "agent_named": true}),
    );
    let err = n.ack("p2").await.unwrap_err();
    assert!(err.starts_with("a turn is already running"), "{err}");
    assert!(
        err.contains("pick the agent for your next message"),
        "{err}"
    );
    n.send_json(
        json!({"type": "session.prompt", "req_id": "p3", "session_id": sid,
        "text": "@coder also this", "agent": agent("coder"), "agent_named": true}),
    );
    assert_eq!(n.ack("p3").await, Ok(()), "the running turn's own agent");
    n.send_json(
        json!({"type": "session.prompt", "req_id": "p4", "session_id": sid,
        "text": "plain words", "agent": agent("general")}),
    );
    assert_eq!(n.ack("p4").await, Ok(()), "named nobody: it waits");
    assert_eq!(queued(&n.log(sid)).len(), 2);
    model.gate.notify_one();
    n.ended(2).await;
}

/// Review ST-6: a sub-agent's chat takes no message while it works; its
/// parent's turn reads its answer.
#[tokio::test]
async fn a_sub_agents_chat_takes_no_message_while_it_works() {
    let model = Gated::with(
        |n, _| match n {
            0 => call("delegate", json!({"agent": "scout", "task": "look around"})),
            _ => answer("done"),
        },
        1,
        None,
    );
    let mut n = Node::start(model.clone()).await;
    let sid = n.create(ApprovalMode::Auto).await;
    let mut parent = agent("coder");
    // A scout that only reads works in this folder (no copy of its own).
    let mut scout = agent("scout");
    scout["tools"] = json!(["read"]);
    parent["delegates"] = json!([scout]);
    n.send_json(
        json!({"type": "session.prompt", "req_id": "p1", "session_id": sid,
        "text": "delegate it", "agent": parent}),
    );
    assert_eq!(n.ack("p1").await, Ok(()));
    model.entered.notified().await;
    let child: SessionId = loop {
        if let Some(c) = n.seen.iter().find_map(|e| match e.typed() {
            Ok(EventKind::SessionCreated(c)) if c.parent_session.is_some() => Some(e.session_id),
            _ => None,
        }) {
            break c;
        }
        n.next().await;
    };
    n.prompt(child, "q", "hello child");
    let err = n.ack("q").await.unwrap_err();
    assert!(err.starts_with("a turn is already running"), "{err}");
    assert!(err.contains("sub-agent"), "{err}");
    model.gate.notify_one();
    n.ended(1).await;
}

/// Review ST-7: parallel answers take no message while they run.
#[tokio::test]
async fn parallel_answers_take_no_message_while_they_run() {
    let model = Gated::new(|_, _| answer("ok"));
    let mut n = Node::start_as(model.clone(), Profile::Chat).await;
    let sid = SessionId::new();
    n.send(MasterToNode::Create {
        exec_mode: None,
        no_callbacks: false,
        profile: Some(Profile::Chat),
        incognito: false,
        agent: None,
        req_id: "c".into(),
        session_id: sid,
        title: "t".into(),
        cwd: String::new(),
        model: None,
        approval_mode: None,
    });
    assert_eq!(n.ack("c").await, Ok(()));
    n.send_json(
        json!({"type": "session.prompt", "req_id": "p1", "session_id": sid,
        "text": "compare", "compare": ["mock-a", "mock-b"]}),
    );
    assert_eq!(n.ack("p1").await, Ok(()));
    model.entered.notified().await;
    n.prompt(sid, "p2", "and this");
    let err = n.ack("p2").await.unwrap_err();
    assert!(err.starts_with("a turn is already running"), "{err}");
    assert!(err.contains("parallel answers"), "{err}");
    model.gate.notify_one();
    n.ended(1).await;
}
