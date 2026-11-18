//! The model's thinking in the loop (X.25, PLAN.md T73): recorded with
//! the step through the session's one writer, sent back only to the model
//! that wrote it, scrubbed or kept back, and a provider that refuses it
//! does not end the chat. Every request rebuilds byte for byte from the
//! log, the thinking included.

use std::sync::{Arc, Mutex};

use harness_agent_native::llm::{ModelRouter, Route};
use harness_agent_native::{
    ChatMsg, ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError,
    Rebuild, RetryHint, place_notes, rebuild,
};
use harness_core::agent::{AgentSpec, ToolSelection};
use harness_core::backend::{AgentBackend, LogError, TurnCtx, TurnExtra, TurnHost, TurnOutcome};
use harness_core::event::*;
use harness_core::policy::{Decision, ToolRisk};
use harness_core::{RawEvent, Secret, SecretSet, SessionId};
use serde_json::{Value, json};
use tokio::sync::mpsc;

#[derive(Default)]
struct Host {
    sid: SessionId,
    log: Mutex<Vec<RawEvent>>,
}

impl Host {
    fn push(&self, k: EventKind) {
        let mut log = self.log.lock().unwrap();
        let seq = log.last().map_or(1, |e| e.seq + 1);
        let (kind, payload) = k.into_parts();
        log.push(RawEvent {
            session_id: self.sid,
            seq,
            ts_ms: 0,
            kind,
            payload,
        });
    }
    fn log(&self) -> Vec<RawEvent> {
        self.log.lock().unwrap().clone()
    }
    fn kinds(&self, kind: &str) -> Vec<Value> {
        self.log()
            .into_iter()
            .filter(|e| e.kind == kind)
            .map(|e| e.payload)
            .collect()
    }
}

#[async_trait::async_trait]
impl TurnHost for Host {
    async fn emit(&self, kind: EventKind) -> Result<(), LogError> {
        self.push(kind);
        Ok(())
    }
    async fn approve(&self, _: &str, _: &str, _: &Value, _: ToolRisk) -> Decision {
        Decision::Allow
    }
    fn runs_unasked(&self, _: &str, risk: ToolRisk) -> bool {
        risk == ToolRisk::ReadOnly
    }
}

type Answer = Box<dyn Fn(&ChatRequest, usize) -> Result<Completion, ProviderError> + Send + Sync>;

/// A provider driven by a closure, keeping every request. It sends back
/// thinking as a real one would: any step's that reaches it.
struct Model {
    answer: Answer,
    seen: Mutex<Vec<ChatRequest>>,
}

impl Model {
    fn new(answer: Answer) -> Arc<Self> {
        Arc::new(Self {
            answer,
            seen: Mutex::default(),
        })
    }
    fn requests(&self) -> Vec<ChatRequest> {
        self.seen.lock().unwrap().clone()
    }
}

fn carries(req: &ChatRequest) -> bool {
    req.messages.iter().any(|m| {
        matches!(
            m,
            ChatMsg::Assistant {
                thinking: Some(t),
                ..
            } if !t.blocks.is_empty()
        )
    })
}

#[async_trait::async_trait]
impl Provider for Model {
    fn name(&self) -> &'static str {
        "model"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        _: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        let n = {
            let mut s = self.seen.lock().unwrap();
            s.push(req.clone());
            s.len() - 1
        };
        (self.answer)(req, n)
    }
    fn replays_thinking(&self, req: &ChatRequest) -> bool {
        carries(req)
    }
}

fn thought(text: &str) -> Option<Thinking> {
    Some(Thinking {
        model: "set by the loop".into(),
        blocks: vec![ThinkingBlock::Text { text: text.into() }],
        prefix: String::new(),
        unsent: false,
    })
}

fn read(id: &str, why: &str) -> Completion {
    Completion {
        tool_calls: vec![ToolCallRef {
            call_id: id.into(),
            tool: "read".into(),
            input: json!({"path": "notes.txt"}),
        }],
        thinking: thought(why),
        ..Default::default()
    }
}

