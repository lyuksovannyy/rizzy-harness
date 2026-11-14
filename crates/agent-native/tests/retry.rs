//! Retry with backoff: a passing provider failure (rate limit, overloaded
//! server, dropped connection) is retried a bounded number of times before
//! the agent's fallbacks; a refused request is not; nothing is retried once
//! the user has seen output; a cancel ends the wait at once.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::response::IntoResponse;
use axum::{Router, routing::post};
use harness_agent_native::llm::{ApiKind, LlmProvider, ModelRouter, Route};
use harness_agent_native::{
    ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError, RetryHint,
    RetryPolicy,
};
use harness_core::agent::{AgentSpec, ToolSelection};
use harness_core::backend::{AgentBackend, TurnCtx, TurnHost, TurnOutcome};
use harness_core::event::{EventKind, TurnEndReason};
use harness_core::policy::{Decision, ToolRisk};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct Host(Mutex<Vec<EventKind>>);

#[async_trait::async_trait]
impl TurnHost for Host {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        self.0.lock().unwrap().push(kind);
        Ok(())
    }
    async fn approve(&self, _: &str, _: &str, _: &Value, _: ToolRisk) -> Decision {
        Decision::Deny
    }
}

impl Host {
    fn fallbacks(&self) -> usize {
        let evs = self.0.lock().unwrap();
        evs.iter()
            .filter(|k| matches!(k, EventKind::ModelFallback(_)))
            .count()
    }
    fn answer(&self) -> Option<String> {
        self.0.lock().unwrap().iter().find_map(|k| match k {
            EventKind::AssistantMessage(m) => Some(m.text.clone()),
            _ => None,
        })
    }
}

/// One scripted reply.
enum Reply {
    Fail(u16),
    /// Fails with a status and a `Retry-After` of this many seconds.
    FailAfter(u16, u64),
    /// Streams some text first, then fails with a 503.
    FailAfterText,
    Ok,
}

/// Answers per model from a script (`Ok` once the script is used up) and
/// records each call's model.
#[derive(Default)]
struct Scripted {
    script: Mutex<std::collections::HashMap<String, VecDeque<Reply>>>,
    calls: Mutex<Vec<String>>,
}

impl Scripted {
    fn new(script: Vec<(&str, Vec<Reply>)>) -> Arc<Self> {
        Arc::new(Self {
            script: Mutex::new(
                script
                    .into_iter()
                    .map(|(m, r)| (m.to_string(), r.into()))
                    .collect(),
            ),
            calls: Mutex::default(),
        })
    }
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

fn status(status: u16) -> ProviderError {
    ProviderError::Status {
        status,
        body: format!("{{\"error\":{{\"message\":\"failed with {status}\"}}}}"),
        hint: RetryHint::default(),
    }
}

#[async_trait::async_trait]
impl Provider for Scripted {
    fn name(&self) -> &'static str {
        "scripted"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        deltas: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        self.calls.lock().unwrap().push(req.model.clone());
        let next = self
            .script
            .lock()
            .unwrap()
            .get_mut(&req.model)
            .and_then(|q| q.pop_front());
        match next.unwrap_or(Reply::Ok) {
            Reply::Fail(s) => Err(status(s)),
            Reply::FailAfter(s, secs) => Err(ProviderError::Status {
                status: s,
                body: "{\"error\":{\"message\":\"secret-ish body\"}}".into(),
                hint: RetryHint {
                    after: Some(Duration::from_secs(secs)),
                    no_retry: false,
                },
            }),
            Reply::FailAfterText => {
                let _ = deltas.send("half an ans".into());
                tokio::time::sleep(harness_agent_native::FLUSH_INTERVAL * 3).await;
                Err(status(503))
            }
            Reply::Ok => Ok(Completion {
                text: format!("answer from {}", req.model),
                ..Default::default()
            }),
        }
    }
}

fn fast() -> RetryPolicy {
    RetryPolicy {
        retries: 3,
        base: Duration::from_millis(1),
        max: Duration::from_millis(20),
    }
}

fn backend(p: Arc<dyn Provider>, retry: RetryPolicy) -> NativeBackend {
    NativeBackend::new(
        p,
        Vec::new(),
        NativeConfig {
            retry,
            ..Default::default()
        },
    )
}

