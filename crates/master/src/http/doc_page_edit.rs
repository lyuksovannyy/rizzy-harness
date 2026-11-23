//! `POST /api/sessions/{id}/files/{file_id}/edit`: a person's text changes to
//! a Word or Excel file of a chat without a folder, saved as a NEW file the
//! person holds like an upload (ADOPTION_PLAN X.60, PLAN.md T100). The
//! checks are those of the preview (`doc_preview.rs`), in the same order,
//! plus the rule of every POST of this API (the guard of `auth.rs`: a cookie
//! request must carry an allowed `Origin`) and `acting_session`, which marks a
//! chat another caller (the API token) acts in. The file is changed only by
//! the confined `doc-edit` helper (T98), one at a time per account, through
//! the request `PageEdit`, which turns each change into one of the edit
//! tools' own operations; the result is stored by the function the upload
//! route stores a file with (`attachments::keep`: the same quota, the same
//! limits, the same name cleaning), as a held attachment of the CALLER, and
//! the answer is what that route answers for one file. The original is never
//! changed.
//!
//! The helper's own words may hold text of the file: they are never
//! forwarded; each change that cannot be applied is told by a word of the
//! helper turned into a sentence of ours.

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use harness_docs::edit::page::{Change, ChangeWhy};
use harness_docs::edit::serve::{Request, frame_request, parse_reply};
use harness_protocol::{
    ChangeFailure, ChangeResult, MAX_PAGE_EDIT_AT_BYTES, MAX_PAGE_EDIT_CHANGES,
    MAX_PAGE_EDIT_TEXT_BYTES, PageChange,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::attachments::{AsFile, Target, UploadQuery, keep, target};
use super::doc_preview::{refusal, why};
use super::{ApiError, ApiResult, acting_session, require};
use crate::pdf::DocError;
use crate::{Master, attached};

/// The most a request body may be: the helper's own header bound, which is
/// what the changes are framed into.
pub(super) const MAX_BODY_BYTES: usize = harness_docs::edit::serve::MAX_HEADER_BYTES;
/// What the changes may be once framed: the body bound less room for the
/// rest of the header.
pub(super) const MAX_CHANGES_BYTES: usize = MAX_BODY_BYTES - 4096;

pub(super) fn routes() -> Router<Master> {
    Router::new().route(
        "/api/sessions/{id}/files/{file_id}/edit",
        post(edit).layer(DefaultBodyLimit::max(MAX_BODY_BYTES)),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Body {
    #[serde(default)]
    sheet: Option<u32>,
    changes: Vec<PageChange>,
}

/// The changes of a request, bounded: at least one, at most 200, a place of at
/// most 64 bytes and a text of at most 80,000, all of them together at most
/// what the helper's header takes. Checked here so that a request the node
/// or the helper would refuse is refused before either is asked.
pub(super) fn check_changes(changes: &[PageChange]) -> ApiResult<()> {
    let bad = |m: &str| ApiError(StatusCode::BAD_REQUEST, m.into());
    if changes.is_empty() {
        return Err(bad("there is nothing to save: no change was made"));
    }
    if changes.len() > MAX_PAGE_EDIT_CHANGES {
        return Err(bad(&format!(
            "at most {MAX_PAGE_EDIT_CHANGES} changes can be saved at once"
        )));
    }
    if changes
        .iter()
        .any(|c| c.at.len() > MAX_PAGE_EDIT_AT_BYTES || c.text.len() > MAX_PAGE_EDIT_TEXT_BYTES)
    {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "a change is too long to be saved".into(),
        ));
    }
    let size = serde_json::to_vec(changes).map_or(usize::MAX, |v| v.len());
    if size > MAX_CHANGES_BYTES {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "the changes are too long to be saved at once; save fewer".into(),
        ));
    }
    Ok(())
}

/// The body of a request, read after the caller is known.
pub(super) fn parse<T: for<'de> Deserialize<'de>>(body: &Bytes) -> ApiResult<T> {
    serde_json::from_slice(body).map_err(|_| {
        ApiError(
            StatusCode::BAD_REQUEST,
            "the request was not understood".into(),
        )
    })
}

