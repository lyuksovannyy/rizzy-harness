//! `remember`: the model keeps, replaces or forgets a memory. The tool
//! itself changes nothing; the master sees the mirrored call and files it
//! (README, Memory): in the person's Personal workspace it is saved at once,
//! elsewhere it waits for them as a proposal. `sensitive` keeps a fact in
//! the workspace it was learned in instead of the person's own facts, which
//! can follow them into other workspaces. `scope: "project"` files it for
//! the chat's project instead (docs/projects.md, Memory). The master checks
//! everything again: what the model sends is never trusted.

use harness_core::policy::ToolRisk;
use harness_tools::{Tool, ToolCtx, ToolOutcome};
use serde_json::{Value, json};

pub struct Remember;

/// The reply to every accepted call. The node cannot know what the master
/// will do with it, so it says what holds (review M4): a call from the
/// person's own message in their Personal workspace is kept at once; any
/// other waits for them; a refusal is told next turn.
const SENT: &str = "Sent to memory. From the person's own message in their Personal workspace it \
     is kept at once (they see it in the chat, with Undo); anywhere else it waits for their \
     confirmation. You will be told next turn if it was not saved.";

/// Whether `input` names a fact to retire with no new text.
fn forget_only(input: &Value) -> bool {
    input.get("forget").and_then(|v| v.as_str()).is_some()
        && input.get("text").and_then(|v| v.as_str()).is_none()
}

#[async_trait::async_trait]
impl Tool for Remember {
    fn name(&self) -> &'static str {
        "remember"
    }
    fn description(&self) -> &'static str {
        "Keep a lasting fact for future chats: about the person (their preferences, people, \
         projects, where they live, how they like to work) or, in a chat that belongs to a project, \
         about that project (decisions, conventions). One short sentence each, only things they \
         would want remembered. Set `sensitive` for anything about the person they would not want a \
         colleague's workspace to see: health, money, identity documents, details near passwords \
         or accounts, family, relationships, beliefs, legal matters; a sensitive fact stays in this \
         workspace only. Never keep a password, key or token. To correct a fact you were shown, \
         pass its id as `replaces` with the new text; to drop one, pass its id as `forget` alone. \
         The person sees every call in the chat: some save at once (with undo), others wait for \
         their Keep."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "The fact, one short sentence" },
                "kind": { "type": "string", "enum": ["fact", "preference", "location"] },
                "sensitive": {
                    "type": "boolean",
                    "description": "true: a private detail (health, money, identity, family...) kept in this workspace only"
                },
                "replaces": { "type": "string", "description": "Id of a fact this one corrects" },
                "forget": { "type": "string", "description": "Id of a fact to drop (no text)" },
                "scope": {
                    "type": "string",
                    "enum": ["user", "project"],
                    "description": "user (default): about the person. project: about the project this chat belongs to"
                }
            }
        })
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }
    async fn run(&self, _ctx: &ToolCtx, input: Value) -> ToolOutcome {
        if forget_only(&input) {
            return ToolOutcome::ok(SENT);
        }
        match input.get("text").and_then(|v| v.as_str()) {
            Some(t) => match crate::clean_text(t) {
                Ok(_) if input.get("scope").and_then(|v| v.as_str()) == Some("project") => {
                    ToolOutcome::ok(
                        "Proposed for the project. The person will confirm or discard it.",
                    )
                }
                Ok(_) => ToolOutcome::ok(SENT),
                Err(crate::MemoryError::Invalid(why)) => ToolOutcome::err(why),
                Err(e) => ToolOutcome::err(e.to_string()),
            },
            None => ToolOutcome::err(format!(
                "`text` must be a non-empty sentence of at most {} characters (or pass `forget` alone)",
                crate::MAX_FACT_CHARS
            )),
        }
    }
}
