//! `doc_create` and `sheet_create` on the master, for a chat without a
//! folder (README "Documents the agent makes"; PLAN.md T97): a node's tool
//! asks, for the session it runs, to make a Word or Excel file from the
//! model's own text, and this builds it, keeps it as the owner's blob and
//! answers with the file's id.
//!
//! A built-in service, never a tool of the master: it runs no shell, no file
//! tool and no program the model names, and it writes no path of this machine
//! the model chose. The conversion (`harness-docs`) is a pure function from
//! bounded text to bytes, run in `spawn_blocking` under the same places and
//! time limit as `attached_file`; its input is the model's text, never a
//! hostile file, which is why it does not run in the confined helper the
//! readers use (the tools that edit an existing file will).
//!
//! The scope is the session alone. The chat must be a plain chat (not a
//! sub-agent's, not a temporary one), the node must offer the tools, and the
//! asking turn must be one the master granted it: the log's open turn must
//! not be a callback turn, must not have the tool withheld
//! (`AgentRef::withheld`), and its agent must have the switch "Create and
//! edit documents" (`Features::office`). The file is recorded for this chat
//! with `logged` off; it becomes the chat's, and downloadable, only when the
//! node's own `tool_call_finished.made` names it (`harness_store::
//! session_files`), so the master never trusts a call it did not see logged.
//! A turn may ask 60 times, a chat 240 times in ten minutes, one at a time,
//! and a chat keeps at most 100 made files.

use std::time::{Duration, Instant};

use bytes::Bytes;
use harness_core::SessionId;
use harness_core::docs::{DOCX_MIME, DocCreated, DocRequest, TOOLS, XLSX_MIME};
use harness_docs::limits::{MAX_OUTPUT_BYTES, MAX_SPEC_BYTES};
use harness_protocol::MasterToNode;
use harness_store::OpenTurn;
use harness_store::blobs::NewBlob;
use harness_store::session_files::{MAX_MADE_PER_CHAT, MadePut, NewMade};

use crate::attached::{Grants, MADE_WORDS, TIME_LIMIT, WAIT, bytes_of, file_of};
use crate::{Master, storage};

/// The grants this service keeps: the places to build in, and what each chat
/// and node has used.
pub fn grants() -> Grants {
    Grants::with_wait(WAIT, &MADE_WORDS)
}

/// Withhold the tools from an agent and every sub-agent it may hand work to.
pub fn deny(spec: &mut harness_core::agent::AgentSpec) {
    let deny = |list: &mut Vec<String>| {
        for t in TOOLS {
            if !list.iter().any(|x| x == t) {
                list.push(t.into());
            }
        }
    };
    deny(&mut spec.deny_tools);
    for d in &mut spec.delegates {
        deny(&mut d.deny_tools);
    }
}

/// Withhold the tools from the agents this one may hand work to, and leave
/// its own: a sub-agent's chat has no files (`answer` refuses it), so a
/// delegate offered the tools could only fail every call.
pub fn deny_delegates(spec: &mut harness_core::agent::AgentSpec) {
    for d in &mut spec.delegates {
        for t in TOOLS {
            if !d.deny_tools.iter().any(|x| x == t) {
                d.deny_tools.push(t.into());
            }
        }
    }
}

/// What a turn is told when its agent has no switch.
pub const NOT_OFFERED: &str = "making documents is not available to this agent: its \"Create and edit documents\" switch is off";

/// Answer a node's `docs.request` from a task of its own (building never
/// holds up the node's events). The answer goes to that node only.
pub fn handle(m: &Master, node_id: &str, req_id: String, sid: SessionId, r: DocRequest) {
    // `docs.result` goes only to a node that announced the capability: a
    // request from one that did not is dropped, not answered.
    if !m.node_has(node_id, harness_protocol::CAP_DOCS) {
        tracing::warn!(node = %node_id, "docs.request from a node without the capability");
        return;
    }
    let Ok(permit) = m.inner.made.node(node_id).try_acquire_owned() else {
        m.send_unacked(
            node_id,
            MasterToNode::DocsResult {
                req_id,
                answer: None,
                error: Some(
                    "too many documents are being made for this node at once; try again".into(),
                ),
            },
        );
        return;
    };
    let (m, node_id) = (m.clone(), node_id.to_string());
    tokio::spawn(async move {
        let res = answer(&m, &node_id, sid, r).await;
        drop(permit);
        let (answer, error) = match res {
            Ok(a) => (Some(a), None),
            Err(e) => {
                // Lengths only: a refusal may carry text the model wrote.
                tracing::debug!(node = %node_id, session = %sid, len = e.len(), "a document request was refused");
                (None, Some(harness_core::text::clean_line(&e, 400)))
            }
        };
        m.send_unacked(
            &node_id,
            MasterToNode::DocsResult {
                req_id,
                answer,
                error,
            },
        );
    });
}

