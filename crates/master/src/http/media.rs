//! Media over HTTP (docs/media.md). Every item is its owner's alone (the
//! API token sees all); anyone else gets 404, admins included (T10). While
//! Media is off every route answers 404.

use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use harness_store::blobs::{Limits, NewBlob};
use harness_store::media::{MediaInsert, MediaQuery, MediaRow, NewItem};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, query_flag, require};
use crate::auth::Principal;
use crate::media::{self, item_json};
use crate::workspaces as ws;
use crate::{Master, storage};

/// Thumbnails are small pictures, never files.
const MAX_THUMB: usize = 64 * 1024;
/// Items one bulk delete may name.
const MAX_BULK: usize = 100;
const DEFAULT_PAGE: usize = 60;
const MAX_PAGE: usize = 200;

pub(super) fn routes() -> Router<Master> {
    Router::new()
        .route(
            "/api/media",
            // Uploads stream to disk with their own limits, read from the
            // config on every request (it changes without a restart).
            get(list).post(upload).layer(DefaultBodyLimit::disable()),
        )
        .route("/api/media/delete", post(delete_many))
        .route(
            "/api/media/{id}",
            get(detail).patch(update).delete(delete_one),
        )
        .route("/api/media/{id}/content", get(content))
        .route(
            "/api/media/{id}/thumb",
            get(thumb)
                .put(put_thumb)
                .layer(DefaultBodyLimit::max(4 * MAX_THUMB)),
        )
}

fn unknown() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "unknown media item".into())
}

fn refused((code, msg): storage::Refusal) -> ApiError {
    ApiError(code, msg)
}

/// The caller, while Media is on (404 otherwise, like a missing route).
async fn caller(m: &Master, headers: &HeaderMap) -> ApiResult<Principal> {
    let who = require(m, headers).await?;
    if !media::enabled(m) {
        return Err(unknown());
    }
    Ok(who)
}

/// Item `id`, if the caller may see it: its owner, or the API token.
async fn visible(m: &Master, headers: &HeaderMap, id: &str) -> ApiResult<(Principal, MediaRow)> {
    let who = caller(m, headers).await?;
    let mid = id.to_string();
    let row = m.inner.mirror.call(move |s| s.media_get(&mid)).await?;
    match row {
        Some(r) if who.can_see(r.owner_id.as_deref()) => Ok((who, r)),
        _ => Err(unknown()),
    }
}

/// The preview hints and thumbnail belong to the blob, so only its owner
/// sets them (a viewer can never plant one).
fn blob_owner(who: &Principal, r: &MediaRow) -> ApiResult<()> {
    if who.user_id == r.blob.owner_id {
        return Ok(());
    }
    Err(ApiError(
        StatusCode::FORBIDDEN,
        "only the file's owner can change its preview".into(),
    ))
}

fn changed(m: &Master, who: &Principal) {
    m.changed("media", who.user_id.clone());
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default)]
    workspace: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    origin: Option<String>,
    #[serde(default)]
    session: Option<String>,
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    before: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

/// `next` from the previous page: `<created_ms>_<media_id>`.
fn parse_cursor(c: &str) -> Option<(i64, String)> {
    let (ms, id) = c.split_once('_')?;
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some((ms.parse().ok()?, id.to_string()))
}

async fn usage(m: &Master, who: &Principal) -> ApiResult<Value> {
    let owner = who.user_id.clone();
    let used = m
        .inner
        .mirror
        .call(move |s| s.blob_usage(owner.as_deref()))
        .await?
        .used;
    let quota = storage::config(m).limits(who.user_id.as_deref()).account;
    Ok(json!({ "used": used, "quota": quota }))
}