/// One sentence of ours for each reason a change was not applied.
pub(super) fn sentence(w: Option<ChangeFailure>) -> &'static str {
    match w {
        Some(ChangeFailure::BadPlace) => "this place cannot be changed",
        Some(ChangeFailure::Stale) => {
            "this paragraph changed since the preview was made; open the preview again"
        }
        Some(ChangeFailure::Gone) => {
            "this paragraph is not in the file any more; open the preview again"
        }
        Some(ChangeFailure::NotEditable) => {
            "this place holds something that cannot be changed here (a field, a link, a picture, tracked changes, or a merged, hidden or calculated cell)"
        }
        Some(ChangeFailure::BadFormula) => {
            "this formula is empty or uses something that is not allowed"
        }
        Some(ChangeFailure::TooLong) => "this text is too long",
        Some(ChangeFailure::Duplicate) => "this place was changed twice",
        Some(ChangeFailure::Skipped) => "not saved, because another change could not be saved",
        Some(ChangeFailure::Other) | None => "this change could not be applied",
    }
}

/// The results as the page reads them: `ok`, and for a refusal its word and
/// our sentence.
pub(super) fn results_json(rs: &[ChangeResult]) -> Vec<Value> {
    rs.iter()
        .map(|r| {
            if r.ok {
                json!({"ok": true})
            } else {
                json!({"ok": false, "why": r.why, "message": sentence(r.why)})
            }
        })
        .collect()
}

/// 422: nothing was saved, and which changes could not be applied.
pub(super) fn refused_changes(rs: &[ChangeResult]) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({
            "error": "nothing was saved: some changes could not be applied",
            "results": results_json(rs),
        })),
    )
        .into_response()
}

fn failure(w: ChangeWhy) -> ChangeFailure {
    match w {
        ChangeWhy::BadPlace => ChangeFailure::BadPlace,
        ChangeWhy::Stale => ChangeFailure::Stale,
        ChangeWhy::Gone => ChangeFailure::Gone,
        ChangeWhy::NotEditable => ChangeFailure::NotEditable,
        ChangeWhy::BadFormula => ChangeFailure::BadFormula,
        ChangeWhy::TooLong => ChangeFailure::TooLong,
        ChangeWhy::Duplicate => ChangeFailure::Duplicate,
        ChangeWhy::Skipped => ChangeFailure::Skipped,
        ChangeWhy::Other => ChangeFailure::Other,
    }
}

fn unknown() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "unknown file".into())
}

fn helper_sentence(e: DocError) -> ApiError {
    super::doc_preview::sentence(e)
}

/// The name of the copy: the file's own name and ` (edited)`, with the
/// extension kept where it keeps its kind (macros), as the editing tools do.
fn copy_name(name: &str) -> (String, &'static str, &'static str) {
    let lower = name.to_ascii_lowercase();
    let ext = match lower.rsplit_once('.').map(|(_, e)| e) {
        Some("xlsm") => "xlsm",
        Some("xlsx") => "xlsx",
        Some("docm") => "docm",
        _ => "docx",
    };
    let mime = if ext.starts_with('x') {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    } else {
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    };
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = harness_docs::name::edited_stem(base, ext);
    (format!("{stem}.{ext}"), ext, mime)
}

