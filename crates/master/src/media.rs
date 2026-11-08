//! Media: each account's private library of images, video and audio, and
//! images in chats (docs/media.md). Owned by the files backend. Phase 0
//! holds the signatures the rest of the master calls.

use axum::http::StatusCode;
use harness_core::SessionId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::Master;
use crate::auth::Principal;
use crate::search::{SearchCtx, SearchResult};
use crate::workspaces::Access;

/// `[media]` in config.toml; left out of the file while it is the default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MediaConfig {
    /// Off until images in chats land: Media alone would be an upload-only
    /// locker.
    pub enabled: bool,
}

// Written out, not derived: `false` is a decision that flips once images in
// chats land, not the type's zero.
#[allow(clippy::derivable_impls)]
impl Default for MediaConfig {
    fn default() -> Self {
        Self { enabled: false }
    }
}

impl MediaConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// One image sent with a message, as the node gets it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatImage {
    pub sha256: String,
    pub mime: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// The bytes, base64.
    pub data: String,
}

/// The `media` block of `/api/features` for `who` in `scope`.
pub async fn features(_m: &Master, _who: &Principal, _scope: Option<&Access>) -> Value {
    json!({ "enabled": false, "show": false, "max_file": 0, "vision_images": false })
}

/// The search provider over Media names.
pub fn search(
    _s: &harness_store::Store,
    _ctx: &SearchCtx,
) -> harness_store::Result<Vec<SearchResult>> {
    Ok(vec![])
}

/// The images `ids` for a prompt in chat `row`, checked for `who`.
pub async fn prompt_images(
    _m: &Master,
    _who: &Principal,
    _row: &harness_store::SessionRow,
    ids: &[String],
) -> Result<Vec<ChatImage>, (StatusCode, String)> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    Err((
        StatusCode::BAD_REQUEST,
        "images are not available yet".into(),
    ))
}

/// Before chat `sid` is deleted with `?media=1`: delete `who`'s Media items
/// that came from it.
pub async fn on_chat_deleted(
    _m: &Master,
    _who: &Principal,
    _sid: SessionId,
) -> harness_store::Result<()> {
    Ok(())
}
