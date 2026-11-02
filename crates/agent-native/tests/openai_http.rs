//! The OpenAI-compatible provider against a local SSE server.

use std::sync::{Arc, Mutex};

use axum::{Router, http::HeaderMap, response::IntoResponse, routing::post};
use harness_agent_native::openai::OpenAiCompat;
use harness_agent_native::{ChatMsg, ChatRequest, Provider};
use harness_core::Secret;
use serde_json::json;

#[tokio::test]
async fn streams_from_sse_server_with_bearer_auth() {
    let seen: Arc<Mutex<Option<(String, serde_json::Value)>>> = Arc::default();
    let seen2 = seen.clone();
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move |headers: HeaderMap, body: String| {
            let seen = seen2.clone();
            async move {
                let auth = headers
                    .get("authorization")
                    .map(|v| v.to_str().unwrap().to_string())
                    .unwrap_or_default();
                *seen.lock().unwrap() = Some((auth, serde_json::from_str(&body).unwrap()));
                let sse = [
                    r#"data: {"choices":[{"delta":{"content":"pong"}}]}"#,
                    r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"bash","arguments":"{\"command\":\"ls\"}"}}]}}]}"#,
                    "data: [DONE]",
                ]
                .join("\n\n")
                    + "\n\n";
                ([("content-type", "text/event-stream")], sse).into_response()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let p = OpenAiCompat::new(
        format!("http://{addr}/v1/"),
        Some(Secret::new("sk-test".into())),
    );
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let c = p
        .complete(
            &ChatRequest {
                model: "m".into(),
                messages: vec![ChatMsg::User("ping".into())],
                tools: vec![],
            },
            tx,
        )
        .await
        .unwrap();
    assert_eq!(c.text, "pong");
    assert_eq!(c.tool_calls[0].input, json!({"command": "ls"}));
    assert_eq!(rx.recv().await.unwrap(), "pong");
    let (auth, body) = seen.lock().unwrap().clone().unwrap();
    assert_eq!(auth, "Bearer sk-test");
    assert_eq!(body["stream"], true);
    assert_eq!(body["messages"][0]["content"], "ping");
}

#[tokio::test]
async fn http_error_is_surfaced_without_secret() {
    let app = Router::new().route(
        "/chat/completions",
        post(|| async { (axum::http::StatusCode::UNAUTHORIZED, "bad key") }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let p = OpenAiCompat::new(
        format!("http://{addr}"),
        Some(Secret::new("sk-hidden-1".into())),
    );
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let err = p
        .complete(
            &ChatRequest {
                model: "m".into(),
                messages: vec![],
                tools: vec![],
            },
            tx,
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("401"), "{err}");
    assert!(!err.contains("sk-hidden-1"));
}
