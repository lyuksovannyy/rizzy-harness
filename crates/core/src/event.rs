//! Event schema. See PLAN.md §2. Changes here must be additive.

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
    #[serde(default)]
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRequested {
    pub approval_id: String,
    pub call_id: String,
    pub tool: String,
    pub input: Value,
    pub reason: String,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub turn_id: String,
    pub phase: CheckpointPhase,
    pub git_ref: String,
    /// `after`: files the turn changed; `restored`: files put back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
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
    #[serde(default)]
    pub context_tokens: u64,
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
                }),
            }),
            EventKind::AgentChanged(AgentChanged { agent }),
            EventKind::VariantSelected(VariantSelected {
                group: "g".into(),
                message_id: "m".into(),
            }),
        ];
        for k in kinds {
            let (kind, payload) = k.clone().into_parts();
            assert_eq!(kind, k.name());
            assert_eq!(EventKind::from_parts(&kind, payload).unwrap(), k);
        }
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
        // Extra unknown fields from a newer node must not break an older reader.
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