async fn list(
    State(m): State<Master>,
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let who = caller(&m, &headers).await?;
    let scope = ws::scope(&m, &who, q.workspace.as_deref()).await?;
    if let Some(k) = &q.kind
        && !matches!(k.as_str(), "image" | "video" | "audio" | "other")
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "kind is image, video, audio or other".into(),
        ));
    }
    if let Some(o) = &q.origin
        && !matches!(o.as_str(), "upload" | "chat" | "agent")
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "origin is upload, chat or agent".into(),
        ));
    }
    let before = match q.before.as_deref().filter(|c| !c.is_empty()) {
        None => None,
        Some(c) => Some(
            parse_cursor(c)
                .ok_or_else(|| ApiError(StatusCode::BAD_REQUEST, "bad cursor".into()))?,
        ),
    };
    let limit = q.limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE);
    // The same tokens and FTS query as the search panel's provider.
    let tokens = crate::search::tokens(q.q.as_deref().unwrap_or(""));
    let query = MediaQuery {
        owner: who.user_id.clone(),
        workspace: scope.as_ref().map(|a| a.id().to_string()),
        kind: q.kind,
        origin: q.origin,
        session: q.session,
        fts: crate::search::fts_query(&tokens, false),
        before,
        limit: limit + 1,
    };
    let mut rows = m.inner.mirror.call(move |s| s.media_list(&query)).await?;
    let next = (rows.len() > limit).then(|| {
        rows.truncate(limit);
        rows.last()
            .map(|r| format!("{}_{}", r.created_ms, r.media_id))
    });
    Ok(Json(json!({
        "items": rows.iter().map(item_json).collect::<Vec<_>>(),
        "next": next.flatten(),
        "usage": usage(&m, &who).await?,
        "limits": { "max_file": storage::config(&m).max_file() },
    })))
}

#[derive(Deserialize)]
struct UploadQuery {
    #[serde(default)]
    workspace: Option<String>,
    #[serde(default)]
    origin: Option<String>,
    /// `origin=chat`: the chat it is attached in, when it exists already
    /// (the home composer uploads before the chat does; the first chat it
    /// is sent in becomes its source).
    #[serde(default)]
    session: Option<String>,
}

/// Where an upload comes from: the Media page, or a chat's composer (an
/// image, never in an incognito chat). Returns whether it is a chat's and
/// that chat when given.
async fn upload_origin(
    m: &Master,
    who: &Principal,
    q: &UploadQuery,
) -> ApiResult<(bool, Option<String>)> {
    match q.origin.as_deref() {
        None | Some("upload") => Ok((false, None)),
        Some("chat") => {
            let Some(id) = q.session.as_deref().filter(|s| !s.is_empty()) else {
                return Ok((true, None));
            };
            let row = super::visible_session(m, who, id).await?;
            if row.incognito {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "Images can't go into an incognito chat.".into(),
                ));
            }
            Ok((true, Some(row.session_id.to_string())))
        }
        Some(_) => Err(ApiError(
            StatusCode::BAD_REQUEST,
            "origin is upload or chat".into(),
        )),
    }
}

/// `X-File-Name`, percent-encoded UTF-8 (names never go in the URL).
fn file_name(headers: &HeaderMap) -> ApiResult<String> {
    let Some(raw) = headers.get("x-file-name") else {
        return Ok("file".into());
    };
    let bad = || ApiError(StatusCode::BAD_REQUEST, "bad X-File-Name".into());
    let raw = raw.to_str().map_err(|_| bad())?;
    let mut out = Vec::with_capacity(raw.len());
    let b = raw.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = raw.get(i + 1..i + 3).ok_or_else(bad)?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| bad())?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    let name = String::from_utf8(out).map_err(|_| bad())?;
    Ok(media::clean_name(&name))
}

