//! `doc_create` and `sheet_create`: Word and Excel files the agent makes
//! from its own text (PLAN.md T97, ADOPTION_PLAN X.56). Where a chat has a
//! folder the tool runs on the node and writes the file there as `write`
//! does. In a chat without one nothing is written to any disk path: the
//! node's tool files a [`DocRequest`] for the session it runs over the link
//! and the master's built-in service builds the file, keeps it as the owner's
//! blob (counted in their quota) and answers with a [`DocCreated`]; the node
//! then names the file in the call's `tool_call_finished.made`, and only that
//! line in the chat's own log makes the file the chat's (`session_files`,
//! `logged`). A request names no user, workspace or chat: the scope is the
//! session's alone.

use serde::{Deserialize, Serialize};

/// The tools' names, the agent switch's tools, the capability's and the
/// harness's word.
pub const DOC_TOOL: &str = "doc_create";
pub const SHEET_TOOL: &str = "sheet_create";

/// The one tool name each kind has, in the order a switch lists them.
pub const TOOLS: [&str; 2] = [DOC_TOOL, SHEET_TOOL];

/// The longest request a node may send (the link's frame is 8 MiB).
pub const MAX_REQUEST_BYTES: usize = 6 << 20;

/// The `.docx` and `.xlsx` types the master stores them under.
pub const DOCX_MIME: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
pub const XLSX_MIME: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

/// One call, as the model asked for it. Only words and the document's own
/// content: the scope is the master's to decide from the session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DocRequest {
    /// Markdown to a `.docx`.
    Doc {
        name: String,
        markdown: String,
        /// `a4` (the default) or `letter`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        page: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        landscape: bool,
        /// A property of the file, not a heading in it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    /// A sheet spec to an `.xlsx`.
    Sheet {
        name: String,
        spec: serde_json::Value,
    },
}

impl DocRequest {
    /// The tool that makes this kind of file.
    pub fn tool(&self) -> &'static str {
        match self {
            Self::Doc { .. } => DOC_TOOL,
            Self::Sheet { .. } => SHEET_TOOL,
        }
    }
}

/// What the master made and kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocCreated {
    /// The id the chat's own log will name the file by (its `made`).
    pub file_id: String,
    /// The name it has: cleaned, its extension the tool's.
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
    pub mime: String,
    /// Sentences of the harness's own about what the file holds and what
    /// was left out, built from counts (never from the model's text).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// Where a plain chat's `doc_create` and `sheet_create` calls go: the node's
/// link to the master, bound to the session the call runs for.
#[async_trait::async_trait]
pub trait DocSource: Send + Sync {
    async fn create(&self, r: DocRequest) -> Result<DocCreated, String>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A request carries the file's content and nothing that names a user,
    /// a workspace, a chat or a place: such a field does not parse.
    #[test]
    fn a_request_carries_content_only() {
        let r = DocRequest::Doc {
            name: "a".into(),
            markdown: "# x".into(),
            page: None,
            landscape: false,
            title: None,
        };
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"kind": "doc", "name": "a", "markdown": "# x"})
        );
        for extra in [
            "user_id",
            "owner",
            "session_id",
            "workspace_id",
            "path",
            "file_id",
        ] {
            for base in [
                json!({"kind": "doc", "name": "a", "markdown": "x"}),
                json!({"kind": "sheet", "name": "a", "spec": {}}),
            ] {
                let mut v = base;
                v[extra] = json!("x");
                assert!(serde_json::from_value::<DocRequest>(v).is_err(), "{extra}");
            }
        }
        assert!(
            serde_json::from_value::<DocRequest>(json!({"kind": "slides", "name": "a"})).is_err()
        );
    }
}
