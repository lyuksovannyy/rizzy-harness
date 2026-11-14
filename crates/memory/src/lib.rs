//! Long-term memory: short facts about the user, each linked to the chat it
//! came from. Where a fact lives (the person's own facts, one workspace's,
//! a project's) is its owner key, which the master chooses; whether the
//! model's `remember` saves at once or waits as *proposed* is the master's
//! call too (README, Memory). A fact the model replaced or forgot is
//! *retired*: kept, never recalled, restorable.
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
pub use sqlite::{DB_SCHEMA, MIGRATIONS, SqliteMemory};
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
    /// Replaced or forgotten by the model: never recalled, shown to the
    /// person, who can restore or delete it.
    Retired,
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
    /// The schema version: a newer build's database, or a failed migration.
    #[error(transparent)]
    Migrate(#[from] harness_store::migrate::MigrateError),
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
    /// The fact of `owner` added with idempotency key `key`, if any.
    async fn by_key(&self, owner: Option<&str>, key: &str) -> Result<Option<Fact>>;
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
    if looks_secret(&cleaned) {
        return Err(MemoryError::Invalid(SECRET_REFUSED.into()));
    }
    Ok(cleaned)
}

/// Why a fact that looks like a secret is refused.
pub const SECRET_REFUSED: &str = "this looks like a password, key or token: memory never keeps \
     secrets (remember where it is kept, not the value)";

/// Prefixes of well-known API keys and tokens.
const KEY_PREFIXES: &[&str] = &[
    "sk-",
    "sk_live_",
    "sk_test_",
    "rk_live_",
    "pk_live_",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "github_pat_",
    "glpat-",
    "xoxb-",
    "xoxp-",
    "xoxa-",
    "xapp-",
    "akia",
    "asia",
    "aiza",
    "ya29.",
    "hf_",
    "npm_",
    "pypi-",
    "dop_v1_",
    "shpat_",
    "eyj",
];

/// Does `t` look like it holds a secret value? A guess that errs towards
/// refusing (a fact is one sentence; losing one is cheap, keeping a key in
/// every later prompt is not):
/// * a word that starts like a well-known key or token (`ghp_`, `sk-`,
///   `AKIA`, a JWT's `eyJ`, a PEM header), at least 16 characters long;
/// * a long random-looking word: 20+ characters of letters and digits
///   mixed (no `://`, so links pass);
/// * a sentence that names a secret (`secret_like`: password, PIN, API
///   key, recovery code...) and gives a value (`is`, `:` or `=` after it).
pub fn looks_secret(t: &str) -> bool {
    if t.contains("-----BEGIN") {
        return true;
    }
    for raw in t.split_whitespace() {
        let w = raw.trim_matches(|c: char| !c.is_alphanumeric() && !"-_./+=".contains(c));
        let w = w.trim_end_matches(['.', ',']);
        if w.contains("://") {
            continue;
        }
        let lower = w.to_ascii_lowercase();
        let n = w.chars().count();
        if n >= 16 && KEY_PREFIXES.iter().any(|p| lower.starts_with(p)) {
            return true;
        }
        let letters = w.chars().filter(|c| c.is_ascii_alphabetic()).count();
        let digits = w.chars().filter(|c| c.is_ascii_digit()).count();
        let upper = w.chars().any(|c| c.is_ascii_uppercase());
        let lower_c = w.chars().any(|c| c.is_ascii_lowercase());
        let symbols_ok = w
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_/+=".contains(c));
        if n >= 20 && symbols_ok && digits >= 2 && letters >= 4 && (upper && lower_c || n >= 28) {
            return true;
        }
        if n >= 32 && w.chars().all(|c| c.is_ascii_hexdigit()) {
            return true;
        }
    }
    if harness_core::ui::secret_like(t) {
        let words: Vec<String> = t
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect();
        return t.contains(':')
            || t.contains('=')
            || words.iter().any(|w| w == "is" || w == "are" || w == "was");
    }
    false
}

#[cfg(test)]
mod clean_tests {
    use super::*;

    #[test]
    fn secrets_are_refused() {
        for t in [
            "Alex's GitHub token is ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            "OpenAI key sk-proj-AbCdEf0123456789xyzXYZ",
            "AWS: AKIAIOSFODNN7EXAMPLE",
            "uses the key 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
            "Alex's password is tulip",
            "wifi password: correct horse",
            "Alex's PIN is 4521",
            "recovery code = 7781-2231",
            "deploy token Zq8LmN2pR7sT4vW9xY3bC6dF",
            "-----BEGIN OPENSSH PRIVATE KEY----- b3BlbnNzaC1rZXk",
        ] {
            assert!(looks_secret(t), "{t}");
            assert!(clean_text(t).is_err(), "{t}");
        }
        for t in [
            "Alex rides the number 7 tram to work",
            "Alex uses 2FA everywhere",
            "Prefers a password manager over browser autofill",
            "Works on https://github.com/acme/rocket-launcher-2024 daily",
            "Has type 1 diabetes and takes insulin before lunch",
            "Lives at Brivibas iela 21, Riga",
            "Their daughter Mia was born on 2019-04-12",
            "Keeps the API keys for work in 1Password",
        ] {
            assert!(!looks_secret(t), "{t}");
        }
    }

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
