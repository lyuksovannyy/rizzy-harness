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
