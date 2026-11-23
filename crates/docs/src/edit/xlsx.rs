//! `doc_view` and `sheet_edit` of an Excel file (PLAN.md T98, report 3.3 and
//! 3.4 xlsx 1-13). One worksheet part is patched row by row: a row that an
//! edit touches is written again from its own bytes (the cells it did not
//! touch are copied as they were), every other row is copied. Besides that
//! part only two things change, and only when an edit needs them:
//! `calcPr fullCalcOnLoad="1"` in the workbook, and the calculation chain
//! (its entry, its content type and its relationship) when a formula cell is
//! added, removed or overwritten. `.xlsm`'s `vbaProject.bin` is copied like
//! any other entry and never read.

use std::collections::{BTreeMap, HashMap, HashSet};

use quick_xml::events::{BytesStart, Event};
use serde_json::Value;

use super::Outcome;
use super::check::resolve;
use super::pkg::{Rewrite, Source};
use super::scan::{NS_REL, NS_S, Patches, Scan, attr};
use crate::Result;
use crate::formula;
use crate::limits::*;
use crate::xml;

pub(super) const WORKBOOK: &str = "xl/workbook.xml";
const MAX_ROW: u32 = 1_048_576;
const MAX_COL: u32 = 16_384;
/// Rows of a sheet an edit will index.
const MAX_INDEXED_ROWS: usize = 300_000;
/// Validation and conditional-format entries kept, and ranges read from one
/// entry: what a view prints and what an edit checks its cells against.
const MAX_RANGE_ENTRIES: usize = 200;
const MAX_RANGES_PER_ENTRY: usize = 20;
/// Cells one view line shows at most.
const MAX_LINE_CELLS: usize = 60;
/// Bounds of the warnings an edit gives.
const MAX_NOTE_CELLS: usize = 10_000;
const MAX_NOTE_RANGES: usize = 400;
const MAX_NOTES: usize = 20;

/// The ranges of `sqref` entries, parsed, never the file's own text (which
/// may hold anything after a space): at most [`MAX_RANGES_PER_ENTRY`] of each
/// entry; the second value says some were left out.
fn sq_rects(list: &[String]) -> (Vec<Rect>, bool) {
    let mut out = vec![];
    let mut more = false;
    for sq in list {
        for (n, tok) in sq.split_whitespace().enumerate() {
            if n >= MAX_RANGES_PER_ENTRY {
                more = true;
                break;
            }
            if let Some(r) = parse_range(tok) {
                out.push(r);
            }
        }
    }
    (out, more)
}

fn rects_text(list: &[String]) -> String {
    let (r, more) = sq_rects(list);
    let mut s = r
        .iter()
        .take(20)
        .map(|r| format!("{}:{}", addr(r.0, r.1), addr(r.2, r.3)))
        .collect::<Vec<_>>()
        .join(", ");
    if r.len() > 20 || more {
        s.push_str(" and more");
    }
    s
}

// -- addresses ----------------------------------------------------------------

/// `B7` as (column 2, row 7), or `None`.
pub fn parse_addr(s: &str) -> Option<(u32, u32)> {
    let split = s.find(|c: char| c.is_ascii_digit())?;
    let (l, d) = s.split_at(split);
    if l.is_empty() || l.len() > 3 || !l.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    if d.is_empty() || d.len() > 7 || !d.bytes().all(|b| b.is_ascii_digit()) || d.starts_with('0') {
        return None;
    }
    let col = l
        .to_ascii_uppercase()
        .bytes()
        .fold(0u32, |a, b| a * 26 + u32::from(b - b'A' + 1));
    let row: u32 = d.parse().ok()?;
    ((1..=MAX_COL).contains(&col) && (1..=MAX_ROW).contains(&row)).then_some((col, row))
}

pub(crate) fn col_name(mut c: u32) -> String {
    let mut o = Vec::new();
    while c > 0 {
        c -= 1;
        o.push(b'A' + (c % 26) as u8);
        c /= 26;
    }
    o.reverse();
    String::from_utf8(o).unwrap_or_default()
}

pub(super) fn addr(col: u32, row: u32) -> String {
    format!("{}{row}", col_name(col))
}

/// `A1:C5` or `B7` as (col1, row1, col2, row2).
pub fn parse_range(s: &str) -> Option<(u32, u32, u32, u32)> {
    let (a, b) = s.split_once(':').unwrap_or((s, s));
    let (c1, r1) = parse_addr(a)?;
    let (c2, r2) = parse_addr(b)?;
    Some((c1.min(c2), r1.min(r2), c1.max(c2), r1.max(r2)))
}

type Rect = (u32, u32, u32, u32);

fn inside(r: Rect, col: u32, row: u32) -> bool {
    col >= r.0 && col <= r.2 && row >= r.1 && row <= r.3
}

/// Whether the cell touches the rectangle (inside it, or next to it).
fn adjoins(r: Rect, col: u32, row: u32) -> bool {
    col + 1 >= r.0 && col <= r.2 + 1 && row + 1 >= r.1 && row <= r.3 + 1
}

// -- the workbook ------------------------------------------------------------

/// An element's attributes in order.
type Attrs = Vec<(String, String)>;

/// A cell formula's `t`, `si` and whether it has a `ref`.
type FAttrs = (Option<String>, Option<String>, bool);

#[derive(Debug, Clone)]
pub struct SheetRef {
    pub name: String,
    pub state: String,
    pub path: String,
}

pub struct Wb {
    pub sheets: Vec<SheetRef>,
    pub date1904: bool,
    pub names: Vec<(String, String)>,
    calc_pr: Option<(usize, usize, Attrs)>,
    /// Where a new `calcPr` goes.
    calc_at: usize,
    pub has_calc_chain: bool,
}

pub(super) fn read_wb(src: &Source<'_>, part: &[u8]) -> Result<Wb> {
    // Relationship ids to targets.
    let rels_name = "xl/_rels/workbook.xml.rels";
    let mut rels: HashMap<String, String> = HashMap::new();
    let mut has_chain = false;
    if src.has(rels_name) {
        let r = src.read(rels_name, 4 << 20)?;
        let mut s = Scan::new(&r, NS_REL, MAX_EDIT_EVENTS)?;
        while let Some(t) = s.next()? {
            if let Event::Start(e) | Event::Empty(e) = &t.ev
                && s.local(e.name()) == Some("Relationship")
            {
                let (Some(id), Some(target)) = (attr(e, "Id")?, attr(e, "Target")?) else {
                    continue;
                };
                if attr(e, "TargetMode")?.as_deref() == Some("External") {
                    continue;
                }
                if attr(e, "Type")?.is_some_and(|t| t.ends_with("/calcChain")) {
                    has_chain = true;
                }
                if let Some(p) = resolve("xl/", &target) {
                    rels.insert(id, p);
                }
            }
        }
    }
    let mut s = Scan::new(part, NS_S, MAX_EDIT_EVENTS)?;
    let mut wb = Wb {
        sheets: vec![],
        date1904: false,
        names: vec![],
        calc_pr: None,
        calc_at: 0,
        has_calc_chain: has_chain && src.has("xl/calcChain.xml"),
    };
    let mut saw_root = false;
    let mut in_name: Option<String> = None;
    let mut name_text = String::new();
    // Children of the root, in order, for the place of `calcPr`.
    let order = [
        "fileVersion",
        "fileSharing",
        "workbookPr",
        "workbookProtection",
        "bookViews",
        "sheets",
        "functionGroups",
        "externalReferences",
        "definedNames",
    ];
    while let Some(t) = s.next()? {
        let (e, empty) = match &t.ev {
            Event::Start(e) => (e, false),
            Event::Empty(e) => (e, true),
            Event::End(e) => {
                if t.depth == 2 && order.contains(&s.local(e.name()).unwrap_or("")) {
                    wb.calc_at = t.end;
                }
                if let Some(n) = in_name.take() {
                    wb.names
                        .push((n, harness_core::text::clean_line(&name_text, 200)));
                    name_text.clear();
                }
                continue;
            }
            ev => {
                if in_name.is_some()
                    && let Some(x) = super::scan::event_text(ev)
                {
                    name_text.push_str(&x);
                }
                continue;
            }
        };
        let local = s.local(e.name());
        if !saw_root {
            saw_root = true;
            if local != Some("workbook") {
                return Err("this is not an Excel workbook".into());
            }
            continue;
        }
        let child_depth = if empty { 1 } else { 2 };
        if t.depth == child_depth && order.contains(&local.unwrap_or("")) && empty {
            wb.calc_at = t.end;
        }
        match local {
            Some("workbookPr") => {
                wb.date1904 = matches!(attr(e, "date1904")?.as_deref(), Some("1" | "true"));
            }
            Some("sheet") => {
                let name = attr(e, "name")?.ok_or("a sheet has no name")?;
                let rid = attr(e, "id")?.ok_or("a sheet has no relationship id")?;
                let path = rels
                    .get(&rid)
                    .cloned()
                    .ok_or("a sheet's relationship is missing or points outside the package")?;
                wb.sheets.push(SheetRef {
                    name,
                    state: attr(e, "state")?.unwrap_or_else(|| "visible".into()),
                    path,
                });
            }
            Some("definedName") if !empty => {
                in_name = Some(attr(e, "name")?.unwrap_or_default());
            }
            Some("calcPr") if empty => wb.calc_pr = Some((t.start, t.end, attrs_of(e)?)),
            Some("calcPr") => {
                return Err(
                    "the workbook's calculation settings are not in the form an edit can change"
                        .into(),
                );
            }
            _ => {}
        }
        if wb.sheets.len() > 500 {
            return Err(super::E_SHEETS.into());
        }
    }
    if wb.sheets.is_empty() {
        return Err("this workbook has no sheets".into());
    }
    Ok(wb)
}

