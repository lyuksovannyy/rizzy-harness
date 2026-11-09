//! Media: each account's private library of images, video and audio, and
//! images in chats (docs/media.md). Items are private to their owner (and
//! the API token); the workspace an item was added in filters lists, it
//! never shares them. The bytes live in the blob store (`storage`).

use axum::http::StatusCode;
use base64::Engine as _;
use harness_core::SessionId;
use harness_protocol::{CAP_IMAGES, MAX_IMAGE_BYTES, MAX_IMAGES};
use harness_store::SessionRow;
use harness_store::media::MediaRow;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::Master;
use crate::auth::Principal;
use crate::search::{SearchCtx, SearchResult};
use crate::workspaces::Access;

/// One image sent with a message, as the node gets it.
pub use harness_protocol::ChatImage;

/// `[media]` in config.toml; left out of the file while it is the default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MediaConfig {
    /// On by default: the library, images in chats, and the items of
    /// agent-published media.
    pub enabled: bool,
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl MediaConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Images one chat may carry in all.
pub const MAX_CHAT_IMAGES: usize = 50;

/// What a chat takes: the formats every vision provider reads (the
/// composer re-encodes anything else, HEIC included, as JPEG).
const CHAT_FORMATS: &[&str] = &["png", "jpeg", "webp", "gif"];

/// Why a message's images were refused: the status and the user's text.
pub type Refusal = (StatusCode, String);

const TOO_MUCH: &str = "Too much attached to one message";

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
    // Images in chats: on with Media, while some model can see them (each
    // chat's own model is checked when it is sent one).
    let vision = on && m.models().iter().any(|i| i.vision && !i.unavailable);
    json!({ "enabled": on, "show": on, "max_file": max_file, "vision_images": vision })
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
        // Chats it was sent in that still exist: deleting the item leaves
        // their copies (and the provider's) where they are.
        "chats": r.chats,
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

/// The images `ids` for a prompt in chat `row`, checked for `who`: their
/// own image items in a format models take, at most `MAX_IMAGES` and
/// `MAX_IMAGE_BYTES` together, never in an incognito chat, only to a node
/// that takes images and a model that sees them (the chat's own; a turn run
/// by another agent or compared across models is checked by
/// `Master::prompt`). The chat starts carrying them once the prompt is
/// sent (`prompted`).
pub async fn prompt_images(
    m: &Master,
    who: &Principal,
    row: &SessionRow,
    ids: &[String],
) -> Result<Vec<ChatImage>, Refusal> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    if !enabled(m) {
        return Err(unknown());
    }
    check_ids(ids)?;
    if row.incognito {
        return Err((
            StatusCode::CONFLICT,
            "Images can't go into an incognito chat.".into(),
        ));
    }
    let items = chat_items(m, who, ids).await?;
    if items.iter().map(|r| r.blob.size).sum::<u64>() > MAX_IMAGE_BYTES {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, TOO_MUCH.into()));
    }
    if !m.node_has(&row.node_id, CAP_IMAGES) {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            "This chat's worker can't take images yet; update it.".into(),
        ));
    }
    if let Some(model) = m.blind_model(row, None, &[]).await {
        return Err((StatusCode::UNPROCESSABLE_ENTITY, blind_message(&model)));
    }
    room_in_chat(m, row.session_id, ids).await?;
    let mut out = Vec::with_capacity(items.len());
    for r in items {
        out.push(read_image(m, r).await?);
    }
    Ok(out)
}

/// What a model that cannot see images is refused with.
pub fn blind_message(model: &str) -> String {
    format!("{model} can't see images; choose a model that can.")
}

fn unknown() -> Refusal {
    (StatusCode::NOT_FOUND, "unknown media item".into())
}

fn store_failed(e: harness_store::StoreError) -> Refusal {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// At most `MAX_IMAGES`, each once.
fn check_ids(ids: &[String]) -> Result<(), Refusal> {
    if ids.len() > MAX_IMAGES {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("At most {MAX_IMAGES} images per message."),
        ));
    }
    if ids.iter().enumerate().any(|(i, id)| ids[..i].contains(id)) {
        return Err((
            StatusCode::BAD_REQUEST,
            "the same image is attached twice".into(),
        ));
    }
    Ok(())
}

/// The items, if `who` may send each one to a model.
async fn chat_items(m: &Master, who: &Principal, ids: &[String]) -> Result<Vec<MediaRow>, Refusal> {
    let want = ids.to_vec();
    let rows = m
        .inner
        .mirror
        .call(move |s| {
            want.iter()
                .map(|id| s.media_get(id))
                .collect::<harness_store::Result<Vec<_>>>()
        })
        .await
        .map_err(store_failed)?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let Some(r) = r.filter(|r| who.can_see(r.owner_id.as_deref())) else {
            return Err(unknown());
        };
        if r.kind() != "image" || !CHAT_FORMATS.contains(&r.blob.format.as_str()) {
            return Err((
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                format!(
                    "{}: only PNG, JPEG, WebP and GIF images go into a chat.",
                    r.name
                ),
            ));
        }
        out.push(r);
    }
    Ok(out)
}

