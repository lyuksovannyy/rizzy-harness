//! Artifacts over HTTP (docs/artifacts.md). A private artifact is its
//! owner's alone; a shared one is every member's of its workspace to see
//! and download. Anything invisible is 404, admins included (T10); the API
//! token sees all. Files are served by `blobs::serve` (T17): never
//! rendered, previews only for the sniffed kinds a browser may show.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use harness_core::SessionId;
use harness_protocol::{CAP_ARTIFACTS, MasterToNode};
use harness_store::artifacts::{ArtifactQuery, ArtifactRow, VersionRow};
use serde::Deserialize;
use serde_json::{Value, json};

use super::links::require_bytes;
use super::{ApiError, ApiResult, agentic_gate, query_flag, require, visible_session};
use crate::auth::Principal;
use crate::workspaces::{self as ws, Perm};
use crate::{CommandError, Master, artifacts, storage};

const MAX_THUMB: usize = 64 * 1024;
const DEFAULT_PAGE: usize = 50;
const MAX_PAGE: usize = 100;

pub(super) fn routes() -> Router<Master> {
    Router::new()
        .route("/api/artifacts", get(list))
        .route(
            "/api/artifacts/{id}",
            get(detail).patch(update).delete(remove),
        )
        .route("/api/artifacts/{id}/latest/download", get(latest_download))
        .route(
            "/api/artifacts/{id}/versions/{vid}",
            get(version).patch(version_update).delete(version_delete),
        )
        .route("/api/artifacts/{id}/versions/{vid}/download", get(download))
        .route("/api/artifacts/{id}/versions/{vid}/preview", get(preview))
        .route(
            "/api/artifacts/{id}/versions/{vid}/thumb",
            get(thumb)
                .put(put_thumb)
                .layer(DefaultBodyLimit::max(4 * MAX_THUMB)),
        )
        .route(
            "/api/sessions/{id}/artifacts",
            get(session_list).post(session_save),
        )
}

fn unknown() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "unknown artifact".into())
}

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

/// Artifact `id`, if `who` may see it: the owner, a member of its
/// workspace while it is shared there, or the API token.
async fn visible(m: &Master, who: &Principal, id: &str) -> ApiResult<ArtifactRow> {
    let aid = id.to_string();
    let row = m
        .inner
        .mirror
        .call(move |s| s.artifact_get(&aid))
        .await?
        .ok_or_else(unknown)?;
    let Some(me) = who.user_id.as_deref() else {
        return Ok(row);
    };
    let Some(eff) = row.effective_ws.as_deref() else {
        return Err(unknown());
    };
    if ws::access(m, who, eff).await.is_err() {
        return Err(unknown());
    }
    if row.owner_id.as_deref() == Some(me) || row.shared() {
        Ok(row)
    } else {
        Err(unknown())
    }
}

fn mine(who: &Principal, row: &ArtifactRow) -> bool {
    who.user_id.is_none() || who.user_id == row.owner_id
}

/// Owner-only changes; a visible artifact of someone else's is 403.
fn owner_only(who: &Principal, row: &ArtifactRow) -> ApiResult<()> {
    if mine(who, row) {
        return Ok(());
    }
    Err(ApiError(
        StatusCode::FORBIDDEN,
        "only its owner can change this artifact".into(),
    ))
}

/// A version of a visible artifact.
async fn visible_version(
    m: &Master,
    who: &Principal,
    id: &str,
    vid: &str,
) -> ApiResult<(ArtifactRow, VersionRow)> {
    let row = visible(m, who, id).await?;
    let v = vid.to_string();
    let ver = m
        .inner
        .mirror
        .call(move |s| s.artifact_version(&v))
        .await?
        .filter(|v| v.artifact_id == row.artifact_id)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "unknown version".into()))?;
    Ok((row, ver))
}

/// What a request needs to describe versions: node names and the chats
/// they came from.
struct View {
    viewer: Principal,
    nodes: HashMap<String, String>,
    /// Session id → (owner, title) for chats that still exist.
    chats: HashMap<String, (Option<String>, String)>,
}

