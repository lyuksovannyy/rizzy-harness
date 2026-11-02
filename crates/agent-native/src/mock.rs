//! A deterministic scripted provider for tests and for trying the UI without
//! an API key. It understands a tiny command language in the user message:
//!
//! * `run: <cmd>`    -> one `bash` tool call, then a summary
//! * `read: <path>`  -> one `read` tool call, then a summary
//! * `slow: <n>`     -> streams `n` chunks, `delay_ms` apart (default 50)
//! * anything else   -> echoes the message back, streamed word by word

use std::time::Duration;

use harness_core::event::{ToolCallRef, Usage};
use serde_json::json;
use tokio::sync::mpsc;

use crate::provider::*;

pub struct Mock {
    pub delay: Duration,
}

impl Default for Mock {
    fn default() -> Self {
        Self {
            delay: Duration::from_millis(50),
        }
    }
}

impl Mock {
    async fn stream_words(
        &self,
        words: Vec<String>,
        deltas: &mpsc::UnboundedSender<String>,
    ) -> String {
        let mut text = String::new();
        for w in words {
            tokio::time::sleep(self.delay).await;
            text.push_str(&w);
            let _ = deltas.send(w);
        }
        text
    }
}

#[async_trait::async_trait]
impl Provider for Mock {
    fn name(&self) -> &'static str {
        "mock"
    }

    async fn complete(
        &self,
        req: &ChatRequest,
        deltas: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        let usage = Some(Usage {
            input_tokens: req.messages.len() as u64,
            output_tokens: 1,
        });
        // After a tool result, summarise it and stop.
        if let Some(ChatMsg::Tool { content, .. }) = req.messages.last() {
            let first = content.lines().next().unwrap_or("").to_string();
            let text = self
                .stream_words(vec!["Tool ".into(), "said: ".into(), first], &deltas)
                .await;
            return Ok(Completion {
                text,
                tool_calls: vec![],
                usage,
            });
        }
        let user = req
            .messages
            .iter()
            .rev()
            .find_map(|m| match m {
                ChatMsg::User(t) => Some(t.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let call = |tool: &str, input| Completion {
            text: String::new(),
            tool_calls: vec![ToolCallRef {
                call_id: harness_core::new_id(),
                tool: tool.into(),
                input,
            }],
            usage: usage.clone(),
        };
        if let Some(cmd) = user.strip_prefix("run:") {
            return Ok(call("bash", json!({"command": cmd.trim()})));
        }
        if let Some(path) = user.strip_prefix("read:") {
            return Ok(call("read", json!({"path": path.trim()})));
        }
        if let Some(n) = user.strip_prefix("slow:") {
            let n: usize = n.trim().parse().unwrap_or(20);
            let words = (0..n).map(|i| format!("chunk{i} ")).collect();
            let text = self.stream_words(words, &deltas).await;
            return Ok(Completion {
                text,
                tool_calls: vec![],
                usage,
            });
        }
        let words = std::iter::once("You said: ".to_string())
            .chain(user.split_inclusive(' ').map(str::to_string))
            .collect();
        let text = self.stream_words(words, &deltas).await;
        Ok(Completion {
            text,
            tool_calls: vec![],
            usage,
        })
    }
}
