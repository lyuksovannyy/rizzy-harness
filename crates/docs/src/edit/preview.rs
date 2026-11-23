//! A file as data for the web page's side panel (PLAN.md T99): what a person
//! sees of a Word document (blocks of text with bold, italic and strike) or
//! of one sheet of a workbook (cells with address, value and formula). It is
//! read by the same code as `doc_view` (`docx::index`, `para::scan`,
//! `xlsx::index`, `xlsx::scan_row`), so what the view leaves out (hidden text,
//! tracked deletions, objects, hidden rows, columns and sheets) is left out
//! here the same way. Nothing is HTML and no part of it is a link: the page
//! draws every string as text. Like every edit-engine function it runs only in
//! the confined helper.
//!
//! The keys are short and stable (they are the reply of the helper and of the
//! master's route). A block or a cell can later get an optional `tag` field
//! (the address the edit tools take) without breaking a reader.

use serde::{Deserialize, Serialize};

use super::docx;
use super::para::{self, OBJ};
use super::pkg::Source;
use super::scan::NS_W;
use super::xlsx::{self, Val};
use crate::limits::*;

/// A stretch of text with one direct formatting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub t: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub b: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub i: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub s: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// One block of a document, in reading order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "k", rename_all = "snake_case")]
pub enum Block {
    /// A heading of level 1 to 6 (a title is 1, a subtitle 2).
    H {
        l: u8,
        r: Vec<Run>,
    },
    P {
        r: Vec<Run>,
    },
    /// A list item.
    Li {
        r: Vec<Run>,
    },
    /// A table: rows of cells, a cell its lines joined with `\n`.
    T {
        rows: Vec<Vec<String>>,
    },
}

/// One cell with something in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    /// `B7`.
    pub a: String,
    /// The shown value (empty for a formula: nothing here is calculated).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub v: String,
    /// The formula as `doc_view` prints it (`=B2*2`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub f: Option<String>,
}

/// A row of a sheet that has cells.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    pub n: u32,
    pub c: Vec<Cell>,
}

/// What a file looks like, cut to the limits of [`crate::limits`]; `more` is
/// set when anything was left out for size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Preview {
    Doc {
        blocks: Vec<Block>,
        more: bool,
        /// Fixed sentences about what is left out on purpose.
        notes: Vec<String>,
    },
    Sheet {
        /// The visible sheets.
        sheets: Vec<String>,
        /// The place of the one shown among `sheets`.
        at: u32,
        rows: Vec<Row>,
        more: bool,
        notes: Vec<String>,
    },
}

/// Why a file has no preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Why {
    /// Not a Word or Excel file.
    NotOffice,
    /// Protected with a password (an encrypted container).
    Protected,
    /// Past a limit of the reader (a part, the paragraphs, the rows).
    TooLarge,
    /// Cut short, protected or signed in a way the reader refuses, or
    /// malformed.
    Damaged,
    /// A workbook with no sheet at that place (or none visible).
    NoSheet,
}

/// The preview of a Word or Excel file, or why there is none. `sheet` is the
/// place of the sheet among the visible ones (a workbook's names are text of
/// the file and cannot be sent back reliably).
pub fn preview(file: &[u8], sheet: Option<u32>) -> std::result::Result<Preview, Why> {
    // An encrypted Office file is an OLE container (`D0 CF 11 E0`), not a zip.
    if file.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
        return Err(Why::Protected);
    }
    if !file.starts_with(b"PK") {
        return Err(Why::NotOffice);
    }
    let src = Source::open(file).map_err(|e| why_of(&e))?;
    if src.has("word/document.xml") {
        doc(&src)
    } else if src.has(xlsx::WORKBOOK) {
        sheet_of(&src, sheet)
    } else {
        Err(Why::NotOffice)
    }
}

/// A refusal of the reader as a kind: a limit, or damage.
fn why_of(e: &str) -> Why {
    if crate::edit::LIMIT_REFUSALS.contains(&e) {
        Why::TooLarge
    } else {
        Why::Damaged
    }
}

/// A part, unpacked: past the ceiling is too large, anything else damage.
fn read(src: &Source<'_>, name: &str) -> std::result::Result<Vec<u8>, Why> {
    if src
        .part_size(name)
        .is_some_and(|n| n > MAX_EDIT_PART_BYTES as u64)
    {
        return Err(Why::TooLarge);
    }
    src.read(name, MAX_EDIT_PART_BYTES).map_err(|e| why_of(&e))
}

/// What has been put in a reply so far, against [`MAX_PREVIEW_BYTES`].
#[derive(Default)]
struct Budget {
    used: usize,
}

impl Budget {
    /// Room for text `s` in a piece of `over` bytes of JSON around it? The
    /// text is counted as JSON writes it (a quote, a backslash, a tab and a
    /// line break take two bytes).
    fn take(&mut self, s: &str, over: usize) -> bool {
        let n = s.len()
            + s.bytes()
                .filter(|b| matches!(b, b'"' | b'\\' | b'\t' | b'\n'))
                .count();
        self.take_n(n, over)
    }

