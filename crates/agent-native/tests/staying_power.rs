//! The loop's staying power: what a person's turn does at its step cap (a
//! last request that says what is done and what is left, ended as a limit
//! and not as an error), the note at half the budget, the provider stream's
//! idle timeout, what a call cut off by a restart reads as, and the
//! environment line each turn opens with.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use harness_agent_native::llm::{ApiKind, LlmProvider};
use harness_agent_native::{
    ChatMsg, ChatRequest, Completion, NOT_RUN_AT_STEP_LIMIT, NativeBackend, NativeConfig, Provider,
    ProviderError, history_to_messages,
};
use harness_core::backend::{AgentBackend, TurnCtx, TurnHost};
use harness_core::event::*;
use harness_core::policy::{Decision, ToolRisk};
use harness_core::{RawEvent, SessionId};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

#[derive(Default)]
struct Host(Mutex<Vec<EventKind>>);

#[async_trait::async_trait]
impl TurnHost for Host {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        self.0.lock().unwrap().push(kind);
        Ok(())
    }
    async fn approve(&self, _: &str, _: &str, _: &Value, _: ToolRisk) -> Decision {
        Decision::Allow
    }
}

impl Host {
    fn finished(&self) -> Vec<ToolCallFinished> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                EventKind::ToolCallFinished(f) => Some(f.clone()),
                _ => None,
            })
            .collect()
    }
}

type Answer = Box<dyn Fn(usize, &ChatRequest) -> Completion + Send + Sync>;

/// Records every request; `answer(n, req)` gives request `n`'s (from 1).
struct Script {
    seen: Mutex<Vec<ChatRequest>>,
    answer: Answer,
}

impl Script {
    fn new(
        answer: impl Fn(usize, &ChatRequest) -> Completion + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            seen: Mutex::default(),
            answer: Box::new(answer),
        })
    }
    fn seen(&self) -> Vec<ChatRequest> {
        self.seen.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl Provider for Script {
    fn name(&self) -> &'static str {
        "script"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        _: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        let n = {
            let mut s = self.seen.lock().unwrap();
            s.push(req.clone());
            s.len()
        };
        Ok((self.answer)(n, req))
    }
}

/// Another `true` each time, so the step cap ends the turn and not the
/// repeated-call guard.
fn bash_call(n: usize) -> Completion {
    Completion {
        text: String::new(),
        tool_calls: vec![ToolCallRef {
            call_id: format!("c{n}"),
            tool: "bash".into(),
            input: json!({"command": format!("true {n}")}),
        }],
        usage: None,
        stop: Default::default(),
    }
}

fn said(text: &str) -> Completion {
    Completion {
        text: text.into(),
        ..Default::default()
    }
}

fn turn<'a>(host: &'a Host, cwd: &std::path::Path) -> TurnCtx<'a> {
    TurnCtx {
        session_id: SessionId::new(),
        turn_id: "t".into(),
        history: vec![],
        user_text: "go".into(),
        cwd: cwd.to_path_buf(),
        model: "x".into(),
        agent: None,
        compare: vec![],
        parent_session: None,
        extra: Default::default(),
        cancel: Default::default(),
        host,
    }
}

fn steps(n: usize) -> NativeConfig {
    NativeConfig {
        max_steps: n,
        ..Default::default()
    }
}

