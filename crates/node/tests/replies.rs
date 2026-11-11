//! Answers to ui:choices and ui:form blocks: the node checks each against
//! its own log while it holds the turn slot (rich_plan §7.3).

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::SessionId;
use harness_core::event::*;
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster};
use harness_store::Store;
use serde_json::{Value, json};

struct Chat {
    _dir: tempfile::TempDir,
    _node: NodeRuntime,
    link: NodeLink,
    sid: SessionId,
    seen: Vec<EventKind>,
}

impl Chat {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let node = NodeRuntime::start(NodeConfig {
            node_id: "master".into(),
            hostname: "test".into(),
            policy: NodePolicy {
                workspace_roots: vec![dir.path().into()],
                ..Default::default()
            },
            store: Store::open_in_memory().unwrap(),
            backend: Arc::new(NativeBackend::new(
                Arc::new(Mock {
                    delay: Duration::from_millis(2),
                }),
                vec![],
                NativeConfig::default(),
            )),
            default_model: "mock".into(),
            secrets: Default::default(),
            profile: Profile::Chat,
            incognito_ttl: None,
            checkpoints_dir: None,
            git_sandbox: None,
            artifacts: None,
        })
        .await
        .unwrap();
        let link = node.attach().await;
        let mut chat = Self {
            _dir: dir,
            _node: node,
            link,
            sid: SessionId::new(),
            seen: vec![],
        };
        chat.send(MasterToNode::Create {
            req_id: "c".into(),
            session_id: chat.sid,
            title: "t".into(),
            cwd: String::new(),
            model: None,
            approval_mode: None,
            agent: None,
            profile: Some(Profile::Chat),
            incognito: false,
        });
        assert_eq!(chat.ack("c").await, Ok(()));
        chat
    }

    fn send(&self, m: MasterToNode) {
        self.link.to_node.try_send(m).unwrap();
    }

    fn prompt(&self, req_id: &str, text: &str, reply: Option<FormReply>) {
        self.send(MasterToNode::Prompt {
            req_id: req_id.into(),
            session_id: self.sid,
            text: text.into(),
            agent: None,
            attachments: vec![],
            compare: vec![],
            savings: None,
            context: None,
            reply,
            images: vec![],
        });
    }

    /// The answer to `req_id`, keeping the events that arrive meanwhile.
    async fn ack(&mut self, req_id: &str) -> Result<(), String> {
        loop {
            match self.next().await {
                NodeToMaster::Ack {
                    req_id: r,
                    ok,
                    error,
                } if r == req_id => return if ok { Ok(()) } else { Err(error.unwrap()) },
                _ => {}
            }
        }
    }

    async fn next(&mut self) -> NodeToMaster {
        let msg = tokio::time::timeout(Duration::from_secs(10), self.link.from_node.recv())
            .await
            .expect("timed out")
            .expect("link closed");
        if let NodeToMaster::Event { event } = &msg {
            self.seen.push(event.typed().unwrap());
        }
        msg
    }

    /// Events of the turn just prompted, until it ends. Its start and message
    /// are written before the prompt is acked, so they may already be here.
    async fn turn(&mut self) -> Vec<EventKind> {
        let from = self
            .seen
            .iter()
            .rposition(|k| matches!(k, EventKind::TurnStarted(_)))
            .filter(|&i| {
                !self.seen[i..]
                    .iter()
                    .any(|k| matches!(k, EventKind::TurnEnded(_)))
            })
            .unwrap_or(self.seen.len());
        while !matches!(self.seen.last(), Some(EventKind::TurnEnded(_))) || self.seen.len() == from
        {
            self.next().await;
        }
        self.seen[from..].to_vec()
    }

    /// Ask the canned colour question; returns its message id.
    async fn ask(&mut self) -> String {
        self.prompt("ask", "ui: choices", None);
        assert_eq!(self.ack("ask").await, Ok(()));
        let events = self.turn().await;
        events
            .iter()
            .find_map(|k| match k {
                EventKind::AssistantMessage(m) if !m.blocks.is_empty() => {
                    Some(m.message_id.clone())
                }
                _ => None,
            })
            .expect("the question")
    }
}

