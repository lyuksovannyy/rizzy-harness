//! What happens inside a chat's turns: sending a message, cancelling,
//! switching the agent, answering an approval, restoring a turn and picking
//! a variant. New per-turn routes go here.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use harness_core::policy::Decision;
use serde::Deserialize;
use serde_json::json;

use super::caller::{self, Caller, User};
use super::{ApiError, ApiResult, acting_session, agentic_gate, files};
use crate::{CommandError, Master};

pub(super) fn routes() -> Router<Master> {
    Router::new()
        .route("/api/sessions/{id}/prompt", post(prompt))
        .route("/api/sessions/{id}/cancel", post(cancel))
        .route("/api/sessions/{id}/agent", post(set_agent))
        .route("/api/sessions/{id}/variants", post(select_variant))
        .route(
            "/api/sessions/{id}/turns/{turn}/restore",
            post(restore_turn),
        )
        .route("/api/sessions/{id}/approvals/{approval_id}", post(approval))
}

#[derive(Deserialize)]
struct PromptBody {
    text: String,
    /// Run this turn as another agent (mention popup); the session's agent
    /// stays unchanged.
    #[serde(default)]
    agent_id: Option<String>,
    /// Reasoning effort for this message only (if the model supports it).
    #[serde(default)]
    effort: Option<String>,
    /// Upload ids from `POST /api/attachments`; consumed by this message.
    #[serde(default)]
    attachments: Vec<String>,
    /// Parallel answers: model ids (2-4).
    #[serde(default)]
    compare: Vec<String>,
    /// An answer to a choices or form block; `text` is then an optional
    /// note.
    #[serde(default)]
    reply: Option<harness_core::event::FormReply>,
    /// Media item ids of images for this message (`media::prompt_images`).
    #[serde(default)]
    images: Vec<String>,
    /// Paths the person mentioned with `@path` (`files::check_mentions`).
    #[serde(default)]
    files: Vec<String>,
}

/// The files for the folder a refused message held can be sent again.
async fn give_back_files(m: &Master, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    let ids = ids.to_vec();
    if let Err(e) = m
        .inner
        .mirror
        .call(move |s| s.chat_uploads_give_back(&ids))
        .await
    {
        tracing::warn!(error = %e, "could not give a refused message's files back");
    }
}