// -- styles and shared strings -------------------------------------------------

/// The cell style indexes (`s`) whose number format is a date or a time.
pub(super) fn date_styles(src: &Source<'_>) -> Result<HashSet<u32>> {
    let mut out = HashSet::new();
    if !src.has("xl/styles.xml") {
        return Ok(out);
    }
    let part = src.read("xl/styles.xml", MAX_EDIT_PART_BYTES)?;
    let mut s = Scan::new(&part, NS_S, MAX_EDIT_EVENTS)?;
    let mut custom: HashMap<u32, bool> = HashMap::new();
    let mut in_xfs = false;
    let mut k = 0u32;
    while let Some(t) = s.next()? {
        match &t.ev {
            Event::Start(e) | Event::Empty(e) => match s.local(e.name()) {
                Some("numFmt") => {
                    if let (Some(id), Some(code)) = (attr(e, "numFmtId")?, attr(e, "formatCode")?)
                        && let Ok(id) = id.parse::<u32>()
                    {
                        custom.insert(id, is_date_code(&code));
                    }
                }
                Some("cellXfs") => in_xfs = true,
                Some("xf") if in_xfs => {
                    let id: u32 = attr(e, "numFmtId")?
                        .and_then(|x| x.parse().ok())
                        .unwrap_or(0);
                    let date = custom.get(&id).copied().unwrap_or(matches!(
                        id,
                        14..=22 | 27..=36 | 45..=47 | 50..=58
                    ));
                    if date {
                        out.insert(k);
                    }
                    k += 1;
                }
                _ => {}
            },
            Event::End(e) if s.local(e.name()) == Some("cellXfs") => in_xfs = false,
            _ => {}
        }
    }
    Ok(out)
}

/// A number format code that shows a date or a time: a `d`, `m`, `y`, `h` or
/// `s` outside quotes, brackets and escapes.
fn is_date_code(code: &str) -> bool {
    let mut in_q = false;
    let mut in_b = false;
    let mut esc = false;
    for c in code.chars() {
        if esc {
            esc = false;
            continue;
        }
        match c {
            '\\' | '_' | '*' => esc = true,
            '"' => in_q = !in_q,
            '[' if !in_q => in_b = true,
            ']' if !in_q => in_b = false,
            'd' | 'm' | 'y' | 'h' | 's' | 'D' | 'M' | 'Y' | 'H' | 'S' if !in_q && !in_b => {
                return true;
            }
            _ => {}
        }
    }
    false
}

pub(super) fn shared_strings(src: &Source<'_>) -> Result<Vec<String>> {
    let mut out = vec![];
    if !src.has("xl/sharedStrings.xml") {
        return Ok(out);
    }
    let part = src.read("xl/sharedStrings.xml", MAX_EDIT_PART_BYTES)?;
    let mut s = Scan::new(&part, NS_S, MAX_EDIT_EVENTS)?;
    let mut cur: Option<String> = None;
    let mut in_t = false;
    let mut skip = 0usize;
    let mut total = 0usize;
    while let Some(t) = s.next()? {
        match &t.ev {
            Event::Start(e) => match s.local(e.name()) {
                Some("si") => cur = Some(String::new()),
                Some("rPh") => skip += 1,
                Some("t") if skip == 0 => in_t = true,
                _ => {}
            },
            Event::Empty(e) if s.local(e.name()) == Some("si") => out.push(String::new()),
            Event::End(e) => match s.local(e.name()) {
                Some("si") => {
                    if let Some(c) = cur.take() {
                        total += c.len();
                        if total > MAX_EDIT_PART_BYTES {
                            return Err(super::E_STRINGS.into());
                        }
                        out.push(c);
                    }
                }
                Some("rPh") => skip = skip.saturating_sub(1),
                Some("t") => in_t = false,
                _ => {}
            },
            ev => {
                if in_t
                    && let Some(c) = cur.as_mut()
                    && let Some(x) = super::scan::event_text(ev)
                {
                    c.push_str(&x);
                }
            }
        }
    }
    Ok(out)
}

// -- the sheet -----------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RowRec {
    pub r: u32,
    pub start: usize,
    pub end: usize,
    pub hidden: bool,
}

#[derive(Default)]
pub struct SheetIdx {
    pub rows: Vec<RowRec>,
    /// `<sheetData>`: whole element, end of its start tag, start of its end
    /// tag; `empty` for `<sheetData/>`.
    pub sd: (usize, usize, usize, usize),
    pub sd_empty: bool,
    pub dim: Option<(usize, usize, Rect)>,
    pub merges: Vec<Rect>,
    pub arrays: Vec<Rect>,
    pub shared: Vec<(Rect, (u32, u32))>,
    pub cond: Vec<String>,
    pub valid: Vec<String>,
    pub autofilter: Option<String>,
    pub protected: bool,
    pub table_rids: Vec<String>,
    pub hidden_cols: Vec<(u32, u32)>,
    pub formulas: usize,
    pub prefix: String,
}

impl SheetIdx {
    /// Is column `col` (1-based) in a hidden range? The ranges are sorted
    /// and apart (see [`index`]), so this is a binary search.
    pub fn col_hidden(&self, col: u32) -> bool {
        let i = self.hidden_cols.partition_point(|(_, hi)| *hi < col);
        self.hidden_cols.get(i).is_some_and(|(lo, _)| *lo <= col)
    }
}

fn truthy(v: Option<String>) -> bool {
    matches!(v.as_deref(), Some("1" | "true" | "on"))
}