fn reply(message_id: &str, values: Value) -> FormReply {
    FormReply {
        message_id: message_id.into(),
        block_id: "colour".into(),
        values: values.as_object().unwrap().clone(),
    }
}

fn user_messages(events: &[EventKind]) -> Vec<UserMessage> {
    events
        .iter()
        .filter_map(|k| match k {
            EventKind::UserMessage(m) => Some(m.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_valid_answer_is_recorded_as_its_canonical_text_and_runs() {
    let mut c = Chat::new().await;
    let asked = c.ask().await;
    let r = reply(&asked, json!({"selected": ["blue"]}));
    c.prompt("r1", "  thanks ", Some(r.clone()));
    assert_eq!(c.ack("r1").await, Ok(()));
    let events = c.turn().await;
    let users = user_messages(&events);
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].text, "Choice answer · colour: blue\n\nthanks");
    assert_eq!(users[0].reply, Some(r.clone()));
    let echoed = events.iter().any(|k| {
        matches!(k, EventKind::AssistantMessage(m) if m.text.starts_with("You said: Choice answer · colour: blue"))
    });
    assert!(echoed, "the model saw the canonical answer: {events:?}");

    // Answered: a second answer to the same question is stale.
    c.prompt("r2", "", Some(reply(&asked, json!({"selected": ["red"]}))));
    assert_eq!(c.ack("r2").await, Err(harness_core::ui::NOT_OPEN.into()));
}

#[tokio::test]
async fn prose_after_a_question_closes_it() {
    let mut c = Chat::new().await;
    let asked = c.ask().await;
    c.prompt("p", "red, actually", None);
    assert_eq!(c.ack("p").await, Ok(()));
    c.turn().await;
    c.prompt("r", "", Some(reply(&asked, json!({"selected": ["red"]}))));
    let err = c.ack("r").await.unwrap_err();
    assert!(err.contains("form is no longer open"), "{err}");
}

#[tokio::test]
async fn an_answer_waits_for_the_running_turn() {
    let mut c = Chat::new().await;
    let asked = c.ask().await;
    c.prompt("slow", "slow: 200", None);
    assert_eq!(c.ack("slow").await, Ok(()));
    c.prompt("r", "", Some(reply(&asked, json!({"selected": ["red"]}))));
    assert_eq!(c.ack("r").await, Err("a turn is already running".into()));
}

#[tokio::test]
async fn a_bad_answer_records_nothing_and_frees_the_slot() {
    let mut c = Chat::new().await;
    let asked = c.ask().await;
    let before = c.seen.len();
    for (i, (r, err)) in [
        (
            reply(&asked, json!({"selected": ["pink"]})),
            "selected: not one of the options",
        ),
        (
            reply("no-such-message", json!({"selected": ["red"]})),
            "no such message",
        ),
        (
            FormReply {
                block_id: "other".into(),
                ..reply(&asked, json!({}))
            },
            "no such question or form in that message",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("bad{i}");
        c.prompt(&id, "", Some(r));
        assert_eq!(c.ack(&id).await, Err(err.into()));
    }
    // Nothing was appended, and the question is still open.
    assert_eq!(c.seen.len(), before);
    c.prompt(
        "ok",
        "",
        Some(reply(&asked, json!({"selected": ["green"]}))),
    );
    assert_eq!(c.ack("ok").await, Ok(()));
    let users = user_messages(&c.turn().await);
    assert_eq!(users[0].text, "Choice answer · colour: green");
    // And a plain prompt after it runs as usual.
    c.prompt("plain", "hello", None);
    assert_eq!(c.ack("plain").await, Ok(()));
    assert_eq!(user_messages(&c.turn().await)[0].text, "hello");
}