/// The open turn of `row`, if it may make files: not a callback turn, the
/// tool not withheld from it, its agent with the switch.
async fn turn_may(
    m: &Master,
    row: &harness_store::SessionRow,
    tool: &str,
) -> Result<OpenTurn, String> {
    let sid = row.session_id;
    let open = m
        .inner
        .mirror
        .call(move |s| s.open_turn(sid))
        .await
        .ok()
        .flatten()
        .ok_or("making documents is not available outside a turn")?;
    if open.callback || open.withheld.iter().any(|t| t == tool) {
        return Err("making documents is not available in this turn".into());
    }
    let r = m.resolver_for(row.workspace_id.as_deref()).await;
    let on = open
        .agent_id
        .as_deref()
        .and_then(|id| r.agents().get(id))
        .is_some_and(|a| a.features.office);
    if !on {
        return Err(NOT_OFFERED.into());
    }
    Ok(open)
}

pub async fn answer(
    m: &Master,
    node_id: &str,
    sid: SessionId,
    q: DocRequest,
) -> Result<DocCreated, String> {
    if !m.node_has(node_id, harness_protocol::CAP_DOCS) {
        return Err("this node does not offer document tools".into());
    }
    // One call in flight per chat, refused at once.
    let _one = m.inner.made.chat(sid)?;
    let (row, _top) = crate::recall::sessions(m, node_id, sid).await?;
    if row.parent_id.is_some() {
        return Err("a sub-agent's chat keeps no files".into());
    }
    if row.profile.as_deref() != Some("chat") {
        return Err("a chat with a folder writes the file there itself".into());
    }
    if row.incognito {
        return Err("a temporary chat keeps no files".into());
    }
    let open = turn_may(m, &row, q.tool()).await?;
    m.inner.made.take(sid, &open.turn_id)?;
    if matches!(
        q,
        DocRequest::View { .. } | DocRequest::DocEdit { .. } | DocRequest::SheetEdit { .. }
    ) {
        return match edit_answer(m, &row, q).await {
            Ok(a) => Ok(a),
            Err(e) => {
                // A refusal is not a call: it gives its count back.
                m.inner.made.give_back(sid, &open.turn_id);
                Err(e)
            }
        };
    }
    // A refusal for lack of room is not a call: it gives its count back.
    let slot = match m.inner.made.slot().await {
        Ok(s) => s,
        Err(e) => {
            m.inner.made.give_back(sid, &open.turn_id);
            return Err(e);
        }
    };
    let deadline = Instant::now() + TIME_LIMIT;
    // The place is held until the work really ends, also when the call
    // above gave up waiting for it.
    let work = tokio::task::spawn_blocking(move || {
        let _slot = slot;
        make(&q, deadline)
    });
    let built = match tokio::time::timeout(TIME_LIMIT + Duration::from_secs(2), work).await {
        Ok(Ok(r)) => r?,
        Ok(Err(_)) => return Err("making the file failed".into()),
        Err(_) => return Err("making the file took too long and was stopped".into()),
    };
    keep(m, &row, built).await
}

