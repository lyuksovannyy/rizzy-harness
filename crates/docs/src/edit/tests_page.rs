//! Changing text from the page (ADOPTION_PLAN X.60, PLAN.md T99): the
//! preview names the places the edit ops take, a typed cell means one thing
//! decided in one place, and a request of changes goes through the existing
//! edit functions, so every check of theirs applies.

use std::collections::HashMap;

use super::docx;
use super::fixtures::*;
use super::page::{Change, ChangeWhy, PageKind, cell_value, strict_number};
use super::preview::{Block, Preview, Run, Why, preview};
use super::serve::{Reply, Request, frame_request, parse_reply, serve};
use super::tests_preview::book;
use super::xlsx::CellVal;
use crate::limits::*;

/// The kind a file's content says (what the caller names it as, in these tests).
fn kind_of(file: &[u8]) -> PageKind {
    if file.windows(17).any(|w| w == b"word/document.xml") {
        PageKind::Doc
    } else {
        PageKind::Sheet
    }
}

fn apply(file: &[u8], sheet: Option<u32>, changes: &[Change]) -> Result<super::page::Applied, Why> {
    super::page::apply(file, kind_of(file), sheet, changes)
}

fn ch(at: &str, text: &str) -> Change {
    Change {
        at: at.into(),
        text: text.into(),
    }
}

fn blocks(b: &[u8]) -> Vec<Block> {
    match preview(b, None).expect("a document") {
        Preview::Doc { blocks, .. } => blocks,
        other => panic!("{other:?}"),
    }
}

fn runs_of(b: &Block) -> Option<&Vec<Run>> {
    match b {
        Block::H { r, .. } | Block::P { r, .. } | Block::Li { r, .. } => Some(r),
        Block::T { .. } => None,
    }
}

fn at_of_block(b: &Block) -> &str {
    match b {
        Block::H { at, .. } | Block::P { at, .. } | Block::Li { at, .. } => at,
        Block::T { .. } => "",
    }
}

/// The tag of the block whose text is `text`.
fn at_of(file: &[u8], text: &str) -> String {
    blocks(file)
        .iter()
        .find(|b| {
            runs_of(b).is_some_and(|r| r.iter().map(|x| x.t.as_str()).collect::<String>() == text)
        })
        .map(|b| at_of_block(b).to_string())
        .unwrap_or_else(|| panic!("no block {text:?}"))
}

fn texts(file: &[u8]) -> Vec<String> {
    blocks(file)
        .iter()
        .filter_map(runs_of)
        .map(|r| r.iter().map(|x| x.t.as_str()).collect())
        .collect()
}

/// `address -> (shown value, formula)` of the sheet at `place`.
fn grid(file: &[u8], place: Option<u32>) -> HashMap<String, (String, Option<String>)> {
    match preview(file, place).expect("a workbook") {
        Preview::Sheet { rows, .. } => rows
            .into_iter()
            .flat_map(|r| r.c)
            .map(|c| (c.a, (c.v, c.f)))
            .collect(),
        other => panic!("{other:?}"),
    }
}

fn whys(o: &super::page::Applied) -> Vec<Option<ChangeWhy>> {
    o.items.iter().map(|i| i.why).collect()
}

// -- the preview names the places the edit tools take ------------------------

