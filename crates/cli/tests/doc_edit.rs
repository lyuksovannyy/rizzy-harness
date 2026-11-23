//! The real `rizzy-harness doc-edit` helper (ADOPTION_PLAN X.57, T98): a
//! request and a file in, the reply and the new file out, confined like
//! `doc-text`. The file is hostile: a DOCTYPE, a bomb in an entry the edit
//! does not touch, a file of the largest size, a request for a file the
//! helper must not open.
//!
//! Where the helper cannot confine itself (no Landlock, or a kernel before
//! ABI 4) these tests skip with a printed reason, but fail when `CI` is set.

use std::io::Write;
use std::time::{Duration, Instant};

use harness_docs::edit::serve::{Request, frame_request, parse_reply};
use harness_docs::zip::ZipWriter;
use harness_master::pdf::{self, DocError, PdfReader};
use serde_json::json;

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

fn docx_bytes(md: &str) -> Vec<u8> {
    harness_docs::docx::build(md, &Default::default())
        .unwrap()
        .bytes
}

fn xlsx_bytes() -> Vec<u8> {
    harness_docs::xlsx::build(
        &json!({"sheets": [{"name": "Data", "columns": [{"header": "Name"}, {"header": "Qty", "format": "integer"}], "rows": [["a", 1], ["b", 2]]}]}),
        0,
    )
    .unwrap()
    .bytes
}

/// `None` where the helper cannot confine itself here.
async fn call(r: &PdfReader, req: &Request, file: &[u8]) -> Option<Result<Vec<u8>, DocError>> {
    match r.edit(frame_request(req, file)).await {
        Err(DocError::Unconfined) => {
            assert!(
                std::env::var_os("CI").is_none(),
                "doc-edit cannot confine itself on this CI kernel"
            );
            eprintln!("skipped: doc-edit cannot confine itself here (Landlock ABI 4 needed)");
            None
        }
        other => Some(other),
    }
}

#[tokio::test]
async fn the_helper_views_and_edits_a_word_file_and_a_workbook() {
    let r = PdfReader::new(EXE.into());
    let doc = docx_bytes("# Title\n\nThe quick brown fox.\n");
    let view = Request::DocView {
        sheet: None,
        from: 1,
        limit: 100,
    };
    let Some(out) = call(&r, &view, &doc).await else {
        return;
    };
    let out = out.unwrap();
    let (reply, rest) = parse_reply(&out).unwrap();
    assert!(
        rest.is_empty() && reply.text.contains("The quick brown fox."),
        "{}",
        reply.text
    );
    let tag = reply
        .text
        .lines()
        .find(|l| l.contains("quick brown"))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let edit = Request::DocEdit {
        edits: vec![
            json!({"op": "replace_text", "at": tag, "find": "quick brown", "replace": "slow red"}),
        ],
    };
    let out = call(&r, &edit, &doc).await.unwrap().unwrap();
    let (reply, new) = parse_reply(&out).unwrap();
    assert!(reply.file && reply.results[0].ok, "{reply:?}");
    assert_ne!(new, doc);
    // The new file is a Word file the helper reads as edited, and it did not
    // overwrite anything: the input is the caller's bytes.
    let out = call(&r, &view, new).await.unwrap().unwrap();
    assert!(
        parse_reply(&out)
            .unwrap()
            .0
            .text
            .contains("The slow red fox.")
    );
    // A refused edit: no file, a reason.
    let bad = Request::DocEdit {
        edits: vec![json!({"op": "set_text", "at": "p1#0000", "text": "x"})],
    };
    let out = call(&r, &bad, &doc).await.unwrap().unwrap();
    let (reply, rest) = parse_reply(&out).unwrap();
    assert!(!reply.file && rest.is_empty() && !reply.results[0].ok);
    // A workbook.
    let x = xlsx_bytes();
    let view = Request::DocView {
        sheet: Some("Data".into()),
        from: 1,
        limit: 100,
    };
    let out = call(&r, &view, &x).await.unwrap().unwrap();
    assert!(parse_reply(&out).unwrap().0.text.contains("A2=\"a\""));
    let edit = Request::SheetEdit {
        sheet: "Data".into(),
        edits: vec![json!({"op": "set", "cells": {"B2": 41, "C2": {"f": "B2+1"}}})],
    };
    let out = call(&r, &edit, &x).await.unwrap().unwrap();
    let (reply, new) = parse_reply(&out).unwrap();
    assert!(reply.file && reply.results[0].ok, "{reply:?}");
    let out = call(&r, &view, new).await.unwrap().unwrap();
    let text = parse_reply(&out).unwrap().0.text;
    assert!(
        text.contains("B2=41") && text.contains("C2==B2+1"),
        "{text}"
    );
    // The wrong tool for the file.
    let out = call(
        &r,
        &Request::SheetEdit {
            sheet: "Data".into(),
            edits: vec![json!({"op": "clear", "range": "A1"})],
        },
        &doc,
    )
    .await
    .unwrap();
    assert!(matches!(out, Err(DocError::Unreadable(m)) if m.contains("doc_edit")));
    assert_eq!(r.free_slots(), pdf::CONCURRENT);
}

