//! Agent fallbacks: a turn whose model fails before any output, or is
//! known to be unavailable, is answered by the next fallback, and the
//! session is told; once output has started there is no second try.

use std::sync::{Arc, Mutex};

use axum::response::IntoResponse;
use axum::{Router, routing::post};
use harness_agent_native::llm::{ApiKind, LlmProvider, ModelRouter, Route};
use harness_agent_native::{
    ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError, RetryPolicy,
};
use harness_core::agent::{AgentSpec, ToolSelection};
use harness_core::backend::{AgentBackend, TurnCtx, TurnHost};
use harness_core::event::{EventKind, ModelFallback, TurnEndReason};
use harness_core::policy::{Decision, ToolRisk};
use serde_json::{Value, json};
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
        Decision::Deny
    }
}

impl Host {
    fn fallbacks(&self) -> Vec<ModelFallback> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|k| match k {
                EventKind::ModelFallback(f) => Some(f.clone()),
                _ => None,
            })
            .collect()
    }

    fn answer(&self) -> Option<String> {
        self.0.lock().unwrap().iter().find_map(|k| match k {
            EventKind::AssistantMessage(m) => Some(m.text.clone()),
            _ => None,
        })
    }

    /// The fallback note comes before the first text of the answer.
    fn note_first(&self) -> bool {
        let evs = self.0.lock().unwrap();
        let note = evs
            .iter()
            .position(|k| matches!(k, EventKind::ModelFallback(_)));
        let text = evs.iter().position(|k| {
            matches!(
                k,
                EventKind::AssistantDelta(_) | EventKind::AssistantMessage(_)
            )
        });
        matches!((note, text), (Some(n), Some(t)) if n < t)
    }
}

type Seen = Arc<Mutex<Vec<String>>>;

/// An OpenAI-compatible server: `gone` is not found, `busy` is rate
/// limited, `bad-key` is refused, every other model streams an answer
/// naming itself. Records the models asked for, and the reasoning effort
/// each request carried ("-" for none).
async fn serve_efforts() -> (String, Seen, Seen) {
    let seen: Seen = Arc::default();
    let efforts: Seen = Arc::default();
    let (s2, e2) = (seen.clone(), efforts.clone());
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move |body: String| {
            let (seen, efforts) = (s2.clone(), e2.clone());
            async move {
                let req: Value = serde_json::from_str(&body).unwrap();
                let model = req["model"].as_str().unwrap().to_string();
                seen.lock().unwrap().push(model.clone());
                let effort = req["reasoning_effort"].as_str().unwrap_or("-").to_string();
                efforts.lock().unwrap().push(effort);
                let err = |code: u16, msg: &str| {
                    (
                        axum::http::StatusCode::from_u16(code).unwrap(),
                        axum::Json(json!({"error": {"message": msg}})),
                    )
                        .into_response()
                };
                match model.as_str() {
                    "gone" => err(404, "The model `gone` does not exist"),
                    "busy" => err(429, "Rate limit reached"),
                    "bad-key" => err(401, "Incorrect API key"),
                    _ => {
                        let chunk = |t: &str| {
                            format!(
                                "data: {}\n\n",
                                json!({"choices":[{"index":0,"delta":{"content":t}}]})
                            )
                        };
                        let body = chunk("answer from ") + &chunk(&model) + "data: [DONE]\n\n";
                        ([("content-type", "text/event-stream")], body).into_response()
                    }
                }
            }
        }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (base, seen, efforts)
}

async fn serve() -> (String, Seen) {
    let (base, seen, _) = serve_efforts().await;
    (base, seen)
}

fn backend(base: &str) -> NativeBackend {
    let key = harness_core::Secret::new("sk-test".into());
    let p = LlmProvider::new(ApiKind::Openai, base, Some(key)).unwrap();
    let router = ModelRouter::new(vec![Route {
        prefix: "loc".into(),
        provider: Arc::new(p),
    }]);
    NativeBackend::new(
        Arc::new(router),
        Vec::new(),
        NativeConfig {
            // `busy` is retried (tests/retry.rs) before its fallback; fast
            // here.
            retry: RetryPolicy {
                retries: BUSY_RETRIES,
                base: std::time::Duration::from_millis(1),
                max: std::time::Duration::from_millis(5),
            },
            ..Default::default()
        },
    )
}

const BUSY_RETRIES: u32 = 2;

fn agent(fallbacks: &[&str], unavailable: &[&str]) -> AgentSpec {
    AgentSpec {
        agent_id: "general".into(),
        name: "General".into(),
        system_prompt: "Be brief.".into(),
        tools: ToolSelection::List(vec![]),
        model: None,
        fallbacks: fallbacks.iter().map(|s| s.to_string()).collect(),
        unavailable: unavailable.iter().map(|s| s.to_string()).collect(),
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

async fn run(
    b: &NativeBackend,
    host: &Host,
    model: &str,
    agent: AgentSpec,
) -> harness_core::backend::TurnOutcome {
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
        cancel: Default::default(),
        host,
    })
    .await
}