/// `doc_view`, `doc_edit` and `sheet_edit` of a plain chat (ADOPTION_PLAN
/// X.57, PLAN.md T98): the chat's own file (the id its log names; the
/// owner's blob) goes to the confined `doc-edit` helper, never parsed in
/// here, under the account's one helper slot; the result is kept as a made
/// file like a created one. A refused call (any edit refused: nothing is
/// written) is an error with each edit's reason.
async fn edit_answer(
    m: &Master,
    row: &harness_store::SessionRow,
    q: DocRequest,
) -> Result<DocCreated, String> {
    use harness_docs::edit::serve::{Request, frame_request, parse_reply};
    let (file_id, req, save_as) = match q {
        DocRequest::View {
            file,
            sheet,
            from,
            limit,
        } => (
            file,
            Request::DocView {
                sheet,
                from: from as usize,
                limit: limit as usize,
            },
            None,
        ),
        DocRequest::DocEdit {
            file,
            edits,
            save_as,
        } => (file, Request::DocEdit { edits }, save_as),
        DocRequest::SheetEdit {
            file,
            sheet,
            edits,
            save_as,
        } => (file, Request::SheetEdit { sheet, edits }, save_as),
        _ => return Err("not an edit".into()),
    };
    if file_id.is_empty()
        || file_id.len() > harness_core::attached::MAX_ID_CHARS
        || !file_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err("no such file in this chat: use the id the message names the file by".into());
    }
    let reader = m
        .pdf_reader()
        .ok_or("this server cannot edit documents (the editor is not set up)")?;
    let file = file_of(m, row, &file_id).await?;
    let who = row
        .owner_id
        .clone()
        .unwrap_or_else(|| format!("chat:{}", row.session_id));
    let _mine = reader.account_slot(&who).await.map_err(|e| e.to_string())?;
    let bytes = bytes_of(m, &file).await?;
    let is_view = matches!(req, Request::DocView { .. });
    let sheet_tool = matches!(req, Request::SheetEdit { .. });
    let out = reader
        .edit(frame_request(&req, &bytes))
        .await
        .map_err(|e| match e {
            crate::pdf::DocError::Unreadable(m) => m,
            other => other.to_string(),
        })?;
    drop(bytes);
    let (reply, new) =
        parse_reply(&out).ok_or("the document editor's answer was not understood")?;
    if is_view {
        return Ok(DocCreated {
            file_id: String::new(),
            name: String::new(),
            bytes: 0,
            sha256: String::new(),
            mime: String::new(),
            notes: vec![],
            text: reply.text,
        });
    }
    let mut notes: Vec<String> = reply.results.iter().map(|r| r.msg.clone()).collect();
    if !reply.file {
        notes.extend(reply.notes.iter().cloned());
        return Err(format!("nothing was changed.\n{}", notes.join("\n")));
    }
    notes.extend(reply.notes.iter().cloned());
    let in_ext = file
        .name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase());
    let (ext, mime): (&str, &'static str) = match (sheet_tool, in_ext.as_deref()) {
        (true, Some("xlsm")) => ("xlsm", "application/vnd.ms-excel.sheet.macroEnabled.12"),
        (true, _) => ("xlsx", XLSX_MIME),
        (false, Some("docm")) => ("docm", "application/vnd.ms-word.document.macroEnabled.12"),
        (false, _) => ("docx", DOCX_MIME),
    };
    let base = match save_as.as_deref() {
        Some(n) if !n.trim().is_empty() => n.to_string(),
        _ => format!(
            "{} (edited)",
            file.name
                .rsplit_once('.')
                .map_or(file.name.as_str(), |(s, _)| s)
        ),
    };
    let name = harness_docs::name::file_name(&base, ext, "edited");
    keep(
        m,
        row,
        Built {
            name,
            mime,
            bytes: new.to_vec(),
            notes,
        },
    )
    .await
}

/// A finished file, before it is kept.
pub struct Built {
    pub name: String,
    pub mime: &'static str,
    pub bytes: Vec<u8>,
    pub notes: Vec<String>,
}

/// The conversion, off the async threads. Pure: a request in, bytes out.
pub fn make(q: &DocRequest, deadline: Instant) -> Result<Built, String> {
    if Instant::now() > deadline {
        return Err("making the file took too long and was stopped".into());
    }
    match q {
        DocRequest::Doc {
            name,
            markdown,
            page,
            landscape,
            title,
        } => {
            let o = harness_docs::docx::DocOptions {
                page: match page.as_deref() {
                    None | Some("a4") => harness_docs::docx::Page::A4,
                    Some("letter") => harness_docs::docx::Page::Letter,
                    Some(_) => return Err("`page` is a4 or letter".into()),
                },
                landscape: *landscape,
                title: title.clone(),
                created: harness_docs::now_unix(),
            };
            let b = harness_docs::docx::build(markdown, &o)?;
            Ok(Built {
                name: harness_docs::name::file_name(name, "docx", "document"),
                mime: DOCX_MIME,
                notes: harness_docs::notes::doc(&b.report),
                bytes: b.bytes,
            })
        }
        DocRequest::Sheet { name, spec } => {
            // The spec was bounded where it was read; a node that sends a
            // bigger one is refused the same.
            let size = serde_json::to_vec(spec)
                .map(|b| b.len())
                .unwrap_or(usize::MAX);
            if size > MAX_SPEC_BYTES {
                return Err(format!(
                    "the sheet spec is larger than {} MiB",
                    MAX_SPEC_BYTES >> 20
                ));
            }
            let b = harness_docs::xlsx::build(spec, harness_docs::now_unix())?;
            Ok(Built {
                name: harness_docs::name::file_name(name, "xlsx", "spreadsheet"),
                mime: XLSX_MIME,
                notes: harness_docs::notes::sheet(&b.report),
                bytes: b.bytes,
            })
        }
        _ => Err("not a request to make a file".into()),
    }
}