/// A zip bomb in an entry the edit does not touch is copied, not opened:
/// the helper's memory cap and time hold, and the bomb comes out the same.
#[tokio::test]
async fn a_bomb_in_an_untouched_entry_is_copied_not_inflated() {
    let r = PdfReader::new(EXE.into());
    let base = docx_bytes("one\n\ntwo\n");
    let p =
        harness_attached::Package::open(&base, Instant::now() + Duration::from_secs(5)).unwrap();
    // 300 MiB of zeros, deflated in pieces (a few hundred KB of data).
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    let chunk = vec![0u8; 1 << 20];
    for _ in 0..300 {
        e.write_all(&chunk).unwrap();
    }
    let bomb = e.finish().unwrap();
    assert!(bomb.len() < 1 << 20);
    let mut w = ZipWriter::new(10 << 20);
    for en in p.raw_entries().unwrap() {
        let n = std::str::from_utf8(en.name).unwrap();
        w.put_raw(
            n,
            en.method,
            en.crc,
            en.comp as u32,
            en.size as u32,
            en.time,
            en.date,
            en.data,
        )
        .unwrap();
    }
    w.put_raw(
        "word/bomb.bin",
        8,
        0xDEAD_BEEF,
        bomb.len() as u32,
        300 << 20,
        0,
        0,
        &bomb,
    )
    .unwrap();
    let file = w.finish().unwrap();
    let view = Request::DocView {
        sheet: None,
        from: 1,
        limit: 10,
    };
    let Some(out) = call(&r, &view, &file).await else {
        return;
    };
    let tag = parse_reply(&out.unwrap())
        .unwrap()
        .0
        .text
        .lines()
        .find(|l| l.contains("two"))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let t = Instant::now();
    let edit = Request::DocEdit {
        edits: vec![json!({"op": "set_text", "at": tag, "text": "2"})],
    };
    let out = call(&r, &edit, &file).await.unwrap().unwrap();
    assert!(t.elapsed() < Duration::from_secs(15), "{:?}", t.elapsed());
    let (reply, new) = parse_reply(&out).unwrap();
    assert!(reply.file && reply.results[0].ok, "{reply:?}");
    let q = harness_attached::Package::open(new, Instant::now() + Duration::from_secs(5)).unwrap();
    let kept = q
        .raw_entries()
        .unwrap()
        .into_iter()
        .find(|x| x.name == b"word/bomb.bin")
        .unwrap();
    assert_eq!(
        (kept.data, kept.crc, kept.size),
        (&bomb[..], 0xDEAD_BEEF, 300 << 20)
    );
}