#[tokio::test]
async fn model_not_found_falls_back_in_order() {
    let (base, seen) = serve().await;
    let b = backend(&base);
    let host = Host::default();
    let out = run(
        &b,
        &host,
        "loc::gone",
        agent(&["loc::busy", "loc::ok"], &[]),
    )
    .await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    // `gone` is not retried (404), `busy` is (429), then falls back.
    assert_eq!(
        *seen.lock().unwrap(),
        ["gone", "busy", "busy", "busy", "ok"]
    );
    assert_eq!(host.answer().as_deref(), Some("answer from ok"));
    let notes = host.fallbacks();
    assert_eq!(notes.len(), 1, "one note, for the model that answered");
    assert_eq!(
        (&notes[0].model[..], &notes[0].from[..]),
        ("loc::ok", "loc::gone")
    );
    // Why `gone` was not used: its own error, not the 429 of `busy`.
    let why = notes[0].reason.as_deref().unwrap();
    assert!(
        why.contains("does not exist") && !why.contains("429"),
        "{notes:?}"
    );
    assert!(host.note_first());
}

#[tokio::test]
async fn unavailable_model_is_skipped() {
    let (base, seen) = serve().await;
    let b = backend(&base);
    let host = Host::default();
    // The first fallback is unavailable too: skipped, not tried.
    let a = agent(&["loc::old", "loc::ok"], &["loc::main", "loc::old"]);
    let out = run(&b, &host, "loc::main", a).await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    assert_eq!(
        *seen.lock().unwrap(),
        ["ok"],
        "never asked the unavailable ones"
    );
    let notes = host.fallbacks();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].reason.as_deref(), Some("unavailable"));
    assert!(host.note_first());

    // A fallback failing after an unavailable skip: the note still says
    // why the turn's own model was not used.
    let host = Host::default();
    let a = agent(&["loc::busy", "loc::ok"], &["loc::main"]);
    let out = run(&b, &host, "loc::main", a).await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    let notes = host.fallbacks();
    assert_eq!(
        (&notes[0].model[..], notes[0].reason.as_deref()),
        ("loc::ok", Some("unavailable"))
    );

    // Nothing left but the unavailable model: it is tried anyway.
    let host = Host::default();
    seen.lock().unwrap().clear();
    let out = run(&b, &host, "loc::main", agent(&[], &["loc::main"])).await;
    assert_eq!(out.reason, TurnEndReason::Completed);
    assert_eq!(*seen.lock().unwrap(), ["main"]);
    assert!(host.fallbacks().is_empty());
}

/// A fallback that takes no reasoning effort is asked without one.
#[tokio::test]
async fn effort_only_for_models_that_take_it() {
    let (base, seen, efforts) = serve_efforts().await;
    let b = backend(&base);
    let host = Host::default();
    let mut a = agent(&["loc::ok"], &[]);
    a.effort = Some("high".into());
    a.no_effort = vec!["loc::ok".into()];
    let out = run(&b, &host, "loc::busy", a.clone()).await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    assert_eq!(*seen.lock().unwrap(), ["busy", "busy", "busy", "ok"]);
    assert_eq!(*efforts.lock().unwrap(), ["high", "high", "high", "-"]);

    // Starting on the fallback (the main model is unavailable) too.
    efforts.lock().unwrap().clear();
    a.unavailable = vec!["loc::busy".into()];
    let out = run(&b, &host, "loc::busy", a).await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    assert_eq!(*efforts.lock().unwrap(), ["-"]);
}

#[tokio::test]
async fn request_errors_do_not_fall_back() {
    let (base, seen) = serve().await;
    let b = backend(&base);
    let host = Host::default();
    // A refused key is not a model problem; without fallbacks a model
    // problem just fails.
    let out = run(&b, &host, "loc::bad-key", agent(&["loc::ok"], &[])).await;
    assert_eq!(out.reason, TurnEndReason::Error);
    let out = run(&b, &host, "loc::gone", agent(&[], &[])).await;
    assert_eq!(out.reason, TurnEndReason::Error);
    assert_eq!(*seen.lock().unwrap(), ["bad-key", "gone"]);
    assert!(host.fallbacks().is_empty());
}

/// Streams some text, then fails with a server error.
struct Cut(Mutex<Vec<String>>);

#[async_trait::async_trait]
impl Provider for Cut {
    fn name(&self) -> &'static str {
        "cut"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        deltas: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        self.0.lock().unwrap().push(req.model.clone());
        let _ = deltas.send("half an ans".into());
        // Long enough for the batch to be flushed to the host.
        tokio::time::sleep(harness_agent_native::FLUSH_INTERVAL * 3).await;
        Err(ProviderError::Status {
            status: 503,
            body: "overloaded".into(),
            hint: Default::default(),
        })
    }
}

#[tokio::test]
async fn no_fallback_after_output_started() {
    let p = Arc::new(Cut(Mutex::default()));
    let b = NativeBackend::new(p.clone(), Vec::new(), NativeConfig::default());
    let host = Host::default();
    let out = run(&b, &host, "loc::main", agent(&["loc::ok"], &[])).await;
    assert_eq!(out.reason, TurnEndReason::Error);
    assert_eq!(*p.0.lock().unwrap(), ["loc::main"]);
    assert!(host.fallbacks().is_empty());
}