fn answer(t: &str, why: &str) -> Completion {
    Completion {
        text: t.into(),
        thinking: thought(why),
        ..Default::default()
    }
}

fn refused() -> ProviderError {
    ProviderError::Status {
        status: 400,
        body: r#"{"type":"error","error":{"type":"invalid_request_error","message":"messages.1.content.0: thinking blocks cannot be modified"}}"#.into(),
        hint: RetryHint::default(),
    }
}

fn unavailable() -> ProviderError {
    ProviderError::Status {
        status: 503,
        body: r#"{"error":{"message":"overloaded"}}"#.into(),
        hint: RetryHint {
            after: None,
            no_retry: true,
        },
    }
}

fn agent(fallbacks: &[&str]) -> AgentSpec {
    AgentSpec {
        agent_id: "dev".into(),
        name: "Dev".into(),
        system_prompt: "Be brief.".into(),
        tools: ToolSelection::List(vec!["read".into()]),
        model: None,
        fallbacks: fallbacks.iter().map(|s| s.to_string()).collect(),
        unavailable: vec![],
        no_effort: vec![],
        vision: vec![],
        context_windows: Default::default(),
        effort: None,
        ui: Default::default(),
        prompt_hash: "h".into(),
        deny_tools: vec![],
        deny_skills: vec![],
        deny_mcp_servers: vec![],
        delegates: vec![],
        caps: None,
        no_personalisation: false,
    }
}

fn backend(p: Arc<dyn Provider>, secrets: &[&str]) -> NativeBackend {
    NativeBackend::new(
        p,
        vec![Arc::new(harness_tools::Read)],
        NativeConfig {
            secrets: SecretSet::new(secrets.iter().map(|s| Secret::new(s.to_string())).collect()),
            ..Default::default()
        },
    )
}

struct Chat<'a> {
    b: &'a NativeBackend,
    host: &'a Host,
    ws: &'a std::path::Path,
    agent: AgentSpec,
    model: &'a str,
}

impl Chat<'_> {
    async fn run(&self, turn_id: &str, text: &str, retry: Option<RetryOf>) -> TurnOutcome {
        let history = self.host.log();
        self.host.push(EventKind::TurnStarted(TurnStarted {
            continued_by: None,
            retry: retry.clone(),
            cause: None,
            agent: None,
            turn_id: turn_id.into(),
            environment: None,
        }));
        if retry.is_none() {
            self.host.push(EventKind::UserMessage(UserMessage {
                turn_id: turn_id.into(),
                text: text.into(),
                attachments: vec![],
                reply: None,
                images: vec![],
                queued: false,
                files: vec![],
            }));
        }
        let out = self
            .b
            .run_turn(TurnCtx {
                session_id: self.host.sid,
                turn_id: turn_id.into(),
                history,
                user_text: text.into(),
                cwd: self.ws.to_path_buf(),
                model: self.model.into(),
                agent: Some(self.agent.clone()),
                compare: vec![],
                parent_session: None,
                extra: TurnExtra {
                    retry,
                    ..Default::default()
                },
                cancel: Default::default(),
                host: self.host,
            })
            .await;
        if let Some(e) = &out.error {
            self.host.push(EventKind::Error(ErrorEvent {
                turn_id: Some(turn_id.into()),
                message: e.clone(),
            }));
        }
        self.host.push(EventKind::TurnEnded(TurnEnded {
            turn_id: turn_id.into(),
            reason: out.reason,
            usage: out.usage.clone(),
        }));
        out
    }
}

/// What a request built from `log` sends (less the system prompt).
fn rebuilt(log: &[RawEvent]) -> Vec<ChatMsg> {
    let mut m = vec![ChatMsg::System("x".into())];
    m.extend(rebuild(
        log,
        &Rebuild {
            blocks: true,
            agent: Some("dev"),
            ..Default::default()
        },
    ));
    place_notes(m).split_off(1)
}

