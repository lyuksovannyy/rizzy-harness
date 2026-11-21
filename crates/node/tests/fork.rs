//! A person's fork of a chat on a real node (ADOPTION_PLAN C.2,
//! docs/fork.md): a new chat whose log is the parent's events up to a
//! completed turn at the same seqs, then the divider, then its own. The
//! parent is only read: it is not changed, whatever it is doing. One
//! transaction writes the fork, so a restart right after it loads a whole
//! chat. Refused for a turn that has not ended, one that is not there, and
//! (on the master's side) for anyone but the owner.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_agent_native::provider::ChatRequest;
use harness_agent_native::{NativeBackend, NativeConfig, history_to_messages, mock::Mock};
use harness_core::event::*;
use harness_core::policy::{ApprovalMode, Decision};
use harness_core::view::{Item, SessionView};
use harness_core::{RawEvent, SessionId};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{FORK_REFUSED, MasterToNode, NodeToMaster, PromptMsg};
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
                capabilities.iter().any(|c| c == harness_protocol::CAP_FORK),
                "{capabilities:?}"
            );
        }
        other => panic!("expected hello, got {other:?}"),
    }
    link
}

/// What a fork command answered, and every event the node sent for the new
/// chat before it.
struct Forked {
    sid: SessionId,
    result: Result<(), String>,
    events: Vec<RawEvent>,
}

impl H {
    async fn new() -> H {
        H::with_mode(ApprovalMode::Auto).await
    }