async fn edit(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, file_id)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult<Response> {
    let who = require(&m, &headers).await?;
    let row = acting_session(&m, &who, &id).await?;
    // Only a plain chat keeps files: a coding chat's are in its folder.
    if row.profile.as_deref() != Some("chat") || row.parent_id.is_some() || row.incognito {
        return Err(unknown());
    }
    if file_id.is_empty()
        || file_id.len() > harness_core::attached::MAX_ID_CHARS
        || !file_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err(unknown());
    }
    let sid = row.session_id.to_string();
    let fid = file_id.clone();
    let file = m
        .inner
        .mirror
        .call(move |s| s.session_file(&sid, &fid))
        .await?
        .ok_or_else(unknown)?;
    // The blob is the chat owner's, whatever a row says.
    if file.owner_id != row.owner_id {
        return Err(unknown());
    }
    if !super::doc_preview::office_name(&file.name) {
        return Err(super::doc_preview::not_office());
    }
    if file.size == 0 {
        return Err(refusal(harness_protocol::PreviewRefusal::Empty, false));
    }
    if file.size > attached::MAX_FILE {
        return Err(refusal(
            harness_protocol::PreviewRefusal::FileTooLarge,
            false,
        ));
    }
    let Body { sheet, changes } = parse(&body)?;
    check_changes(&changes)?;
    // The copy is held as an upload is: the chat must keep files, and the
    // caller's room must have a place for it.
    let (copy, ext, mime) = copy_name(&file.name);
    let q = UploadQuery {
        name: copy,
        mime: Some(mime.into()),
        session: Some(row.session_id.to_string()),
        agentic: None,
        agent: None,
    };
    let dest = match target(&m, &who, &q).await? {
        Target::Folder(d) if d.kept => d,
        Target::Folder(_) => return Err(unknown()),
        Target::Plain(why) => return Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, why)),
    };
    let reader = m
        .pdf_reader()
        .ok_or_else(|| helper_sentence(DocError::NotSetUp))?;
    let account = who.user_id.clone().unwrap_or_else(|| "api-token".into());
    let n = changes.len();
    let asked: Vec<Change> = changes
        .into_iter()
        .map(|c| Change {
            at: c.at,
            text: c.text,
        })
        .collect();
    let (reply, new) = {
        // One helper at a time per account; the blob is read only after.
        let _mine = reader
            .account_slot(&account)
            .await
            .map_err(|e| helper_sentence(e.into()))?;
        let bytes = attached::bytes_of(&m, &file).await.map_err(|_| unknown())?;
        let out = reader
            .edit(frame_request(
                &Request::PageEdit {
                    kind: if ext.starts_with('x') {
                        harness_docs::edit::page::PageKind::Sheet
                    } else {
                        harness_docs::edit::page::PageKind::Doc
                    },
                    sheet,
                    changes: asked,
                },
                &bytes,
            ))
            .await
            .map_err(helper_sentence)?;
        drop(bytes);
        let (reply, rest) = parse_reply(&out).ok_or_else(|| {
            ApiError(
                StatusCode::BAD_GATEWAY,
                "the document reader's answer was not understood".into(),
            )
        })?;
        let rest = rest.to_vec();
        (reply, rest)
    };
    if let Some(w) = reply.why {
        return Err(why(w));
    }
    if reply.results.len() != n {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "the document reader's answer was not understood".into(),
        ));
    }
    let results: Vec<ChangeResult> = reply
        .results
        .iter()
        .map(|i| ChangeResult {
            ok: i.ok,
            why: i.why.map(failure),
        })
        .collect();
    if !reply.file {
        if results.iter().all(|r| r.ok) {
            return Err(ApiError(
                StatusCode::BAD_GATEWAY,
                "the document reader's answer was not understood".into(),
            ));
        }
        return Ok(refused_changes(&results));
    }
    if !results.iter().all(|r| r.ok) || new.is_empty() {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "the document reader's answer was not understood".into(),
        ));
    }
    if new.len() as u64 > attached::MAX_FILE {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "the changed copy would be larger than a file may be here (10 MB)".into(),
        ));
    }
    let _ = ext;
    let len = new.len() as u64;
    let Json(file) = keep(
        &m,
        &who,
        &q,
        dest,
        Some(len),
        axum::body::Body::from(new),
        AsFile::default(),
    )
    .await?;
    Ok(Json(json!({"file": file, "results": results_json(&results)})).into_response())
}

#[cfg(test)]
mod tests {
    use super::copy_name;

    /// The copy's name keeps ` (edited)` at every length and is never the
    /// original's name; the kind of the copy is the kind of the name.
    #[test]
    fn a_long_name_keeps_the_suffix_and_never_names_the_original() {
        for n in [91usize, 92, 100, 120] {
            let name = format!("{}.docx", "a".repeat(n));
            let (copy, ext, _) = copy_name(&name);
            assert!(copy.ends_with(" (edited).docx"), "{n}: {copy}");
            assert_ne!(copy, name, "{n}");
            assert!(copy.chars().count() <= 100 + ".docx".len(), "{n}");
            assert_eq!(ext, "docx");
        }
        let (copy, ext, mime) = copy_name(&format!("{}.xlsm", "é日".repeat(45)));
        assert!(
            copy.ends_with(" (edited).xlsm") && copy.len() <= 255,
            "{copy}"
        );
        assert_eq!(ext, "xlsm");
        assert!(mime.contains("spreadsheetml"));
        assert_eq!(copy_name("plan.docx").0, "plan (edited).docx");
    }
}