impl View {
    async fn new(m: &Master, who: &Principal, versions: &[&VersionRow]) -> ApiResult<Self> {
        let ids: Vec<SessionId> = versions
            .iter()
            .filter_map(|v| v.session_id.as_deref()?.parse().ok())
            .collect();
        let chats = m
            .inner
            .mirror
            .call(move |s| {
                let mut out = HashMap::new();
                for sid in ids {
                    if let Some(r) = s.session(sid)? {
                        out.insert(sid.to_string(), (r.owner_id, r.title));
                    }
                }
                Ok::<_, harness_store::StoreError>(out)
            })
            .await?;
        Ok(Self {
            viewer: who.clone(),
            nodes: m
                .nodes()
                .into_iter()
                .map(|n| (n.node_id, n.hostname))
                .collect(),
            chats,
        })
    }

    /// `ArtifactVersion` (sidebar areas §7.8) of a version of `r`.
    fn version(&self, m: &Master, r: &ArtifactRow, v: &VersionRow) -> Value {
        let progress = (v.status == "pending")
            .then(|| crate::artifact_pull::progress(m, &v.version_id))
            .flatten()
            .map(|(received, total)| json!({ "received": received, "total": total }));
        let session = match v.session_id.as_deref() {
            None => Value::Null,
            Some(sid) => match self.chats.get(sid) {
                None => Value::Null,
                Some((owner, title)) if self.viewer.can_see(owner.as_deref()) => {
                    json!({ "session_id": sid, "title": title })
                }
                Some(_) => json!({ "private": true }),
            },
        };
        let mut meta: Value = serde_json::from_str(&v.meta).unwrap_or_else(|_| json!({}));
        let b = v.blob.as_ref();
        // The node names its own platform for every file; only a build has one.
        if !crate::artifacts::is_build(b.map(|b| b.kind.as_str()))
            && let Some(m) = meta.as_object_mut()
        {
            m.remove("target");
        }
        // Where the file was on its node is the owner's directory layout:
        // not for the members a shared artifact is shown to (AUDIT L5).
        if !mine(&self.viewer, r)
            && let Some(m) = meta.as_object_mut()
        {
            m.remove("source_path");
        }
        // An image's size from its header (read at commit, never decoded);
        // other kinds' previews hold only the owner's browser hints.
        let image = b.is_some_and(|b| b.kind == "image");
        let (width, height) = if image {
            (v.preview.width, v.preview.height)
        } else {
            (None, None)
        };
        json!({
            "version_id": v.version_id,
            "seq": v.seq,
            "status": v.status,
            "error": v.error,
            "progress": progress,
            "size": v.size,
            "sha256": v.sha256,
            "filename": v.filename,
            "packed": v.packed,
            "kind": b.map(|b| b.kind.clone()),
            "format": b.map(|b| b.format.clone()),
            "display": b.map(|b| b.display.clone()),
            "width": width,
            "height": height,
            "meta": meta,
            "notes": v.notes,
            "node": { "node_id": v.node_id, "hostname": self.nodes.get(&v.node_id).cloned().unwrap_or_default() },
            "session": session,
            "by_user": v.by_user,
            "pinned": v.pinned,
            "has_thumb": v.preview.has_thumb,
            "created_ms": v.created_ms,
        })
    }

    /// `Artifact` (§7.8). The Media item is named to its owner only (Media
    /// is private).
    fn artifact(&self, m: &Master, r: &ArtifactRow, latest: Option<&VersionRow>) -> Value {
        let is_mine = mine(&self.viewer, r);
        json!({
            "artifact_id": r.artifact_id,
            "name": r.name,
            "kind": latest.and_then(|v| v.blob.as_ref()).map(|b| b.kind.clone()),
            "visibility": r.visibility,
            "mine": is_mine,
            "owner": r.owner_name.as_ref().map(|u| json!({ "username": u })),
            "project_id": r.project_id,
            "versions": r.ready,
            "latest": latest.map(|v| self.version(m, r, v)),
            "media_id": if is_mine { r.media_id.clone() } else { None },
            "updated_ms": r.updated_ms,
        })
    }
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default)]
    workspace: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default, deserialize_with = "query_flag")]
    mine: bool,
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    cursor: Option<String>,
}

