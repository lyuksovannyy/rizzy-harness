//! Media: each account's private library of images, video and audio, and
//! images in chats (docs/media.md). Items are private to their owner (and
//! the API token); the workspace an item was added in filters lists, it
//! never shares them. The bytes live in the blob store (`storage`).

use axum::http::StatusCode;
use harness_core::SessionId;
use harness_store::media::MediaRow;
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

/// Longest item name kept.
pub const MAX_NAME: usize = 120;
/// Video size hints are kept within this many pixels a side, and durations
/// within 48 hours (they come from the owner's browser).
const MAX_HINT_PX: u32 = 100_000;
const MAX_DURATION_MS: u64 = 48 * 3600 * 1000;

/// Whether Media is on: switched on in config.toml, with a blob store.
pub fn enabled(m: &Master) -> bool {
    crate::storage::get(m).is_some()
        && m.provider_runtime()
            .is_some_and(|rt| rt.config().media.enabled)
}

/// The `media` block of `/api/features` for `who` in `scope`: every member
/// keeps their own Media, guests included.
pub async fn features(m: &Master, _who: &Principal, _scope: Option<&Access>) -> Value {
    let on = enabled(m);
    let max_file = if on {
        crate::storage::config(m).max_file()
    } else {
        0
    };
    json!({ "enabled": on, "show": on, "max_file": max_file, "vision_images": false })
}

/// A display name for an item: one clean line (hidden characters and
/// controls gone), its last path segment, at most `MAX_NAME` characters.
pub fn clean_name(name: &str) -> String {
    let line = harness_core::text::clean_line(name, name.len());
    let base = line.rsplit(['/', '\\']).next().unwrap_or("").trim();
    let capped: String = base.chars().take(MAX_NAME).collect();
    let capped = capped.trim_end();
    if capped.is_empty() {
        "file".into()
    } else {
        capped.to_string()
    }
}

/// A browser's size hint, kept only when it is a plausible picture size.
pub fn clamp_px(v: i64) -> u32 {
    v.clamp(1, i64::from(MAX_HINT_PX)) as u32
}

pub fn clamp_duration(v: i64) -> u64 {
    v.clamp(0, MAX_DURATION_MS as i64) as u64
}

/// An item as the API shows it (`MediaItem`).
pub fn item_json(r: &MediaRow) -> Value {
    let source = match (&r.session_id, &r.session_title) {
        (None, _) => Value::Null,
        (Some(sid), Some(title)) => json!({ "session_id": sid, "title": title }),
        // The chat is gone, or is not the owner's own.
        (Some(_), None) => json!("deleted"),
    };
    json!({
        "media_id": r.media_id,
        "name": r.name,
        "kind": r.kind(),
        "format": r.blob.format,
        "mime": r.blob.mime,
        "size": r.blob.size,
        "width": r.preview.width,
        "height": r.preview.height,
        "duration_ms": r.preview.duration_ms,
        "display": r.blob.display,
        "has_thumb": r.preview.has_thumb,
        "no_thumb": r.preview.no_thumb,
        "origin": r.origin,
        "source": source,
        // An agent item: the artifact version it shows, while it exists.
        "artifact": match (&r.artifact_id, &r.version_id, r.artifact_seq, &r.artifact_name) {
            (Some(a), Some(v), Some(seq), Some(name)) => {
                json!({ "artifact_id": a, "version_id": v, "seq": seq, "name": name })
            }
            _ => Value::Null,
        },
        // Chats still carrying the item arrive with images in chats.
        "chats": 0,
        "workspace_id": r.workspace_id,
        "created_ms": r.created_ms,
        "updated_ms": r.updated_ms,
    })
}

/// The search provider over Media names. Visibility is the list's: the
/// owner's items (the API token: all) in the caller's workspace.
pub fn search(
    s: &harness_store::Store,
    ctx: &SearchCtx,
) -> harness_store::Result<Vec<SearchResult>> {
    let rows = s.media_search(
        ctx.owner,
        ctx.workspace,
        ctx.fts.as_deref(),
        ctx.limit,
        ctx.offset,
    )?;
    Ok(rows
        .into_iter()
        .map(|(r, marked)| {
            // Only names are indexed: every term in the name, or some.
            let tier =
                if ctx.tokens.is_empty() || crate::search::coverage(ctx.tokens, &r.name) >= 1.0 {
                    0
                } else {
                    1
                };
            let preview = match (r.origin.as_str(), &r.session_title) {
                ("chat", Some(title)) => format!("From chat \u{2018}{title}\u{2019}"),
                ("chat", None) => "From a deleted chat".into(),
                ("agent", _) => "Published by an agent".into(),
                _ => "Uploaded".into(),
            };
            SearchResult {
                kind: "media",
                type_: match r.kind() {
                    "image" => "image",
                    "video" => "video",
                    "audio" => "audio",
                    _ => "media_other",
                },
                title: marked,
                preview,
                snippet: None,
                time_ms: r.updated_ms,
                project_id: None,
                running: None,
                thumb: r
                    .preview
                    .has_thumb
                    .then(|| format!("/api/media/{}/thumb", r.media_id)),
                tier,
                id: r.media_id,
            }
        })
        .collect())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_hints() {
        assert_eq!(clean_name("../../photos/IMG_0001.HEIC"), "IMG_0001.HEIC");
        assert_eq!(clean_name("a\u{202e}b\nc.png"), "ab c.png");
        assert_eq!(clean_name("   "), "file");
        assert_eq!(clean_name(&"x".repeat(500)).chars().count(), MAX_NAME);
        assert_eq!(clamp_px(0), 1);
        assert_eq!(clamp_px(10_000_000), MAX_HINT_PX);
        assert_eq!(clamp_duration(-5), 0);
        assert_eq!(clamp_duration(i64::MAX), MAX_DURATION_MS);
    }
}