fn agent(fallbacks: &[&str]) -> AgentSpec {
    AgentSpec {
        agent_id: "general".into(),
        name: "General".into(),
        system_prompt: "Be brief.".into(),
        tools: ToolSelection::List(vec![]),
        model: None,
        fallbacks: fallbacks.iter().map(|s| s.to_string()).collect(),
        unavailable: vec![],
        no_effort: vec![],
        context_windows: Default::default(),
        effort: None,
        budget: Default::default(),
        ui: Default::default(),
        prompt_hash: "h".into(),
        deny_tools: vec![],
        delegates: vec![],
        caps: None,
    }
}

async fn run_with(
    b: &NativeBackend,
    host: &Host,
    model: &str,
    agent: AgentSpec,
    cancel: CancellationToken,
) -> TurnOutcome {
    b.run_turn(TurnCtx {
        session_id: harness_core::SessionId::new(),
        turn_id: "t".into(),
        history: vec![],
        user_text: "hi".into(),
        cwd: Default::default(),
        model: model.into(),
        agent: Some(agent),
        compare: vec![],
        parent_session: None,
        extra: Default::default(),
        cancel,
        host,
    })
    .await
}

async fn run(b: &NativeBackend, host: &Host, model: &str, agent: AgentSpec) -> TurnOutcome {
    run_with(b, host, model, agent, CancellationToken::new()).await
}

#[tokio::test]
async fn rate_limit_then_success_is_retried_without_falling_back() {
    let p = Scripted::new(vec![("m", vec![Reply::Fail(429)])]);
    let b = backend(p.clone(), fast());
    let host = Host::default();
    let out = run(&b, &host, "m", agent(&["fb"])).await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    assert_eq!(p.calls(), ["m", "m"]);
    assert_eq!(host.answer().as_deref(), Some("answer from m"));
    assert_eq!(host.fallbacks(), 0, "a retry is not a fallback");
}

#[tokio::test]
async fn refused_requests_are_not_retried() {
    for code in [400, 401, 403] {
        let p = Scripted::new(vec![("m", vec![Reply::Fail(code)])]);
        let b = backend(p.clone(), fast());
        let host = Host::default();
        let out = run(&b, &host, "m", agent(&[])).await;
        assert_eq!(out.reason, TurnEndReason::Error, "{code}");
        assert_eq!(p.calls(), ["m"], "{code} was retried");
    }
}

#[tokio::test]
async fn attempts_are_bounded_then_the_fallback_answers() {
    let always = || (0..10).map(|_| Reply::Fail(503)).collect::<Vec<_>>();
    // No fallback: 1 attempt + 3 retries, then the turn fails.
    let p = Scripted::new(vec![("m", always())]);
    let b = backend(p.clone(), fast());
    let out = run(&b, &Host::default(), "m", agent(&[])).await;
    assert_eq!(out.reason, TurnEndReason::Error);
    assert_eq!(p.calls(), ["m"; 4]);

    // With one: it answers after the retries of the first are used up.
    let p = Scripted::new(vec![("m", always())]);
    let b = backend(p.clone(), fast());
    let host = Host::default();
    let out = run(&b, &host, "m", agent(&["fb"])).await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    assert_eq!(p.calls(), ["m", "m", "m", "m", "fb"]);
    assert_eq!(host.answer().as_deref(), Some("answer from fb"));
    assert_eq!(host.fallbacks(), 1);
}

#[tokio::test]
async fn no_retry_once_output_has_streamed() {
    let p = Scripted::new(vec![("m", vec![Reply::FailAfterText])]);
    let b = backend(p.clone(), fast());
    let host = Host::default();
    let out = run(&b, &host, "m", agent(&["fb"])).await;
    assert_eq!(out.reason, TurnEndReason::Error);
    assert_eq!(p.calls(), ["m"], "a partial answer must not be repeated");
}

