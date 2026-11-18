//! Review of PR 130 (X.23, PLAN.md T71), its proofs turned into tests:
//! an image the provider refuses is withheld and the chat goes on; the
//! bytes a turn holds stay bounded by the request window; a file is capped
//! so its base64 fits Anthropic's 5 MB per image; a lost blob says so; a
//! model that saw no image did not read the file.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use base64::Engine as _;
use harness_agent_native::{
    ChatMsg, ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError,
    Rebuild, RetryHint, images, place_notes, rebuild,
};
use harness_core::agent::{AgentSpec, ToolSelection};
use harness_core::backend::{
    AgentBackend, ImageData, KeptImage, LogError, TurnCtx, TurnExtra, TurnHost, TurnOutcome,
};
use harness_core::event::*;
use harness_core::policy::{Decision, ToolRisk};
use harness_core::{RawEvent, SessionId};
use serde_json::{Value, json};
use tokio::sync::mpsc;

fn png(w: u32, h: u32, pad: usize) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    b.extend(w.to_be_bytes());
    b.extend(h.to_be_bytes());
    b.extend([8, 6, 0, 0, 0]);
    b.extend((0..pad).map(|i| (i % 251) as u8));
    b
}

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

// ---- a host with a log, a store, and a record of every load --------------

#[derive(Default)]
struct Host {
    sid: SessionId,
    log: Mutex<Vec<RawEvent>>,
    store: Mutex<HashMap<String, Vec<u8>>>,
    /// Every base64 copy `images` handed out, to measure what is held.
    loads: Mutex<Vec<Weak<str>>>,
}

impl Host {
    fn new() -> Self {
        Self {
            sid: SessionId::new(),
            ..Default::default()
        }
    }
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
    fn held(&self) -> usize {
        self.loads
            .lock()
            .unwrap()
            .iter()
            .filter_map(|w| w.upgrade())
            .map(|a| a.len())
            .sum()
    }
    fn loaded(&self) -> Vec<ImageData> {
        self.store
            .lock()
            .unwrap()
            .iter()
            .map(|(s, b)| ImageData {
                sha256: s.clone(),
                mime: "image/png".into(),
                data: b64(b).into(),
            })
            .collect()
    }
    fn withheld(&self) -> Vec<Value> {
        self.log()
            .into_iter()
            .filter(|e| e.kind == "images_withheld")
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
        let v: Vec<ImageData> = self
            .loaded()
            .into_iter()
            .filter(|d| sha256.contains(&d.sha256))
            .collect();
        let mut l = self.loads.lock().unwrap();
        for d in &v {
            l.push(Arc::downgrade(&d.data));
        }
        v
    }
}

// ---- a provider driven by a closure -------------------------------------

type Answer = Box<dyn Fn(&ChatRequest, usize) -> Result<Completion, ProviderError> + Send + Sync>;

struct Model {
    answer: Answer,
    seen: Mutex<Vec<ChatRequest>>,
    /// The base64 length of each image each request carried.
    sizes: Mutex<Vec<Vec<usize>>>,
    /// Bytes the host held when each request was made.
    held: Mutex<Vec<usize>>,
    host: Mutex<Option<Arc<Host>>>,
}

impl Model {
    fn new(answer: Answer) -> Arc<Self> {
        Arc::new(Self {
            answer,
            seen: Mutex::default(),
            sizes: Mutex::default(),
            held: Mutex::default(),
            host: Mutex::default(),
        })
    }
    fn requests(&self) -> Vec<ChatRequest> {
        self.seen.lock().unwrap().clone()
    }
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
        if let Some(h) = self.host.lock().unwrap().as_ref() {
            self.held.lock().unwrap().push(h.held());
        }
        // Kept without the bytes (their digest in their place, their
        // length in `sizes`): a test that kept every copy would hold what
        // the loop no longer does.
        let mut lite = req.clone();
        let mut sizes = Vec::new();
        for m in &mut lite.messages {
            if let ChatMsg::ToolImage { image, .. } = m
                && let Some(d) = image.data.take()
            {
                sizes.push(d.len());
                image.data = Some(digest(&d));
            }
        }
        self.sizes.lock().unwrap().push(sizes);
        let n = {
            let mut s = self.seen.lock().unwrap();
            s.push(lite);
            s.len() - 1
        };
        (self.answer)(req, n)
    }
}

