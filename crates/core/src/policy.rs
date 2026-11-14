use serde::{Deserialize, Serialize};

/// How much a session may do without a human click. Ordered from strictest to
/// loosest so that `min` picks the stricter mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    /// Every tool call needs approval.
    AskAll,
    /// Mutating tools (bash, edit, write) need approval; read-only tools run.
    #[default]
    Ask,
    /// Nothing needs approval.
    Auto,
}

impl ApprovalMode {
    /// The effective mode is the stricter of what was requested and what the
    /// node allows. A master can tighten, never loosen.
    pub fn clamp(requested: ApprovalMode, node_max: ApprovalMode) -> ApprovalMode {
        requested.min(node_max)
    }

    pub fn needs_approval(self, risk: ToolRisk) -> bool {
        match self {
            ApprovalMode::AskAll => risk != ToolRisk::ChatOnly,
            ApprovalMode::Ask => risk == ToolRisk::Mutating,
            ApprovalMode::Auto => false,
        }
    }
}

/// How an agentic session's commands run (O13, N.10a; docs/execution-modes.md).
/// Chosen when the session is created, recorded in its `session_created`
/// and never changed. Ordered from the most confined, so `min` picks the
/// more confined mode. `container` (rootless Podman, N.4) joins below
/// `Sandbox` once a node can offer it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecMode {
    /// Each command in bubblewrap: its own PID namespace and file view,
    /// under its guard, Landlock inside (T.8a to T.8e).
    Sandbox,
    /// Each command as the node's own user, without bubblewrap: at most
    /// the Landlock rules of `[sandbox] mode`, no process containment.
    /// Offered only where the node's admin set `[execution] host = true`.
    Host,
}

impl ExecMode {
    /// Every mode, most confined first.
    pub const ALL: [ExecMode; 2] = [ExecMode::Sandbox, ExecMode::Host];

    pub fn id(self) -> &'static str {
        match self {
            ExecMode::Sandbox => "sandbox",
            ExecMode::Host => "host",
        }
    }

    pub fn parse(s: &str) -> Option<ExecMode> {
        Self::ALL.into_iter().find(|m| m.id() == s)
    }

    /// The loosest approval mode a session of this mode may have (review
    /// N-e): a `host` session never runs in `auto`, because its approval
    /// card is the only check between a steered agent and the node's user.
    pub fn max_approval(self) -> ApprovalMode {
        match self {
            ExecMode::Sandbox => ApprovalMode::Auto,
            ExecMode::Host => ApprovalMode::Ask,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRisk {
    ReadOnly,
    Mutating,
    /// Writes nothing but the chat's own log (the agent's task list): no
    /// mode asks about it, `ask_all` included. A node's `denied_tools`
    /// still refuses it.
    ChatOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allow,
    Deny,
    /// Allow this tool for the rest of the session.
    AllowSession,
}

impl Decision {
    pub fn allows(self) -> bool {
        !matches!(self, Decision::Deny)
    }
}

/// Model calls one turn may make when node.toml does not say, and the range
/// node.toml may set it in (`[limits] max_steps`).
pub const DEFAULT_MAX_STEPS: usize = 50;
pub const MIN_MAX_STEPS: usize = 1;
pub const MAX_MAX_STEPS: usize = 1000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_never_loosens() {
        use ApprovalMode::*;
        assert_eq!(ApprovalMode::clamp(Auto, Ask), Ask);
        assert_eq!(ApprovalMode::clamp(AskAll, Auto), AskAll);
        assert_eq!(ApprovalMode::clamp(Ask, AskAll), AskAll);
    }

    #[test]
    fn needs_approval_matrix() {
        assert!(ApprovalMode::Ask.needs_approval(ToolRisk::Mutating));
        assert!(!ApprovalMode::Ask.needs_approval(ToolRisk::ReadOnly));
        assert!(ApprovalMode::AskAll.needs_approval(ToolRisk::ReadOnly));
        assert!(!ApprovalMode::Auto.needs_approval(ToolRisk::Mutating));
    }

    /// A tool that writes only the chat's own log (`todo`) is never asked
    /// about, `ask_all` included (TD-6).
    #[test]
    fn chat_only_is_never_asked() {
        for mode in [ApprovalMode::AskAll, ApprovalMode::Ask, ApprovalMode::Auto] {
            assert!(!mode.needs_approval(ToolRisk::ChatOnly), "{mode:?}");
        }
    }
}
