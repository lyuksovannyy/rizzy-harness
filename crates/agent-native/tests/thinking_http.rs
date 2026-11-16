//! The model's thinking on the wire (X.25, PLAN.md T73), against local
//! servers that speak each format: what an answer brings is captured
//! exactly, and the next request sends it back the way that API needs.
//! The shapes follow the providers' documentation (Anthropic "Thinking":
//! a thinking block's `signature` arrives as `signature_delta` events and
//! goes back unchanged, `redacted_thinking` with its `data`, in order;
//! DeepSeek "Thinking mode": `reasoning_content` back in later requests).

use std::sync::{Arc, Mutex};

use axum::response::IntoResponse;
use axum::{Router, routing::post};
use harness_agent_native::llm::{ApiKind, LlmProvider};
use harness_agent_native::{ChatMsg, ChatRequest, Provider, ToolSpec};
use harness_core::Secret;
use harness_core::event::{Thinking, ThinkingBlock, ToolCallRef};
use serde_json::{Value, json};

type Seen = Arc<Mutex<Vec<Value>>>;

async fn serve(path: &'static str, body: String) -> (String, Seen) {
    let seen: Seen = Arc::default();
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

fn request(messages: Vec<ChatMsg>) -> ChatRequest {
    ChatRequest {
        model: "test-model".into(),
        messages,
        tools: vec![ToolSpec {
            name: "bash".into(),
            description: "run".into(),
            schema: json!({"type": "object", "properties": {"command": {"type": "string"}}}),
        }],
        effort: None,
        cache: false,
        limits: None,
        no_tool_calls: false,
        max_tokens: None,
    }
}

fn first() -> Vec<ChatMsg> {
    vec![
        ChatMsg::System("be brief".into()),
        ChatMsg::User("list the files".into()),
    ]
}

fn ev(name: &str, data: Value) -> String {
    format!("event: {name}\ndata: {data}\n\n")
}

/// An Anthropic answer: `blocks` are (type, content block, deltas).
fn anthropic_sse(blocks: &[(&str, Value, Vec<Value>)]) -> String {
    let mut s = vec![ev(
        "message_start",
        json!({"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"test-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":12,"output_tokens":1}}}),
    )];
    for (i, (_, start, deltas)) in blocks.iter().enumerate() {
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

fn thinking_block(text: &[&str], sig: &[&str]) -> (&'static str, Value, Vec<Value>) {
    let mut d: Vec<Value> = text
        .iter()
        .map(|t| json!({"type":"thinking_delta","thinking":t}))
        .collect();
    d.extend(
        sig.iter()
            .map(|s| json!({"type":"signature_delta","signature":s})),
    );
    (
        "thinking",
        json!({"type":"thinking","thinking":"","signature":""}),
        d,
    )
}

fn tool_use(id: &str) -> (&'static str, Value, Vec<Value>) {
    (
        "tool_use",
        json!({"type":"tool_use","id":id,"name":"bash","input":{}}),
        vec![json!({"type":"input_json_delta","partial_json":"{\"command\":\"ls\"}"})],
    )
}

fn anthropic(base: &str) -> LlmProvider {
    LlmProvider::new(
        ApiKind::Anthropic,
        &format!("{base}/v1"),
        Some(Secret::new("k".into())),
    )
    .unwrap()
}

/// The step as the loop keeps it, then its result.
fn step(c: &harness_agent_native::Completion) -> [ChatMsg; 2] {
    [
        ChatMsg::Assistant {
            text: c.text.clone(),
            tool_calls: c.tool_calls.clone(),
            thinking: c.thinking.clone().map(Arc::new),
        },
        ChatMsg::Tool {
            call_id: c.tool_calls[0].call_id.clone(),
            content: "a.txt".into(),
        },
    ]
}

/// Anthropic: a thinking block's text and its signature (split over
/// `signature_delta` events), a redacted block, then text and a tool call:
/// captured exactly and in order, the thinking never streamed as the
/// answer's text; the next request sends the blocks back unchanged, in
/// their order, in front of the text and the call.
#[tokio::test]
async fn anthropic_blocks_are_captured_and_sent_back_as_they_came() {
    let sse = anthropic_sse(&[
        thinking_block(&["Run ", "ls."], &["EqQBCgIYAhIM", "1gbcDa9GJwZA"]),
        (
            "redacted_thinking",
            json!({"type":"redacted_thinking","data":"EmwKAhgBEgy3va"}),
            vec![],
        ),
        (
            "text",
            json!({"type":"text","text":""}),
            vec![json!({"type":"text_delta","text":"Listing."})],
        ),
        tool_use("toolu_1"),
    ]);
    let (base, seen) = serve("/v1/messages", sse).await;
    let p = anthropic(&base);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let c = p.complete(&request(first()), tx).await.unwrap();
    let mut deltas = String::new();
    while let Ok(d) = rx.try_recv() {
        deltas.push_str(&d);
    }
    assert_eq!(deltas, "Listing.", "thinking is not the answer's text");
    assert_eq!(c.text, "Listing.");
    let t = c.thinking.clone().expect("thinking captured");
    assert_eq!(
        t.blocks,
        vec![
            ThinkingBlock::Signed {
                text: "Run ls.".into(),
                signature: "EqQBCgIYAhIM1gbcDa9GJwZA".into(),
            },
            ThinkingBlock::Redacted {
                data: "EmwKAhgBEgy3va".into()
            },
        ]
    );
    assert_eq!(t.model, "test-model");
    assert!(!t.unsent);
    assert_eq!(t.prefix.len(), 64);
    // The first request sent no thinking parameter (R8: no effort).
    let body = seen.lock().unwrap()[0].clone();
    assert!(body.get("thinking").is_none(), "{body}");

    let mut msgs = first();
    msgs.extend(step(&c));
    let r = request(msgs);
    assert!(p.replays_thinking(&r));
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    p.complete(&r, tx).await.unwrap();
    let body = seen.lock().unwrap()[1].clone();
    assert_eq!(
        body["messages"][1]["content"],
        json!([
            {"type": "thinking", "thinking": "Run ls.", "signature": "EqQBCgIYAhIM1gbcDa9GJwZA"},
            {"type": "redacted_thinking", "data": "EmwKAhgBEgy3va"},
            {"type": "text", "text": "Listing."},
            {"type": "tool_use", "id": "toolu_1", "name": "bash", "input": {"command": "ls"}},
        ]),
        "{body}"
    );
}

/// An answer with a thinking block after a tool call (a progress update
/// before the second of two parallel calls): genai would send it first,
/// so it is kept for the person and never sent back.
#[tokio::test]
async fn thinking_after_a_tool_call_is_kept_but_not_sent_back() {
    let sse = anthropic_sse(&[
        thinking_block(&["Two reads."], &["SIGA"]),
        tool_use("toolu_1"),
        thinking_block(&["Now the second."], &["SIGB"]),
        tool_use("toolu_2"),
    ]);
    let (base, _) = serve("/v1/messages", sse).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let c = anthropic(&base)
        .complete(&request(first()), tx)
        .await
        .unwrap();
    let t = c.thinking.unwrap();
    assert_eq!(t.blocks.len(), 2);
    assert!(t.unsent, "{t:?}");
}

/// A signed block whose signature never came (a stream cut, a broken
/// provider) is not a block genai keeps: nothing to send back, no crash.
#[tokio::test]
async fn a_thinking_block_without_a_signature_is_dropped() {
    let sse = anthropic_sse(&[thinking_block(&["no end"], &[]), tool_use("toolu_1")]);
    let (base, _) = serve("/v1/messages", sse).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let c = anthropic(&base)
        .complete(&request(first()), tx)
        .await
        .unwrap();
    assert!(c.thinking.is_none(), "{:?}", c.thinking);
    assert_eq!(c.tool_calls.len(), 1);
}

/// OpenAI-compatible (DeepSeek's `reasoning_content`): the reasoning text
/// is captured, not streamed as the answer, and the next request carries
/// it back on the assistant message.
#[tokio::test]
async fn openai_reasoning_content_is_captured_and_sent_back() {
    let sse = [
        r#"data: {"id":"c","object":"chat.completion.chunk","created":1,"model":"test-model","choices":[{"index":0,"delta":{"role":"assistant","reasoning_content":"List "},"finish_reason":null}]}"#,
        r#"data: {"id":"c","object":"chat.completion.chunk","created":1,"model":"test-model","choices":[{"index":0,"delta":{"reasoning_content":"first."},"finish_reason":null}]}"#,
        r#"data: {"id":"c","object":"chat.completion.chunk","created":1,"model":"test-model","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"bash","arguments":"{\"command\":\"ls\"}"}}]},"finish_reason":"tool_calls"}]}"#,
        "data: [DONE]",
    ]
    .join("\n\n")
        + "\n\n";
    let (base, seen) = serve("/v1/chat/completions", sse).await;
    let p = LlmProvider::new(
        ApiKind::Openai,
        &format!("{base}/v1"),
        Some(Secret::new("k".into())),
    )
    .unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let c = p.complete(&request(first()), tx).await.unwrap();
    assert!(rx.try_recv().is_err(), "reasoning is not the answer's text");
    let t = c.thinking.clone().unwrap();
    assert_eq!(
        t.blocks,
        vec![ThinkingBlock::Text {
            text: "List first.".into()
        }]
    );
    assert!(t.prefix.is_empty() && !t.unsent);
    let mut msgs = first();
    msgs.extend(step(&c));
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    p.complete(&request(msgs), tx).await.unwrap();
    let body = seen.lock().unwrap()[1].clone();
    let a = &body["messages"][2];
    assert_eq!(a["role"], "assistant", "{body}");
    assert_eq!(a["reasoning_content"], "List first.", "{body}");
}

/// Thinking another model wrote is not sent back by a provider used
/// directly either when the router left it (the router is what filters by
/// model; see `llm::tests::the_router_sends_only_the_models_own_thinking`):
/// here, an Anthropic signed block is never sent to an OpenAI-compatible
/// API, nor plain reasoning to Anthropic.
#[tokio::test]
async fn one_apis_thinking_never_goes_to_the_other() {
    let signed = Thinking {
        model: "test-model".into(),
        blocks: vec![ThinkingBlock::Signed {
            text: "x".into(),
            signature: "S".into(),
        }],
        prefix: "00".repeat(32),
        unsent: false,
    };
    let plain = Thinking {
        model: "test-model".into(),
        blocks: vec![ThinkingBlock::Text { text: "y".into() }],
        prefix: String::new(),
        unsent: false,
    };
    let call = ToolCallRef {
        call_id: "c1".into(),
        tool: "bash".into(),
        input: json!({"command": "ls"}),
    };
    let with = |t: Thinking| {
        let mut m = first();
        m.push(ChatMsg::Assistant {
            text: String::new(),
            tool_calls: vec![call.clone()],
            thinking: Some(Arc::new(t)),
        });
        m.push(ChatMsg::Tool {
            call_id: "c1".into(),
            content: "r".into(),
        });
        request(m)
    };
    let o = LlmProvider::new(ApiKind::Openai, "http://127.0.0.1:9/v1", None).unwrap();
    let a = LlmProvider::new(ApiKind::Anthropic, "http://127.0.0.1:9/v1", None).unwrap();
    assert!(!o.replays_thinking(&with(signed.clone())));
    assert!(!a.replays_thinking(&with(plain)));
    // And a signed block whose prefix is not this request's is not sent.
    assert!(!a.replays_thinking(&with(signed)));
}