/// The request carries `name`'s bytes (as an image the model sees).
fn carries(req: &ChatRequest, name: &str) -> bool {
    req.messages.iter().any(
        |m| matches!(m, ChatMsg::ToolImage { image, .. } if image.name == name && image.data.is_some()),
    )
}

fn carried_calls(req: &ChatRequest) -> Vec<String> {
    req.messages
        .iter()
        .filter_map(|m| match m {
            ChatMsg::ToolImage { call_id, image, .. } if image.data.is_some() => {
                Some(call_id.clone())
            }
            _ => None,
        })
        .collect()
}

fn refused() -> ProviderError {
    ProviderError::Status {
        status: 400,
        body: r#"{"type":"error","error":{"type":"invalid_request_error","message":"messages.3.content.1.image.source.base64.data: Could not process image"}}"#.into(),
        hint: RetryHint::default(),
    }
}

fn reads(paths: &[&str], tag: &str) -> Completion {
    Completion {
        tool_calls: paths
            .iter()
            .enumerate()
            .map(|(k, p)| ToolCallRef {
                call_id: format!("{tag}_{k}"),
                tool: "read".into(),
                input: json!({ "path": p }),
            })
            .collect(),
        ..Default::default()
    }
}

fn text(t: &str) -> Completion {
    Completion {
        text: t.into(),
        ..Default::default()
    }
}

fn agent(vision: &[&str], fallbacks: &[&str], tools: &[&str]) -> AgentSpec {
    AgentSpec {
        agent_id: "dev".into(),
        name: "Dev".into(),
        system_prompt: "Be brief.".into(),
        tools: ToolSelection::List(tools.iter().map(|s| s.to_string()).collect()),
        model: None,
        fallbacks: fallbacks.iter().map(|s| s.to_string()).collect(),
        unavailable: vec![],
        no_effort: vec![],
        vision: vision.iter().map(|s| s.to_string()).collect(),
        context_windows: Default::default(),
        effort: None,
        ui: Default::default(),
        no_personalisation: false,
        prompt_hash: "h".into(),
        deny_tools: vec![],
        deny_skills: vec![],
        deny_mcp_servers: vec![],
        delegates: vec![],
        caps: None,
    }
}

fn backend(p: Arc<dyn Provider>, cfg: NativeConfig) -> NativeBackend {
    NativeBackend::new(
        p,
        vec![
            Arc::new(harness_tools::Read),
            Arc::new(harness_tools::Write),
        ],
        cfg,
    )
}

struct Turn<'a> {
    b: &'a NativeBackend,
    host: &'a Host,
    ws: &'a std::path::Path,
    agent: AgentSpec,
}

impl Turn<'_> {
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
                model: "m".into(),
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

/// What a request built from `log` sends (less the system prompt), with the
/// host's stored bytes, as the loop builds it.
fn rebuilt(log: &[RawEvent], loaded: &[ImageData]) -> Vec<ChatMsg> {
    let mut m = vec![ChatMsg::System("x".into())];
    m.extend(rebuild(
        log,
        &Rebuild {
            blocks: true,
            agent: Some("dev"),
            ..Default::default()
        },
    ));
    images::tool_images(&mut m, true, loaded);
    place_notes(m).split_off(1)
}

/// `msgs` as [`Model`] keeps them: each image's bytes as their digest.
fn lite(mut msgs: Vec<ChatMsg>) -> Vec<ChatMsg> {
    for m in &mut msgs {
        if let ChatMsg::ToolImage { image, .. } = m
            && let Some(d) = image.data.take()
        {
            image.data = Some(digest(&d));
        }
    }
    msgs
}

