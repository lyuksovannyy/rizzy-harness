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
    ProviderError, RetryPolicy, StreamLimits, history_to_messages,
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

fn turn<'a>(host: &'a dyn TurnHost, cwd: &std::path::Path) -> TurnCtx<'a> {
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
            ChatMsg::Note(t)
            | ChatMsg::CallNote(t)
            | ChatMsg::Summary(t)
            | ChatMsg::StepContext(t) => t.clone(),
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
    // A model mid-task: it calls a tool whenever it may.
    let p = Script::new(|n, req| {
        if req.no_tool_calls {
            said("Done: three checks. Left: the rest.")
        } else {
            bash_call(n)
        }
    });
    let b = NativeBackend::new(p.clone(), harness_tools::builtin(), steps(3));
    let out = b.run_turn(turn(&host, dir.path())).await;
    let seen = p.seen();
    assert_eq!(seen.len(), 4, "three steps, then the wrap-up request");
    let last = text_of(&seen[3]);
    assert!(
        last.contains("[harness] This turn has used its 3 steps"),
        "{last}"
    );
    assert!(seen[3].no_tool_calls, "tool_choice none on the wrap-up");
    assert!(
        seen[..3]
            .iter()
            .all(|r| !r.no_tool_calls && !text_of(r).contains("its 3 steps")),
        "only the last"
    );
    assert_eq!(
        seen[3].tools.len(),
        seen[0].tools.len(),
        "the tool list stays (prompt cache)"
    );
    assert_eq!(out.reason, TurnEndReason::Limit);
    assert_eq!(out.error, None, "not an error");
    assert_eq!(host.finished().len(), 3, "all three steps' calls ran");
    let account = host
        .0
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find_map(|e| match e {
            EventKind::AssistantMessage(m) => Some(m.text.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        account, "Done: three checks. Left: the rest.",
        "the account"
    );
}

/// `max_steps = 1` still runs a tool: the wrap-up is a request of its own
/// after the budget (review L6).
#[tokio::test]
async fn one_step_still_runs_a_tool() {
    let dir = tempfile::tempdir().unwrap();
    let host = Host::default();
    let p = Script::new(|n, req| {
        if req.no_tool_calls {
            said("ok")
        } else {
            bash_call(n)
        }
    });
    let b = NativeBackend::new(p.clone(), harness_tools::builtin(), steps(1));
    let out = b.run_turn(turn(&host, dir.path())).await;
    assert_eq!(p.seen().len(), 2);
    assert!(host.finished()[0].ok, "{}", host.finished()[0].output);
    assert_eq!(out.reason, TurnEndReason::Limit);
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
        if n < 3 {
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
                files: vec![],
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

/// The node's limits reach every request the loop sends.
#[tokio::test]
async fn the_stream_limits_are_on_every_request() {
    let dir = tempfile::tempdir().unwrap();
    let host = Host::default();
    let p = Script::new(|_, _| said("ok"));
    let limits = StreamLimits {
        first_byte: Duration::from_secs(42),
        idle: Duration::from_secs(7),
    };
    let b = NativeBackend::new(
        p.clone(),
        harness_tools::builtin(),
        NativeConfig {
            stream_limits: limits,
            ..Default::default()
        },
    );
    b.run_turn(turn(&host, dir.path())).await;
    assert_eq!(p.seen()[0].limits, Some(limits));
    assert!(!p.seen()[0].no_tool_calls);
    assert_eq!(
        StreamLimits::default(),
        StreamLimits {
            first_byte: Duration::from_secs(600),
            idle: Duration::from_secs(300),
        }
    );
}

/// How far a stalling server goes before it sends nothing more.
#[derive(Clone, Copy)]
enum Stall {
    /// No response at all.
    Nothing,
    /// The headers, then nothing.
    Headers,
    /// The headers and one chunk, then nothing.
    Chunk,
}

/// A server that stalls as `how` says and keeps the connection open.
async fn stalling(how: Stall) -> String {
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
                let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n";
                let out = match how {
                    Stall::Nothing => String::new(),
                    Stall::Headers => head.to_string(),
                    Stall::Chunk => {
                        let chunk = r#"data: {"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"role":"assistant","content":"par"},"finish_reason":null}]}"#;
                        let body = format!("{chunk}\n\n");
                        format!("{head}{:x}\r\n{body}\r\n", body.len())
                    }
                };
                let _ = s.write_all(out.as_bytes()).await;
                let _ = s.flush().await;
                tokio::time::sleep(Duration::from_secs(60)).await;
                drop(s);
            });
        }
    });
    format!("http://{addr}/v1")
}

