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

/// What a person may run (O13, N.10a part 2; docs/execution-modes.md): an
/// account's ceiling, set by server admins, and a member's level in a
/// workspace, set by its admins. Ordered, so the effective level is the
/// `min` of the two, and a session's mode must be at most it. Each level
/// includes the ones before it. `Container` exists as a level, but no node
/// offers the container mode until N.4, so it allows no mode yet.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ExecLevel {
    /// No agent chat at all (every new account and every new member).
    #[default]
    None,
    /// Sessions in an environment an admin defined (rootless Podman, N.4).
    Container,
    /// Also `sandbox` sessions (bubblewrap).
    Sandbox,
    /// Also `host` sessions: a shell as the node's user, unconfined.
    Host,
}

impl ExecLevel {
    /// Every level, lowest first.
    pub const ALL: [ExecLevel; 4] = [
        ExecLevel::None,
        ExecLevel::Container,
        ExecLevel::Sandbox,
        ExecLevel::Host,
    ];

    pub fn id(self) -> &'static str {
        match self {
            ExecLevel::None => "none",
            ExecLevel::Container => "container",
            ExecLevel::Sandbox => "sandbox",
            ExecLevel::Host => "host",
        }
    }

    pub fn parse(s: &str) -> Option<ExecLevel> {
        Self::ALL.into_iter().find(|l| l.id() == s)
    }

    /// The level a session of mode `m` needs.
    pub fn needed_for(m: ExecMode) -> ExecLevel {
        match m {
            ExecMode::Sandbox => ExecLevel::Sandbox,
            ExecMode::Host => ExecLevel::Host,
        }
    }

    /// Whether this level may have (create, and drive) a session of mode `m`.
    pub fn allows(self, m: ExecMode) -> bool {
        self >= Self::needed_for(m)
    }

    /// The most confined existing mode this level allows, the one a create
    /// without a request gets; `None` at `none` and at `container` (no
    /// container mode exists yet).
    pub fn default_mode(self) -> Option<ExecMode> {
        ExecMode::ALL.into_iter().find(|m| self.allows(*m))
    }
}

#[cfg(test)]
mod level_tests {
    use super::*;

    #[test]
    fn levels_are_ordered_and_allow_the_modes_below_them() {
        assert!(ExecLevel::None < ExecLevel::Container);
        assert!(ExecLevel::Container < ExecLevel::Sandbox);
        assert!(ExecLevel::Sandbox < ExecLevel::Host);
        assert_eq!(ExecLevel::default(), ExecLevel::None);
        for l in ExecLevel::ALL {
            assert_eq!(ExecLevel::parse(l.id()), Some(l));
            assert_eq!(serde_json::to_value(l).unwrap(), l.id());
        }
        assert_eq!(ExecLevel::parse("root"), None);
        // `container` allows no mode until N.4.
        for l in [ExecLevel::None, ExecLevel::Container] {
            assert!(!l.allows(ExecMode::Sandbox) && !l.allows(ExecMode::Host));
            assert_eq!(l.default_mode(), None);
        }
        assert!(ExecLevel::Sandbox.allows(ExecMode::Sandbox));
        assert!(!ExecLevel::Sandbox.allows(ExecMode::Host));
        assert!(ExecLevel::Host.allows(ExecMode::Host));
        // Never `host` by default.
        assert_eq!(ExecLevel::Host.default_mode(), Some(ExecMode::Sandbox));
        assert_eq!(ExecLevel::Sandbox.min(ExecLevel::Host), ExecLevel::Sandbox);
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

/// Seconds a model's stream may send nothing (no headers yet, or no byte
/// of the body) before the call fails as a broken stream, when node.toml
/// does not say, and the range it may set (`[limits]
/// model_idle_timeout_s`). Codex's value; OpenClaw's for self-hosted
/// models (its cloud default is 120 s, which a reasoning model that sends
/// nothing while it thinks can pass).
pub const DEFAULT_MODEL_IDLE_S: u64 = 300;
pub const MIN_MODEL_IDLE_S: u64 = 10;
pub const MAX_MODEL_IDLE_S: u64 = 3600;

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