    fn take_n(&mut self, n: usize, over: usize) -> bool {
        let next = self.used + n + over;
        if next > MAX_PREVIEW_BYTES {
            return false;
        }
        self.used = next;
        true
    }
}

/// Text for the page: invisible and direction characters dropped, controls
/// read as a space, an object as `[obj]`, a tab and a line break kept; cut at
/// `max` characters (`true` when it was cut).
fn clean(text: &str, max: usize) -> (String, bool) {
    let mut o = String::new();
    let mut n = 0;
    for c in text.chars() {
        if n >= max {
            return (o, true);
        }
        match c {
            '\t' | '\n' => o.push(c),
            OBJ => o.push_str("[obj]"),
            c if harness_core::text::is_hidden(c) => continue,
            c if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') => o.push(' '),
            c => o.push(c),
        }
        n += 1;
    }
    (o, false)
}

fn heading_level(name: &str) -> Option<u8> {
    match name {
        "title" => Some(1),
        "subtitle" => Some(2),
        n => n
            .strip_prefix("heading")
            .and_then(|r| r.trim().parse::<u16>().ok())
            .filter(|l| *l >= 1)
            .map(|l| l.min(6) as u8),
    }
}

/// The table being read: rows of cells, the cell the next paragraph of the
/// same cell goes to.
struct Table {
    t: u32,
    rows: Vec<Vec<String>>,
    last: (u32, u32),
    /// The cell that takes more lines (`None` when it was left out).
    open: Option<(usize, usize)>,
}

fn doc(src: &Source<'_>) -> std::result::Result<Preview, Why> {
    let part = read(src, "word/document.xml")?;
    let idx = docx::index(&part).map_err(|e| why_of(&e))?;
    let st = docx::styles(src).map_err(|e| why_of(&e))?;
    let mut blocks: Vec<Block> = vec![];
    let mut more = false;
    let mut budget = Budget::default();
    let mut cells = 0usize;
    let mut tbl: Option<Table> = None;
    let flush = |tbl: &mut Option<Table>, blocks: &mut Vec<Block>, more: &mut bool| {
        if let Some(mut t) = tbl.take() {
            t.rows.retain(|r| !r.is_empty());
            if t.rows.is_empty() {
                return;
            }
            if blocks.len() >= MAX_PREVIEW_BLOCKS {
                *more = true;
            } else {
                blocks.push(Block::T { rows: t.rows });
            }
        }
    };
    'paras: for p in &idx.paras {
        if let Some((t, r, c)) = p.cell {
            if !matches!(&tbl, Some(cur) if cur.t == t) {
                flush(&mut tbl, &mut blocks, &mut more);
                if blocks.len() >= MAX_PREVIEW_BLOCKS {
                    more = true;
                    break 'paras;
                }
                tbl = Some(Table {
                    t,
                    rows: vec![],
                    last: (0, 0),
                    open: None,
                });
            }
            let Some(cur) = tbl.as_mut() else { break };
            let (text, cut) = clean(&p.text, MAX_PREVIEW_CELL_CHARS);
            more |= cut;
            if (r, c) == cur.last {
                // Another paragraph of the same cell: a line more.
                if let Some((ri, ci)) = cur.open {
                    if budget.take(&text, 4) {
                        let cell = &mut cur.rows[ri][ci];
                        cell.push('\n');
                        cell.push_str(&text);
                    } else {
                        more = true;
                        break 'paras;
                    }
                }
                continue;
            }
            if r != cur.last.0 || cur.rows.is_empty() {
                cur.rows.push(vec![]);
            }
            cur.last = (r, c);
            cur.open = None;
            let row = cur.rows.len() - 1;
            if cur.rows[row].len() >= MAX_PREVIEW_COLS {
                more = true;
                continue;
            }
            if cells >= MAX_PREVIEW_TABLE_CELLS || !budget.take(&text, 4) {
                more = true;
                break 'paras;
            }
            cells += 1;
            cur.rows[row].push(text);
            cur.open = Some((row, cur.rows[row].len() - 1));
            continue;
        }
        flush(&mut tbl, &mut blocks, &mut more);
        if p.text.is_empty() {
            continue;
        }
        if blocks.len() >= MAX_PREVIEW_BLOCKS {
            more = true;
            break;
        }
        // The paragraph again, for its runs: the same reader as the index.
        let ps = para::scan(&part[p.start..p.end], &idx.prefix, NS_W, p.field_in)
            .map_err(|e| why_of(&e))?;
        let mut left = MAX_PREVIEW_PARA_CHARS;
        let mut runs: Vec<Run> = vec![];
        for sp in ps.spans() {
            let (text, cut) = clean(&sp.text, left);
            left = left.saturating_sub(text.chars().count());
            if !text.is_empty() {
                if !budget.take(&text, 48) {
                    more = true;
                    break 'paras;
                }
                runs.push(Run {
                    t: text,
                    b: sp.bold,
                    i: sp.italic,
                    s: sp.strike,
                });
            }
            if cut {
                more = true;
                break;
            }
        }
        if runs.iter().all(|r| r.t.trim().is_empty()) {
            continue;
        }
        let name = p.style.as_deref().map(|sid| {
            st.by_id
                .get(sid)
                .cloned()
                .unwrap_or_else(|| sid.to_lowercase())
        });
        blocks.push(match name.as_deref().and_then(heading_level) {
            Some(l) => Block::H { l, r: runs },
            None if p.list => Block::Li { r: runs },
            None => Block::P { r: runs },
        });
    }
    flush(&mut tbl, &mut blocks, &mut more);
    let mut notes = vec![];
    if idx.paras.iter().any(|p| p.hidden) {
        notes.push("Hidden text is not shown.".to_string());
    }
    if idx.paras.iter().any(|p| p.tracked) {
        notes.push("Tracked changes: deleted text is not shown; inserted text is.".to_string());
    }
    Ok(Preview::Doc {
        blocks,
        more,
        notes,
    })
}

