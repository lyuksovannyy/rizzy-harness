//! **Custom connectors' tools for agents** (ADOPTION_PLAN X.35 step 2,
//! PLAN.md T85): a workspace's remote MCP servers, which the master speaks
//! to with the chat owner's own account, offered to a node's turn as MCP
//! tools (`mcp__<connector>__<tool>`). The master sends the list with a
//! turn (`AgentSpec::connector_tools`, the top spec only); a call becomes
//! one typed request over the link ([`ConnectorSource`]) that names the
//! session, the tool and its arguments, nothing else. Nothing here names a
//! program, a file or an address: the node starts and reads nothing for
//! them, and a token never leaves the master.

use serde::{Deserialize, Serialize};

/// Entries one turn's list may hold (the node's per-chat cap of MCP tools,
/// `harness_tools::mcp::MAX_TOOLS`): more are not taken.
pub const MAX_LISTED: usize = 40;
/// Bytes of one entry's input schema (the node's `MAX_SCHEMA_BYTES`).
pub const MAX_SCHEMA_BYTES: usize = 8_000;
/// Characters of one entry's description (the node's MCP cap).
pub const MAX_DESCRIPTION_CHARS: usize = 4_096;
/// Bytes of a call's arguments, as JSON text: the node refuses a larger
/// call before it asks, and the master again.
pub const MAX_ARGUMENT_BYTES: usize = 64 * 1024;

/// One connector tool a turn may be offered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorToolEntry {
    /// The connector's name in the workspace (an MCP server name).
    pub connector: String,
    /// The public name, `mcp__<connector>__<tool>` (cleaned, maybe
    /// hashed: `harness_tools::mcp::public_name`).
    pub name: String,
    /// The remote server's own name of the tool, cleaned (the approval
    /// card shows it).
    pub tool: String,
    /// What the model reads: `(MCP server \`<connector>\`) ` and the
    /// server's description, cleaned and capped.
    pub description: String,
    /// The input schema as compact JSON text, a JSON object schema of at
    /// most [`MAX_SCHEMA_BYTES`].
    pub schema: String,
}

impl ConnectorToolEntry {
    /// Whether this entry has the shape the master sends: a public name of
    /// `connector`, a schema that is a JSON object of the size allowed, a
    /// description within its cap. The node takes no other.
    pub fn well_formed(&self) -> bool {
        crate::agent::valid_mcp_server_name(&self.connector)
            && crate::agent::mcp_server_of(&self.name) == Some(self.connector.as_str())
            && self.name.len() <= 64
            && self
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            && !self.tool.is_empty()
            && self.tool.chars().count() <= 128
            && self.description.chars().count() <= MAX_DESCRIPTION_CHARS + 64
            && self.schema.len() <= MAX_SCHEMA_BYTES
            && serde_json::from_str::<serde_json::Value>(&self.schema)
                .ok()
                .is_some_and(|v| v.get("type").and_then(|t| t.as_str()) == Some("object"))
    }
}

/// What a connector call came back with: text only (other content named,
/// as for node MCP tools), and whether the server marked it an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorAnswer {
    pub text: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_error: bool,
}

/// Where a session's connector calls go: the node's link to the master,
/// bound to the session the call runs for. The master decides from the
/// session alone whose account, which connector and whether this turn's
/// agent may call it.
#[async_trait::async_trait]
pub trait ConnectorSource: Send + Sync {
    async fn call(
        &self,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<ConnectorAnswer, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> ConnectorToolEntry {
        ConnectorToolEntry {
            connector: "linear".into(),
            name: "mcp__linear__create_issue".into(),
            tool: "create_issue".into(),
            description: "(MCP server `linear`) Makes an issue".into(),
            schema: r#"{"type":"object"}"#.into(),
        }
    }

    #[test]
    fn only_well_formed_entries_are_taken() {
        assert!(entry().well_formed());
        let bad = [
            ConnectorToolEntry {
                name: "mcp__other__create_issue".into(),
                ..entry()
            },
            ConnectorToolEntry {
                name: "bash".into(),
                ..entry()
            },
            ConnectorToolEntry {
                name: "mcp__linear__a b".into(),
                ..entry()
            },
            ConnectorToolEntry {
                connector: "Linear".into(),
                name: "mcp__Linear__x".into(),
                ..entry()
            },
            ConnectorToolEntry {
                schema: r#"{"type":"string"}"#.into(),
                ..entry()
            },
            ConnectorToolEntry {
                schema: "not json".into(),
                ..entry()
            },
            ConnectorToolEntry {
                schema: format!(
                    r#"{{"type":"object","d":"{}"}}"#,
                    "x".repeat(MAX_SCHEMA_BYTES)
                ),
                ..entry()
            },
            ConnectorToolEntry {
                tool: String::new(),
                ..entry()
            },
            ConnectorToolEntry {
                description: "d".repeat(MAX_DESCRIPTION_CHARS + 100),
                ..entry()
            },
        ];
        for b in bad {
            assert!(!b.well_formed(), "{b:?}");
        }
    }
}