#[test]
fn a_preview_names_the_places_doc_view_names() {
    let file = docx_real();
    let view = docx::view(&file, 1, 400).unwrap();
    let bl = blocks(&file);
    let mut named = 0;
    for b in &bl {
        let at = at_of_block(b);
        if at.is_empty() {
            continue;
        }
        named += 1;
        // The same tag, at the start of a line of the view.
        assert!(
            view.lines().any(|l| l.starts_with(&format!("{at} "))),
            "{at} is not a tag of the view:\n{view}"
        );
        assert!(at.starts_with('p') && at.contains('#'), "{at}");
    }
    assert!(named >= 10, "{named}: the body's paragraphs are named");
    // A table is not editable from the page, and says nothing of a tag.
    assert!(bl.iter().any(|b| matches!(b, Block::T { .. })));
    let json = serde_json::to_string(&bl).unwrap();
    assert!(!json.contains("\"at\":\"t1"), "{json}");
    // Additive: a block with no `at` reads (an older node's preview).
    let old: Block = serde_json::from_str(r#"{"k":"p","r":[{"t":"x"}]}"#).unwrap();
    assert_eq!(at_of_block(&old), "");
}

#[test]
fn a_paragraph_the_preview_cut_is_not_named() {
    let long = "x".repeat(MAX_PREVIEW_PARA_CHARS + 1);
    let edge = "y".repeat(MAX_PREVIEW_PARA_CHARS);
    let file = docx_with(
        &format!(
            "{DECL}<w:document {W_NS}><w:body><w:p><w:r><w:t>{long}</w:t></w:r></w:p><w:p><w:r><w:t>{edge}</w:t></w:r></w:p><w:sectPr/></w:body></w:document>"
        ),
        &[],
    );
    let bl = blocks(&file);
    assert_eq!(bl.len(), 2);
    assert_eq!(at_of_block(&bl[0]), "", "cut: its end would be lost");
    assert!(
        !at_of_block(&bl[1]).is_empty(),
        "exactly the limit is whole"
    );
}

#[test]
fn a_cell_the_preview_cut_or_stood_in_for_is_marked_not_editable() {
    let long = "z".repeat(MAX_PREVIEW_CELL_CHARS + 1);
    let edge = "w".repeat(MAX_PREVIEW_CELL_CHARS);
    let formula = format!("1+{}", "1+".repeat(MAX_PREVIEW_FORMULA_CHARS));
    let sheet = format!(
        "{DECL}<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>\
<row r=\"1\"><c r=\"A1\" t=\"inlineStr\"><is><t>{long}</t></is></c><c r=\"B1\" t=\"inlineStr\"><is><t>{edge}</t></is></c><c r=\"C1\"><f>{formula}1</f><v>1</v></c><c r=\"D1\"><f>1+1</f><v>2</v></c></row>\
<row r=\"2\"><c r=\"A2\"><f t=\"shared\" ref=\"A2:A3\" si=\"0\">1+1</f><v>2</v></c></row>\
<row r=\"3\"><c r=\"A3\"><f t=\"shared\" si=\"0\"/><v>2</v></c></row>\
</sheetData></worksheet>"
    );
    let file = xlsx_with(&sheet, "", &[]);
    let Preview::Sheet { rows, .. } = preview(&file, None).unwrap() else {
        panic!()
    };
    let by: HashMap<String, bool> = rows
        .iter()
        .flat_map(|r| r.c.iter().map(|c| (c.a.clone(), c.x)))
        .collect();
    assert!(by["A1"], "cut text");
    assert!(!by["B1"], "exactly the limit is whole");
    assert!(by["C1"], "cut formula");
    assert!(!by["D1"]);
    assert!(by["A3"], "a stand-in for a shared formula");
}

// -- what a typed cell means --------------------------------------------------

#[test]
fn what_a_typed_cell_means_is_decided_in_one_place() {
    let num = |s: &str| match cell_value(s) {
        Ok(CellVal::Num(x)) => x,
        other => panic!("{s:?} -> {other:?}"),
    };
    let text = |s: &str| match cell_value(s) {
        Ok(CellVal::Text(t)) => assert_eq!(t, s),
        other => panic!("{s:?} -> {other:?}"),
    };
    assert_eq!(cell_value(""), Ok(CellVal::Null));
    for (s, want) in [
        ("0", 0.0),
        ("7", 7.0),
        ("-3", -3.0),
        ("+2.5", 2.5),
        ("0.5", 0.5),
        (".5", 0.5),
        ("1e3", 1000.0),
        ("1.5E-2", 0.015),
        ("-0.25e+2", -25.0),
        ("123456789012345", 123456789012345.0),
    ] {
        assert_eq!(num(s), want, "{s}");
    }
    for s in [
        "1,200",
        "1 200",
        " 5",
        "5 ",
        "5\n",
        "1e999",
        "0x10",
        "inf",
        "NaN",
        "Infinity",
        "1.",
        "1.2.3",
        "--1",
        "+",
        "-",
        ".",
        "e5",
        "1e",
        "007",
        "00.5",
        "1234567890123456",
        "12\u{ff10}",
        "5%",
        "$5",
        "1,5",
        "text",
    ] {
        text(s);
    }
    // A formula: a leading `=`, stored without it, checked by the engine's
    // own formula check.
    assert_eq!(
        cell_value("=SUM(A1:A3)"),
        Ok(CellVal::Formula("SUM(A1:A3)".into()))
    );
    assert_eq!(cell_value("=WEBSERVICE(\"x\")"), Err(ChangeWhy::BadFormula));
    assert_eq!(cell_value("="), Err(ChangeWhy::BadFormula));
    assert_eq!(cell_value("=   "), Err(ChangeWhy::BadFormula));
    // A long text is refused at the edit tools' own bound, at its edge.
    let edge = "a".repeat(MAX_EDIT_TEXT_CHARS);
    assert!(matches!(cell_value(&edge), Ok(CellVal::Text(_))));
    assert_eq!(cell_value(&format!("{edge}a")), Err(ChangeWhy::TooLong));
    // The strict rule on its own.
    assert_eq!(strict_number("12"), Some(12.0));
    assert_eq!(strict_number("1e999"), None);
}

// -- a Word file ---------------------------------------------------------------

#[test]
fn changed_paragraphs_make_a_new_file_and_the_rest_is_kept() {
    let file = docx_real();
    let before = texts(&file);
    let h = at_of(&file, "Quarterly report");
    let li = at_of(&file, "second item");
    let o = apply(
        &file,
        None,
        &[ch(&h, "Annual <report> & \"more\""), ch(&li, "last item")],
    )
    .unwrap();
    let out = o.bytes.expect("a file");
    assert!(
        o.items.iter().all(|i| i.ok && i.why.is_none()),
        "{:?}",
        o.items
    );
    let after = texts(&out);
    assert_eq!(after.len(), before.len());
    for (i, (a, b)) in before.iter().zip(&after).enumerate() {
        match b.as_str() {
            "Annual <report> & \"more\"" | "last item" => assert_ne!(a, b),
            _ => assert_eq!(a, b, "block {i}"),
        }
    }
    assert!(after.contains(&"last item".to_string()));
    // The heading keeps its level; the table is what it was.
    assert!(matches!(&blocks(&out)[0], Block::H { l: 1, .. }));
    assert!(blocks(&out).iter().any(|b| matches!(b, Block::T { .. })));
    // The input bytes are not the output's.
    assert_ne!(file, out);
}

/// What `set_text` does with inline formatting, pinned: the paragraph's text
/// becomes one run that keeps the FIRST run's properties; the other runs and
/// their bold and italic go.
#[test]
fn set_text_keeps_the_first_runs_formatting_and_drops_the_others() {
    let file = docx_real();
    let at = at_of(&file, "The quick brown fox jumps");
    let o = apply(&file, None, &[ch(&at, "Hello there")]).unwrap();
    let out = o.bytes.unwrap();
    let bl = blocks(&out);
    let Block::P { r, .. } = bl
        .iter()
        .find(|b| runs_of(b).is_some_and(|r| r.iter().any(|x| x.t == "Hello there")))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        r,
        &vec![Run {
            t: "Hello there".into(),
            b: true,
            i: false,
            s: false
        }],
        "the first run was bold, the second italic"
    );
}

