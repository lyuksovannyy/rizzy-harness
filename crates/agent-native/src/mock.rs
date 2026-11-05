//! A deterministic scripted provider for tests and for trying the UI without
//! an API key. It understands a tiny command language in the user message:
//!
//! * `run: <cmd>`    -> one `bash` tool call, then a summary
//! * `read: <path>`  -> one `read` tool call, then a summary
//! * `search: <q>`  -> one `web_search` tool call, then a summary
//! * `fetch: <url>`  -> one `web_fetch` tool call, then a summary
//! * `remember: <t>` -> one `remember` tool call (a memory proposal)
//! * `memory?`       -> repeats the recalled-memory block (per-turn context)
//! * `delegate: a=task | b=task` -> one `delegate` call per pair (parallel)
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
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            input_tokens: estimate_tokens(&req.messages),
            output_tokens: 1,
            context_tokens: 0,
        });
        if let Some(ChatMsg::System(p)) = req.messages.first()
            && p == crate::SUMMARY_PROMPT
        {
            let n = match req.messages.get(1) {
                Some(ChatMsg::User(t)) => t.matches("User: ").count(),
                _ => 0,
            };
            return Ok(Completion {
                text: format!("Summary of {n} earlier messages."),
                tool_calls: vec![],
                usage,
            });
        }
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
        let raw_user = req
            .messages
            .iter()
            .rev()
            .find_map(|m| match m {
                ChatMsg::User(t) => Some(t.clone()),
                _ => None,
            })
            .unwrap_or_default();
        // Per-turn context comes first in the user message; commands follow it.
        let (context, user) = match raw_user
            .strip_prefix("<context>\n")
            .and_then(|r| r.split_once("\n</context>\n\n"))
        {
            Some((c, u)) => (c.to_string(), u.to_string()),
            None => (String::new(), raw_user),
        };
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
        if let Some(q) = user.strip_prefix("search:") {
            return Ok(call("web_search", json!({"query": q.trim()})));
        }
        if let Some(rest) = user.strip_prefix("delegate:") {
            let calls = rest
                .split('|')
                .filter_map(|p| p.split_once('='))
                .map(|(a, t)| ToolCallRef {
                    call_id: harness_core::new_id(),
                    tool: "delegate".into(),
                    input: json!({"agent": a.trim(), "task": t.trim()}),
                })
                .collect();
            return Ok(Completion {
                text: String::new(),
                tool_calls: calls,
                usage,
            });
        }
        if user.trim().starts_with("memory?") {
            let sys = match req.messages.first() {
                Some(ChatMsg::System(s)) => s.clone(),
                _ => String::new(),
            };
            let block = format!("{context}\n{sys}")
                .split("What you remember about the user")
                .nth(1)
                .map(|b| format!("Memory:{b}"))
                .unwrap_or_else(|| "Memory: nothing recalled.".into());
            let text = self.stream_words(vec![block], &deltas).await;
            return Ok(Completion {
                text,
                tool_calls: vec![],
                usage,
            });
        }
        if let Some(t) = user.strip_prefix("remember:") {
            return Ok(call("remember", json!({"text": t.trim(), "kind": "fact"})));
        }
        if let Some(u) = user.strip_prefix("fetch:") {
            return Ok(call("web_fetch", json!({"url": u.trim()})));
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
        // `mock-*` models (parallel answers in tests) tag their replies.
        let lead = if req.model.starts_with("mock-") {
            format!("[{}] You said: ", req.model)
        } else {
            "You said: ".to_string()
        };
        let words = std::iter::once(lead)
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
