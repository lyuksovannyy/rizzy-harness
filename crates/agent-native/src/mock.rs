//! A deterministic scripted provider for tests and for trying the UI without
//! an API key. It understands a tiny command language in the user message:
//!
//! * `run: <cmd>`    -> one `bash` tool call, then a summary
//! * `read: <path>`  -> one `read` tool call, then a summary
//! * `search: <q>`  -> one `web_search` tool call, then a summary
//! * `fetch: <url>`  -> one `web_fetch` tool call, then a summary
//! * `cite: <q>`     -> one `web_search` call, then an answer citing the
//!   first two labels of its result and one label that does not exist
//! * `publish: <path>[ as <name>]` -> one `publish` tool call
//! * `remember: <t>` -> one `remember` tool call (a memory proposal)
//! * `remember project: <t>` -> the same with `scope: "project"`
//! * `memory?`       -> repeats the recalled-memory block (per-turn context)
//! * `delegate: a=task | b=task` -> one `delegate` call per pair (parallel),
//!   then passes on the first line of the (last) sub-agent's answer
//! * `slow: <n>`     -> streams `n` chunks, `delay_ms` apart (default 50)
//! * `rich: demo`    -> streams Markdown that exercises the rich renderer
//! * `ui: <name>`    -> streams a canned `ui:` block: choices, form, progress,
//!   chart, secret (a form asking for a key), bad (a label over the limit),
//!   unknown (`ui:widget`), two (two questions in one message)
//! * `progress: <n>` -> a job of `n` steps (default 3, at most 20): one
//!   message per step with the same `ui:progress` block moved on, each with
//!   a cheap call (bash `true`, else web_search) so the next step is a new
//!   request; the last message has every step done and calls nothing
//! * a message with images -> `I see N image(s).` (the images it was shown)
//! * anything else   -> echoes the message back, streamed word by word; an
//!   answer to a question or form echoes its canonical text
//!   (`You said: Choice answer · colour: blue`)

use std::time::Duration;

use harness_core::event::{ToolCallRef, Usage};
use serde_json::json;
use tokio::sync::mpsc;

use crate::provider::*;

mod ui;

