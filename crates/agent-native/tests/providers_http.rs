//! Both real providers against local servers that speak each wire format,
//! with streamed text and a tool call split across chunks.

use std::sync::{Arc, Mutex};

use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{Router, routing::post};
use harness_agent_native::llm::{ApiKind, LlmProvider};
use harness_agent_native::{ChatMsg, ChatRequest, Provider, ToolSpec};
use harness_core::Secret;
use serde_json::{Value, json};

type Seen = Arc<Mutex<Option<(HeaderMap, Value)>>>;

async fn serve(path: &'static str, body: String) -> (String, Seen) {
    let seen: Seen = Arc::default();
    let s2 = seen.clone();
    let app = Router::new().route(
        path,
        post(move |headers: HeaderMap, req: String| {
            let seen = s2.clone();
            let body = body.clone();
            async move {
                *seen.lock().unwrap() = Some((headers, serde_json::from_str(&req).unwrap()));
                ([("content-type", "text/event-stream")], body).into_response()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), seen)
}

fn request() -> ChatRequest {
    ChatRequest {
        model: "test-model".into(),
        messages: vec![
            ChatMsg::System("be brief".into()),
            ChatMsg::User("ping".into()),
        ],
        tools: vec![ToolSpec {
            name: "bash".into(),
            description: "run".into(),
            schema: json!({"type": "object", "properties": {"command": {"type": "string"}}}),
        }],
        effort: None,
        cache: false,
    }
}

#[tokio::test]
async fn openai_compatible_stream() {
    let sse = [
        r#"data: {"id":"c","object":"chat.completion.chunk","created":1,"model":"test-model","choices":[{"index":0,"delta":{"role":"assistant","content":"po"},"finish_reason":null}]}"#,
        r#"data: {"id":"c","object":"chat.completion.chunk","created":1,"model":"test-model","choices":[{"index":0,"delta":{"content":"ng"},"finish_reason":null}]}"#,
        r#"data: {"id":"c","object":"chat.completion.chunk","created":1,"model":"test-model","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"bash","arguments":"{\"comm"}}]},"finish_reason":null}]}"#,
        r#"data: {"id":"c","object":"chat.completion.chunk","created":1,"model":"test-model","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"and\":\"ls\"}"}}]},"finish_reason":"tool_calls"}]}"#,
        r#"data: {"id":"c","object":"chat.completion.chunk","created":1,"model":"test-model","choices":[],"usage":{"prompt_tokens":11,"completion_tokens":7,"total_tokens":18}}"#,
        "data: [DONE]",
    ]
    .join("\n\n")
        + "\n\n";
    let (base, seen) = serve("/v1/chat/completions", sse).await;
    let p = LlmProvider::new(
        ApiKind::Openai,
        &format!("{base}/v1"),
        Some(Secret::new("sk-test".into())),
    )
    .unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let c = p.complete(&request(), tx).await.unwrap();
    assert_eq!(c.text, "pong");
    assert_eq!(c.tool_calls.len(), 1);
    assert_eq!(c.tool_calls[0].call_id, "call_1");
    assert_eq!(c.tool_calls[0].input, json!({"command": "ls"}));
    let mut deltas = String::new();
    while let Ok(d) = rx.try_recv() {
        deltas.push_str(&d);
    }
    assert_eq!(deltas, "pong");
    let (headers, body) = seen.lock().unwrap().clone().unwrap();
    assert_eq!(headers["authorization"], "Bearer sk-test");
    assert_eq!(body["model"], "test-model");
    assert_eq!(body["stream"], true);
    assert_eq!(body["tools"][0]["function"]["name"], "bash");
}

#[tokio::test]
async fn keyless_endpoint_still_completes() {
    let sse = [
        r#"data: {"id":"c","object":"chat.completion.chunk","created":1,"model":"test-model","choices":[{"index":0,"delta":{"role":"assistant","content":"ok"},"finish_reason":"stop"}]}"#,
        "data: [DONE]",
    ]
    .join("\n\n")
        + "\n\n";
    let (base, seen) = serve("/v1/chat/completions", sse).await;
    let p = LlmProvider::new(ApiKind::Openai, &format!("{base}/v1"), None).unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let c = p.complete(&request(), tx).await.unwrap();
    assert_eq!(c.text, "ok");
    let (headers, _) = seen.lock().unwrap().clone().unwrap();
    assert_eq!(headers["authorization"], "Bearer no-key");
}

#[tokio::test]
async fn anthropic_stream() {
    let ev = |name: &str, data: Value| format!("event: {name}\ndata: {data}\n\n");
    let sse = [
        ev("message_start", json!({"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"test-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":12,"output_tokens":1}}})),
        ev("content_block_start", json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}})),
        ev("content_block_delta", json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"po"}})),
        ev("content_block_delta", json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ng"}})),
        ev("content_block_stop", json!({"type":"content_block_stop","index":0})),
        ev("content_block_start", json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"bash","input":{}}})),
        ev("content_block_delta", json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"command\":"}})),
        ev("content_block_delta", json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"ls\"}"}})),
        ev("content_block_stop", json!({"type":"content_block_stop","index":1})),
        ev("message_delta", json!({"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":9}})),
        ev("message_stop", json!({"type":"message_stop"})),
    ]
    .concat();
    let (base, seen) = serve("/v1/messages", sse).await;
    let p = LlmProvider::new(
        ApiKind::Anthropic,
        &format!("{base}/v1"),
        Some(Secret::new("sk-ant-test".into())),
    )
    .unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let c = p.complete(&request(), tx).await.unwrap();
    assert_eq!(c.text, "pong");
    assert_eq!(c.tool_calls.len(), 1, "{c:?}");
    assert_eq!(c.tool_calls[0].call_id, "toolu_1");
    assert_eq!(c.tool_calls[0].input, json!({"command": "ls"}));
    let u = c.usage.unwrap();
    assert_eq!(u.input_tokens, 12);
    let (headers, body) = seen.lock().unwrap().clone().unwrap();
    assert_eq!(headers["x-api-key"], "sk-ant-test");
    assert!(headers.contains_key("anthropic-version"));
    assert!(
        !headers.contains_key("authorization"),
        "API key only, no bearer/OAuth"
    );
    assert_eq!(body["stream"], true);
    assert_eq!(body["tools"][0]["name"], "bash");
    // System prompt goes in the top-level field, not as a message.
    assert!(body["system"].to_string().contains("be brief"), "{body}");
}