#[test]
fn a_change_that_cannot_be_applied_says_why_and_saves_nothing() {
    let file = docx_real();
    let ok = at_of(&file, "The end.");
    let field = at_of(&file, "Page 3 of the report");
    let other = at_of(&file, "first item");
    let stale = format!("{}#{:04x}", other.split('#').next().unwrap(), 0xbeefu16);
    let gone = "p9999#0000".to_string();
    let o = apply(
        &file,
        None,
        &[
            ch(&ok, "fine"),
            ch(&stale, "x"),
            ch(&gone, "x"),
            ch(&field, "x"),
            ch("not a tag", "x"),
            ch("t1r1c1#0000", "x"),
            ch("p1", "x"),
            ch("p1#zzzz", "x"),
            ch("p1#000", "x"),
            ch("", "x"),
            ch("../p1#0000", "x"),
        ],
    )
    .unwrap();
    assert!(o.bytes.is_none(), "one refusal: nothing is made");
    assert_eq!(
        whys(&o),
        vec![
            Some(ChangeWhy::Skipped),
            Some(ChangeWhy::Stale),
            Some(ChangeWhy::Gone),
            Some(ChangeWhy::NotEditable),
            Some(ChangeWhy::BadPlace),
            Some(ChangeWhy::BadPlace),
            Some(ChangeWhy::BadPlace),
            Some(ChangeWhy::BadPlace),
            Some(ChangeWhy::BadPlace),
            Some(ChangeWhy::BadPlace),
            Some(ChangeWhy::BadPlace),
        ]
    );
    assert!(o.items.iter().all(|i| !i.ok), "{:?}", o.items);
}

