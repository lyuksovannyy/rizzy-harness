//! The preview of a file as data (PLAN.md T99): the same text `doc_view`
//! shows, bounded, hostile text left as text.

use super::fixtures::*;
use super::preview::{Block, Preview, Run, preview};
use super::serve::{Request, frame_request, parse_reply, serve};
use crate::limits::*;

fn doc_of(body: &str) -> Vec<u8> {
    docx_with(
        &format!("{DECL}<w:document {W_NS}><w:body>{body}<w:sectPr/></w:body></w:document>"),
        &[],
    )
}

fn blocks(b: &[u8]) -> (Vec<Block>, bool, Vec<String>) {
    match preview(b, None).unwrap().expect("a document") {
        Preview::Doc {
            blocks,
            more,
            notes,
        } => (blocks, more, notes),
        other => panic!("{other:?}"),
    }
}

#[allow(clippy::type_complexity)]
fn sheet_of(
    b: &[u8],
    sheet: Option<&str>,
) -> (
    Vec<String>,
    String,
    Vec<super::preview::Row>,
    bool,
    Vec<String>,
) {
    match preview(b, sheet).unwrap().expect("a workbook") {
        Preview::Sheet {
            sheets,
            sheet,
            rows,
            more,
            notes,
        } => (sheets, sheet, rows, more, notes),
        other => panic!("{other:?}"),
    }
}

fn text(r: &[Run]) -> String {
    r.iter().map(|x| x.t.as_str()).collect()
}

fn run(t: &str, b: bool, i: bool, s: bool) -> Run {
    Run {
        t: t.into(),
        b,
        i,
        s,
    }
}

fn plain(t: &str) -> Run {
    run(t, false, false, false)
}

fn size(p: &Preview) -> usize {
    serde_json::to_vec(p).unwrap().len()
}

#[test]
fn a_document_is_blocks_with_the_formatting_of_its_runs() {
    let (bl, more, notes) = blocks(&docx_real());
    assert!(!more);
    // Heading 1 by its style's NAME (the id is `berschrift1`).
    assert_eq!(
        bl[0],
        Block::H {
            l: 1,
            r: vec![plain("Quarterly report")]
        }
    );
    // Split runs keep their own bold and italic; the spelling marks vanish.
    assert_eq!(
        bl[1],
        Block::P {
            r: vec![
                run("The qu", true, false, false),
                run("ick bro", false, true, false),
                plain("wn fox jumps")
            ]
        }
    );
    // A hyperlink is its text and nothing else.
    assert_eq!(
        bl[2],
        Block::P {
            r: vec![plain("Visit our site today")]
        }
    );
    // A field shows its result.
    assert!(matches!(&bl[3], Block::P { r } if text(r) == "Page 3 of the report"));
    // Inserted text is shown, deleted text is not.
    assert!(matches!(&bl[4], Block::P { r } if text(r) == "Keep added end"));
    // Hidden text is left out, and the line says so in a fixed sentence.
    assert!(matches!(&bl[5], Block::P { r } if text(r) == "Visible "));
    // An object is a mark, and the text box inside it is not text.
    assert!(matches!(&bl[6], Block::P { r } if text(r) == "Picture: [obj] after"));
    assert!(matches!(&bl[7], Block::Li { r } if text(r) == "first item"));
    assert!(matches!(&bl[8], Block::Li { r } if text(r) == "second item"));
    assert!(matches!(&bl[9], Block::P { r } if text(r) == "End of section one"));
    assert_eq!(
        bl[10],
        Block::T {
            rows: vec![
                vec!["Name".into(), "Amount".into()],
                vec!["Alice\n(lead)".into(), "1,200".into()],
            ]
        }
    );
    assert!(matches!(&bl[11], Block::P { r } if text(r) == "Contents"));
    assert!(matches!(&bl[12], Block::P { r } if text(r) == "The end."));
    assert_eq!(bl.len(), 13, "the empty last paragraph is not a block");
    let all = serde_json::to_string(&bl).unwrap();
    assert!(!all.contains("secret") && !all.contains("removed") && !all.contains("inside the box"));
    assert!(notes.iter().any(|n| n.contains("Hidden text")), "{notes:?}");
    assert!(
        notes.iter().any(|n| n.contains("deleted text")),
        "{notes:?}"
    );
}

