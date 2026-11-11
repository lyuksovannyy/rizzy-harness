//! Event schema. See PLAN.md §2.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

use crate::policy::Decision;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub Uuid);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for SessionId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s).map(SessionId)
    }
}

/// Storage and wire form of an event. `kind` and `payload` are opaque at this
/// layer so that a mirror never fails on kinds it does not know.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawEvent {
    pub session_id: SessionId,
    pub seq: u64,
    pub ts_ms: i64,
    pub kind: String,
    pub payload: Value,
}

impl RawEvent {
    pub fn typed(&self) -> Result<EventKind, serde_json::Error> {
        EventKind::from_parts(&self.kind, self.payload.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum EventKind {
    SessionCreated(SessionCreated),
    TurnStarted(TurnStarted),
    UserMessage(UserMessage),
    AssistantDelta(AssistantDelta),
    AssistantMessage(AssistantMessage),
    ToolCallStarted(ToolCallStarted),
    ToolOutputDelta(ToolOutputDelta),
    ToolCallFinished(ToolCallFinished),
    ApprovalRequested(ApprovalRequested),
    ApprovalResolved(ApprovalResolved),
    Checkpoint(Checkpoint),
    TurnEnded(TurnEnded),
    Error(ErrorEvent),
    AgentChanged(AgentChanged),
    VariantSelected(VariantSelected),
    HistorySummarized(HistorySummarized),
    ModelFallback(ModelFallback),
    // Boxed: its metadata makes it many times larger than the rest.
    ArtifactStaged(Box<ArtifactStaged>),
    ArtifactStageFailed(ArtifactStageFailed),
}

impl EventKind {
    /// Split into `(kind, payload)` for storage.
    pub fn into_parts(self) -> (String, Value) {
        let v = serde_json::to_value(self).expect("EventKind always serializes");
        let Value::Object(mut map) = v else {
            unreachable!("adjacently tagged enum serializes to an object")
        };
        let kind = match map.remove("kind") {
            Some(Value::String(s)) => s,
            _ => unreachable!("tag is a string"),
        };
        let payload = map.remove("payload").unwrap_or(Value::Null);
        (kind, payload)
    }

    pub fn from_parts(kind: &str, payload: Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(serde_json::json!({ "kind": kind, "payload": payload }))
    }

    pub fn name(&self) -> &'static str {
        match self {
            EventKind::SessionCreated(_) => "session_created",
            EventKind::TurnStarted(_) => "turn_started",
            EventKind::UserMessage(_) => "user_message",
            EventKind::AssistantDelta(_) => "assistant_delta",
            EventKind::AssistantMessage(_) => "assistant_message",
            EventKind::ToolCallStarted(_) => "tool_call_started",
            EventKind::ToolOutputDelta(_) => "tool_output_delta",
            EventKind::ToolCallFinished(_) => "tool_call_finished",
            EventKind::ApprovalRequested(_) => "approval_requested",
            EventKind::ApprovalResolved(_) => "approval_resolved",
            EventKind::Checkpoint(_) => "checkpoint",
            EventKind::TurnEnded(_) => "turn_ended",
            EventKind::Error(_) => "error",
            EventKind::AgentChanged(_) => "agent_changed",
            EventKind::VariantSelected(_) => "variant_selected",
            EventKind::HistorySummarized(_) => "history_summarized",
            EventKind::ModelFallback(_) => "model_fallback",
            EventKind::ArtifactStaged(_) => "artifact_staged",
            EventKind::ArtifactStageFailed(_) => "artifact_stage_failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionCreated {
    pub title: String,
    pub cwd: String,
    pub model: String,
    pub backend: String,
    pub node_id: String,
    pub approval_mode: crate::policy::ApprovalMode,
    pub profile: Profile,
    /// The agent active when the session started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Incognito sessions are excluded from search, suggestions and memory.
    #[serde(default, skip_serializing_if = "is_false")]
    pub incognito: bool,
    /// Set on a sub-agent's session: the session whose turn delegated to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session: Option<crate::SessionId>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Where a session runs and which toolset family it gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    /// On the master: documents, web, research. No filesystem or shell.
    Chat,
    /// On a node, in a workspace.
    Agentic,
}

/// Which agent ran something, frozen at that time so old turns stay
/// explainable after the agent definition is edited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRef {
    pub agent_id: String,
    pub name: String,
    /// Hash of the system prompt + tool list the agent had for this turn.
    pub prompt_hash: String,
}

/// Groups parallel answers to the same prompt (e.g. several models).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variant {
    pub group: String,
    pub index: u32,
    /// The model that wrote this answer (parallel answers compare models).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnStarted {
    pub turn_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserMessage {
    pub turn_id: String,
    pub text: String,
    /// Documents attached to this message, with their extracted text (the
    /// log alone must be enough to rebuild what the model saw).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
    /// Set when the message answers a `ui:choices` or `ui:form` block; `text`
    /// is then the harness's canonical answer (ids, formatted values and the
    /// user's own typing, never model text), plus the user's note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<FormReply>,
    /// Images sent with the message. Only references: the bytes stay in the
    /// owner's Media on the master and in the node's `session_images`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ImageRef>,
}

/// An image a user message carries (`user_message.images`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageRef {
    /// The Media item it was sent from (it may be deleted since).
    pub media_id: String,
    /// Hex sha256 of the bytes; the node keys its copy by it.
    pub sha256: String,
    pub mime: String,
    pub name: String,
    /// From the file's header; 0 when it could not be read.
    pub width: u32,
    pub height: u32,
}

/// The user's answer to an input block of an earlier assistant message.
/// Choices: `{"selected": [option ids], "other"?: text}`. Form: field id to
/// a string, number, boolean or list of option ids; unanswered optional
/// fields are left out. The node checks it against its own log
/// (`ui::check_reply`) before it is recorded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormReply {
    pub message_id: String,
    pub block_id: String,
    pub values: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    pub name: String,
    pub mime: String,
    pub text: String,
    /// The extracted text was cut to the size limit.
    #[serde(default)]
    pub truncated: bool,
}

impl UserMessage {
    /// What the model sees: the message, then each attachment in a tagged block.
    pub fn model_text(&self) -> String {
        let mut out = self.text.clone();
        for a in &self.attachments {
            out.push_str(&format!(
                "\n\n<attachment name=\"{}\" type=\"{}\"{}>\n{}\n</attachment>",
                a.name.replace('"', "'"),
                a.mime,
                if a.truncated {
                    " truncated=\"true\""
                } else {
                    ""
                },
                a.text
            ));
        }
        out
    }
}

/// Older turns were replaced by a summary for the model (the log keeps them;
/// only what is sent to the provider shrinks).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistorySummarized {
    pub turn_id: String,
    /// Every event with `seq <= through_seq` is covered by `text`.
    pub through_seq: u64,
    pub text: String,
}

