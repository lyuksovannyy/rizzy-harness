//! Artifacts: files agents publish from agentic chats, kept as named
//! lineages of versions (docs/artifacts.md). Owned by the files backend.
//! Phase 0 holds the signatures the rest of the master calls.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::Master;
use crate::auth::Principal;
use crate::search::{SearchCtx, SearchResult};
use crate::workspaces::Access;

/// `[artifacts]` in config.toml; left out of the file while it is the
/// default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ArtifactsConfig {
    pub enabled: bool,
    /// Ready, unpinned versions kept per artifact.
    pub keep_versions: u32,
    /// Pinned versions allowed per artifact.
    pub max_pinned: u32,
}

impl Default for ArtifactsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            keep_versions: 5,
            max_pinned: 10,
        }
    }
}

impl ArtifactsConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// The `artifacts` block of `/api/features` for `who` in `scope`.
pub async fn features(_m: &Master, _who: &Principal, _scope: Option<&Access>) -> Value {
    json!({ "enabled": false, "show": false, "why": null })
}

/// The search provider over artifact names, file names and notes.
pub fn search(
    _s: &harness_store::Store,
    _ctx: &SearchCtx,
) -> harness_store::Result<Vec<SearchResult>> {
    Ok(vec![])
}

/// After node `node_id` was revoked: its pending versions fail.
pub async fn on_node_revoked(_m: &Master, _node_id: &str) {}