/// A DOCTYPE, bytes that are not UTF-8, a repeated entry name: refused with
/// a sentence, nothing read from the machine.
#[tokio::test]
async fn hostile_files_are_refused_with_a_sentence() {
    let r = PdfReader::new(EXE.into());
    let base = docx_bytes("one\n");
    let p =
        harness_attached::Package::open(&base, Instant::now() + Duration::from_secs(5)).unwrap();
    let with = |doc: &[u8]| {
        let mut w = ZipWriter::new(10 << 20);
        for en in p.raw_entries().unwrap() {
            let n = std::str::from_utf8(en.name).unwrap();
            if n == "word/document.xml" {
                w.add(n, doc).unwrap();
            } else {
                w.put_raw(
                    n,
                    en.method,
                    en.crc,
                    en.comp as u32,
                    en.size as u32,
                    en.time,
                    en.date,
                    en.data,
                )
                .unwrap();
            }
        }
        w.finish().unwrap()
    };
    let view = Request::DocView {
        sheet: None,
        from: 1,
        limit: 10,
    };
    let xxe = b"<?xml version=\"1.0\"?><!DOCTYPE d [<!ENTITY x SYSTEM \"file:///etc/passwd\">]><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>&x;</w:t></w:r></w:p></w:body></w:document>";
    let Some(out) = call(&r, &view, &with(xxe)).await else {
        return;
    };
    assert!(
        matches!(&out, Err(DocError::Unreadable(m)) if m.contains("DOCTYPE")),
        "{out:?}"
    );
    let out = call(&r, &view, &with(b"<w:document \xff/>")).await.unwrap();
    assert!(
        matches!(&out, Err(DocError::Unreadable(m)) if m.contains("UTF-8")),
        "{out:?}"
    );
    // Not a request, not a zip.
    let out = r.edit(vec![0, 0, 0, 9, b'x']).await;
    assert!(matches!(out, Err(DocError::Unreadable(_))), "{out:?}");
    let out = call(&r, &view, b"not a zip at all").await.unwrap();
    assert!(matches!(out, Err(DocError::Unreadable(_))), "{out:?}");
    assert_eq!(r.free_slots(), pdf::CONCURRENT);
}

/// The largest input the upload limit allows, edited in the helper under its
/// memory cap: the part at its ceiling is edited, and one past it is refused.
#[tokio::test]
async fn the_largest_part_is_edited_inside_the_memory_cap() {
    use harness_docs::limits::MAX_EDIT_PART_BYTES;
    let r = PdfReader::new(EXE.into());
    let head = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>";
    let para = "<w:p><w:r><w:t>0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 abc</w:t></w:r></w:p>";
    let make = |size: usize| {
        let mut doc = String::from(head);
        while doc.len() + para.len() + 20 < size {
            doc.push_str(para);
        }
        doc.push_str("</w:body></w:document>");
        let base = docx_bytes("x\n");
        let p = harness_attached::Package::open(&base, Instant::now() + Duration::from_secs(5))
            .unwrap();
        let mut w = ZipWriter::new(10 << 20);
        for en in p.raw_entries().unwrap() {
            let n = std::str::from_utf8(en.name).unwrap();
            if n == "word/document.xml" {
                // Stored, so the zip itself is as large as the part.
                w.add_stored(n, doc.as_bytes()).unwrap();
            } else {
                w.put_raw(
                    n,
                    en.method,
                    en.crc,
                    en.comp as u32,
                    en.size as u32,
                    en.time,
                    en.date,
                    en.data,
                )
                .unwrap();
            }
        }
        w.finish().unwrap()
    };
    let big = make(MAX_EDIT_PART_BYTES - 1000);
    assert!(big.len() <= 10 << 20, "{}", big.len());
    let view = Request::DocView {
        sheet: None,
        from: 1,
        limit: 2,
    };
    let Some(out) = call(&r, &view, &big).await else {
        return;
    };
    let text = parse_reply(&out.unwrap()).unwrap().0.text;
    assert!(text.contains("p1#"), "{text}");
    let edit = Request::DocEdit {
        edits: vec![
            json!({"op": "replace_text", "at": text.lines().nth(1).unwrap().split_whitespace().next().unwrap(), "find": "abc", "replace": "xyz"}),
        ],
    };
    let t = Instant::now();
    let out = call(&r, &edit, &big).await.unwrap().unwrap();
    let (reply, new) = parse_reply(&out).unwrap();
    assert!(reply.file && reply.results[0].ok, "{reply:?}");
    assert!(new.len() <= 10 << 20);
    eprintln!("largest edit took {:?}", t.elapsed());
    // One past the part ceiling.
    let over = make(MAX_EDIT_PART_BYTES + 1000);
    if over.len() <= 10 << 20 {
        let out = call(&r, &view, &over).await.unwrap();
        assert!(
            matches!(&out, Err(DocError::Unreadable(m)) if m.contains("larger")),
            "{out:?}"
        );
    }
}

