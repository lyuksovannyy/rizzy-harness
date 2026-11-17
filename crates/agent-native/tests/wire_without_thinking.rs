//! A chat without thinking is sent exactly as before thinking was kept
//! (X.25): the request bodies both APIs get for a log of text, tool calls,
//! results and a second turn are pinned in
//! `tests/fixtures/wire_without_thinking.json`, written on `main` before
//! the change (`UPDATE_FIXTURES=1` writes it) and compared after it. Built
//! from the log alone (`history_to_messages`), so this file compiles on
//! both sides of the change.

use std::sync::{Arc, Mutex};

use axum::response::IntoResponse;
use axum::{Router, routing::post};
use harness_agent_native::llm::{ApiKind, LlmProvider};
use harness_agent_native::{ChatMsg, ChatRequest, Provider, ToolSpec, history_to_messages};
use harness_core::{RawEvent, Secret, SessionId};
use serde_json::{Value, json};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/wire_without_thinking.json"
);

async fn serve(path: &'static str, body: String) -> (String, Arc<Mutex<Vec<Value>>>) {
    let seen: Arc<Mutex<Vec<Value>>> = Arc::default();
    let s2 = seen.clone();
    let app = Router::new().route(
        path,
        post(move |req: String| {
            let seen = s2.clone();
            let body = body.clone();
            async move {
                seen.lock()
                    .unwrap()
                    .push(serde_json::from_str(&req).unwrap());
                ([("content-type", "text/event-stream")], body).into_response()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), seen)
}

fn log() -> Vec<RawEvent> {
    let sid = SessionId::new();
    let events = [
        ("turn_started", json!({"turn_id": "t1"})),
        (
            "user_message",
            json!({"turn_id": "t1", "text": "list the files"}),
        ),
        (
            "assistant_message",
            json!({"turn_id": "t1", "message_id": "m1", "text": "Listing.",
                   "tool_calls": [{"call_id": "c1", "tool": "bash", "input": {"command": "ls"}}]}),
        ),
        (
            "tool_call_finished",
            json!({"turn_id": "t1", "call_id": "c1", "ok": true, "output": "a.txt\nb.txt"}),
        ),
        (
            "assistant_message",
            json!({"turn_id": "t1", "message_id": "m2", "text": "Two files.", "tool_calls": []}),
        ),
        (
            "turn_ended",
            json!({"turn_id": "t1", "reason": "completed"}),
        ),
        ("turn_started", json!({"turn_id": "t2"})),
        ("user_message", json!({"turn_id": "t2", "text": "and b?"})),
    ];
    events
        .into_iter()
        .enumerate()
        .map(|(i, (kind, payload))| RawEvent {
            session_id: sid,
            seq: i as u64 + 1,
            ts_ms: 0,
            kind: kind.into(),
            payload,
        })
        .collect()
}

fn request(cache: bool) -> ChatRequest {
    let mut messages = vec![ChatMsg::System("be brief".into())];
    messages.extend(history_to_messages(&log()));
    ChatRequest {
        model: "test-model".into(),
        messages,
        tools: vec![ToolSpec {
            name: "bash".into(),
            description: "run".into(),
            schema: json!({"type": "object", "properties": {"command": {"type": "string"}}}),
        }],
        effort: None,
        cache,
        limits: None,
        no_tool_calls: false,
        max_tokens: None,
    }
}

async fn body(kind: ApiKind, cache: bool) -> Value {
    let (path, sse) = match kind {
        ApiKind::Openai => (
            "/v1/chat/completions",
            "data: {\"id\":\"c\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"test-model\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n".to_string(),
        ),
        ApiKind::Anthropic => (
            "/v1/messages",
            [
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"test-model\",\"content\":[],\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n\n",
                "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n",
                "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":1}}\n\n",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            ]
            .concat(),
        ),
    };
    let (base, seen) = serve(path, sse).await;
    let p = LlmProvider::new(kind, &format!("{base}/v1"), Some(Secret::new("k".into()))).unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    p.complete(&request(cache), tx).await.unwrap();
    seen.lock().unwrap()[0].clone()
}

#[tokio::test]
async fn a_chat_without_thinking_is_sent_as_before() {
    // Every event of the log is read: the system prompt, both turns'
    // messages, the call, its result and the answer.
    assert_eq!(request(false).messages.len(), 6);
    let mut got = serde_json::Map::new();
    for (name, kind) in [
        ("openai", ApiKind::Openai),
        ("anthropic", ApiKind::Anthropic),
    ] {
        for cache in [false, true] {
            got.insert(format!("{name} cache={cache}"), body(kind, cache).await);
        }
    }
    let got = Value::Object(got);
    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        std::fs::write(FIXTURE, serde_json::to_string_pretty(&got).unwrap() + "\n").unwrap();
        return;
    }
    let want: Value = serde_json::from_str(&std::fs::read_to_string(FIXTURE).unwrap()).unwrap();
    assert_eq!(got, want, "a request without thinking changed on the wire");
}
