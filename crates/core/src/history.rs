//! Where a session's `history` calls read its own chat (PLAN.md T91): the
//! node's store, bound to the session the call runs for. Nothing leaves the
//! node, and a source only ever reads the one session it was made for.

use crate::RawEvent;

/// The tool's name.
pub const TOOL: &str = "history";

/// An image kept with a session (`session_images`), by its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredImage {
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// The session's own log and images, read-only.
#[async_trait::async_trait]
pub trait HistorySource: Send + Sync {
    /// Every event of the session, in order, as the node's store holds them
    /// (so a call after a restart reads the same). `Err` says, for the
    /// model, why it could not be read.
    async fn log(&self) -> Result<Vec<RawEvent>, String>;

    /// The bytes of one of the session's images by their hex SHA-256;
    /// `Ok(None)` when the store no longer has them.
    async fn image(&self, sha256: &str) -> Result<Option<StoredImage>, String>;
}
