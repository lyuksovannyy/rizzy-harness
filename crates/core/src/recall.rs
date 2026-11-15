//! `recall`: a bounded, read-only search of the person's own earlier chats
//! and memory (ADOPTION_PLAN X.13, PLAN.md T55). The data is on the master
//! and the master never runs tools: the node's tool files a
//! [`RecallQuery`] for the session it runs over the link, the master
//! decides the scope from that session alone (its owner, its workspace,
//! the memory its turn may read) and answers with [`RecallHit`]s. Nothing
//! in a query names a user, a workspace or a chat.

use serde::{Deserialize, Serialize};

/// The longest query the master takes (characters, after cleaning).
pub const MAX_QUERY_CHARS: usize = 300;
/// Hits per search when the model names no `limit`, and the most it may.
pub const DEFAULT_LIMIT: u32 = 5;
pub const MAX_LIMIT: u32 = 10;
/// One hit's excerpt, at most (characters).
pub const EXCERPT_CHARS: usize = 700;
/// Every excerpt of one answer together, at most (characters).
pub const TOTAL_CHARS: usize = 6000;
/// A title as a hit carries it (characters).
pub const TITLE_CHARS: usize = 120;

/// What to search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallIn {
    Chats,
    Memory,
    #[default]
    Both,
}

impl RecallIn {
    pub fn chats(self) -> bool {
        matches!(self, RecallIn::Chats | RecallIn::Both)
    }
    pub fn memory(self) -> bool {
        matches!(self, RecallIn::Memory | RecallIn::Both)
    }
}

/// One search, as the model asked for it. Only words: the scope is the
/// master's to decide.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecallQuery {
    pub query: String,
    #[serde(rename = "in", default)]
    pub scope: RecallIn,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HitKind {
    Chat,
    Memory,
}

/// One result. `id` is a chat's session id or a fact's id: something the
/// agent can cite, which no tool reads more by.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallHit {
    pub kind: HitKind,
    pub id: String,
    /// A chat's title; empty for a fact.
    #[serde(default)]
    pub title: String,
    /// When the chat was last active, or the fact last changed (ms).
    pub when_ms: i64,
    pub excerpt: String,
}

/// Where a session's `recall` calls go: the node's link to the master,
/// bound to the session the call runs for.
#[async_trait::async_trait]
pub trait RecallSource: Send + Sync {
    async fn recall(&self, q: RecallQuery) -> Result<Vec<RecallHit>, String>;
}