#[test]
fn it_is_the_text_the_view_shows() {
    // Every paragraph the view prints is, with the tag and the style marks
    // cut off, a block's text (or a table's cell), in the same order.
    let b = docx_real();
    let view = super::docx::view(&b, 1, 400).unwrap();
    let want: Vec<String> = view
        .lines()
        .filter(|l| (l.starts_with('p') && l.contains('#')) || l.starts_with("(not editable)"))
        .map(|l| {
            let rest = if l.starts_with("(not editable)") {
                l.trim_start_matches("(not editable)")
            } else {
                l.split_once(' ').unwrap().1
            };
            let rest = rest
                .trim_start_matches(|c| c == ' ')
                .trim_start_matches("[heading 1] ")
                .trim_start_matches("[list paragraph] ")
                .trim_start_matches("[list] ");
            rest.trim().to_string()
        })
        .filter(|t| !t.is_empty())
        .collect();
    let (bl, _, _) = blocks(&b);
    let got: Vec<String> = bl
        .iter()
        .filter_map(|x| match x {
            Block::H { r, .. } | Block::P { r } | Block::Li { r } => {
                Some(text(r).replace('\u{FFFC}', "[obj]"))
            }
            Block::T { .. } => None,
        })
        .collect();
    assert_eq!(got.len(), want.len(), "{got:?} / {want:?}");
    for (g, w) in got.iter().zip(&want) {
        assert_eq!(g.trim(), w.trim());
    }
}

#[test]
fn title_subtitle_strike_and_deep_headings() {
    let b = doc_of(
        "<w:p><w:pPr><w:pStyle w:val=\"Title\"/></w:pPr><w:r><w:t>T</w:t></w:r></w:p>\
         <w:p><w:pPr><w:pStyle w:val=\"Subtitle\"/></w:pPr><w:r><w:t>S</w:t></w:r></w:p>\
         <w:p><w:pPr><w:pStyle w:val=\"Heading3\"/></w:pPr><w:r><w:t>H3</w:t></w:r></w:p>\
         <w:p><w:pPr><w:pStyle w:val=\"Heading9\"/></w:pPr><w:r><w:t>H9</w:t></w:r></w:p>\
         <w:p><w:r><w:rPr><w:strike/><w:b w:val=\"0\"/></w:rPr><w:t>gone</w:t></w:r><w:r><w:rPr><w:dstrike/><w:i/><w:b/></w:rPr><w:t>twice</w:t></w:r></w:p>",
    );
    let (bl, _, _) = blocks(&b);
    assert_eq!(
        bl[..4],
        [
            Block::H {
                l: 1,
                r: vec![plain("T")]
            },
            Block::H {
                l: 2,
                r: vec![plain("S")]
            },
            Block::H {
                l: 3,
                r: vec![plain("H3")]
            },
            Block::H {
                l: 6,
                r: vec![plain("H9")]
            },
        ]
    );
    assert_eq!(
        bl[4],
        Block::P {
            r: vec![
                run("gone", false, false, true),
                run("twice", true, true, true)
            ]
        }
    );
}

#[test]
fn hostile_text_stays_text_and_invisible_characters_are_not_shown() {
    let evil = "<script>alert(1)</script> javascript:alert(1) \u{202e}evil\u{200b}\u{0}\u{7} <img src=x onerror=1>";
    let b = doc_of(&format!(
        "<w:p><w:hyperlink r:id=\"rId5\"><w:r><w:t>{}</w:t></w:r></w:hyperlink></w:p>\
         <w:tbl><w:tr><w:tc><w:p><w:r><w:t>=cmd|' /C calc'!A0\u{202e}</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
        evil.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('\u{0}', "")
    ));
    let (bl, _, _) = blocks(&b);
    let Block::P { r } = &bl[0] else {
        panic!("{bl:?}")
    };
    let t = text(r);
    assert!(t.contains("<script>alert(1)</script> javascript:alert(1)"));
    assert!(
        !t.contains('\u{202e}') && !t.contains('\u{200b}') && !t.contains('\u{7}'),
        "{t:?}"
    );
    // No link is made from the document: the type has no address in it.
    let json = serde_json::to_string(&bl).unwrap();
    assert!(
        !json.contains("example.com") && !json.contains("href"),
        "{json}"
    );
    let Block::T { rows } = &bl[1] else {
        panic!("{bl:?}")
    };
    assert_eq!(rows[0][0], "=cmd|' /C calc'!A0");
}