#[test]
fn a_text_at_the_bound_is_made_and_one_past_it_is_too_long() {
    let file = docx_real();
    let at = at_of(&file, "The end.");
    let edge = "a".repeat(MAX_EDIT_TEXT_CHARS);
    let o = apply(&file, None, &[ch(&at, &edge)]).unwrap();
    assert!(o.bytes.is_some());
    let o = apply(&file, None, &[ch(&at, &format!("{edge}a"))]).unwrap();
    assert!(o.bytes.is_none());
    assert_eq!(whys(&o), vec![Some(ChangeWhy::TooLong)]);
}

#[test]
fn the_same_place_twice_in_one_request_is_refused() {
    let file = docx_real();
    let at = at_of(&file, "The end.");
    let o = apply(&file, None, &[ch(&at, "one"), ch(&at, "two")]).unwrap();
    assert!(o.bytes.is_none());
    assert_eq!(
        whys(&o),
        vec![Some(ChangeWhy::Skipped), Some(ChangeWhy::Duplicate)]
    );
    // A workbook: `b2` and `B2` are one cell.
    let book = xlsx_real();
    let o = apply(&book, None, &[ch("B2", "1"), ch("b2", "2")]).unwrap();
    assert!(o.bytes.is_none());
    assert_eq!(
        whys(&o),
        vec![Some(ChangeWhy::Skipped), Some(ChangeWhy::Duplicate)]
    );
}

#[test]
fn a_protected_document_is_locked_not_damaged() {
    let file = docx_with(
        &document_xml(),
        &[(
            "word/settings.xml",
            &format!(
                "{DECL}<w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:documentProtection w:edit=\"readOnly\" w:enforcement=\"1\"/></w:settings>"
            ),
        )],
    );
    let at = at_of(&file, "The end.");
    assert_eq!(
        apply(&file, None, &[ch(&at, "x")]).unwrap_err(),
        Why::Locked
    );
}

// -- a workbook ----------------------------------------------------------------

