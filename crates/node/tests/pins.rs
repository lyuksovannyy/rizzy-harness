//! A person's pin of a message on a real node (docs/pins.md): one
//! `message_pinned` event in the same log, written by the session's one
//! writer with contiguous seqs, also while a turn runs; the rule is
//! `harness_core::pins::check`; the model is sent nothing about it; the pins
//! are there after a node restart; a node that does not announce `pin`
//! cannot be sent one by a master that checks first.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_agent_native::provider::ChatRequest;
use harness_agent_native::{NativeBackend, NativeConfig, history_to_messages, mock::Mock};
use harness_core::event::*;
use harness_core::policy::ApprovalMode;
use harness_core::view::SessionView;
use harness_core::{RawEvent, SessionId};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster, PromptMsg};
use harness_store::{Durability, Store};

type Requests = Arc<Mutex<Vec<ChatRequest>>>;

/// The scripted agent, keeping every request it is sent.
struct Recording {
    inner: Mock,
    seen: Requests,
}

#[async_trait::async_trait]
impl harness_agent_native::provider::Provider for Recording {
    fn name(&self) -> &'static str {
        "mock"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        d: tokio::sync::mpsc::UnboundedSender<String>,
    ) -> Result<
        harness_agent_native::provider::Completion,
        harness_agent_native::provider::ProviderError,
    > {
        self.seen.lock().unwrap().push(req.clone());
        self.inner.complete(req, d).await
    }
}

struct H {
    node: Option<NodeRuntime>,
    link: NodeLink,
    requests: Requests,
    dir: tempfile::TempDir,
    ws: std::path::PathBuf,
    sid: SessionId,
    n: u32,
}

fn config(dir: &std::path::Path, requests: &Requests) -> NodeConfig {
    NodeConfig {
        node_id: "local".into(),
        hostname: "test".into(),
        policy: NodePolicy {
            workspace_roots: vec![dir.into()],
            max_approval_mode: ApprovalMode::Auto,
            // Not about skills: the developer's own stay out.
            skills: harness_node::SkillsPolicy {
                roots: vec![],
                ..Default::default()
            },
            ..Default::default()
        },
        store: Store::open(&dir.join("node.db"), Durability::Full).unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Recording {
                inner: Mock {
                    delay: Duration::from_millis(5),
                },
                seen: requests.clone(),
            }),
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: harness_core::SecretSet::new(vec![]),
        profile: Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: Some(dir.join("checkpoints")),
        git_sandbox: None,
        artifacts: None,
    }
}

async fn attach(node: &NodeRuntime) -> NodeLink {
    let mut link = node.attach().await;
    match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { capabilities, .. } => {
            assert!(
                capabilities.iter().any(|c| c == harness_protocol::CAP_PIN),
                "{capabilities:?}"
            );
        }
        other => panic!("expected hello, got {other:?}"),
    }
    link
}

impl H {
    async fn new() -> H {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir(&ws).unwrap();
        let requests: Requests = Arc::default();
        let node = NodeRuntime::start(config(dir.path(), &requests))
            .await
            .unwrap();
        let link = attach(&node).await;
        let mut h = H {
            node: Some(node),
            link,
            requests,
            dir,
            ws,
            sid: SessionId::new(),
            n: 0,
        };
        h.link
            .to_node
            .try_send(MasterToNode::Create {
                exec_mode: None,
                no_callbacks: false,
                profile: None,
                incognito: false,
                agent: None,
                req_id: "c".into(),
                session_id: h.sid,
                title: "t".into(),
                cwd: h.ws.display().to_string(),
                model: None,
                approval_mode: Some(ApprovalMode::Auto),
            })
            .unwrap();
        h.until(|k| matches!(k, EventKind::SessionCreated(_))).await;
        h
    }

    async fn restart(&mut self) {
        drop(self.node.take());
        tokio::time::sleep(Duration::from_millis(200)).await;
        let node = NodeRuntime::start(config(self.dir.path(), &self.requests))
            .await
            .unwrap();
        self.link = attach(&node).await;
        self.node = Some(node);
    }

    async fn recv(&mut self) -> NodeToMaster {
        tokio::time::timeout(Duration::from_secs(60), self.link.from_node.recv())
            .await
            .expect("timed out")
            .expect("link closed")
    }