/// Every answered request equals what the log rebuilds right before its
/// answer (`answered`: the requests that got one, in order).
fn rebuilds_at_every_step(log: &[RawEvent], answered: &[ChatRequest]) {
    let cuts: Vec<usize> = log
        .iter()
        .enumerate()
        .filter(|(_, e)| e.kind == "assistant_message")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(cuts.len(), answered.len(), "one answer a request");
    for (k, (cut, req)) in cuts.iter().zip(answered).enumerate() {
        assert_eq!(
            rebuilt(&log[..*cut]),
            req.messages[1..],
            "request {k} differs from the log"
        );
    }
}

fn thinking_of(req: &ChatRequest) -> Vec<String> {
    req.messages
        .iter()
        .filter_map(|m| match m {
            ChatMsg::Assistant {
                thinking: Some(t), ..
            } if !t.blocks.is_empty() => Some(t.shown().0),
            _ => None,
        })
        .collect()
}

fn workspace() -> tempfile::TempDir {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("notes.txt"), "hello\n").unwrap();
    ws
}

/// Recorded with its step, under the model that wrote it, and sent back
/// in the next request and the next turn; a restart (a fresh backend over
/// the same log) sends the same.
#[tokio::test]
async fn thinking_is_recorded_and_sent_back_from_the_log() {
    let ws = workspace();
    let p = Model::new(Box::new(|_, n| {
        Ok(match n {
            0 => read("a", "read first"),
            1 => answer("hello", "it says hello"),
            _ => answer("again", "second turn"),
        })
    }));
    let host = Host::default();
    let b = backend(p.clone(), &[]);
    let chat = Chat {
        b: &b,
        host: &host,
        ws: ws.path(),
        agent: agent(&[]),
        model: "m",
    };
    assert_eq!(
        chat.run("t1", "what is in notes?", None).await.reason,
        TurnEndReason::Completed
    );
    let msgs = host.kinds("assistant_message");
    assert_eq!(msgs[0]["thinking"]["model"], "m");
    assert_eq!(msgs[0]["thinking"]["blocks"][0]["text"], "read first");
    // A restart: a new backend, the same log.
    let b2 = backend(p.clone(), &[]);
    let chat2 = Chat { b: &b2, ..chat };
    assert_eq!(
        chat2.run("t2", "and now?", None).await.reason,
        TurnEndReason::Completed
    );
    let r = p.requests();
    assert_eq!(thinking_of(&r[0]), Vec::<String>::new());
    assert_eq!(thinking_of(&r[1]), vec!["read first"]);
    assert_eq!(thinking_of(&r[2]), vec!["read first", "it says hello"]);
    rebuilds_at_every_step(&host.log(), &r);
}

/// T73: the provider refuses any request that sends thinking back. The
/// step is sent once more without it, that is answered, one record names
/// the model, and no later request (the rest of the turn, the next turn)
/// carries that model's thinking; each still rebuilds from the log.
#[tokio::test]
async fn a_refused_replay_is_resent_without_and_the_chat_goes_on() {
    let ws = workspace();
    let p = Model::new(Box::new(|req, n| {
        if carries(req) {
            return Err(refused());
        }
        Ok(match n {
            0 => read("a", "read first"),
            2 => read("b", "read again"),
            _ => answer("done", "all read"),
        })
    }));
    let host = Host::default();
    let b = backend(p.clone(), &[]);
    let chat = Chat {
        b: &b,
        host: &host,
        ws: ws.path(),
        agent: agent(&[]),
        model: "m",
    };
    let o = chat.run("t1", "read it", None).await;
    assert_eq!(o.reason, TurnEndReason::Completed, "{:?}", o.error);
    let w = host.kinds("thinking_withheld");
    assert_eq!(w.len(), 1, "one record");
    assert_eq!(w[0]["model"], "m");
    assert_eq!(w[0]["status"], 400);
    assert!(
        w[0]["reason"]
            .as_str()
            .unwrap()
            .contains("cannot be modified")
    );
    // The steps after the record keep their thinking for the person, as
    // never to be sent.
    let msgs = host.kinds("assistant_message");
    assert_eq!(msgs[1]["thinking"]["unsent"], true, "{}", msgs[1]);
    assert!(msgs[0]["thinking"].get("unsent").is_none());
    assert_eq!(
        chat.run("t2", "more", None).await.reason,
        TurnEndReason::Completed
    );
    let r = p.requests();
    let answered: Vec<ChatRequest> = r.iter().filter(|q| !carries(q)).cloned().collect();
    assert_eq!(r.len() - answered.len(), 1, "one refused request");
    rebuilds_at_every_step(&host.log(), &answered);
}

