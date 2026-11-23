//! What the confined helper (`rizzy-harness doc-edit`) does with its stdin
//! (PLAN.md T98): one request and one file in, one reply and, when every edit
//! applied, the new file out. The framing is a 4-byte big-endian length, that
//! many bytes of JSON, then the rest. The server and the node write the
//! request and read the reply with [`frame_request`] and [`parse_reply`]; the
//! helper's side is [`serve`]. Nothing of the file is a path, a command or a
//! URL.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::pkg::Source;
use super::{docx, xlsx};
use crate::Result;
use crate::limits::MAX_EDITS;

/// The JSON header of a request, at most this many bytes.
pub const MAX_HEADER_BYTES: usize = 1 << 20;

/// What to do with the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    DocView {
        #[serde(default)]
        sheet: Option<String>,
        #[serde(default)]
        from: usize,
        #[serde(default)]
        limit: usize,
    },
    /// The file as data for the web page (`preview::preview`): no tags, not
    /// bounded by the model's view.
    Preview {
        /// The place of the sheet among the visible ones.
        #[serde(default)]
        sheet: Option<u32>,
    },
    /// Changes of text from the web page (`page::apply`): a paragraph's tag
    /// or a cell's address with the text typed for it. `sheet` is the place
    /// among the visible sheets.
    PageEdit {
        #[serde(default)]
        sheet: Option<u32>,
        changes: Vec<super::page::Change>,
    },
    DocEdit {
        edits: Vec<Value>,
    },
    SheetEdit {
        sheet: String,
        edits: Vec<Value>,
    },
}

/// One edit's outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub ok: bool,
    pub msg: String,
    /// A page edit's reason, as a word (the engine's `msg` may hold text of
    /// the file and is not for the page).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<super::page::ChangeWhy>,
}

/// What came back. The new file follows the header when `file` is set.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reply {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub results: Vec<Item>,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub file: bool,
    /// The answer to `preview`; none when the file is not a Word or Excel
    /// file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<super::preview::Preview>,
    /// Why there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<super::preview::Why>,
}

/// A request and the file, as the helper reads them.
pub fn frame_request(req: &Request, file: &[u8]) -> Vec<u8> {
    let head = serde_json::to_vec(req).unwrap_or_default();
    let mut out = Vec::with_capacity(4 + head.len() + file.len());
    out.extend_from_slice(&(head.len() as u32).to_be_bytes());
    out.extend_from_slice(&head);
    out.extend_from_slice(file);
    out
}

/// The reply and the file bytes (empty unless `file`).
pub fn parse_reply(out: &[u8]) -> Option<(Reply, &[u8])> {
    let n = u32::from_be_bytes(out.get(..4)?.try_into().ok()?) as usize;
    if n > MAX_HEADER_BYTES {
        return None;
    }
    let head = out.get(4..4 + n)?;
    let reply: Reply = serde_json::from_slice(head).ok()?;
    let rest = &out[4 + n..];
    if reply.file != !rest.is_empty() {
        return None;
    }
    Some((reply, rest))
}

fn reply(r: &Reply, file: Option<Vec<u8>>) -> Vec<u8> {
    let head = serde_json::to_vec(r).unwrap_or_default();
    let mut out = Vec::new();
    out.extend_from_slice(&(head.len() as u32).to_be_bytes());
    out.extend_from_slice(&head);
    if let Some(f) = file {
        out.extend_from_slice(&f);
    }
    out
}

/// Which kind of file it is, by its parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Doc,
    Sheet,
}

pub fn kind_of(file: &[u8]) -> Result<Kind> {
    let s = Source::open(file)?;
    if s.has("word/document.xml") {
        Ok(Kind::Doc)
    } else if s.has("xl/workbook.xml") {
        Ok(Kind::Sheet)
    } else {
        Err("this is not a Word or Excel file (.docx or .xlsx) that can be edited".into())
    }
}

