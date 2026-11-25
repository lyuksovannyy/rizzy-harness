//! `POST /api/attachments`: what a person attaches to a message.
//!
//! Text, Markdown, code, CSV, JSON and PDF are read as text on the master
//! and travel in the message, as they always did (at most 10 MiB). Any
//! other file (a zip, docx, xlsx, a binary), and a text file or PDF over
//! that size, is kept as the owner's blob for a coding chat: the node
//! saves it into the chat's folder when the message is sent (PLAN.md T94,
//! `crate::uploads`). A chat without tools keeps it too when its agent has
//! the switch "Read attached files": the master holds it, the agent reads
//! it through `attached_file`, and nothing is written to any folder (T95,
//! `crate::attached`); otherwise it is refused with the reason. The
//! body is streamed: only the first 10 MiB is ever held, to see whether it
//! is text, and a file for the folder goes to disk as it arrives, under
//! the node's size limit.

use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::routing::post;
use futures::StreamExt;
use harness_store::blobs::NewBlob;
use harness_store::chat_uploads::{NewUpload, Put};
use serde::Deserialize;
use serde_json::{Value, json};

use super::caller::User;
use super::{ApiError, ApiResult};
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
pub(super) struct UploadQuery {
    pub(super) name: String,
    /// The file's own type as the browser said (informational).
    #[serde(default, rename = "type")]
    pub(super) mime: Option<String>,
    /// The chat it is for, when it exists.
    #[serde(default)]
    pub(super) session: Option<String>,
    /// For a chat that does not exist yet (the home composer): it will be
    /// a coding chat. Checked again at send.
    #[serde(default)]
    pub(super) agentic: Option<String>,
    /// The agent the next message goes to (a plain chat keeps a file only
    /// for an agent with the switch); the chat's own when absent. Checked
    /// again at send.
    #[serde(default)]
    pub(super) agent: Option<String>,
}

/// Where a file the model cannot read may go.
pub(super) enum Target {
    /// Nowhere: the reason it is refused with.
    Plain(String),
    /// A coding chat: its node's limit when the chat is known. `kept`: a
    /// plain chat keeps it for `attached_file` (T95) instead of a folder.
    Folder(Dest),
}

/// Where a kept file goes.
pub(super) struct Dest {
    pub(super) max: u64,
    pub(super) session: Option<String>,
    pub(super) kept: bool,
}

/// A plain chat (or one to come) keeps the file when the agent it goes to
/// has the switch. An agent the server's own list does not know (one of a
/// workspace) is let through: the send decides.
async fn kept_or_plain(
    m: &Master,
    row: Option<&harness_store::SessionRow>,
    agent: Option<&str>,
) -> Target {
    let plain = || Target::Plain(attachments::NO_TOOLS.into());
    if let Some(row) = row {
        if row.incognito {
            return Target::Plain("a temporary chat keeps no attached files".into());
        }
        if !m.node_has(&row.node_id, harness_protocol::CAP_ATTACHED) {
            return plain();
        }
    }
    if storage::get(m).is_none() {
        return plain();
    }
    // No chat yet: the runtime that will run it must offer the tool.
    if row.is_none() && !m.chat_runtime_has(harness_protocol::CAP_ATTACHED) {
        return plain();
    }
    let r = m
        .resolver_for(row.and_then(|r| r.workspace_id.as_deref()))
        .await;
    let id = agent
        .map(str::to_string)
        .or_else(|| row.and_then(|r| r.agent_id.clone()))
        .or_else(|| {
            r.default_for(harness_core::event::Profile::Chat)
                .ok()
                .map(str::to_string)
        });
    if let Some(a) = id.as_deref().and_then(|id| r.agents().get(id))
        && !a.features.attached_files
    {
        return Target::Plain(uploads::switch_off(&a.name));
    }
    Target::Folder(Dest {
        max: crate::attached::MAX_FILE,
        session: row.map(|r| r.session_id.to_string()),
        kept: true,
    })
}

fn too_large(max: u64) -> ApiError {
    ApiError(
        StatusCode::PAYLOAD_TOO_LARGE,
        format!("Files are at most {} MB.", max.div_ceil(1024 * 1024)),
    )
}

pub(super) async fn target(
    m: &Master,
    who: &crate::auth::Principal,
    q: &UploadQuery,
) -> ApiResult<Target> {
    let Some(id) = q.session.as_deref().filter(|s| !s.is_empty()) else {
        return Ok(if matches!(q.agentic.as_deref(), Some("1" | "true")) {
            Target::Folder(Dest {
                max: uploads::DEFAULT_MAX,
                session: None,
                kept: false,
            })
        } else {
            kept_or_plain(m, None, q.agent.as_deref()).await
        });
    };
    let row = super::acting_session(m, who, id).await?;
    if row.parent_id.is_some() {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "a sub-agent's chat takes no attached files".into(),
        ));
    }
    if uploads::lane(&row) == uploads::Lane::Attached {
        return Ok(kept_or_plain(m, Some(&row), q.agent.as_deref()).await);
    }
    // The node's own number; the default for one that announces none (the
    // send refuses a node that cannot save files, and says to update it).
    let max =
        uploads::limits(m, &row.node_id).map_or(uploads::DEFAULT_MAX, |l| l.max_mb * 1024 * 1024);
    Ok(Target::Folder(Dest {
        max,
        session: Some(row.session_id.to_string()),
        kept: false,
    }))
}

