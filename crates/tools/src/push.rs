//! `push`: one branch of the chat's project folder to the forge repository
//! the project was cloned from, as the chat's owner (ADOPTION_PLAN X.27
//! step 4, PLAN.md T101, `docs/forge-push.md`).
//!
//! Nothing here runs git or holds a token: the call goes to the session's
//! node ([`ToolCtx::push`]), which shows the person a card with what will
//! be pushed (every time, in every mode), asks the master for the token
//! only after an Allow, and pushes the commit the card showed from a
//! repository of its own. The tool takes a branch name and nothing else: no
//! URL, remote, repository, force, delete or tag can be asked for.

use harness_core::forge::{MAX_BRANCH, PUSH_TOOL, valid_branch};
use harness_core::policy::ToolRisk;
use harness_core::text::clean_line;
use serde_json::{Value, json};

use crate::{Tool, ToolCtx, ToolOutcome};

/// The tool.
pub struct Push;

/// The call's branch, when it names one: `Err` for a name the tool never
/// pushes.
pub fn branch_arg(input: &Value) -> Result<Option<String>, String> {
    match input.get("branch") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(b)) if b.is_empty() => Ok(None),
        Some(Value::String(b)) if valid_branch(b) => Ok(Some(b.clone())),
        Some(Value::String(_)) => Err(format!(
            "that is not a branch name `push` takes: at most {MAX_BRANCH} bytes of letters, \
             digits and `. _ - + @`, parts separated by `/`, none starting with `.`, `-` or `+`"
        )),
        Some(_) => Err("`branch` must be a string".into()),
    }
}

#[async_trait::async_trait]
impl Tool for Push {
    fn name(&self) -> &'static str {
        PUSH_TOOL
    }
    fn description(&self) -> &'static str {
        "Push one branch of this project's folder to the forge repository the project was \
         cloned from, as the person who owns this chat. The person is shown what will be pushed \
         (repository, branch, commit) and must allow it every time; nothing else can be pushed: \
         no other repository, no tags, no deleting, no force. Commit first. `branch`: the \
         branch to push (default: the branch the folder is on); it goes to the branch of the \
         same name. If the forge refuses (the branch moved there, a protected branch), its \
         answer is returned: this tool cannot fetch or merge."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "branch": {
                    "type": "string",
                    "description": "The branch to push (default: the folder's current branch)"
                }
            }
        })
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::External
    }
    fn refusal(&self, ctx: &ToolCtx, input: &Value) -> Option<String> {
        if ctx.push.is_none() {
            return Some(NOT_HERE.into());
        }
        branch_arg(input).err()
    }
    async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome {
        let Some(host) = ctx.push.clone() else {
            return ToolOutcome::err(NOT_HERE);
        };
        if let Err(e) = branch_arg(&input) {
            return ToolOutcome::err(e);
        }
        match host.push(&ctx.call_id, &input, ctx.cancel.clone()).await {
            Ok(text) => ToolOutcome::ok(text),
            Err(e) => ToolOutcome::err(format!("push: {}", clean_line(&e, 1200))),
        }
    }
}

/// Why a call is refused where the node offers no push.
pub const NOT_HERE: &str = "push is not available in this turn: only in a project chat's own \
     turn that its owner started, for a project cloned from a forge, with the owner's connection \
     to that forge, on a node that pushes";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branches() {
        assert_eq!(branch_arg(&json!({})).unwrap(), None);
        assert_eq!(branch_arg(&json!({"branch": ""})).unwrap(), None);
        assert_eq!(
            branch_arg(&json!({"branch": "feature/x"})).unwrap(),
            Some("feature/x".into())
        );
        for bad in [
            json!({"branch": "+main"}),
            json!({"branch": "a:b"}),
            json!({"branch": "--delete"}),
            json!({"branch": 1}),
        ] {
            assert!(branch_arg(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn it_asks_every_time_and_never_runs_without_the_node() {
        assert_eq!(Push.risk(), ToolRisk::External);
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx::new(dir.path());
        assert_eq!(Push.refusal(&ctx, &json!({})).as_deref(), Some(NOT_HERE));
    }
}
