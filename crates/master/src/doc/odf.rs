//! OpenDocument text, spreadsheets and presentations: a zip whose
//! `content.xml` holds the document. Only that part is read.

use harness_attached::Package;
use harness_core::text::clean_line;

use super::flow::{CELL_CHARS, Flow, LINE_CHARS, Out, push_capped};
use super::xml::{Ev, Xml, attr, end_name, name};
use super::{MAX_COLS, MAX_ROWS, Res, fail, fail_part};

/// Rows of a sheet that repeat (`number-rows-repeated`) are written at most
/// this many times; an empty one is never written.
const REPEAT_ROWS: usize = 100;

fn content<'p>(pkg: &'p Package<'p>) -> Result<Xml<'p>, String> {
    let r = pkg.read("content.xml").map_err(|e| fail_part(pkg, e))?;
    Ok(Xml::new(r))
}

/// Elements whose subtree is not the document's text: comments, footnotes
/// and tracked changes (which hold the deleted text).
fn skipped(n: &str) -> bool {
    matches!(
        n,
        "annotation" | "note" | "tracked-changes" | "deletion" | "forms"
    )
}

/// Characters a `text:s` stands for, at most.
const MAX_SPACES: usize = 100;

/// The inline elements of a paragraph, as text.
fn inline(e: &quick_xml::events::BytesStart<'_>, flow: &mut Flow) {
    match name(e) {
        "s" => {
            let n = attr(e, "c")
                .and_then(|c| c.parse::<usize>().ok())
                .unwrap_or(1)
                .min(MAX_SPACES);
            flow.text(&" ".repeat(n));
        }
        "tab" | "line-break" => flow.text(" "),
        _ => {}
    }
}