fn digest(d: &str) -> Arc<str> {
    use sha2::Digest as _;
    let h = sha2::Sha256::digest(d.as_bytes());
    Arc::from(h.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

/// The log up to (not including) the `n`-th assistant message from `from`.
fn before_answer(log: &[RawEvent], from: usize, n: usize) -> &[RawEvent] {
    let cut = log
        .iter()
        .enumerate()
        .filter(|(i, e)| *i >= from && e.kind == "assistant_message")
        .map(|(i, _)| i)
        .nth(n)
        .expect("an answer");
    &log[..cut]
}

/// The reviewer's wedge, turned around: one read of an image the provider
/// refuses. The request is sent once more without it, that works, the
/// turn completes, one record names the call, and no later request (the
/// rest of the turn, the next turn) carries the image. Every request still
/// rebuilds byte for byte from the log.
#[tokio::test]
async fn a_refused_image_is_withheld_and_the_chat_goes_on() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("bad.png"), png(100, 100, 2_000)).unwrap();
    std::fs::write(ws.path().join("notes.txt"), "hello\n").unwrap();
    let p = Model::new(Box::new(|req, n| {
        if carries(req, "bad.png") {
            return Err(refused());
        }
        Ok(match n {
            0 => reads(&["bad.png"], "a"),
            2 => reads(&["notes.txt"], "b"),
            _ => text("done"),
        })
    }));
    let b = backend(p.clone(), NativeConfig::default());
    let host = Host::new();
    let t = Turn {
        b: &b,
        host: &host,
        ws: ws.path(),
        agent: agent(&["m"], &[], &["read"]),
    };
    let o1 = t.run("t1", "Look at bad.png", None).await;
    assert_eq!(o1.reason, TurnEndReason::Completed, "{:?}", o1.error);
    let rec = host.withheld();
    assert_eq!(rec.len(), 1, "one record");
    assert_eq!(rec[0]["calls"], json!(["a_0"]));
    assert_eq!(rec[0]["status"], json!(400));
    // req 0 (reads), req 1 (refused), req 1 again without it, then on.
    let reqs = p.requests();
    assert!(carries(&reqs[1], "bad.png"));
    assert!(reqs[2..].iter().all(|r| !carries(r, "bad.png")));
    // The next turn: still not shown.
    let from = host.log().len();
    let o2 = t.run("t2", "And now?", None).await;
    assert_eq!(o2.reason, TurnEndReason::Completed, "{:?}", o2.error);
    let reqs = p.requests();
    assert!(reqs.iter().skip(2).all(|r| !carries(r, "bad.png")));
    // Live and rebuilt: the resend, and the next turn's first request.
    let log = host.log();
    let resend = &reqs[2].messages[1..];
    assert_eq!(
        lite(rebuilt(before_answer(&log, 0, 1), &host.loaded())),
        resend
    );
    let next = reqs.len() - 1;
    assert_eq!(
        lite(rebuilt(before_answer(&log, from, 0), &host.loaded())),
        &reqs[next].messages[1..]
    );
}