/// All the text a request carries, notes placed (what the model reads).
fn text_of(req: &ChatRequest) -> String {
    req.messages
        .iter()
        .map(|m| match m {
            ChatMsg::System(t) | ChatMsg::User(t) | ChatMsg::Opening(t) => t.clone(),
            ChatMsg::UserImages { text, .. } => text.clone(),
            ChatMsg::Tool { content, .. } => content.clone(),
            ChatMsg::Assistant { text, .. } => text.clone(),
            ChatMsg::Note(t) | ChatMsg::CallNote(t) => t.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The person's turn at its cap: the last request says so (and keeps the
/// tool list, for the prompt cache), the text answer it gets is the
/// turn's last word, and the turn ends at a limit, not with an error the
/// person has to read as a failure.
#[tokio::test]
async fn the_last_step_asks_for_a_wrap_up_and_ends_at_a_limit() {
    let dir = tempfile::tempdir().unwrap();
    let host = Host::default();
    let p = Script::new(|n, _| {
        if n < 3 {
            bash_call(n)
        } else {
            said("Done: two checks. Left: the rest.")
        }
    });
    let b = NativeBackend::new(p.clone(), harness_tools::builtin(), steps(3));
    let out = b.run_turn(turn(&host, dir.path())).await;
    let seen = p.seen();
    assert_eq!(seen.len(), 3, "the cap counts the wrap-up request");
    let last = text_of(&seen[2]);
    assert!(
        last.contains("[harness] This is the last step of this turn"),
        "{last}"
    );
    assert!(!text_of(&seen[1]).contains("last step"), "only the last");
    assert_eq!(
        seen[2].tools.len(),
        seen[0].tools.len(),
        "the tool list stays (prompt cache)"
    );
    assert_eq!(out.reason, TurnEndReason::Limit);
    assert_eq!(out.error, None, "not an error");
    assert_eq!(host.finished().len(), 2, "both earlier calls ran");
}

/// A call the model still makes on its last step is not run: it is
/// answered with the harness's reason, which the next turn reads, and the
/// turn ends at the limit.
#[tokio::test]
async fn a_call_on_the_last_step_is_not_run() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("ran");
    let host = Host::default();
    let m = marker.clone();
    let p = Script::new(move |n, _| {
        if n < 2 {
            bash_call(n)
        } else {
            Completion {
                text: "One more thing.".into(),
                tool_calls: vec![ToolCallRef {
                    call_id: "late".into(),
                    tool: "bash".into(),
                    input: json!({"command": format!("touch {}", m.display())}),
                }],
                usage: None,
                stop: Default::default(),
            }
        }
    });
    let b = NativeBackend::new(p.clone(), harness_tools::builtin(), steps(2));
    let out = b.run_turn(turn(&host, dir.path())).await;
    assert_eq!(out.reason, TurnEndReason::Limit);
    assert_eq!(out.error, None);
    let late = host
        .finished()
        .into_iter()
        .find(|f| f.call_id == "late")
        .expect("the late call is answered");
    assert!(!late.ok);
    assert_eq!(late.output, NOT_RUN_AT_STEP_LIMIT);
    assert!(!marker.exists(), "the late call ran");
}

/// Halfway through its budget the model is told how much it has used,
/// once (goose's turn-budget note).
#[tokio::test]
async fn half_the_budget_is_noted_once() {
    let dir = tempfile::tempdir().unwrap();
    let host = Host::default();
    let p = Script::new(|n, _| if n < 6 { bash_call(n) } else { said("ok") });
    let b = NativeBackend::new(p.clone(), harness_tools::builtin(), steps(8));
    b.run_turn(turn(&host, dir.path())).await;
    let seen = p.seen();
    let note = "[harness] You have used 4 of the 8 steps this turn may take";
    assert!(!text_of(&seen[3]).contains(note), "not before half");
    assert!(text_of(&seen[4]).contains(note), "{}", text_of(&seen[4]));
    for r in &seen[5..] {
        assert_eq!(text_of(r).matches(note).count(), 1, "once");
    }
}

/// A sub-agent keeps its own last step (`finish` alone) and its error.
#[tokio::test]
async fn a_sub_agent_keeps_its_finish_only_last_step() {
    let dir = tempfile::tempdir().unwrap();
    let host = Host::default();
    let p = Script::new(|n, _| bash_call(n));
    let b = NativeBackend::new(p.clone(), harness_tools::builtin(), steps(2));
    let mut t = turn(&host, dir.path());
    t.parent_session = Some(SessionId::new());
    let out = b.run_turn(t).await;
    assert_eq!(out.reason, TurnEndReason::Error);
    let seen = p.seen();
    assert_eq!(seen[1].tools.len(), 1, "finish alone");
}

/// A call left without a result by a restart is not reported as "did not
/// run": its outcome is unknown, and the model is told to check first.
#[test]
fn a_call_cut_off_by_a_restart_reads_as_unknown() {
    let sid = SessionId::new();
    let ev = |seq, k: EventKind| {
        let (kind, payload) = k.into_parts();
        RawEvent {
            session_id: sid,
            seq,
            ts_ms: 0,
            kind,
            payload,
        }
    };
    let history = vec![
        ev(
            1,
            EventKind::UserMessage(UserMessage {
                turn_id: "t1".into(),
                text: "deploy".into(),
                attachments: vec![],
                reply: None,
                images: vec![],
                queued: false,
            }),
        ),
        ev(
            2,
            EventKind::AssistantMessage(AssistantMessage {
                author: None,
                variant: None,
                turn_id: "t1".into(),
                message_id: "m1".into(),
                text: String::new(),
                tool_calls: vec![ToolCallRef {
                    call_id: "c1".into(),
                    tool: "bash".into(),
                    input: json!({"command": "./deploy.sh"}),
                }],
                blocks: vec![],
            }),
        ),
    ];
    let msgs = history_to_messages(&history);
    let Some(ChatMsg::Tool { call_id, content }) = msgs.last() else {
        panic!("{msgs:?}")
    };
    assert_eq!(call_id, "c1");
    assert!(
        content.contains("may have run fully, partly or not at all"),
        "{content}"
    );
    assert!(content.contains("Check the current state"), "{content}");
    assert!(!content.contains("[no result: the turn was interrupted]"));
}

/// The node's limit reaches every request the loop sends.
#[tokio::test]
async fn the_idle_timeout_is_on_every_request() {
    let dir = tempfile::tempdir().unwrap();
    let host = Host::default();
    let p = Script::new(|_, _| said("ok"));
    let b = NativeBackend::new(
        p.clone(),
        harness_tools::builtin(),
        NativeConfig {
            model_idle_timeout: Duration::from_secs(42),
            ..Default::default()
        },
    );
    b.run_turn(turn(&host, dir.path())).await;
    assert_eq!(p.seen()[0].idle_timeout, Some(Duration::from_secs(42)));
    assert_eq!(
        NativeConfig::default().model_idle_timeout,
        Duration::from_secs(harness_core::policy::DEFAULT_MODEL_IDLE_S)
    );
    assert_eq!(harness_core::policy::DEFAULT_MODEL_IDLE_S, 300);
}

/// A server that answers the headers and one chunk, then sends nothing
/// and keeps the connection open (`stall_after_head`), or never answers
/// at all.
async fn stalling(stall_after_head: bool) -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 64 * 1024];
                let _ = s.read(&mut buf).await;
                if stall_after_head {
                    let chunk = r#"data: {"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"role":"assistant","content":"par"},"finish_reason":null}]}"#;
                    let body = format!("{chunk}\n\n");
                    let head = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n{:x}\r\n{body}\r\n",
                        body.len()
                    );
                    let _ = s.write_all(head.as_bytes()).await;
                    let _ = s.flush().await;
                }
                tokio::time::sleep(Duration::from_secs(60)).await;
                drop(s);
            });
        }
    });
    format!("http://{addr}/v1")
}

