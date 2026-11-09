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
    async fn emit(&self, kind: EventKind);

    /// Ask whether a tool call may run. The host applies node policy first
    /// and only asks the user if policy requires it. Emits the approval events.
    async fn approve(&self, call_id: &str, tool: &str, input: &Value, risk: ToolRisk) -> Decision;

    /// Run `task` as `agent` in a new child session on this node and wait for
    /// its turn to end. `started` is called with the child's id as soon as it
    /// exists (so the UI can link to it while it works). The child labels its
    /// web results from `refs`, the caller's book.
    async fn delegate(
        &self,
        _agent: crate::agent::AgentSpec,
        _task: String,
        _refs: SharedRefs,
        _cancel: CancellationToken,
        _started: Box<dyn FnOnce(crate::SessionId) + Send>,
    ) -> Result<DelegateOutcome, String> {
        Err("delegation is not available here".into())
    }

    /// Where this session's turns publish files (`publish`); `None` when
    /// publishing is off here (chat sessions, nodes without staging).
    fn artifacts(&self) -> Option<std::sync::Arc<dyn crate::artifacts::ArtifactSink>> {
        None
    }

    /// The bytes of this session's images among `sha256` (those it has; an
    /// image it lacks is named to the model instead of shown).
    async fn images(&self, _sha256: &[String]) -> Vec<ImageData> {
        Vec::new()
    }
}

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
    /// The sub-agent's final answer.
    pub text: String,
    pub reason: TurnEndReason,
    pub usage: Option<Usage>,
    /// The pages the child's web tools returned, with the labels its answer
    /// cites them by.
    pub sources: Vec<Source>,
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
    /// A delegated sub-task's parent book: its labels continue the parent's
    /// numbering, so `[S3]` in the answer it returns names the parent's S3.
    /// `None` means the book comes from this session's own log.
    pub refs: Option<SharedRefs>,
    /// The images sent with this turn's message (their bytes come from
    /// `TurnHost::images`).
    pub images: Vec<crate::event::ImageRef>,
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
}