/// A Retry (X.21) of a turn that withheld an image and failed later for
/// another reason replays the attempt without the image, and completes.
#[tokio::test]
async fn a_retry_after_a_withhold_does_not_send_the_image_again() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("bad.png"), png(100, 100, 2_000)).unwrap();
    std::fs::write(ws.path().join("notes.txt"), "hello\n").unwrap();
    let p = Model::new(Box::new(|req, n| {
        if carries(req, "bad.png") {
            return Err(refused());
        }
        match n {
            0 => Ok(reads(&["bad.png"], "a")),
            2 => Ok(reads(&["notes.txt"], "b")),
            // The provider refuses the key: the turn fails after the
            // withhold, for a reason that is not the image.
            3 => Err(ProviderError::Status {
                status: 403,
                body: r#"{"error":{"message":"forbidden"}}"#.into(),
                hint: RetryHint::default(),
            }),
            _ => Ok(text("done")),
        }
    }));
    let b = backend(p.clone(), NativeConfig::default());
    let host = Host::new();
    let t = Turn {
        b: &b,
        host: &host,
        ws: ws.path(),
        agent: agent(&["m"], &[], &["read"]),
    };
    let o1 = t.run("t1", "Look at bad.png", None).await;
    assert_eq!(o1.reason, TurnEndReason::Error);
    assert_eq!(host.withheld().len(), 1);
    let r = harness_core::retry::check(&host.log(), "t1").expect("retryable");
    let before = p.requests().len();
    let from = host.log().len();
    let o2 = t.run("t2", "", Some(r)).await;
    assert_eq!(o2.reason, TurnEndReason::Completed, "{:?}", o2.error);
    let reqs = p.requests();
    assert!(reqs[before..].iter().all(|r| !carries(r, "bad.png")));
    let log = host.log();
    assert_eq!(
        lite(rebuilt(before_answer(&log, from, 0), &host.loaded())),
        &reqs[before].messages[1..]
    );
}

/// When the resend fails too, it was not the images: the original error
/// stands, as before, and nothing is recorded.
#[tokio::test]
async fn when_the_resend_fails_too_the_old_error_stands() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("bad.png"), png(100, 100, 2_000)).unwrap();
    let p = Model::new(Box::new(|_, n| match n {
        0 => Ok(reads(&["bad.png"], "a")),
        _ => Err(refused()),
    }));
    let b = backend(p.clone(), NativeConfig::default());
    let host = Host::new();
    let t = Turn {
        b: &b,
        host: &host,
        ws: ws.path(),
        agent: agent(&["m"], &[], &["read"]),
    };
    let o = t.run("t1", "Look", None).await;
    assert_eq!(o.reason, TurnEndReason::Error);
    assert!(o.error.unwrap().contains("Could not process image"));
    assert!(host.withheld().is_empty(), "nothing recorded");
    let reqs = p.requests();
    assert_eq!(reqs.len(), 3, "the read, the refused request, one resend");
    assert!(carries(&reqs[1], "bad.png") && !carries(&reqs[2], "bad.png"));
}

/// No resend for errors that are not about the request's content: auth,
/// rate limits, server errors, an overflow.
#[tokio::test]
async fn other_errors_are_not_resent() {
    for (status, body) in [
        (401u16, r#"{"error":{"message":"bad key"}}"#),
        (
            429,
            r#"{"error":{"message":"slow down","type":"insufficient_quota"}}"#,
        ),
        (
            400,
            r#"{"error":{"code":"context_length_exceeded","message":"maximum context length"}}"#,
        ),
    ] {
        let ws = tempfile::tempdir().unwrap();
        std::fs::write(ws.path().join("a.png"), png(10, 10, 100)).unwrap();
        let p = Model::new(Box::new(move |_, n| match n {
            0 => Ok(reads(&["a.png"], "a")),
            _ => Err(ProviderError::Status {
                status,
                body: body.into(),
                hint: RetryHint {
                    no_retry: true,
                    ..Default::default()
                },
            }),
        }));
        let cfg = NativeConfig {
            summarize_after_tokens: Some(0),
            ..Default::default()
        };
        let b = backend(p.clone(), cfg);
        let host = Host::new();
        let t = Turn {
            b: &b,
            host: &host,
            ws: ws.path(),
            agent: agent(&["m"], &[], &["read"]),
        };
        t.run("t1", "Look", None).await;
        assert!(host.withheld().is_empty(), "{status}");
        assert!(
            p.requests()[1..].iter().all(|r| carries(r, "a.png")),
            "{status}: no request went without the image"
        );
    }
}

/// A withheld file read again is a new call: it is shown, alone.
#[tokio::test]
async fn a_re_read_of_a_withheld_file_is_shown() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("bad.png"), png(100, 100, 2_000)).unwrap();
    let p = Model::new(Box::new(|req, n| {
        // Only the first call's image is refused.
        if carried_calls(req).contains(&"a_0".to_string()) {
            return Err(refused());
        }
        Ok(match n {
            0 => reads(&["bad.png"], "a"),
            2 => reads(&["bad.png"], "b"),
            _ => text("done"),
        })
    }));
    let b = backend(p.clone(), NativeConfig::default());
    let host = Host::new();
    let t = Turn {
        b: &b,
        host: &host,
        ws: ws.path(),
        agent: agent(&["m"], &[], &["read"]),
    };
    let o = t.run("t1", "Look", None).await;
    assert_eq!(o.reason, TurnEndReason::Completed, "{:?}", o.error);
    let last = p.requests().pop().unwrap();
    assert_eq!(carried_calls(&last), ["b_0"]);
}