#[test]
fn a_hundred_thousand_paragraphs_give_a_bounded_reply_that_says_more() {
    let body: String = (0..100_000)
        .map(|i| format!("<w:p><w:r><w:t>line {i}</w:t></w:r></w:p>"))
        .collect();
    let p = preview(&doc_of(&body), None).unwrap().unwrap();
    let Preview::Doc { blocks, more, .. } = &p else {
        panic!()
    };
    assert!(*more);
    assert_eq!(blocks.len(), MAX_PREVIEW_BLOCKS);
    assert!(size(&p) < 262_144, "{}", size(&p));
}

#[test]
fn the_block_limit_holds_at_its_edge_and_one_past() {
    let make = |n: usize| -> Vec<u8> {
        doc_of(
            &(0..n)
                .map(|i| format!("<w:p><w:r><w:t>{i}</w:t></w:r></w:p>"))
                .collect::<String>(),
        )
    };
    let (b, more, _) = blocks(&make(MAX_PREVIEW_BLOCKS));
    assert_eq!((b.len(), more), (MAX_PREVIEW_BLOCKS, false));
    let (b, more, _) = blocks(&make(MAX_PREVIEW_BLOCKS + 1));
    assert_eq!((b.len(), more), (MAX_PREVIEW_BLOCKS, true));
}

#[test]
fn big_paragraphs_and_big_tables_are_cut_and_the_worst_case_fits() {
    // One paragraph of a megabyte, and quotes (JSON doubles them).
    let big = "\"".repeat(100_000);
    let (bl, more, _) = blocks(&doc_of(&format!(
        "<w:p><w:r><w:t>{}</w:t></w:r></w:p>",
        big.replace('"', "&quot;")
    )));
    assert!(more);
    let Block::P { r } = &bl[0] else { panic!() };
    assert!(text(r).chars().count() <= MAX_PREVIEW_PARA_CHARS + 1);
    // Many long paragraphs of quotes: the byte budget holds the reply.
    let para = format!(
        "<w:p><w:r><w:t>{}</w:t></w:r></w:p>",
        "&quot;".repeat(MAX_PREVIEW_PARA_CHARS)
    );
    let p = preview(&doc_of(&para.repeat(200)), None).unwrap().unwrap();
    assert!(size(&p) < 262_144, "{}", size(&p));
    // A table of 40,000 cells: cut at the cell limit.
    let rows: String = (0..2_000)
        .map(|r| {
            format!(
                "<w:tr>{}</w:tr>",
                (0..20)
                    .map(|c| format!("<w:tc><w:p><w:r><w:t>{r}x{c}</w:t></w:r></w:p></w:tc>"))
                    .collect::<String>()
            )
        })
        .collect();
    let (bl, more, _) = blocks(&doc_of(&format!("<w:tbl>{rows}</w:tbl>")));
    assert!(more);
    let Block::T { rows } = &bl[0] else { panic!() };
    assert!(rows.iter().map(Vec::len).sum::<usize>() <= MAX_PREVIEW_TABLE_CELLS);
    // A row of 200 columns keeps the first MAX_PREVIEW_COLS.
    let wide: String = (0..200)
        .map(|c| format!("<w:tc><w:p><w:r><w:t>c{c}</w:t></w:r></w:p></w:tc>"))
        .collect();
    let (bl, more, _) = blocks(&doc_of(&format!("<w:tbl><w:tr>{wide}</w:tr></w:tbl>")));
    assert!(more);
    let Block::T { rows } = &bl[0] else { panic!() };
    assert_eq!(rows[0].len(), MAX_PREVIEW_COLS);
}

