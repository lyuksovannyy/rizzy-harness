//! OpenAI-compatible chat-completions provider with streaming tool calls.
//! Works with OpenAI, DeepSeek, vLLM, Ollama, OpenRouter and similar.

use std::collections::BTreeMap;
use std::time::Duration;

use futures::StreamExt;
use harness_core::Secret;
use harness_core::event::{ToolCallRef, Usage};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::provider::*;

pub struct OpenAiCompat {
    client: reqwest::Client,
    base_url: String,
    api_key: Option<Secret<String>>,
}

impl OpenAiCompat {
    pub fn new(base_url: impl Into<String>, api_key: Option<Secret<String>>) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .build()
            .expect("reqwest client");
        Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key,
        }
    }
}

pub(crate) fn messages_json(msgs: &[ChatMsg]) -> Vec<Value> {
    msgs.iter()
        .map(|m| match m {
            ChatMsg::System(t) => json!({"role": "system", "content": t}),
            ChatMsg::User(t) => json!({"role": "user", "content": t}),
            ChatMsg::Assistant { text, tool_calls } => {
                let mut v = json!({"role": "assistant", "content": if text.is_empty() { Value::Null } else { json!(text) }});
                if !tool_calls.is_empty() {
                    v["tool_calls"] = tool_calls
                        .iter()
                        .map(|c| {
                            json!({
                                "id": c.call_id,
                                "type": "function",
                                "function": {"name": c.tool, "arguments": c.input.to_string()}
                            })
                        })
                        .collect();
                }
                v
            }
            ChatMsg::Tool { call_id, content } => {
                json!({"role": "tool", "tool_call_id": call_id, "content": content})
            }
        })
        .collect()
}

#[derive(Default)]
struct PartialCall {
    id: String,
    name: String,
    args: String,
}

/// Incremental parser for the chat-completions SSE stream.
#[derive(Default)]
pub(crate) struct StreamParser {
    buf: String,
    text: String,
    calls: BTreeMap<u64, PartialCall>,
    usage: Option<Usage>,
    done: bool,
}