/// The blob kinds a list filter stands for.
fn kinds(filter: &str) -> Option<Vec<String>> {
    let k: &[&str] = match filter {
        "builds" => &["binary", "package"],
        "archives" => &["archive"],
        "documents" => &["document"],
        "media" => &["image", "video", "audio"],
        "other" => &["other"],
        _ => return None,
    };
    Some(k.iter().map(|s| s.to_string()).collect())
}

/// `next_cursor`: `<updated_ms>_<artifact_id>`.
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
    let who = require(&m, &headers).await?;
    let scope = ws::scope(&m, &who, q.workspace.as_deref()).await?;
    let kinds = match q.kind.as_deref().filter(|k| !k.is_empty()) {
        None => None,
        Some(k) => Some(
            kinds(k).ok_or_else(|| bad("kind is builds, archives, documents, media or other"))?,
        ),
    };
    let before = match q.cursor.as_deref().filter(|c| !c.is_empty()) {
        None => None,
        Some(c) => Some(parse_cursor(c).ok_or_else(|| bad("bad cursor"))?),
    };
    let limit = q.limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE);
    // The same tokens and FTS query as the search panel's provider.
    let tokens = crate::search::tokens(q.q.as_deref().unwrap_or(""));
    let query = ArtifactQuery {
        viewer: who.user_id.clone(),
        workspace: scope.as_ref().map(|a| a.id().to_string()),
        project: q.project.filter(|p| !p.is_empty()),
        kinds,
        mine: q.mine,
        fts: crate::search::fts_query(&tokens, false),
        before,
        limit: limit + 1,
    };
    let (mut rows, latest) = m
        .inner
        .mirror
        .call(move |s| {
            let rows = s.artifact_list(&query)?;
            let latest = rows
                .iter()
                .map(|r| s.artifact_latest(&r.artifact_id))
                .collect::<harness_store::Result<Vec<_>>>()?;
            Ok::<_, harness_store::StoreError>((rows, latest))
        })
        .await?;
    let next = (rows.len() > limit).then(|| {
        rows.truncate(limit);
        rows.last()
            .map(|r| format!("{}_{}", r.updated_ms, r.artifact_id))
    });
    let shown: Vec<&VersionRow> = latest.iter().flatten().collect();
    let view = View::new(&m, &who, &shown).await?;
    let artifacts: Vec<Value> = rows
        .iter()
        .zip(&latest)
        .map(|(r, l)| view.artifact(&m, r, l.as_ref()))
        .collect();
    Ok(Json(json!({
        "artifacts": artifacts,
        "next_cursor": next.flatten(),
        "usage": usage(&m, &who).await?,
    })))
}

async fn detail(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let row = visible(&m, &who, &id).await?;
    let aid = row.artifact_id.clone();
    let versions = m
        .inner
        .mirror
        .call(move |s| s.artifact_versions(&aid))
        .await?;
    let refs: Vec<&VersionRow> = versions.iter().collect();
    let view = View::new(&m, &who, &refs).await?;
    let latest = versions
        .iter()
        .find(|v| v.status == "ready")
        .or(versions.first());
    let mut v = view.artifact(&m, &row, latest);
    v["versions"] = json!(
        versions
            .iter()
            .map(|x| view.version(&m, &row, x))
            .collect::<Vec<_>>()
    );
    Ok(Json(v))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Patch {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    visibility: Option<String>,
}

async fn update(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<Patch>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let row = visible(&m, &who, &id).await?;
    owner_only(&who, &row)?;
    let name = b
        .name
        .as_deref()
        .map(harness_core::artifacts::check_name)
        .transpose()
        .map_err(bad)?;
    if let Some(v) = b.visibility.as_deref() {
        if !matches!(v, "private" | "workspace") {
            return Err(bad("visibility is private or workspace"));
        }
        let personal = match row.effective_ws.as_deref() {
            Some(w) => ws::access(&m, &who, w)
                .await
                .map(|a| a.personal)
                .unwrap_or(true),
            None => true,
        };
        if v == "workspace" && (personal || row.workspace_id != row.effective_ws) {
            return Err(bad("only artifacts in a team workspace can be shared"));
        }
    }
    let (aid, vis) = (row.artifact_id.clone(), b.visibility.clone());
    let res = m
        .inner
        .mirror
        .call(move |s| s.artifact_update(&aid, name.as_deref(), vis.as_deref()))
        .await;
    match res {
        Ok(_) => {}
        Err(harness_store::StoreError::Conflict(_)) => {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "another artifact here already has that name".into(),
            ));
        }
        Err(e) => return Err(e.into()),
    }
    // Sharing or unsharing changes other members' lists too.
    artifacts::notify(
        &m,
        row.owner_id.clone(),
        row.shared() || b.visibility.is_some(),
    );
    detail(State(m), headers, Path(id)).await
}