#[tokio::test]
async fn cancel_ends_the_wait_at_once() {
    let p = Scripted::new(vec![("m", vec![Reply::Fail(503)])]);
    let slow = RetryPolicy {
        retries: 3,
        base: Duration::from_secs(30),
        max: Duration::from_secs(60),
    };
    let b = backend(p.clone(), slow);
    let host = Host::default();
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        c2.cancel();
    });
    let t = Instant::now();
    let out = run_with(&b, &host, "m", agent(&[]), cancel).await;
    assert_eq!(out.reason, TurnEndReason::Cancelled);
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    assert_eq!(p.calls(), ["m"]);
}

/// An OpenAI-compatible server that rate-limits each model's first request
/// with the given headers, then answers.
async fn serve(headers: Vec<(&'static str, &'static str)>) -> (String, Arc<Mutex<u32>>) {
    serve_status(429, headers).await
}

/// The same, failing the first request with `status`.
async fn serve_status(
    status: u16,
    headers: Vec<(&'static str, &'static str)>,
) -> (String, Arc<Mutex<u32>>) {
    let hits = Arc::new(Mutex::new(0u32));
    let h2 = hits.clone();
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move |_body: String| {
            let (hits, headers) = (h2.clone(), headers.clone());
            async move {
                let n = {
                    let mut h = hits.lock().unwrap();
                    *h += 1;
                    *h
                };
                if n == 1 {
                    let mut r = (
                        axum::http::StatusCode::from_u16(status).unwrap(),
                        axum::Json(json!({"error": {"message": "Rate limit reached"}})),
                    )
                        .into_response();
                    for (k, v) in headers {
                        r.headers_mut().insert(k, v.parse().unwrap());
                    }
                    return r;
                }
                let chunk = json!({"choices":[{"index":0,"delta":{"content":"ok"}}]});
                (
                    [("content-type", "text/event-stream")],
                    format!("data: {chunk}\n\ndata: [DONE]\n\n"),
                )
                    .into_response()
            }
        }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (base, hits)
}

fn http_backend(base: &str, retry: RetryPolicy) -> NativeBackend {
    let key = harness_core::Secret::new("sk-test".into());
    let p = LlmProvider::new(ApiKind::Openai, base, Some(key)).unwrap();
    let router = ModelRouter::new(vec![Route {
        prefix: "loc".into(),
        provider: Arc::new(p),
    }]);
    backend(Arc::new(router), retry)
}

#[tokio::test]
async fn retry_after_is_honoured() {
    let (base, hits) = serve(vec![("retry-after-ms", "150")]).await;
    let b = http_backend(
        &base,
        RetryPolicy {
            retries: 3,
            base: Duration::from_millis(1),
            max: Duration::from_secs(10),
        },
    );
    let t = Instant::now();
    let out = run(&b, &Host::default(), "loc::m", agent(&[])).await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    assert_eq!(*hits.lock().unwrap(), 2);
    assert!(
        t.elapsed() >= Duration::from_millis(150),
        "waited only {:?}",
        t.elapsed()
    );
}

/// The cap here is 2 s, so a wait that is clamped to it (the old behaviour)
/// takes at least 2 s and a second request.
fn capped() -> RetryPolicy {
    RetryPolicy {
        retries: 3,
        base: Duration::from_millis(1),
        max: Duration::from_secs(2),
    }
}

#[tokio::test]
async fn a_retry_after_over_the_cap_is_not_retried_and_names_the_wait() {
    for status in [429, 503] {
        let (base, hits) = serve_status(status, vec![("retry-after", "300")]).await;
        let b = http_backend(&base, capped());
        let t = Instant::now();
        let out = run(&b, &Host::default(), "loc::m", agent(&[])).await;
        assert_eq!(out.reason, TurnEndReason::Error, "{status}");
        assert_eq!(*hits.lock().unwrap(), 1, "{status}: sent again too early");
        assert!(t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());
        let msg = out.error.unwrap();
        assert!(msg.contains("5 minutes"), "{msg}");
        assert!(msg.contains("loc::m"), "{msg}");
        assert!(!msg.contains("Rate limit reached"), "body leaked: {msg}");
    }
}