/// Headings, a table with numbers, a fence, math in every supported form, a
/// price that must stay text, a callout, a remote image and a link with a
/// query (neither may be fetched), and a hidden bidi override.
const RICH_DEMO: &str = concat!(
    r#"## Rich text demo

Build times by crate:

| crate | debug (s) | release (s) |
|---|---:|---:|
| core | 12.5 | 40 |
| web | 8 | 22 |
| master | 30 | 95 |

```rust
fn main() {
    let answer: u32 = 42;
    println!("{answer}");
}
```

Pythagoras: \(a^2+b^2=c^2\), and an integral:

$$
\int_0^1 x^2\,dx = \frac{1}{3}
$$

Prices stay text: $5 and $10.

> [!WARNING] Check before you run
> This deletes the build cache.

![x](https://img.example/p.png?q=s) and [x](https://evil.example/a?q=1)

An equation that tries to take the footnote's id:

$$
E = mc^2 \tag{1}\label{user-content-fn-1}
$$

See \(\eqref{user-content-fn-1}\) and \(\char"202E x\), and a footnote[^1].

| check | link |
|---|---|
| query | [y](https://evil.example/t?q=2) |

Reversed: [&#x202E;moc.knab](https://evil.example/) and more.

[^1]: The footnote.

"#,
    "Hidden mark: abc\u{202E}def\n"
);

/// `Answer [S1] and [S2], both [S1][S2]. Unknown [S9].` with the first two
/// labels the result gave (labels are session-wide, so they are not always
/// S1 and S2): single labels, an adjacent pair and a label with no source.
fn cite_answer(result: &str) -> Vec<String> {
    let labels: Vec<String> = result
        .lines()
        .filter_map(|l| l.strip_prefix('[')?.split_once(']'))
        .map(|(id, _)| format!("[{id}]"))
        .take(2)
        .collect();
    let mut words = vec!["Answer ".to_string()];
    match labels.as_slice() {
        [a, b] => words.extend([
            format!("{a} "),
            "and ".into(),
            format!("{b}, "),
            format!("both {a}{b}. "),
        ]),
        [a] => words.push(format!("{a}. ")),
        _ => {}
    }
    words.extend(["Unknown ".into(), "[S9].".into()]);
    words
}

/// For `progress: <n>`: the number of steps and which one this request is
/// (1 + the tool results since the user's message, so every call moves the
/// job on).
fn progress_step(user: &str, messages: &[ChatMsg]) -> Option<(usize, usize)> {
    let n = user.strip_prefix("progress:")?.trim();
    let n = n.parse::<usize>().unwrap_or(3).clamp(1, 20);
    let since = messages
        .iter()
        .rposition(|m| m.user_text().is_some())
        .map_or(0, |i| i + 1);
    let results = messages[since..]
        .iter()
        .filter(|m| matches!(m, ChatMsg::Tool { .. }))
        .count();
    Some((n, 1 + results))
}

/// The first line of a sub-agent's answer in a `delegate` result
/// (`session: <id>\n@<agent> ended (<reason>):\n<answer>`).
fn delegate_answer(result: &str) -> Option<String> {
    let (_, answer) = result.strip_prefix("session: ")?.split_once("):\n")?;
    Some(answer.lines().next()?.to_string())
}

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
        let raw_user = req
            .messages
            .iter()
            .rev()
            .find_map(|m| m.user_text().map(str::to_string))
            .unwrap_or_default();
        // Just shown images: say how many arrived with their bytes.
        if let Some(ChatMsg::UserImages { images, .. }) = req.messages.last() {
            let n = images.iter().filter(|i| i.data.is_some()).count();
            let text = self
                .stream_words(vec![format!("I see {n} image(s).")], &deltas)
                .await;
            return Ok(Completion {
                text,
                tool_calls: vec![],
                usage,
            });
        }
        // Per-turn context comes first in the user message; commands follow it.
        let (context, user) = match raw_user
            .strip_prefix("<context>\n")
            .and_then(|r| r.split_once("\n</context>\n\n"))
        {
            Some((c, u)) => (c.to_string(), u.to_string()),
            None => (String::new(), raw_user),
        };
        // Before the generic tool-result branch: a progress job goes on
        // after each of its own calls.
        if let Some((n, k)) = progress_step(&user, &req.messages) {
            let offered = |name: &str| req.tools.iter().any(|t| t.name == name);
            let call = if offered("bash") {
                Some(("bash", json!({"command": "true"})))
            } else if offered("web_search") {
                Some(("web_search", json!({"query": "progress"})))
            } else {
                None
            };
            // With nothing to call between steps, the whole job is one message.
            let k = if call.is_some() { k } else { n };
            let text = ui::progress(n, k);
            let words = text.split_inclusive(' ').map(str::to_string).collect();
            let text = self.stream_words(words, &deltas).await;
            let tool_calls = match call {
                Some((tool, input)) if k < n => vec![ToolCallRef {
                    call_id: harness_core::new_id(),
                    tool: tool.into(),
                    input,
                }],
                _ => vec![],
            };
            return Ok(Completion {
                text,
                tool_calls,
                usage,
            });
        }
        // After a tool result, summarise it (or cite it) and stop.
        if let Some(ChatMsg::Tool { content, .. }) = req.messages.last() {
            let words = if user.starts_with("cite:") {
                cite_answer(content)
            } else if let Some(answer) = delegate_answer(content) {
                vec!["Sub-agent ".into(), "said: ".into(), answer]
            } else {
                let first = content.lines().next().unwrap_or("").to_string();
                vec!["Tool ".into(), "said: ".into(), first]
            };
            let text = self.stream_words(words, &deltas).await;
            return Ok(Completion {
                text,
                tool_calls: vec![],
                usage,
            });
        }
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
        if let Some(q) = user
            .strip_prefix("search:")
            .or_else(|| user.strip_prefix("cite:"))
        {
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
        if let Some(t) = user.strip_prefix("remember project:") {
            return Ok(call(
                "remember",
                json!({"text": t.trim(), "kind": "fact", "scope": "project"}),
            ));
        }
        if let Some(t) = user.strip_prefix("remember:") {
            return Ok(call("remember", json!({"text": t.trim(), "kind": "fact"})));
        }
        if let Some(rest) = user.strip_prefix("publish:") {
            let (path, name) = match rest.split_once(" as ") {
                Some((p, n)) => (p.trim(), Some(n.trim())),
                None => (rest.trim(), None),
            };
            let mut input = json!({ "path": path });
            if let Some(n) = name {
                input["name"] = json!(n);
            }
            return Ok(call("publish", input));
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
        if user.strip_prefix("rich:").map(str::trim) == Some("demo") {
            let words = RICH_DEMO.split_inclusive(' ').map(str::to_string).collect();
            let text = self.stream_words(words, &deltas).await;
            return Ok(Completion {
                text,
                tool_calls: vec![],
                usage,
            });
        }
        // `@writer ui: choices` asks as Writer: the master leaves the mention
        // in the text, and tests check that the answer goes back to Writer.
        let unmentioned = user
            .strip_prefix('@')
            .and_then(|r| r.split_once(' '))
            .map_or(user.as_str(), |(_, rest)| rest);
        if let Some(text) = unmentioned
            .strip_prefix("ui:")
            .and_then(|n| ui::canned(n.trim()))
        {
            let words = text.split_inclusive(' ').map(str::to_string).collect();
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