#[tokio::test]
async fn http_error_is_surfaced_without_secret() {
    let app = Router::new().route(
        "/v1/chat/completions",
        post(|| async { (axum::http::StatusCode::UNAUTHORIZED, "bad key") }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let p = LlmProvider::new(
        ApiKind::Openai,
        &format!("http://{addr}/v1"),
        Some(Secret::new("sk-hidden-123".into())),
    )
    .unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let err = p.complete(&request(), tx).await.unwrap_err().to_string();
    assert!(err.contains("401"), "{err}");
    assert!(!err.contains("sk-hidden-123"), "{err}");
}

#[tokio::test]
async fn list_models_both_kinds_and_bad_key() {
    use axum::routing::get;
    let app = Router::new().route(
        "/v1/models",
        get(|headers: HeaderMap| async move {
            let openai_ok =
                headers.get("authorization").and_then(|v| v.to_str().ok()) == Some("Bearer good");
            let anth_ok = headers.get("x-api-key").and_then(|v| v.to_str().ok()) == Some("good")
                && headers.contains_key("anthropic-version");
            if openai_ok || anth_ok {
                (
                    axum::http::StatusCode::OK,
                    r#"{"data":[{"id":"model-b"},{"id":"model-a"}]}"#,
                )
                    .into_response()
            } else {
                (axum::http::StatusCode::UNAUTHORIZED, "nope").into_response()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    use harness_agent_native::models::list_models;
    let good = Secret::new("good".to_string());
    let bad = Secret::new("sk-wrong-123456".to_string());
    let ids = |v: Vec<harness_agent_native::models::ModelMeta>| {
        v.into_iter().map(|m| m.id).collect::<Vec<_>>()
    };
    assert_eq!(
        ids(
            list_models(ApiKind::Openai, &base, Some(&good), false, &None)
                .await
                .unwrap()
        ),
        ["model-a", "model-b"]
    );
    assert_eq!(
        ids(
            list_models(ApiKind::Anthropic, &base, Some(&good), false, &None)
                .await
                .unwrap()
        ),
        ["model-a", "model-b"]
    );
    let err = list_models(ApiKind::Openai, &base, Some(&bad), false, &None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("401") && !err.contains("sk-wrong"), "{err}");
}

/// OpenRouter-style metadata comes through; Anthropic pages are followed and
/// its effort is never offered (PLAN.md R8).
#[tokio::test]
async fn list_models_metadata_and_anthropic_pages() {
    use axum::extract::Query;
    use axum::routing::get;
    use harness_agent_native::models::list_models;
    let app = Router::new()
        .route(
            "/or/v1/models",
            get(|| async {
                axum::Json(json!({"data": [{
                    "id": "openai/gpt-4o", "name": "OpenAI: GPT-4o", "context_length": 128000,
                    "architecture": {"input_modalities": ["text", "image"]},
                    "top_provider": {"max_completion_tokens": 16384},
                    "supported_parameters": ["tools", "reasoning"]
                }]}))
            }),
        )
        .route(
            "/an/v1/models",
            get(
                |Query(q): Query<std::collections::HashMap<String, String>>| async move {
                    axum::Json(match q.get("after_id").map(String::as_str) {
                        None => json!({"data": [{
                            "id": "claude-b", "display_name": "Claude B",
                            "max_input_tokens": 200000, "max_tokens": 64000,
                            "capabilities": {"image_input": {"supported": true},
                                             "thinking": {"supported": true}}
                        }], "has_more": true, "last_id": "claude-b"}),
                        Some("claude-b") => {
                            json!({"data": [{"id": "claude-a"}], "has_more": false, "last_id": "claude-a"})
                        }
                        Some(_) => json!({"data": [{"id": "wrong-page"}], "has_more": false}),
                    })
                },
            ),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let or = list_models(
        ApiKind::Openai,
        &format!("{base}/or/v1"),
        None,
        false,
        &None,
    )
    .await
    .unwrap();
    assert_eq!(or.len(), 1);
    let m = &or[0];
    assert_eq!((m.context, m.max_output), (Some(128000), Some(16384)));
    assert_eq!(
        (m.vision, m.tools, m.effort),
        (Some(true), Some(true), Some(true))
    );

    let an = list_models(
        ApiKind::Anthropic,
        &format!("{base}/an/v1"),
        None,
        false,
        &None,
    )
    .await
    .unwrap();
    let ids: Vec<&str> = an.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["claude-a", "claude-b"]);
    let b = &an[1];
    assert_eq!(b.name.as_deref(), Some("Claude B"));
    assert_eq!(
        (b.context, b.max_output, b.vision),
        (Some(200000), Some(64000), Some(true))
    );
    assert!(an.iter().all(|m| m.effort == Some(false)));
}

/// A key never follows a redirect to another origin (reqwest keeps
/// `x-api-key` on cross-origin hops), and an oversized listing is refused.
#[tokio::test]
async fn list_models_refuses_cross_origin_redirects_and_huge_bodies() {
    use axum::routing::get;
    use harness_agent_native::models::list_models;
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let other = {
        let seen = seen.clone();
        let app = Router::new().route(
            "/v1/models",
            get(move |h: HeaderMap| {
                let seen = seen.clone();
                async move {
                    if let Some(k) = h.get("x-api-key") {
                        seen.lock().unwrap().push(k.to_str().unwrap().to_string());
                    }
                    axum::Json(json!({"data": [{"id": "stolen"}]}))
                }
            }),
        );
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        base
    };
    let to = format!("{other}/v1/models");
    let app = Router::new()
        .route(
            "/r/v1/models",
            get(move || {
                let to = to.clone();
                async move { axum::response::Redirect::temporary(&to) }
            }),
        )
        .route(
            "/big/v1/models",
            get(|| async { format!("{{\"data\": [\"{}\"]}}", "x".repeat(9 << 20)) }),
        );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });

    let key = Secret::new("sk-ant-secret".to_string());
    let r = list_models(
        ApiKind::Anthropic,
        &format!("{base}/r/v1"),
        Some(&key),
        false,
        &None,
    )
    .await;
    assert!(r.is_err(), "the redirect is not followed: {r:?}");
    assert!(
        seen.lock().unwrap().is_empty(),
        "the key went to another host"
    );

    let err = list_models(
        ApiKind::Openai,
        &format!("{base}/big/v1"),
        None,
        false,
        &None,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("too large"), "{err}");
}

/// Ollama: `/api/show` fills in context and capabilities per model.
#[tokio::test]
async fn list_models_probes_ollama_show() {
    use axum::routing::get;
    use harness_agent_native::models::list_models;
    let app = Router::new()
        .route(
            "/v1/models",
            get(|| async { axum::Json(json!({"data": [{"id": "llava"}, {"id": "missing"}]})) }),
        )
        .route(
            "/api/show",
            post(|axum::Json(b): axum::Json<Value>| async move {
                match b["model"].as_str() {
                    Some("llava") => axum::Json(json!({
                        "model_info": {"llama.context_length": 32768},
                        "capabilities": ["completion", "vision"]
                    }))
                    .into_response(),
                    _ => (axum::http::StatusCode::NOT_FOUND, "no").into_response(),
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let ms = list_models(ApiKind::Openai, &base, None, true, &None)
        .await
        .unwrap();
    assert_eq!(ms[0].id, "llava");
    assert_eq!(ms[0].context, Some(32768));
    assert_eq!(
        (ms[0].vision, ms[0].tools, ms[0].effort),
        (Some(true), Some(false), Some(false))
    );
    assert_eq!(
        (ms[1].context, ms[1].vision),
        (None, None),
        "unknown stays unknown"
    );
}

#[tokio::test]
async fn swappable_provider_switches_between_turns() {
    use harness_agent_native::llm::SwappableProvider;
    use harness_agent_native::mock::Mock;
    use std::sync::Arc;
    let sw = SwappableProvider::new(Arc::new(Mock {
        delay: std::time::Duration::ZERO,
    }));
    assert_eq!(sw.name(), "mock");
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    assert!(
        sw.complete(&request(), tx)
            .await
            .unwrap()
            .text
            .starts_with("You said")
    );
    sw.set(Arc::new(
        LlmProvider::new(ApiKind::Openai, "http://127.0.0.1:9/v1", None).unwrap(),
    ));
    assert_eq!(sw.name(), "openai-compatible");
}

#[tokio::test]
async fn anthropic_prompt_cache_markers_and_usage() {
    let ev = |name: &str, data: Value| format!("event: {name}\ndata: {data}\n\n");
    let sse = [
        ev("message_start", json!({"type":"message_start","message":{"id":"m","type":"message","role":"assistant","model":"test-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":20,"cache_read_input_tokens":3000,"cache_creation_input_tokens":500,"output_tokens":1}}})),
        ev("content_block_start", json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}})),
        ev("content_block_delta", json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}})),
        ev("content_block_stop", json!({"type":"content_block_stop","index":0})),
        ev("message_delta", json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":2}})),
        ev("message_stop", json!({"type":"message_stop"})),
    ]
    .concat();
    let (base, seen) = serve("/v1/messages", sse).await;
    let p = LlmProvider::new(
        ApiKind::Anthropic,
        &format!("{base}/v1"),
        Some(Secret::new("k".into())),
    )
    .unwrap();
    let mut req = request();
    req.cache = true;
    req.messages.push(ChatMsg::Assistant {
        text: "pong".into(),
        tool_calls: vec![],
    });
    req.messages.push(ChatMsg::User("again".into()));
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let c = p.complete(&req, tx).await.unwrap();
    let u = c.usage.unwrap();
    assert_eq!(u.cache_read_tokens, 3000, "{u:?}");
    assert_eq!(u.cache_write_tokens, 500, "{u:?}");
    let (_, body) = seen.lock().unwrap().clone().unwrap();
    // Breakpoint on the system prompt (covers tools + system)...
    assert_eq!(
        body["system"][0]["cache_control"]["type"], "ephemeral",
        "{body}"
    );
    // ...on the end of the history before this turn's message ("pong": the
    // next turn rebuilds it from the log as it is, review C43-1), and on
    // the newest message.
    let msgs = body["messages"].as_array().unwrap();
    let marked: Vec<usize> = msgs
        .iter()
        .enumerate()
        .filter(|(_, m)| m.to_string().contains("cache_control"))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(marked, vec![1, 2], "{body}");
    assert_eq!(msgs[1]["content"][0]["text"], "pong", "{body}");
    // Anthropic refuses more than 4 breakpoints in a request.
    assert!(
        body.to_string().matches("cache_control").count() <= 4,
        "{body}"
    );

    // Without the flag: no markers at all.
    let (base, seen) = serve("/v1/messages", String::new()).await;
    let p = LlmProvider::new(
        ApiKind::Anthropic,
        &format!("{base}/v1"),
        Some(Secret::new("k".into())),
    )
    .unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let _ = p.complete(&request(), tx).await;
    let (_, body) = seen.lock().unwrap().clone().unwrap();
    assert!(!body.to_string().contains("cache_control"), "{body}");
}

// Stop reasons on the wire. Shapes follow each provider's streaming
// reference: OpenAI "Chat Completions: the chat completion chunk object"
// (`choices[0].finish_reason`: `stop`, `length`, `tool_calls`,
// `content_filter`), Anthropic "Messages: streaming" (`message_delta` with
// `delta.stop_reason`: `end_turn`, `max_tokens`, `stop_sequence`,
// `tool_use`, `pause_turn`, `refusal`), as in the tests above.

fn openai_sse(args: &[&str], finish: &str) -> String {
    let chunk = |delta: Value, finish: Value| {
        format!(
            "data: {}\n\n",
            json!({"id":"c","object":"chat.completion.chunk","created":1,"model":"test-model",
                   "choices":[{"index":0,"delta":delta,"finish_reason":finish}]})
        )
    };
    let mut s = chunk(
        json!({"role":"assistant","content":"Writing."}),
        Value::Null,
    );
    for (i, a) in args.iter().enumerate() {
        let call = if i == 0 {
            json!({"index":0,"id":"call_1","type":"function","function":{"name":"write","arguments":a}})
        } else {
            json!({"index":0,"function":{"arguments":a}})
        };
        let last = i + 1 == args.len();
        s += &chunk(
            json!({"tool_calls":[call]}),
            if last { json!(finish) } else { Value::Null },
        );
    }
    s + "data: [DONE]\n\n"
}

async fn openai_complete(sse: String) -> harness_agent_native::Completion {
    let (base, _) = serve("/v1/chat/completions", sse).await;
    let p = LlmProvider::new(ApiKind::Openai, &format!("{base}/v1"), None).unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    p.complete(&request(), tx).await.unwrap()
}

/// OpenAI: `length` with a call cut mid-JSON. genai leaves the arguments
/// as the string they got to; the stop says why.
#[tokio::test]
async fn openai_length_stop_is_the_output_limit() {
    use harness_agent_native::StopReason;
    let c = openai_complete(openai_sse(
        &[r#"{"path":"a.txt","#, r#""content":"half of"#],
        "length",
    ))
    .await;
    assert_eq!(c.stop, StopReason::OutputLimit);
    assert_eq!(c.text, "Writing.");
    assert_eq!(c.tool_calls.len(), 1);
    assert!(c.tool_calls[0].input.is_string(), "{:?}", c.tool_calls[0]);

    // A call whose arguments happen to be complete when the limit hit
    // arrives as a valid object: only the stop reason tells it apart.
    let c = openai_complete(openai_sse(
        &[r#"{"path":"a.txt","#, r#""content":"x"}"#],
        "length",
    ))
    .await;
    assert_eq!(c.stop, StopReason::OutputLimit);
    assert_eq!(
        c.tool_calls[0].input,
        json!({"path": "a.txt", "content": "x"})
    );

    let c = openai_complete(openai_sse(
        &[r#"{"path":"a.txt","content":"x"}"#],
        "tool_calls",
    ))
    .await;
    assert_eq!(c.stop, StopReason::ToolUse);
    let c = openai_complete(openai_sse(&[r#"{"path":"a.txt","content":"x"}"#], "stop")).await;
    assert_eq!(c.stop, StopReason::End);
}

fn anthropic_sse(partials: &[&str], stop: &str) -> String {
    let ev = |name: &str, data: Value| format!("event: {name}\ndata: {data}\n\n");
    let mut s = vec![
        ev(
            "message_start",
            json!({"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"test-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":12,"output_tokens":1}}}),
        ),
        ev(
            "content_block_start",
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        ),
        ev(
            "content_block_delta",
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Writing."}}),
        ),
        ev(
            "content_block_stop",
            json!({"type":"content_block_stop","index":0}),
        ),
        ev(
            "content_block_start",
            json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"write","input":{}}}),
        ),
    ];
    for p in partials {
        s.push(ev("content_block_delta", json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":p}})));
    }
    s.push(ev(
        "content_block_stop",
        json!({"type":"content_block_stop","index":1}),
    ));
    s.push(ev("message_delta", json!({"type":"message_delta","delta":{"stop_reason":stop,"stop_sequence":null},"usage":{"output_tokens":4096}})));
    s.push(ev("message_stop", json!({"type":"message_stop"})));
    s.concat()
}

async fn anthropic_complete(sse: String) -> harness_agent_native::Completion {
    let (base, _) = serve("/v1/messages", sse).await;
    let p = LlmProvider::new(
        ApiKind::Anthropic,
        &format!("{base}/v1"),
        Some(Secret::new("k".into())),
    )
    .unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    p.complete(&request(), tx).await.unwrap()
}

/// Anthropic: `max_tokens` with a `tool_use` block cut mid-JSON.
#[tokio::test]
async fn anthropic_max_tokens_stop_is_the_output_limit() {
    use harness_agent_native::StopReason;
    let c = anthropic_complete(anthropic_sse(
        &[r#"{"path":"a.txt","#, r#""content":"half of"#],
        "max_tokens",
    ))
    .await;
    assert_eq!(c.stop, StopReason::OutputLimit);
    assert_eq!(c.text, "Writing.");
    assert!(c.tool_calls[0].input.is_string(), "{:?}", c.tool_calls[0]);

    let c = anthropic_complete(anthropic_sse(
        &[r#"{"path":"a.txt","content":"x"}"#],
        "max_tokens",
    ))
    .await;
    assert_eq!(c.stop, StopReason::OutputLimit);
    assert!(c.tool_calls[0].input.is_object());

    let c = anthropic_complete(anthropic_sse(
        &[r#"{"path":"a.txt","content":"x"}"#],
        "tool_use",
    ))
    .await;
    assert_eq!(c.stop, StopReason::ToolUse);
    let c = anthropic_complete(anthropic_sse(
        &[r#"{"path":"a.txt","content":"x"}"#],
        "end_turn",
    ))
    .await;
    assert_eq!(c.stop, StopReason::End);
}

/// A call cut mid-JSON is kept in the history as a string; sent back as
/// it is, Anthropic would refuse the next request (`input` must be an
/// object) and the model would never read why its call was refused.
#[tokio::test]
async fn a_cut_call_in_the_history_is_sent_as_an_object() {
    let (base, seen) = serve("/v1/messages", String::new()).await;
    let p = LlmProvider::new(
        ApiKind::Anthropic,
        &format!("{base}/v1"),
        Some(Secret::new("k".into())),
    )
    .unwrap();
    let mut req = request();
    req.messages.push(ChatMsg::Assistant {
        text: "Writing.".into(),
        tool_calls: vec![harness_core::event::ToolCallRef {
            call_id: "toolu_1".into(),
            tool: "write".into(),
            input: json!(r#"{"path":"a.txt","content":"FRAGMENT"#),
        }],
    });
    req.messages.push(ChatMsg::Tool {
        call_id: "toolu_1".into(),
        content: "not run".into(),
    });
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let _ = p.complete(&req, tx).await;
    let (_, body) = seen.lock().unwrap().clone().unwrap();
    let input = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["content"].as_array().cloned().unwrap_or_default())
        .find(|b| b["type"] == "tool_use")
        .map(|b| b["input"].clone())
        .unwrap();
    assert_eq!(input, json!({}), "{body}");
}