/// Two images in one step, one of them bad: both are withheld (which was
/// refused is not known), the record names both calls, and both stay in
/// the request as the harness's line, never their bytes.
#[tokio::test]
async fn two_images_one_bad_both_withheld() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("bad.png"), png(100, 100, 2_000)).unwrap();
    std::fs::write(ws.path().join("good.png"), png(10, 10, 300)).unwrap();
    let p = Model::new(Box::new(|req, n| {
        if carries(req, "bad.png") {
            return Err(refused());
        }
        Ok(match n {
            0 => reads(&["good.png", "bad.png"], "a"),
            _ => text("done"),
        })
    }));
    let b = backend(p.clone(), NativeConfig::default());
    let host = Host::new();
    let t = Turn {
        b: &b,
        host: &host,
        ws: ws.path(),
        agent: agent(&["m"], &[], &["read"]),
    };
    let o = t.run("t1", "Look at both", None).await;
    assert_eq!(o.reason, TurnEndReason::Completed, "{:?}", o.error);
    assert_eq!(host.withheld()[0]["calls"], json!(["a_0", "a_1"]));
    let last = p.requests().pop().unwrap();
    let imgs: Vec<_> = last
        .messages
        .iter()
        .filter_map(|m| match m {
            ChatMsg::ToolImage { image, .. } => Some((image.name.clone(), image.data.is_some())),
            _ => None,
        })
        .collect();
    assert_eq!(
        imgs,
        [
            ("good.png".to_string(), false),
            ("bad.png".to_string(), false)
        ]
    );
}

/// A fallback that refuses the image. A model moves to its fallback only
/// before the turn's first output, so: turn 1 on `m` reads the image and
/// `m` takes it; turn 2's `m` is down, the turn moves to `m2`, whose first
/// request carries turn 1's image and is refused: it is sent again without
/// it, on `m2`, and the turn completes.
#[tokio::test]
async fn a_fallback_that_refuses_the_image() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("bad.png"), png(100, 100, 2_000)).unwrap();
    let p = Model::new(Box::new(move |req, n| {
        let second = req
            .messages
            .iter()
            .any(|m| m.user_text().is_some_and(|t| t.contains("SECOND")));
        if req.model == "m" && second {
            return Err(ProviderError::Status {
                status: 503,
                body: "down".into(),
                hint: RetryHint {
                    no_retry: true,
                    ..Default::default()
                },
            });
        }
        if req.model == "m2" && carries(req, "bad.png") {
            return Err(refused());
        }
        Ok(if n == 0 {
            reads(&["bad.png"], "a")
        } else {
            text("done")
        })
    }));
    let b = backend(p.clone(), NativeConfig::default());
    let host = Host::new();
    let t = Turn {
        b: &b,
        host: &host,
        ws: ws.path(),
        agent: agent(&["m", "m2"], &["m2"], &["read"]),
    };
    let o = t.run("t1", "Look", None).await;
    assert_eq!(o.reason, TurnEndReason::Completed, "{:?}", o.error);
    assert!(host.withheld().is_empty());
    let o = t.run("t2", "SECOND look", None).await;
    assert_eq!(o.reason, TurnEndReason::Completed, "{:?}", o.error);
    let rec = host.withheld();
    assert_eq!(rec.len(), 1);
    assert_eq!(rec[0]["calls"], json!(["a_0"]));
    let reqs = p.requests();
    let last = reqs.last().unwrap();
    assert_eq!(last.model, "m2");
    assert!(!carries(last, "bad.png"));
    assert!(host.log().iter().any(|e| e.kind == "model_fallback"));
}