fn limited(first_byte: u64, idle: u64) -> ChatRequest {
    ChatRequest {
        model: "m".into(),
        messages: vec![ChatMsg::User("hi".into())],
        tools: vec![],
        effort: None,
        cache: false,
        limits: Some(StreamLimits {
            first_byte: Duration::from_secs(first_byte),
            idle: Duration::from_secs(idle),
        }),
        no_tool_calls: false,
        max_tokens: None,
    }
}

async fn silent_after(how: Stall, req: ChatRequest) -> (ProviderError, Duration, Vec<String>) {
    let base = stalling(how).await;
    let p = LlmProvider::new(ApiKind::Openai, &base, None).unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let t = Instant::now();
    let r = p.complete(&req, tx).await;
    let took = t.elapsed();
    let mut shown = vec![];
    while let Ok(d) = rx.try_recv() {
        shown.push(d);
    }
    match r {
        Err(e) => (e, took, shown),
        Ok(c) => panic!("{c:?}"),
    }
}

/// No answer at all, the headers and then nothing, or a stream that goes
/// quiet: each ends as `Silent` (retryable, another model may answer),
/// naming both keys, instead of hanging the turn until Stop. The first
/// answer gets the longer limit.
#[tokio::test]
async fn a_silent_provider_ends_the_call_within_its_limits() {
    for how in [Stall::Nothing, Stall::Headers] {
        let (e, took, shown) = silent_after(how, limited(1, 30)).await;
        assert!(took < Duration::from_secs(10), "{took:?}");
        assert!(matches!(e, ProviderError::Silent(_)), "{e:?}");
        assert!(e.is_retryable() && e.is_model_level());
        let m = e.to_string();
        assert!(
            m.contains("model_first_byte_timeout_s") && m.contains("model_idle_timeout_s"),
            "{m}"
        );
        assert!(shown.is_empty());
    }
    // After the first chunk, the shorter gap applies.
    let (e, took, shown) = silent_after(Stall::Chunk, limited(30, 1)).await;
    assert!(took < Duration::from_secs(10), "{took:?}");
    assert_eq!(shown, ["par"], "the chunk came");
    assert!(e.to_string().contains("sent nothing more for 1 s"), "{e}");
}

/// Fails silent every time; shows `shown` first when given.
struct Quiet {
    calls: Mutex<usize>,
    shown: Option<&'static str>,
}

#[async_trait::async_trait]
impl Provider for Quiet {
    fn name(&self) -> &'static str {
        "quiet"
    }
    async fn complete(
        &self,
        _: &ChatRequest,
        tx: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        *self.calls.lock().unwrap() += 1;
        if let Some(s) = self.shown {
            let _ = tx.send(s.to_string());
        }
        Err(ProviderError::Silent("quiet".into()))
    }
}