async fn prompt(
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
    Json(b): Json<PromptBody>,
) -> ApiResult<Json<serde_json::Value>> {
    if b.text.trim().is_empty() && b.reply.is_none() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "empty prompt".into()));
    }
    if let Some(r) = &b.reply {
        check_reply_body(r, &b)?;
    }
    // One count for everything a message carries: text, PDFs and saved
    // files (the attachments) and images together.
    if b.attachments.len() + b.images.len() > crate::attachments::MAX_PER_MESSAGE {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            format!(
                "at most {} files and images per message",
                crate::attachments::MAX_PER_MESSAGE
            ),
        ));
    }
    let row = acting_session(&m, &who, &id).await?;
    // Before the uploads are taken, so a refusal leaves them for later.
    agentic_gate(&m, &who, &row).await?;
    files::check_mentions(&m, &row, &b.files)?;
    let images = crate::media::prompt_images(&m, &who, &row, &b.images)
        .await
        .map_err(|(c, e)| ApiError(c, e))?;
    // A chat a routine started from an external event stays untrusted for
    // every turn the owner sends there.
    let flags = crate::routines::on_owner_prompt(&m, row.session_id).await;
    let sid = row.session_id;
    // The take, the send and what is done with the uploads after it run in
    // a task of their own: a request the route's time limit (or a client
    // that went away) drops cannot cancel the bookkeeping half way and leave
    // a file held for a message the node never got (T94).
    let task = tokio::spawn(async move {
        // The ids name text attachments and files for the folder alike; the
        // files are held (not consumed) until the node has taken the message.
        let (file_ids, text_ids): (Vec<String>, Vec<String>) = if b.attachments.is_empty() {
            (vec![], vec![])
        } else {
            let ids = b.attachments.clone();
            let files = m
                .inner
                .mirror
                .call(move |s| s.chat_uploads_among(&ids))
                .await?;
            let text = b
                .attachments
                .iter()
                .filter(|i| !files.contains(i))
                .cloned()
                .collect();
            (files, text)
        };
        let unknown = || {
            ApiError(
                StatusCode::BAD_REQUEST,
                "unknown or already used attachment; upload it again".into(),
            )
        };
        let files = if file_ids.is_empty() {
            Vec::new()
        } else {
            let (ids, owner) = (file_ids.clone(), who.user_id.clone());
            m.inner
                .mirror
                .call(move |s| s.chat_uploads_take(&ids, owner.as_deref()))
                .await?
                .ok_or_else(unknown)?
        };
        let attachments = if text_ids.is_empty() {
            Vec::new()
        } else {
            let (ids, owner) = (text_ids.clone(), who.user_id.clone());
            let taken = m
                .inner
                .mirror
                .call(move |s| s.attachments_take_owned(&ids, owner.as_deref()))
                .await?;
            match taken {
                Some(t) => t,
                None => {
                    give_back_files(&m, &file_ids).await;
                    return Err(unknown());
                }
            }
        };
        // A document's text came with its file (X.55): the message that
        // takes the text takes the file too, when it can still be taken.
        let optional = if text_ids.is_empty() {
            Vec::new()
        } else {
            let ids: Vec<String> = text_ids
                .iter()
                .map(|i| crate::attachments::file_id_of(i))
                .collect();
            let owner = who.user_id.clone();
            m.inner
                .mirror
                .call(move |s| s.chat_uploads_take_available(&ids, owner.as_deref()))
                .await?
        };
        let file_ids: Vec<String> = file_ids
            .into_iter()
            .chain(optional.iter().map(|u| u.upload_id.clone()))
            .collect();
        let taken = (!attachments.is_empty()).then(|| (text_ids.clone(), attachments.clone()));
        let attachments = attachments.into_iter().map(|(a, _)| a).collect();
        let sent = m
            .prompt(
                sid,
                b.text,
                crate::PromptOpts {
                    agent_id: b.agent_id,
                    effort: b.effort,
                    context: Some(
                        crate::prefs::Prefs::load(&m, who.user_id.as_deref())
                            .await
                            .for_turn(flags.untrusted)
                            .model_context(
                                &who.username,
                                jiff::Timestamp::now(),
                                row.profile.as_deref() == Some("chat"),
                            ),
                    ),
                    attachments,
                    compare: b.compare,
                    reply: b.reply,
                    untrusted: flags.untrusted,
                    images,
                    callback: vec![],
                    by_owner: who.user_id.is_some() && who.user_id == row.owner_id,
                    // Never from a request: only the routine runner sets it.
                    routine_run: None,
                    files: b.files,
                    uploads: files,
                    optional_uploads: optional,
                    retry: None,
                    retry_images: false,
                },
            )
            .await;
        // A prompt the node certainly did not take gives its uploads back, so
        // "send this again" works (review A19-8): a refusal (a stopping node,
        // a turn already running) or one never sent (the node away, too
        // large). Not after a timeout: the node may have taken it and only its
        // answer was lost, and giving them back could let one file go twice.
        // The files for the folder follow the same rule; once the node took
        // the message they are done.
        match &sent {
            Ok(()) => {
                if !file_ids.is_empty() {
                    let ids = file_ids.clone();
                    if let Err(e) = m
                        .inner
                        .mirror
                        .call(move |s| s.chat_uploads_done(&ids))
                        .await
                    {
                        tracing::warn!(error = %e, "could not clear the files a message carried");
                    }
                }
            }
            Err(e) if !matches!(e, CommandError::Timeout) => {
                give_back_files(&m, &file_ids).await;
                if let Some((ids, atts)) = taken
                    && let Err(e) = m
                        .inner
                        .mirror
                        .call(move |s| s.attachments_give_back(&ids, &atts))
                        .await
                {
                    tracing::warn!(error = %e, "could not give a refused message's uploads back");
                }
            }
            Err(_) => {}
        }
        sent?;
        Ok::<(), ApiError>(())
    });
    task.await.map_err(|e| {
        tracing::error!(error = %e, "a prompt's task failed");
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, "the send failed".into())
    })??;
    Ok(Json(json!({ "ok": true })))
}