/// A refusal that is not the thinking: the resend without it fails too,
/// so the original error stands and nothing is recorded; the person's
/// Retry replays the failed attempt's step with its thinking, and is
/// answered.
#[tokio::test]
async fn a_refusal_that_is_not_the_thinking_ends_the_turn_and_retry_recovers() {
    let ws = workspace();
    let fail = Arc::new(Mutex::new(true));
    let f2 = fail.clone();
    let p = Model::new(Box::new(move |_, n| {
        if n == 0 {
            return Ok(read("a", "read first"));
        }
        if *f2.lock().unwrap() {
            return Err(ProviderError::Status {
                status: 400,
                body: r#"{"error":{"message":"bad tool schema"}}"#.into(),
                hint: RetryHint::default(),
            });
        }
        Ok(answer("done", "recovered"))
    }));
    let host = Host::default();
    let b = backend(p.clone(), &[]);
    let chat = Chat {
        b: &b,
        host: &host,
        ws: ws.path(),
        agent: agent(&[]),
        model: "m",
    };
    let o = chat.run("t1", "read it", None).await;
    assert_eq!(o.reason, TurnEndReason::Error);
    assert!(
        o.error.unwrap().contains("bad tool schema"),
        "the original error"
    );
    assert!(host.kinds("thinking_withheld").is_empty());
    let r = p.requests();
    assert_eq!(r.len(), 3, "the step, its resend without thinking");
    assert!(carries(&r[1]) && !carries(&r[2]));
    *fail.lock().unwrap() = false;
    let o = chat
        .run(
            "t2",
            "",
            Some(RetryOf {
                turn_id: "t1".into(),
                reason: TurnEndReason::Error,
                error: None,
                failed_model: None,
                model: None,
            }),
        )
        .await;
    assert_eq!(o.reason, TurnEndReason::Completed, "{:?}", o.error);
    let r = p.requests();
    assert_eq!(thinking_of(r.last().unwrap()), vec!["read first"]);
    let answered = vec![r[0].clone(), r.last().unwrap().clone()];
    rebuilds_at_every_step(&host.log(), &answered);
}

/// The scrubber changes a step's plain reasoning: kept and sent back
/// scrubbed (plain text is not verified); the secret never leaves.
#[tokio::test]
async fn scrubbed_plain_thinking_is_sent_back_scrubbed() {
    let ws = workspace();
    let secret = "sk-live-0123456789abcdef";
    let p = Model::new(Box::new(move |_, n| {
        Ok(match n {
            0 => read("a", &format!("the key is {secret}")),
            1 => read("b", "clean"),
            _ => answer("done", "end"),
        })
    }));
    let host = Host::default();
    let b = backend(p.clone(), &[secret]);
    let chat = Chat {
        b: &b,
        host: &host,
        ws: ws.path(),
        agent: agent(&[]),
        model: "m",
    };
    assert_eq!(
        chat.run("t1", "go", None).await.reason,
        TurnEndReason::Completed
    );
    let msgs = host.kinds("assistant_message");
    assert_eq!(
        msgs[0]["thinking"]["blocks"][0]["text"],
        "the key is [REDACTED]"
    );
    assert!(msgs[0]["thinking"].get("unsent").is_none());
    let r = p.requests();
    assert!(
        !format!("{r:?}").contains(secret),
        "the secret was sent back"
    );
    assert_eq!(thinking_of(&r[2]), vec!["the key is [REDACTED]", "clean"]);
    rebuilds_at_every_step(&host.log(), &r);
}