/// The preview of a made file, through the real confined helper: a document
/// as blocks, a workbook as cells, and what is no office file as no preview.
#[tokio::test]
async fn the_helper_previews_a_document_a_workbook_and_nothing_else() {
    use harness_docs::edit::preview::{Block, Preview};
    let r = PdfReader::new(EXE.into());
    let req = Request::Preview { sheet: None };
    let Some(out) = call(&r, &req, &docx_bytes("# Title\n\nThe **quick** fox.\n")).await else {
        return;
    };
    let out = out.unwrap();
    let (reply, rest) = parse_reply(&out).unwrap();
    assert!(rest.is_empty());
    let Some(Preview::Doc { blocks, .. }) = reply.preview else {
        panic!("{reply:?}")
    };
    assert!(matches!(&blocks[0], Block::H { l: 1, .. }), "{blocks:?}");
    assert!(
        matches!(&blocks[1], Block::P { r, .. } if r.iter().any(|x| x.t == "quick" && x.b)),
        "{blocks:?}"
    );
    let out = call(&r, &req, &xlsx_bytes()).await.unwrap().unwrap();
    let (reply, _) = parse_reply(&out).unwrap();
    let Some(Preview::Sheet {
        sheets, at, rows, ..
    }) = reply.preview
    else {
        panic!("{reply:?}")
    };
    assert_eq!(sheets[at as usize], "Data");
    assert_eq!(rows[0].c[0].v, "Name");
    // A PDF's bytes: an answer, no preview.
    let out = call(&r, &req, b"%PDF-1.4\n").await.unwrap().unwrap();
    let r = parse_reply(&out).unwrap().0;
    assert!(r.preview.is_none() && r.why == Some(harness_docs::edit::preview::Why::NotOffice));
}

/// A file of a folder, previewed the way the node does it for the page
/// (`harness_tools::folder_preview`, T99, X.59): the node's confined helper,
/// the real one. A hostile document is a sentence-less refusal word, never
/// the helper's own text; a good one is structure and strings.
#[tokio::test]
async fn a_folder_file_is_previewed_through_the_node_helper_and_a_hostile_one_is_refused() {
    use harness_tools::doc_helper::ExeHelper;
    use harness_tools::folder_preview::{Refusal, preview};
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().canonicalize().unwrap();
    let ctx = harness_tools::ToolCtx::new(&ws);
    let helper = ExeHelper::new(EXE.into());
    std::fs::write(
        ws.join("ok.docx"),
        docx_bytes("# Title\n\njavascript:alert(1) and =HYPERLINK(\"http://x\") and \u{202e}rtl\n"),
    )
    .unwrap();
    let got = preview(&ctx, &helper, "ok.docx", None).await;
    if got == Err(Refusal::NotAvailable) {
        assert!(
            std::env::var_os("CI").is_none(),
            "doc-edit cannot confine itself on this CI kernel"
        );
        eprintln!("skipped: doc-edit cannot confine itself here (Landlock ABI 4 needed)");
        return;
    }
    let v = got.unwrap();
    assert_eq!(v["kind"], "doc");
    // Text, nothing else: the script-looking words are strings, and the
    // direction character is dropped by the helper.
    let text = v.to_string();
    assert!(
        text.contains("javascript:alert(1)") && text.contains("=HYPERLINK"),
        "{v}"
    );
    assert!(!text.contains('\u{202e}'), "{v}");
    std::fs::write(ws.join("book.xlsx"), xlsx_bytes()).unwrap();
    let v = preview(&ctx, &helper, "book.xlsx", Some(0)).await.unwrap();
    assert_eq!(v["kind"], "sheet");
    assert_eq!(
        preview(&ctx, &helper, "book.xlsx", Some(7)).await,
        Err(Refusal::NoSheet)
    );
    // A DOCTYPE (an external entity), a renamed PDF, a file that is no zip.
    let base = docx_bytes("one\n");
    let p =
        harness_attached::Package::open(&base, Instant::now() + Duration::from_secs(5)).unwrap();
    let mut w = ZipWriter::new(10 << 20);
    for en in p.raw_entries().unwrap() {
        let n = std::str::from_utf8(en.name).unwrap();
        if n == "word/document.xml" {
            w.add(n, b"<?xml version=\"1.0\"?><!DOCTYPE d [<!ENTITY x SYSTEM \"file:///etc/passwd\">]><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>&x;</w:t></w:r></w:p></w:body></w:document>").unwrap();
        } else {
            w.put_raw(
                n,
                en.method,
                en.crc,
                en.comp as u32,
                en.size as u32,
                en.time,
                en.date,
                en.data,
            )
            .unwrap();
        }
    }
    std::fs::write(ws.join("xxe.docx"), w.finish().unwrap()).unwrap();
    std::fs::write(ws.join("pdf.docx"), b"%PDF-1.4\n").unwrap();
    std::fs::write(ws.join("junk.xlsx"), vec![0x41u8; 5000]).unwrap();
    assert_eq!(
        preview(&ctx, &helper, "xxe.docx", None).await,
        Err(Refusal::Damaged)
    );
    assert_eq!(
        preview(&ctx, &helper, "pdf.docx", None).await,
        Err(Refusal::NotOffice)
    );
    assert_eq!(
        preview(&ctx, &helper, "junk.xlsx", None).await,
        Err(Refusal::NotOffice)
    );
    assert_eq!(helper.free_slots(), harness_tools::doc_helper::CONCURRENT);
}