fn sheet_of(src: &Source<'_>, sheet: Option<u32>) -> std::result::Result<Preview, Why> {
    let wb_part = read(src, xlsx::WORKBOOK)?;
    let wb = xlsx::read_wb(src, &wb_part).map_err(|e| why_of(&e))?;
    let visible: Vec<&xlsx::SheetRef> = wb.sheets.iter().filter(|s| s.state == "visible").collect();
    let at = sheet.unwrap_or(0);
    let sh = visible.get(at as usize).ok_or(Why::NoSheet)?;
    let part = read(src, &sh.path)?;
    let ix = xlsx::index(&part).map_err(|e| why_of(&e))?;
    let sst = xlsx::shared_strings(src).map_err(|e| why_of(&e))?;
    let dates = xlsx::date_styles(src).map_err(|e| why_of(&e))?;
    let mut more = false;
    let sheets: Vec<String> = visible
        .iter()
        .take(MAX_PREVIEW_SHEETS)
        .map(|s| harness_core::text::clean_line(&s.name, MAX_SHEET_NAME * 2))
        .collect();
    more |= visible.len() > MAX_PREVIEW_SHEETS;
    let mut budget = Budget::default();
    let mut rows: Vec<Row> = vec![];
    'rows: for rr in &ix.rows {
        if rr.hidden {
            continue;
        }
        let rs =
            xlsx::scan_row(&part[rr.start..rr.end], &ix.prefix, &sst).map_err(|e| why_of(&e))?;
        let mut cells: Vec<Cell> = vec![];
        for c in &rs.cells {
            if matches!(c.val, Val::Empty) || ix.col_hidden(c.col) {
                continue;
            }
            let (v, f) = match &c.val {
                Val::Empty => continue,
                Val::Text(t) => (
                    harness_core::text::clean_line(t, MAX_PREVIEW_CELL_CHARS),
                    None,
                ),
                Val::Formula(f) if f.is_empty() && c.shared_si.is_some() => {
                    (String::new(), Some("=(shared formula)".to_string()))
                }
                Val::Formula(f) => (
                    String::new(),
                    Some(format!("={}", harness_core::text::clean_line(f, 300))),
                ),
                other => {
                    let date =
                        c.s.as_deref()
                            .and_then(|x| x.parse::<u32>().ok())
                            .is_some_and(|k| dates.contains(&k));
                    let shown = xlsx::show_val(other, date, wb.date1904, false).unwrap_or_default();
                    (
                        shown
                            .strip_suffix(" (date)")
                            .map(str::to_string)
                            .unwrap_or(shown),
                        None,
                    )
                }
            };
            if v.is_empty() && f.is_none() {
                continue;
            }
            if c.col as usize > MAX_PREVIEW_COLS {
                more = true;
                continue;
            }
            let a = xlsx::addr(c.col, rr.r);
            if !(budget.take(&v, 0)
                && budget.take(f.as_deref().unwrap_or(""), 0)
                && budget.take(&a, 32))
            {
                more = true;
                break 'rows;
            }
            cells.push(Cell { a, v, f });
        }
        if cells.is_empty() {
            continue;
        }
        if rows.len() >= MAX_PREVIEW_ROWS {
            more = true;
            break;
        }
        rows.push(Row { n: rr.r, c: cells });
    }
    let mut notes = vec![];
    if ix.rows.iter().any(|r| r.hidden)
        || !ix.hidden_cols.is_empty()
        || wb.sheets.iter().any(|s| s.state != "visible")
    {
        notes.push("Hidden rows, columns and sheets are not shown.".to_string());
    }
    notes.push("Formulas are shown as written; their values are not calculated.".to_string());
    Ok(Preview::Sheet {
        sheets,
        at,
        rows,
        more,
        notes,
    })
}