/// A raw streamed body. 201 with the new item, or 200 with the existing
/// one and `duplicate: true` when the same bytes are already in Media in
/// this workspace.
async fn upload(
    State(m): State<Master>,
    headers: HeaderMap,
    Query(q): Query<UploadQuery>,
    body: Body,
) -> ApiResult<Response> {
    let who = caller(&m, &headers).await?;
    let (from_chat, session) = upload_origin(&m, &who, &q).await?;
    let scope = ws::scope(&m, &who, q.workspace.as_deref()).await?;
    let name = file_name(&headers)?;
    let declared = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let st = storage::get(&m).ok_or_else(unknown)?;
    let Some(_slot) = st.upload_slot(who.user_id.as_deref()) else {
        return Ok((
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, "5")],
            Json(json!({ "error": "Too many uploads at once; try again in a moment." })),
        )
            .into_response());
    };
    let length = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let written = storage::receive(&m, who.user_id.as_deref(), length, body)
        .await
        .map_err(refused)?;
    let sniffed = crate::sniff::sniff(&written.head, Some(&name), declared.as_deref());
    let refusal = if !crate::sniff::is_media(sniffed.kind) {
        Some("not a media file; build outputs go to Artifacts")
    } else if from_chat && sniffed.kind != "image" {
        Some("only images go into a chat")
    } else {
        None
    };
    if let Some(why) = refusal {
        // The writer is done: its file is ours to remove.
        let _ = std::fs::remove_file(&written.path);
        return Err(ApiError(StatusCode::UNSUPPORTED_MEDIA_TYPE, why.into()));
    }
    let item = NewItem {
        media_id: harness_core::new_id(),
        owner_id: who.user_id.clone(),
        workspace_id: scope.as_ref().map(|a| a.id().to_string()),
        name,
        origin: if from_chat { "chat" } else { "upload" }.into(),
        session_id: session,
    };
    let blob = NewBlob {
        blob_id: written.blob_id.clone(),
        owner_id: who.user_id.clone(),
        sha256: written.sha256.clone(),
        size: written.size,
        kind: sniffed.kind.into(),
        format: sniffed.format,
        mime: sniffed.mime,
        display: sniffed.display.into(),
        key: written.key.clone(),
        width: sniffed.width,
        height: sniffed.height,
    };
    let limits: Limits = storage::config(&m).limits(who.user_id.as_deref());
    let res = storage::commit(&m, written, move |s| {
        let r = s.media_insert(&item, &blob, limits)?;
        let keep = matches!(r, MediaInsert::Added { new_blob: true, .. });
        Ok((r, keep))
    })
    .await
    .map_err(refused)?;
    let (status, media_id, duplicate) = match res {
        MediaInsert::Added { media_id, .. } => (StatusCode::CREATED, media_id, false),
        MediaInsert::Duplicate { media_id } => (StatusCode::OK, media_id, true),
        MediaInsert::OverQuota(q) => {
            return Err(ApiError(
                StatusCode::INSUFFICIENT_STORAGE,
                storage::full_message(&q),
            ));
        }
    };
    let row = m
        .inner
        .mirror
        .call(move |s| s.media_get(&media_id))
        .await?
        .ok_or_else(unknown)?;
    if !duplicate {
        changed(&m, &who);
    }
    let mut v = item_json(&row);
    if duplicate {
        v["duplicate"] = json!(true);
    }
    Ok((status, Json(v)).into_response())
}