/// A person's text changes to a file of a folder, saved as a copy through the
/// real confined helper (`harness_tools::folder_edit`, T100, X.60): a changed
/// paragraph and a changed cell are in the copy, the original is
/// byte-identical, a hostile file (a DOCTYPE, a renamed PDF) and a change the
/// engine refuses write nothing.
#[tokio::test]
async fn a_folder_file_is_changed_into_a_copy_through_the_node_helper() {
    use harness_docs::edit::preview::{Block, Preview, preview};
    use harness_tools::doc_helper::ExeHelper;
    use harness_tools::folder_edit::{Change, Edited, edit};
    use harness_tools::folder_preview::Refusal;
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().canonicalize().unwrap();
    let ctx = harness_tools::ToolCtx::new(&ws);
    let helper = ExeHelper::new(EXE.into());
    let doc = docx_bytes("# Title\n\nThe **quick** fox.\n");
    std::fs::write(ws.join("a.docx"), &doc).unwrap();
    let Preview::Doc { blocks, .. } = preview(&doc, None).unwrap() else {
        panic!()
    };
    let at = blocks
        .iter()
        .find_map(|b| match b {
            Block::P { at, .. } => Some(at.clone()),
            _ => None,
        })
        .unwrap();
    let got = edit(
        &ctx,
        &helper,
        "a.docx",
        None,
        vec![Change {
            at: at.clone(),
            text: "A <changed> & \"quoted\" line".into(),
        }],
    )
    .await;
    if got == Err(Refusal::NotAvailable) {
        assert!(
            std::env::var_os("CI").is_none(),
            "doc-edit cannot confine itself on this CI kernel"
        );
        eprintln!("skipped: doc-edit cannot confine itself here (Landlock ABI 4 needed)");
        return;
    }
    let Edited::Saved { path, results } = got.unwrap() else {
        panic!()
    };
    assert_eq!(path, "a (edited).docx");
    assert!(results.iter().all(|r| r.ok));
    assert_eq!(std::fs::read(ws.join("a.docx")).unwrap(), doc);
    let new = std::fs::read(ws.join(&path)).unwrap();
    assert!(
        format!("{:?}", preview(&new, None).unwrap()).contains("A <changed> & \\\"quoted\\\" line")
    );
    // A workbook: a number, a formula and text.
    std::fs::write(ws.join("book.xlsx"), xlsx_bytes()).unwrap();
    let Edited::Saved { path, .. } = edit(
        &ctx,
        &helper,
        "book.xlsx",
        Some(0),
        vec![
            Change {
                at: "B2".into(),
                text: "42".into(),
            },
            Change {
                at: "C2".into(),
                text: "=B2*2".into(),
            },
            Change {
                at: "A2".into(),
                text: "z".into(),
            },
        ],
    )
    .await
    .unwrap() else {
        panic!()
    };
    let Preview::Sheet { rows, .. } =
        preview(&std::fs::read(ws.join(path)).unwrap(), Some(0)).unwrap()
    else {
        panic!()
    };
    let cell = |a: &str| {
        rows.iter()
            .flat_map(|r| r.c.iter())
            .find(|c| c.a == a)
            .cloned()
    };
    assert_eq!(cell("B2").unwrap().v, "42");
    assert_eq!(cell("C2").unwrap().f.as_deref(), Some("=B2*2"));
    assert_eq!(cell("A2").unwrap().v, "z");
    // A formula the engine refuses: nothing is written.
    let before = std::fs::read_dir(&ws).unwrap().count();
    let refused = edit(
        &ctx,
        &helper,
        "book.xlsx",
        Some(0),
        vec![Change {
            at: "B2".into(),
            text: "=WEBSERVICE(\"http://x\")".into(),
        }],
    )
    .await
    .unwrap();
    assert!(matches!(refused, Edited::Refused(_)), "{refused:?}");
    assert_eq!(std::fs::read_dir(&ws).unwrap().count(), before);
    // A DOCTYPE, a renamed PDF, a file that is no zip.
    let p = harness_attached::Package::open(&doc, Instant::now() + Duration::from_secs(5)).unwrap();
    let mut w = ZipWriter::new(10 << 20);
    for en in p.raw_entries().unwrap() {
        let n = std::str::from_utf8(en.name).unwrap();
        if n == "word/document.xml" {
            w.add(n, b"<?xml version=\"1.0\"?><!DOCTYPE d [<!ENTITY x SYSTEM \"file:///etc/passwd\">]><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>&x;</w:t></w:r></w:p></w:body></w:document>").unwrap();
        } else {
            w.put_raw(
                n,
                en.method,
                en.crc,
                en.comp as u32,
                en.size as u32,
                en.time,
                en.date,
                en.data,
            )
            .unwrap();
        }
    }
    std::fs::write(ws.join("xxe.docx"), w.finish().unwrap()).unwrap();
    std::fs::write(ws.join("pdf.docx"), b"%PDF-1.4\n").unwrap();
    let ch = || {
        vec![Change {
            at: at.clone(),
            text: "x".into(),
        }]
    };
    let n = std::fs::read_dir(&ws).unwrap().count();
    assert_eq!(
        edit(&ctx, &helper, "xxe.docx", None, ch()).await,
        Err(Refusal::Damaged)
    );
    assert_eq!(
        edit(&ctx, &helper, "pdf.docx", None, ch()).await,
        Err(Refusal::NotOffice)
    );
    assert_eq!(
        std::fs::read_dir(&ws).unwrap().count(),
        n,
        "nothing written"
    );
}