/// The record names calls, a status and the provider's reason scrubbed of
/// the node's secrets and cut short: no bytes, no secret, no long text.
#[tokio::test]
async fn the_record_holds_no_bytes_and_no_unscrubbed_text() {
    let ws = tempfile::tempdir().unwrap();
    let bytes = png(100, 100, 50_000);
    std::fs::write(ws.path().join("bad.png"), &bytes).unwrap();
    let p = Model::new(Box::new(|req, n| {
        if carries(req, "bad.png") {
            return Err(ProviderError::Status {
                status: 422,
                body: format!(
                    r#"{{"error":{{"message":"cannot read image (key sk-SECRET-123) {}"}}}}"#,
                    "x".repeat(10_000)
                ),
                hint: RetryHint::default(),
            });
        }
        Ok(match n {
            0 => reads(&["bad.png"], "a"),
            _ => text("done"),
        })
    }));
    let cfg = NativeConfig {
        secrets: harness_core::SecretSet::new(vec![harness_core::Secret::new(
            "sk-SECRET-123".to_string(),
        )]),
        ..Default::default()
    };
    let b = backend(p.clone(), cfg);
    let host = Host::new();
    let t = Turn {
        b: &b,
        host: &host,
        ws: ws.path(),
        agent: agent(&["m"], &[], &["read"]),
    };
    t.run("t1", "Look", None).await;
    let rec = host.withheld();
    assert_eq!(rec.len(), 1);
    let raw = rec[0].to_string();
    assert!(!raw.contains("sk-SECRET-123"), "{raw}");
    assert!(!raw.contains(&b64(&bytes)[..100]));
    assert!(raw.len() < 1_000, "{} bytes", raw.len());
    assert_eq!(rec[0]["status"], json!(422));
    assert!(
        rec[0]["reason"]
            .as_str()
            .unwrap()
            .contains("cannot read image")
    );
}

/// The reviewer's memory proof, turned around: an image from an earlier
/// turn, still in the window, is loaded once, not once a step, and what
/// the turn holds stays within one copy over 40 steps.
#[tokio::test]
async fn earlier_turn_images_are_loaded_once_and_held_bounded() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("big.png"), png(2000, 2000, 3 * 1024 * 1024)).unwrap();
    for k in 0..50 {
        std::fs::write(ws.path().join(format!("t{k}.txt")), format!("text {k}\n")).unwrap();
    }
    let p = Model::new(Box::new(|req, _| {
        // Count this turn's steps from its own results.
        let since = req
            .messages
            .iter()
            .rposition(|m| m.user_text().is_some())
            .unwrap_or(0);
        let done = req.messages[since..]
            .iter()
            .filter(|m| matches!(m, ChatMsg::Tool { .. }))
            .count();
        let first = req
            .messages
            .iter()
            .any(|m| m.user_text().is_some_and(|t| t.contains("FIRST")));
        let is_t1 = first
            && req
                .messages
                .iter()
                .filter(|m| m.user_text().is_some())
                .count()
                == 1;
        Ok(if is_t1 && done == 0 {
            reads(&["big.png"], &format!("i{done}"))
        } else if done < 40 {
            reads(&[&format!("t{done}.txt")], &format!("s{done}"))
        } else {
            text("ok")
        })
    }));
    let cfg = NativeConfig {
        max_steps: 200,
        ..Default::default()
    };
    let host = Arc::new(Host::new());
    *p.host.lock().unwrap() = Some(host.clone());
    let b = backend(p.clone(), cfg);
    let t = Turn {
        b: &b,
        host: &host,
        ws: ws.path(),
        agent: agent(&["m"], &[], &["read"]),
    };
    assert_eq!(
        t.run("t1", "FIRST", None).await.reason,
        TurnEndReason::Completed
    );
    let one = b64(&std::fs::read(ws.path().join("big.png")).unwrap()).len();
    let mark = p.held.lock().unwrap().len();
    let loads = host.loads.lock().unwrap().len();
    assert_eq!(
        t.run("t2", "again", None).await.reason,
        TurnEndReason::Completed
    );
    let held: Vec<usize> = p.held.lock().unwrap()[mark..].to_vec();
    assert!(held.len() > 40, "{} requests", held.len());
    assert!(
        held.iter().all(|h| *h <= one + one / 20),
        "held at most one copy ({one} bytes): {:?}",
        &held[held.len() - 3..]
    );
    let new_loads = host.loads.lock().unwrap().len() - loads;
    assert!(
        new_loads <= 1,
        "the image was loaded {new_loads} times in turn 2"
    );
}