/// Delete for good: the owner, or for a shared artifact a member who may
/// change the workspace's settings.
async fn remove(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let row = visible(&m, &who, &id).await?;
    if !mine(&who, &row) {
        let w = row.effective_ws.clone().ok_or_else(unknown)?;
        ws::access(&m, &who, &w).await?.require(Perm::Settings)?;
    }
    let aid = row.artifact_id.clone();
    let gone = m
        .inner
        .mirror
        .call(move |s| {
            let gone = s.artifact_delete(&aid)?;
            if !gone.is_empty() {
                s.checkpoint()?;
            }
            Ok::<_, harness_store::StoreError>(gone)
        })
        .await?;
    storage::unlink(&m, gone).await;
    artifacts::notify(&m, row.owner_id.clone(), row.shared());
    Ok(Json(json!({ "ok": true })))
}

/// 302 to the newest ready version's download (scripts: `curl -L` with the
/// API token). Reached by link (a browser download), the redirect carries
/// a fresh link of its own.
async fn latest_download(
    State(m): State<Master>,
    headers: HeaderMap,
    uri: Uri,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let by_link = crate::auth::authenticate(&m, &headers).await.is_none();
    let who = require_bytes(&m, &headers, &uri).await?;
    let row = visible(&m, &who, &id).await?;
    let aid = row.artifact_id.clone();
    let latest = m
        .inner
        .mirror
        .call(move |s| s.artifact_versions(&aid))
        .await?
        .into_iter()
        .find(|v| v.status == "ready")
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "no version is ready yet".into()))?;
    let to = format!(
        "/api/artifacts/{}/versions/{}/download",
        row.artifact_id, latest.version_id
    );
    let to = match by_link {
        true => crate::auth::relink(&m, &headers, &to).ok_or_else(unknown)?,
        false => to,
    };
    Ok((StatusCode::FOUND, [(header::LOCATION, to)]).into_response())
}

async fn version(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, vid)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let (row, v) = visible_version(&m, &who, &id, &vid).await?;
    let view = View::new(&m, &who, &[&v]).await?;
    Ok(Json(view.version(&m, &row, &v)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionPatch {
    #[serde(default)]
    pinned: Option<bool>,
    #[serde(default)]
    no_thumb: Option<bool>,
}

async fn version_update(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, vid)): Path<(String, String)>,
    Json(b): Json<VersionPatch>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let (row, v) = visible_version(&m, &who, &id, &vid).await?;
    if let Some(pinned) = b.pinned {
        owner_only(&who, &row)?;
        let max = u64::from(artifacts::config(&m).max_pinned);
        let (aid, vid) = (row.artifact_id.clone(), v.version_id.clone());
        let res = m
            .inner
            .mirror
            .call(move |s| s.artifact_pin(&aid, &vid, pinned, max))
            .await;
        if let Err(harness_store::StoreError::Conflict(e)) = &res {
            return Err(ApiError(StatusCode::CONFLICT, e.clone()));
        }
        res?;
    }
    if let Some(no_thumb) = b.no_thumb {
        let blob = blob_owner(&who, &v)?;
        m.inner
            .mirror
            .call(move |s| s.preview_set_no_thumb(&blob, no_thumb))
            .await?;
    }
    artifacts::notify(&m, row.owner_id.clone(), row.shared());
    version(State(m), headers, Path((id, vid))).await
}