/// The gate is by the name and the edit by the content, so the helper is told
/// the kind the name says and refuses a file of the other kind (X.60, T100): a
/// workbook named `.docx` on a node that denies `sheet_edit`, and a document
/// named `.xlsx`, write nothing; characters the page could not show are not
/// changed from it either.
#[tokio::test]
async fn a_file_named_as_the_other_kind_or_with_hidden_characters_is_not_changed() {
    use harness_tools::doc_helper::ExeHelper;
    use harness_tools::folder_edit::{Change, edit};
    use harness_tools::folder_preview::Refusal;
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().canonicalize().unwrap();
    let ctx = harness_tools::ToolCtx::new(&ws);
    let helper = ExeHelper::new(EXE.into());
    std::fs::write(ws.join("book.docx"), xlsx_bytes()).unwrap();
    std::fs::write(ws.join("doc.xlsx"), docx_bytes("# T\n\nBody\n")).unwrap();
    let ch = |at: &str| {
        vec![Change {
            at: at.into(),
            text: "x".into(),
        }]
    };
    let got = edit(&ctx, &helper, "book.docx", None, ch("A2")).await;
    if got == Err(Refusal::NotAvailable) {
        assert!(std::env::var_os("CI").is_none(), "cannot confine here");
        eprintln!("skipped: doc-edit cannot confine itself here (Landlock ABI 4 needed)");
        return;
    }
    assert_eq!(got, Err(Refusal::NotOffice));
    assert_eq!(
        edit(&ctx, &helper, "doc.xlsx", None, ch("A1")).await,
        Err(Refusal::NotOffice)
    );
    // Hidden characters in a paragraph and a cell: refused, not rewritten.
    let hidden = harness_docs::docx::build("Body\u{200c}text\n", &Default::default())
        .unwrap()
        .bytes;
    let view = harness_docs::edit::docx::view(&hidden, 1, 400).unwrap();
    let tag = view
        .lines()
        .find(|l| l.contains("Body"))
        .and_then(|l| l.split_whitespace().next())
        .unwrap()
        .to_string();
    std::fs::write(ws.join("h.docx"), &hidden).unwrap();
    let n = std::fs::read_dir(&ws).unwrap().count();
    let r = edit(&ctx, &helper, "h.docx", None, ch(&tag)).await.unwrap();
    assert!(
        matches!(&r, harness_tools::folder_edit::Edited::Refused(v) if v[0].why == Some(harness_tools::folder_edit::ChangeWhy::NotEditable)),
        "{r:?}"
    );
    assert_eq!(
        std::fs::read_dir(&ws).unwrap().count(),
        n,
        "nothing written"
    );
}