/// The scrubber changes a step's signed block: kept scrubbed for the
/// person and never sent (the provider refuses a changed block), and no
/// later signed block is sent either (the gap rule).
#[tokio::test]
async fn a_scrubbed_signed_block_is_never_sent_nor_any_after_it() {
    let ws = workspace();
    let secret = "sk-live-0123456789abcdef";
    let signed = |text: &str| {
        Some(Thinking {
            model: "set by the loop".into(),
            blocks: vec![ThinkingBlock::Signed {
                text: text.into(),
                signature: "SIG".into(),
            }],
            prefix: "p".into(),
            unsent: false,
        })
    };
    let p = Model::new(Box::new(move |_, n| {
        let mut c = match n {
            0 => read("a", ""),
            1 => read("b", ""),
            2 => read("c", ""),
            _ => answer("done", ""),
        };
        c.thinking = match n {
            0 => signed("first"),
            1 => signed(&format!("the key is {secret}")),
            _ => signed("later"),
        };
        Ok(c)
    }));
    let host = Host::default();
    let b = backend(p.clone(), &[secret]);
    let chat = Chat {
        b: &b,
        host: &host,
        ws: ws.path(),
        agent: agent(&[]),
        model: "m",
    };
    assert_eq!(
        chat.run("t1", "go", None).await.reason,
        TurnEndReason::Completed
    );
    let msgs = host.kinds("assistant_message");
    assert_eq!(msgs[1]["thinking"]["unsent"], true);
    assert_eq!(
        msgs[1]["thinking"]["blocks"][0]["text"],
        "the key is [REDACTED]"
    );
    let r = p.requests();
    assert!(!format!("{r:?}").contains(secret));
    // Request 3 holds step 1's blocks only: step 2's are a marker, and
    // step 3's come after the gap.
    assert_eq!(thinking_of(&r[3]), vec!["first"]);
    rebuilds_at_every_step(&host.log(), &r);
}

/// A fallback to another provider's model (the first one fails before it
/// answers the second turn): none of the first model's thinking reaches
/// it, and its own goes back only to it.
#[tokio::test]
async fn a_fallback_to_another_provider_gets_none_of_the_first_models_thinking() {
    let ws = workspace();
    let p = Model::new(Box::new(|req, n| {
        if req.model == "one" && n > 1 {
            return Err(unavailable());
        }
        Ok(match n {
            0 => read("a", "by one"),
            1 => answer("hello", "one again"),
            // 2 is the first model's failed request of the second turn.
            3 => read("b", "by two"),
            _ => answer("done", "two again"),
        })
    }));
    let router = Arc::new(ModelRouter::new(vec![
        Route {
            prefix: "a".into(),
            provider: p.clone(),
        },
        Route {
            prefix: "b".into(),
            provider: p.clone(),
        },
    ]));
    let host = Host::default();
    let b = backend(router, &[]);
    let chat = Chat {
        b: &b,
        host: &host,
        ws: ws.path(),
        agent: agent(&["b::two"]),
        model: "a::one",
    };
    assert_eq!(
        chat.run("t1", "go", None).await.reason,
        TurnEndReason::Completed
    );
    let o = chat.run("t2", "again", None).await;
    assert_eq!(o.reason, TurnEndReason::Completed, "{:?}", o.error);
    let msgs = host.kinds("assistant_message");
    assert_eq!(msgs[0]["thinking"]["model"], "a::one");
    assert_eq!(msgs[2]["thinking"]["model"], "b::two");
    let r = p.requests();
    // The first model still gets its own thinking in the second turn.
    assert_eq!(thinking_of(&r[2]), vec!["by one", "one again"]);
    let to_two: Vec<&ChatRequest> = r.iter().filter(|q| q.model == "two").collect();
    assert_eq!(thinking_of(to_two[0]), Vec::<String>::new());
    assert_eq!(thinking_of(to_two[1]), vec!["by two"]);
}
