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
            ApprovalMode::AskAll => true,
            ApprovalMode::Ask => risk == ToolRisk::Mutating,
            ApprovalMode::Auto => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRisk {
    ReadOnly,
    Mutating,
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
}