    async fn until(&mut self, pred: impl Fn(&EventKind) -> bool) {
        loop {
            match self.recv().await {
                NodeToMaster::Event { event } if event.session_id == self.sid => {
                    if pred(&event.typed().unwrap()) {
                        return;
                    }
                }
                NodeToMaster::Ack { ok, error, .. } => assert!(ok, "nack: {error:?}"),
                _ => {}
            }
        }
    }

    fn log(&self) -> Vec<RawEvent> {
        self.node
            .as_ref()
            .unwrap()
            .store()
            .events_after(self.sid, 0, 100_000)
            .unwrap()
    }

    fn send(&mut self, m: MasterToNode) -> String {
        self.n += 1;
        let req = format!("q{}", self.n);
        let m = match m {
            MasterToNode::Prompt(p) => MasterToNode::Prompt(PromptMsg {
                req_id: req.clone(),
                ..p
            }),
            MasterToNode::Pin {
                session_id,
                message_seq,
                pinned,
                ..
            } => MasterToNode::Pin {
                req_id: req.clone(),
                session_id,
                message_seq,
                pinned,
            },
            MasterToNode::Rewind {
                session_id,
                message_seq,
                files,
                force,
                ..
            } => MasterToNode::Rewind {
                req_id: req.clone(),
                session_id,
                message_seq,
                files,
                force,
            },
            other => other,
        };
        self.link.to_node.try_send(m).unwrap();
        req
    }

    /// One turn with `text`; waits for it to end.
    async fn turn(&mut self, text: &str) {
        let mark = self.log().iter().filter(|e| e.kind == "turn_ended").count();
        self.send(MasterToNode::Prompt(PromptMsg::test(self.sid, text)));
        loop {
            self.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
            if self.log().iter().filter(|e| e.kind == "turn_ended").count() > mark {
                return;
            }
        }
    }

    async fn ack(&mut self, req: &str) -> Result<(), String> {
        loop {
            if let NodeToMaster::Ack {
                req_id, ok, error, ..
            } = self.recv().await
                && req_id == req
            {
                return if ok {
                    Ok(())
                } else {
                    Err(error.unwrap_or_default())
                };
            }
        }
    }

    async fn pin(&mut self, message_seq: u64, pinned: bool) -> Result<(), String> {
        let req = self.send(MasterToNode::Pin {
            req_id: String::new(),
            session_id: self.sid,
            message_seq,
            pinned,
        });
        self.ack(&req).await
    }

    /// The seqs of the assistant's answers, in order.
    fn answers(&self) -> Vec<u64> {
        self.log()
            .iter()
            .filter(
                |e| matches!(e.typed(), Ok(EventKind::AssistantMessage(m)) if !m.text.is_empty()),
            )
            .map(|e| e.seq)
            .collect()
    }

    /// The seqs of the person's messages that opened turns, in order.
    fn messages(&self) -> Vec<u64> {
        self.log()
            .iter()
            .filter(|e| matches!(e.typed(), Ok(EventKind::UserMessage(m)) if !m.queued))
            .map(|e| e.seq)
            .collect()
    }

    fn pins(&self) -> Vec<u64> {
        harness_core::pins::pins(&self.log())
            .iter()
            .map(|p| p.seq)
            .collect()
    }