impl StreamParser {
    /// Feed bytes; returns text deltas found.
    pub(crate) fn feed(&mut self, chunk: &str) -> Result<Vec<String>, ProviderError> {
        self.buf.push_str(&chunk.replace("\r\n", "\n"));
        let mut out = Vec::new();
        while let Some(pos) = self.buf.find("\n\n") {
            let frame: String = self.buf.drain(..pos + 2).collect();
            for line in frame.lines() {
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data == "[DONE]" {
                    self.done = true;
                    continue;
                }
                if data.is_empty() {
                    continue;
                }
                let v: Value = serde_json::from_str(data)
                    .map_err(|e| ProviderError::Stream(format!("{e}: {data}")))?;
                if let Some(err) = v.get("error") {
                    return Err(ProviderError::Stream(err.to_string()));
                }
                if let Some(u) = v.get("usage").filter(|u| !u.is_null()) {
                    self.usage = Some(Usage {
                        input_tokens: u["prompt_tokens"].as_u64().unwrap_or(0),
                        output_tokens: u["completion_tokens"].as_u64().unwrap_or(0),
                    });
                }
                let Some(delta) = v.pointer("/choices/0/delta") else {
                    continue;
                };
                if let Some(t) = delta.get("content").and_then(|c| c.as_str())
                    && !t.is_empty()
                {
                    self.text.push_str(t);
                    out.push(t.to_string());
                }
                if let Some(tcs) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                    for (pos, tc) in tcs.iter().enumerate() {
                        // Some servers omit `index`; fall back to position.
                        let idx = tc["index"].as_u64().unwrap_or(pos as u64);
                        let p = self.calls.entry(idx).or_default();
                        if let Some(id) = tc["id"].as_str() {
                            p.id.push_str(id);
                        }
                        if let Some(n) = tc.pointer("/function/name").and_then(|n| n.as_str()) {
                            p.name.push_str(n);
                        }
                        if let Some(a) = tc.pointer("/function/arguments").and_then(|a| a.as_str())
                        {
                            p.args.push_str(a);
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    pub(crate) fn finish(self) -> Completion {
        let tool_calls = self
            .calls
            .into_values()
            .filter(|p| !p.name.is_empty())
            .map(|p| ToolCallRef {
                call_id: if p.id.is_empty() {
                    harness_core::new_id()
                } else {
                    p.id
                },
                tool: p.name,
                // Unparseable arguments are passed as a string so the tool can
                // report a clear error to the model instead of us guessing.
                input: if p.args.trim().is_empty() {
                    json!({})
                } else {
                    serde_json::from_str(&p.args).unwrap_or(Value::String(p.args))
                },
            })
            .collect();
        Completion {
            text: self.text,
            tool_calls,
            usage: self.usage,
        }
    }
}

#[async_trait::async_trait]
impl Provider for OpenAiCompat {
    fn name(&self) -> &'static str {
        "openai-compatible"
    }

    async fn complete(
        &self,
        req: &ChatRequest,
        deltas: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        let mut body = json!({
            "model": req.model,
            "messages": messages_json(&req.messages),
            "stream": true,
            "stream_options": {"include_usage": true},
        });
        if !req.tools.is_empty() {
            body["tools"] = req
                .tools
                .iter()
                .map(|t| {
                    json!({"type": "function", "function": {
                        "name": t.name, "description": t.description, "parameters": t.schema
                    }})
                })
                .collect();
        }
        let url = format!("{}/chat/completions", self.base_url);
        let mut attempt = 0u32;
        let resp = loop {
            let mut rb = self.client.post(&url).json(&body);
            if let Some(k) = &self.api_key {
                rb = rb.bearer_auth(k.expose());
            }
            let resp = rb
                .send()
                .await
                .map_err(|e| ProviderError::Http(e.without_url().to_string()))?;
            let status = resp.status().as_u16();
            if (status == 429 || status >= 500) && attempt < 3 {
                attempt += 1;
                tokio::time::sleep(Duration::from_millis(500 * 2u64.pow(attempt))).await;
                continue;
            }
            if !(200..300).contains(&status) {
                let text = resp.text().await.unwrap_or_default();
                return Err(ProviderError::Status {
                    status,
                    body: text.chars().take(2000).collect(),
                });
            }
            break resp;
        };
        let mut parser = StreamParser::default();
        let mut stream = resp.bytes_stream();
        let mut pending: Vec<u8> = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| ProviderError::Http(e.without_url().to_string()))?;
            pending.extend_from_slice(&chunk);
            // Only hand complete UTF-8 to the parser.
            let valid = match std::str::from_utf8(&pending) {
                Ok(_) => pending.len(),
                Err(e) => e.valid_up_to(),
            };
            let text = String::from_utf8_lossy(&pending[..valid]).into_owned();
            pending.drain(..valid);
            for d in parser.feed(&text)? {
                let _ = deltas.send(d);
            }
            if parser.done {
                break;
            }
        }
        // Flush a final frame without a trailing blank line.
        for d in parser.feed("\n\n")? {
            let _ = deltas.send(d);
        }
        Ok(parser.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_and_split_tool_calls() {
        let mut p = StreamParser::default();
        let frames = [
            r#"data: {"choices":[{"delta":{"content":"Hel"}}]}"#,
            r#"data: {"choices":[{"delta":{"content":"lo"}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"bash","arguments":"{\"comm"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"and\":\"ls\"}"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":1,"id":"call_2","function":{"name":"read","arguments":"{}"}}]}}]}"#,
            r#"data: {"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5}}"#,
            "data: [DONE]",
        ];
        let joined = frames.join("\n\n") + "\n\n";
        // Feed in awkward slices to exercise buffering.
        let mut deltas = Vec::new();
        for piece in joined.as_bytes().chunks(7) {
            deltas.extend(p.feed(std::str::from_utf8(piece).unwrap()).unwrap());
        }
        assert_eq!(deltas.concat(), "Hello");
        let c = p.finish();
        assert_eq!(c.text, "Hello");
        assert_eq!(c.tool_calls.len(), 2);
        assert_eq!(c.tool_calls[0].call_id, "call_1");
        assert_eq!(c.tool_calls[0].input, json!({"command": "ls"}));
        assert_eq!(c.tool_calls[1].tool, "read");
        assert_eq!(
            c.usage,
            Some(Usage {
                input_tokens: 10,
                output_tokens: 5
            })
        );
    }

    #[test]
    fn stream_error_is_reported() {
        let mut p = StreamParser::default();
        assert!(
            p.feed("data: {\"error\":{\"message\":\"boom\"}}\n\n")
                .is_err()
        );
    }

    #[test]
    fn assistant_tool_calls_serialize() {
        let v = messages_json(&[ChatMsg::Assistant {
            text: String::new(),
            tool_calls: vec![ToolCallRef {
                call_id: "c".into(),
                tool: "bash".into(),
                input: json!({"command": "ls"}),
            }],
        }]);
        assert!(v[0]["content"].is_null());
        assert_eq!(
            v[0]["tool_calls"][0]["function"]["arguments"],
            "{\"command\":\"ls\"}"
        );
    }
}
