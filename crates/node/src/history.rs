//! `history` on the node (PLAN.md T91): the tool reads the log and the
//! images of the one session its call runs in, from the node's own store.
//! Nothing goes to the master, and the session id is the one the turn was
//! built for: a sub-agent's turn holds its own id, never its parent's.

use std::sync::Arc;

use harness_core::RawEvent;
use harness_core::SessionId;
use harness_core::history::{HistorySource, StoredImage};

use crate::Inner;

/// One session's own log and images.
pub(crate) struct OwnHistory {
    pub(crate) node: Arc<Inner>,
    pub(crate) session: SessionId,
}

#[async_trait::async_trait]
impl HistorySource for OwnHistory {
    async fn log(&self) -> Result<Vec<RawEvent>, String> {
        let id = self.session;
        self.node
            .cfg
            .store
            .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
            .await
            .map_err(|e| {
                tracing::warn!(session = %self.session, error = %e, "history could not read the log");
                "the store could not be read".to_string()
            })
    }

    async fn image(&self, sha256: &str) -> Result<Option<StoredImage>, String> {
        let (sid, wanted) = (self.session.to_string(), vec![sha256.to_string()]);
        let found = self
            .node
            .cfg
            .store
            .call(move |s| s.session_images(&sid, &wanted))
            .await
            .map_err(|e| {
                tracing::warn!(session = %self.session, error = %e, "history could not read an image");
                "the store could not be read".to_string()
            })?;
        Ok(found.into_iter().next().map(|i| StoredImage {
            mime: i.mime,
            bytes: i.bytes,
        }))
    }
}
