//! Master <-> node messages. PLAN.md §3. JSON, `{"type": "..."}`-tagged.
//! Changes must be additive within a major `PROTOCOL_VERSION`.

use harness_core::agent::AgentSpec;
use harness_core::event::Profile;
use harness_core::policy::{ApprovalMode, Decision};
use harness_core::{RawEvent, SessionId};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionHead {
    pub session_id: SessionId,
    pub head_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NodeToMaster {
    #[serde(rename = "node.hello")]
    Hello {
        protocol_version: u32,
        node_id: String,
        hostname: String,
        version: String,
        #[serde(default)]
        capabilities: Vec<String>,
        #[serde(default)]
        sessions: Vec<SessionHead>,
    },
    #[serde(rename = "session.event")]
    Event { event: RawEvent },
    /// A replay batch answering `resume`. `head_seq` is the node's head at the
    /// time the batch was read; more batches follow while `seq < head_seq`.
    #[serde(rename = "session.snapshot")]
    Snapshot {
        session_id: SessionId,
        head_seq: u64,
        events: Vec<RawEvent>,
    },
    #[serde(rename = "ack")]
    Ack {
        req_id: String,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MasterToNode {
    #[serde(rename = "master.welcome")]
    Welcome { protocol_version: u32 },
    #[serde(rename = "session.create")]
    Create {
        req_id: String,
        session_id: SessionId,
        title: String,
        cwd: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// A request only; the node clamps it to its own policy.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        approval_mode: Option<ApprovalMode>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<Profile>,
        /// The session's initial agent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent: Option<AgentSpec>,
    },
    #[serde(rename = "session.prompt")]
    Prompt {
        req_id: String,
        session_id: SessionId,
        text: String,
        /// The agent that runs this turn (the session's agent, or an
        /// @mention override). Resolved by the master; the node does not
        /// store agent definitions.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent: Option<AgentSpec>,
    },
    /// Change the session's active agent (records `agent_changed`).
    #[serde(rename = "session.set_agent")]
    SetAgent {
        req_id: String,
        session_id: SessionId,
        agent: AgentSpec,
    },
    #[serde(rename = "session.cancel")]
    Cancel {
        req_id: String,
        session_id: SessionId,
    },
    #[serde(rename = "approval.respond")]
    ApprovalRespond {
        req_id: String,
        session_id: SessionId,
        approval_id: String,
        decision: Decision,
    },
    #[serde(rename = "resume")]
    Resume {
        session_id: SessionId,
        last_seq: u64,
    },
}

impl MasterToNode {
    pub fn req_id(&self) -> Option<&str> {
        match self {
            MasterToNode::Create { req_id, .. }
            | MasterToNode::Prompt { req_id, .. }
            | MasterToNode::SetAgent { req_id, .. }
            | MasterToNode::Cancel { req_id, .. }
            | MasterToNode::ApprovalRespond { req_id, .. } => Some(req_id),
            MasterToNode::Welcome { .. } | MasterToNode::Resume { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wire_shapes_are_stable() {
        let sid: SessionId = "0193f2a0-0000-7000-8000-000000000001".parse().unwrap();
        let m = MasterToNode::Resume {
            session_id: sid,
            last_seq: 7,
        };
        assert_eq!(
            serde_json::to_value(&m).unwrap(),
            json!({"type": "resume", "session_id": "0193f2a0-0000-7000-8000-000000000001", "last_seq": 7})
        );
        let a = NodeToMaster::Ack {
            req_id: "r".into(),
            ok: true,
            error: None,
        };
        assert_eq!(
            serde_json::to_value(&a).unwrap(),
            json!({"type": "ack", "req_id": "r", "ok": true})
        );
        let d = MasterToNode::ApprovalRespond {
            req_id: "r".into(),
            session_id: sid,
            approval_id: "a".into(),
            decision: Decision::AllowSession,
        };
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["type"], "approval.respond");
        assert_eq!(v["decision"], "allow_session");
        assert_eq!(serde_json::from_value::<MasterToNode>(v).unwrap(), d);
    }
}
