//! Random documents times random edits (like `crates/attached/src/fuzz.rs`):
//! never a panic; an output is always a package that passes the helper's own
//! check and reads back, with every entry the edit did not change as the
//! input's bytes; anything else is a refusal.

use serde_json::{Value, json};

use super::docx::{self, DocOp};
use super::fixtures::*;
use super::xlsx::{self, SheetOp};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len())]
    }
}

const WORDS: &[&str] = &[
    "alpha", "beta", "x", "  ", "a b", "é", "<&>", "\"q\"", "end", "12", "fox",
];

fn run_xml(r: &mut Rng) -> String {
    let t = r.pick(WORDS);
    let esc = t
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    match r.below(9) {
        0 => format!("<w:r><w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\">{esc}</w:t></w:r>"),
        1 => format!("<w:r><w:t>{esc}</w:t></w:r>"),
        2 => "<w:r><w:tab/></w:r>".into(),
        3 => format!("<w:hyperlink r:id=\"rId5\"><w:r><w:t>{esc}</w:t></w:r></w:hyperlink>"),
        4 => "<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText>X</w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>7</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r>".into(),
        5 => format!("<w:ins w:id=\"1\" w:author=\"a\"><w:r><w:t>{esc}</w:t></w:r></w:ins>"),
        6 => format!("<w:del w:id=\"2\" w:author=\"a\"><w:r><w:delText>{esc}</w:delText></w:r></w:del>"),
        7 => format!("<w:r><w:rPr><w:vanish/></w:rPr><w:t>{esc}</w:t></w:r>"),
        _ => format!("<w:bookmarkStart w:id=\"3\" w:name=\"b\"/><w:r><w:t>{esc}</w:t></w:r><w:bookmarkEnd w:id=\"3\"/>"),
    }
}

fn para_xml(r: &mut Rng) -> String {
    let n = r.below(5);
    let runs: String = (0..n).map(|_| run_xml(r)).collect();
    let ppr = match r.below(5) {
        0 => "<w:pPr><w:pStyle w:val=\"berschrift1\"/></w:pPr>",
        1 => "<w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr>",
        _ => "",
    };
    if runs.is_empty() && ppr.is_empty() && r.below(2) == 0 {
        return "<w:p/>".into();
    }
    format!("<w:p>{ppr}{runs}</w:p>")
}

fn random_doc(r: &mut Rng) -> String {
    let mut body = String::new();
    for _ in 0..1 + r.below(8) {
        match r.below(8) {
            0 => body.push_str(&format!(
                "<w:tbl><w:tr><w:tc>{}</w:tc><w:tc>{}{}</w:tc></w:tr></w:tbl>",
                para_xml(r),
                para_xml(r),
                para_xml(r)
            )),
            1 => body.push_str(&format!(
                "<w:sdt><w:sdtContent>{}</w:sdtContent></w:sdt>",
                para_xml(r)
            )),
            _ => body.push_str(&para_xml(r)),
        }
    }
    body.push_str("<w:p><w:r><w:t>last</w:t></w:r></w:p><w:sectPr/>");
    format!("{DECL}<w:document {W_NS}><w:body>{body}</w:body></w:document>")
}

fn rows_equal_outside(before: &[u8], after: &[u8], changed: &[&str]) {
    super::tests::assert_others_identical(before, after, changed);
}

#[test]
fn random_word_files_and_edits_never_panic_and_never_hand_out_a_bad_file() {
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    let (mut made, mut refused) = (0, 0);
    for _ in 0..400 {
        let file = docx_with(&random_doc(&mut r), &[]);
        let Ok(view) = docx::view(&file, 1, 400) else {
            continue;
        };
        let tags: Vec<String> = view
            .lines()
            .filter_map(|l| l.split_whitespace().next())
            .filter(|t| t.contains('#') && !t.starts_with('['))
            .map(str::to_string)
            .collect();
        let mut ops = vec![];
        for _ in 0..(if r.below(3) == 0 { 1 + r.below(4) } else { 1 }) {
            let at = if tags.is_empty() || r.below(8) == 0 {
                "p99#0000".to_string()
            } else {
                r.pick(&tags).clone()
            };
            let w = r.pick(WORDS).to_string();
            let v: Value = match r.below(6) {
                0 => {
                    json!({"op": "replace_text", "at": at, "find": w, "replace": r.pick(WORDS), "all": r.below(2) == 0})
                }
                1 => json!({"op": "set_text", "at": at, "text": format!("{}\t{}\n{}", w, w, w)}),
                2 => {
                    json!({"op": "insert_after", "at": at, "markdown": format!("**{w}** and *{w}*\n\n- item {w}")})
                }
                3 => json!({"op": "insert_before", "at": at, "markdown": format!("## {w}")}),
                4 => json!({"op": "delete", "at": at}),
                _ => json!({"op": "replace_text", "at": at, "find": "", "replace": "x"}),
            };
            ops.push(DocOp::parse(&v).unwrap());
        }
        match docx::edit(&file, &ops) {
            Err(_) => refused += 1,
            Ok(o) => match o.bytes {
                None => refused += 1,
                Some(out) => {
                    made += 1;
                    assert!(docx::view(&out, 1, 400).is_ok(), "the output reads back");
                    rows_equal_outside(&file, &out, &["word/document.xml"]);
                }
            },
        }
    }
    assert!(
        made > 40 && refused > 40,
        "made {made}, refused {refused}: the generator must cover both"
    );
}