/// Text documents (`.odt`) and presentations (`.odp`): paragraphs, headings,
/// lists and tables; for a presentation a heading per page and its notes.
pub fn text(pkg: &Package<'_>, slides: bool, out: &mut Out) -> Res {
    let mut x = content(pkg)?;
    let mut flow = Flow::default();
    let mut skip = 0usize;
    let mut lists = 0usize;
    let mut page = 0usize;
    let mut notes: Option<Out> = None;
    while !out.full() {
        let ev = x.next().map_err(|f| fail(pkg, f))?;
        match ev {
            Ev::Open(e) => {
                let n = name(&e);
                if skip > 0 {
                    skip += 1;
                    continue;
                }
                if skipped(n) {
                    skip = 1;
                    continue;
                }
                match n {
                    "h" => {
                        flow.begin_para();
                        let l = attr(&e, "outline-level")
                            .and_then(|l| l.parse::<u8>().ok())
                            .unwrap_or(1);
                        if !slides {
                            flow.heading(l);
                        }
                    }
                    "p" => {
                        flow.begin_para();
                        if lists > 0 && !slides {
                            flow.list(u8::try_from(lists - 1).unwrap_or(8));
                        }
                    }
                    "list" => lists += 1,
                    "table" => flow.begin_table(),
                    "table-row" => flow.begin_row(),
                    "table-cell" | "covered-table-cell" => flow.begin_cell(),
                    "page" if slides => {
                        page += 1;
                        out.line(&format!("## Slide {page}"));
                    }
                    "notes" if slides => notes = Some(Out::new(LINE_CHARS * 4)),
                    _ => inline(&e, &mut flow),
                }
            }
            Ev::Close(e) => {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                let sink = notes.as_mut().unwrap_or(out);
                match end_name(&e) {
                    "h" | "p" => flow.end_para(sink),
                    "list" => lists = lists.saturating_sub(1),
                    "table" => flow.end_table(),
                    "table-row" => flow.end_row(sink),
                    "table-cell" | "covered-table-cell" => flow.end_cell(),
                    "notes" => {
                        if let Some(n) = notes.take()
                            && !n.s.is_empty()
                        {
                            out.line("Notes:");
                            for l in n.s.lines() {
                                out.line(l);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ev::Text(t) if skip == 0 => flow.text(&t),
            Ev::Eof => break,
            _ => {}
        }
    }
    Ok(())
}

/// Spreadsheets (`.ods`): per table, rows of tab-joined cell values.
pub fn sheets(pkg: &Package<'_>, out: &mut Out) -> Res {
    let mut x = content(pkg)?;
    let mut skip = 0usize;
    let mut depth = 0usize;
    let mut rows = 0usize;
    let mut cut = false;
    let mut row: Vec<String> = Vec::new();
    let mut row_repeat = 1usize;
    // The cell being read: what it stores, its text, its repeat count.
    let mut stored: Option<String> = None;
    let mut cell = String::new();
    let mut repeat = 1usize;
    let mut in_p = false;
    let mut blank = false;
    // Hidden sheets, rows and columns (`display="false"`, `visibility` that
    // is not `visible`) are not on the person's screen: left out.
    let mut row_hidden = false;
    let (mut col_at, mut cell_at) = (0usize, 0usize);
    let mut hidden_cols: Vec<(usize, usize)> = Vec::new();
    while !out.full() {
        match x.next().map_err(|f| fail(pkg, f))? {
            Ev::Open(e) => {
                let n = name(&e);
                if skip > 0 {
                    skip += 1;
                    continue;
                }
                if skipped(n) || n == "frame" {
                    skip = 1;
                    continue;
                }
                if n == "table" && depth == 0 && attr(&e, "display").as_deref() == Some("false") {
                    out.hidden = true;
                    skip = 1;
                    continue;
                }
                match n {
                    "table-column" if depth == 1 => {
                        let n = repeats(&e, "number-columns-repeated");
                        if !is_visible(&e) && hidden_cols.len() < 1000 {
                            hidden_cols.push((col_at, col_at.saturating_add(n - 1)));
                        }
                        col_at = col_at.saturating_add(n);
                    }
                    "table" => {
                        depth += 1;
                        if depth == 1 {
                            col_at = 0;
                            hidden_cols.clear();
                            rows = 0;
                            cut = false;
                            blank = false;
                            let nm = attr(&e, "name").unwrap_or_default();
                            out.line(&format!("## Sheet: {}", clean_line(&nm, 100)));
                        }
                    }
                    "table-row" if depth == 1 => {
                        row_hidden = !is_visible(&e);
                        cell_at = 0;
                        row.clear();
                        row_repeat = repeats(&e, "number-rows-repeated");
                    }
                    "table-cell" | "covered-table-cell" if depth == 1 => {
                        repeat = repeats(&e, "number-columns-repeated");
                        cell.clear();
                        stored = if n == "covered-table-cell" {
                            None
                        } else {
                            value(&e)
                        };
                    }
                    "p" if depth == 1 => {
                        in_p = true;
                        if !cell.is_empty() {
                            cell.push(' ');
                        }
                    }
                    // `text:s`, `text:tab`, `text:line-break`: a space.
                    "s" | "tab" | "line-break" if depth == 1 && in_p => cell.push(' '),
                    _ => {}
                }
            }
            Ev::Close(e) => {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                match end_name(&e) {
                    "p" => in_p = false,
                    "table-cell" | "covered-table-cell" if depth == 1 => {
                        let v = stored
                            .take()
                            .unwrap_or_else(|| clean_line(&cell, CELL_CHARS));
                        let v = clean_line(&v, CELL_CHARS);
                        // An empty run only moves the column on; none goes past
                        // the column cap.
                        let end = cell_at.saturating_add(repeat);
                        let hidden = !v.is_empty()
                            && (row_hidden
                                || hidden_cols.iter().any(|(a, b)| *a < end && cell_at <= *b));
                        cell_at = end;
                        let v = if hidden {
                            out.hidden = true;
                            String::new()
                        } else {
                            v
                        };
                        for _ in 0..repeat.min(MAX_COLS.saturating_sub(row.len())) {
                            row.push(v.clone());
                        }
                    }
                    "table-row" if depth == 1 => {
                        while row.last().is_some_and(String::is_empty) {
                            row.pop();
                        }
                        if row.is_empty() {
                            blank = rows > 0;
                            continue;
                        }
                        let times = row_repeat.min(REPEAT_ROWS);
                        for _ in 0..times {
                            if rows >= MAX_ROWS {
                                if !cut {
                                    out.line(&format!("[cut: more rows follow after {MAX_ROWS}]"));
                                    cut = true;
                                }
                                break;
                            }
                            if blank {
                                out.line("");
                                blank = false;
                            }
                            out.line(&row.join("\t"));
                            rows += 1;
                        }
                    }
                    "table" => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
            Ev::Text(t) if skip == 0 && depth == 1 && in_p => {
                push_capped(&mut cell, &t, CELL_CHARS * 4);
            }
            Ev::Eof => break,
            _ => {}
        }
    }
    Ok(())
}

/// Whether a row or column is shown (`table:visibility` absent or `visible`).
fn is_visible(e: &quick_xml::events::BytesStart<'_>) -> bool {
    matches!(attr(e, "visibility").as_deref(), None | Some("visible"))
}

fn repeats(e: &quick_xml::events::BytesStart<'_>, key: &str) -> usize {
    attr(e, key)
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1)
        .max(1)
}

/// What a cell stores, as the file says it (not as a sheet would format
/// it); `None` for a text cell, whose text is its content.
fn value(e: &quick_xml::events::BytesStart<'_>) -> Option<String> {
    match attr(e, "value-type").as_deref()? {
        "float" | "percentage" | "currency" => attr(e, "value"),
        "date" => attr(e, "date-value"),
        "time" => attr(e, "time-value"),
        "boolean" => attr(e, "boolean-value").map(|b| b.to_uppercase()),
        _ => None,
    }
}