fn idle_request(idle: Duration) -> ChatRequest {
    ChatRequest {
        model: "m".into(),
        messages: vec![ChatMsg::User("hi".into())],
        tools: vec![],
        effort: None,
        cache: false,
        idle_timeout: Some(idle),
    }
}

/// A provider that stops sending mid-stream ends the call with the
/// retryable stream error (the loop's retry and fallback rules then
/// apply, and a partial answer is never repeated), instead of hanging the
/// turn until the person presses Stop.
#[tokio::test]
async fn a_stream_that_goes_quiet_fails_as_a_broken_stream() {
    let base = stalling(true).await;
    let p = LlmProvider::new(ApiKind::Openai, &base, None).unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let t = Instant::now();
    let r = p.complete(&idle_request(Duration::from_secs(1)), tx).await;
    assert!(t.elapsed() < Duration::from_secs(10), "{:?}", t.elapsed());
    assert_eq!(rx.recv().await.as_deref(), Some("par"), "the chunk came");
    match r {
        Err(e @ ProviderError::Stream(_)) => {
            assert!(e.is_retryable());
            assert!(e.to_string().contains("model_idle_timeout_s"), "{e}");
        }
        other => panic!("{other:?}"),
    }
}

/// No answer at all (not even the headers) within the limit: the provider
/// cannot be reached, which a fallback model may answer.
#[tokio::test]
async fn a_provider_that_never_answers_fails_as_unreachable() {
    let base = stalling(false).await;
    let p = LlmProvider::new(ApiKind::Openai, &base, None).unwrap();
    let (tx, _rx) = mpsc::unbounded_channel();
    let t = Instant::now();
    let r = p.complete(&idle_request(Duration::from_secs(1)), tx).await;
    assert!(t.elapsed() < Duration::from_secs(10), "{:?}", t.elapsed());
    match r {
        Err(e @ ProviderError::Http(_)) => {
            assert!(e.is_retryable() && e.is_model_level());
            assert!(e.to_string().contains("model_idle_timeout_s"), "{e}");
        }
        other => panic!("{other:?}"),
    }
}

const ENV: &str = "[harness] Environment of this turn: test line.";

/// The node's environment line opens a person's turn's message, ahead of
/// the per-turn context, and is not in the system prompt (cache prefix).
#[tokio::test]
async fn the_environment_line_opens_the_turn() {
    let dir = tempfile::tempdir().unwrap();
    let host = Host::default();
    let p = Script::new(|_, _| said("ok"));
    let b = NativeBackend::new(p.clone(), harness_tools::builtin(), NativeConfig::default());
    let mut t = turn(&host, dir.path());
    t.extra.environment = Some(ENV.into());
    t.extra.context = Some("project facts".into());
    b.run_turn(t).await;
    let req = &p.seen()[0];
    let Some(ChatMsg::System(sys)) = req.messages.first() else {
        panic!()
    };
    assert!(!sys.contains(ENV), "not in the system prompt");
    let user = req.messages.last().and_then(|m| m.user_text()).unwrap();
    assert!(user.starts_with(&format!("{ENV}\n\n")), "{user}");
    assert!(user.ends_with("\n\ngo"), "{user}");
    assert!(user.contains("project facts"));
}

/// A sub-agent's task gets it too.
#[tokio::test]
async fn a_sub_agent_gets_the_environment_line() {
    let dir = tempfile::tempdir().unwrap();
    let host = Host::default();
    let p = Script::new(|_, _| said("ok"));
    let b = NativeBackend::new(p.clone(), harness_tools::builtin(), NativeConfig::default());
    let mut t = turn(&host, dir.path());
    t.parent_session = Some(SessionId::new());
    t.extra.environment = Some(ENV.into());
    b.run_turn(t).await;
    let req = &p.seen()[0];
    let user = req.messages.last().and_then(|m| m.user_text()).unwrap();
    assert!(user.starts_with(&format!("{ENV}\n\n")), "{user}");
}
