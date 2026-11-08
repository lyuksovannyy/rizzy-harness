//! `remember`: the model proposes a memory. The tool itself changes nothing;
//! the master sees the mirrored call and files it as a *proposal* for the
//! chat's owner, who confirms or discards it. `scope: "project"` files it
//! for the chat's project instead (docs/projects.md, Memory).

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
        "Propose a lasting fact to remember in future chats: about the user (their \
         preferences, people, projects, where they live) or, in a chat that belongs to a \
         project, about that project (decisions, conventions). Only for things they would want \
         remembered; one short sentence each. The user must confirm it before it is used."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "The fact, one short sentence" },
                "kind": { "type": "string", "enum": ["fact", "preference", "location"] },
                "scope": {
                    "type": "string",
                    "enum": ["user", "project"],
                    "description": "user (default): about the user. project: about the project this chat belongs to"
                }
            },
            "required": ["text"]
        })
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }
    async fn run(&self, _ctx: &ToolCtx, input: Value) -> ToolOutcome {
        match input.get("text").and_then(|v| v.as_str()) {
            Some(t) if crate::clean_text(t).is_ok() => {
                if input.get("scope").and_then(|v| v.as_str()) == Some("project") {
                    ToolOutcome::ok(
                        "Proposed for the project. The user will confirm or discard it.",
                    )
                } else {
                    ToolOutcome::ok("Proposed. The user will confirm or discard it in Memory.")
                }
            }
            _ => ToolOutcome::err(format!(
                "`text` must be a non-empty sentence of at most {} characters",
                crate::MAX_FACT_CHARS
            )),
        }
    }
}
