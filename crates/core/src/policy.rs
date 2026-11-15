use serde::{Deserialize, Serialize};

/// How much a session may do without a human click. Ordered from strictest to
/// loosest so that `min` picks the stricter mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    /// Plan mode (ADOPTION_PLAN X.7, PLAN.md T67): the agent investigates
    /// and writes a plan; nothing changes. Not "ask": a call that would
    /// change something is refused by the node before any card
    /// ([`ApprovalMode::refuses`]), `bash` runs in the read-only shell, and
    /// no answer can lift it. The strictest mode, so the clamp, a
    /// sub-agent's ceiling and a webhook run's cap keep it. Set and left
    /// only by the chat's owner, through the same route and event as the
    /// other modes.
    Plan,
    /// Every tool call needs approval.
    AskAll,
    /// Mutating tools (bash, edit, write) need approval; read-only tools run.
    #[default]
    Ask,
    /// As `Ask`, except that `write` and `edit` of a file inside the chat's
    /// own folder run without a card: they are checkpointed and undone with
    /// the turn. The node decides what "inside" means
    /// ([`ApprovalMode::accepts_edits`]); `bash`, `publish` and everything
    /// else still ask.
    AcceptEdits,
    /// Nothing needs approval.
    Auto,
}

/// The mode a new chat of every kind gets when its create names none, before
/// the node clamps it to its own ceiling: `auto`, the owner's decision of
/// 2026-11-14 ("default accept mode is Auto for every chat type", PLAN.md
/// T50). No mode is limited by the chat's kind or its execution mode.
pub const DEFAULT_CHAT_APPROVAL: ApprovalMode = ApprovalMode::Auto;

/// The file tools `AcceptEdits` covers, by their registered names.
pub const EDIT_TOOLS: [&str; 2] = ["write", "edit"];

/// The one tool that changes things plan mode runs: `bash`, in the
/// read-only shell (ADOPTION_PLAN T.7); where the node has none it is
/// refused too.
pub const PLAN_SHELL: &str = crate::agent::SHELL;

/// The orchestration tool: whether plan mode starts a child is decided per
/// agent by the loop (a writer is refused, a reader inherits the mode), so
/// [`ApprovalMode::refuses`] leaves it alone.
pub const DELEGATE: &str = "delegate";

/// Tools on the read-only list ([`crate::agent::READ_ONLY_TOOLS`]) that
/// plan mode refuses all the same: `remember` files a memory through the master,
/// which changes what later chats are told.
pub const PLAN_REFUSED: &[&str] = &["remember"];

/// The result of a call plan mode refuses (a plain sentence the model can
/// act on; no card is shown and nothing runs).
pub fn plan_refusal(tool: &str) -> String {
    format!(
        "[harness] plan mode: `{}` was not run and nothing was changed. In plan mode you may \
         read, search and look; describe this change in your plan instead. Only the person can \
         end plan mode.",
        crate::event::shown_name(tool)
    )
}

impl ApprovalMode {
    /// The effective mode is the stricter of what was requested and what the
    /// node allows. A master can tighten, never loosen.
    pub fn clamp(requested: ApprovalMode, node_max: ApprovalMode) -> ApprovalMode {
        requested.min(node_max)
    }

    /// Whether a call of this risk asks, on its risk alone (after
    /// [`Self::refuses`], which plan mode needs). `AcceptEdits`
    /// asks as `Ask` does here: the node lets an edit inside the folder
    /// through before it gets here (it alone can resolve the path).
    pub fn needs_approval(self, risk: ToolRisk) -> bool {
        match self {
            // What plan mode lets run, runs unasked (`bash` in the
            // read-only shell); what it does not is refused, never asked
            // ([`Self::refuses`], which every caller checks first).
            ApprovalMode::Plan => false,
            ApprovalMode::AskAll => risk != ToolRisk::ChatOnly,
            ApprovalMode::Ask | ApprovalMode::AcceptEdits => risk == ToolRisk::Mutating,
            ApprovalMode::Auto => false,
        }
    }

    /// Whether `write` and `edit` inside the chat's folder run unasked.
    pub fn accepts_edits(self) -> bool {
        self >= ApprovalMode::AcceptEdits
    }

    /// Whether plan mode refuses a call of `tool` outright: every tool
    /// that changes things by the shared rule ([`crate::agent::tool_changes`],
    /// an allow-list: a tool not on it, and every MCP tool, is refused) but
    /// `bash` (which runs in the read-only shell there), and the listed
    /// tools that change something outside the chat ([`PLAN_REFUSED`]).
    /// `delegate` is the loop's (a writer agent is refused there, and every
    /// child inherits the mode). Always `false` in every other mode.
    pub fn refuses(self, tool: &str) -> bool {
        self == ApprovalMode::Plan
            && tool != DELEGATE
            && (crate::agent::tool_changes(tool, true) || PLAN_REFUSED.contains(&tool))
    }

    /// Every mode, strictest first.
    pub const ALL: [ApprovalMode; 5] = [
        ApprovalMode::Plan,
        ApprovalMode::AskAll,
        ApprovalMode::Ask,
        ApprovalMode::AcceptEdits,
        ApprovalMode::Auto,
    ];

    /// The wire name (`plan`, `ask_all`, `ask`, `accept_edits`, `auto`).
    pub fn id(self) -> &'static str {
        match self {
            ApprovalMode::Plan => "plan",
            ApprovalMode::AskAll => "ask_all",
            ApprovalMode::Ask => "ask",
            ApprovalMode::AcceptEdits => "accept_edits",
            ApprovalMode::Auto => "auto",
        }
    }

    pub fn parse(s: &str) -> Option<ApprovalMode> {
        Self::ALL.into_iter().find(|m| m.id() == s)
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
    /// Allow this exact call for the rest of the turn (the wire name is
    /// older than the turn scope).
    AllowSession,
    /// Allow this exact call for the rest of the chat: the node records the
    /// grant (`chat_allow_added`), rebuilds it from the log after a restart,
    /// and never passes it to a sub-agent or another chat.
    AllowChat,
}