pub fn index(part: &[u8]) -> Result<SheetIdx> {
    let mut s = Scan::new(part, NS_S, MAX_EDIT_EVENTS)?;
    let mut ix = SheetIdx::default();
    let mut saw_root = false;
    let mut cell_addr = (0u32, 0u32);
    let mut last_row = 0u32;
    while let Some(t) = s.next()? {
        let (e, empty) = match &t.ev {
            Event::Start(e) => (e, false),
            Event::Empty(e) => (e, true),
            _ => continue,
        };
        let local = s.local(e.name());
        if !saw_root {
            saw_root = true;
            if local != Some("worksheet") {
                return Err("this is not a worksheet".into());
            }
            ix.prefix = s.prefix().to_string();
            continue;
        }
        // Depth of this element's parent chain: the root's children are at
        // depth 2 as a start tag and 1 as an empty one.
        let d = if empty { t.depth + 1 } else { t.depth };
        match (d, local) {
            (2, Some("sheetData")) => {
                ix.sd.0 = t.start;
                ix.sd.1 = t.end;
                if empty {
                    ix.sd_empty = true;
                    ix.sd = (t.start, t.end, t.end, t.end);
                    continue;
                }
                // Rows.
                loop {
                    let Some(c) = s.next()? else {
                        return Err("a part of the file is cut short".into());
                    };
                    match &c.ev {
                        Event::End(_) if c.depth == 2 => {
                            ix.sd = (t.start, t.end, c.start, c.end);
                            break;
                        }
                        Event::Start(re) | Event::Empty(re)
                            if s.local(re.name()) == Some("row") =>
                        {
                            let rempty = matches!(c.ev, Event::Empty(_));
                            let r: u32 = attr(re, "r")?
                                .and_then(|x| x.parse().ok())
                                .filter(|r| (1..=MAX_ROW).contains(r))
                                .ok_or("a row has no valid number; the sheet cannot be edited")?;
                            if r <= last_row {
                                return Err(
                                    "the sheet's rows are not in order; it cannot be edited".into(),
                                );
                            }
                            last_row = r;
                            if ix.rows.len() >= MAX_INDEXED_ROWS {
                                return Err(super::E_ROWS.into());
                            }
                            let hidden = truthy(attr(re, "hidden")?);
                            let mut end = c.end;
                            if !rempty {
                                // Cells: their formulas matter (shared, array).
                                let rd = c.depth;
                                loop {
                                    let Some(x) = s.next()? else {
                                        return Err("a part of the file is cut short".into());
                                    };
                                    match &x.ev {
                                        Event::End(_) if x.depth == rd => {
                                            end = x.end;
                                            break;
                                        }
                                        Event::Start(ce) | Event::Empty(ce)
                                            if s.local(ce.name()) == Some("c") =>
                                        {
                                            cell_addr = attr(ce, "r")?
                                                .as_deref()
                                                .and_then(parse_addr)
                                                .ok_or("a cell has no valid address; the sheet cannot be edited")?;
                                            if cell_addr.1 != r {
                                                return Err("a cell is in another row than its address says; the sheet cannot be edited".into());
                                            }
                                        }
                                        Event::Start(fe) | Event::Empty(fe)
                                            if s.local(fe.name()) == Some("f") =>
                                        {
                                            ix.formulas += 1;
                                            let kind = attr(fe, "t")?;
                                            let rf =
                                                attr(fe, "ref")?.as_deref().and_then(parse_range);
                                            match (kind.as_deref(), rf) {
                                                (Some("array"), Some(rc)) => ix.arrays.push(rc),
                                                (Some("shared"), Some(rc)) => {
                                                    ix.shared.push((rc, cell_addr));
                                                }
                                                _ => {}
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            ix.rows.push(RowRec {
                                r,
                                start: c.start,
                                end,
                                hidden,
                            });
                        }
                        _ => {}
                    }
                }
            }
            (2, Some("dimension")) => {
                let rf = attr(e, "ref")?.as_deref().and_then(parse_range);
                let end = if empty {
                    t.end
                } else {
                    s.skip_to_end(t.depth)?
                };
                if let Some(rf) = rf {
                    ix.dim = Some((t.start, end, rf));
                }
            }
            (2, Some("mergeCells")) if !empty => loop {
                let Some(c) = s.next()? else {
                    return Err("a part of the file is cut short".into());
                };
                match &c.ev {
                    Event::End(_) if c.depth == 2 => break,
                    Event::Start(m) | Event::Empty(m) if s.local(m.name()) == Some("mergeCell") => {
                        if let Some(rc) = attr(m, "ref")?.as_deref().and_then(parse_range) {
                            ix.merges.push(rc);
                        }
                    }
                    _ => {}
                }
            },
            (2, Some("cols")) if !empty => loop {
                let Some(c) = s.next()? else {
                    return Err("a part of the file is cut short".into());
                };
                match &c.ev {
                    Event::End(_) if c.depth == 2 => break,
                    Event::Start(m) | Event::Empty(m)
                        if s.local(m.name()) == Some("col") && truthy(attr(m, "hidden")?) =>
                    {
                        let lo = attr(m, "min")?.and_then(|x| x.parse().ok()).unwrap_or(1);
                        let hi = attr(m, "max")?.and_then(|x| x.parse().ok()).unwrap_or(lo);
                        ix.hidden_cols.push((lo, hi));
                    }
                    _ => {}
                }
            },
            (2, Some("conditionalFormatting")) => {
                if let Some(x) = attr(e, "sqref")?
                    && ix.cond.len() < MAX_RANGE_ENTRIES
                {
                    ix.cond.push(x);
                }
                if !empty {
                    s.skip_to_end(t.depth)?;
                }
            }
            (2, Some("dataValidations")) if !empty => loop {
                let Some(c) = s.next()? else {
                    return Err("a part of the file is cut short".into());
                };
                match &c.ev {
                    Event::End(_) if c.depth == 2 => break,
                    Event::Start(m) | Event::Empty(m)
                        if s.local(m.name()) == Some("dataValidation") =>
                    {
                        if let Some(x) = attr(m, "sqref")?
                            && ix.valid.len() < MAX_RANGE_ENTRIES
                        {
                            ix.valid.push(x);
                        }
                    }
                    _ => {}
                }
            },
            (2, Some("autoFilter")) => {
                ix.autofilter = attr(e, "ref")?;
                if !empty {
                    s.skip_to_end(t.depth)?;
                }
            }
            (2, Some("sheetProtection")) => {
                if truthy(attr(e, "sheet")?) {
                    ix.protected = true;
                }
                if !empty {
                    s.skip_to_end(t.depth)?;
                }
            }
            (2, Some("tableParts")) if !empty => loop {
                let Some(c) = s.next()? else {
                    return Err("a part of the file is cut short".into());
                };
                match &c.ev {
                    Event::End(_) if c.depth == 2 => break,
                    Event::Start(m) | Event::Empty(m) if s.local(m.name()) == Some("tablePart") => {
                        if let Some(x) = attr(m, "id")? {
                            ix.table_rids.push(x);
                        }
                    }
                    _ => {}
                }
            },
            _ => {
                if !empty {
                    s.skip_to_end(t.depth)?;
                }
            }
        }
    }
    if !saw_root || (ix.sd.3 == 0 && !ix.sd_empty) {
        return Err("this worksheet has no cell data to edit".into());
    }
    // Sorted and apart: a sheet may name tens of thousands of ranges, and
    // every cell asks.
    ix.hidden_cols.retain(|(lo, hi)| lo <= hi);
    ix.hidden_cols.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::with_capacity(ix.hidden_cols.len());
    for (lo, hi) in ix.hidden_cols.drain(..) {
        match merged.last_mut() {
            Some(l) if lo <= l.1.saturating_add(1) => l.1 = l.1.max(hi),
            _ => merged.push((lo, hi)),
        }
    }
    ix.hidden_cols = merged;
    Ok(ix)
}

// -- cells -----------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Empty,
    Num(f64),
    Text(String),
    Bool(bool),
    Err(String),
    /// A formula (the text; empty for a shared formula's follower).
    Formula(String),
}

#[derive(Debug, Clone)]
pub struct Cell {
    pub start: usize,
    pub end: usize,
    pub col: u32,
    pub s: Option<String>,
    pub val: Val,
    pub shared_si: Option<String>,
    pub shared_master: bool,
}

pub struct RowScan {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub open_end: usize,
    pub close_start: usize,
    pub self_closing: bool,
    pub cells: Vec<Cell>,
}

fn attrs_of(e: &BytesStart<'_>) -> Result<Vec<(String, String)>> {
    let mut v = vec![];
    for a in e.attributes() {
        let a = a.map_err(|_| "a row has a damaged attribute".to_string())?;
        v.push((a.key.as_ref().to_string(), super::scan::unescape(&a.value)));
    }
    Ok(v)
}

/// Read one row's element (positions relative to `src`). `sst` resolves
/// shared strings.
pub fn scan_row(src: &[u8], prefix: &str, sst: &[String]) -> Result<RowScan> {
    let mut s = Scan::within(src, NS_S, prefix, MAX_EDIT_EVENTS)?;
    let mut out = RowScan {
        name: String::new(),
        attrs: vec![],
        open_end: 0,
        close_start: 0,
        self_closing: false,
        cells: vec![],
    };
    let mut first = true;
    let mut row_no = 0u32;
    while let Some(t) = s.next()? {
        let (e, empty) = match &t.ev {
            Event::Start(e) => (e, false),
            Event::Empty(e) => (e, true),
            Event::End(_) if t.depth == 1 => {
                out.close_start = t.start;
                break;
            }
            _ => continue,
        };
        if first {
            first = false;
            out.name = e.name().as_ref().to_string();
            out.attrs = attrs_of(e)?;
            out.open_end = t.end;
            out.self_closing = empty;
            row_no = attr(e, "r")?.and_then(|x| x.parse().ok()).unwrap_or(0);
            if empty {
                out.close_start = t.end;
                break;
            }
            continue;
        }
        if s.local(e.name()) != Some("c") {
            if !empty {
                s.skip_to_end(t.depth)?;
            }
            continue;
        }
        let a = attr(e, "r")?.ok_or("a cell has no address")?;
        let (col, r) = parse_addr(&a).ok_or("a cell has a damaged address")?;
        if r != row_no {
            return Err("a cell is in another row than its address says".into());
        }
        if out.cells.last().is_some_and(|c| c.col >= col) {
            return Err("the cells of a row are not in order; the sheet cannot be edited".into());
        }
        let style = attr(e, "s")?;
        let ty = attr(e, "t")?;
        let (mut f, mut v, mut is, mut f_attrs): (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<FAttrs>,
        ) = (None, None, None, None);
        let mut end = t.end;
        if !empty {
            let cd = t.depth;
            let mut cur: Option<&'static str> = None;
            let mut in_t = false;
            let mut skip_ph = 0usize;
            loop {
                let Some(x) = s.next()? else {
                    return Err("a part of the file is cut short".into());
                };
                match &x.ev {
                    Event::End(_) if x.depth == cd => {
                        end = x.end;
                        break;
                    }
                    Event::Start(ce) | Event::Empty(ce) => {
                        let xe = matches!(x.ev, Event::Empty(_));
                        match s.local(ce.name()) {
                            Some("f") => {
                                f = Some(String::new());
                                f_attrs = Some((
                                    attr(ce, "t")?,
                                    attr(ce, "si")?,
                                    attr(ce, "ref")?.is_some(),
                                ));
                                if !xe {
                                    cur = Some("f");
                                }
                            }
                            Some("v") => {
                                v = Some(String::new());
                                if !xe {
                                    cur = Some("v");
                                }
                            }
                            Some("is") => {
                                is = Some(String::new());
                                if !xe {
                                    cur = Some("is");
                                }
                            }
                            Some("rPh") if !xe => skip_ph += 1,
                            Some("t") if cur == Some("is") && skip_ph == 0 && !xe => in_t = true,
                            _ => {
                                if !xe && cur.is_none() {
                                    s.skip_to_end(x.depth)?;
                                }
                            }
                        }
                    }
                    Event::End(ce) => match s.local(ce.name()) {
                        Some("f" | "v") => cur = None,
                        Some("is") => cur = None,
                        Some("rPh") => skip_ph = skip_ph.saturating_sub(1),
                        Some("t") => in_t = false,
                        _ => {}
                    },
                    ev => {
                        if let Some(txt) = super::scan::event_text(ev) {
                            match cur {
                                Some("f") => {
                                    if let Some(f) = f.as_mut() {
                                        f.push_str(&txt);
                                    }
                                }
                                Some("v") => {
                                    if let Some(v) = v.as_mut() {
                                        v.push_str(&txt);
                                    }
                                }
                                Some("is") if in_t => {
                                    if let Some(i) = is.as_mut() {
                                        i.push_str(&txt);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        let val = if let Some(ft) = f {
            Val::Formula(ft)
        } else {
            match ty.as_deref() {
                Some("inlineStr") => Val::Text(is.unwrap_or_default()),
                Some("s") => match v.as_deref().and_then(|x| x.trim().parse::<usize>().ok()) {
                    Some(i) => Val::Text(sst.get(i).cloned().unwrap_or_default()),
                    None => Val::Empty,
                },
                Some("str") => Val::Text(v.unwrap_or_default()),
                Some("b") => Val::Bool(matches!(v.as_deref().map(str::trim), Some("1" | "true"))),
                Some("e") => Val::Err(v.unwrap_or_default()),
                _ => match v.as_deref().map(str::trim) {
                    None | Some("") => Val::Empty,
                    Some(x) => x
                        .parse::<f64>()
                        .map(Val::Num)
                        .unwrap_or(Val::Text(x.to_string())),
                },
            }
        };
        let (si, master) = match &f_attrs {
            Some((Some(k), si, has_ref)) if k == "shared" => (si.clone(), *has_ref),
            _ => (None, false),
        };
        out.cells.push(Cell {
            start: t.start,
            end,
            col,
            s: style,
            val,
            shared_si: si,
            shared_master: master,
        });
    }
    Ok(out)
}

// -- the view ----------------------------------------------------------------------

fn shown_num(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        format!("{x}")
    }
}

fn serial_iso(x: f64, date1904: bool) -> Option<String> {
    let days = x.floor();
    if !(0.0..3_000_000.0).contains(&days) {
        return None;
    }
    let base = if date1904 {
        crate::days_from_civil(1904, 1, 1)
    } else {
        crate::days_from_civil(1899, 12, 30)
    };
    let (y, m, d) = crate::civil_from_days(base + days as i64);
    let secs = ((x - days) * 86_400.0).round() as i64;
    Some(if secs == 0 {
        format!("{y:04}-{m:02}-{d:02}")
    } else {
        format!(
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}",
            secs / 3600,
            (secs % 3600) / 60,
            secs % 60
        )
    })
}

pub(super) fn show_val(v: &Val, date: bool, d1904: bool, shared: bool) -> Option<String> {
    Some(match v {
        Val::Empty => return None,
        Val::Num(x) if date => match serial_iso(*x, d1904) {
            Some(i) => format!("{i} (date)"),
            None => shown_num(*x),
        },
        Val::Num(x) => shown_num(*x),
        Val::Text(t) => {
            let mut s = String::from("\"");
            for c in harness_core::text::clean_line(t, 200).chars() {
                if c == '"' {
                    s.push_str("\\\"");
                } else {
                    s.push(c);
                }
            }
            s.push('"');
            s
        }
        Val::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Val::Err(e) => harness_core::text::clean_line(e, 20),
        Val::Formula(f) if f.is_empty() && shared => "=(shared formula, not calculated)".into(),
        Val::Formula(f) => format!(
            "={} (not calculated)",
            harness_core::text::clean_line(f, 300)
        ),
    })
}

pub(super) fn sheet_ref<'a>(wb: &'a Wb, name: Option<&str>) -> Result<&'a SheetRef> {
    let list = || {
        wb.sheets
            .iter()
            .map(|s| format!("\"{}\"", harness_core::text::clean_line(&s.name, 40)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let sh = match name {
        None | Some("") => wb
            .sheets
            .iter()
            .find(|s| s.state == "visible")
            .ok_or("every sheet of this workbook is hidden")?,
        Some(n) => wb
            .sheets
            .iter()
            .find(|s| s.name == n)
            .or_else(|| wb.sheets.iter().find(|s| s.name.eq_ignore_ascii_case(n)))
            .ok_or_else(|| {
                format!(
                    "no sheet \"{}\"; the sheets are {}",
                    harness_core::text::clean_line(n, 40),
                    list()
                )
            })?,
    };
    if sh.state != "visible" {
        return Err(format!(
            "the sheet \"{}\" is hidden; hidden content cannot be addressed",
            harness_core::text::clean_line(&sh.name, 40)
        ));
    }
    Ok(sh)
}

/// Warnings an edit near these things must give, and the lines of a view.
struct Extras {
    tables: Vec<(String, Rect)>,
    pivots: usize,
}

fn extras(src: &Source<'_>, sheet_path: &str) -> Result<Extras> {
    let mut ex = Extras {
        tables: vec![],
        pivots: 0,
    };
    let (dir, file) = sheet_path.rsplit_once('/').unwrap_or(("", sheet_path));
    let rels = format!("{dir}/_rels/{file}.rels");
    if !src.has(&rels) {
        return Ok(ex);
    }
    let r = src.read(&rels, 4 << 20)?;
    let mut s = Scan::new(&r, NS_REL, MAX_EDIT_EVENTS)?;
    let mut targets = vec![];
    while let Some(t) = s.next()? {
        if let Event::Start(e) | Event::Empty(e) = &t.ev
            && s.local(e.name()) == Some("Relationship")
            && attr(e, "TargetMode")?.as_deref() != Some("External")
        {
            let ty = attr(e, "Type")?.unwrap_or_default();
            let Some(target) = attr(e, "Target")? else {
                continue;
            };
            if ty.ends_with("/table")
                && let Some(p) = resolve(&format!("{dir}/"), &target)
            {
                targets.push(p);
            }
            if ty.ends_with("/pivotTable") {
                ex.pivots += 1;
            }
        }
    }
    for p in targets.into_iter().take(50) {
        if !src.has(&p) {
            continue;
        }
        let part = src.read(&p, 1 << 20)?;
        let mut s = Scan::new(&part, NS_S, MAX_EDIT_EVENTS)?;
        while let Some(t) = s.next()? {
            if let Event::Start(e) | Event::Empty(e) = &t.ev
                && s.local(e.name()) == Some("table")
            {
                if let Some(rc) = attr(e, "ref")?.as_deref().and_then(parse_range) {
                    let name = attr(e, "displayName")?
                        .or(attr(e, "name")?)
                        .unwrap_or_default();
                    ex.tables.push((name, rc));
                }
                break;
            }
        }
    }
    Ok(ex)
}

/// The view of one sheet: rows with addresses, values, formulas, and the
/// things an edit could touch.
pub fn view(bytes: &[u8], sheet: Option<&str>, from: usize, limit: usize) -> Result<String> {
    let src = Source::open(bytes)?;
    if !src.has(WORKBOOK) {
        return Err("this is not an Excel workbook".into());
    }
    let wb_part = src.read(WORKBOOK, MAX_EDIT_PART_BYTES)?;
    let wb = read_wb(&src, &wb_part)?;
    let sh = sheet_ref(&wb, sheet)?;
    let part = src.read(&sh.path, MAX_EDIT_PART_BYTES)?;
    let ix = index(&part)?;
    let sst = shared_strings(&src)?;
    let dates = date_styles(&src)?;
    let ex = extras(&src, &sh.path)?;
    let limit = if limit == 0 {
        200
    } else {
        limit.clamp(1, MAX_VIEW_ITEMS)
    };
    let mut out = format!(
        "Excel workbook, sheet \"{}\" (sheets: {}). Date system {}. Edit with sheet_edit: `set` cells by address, `append_rows`, `clear` a range. Formulas show as =formula (not calculated): the file holds old cached values that are not shown as current.\n",
        harness_core::text::clean_line(&sh.name, 40),
        wb.sheets
            .iter()
            .map(|s| format!(
                "\"{}\"{}",
                harness_core::text::clean_line(&s.name, 40),
                if s.state == "visible" {
                    ""
                } else {
                    " (hidden)"
                }
            ))
            .collect::<Vec<_>>()
            .join(", "),
        if wb.date1904 { "1904" } else { "1900" }
    );
    if let Some((_, _, d)) = ix.dim {
        out.push_str(&format!(
            "Used range {}:{}\n",
            addr(d.0, d.1),
            addr(d.2, d.3)
        ));
    }
    let list = |v: Vec<String>| v.into_iter().take(20).collect::<Vec<_>>().join(", ");
    if !ix.merges.is_empty() {
        out.push_str(&format!(
            "Merged: {}\n",
            list(
                ix.merges
                    .iter()
                    .map(|m| format!("{}:{}", addr(m.0, m.1), addr(m.2, m.3)))
                    .collect()
            )
        ));
    }
    for (n, r) in &ex.tables {
        out.push_str(&format!(
            "Table \"{}\" {}:{} (a row added under it does not extend it)\n",
            harness_core::text::clean_line(n, 40),
            addr(r.0, r.1),
            addr(r.2, r.3)
        ));
    }
    if !ix.shared.is_empty() {
        out.push_str(&format!(
            "Shared formulas: {} (their first cell cannot be overwritten)\n",
            list(
                ix.shared
                    .iter()
                    .map(|(r, m)| format!(
                        "{}:{} from {}",
                        addr(r.0, r.1),
                        addr(r.2, r.3),
                        addr(m.0, m.1)
                    ))
                    .collect()
            )
        ));
    }
    if !ix.arrays.is_empty() {
        out.push_str(&format!(
            "Array formulas: {} (cells inside cannot be edited)\n",
            list(
                ix.arrays
                    .iter()
                    .map(|r| format!("{}:{}", addr(r.0, r.1), addr(r.2, r.3)))
                    .collect()
            )
        ));
    }
    if !ix.valid.is_empty() {
        out.push_str(&format!("Data validation on {}\n", rects_text(&ix.valid)));
    }
    if !ix.cond.is_empty() {
        out.push_str(&format!(
            "Conditional formatting on {}\n",
            rects_text(&ix.cond)
        ));
    }
    if ex.pivots > 0 {
        out.push_str("This sheet has a pivot table: editing data under it does not refresh it.\n");
    }
    let names: Vec<String> = wb
        .names
        .iter()
        .take(20)
        .map(|(n, v)| {
            format!(
                "{} = {}",
                harness_core::text::clean_line(n, 40),
                harness_core::text::clean_line(v, 120)
            )
        })
        .collect();
    if !names.is_empty() {
        out.push_str(&format!("Defined names: {}\n", names.join("; ")));
    }
    if ix.protected {
        out.push_str("The sheet is protected: it cannot be edited.\n");
    }
    let hidden_rows = ix.rows.iter().filter(|r| r.hidden).count();
    if hidden_rows > 0
        || !ix.hidden_cols.is_empty()
        || wb.sheets.iter().any(|s| s.state != "visible")
    {
        out.push_str("[left out: hidden rows, columns or sheets]\n");
    }
    let start = ix.rows.partition_point(|r| (r.r as usize) < from.max(1));
    let mut shown = 0usize;
    let mut i = start;
    while i < ix.rows.len() && shown < limit && out.len() <= MAX_VIEW_BYTES {
        let rr = &ix.rows[i];
        i += 1;
        if rr.hidden {
            continue;
        }
        let rs = scan_row(&part[rr.start..rr.end], &ix.prefix, &sst)?;
        let mut line = String::new();
        let mut cells_shown = 0usize;
        let mut skipped = 0usize;
        for c in &rs.cells {
            if cells_shown >= MAX_LINE_CELLS {
                skipped += 1;
                continue;
            }
            if ix.col_hidden(c.col) {
                continue;
            }
            let date =
                c.s.as_deref()
                    .and_then(|x| x.parse::<u32>().ok())
                    .is_some_and(|k| dates.contains(&k));
            if let Some(t) = show_val(&c.val, date, wb.date1904, c.shared_si.is_some()) {
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(&format!("{}={t}", addr(c.col, rr.r)));
                cells_shown += 1;
            }
        }
        if skipped > 0 {
            line.push_str(&format!(" [+{skipped} more cells in this row]"));
        }
        shown += 1;
        out.push_str(&format!("{}: {}\n", rr.r, line));
    }
    if i < ix.rows.len() {
        out.push_str(&format!(
            "[cut: showed {shown} rows up to row {}; {} rows more; call doc_view with from={}]\n",
            ix.rows[i - 1].r,
            ix.rows.len() - i,
            ix.rows[i].r
        ));
    }
    Ok(super::cap_view(out))
}

// -- edits ---------------------------------------------------------------------------

/// What a cell is set to.
#[derive(Debug, Clone, PartialEq)]
pub enum CellVal {
    Null,
    Num(f64),
    /// Text; an ISO date goes to a date-formatted cell as a date.
    Text(String),
    Bool(bool),
    Formula(String),
}

impl CellVal {
    pub fn parse(v: &Value) -> Result<Self> {
        Ok(match v {
            Value::Null => Self::Null,
            Value::Bool(b) => Self::Bool(*b),
            Value::Number(n) => {
                let x = n
                    .as_f64()
                    .filter(|x| x.is_finite())
                    .ok_or("a number is not finite")?;
                Self::Num(x)
            }
            Value::String(t) => {
                if t.chars().count() > MAX_EDIT_TEXT_CHARS {
                    return Err(format!(
                        "a text is longer than {MAX_EDIT_TEXT_CHARS} characters"
                    ));
                }
                Self::Text(xml::clean(t))
            }
            Value::Object(o) if o.len() == 1 && o.contains_key("f") => {
                let f = o["f"].as_str().ok_or("`f` is the formula as text")?;
                Self::Formula(formula::check(f)?)
            }
            _ => {
                return Err(
                    "a cell is a number, text, true or false, null, or {\"f\": \"formula\"}".into(),
                );
            }
        })
    }
}

/// One edit of a sheet.
#[derive(Debug, Clone, PartialEq)]
pub enum SheetOp {
    Set(Vec<(String, CellVal)>),
    Append {
        rows: Vec<Vec<CellVal>>,
        like_row_above: bool,
    },
    Clear(String),
}

impl SheetOp {
    pub fn parse(v: &Value) -> Result<Self> {
        let o = v.as_object().ok_or("an edit is an object")?;
        let op = o
            .get("op")
            .and_then(Value::as_str)
            .ok_or("an edit needs `op`")?;
        let known: &[&str] = match op {
            "set" => &["op", "cells"],
            "append_rows" => &["op", "rows", "like_row_above"],
            "clear" => &["op", "range"],
            _ => {
                return Err(format!(
                    "`op` is set, append_rows or clear, not \"{}\"",
                    harness_core::text::clean_line(op, 30)
                ));
            }
        };
        if let Some(k) = o.keys().find(|k| !known.contains(&k.as_str())) {
            return Err(format!(
                "`{}` is not an argument of {op}",
                harness_core::text::clean_line(k, 30)
            ));
        }
        Ok(match op {
            "set" => {
                let cells = o
                    .get("cells")
                    .and_then(Value::as_object)
                    .ok_or("`set` needs `cells`: an object of address to value")?;
                if cells.len() > MAX_EDIT_CELLS {
                    return Err(format!("at most {MAX_EDIT_CELLS} cells in one `set`"));
                }
                let mut v = vec![];
                for (k, x) in cells {
                    v.push((
                        k.clone(),
                        CellVal::parse(x).map_err(|e| {
                            format!("{}: {e}", harness_core::text::clean_line(k, 20))
                        })?,
                    ));
                }
                Self::Set(v)
            }
            "append_rows" => {
                let rows = o
                    .get("rows")
                    .and_then(Value::as_array)
                    .ok_or("`append_rows` needs `rows`: a list of lists")?;
                let mut total = 0usize;
                let mut out = vec![];
                for r in rows {
                    let r = r.as_array().ok_or("each row is a list of cells")?;
                    total += r.len();
                    if total > MAX_EDIT_CELLS || r.len() > MAX_COL as usize {
                        return Err(format!(
                            "at most {MAX_EDIT_CELLS} cells in one `append_rows`"
                        ));
                    }
                    out.push(r.iter().map(CellVal::parse).collect::<Result<Vec<_>>>()?);
                }
                Self::Append {
                    rows: out,
                    like_row_above: match o.get("like_row_above") {
                        None | Some(Value::Null) => false,
                        Some(Value::Bool(b)) => *b,
                        Some(_) => return Err("`like_row_above` is true or false".into()),
                    },
                }
            }
            _ => Self::Clear(
                o.get("range")
                    .and_then(Value::as_str)
                    .ok_or("`clear` needs `range`")?
                    .to_string(),
            ),
        })
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Set(_) => "set",
            Self::Append { .. } => "append_rows",
            Self::Clear(_) => "clear",
        }
    }
}

struct Slot {
    /// New bytes; `None`: the original cell as it was.
    xml: Option<String>,
    start: usize,
    end: usize,
    val: Val,
    s: Option<String>,
}

struct RowWork {
    exists: bool,
    /// Where the original row is, and how its tags read.
    rec: Option<(usize, usize)>,
    name: String,
    open_tag: String,
    close_tag: String,
    attrs: Vec<(String, String)>,
    self_closing: bool,
    slots: BTreeMap<u32, Slot>,
    spans_changed: bool,
}

struct Ctx<'a> {
    part: &'a [u8],
    ix: &'a SheetIdx,
    sst: &'a [String],
    dates: HashSet<u32>,
    date1904: bool,
    p: String,
    works: BTreeMap<u32, RowWork>,
    last_row: u32,
    min: (u32, u32),
    max: (u32, u32),
    formula_touched: bool,
    touched: Vec<(u32, u32)>,
    notes: Vec<String>,
    wrote: Vec<(u32, u32, CellVal)>,
}

impl Ctx<'_> {
    fn row(&mut self, r: u32) -> Result<&mut RowWork> {
        if !self.works.contains_key(&r) {
            let w = match self.ix.rows.binary_search_by_key(&r, |x| x.r) {
                Ok(i) => {
                    let rec = &self.ix.rows[i];
                    let rs = scan_row(&self.part[rec.start..rec.end], &self.ix.prefix, self.sst)?;
                    let text = |a: usize, b: usize| {
                        String::from_utf8_lossy(&self.part[rec.start + a..rec.start + b])
                            .into_owned()
                    };
                    let mut slots = BTreeMap::new();
                    for c in &rs.cells {
                        slots.insert(
                            c.col,
                            Slot {
                                xml: None,
                                start: rec.start + c.start,
                                end: rec.start + c.end,
                                val: c.val.clone(),
                                s: c.s.clone(),
                            },
                        );
                    }
                    RowWork {
                        exists: true,
                        rec: Some((rec.start, rec.end)),
                        name: rs.name.clone(),
                        open_tag: text(0, rs.open_end),
                        close_tag: if rs.self_closing {
                            String::new()
                        } else {
                            text(rs.close_start, rec.end - rec.start)
                        },
                        attrs: rs.attrs.clone(),
                        self_closing: rs.self_closing,
                        slots,
                        spans_changed: false,
                    }
                }
                Err(_) => RowWork {
                    exists: false,
                    rec: None,
                    name: format!("{}row", self.p),
                    open_tag: String::new(),
                    close_tag: String::new(),
                    attrs: vec![("r".into(), r.to_string())],
                    self_closing: false,
                    slots: BTreeMap::new(),
                    spans_changed: false,
                },
            };
            self.works.insert(r, w);
        }
        self.works.get_mut(&r).ok_or_else(|| "internal".to_string())
    }

    fn guard(&mut self, col: u32, row: u32) -> std::result::Result<(), String> {
        let a = addr(col, row);
        if let Some(rec) = self
            .ix
            .rows
            .binary_search_by_key(&row, |x| x.r)
            .ok()
            .map(|i| &self.ix.rows[i])
            && rec.hidden
        {
            return Err(format!(
                "row {row} is hidden; hidden content cannot be addressed"
            ));
        }
        if self.ix.col_hidden(col) {
            return Err(format!(
                "{a} is in a hidden column; hidden content cannot be addressed"
            ));
        }
        for m in &self.ix.merges {
            if inside(*m, col, row) && (col, row) != (m.0, m.1) {
                return Err(format!(
                    "{a} is inside the merged range {}:{}; only its top-left cell {} holds a value",
                    addr(m.0, m.1),
                    addr(m.2, m.3),
                    addr(m.0, m.1)
                ));
            }
        }
        for r in &self.ix.arrays {
            if inside(*r, col, row) {
                return Err(format!(
                    "{a} is inside the array formula {}:{}, which Excel keeps as one; it is not edited",
                    addr(r.0, r.1),
                    addr(r.2, r.3)
                ));
            }
        }
        for (r, master) in &self.ix.shared {
            if (col, row) == *master {
                return Err(format!(
                    "{a} holds a shared formula that fills {}:{}; overwriting it would break the cells that follow it",
                    addr(r.0, r.1),
                    addr(r.2, r.3)
                ));
            }
        }
        Ok(())
    }

    fn is_date(&self, s: Option<&str>) -> bool {
        s.and_then(|x| x.parse::<u32>().ok())
            .is_some_and(|k| self.dates.contains(&k))
    }

    /// Write one cell. `like`: the style a new cell takes.
    fn put(
        &mut self,
        col: u32,
        row: u32,
        v: &CellVal,
        like: Option<String>,
    ) -> std::result::Result<(), String> {
        let a = addr(col, row);
        self.guard(col, row)?;
        let p = self.p.clone();
        let existing = self
            .row(row)
            .map_err(|e| e.to_string())?
            .slots
            .get(&col)
            .map(|s| (s.s.clone(), s.val.clone()));
        let style = match &existing {
            Some((s, _)) => s.clone(),
            None => like,
        };
        if matches!(existing, Some((_, Val::Formula(_)))) {
            self.formula_touched = true;
        }
        if existing.is_none() && *v == CellVal::Null {
            return Ok(());
        }
        let sattr = style
            .as_ref()
            .map(|s| format!(" s=\"{}\"", xml::attr(s)))
            .unwrap_or_default();
        let mut effective = v.clone();
        if let CellVal::Text(t) = v
            && let Some(serial) = crate::xlsx::iso_serial_in(t, self.date1904)
        {
            if self.is_date(style.as_deref()) {
                effective = CellVal::Num(serial);
            } else {
                let n = format!(
                    "{a} was written as text: the cell has no date format, and an edit does not change formats"
                );
                if self.notes.len() < 5 && !self.notes.contains(&n) {
                    self.notes.push(n);
                }
            }
        }
        let (xml, val) = match &effective {
            CellVal::Null => (format!("<{p}c r=\"{a}\"{sattr}/>"), Val::Empty),
            CellVal::Num(x) => (
                format!("<{p}c r=\"{a}\"{sattr}><{p}v>{x}</{p}v></{p}c>"),
                Val::Num(*x),
            ),
            CellVal::Bool(b) => (
                format!(
                    "<{p}c r=\"{a}\"{sattr} t=\"b\"><{p}v>{}</{p}v></{p}c>",
                    u8::from(*b)
                ),
                Val::Bool(*b),
            ),
            CellVal::Text(t) => (
                format!(
                    "<{p}c r=\"{a}\"{sattr} t=\"inlineStr\"><{p}is><{p}t xml:space=\"preserve\">{}</{p}t></{p}is></{p}c>",
                    xml::text(t)
                ),
                Val::Text(t.clone()),
            ),
            CellVal::Formula(f) => {
                self.formula_touched = true;
                (
                    format!(
                        "<{p}c r=\"{a}\"{sattr}><{p}f>{}</{p}f></{p}c>",
                        xml::text(f)
                    ),
                    Val::Formula(f.clone()),
                )
            }
        };
        let w = self.row(row).map_err(|e| e.to_string())?;
        let was_new = !w.slots.contains_key(&col);
        if effective == CellVal::Null && style.is_none() {
            w.slots.remove(&col);
        } else {
            w.slots.insert(
                col,
                Slot {
                    xml: Some(xml),
                    start: 0,
                    end: 0,
                    val,
                    s: style,
                },
            );
        }
        if was_new && w.exists {
            // Keep the row's `spans` hint right.
            if let Some(i) = w.attrs.iter().position(|(k, _)| k == "spans")
                && let Some((lo, hi)) = w.attrs[i]
                    .1
                    .split_once(':')
                    .and_then(|(a, b)| Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?)))
                && (col < lo || col > hi)
            {
                w.attrs[i].1 = format!("{}:{}", lo.min(col), hi.max(col));
                w.spans_changed = true;
            }
        }
        self.min = (self.min.0.min(col), self.min.1.min(row));
        self.max = (self.max.0.max(col), self.max.1.max(row));
        self.touched.push((col, row));
        self.wrote.push((col, row, effective));
        Ok(())
    }
}

/// Edit one sheet of an Excel file.
pub fn edit(bytes: &[u8], sheet: &str, ops: &[SheetOp]) -> Result<Outcome> {
    if ops.is_empty() {
        return Err("give at least one edit".into());
    }
    if ops.len() > MAX_EDITS {
        return Err(format!("at most {MAX_EDITS} edits in one call"));
    }
    let src = Source::open(bytes)?;
    if !src.has(WORKBOOK) {
        return Err("this is not an Excel workbook".into());
    }
    let wb_part = src.read(WORKBOOK, MAX_EDIT_PART_BYTES)?;
    let wb = read_wb(&src, &wb_part)?;
    let sh = sheet_ref(&wb, Some(sheet))?.clone();
    if !src.has(&sh.path) {
        return Err("the sheet's part is missing from the file".into());
    }
    let part = src.read(&sh.path, MAX_EDIT_PART_BYTES)?;
    let ix = index(&part)?;
    if ix.protected {
        return Err(super::E_SHEET_LOCKED.into());
    }
    let sst = shared_strings(&src)?;
    let ex = extras(&src, &sh.path)?;
    let mut cx = Ctx {
        part: &part,
        ix: &ix,
        sst: &sst,
        dates: date_styles(&src)?,
        date1904: wb.date1904,
        p: if ix.prefix.is_empty() {
            String::new()
        } else {
            format!("{}:", ix.prefix)
        },
        works: BTreeMap::new(),
        last_row: ix.rows.last().map_or(0, |r| r.r),
        min: (u32::MAX, u32::MAX),
        max: (0, 0),
        formula_touched: false,
        touched: vec![],
        notes: vec![],
        wrote: vec![],
    };
    let mut results: Vec<std::result::Result<String, String>> = vec![];
    for (k, op) in ops.iter().enumerate() {
        let label = format!("edit {} ({})", k + 1, op.name());
        let r: std::result::Result<String, String> = (|| match op {
            SheetOp::Set(cells) => {
                for (a, v) in cells {
                    let (c, r) = parse_addr(a).ok_or_else(|| {
                        format!(
                            "\"{}\" is not a cell address like B7",
                            harness_core::text::clean_line(a, 20)
                        )
                    })?;
                    cx.put(c, r, v, None)?;
                }
                Ok(format!(
                    "{} cell{} written",
                    cells.len(),
                    if cells.len() == 1 { "" } else { "s" }
                ))
            }
            SheetOp::Append {
                rows,
                like_row_above,
            } => {
                let like_row = cx.last_row;
                let like: HashMap<u32, String> = if *like_row_above && like_row > 0 {
                    cx.row(like_row)
                        .map_err(|e| e.to_string())?
                        .slots
                        .iter()
                        .filter_map(|(c, s)| s.s.clone().map(|x| (*c, x)))
                        .collect()
                } else {
                    HashMap::new()
                };
                let first = cx.last_row + 1;
                for row in rows {
                    let r = cx.last_row + 1;
                    if r > MAX_ROW {
                        return Err("the sheet is full: there is no row to append to".into());
                    }
                    cx.row(r).map_err(|e| e.to_string())?;
                    cx.last_row = r;
                    for (i, v) in row.iter().enumerate() {
                        let c = i as u32 + 1;
                        cx.put(c, r, v, like.get(&c).cloned())?;
                    }
                    if row.is_empty() {
                        cx.touched.push((1, r));
                        cx.min = (cx.min.0.min(1), cx.min.1.min(r));
                        cx.max = (cx.max.0.max(1), cx.max.1.max(r));
                    }
                }
                Ok(format!(
                    "{} row{} appended at row {}{}",
                    rows.len(),
                    if rows.len() == 1 { "" } else { "s" },
                    first,
                    if rows.len() > 1 {
                        format!(" to {}", cx.last_row)
                    } else {
                        String::new()
                    }
                ))
            }
            SheetOp::Clear(range) => {
                let rc = parse_range(range).ok_or_else(|| {
                    format!(
                        "\"{}\" is not a range like A2:C5",
                        harness_core::text::clean_line(range, 30)
                    )
                })?;
                let mut n = 0usize;
                let rows: Vec<u32> = ix
                    .rows
                    .iter()
                    .map(|r| r.r)
                    .filter(|r| *r >= rc.1 && *r <= rc.3)
                    .collect();
                for r in rows {
                    let cols: Vec<u32> = cx
                        .row(r)
                        .map_err(|e| e.to_string())?
                        .slots
                        .keys()
                        .copied()
                        .filter(|c| *c >= rc.0 && *c <= rc.2)
                        .collect();
                    for c in cols {
                        if n >= MAX_EDIT_CELLS {
                            return Err(format!(
                                "the range holds more than {MAX_EDIT_CELLS} cells; clear it in parts"
                            ));
                        }
                        cx.put(c, r, &CellVal::Null, None)?;
                        n += 1;
                    }
                }
                Ok(format!(
                    "{n} cell{} cleared in {range}",
                    if n == 1 { "" } else { "s" }
                ))
            }
        })();
        results.push(match r {
            Ok(m) => Ok(format!("{label}: {m}")),
            Err(e) => Err(format!("{label}: {e}")),
        });
    }
    if results.iter().any(|r| r.is_err()) {
        return Ok(Outcome {
            results: results
                .into_iter()
                .map(|r| match r {
                    Ok(m) => Ok(format!("{m} (not applied: another edit was refused, and a call applies all its edits or none)")),
                    e => e,
                })
                .collect(),
            bytes: None,
            notes: vec![],
        });
    }
    // The patches: every touched row, written again.
    let mut patches = Patches::new();
    let p = cx.p.clone();
    let mut new_rows: Vec<(u32, String)> = vec![];
    for (r, w) in &cx.works {
        let cells: String = w
            .slots
            .values()
            .map(|s| match &s.xml {
                Some(x) => x.clone(),
                None => String::from_utf8_lossy(&part[s.start..s.end]).into_owned(),
            })
            .collect();
        if w.exists {
            let open = if w.spans_changed {
                let attrs: String = w
                    .attrs
                    .iter()
                    .map(|(k, v)| format!(" {k}=\"{}\"", xml::attr(v)))
                    .collect();
                format!("<{}{attrs}>", w.name)
            } else if w.self_closing {
                format!("{}>", w.open_tag.trim_end_matches("/>").trim_end())
            } else {
                w.open_tag.clone()
            };
            let close = if w.self_closing {
                format!("</{}>", w.name)
            } else {
                w.close_tag.clone()
            };
            if let Some((a, b)) = w.rec {
                patches.replace(a, b, format!("{open}{cells}{close}").into_bytes());
            }
        } else {
            new_rows.push((*r, format!("<{p}row r=\"{r}\">{cells}</{p}row>")));
        }
    }
    if !new_rows.is_empty() {
        if ix.sd_empty {
            let all: String = new_rows.iter().map(|(_, x)| x.as_str()).collect();
            patches.replace(
                ix.sd.0,
                ix.sd.1,
                format!("<{p}sheetData>{all}</{p}sheetData>").into_bytes(),
            );
        } else {
            // Before the first row with a higher number, else at the end of the data.
            let mut groups: BTreeMap<usize, String> = BTreeMap::new();
            for (r, x) in &new_rows {
                let at = ix
                    .rows
                    .iter()
                    .find(|rr| rr.r > *r)
                    .map_or(ix.sd.2, |rr| rr.start);
                groups.entry(at).or_default().push_str(x);
            }
            for (at, x) in groups {
                patches.replace(at, at, x.into_bytes());
            }
        }
    }
    if let Some((a, b, d)) = ix.dim
        && cx.max.0 > 0
    {
        let (c1, r1) = (d.0.min(cx.min.0), d.1.min(cx.min.1));
        let (c2, r2) = (d.2.max(cx.max.0), d.3.max(cx.max.1));
        if (c1, r1, c2, r2) != d {
            patches.replace(
                a,
                b,
                format!("<{p}dimension ref=\"{}:{}\"/>", addr(c1, r1), addr(c2, r2)).into_bytes(),
            );
        }
    }
    let out_part = patches.apply(&part, MAX_EDIT_OUT_PART_BYTES)?;
    super::docx::wellformed_ns(&out_part, NS_S)?;
    super::scan::all_prefixes_bound(&out_part)?;
    let again = index(&out_part).map_err(|e| {
        format!("the edited sheet did not pass its own check ({e}); nothing was written")
    })?;
    // Only the last write of a cell is what the file must say.
    let mut last: BTreeMap<(u32, u32), &CellVal> = BTreeMap::new();
    for (col, row, v) in &cx.wrote {
        last.insert((*col, *row), v);
    }
    for ((col, row), want) in last {
        let (col, row) = (&col, &row);
        let got = match again.rows.binary_search_by_key(row, |x| x.r) {
            Ok(i) => {
                let rr = &again.rows[i];
                scan_row(&out_part[rr.start..rr.end], &again.prefix, &sst)?
                    .cells
                    .iter()
                    .find(|c| c.col == *col)
                    .map(|c| c.val.clone())
                    .unwrap_or(Val::Empty)
            }
            Err(_) => Val::Empty,
        };
        let ok = match (want, &got) {
            (CellVal::Null, Val::Empty) => true,
            (CellVal::Num(a), Val::Num(b)) => a == b,
            (CellVal::Text(a), Val::Text(b)) => a == b,
            (CellVal::Bool(a), Val::Bool(b)) => a == b,
            (CellVal::Formula(a), Val::Formula(b)) => a == b,
            _ => false,
        };
        if !ok {
            return Err("the edited sheet did not read back as the edits say (a check of its own); nothing was written".into());
        }
    }
    // The workbook: ask for a full calculation; drop the chain when formula cells changed.
    let mut rw = Rewrite::default();
    rw.replaced.insert(sh.path.clone(), out_part);
    let mut wp = Patches::new();
    let calc = |attrs: &[(String, String)]| {
        let mut s = format!("<{p}calcPr");
        let mut has = false;
        for (k, v) in attrs {
            if k == "fullCalcOnLoad" {
                has = true;
                s.push_str(" fullCalcOnLoad=\"1\"");
            } else {
                s.push_str(&format!(" {k}=\"{}\"", xml::attr(v)));
            }
        }
        if !has {
            s.push_str(" fullCalcOnLoad=\"1\"");
        }
        s.push_str("/>");
        s
    };
    // The workbook's own prefix, which may differ from the sheet's.
    let wprefix = {
        let s = Scan::new(&wb_part, NS_S, MAX_EDIT_EVENTS)?;
        let mut s = s;
        let _ = s.next()?;
        s.prefix().to_string()
    };
    let wp_pfx = if wprefix.is_empty() {
        String::new()
    } else {
        format!("{wprefix}:")
    };
    let calc = |attrs: &[(String, String)]| {
        calc(attrs).replace(&format!("<{p}calcPr"), &format!("<{wp_pfx}calcPr"))
    };
    match &wb.calc_pr {
        Some((a, b, attrs)) => wp.replace(*a, *b, calc(attrs).into_bytes()),
        None => wp.replace(wb.calc_at, wb.calc_at, calc(&[]).into_bytes()),
    }
    rw.replaced.insert(
        WORKBOOK.into(),
        wp.apply(&wb_part, MAX_EDIT_PART_BYTES + 4096)?,
    );
    if cx.formula_touched && wb.has_calc_chain {
        rw.removed.insert("xl/calcChain.xml".into());
        let ct = src.read("[Content_Types].xml", 4 << 20)?;
        let np = remove_elements(&ct, super::scan::NS_CT, |s, e| {
            s.local(e.name()) == Some("Override")
                && attr(e, "PartName")
                    .ok()
                    .flatten()
                    .is_some_and(|x| x.eq_ignore_ascii_case("/xl/calcChain.xml"))
        })?;
        rw.replaced.insert("[Content_Types].xml".into(), np);
        let rels = src.read("xl/_rels/workbook.xml.rels", 4 << 20)?;
        let np = remove_elements(&rels, NS_REL, |s, e| {
            s.local(e.name()) == Some("Relationship")
                && attr(e, "Type")
                    .ok()
                    .flatten()
                    .is_some_and(|x| x.ends_with("/calcChain"))
        })?;
        rw.replaced.insert("xl/_rels/workbook.xml.rels".into(), np);
    }
    let out = src.rewrite(&rw)?;
    super::check::package(bytes, &out)?;
    // What the person should know.
    let mut notes = cx.notes.clone();
    notes.push("Formulas are calculated when the file is opened in Excel; the file holds no calculated value for a formula you wrote, and other cached results are refreshed then.".into());
    // Cells touched, once each and at most `MAX_NOTE_CELLS`; the ranges they
    // are checked against are parsed (never the file's own text) and bounded,
    // so the work is a fixed number of comparisons whatever the file holds.
    let touched: std::collections::BTreeSet<(u32, u32)> =
        cx.touched.iter().copied().take(MAX_NOTE_CELLS).collect();
    let mut checked_all = cx.touched.len() <= MAX_NOTE_CELLS;
    let mut groups: Vec<(&str, Vec<Rect>)> = vec![];
    for (what, list) in [
        ("conditional formatting", &ix.cond),
        ("data validation", &ix.valid),
    ] {
        let (mut r, more) = sq_rects(list);
        checked_all &= !more && r.len() <= MAX_NOTE_RANGES;
        r.truncate(MAX_NOTE_RANGES);
        groups.push((what, r));
    }
    let mut seen: HashSet<String> = HashSet::new();
    for (c, r) in &touched {
        for (n, rc) in ex.tables.iter().take(MAX_NOTE_RANGES) {
            if adjoins(*rc, *c, *r) && seen.insert(format!("t{n}")) {
                notes.push(format!(
                    "{} is in or next to the table \"{}\" ({}:{}); the table's range is not extended.",
                    addr(*c, *r),
                    harness_core::text::clean_line(n, 40),
                    addr(rc.0, rc.1),
                    addr(rc.2, rc.3)
                ));
            }
        }
        for (what, rects) in &groups {
            for rc in rects {
                let range = format!("{}:{}", addr(rc.0, rc.1), addr(rc.2, rc.3));
                if adjoins(*rc, *c, *r) && seen.insert(format!("{what}{range}")) {
                    notes.push(format!(
                        "{} is in or next to {what} on {range}; its range is not extended.",
                        addr(*c, *r)
                    ));
                }
            }
        }
        if let Some(af) = &ix.autofilter
            && let Some(rc) = parse_range(af)
            && adjoins(rc, *c, *r)
            && seen.insert("af".into())
        {
            notes.push(format!(
                "{} is in or next to the filter on {}:{}; the filter's range is not extended.",
                addr(*c, *r),
                addr(rc.0, rc.1),
                addr(rc.2, rc.3)
            ));
        }
        if notes.len() > MAX_NOTES {
            break;
        }
    }
    if notes.len() > MAX_NOTES {
        notes.truncate(MAX_NOTES);
        checked_all = false;
    }
    if !checked_all {
        notes.push("More cells or ranges than are checked here may be next to tables, filters, validations or conditional formats; none of them is extended.".into());
    }
    if ex.pivots > 0 {
        notes.push("This sheet has a pivot table; it is not refreshed by an edit.".into());
    }
    Ok(Outcome {
        results,
        bytes: Some(out),
        notes,
    })
}

/// `part` without the elements `pred` picks (each removed whole).
fn remove_elements(
    part: &[u8],
    ns: &'static str,
    pred: impl Fn(&Scan<'_>, &BytesStart<'_>) -> bool,
) -> Result<Vec<u8>> {
    let mut s = Scan::new(part, ns, MAX_EDIT_EVENTS)?;
    let mut p = Patches::new();
    let mut root = true;
    while let Some(t) = s.next()? {
        let (e, empty) = match &t.ev {
            Event::Start(e) => (e, false),
            Event::Empty(e) => (e, true),
            _ => continue,
        };
        if root {
            root = false;
            continue;
        }
        if pred(&s, e) {
            let end = if empty {
                t.end
            } else {
                s.skip_to_end(t.depth)?
            };
            p.replace(t.start, end, vec![]);
        }
    }
    p.apply(part, part.len())
}