/// The turn's model was unavailable or failed before any output, and one
/// of the agent's fallbacks answers instead. Emitted before its answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelFallback {
    pub turn_id: String,
    /// The model that answers (prefixed).
    pub model: String,
    /// The turn's own model.
    pub from: String,
    /// Why `from` was not used: "unavailable", or the provider's error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantDelta {
    pub turn_id: String,
    pub message_id: String,
    pub text: String,
    /// Agent id that produced this text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<Variant>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallRef {
    pub call_id: String,
    pub tool: String,
    pub input: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantMessage {
    pub turn_id: String,
    pub message_id: String,
    pub text: String,
    #[serde(default)]
    pub tool_calls: Vec<ToolCallRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<Variant>,
    /// The message's `ui:` blocks, validated and normalized from `text` by
    /// `ui::extract`. The UI renders these specs, never the model's JSON.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<UiBlock>,
}

/// One `ui:<type>` fence of an assistant message. `ordinal` (its index
/// among the message's ui fences) and `hash` (of its body) tie it to the
/// fence the browser finds in the text; a fence whose ordinal, hash and type
/// do not all match renders as plain code. Exactly one of `spec` and `error`
/// is set.
///
/// Unknown keys are tolerated here like everywhere in the event schema; the
/// model's own JSON is what `ui` rejects unknown keys in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiBlock {
    pub ordinal: u32,
    /// FNV-1a 32-bit of the body's UTF-8 bytes, as 8 lowercase hex digits.
    pub hash: String,
    #[serde(rename = "type")]
    pub kind: UiKind,
    /// The block's id; `b<ordinal>` when it has no valid one.
    pub id: String,
    /// The normalized spec (`ui::ChoicesSpec`, `FormSpec`, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<Value>,
    /// Why the block is not shown: `<json path>: <reason>`, harness text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiKind {
    Choices,
    Form,
    Progress,
    Chart,
}

