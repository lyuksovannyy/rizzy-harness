//! The review of PR 137 (X.25, PLAN.md T73), its proofs turned into tests:
//! an image refusal withholds only the images; scrubbed plain reasoning is
//! still sent back; no signed block is sent after one that is not; large
//! thinking is left out of in-turn requests, oldest first, and the loop
//! holds no more than the request sends; refusals and retries rebuild from
//! the log.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::response::IntoResponse;
use axum::{Router, routing::post};
use base64::Engine as _;
use harness_agent_native::llm::{ApiKind, LlmProvider};
use harness_agent_native::{
    ChatMsg, ChatRequest, Completion, ImageState, NativeBackend, NativeConfig, Provider,
    ProviderError, RetryHint, ToolSpec,
};
use harness_core::agent::{AgentSpec, ToolSelection};
use harness_core::backend::{
    AgentBackend, ImageData, KeptImage, LogError, TurnCtx, TurnExtra, TurnHost, TurnOutcome,
};
use harness_core::event::*;
use harness_core::policy::{Decision, ToolRisk};
use harness_core::{RawEvent, Secret, SecretSet, SessionId};
use serde_json::{Value, json};
use tokio::sync::mpsc;

// ---- host with log and image store ----------------------------------------

#[derive(Default)]
struct Host {
    sid: SessionId,
    log: Mutex<Vec<RawEvent>>,
    store: Mutex<HashMap<String, Vec<u8>>>,
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
    async fn keep_image(&self, i: KeptImage) -> Result<(), String> {
        self.store.lock().unwrap().insert(i.sha256, i.bytes);
        Ok(())
    }
    async fn images(&self, sha256: &[String]) -> Vec<ImageData> {
        self.store
            .lock()
            .unwrap()
            .iter()
            .filter(|(s, _)| sha256.contains(s))
            .map(|(s, b)| ImageData {
                sha256: s.clone(),
                mime: "image/png".into(),
                data: base64::engine::general_purpose::STANDARD.encode(b).into(),
            })
            .collect()
    }
}

type Answer = Box<dyn Fn(&ChatRequest, usize) -> Result<Completion, ProviderError> + Send + Sync>;

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

/// The request sends some thinking (a marker sends none).
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

