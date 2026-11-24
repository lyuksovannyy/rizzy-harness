//! Pushing a project's branch to the forge repository it was cloned from
//! (ADOPTION_PLAN X.27 step 4, PLAN.md T101, `docs/forge-push.md`).
//!
//! The master decides per turn whether a chat may push and names where to
//! ([`PushTarget`], on the turn's top agent spec): the project's recorded
//! source and the chat owner's own connection for that host. Nothing here
//! is secret. The node offers the `push` tool only with a target, runs the
//! push itself and asks the master for the connection's token only after
//! the person allowed the one push on its card.

use serde::{Deserialize, Serialize};

/// The tool's name.
pub const PUSH_TOOL: &str = "push";

/// The open-pull-request tool's name (GitLab: a merge request; X.27,
/// PLAN.md T102, `docs/forge-pr.md`). It runs on the master: the node shows
/// the card and files the request; the master creates the pull request from
/// the call's input as its own log holds it.
pub const PR_TOOL: &str = "pull_request";

/// The tools that act as the person on their forge account: never brought
/// by `tools: all`, a card every time.
pub const FORGE_TOOLS: [&str; 2] = [PUSH_TOOL, PR_TOOL];

/// The longest title of a pull request, in characters.
pub const MAX_PR_TITLE: usize = 256;

/// The longest description of a pull request, in characters.
pub const MAX_PR_BODY: usize = 20_000;

/// A pull request the agent asks for, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrRequest {
    /// The branch with the work (on the source repository itself).
    pub head: String,
    /// The branch to merge into; `None`: the repository's default branch.
    pub base: Option<String>,
    pub title: String,
    pub body: String,
    pub draft: bool,
}

/// The tool's input as a [`PrRequest`], by the one rule the node (before
/// its card) and the master (on the input its own log holds) both apply:
/// only the keys `head`, `base`, `title`, `body`, `draft`; branch names by
/// [`valid_branch`], head and base different; a title of 1 to
/// [`MAX_PR_TITLE`] characters with no control character; a body of at
/// most [`MAX_PR_BODY`] characters with no control character but newline
/// and tab. Hidden and bidirectional characters are kept (the card shows
/// them) and sent as they are.
pub fn parse_pr(input: &serde_json::Value) -> Result<PrRequest, String> {
    let o = input.as_object().ok_or("the arguments must be an object")?;
    if let Some(k) = o
        .keys()
        .find(|k| !matches!(k.as_str(), "head" | "base" | "title" | "body" | "draft"))
    {
        return Err(format!(
            "unknown argument `{}`: only head, base, title, body and draft",
            k.chars().take(40).collect::<String>()
        ));
    }
    let text = |k: &str| match o.get(k) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("`{k}` must be a string")),
    };
    let branch = |k: &str| -> Result<Option<String>, String> {
        match text(k)? {
            None => Ok(None),
            Some(b) if valid_branch(&b) => Ok(Some(b)),
            Some(_) => Err(format!(
                "`{k}` is not a branch name this tool takes (letters, digits and `. _ - + @`, \
                 parts separated by `/`, none starting with `.`, `-` or `+`, no `..`)"
            )),
        }
    };
    let head = branch("head")?.ok_or("`head` (the branch with the work) is required")?;
    let base = branch("base")?;
    if base.as_deref() == Some(head.as_str()) {
        return Err("`head` and `base` are the same branch".into());
    }
    let title = text("title")?.unwrap_or_default();
    let n = title.chars().count();
    if n == 0 || n > MAX_PR_TITLE {
        return Err(format!("`title` must be 1 to {MAX_PR_TITLE} characters"));
    }
    if title.chars().any(char::is_control) {
        return Err("`title` must be one line with no control characters".into());
    }
    let body = text("body")?.unwrap_or_default();
    if body.chars().count() > MAX_PR_BODY {
        return Err(format!("`body` must be at most {MAX_PR_BODY} characters"));
    }
    if body
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err("`body` may hold no control characters but newlines and tabs".into());
    }
    let draft = match o.get("draft") {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(_) => return Err("`draft` must be true or false".into()),
    };
    Ok(PrRequest {
        head,
        base,
        title,
        body,
        draft,
    })
}

/// The longest branch name the tool takes, in bytes.
pub const MAX_BRANCH: usize = 255;

/// Where a turn's push goes, as the master decided it for that turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PushTarget {
    /// The project's source host, as recorded at the clone.
    pub host: String,
    /// `owner/name` on that host, as recorded at the clone.
    pub full_name: String,
    /// The chat owner's connection that pushes (its row's id, not secret).
    pub connection_id: String,
    /// That connection's forge account name, for the card.
    pub login: String,
}

