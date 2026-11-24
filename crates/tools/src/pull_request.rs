//! `pull_request`: open a pull request (GitLab: a merge request) in the
//! forge repository the chat's project was cloned from, as the chat's owner
//! (ADOPTION_PLAN X.27, PLAN.md T102, `docs/forge-pr.md`).
//!
//! Nothing here holds a token or makes a call: the session's node shows
//! the person a card with exactly this call's input (every time, in every
//! mode) and, after an Allow, asks the master, which creates the pull
//! request from the input of this call as its own log holds it. The tool
//! takes branch names, a title, a description and whether it is a draft;
//! no repository, fork or URL.

use harness_core::forge::{MAX_PR_BODY, MAX_PR_TITLE, PR_TOOL, parse_pr};
use harness_core::policy::ToolRisk;
use harness_core::text::clean_line;
use serde_json::{Value, json};

use crate::{Tool, ToolCtx, ToolOutcome};

/// The tool.
pub struct PullRequest;

#[async_trait::async_trait]
impl Tool for PullRequest {
    fn name(&self) -> &'static str {
        PR_TOOL
    }
    fn description(&self) -> &'static str {
        "Open a pull request (GitLab: a merge request) in the forge repository this project was \
         cloned from, as the person who owns this chat, from a branch that is already on the \
         forge (push it first) into a base branch of the same repository. The person sees the \
         repository, the branches, the title and the whole description and must allow it every \
         time. Write a short title and a description of what changed and why. `base` defaults \
         to the repository's default branch. The forge's answer is returned: the pull request's \
         number and address, or why it refused (for example no commits between the branches, \
         or one already open)."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "head": { "type": "string", "description": "The branch with the work, already pushed" },
                "base": { "type": "string", "description": "The branch to merge into (default: the repository's default branch)" },
                "title": { "type": "string", "maxLength": MAX_PR_TITLE, "description": "One line" },
                "body": { "type": "string", "maxLength": MAX_PR_BODY, "description": "The description (Markdown)" },
                "draft": { "type": "boolean", "description": "Open it as a draft" }
            },
            "required": ["head", "title"]
        })
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::External
    }
    fn refusal(&self, ctx: &ToolCtx, input: &Value) -> Option<String> {
        if ctx.push.is_none() {
            return Some(NOT_HERE.into());
        }
        parse_pr(input).err()
    }
    async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome {
        let Some(host) = ctx.push.clone() else {
            return ToolOutcome::err(NOT_HERE);
        };
        if let Err(e) = parse_pr(&input) {
            return ToolOutcome::err(e);
        }
        match host
            .open_pull_request(&ctx.call_id, &input, ctx.cancel.clone())
            .await
        {
            Ok(text) => ToolOutcome::ok(text),
            Err(e) => ToolOutcome::err(format!("pull_request: {}", clean_line(&e, 1200))),
        }
    }
}

/// Why a call is refused where the node offers no pull requests.
pub const NOT_HERE: &str = "pull_request is not available in this turn: only in a project \
     chat's own turn that its owner started, for a project cloned from a forge, with the \
     owner's connection to that forge, on a node and server that offer it";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_asks_every_time_and_never_runs_without_the_node() {
        assert_eq!(PullRequest.risk(), ToolRisk::External);
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx::new(dir.path());
        assert_eq!(
            PullRequest
                .refusal(&ctx, &json!({"head": "a", "title": "t"}))
                .as_deref(),
            Some(NOT_HERE)
        );
    }
}
