//! `todo`: the agent's own task list for the chat (`harness_core::todo`).
//! The tool only checks the list and hands it to the loop, which records it
//! in the chat's log before the call's result; nothing outside the chat
//! changes, so no approval mode asks about it (`ToolRisk::ChatOnly`).

use harness_core::policy::ToolRisk;
use harness_core::todo::{self, MAX_CHARS, MAX_ITEMS};
use serde_json::{Value, json};

use crate::{Tool, ToolCtx, ToolOutcome};

pub struct Todo;

#[async_trait::async_trait]
impl Tool for Todo {
    fn name(&self) -> &'static str {
        "todo"
    }

    fn description(&self) -> &'static str {
        "Your task list for this chat; each call replaces the whole list, and it stays with \
         you after older turns are summarised. For multi-step tasks, write the plan before you \
         start and update it as you go, with at most one item in_progress; clear it with [] when \
         the work is done."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "maxItems": MAX_ITEMS,
                    "description": "The whole list, in order ([] clears it)",
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": {
                                "type": "string",
                                "maxLength": MAX_CHARS,
                                "description": "One short line"
                            },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "done"]
                            }
                        },
                        "required": ["content", "status"]
                    }
                }
            },
            "required": ["items"]
        })
    }

    /// It writes only the chat's own log: never asked about.
    fn risk(&self) -> ToolRisk {
        ToolRisk::ChatOnly
    }

    async fn run(&self, _ctx: &ToolCtx, input: Value) -> ToolOutcome {
        match todo::parse(&input) {
            Err(why) => ToolOutcome::err(format!("The task list was not changed: {why}.")),
            Ok(items) => {
                let said = if items.is_empty() {
                    "Task list cleared.".to_string()
                } else {
                    format!("Task list saved: {}.", todo::tally(&items))
                };
                ToolOutcome {
                    todo: Some(items),
                    ..ToolOutcome::ok(said)
                }
            }
        }
    }
}