/// Where the node runs a session's `push` (its link to the master, its
/// folder, its approval card), bound to that session.
#[async_trait::async_trait]
pub trait PushHost: Send + Sync {
    /// Run call `call_id` with the tool's `input`: the card, the person's
    /// answer, the push. The text for the model, or why nothing was pushed.
    async fn push(
        &self,
        call_id: &str,
        input: &serde_json::Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<String, String>;

    /// Run call `call_id` of the open-pull-request tool: the card, the
    /// person's answer, the master's request (PLAN.md T102).
    async fn open_pull_request(
        &self,
        call_id: &str,
        input: &serde_json::Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<String, String>;
}

/// A branch name the tool pushes: 1 to [`MAX_BRANCH`] bytes of `/`-separated
/// components of `[A-Za-z0-9._+@-]`, none empty, starting with `.`, `-` or `+`,
/// ending in `.lock` or `.`, holding `..` or `@{`. Stricter than `git
/// check-ref-format`: no `:`, `*`, `^`, `~`, `?`, `[`, `\`, space or
/// control character can reach a refspec.
pub fn valid_branch(b: &str) -> bool {
    (1..=MAX_BRANCH).contains(&b.len())
        && !b.contains("..")
        && !b.contains("@{")
        && b != "@"
        && b.split('/').all(|c| {
            !c.is_empty()
                && !c.starts_with('.')
                && !c.starts_with('-')
                && !c.starts_with('+')
                && !c.ends_with(".lock")
                && !c.ends_with('.')
                && c.bytes()
                    .all(|x| x.is_ascii_alphanumeric() || b"-_.+@".contains(&x))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_names() {
        for ok in ["main", "feature/x", "fix-1.2", "a+b", "user@x", "a_b"] {
            assert!(valid_branch(ok), "{ok}");
        }
        for bad in [
            "",
            "+main",
            "-x",
            "a:b",
            "a*",
            "a..b",
            "a@{1}",
            "@",
            ".hidden",
            "x.lock",
            "x.",
            "a//b",
            "/a",
            "a/",
            "a b",
            "a\tb",
            "ä",
            "a^",
            "a~1",
            "refs/heads/../x",
            &"b".repeat(MAX_BRANCH + 1),
        ] {
            assert!(!valid_branch(bad), "{bad:?}");
        }
        assert!(valid_branch(&"b".repeat(MAX_BRANCH)));
    }

    #[test]
    fn pull_request_input() {
        use serde_json::json;
        let ok =
            parse_pr(&json!({"head": "feat/x", "title": "Add x", "body": "a\n\tb", "draft": true}))
                .unwrap();
        assert_eq!(ok.head, "feat/x");
        assert_eq!(ok.base, None);
        assert!(ok.draft);
        assert_eq!(
            parse_pr(&json!({"head": "a", "base": "main", "title": "t"}))
                .unwrap()
                .base
                .as_deref(),
            Some("main")
        );
        // Hidden and bidi characters are kept.
        assert!(parse_pr(&json!({"head": "a", "title": "t\u{202e}x"})).is_ok());
        let long_body = "b".repeat(MAX_PR_BODY + 1);
        for bad in [
            json!({"head": "a", "base": "a", "title": "t"}),
            json!({"head": "a..b", "title": "t"}),
            json!({"head": "a~1", "title": "t"}),
            json!({"head": "a b", "title": "t"}),
            json!({"head": "-a", "title": "t"}),
            json!({"head": "a", "title": "t".repeat(MAX_PR_TITLE + 1)}),
            json!({"head": "a", "title": "two\nlines"}),
            json!({"head": "a", "title": ""}),
            json!({"head": "a", "title": "t", "body": long_body}),
            json!({"head": "a", "title": "t", "body": "bell\u{7}"}),
            json!({"head": "a", "title": "t", "repo": "other/x"}),
            json!({"title": "t"}),
            json!({"head": "a", "title": "t", "draft": "yes"}),
        ] {
            assert!(parse_pr(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_target_names_no_secret_and_takes_nothing_else() {
        let t: PushTarget = serde_json::from_value(serde_json::json!({
            "host": "github.com", "full_name": "o/n", "connection_id": "c1", "login": "ada"
        }))
        .unwrap();
        assert_eq!(t.login, "ada");
        assert!(
            serde_json::from_value::<PushTarget>(serde_json::json!({
                "host": "h", "full_name": "o/n", "connection_id": "c", "login": "l", "token": "x"
            }))
            .is_err()
        );
    }
}
