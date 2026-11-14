//! The agent backend boundary (brief §2.6). The node's session actor is the
//! host: it owns seq assignment, persistence and approvals. A backend runs one
//! turn and reports through the host. The native loop implements this today;
//! an ACP backend driving external agents will implement it later.

use std::path::PathBuf;

use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::RawEvent;
use crate::event::{EventKind, Source, TurnEndReason, Usage};
use crate::policy::{Decision, ToolRisk};
use crate::refs::SharedRefs;

#[async_trait::async_trait]
pub trait TurnHost: Send + Sync {
    /// Append an event to the session log (assigns seq, persists, publishes).
    /// The log is written first: on `Err` nothing the event describes may
    /// happen, and the backend ends the turn (no further tool body, no
    /// further model request).
    async fn emit(&self, kind: EventKind) -> Result<(), LogError>;

    /// Ask whether a tool call may run. The host applies node policy first
    /// and only asks the user if policy requires it. Emits the approval events.
    async fn approve(&self, call_id: &str, tool: &str, input: &Value, risk: ToolRisk) -> Decision;

    /// Run `task` as `agent` in a new child session on this node and wait for
    /// its turn to end. `started` is called with the child's id as soon as it
    /// exists (so the UI can link to it while it works). The child labels its
    /// web results from `refs`, the caller's book. `observed` is the URL
    /// provenance the caller holds (the user's words and trusted tool
    /// results, never the task): the child's `web_fetch` may follow these.
    async fn delegate(
        &self,
        _agent: crate::agent::AgentSpec,
        _task: String,
        _refs: SharedRefs,
        _observed: std::sync::Arc<Vec<String>>,
        _cancel: CancellationToken,
        _started: Box<dyn FnOnce(crate::SessionId) + Send>,
    ) -> Result<DelegateOutcome, String> {
        Err("delegation is not available here".into())
    }

    /// Whether this node's policy denies `tool` outright (`denied_tools`).
    /// A sub-agent whose declared tools are all denied or unregistered here
    /// is not offered as a delegate.
    fn tool_denied(&self, _tool: &str) -> bool {
        false
    }

    /// Where this session's turns publish files (`publish`); `None` when
    /// publishing is off here (chat sessions, nodes without staging).
    fn artifacts(&self) -> Option<std::sync::Arc<dyn crate::artifacts::ArtifactSink>> {
        None
    }

    /// This session's folder as pinned when the session was created
    /// ([`crate::folder::FolderPin`]): every tool call checks it before it
    /// reads, writes or starts anything there. `None` where no folder is
    /// pinned (chat sessions, tests).
    fn folder_pin(&self) -> Option<crate::folder::FolderPin> {
        None
    }

    /// The node-owned folder this session works in, when it is a
    /// sub-agent's ([`crate::folder::OwnedFolder`]): the tools and the
    /// sandbox grant it although it is inside a protected folder.
    fn owned_folder(&self) -> Option<crate::folder::OwnedFolder> {
        None
    }

    /// How this session's commands run (N.10a), as its `session_created`
    /// recorded it: the backend starts its shell in that mode's
    /// confinement. `None` for chat sessions and tests (the backend's
    /// default confinement).
    fn exec_mode(&self) -> Option<crate::policy::ExecMode> {
        None
    }

    /// The bytes of this session's images among `sha256` (those it has; an
    /// image it lacks is named to the model instead of shown).
    async fn images(&self, _sha256: &[String]) -> Vec<ImageData> {
        Vec::new()
    }

    /// This session's table of background commands
    /// (`harness_tools::background::SessionBg`, docs/background-commands.md
    /// §4.3), owned by the node and bounded by its `[limits]`. Type-erased
    /// because this crate does not know the tools; `None` where background
    /// commands do not exist (the chat runtime, tests). The loop refuses
    /// them for a sub-agent whatever this returns.
    fn background(&self) -> Option<std::sync::Arc<dyn std::any::Any + Send + Sync>> {
        None
    }

    /// This session's MCP servers (`harness_tools::mcp::SessionMcp`,
    /// ADOPTION_PLAN X.3), made by the node from its own node.toml alone.
    /// Type-erased like [`Self::background`]; `None` where there are none
    /// (chat sessions, sub-agents, nodes without servers, tests). The loop
    /// offers none to a sub-agent whatever this returns.
    fn mcp(&self) -> Option<std::sync::Arc<dyn std::any::Any + Send + Sync>> {
        None
    }

    /// At a step boundary (after a step's tool results are recorded, before
    /// the next model call): append the background ends queued since the
    /// last one (`delivered: true`) and return them, so the loop adds their
    /// notes where the log has them. On `Err` the turn ends.
    async fn background_notes(&self) -> Result<Vec<crate::event::BackgroundEnded>, LogError> {
        Ok(Vec::new())
    }

    /// Before each model request of a turn: append one `queued_delivered`
    /// for the person's messages sent while this turn ran that the model
    /// has not read (`user_message.queued`, X.5), and return them in the
    /// order they were sent, so the loop adds them where the log has them.
    /// On `Err` the turn ends.
    async fn queued_messages(&self) -> Result<Vec<crate::event::UserMessage>, LogError> {
        Ok(Vec::new())
    }
}

