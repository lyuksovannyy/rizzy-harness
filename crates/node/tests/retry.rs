//! A person's Retry of the chat's failed last turn on a real node
//! (ADOPTION_PLAN X.21, README "Retry"): a new turn of the person's whose
//! `turn_started` names the failed turn and which writes no second
//! `user_message`; the model is sent the message once, the failed
//! attempt's steps (its tool calls and results) and the harness's line
//! saying they happened, so a command or an edit is not repeated blindly;
//! refused for anything but the last failed turn, while a turn runs and a
//! second time while the retry runs; Undo of the retry leaves the failed
//! attempt's changes; attachments, images and file mentions are in the
//! history once; a node restart between the failure and the retry changes
//! nothing; the next turn rebuilds the retry's request byte for byte.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use harness_agent_native::llm::{ModelRouter, Route};
use harness_agent_native::{
    ChatMsg, ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError,
    RetryHint, mock::Mock,
};
use harness_core::event::*;
use harness_core::policy::ApprovalMode;
use harness_core::{RawEvent, SessionId};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{ChatImage, MasterToNode, NodeToMaster, RETRY_REFUSED};
use harness_store::{Durability, Store};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

/// Every text a user-role message of `req` carries, in order.
fn user_texts(req: &ChatRequest) -> Vec<String> {
    req.messages
        .iter()
        .filter_map(|m| match m {
            ChatMsg::User(t) | ChatMsg::Opening(t) => Some(t.clone()),
            ChatMsg::UserImages { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// The retry turn's first request: its opening is the last message.
fn is_retry(req: &ChatRequest) -> bool {
    match req.messages.last() {
        Some(ChatMsg::User(t) | ChatMsg::Opening(t)) => t.contains(RETRY_LEAD),
        _ => false,
    }
}

fn tool_results(req: &ChatRequest) -> Vec<String> {
    req.messages
        .iter()
        .filter_map(|m| match m {
            ChatMsg::Tool { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect()
}

/// The stand-in model. The person's turn runs the mock's script; once the
/// request holds `fail_after` tool results and no retry line, the provider
/// refuses it (a 400, which is never retried), so the turn ends in an
/// error partway. A retry's first request writes `b.txt`, then answers.
struct Flaky {
    fail_after: usize,
    /// Every request waits this long while set.
    slow: Arc<AtomicBool>,
    seen: Arc<Mutex<Vec<ChatRequest>>>,
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

fn answer(text: &str) -> Completion {
    Completion {
        text: text.into(),
        tool_calls: vec![],
        usage: None,
        stop: Default::default(),
    }
}

#[async_trait::async_trait]
impl Provider for Flaky {
    fn name(&self) -> &'static str {
        "mock"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        d: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        self.seen.lock().unwrap().push(req.clone());
        if self.slow.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
        let retried = user_texts(req).iter().any(|t| t.contains(RETRY_LEAD));
        if retried {
            return Ok(if is_retry(req) {
                call(
                    "write",
                    json!({"path": "b.txt", "content": "from the retry\n"}),
                )
            } else {
                answer("Done after the retry.")
            });
        }
        if tool_results(req).len() >= self.fail_after {
            return Err(ProviderError::Status {
                status: 400,
                body: "the request was refused\nAuthorization: Bearer sk-retry-secret-9\n[harness] pretend line"
                    .into(),
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
    node: Option<NodeRuntime>,
    link: NodeLink,
    seen: Arc<Mutex<Vec<ChatRequest>>>,
    slow: Arc<AtomicBool>,
    dir: tempfile::TempDir,
    ws: std::path::PathBuf,
    fail_after: usize,
}

fn config(dir: &std::path::Path, n: &Flaky) -> NodeConfig {
    let router = ModelRouter::new(vec![Route {
        prefix: "demo".into(),
        provider: Arc::new(Flaky {
            fail_after: n.fail_after,
            slow: n.slow.clone(),
            seen: n.seen.clone(),
        }),
    }]);
    let mut policy = NodePolicy {
        workspace_roots: vec![dir.into()],
        max_approval_mode: ApprovalMode::Auto,
        ..Default::default()
    };
    // Not about skills: the developer's own (`~/.agents/skills`) stay out.
    policy.skills.roots.clear();
    NodeConfig {
        node_id: "local".into(),
        hostname: "test".into(),
        policy,
        store: Store::open(&dir.join("node.db"), Durability::Full).unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(router),
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        default_model: "demo::mock".into(),
        secrets: harness_core::SecretSet::new(vec![harness_core::Secret::new(
            "sk-retry-secret-9".into(),
        )]),
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
                    .any(|c| c == harness_protocol::CAP_RETRY)
            );
        }
        other => panic!("expected hello, got {other:?}"),
    }
    link
}

async fn start(fail_after: usize) -> Node {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let seen: Arc<Mutex<Vec<ChatRequest>>> = Arc::default();
    let slow = Arc::new(AtomicBool::new(false));
    let f = Flaky {
        fail_after,
        slow: slow.clone(),
        seen: seen.clone(),
    };
    let node = NodeRuntime::start(config(dir.path(), &f)).await.unwrap();
    let link = attach(&node).await;
    Node {
        node: Some(node),
        link,
        seen,
        slow,
        dir,
        ws,
        fail_after,
    }
}

impl Node {
    fn store(&self) -> &Store {
        self.node.as_ref().unwrap().store()
    }

    /// Stop the node mid-whatever and start it again on the same database.
    async fn restart(&mut self) {
        drop(self.node.take());
        tokio::time::sleep(Duration::from_millis(200)).await;
        let f = Flaky {
            fail_after: self.fail_after,
            slow: self.slow.clone(),
            seen: self.seen.clone(),
        };
        let node = NodeRuntime::start(config(self.dir.path(), &f))
            .await
            .unwrap();
        self.link = attach(&node).await;
        self.node = Some(node);
    }

    fn requests(&self) -> Vec<ChatRequest> {
        self.seen.lock().unwrap().clone()
    }
}

/// Events until one for `sid` matches; acks must be ok.
async fn until(link: &mut NodeLink, sid: SessionId, pred: impl Fn(&EventKind) -> bool) {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(20), link.from_node.recv())
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
        let msg = tokio::time::timeout(Duration::from_secs(20), link.from_node.recv())
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

fn prompt_with(
    link: &NodeLink,
    sid: SessionId,
    req_id: &str,
    text: &str,
    attachments: Vec<Attachment>,
    images: Vec<ChatImage>,
    files: Vec<String>,
) {
    link.to_node
        .try_send(MasterToNode::Prompt {
            savings: None,
            context: None,
            reply: None,
            agent: None,
            attachments,
            compare: vec![],
            req_id: req_id.into(),
            session_id: sid,
            text: text.into(),
            images,
            files,
            agent_named: false,
        })
        .unwrap();
}

fn prompt(link: &NodeLink, sid: SessionId, req_id: &str, text: &str) {
    prompt_with(link, sid, req_id, text, vec![], vec![], vec![]);
}

fn retry(link: &NodeLink, sid: SessionId, req_id: &str, turn: &str) {
    link.to_node
        .try_send(MasterToNode::Retry {
            req_id: req_id.into(),
            session_id: sid,
            turn_id: turn.into(),
            agent: None,
            savings: None,
            context: None,
        })
        .unwrap();
}

async fn turn_ended(link: &mut NodeLink, sid: SessionId) {
    until(link, sid, |k| matches!(k, EventKind::TurnEnded(_))).await;
}

fn log(store: &Store, sid: SessionId) -> Vec<RawEvent> {
    store.events_after(sid, 0, 10_000).unwrap()
}

fn kinds(log: &[RawEvent]) -> Vec<EventKind> {
    log.iter().filter_map(|e| e.typed().ok()).collect()
}

fn starts(log: &[RawEvent]) -> Vec<TurnStarted> {
    kinds(log)
        .into_iter()
        .filter_map(|k| match k {
            EventKind::TurnStarted(t) => Some(t),
            _ => None,
        })
        .collect()
}

fn ends(log: &[RawEvent]) -> Vec<TurnEnded> {
    kinds(log)
        .into_iter()
        .filter_map(|k| match k {
            EventKind::TurnEnded(t) => Some(t),
            _ => None,
        })
        .collect()
}

fn user_messages(log: &[RawEvent]) -> usize {
    log.iter().filter(|e| e.kind == "user_message").count()
}

/// The script of the failing turn: write `a.txt`, append to `ran.log` by a
/// command; the provider then refuses the next request.
fn work() -> String {
    let steps = json!([
        {"tool": "write", "input": {"path": "a.txt", "content": "one\n"}},
        {"tool": "bash", "input": {"command": "echo ran >> ran.log"}},
    ]);
    format!("script:{steps}")
}

/// Fail a turn of `work()` after both its calls ran; its id.
async fn failed_turn(n: &mut Node, sid: SessionId) -> String {
    prompt(&n.link, sid, "p1", &work());
    turn_ended(&mut n.link, sid).await;
    let l = log(n.store(), sid);
    let end = ends(&l).pop().unwrap();
    assert_eq!(end.reason, TurnEndReason::Error, "{:?}", kinds(&l));
    end.turn_id
}

/// The retry is a new turn naming the failed one, with no second message;
/// its first request has the message once, the failed attempt's two calls
/// and their results, and the harness's line last, saying the calls ran
/// and quoting the error; the command and the edit are not run again; the
/// next turn's request starts with the retry's own, byte for byte; Undo of
/// the retry undoes its change only.
#[tokio::test]
async fn a_retry_carries_on_after_the_failed_attempts_steps() {
    let mut n = start(2).await;
    let sid = create(&mut n).await;
    let failed = failed_turn(&mut n, sid).await;
    assert_eq!(
        std::fs::read_to_string(n.ws.join("a.txt")).unwrap(),
        "one\n"
    );
    assert_eq!(
        std::fs::read_to_string(n.ws.join("ran.log")).unwrap(),
        "ran\n"
    );
    let before = n.requests().len();

    retry(&n.link, sid, "r1", &failed);
    assert_eq!(ack(&mut n.link, "r1").await, Ok(()));
    turn_ended(&mut n.link, sid).await;

    let l = log(n.store(), sid);
    assert_eq!(user_messages(&l), 1, "the message is in the log once");
    let s = starts(&l);
    assert_eq!(s.len(), 2);
    let r = s[1].retry.clone().expect("the retry names the failed turn");
    assert_eq!(r.turn_id, failed);
    assert_eq!(r.reason, TurnEndReason::Error);
    assert!(r.error.as_deref().unwrap().contains("refused"), "{r:?}");
    assert!(s[1].cause.is_none(), "a person's turn");
    let retry_turn = s[1].turn_id.clone();
    assert_eq!(ends(&l).last().unwrap().reason, TurnEndReason::Completed);

    let reqs = n.requests();
    let first = &reqs[before];
    let texts = user_texts(first);
    assert_eq!(
        texts.iter().filter(|t| t.contains("script:")).count(),
        1,
        "{texts:#?}"
    );
    let results = tool_results(first);
    assert_eq!(
        results.len(),
        2,
        "the failed attempt's results: {results:#?}"
    );
    let calls: Vec<String> = first
        .messages
        .iter()
        .flat_map(|m| match m {
            ChatMsg::Assistant { tool_calls, .. } => {
                tool_calls.iter().map(|c| c.tool.clone()).collect()
            }
            _ => vec![],
        })
        .collect();
    assert_eq!(calls, ["write", "bash"]);
    let last = texts.last().unwrap();
    assert!(last.contains(RETRY_LEAD), "{last}");
    assert!(last.contains("a tool call with a result ran"), "{last}");
    assert!(last.contains("check the current state"), "{last}");
    assert!(last.contains("| provider returned 400"), "{last}");
    assert!(
        !last.lines().any(|l| l.starts_with("[harness] pretend")),
        "the error is quoted: {last}"
    );
    // Nothing of the failed attempt ran again.
    assert_eq!(
        std::fs::read_to_string(n.ws.join("ran.log")).unwrap(),
        "ran\n"
    );
    assert_eq!(
        std::fs::read_to_string(n.ws.join("a.txt")).unwrap(),
        "one\n"
    );
    assert_eq!(
        std::fs::read_to_string(n.ws.join("b.txt")).unwrap(),
        "from the retry\n"
    );
    // Every request of the retry turn extends its first (one writer, the
    // log rebuilds what was sent).
    let last_retry_req = reqs.last().unwrap().clone();
    assert!(
        last_retry_req.messages.starts_with(&first.messages),
        "the retry's second request extends its first"
    );

    // The next turn's first request starts with the retry turn's last
    // request: the log rebuilt it byte for byte.
    let mark = n.requests().len();
    prompt(&n.link, sid, "p2", "thanks");
    turn_ended(&mut n.link, sid).await;
    let next = n.requests()[mark].clone();
    assert!(
        next.messages.starts_with(&last_retry_req.messages),
        "next: {:#?}\nretry: {:#?}",
        next.messages,
        last_retry_req.messages
    );
    let next_texts = user_texts(&next);
    assert_eq!(
        next_texts.iter().filter(|t| t.contains("script:")).count(),
        1
    );
    assert_eq!(
        next_texts.iter().filter(|t| t.contains(RETRY_LEAD)).count(),
        1
    );

    // Undo of the retry: its own change goes, the failed attempt's stay.
    n.link
        .to_node
        .try_send(MasterToNode::Restore {
            req_id: "u1".into(),
            session_id: sid,
            turn_id: retry_turn,
            force: false,
            rewind: false,
        })
        .unwrap();
    assert_eq!(ack(&mut n.link, "u1").await, Ok(()));
    assert!(!n.ws.join("b.txt").exists(), "the retry's file is undone");
    assert_eq!(
        std::fs::read_to_string(n.ws.join("a.txt")).unwrap(),
        "one\n"
    );
    assert_eq!(
        std::fs::read_to_string(n.ws.join("ran.log")).unwrap(),
        "ran\n"
    );
}

/// A turn that failed in Auto and is retried after the chat entered plan
/// mode runs under plan mode: the node reads the chat's mode when the retry
/// runs, it is not taken from the failed turn's record (X.21 with X.7). The
/// stand-in model's retry asks for a `write`: refused, nothing written;
/// what the failed attempt did in Auto stays as it was.
#[tokio::test]
async fn a_retry_runs_under_the_chats_mode_as_it_is_now() {
    let mut n = start(2).await;
    let sid = create(&mut n).await;
    let failed = failed_turn(&mut n, sid).await;
    n.link
        .to_node
        .try_send(MasterToNode::SetApprovalMode {
            lowering: true,
            req_id: "plan".into(),
            session_id: sid,
            mode: ApprovalMode::Plan,
        })
        .unwrap();
    assert_eq!(ack(&mut n.link, "plan").await, Ok(()));

    retry(&n.link, sid, "r1", &failed);
    assert_eq!(ack(&mut n.link, "r1").await, Ok(()));
    turn_ended(&mut n.link, sid).await;

    assert!(!n.ws.join("b.txt").exists(), "the retry wrote in plan mode");
    let l = log(n.store(), sid);
    let last = kinds(&l)
        .into_iter()
        .rev()
        .find_map(|k| match k {
            EventKind::ToolCallFinished(f) => Some((f.ok, f.output)),
            _ => None,
        })
        .expect("the retry's call has a result");
    assert!(!last.0 && last.1.contains("plan mode"), "{last:?}");
    assert_eq!(
        std::fs::read_to_string(n.ws.join("a.txt")).unwrap(),
        "one\n"
    );
}

/// Only the last failed turn: not one that completed, not one with a turn
/// after it, not while a turn runs, and only once while the retry runs.
#[tokio::test]
async fn retry_is_refused_unless_it_is_the_failed_last_turn() {
    let mut n = start(2).await;
    let sid = create(&mut n).await;
    prompt(&n.link, sid, "p0", "hello");
    turn_ended(&mut n.link, sid).await;
    let done = ends(&log(n.store(), sid)).pop().unwrap().turn_id;
    retry(&n.link, sid, "r0", &done);
    let e = ack(&mut n.link, "r0").await.unwrap_err();
    assert!(e.starts_with(RETRY_REFUSED) && e.contains("answer"), "{e}");

    let failed = failed_turn(&mut n, sid).await;
    retry(&n.link, sid, "r1", &done);
    let e = ack(&mut n.link, "r1").await.unwrap_err();
    assert!(e.contains("last turn"), "{e}");
    retry(&n.link, sid, "r2", "no-such-turn");
    assert!(ack(&mut n.link, "r2").await.is_err());

    // Two clicks while the first retry runs: one retry.
    n.slow.store(true, Ordering::SeqCst);
    retry(&n.link, sid, "r3", &failed);
    retry(&n.link, sid, "r4", &failed);
    assert_eq!(ack(&mut n.link, "r3").await, Ok(()));
    let e = ack(&mut n.link, "r4").await.unwrap_err();
    assert!(e.contains("already running"), "{e}");
    n.slow.store(false, Ordering::SeqCst);
    turn_ended(&mut n.link, sid).await;
    let l = log(n.store(), sid);
    assert_eq!(
        starts(&l).iter().filter(|s| s.retry.is_some()).count(),
        1,
        "one retry"
    );
    // The retry completed: it is no longer a failed turn to retry.
    let retried = starts(&l).pop().unwrap().turn_id;
    retry(&n.link, sid, "r5", &retried);
    assert!(ack(&mut n.link, "r5").await.is_err());
    retry(&n.link, sid, "r6", &failed);
    assert!(ack(&mut n.link, "r6").await.is_err());
    assert_eq!(user_messages(&log(n.store(), sid)), 2);
}

/// The message's attachment, image and file mention are what the failed
/// turn recorded: the retry's request carries each once (the file as the
/// node read it then, not read again), and writes no message.
#[tokio::test]
async fn attachments_images_and_file_mentions_are_sent_once() {
    let mut n = start(0).await;
    let sid = create(&mut n).await;
    std::fs::write(n.ws.join("notes.md"), "MENTIONED-FILE-CONTENT\n").unwrap();
    let png = b"\x89PNG\r\n\x1a\nfake";
    let image = ChatImage {
        media_id: "m1".into(),
        sha256: hex::encode(Sha256::digest(png)),
        mime: "image/png".into(),
        name: "shot.png".into(),
        width: 1,
        height: 1,
        data: base64::engine::general_purpose::STANDARD.encode(png),
    };
    prompt_with(
        &n.link,
        sid,
        "p1",
        "look at these",
        vec![Attachment {
            name: "spec.txt".into(),
            mime: "text/plain".into(),
            text: "ATTACHED-TEXT".into(),
            truncated: false,
        }],
        vec![image],
        vec!["notes.md".into()],
    );
    turn_ended(&mut n.link, sid).await;
    let failed = ends(&log(n.store(), sid)).pop().unwrap();
    assert_eq!(failed.reason, TurnEndReason::Error);
    // Changed since: the retry sends what the message recorded.
    std::fs::write(n.ws.join("notes.md"), "CHANGED-SINCE\n").unwrap();
    let before = n.requests().len();
    retry(&n.link, sid, "r1", &failed.turn_id);
    assert_eq!(ack(&mut n.link, "r1").await, Ok(()));
    turn_ended(&mut n.link, sid).await;
    let first = n.requests()[before].clone();
    let all = user_texts(&first).join("\n");
    for needle in ["look at these", "ATTACHED-TEXT", "MENTIONED-FILE-CONTENT"] {
        assert_eq!(all.matches(needle).count(), 1, "{needle}: {all}");
    }
    assert!(!all.contains("CHANGED-SINCE"), "{all}");
    let images: usize = first
        .messages
        .iter()
        .map(|m| match m {
            ChatMsg::UserImages { images, .. } => images.len(),
            _ => 0,
        })
        .sum();
    assert_eq!(images, 1);
    assert_eq!(user_messages(&log(n.store(), sid)), 1);
}

/// A turn the node's stop cut off (`stopped`), then a restart: the person
/// retries it after the restart, and the log decides as before.
#[tokio::test]
async fn a_turn_cut_off_by_a_node_stop_is_retried_after_a_restart() {
    let mut n = start(usize::MAX).await;
    let sid = create(&mut n).await;
    n.slow.store(true, Ordering::SeqCst);
    prompt(&n.link, sid, "p1", "run: echo started >> once.log");
    until(&mut n.link, sid, |k| matches!(k, EventKind::UserMessage(_))).await;
    n.node.as_ref().unwrap().stop(Duration::from_secs(10)).await;
    n.restart().await;
    n.slow.store(false, Ordering::SeqCst);
    let l = log(n.store(), sid);
    let end = ends(&l).pop().expect("the stop closed the turn");
    assert_eq!(end.reason, TurnEndReason::Stopped, "{:?}", kinds(&l));
    let before = n.requests().len();
    retry(&n.link, sid, "r1", &end.turn_id);
    assert_eq!(ack(&mut n.link, "r1").await, Ok(()));
    turn_ended(&mut n.link, sid).await;
    let l = log(n.store(), sid);
    let r = starts(&l).pop().unwrap().retry.unwrap();
    assert_eq!(r.reason, TurnEndReason::Stopped);
    let first = n.requests()[before].clone();
    let last = user_texts(&first).pop().unwrap();
    assert!(last.contains("cut off when the node was stopped"), "{last}");
    assert_eq!(user_messages(&l), 1);
}

/// A turn open in the log when the node starts (the process died) is
/// closed `interrupted` at start; the person retries it.
#[tokio::test]
async fn a_turn_the_process_died_in_is_retried_after_the_restart() {
    let mut n = start(usize::MAX).await;
    let sid = create(&mut n).await;
    drop(n.node.take());
    tokio::time::sleep(Duration::from_millis(200)).await;
    {
        let store = Store::open(&n.dir.path().join("node.db"), Durability::Full).unwrap();
        let head = log(&store, sid).last().unwrap().seq;
        for (seq, k) in (head + 1..).zip([
            EventKind::TurnStarted(TurnStarted {
                turn_id: "died".into(),
                agent: None,
                cause: None,
                environment: None,
                retry: None,
            }),
            EventKind::UserMessage(
                serde_json::from_value(
                    json!({"turn_id": "died", "text": "WRITTEN-BEFORE-THE-CRASH"}),
                )
                .unwrap(),
            ),
        ]) {
            let (kind, payload) = k.into_parts();
            store
                .append(
                    "local",
                    &RawEvent {
                        session_id: sid,
                        seq,
                        ts_ms: 0,
                        kind,
                        payload,
                    },
                )
                .unwrap();
        }
    }
    n.restart().await;
    let l = log(n.store(), sid);
    let end = ends(&l).pop().expect("the start closed the turn");
    assert_eq!(
        (end.turn_id.as_str(), end.reason),
        ("died", TurnEndReason::Interrupted)
    );
    let before = n.requests().len();
    retry(&n.link, sid, "r1", "died");
    assert_eq!(ack(&mut n.link, "r1").await, Ok(()));
    turn_ended(&mut n.link, sid).await;
    let first = n.requests()[before].clone();
    let texts = user_texts(&first);
    assert_eq!(
        texts
            .iter()
            .filter(|t| t.contains("WRITTEN-BEFORE-THE-CRASH"))
            .count(),
        1
    );
    assert!(
        texts
            .last()
            .unwrap()
            .contains("cut off when the node restarted")
    );
}

/// A `/skill` at the start of the failed message loads again for the retry,
/// as it did for the failed turn (the retry's context says the person asks
/// for it). Failed on the first version: the retry ran with no message
/// text, so nothing asked for the skill.
#[tokio::test]
async fn a_skill_the_message_names_loads_for_the_retry() {
    let mut n = start(0).await;
    std::fs::create_dir_all(n.ws.join(".git")).unwrap();
    std::fs::create_dir_all(n.ws.join(".agents/skills/release")).unwrap();
    std::fs::write(
        n.ws.join(".agents/skills/release/SKILL.md"),
        "---\nname: release\ndescription: Cut a release\n---\nTag it, then write notes.\n",
    )
    .unwrap();
    let sid = create(&mut n).await;
    prompt(&n.link, sid, "p1", "/release now");
    turn_ended(&mut n.link, sid).await;
    let failed = ends(&log(n.store(), sid)).pop().unwrap();
    assert_eq!(failed.reason, TurnEndReason::Error);
    let before = n.requests().len();
    retry(&n.link, sid, "r1", &failed.turn_id);
    assert_eq!(ack(&mut n.link, "r1").await, Ok(()));
    turn_ended(&mut n.link, sid).await;
    let first = n.requests()[before].clone();
    let opening = user_texts(&first).pop().unwrap();
    assert!(
        opening.contains("starts with /release: they ask for the skill `release`"),
        "{opening}"
    );
    assert_eq!(user_messages(&log(n.store(), sid)), 1);
}

/// A provider error that echoes a key the node holds: the key is in
/// neither the log nor the retry's line (the scrubbed error is what both
/// read). From the review of PR 128.
#[tokio::test]
async fn a_secret_in_the_failed_attempts_error_is_not_retried_to_the_model() {
    let mut n = start(2).await;
    let sid = create(&mut n).await;
    let failed = failed_turn(&mut n, sid).await;
    let before = n.requests().len();
    retry(&n.link, sid, "r1", &failed);
    assert_eq!(ack(&mut n.link, "r1").await, Ok(()));
    turn_ended(&mut n.link, sid).await;
    let raw: String = log(n.store(), sid)
        .iter()
        .map(|e| e.payload.to_string())
        .collect();
    let opening = user_texts(&n.requests()[before]).pop().unwrap();
    assert!(!raw.contains("sk-retry-secret-9"), "in the log");
    assert!(!opening.contains("sk-retry-secret-9"), "sent to the model");
    assert!(opening.contains("| provider returned 400"), "{opening}");
}

/// The process died with one call started and no result and one approval
/// card waiting: the retry gets a result for each (never an orphan call),
/// the card's says it did not run (it was denied at the restart), the
/// started one's that it may have run, and the line says both, without
/// quoting the restart's "Send a message to continue". From the review of
/// PR 128 (M1).
#[tokio::test]
async fn a_crash_with_a_running_call_and_a_waiting_card_is_retried_truthfully() {
    let mut n = start(usize::MAX).await;
    let sid = create(&mut n).await;
    drop(n.node.take());
    tokio::time::sleep(Duration::from_millis(200)).await;
    {
        let store = Store::open(&n.dir.path().join("node.db"), Durability::Full).unwrap();
        let head = log(&store, sid).last().unwrap().seq;
        let evs: Vec<EventKind> = vec![
            EventKind::TurnStarted(TurnStarted {
                turn_id: "died".into(),
                agent: None,
                cause: None,
                environment: None,
                retry: None,
            }),
            EventKind::UserMessage(
                serde_json::from_value(json!({"turn_id": "died", "text": "deploy it"})).unwrap(),
            ),
            EventKind::AssistantMessage(
                serde_json::from_value(json!({
                    "turn_id": "died", "message_id": "m1", "text": "",
                    "tool_calls": [
                        {"call_id": "c-orphan", "tool": "bash", "input": {"command": "echo half >> deploy.log"}},
                        {"call_id": "c-card", "tool": "bash", "input": {"command": "rm -rf build"}}
                    ]
                }))
                .unwrap(),
            ),
            EventKind::ToolCallStarted(
                serde_json::from_value(json!({
                    "turn_id": "died", "call_id": "c-orphan", "tool": "bash",
                    "input": {"command": "echo half >> deploy.log"}
                }))
                .unwrap(),
            ),
            EventKind::ApprovalRequested(
                serde_json::from_value(json!({
                    "approval_id": "a-card", "call_id": "c-card", "tool": "bash",
                    "input": {"command": "rm -rf build"}, "reason": "asks", "turn_allow": true
                }))
                .unwrap(),
            ),
        ];
        for (seq, k) in (head + 1..).zip(evs) {
            let (kind, payload) = k.into_parts();
            store
                .append(
                    "local",
                    &RawEvent {
                        session_id: sid,
                        seq,
                        ts_ms: 0,
                        kind,
                        payload,
                    },
                )
                .unwrap();
        }
    }
    n.restart().await;
    let before = n.requests().len();
    retry(&n.link, sid, "r1", "died");
    assert_eq!(ack(&mut n.link, "r1").await, Ok(()));
    turn_ended(&mut n.link, sid).await;
    let first = n.requests()[before].clone();
    let result = |id: &str| {
        first
            .messages
            .iter()
            .find_map(|m| match m {
                ChatMsg::Tool { call_id, content } if call_id == id => Some(content.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{id} sent without a result"))
    };
    assert!(result("c-orphan").contains("may have run"));
    assert!(
        result("c-card").contains("did not run"),
        "{}",
        result("c-card")
    );
    let opening = user_texts(&first).pop().unwrap();
    assert!(
        opening.contains("cut off when the node restarted"),
        "{opening}"
    );
    assert!(opening.contains("may or may not have run"), "{opening}");
    assert!(!opening.contains("Send a message to continue"), "{opening}");
}
