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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnStarted {
    pub turn_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserMessage {
    pub turn_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantDelta {
    pub turn_id: String,
    pub message_id: String,
    pub text: String,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallStarted {
    pub turn_id: String,
    pub call_id: String,
    pub tool: String,
    pub input: Value,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub turn_id: String,
    pub phase: CheckpointPhase,
    pub git_ref: String,
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
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnEnded {
    pub turn_id: String,
    pub reason: TurnEndReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
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
                turn_id: "t1".into()
            })
        );
    }
}