/// An event could not be appended to the session log (full disk, I/O
/// error, a broken database). Holds the store's reason, never the payload.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the session log could not be written ({0})")]
pub struct LogError(pub String);

/// An image's bytes for the model (`TurnHost::images`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageData {
    pub sha256: String,
    pub mime: String,
    /// Base64, shared: a turn copies its messages on every step.
    pub data: std::sync::Arc<str>,
}

/// How a delegated sub-task ended.
#[derive(Debug, Clone, PartialEq)]
pub struct DelegateOutcome {
    pub child: crate::SessionId,
    /// The sub-agent's last assistant text (its report when it never
    /// called `finish`).
    pub text: String,
    pub reason: TurnEndReason,
    pub usage: Option<Usage>,
    /// The pages the child's web tools returned, with the labels its answer
    /// cites them by.
    pub sources: Vec<Source>,
    /// The `finish` call that ended its turn, from its log.
    pub finish: Option<crate::subagent::Finish>,
    /// The turn's own end text (its `error` event: the step cap, the
    /// repeated-call guard, a provider error), from its log.
    pub error: Option<String>,
    /// The harness's own note after the report: what became of the child's
    /// changes, or why it stopped.
    pub note: Option<String>,
}

pub struct TurnCtx<'a> {
    /// The session this turn belongs to. Per-session state kept by a
    /// backend shared by many sessions (stored tool outputs) is keyed by it.
    pub session_id: crate::SessionId,
    pub turn_id: String,
    /// Every event of this session before this turn, in seq order.
    pub history: Vec<RawEvent>,
    pub user_text: String,
    pub cwd: PathBuf,
    pub model: String,
    /// The agent running this turn; `None` means the backend's defaults.
    pub agent: Option<crate::agent::AgentSpec>,
    /// Parallel answers: run the turn once per model listed here (no tools)
    /// and record each answer as a variant. Empty = a normal turn.
    pub compare: Vec<String>,
    /// Set when this session is a delegated sub-task: the session whose
    /// turn delegated to it. Passed explicitly, not read from `history`, so
    /// a failed history read cannot lift the sub-task's limits.
    pub parent_session: Option<crate::SessionId>,
    pub extra: TurnExtra,
    pub cancel: CancellationToken,
    pub host: &'a dyn TurnHost,
}

/// Per-turn inputs that are not part of the agent definition.
#[derive(Debug, Clone, Default)]
pub struct TurnExtra {
    /// Token-saving methods in effect for this turn.
    pub savings: crate::savings::Savings,
    /// Context that changes every turn (memory recall, page context). It is
    /// sent after the history, not in the system prompt, so the cached
    /// prefix stays the same from turn to turn. Never stored.
    pub context: Option<String>,
    /// The repository's instruction files (`AGENTS.md`, `CLAUDE.md`), and
    /// the catalogue of skills after them, read by the node at the start
    /// of this turn. Set only by the node, never
    /// from the protocol. Sent after the history like `context`, in front
    /// of it; a callback turn, which takes no `context`, gets them after
    /// its opening. Never stored.
    pub instructions: Option<String>,
    /// The skills the node found for this turn (`crate::skills`), which
    /// the `skill` tool loads from; their catalogue is at the end of
    /// `instructions`. Set only by the node, never from the protocol.
    pub skills: Option<std::sync::Arc<crate::skills::Catalogue>>,
    /// A delegated sub-task's parent book: its labels continue the parent's
    /// numbering, so `[S3]` in the answer it returns names the parent's S3.
    /// `None` means the book comes from this session's own log.
    pub refs: Option<SharedRefs>,
    /// A delegated sub-task's inherited URL provenance (see
    /// `TurnHost::delegate`). Set only by the node, never from the protocol.
    pub observed: Option<std::sync::Arc<Vec<String>>>,
    /// The images sent with this turn's message (their bytes come from
    /// `TurnHost::images`).
    pub images: Vec<crate::event::ImageRef>,
    /// Why a turn with no person's message runs (a callback turn, T.8d
    /// step 3): it opens with the harness's frame instead of `user_text`,
    /// which is then empty. Set only by the node, after its own checks.
    pub cause: Option<crate::event::TurnCause>,
    /// A lower step cap for this turn than the backend's own `max_steps`
    /// (a callback turn's `callback_max_steps`). Set only by the node.
    pub max_steps: Option<usize>,
    /// Messages the person sent while an earlier turn ran that it never
    /// read (X.5): the node recorded them as read (`queued_delivered`)
    /// right after this turn's `turn_started`, and the loop sends them, in
    /// order, before this turn's own message. Set only by the node.
    pub queued: Vec<crate::event::UserMessage>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TurnOutcome {
    pub reason: TurnEndReason,
    pub usage: Option<Usage>,
    pub error: Option<String>,
}

#[async_trait::async_trait]
pub trait AgentBackend: Send + Sync {
    fn name(&self) -> &'static str;
    async fn run_turn(&self, ctx: TurnCtx<'_>) -> TurnOutcome;
    /// Whether this backend has a tool of that name registered (for what a
    /// node announces; the agent and the node's policy still decide each
    /// turn's tools).
    fn has_tool(&self, _name: &str) -> bool {
        false
    }
    /// The session was deleted: drop whatever is kept in memory for it.
    fn forget_session(&self, _session: crate::SessionId) {}
}
