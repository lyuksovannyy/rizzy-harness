//! The re-check of PR 137's fix round (X.25, PLAN.md T73), its proofs as
//! tests: the in-turn tier leaves plain reasoning in, so a host that wants
//! every step's back keeps answering; the gap rule on the wire through the
//! real loop and Anthropic adapter (restarts, a retry, every shortening),
//! with rebuild equal to live; and the three resends of an unrelated
//! refusal, after which the original error stands.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::response::IntoResponse;
use axum::{Router, routing::post};
use base64::Engine as _;
use harness_agent_native::llm::{ApiKind, LlmProvider};
use harness_agent_native::{
    ChatMsg, ChatRequest, Completion, ImageState, NativeBackend, NativeConfig, Provider,
    ProviderError, RetryHint,
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
        deny_skills: vec![],
        deny_mcp_servers: vec![],
        stored_skills: vec![],
        connector_tools: vec![],
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

// ---- Anthropic on the wire: a gap in the kept blocks ----------------------

type Seen = Arc<Mutex<Vec<Value>>>;

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

// ---- rebuild = live --------------------------------------------------------

// ===== r-thinking2: re-check of the fix round of PR 137 ====================

struct Lcg2(u64);
impl Lcg2 {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn pick(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

async fn run_spec(
    b: &NativeBackend,
    host: &Host,
    ws: &std::path::Path,
    spec: AgentSpec,
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
            agent: Some(spec),
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

/// A1. DeepSeek's documented rule (thinking + tools: every assistant
/// step's `reasoning_content` must come back, else 400), with large plain
/// reasoning and a window: the tier that leaves out thinking leaves plain
/// text in, so the strict host answers every request, this turn and the
/// next (older steps go whole with a fold, which the rule allows).
#[tokio::test]
async fn a1_a_strict_plain_reasoning_host_answers_through_shortening() {
    let ws = tempfile::tempdir().unwrap();
    for n in 0..40 {
        std::fs::write(ws.path().join(format!("f{n}.txt")), format!("hello {n}\n")).unwrap();
    }
    let big: Arc<String> = Arc::new("r".repeat(200_000));
    let b2 = big.clone();
    let main = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let p = Model::new(Box::new(move |req, _n| {
        if req.tools.is_empty() {
            return Ok(Completion {
                text: "summary".into(),
                ..Default::default()
            });
        }
        let missing = req.messages.iter().any(|m| {
            matches!(m, ChatMsg::Assistant { tool_calls, thinking, .. }
                if !tool_calls.is_empty()
                    && thinking.as_ref().is_none_or(|t| t.blocks.is_empty()))
        });
        if missing {
            return Err(status400(
                "Missing reasoning_content field in the assistant message",
            ));
        }
        // The turn's step, counted by the requests answered (a fold takes
        // earlier results out of the request, so they cannot be counted).
        let steps = main.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(if steps < 30 {
            read(
                &format!("c{steps}"),
                &format!("f{steps}.txt"),
                &format!("{steps}{b2}"),
            )
        } else {
            answer("done", "end")
        })
    }));
    let host = Host::default();
    let b = backend(p.clone(), &[]);
    let mut spec = agent(false);
    spec.context_windows.insert("m".into(), 200_000);
    let o = run_spec(&b, &host, ws.path(), spec.clone(), "t1", "go", None).await;
    let tier0 = host
        .kinds("context_shortened")
        .iter()
        .filter(|c| c.get("thinking").is_some())
        .count();
    let refused = p
        .requests()
        .iter()
        .filter(|q| {
            !q.tools.is_empty()
                && q.messages.iter().any(|m| matches!(m, ChatMsg::Assistant { tool_calls, thinking, .. }
                    if !tool_calls.is_empty() && thinking.as_ref().is_none_or(|t| t.blocks.is_empty())))
        })
        .count();
    eprintln!(
        "t1: {:?} {:?}; tier-0 records {tier0}; requests {}, refused {refused}; thinking_withheld {}",
        o.reason,
        o.error,
        p.requests().len(),
        host.kinds("thinking_withheld").len()
    );
    let o2 = run_spec(&b, &host, ws.path(), spec, "t2", "go on", None).await;
    eprintln!("t2: {:?} {:?}", o2.reason, o2.error);
    assert_eq!(o.reason, TurnEndReason::Completed, "{:?}", o.error);
    assert_eq!(o2.reason, TurnEndReason::Completed, "{:?}", o2.error);
    assert_eq!(refused, 0, "the strict host refused a request");
    assert_eq!(tier0, 0, "plain reasoning was left out by the tier");
    assert!(
        !host.kinds("context_shortened").is_empty(),
        "nothing was shortened: the case does not reach the tiers"
    );
}

// ---- B: the gap rule on the wire, through the loop, with real prefixes ----

struct Rec {
    inner: LlmProvider,
    seen: Mutex<Vec<ChatRequest>>,
}

#[async_trait::async_trait]
impl Provider for Rec {
    fn name(&self) -> &'static str {
        "rec"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        tx: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        let r = self.inner.complete(req, tx).await;
        if r.is_ok() {
            self.seen.lock().unwrap().push(req.clone());
        }
        r
    }
    fn replays_thinking(&self, req: &ChatRequest) -> bool {
        self.inner.replays_thinking(req)
    }
}

fn read_call(id: &str, path: &str) -> (Value, Vec<Value>) {
    (
        json!({"type":"tool_use","id":id,"name":"read","input":{}}),
        vec![json!({"type":"input_json_delta","partial_json": json!({"path": path}).to_string()})],
    )
}

fn sse_end(text: &str) -> String {
    [
        ev("message_start", json!({"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"m","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":12,"output_tokens":1}}})),
        ev("content_block_start", json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}})),
        ev("content_block_delta", json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}})),
        ev("content_block_stop", json!({"type":"content_block_stop","index":0})),
        ev("message_delta", json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":20}})),
        ev("message_stop", json!({"type":"message_stop"})),
    ]
    .concat()
}

/// What the server made at each step: (step, kind). kind 0 leads with
/// thinking; 1 is a progress update (thinking between two calls: unsent);
/// 2 has no thinking; 3 leads with thinking and has two calls.
type Made = Arc<Mutex<HashMap<String, (usize, u64)>>>;

async fn serve_dyn(
    seed: u64,
    steps_per_turn: usize,
    made: Made,
    fail_at: Option<usize>,
) -> (String, Seen) {
    let seen: Seen = Arc::default();
    let s2 = seen.clone();
    let counter = Arc::new(Mutex::new(0usize));
    let app = Router::new().route(
        "/v1/messages",
        post(move |req: String| {
            let seen = s2.clone();
            let made = made.clone();
            let counter = counter.clone();
            async move {
                let body: Value = serde_json::from_str(&req).unwrap();
                seen.lock().unwrap().push(body.clone());
                if body.get("tools").is_none_or(|t| t.as_array().is_none_or(|a| a.is_empty())) {
                    return ([("content-type", "text/event-stream")], sse_end("summary of earlier steps")).into_response();
                }
                // The step of this turn: tool results since the last user text.
                let msgs = body["messages"].as_array().unwrap();
                let mut results = 0;
                for m in msgs.iter().rev() {
                    let c = m["content"].as_array();
                    let is_result = c.is_some_and(|c| c.iter().any(|p| p["type"] == "tool_result"));
                    let is_text_user = m["role"] == "user" && !is_result;
                    if is_text_user {
                        break;
                    }
                    if is_result {
                        results += 1;
                    }
                }
                let n = {
                    let mut c = counter.lock().unwrap();
                    *c += 1;
                    *c
                };
                if fail_at == Some(n) {
                    return (
                        axum::http::StatusCode::UNAUTHORIZED,
                        [("content-type", "application/json")],
                        r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#.to_string(),
                    )
                        .into_response();
                }
                if results >= steps_per_turn {
                    return ([("content-type", "text/event-stream")], sse_end("done")).into_response();
                }
                let mut r = Lcg2(seed * 1_000_003 + n as u64);
                let kind = r.pick(4);
                let sig = |tag: &str, r: &mut Lcg2| format!("{tag}-{}", "s".repeat(20_000 + r.pick(40_000) as usize));
                let a = format!("n{n}a");
                let bb = format!("n{n}b");
                let blocks = match kind {
                    0 => vec![think(&format!("why {n}"), &sig(&format!("S{n}"), &mut r)), read_call(&a, &format!("f{n}a.txt"))],
                    1 => vec![
                        read_call(&a, &format!("f{n}a.txt")),
                        think(&format!("progress {n}"), &sig(&format!("S{n}"), &mut r)),
                        read_call(&bb, &format!("f{n}b.txt")),
                    ],
                    2 => vec![read_call(&a, &format!("f{n}a.txt"))],
                    _ => vec![
                        think(&format!("why {n}"), &sig(&format!("S{n}"), &mut r)),
                        read_call(&a, &format!("f{n}a.txt")),
                        read_call(&bb, &format!("f{n}b.txt")),
                    ],
                };
                made.lock().unwrap().insert(a, (n, kind));
                ([("content-type", "text/event-stream")], anthropic_sse(&blocks)).into_response()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), seen)
}

/// For one wire body: in the order of the assistant messages, each step
/// that PRODUCED signed thinking (kinds 0, 1, 3) and whether this body
/// sends its signature. Returns an error naming the gap "in between".
fn gap_in_between(
    body: &Value,
    made: &HashMap<String, (usize, u64)>,
) -> Result<(usize, usize), String> {
    let mut seq: Vec<(usize, bool)> = vec![];
    for m in body["messages"].as_array().unwrap() {
        if m["role"] != "assistant" {
            continue;
        }
        let Some(c) = m["content"].as_array() else {
            continue;
        };
        let Some(step) = c.iter().find_map(|p| {
            (p["type"] == "tool_use")
                .then(|| made.get(p["id"].as_str().unwrap_or("")))
                .flatten()
        }) else {
            continue;
        };
        if step.1 == 2 {
            continue;
        }
        let sent = c.iter().any(|p| p["type"] == "thinking");
        seq.push((step.0, sent));
    }
    let sent: Vec<usize> = seq
        .iter()
        .enumerate()
        .filter(|(_, s)| s.1)
        .map(|(i, _)| i)
        .collect();
    if let (Some(&a), Some(&z)) = (sent.first(), sent.last())
        && let Some(hole) = seq[a..=z].iter().find(|s| !s.1)
    {
        return Err(format!(
            "step {} not sent between sent ones: {seq:?}",
            hole.0
        ));
    }
    Ok((seq.len(), sent.len()))
}

async fn b_seed(seed: u64, window: u64, retry_case: bool) -> (usize, usize, usize, usize) {
    let ws = tempfile::tempdir().unwrap();
    for n in 0..400 {
        std::fs::write(
            ws.path().join(format!("f{n}a.txt")),
            format!("hello {n}a\n{}", "line of file content\n".repeat(400)),
        )
        .unwrap();
        std::fs::write(
            ws.path().join(format!("f{n}b.txt")),
            format!("hello {n}b\n"),
        )
        .unwrap();
    }
    let made: Made = Arc::default();
    // In the retry case the 9th request is a 503: the turn fails and is retried.
    let (base, seen) = serve_dyn(seed, 14, made.clone(), retry_case.then_some(9)).await;
    let rec = Arc::new(Rec {
        inner: LlmProvider::new(
            ApiKind::Anthropic,
            &format!("{base}/v1"),
            Some(Secret::new("k".into())),
        )
        .unwrap(),
        seen: Mutex::default(),
    });
    let host = Host::default();
    let mut spec = agent(false);
    spec.context_windows.insert("m".into(), window);
    let b = backend(rec.clone(), &[]);
    let o1 = run_spec(&b, &host, ws.path(), spec.clone(), "t1", "go", None).await;
    // Restart between turns: a new backend over the same log.
    let b2 = backend(rec.clone(), &[]);
    let o2 = if retry_case && o1.reason != TurnEndReason::Completed {
        run_spec(
            &b2,
            &host,
            ws.path(),
            spec.clone(),
            "t1r",
            "",
            Some(RetryOf {
                turn_id: "t1".into(),
                reason: o1.reason,
                error: None,
                failed_model: None,
                model: None,
            }),
        )
        .await
    } else {
        run_spec(&b2, &host, ws.path(), spec.clone(), "t2", "more", None).await
    };
    let b3 = backend(rec.clone(), &[]);
    let o3 = run_spec(&b3, &host, ws.path(), spec, "t3", "again", None).await;
    let tier0 = host
        .kinds("context_shortened")
        .iter()
        .filter(|c| c.get("thinking").is_some())
        .count();
    let other_sh = host.kinds("context_shortened").len() - tier0;
    let made = made.lock().unwrap().clone();
    let bodies = seen.lock().unwrap().clone();
    let mut checked = 0;
    let mut sent_any = 0;
    for (i, body) in bodies.iter().enumerate() {
        if body.get("tools").is_none() {
            continue;
        }
        match gap_in_between(body, &made) {
            Ok((_, s)) => {
                checked += 1;
                sent_any += usize::from(s > 0);
            }
            Err(e) => panic!("seed {seed} window {window}: request {i}: GAP IN BETWEEN: {e}"),
        }
    }
    // rebuild = live, for every answered main request.
    let answered: Vec<ChatRequest> = {
        let all = rec.seen.lock().unwrap().clone();
        // drop summaries and the 503'd one
        all.into_iter()
            .filter(|q| !q.tools.is_empty())
            .collect::<Vec<ChatRequest>>()
    };
    let log = host.log();
    let cuts: Vec<usize> = log
        .iter()
        .enumerate()
        .filter(|(_, e)| e.kind == "assistant_message")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        cuts.len(),
        answered.len(),
        "seed {seed}: one answer a request ({:?} {:?} {:?})",
        o1.error,
        o2.error,
        o3.error
    );
    for (k, (cut, req)) in cuts.iter().zip(&answered).enumerate() {
        let (_s, recent) = harness_agent_native::split_summary(&log[..*cut]);
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
        let a: Vec<String> = got.iter().map(|m| format!("{m:?}")).collect();
        let w: Vec<String> = req.messages[1..].iter().map(|m| format!("{m:?}")).collect();
        if a != w {
            let at = a
                .iter()
                .zip(&w)
                .position(|(x, y)| x != y)
                .unwrap_or(a.len().min(w.len()));
            panic!(
                "seed {seed} window {window}: request {k} differs from the log at msg {at} (rebuilt {} live {})\n rebuilt: {:.300}\n live:    {:.300}",
                a.len(),
                w.len(),
                a.get(at).cloned().unwrap_or_default(),
                w.get(at).cloned().unwrap_or_default()
            );
        }
    }
    eprintln!(
        "seed {seed} window {window} retry {retry_case}: t1 {:?} {:?} t2(retry if t1 failed) {:?} t3 {:?}; {} wire requests checked ({sent_any} sent signed blocks), {} rebuilt equal, tier0 {tier0}, other shortenings {other_sh}, withheld {}",
        o1.reason,
        o1.error,
        o2.reason,
        o3.reason,
        checked,
        cuts.len(),
        host.kinds("thinking_withheld").len()
    );
    (checked, sent_any, tier0, other_sh)
}

#[tokio::test]
async fn b_gap_rule_on_the_wire_property() {
    let mut tot = (0, 0, 0, 0);
    // Six seeds (the review ran 16, 32 s here): about 12 s in a debug build.
    for seed in 1..=6u64 {
        for (window, retry) in [
            (30_000u64, false),
            (45_000, false),
            (60_000, false),
            (30_000, true),
            (45_000, true),
        ] {
            let r = b_seed(seed, window, retry).await;
            tot.0 += r.0;
            tot.1 += r.1;
            tot.2 += r.2;
            tot.3 += r.3;
        }
    }
    eprintln!(
        "TOTAL: wire requests checked {}, with sent signed blocks {}, tier-0 records {}, other shortenings {}",
        tot.0, tot.1, tot.2, tot.3
    );
    assert!(tot.1 > 0 && tot.2 > 0);
}

/// C. A request refused for a reason that is neither the thinking nor the
/// images: three resends (each from the original), then the original
/// error, nothing recorded; rebuild still equals the answered requests.
#[tokio::test]
async fn c_all_three_resends_fail() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("a.png"), png(10, 10, 100)).unwrap();
    let p = Model::new(Box::new(|req, n| {
        let steps = req
            .messages
            .iter()
            .filter(|m| matches!(m, ChatMsg::Tool { .. }))
            .count();
        if steps >= 1 {
            return Err(status400(&format!("unrelated refusal {n}")));
        }
        Ok(read("a", "a.png", "look"))
    }));
    let host = Host::default();
    let b = backend(p.clone(), &[]);
    let o = run(&b, &host, ws.path(), true, "t1", "what is it?", None).await;
    let r = p.requests();
    for (i, q) in r.iter().enumerate() {
        eprintln!(
            "request {i}: thinking={} image={}",
            carries(q),
            shows_image(q)
        );
    }
    eprintln!("outcome {:?} {:?}", o.reason, o.error);
    assert_eq!(r.len(), 5, "1 answered + 1 refused + 3 resends");
    assert!(carries(&r[1]) && shows_image(&r[1]));
    assert!(!carries(&r[2]) && shows_image(&r[2]));
    assert!(carries(&r[3]) && !shows_image(&r[3]));
    assert!(!carries(&r[4]) && !shows_image(&r[4]));
    assert!(
        o.error
            .as_deref()
            .unwrap_or("")
            .contains("unrelated refusal 1"),
        "the ORIGINAL error stands: {:?}",
        o.error
    );
    assert!(host.kinds("thinking_withheld").is_empty() && host.kinds("images_withheld").is_empty());
}