#[test]
fn what_is_not_an_office_file_is_none() {
    assert!(preview(b"just text", None).unwrap().is_none());
    assert!(preview(b"%PDF-1.7\n1 0 obj", None).unwrap().is_none());
    assert!(preview(b"", None).unwrap().is_none());
    // A zip with neither part.
    let mut w = crate::zip::ZipWriter::new(1 << 20);
    w.add("a.txt", b"x").unwrap();
    assert!(preview(&w.finish().unwrap(), None).unwrap().is_none());
    // A document with a DOCTYPE is refused with a sentence, not None.
    let b = docx_with(
        "<?xml version=\"1.0\"?><!DOCTYPE x [<!ENTITY a \"b\">]><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body/></w:document>",
        &[],
    );
    assert!(preview(&b, None).is_err());
}

// -- workbooks ---------------------------------------------------------------

#[test]
fn a_sheet_has_addresses_values_formulas_and_dates_and_leaves_hidden_things_out() {
    let (sheets, name, rows, more, notes) = sheet_of(&xlsx_real(), None);
    assert_eq!(
        (sheets, name.as_str(), more),
        (vec!["Data".to_string()], "Data", false)
    );
    let cell = |r: u32, a: &str| {
        rows.iter()
            .find(|x| x.n == r)
            .unwrap()
            .c
            .iter()
            .find(|c| c.a == a)
            .cloned()
    };
    assert_eq!(cell(1, "A1").unwrap().v, "Name");
    assert_eq!(cell(2, "A2").unwrap().v, "Alice");
    assert_eq!(cell(2, "B2").unwrap().v, "100");
    // Dates as the view shows them (the date; the view's "(date)" mark is
    // the model's, a person sees a date).
    assert_eq!(cell(2, "C2").unwrap().v, "2025-12-09");
    // A formula shows its formula, no value; a shared one's follower says so.
    let d2 = cell(2, "D2").unwrap();
    assert_eq!((d2.v.as_str(), d2.f.as_deref()), ("", Some("=B2*2")));
    assert_eq!(
        cell(3, "D3").unwrap().f.as_deref(),
        Some("=(shared formula)")
    );
    // Inline string runs join.
    assert_eq!(cell(4, "A4").unwrap().v, "Cy");
    // A hidden row, a hidden column and the empty styled cell B5 are not there.
    assert!(rows.iter().all(|r| r.n != 6));
    assert!(cell(7, "E7").is_none());
    assert!(cell(5, "B5").is_none());
    assert_eq!(cell(7, "A7").unwrap().f.as_deref(), Some("=SUM(B2:B4)"));
    assert!(notes.iter().any(|n| n.contains("Hidden rows")), "{notes:?}");
    // Rows come in order.
    assert!(rows.windows(2).all(|w| w[0].n < w[1].n));
}

#[test]
fn a_hidden_or_unknown_sheet_is_refused_with_a_sentence() {
    let b = xlsx_real();
    let e = preview(&b, Some("Old")).unwrap_err();
    assert!(e.contains("hidden"), "{e}");
    let e = preview(&b, Some("Nope")).unwrap_err();
    assert!(e.contains("no sheet"), "{e}");
    let (_, name, _, _, _) = sheet_of(&b, Some("data"));
    assert_eq!(name, "Data");
}

#[test]
fn hostile_cells_are_text() {
    let sheet = format!(
        "{DECL}<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>\
         <row r=\"1\"><c r=\"A1\" t=\"inlineStr\"><is><t>=cmd|' /C calc'!A0\u{202e}</t></is></c>\
         <c r=\"B1\"><f>HYPERLINK(\"http://evil.example/\",\"x\")</f><v>0</v></c>\
         <c r=\"C1\" t=\"inlineStr\"><is><t>&lt;script&gt;alert(1)&lt;/script&gt;</t></is></c></row>\
         </sheetData></worksheet>"
    );
    let (_, _, rows, _, _) = sheet_of(&xlsx_with(&sheet, "", &[]), None);
    let c = &rows[0].c;
    assert_eq!(c[0].v, "=cmd|' /C calc'!A0");
    assert_eq!(
        c[1].f.as_deref(),
        Some("=HYPERLINK(\"http://evil.example/\",\"x\")")
    );
    assert_eq!(c[2].v, "<script>alert(1)</script>");
}

