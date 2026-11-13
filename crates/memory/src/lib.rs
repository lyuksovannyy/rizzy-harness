//! Long-term memory: short facts about the user, each linked to the chat it
//! came from. Facts the model suggests (`remember` tool) start as
//! *proposed* and are only used after the user confirms them.
//!
//! The built-in store is SQLite (its own `memory.db`) with FTS5 recall and,
//! when an embeddings endpoint is configured, vector recall on top
//! (PLAN.md §0: "services are URLs"; no ML runs in this process). External
//! memory servers plug in behind [`MemoryStore`].

mod embed;
mod mem0;
mod qdrant;
mod sqlite;
mod tool;

use harness_core::SessionId;
use serde::{Deserialize, Serialize};

pub use embed::{ApiStyle, Embedder, Preset};
pub use mem0::Mem0Memory;
pub use qdrant::QdrantIndex;
pub use sqlite::{MIGRATIONS, SqliteMemory};
pub use tool::Remember;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactKind {
    #[default]
    Fact,
    Preference,
    /// Where the user lives or is; used for local questions.
    Location,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Suggested by the model; not used until the user confirms.
    Proposed,
    Active,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fact {
    pub id: String,
    pub owner_id: Option<String>,
    pub text: String,
    pub kind: FactKind,
    pub status: Status,
    /// The chat this came from, if any.
    pub source_session: Option<SessionId>,
    pub created_ms: i64,
    pub updated_ms: i64,
}

#[derive(Debug, Clone)]
pub struct NewFact {
    pub owner_id: Option<String>,
    pub text: String,
    pub kind: FactKind,
    pub status: Status,
    pub source_session: Option<SessionId>,
    /// Idempotency key (e.g. the tool call id), so a replayed event cannot
    /// propose the same fact twice.
    pub dedupe_key: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("{0}")]
    Invalid(String),
    #[error("embeddings: {0}")]
    Embed(String),
    /// The endpoint answered with an error status; `detail` is a short,
    /// sanitised piece of its body (may be empty).
    #[error("embeddings: endpoint answered {status}{}", if .detail.is_empty() { String::new() } else { format!(": {}", .detail) })]
    EmbedHttp { status: u16, detail: String },
}

pub type Result<T> = std::result::Result<T, MemoryError>;

/// Longest fact accepted.
pub const MAX_FACT_CHARS: usize = 500;

#[async_trait::async_trait]
pub trait MemoryStore: Send + Sync {
    fn name(&self) -> &'static str;
    /// Returns `None` when `dedupe_key` was seen before.
    async fn add(&self, f: NewFact) -> Result<Option<Fact>>;
    async fn get(&self, id: &str) -> Result<Option<Fact>>;
    /// Every fact of `owner` (`None` = facts without an owner), newest first.
    async fn list(&self, owner: Option<&str>) -> Result<Vec<Fact>>;
    async fn update(
        &self,
        id: &str,
        text: Option<String>,
        status: Option<Status>,
    ) -> Result<Option<Fact>>;
    async fn delete(&self, id: &str) -> Result<bool>;
    /// Active facts of `owner` most relevant to `query`, best first.
    async fn recall(&self, owner: Option<&str>, query: &str, k: usize) -> Result<Vec<Fact>>;
}

/// Trim and validate a fact's text.
pub fn clean_text(t: &str) -> Result<String> {
    let t = t.trim();
    if t.is_empty() {
        return Err(MemoryError::Invalid("a memory cannot be empty".into()));
    }
    if t.chars().count() > MAX_FACT_CHARS {
        return Err(MemoryError::Invalid(format!(
            "a memory is at most {MAX_FACT_CHARS} characters"
        )));
    }
    // Facts go into later system prompts and the Memory page: drop controls
    // and the hidden set (bidi, zero-width, tag characters), the usual
    // carriers for invisible prompt-injection text.
    let cleaned: String = t
        .chars()
        .filter(|c| (!c.is_control() || *c == '\n') && !harness_core::text::is_hidden(*c))
        .collect();
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() {
        return Err(MemoryError::Invalid("a memory cannot be empty".into()));
    }
    Ok(cleaned)
}

/// The block added to the system prompt for recalled facts.
pub fn context_block(facts: &[Fact]) -> Option<String> {
    if facts.is_empty() {
        return None;
    }
    let mut s = String::from(
        "What you remember about the user (they confirmed these; may be outdated, \
         do not mention unless relevant):",
    );
    for f in facts {
        s.push_str("\n- ");
        s.push_str(&f.text);
    }
    Some(s)
}

#[cfg(test)]
mod clean_tests {
    use super::*;

    #[test]
    fn hidden_characters_are_dropped() {
        let t = clean_text("likes\u{202E} tea\u{200B}\u{E0041}\u{0007}").unwrap();
        assert_eq!(t, "likes tea");
        assert!(
            clean_text("\u{200B}\u{2066}").is_err(),
            "only hidden characters"
        );
        assert_eq!(
            clean_text("line one\nline two").unwrap(),
            "line one\nline two"
        );
    }
}