    /// Everything the model was sent in the requests since `mark`, as one
    /// string (the messages as the provider layer holds them).
    fn sent_since(&self, mark: usize) -> String {
        self.requests.lock().unwrap()[mark..]
            .iter()
            .map(|r| format!("{:?}", r.messages))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn mark(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

fn contiguous(log: &[RawEvent]) {
    for (i, e) in log.iter().enumerate() {
        assert_eq!(e.seq, i as u64 + 1, "seq stays contiguous");
    }
}

#[tokio::test]
async fn a_pin_is_one_event_in_the_same_log_and_an_unpin_another() {
    let mut h = H::new().await;
    h.turn("run: echo one > a.txt").await;
    let a = h.answers();
    let before = h.log();
    h.pin(a[0], true).await.unwrap();
    let after = h.log();
    contiguous(&after);
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(&after[..before.len()], &before[..]);
    let last = after.last().unwrap();
    assert_eq!(last.kind, "message_pinned");
    assert_eq!(last.payload["message_seq"], a[0]);
    assert_eq!(h.pins(), vec![a[0]]);
    h.pin(a[0], false).await.unwrap();
    assert!(h.pins().is_empty());
    contiguous(&h.log());
}

#[tokio::test]
async fn what_the_rule_refuses_is_refused_and_writes_nothing() {
    let mut h = H::new().await;
    h.turn("run: echo one > a.txt").await;
    let a = h.answers();
    let n = h.log().len();
    // Not a message of this chat; unpinning what is not pinned.
    let e = h.pin(9999, true).await.unwrap_err();
    assert!(e.contains("message of this chat"), "{e}");
    let e = h.pin(a[0], false).await.unwrap_err();
    assert!(e.contains("not pinned"), "{e}");
    assert_eq!(h.log().len(), n);
    h.pin(a[0], true).await.unwrap();
    let e = h.pin(a[0], true).await.unwrap_err();
    assert!(e.contains("already pinned"), "{e}");
    assert_eq!(h.log().len(), n + 1);
}

#[tokio::test]
async fn the_fifty_first_pin_is_refused() {
    let mut h = H::new().await;
    // Many short turns: each has a message of the person's to pin.
    for i in 0..51 {
        h.turn(&format!("note {i}")).await;
    }
    let m = h.messages();
    assert!(m.len() >= 51);
    for s in &m[..50] {
        h.pin(*s, true).await.unwrap();
    }
    let e = h.pin(m[50], true).await.unwrap_err();
    assert!(e.contains("50 pinned messages"), "{e}");
    assert_eq!(h.pins().len(), 50);
}

/// A pin changes nothing the model reads, so a running turn does not stop
/// it; the log stays valid (contiguous seqs, one writer) and the turn
/// finishes as it would have.
#[tokio::test]
async fn a_pin_while_a_turn_runs_keeps_the_log_valid() {
    let mut h = H::new().await;
    h.turn("run: echo one > a.txt").await;
    let a = h.answers();
    h.send(MasterToNode::Prompt(PromptMsg::test(h.sid, "run: sleep 2")));
    h.until(|k| matches!(k, EventKind::ToolCallStarted(_)))
        .await;
    h.pin(a[0], true).await.unwrap();
    h.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
    let log = h.log();
    contiguous(&log);
    assert!(SessionView::fold(&log).is_ok());
    assert_eq!(h.pins(), vec![a[0]]);
    assert_eq!(log.iter().filter(|e| e.kind == "turn_ended").count(), 2);
}

#[tokio::test]
async fn the_model_is_sent_nothing_about_a_pin() {
    let mut h = H::new().await;
    h.turn("run: echo one > a.txt").await;
    let a = h.answers();
    let without = history_to_messages(&h.log());
    h.pin(a[0], true).await.unwrap();
    assert_eq!(history_to_messages(&h.log()), without);
    let mark = h.mark();
    h.turn("run: echo two > b.txt").await;
    let sent = h.sent_since(mark);
    assert!(
        !sent.contains("message_pinned") && !sent.to_lowercase().contains("pinned"),
        "{sent}"
    );
}

#[tokio::test]
async fn a_pin_survives_a_node_restart() {
    let mut h = H::new().await;
    h.turn("run: echo one > a.txt").await;
    h.turn("run: echo two > b.txt").await;
    let a = h.answers();
    h.pin(a[1], true).await.unwrap();
    let log = h.log();
    h.restart().await;
    let again = h.log();
    assert_eq!(&again[..log.len()], &log[..]);
    assert_eq!(h.pins(), vec![a[1]]);
    // And it is still the writer: the next pin gets the next seq.
    h.pin(a[0], true).await.unwrap();
    contiguous(&h.log());
    assert_eq!(h.pins(), vec![a[0], a[1]]);
}

#[tokio::test]
async fn a_node_announces_the_capability() {
    // `attach` asserts the hello lists `pin`; here the constant is the wire word.
    assert_eq!(harness_protocol::CAP_PIN, "pin");
}

#[tokio::test]
async fn a_rewound_message_cannot_be_pinned() {
    let mut h = H::new().await;
    h.turn("run: echo one > a.txt").await;
    h.turn("run: echo two > b.txt").await;
    let (m, a) = (h.messages(), h.answers());
    let req = h.send(MasterToNode::Rewind {
        req_id: String::new(),
        session_id: h.sid,
        message_seq: m[1],
        files: false,
        force: false,
    });
    h.ack(&req).await.unwrap();
    let e = h.pin(a[1], true).await.unwrap_err();
    assert!(e.contains("rewound"), "{e}");
    h.pin(a[0], true).await.unwrap();
}