impl UiKind {
    pub const ALL: [UiKind; 4] = [
        UiKind::Choices,
        UiKind::Form,
        UiKind::Progress,
        UiKind::Chart,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            UiKind::Choices => "choices",
            UiKind::Form => "form",
            UiKind::Progress => "progress",
            UiKind::Chart => "chart",
        }
    }

    /// Blocks the user answers (they end the turn); the others only show.
    pub fn is_input(self) -> bool {
        matches!(self, UiKind::Choices | UiKind::Form)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallStarted {
    pub turn_id: String,
    pub call_id: String,
    pub tool: String,
    pub input: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolOutputDelta {
    pub call_id: String,
    pub stream: OutputStream,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallFinished {
    pub call_id: String,
    pub ok: bool,
    /// What the model sees as the tool result.
    pub output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub truncated: bool,
    /// Pages a web tool read or found, under the labels its output gave them
    /// (`[S3]`), so the UI can resolve the model's citations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<Source>,
}

/// A web page behind a citation label. Only the harness's web tools write
/// these, so a label resolves to a URL the model did not choose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// The label without brackets, e.g. `S4`; stable for the whole session.
    pub id: String,
    pub url: String,
    /// Plain text (see `text::clean_line`), at most `Source::MAX_TITLE` chars.
    pub title: String,
}

impl Source {
    pub const MAX_TITLE: usize = 200;

    /// The label's number (`S4` -> 4), if it is one.
    pub fn number(&self) -> Option<u32> {
        let digits = self.id.strip_prefix('S')?;
        // `parse` would also take "+4".
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        digits.parse().ok()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRequested {
    pub approval_id: String,
    pub call_id: String,
    pub tool: String,
    pub input: Value,
    pub reason: String,
    /// "Allow for this turn" (`allow_session`) may be offered. The node sets
    /// it on every request a person can answer; it would cover only this
    /// exact call (tool and canonical argument) for the rest of the turn. A
    /// policy denial carries it false and is resolved by the node at once.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub turn_allow: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedBy {
    User,
    Policy,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalResolved {
    pub approval_id: String,
    pub decision: Decision,
    pub by: ResolvedBy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointPhase {
    Before,
    After,
    /// The turn's file changes were undone (back to its `before` snapshot).
    Restored,
    /// The folder as it was just before a restore, written before the
    /// restore changes any file. `turn_id` names the restore, not a turn:
    /// its `after` event (the folder once restored) follows, so restoring
    /// that id undoes the restore. `files`: the paths the restore writes.
    BeforeRestore,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub turn_id: String,
    pub phase: CheckpointPhase,
    pub git_ref: String,
    /// `after`: files the turn changed; `restored`: files put back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// Git repositories inside the folder (a sub-folder holding a `.git`
    /// directory or file) that this snapshot left out and that undo never
    /// touches. On `restored`: the ones the restore left alone. Relative
    /// to the checkpoint's folder.
    /// At most 50 names, control characters replaced; `excluded_more`
    /// counts the rest.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded: Vec<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub excluded_more: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnEndReason {
    Completed,
    Cancelled,
    Error,
    Interrupted,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Summed over every model call in the turn.
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Input tokens of the turn's last model call: the context size.
    pub context_tokens: u64,
    /// Of `input_tokens`, how many were read from the provider's prompt
    /// cache (billed at a fraction) and how many were written to it.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cache_read_tokens: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cache_write_tokens: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

impl Usage {
    /// Add another call's usage (context size is the latest call's).
    pub fn add(&mut self, u: &Usage) {
        self.input_tokens += u.input_tokens;
        self.output_tokens += u.output_tokens;
        self.cache_read_tokens += u.cache_read_tokens;
        self.cache_write_tokens += u.cache_write_tokens;
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnEnded {
    pub turn_id: String,
    pub reason: TurnEndReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// The session's active agent changed (selector, not an @mention: a mention
/// only affects one turn and is recorded in that turn's `TurnStarted`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentChanged {
    pub agent: AgentRef,
}

/// The user picked one of several parallel answers to continue from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VariantSelected {
    pub group: String,
    pub message_id: String,
}

/// A file was copied into the node's stage for the master to pull
/// (docs/artifacts.md): by the `publish` tool (`call_id` names the call) or
/// by "Save as artifact". The master validates it before anything moves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactStaged {
    /// A uuid v7, the key of the node's stage index.
    pub stage_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    pub by: crate::artifacts::StagedBy,
    pub name: String,
    /// The file name it is offered under (`<folder>.tar.gz` for a pack).
    pub filename: String,
    /// `null` | `"tar.gz"`.
    #[serde(default)]
    pub packed: Option<String>,
    pub size: u64,
    /// Lowercase hex SHA-256 of the staged bytes.
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind_hint: Option<String>,
    /// Where it came from, as the node resolved it.
    pub source_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default)]
    pub meta: crate::artifacts::ArtifactMeta,
}

/// "Save as artifact" failed on the node after it accepted the path (a
/// failing `publish` call returns a tool error instead).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactStageFailed {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parts_roundtrip() {
        let k = EventKind::ToolCallStarted(ToolCallStarted {
            author: None,
            turn_id: "t".into(),
            call_id: "c".into(),
            tool: "bash".into(),
            input: json!({"command": "ls"}),
        });
        let (kind, payload) = k.clone().into_parts();
        assert_eq!(kind, "tool_call_started");
        assert_eq!(kind, k.name());
        assert_eq!(payload["tool"], "bash");
        assert_eq!(EventKind::from_parts(&kind, payload).unwrap(), k);
    }

    /// Optional fields are omitted when empty, so payloads stay minimal and
    /// round-trip exactly.
    #[test]
    fn optional_fields_are_omitted_and_roundtrip() {
        let minimal = [
            (
                "session_created",
                json!({"title": "t", "cwd": "/w", "model": "m", "backend": "native", "node_id": "local", "approval_mode": "ask", "profile": "agentic"}),
            ),
            ("turn_started", json!({"turn_id": "t1"})),
            ("user_message", json!({"turn_id": "t1", "text": "hi"})),
            (
                "assistant_delta",
                json!({"turn_id": "t1", "message_id": "m1", "text": "hi"}),
            ),
            (
                "assistant_message",
                json!({"turn_id": "t1", "message_id": "m1", "text": "hi", "tool_calls": []}),
            ),
            (
                "tool_call_started",
                json!({"turn_id": "t1", "call_id": "c", "tool": "bash", "input": {}}),
            ),
            (
                "tool_call_finished",
                json!({"call_id": "c", "ok": true, "output": "x", "truncated": false}),
            ),
        ];
        for (kind, payload) in minimal {
            let k = EventKind::from_parts(kind, payload.clone()).unwrap();
            let (k2, p2) = k.into_parts();
            assert_eq!(k2, kind);
            assert_eq!(p2, payload, "{kind} changed shape");
        }
    }

    #[test]
    fn agent_and_variant_fields_roundtrip() {
        let agent = AgentRef {
            agent_id: "coder".into(),
            name: "Coder".into(),
            prompt_hash: "abc".into(),
        };
        let kinds = vec![
            EventKind::SessionCreated(SessionCreated {
                title: "t".into(),
                cwd: "".into(),
                model: "m".into(),
                backend: "native".into(),
                node_id: "master".into(),
                approval_mode: Default::default(),
                profile: Profile::Chat,
                agent_id: Some("general".into()),
                incognito: true,
                parent_session: None,
            }),
            EventKind::TurnStarted(TurnStarted {
                turn_id: "t".into(),
                agent: Some(agent.clone()),
            }),
            EventKind::AssistantMessage(AssistantMessage {
                turn_id: "t".into(),
                message_id: "m".into(),
                text: "x".into(),
                tool_calls: vec![],
                author: Some("coder".into()),
                variant: Some(Variant {
                    group: "g".into(),
                    index: 1,
                    model: None,
                }),
                blocks: vec![],
            }),
            EventKind::AgentChanged(AgentChanged { agent }),
            EventKind::VariantSelected(VariantSelected {
                group: "g".into(),
                message_id: "m".into(),
            }),
            EventKind::ModelFallback(ModelFallback {
                turn_id: "t".into(),
                model: "b::m2".into(),
                from: "a::m1".into(),
                reason: Some("unavailable".into()),
            }),
        ];
        for k in kinds {
            let (kind, payload) = k.clone().into_parts();
            assert_eq!(kind, k.name());
            assert_eq!(EventKind::from_parts(&kind, payload).unwrap(), k);
        }
    }

    #[test]
    fn sources_roundtrip() {
        let k = EventKind::ToolCallFinished(ToolCallFinished {
            call_id: "c".into(),
            ok: true,
            output: "[S4] Tokio\n    https://tokio.rs/".into(),
            exit_code: None,
            truncated: false,
            sources: vec![Source {
                id: "S4".into(),
                url: "https://tokio.rs/".into(),
                title: "Tokio".into(),
            }],
        });
        let (kind, payload) = k.clone().into_parts();
        assert_eq!(
            payload["sources"],
            json!([{"id": "S4", "url": "https://tokio.rs/", "title": "Tokio"}])
        );
        assert_eq!(EventKind::from_parts(&kind, payload).unwrap(), k);
        let s = Source {
            id: "S12".into(),
            url: String::new(),
            title: String::new(),
        };
        assert_eq!(s.number(), Some(12));
        for bad in ["x3", "S", "S+3", "S-1"] {
            let s = Source {
                id: bad.into(),
                ..s.clone()
            };
            assert_eq!(s.number(), None, "{bad}");
        }
    }

    #[test]
    fn blocks_roundtrip_and_are_omitted_when_empty() {
        let msg = |blocks| {
            EventKind::AssistantMessage(AssistantMessage {
                turn_id: "t".into(),
                message_id: "m".into(),
                text: "x".into(),
                tool_calls: vec![],
                author: None,
                variant: None,
                blocks,
            })
        };
        let (_, payload) = msg(vec![]).into_parts();
        assert!(payload.get("blocks").is_none());
        let k = msg(vec![
            UiBlock {
                ordinal: 0,
                hash: "1c9a3f0e".into(),
                kind: UiKind::Choices,
                id: "db".into(),
                spec: Some(json!({"question": "Which?"})),
                error: None,
            },
            UiBlock {
                ordinal: 1,
                hash: "7b02d4aa".into(),
                kind: UiKind::Form,
                id: "b1".into(),
                spec: None,
                error: Some("body: larger than 16 KiB".into()),
            },
        ]);
        let (kind, payload) = k.clone().into_parts();
        assert_eq!(
            payload["blocks"],
            json!([
                {"ordinal": 0, "hash": "1c9a3f0e", "type": "choices", "id": "db", "spec": {"question": "Which?"}},
                {"ordinal": 1, "hash": "7b02d4aa", "type": "form", "id": "b1", "error": "body: larger than 16 KiB"},
            ])
        );
        assert_eq!(EventKind::from_parts(&kind, payload).unwrap(), k);
        assert!(UiKind::Form.is_input() && !UiKind::Chart.is_input());
    }

    #[test]
    fn reply_roundtrips() {
        let mut values = serde_json::Map::new();
        values.insert("selected".into(), json!(["pg"]));
        let k = EventKind::UserMessage(UserMessage {
            turn_id: "t".into(),
            text: "Choice answer · db: pg".into(),
            attachments: vec![],
            reply: Some(FormReply {
                message_id: "m1".into(),
                block_id: "db".into(),
                values,
            }),
            images: vec![],
        });
        let (kind, payload) = k.clone().into_parts();
        assert_eq!(
            payload["reply"],
            json!({"message_id": "m1", "block_id": "db", "values": {"selected": ["pg"]}})
        );
        assert_eq!(EventKind::from_parts(&kind, payload).unwrap(), k);
    }

    #[test]
    fn unknown_kind_is_error_only_at_typed_layer() {
        let raw = RawEvent {
            session_id: SessionId::new(),
            seq: 1,
            ts_ms: 0,
            kind: "from_the_future".into(),
            payload: json!({"x": 1}),
        };
        let s = serde_json::to_string(&raw).unwrap();
        let back: RawEvent = serde_json::from_str(&s).unwrap();
        assert_eq!(back, raw);
        assert!(back.typed().is_err());
    }

    #[test]
    fn additive_fields_tolerated() {
        // Unknown fields are ignored, not rejected.
        let k = EventKind::from_parts(
            "turn_started",
            json!({"turn_id": "t1", "added_later": true}),
        )
        .unwrap();
        assert_eq!(
            k,
            EventKind::TurnStarted(TurnStarted {
                agent: None,
                turn_id: "t1".into()
            })
        );
    }
}