#[test]
fn typed_cells_are_made_a_number_text_a_formula_or_empty() {
    let file = xlsx_real();
    let before = grid(&file, None);
    assert_eq!(before["B3"].0, "250");
    let o = apply(
        &file,
        None,
        &[
            ch("B2", "250.5"),
            ch("A2", "Carol"),
            ch("B3", "=B2*3"),
            ch("B4", ""),
            // An empty cell inside the grid.
            ch("C4", "typed"),
            ch("D1", "12"),
        ],
    )
    .unwrap();
    assert!(o.items.iter().all(|i| i.ok), "{:?}", o.items);
    let out = o.bytes.expect("a file");
    let after = grid(&out, None);
    assert_eq!(after["B2"], ("250.5".into(), None));
    assert_eq!(after["A2"], ("Carol".into(), None));
    assert_eq!(after["B3"].1.as_deref(), Some("=B2*3"));
    assert!(!after.contains_key("B4"), "emptied");
    assert_eq!(after["C4"].0, "typed");
    assert_eq!(after["D1"].0, "12");
    // What was not changed is as it was.
    assert_eq!(after["A1"], before["A1"]);
    assert_eq!(after["A3"], before["A3"]);
}

#[test]
fn a_cell_the_engine_refuses_is_said_so_per_cell() {
    let file = xlsx_real();
    let o = apply(
        &file,
        None,
        &[
            ch("B2", "1"),
            ch("B5", "x"), // inside the merged A5:B5
            ch("D2", "x"), // a shared formula's master
            ch("A7", "x"), // an array formula
            ch("A6", "x"), // a hidden row
            ch("E7", "x"), // a hidden column
            ch("B3", "=WEBSERVICE(\"http://x\")"),
            ch("B", "x"),
            ch("7", "x"),
            ch("ZZZZ9", "x"),
            ch("A0", "x"),
            ch("A1048577", "x"),
            ch("XFE1", "x"),
            ch("B7:C8", "x"),
            ch("", "x"),
        ],
    )
    .unwrap();
    assert!(o.bytes.is_none());
    use ChangeWhy::*;
    assert_eq!(
        whys(&o),
        vec![
            Some(Skipped),
            Some(NotEditable),
            Some(NotEditable),
            Some(NotEditable),
            Some(NotEditable),
            Some(NotEditable),
            Some(BadFormula),
            Some(BadPlace),
            Some(BadPlace),
            Some(BadPlace),
            Some(BadPlace),
            Some(BadPlace),
            Some(BadPlace),
            Some(BadPlace),
            Some(BadPlace),
        ]
    );
}

#[test]
fn the_cell_at_the_last_address_the_engine_allows_is_made() {
    let file = xlsx_real();
    let o = apply(&file, None, &[ch("XFD1048576", "far")]).unwrap();
    assert!(o.bytes.is_some(), "{:?}", o.items);
}

#[test]
fn a_sheet_is_picked_by_its_place_and_a_bad_place_is_no_sheet() {
    let b = book(&[("One", true), ("Two", false), ("Three", true)]);
    let o = apply(&b, Some(1), &[ch("B2", "9")]).unwrap();
    let out = o.bytes.unwrap();
    assert_eq!(grid(&out, Some(1))["B2"].0, "9");
    assert!(
        !grid(&out, Some(0)).contains_key("B2"),
        "the other sheet is as it was"
    );
    // The hidden sheet has no place of its own; past the end; none visible.
    assert_eq!(
        apply(&b, Some(2), &[ch("B2", "9")]).unwrap_err(),
        Why::NoSheet
    );
    assert_eq!(
        apply(&b, Some(u32::MAX), &[ch("B2", "9")]).unwrap_err(),
        Why::NoSheet
    );
    assert_eq!(
        apply(&book(&[("H", false)]), None, &[ch("B2", "9")]).unwrap_err(),
        Why::NoSheet
    );
    // Two sheets of one name: the place would not say which is meant.
    let twin = book(&[("Same", true), ("Same", true)]);
    assert_eq!(
        apply(&twin, Some(1), &[ch("B2", "9")]).unwrap_err(),
        Why::Damaged
    );
}