/// What an answer may come with. Whether it answers anything is the node's
/// to say, against its log; this only keeps the request plain.
fn check_reply_body(r: &harness_core::event::FormReply, b: &PromptBody) -> ApiResult<()> {
    let bad = |m: &str| Err(ApiError(StatusCode::BAD_REQUEST, m.into()));
    if !b.attachments.is_empty() {
        return bad("an answer cannot carry attachments");
    }
    if !b.compare.is_empty() {
        return bad("an answer cannot ask for parallel answers");
    }
    if !b.images.is_empty() {
        return bad("an answer cannot carry images");
    }
    if !b.files.is_empty() {
        return bad("an answer cannot mention files");
    }
    if harness_core::ui::too_large(r) {
        return bad(harness_core::ui::TOO_LARGE);
    }
    Ok(())
}

async fn cancel(
    Caller(who): Caller,
    State(m): State<Master>,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    // Stopping work is always allowed, even before a pending password
    // change (Gate 3 review P-7): only the owner's own chat, as ever.
    let sid = acting_session(&m, &who, &id).await?.session_id;
    m.cancel(sid).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct AgentBody {
    agent_id: String,
}

async fn set_agent(
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
    Json(b): Json<AgentBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let row = acting_session(&m, &who, &id).await?;
    agentic_gate(&m, &who, &row).await?;
    m.set_agent(row.session_id, &b.agent_id).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ApprovalBody {
    decision: Decision,
    /// The person's note with a denial (ignored with an allow).
    #[serde(default)]
    note: Option<String>,
}

async fn approval(
    Caller(who): Caller,
    State(m): State<Master>,
    Path((id, approval_id)): Path<(String, String)>,
    Json(b): Json<ApprovalBody>,
) -> ApiResult<Json<serde_json::Value>> {
    // Allowing makes the chat do work; denying stops it, like cancel, so a
    // user who lost access, or must change their password first, can
    // still refuse (Gate 3 review P-7).
    if b.decision.allows() {
        caller::settled(&who)?;
    }
    // An id outside the harness's id rule names no card (review N4): the
    // same answer as an unknown session, before anything is relayed.
    if !harness_core::valid_event_id(&approval_id) {
        return Err(ApiError(StatusCode::NOT_FOUND, "unknown approval".into()));
    }
    let row = acting_session(&m, &who, &id).await?;
    if b.decision.allows() {
        agentic_gate(&m, &who, &row).await?;
    }
    if b.note
        .as_ref()
        .is_some_and(|n| n.chars().count() > harness_core::policy::MAX_DENY_NOTE)
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            format!(
                "the note is longer than {} characters",
                harness_core::policy::MAX_DENY_NOTE
            ),
        ));
    }
    // With an allow a note means nothing: it is dropped, not refused.
    let note = b.note.filter(|_| !b.decision.allows());
    // A push card's Allow by the chat's owner (X.27 step 4): recorded
    // before it is relayed, since the node asks for the credential at once.
    if b.decision.allows() {
        m.note_push_click(who.user_id.as_deref(), &row, &approval_id)
            .await;
    }
    m.respond_approval(row.session_id, approval_id, b.decision, note)
        .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct RestoreBody {
    #[serde(default)]
    force: bool,
    #[serde(default)]
    rewind: bool,
}

async fn restore_turn(
    User(who): User,
    State(m): State<Master>,
    Path((id, turn)): Path<(String, String)>,
    Json(b): Json<RestoreBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let row = acting_session(&m, &who, &id).await?;
    agentic_gate(&m, &who, &row).await?;
    m.restore_turn(row.session_id, turn, b.force, b.rewind)
        .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct VariantBody {
    group: String,
    message_id: String,
}

async fn select_variant(
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
    Json(b): Json<VariantBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let row = acting_session(&m, &who, &id).await?;
    agentic_gate(&m, &who, &row).await?;
    m.select_variant(row.session_id, b.group, b.message_id)
        .await?;
    Ok(Json(json!({ "ok": true })))
}