/// A chat carries at most `MAX_CHAT_IMAGES` (images it already carries are
/// free to send again).
async fn room_in_chat(m: &Master, sid: SessionId, ids: &[String]) -> Result<(), Refusal> {
    let s = sid.to_string();
    let carried = m
        .inner
        .mirror
        .call(move |st| st.media_uses_in(&s))
        .await
        .map_err(store_failed)?;
    let new = ids.iter().filter(|id| !carried.contains(id)).count();
    if carried.len() + new > MAX_CHAT_IMAGES {
        return Err((
            StatusCode::CONFLICT,
            format!("A chat holds at most {MAX_CHAT_IMAGES} images; start a new chat for more."),
        ));
    }
    Ok(())
}

/// An item's bytes for the node, decrypted from the blob store.
async fn read_image(m: &Master, r: MediaRow) -> Result<ChatImage, Refusal> {
    let st = crate::storage::get(m).ok_or_else(unknown)?;
    let path = st.path(&r.blob.blob_id).ok_or_else(unknown)?;
    let blob = r.blob.blob_id.clone();
    let key = m
        .inner
        .mirror
        .call(move |s| s.blob_key(&blob))
        .await
        .map_err(store_failed)?
        .ok_or_else(unknown)?;
    let (id, size) = (r.blob.blob_id.clone(), r.blob.size);
    let bytes = tokio::task::spawn_blocking(move || crate::blobs::read_all(&path, &id, &key, size))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("{}: the stored file could not be read: {e}", r.name),
            )
        })?;
    Ok(ChatImage {
        media_id: r.media_id,
        sha256: r.blob.sha256,
        mime: r.blob.mime,
        name: r.name,
        width: r.preview.width.unwrap_or(0),
        height: r.preview.height.unwrap_or(0),
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

/// The prompt with `images` reached chat `row`'s node: the chat now
/// carries them (the delete dialog's "still in N chats").
pub(crate) async fn prompted(m: &Master, row: &SessionRow, images: &[String]) {
    if images.is_empty() {
        return;
    }
    let (sid, ids) = (row.session_id.to_string(), images.to_vec());
    match m
        .inner
        .mirror
        .call(move |s| s.media_uses_add(&sid, &ids))
        .await
    {
        Ok(()) => m.changed("media", row.owner_id.clone()),
        Err(e) => tracing::warn!(error = %e, "could not record which chat carries an image"),
    }
}

/// Delete `who`'s items among `ids` (the API token: anyone's), then the
/// blobs nothing references any more: rows first (`secure_delete`), the
/// WAL truncated, then the files. Returns how many items went.
pub async fn delete(m: &Master, who: &Principal, ids: Vec<String>) -> harness_store::Result<usize> {
    let owner = who.user_id.clone();
    let gone = m
        .inner
        .mirror
        .call(move |s| {
            let d = s.media_delete(owner.as_deref(), &ids)?;
            if !d.blobs.is_empty() {
                s.checkpoint()?;
            }
            Ok::<_, harness_store::StoreError>(d)
        })
        .await?;
    crate::storage::unlink(m, gone.blobs).await;
    if gone.items > 0 {
        m.changed("media", who.user_id.clone());
    }
    Ok(gone.items)
}

/// Before chat `sid` is deleted with `?media=1`: delete `who`'s Media items
/// that came from it (sent there or published there by an agent; the
/// artifacts behind agent items stay).
pub async fn on_chat_deleted(
    m: &Master,
    who: &Principal,
    sid: SessionId,
) -> harness_store::Result<()> {
    let (owner, s) = (who.user_id.clone(), sid.to_string());
    let ids = m
        .inner
        .mirror
        .call(move |st| st.media_from_session(owner.as_deref(), &s))
        .await?;
    if !ids.is_empty() {
        delete(m, who, ids).await?;
    }
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

    #[test]
    fn image_ids_are_few_and_distinct() {
        let ids = |n: usize| (0..n).map(|i| format!("{i:032x}")).collect::<Vec<_>>();
        assert!(check_ids(&ids(MAX_IMAGES)).is_ok());
        assert_eq!(
            check_ids(&ids(MAX_IMAGES + 1)).unwrap_err().0,
            StatusCode::PAYLOAD_TOO_LARGE
        );
        let twice = vec![ids(1)[0].clone(), ids(1)[0].clone()];
        assert_eq!(check_ids(&twice).unwrap_err().0, StatusCode::BAD_REQUEST);
        assert!(MediaConfig::default().enabled);
    }
}
