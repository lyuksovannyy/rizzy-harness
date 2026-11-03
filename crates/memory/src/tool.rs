//! `remember`: the model proposes a memory. The tool itself changes nothing;
//! the master sees the mirrored call and files it as a *proposal* for the
//! chat's owner, who confirms or discards it.

use harness_core::policy::ToolRisk;
use harness_tools::{Tool, ToolCtx, ToolOutcome};
use serde_json::{Value, json};

pub struct Remember;

#[async_trait::async_trait]
impl Tool for Remember {
    fn name(&self) -> &'static str {
        "remember"
    }
    fn description(&self) -> &'static str {
        "Propose a lasting fact about the user to remember in future chats (their \
         preferences, people, projects, where they live). Only for things they would want \
         remembered; one short sentence each. The user must confirm it before it is used."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "The fact, one short sentence" },
                "kind": { "type": "string", "enum": ["fact", "preference", "location"] }
            },
            "required": ["text"]
        })
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }
    async fn run(&self, _ctx: &ToolCtx, input: Value) -> ToolOutcome {
        match input.get("text").and_then(|v| v.as_str()) {
            Some(t) if crate::clean_text(t).is_ok() => ToolOutcome {
                ok: true,
                output: "Proposed. The user will confirm or discard it in Memory.".into(),
                exit_code: None,
                truncated: false,
            },
            _ => ToolOutcome::err(format!(
                "`text` must be a non-empty sentence of at most {} characters",
                crate::MAX_FACT_CHARS
            )),
        }
    }
}