    async fn with_mode(mode: ApprovalMode) -> H {
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
                title: "Parent".into(),
                cwd: h.ws.display().to_string(),
                model: None,
                approval_mode: Some(mode),
            })
            .unwrap();
        let sid = h.sid;
        h.until(sid, |k| matches!(k, EventKind::SessionCreated(_)))
            .await;
        h
    }

    fn store(&self) -> &Store {
        self.node.as_ref().unwrap().store()
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

    async fn until(&mut self, sid: SessionId, pred: impl Fn(&EventKind) -> bool) {
        loop {
            match self.recv().await {
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

    fn log_of(&self, sid: SessionId) -> Vec<RawEvent> {
        self.store().events_after(sid, 0, 100_000).unwrap()
    }

    fn log(&self) -> Vec<RawEvent> {
        self.log_of(self.sid)
    }

    fn prompt(&mut self, sid: SessionId, text: &str) {
        self.n += 1;
        self.link
            .to_node
            .try_send(MasterToNode::Prompt(PromptMsg {
                req_id: format!("p{}", self.n),
                ..PromptMsg::test(sid, text)
            }))
            .unwrap();
    }

    /// One turn of chat `sid` with `text`; waits for it to end.
    async fn turn_of(&mut self, sid: SessionId, text: &str) {
        let mark = self
            .log_of(sid)
            .iter()
            .filter(|e| e.kind == "turn_ended")
            .count();
        self.prompt(sid, text);
        loop {
            self.until(sid, |k| matches!(k, EventKind::TurnEnded(_)))
                .await;
            if self
                .log_of(sid)
                .iter()
                .filter(|e| e.kind == "turn_ended")
                .count()
                > mark
            {
                return;
            }
        }
    }

    async fn turn(&mut self, text: &str) {
        let sid = self.sid;
        self.turn_of(sid, text).await;
    }

    /// One turn of `sid` that answers every approval card with `decision`;
    /// the number of cards it raised.
    async fn cards_of(&mut self, sid: SessionId, text: &str, decision: Decision) -> usize {
        self.prompt(sid, text);
        let mut cards = 0;
        loop {
            match self.recv().await {
                NodeToMaster::Event { event } if event.session_id == sid => {
                    match event.typed().unwrap() {
                        EventKind::ApprovalRequested(a) => {
                            cards += 1;
                            self.n += 1;
                            let req_id = format!("a{}", self.n);
                            self.link
                                .to_node
                                .try_send(MasterToNode::ApprovalRespond {
                                    req_id,
                                    session_id: sid,
                                    approval_id: a.approval_id,
                                    decision,
                                    note: None,
                                })
                                .unwrap();
                        }
                        EventKind::TurnEnded(_) => return cards,
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    /// The ids of the turns of `log` that ended, in order.
    fn ended(log: &[RawEvent]) -> Vec<String> {
        log.iter()
            .filter_map(|e| match e.typed() {
                Ok(EventKind::TurnEnded(t)) => Some(t.turn_id),
                _ => None,
            })
            .collect()
    }

    /// Fork `parent` at the end of `turn_id`; every event the node sends
    /// for the new chat before the ack is kept.
    async fn fork(&mut self, parent: SessionId, turn_id: &str) -> Forked {
        let sid = SessionId::new();
        self.n += 1;
        let req = format!("f{}", self.n);
        let title = format!("{} (fork)", "Parent");
        self.link
            .to_node
            .try_send(MasterToNode::Fork {
                req_id: req.clone(),
                session_id: sid,
                parent,
                turn_id: turn_id.into(),
                title,
            })
            .unwrap();
        let mut events = Vec::new();
        loop {
            match self.recv().await {
                NodeToMaster::Event { event } if event.session_id == sid => events.push(event),
                NodeToMaster::Ack {
                    req_id, ok, error, ..
                } if req_id == req => {
                    return Forked {
                        sid,
                        result: if ok {
                            Ok(())
                        } else {
                            Err(error.unwrap_or_default())
                        },
                        events,
                    };
                }
                _ => {}
            }
        }
    }

    fn mark(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    fn sent_since(&self, mark: usize) -> String {
        self.requests.lock().unwrap()[mark..]
            .iter()
            .map(|r| format!("{:?}", r.messages))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn contiguous(log: &[RawEvent]) {
    for (i, e) in log.iter().enumerate() {
        assert_eq!(e.seq, i as u64 + 1, "seq stays contiguous");
    }
}

fn created(e: &RawEvent) -> SessionCreated {
    match e.typed() {
        Ok(EventKind::SessionCreated(c)) => c,
        other => panic!("not a creation: {other:?}"),
    }
}

/// The fork's events are the parent's `2..=T` as they were, at the same
/// seqs under the new id; its first is the parent's creation with the
/// fork's title and `forked`; the divider follows; the parent's log is
/// as it was; what the node sent is what the store holds; the fork's
/// usage is zero and its copied checkpoints say so.
#[tokio::test]
async fn a_fork_has_the_parents_events_at_the_same_seqs_and_the_parent_is_untouched() {
    let mut h = H::new().await;
    h.turn("run: echo one > a.txt").await;
    h.turn("hello two").await;
    h.turn("hello three").await;
    let parent = h.log();
    let turns = H::ended(&parent);
    assert_eq!(turns.len(), 3);
    let end = parent
        .iter()
        .find(|e| matches!(e.typed(), Ok(EventKind::TurnEnded(t)) if t.turn_id == turns[1]))
        .unwrap()
        .seq;
    let f = h.fork(h.sid, &turns[1]).await;
    assert_eq!(f.result, Ok(()));
    let log = h.log_of(f.sid);
    contiguous(&log);
    assert_eq!(log.len() as u64, end + 1, "the copy and the divider");
    assert_eq!(f.events, log, "what was sent is what is stored");
    for i in 1..end as usize {
        assert_eq!(
            (&log[i].kind, &log[i].payload, log[i].ts_ms),
            (&parent[i].kind, &parent[i].payload, parent[i].ts_ms),
            "event {} is the parent's",
            i + 1
        );
        assert_eq!(log[i].session_id, f.sid);
    }
    let c = created(&log[0]);
    assert_eq!(c.title, "Parent (fork)");
    assert_eq!(c.cwd, created(&parent[0]).cwd, "they share the folder");
    assert_eq!(c.forked.unwrap().parent, h.sid);
    assert_eq!(c.forked.unwrap().through_seq, end);
    assert_eq!(c.parent_session, None);
    let Ok(EventKind::SessionForked(d)) = log[end as usize].typed() else {
        panic!("{:?}", log[end as usize]);
    };
    assert_eq!(
        (d.parent, d.through_seq, d.turn_id),
        (h.sid, end, turns[1].clone())
    );
    // The parent is as it was (and has no event of the fork's).
    assert_eq!(h.log(), parent);
    // The fork's view: nothing of the parent's cost, the divider after the
    // cut, the parent's checkpoint marked as not restorable here.
    let v = SessionView::fold(&log).unwrap();
    assert_eq!(v.usage.input_tokens, 0);
    assert!(v.running_turn.is_none());
    assert!(matches!(v.items.last(), Some(Item::Forked { .. })));
    assert!(v.checkpoints[&turns[0]].inherited);
    let pv = SessionView::fold(&parent).unwrap();
    assert!(pv.usage.input_tokens > 0, "the parent still holds its own");
    // The fork's row is the node's own session: it takes a turn.
    h.turn_of(f.sid, "hello fork").await;
    let log = h.log_of(f.sid);
    let v = SessionView::fold(&log).unwrap();
    assert!(v.usage.input_tokens > 0, "its own turn is counted");
}

/// What the model is sent for the fork is what the parent had at the cut:
/// the rebuild of the fork's log equals the rebuild of the parent's up to
/// the cut, and the fork's first request holds it with the new message.
#[tokio::test]
async fn the_model_is_sent_what_the_parent_was_at_the_cut_and_the_new_message() {
    let mut h = H::new().await;
    h.turn("hello alpha").await;
    h.turn("hello beta").await;
    h.turn("hello gamma").await;
    let parent = h.log();
    let turns = H::ended(&parent);
    let f = h.fork(h.sid, &turns[1]).await;
    f.result.clone().unwrap();
    let end = created(&h.log_of(f.sid)[0]).forked.unwrap().through_seq as usize;
    let at_cut = history_to_messages(&parent[..end]);
    let fork_log = h.log_of(f.sid);
    assert_eq!(
        history_to_messages(&fork_log),
        at_cut,
        "byte for byte, divider included"
    );
    let mark = h.mark();
    h.turn_of(f.sid, "hello delta").await;
    let sent = h.requests.lock().unwrap()[mark].clone();
    assert!(
        sent.messages
            .windows(at_cut.len())
            .any(|w| w == &at_cut[..]),
        "the first request holds the parent's conversation as it was"
    );
    let all = h.sent_since(mark);
    assert!(all.contains("hello alpha") && all.contains("hello beta"));
    assert!(
        !all.contains("hello gamma"),
        "the parent's later turn is not in it"
    );
    assert!(all.contains("hello delta"));
}

/// The parent keeps running during the fork: its later events never reach
/// the fork, which ends at the cut and the divider.
#[tokio::test]
async fn the_parent_keeps_running_and_its_later_events_do_not_appear() {
    let mut h = H::new().await;
    h.turn("hello one").await;
    let turns = H::ended(&h.log());
    // A second turn that takes a while.
    h.prompt(h.sid, "run: sleep 2");
    let sid = h.sid;
    h.until(sid, |k| matches!(k, EventKind::ToolCallStarted(_)))
        .await;
    let f = h.fork(h.sid, &turns[0]).await;
    f.result.clone().unwrap();
    let before = h.log_of(f.sid);
    // A turn that has not ended is not a cut.
    let running = h
        .log()
        .iter()
        .rev()
        .find_map(|e| match e.typed() {
            Ok(EventKind::TurnStarted(t)) => Some(t.turn_id),
            _ => None,
        })
        .unwrap();
    let refused = h.fork(h.sid, &running).await;
    let why = refused.result.clone().unwrap_err();
    assert!(
        why.starts_with(FORK_REFUSED) && why.contains("still running"),
        "{why}"
    );
    assert!(refused.events.is_empty());
    assert!(
        h.store().session(refused.sid).unwrap().is_none(),
        "nothing created"
    );
    h.until(sid, |k| matches!(k, EventKind::TurnEnded(_))).await;
    // Its end is the parent's alone.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(h.log_of(f.sid), before);
    assert!(h.log().len() > before.len() - 1);
    assert!(H::ended(&h.log()).len() == 2);
}

#[tokio::test]
async fn an_unknown_turn_and_an_unknown_chat_are_refused_and_create_nothing() {
    let mut h = H::new().await;
    h.turn("hello one").await;
    let f = h.fork(h.sid, "no-such-turn").await;
    let why = f.result.unwrap_err();
    assert!(why.starts_with(FORK_REFUSED), "{why}");
    assert!(h.store().session(f.sid).unwrap().is_none());
    let turns = H::ended(&h.log());
    let f = h.fork(SessionId::new(), &turns[0]).await;
    assert_eq!(f.result.unwrap_err(), "unknown session");
    assert!(h.store().session(f.sid).unwrap().is_none());
}

/// The fork is written whole before the ack: a restart right after loads
/// it, appends nothing, and the chat takes a turn; the parent too.
#[tokio::test]
async fn a_restart_right_after_a_fork_loads_it_and_writes_nothing() {
    let mut h = H::new().await;
    h.turn("hello one").await;
    h.turn("hello two").await;
    let turns = H::ended(&h.log());
    let f = h.fork(h.sid, &turns[0]).await;
    f.result.clone().unwrap();
    let (parent, fork) = (h.log(), h.log_of(f.sid));
    h.restart().await;
    assert_eq!(h.log_of(f.sid), fork, "the restart appended nothing to it");
    assert_eq!(h.log(), parent);
    h.turn_of(f.sid, "hello after the restart").await;
    let log = h.log_of(f.sid);
    contiguous(&log);
    assert!(log.len() > fork.len());
    assert_eq!(&log[..fork.len()], &fork[..]);
    let sid = h.sid;
    h.turn_of(sid, "the parent too").await;
    contiguous(&h.log());
}

/// A fork of a fork: its own cut, the first divider kept as part of the
/// copy, a lineage that names its direct parent.
#[tokio::test]
async fn a_fork_of_a_fork_has_its_own_cut() {
    let mut h = H::new().await;
    h.turn("hello one").await;
    h.turn("hello two").await;
    let turns = H::ended(&h.log());
    let f1 = h.fork(h.sid, &turns[0]).await;
    f1.result.clone().unwrap();
    h.turn_of(f1.sid, "hello own").await;
    let l1 = h.log_of(f1.sid);
    let own = H::ended(&l1).pop().unwrap();
    let f2 = h.fork(f1.sid, &own).await;
    f2.result.clone().unwrap();
    let l2 = h.log_of(f2.sid);
    contiguous(&l2);
    let c = created(&l2[0]);
    assert_eq!(c.forked.unwrap().parent, f1.sid);
    let through = c.forked.unwrap().through_seq as usize;
    assert_eq!(through, l1.len());
    assert!(
        l2[..through]
            .iter()
            .skip(1)
            .zip(&l1[1..])
            .all(|(a, b)| { a.kind == b.kind && a.payload == b.payload })
    );
    assert_eq!(
        l2.iter().filter(|e| e.kind == "session_forked").count(),
        2,
        "the first divider is part of the copy"
    );
    let v = SessionView::fold(&l2).unwrap();
    assert_eq!(v.usage.input_tokens, 0);
    assert_eq!(c.title, "Parent (fork)");
    assert_eq!(history_to_messages(&l2), history_to_messages(&l1));
}

/// A fork of a compacted chat: the summary event is copied at its seq, so
/// the summary's `through_seq` still names the same events, and the model
/// is sent the same summary and recent turns.
#[tokio::test]
async fn a_fork_of_a_compacted_chat_keeps_its_summary() {
    let mut h = H::new().await;
    let long = |tag: &str| {
        format!(
            "{tag} {}",
            "words about the database migration ".repeat(120)
        )
    };
    for tag in ["FIRST-TURN", "SECOND-TURN", "THIRD-TURN"] {
        h.turn(&long(tag)).await;
    }
    h.n += 1;
    h.link
        .to_node
        .try_send(MasterToNode::Compact {
            req_id: "k".into(),
            session_id: h.sid,
            focus: None,
            windows: Default::default(),
        })
        .unwrap();
    let sid = h.sid;
    h.until(sid, |k| matches!(k, EventKind::HistorySummarized(_)))
        .await;
    h.turn(&long("FOURTH-TURN")).await;
    let parent = h.log();
    let summary = parent
        .iter()
        .find_map(|e| match e.typed() {
            Ok(EventKind::HistorySummarized(s)) => Some(s.through_seq),
            _ => None,
        })
        .expect("compacted");
    let turns = H::ended(&parent);
    let f = h.fork(h.sid, turns.last().unwrap()).await;
    f.result.clone().unwrap();
    let log = h.log_of(f.sid);
    let end = created(&log[0]).forked.unwrap().through_seq as usize;
    let copied = log
        .iter()
        .find_map(|e| match e.typed() {
            Ok(EventKind::HistorySummarized(s)) => Some(s.through_seq),
            _ => None,
        })
        .expect("the summary is copied");
    assert_eq!(copied, summary);
    assert_eq!(
        history_to_messages(&log),
        history_to_messages(&parent[..end])
    );
    let mark = h.mark();
    h.turn_of(f.sid, "hello fork").await;
    let sent = h.sent_since(mark);
    assert!(sent.contains("FOURTH-TURN"), "the recent turn is sent");
    assert!(
        !sent.contains("FIRST-TURN words"),
        "the summarised turn is not"
    );
}

/// A fork of a chat with a rewind before the cut: the rewind event is copied
/// at its seq, so its range hides the same events, and the model is not sent
/// the rewound turn.
#[tokio::test]
async fn a_fork_after_a_rewind_keeps_the_ranges() {
    let mut h = H::new().await;
    h.turn("hello one").await;
    h.turn("hello rewound two").await;
    let m = h
        .log()
        .iter()
        .filter(|e| matches!(e.typed(), Ok(EventKind::UserMessage(m)) if !m.queued))
        .map(|e| e.seq)
        .nth(1)
        .unwrap();
    h.n += 1;
    h.link
        .to_node
        .try_send(MasterToNode::Rewind {
            req_id: "r".into(),
            session_id: h.sid,
            message_seq: m,
            files: false,
            force: false,
        })
        .unwrap();
    loop {
        if let NodeToMaster::Ack { req_id, ok, error } = h.recv().await
            && req_id == "r"
        {
            assert!(ok, "{error:?}");
            break;
        }
    }
    h.turn("hello four").await;
    let parent = h.log();
    let turns = H::ended(&parent);
    let f = h.fork(h.sid, turns.last().unwrap()).await;
    f.result.clone().unwrap();
    let log = h.log_of(f.sid);
    let end = created(&log[0]).forked.unwrap().through_seq as usize;
    let a = harness_core::rewind::rewound(&parent[..end]);
    let b = harness_core::rewind::rewound(&log);
    assert_eq!(a.active(), b.active());
    assert!(b.holds(m), "the same events are hidden, at the same seqs");
    let messages = format!("{:?}", history_to_messages(&log));
    assert!(messages.contains("hello one") && messages.contains("hello four"));
    assert!(!messages.contains("hello rewound two"), "{messages}");
    // The turn the rewind removed is no cut.
    let f = h.fork(h.sid, &turns[1]).await;
    assert!(f.result.unwrap_err().contains("rewind"));
}

/// A background command running at the cut belongs to the parent: the fork
/// closes it, never runs it, and the closing starts no turn. (The parent's
/// log is written straight into the node's store: a command needs the
/// binary's own guard, which a test of the library does not have; the
/// real thing runs in `crates/cli/tests`.)
#[tokio::test]
async fn a_background_command_at_the_cut_is_closed_in_the_fork_and_calls_nothing_back() {
    let mut h = H::new().await;
    let at = h.log().len() as u64;
    let mut next = at;
    let mut put = |kind: &str, payload: serde_json::Value| {
        next += 1;
        h.store()
            .append(
                "local",
                &RawEvent {
                    session_id: h.sid,
                    seq: next,
                    ts_ms: 1,
                    kind: kind.into(),
                    payload,
                },
            )
            .unwrap();
    };
    put("turn_started", serde_json::json!({"turn_id": "bgturn"}));
    put(
        "user_message",
        serde_json::json!({"turn_id": "bgturn", "text": "start it"}),
    );
    put(
        "background_started",
        serde_json::json!({"bg_id": "bg-1", "call_id": "c1", "turn_id": "bgturn",
            "timeout_s": 120, "ends_by_ms": 1}),
    );
    put(
        "assistant_message",
        serde_json::json!({"turn_id": "bgturn", "message_id": "m", "text": "started"}),
    );
    put(
        "turn_ended",
        serde_json::json!({"turn_id": "bgturn", "reason": "completed"}),
    );
    let parent = h.log();
    assert!(parent.iter().any(|e| e.kind == "background_started"));
    assert!(!parent.iter().any(|e| e.kind == "background_ended"));
    let f = h.fork(h.sid, "bgturn").await;
    f.result.clone().unwrap();
    let log = h.log_of(f.sid);
    let closing = log.last().unwrap();
    let Ok(EventKind::BackgroundEnded(b)) = closing.typed() else {
        panic!("{closing:?}");
    };
    assert_eq!(
        (b.bg_id.as_str(), b.reason),
        ("bg-1", BackgroundReason::Forked)
    );
    assert!(!b.calls_back());
    assert_eq!(log[log.len() - 2].kind, "session_forked");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let log2 = h.log_of(f.sid);
    assert_eq!(log2.len(), log.len(), "no callback turn, nothing else");
    // The command is still the parent's.
    let pv = SessionView::fold(&h.log()).unwrap();
    assert_eq!(pv.running_background(), vec!["bg-1".to_string()]);
    let fv = SessionView::fold(&log2).unwrap();
    assert!(fv.running_background().is_empty());
    // A restart finds nothing of it to close in the fork, and its model is
    // told whose the command was.
    let note = b.model_note().unwrap();
    assert!(note.contains("forked"), "{note}");
    h.restart().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        h.log_of(f.sid).len(),
        log.len(),
        "the restart's sweep has nothing to close in the fork"
    );
}

/// A copied checkpoint is not restorable in the fork (its refs are the
/// parent's); the fork's own turns undo as any.
#[tokio::test]
async fn an_inherited_turn_cannot_be_restored_in_the_fork() {
    let mut h = H::new().await;
    h.turn("run: echo one > a.txt").await;
    let turns = H::ended(&h.log());
    let f = h.fork(h.sid, &turns[0]).await;
    f.result.clone().unwrap();
    h.n += 1;
    let req = format!("u{}", h.n);
    h.link
        .to_node
        .try_send(MasterToNode::Restore {
            req_id: req.clone(),
            session_id: f.sid,
            turn_id: turns[0].clone(),
            force: false,
            rewind: false,
        })
        .unwrap();
    loop {
        if let NodeToMaster::Ack { req_id, ok, error } = h.recv().await
            && req_id == req
        {
            assert!(!ok);
            let e = error.unwrap();
            assert!(e.contains("forked from"), "{e}");
            break;
        }
    }
    assert!(h.ws.join("a.txt").exists(), "nothing was touched");
}

/// The folder is checked as at a create: one the node no longer allows (its
/// `workspace_roots` changed since) refuses the fork and writes nothing.
#[tokio::test]
async fn a_folder_the_node_no_longer_allows_is_refused() {
    let mut h = H::new().await;
    h.turn("hello one").await;
    let turns = H::ended(&h.log());
    drop(h.node.take());
    tokio::time::sleep(Duration::from_millis(200)).await;
    let elsewhere = h.dir.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    let mut cfg = config(h.dir.path(), &h.requests);
    cfg.policy.workspace_roots = vec![elsewhere];
    let node = NodeRuntime::start(cfg).await.unwrap();
    h.link = attach(&node).await;
    h.node = Some(node);
    let f = h.fork(h.sid, &turns[0]).await;
    let why = f.result.unwrap_err();
    assert!(why.starts_with(FORK_REFUSED), "{why}");
    assert!(
        why.contains("workspace_roots") || why.contains("allow"),
        "{why}"
    );
    assert!(h.store().session(f.sid).unwrap().is_none());
    assert!(h.store().fork_of(f.sid).unwrap().is_none());
}

/// The fork shares the parent's folder and copied its events at their old
/// times. Those copies are not the fork's work: the parent's undo of an old
/// turn is not refused as "another chat changed these files then" (T57
/// reads other chats' logs by time).
#[tokio::test]
async fn a_forks_copied_events_are_not_another_chats_work_for_the_parents_undo() {
    let mut h = H::new().await;
    h.turn("run: echo one > a.txt").await;
    h.turn("hello two").await;
    let turns = H::ended(&h.log());
    let f = h.fork(h.sid, &turns[1]).await;
    f.result.clone().unwrap();
    h.n += 1;
    let req = format!("u{}", h.n);
    h.link
        .to_node
        .try_send(MasterToNode::Restore {
            req_id: req.clone(),
            session_id: h.sid,
            turn_id: turns[0].clone(),
            force: false,
            rewind: false,
        })
        .unwrap();
    loop {
        if let NodeToMaster::Ack { req_id, ok, error } = h.recv().await
            && req_id == req
        {
            assert!(ok, "the undo was refused: {error:?}");
            break;
        }
    }
    assert!(!h.ws.join("a.txt").exists(), "the parent's turn was undone");
}

/// The mock's call of `bash` with `command`.
fn bash(command: &str) -> String {
    format!(r#"script:[{{"tool":"bash","input":{{"command":"{command}"}}}}]"#)
}

/// "Allow for this chat" is the parent's, bound to its folder: the fork
/// runs the same command and is ASKED, while the parent still is not.
#[tokio::test]
async fn a_chat_grant_is_not_inherited_by_the_fork() {
    let mut h = H::with_mode(ApprovalMode::Ask).await;
    let sid = h.sid;
    let call = bash("echo granted");
    assert_eq!(h.cards_of(sid, &call, Decision::AllowChat).await, 1);
    assert!(h.log().iter().any(|e| e.kind == "chat_allow_added"));
    assert_eq!(h.cards_of(sid, &call, Decision::Deny).await, 0, "granted");
    let turns = H::ended(&h.log());
    let f = h.fork(sid, turns.last().unwrap()).await;
    f.result.clone().unwrap();
    assert_eq!(
        h.cards_of(f.sid, &call, Decision::Deny).await,
        1,
        "the fork is asked: the grant is the parent's"
    );
    assert_eq!(h.cards_of(sid, &call, Decision::Deny).await, 0);
    // The same after a restart.
    h.restart().await;
    assert_eq!(h.cards_of(f.sid, &call, Decision::Deny).await, 1);
    assert_eq!(h.cards_of(sid, &call, Decision::Deny).await, 0);
}

fn png_image() -> harness_protocol::ChatImage {
    use base64::Engine as _;
    use sha2::Digest as _;
    let png = b"\x89PNG\r\n\x1a\nfork-image";
    harness_protocol::ChatImage {
        media_id: "m1".into(),
        sha256: hex::encode(sha2::Sha256::digest(png)),
        mime: "image/png".into(),
        name: "shot.png".into(),
        width: 1,
        height: 1,
        data: base64::engine::general_purpose::STANDARD.encode(png),
    }
}

/// An image sent in an inherited message is the fork's too: its bytes are
/// under the fork's id, it goes to the model on the fork's first turn, and
/// deleting the parent afterwards does not take it away.
#[tokio::test]
async fn an_inherited_image_is_the_forks_own_and_survives_the_parents_deletion() {
    let mut h = H::new().await;
    let img = png_image();
    h.n += 1;
    h.link
        .to_node
        .try_send(MasterToNode::Prompt(PromptMsg {
            req_id: format!("p{}", h.n),
            images: vec![img.clone()],
            ..PromptMsg::test(h.sid, "look at this")
        }))
        .unwrap();
    let sid = h.sid;
    h.until(sid, |k| matches!(k, EventKind::TurnEnded(_))).await;
    let turns = H::ended(&h.log());
    let f = h.fork(sid, &turns[0]).await;
    f.result.clone().unwrap();
    let held = |h: &H, who: SessionId| {
        h.store()
            .session_images(&who.to_string(), std::slice::from_ref(&img.sha256))
            .unwrap()
            .len()
    };
    assert_eq!(held(&h, f.sid), 1, "a row under the fork's id");
    // The parent goes; the fork keeps its copy.
    h.n += 1;
    h.link
        .to_node
        .try_send(MasterToNode::Delete {
            req_id: format!("d{}", h.n),
            session_id: sid,
        })
        .unwrap();
    loop {
        if let NodeToMaster::Ack { ok, error, .. } = h.recv().await {
            assert!(ok, "{error:?}");
            break;
        }
    }
    assert_eq!(held(&h, sid), 0, "the parent's went with it");
    assert_eq!(held(&h, f.sid), 1, "the fork's did not");
    let mark = h.mark();
    h.turn_of(f.sid, "and now?").await;
    let sent = h.requests.lock().unwrap()[mark].clone();
    let shown: usize = sent
        .messages
        .iter()
        .map(|m| match m {
            harness_agent_native::provider::ChatMsg::UserImages { images, .. } => {
                images.iter().filter(|i| i.data.is_some()).count()
            }
            _ => 0,
        })
        .sum();
    assert_eq!(
        shown, 1,
        "the model sees the inherited image with its bytes"
    );
}

/// A hand-off note is due for what a chat did itself: the fork's copied
/// turn changed a file, yet the node answers "nothing due" for the fork
/// until a turn of its own changed something.
#[tokio::test]
async fn a_fork_is_due_a_handoff_note_only_for_turns_of_its_own() {
    let mut h = H::new().await;
    h.turn("run: echo one > a.txt").await;
    let turns = H::ended(&h.log());
    let f = h.fork(h.sid, &turns[0]).await;
    f.result.clone().unwrap();
    let handoff = |h: &mut H, id: &str, sid: SessionId| {
        h.link
            .to_node
            .try_send(MasterToNode::Handoff {
                req_id: id.into(),
                session_id: sid,
                small_model: None,
                windows: Default::default(),
                low_effort: vec![],
            })
            .unwrap();
        id.to_string()
    };
    let ack = async |h: &mut H, id: String| loop {
        if let NodeToMaster::Ack { req_id, ok, error } = h.recv().await
            && req_id == id
        {
            return if ok {
                Ok(())
            } else {
                Err(error.unwrap_or_default())
            };
        }
    };
    let id = handoff(&mut h, "h1", f.sid);
    let got = ack(&mut h, id).await;
    assert_eq!(
        got,
        Err(harness_protocol::HANDOFF_NOTHING.to_string()),
        "the copied turn is the parent's to be noted"
    );
    h.turn_of(f.sid, "run: echo two > b.txt").await;
    let id = handoff(&mut h, "h2", f.sid);
    assert_eq!(ack(&mut h, id).await, Ok(()), "its own turn changed a file");
}