#[tokio::test]
async fn a_retry_after_over_the_cap_goes_to_the_fallback_at_once() {
    for status in [429, 503] {
        let p = Scripted::new(vec![("m", vec![Reply::FailAfter(status, 300)])]);
        let b = backend(p.clone(), capped());
        let host = Host::default();
        let t = Instant::now();
        let out = run(&b, &host, "m", agent(&["fb"])).await;
        assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
        assert_eq!(p.calls(), ["m", "fb"], "{status}");
        assert!(t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());
        assert_eq!(host.answer().as_deref(), Some("answer from fb"));
        assert_eq!(host.fallbacks(), 1);
    }
}

#[tokio::test]
async fn a_retry_after_within_the_cap_is_still_waited_and_retried() {
    let (base, hits) = serve(vec![("retry-after", "1")]).await;
    let b = http_backend(&base, capped());
    let t = Instant::now();
    let out = run(&b, &Host::default(), "loc::m", agent(&[])).await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    assert_eq!(*hits.lock().unwrap(), 2);
    assert!(t.elapsed() >= Duration::from_secs(1), "{:?}", t.elapsed());
}

impl Host {
    fn retries(&self) -> Vec<harness_core::event::ModelRetry> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|k| match k {
                EventKind::ModelRetry(r) => Some(r.clone()),
                _ => None,
            })
            .collect()
    }
}

/// The person sees each wait (model, attempt, when, why) before it starts,
/// and never the provider's body.
#[tokio::test]
async fn each_retry_is_recorded_before_its_wait() {
    use harness_core::event::RetryReason;
    let p = Scripted::new(vec![("m", vec![Reply::Fail(429), Reply::Fail(503)])]);
    let b = backend(p.clone(), fast());
    let host = Host::default();
    let before = harness_core::now_ms();
    let out = run(&b, &host, "m", agent(&[])).await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    let r = host.retries();
    assert_eq!(r.len(), 2, "{r:?}");
    assert_eq!(
        (r[0].attempt, r[0].of, r[0].reason),
        (1, 3, RetryReason::RateLimit)
    );
    assert_eq!((r[1].attempt, r[1].reason), (2, RetryReason::Overloaded));
    assert!(
        r.iter()
            .all(|r| r.model == "m" && r.turn_id == "t" && r.at_ms >= before)
    );
    // Before the answer it waited for.
    let kinds: Vec<&str> = host.0.lock().unwrap().iter().map(|k| k.name()).collect();
    let last_retry = kinds.iter().rposition(|k| *k == "model_retry").unwrap();
    let answer = kinds
        .iter()
        .position(|k| *k == "assistant_message")
        .unwrap();
    assert!(last_retry < answer, "{kinds:?}");
}

#[tokio::test]
async fn a_parallel_answer_records_its_retry() {
    let p = Scripted::new(vec![("b", vec![Reply::Fail(502)])]);
    let b = backend(p.clone(), fast());
    let host = Host::default();
    let out = b
        .run_turn(TurnCtx {
            session_id: harness_core::SessionId::new(),
            turn_id: "t".into(),
            history: vec![],
            user_text: "hi".into(),
            cwd: Default::default(),
            model: "a".into(),
            agent: Some(agent(&[])),
            compare: vec!["a".into(), "b".into()],
            parent_session: None,
            extra: Default::default(),
            cancel: CancellationToken::new(),
            host: &host,
        })
        .await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    let r = host.retries();
    assert_eq!(r.len(), 1, "{r:?}");
    assert_eq!(r[0].model, "b");
    assert_eq!(r[0].reason, harness_core::event::RetryReason::ServerError);
}

/// The turn's usage carries the window of the model it started on and
/// where the next summary is written, for the chat's meter.
#[tokio::test]
async fn the_turn_reports_its_window_and_summary_point() {
    let p = Scripted::new(vec![]);
    let b = backend(p.clone(), fast());
    let mut a = agent(&[]);
    a.context_windows.insert("m".into(), 200_000);
    let out = run(&b, &Host::default(), "m", a).await;
    let u = out.usage.unwrap();
    assert_eq!(u.window, Some(200_000));
    assert_eq!(u.summary_at, Some(160_000));
    // Unknown: no window, and the fixed trigger.
    let out = run(&b, &Host::default(), "m", agent(&[])).await;
    let u = out.usage.unwrap();
    assert_eq!((u.window, u.summary_at), (None, Some(24_000)));
}
