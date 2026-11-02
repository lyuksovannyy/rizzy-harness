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
