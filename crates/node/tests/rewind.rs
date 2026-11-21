//! A person's rewind of the conversation on a real node (ADOPTION_PLAN
//! X.46, docs/rewind.md, README "Rewind"; plan row B.6's restart test): one
//! `conversation_rewound` event in the same log, nothing deleted; the model
//! is sent the chat without the rewound turns; the files are put back only
//! when asked (and a refusal of that writes nothing); what the agent read
//! in a rewound turn is not read any more; refused while anything of the
//! chat runs, for a message inside a rewound range, and for an undo after
//! something new; the same after a node restart.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_agent_native::provider::ChatRequest;
use harness_agent_native::{NativeBackend, NativeConfig, history_to_messages, mock::Mock};
use harness_core::event::*;
use harness_core::policy::ApprovalMode;
use harness_core::view::SessionView;
use harness_core::{RawEvent, SessionId};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster, PromptMsg, REWIND_REFUSED};
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
                capabilities
                    .iter()
                    .any(|c| c == harness_protocol::CAP_REWIND),
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
            MasterToNode::RewindUndo {
                session_id,
                rewind_seq,
                ..
            } => MasterToNode::RewindUndo {
                req_id: req.clone(),
                session_id,
                rewind_seq,
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

    async fn rewind(&mut self, message_seq: u64, files: bool, force: bool) -> Result<(), String> {
        let req = self.send(MasterToNode::Rewind {
            req_id: String::new(),
            session_id: self.sid,
            message_seq,
            files,
            force,
        });
        self.ack(&req).await
    }

    async fn undo(&mut self, rewind_seq: u64) -> Result<(), String> {
        let req = self.send(MasterToNode::RewindUndo {
            req_id: String::new(),
            session_id: self.sid,
            rewind_seq,
        });
        self.ack(&req).await
    }

    /// The seqs of the person's messages that opened turns, in order.
    fn messages(&self) -> Vec<u64> {
        self.log()
            .iter()
            .filter(|e| matches!(e.typed(), Ok(EventKind::UserMessage(m)) if !m.queued))
            .map(|e| e.seq)
            .collect()
    }

    fn rewinds(&self) -> Vec<(u64, ConversationRewound)> {
        self.log()
            .iter()
            .filter_map(|e| match e.typed() {
                Ok(EventKind::ConversationRewound(r)) => Some((e.seq, r)),
                _ => None,
            })
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

/// The rewind is one event in the same log; nothing is deleted; the model
/// is sent the chat without the rewound turns and, as the files were kept,
/// the harness's line naming them; the files are untouched.
#[tokio::test]
async fn a_rewind_cuts_the_conversation_and_keeps_the_files() {
    let mut h = H::new().await;
    h.turn("run: echo one > a.txt").await;
    h.turn("run: echo two > b.txt").await;
    h.turn("run: echo three > c.txt").await;
    let before = h.log();
    let m = h.messages();
    assert_eq!(m.len(), 3);
    h.rewind(m[1], false, false).await.unwrap();
    let after = h.log();
    contiguous(&after);
    assert!(
        after[..before.len()]
            .iter()
            .zip(&before)
            .all(|(a, b)| a == b),
        "nothing was deleted or changed"
    );
    let rewinds = h.rewinds();
    assert_eq!(rewinds.len(), 1);
    let (seq, r) = &rewinds[0];
    assert_eq!(*seq, before.len() as u64 + 1, "the next seq");
    assert_eq!((r.message_seq, r.files), (m[1], false));
    // The files stay as they are.
    for f in ["a.txt", "b.txt", "c.txt"] {
        assert!(h.ws.join(f).exists(), "{f}");
    }
    // The next turn: the model is sent turn one, not two or three, and the
    // line naming the files the removed turns changed.
    let mark = h.mark();
    h.turn("run: echo four > d.txt").await;
    let sent = h.sent_since(mark);
    assert!(sent.contains("echo one"), "{sent}");
    assert!(
        !sent.contains("echo two") && !sent.contains("echo three"),
        "{sent}"
    );
    assert!(sent.contains("took this conversation back"), "{sent}");
    assert!(sent.contains("b.txt") && sent.contains("c.txt"), "{sent}");
    assert!(
        !sent.contains("| a.txt"),
        "turn one's file is not one of the removed"
    );
    // The line is said once, in the fourth turn's own message: the turn
    // after has none of its own, and sends that message byte for byte.
    let fourth = first_line_message(&h, mark);
    let mark = h.mark();
    h.turn("run: echo five > e.txt").await;
    let sent = h.sent_since(mark);
    assert_eq!(
        first_line_message(&h, mark),
        fourth,
        "the rebuilt message is the one that was sent"
    );
    let reqs = h.requests.lock().unwrap()[mark..].len();
    assert_eq!(
        sent.matches("took this conversation back").count(),
        reqs,
        "once per request"
    );
}

/// The user message carrying the rewind line in the request at `at`.
fn first_line_message(h: &H, at: usize) -> String {
    let r = h.requests.lock().unwrap()[at].clone();
    r.messages
        .iter()
        .find_map(|m| match m {
            harness_agent_native::ChatMsg::User(t) | harness_agent_native::ChatMsg::Opening(t)
                if t.contains("took this conversation back") =>
            {
                Some(t.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no rewind line in request {at}"))
}

/// With the files part the folder goes back to before the earliest rewound
/// turn, as the files rewind does; the chat says it did; no line says the
/// files were kept.
#[tokio::test]
async fn a_rewind_with_files_puts_the_folder_back() {
    let mut h = H::new().await;
    h.turn("run: echo one > a.txt").await;
    h.turn("run: echo two > b.txt").await;
    h.turn("run: echo three > c.txt").await;
    let m = h.messages();
    h.rewind(m[1], true, false).await.unwrap();
    assert!(h.ws.join("a.txt").exists());
    assert!(!h.ws.join("b.txt").exists() && !h.ws.join("c.txt").exists());
    let (_, r) = &h.rewinds()[0];
    assert!(r.files);
    contiguous(&h.log());
    let mark = h.mark();
    h.turn("run: echo four > d.txt").await;
    let sent = h.sent_since(mark);
    assert!(!sent.contains("took this conversation back"), "{sent}");
    assert!(!sent.contains("echo two"), "{sent}");
}

/// A files part that is refused writes no rewind: the folder was edited
/// after the turn, so it is refused without `force`, the log is as it was,
/// and with `force` it goes through.
#[tokio::test]
async fn a_refused_files_part_writes_nothing() {
    let mut h = H::new().await;
    h.turn("run: echo one > a.txt").await;
    let m = h.messages();
    std::fs::write(h.ws.join("a.txt"), "edited by hand\n").unwrap();
    let before = h.log().len();
    let e = h.rewind(m[0], true, false).await.unwrap_err();
    assert!(!e.is_empty());
    assert_eq!(h.log().len(), before, "nothing was appended: {e}");
    assert!(h.rewinds().is_empty());
    assert_eq!(
        std::fs::read_to_string(h.ws.join("a.txt")).unwrap(),
        "edited by hand\n"
    );
    h.rewind(m[0], true, true).await.unwrap();
    assert!(!h.ws.join("a.txt").exists());
    assert_eq!(h.rewinds().len(), 1);
}

#[tokio::test]
async fn what_the_rule_refuses_is_refused_and_writes_nothing() {
    let mut h = H::new().await;
    h.turn("run: echo one > a.txt").await;
    h.turn("run: echo two > b.txt").await;
    let m = h.messages();
    let head = h.log().len();
    // Not a message of the person's; not in the chat.
    let e = h.rewind(1, false, false).await.unwrap_err();
    assert!(e.starts_with(REWIND_REFUSED), "{e}");
    assert!(h.rewind(9999, false, false).await.is_err());
    assert_eq!(h.log().len(), head);
    // A message inside a rewound range, and twice to the same message.
    h.rewind(m[0], false, false).await.unwrap();
    let e = h.rewind(m[1], false, false).await.unwrap_err();
    assert!(e.contains("already rewound"), "{e}");
    let e = h.rewind(m[0], false, false).await.unwrap_err();
    assert!(e.contains("already rewound"), "{e}");
    assert_eq!(h.rewinds().len(), 1);
}

#[tokio::test]
async fn a_rewind_is_refused_while_a_turn_runs() {
    let mut h = H::new().await;
    h.turn("run: echo one > a.txt").await;
    let m = h.messages();
    h.send(MasterToNode::Prompt(PromptMsg::test(h.sid, "run: sleep 3")));
    h.until(|k| matches!(k, EventKind::ToolCallStarted(_)))
        .await;
    let e = h.rewind(m[0], false, false).await.unwrap_err();
    assert!(e.contains("already running"), "{e}");
    assert!(h.rewinds().is_empty());
    h.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
    // And once it has ended, it goes through.
    tokio::time::sleep(Duration::from_millis(100)).await;
    h.rewind(m[0], false, false).await.unwrap();
}

/// Undo: allowed right after, the turns are sent again as they were; not
/// twice; not after a new turn.
#[tokio::test]
async fn an_undo_puts_the_turns_back_until_something_new_is_said() {
    let mut h = H::new().await;
    h.turn("run: echo one > a.txt").await;
    h.turn("run: echo two > b.txt").await;
    let m = h.messages();
    h.rewind(m[1], false, false).await.unwrap();
    let r = h.rewinds()[0].0;
    h.undo(r).await.unwrap();
    contiguous(&h.log());
    let e = h.undo(r).await.unwrap_err();
    assert!(e.contains("already undone"), "{e}");
    let view = SessionView::fold(&h.log()).unwrap();
    assert!(view.rewinds[0].undone);
    let mark = h.mark();
    h.turn("run: echo three > c.txt").await;
    let sent = h.sent_since(mark);
    assert!(
        sent.contains("echo one") && sent.contains("echo two"),
        "{sent}"
    );
    assert!(!sent.contains("took this conversation back"), "{sent}");
    // Rewind again, then say something new: it can no longer be put back.
    let m = h.messages();
    h.rewind(m[1], false, false).await.unwrap();
    let r2 = h.rewinds().last().unwrap().0;
    h.turn("run: echo four > d.txt").await;
    let e = h.undo(r2).await.unwrap_err();
    assert!(e.starts_with(REWIND_REFUSED), "{e}");
    assert!(e.contains("no longer"), "{e}");
    // Not an event of a rewind at all.
    assert!(h.undo(1).await.is_err());
}

/// Rewind, a new turn, and a rewind to a message before the first one:
/// the ranges are a union; the model is sent only the kept turn.
#[tokio::test]
async fn rewinds_nest() {
    let mut h = H::new().await;
    h.turn("run: echo one > a.txt").await;
    h.turn("run: echo two > b.txt").await;
    let m = h.messages();
    h.rewind(m[1], false, false).await.unwrap();
    h.turn("run: echo three > c.txt").await;
    let m = h.messages();
    h.rewind(m[0], false, false).await.unwrap();
    let mark = h.mark();
    h.turn("run: echo four > d.txt").await;
    let sent = h.sent_since(mark);
    for gone in ["echo one", "echo two", "echo three"] {
        assert!(!sent.contains(gone), "{gone}: {sent}");
    }
    assert!(sent.contains("echo four"));
    contiguous(&h.log());
}

/// What a file read in a rewound turn does for an edit: nothing. The next
/// turn's edit without a read is refused as unread; without the rewind the
/// same edit goes through.
#[tokio::test]
async fn a_file_read_in_a_rewound_turn_is_not_read() {
    for rewound in [true, false] {
        let mut h = H::new().await;
        std::fs::write(h.ws.join("f.txt"), "one\ntwo\n").unwrap();
        h.turn(r#"script:[{"tool":"read","input":{"path":"f.txt"}}]"#)
            .await;
        if rewound {
            let m = h.messages();
            h.rewind(m[0], false, false).await.unwrap();
        }
        h.turn(
            r#"script:[{"tool":"edit","input":{"path":"f.txt","old_string":"one","new_string":"1"}}]"#,
        )
        .await;
        let done = std::fs::read_to_string(h.ws.join("f.txt")).unwrap();
        let result = h
            .log()
            .into_iter()
            .rev()
            .find_map(|e| match e.typed() {
                Ok(EventKind::ToolCallFinished(f)) => Some(f),
                _ => None,
            })
            .unwrap();
        if rewound {
            assert!(!result.ok, "an unread file was edited: {}", result.output);
            assert_eq!(done, "one\ntwo\n");
        } else {
            assert!(result.ok, "{}", result.output);
            assert_eq!(done, "1\ntwo\n");
        }
    }
}

/// B.6 (D13): a rewind, the node restarts, the chat is opened: the view
/// and the model's history are what they were, and the next turn is sent
/// the chat without the rewound turn.
#[tokio::test]
async fn a_rewind_survives_a_node_restart() {
    let mut h = H::new().await;
    h.turn("run: echo one > a.txt").await;
    h.turn("run: echo two > b.txt").await;
    let m = h.messages();
    h.rewind(m[1], false, false).await.unwrap();
    let log = h.log();
    let view = serde_json::to_value(SessionView::fold(&log).unwrap()).unwrap();
    let model = history_to_messages(&log);
    h.restart().await;
    let again = h.log();
    assert_eq!(&again[..log.len()], &log[..], "the log is as it was");
    let fold = SessionView::fold(&again).unwrap();
    assert_eq!(fold.rewinds, SessionView::fold(&log).unwrap().rewinds);
    let now = serde_json::to_value(&fold).unwrap();
    assert_eq!(
        now["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i.get("rewound").is_some())
            .count(),
        view["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i.get("rewound").is_some())
            .count()
    );
    assert_eq!(&history_to_messages(&again)[..model.len()], &model[..]);
    // An undo and a new rewind work on the restarted node as before.
    let mark = h.mark();
    h.turn("run: echo three > c.txt").await;
    let sent = h.sent_since(mark);
    assert!(
        sent.contains("echo one") && !sent.contains("echo two"),
        "{sent}"
    );
    assert!(h.rewinds().len() == 1);
}