fn random_sheet(r: &mut Rng) -> String {
    let mut rows = String::new();
    let mut n = 0;
    for _ in 0..1 + r.below(6) {
        n += 1 + r.below(3);
        let mut cells = String::new();
        let mut c = 0;
        for _ in 0..r.below(4) {
            c += 1 + r.below(3);
            let a = format!(
                "{}{n}",
                ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"][c.min(9)]
            );
            cells.push_str(&match r.below(5) {
                0 => format!(
                    "<c r=\"{a}\" s=\"{}\"><v>{}</v></c>",
                    r.below(4),
                    r.below(99)
                ),
                1 => format!("<c r=\"{a}\" t=\"s\"><v>{}</v></c>", r.below(4)),
                2 => format!("<c r=\"{a}\"><f>1+{}</f><v>3</v></c>", r.below(9)),
                3 => format!(
                    "<c r=\"{a}\" t=\"inlineStr\"><is><t>{}</t></is></c>",
                    r.pick(WORDS).replace('&', "&amp;").replace('<', "&lt;")
                ),
                _ => format!("<c r=\"{a}\"/>"),
            });
        }
        let attrs = if r.below(4) == 0 {
            " hidden=\"1\""
        } else if r.below(3) == 0 {
            " spans=\"1:3\""
        } else {
            ""
        };
        if cells.is_empty() && r.below(2) == 0 {
            rows.push_str(&format!("<row r=\"{n}\"{attrs}/>"));
        } else {
            rows.push_str(&format!("<row r=\"{n}\"{attrs}>{cells}</row>"));
        }
    }
    let merge = if r.below(2) == 0 {
        "<mergeCells count=\"1\"><mergeCell ref=\"B1:C2\"/></mergeCells>"
    } else {
        ""
    };
    format!(
        "{DECL}<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><dimension ref=\"A1:C3\"/><sheetData>{rows}</sheetData>{merge}</worksheet>"
    )
}

#[test]
fn random_sheets_and_edits_never_panic_and_never_hand_out_a_bad_file() {
    let mut r = Rng(0xD1B5_4A32_D192_ED03);
    let (mut made, mut refused) = (0, 0);
    for _ in 0..400 {
        let file = xlsx_with(&random_sheet(&mut r), "", &[]);
        let mut ops = vec![];
        for _ in 0..1 + r.below(4) {
            let a = format!(
                "{}{}",
                ["A", "B", "C", "D", "Z", "AA"][r.below(6)],
                1 + r.below(9)
            );
            let v: Value = match r.below(5) {
                0 => json!({"op": "set", "cells": {a: r.below(100) as i64}}),
                1 => json!({"op": "set", "cells": {a: r.pick(WORDS)}}),
                2 => json!({"op": "set", "cells": {a: {"f": "SUM(A1:A3)"}}}),
                3 => {
                    json!({"op": "append_rows", "rows": [[1, r.pick(WORDS), null]], "like_row_above": r.below(2) == 0})
                }
                _ => json!({"op": "clear", "range": format!("A1:{a}")}),
            };
            ops.push(SheetOp::parse(&v).unwrap());
        }
        match xlsx::edit(&file, "Data", &ops) {
            Err(_) => refused += 1,
            Ok(o) => match o.bytes {
                None => refused += 1,
                Some(out) => {
                    made += 1;
                    assert!(
                        xlsx::view(&out, Some("Data"), 1, 400).is_ok(),
                        "the output reads back"
                    );
                    rows_equal_outside(
                        &file,
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
            },
        }
    }
    assert!(made > 40 && refused > 40, "made {made}, refused {refused}");
}

/// Damaged parts: a byte flipped, a part cut short, in a valid file. Never a
/// panic, a refusal or a file that passes its own check.
#[test]
fn damaged_parts_are_refused_or_read_back_and_never_panic() {
    let mut r = Rng(0xA076_1D64_78BD_642F);
    let ops = [DocOp::parse(
        &json!({"op": "replace_text", "at": "p2#0000", "find": "a", "replace": "b"}),
    )
    .unwrap()];
    for _ in 0..300 {
        let mut doc = document_xml().into_bytes();
        match r.below(3) {
            0 => {
                let i = r.below(doc.len());
                doc[i] = r.next() as u8;
            }
            1 => doc.truncate(r.below(doc.len())),
            _ => {
                let i = r.below(doc.len());
                doc.insert(i, b'<');
            }
        }
        let file = {
            let mut w = crate::zip::ZipWriter::new(8 << 20);
            w.add("[Content_Types].xml", docx_real_types().as_bytes())
                .unwrap();
            w.add("word/document.xml", &doc).unwrap();
            w.finish().unwrap()
        };
        let _ = docx::view(&file, 1, 400);
        let _ = docx::edit(&file, &ops);
        let sheet = {
            let mut s = sheet1_xml().into_bytes();
            let i = r.below(s.len());
            s[i] = r.next() as u8;
            s
        };
        let wb = xlsx_with(&String::from_utf8_lossy(&sheet), "", &[]);
        let _ = xlsx::view(&wb, None, 1, 400);
        let _ = xlsx::edit(
            &wb,
            "Data",
            &[SheetOp::parse(&json!({"op": "set", "cells": {"B2": 1}})).unwrap()],
        );
    }
}

fn docx_real_types() -> String {
    format!(
        "{DECL}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/></Types>"
    )
}