async fn version_delete(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, vid)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let (row, v) = visible_version(&m, &who, &id, &vid).await?;
    owner_only(&who, &row)?;
    let (aid, vid) = (row.artifact_id.clone(), v.version_id.clone());
    let gone = m
        .inner
        .mirror
        .call(move |s| {
            let gone = s.artifact_version_delete(&aid, &vid)?.unwrap_or_default();
            if !gone.is_empty() {
                s.checkpoint()?;
            }
            Ok::<_, harness_store::StoreError>(gone)
        })
        .await?;
    storage::unlink(&m, gone).await;
    artifacts::notify(&m, row.owner_id.clone(), row.shared());
    Ok(Json(json!({ "ok": true })))
}

/// Send a ready version's file under the T17 rules.
async fn send(
    m: &Master,
    headers: &HeaderMap,
    v: &VersionRow,
    inline: bool,
) -> ApiResult<Response> {
    let blob = v
        .blob
        .as_ref()
        .filter(|_| v.status == "ready")
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "this version is not ready".into()))?;
    let st = storage::get(m).ok_or_else(unknown)?;
    let path = st.path(&blob.blob_id).ok_or_else(unknown)?;
    let id = blob.blob_id.clone();
    let key = m
        .inner
        .mirror
        .call(move |s| s.blob_key(&id))
        .await?
        .ok_or_else(unknown)?;
    Ok(crate::blobs::serve(
        headers,
        crate::blobs::Serve {
            blob,
            path,
            key,
            name: &v.filename,
            inline,
        },
    )
    .await)
}

async fn download(
    State(m): State<Master>,
    headers: HeaderMap,
    uri: Uri,
    Path((id, vid)): Path<(String, String)>,
) -> ApiResult<Response> {
    let who = require_bytes(&m, &headers, &uri).await?;
    let (_, v) = visible_version(&m, &who, &id, &vid).await?;
    send(&m, &headers, &v, false).await
}

/// Only kinds a browser may show (`display` inline or img); never HTML,
/// SVG outside `<img>`, PDF or anything else.
async fn preview(
    State(m): State<Master>,
    headers: HeaderMap,
    uri: Uri,
    Path((id, vid)): Path<(String, String)>,
) -> ApiResult<Response> {
    let who = require_bytes(&m, &headers, &uri).await?;
    let (_, v) = visible_version(&m, &who, &id, &vid).await?;
    let shown = v
        .blob
        .as_ref()
        .is_some_and(|b| matches!(b.display.as_str(), "inline" | "img"));
    if v.status == "ready" && !shown {
        return Err(ApiError(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "this file has no preview; download it".into(),
        ));
    }
    send(&m, &headers, &v, true).await
}

/// The blob's owner, who alone sets its preview hints and thumbnail (a
/// member who can see a shared artifact can never plant one).
fn blob_owner(who: &Principal, v: &VersionRow) -> ApiResult<String> {
    match &v.blob {
        Some(b) if who.user_id == b.owner_id => Ok(b.blob_id.clone()),
        Some(_) => Err(ApiError(
            StatusCode::FORBIDDEN,
            "only the file's owner can change its preview".into(),
        )),
        None => Err(ApiError(
            StatusCode::NOT_FOUND,
            "this version is not ready".into(),
        )),
    }
}

async fn thumb(
    State(m): State<Master>,
    headers: HeaderMap,
    uri: Uri,
    Path((id, vid)): Path<(String, String)>,
) -> ApiResult<Response> {
    let who = require_bytes(&m, &headers, &uri).await?;
    let (_, v) = visible_version(&m, &who, &id, &vid).await?;
    let blob = v
        .blob
        .as_ref()
        .map(|b| b.blob_id.clone())
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "no thumbnail".into()))?;
    let (mime, bytes) = m
        .inner
        .mirror
        .call(move |s| s.preview_thumb(&blob))
        .await?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "no thumbnail".into()))?;
    Ok(crate::blobs::serve_thumb(&headers, &mime, bytes))
}