/// A silence that showed nothing is tried once more, not three times (a
/// whole timeout each); one after visible text is not tried again.
#[tokio::test]
async fn silence_is_retried_at_most_once_and_never_after_text() {
    for (shown, want) in [(None, 2), (Some("half an ans"), 1)] {
        let dir = tempfile::tempdir().unwrap();
        let host = Host::default();
        let p = Arc::new(Quiet {
            calls: Mutex::new(0),
            shown,
        });
        let b = NativeBackend::new(
            p.clone(),
            harness_tools::builtin(),
            NativeConfig {
                retry: RetryPolicy {
                    retries: 3,
                    base: Duration::from_millis(1),
                    max: Duration::from_millis(5),
                },
                ..Default::default()
            },
        );
        let out = b.run_turn(turn(&host, dir.path())).await;
        assert_eq!(out.reason, TurnEndReason::Error);
        assert_eq!(*p.calls.lock().unwrap(), want, "{shown:?}");
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

/// The provider's prompt cache across turns: the next turn's first request
/// starts with this turn's last request byte for byte, environment line,
/// half-budget note and wrap-up note included (they are in the log:
/// `turn_started.environment`, `tool_call_finished.note`).
#[tokio::test]
async fn the_next_turn_rebuilds_this_turns_requests_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let host = Host::default();
    let p = Script::new(|n, req| {
        if req.no_tool_calls {
            said("Done: four checks.")
        } else {
            bash_call(n)
        }
    });
    let b = NativeBackend::new(p.clone(), harness_tools::builtin(), steps(4));
    let sid = SessionId::new();
    let mut t1 = turn(&host, dir.path());
    t1.session_id = sid;
    t1.turn_id = "t1".into();
    t1.extra.environment = Some("[harness] Environment of this turn: one.".into());
    assert_eq!(b.run_turn(t1).await.reason, TurnEndReason::Limit);
    let first = p.seen();
    let last = first.last().unwrap().clone();
    assert!(text_of(&last).contains("[harness] You have used 2 of the 4 steps"));
    assert!(text_of(&last).contains("[harness] This turn has used its 4 steps"));
    // The log as the node writes it.
    let mut kinds = vec![
        EventKind::TurnStarted(TurnStarted {
            turn_id: "t1".into(),
            agent: None,
            cause: None,
            environment: Some("[harness] Environment of this turn: one.".into()),
            retry: None,
        }),
        EventKind::UserMessage(UserMessage {
            turn_id: "t1".into(),
            text: "go".into(),
            attachments: vec![],
            reply: None,
            images: vec![],
            queued: false,
            files: vec![],
        }),
    ];
    kinds.extend(host.0.lock().unwrap().iter().cloned());
    kinds.push(EventKind::TurnEnded(TurnEnded {
        turn_id: "t1".into(),
        reason: TurnEndReason::Limit,
        usage: None,
    }));
    let history: Vec<RawEvent> = kinds
        .into_iter()
        .enumerate()
        .map(|(i, k)| {
            let (kind, payload) = k.into_parts();
            RawEvent {
                session_id: sid,
                seq: i as u64 + 1,
                ts_ms: 0,
                kind,
                payload,
            }
        })
        .collect();
    let host2 = Host::default();
    let mut t2 = turn(&host2, dir.path());
    t2.session_id = sid;
    t2.turn_id = "t2".into();
    t2.history = history;
    t2.user_text = "carry on".into();
    t2.extra.environment = Some("[harness] Environment of this turn: two.".into());
    b.run_turn(t2).await;
    let next = &p.seen()[first.len()];
    assert!(
        next.messages.len() > last.messages.len(),
        "{:#?}",
        next.messages
    );
    assert_eq!(
        next.messages[..last.messages.len()],
        last.messages[..],
        "the cached prefix moved"
    );
}

/// A host that says the chat's step limit (`TurnHost::step_limit`, X.14).
#[derive(Default)]
struct Limited(Mutex<Vec<EventKind>>, usize);

#[async_trait::async_trait]
impl TurnHost for Limited {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        self.0.lock().unwrap().push(kind);
        Ok(())
    }
    async fn approve(&self, _: &str, _: &str, _: &Value, _: ToolRisk) -> Decision {
        Decision::Allow
    }
    fn step_limit(&self) -> Option<usize> {
        Some(self.1)
    }
}

/// A person's turn follows its chat's limit, below the node's: its steps,
/// then the wrap-up, whose note names the chat's number.
#[tokio::test]
async fn a_turn_follows_its_chats_step_limit() {
    let dir = tempfile::tempdir().unwrap();
    let host = Limited(Mutex::default(), 2);
    let p = Script::new(|n, req| {
        if req.no_tool_calls {
            said("done")
        } else {
            bash_call(n)
        }
    });
    let b = NativeBackend::new(p.clone(), harness_tools::builtin(), steps(50));
    let out = b.run_turn(turn(&host, dir.path())).await;
    assert_eq!(out.reason, TurnEndReason::Limit);
    let seen = p.seen();
    assert_eq!(seen.len(), 3, "two steps, then the wrap-up");
    assert!(text_of(&seen[2]).contains("This turn has used its 2 steps"));
}

/// A sub-agent's cap is at most its parent chat's limit (the host says the
/// parent's): its last request is `finish` alone at that number.
#[tokio::test]
async fn a_sub_agents_cap_follows_its_parent_chats_limit() {
    let dir = tempfile::tempdir().unwrap();
    let host = Limited(Mutex::default(), 2);
    let p = Script::new(|n, _| bash_call(n));
    let b = NativeBackend::new(p.clone(), harness_tools::builtin(), steps(50));
    let mut t = turn(&host, dir.path());
    t.parent_session = Some(SessionId::new());
    let out = b.run_turn(t).await;
    assert_eq!(out.reason, TurnEndReason::Error);
    let seen = p.seen();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[1].tools.len(), 1, "finish alone");
}