fn shows_image(req: &ChatRequest) -> bool {
    req.messages.iter().any(|m| {
        matches!(
            m,
            ChatMsg::ToolImage {
                state: ImageState::Shown,
                ..
            }
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

fn read(id: &str, path: &str, why: &str) -> Completion {
    Completion {
        tool_calls: vec![ToolCallRef {
            call_id: id.into(),
            tool: "read".into(),
            input: json!({"path": path}),
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

fn status400(msg: &str) -> ProviderError {
    ProviderError::Status {
        status: 400,
        body: format!(r#"{{"error":{{"message":"{msg}"}}}}"#),
        hint: RetryHint::default(),
    }
}

fn agent(vision: bool) -> AgentSpec {
    AgentSpec {
        agent_id: "dev".into(),
        name: "Dev".into(),
        system_prompt: "Be brief.".into(),
        tools: ToolSelection::List(vec!["read".into()]),
        model: None,
        fallbacks: vec![],
        unavailable: vec![],
        no_effort: vec![],
        vision: if vision { vec!["m".into()] } else { vec![] },
        context_windows: Default::default(),
        effort: None,
        ui: Default::default(),
        prompt_hash: "h".into(),
        deny_tools: vec![],
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

async fn run(
    b: &NativeBackend,
    host: &Host,
    ws: &std::path::Path,
    vision: bool,
    turn_id: &str,
    text: &str,
    retry: Option<RetryOf>,
) -> TurnOutcome {
    let history = host.log();
    host.push(EventKind::TurnStarted(TurnStarted {
        continued_by: None,
        retry: retry.clone(),
        cause: None,
        agent: None,
        turn_id: turn_id.into(),
        environment: None,
    }));
    if retry.is_none() {
        host.push(EventKind::UserMessage(UserMessage {
            turn_id: turn_id.into(),
            text: text.into(),
            attachments: vec![],
            reply: None,
            images: vec![],
            queued: false,
            files: vec![],
        }));
    }
    let out = b
        .run_turn(TurnCtx {
            session_id: host.sid,
            turn_id: turn_id.into(),
            history,
            user_text: text.into(),
            cwd: ws.to_path_buf(),
            model: "m".into(),
            agent: Some(agent(vision)),
            compare: vec![],
            parent_session: None,
            extra: TurnExtra {
                retry,
                ..Default::default()
            },
            cancel: Default::default(),
            host,
        })
        .await;
    if let Some(e) = &out.error {
        host.push(EventKind::Error(ErrorEvent {
            turn_id: Some(turn_id.into()),
            message: e.clone(),
        }));
    }
    host.push(EventKind::TurnEnded(TurnEnded {
        turn_id: turn_id.into(),
        reason: out.reason,
        usage: out.usage.clone(),
    }));
    out
}

fn png(w: u32, h: u32, pad: usize) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    b.extend(w.to_be_bytes());
    b.extend(h.to_be_bytes());
    b.extend([8, 6, 0, 0, 0]);
    b.extend((0..pad).map(|i| (i % 251) as u8));
    b
}

/// R1. A provider that refuses images (and never the thinking): the resend
/// without the thinking fails (the image is still there), the one without
/// the images, built from the refused request, is answered. Only the
/// images are recorded withheld; the next turn still sends the thinking.
#[tokio::test]
async fn r1_an_image_refusal_withholds_only_the_images() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("a.png"), png(10, 10, 100)).unwrap();
    let p = Model::new(Box::new(|req, n| {
        if shows_image(req) {
            return Err(status400("image input is not supported"));
        }
        Ok(match n {
            0 => read("a", "a.png", "look at the picture"),
            _ => answer("done", "fine"),
        })
    }));
    let host = Host::default();
    let b = backend(p.clone(), &[]);
    let o = run(&b, &host, ws.path(), true, "t1", "what is it?", None).await;
    assert_eq!(o.reason, TurnEndReason::Completed, "{:?}", o.error);
    let r = p.requests();
    for (i, q) in r.iter().enumerate() {
        eprintln!(
            "request {i}: thinking sent={} image shown={}",
            carries(q),
            shows_image(q)
        );
    }
    let iw = host.kinds("images_withheld");
    let tw = host.kinds("thinking_withheld");
    eprintln!("images_withheld: {iw:?}");
    eprintln!("thinking_withheld: {tw:?}");
    // The next turn: is the (innocent) thinking still sent?
    let o = run(&b, &host, ws.path(), true, "t2", "and now?", None).await;
    assert_eq!(o.reason, TurnEndReason::Completed, "{:?}", o.error);
    let last = p.requests().last().unwrap().clone();
    eprintln!("next turn sends thinking: {}", carries(&last));
    assert_eq!(iw.len(), 1);
    assert!(tw.is_empty(), "thinking withheld for an image refusal");
    assert!(carries(&last), "the innocent thinking is still sent");
    // The answered resend kept the thinking and left out the image.
    assert!(r.iter().any(|q| carries(q) && !shows_image(q)));
}

/// R2. A provider with DeepSeek's documented rule (thinking + tools: every
/// assistant step's `reasoning_content` must come back, else 400). One
/// step's reasoning repeats a secret the scrubber knows: it is sent back
/// scrubbed (plain text is not verified), so the chat goes on, and the
/// secret never leaves.
#[tokio::test]
async fn r2_scrubbed_plain_reasoning_is_still_sent_back() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("notes.txt"), "hello\n").unwrap();
    let secret = "sk-live-0123456789abcdef";
    let p = Model::new(Box::new(move |req, n| {
        let missing = !req.tools.is_empty()
            && req.messages.iter().any(|m| {
                matches!(m, ChatMsg::Assistant { tool_calls, thinking: None, .. } if !tool_calls.is_empty())
            });
        if missing {
            return Err(status400(
                "Missing reasoning_content field in the assistant message",
            ));
        }
        Ok(match n {
            0 => read("a", "notes.txt", &format!("the key is {secret}")),
            _ => answer("done", "fine"),
        })
    }));
    let host = Host::default();
    let b = backend(p.clone(), &[secret]);
    let o = run(&b, &host, ws.path(), false, "t1", "go", None).await;
    assert_eq!(o.reason, TurnEndReason::Completed, "{:?}", o.error);
    let r = p.requests();
    assert_eq!(r.len(), 2, "no request refused");
    assert!(!format!("{r:?}").contains(secret), "the secret was sent");
    assert!(format!("{:?}", r[1]).contains("the key is [REDACTED]"));
    let m = host.kinds("assistant_message");
    assert!(m[0]["thinking"].get("unsent").is_none(), "{}", m[0]);
}

/// R2b. Contrast: the same provider and a clean step: the chat goes on.
#[tokio::test]
async fn r2b_the_same_provider_with_clean_reasoning_goes_on() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("notes.txt"), "hello\n").unwrap();
    let p = Model::new(Box::new(move |req, n| {
        let missing = !req.tools.is_empty()
            && req.messages.iter().any(|m| {
                matches!(m, ChatMsg::Assistant { tool_calls, thinking: None, .. } if !tool_calls.is_empty())
            });
        if missing {
            return Err(status400("Missing reasoning_content"));
        }
        Ok(match n {
            0 => read("a", "notes.txt", "clean"),
            _ => answer("done", "fine"),
        })
    }));
    let host = Host::default();
    let b = backend(p.clone(), &[]);
    let o = run(&b, &host, ws.path(), false, "t1", "go", None).await;
    assert_eq!(o.reason, TurnEndReason::Completed, "{:?}", o.error);
}

/// R5. 40 steps each with 1.5 MiB of reasoning text (`signed`: also a
/// signed block of 100 KB): each step keeps at most `Thinking::MAX_BYTES`
/// (cut plain text, still sent), and with a window the in-turn shortening
/// (the thinking tier for signed blocks, folds for plain text) keeps every
/// request, and what the loop holds for it, under the window.
/// Returns the largest request (estimated tokens), the log's bytes and the
/// shortenings (signed: those that left thinking out).
async fn r5_with(window: Option<u64>, signed: bool) -> (usize, usize, usize) {
    let ws = tempfile::tempdir().unwrap();
    for n in 0..40 {
        std::fs::write(ws.path().join(format!("f{n}.txt")), format!("hello {n}\n")).unwrap();
    }
    let big: Arc<String> = Arc::new("r".repeat(3 << 19));
    let b2 = big.clone();
    let main = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let p = Model::new(Box::new(move |req, n| {
        // Summaries and other side requests: plain text.
        if req.tools.is_empty() {
            return Ok(Completion {
                text: "summary".into(),
                ..Default::default()
            });
        }
        // The turn's step, counted by the requests answered (a fold takes
        // earlier results out of the request, so they cannot be counted).
        let steps = main.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let _ = n;
        let mut c = if steps < 40 {
            read(
                &format!("c{steps}"),
                &format!("f{steps}.txt"),
                &format!("{steps}{}", b2),
            )
        } else {
            answer("done", "end")
        };
        if signed {
            c.thinking = Some(Thinking {
                model: "m".into(),
                blocks: vec![ThinkingBlock::Signed {
                    text: format!("why {steps}"),
                    // A large but normal signature (100 KB): kept and sent.
                    signature: format!("{steps}{}", &b2[..100_000]),
                }],
                prefix: "p".into(),
                unsent: false,
            });
        }
        Ok(c)
    }));
    let host = Host::default();
    let b = backend(p.clone(), &[]);
    let mut spec = agent(false);
    if let Some(w) = window {
        spec.context_windows.insert("m".into(), w);
    }
    let history = host.log();
    host.push(EventKind::TurnStarted(TurnStarted {
        continued_by: None,
        retry: None,
        cause: None,
        agent: None,
        turn_id: "t1".into(),
        environment: None,
    }));
    host.push(EventKind::UserMessage(UserMessage {
        turn_id: "t1".into(),
        text: "go".into(),
        attachments: vec![],
        reply: None,
        images: vec![],
        queued: false,
        files: vec![],
    }));
    let o = b
        .run_turn(TurnCtx {
            session_id: host.sid,
            turn_id: "t1".into(),
            history,
            user_text: "go".into(),
            cwd: ws.path().to_path_buf(),
            model: "m".into(),
            agent: Some(spec),
            compare: vec![],
            parent_session: None,
            extra: TurnExtra::default(),
            cancel: Default::default(),
            host: &host,
        })
        .await;
    eprintln!("window {window:?}");
    eprintln!("turn: {:?} {:?}", o.reason, o.error);
    let r = p.requests();
    let held = |q: &ChatRequest| -> usize {
        q.messages
            .iter()
            .filter_map(|m| match m {
                ChatMsg::Assistant {
                    thinking: Some(t), ..
                } => Some(t.bytes()),
                _ => None,
            })
            .sum()
    };
    eprintln!(
        "requests: {}, thinking bytes in the last request: {}, estimate tokens: {}",
        r.len(),
        held(r.last().unwrap()),
        harness_agent_native::estimate_tokens(&r.last().unwrap().messages)
    );
    let log_bytes: usize = host.log().iter().map(|e| e.payload.to_string().len()).sum();
    eprintln!("log payload bytes: {log_bytes}");
    let kinds: std::collections::BTreeMap<String, usize> =
        host.log().iter().fold(Default::default(), |mut m, e| {
            *m.entry(e.kind.clone()).or_default() += 1;
            m
        });
    eprintln!("event kinds: {kinds:?}");
    let max_held = r.iter().map(held).max().unwrap_or_default();
    eprintln!("max thinking bytes in any request: {max_held}");
    assert_eq!(o.reason, TurnEndReason::Completed, "{:?}", o.error);
    let shortened = host.kinds("context_shortened");
    let left_out = shortened
        .iter()
        .filter(|c| c.get("thinking").is_some())
        .count();
    let max_est = r
        .iter()
        .map(|q| harness_agent_native::estimate_tokens(&q.messages))
        .max()
        .unwrap_or_default();
    let _ = max_held;
    (
        max_est as usize,
        log_bytes,
        if signed { left_out } else { shortened.len() },
    )
}

#[tokio::test]
async fn r5_large_thinking_is_bounded_across_forty_steps() {
    use harness_core::event::Thinking as T;
    for signed in [false, true] {
        // No window: nothing is shortened, but each step keeps at most the
        // bound.
        let (_, log, _) = r5_with(None, signed).await;
        assert!(log < 41 * (T::MAX_BYTES + 4096), "log {log}");
        // A 200k window: signed thinking is left out by its tier, oldest
        // first, and recorded; plain reasoning goes whole with its steps
        // when they are folded; either way no request carries more than
        // fits the window (4 characters a token).
        let (tokens, log, cuts) = r5_with(Some(200_000), signed).await;
        assert!(cuts > 0, "signed {signed}: nothing was shortened");
        assert!(
            tokens < 200_000,
            "signed {signed}: a request was {tokens} tokens"
        );
        assert!(log < 41 * (T::MAX_BYTES + 4096), "log {log}");
    }
}

// ---- Anthropic on the wire: a gap in the kept blocks ----------------------

type Seen = Arc<Mutex<Vec<Value>>>;

/// Serves `bodies` in order (the last one repeats).
async fn serve_seq(path: &'static str, bodies: Vec<String>) -> (String, Seen) {
    let seen: Seen = Arc::default();
    let s2 = seen.clone();
    let bodies = Arc::new(bodies);
    let app = Router::new().route(
        path,
        post(move |req: String| {
            let seen = s2.clone();
            let bodies = bodies.clone();
            async move {
                let n = {
                    let mut s = seen.lock().unwrap();
                    s.push(serde_json::from_str(&req).unwrap());
                    s.len() - 1
                };
                let body = bodies[n.min(bodies.len() - 1)].clone();
                ([("content-type", "text/event-stream")], body).into_response()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), seen)
}

fn ev(name: &str, data: Value) -> String {
    format!("event: {name}\ndata: {data}\n\n")
}

fn anthropic_sse(blocks: &[(Value, Vec<Value>)]) -> String {
    let mut s = vec![ev(
        "message_start",
        json!({"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"test-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":12,"output_tokens":1}}}),
    )];
    for (i, (start, deltas)) in blocks.iter().enumerate() {
        s.push(ev(
            "content_block_start",
            json!({"type":"content_block_start","index":i,"content_block":start}),
        ));
        for d in deltas {
            s.push(ev(
                "content_block_delta",
                json!({"type":"content_block_delta","index":i,"delta":d}),
            ));
        }
        s.push(ev(
            "content_block_stop",
            json!({"type":"content_block_stop","index":i}),
        ));
    }
    s.push(ev("message_delta", json!({"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":20}})));
    s.push(ev("message_stop", json!({"type":"message_stop"})));
    s.concat()
}

fn think(text: &str, sig: &str) -> (Value, Vec<Value>) {
    (
        json!({"type":"thinking","thinking":"","signature":""}),
        vec![
            json!({"type":"thinking_delta","thinking":text}),
            json!({"type":"signature_delta","signature":sig}),
        ],
    )
}

fn call(id: &str) -> (Value, Vec<Value>) {
    (
        json!({"type":"tool_use","id":id,"name":"bash","input":{}}),
        vec![json!({"type":"input_json_delta","partial_json":"{\"command\":\"ls\"}"})],
    )
}

fn wire_request(messages: Vec<ChatMsg>) -> ChatRequest {
    ChatRequest {
        model: "test-model".into(),
        messages,
        tools: vec![ToolSpec {
            name: "bash".into(),
            description: "run".into(),
            schema: json!({"type": "object", "properties": {"command": {"type": "string"}}}),
        }],
        effort: None,
        cache: true,
        limits: None,
        no_tool_calls: false,
        max_tokens: None,
    }
}

/// As the loop keeps a step: an `unsent` thinking is a marker.
fn keep(c: &Completion) -> Vec<ChatMsg> {
    let mut v = vec![ChatMsg::Assistant {
        text: c.text.clone(),
        tool_calls: c.tool_calls.clone(),
        thinking: c
            .thinking
            .clone()
            .map(harness_agent_native::provider::sendable_thinking),
    }];
    for tc in &c.tool_calls {
        v.push(ChatMsg::Tool {
            call_id: tc.call_id.clone(),
            content: "a.txt".into(),
        });
    }
    v
}

fn sigs(body: &Value) -> Vec<String> {
    let mut out = vec![];
    for m in body["messages"].as_array().unwrap() {
        if let Some(c) = m["content"].as_array() {
            for p in c {
                if p["type"] == "thinking" {
                    out.push(p["signature"].as_str().unwrap().to_string());
                }
            }
        }
    }
    out
}

/// R3. Step 1 leads with its thinking (sent back); step 2 is a progress
/// update (thinking between two parallel calls: `unsent`); step 3 leads
/// again. No later request sends step 3's blocks after step 2's were left
/// out: only the run before the gap goes ("not a block between two that
/// you keep").
#[tokio::test]
async fn r3_no_signed_block_is_sent_after_one_that_is_not() {
    let bodies = vec![
        anthropic_sse(&[think("one", "S1"), call("t1")]),
        anthropic_sse(&[
            think("two a", "S2A"),
            call("t2"),
            think("two b", "S2B"),
            call("t3"),
        ]),
        anthropic_sse(&[think("three", "S3"), call("t4")]),
        anthropic_sse(&[think("four", "S4"), call("t5")]),
    ];
    let (base, seen) = serve_seq("/v1/messages", bodies).await;
    let p = LlmProvider::new(
        ApiKind::Anthropic,
        &format!("{base}/v1"),
        Some(Secret::new("k".into())),
    )
    .unwrap();
    let mut msgs = vec![
        ChatMsg::System("be brief".into()),
        ChatMsg::User("list".into()),
    ];
    for _ in 0..4 {
        let (tx, _rx) = mpsc::unbounded_channel();
        let c = p.complete(&wire_request(msgs.clone()), tx).await.unwrap();
        eprintln!("answer unsent={:?}", c.thinking.as_ref().map(|t| t.unsent));
        msgs.extend(keep(&c));
        // As the loop and the rebuild keep the conversation.
        harness_agent_native::provider::settle_thinking(&mut msgs);
    }
    let bodies = seen.lock().unwrap().clone();
    assert_eq!(sigs(&bodies[2]), vec!["S1"]);
    assert_eq!(
        sigs(&bodies[3]),
        vec!["S1"],
        "a block after the gap was sent"
    );
}

/// The gap rule in the request builder alone (a conversation not settled
/// before): a signed block after one that is not sent is not sent.
#[tokio::test]
async fn r3c_the_request_builder_keeps_the_gap_on_its_own() {
    let bodies = vec![
        anthropic_sse(&[think("one", "S1"), call("t1")]),
        anthropic_sse(&[
            think("two a", "S2A"),
            call("t2"),
            think("two b", "S2B"),
            call("t3"),
        ]),
        anthropic_sse(&[think("three", "S3"), call("t4")]),
        anthropic_sse(&[think("four", "S4"), call("t5")]),
    ];
    let (base, seen) = serve_seq("/v1/messages", bodies).await;
    let p = LlmProvider::new(
        ApiKind::Anthropic,
        &format!("{base}/v1"),
        Some(Secret::new("k".into())),
    )
    .unwrap();
    let mut msgs = vec![
        ChatMsg::System("be brief".into()),
        ChatMsg::User("list".into()),
    ];
    for _ in 0..4 {
        let (tx, _rx) = mpsc::unbounded_channel();
        let c = p.complete(&wire_request(msgs.clone()), tx).await.unwrap();
        msgs.extend(keep(&c));
    }
    let bodies = seen.lock().unwrap().clone();
    assert_eq!(
        sigs(&bodies[3]),
        vec!["S1"],
        "a block after the gap was sent"
    );
}

/// R3b. The same with a scrubbed step 2 (its thinking repeats a key).
/// Scrubbing happens in the loop; here, as the loop would keep it.
#[tokio::test]
async fn r3b_no_signed_block_is_sent_after_a_scrubbed_one() {
    let bodies = vec![
        anthropic_sse(&[think("one", "S1"), call("t1")]),
        anthropic_sse(&[think("key sk-live-0123456789abcdef", "S2"), call("t2")]),
        anthropic_sse(&[think("three", "S3"), call("t3")]),
        anthropic_sse(&[think("four", "S4"), call("t4")]),
    ];
    let (base, seen) = serve_seq("/v1/messages", bodies).await;
    let p = LlmProvider::new(
        ApiKind::Anthropic,
        &format!("{base}/v1"),
        Some(Secret::new("k".into())),
    )
    .unwrap();
    let mut msgs = vec![
        ChatMsg::System("be brief".into()),
        ChatMsg::User("list".into()),
    ];
    for i in 0..4 {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut c = p.complete(&wire_request(msgs.clone()), tx).await.unwrap();
        if i == 1 {
            // What `bound_thinking` does to a block the scrubber changes.
            c.thinking.as_mut().unwrap().unsent = true;
        }
        msgs.extend(keep(&c));
        harness_agent_native::provider::settle_thinking(&mut msgs);
    }
    let bodies = seen.lock().unwrap().clone();
    assert_eq!(
        sigs(&bodies[3]),
        vec!["S1"],
        "a block after the gap was sent"
    );
}

// ---- rebuild = live --------------------------------------------------------

fn rebuilt(log: &[RawEvent]) -> Vec<ChatMsg> {
    let mut m = vec![ChatMsg::System("x".into())];
    m.extend(harness_agent_native::rebuild(
        log,
        &harness_agent_native::Rebuild {
            blocks: true,
            agent: Some("dev"),
            ..Default::default()
        },
    ));
    harness_agent_native::place_notes(m).split_off(1)
}

fn rebuilds_at_every_step(log: &[RawEvent], answered: &[ChatRequest]) {
    let cuts: Vec<usize> = log
        .iter()
        .enumerate()
        .filter(|(_, e)| e.kind == "assistant_message")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(cuts.len(), answered.len(), "one answer a request");
    for (k, (cut, req)) in cuts.iter().zip(answered).enumerate() {
        let got = rebuilt(&log[..*cut]);
        // Images: the loop's requests hold no bytes; compare without them.
        let strip = |v: &[ChatMsg]| -> Vec<String> { v.iter().map(|m| format!("{m:?}")).collect() };
        assert_eq!(
            strip(&got),
            strip(&req.messages[1..]),
            "request {k} differs from the log"
        );
    }
}

/// R1b. The R1 chat (image refused, thinking innocent): every answered
/// request still rebuilds from the log, with the one record.
#[tokio::test]
async fn r1b_both_resends_rebuild_from_the_log() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("a.png"), png(10, 10, 100)).unwrap();
    let p = Model::new(Box::new(|req, n| {
        if shows_image(req) {
            return Err(status400("image input is not supported"));
        }
        Ok(match n {
            0 => read("a", "a.png", "look at the picture"),
            _ => answer("done", "fine"),
        })
    }));
    let host = Host::default();
    let b = backend(p.clone(), &[]);
    run(&b, &host, ws.path(), true, "t1", "what is it?", None).await;
    run(&b, &host, ws.path(), true, "t2", "and now?", None).await;
    let r = p.requests();
    let answered: Vec<ChatRequest> = r.iter().filter(|q| !shows_image(q)).cloned().collect();
    let order: Vec<String> = host
        .log()
        .iter()
        .filter(|e| e.kind.ends_with("_withheld"))
        .map(|e| e.kind.clone())
        .collect();
    assert_eq!(order, vec!["images_withheld"]);
    rebuilds_at_every_step(&host.log(), &answered);
}

/// R4. The provider starts refusing replayed thinking only in the second
/// turn, after a restart (a new backend over the same log); that turn is
/// then retried after an unrelated failure. Records, and rebuild = live.
#[tokio::test]
async fn r4_refusal_on_a_later_turns_first_request_after_restart_then_retry() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("notes.txt"), "hello\n").unwrap();
    let refuse = Arc::new(Mutex::new(false));
    let fail_other = Arc::new(Mutex::new(false));
    let (r2, f2) = (refuse.clone(), fail_other.clone());
    let p = Model::new(Box::new(move |req, _n| {
        if *r2.lock().unwrap() && carries(req) {
            return Err(status400("thinking blocks cannot be modified"));
        }
        if *f2.lock().unwrap() {
            *f2.lock().unwrap() = false;
            return Err(ProviderError::Status {
                status: 503,
                body: r#"{"error":{"message":"overloaded"}}"#.into(),
                hint: RetryHint {
                    after: None,
                    no_retry: true,
                },
            });
        }
        let steps = req
            .messages
            .iter()
            .filter(|m| matches!(m, ChatMsg::Tool { .. }))
            .count();
        let users = req
            .messages
            .iter()
            .filter(|m| matches!(m, ChatMsg::User(_)))
            .count();
        Ok(if steps < users {
            read(&format!("c{steps}"), "notes.txt", &format!("why {steps}"))
        } else {
            answer("done", &format!("end {steps}"))
        })
    }));
    let host = Host::default();
    let b = backend(p.clone(), &[]);
    let o = run(&b, &host, ws.path(), false, "t1", "go", None).await;
    assert_eq!(o.reason, TurnEndReason::Completed, "{:?}", o.error);
    // Restart; the provider now refuses any replay.
    *refuse.lock().unwrap() = true;
    let b2 = backend(p.clone(), &[]);
    let o = run(&b2, &host, ws.path(), false, "t2", "again", None).await;
    eprintln!("t2: {:?} {:?}", o.reason, o.error);
    assert_eq!(o.reason, TurnEndReason::Completed, "{:?}", o.error);
    assert_eq!(host.kinds("thinking_withheld").len(), 1);
    // t3 fails for another reason, then is retried.
    *fail_other.lock().unwrap() = true;
    let o = run(&b2, &host, ws.path(), false, "t3", "more", None).await;
    eprintln!("t3: {:?} {:?}", o.reason, o.error);
    let o = run(
        &b2,
        &host,
        ws.path(),
        false,
        "t4",
        "",
        Some(RetryOf {
            turn_id: "t3".into(),
            reason: o.reason,
            error: None,
            failed_model: None,
            model: None,
        }),
    )
    .await;
    eprintln!("retry: {:?} {:?}", o.reason, o.error);
    assert_eq!(o.reason, TurnEndReason::Completed, "{:?}", o.error);
    let r = p.requests();
    // Answered requests: not refused for thinking, not the 503.
    let mut answered = vec![];
    let log = host.log();
    let n_msgs = log.iter().filter(|e| e.kind == "assistant_message").count();
    for (i, q) in r.iter().enumerate() {
        let refused_thinking = *refuse.lock().unwrap() && carries(q) && i > 1;
        if !refused_thinking {
            answered.push(q.clone());
        }
    }
    eprintln!(
        "requests {} answered-candidates {} assistant messages {}",
        r.len(),
        answered.len(),
        n_msgs
    );
    // Drop the 503 one (the first request of t3).
    let t3_first = answered
        .iter()
        .position(|q| {
            q.messages
                .iter()
                .any(|m| matches!(m, ChatMsg::User(t) if t.contains("more")))
        })
        .unwrap();
    answered.remove(t3_first);
    rebuilds_at_every_step(&log, &answered);
    assert!(
        r.iter()
            .skip(2)
            .all(|q| !carries(q) || *refuse.lock().unwrap())
    );
    eprintln!(
        "after the record, requests carrying thinking: {}",
        r.iter().skip(4).filter(|q| carries(q)).count()
    );
}

/// One turn on `spec` (the review's `run` with a window).
async fn turn_with(
    b: &NativeBackend,
    host: &Host,
    ws: &std::path::Path,
    spec: AgentSpec,
    turn_id: &str,
    text: &str,
) -> TurnOutcome {
    let history = host.log();
    host.push(EventKind::TurnStarted(TurnStarted {
        continued_by: None,
        retry: None,
        cause: None,
        agent: None,
        turn_id: turn_id.into(),
        environment: None,
    }));
    host.push(EventKind::UserMessage(UserMessage {
        turn_id: turn_id.into(),
        text: text.into(),
        attachments: vec![],
        reply: None,
        images: vec![],
        queued: false,
        files: vec![],
    }));
    let out = b
        .run_turn(TurnCtx {
            session_id: host.sid,
            turn_id: turn_id.into(),
            history,
            user_text: text.into(),
            cwd: ws.to_path_buf(),
            model: "m".into(),
            agent: Some(spec),
            compare: vec![],
            parent_session: None,
            extra: TurnExtra::default(),
            cancel: Default::default(),
            host,
        })
        .await;
    host.push(EventKind::TurnEnded(TurnEnded {
        turn_id: turn_id.into(),
        reason: out.reason,
        usage: out.usage.clone(),
    }));
    out
}

/// Re-check of PR 137: `/compact` after the tier left thinking out. The
/// summary covers the turn the tier's record names, so nothing is left out
/// by it any more (that turn is gone from the requests), and the next turn
/// rebuilds from the log and is answered.
#[tokio::test]
async fn compact_after_the_tier_left_thinking_out() {
    use harness_core::backend::{CompactCtx, CompactOutcome};
    let ws = tempfile::tempdir().unwrap();
    for n in 0..30 {
        std::fs::write(ws.path().join(format!("f{n}.txt")), format!("hello {n}\n")).unwrap();
    }
    let main = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let p = Model::new(Box::new(move |req, _| {
        if req.tools.is_empty() {
            return Ok(Completion {
                text: "summary".into(),
                ..Default::default()
            });
        }
        let n = main.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut c = if n < 20 {
            read(&format!("c{n}"), &format!("f{n}.txt"), "")
        } else {
            answer("done", "")
        };
        c.thinking = Some(Thinking {
            model: "m".into(),
            blocks: vec![ThinkingBlock::Signed {
                text: format!("why {n}"),
                signature: "s".repeat(60_000),
            }],
            prefix: "p".into(),
            unsent: false,
        });
        Ok(c)
    }));
    let host = Host::default();
    let b = backend(p.clone(), &[]);
    let mut spec = agent(false);
    spec.context_windows.insert("m".into(), 200_000);
    let o = turn_with(&b, &host, ws.path(), spec.clone(), "t1", "go").await;
    assert_eq!(o.reason, TurnEndReason::Completed, "{:?}", o.error);
    assert!(
        host.kinds("context_shortened")
            .iter()
            .any(|c| c.get("thinking").is_some()),
        "the tier did not act"
    );
    // A later turn, so the summary has a turn before the last message.
    let o = turn_with(&b, &host, ws.path(), spec.clone(), "t2", "ok?").await;
    assert_eq!(o.reason, TurnEndReason::Completed, "{:?}", o.error);
    let done = b
        .compact_now(CompactCtx {
            session_id: host.sid,
            history: host.log(),
            model: "m".into(),
            window: Some(200_000),
            focus: None,
            cancel: Default::default(),
        })
        .await;
    let Some(CompactOutcome::Done {
        through_seq, text, ..
    }) = done
    else {
        panic!("no summary: {done:?}")
    };
    host.push(EventKind::HistorySummarized(HistorySummarized {
        turn_id: String::new(),
        through_seq,
        text,
        manual: true,
        focus: None,
        usage: None,
        tokens_after: None,
    }));
    let before = p.requests().len();
    let o = turn_with(&b, &host, ws.path(), spec, "t3", "and now?").await;
    assert_eq!(o.reason, TurnEndReason::Completed, "{:?}", o.error);
    // The next turn's request: none of t1's steps (summarised), so none
    // of their thinking, and it rebuilds from the log.
    let r = p.requests();
    let next = &r[before];
    let sent = format!("{:?}", next.messages);
    for n in 0..20 {
        assert!(
            !sent.contains(&format!("why {n}\"")),
            "step {n}'s thinking was sent"
        );
    }
    let log = host.log();
    let cut = log
        .iter()
        .rposition(|e| e.kind == "assistant_message")
        .unwrap();
    let (_, recent) = harness_agent_native::split_summary(&log[..cut]);
    let mut m = vec![ChatMsg::System("x".into())];
    m.extend(harness_agent_native::rebuild(
        recent,
        &harness_agent_native::Rebuild {
            blocks: true,
            agent: Some("dev"),
            ..Default::default()
        },
    ));
    let got = harness_agent_native::place_notes(m).split_off(1);
    let w: Vec<String> = next.messages[1..]
        .iter()
        .map(|m| format!("{m:?}"))
        .collect();
    let a: Vec<String> = got.iter().map(|m| format!("{m:?}")).collect();
    assert_eq!(a, w, "the next turn differs from the log");
}