async fn put_thumb(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, vid)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let (row, v) = visible_version(&m, &who, &id, &vid).await?;
    let blob = blob_owner(&who, &v)?;
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
    m.inner
        .mirror
        .call(move |s| s.preview_put_thumb(&blob, mime, &body))
        .await?;
    artifacts::notify(&m, row.owner_id.clone(), row.shared());
    Ok(Json(json!({ "ok": true })))
}

/// The chat's staged files, for its cards and failed "Save as artifact"
/// items.
async fn session_list(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let row = visible_session(&m, &who, &id).await?;
    let sid = row.session_id.to_string();
    let versions = m
        .inner
        .mirror
        .call(move |s| s.artifact_session_versions(&sid))
        .await?;
    let out: Vec<Value> = versions
        .iter()
        .map(|v| {
            let path = serde_json::from_str::<Value>(&v.meta)
                .ok()
                .and_then(|m| m.get("source_path").cloned());
            let progress = (v.status == "pending")
                .then(|| crate::artifact_pull::progress(&m, &v.version_id))
                .flatten()
                .map(|(received, total)| json!({ "received": received, "total": total }));
            json!({
                "call_id": v.call_id,
                "stage_id": v.stage_id,
                "version_id": v.version_id,
                "artifact_id": v.artifact_id,
                "status": v.status,
                "error": v.error,
                "progress": progress,
                "by_user": v.by_user,
                "path": path,
            })
        })
        .collect();
    Ok(Json(json!(out)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveBody {
    path: String,
    #[serde(default)]
    name: Option<String>,
}

fn conflict(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::CONFLICT, msg.into())
}

/// "Save as artifact": the node checks the path and acks at once; the copy
/// ends in the chat's log (`artifact_staged` or `artifact_stage_failed`).
/// No approval: the user clicked.
async fn session_save(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<SaveBody>,
) -> ApiResult<Response> {
    let who = require(&m, &headers).await?;
    let row = visible_session(&m, &who, &id).await?;
    agentic_gate(&m, &who, &row).await?;
    if row.profile.as_deref() != Some("agentic") || row.incognito {
        return Err(conflict("only agentic chats can save files as artifacts"));
    }
    if !artifacts::enabled(&m) {
        return Err(conflict("Artifacts are turned off on this server"));
    }
    if b.path.trim().is_empty() {
        return Err(bad("the path is empty"));
    }
    let name = b
        .name
        .as_deref()
        .filter(|n| !n.trim().is_empty())
        .map(harness_core::artifacts::check_name)
        .transpose()
        .map_err(bad)?;
    if !m.is_connected(&row.node_id) {
        return Err(conflict("the chat's worker node is offline"));
    }
    if !m.node_has(&row.node_id, CAP_ARTIFACTS) {
        return Err(conflict("Update the worker node to save files"));
    }
    let (sid, path) = (row.session_id, b.path.clone());
    let res = m
        .send_command(&row.node_id, |req_id| MasterToNode::ArtifactStage {
            req_id,
            session_id: sid,
            path,
            name,
        })
        .await;
    match res {
        Ok(()) => Ok((StatusCode::ACCEPTED, Json(json!({ "requested": true }))).into_response()),
        Err(CommandError::Rejected(e)) => Err(conflict(e)),
        Err(CommandError::NodeOffline(_) | CommandError::Timeout) => {
            Err(conflict("the chat's worker node is offline"))
        }
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_and_cursors() {
        assert_eq!(kinds("builds").unwrap(), ["binary", "package"]);
        assert_eq!(kinds("media").unwrap().len(), 3);
        assert!(kinds("images").is_none());
        let id = "0193f1c2a4b57d6e8f9012345678abcd";
        assert_eq!(parse_cursor(&format!("5_{id}")), Some((5, id.to_string())));
        assert_eq!(parse_cursor("5_x"), None);
    }
}