/// Images read this turn are not kept in memory past the window: twelve
/// images of 1 MiB over three steps, then twenty steps of text; at every
/// request the turn holds at most the window's four.
#[tokio::test]
async fn images_read_this_turn_are_held_only_while_in_the_window() {
    let ws = tempfile::tempdir().unwrap();
    for k in 0..12 {
        std::fs::write(
            ws.path().join(format!("{k}.png")),
            png(64, 64, (1 << 20) + k),
        )
        .unwrap();
    }
    for k in 0..30 {
        std::fs::write(ws.path().join(format!("t{k}.txt")), format!("text {k}\n")).unwrap();
    }
    let p = Model::new(Box::new(|_, n| {
        Ok(if n < 3 {
            let names: Vec<String> = (0..4).map(|k| format!("{}.png", n * 4 + k)).collect();
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            reads(&refs, &format!("i{n}"))
        } else if n < 23 {
            reads(&[&format!("t{n}.txt")], &format!("s{n}"))
        } else {
            text("ok")
        })
    }));
    let cfg = NativeConfig {
        max_steps: 100,
        ..Default::default()
    };
    let host = Arc::new(Host::new());
    *p.host.lock().unwrap() = Some(host.clone());
    let b = backend(p.clone(), cfg);
    let t = Turn {
        b: &b,
        host: &host,
        ws: ws.path(),
        agent: agent(&["m"], &[], &["read"]),
    };
    assert_eq!(
        t.run("t1", "go", None).await.reason,
        TurnEndReason::Completed
    );
    let window = 4 * 4 * ((1 << 20) + 200) / 3;
    let held = p.held.lock().unwrap().clone();
    assert!(
        held.iter().all(|h| *h <= window),
        "at most the window ({window}): {held:?}"
    );
    // Each request shows at most four images, every one with its bytes.
    for r in p.requests() {
        assert!(carried_calls(&r).len() <= 4);
    }
}

