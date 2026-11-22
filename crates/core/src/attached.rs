//! `attached_file`: a bounded, read-only look into a file the person
//! attached to a chat that has no folder (PLAN.md T95, docs/attached-files.md).
//! The bytes are the owner's blob on the master and the master never runs
//! tools: the node's tool files an [`AttachedRequest`] for the session it
//! runs over the link, and the master answers from a built-in service that
//! takes everything but the file's id from the session alone and serves only
//! a file whose id is in that session's own log. Nothing in a request names
//! a user, a workspace or a chat; the id the model gives is looked up in the
//! one chat the call runs in.

use serde::{Deserialize, Serialize};

/// The tool's name, the agent switch's tool, the capability's and the
/// harness line's word.
pub const TOOL: &str = "attached_file";

/// The longest file id a request may carry.
pub const MAX_ID_CHARS: usize = 64;
/// The longest entry path a request may carry (characters, after cleaning).
pub const MAX_ENTRY_CHARS: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachedOp {
    /// Name, size, the type the first bytes say, sha256.
    Info,
    /// The entries of a zip, tar, tar.gz or gz, a page at a time.
    List,
    /// The file, or one entry of an archive.
    Read,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadMode {
    /// Text as numbered lines, anything else as a hex dump.
    #[default]
    Auto,
    Hex,
    /// The printable runs of the data.
    Strings,
}

/// One call, as the model asked for it. Only words: the scope is the
/// master's to decide.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachedRequest {
    pub op: AttachedOp,
    pub file_id: String,
    /// `read`: the listed path of an archive's entry; none reads the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    /// `list`: the first entry (0-based). `read`: the first line (text,
    /// 1-based) or byte (hex and strings, 0-based).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    /// `list`: entries. `read`: lines (text) or bytes (hex, strings).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<ReadMode>,
}

/// One entry of a listing. The path is cleaned text, never a path of any
/// disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryView {
    pub index: u64,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// `file`, `folder`, `link` or `other`.
    pub kind: String,
    /// A link's target or what an `other` entry is.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

/// What a call gets back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttachedAnswer {
    Info {
        name: String,
        bytes: u64,
        sha256: String,
        /// What the first bytes say it is ("ZIP archive", "text (UTF-8)").
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mime: Option<String>,
        /// A zip, tar, tar.gz or gz: `list` and `read` with `entry` work.
        archive: bool,
    },
    List {
        archive: String,
        entries: Vec<EntryView>,
        offset: u64,
        /// Entries read in this call.
        scanned: u64,
        /// The end of the archive was reached: `scanned` is its count.
        complete: bool,
        /// There are entries after this page.
        more: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    Read {
        /// `text`, `hex` or `strings`.
        mode: String,
        /// The lines, dump or runs (raw: the tool quotes them).
        text: String,
        from: u64,
        shown: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
}

/// Where a session's `attached_file` calls go: the node's link to the
/// master, bound to the session the call runs for.
#[async_trait::async_trait]
pub trait AttachedSource: Send + Sync {
    async fn attached(&self, r: AttachedRequest) -> Result<AttachedAnswer, String>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A request carries what to do and the file's id and nothing that
    /// names a user, a workspace or a chat: such a field does not parse, so
    /// a node cannot widen the scope the master takes from the session.
    #[test]
    fn a_request_carries_words_only() {
        let r = AttachedRequest {
            op: AttachedOp::Read,
            file_id: "f1".into(),
            entry: Some("a/b.txt".into()),
            offset: Some(3),
            limit: Some(10),
            mode: Some(ReadMode::Hex),
        };
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"op": "read", "file_id": "f1", "entry": "a/b.txt", "offset": 3,
                   "limit": 10, "mode": "hex"})
        );
        let bare: AttachedRequest =
            serde_json::from_value(json!({"op": "info", "file_id": "f1"})).unwrap();
        assert_eq!(bare.entry, None);
        for extra in ["user_id", "owner", "session_id", "workspace_id", "path"] {
            let v = json!({"op": "info", "file_id": "f1", extra: "x"});
            assert!(serde_json::from_value::<AttachedRequest>(v).is_err(), "{extra}");
        }
        assert!(serde_json::from_value::<AttachedRequest>(json!({"op": "write", "file_id": "f"})).is_err());
    }
}
