//! Full outputs of filtered or truncated tool calls, kept on the node so the
//! model can fetch what the filter dropped (`tool_output`). In memory only,
//! bounded, gone after a restart.

use std::collections::VecDeque;
use std::sync::Mutex;

use harness_core::policy::ToolRisk;
use serde_json::{Value, json};

use crate::{Tool, ToolCtx, ToolOutcome, str_arg};

const KEEP: usize = 64;
/// Largest output stored (characters).
pub const MAX_STORED: usize = 2_000_000;

#[derive(Default)]
pub struct OutputStore {
    items: Mutex<VecDeque<(String, String)>>,
}

impl OutputStore {
    /// Keep `full` and return its id.
    pub fn put(&self, full: String) -> String {
        let id = harness_core::new_id()[..8].to_string();
        let mut items = self.items.lock().unwrap_or_else(|p| p.into_inner());
        if items.len() >= KEEP {
            items.pop_front();
        }
        items.push_back((id.clone(), full));
        id
    }

    pub fn get(&self, id: &str) -> Option<String> {
        self.items
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .find(|(i, _)| i == id)
            .map(|(_, s)| s.clone())
    }
}

/// Read a stored full output: a line range, or only the lines containing a
/// text.
pub struct ToolOutput;

#[async_trait::async_trait]
impl Tool for ToolOutput {
    fn name(&self) -> &'static str {
        "tool_output"
    }

    fn description(&self) -> &'static str {
        "Read the full output of an earlier command whose result was filtered or \
         shortened (its result names the id). Give `contains` to get only matching lines, \
         or `offset`/`limit` for a line range."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string"},
                "contains": {"type": "string", "description": "Only lines containing this text"},
                "offset": {"type": "integer", "description": "1-based first line (default 1)"},
                "limit": {"type": "integer", "description": "Max lines (default 200)"}
            },
            "required": ["id"]
        })
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    fn enabled(&self, s: &harness_core::savings::Savings) -> bool {
        s.filter_shell_output
    }

    async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome {
        let id = match str_arg(&input, "id") {
            Ok(v) => v,
            Err(e) => return ToolOutcome::err(e),
        };
        let Some(full) = ctx.outputs.as_ref().and_then(|o| o.get(id)) else {
            return ToolOutcome::err(format!(
                "no stored output {id:?} (outputs are kept for the last {KEEP} commands \
                 until the node restarts); run the command again"
            ));
        };
        let offset = input
            .get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(1)
            .max(1) as usize;
        let limit = input
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(200)
            .clamp(1, 2000) as usize;
        let contains = input.get("contains").and_then(|v| v.as_str());
        let lines: Vec<(usize, &str)> = full
            .lines()
            .enumerate()
            .filter(|(_, l)| contains.is_none_or(|c| l.contains(c)))
            .collect();
        let total = lines.len();
        let mut out = String::new();
        for (n, l) in lines.iter().skip(offset - 1).take(limit) {
            out.push_str(&format!("{}\t{l}\n", n + 1));
        }
        let shown_end = (offset - 1 + limit).min(total);
        if shown_end < total {
            out.push_str(&format!(
                "[{} more matching lines; use offset={}]\n",
                total - shown_end,
                shown_end + 1
            ));
        }
        if out.is_empty() {
            out = "(no matching lines)".into();
        }
        ToolOutcome::ok(out)
    }
}