#[test]
fn a_protected_sheet_is_locked_not_damaged() {
    let sheet = sheet1_xml().replace("<sheetData>", "<sheetProtection sheet=\"1\"/><sheetData>");
    let file = xlsx_with(&sheet, "", &[]);
    assert_eq!(
        apply(&file, None, &[ch("B2", "1")]).unwrap_err(),
        Why::Locked
    );
}

#[test]
fn a_word_file_given_a_cell_and_a_workbook_given_a_tag_are_refused_per_change() {
    let d = docx_real();
    let o = apply(&d, None, &[ch("B2", "x")]).unwrap();
    assert_eq!(whys(&o), vec![Some(ChangeWhy::BadPlace)]);
    let x = xlsx_real();
    let o = apply(&x, None, &[ch("p1#0000", "x")]).unwrap();
    assert_eq!(whys(&o), vec![Some(ChangeWhy::BadPlace)]);
    // Not a Word or Excel file at all.
    assert_eq!(
        apply(b"junk", None, &[ch("A1", "x")]).unwrap_err(),
        Why::NotOffice
    );
    assert_eq!(
        apply(b"PK\x03\x04junk", None, &[ch("A1", "x")]).unwrap_err(),
        Why::Damaged
    );
    assert_eq!(
        apply(b"%PDF-1.4", None, &[ch("A1", "x")]).unwrap_err(),
        Why::NotOffice
    );
}

// -- through the helper's framing ---------------------------------------------

fn through(
    file: &[u8],
    sheet: Option<u32>,
    changes: Vec<Change>,
) -> Result<(Reply, Vec<u8>), String> {
    let out = serve(&frame_request(
        &Request::PageEdit {
            kind: kind_of(file),
            sheet,
            changes,
        },
        file,
    ))?;
    let (r, rest) = parse_reply(&out).expect("a reply");
    Ok((r, rest.to_vec()))
}

#[test]
fn the_helper_answers_one_result_per_change_and_the_file() {
    let file = docx_real();
    let at = at_of(&file, "The end.");
    let (r, new) = through(&file, None, vec![ch(&at, "Done.")]).unwrap();
    assert!(r.file && !new.is_empty());
    assert_eq!(r.results.len(), 1);
    assert!(r.results[0].ok && r.results[0].why.is_none());
    assert!(r.why.is_none());
    assert!(texts(&new).contains(&"Done.".to_string()));
    // A refusal: no file, the typed reason per change and none of the file's
    // words in a `why`.
    let (r, new) = through(&file, None, vec![ch("p1#0000", "x")]).unwrap();
    assert!(!r.file && new.is_empty());
    assert!(r.results[0].why.is_some());
    // A whole-file reason.
    let (r, _) = through(b"junk", None, vec![ch("A1", "x")]).unwrap();
    assert_eq!(r.why, Some(Why::NotOffice));
    assert!(r.results.is_empty() && !r.file);
}

#[test]
fn the_number_of_changes_is_bounded_at_its_edge() {
    let file = xlsx_real();
    let many =
        |n: usize| -> Vec<Change> { (0..n).map(|i| ch(&format!("H{}", i + 9), "1")).collect() };
    assert_eq!(MAX_PAGE_CHANGES, 200);
    let (r, _) = through(&file, None, many(MAX_PAGE_CHANGES)).unwrap();
    assert!(r.file, "{:?}", r.results);
    assert_eq!(r.results.len(), MAX_PAGE_CHANGES);
    assert!(through(&file, None, many(MAX_PAGE_CHANGES + 1)).is_err());
    assert!(through(&file, None, vec![]).is_err(), "nothing to change");
}