/// The cap is on what the provider gets: a file's base64 must fit
/// Anthropic's 5 MB per image. 4.5 MiB is refused with a note; 3.5 MiB is
/// sent, and its base64 is under 5 MiB.
#[tokio::test]
async fn the_cap_keeps_the_base64_under_five_mib() {
    let ws = tempfile::tempdir().unwrap();
    let big = png(3000, 2000, 4 * 1024 * 1024 + 512 * 1024);
    let fits = png(3000, 2000, (7 << 19) - 200);
    assert!(fits.len() as u64 <= 7 << 19);
    std::fs::write(ws.path().join("big.png"), &big).unwrap();
    std::fs::write(ws.path().join("fits.png"), &fits).unwrap();
    let p = Model::new(Box::new(|_, n| {
        Ok(match n {
            0 => reads(&["big.png"], "a"),
            1 => reads(&["fits.png"], "b"),
            _ => text("ok"),
        })
    }));
    let b = backend(p.clone(), NativeConfig::default());
    let host = Host::new();
    let t = Turn {
        b: &b,
        host: &host,
        ws: ws.path(),
        agent: agent(&["m"], &[], &["read"]),
    };
    t.run("t1", "look", None).await;
    let fin: Vec<Value> = host
        .log()
        .into_iter()
        .filter(|e| e.kind == "tool_call_finished")
        .map(|e| e.payload)
        .collect();
    assert_eq!(fin[0]["ok"], json!(false), "{}", fin[0]);
    assert!(
        fin[0]["output"].as_str().unwrap().contains("3.5 MiB"),
        "{}",
        fin[0]
    );
    let sizes: Vec<usize> = p.sizes.lock().unwrap().concat();
    assert!(!sizes.is_empty());
    assert!(sizes.iter().all(|s| *s <= 5 << 20), "{sizes:?}");
}

/// A blob the store lost: the request keeps the image's place as the
/// harness's line (no bytes), deterministically, and the next turn works.
#[tokio::test]
async fn a_lost_blob_keeps_its_place_without_bytes() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("a.png"), png(10, 10, 500)).unwrap();
    let p = Model::new(Box::new(|_, n| {
        Ok(match n {
            0 => reads(&["a.png"], "a"),
            _ => text("ok"),
        })
    }));
    let b = backend(p.clone(), NativeConfig::default());
    let host = Host::new();
    let t = Turn {
        b: &b,
        host: &host,
        ws: ws.path(),
        agent: agent(&["m"], &[], &["read"]),
    };
    t.run("t1", "look", None).await;
    host.store.lock().unwrap().clear();
    let m = rebuilt(&host.log(), &[]);
    let imgs: Vec<bool> = m
        .iter()
        .filter_map(|x| match x {
            ChatMsg::ToolImage { image, .. } => Some(image.data.is_some()),
            _ => None,
        })
        .collect();
    assert_eq!(imgs, [false], "kept as a line, without bytes");
    let o = t.run("t2", "again", None).await;
    assert_eq!(o.reason, TurnEndReason::Completed);
    let last = p.requests().pop().unwrap();
    assert_eq!(
        last.messages
            .iter()
            .filter(|x| matches!(x, ChatMsg::ToolImage { image, .. } if image.data.is_none()))
            .count(),
        1
    );
}

/// A model without vision read an image file and saw nothing: the file
/// does not count as read, so `write` may not replace it.
#[tokio::test]
async fn a_model_without_vision_did_not_read_the_file() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("a.png"), png(10, 10, 500)).unwrap();
    let p = Model::new(Box::new(|_, n| {
        Ok(match n {
            0 => reads(&["a.png"], "a"),
            1 => Completion {
                tool_calls: vec![ToolCallRef {
                    call_id: "w".into(),
                    tool: "write".into(),
                    input: json!({"path": "a.png", "content": "x"}),
                }],
                ..Default::default()
            },
            _ => text("ok"),
        })
    }));
    let b = backend(p.clone(), NativeConfig::default());
    let host = Host::new();
    let t = Turn {
        b: &b,
        host: &host,
        ws: ws.path(),
        agent: agent(&[], &[], &["read", "write"]),
    };
    t.run("t1", "look", None).await;
    let fin: Vec<Value> = host
        .log()
        .into_iter()
        .filter(|e| e.kind == "tool_call_finished")
        .map(|e| e.payload)
        .collect();
    assert!(fin[0].get("seen").is_none(), "{}", fin[0]);
    assert_eq!(fin[1]["ok"], json!(false), "{}", fin[1]);
    assert_ne!(std::fs::read(ws.path().join("a.png")).unwrap(), b"x");
}