/// Store the file as the chat owner's blob, counted in their quota, and
/// record it for this chat (not yet logged).
async fn keep(
    m: &Master,
    row: &harness_store::SessionRow,
    built: Built,
) -> Result<DocCreated, String> {
    let size = built.bytes.len() as u64;
    if size == 0 || built.bytes.len() > MAX_OUTPUT_BYTES {
        return Err("the file would be larger than a file here may be".into());
    }
    let owner = row.owner_id.clone();
    storage::check_room(m, owner.as_deref(), size).await?;
    let st = storage::get(m).ok_or("this server keeps no files, so it cannot keep this one")?;
    let writer = storage::TempWriter::start(st).map_err(|e| {
        tracing::warn!(error = %e, "cannot start a made file");
        "could not write the file".to_string()
    })?;
    // In pieces, as an upload arrives: the writer hashes and seals them.
    let all = Bytes::from(built.bytes);
    let mut at = 0;
    while at < all.len() {
        let end = (at + (256 << 10)).min(all.len());
        if !writer.write(all.slice(at..end)).await {
            writer.abandon().await;
            return Err("could not write the file".into());
        }
        at = end;
    }
    let written = writer.finish().await.map_err(|e| {
        tracing::warn!(error = %e, "writing a made file failed");
        "could not write the file".to_string()
    })?;
    let file_id = harness_core::new_id();
    let made = NewMade {
        session_id: row.session_id.to_string(),
        file_id: file_id.clone(),
        owner_id: owner.clone(),
        name: built.name.clone(),
        mime: built.mime.to_string(),
    };
    let blob = NewBlob {
        blob_id: written.blob_id.clone(),
        owner_id: owner.clone(),
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
    let sha = written.sha256.clone();
    let limits = storage::config(m).limits(owner.as_deref());
    let put = storage::commit(m, written, move |s| {
        let r = s.session_file_make(&made, &blob, limits, MAX_MADE_PER_CHAT)?;
        let keep = matches!(r, MadePut::Added { new_blob: true });
        Ok((r, keep))
    })
    .await
    .map_err(|(_, e)| e)?;
    match put {
        MadePut::Added { .. } => Ok(DocCreated {
            file_id,
            name: built.name,
            bytes: size,
            sha256: sha,
            mime: built.mime.to_string(),
            notes: built.notes,
            text: String::new(),
        }),
        MadePut::TooMany => Err(format!(
            "this chat already keeps {MAX_MADE_PER_CHAT} made files; no more can be made in it"
        )),
        MadePut::OverQuota(o) => Err(storage::full_message(&o)),
    }
}

/// The two tools' names, for the page that lists them.
pub const NAMES: [&str; 5] = TOOLS;

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::agent::AgentSpec;

    fn spec(delegates: Vec<AgentSpec>) -> AgentSpec {
        let mut v = serde_json::json!({
            "agent_id": "a", "name": "a", "system_prompt": "", "tools": ["doc_create", "sheet_create"],
            "prompt_hash": "h"
        });
        v["delegates"] = serde_json::to_value(delegates).unwrap();
        serde_json::from_value(v).unwrap()
    }

    /// A delegate of a plain chat's agent never has the tools (it could
    /// only fail), the agent itself keeps them; `deny` takes both away.
    #[test]
    fn delegates_lose_the_tools_and_the_agent_keeps_them() {
        let child = || spec(vec![]);
        let mut a = spec(vec![child(), child()]);
        deny_delegates(&mut a);
        assert!(a.allows_tool("doc_create") && a.allows_tool("sheet_create"));
        for d in &a.delegates {
            assert!(!d.allows_tool("doc_create") && !d.allows_tool("sheet_create"));
        }
        deny_delegates(&mut a);
        assert_eq!(a.delegates[0].deny_tools.len(), 2, "once each");
        deny(&mut a);
        assert!(!a.allows_tool("doc_create") && !a.allows_tool("sheet_create"));
    }
}