#[test]
fn a_request_with_a_field_it_does_not_know_is_refused() {
    let head = br#"{"tool":"page_edit","changes":[{"at":"A1","text":"x","extra":1}]}"#;
    let mut framed = (head.len() as u32).to_be_bytes().to_vec();
    framed.extend_from_slice(head);
    framed.extend_from_slice(&xlsx_real());
    assert!(serve(&framed).is_err());
    let head =
        br#"{"tool":"page_edit","sheet":0,"changes":[{"at":"A1","text":"x"}],"path":"/etc"}"#;
    let mut framed = (head.len() as u32).to_be_bytes().to_vec();
    framed.extend_from_slice(head);
    framed.extend_from_slice(&xlsx_real());
    assert!(serve(&framed).is_err());
}

// -- what the page could not show as it is is not changed from the page -------

/// A paragraph document of one-run paragraphs with the given raw `w:t` texts.
fn doc_of(paras: &[&str]) -> Vec<u8> {
    let body: String = paras
        .iter()
        .map(|t| format!("<w:p><w:r><w:t xml:space=\"preserve\">{t}</w:t></w:r></w:p>"))
        .collect();
    docx_with(
        &format!("{DECL}<w:document {W_NS}><w:body>{body}<w:sectPr/></w:body></w:document>"),
        &[],
    )
}

#[test]
fn a_paragraph_with_characters_the_page_cannot_show_is_not_offered_and_not_changed() {
    let file = doc_of(&[
        "plain",
        "zero\u{200c}width",
        "private\u{e000}use",
        "bidi\u{202e}text",
        "ctl\u{1}char",
    ]);
    let bl = blocks(&file);
    assert_eq!(bl.len(), 5);
    assert!(!at_of_block(&bl[0]).is_empty(), "faithful text is offered");
    // The text shown has lost something, so no place is named for it.
    for b in &bl[1..] {
        assert_eq!(at_of_block(b), "", "{b:?}");
    }
    // A request aimed at it anyway (the tag taken from the file itself, as
    // doc_view names it) is refused by the helper, not by the page.
    let idx = super::docx::view(&file, 1, 400).unwrap();
    let tag_of = |needle: &str| -> String {
        idx.lines()
            .find(|l| l.contains(needle))
            .and_then(|l| l.split_whitespace().next())
            .unwrap_or_else(|| panic!("{needle}: {idx}"))
            .to_string()
    };
    for needle in ["zero", "private", "bidi", "ctl"] {
        let tag = tag_of(needle);
        let o = apply(&file, None, &[ch(&tag, "new")]).unwrap();
        assert_eq!(whys(&o), [Some(ChangeWhy::NotEditable)], "{needle} {tag}");
        assert!(o.bytes.is_none());
    }
    // The faithful one still goes through.
    let o = apply(&file, None, &[ch(&at_of(&file, "plain"), "ok")]).unwrap();
    assert!(o.bytes.is_some());
}

#[test]
fn an_object_placeholder_is_never_written_as_text() {
    // A page break is an object to the preview and shows as `[obj]`.
    let file = docx_with(
        &format!(
            "{DECL}<w:document {W_NS}><w:body><w:p><w:r><w:t>before</w:t></w:r><w:r><w:br w:type=\"page\"/></w:r><w:r><w:t>after</w:t></w:r></w:p><w:p><w:r><w:t>other</w:t></w:r></w:p><w:sectPr/></w:body></w:document>"
        ),
        &[],
    );
    let bl = blocks(&file);
    let shown: String = runs_of(&bl[0])
        .unwrap()
        .iter()
        .map(|r| r.t.as_str())
        .collect();
    assert!(shown.contains("[obj]"), "{shown}");
    assert_eq!(at_of_block(&bl[0]), "", "not offered");
    // Aimed at anyway: refused, and `[obj]` is in no copy.
    let view = super::docx::view(&file, 1, 400).unwrap();
    let tag = view
        .lines()
        .find(|l| l.contains("before"))
        .unwrap_or_else(|| panic!("{view}"))
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let o = apply(&file, None, &[ch(&tag, &shown)]).unwrap();
    assert_eq!(whys(&o), [Some(ChangeWhy::NotEditable)]);
    // Another paragraph changed in the same file leaves this one as it was.
    let o = apply(&file, None, &[ch(&at_of(&file, "other"), "changed")]).unwrap();
    let new = o.bytes.unwrap();
    assert_eq!(
        texts(&new)[0],
        shown,
        "the paragraph with the break is shown the same, as an object"
    );
}