fn items(o: &super::Outcome) -> Vec<Item> {
    o.results
        .iter()
        .map(|r| match r {
            Ok(m) => Item {
                ok: true,
                msg: m.clone(),
                why: None,
            },
            Err(m) => Item {
                ok: false,
                msg: m.clone(),
                why: None,
            },
        })
        .collect()
}

/// The helper's whole job: `Ok` is the bytes to write to stdout, `Err` the
/// one sentence the helper exits with.
pub fn serve(input: &[u8]) -> Result<Vec<u8>> {
    let n = u32::from_be_bytes(
        input
            .get(..4)
            .ok_or("the request is cut short")?
            .try_into()
            .map_err(|_| "the request is cut short")?,
    ) as usize;
    if n > MAX_HEADER_BYTES {
        return Err("the request is too large".into());
    }
    let head = input.get(4..4 + n).ok_or("the request is cut short")?;
    let file = &input[4 + n..];
    let req: Request = serde_json::from_slice(head).map_err(|_| "the request is not understood")?;
    match req {
        Request::DocView { sheet, from, limit } => {
            let text = match kind_of(file)? {
                Kind::Doc => docx::view(file, from, limit)?,
                Kind::Sheet => xlsx::view(file, sheet.as_deref(), from, limit)?,
            };
            Ok(reply(
                &Reply {
                    text,
                    ..Default::default()
                },
                None,
            ))
        }
        Request::Preview { sheet } => {
            let (preview, why) = match super::preview::preview(file, sheet) {
                Ok(p) => (Some(p), None),
                Err(w) => (None, Some(w)),
            };
            Ok(reply(
                &Reply {
                    preview,
                    why,
                    ..Default::default()
                },
                None,
            ))
        }
        Request::PageEdit { sheet, changes } => {
            if changes.is_empty() {
                return Err("give at least one change".into());
            }
            if changes.len() > crate::limits::MAX_PAGE_CHANGES {
                return Err(format!(
                    "at most {} changes in one request",
                    crate::limits::MAX_PAGE_CHANGES
                ));
            }
            Ok(match super::page::apply(file, sheet, &changes) {
                Ok(done) => reply(
                    &Reply {
                        results: done.items,
                        file: done.bytes.is_some(),
                        ..Default::default()
                    },
                    done.bytes,
                ),
                Err(why) => reply(
                    &Reply {
                        why: Some(why),
                        ..Default::default()
                    },
                    None,
                ),
            })
        }
        Request::DocEdit { edits } => {
            if kind_of(file)? != Kind::Doc {
                return Err("this is a spreadsheet; use sheet_edit".into());
            }
            if edits.len() > MAX_EDITS {
                return Err(format!("at most {MAX_EDITS} edits in one call"));
            }
            let ops = edits
                .iter()
                .enumerate()
                .map(|(i, v)| docx::DocOp::parse(v).map_err(|e| format!("edit {}: {e}", i + 1)))
                .collect::<Result<Vec<_>>>()?;
            let o = docx::edit(file, &ops)?;
            let r = Reply {
                results: items(&o),
                notes: o.notes.clone(),
                file: o.bytes.is_some(),
                ..Default::default()
            };
            Ok(reply(&r, o.bytes))
        }
        Request::SheetEdit { sheet, edits } => {
            if kind_of(file)? != Kind::Sheet {
                return Err("this is a Word document; use doc_edit".into());
            }
            if edits.len() > MAX_EDITS {
                return Err(format!("at most {MAX_EDITS} edits in one call"));
            }
            let ops = edits
                .iter()
                .enumerate()
                .map(|(i, v)| xlsx::SheetOp::parse(v).map_err(|e| format!("edit {}: {e}", i + 1)))
                .collect::<Result<Vec<_>>>()?;
            let o = xlsx::edit(file, &sheet, &ops)?;
            let r = Reply {
                results: items(&o),
                notes: o.notes.clone(),
                file: o.bytes.is_some(),
                ..Default::default()
            };
            Ok(reply(&r, o.bytes))
        }
    }
}