async fn detail(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let (_, row) = visible(&m, &headers, &id).await?;
    Ok(Json(item_json(&row)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Patch {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    width: Option<i64>,
    #[serde(default)]
    height: Option<i64>,
    #[serde(default)]
    duration_ms: Option<i64>,
    #[serde(default)]
    no_thumb: Option<bool>,
}

/// Rename, and the owner's browser hints: a video's size and duration
/// (clamped; an image's size comes from its header and is never
/// overwritten) and `no_thumb` when it could not make a thumbnail.
async fn update(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<Patch>,
) -> ApiResult<Json<Value>> {
    let (who, row) = visible(&m, &headers, &id).await?;
    if let Some(name) = &b.name {
        if harness_core::text::clean_line(name, 1).is_empty() {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "the name is empty".into(),
            ));
        }
        let (mid, name) = (row.media_id.clone(), media::clean_name(name));
        m.inner
            .mirror
            .call(move |s| s.media_rename(&mid, &name))
            .await?;
    }
    let image = row.blob.kind == "image";
    let (width, height) = if image {
        (None, None)
    } else {
        (b.width.map(media::clamp_px), b.height.map(media::clamp_px))
    };
    let duration = b.duration_ms.map(media::clamp_duration);
    if width.is_some() || height.is_some() || duration.is_some() || b.no_thumb.is_some() {
        blob_owner(&who, &row)?;
        let blob = row.blob.blob_id.clone();
        let no_thumb = b.no_thumb;
        m.inner
            .mirror
            .call(move |s| {
                if width.is_some() || height.is_some() || duration.is_some() {
                    s.preview_set_hints(&blob, width, height, duration)?;
                }
                if let Some(n) = no_thumb {
                    s.preview_set_no_thumb(&blob, n)?;
                }
                Ok::<_, harness_store::StoreError>(())
            })
            .await?;
    }
    changed(&m, &who);
    let mid = row.media_id.clone();
    let row = m
        .inner
        .mirror
        .call(move |s| s.media_get(&mid))
        .await?
        .ok_or_else(unknown)?;
    Ok(Json(item_json(&row)))
}

async fn delete_one(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let (who, row) = visible(&m, &headers, &id).await?;
    media::delete(&m, &who, vec![row.media_id]).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteMany {
    ids: Vec<String>,
}

async fn delete_many(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<DeleteMany>,
) -> ApiResult<Json<Value>> {
    let who = caller(&m, &headers).await?;
    if b.ids.len() > MAX_BULK {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            format!("at most {MAX_BULK} items at once"),
        ));
    }
    // Ids that are not the caller's are skipped (the store checks the
    // owner), so nothing confirms they exist.
    media::delete(&m, &who, b.ids).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ContentQuery {
    #[serde(default, deserialize_with = "query_flag")]
    download: bool,
}

async fn content(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<ContentQuery>,
) -> ApiResult<Response> {
    let (_, row) = visible(&m, &headers, &id).await?;
    let st = storage::get(&m).ok_or_else(unknown)?;
    let path = st.path(&row.blob.blob_id).ok_or_else(unknown)?;
    let blob = row.blob.blob_id.clone();
    let key = m
        .inner
        .mirror
        .call(move |s| s.blob_key(&blob))
        .await?
        .ok_or_else(unknown)?;
    Ok(crate::blobs::serve(
        &headers,
        crate::blobs::Serve {
            blob: &row.blob,
            path,
            key,
            name: &row.name,
            inline: !q.download,
        },
    )
    .await)
}

async fn thumb(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let (_, row) = visible(&m, &headers, &id).await?;
    let blob = row.blob.blob_id.clone();
    let (mime, bytes) = m
        .inner
        .mirror
        .call(move |s| s.preview_thumb(&blob))
        .await?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "no thumbnail".into()))?;
    Ok(crate::blobs::serve_thumb(&headers, &mime, bytes))
}

/// The owner's browser-made thumbnail: WebP, JPEG or PNG by its magic
/// bytes, at most 64 KiB.
async fn put_thumb(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    let (who, row) = visible(&m, &headers, &id).await?;
    blob_owner(&who, &row)?;
    if body.len() > MAX_THUMB {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "thumbnails are at most 64 KiB".into(),
        ));
    }
    let mime = crate::sniff::thumb_mime(&body).ok_or_else(|| {
        ApiError(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "thumbnails are WebP, JPEG or PNG".into(),
        )
    })?;
    let blob = row.blob.blob_id.clone();
    m.inner
        .mirror
        .call(move |s| s.preview_put_thumb(&blob, mime, &body))
        .await?;
    changed(&m, &who);
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursors() {
        let id = "0193f1c2a4b57d6e8f9012345678abcd";
        assert_eq!(
            parse_cursor(&format!("1700000000000_{id}")),
            Some((1_700_000_000_000, id.to_string()))
        );
        assert_eq!(parse_cursor("x_y"), None);
        assert_eq!(parse_cursor(&format!("12_{}", &id[1..])), None);
        assert_eq!(parse_cursor(id), None);
    }

    #[test]
    fn file_name_header() {
        let mut h = HeaderMap::new();
        assert_eq!(file_name(&h).ok().as_deref(), Some("file"));
        h.insert("x-file-name", "Caf%C3%A9%20photo.jpg".parse().unwrap());
        assert_eq!(file_name(&h).ok().as_deref(), Some("Café photo.jpg"));
        h.insert("x-file-name", "..%2F..%2Fetc%2Fpasswd".parse().unwrap());
        assert_eq!(file_name(&h).ok().as_deref(), Some("passwd"));
        h.insert("x-file-name", "bad%zz".parse().unwrap());
        assert!(file_name(&h).is_err());
        h.insert("x-file-name", "%FF%FE".parse().unwrap());
        assert!(file_name(&h).is_err());
    }
}
