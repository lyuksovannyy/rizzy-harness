//! `POST /api/attachments`: what a person attaches to a message.
//!
//! Text, Markdown, code, CSV, JSON and PDF are read as text on the master
//! and travel in the message, as they always did (at most 10 MiB). Any
//! other file (a zip, docx, xlsx, a binary), and a text file or PDF over
//! that size, is kept as the owner's blob for a coding chat: the node
//! saves it into the chat's folder when the message is sent (PLAN.md T94,
//! `crate::uploads`). A chat without tools refuses it with the reason. The
//! body is streamed: only the first 10 MiB is ever held, to see whether it
//! is text, and a file for the folder goes to disk as it arrives, under
//! the node's size limit.

use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::routing::post;
use futures::StreamExt;
use harness_store::blobs::NewBlob;
use harness_store::chat_uploads::{NewUpload, Put};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, require};
use crate::attachments::{self, ExtractError, MAX_UPLOAD};
use crate::{Master, storage, uploads};

/// A body that sends nothing for this long is cut off.
const IDLE: Duration = Duration::from_secs(30);

pub(super) fn routes() -> Router<Master> {
    Router::new().route(
        "/api/attachments",
        // Streamed: the limits are checked by hand, as the bytes arrive.
        post(upload).layer(DefaultBodyLimit::disable()),
    )
}

#[derive(Deserialize)]
struct UploadQuery {
    name: String,
    /// The file's own type as the browser said (informational).
    #[serde(default, rename = "type")]
    mime: Option<String>,
    /// The chat it is for, when it exists.
    #[serde(default)]
    session: Option<String>,
    /// For a chat that does not exist yet (the home composer): it will be
    /// a coding chat. Checked again at send.
    #[serde(default)]
    agentic: Option<String>,
}

/// Where a file the model cannot read may go.
enum Target {
    /// A chat without tools, or no word of a folder.
    Plain,
    /// A coding chat: its node's limit when the chat is known.
    Folder { max: u64, session: Option<String> },
}

fn too_large(max: u64) -> ApiError {
    ApiError(
        StatusCode::PAYLOAD_TOO_LARGE,
        format!("Files are at most {} MB.", max.div_ceil(1024 * 1024)),
    )
}

async fn target(m: &Master, who: &crate::auth::Principal, q: &UploadQuery) -> ApiResult<Target> {
    let Some(id) = q.session.as_deref().filter(|s| !s.is_empty()) else {
        return Ok(if matches!(q.agentic.as_deref(), Some("1" | "true")) {
            Target::Folder {
                max: uploads::DEFAULT_MAX,
                session: None,
            }
        } else {
            Target::Plain
        });
    };
    let row = super::acting_session(m, who, id).await?;
    if row.parent_id.is_some() {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "a sub-agent's chat takes no attached files".into(),
        ));
    }
    if uploads::refusal(&row).is_some() {
        return Ok(Target::Plain);
    }
    // The node's own number; the default for one that announces none (the
    // send refuses a node that cannot save files, and says to update it).
    let max =
        uploads::limits(m, &row.node_id).map_or(uploads::DEFAULT_MAX, |l| l.max_mb * 1024 * 1024);
    Ok(Target::Folder {
        max,
        session: Some(row.session_id.to_string()),
    })
}

async fn upload(
    State(m): State<Master>,
    headers: HeaderMap,
    Query(q): Query<UploadQuery>,
    body: Body,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let target = target(&m, &who, &q).await?;
    let text_max = MAX_UPLOAD as u64;
    let ceiling = match &target {
        Target::Folder { max, .. } => *max,
        Target::Plain => text_max,
    };
    let length = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if length.is_some_and(|l| l > ceiling) {
        return Err(too_large(ceiling));
    }
    // The first 10 MiB: enough to tell text from a file, never more in memory.
    let mut stream = body.into_data_stream();
    let mut head: Vec<u8> = Vec::new();
    let mut more = false;
    loop {
        match tokio::time::timeout(IDLE, stream.next()).await {
            Err(_) => {
                return Err(ApiError(
                    StatusCode::REQUEST_TIMEOUT,
                    "The upload stalled; try again.".into(),
                ));
            }
            Ok(None) => break,
            Ok(Some(Err(_))) => {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "The upload was interrupted.".into(),
                ));
            }
            Ok(Some(Ok(b))) => {
                head.extend_from_slice(&b);
                if head.len() as u64 > text_max {
                    more = true;
                    break;
                }
            }
        }
    }
    if more {
        // Past what is read as text: only a coding chat takes it.
        let Target::Folder { max, session } = target else {
            return Err(too_large(text_max));
        };
        let prefix =
            futures::stream::once(async move { Ok::<Bytes, axum::Error>(Bytes::from(head)) });
        let body = Body::from_stream(prefix.chain(stream));
        return keep(&m, &who, &q, session, length, body, max).await;
    }
    let keep_copy = matches!(target, Target::Folder { .. }).then(|| head.clone());
    match attachments::extract(&q.name, head, m.pdf_reader()).await {
        Ok(a) => store_text(&m, &who, a).await,
        Err(e) if e.busy() => Err(ApiError(StatusCode::SERVICE_UNAVAILABLE, e.to_string())),
        // Not text, or a PDF that cannot be read: a coding chat keeps it
        // as a file (never an empty one: the node saves none).
        Err(e @ (ExtractError::Binary | ExtractError::Pdf(_))) => match (target, keep_copy) {
            (Target::Folder { max, session }, Some(bytes)) => {
                let len = bytes.len() as u64;
                keep(&m, &who, &q, session, Some(len), Body::from(bytes), max).await
            }
            _ if matches!(e, ExtractError::Binary) => Err(ApiError(
                StatusCode::UNPROCESSABLE_ENTITY,
                attachments::NO_TOOLS.into(),
            )),
            _ => Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, e.to_string())),
        },
        Err(e) => Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, e.to_string())),
    }
}