/// The most characters a person's note with a denial may have; the master
/// refuses a longer one and the node cuts one to it.
pub const MAX_DENY_NOTE: usize = 2000;

impl Decision {
    pub fn allows(self) -> bool {
        !matches!(self, Decision::Deny)
    }
}

/// Model calls with tools one turn may make when node.toml does not say,
/// and the range node.toml may set it in (`[limits] max_steps`). 1,000 by
/// the owner's decision (2026-11-15: "Raise max to 1000 by default + add
/// ability for session owners to override max steps (min: 1 - {max
/// setted})"); a chat's owner may set a lower limit for that chat (X.14).
pub const DEFAULT_MAX_STEPS: usize = 1000;
pub const MIN_MAX_STEPS: usize = 1;
pub const MAX_MAX_STEPS: usize = 10_000;

/// Seconds a model call may wait for the provider's first answer (its
/// response headers, then its first event), when node.toml does not say
/// (`[limits] model_first_byte_timeout_s`): longer than the gap between
/// events, since a reasoning model may send nothing while it thinks and a
/// local server may hold its headers while it reads the prompt.
pub const DEFAULT_MODEL_FIRST_BYTE_S: u64 = 600;
/// Seconds the stream may then go without an event (keep-alive pings
/// count), when node.toml does not say (`[limits] model_idle_timeout_s`):
/// Codex's stream idle timeout; OpenClaw's for self-hosted models.
pub const DEFAULT_MODEL_IDLE_S: u64 = 300;
/// The range node.toml may set either in.
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

    /// The order the clamp relies on, and the wire names.
    #[test]
    fn modes_are_ordered_and_named() {
        use ApprovalMode::*;
        assert!(Plan < AskAll && AskAll < Ask && Ask < AcceptEdits && AcceptEdits < Auto);
        // Plan is the strictest: a node's ceiling, a parent's mode and a
        // webhook run's cap of `ask` all keep it.
        for m in ApprovalMode::ALL {
            assert_eq!(ApprovalMode::clamp(Plan, m), Plan);
            assert_eq!(ApprovalMode::clamp(m, Plan), Plan);
        }
        assert!(!Plan.accepts_edits());
        assert_eq!(ApprovalMode::clamp(Auto, AcceptEdits), AcceptEdits);
        assert_eq!(ApprovalMode::clamp(AcceptEdits, Ask), Ask);
        for m in ApprovalMode::ALL {
            assert_eq!(ApprovalMode::parse(m.id()), Some(m));
            assert_eq!(serde_json::to_value(m).unwrap(), m.id());
        }
        assert_eq!(ApprovalMode::parse("yolo"), None);
        assert!(serde_json::from_value::<ApprovalMode>(serde_json::json!("yolo")).is_err());
        // Accept edits asks for every mutating call on its risk alone.
        assert!(AcceptEdits.needs_approval(ToolRisk::Mutating));
        assert!(!AcceptEdits.needs_approval(ToolRisk::ReadOnly));
        assert!(AcceptEdits.accepts_edits() && Auto.accepts_edits());
        assert!(!Ask.accepts_edits() && !AskAll.accepts_edits());
        assert_eq!(
            DEFAULT_CHAT_APPROVAL, Auto,
            "the owner's default (2026-11-14)"
        );
    }

    #[test]
    fn needs_approval_matrix() {
        assert!(ApprovalMode::Ask.needs_approval(ToolRisk::Mutating));
        assert!(!ApprovalMode::Ask.needs_approval(ToolRisk::ReadOnly));
        assert!(ApprovalMode::AskAll.needs_approval(ToolRisk::ReadOnly));
        assert!(!ApprovalMode::Auto.needs_approval(ToolRisk::Mutating));
    }

    /// Plan mode refuses what changes things by the shared list (but
    /// `bash`, which it runs read-only) and `remember`; it never refuses in
    /// another mode. A tool not on the list is refused, whatever risk it
    /// declares (PLAN.md T70).
    #[test]
    fn plan_mode_refuses_what_changes_something() {
        use ApprovalMode::*;
        for (tool, refused) in [
            ("write", true),
            ("edit", true),
            ("publish", true),
            ("mcp__srv__tool", true),
            ("mcp__srv__read", true),
            ("a_tool_added_later", true),
            ("bash", false),
            ("bash_output", false),
            ("remember", true),
            ("read", false),
            ("recall", false),
            ("web_fetch", false),
            ("todo", false),
            // The loop's, per agent.
            ("delegate", false),
        ] {
            assert_eq!(Plan.refuses(tool), refused, "{tool}");
            for m in [AskAll, Ask, AcceptEdits, Auto] {
                assert!(!m.refuses(tool), "{m:?} {tool}");
            }
        }
        // Nothing asks in plan mode: it runs, or it is refused.
        assert!(!Plan.needs_approval(ToolRisk::Mutating));
        assert!(!Plan.needs_approval(ToolRisk::ReadOnly));
        assert!(plan_refusal("write").contains("plan mode: `write` was not run"));
    }

    /// A tool that writes only the chat's own log (`todo`) is never asked
    /// about, `ask_all` included (TD-6).
    #[test]
    fn chat_only_is_never_asked() {
        for mode in ApprovalMode::ALL {
            assert!(!mode.needs_approval(ToolRisk::ChatOnly), "{mode:?}");
        }
    }
}
