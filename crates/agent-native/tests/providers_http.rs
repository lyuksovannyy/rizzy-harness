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
    use harness_agent_native::llm::list_models;
    let good = Secret::new("good".to_string());
    let bad = Secret::new("sk-wrong-123456".to_string());
    assert_eq!(
        list_models(ApiKind::Openai, &base, Some(&good))
            .await
            .unwrap(),
        ["model-a", "model-b"]
    );
    assert_eq!(
        list_models(ApiKind::Anthropic, &base, Some(&good))
            .await
            .unwrap(),
        ["model-a", "model-b"]
    );
    let err = list_models(ApiKind::Openai, &base, Some(&bad))
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("401") && !err.contains("sk-wrong"), "{err}");
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
    // ...and on the newest message only.
    let msgs = body["messages"].as_array().unwrap();
    let marked: Vec<usize> = msgs
        .iter()
        .enumerate()
        .filter(|(_, m)| m.to_string().contains("cache_control"))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(marked, vec![msgs.len() - 1], "{body}");

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
