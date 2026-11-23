//! Writes the files the owner opens in real Word and Excel before the
//! edit tools merge: each made by `doc_create` / `sheet_create`'s own
//! library, then edited with every operation. Not a test of anything (the
//! other tests read the files back with our own reader only): run with
//! `HARNESS_WRITE_SAMPLES=<dir> cargo test -p harness-docs samples -- --ignored`.

use serde_json::json;

use super::docx::{self, DocOp};
use super::xlsx::{self, SheetOp};

fn tag(view: &str, text: &str) -> String {
    view.lines()
        .find(|l| l.contains(text) && !l.starts_with("Word document"))
        .and_then(|l| l.split_whitespace().next())
        .unwrap_or_else(|| panic!("no {text}"))
        .to_string()
}

#[test]
#[ignore = "writes sample files for opening in Word and Excel"]
fn write_samples() {
    let Some(dir) = std::env::var_os("HARNESS_WRITE_SAMPLES") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let md = "# Quarterly plan\n\nShip the **document tools** before Friday.\n\nThe quick brown fox jumps over the lazy dog.\n\n- reader\n- writer\n- editor\n\n| Owner | Due |\n|---|--:|\n| Dana | 2026-12-01 |\n| Lee | 2026-12-15 |\n\nOld closing line to be deleted.\n\nThe end.\n";
    let doc = crate::docx::build(md, &Default::default()).unwrap().bytes;
    let v = docx::view(&doc, 1, 400).unwrap();
    let ops = vec![
        DocOp::ReplaceText { at: tag(&v, "document tools"), find: "before Friday".into(), replace: "before Monday".into(), all: false },
        DocOp::ReplaceText { at: tag(&v, "quick brown"), find: "o".into(), replace: "0".into(), all: true },
        DocOp::SetText { at: tag(&v, "The end."), text: "The end, as rewritten.".into() },
        DocOp::InsertAfter { at: tag(&v, "Quarterly plan"), markdown: "## Added section\n\nA new paragraph with **bold**, *italic*, ~~strike~~ and `code`.".into() },
        DocOp::InsertAfter { at: tag(&v, "editor"), markdown: "- tester".into() },
        DocOp::InsertBefore { at: tag(&v, "Old closing"), markdown: "Inserted before the old closing line.".into() },
        DocOp::Delete { at: tag(&v, "Old closing") },
        DocOp::ReplaceText { at: tag(&v, "Dana"), find: "Dana".into(), replace: "Dana R.".into(), all: false },
    ];
    let out = docx::edit(&doc, &ops).unwrap();
    for r in &out.results {
        assert!(r.is_ok(), "{:?}", out.results);
    }
    std::fs::write(dir.join("report-before.docx"), &doc).unwrap();
    std::fs::write(dir.join("report-after.docx"), out.bytes.unwrap()).unwrap();

    let spec = json!({"currency": "$", "sheets": [{
        "name": "Budget", "freeze_header": true, "autofilter": true,
        "columns": [{"header": "Item", "width": 24}, {"header": "Cost", "format": "currency"}, {"header": "Due", "format": "date"}],
        "rows": [["Hosting", 1200.5, "2026-12-01"], ["Domain", 15, "2026-12-05"], ["Total", {"f": "SUM(B2:B3)"}, null]],
        "bold_last_row": true }]});
    let book = crate::xlsx::build(&spec, 0).unwrap().bytes;
    let ops = vec![
        SheetOp::parse(&json!({"op": "set", "cells": {"B2": 1350.25, "C2": "2026-12-20", "A3": "Domain name", "D1": "Note", "D2": "raised"}})).unwrap(),
        SheetOp::parse(&json!({"op": "set", "cells": {"B4": {"f": "SUM(B2:B3)*1.2"}}})).unwrap(),
        SheetOp::parse(&json!({"op": "append_rows", "like_row_above": true, "rows": [["Support", 300, "2027-01-15"]]})).unwrap(),
        SheetOp::parse(&json!({"op": "clear", "range": "D1:D2"})).unwrap(),
    ];
    let out = xlsx::edit(&book, "Budget", &ops).unwrap();
    for r in &out.results {
        assert!(r.is_ok(), "{:?}", out.results);
    }
    std::fs::write(dir.join("budget-before.xlsx"), &book).unwrap();
    std::fs::write(dir.join("budget-after.xlsx"), out.bytes.unwrap()).unwrap();
    for n in &out.notes {
        println!("note: {n}");
    }
}