#[test]
fn a_huge_sheet_is_cut_at_the_row_and_column_limits() {
    let make = |rows: usize, cols: usize| -> Vec<u8> {
        let body: String = (1..=rows)
            .map(|r| {
                format!(
                    "<row r=\"{r}\">{}</row>",
                    (1..=cols)
                        .map(|c| format!(
                            "<c r=\"{}{r}\"><v>{c}</v></c>",
                            super::xlsx::col_name(c as u32)
                        ))
                        .collect::<String>()
                )
            })
            .collect();
        xlsx_with(
            &format!(
                "{DECL}<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>{body}</sheetData></worksheet>"
            ),
            "",
            &[],
        )
    };
    let (_, _, rows, more, _) = sheet_of(&make(MAX_PREVIEW_ROWS, 3), None);
    assert_eq!((rows.len(), more), (MAX_PREVIEW_ROWS, false));
    let (_, _, rows, more, _) = sheet_of(&make(MAX_PREVIEW_ROWS + 1, 3), None);
    assert_eq!((rows.len(), more), (MAX_PREVIEW_ROWS, true));
    let (_, _, rows, more, _) = sheet_of(&make(3, MAX_PREVIEW_COLS + 1), None);
    assert_eq!((rows[0].c.len(), more), (MAX_PREVIEW_COLS, true));
    // Many rows of long text: the byte budget holds the reply.
    let long = "q".repeat(9_000);
    let body: String = (1..=100)
        .map(|r| {
            format!(
                "<row r=\"{r}\">{}</row>",
                (1..=5)
                    .map(|c| format!(
                        "<c r=\"{}{r}\" t=\"inlineStr\"><is><t>{long}</t></is></c>",
                        super::xlsx::col_name(c)
                    ))
                    .collect::<String>()
            )
        })
        .collect();
    let b = xlsx_with(
        &format!(
            "{DECL}<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>{body}</sheetData></worksheet>"
        ),
        "",
        &[],
    );
    let p = preview(&b, None).unwrap().unwrap();
    assert!(size(&p) < 262_144, "{}", size(&p));
    assert!(matches!(p, Preview::Sheet { more: true, .. }));
}

// -- the helper --------------------------------------------------------------

#[test]
fn the_helper_answers_a_preview_request_and_nothing_else_is_accepted() {
    let out = serve(&frame_request(
        &Request::Preview { sheet: None },
        &docx_real(),
    ))
    .unwrap();
    let (reply, rest) = parse_reply(&out).unwrap();
    assert!(rest.is_empty() && !reply.file);
    assert!(matches!(reply.preview, Some(Preview::Doc { .. })));
    // Not an office file: an answer with no preview, not a failure.
    let out = serve(&frame_request(
        &Request::Preview { sheet: None },
        b"%PDF-1.4",
    ))
    .unwrap();
    assert!(parse_reply(&out).unwrap().0.preview.is_none());
    // A request with another field is not understood.
    let head = br#"{"tool":"preview","sheet":null,"edits":[]}"#;
    let mut inp = (head.len() as u32).to_be_bytes().to_vec();
    inp.extend_from_slice(head);
    assert!(serve(&inp).is_err());
    // A refusal inside a real document is a sentence.
    let e = serve(&frame_request(
        &Request::Preview {
            sheet: Some("Old".into()),
        },
        &xlsx_real(),
    ))
    .unwrap_err();
    assert!(e.contains("hidden"), "{e}");
}

#[test]
fn the_table_cell_limit_holds_at_its_edge_and_one_past() {
    let table = |rows: usize, cols: usize| {
        let body: String = (0..rows)
            .map(|r| {
                format!(
                    "<w:tr>{}</w:tr>",
                    (0..cols)
                        .map(|_| format!("<w:tc><w:p><w:r><w:t>{r}</w:t></w:r></w:p></w:tc>"))
                        .collect::<String>()
                )
            })
            .collect();
        blocks(&doc_of(&format!("<w:tbl>{body}</w:tbl>")))
    };
    let n = |b: &[Block]| match &b[0] {
        Block::T { rows } => rows.iter().map(Vec::len).sum::<usize>(),
        _ => 0,
    };
    let (b, more, _) = table(100, 50);
    assert_eq!((n(&b), more), (MAX_PREVIEW_TABLE_CELLS, false));
    let (b, more, _) = table(101, 50);
    assert_eq!((n(&b), more), (MAX_PREVIEW_TABLE_CELLS, true));
}