fn sheet_with_cells(cells: &str) -> Vec<u8> {
    xlsx_with(
        &format!(
            "{DECL}<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData><row r=\"1\">{cells}</row></sheetData></worksheet>"
        ),
        "",
        &[],
    )
}

#[test]
fn a_cell_shown_changed_is_marked_and_not_changed() {
    let file = sheet_with_cells(
        "<c r=\"A1\" t=\"inlineStr\"><is><t xml:space=\"preserve\">two\nlines</t></is></c>\
<c r=\"B1\" t=\"inlineStr\"><is><t xml:space=\"preserve\">double  space</t></is></c>\
<c r=\"C1\" t=\"inlineStr\"><is><t>zero\u{200c}width</t></is></c>\
<c r=\"D1\" t=\"inlineStr\"><is><t xml:space=\"preserve\"> lead</t></is></c>\
<c r=\"E1\" t=\"inlineStr\"><is><t>fine</t></is></c>\
<c r=\"F1\"><f>1+\n1</f><v>2</v></c>",
    );
    let Preview::Sheet { rows, .. } = preview(&file, None).unwrap() else {
        panic!()
    };
    let by: HashMap<String, bool> = rows
        .iter()
        .flat_map(|r| r.c.iter().map(|c| (c.a.clone(), c.x)))
        .collect();
    for a in ["A1", "B1", "C1", "D1", "F1"] {
        assert!(by[a], "{a} is not offered");
    }
    assert!(!by["E1"]);
    for a in ["A1", "B1", "C1", "D1", "F1"] {
        let o = apply(&file, None, &[ch(a, "new")]).unwrap();
        assert_eq!(whys(&o), [Some(ChangeWhy::NotEditable)], "{a}");
        assert!(o.bytes.is_none());
    }
    // One refused cell saves nothing, whatever else was asked.
    let o = apply(&file, None, &[ch("E1", "ok"), ch("A1", "x")]).unwrap();
    assert_eq!(
        whys(&o),
        [Some(ChangeWhy::Skipped), Some(ChangeWhy::NotEditable)]
    );
    let o = apply(&file, None, &[ch("E1", "ok"), ch("G1", "new")]).unwrap();
    assert!(o.bytes.is_some());
}

// -- the kind the name says is the kind the file is ---------------------------

#[test]
fn a_file_of_the_other_kind_than_its_name_says_is_not_office_here() {
    let doc = docx_real();
    let book = xlsx_real();
    let at = at_of(&doc, "The end.");
    assert_eq!(
        super::page::apply(&book, PageKind::Doc, None, &[ch(&at, "x")]).unwrap_err(),
        Why::NotOffice
    );
    assert_eq!(
        super::page::apply(&doc, PageKind::Sheet, None, &[ch("A1", "x")]).unwrap_err(),
        Why::NotOffice
    );
    // Through the helper's framing too, and nothing is made.
    let framed = frame_request(
        &Request::PageEdit {
            kind: PageKind::Doc,
            sheet: None,
            changes: vec![ch("A1", "x")],
        },
        &book,
    );
    let out = serve(&framed).unwrap();
    let (r, rest) = parse_reply(&out).unwrap();
    assert_eq!(r.why, Some(Why::NotOffice));
    assert!(!r.file && rest.is_empty());
    // The right kinds work.
    assert!(
        super::page::apply(&doc, PageKind::Doc, None, &[ch(&at, "x")])
            .unwrap()
            .bytes
            .is_some()
    );
}