async fn upload(
    User(who): User,
    State(m): State<Master>,
    headers: HeaderMap,
    Query(q): Query<UploadQuery>,
    body: Body,
) -> ApiResult<Json<Value>> {
    let target = target(&m, &who, &q).await?;
    let text_max = MAX_UPLOAD as u64;
    let ceiling = match &target {
        Target::Folder(d) => d.max,
        Target::Plain(_) => text_max,
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
        // Past what is read as text, which is also the most any file may be.
        return Err(too_large(ceiling.min(text_max)));
    }
    let keep_copy = matches!(target, Target::Folder(_)).then(|| head.clone());
    // One helper at a time per account (T96): a second PDF or zip of the same
    // account waits here, holding no server-wide slot, so one account cannot
    // keep both slots busy and starve the others.
    let needs_helper = crate::pdf::is_pdf(&head) || crate::doc::is_zip(&head);
    let (_own, busy) = match m.pdf_reader().filter(|_| needs_helper) {
        Some(r) => match r
            .account_slot(who.user_id.as_deref().unwrap_or("api-token"))
            .await
        {
            Ok(p) => (Some(p), None),
            Err(_) if crate::pdf::is_pdf(&head) => {
                (None, Some(ExtractError::Pdf(crate::pdf::PdfError::Busy)))
            }
            Err(_) => (None, Some(ExtractError::Unsure(crate::pdf::DocError::Busy))),
        },
        None => (None, None),
    };
    let result = match busy {
        Some(e) => Err(e),
        None => attachments::extract(&q.name, head, m.pdf_reader()).await,
    };
    match result {
        Ok(a) => {
            let document = attachments::is_document_mime(&a.mime);
            let stored = store_text(&m, &who, a).await?;
            // A document is read as text and its file is kept as well, where
            // this chat keeps files; where it cannot, the text alone goes and
            // nothing is refused (X.55, T96).
            let Some(mut body) = stored.0.as_object().cloned().filter(|_| document) else {
                return Ok(stored);
            };
            if let (Target::Folder(dest), Some(bytes), Some(id)) = (
                target,
                keep_copy,
                body.get("attachment_id").and_then(Value::as_str),
            ) {
                let len = bytes.len() as u64;
                let file_id = attachments::file_id_of(id);
                let kept = keep(
                    &m,
                    &who,
                    &q,
                    dest,
                    Some(len),
                    Body::from(bytes),
                    AsFile {
                        upload_id: Some(file_id),
                        note: None,
                    },
                )
                .await
                .is_ok();
                body.insert("file_kept".into(), json!(kept));
            }
            Ok(Json(Value::Object(body)))
        }
        // A zip the reader could not judge (busy, timed out, crashed, could
        // not confine itself) is a plain file where files are kept, as it was
        // before documents were read: no failure, no sentence (T96).
        Err(ExtractError::Unsure(_)) if keep_copy.is_some() => match (target, keep_copy) {
            (Target::Folder(dest), Some(bytes)) => {
                let len = bytes.len() as u64;
                keep(
                    &m,
                    &who,
                    &q,
                    dest,
                    Some(len),
                    Body::from(bytes),
                    AsFile::default(),
                )
                .await
            }
            _ => Err(ApiError(
                StatusCode::UNPROCESSABLE_ENTITY,
                "the file could not be kept".into(),
            )),
        },
        Err(e) if e.busy() => Err(ApiError(StatusCode::SERVICE_UNAVAILABLE, e.to_string())),
        // Not text, or a PDF or document that cannot be read: a coding chat
        // keeps it as a file (never an empty one: the node saves none); a
        // document says that its text could not be read.
        Err(e @ (ExtractError::Binary | ExtractError::Pdf(_) | ExtractError::Doc(_))) => {
            match (target, keep_copy) {
                (Target::Folder(dest), Some(bytes)) => {
                    let len = bytes.len() as u64;
                    let note = matches!(e, ExtractError::Doc(_)).then(|| e.to_string());
                    keep(
                        &m,
                        &who,
                        &q,
                        dest,
                        Some(len),
                        Body::from(bytes),
                        AsFile {
                            upload_id: None,
                            note,
                        },
                    )
                    .await
                }
                (Target::Plain(why), _) if matches!(e, ExtractError::Binary) => {
                    Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, why))
                }
                _ => Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, e.to_string())),
            }
        }
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

/// What a kept file is besides a file.
#[derive(Default)]
pub(super) struct AsFile {
    /// The id to store it under (a document's own file:
    /// `attachments::file_id_of`).
    pub(super) upload_id: Option<String>,
    /// A sentence for the person: why this file is only a file.
    pub(super) note: Option<String>,
}

/// Keep a file for the chat's folder: streamed into the blob store under
/// `max` and the account's room, then recorded as the owner's unsent
/// upload.
pub(super) async fn keep(
    m: &Master,
    who: &crate::auth::Principal,
    q: &UploadQuery,
    dest: Dest,
    length: Option<u64>,
    body: Body,
    as_file: AsFile,
) -> ApiResult<Json<Value>> {
    let AsFile { upload_id, note } = as_file;
    let Dest { max, session, kept } = dest;
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
        let _ = tokio::fs::remove_file(&written.path).await;
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
    let id = upload_id.unwrap_or_else(harness_core::new_id);
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
    let said = |mut v: Value| {
        if let Some(n) = &note {
            v["note"] = json!(n);
        }
        Ok(Json(v))
    };
    match put {
        Put::Added { .. } if kept => said(json!({
            "attachment_id": id,
            "name": name,
            "mime": mime,
            "bytes": size,
            "kept": true,
        })),
        Put::Added { .. } => said(json!({
            "attachment_id": id,
            "name": name,
            "mime": mime,
            "bytes": size,
            "saved": true,
        })),
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