/// A text attachment, as it always was: the extracted text waits in the
/// store for the message that carries it.
async fn store_text(
    m: &Master,
    who: &crate::auth::Principal,
    a: harness_core::event::Attachment,
) -> ApiResult<Json<Value>> {
    use attachments::{MAX_UNSENT, MAX_UNSENT_BYTES};
    let id = harness_core::new_id();
    let (aid, owner, att) = (id.clone(), who.user_id.clone(), a.clone());
    let stored = m
        .inner
        .mirror
        .call(move |s| s.attachment_put(&aid, owner.as_deref(), &att, MAX_UNSENT, MAX_UNSENT_BYTES))
        .await?;
    if !stored {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            format!(
                "too many files waiting to be sent (at most {MAX_UNSENT}); send or remove some first"
            ),
        ));
    }
    Ok(Json(json!({
        "attachment_id": id,
        "name": a.name,
        "mime": a.mime,
        "chars": a.text.chars().count(),
        "truncated": a.truncated,
    })))
}

/// Keep a file for the chat's folder: streamed into the blob store under
/// `max` and the account's room, then recorded as the owner's unsent
/// upload.
async fn keep(
    m: &Master,
    who: &crate::auth::Principal,
    q: &UploadQuery,
    session: Option<String>,
    length: Option<u64>,
    body: Body,
    max: u64,
) -> ApiResult<Json<Value>> {
    let refused = |(code, msg): storage::Refusal| ApiError(code, msg);
    let st = storage::get(m).ok_or_else(|| {
        ApiError(
            StatusCode::NOT_FOUND,
            "this server keeps no files, so it cannot take this one".into(),
        )
    })?;
    let Some(_slot) = st.upload_slot(who.user_id.as_deref()) else {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many uploads at once; try again in a moment.".into(),
        ));
    };
    let written = storage::receive_capped(m, who.user_id.as_deref(), length, body, max)
        .await
        .map_err(refused)?;
    if written.size == 0 {
        let _ = std::fs::remove_file(&written.path);
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "the file is empty".into(),
        ));
    }
    let name = attachments::clean_name(&q.name);
    let mime = q
        .mime
        .as_deref()
        .map(|t| harness_core::text::clean_line(t, 80))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "application/octet-stream".into());
    let id = harness_core::new_id();
    let up = NewUpload {
        upload_id: id.clone(),
        owner_id: who.user_id.clone(),
        name: name.clone(),
        mime: mime.clone(),
        session_id: session,
    };
    let blob = NewBlob {
        blob_id: written.blob_id.clone(),
        owner_id: who.user_id.clone(),
        sha256: written.sha256.clone(),
        size: written.size,
        kind: "other".into(),
        format: "bin".into(),
        mime: "application/octet-stream".into(),
        display: "download".into(),
        key: written.key.clone(),
        width: None,
        height: None,
    };
    let size = written.size;
    let limits = storage::config(m).limits(who.user_id.as_deref());
    let put = storage::commit(m, written, move |s| {
        let r = s.chat_upload_put(&up, &blob, limits, uploads::MAX_UNSENT)?;
        let keep = matches!(r, Put::Added { new_blob: true });
        Ok((r, keep))
    })
    .await
    .map_err(refused)?;
    match put {
        Put::Added { .. } => Ok(Json(json!({
            "attachment_id": id,
            "name": name,
            "mime": mime,
            "bytes": size,
            "saved": true,
        }))),
        Put::TooMany => Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            format!(
                "too many files waiting to be sent (at most {}); send or remove some first",
                uploads::MAX_UNSENT
            ),
        )),
        Put::OverQuota(o) => Err(ApiError(
            StatusCode::INSUFFICIENT_STORAGE,
            storage::full_message(&o),
        )),
    }
}
