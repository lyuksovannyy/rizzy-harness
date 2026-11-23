//! A JSON sheet spec to `.xlsx` (PLAN.md T97, report 4.2). A spreadsheet is a
//! typed grid, so the model writes a small spec and never Excel's own
//! vocabulary:
//!
//! ```json
//! {"currency": "$", "sheets": [{
//!   "name": "Q1", "freeze_header": true, "autofilter": true, "bold_last_row": true,
//!   "columns": [{"header": "Item", "width": 28}, {"header": "Cost", "format": "currency"},
//!               {"header": "Due", "format": "date"}],
//!   "rows": [["Hosting", 1200.5, "2026-12-01"], ["Total", {"f": "SUM(B2:B2)"}, null]]}]}
//! ```
//!
//! A cell is a number, a string, a boolean, null (an empty cell) or
//! `{"f": "SUM(B2:B9)"}`. A string is ALWAYS text, never a formula, so one
//! that starts with `=`, `+`, `-` or `@` stays what it says. The formats are
//! a closed list of names, not Excel format codes. An ISO date in a date
//! column becomes a real date. Formulas pass [`crate::formula::check`] and
//! are written without a cached value; the workbook asks Excel for a full
//! calculation when it is opened. Text is written as inline strings, so no
//! shared-string table is needed.

use serde_json::{Map, Value};

use crate::Result;
use crate::formula;
use crate::limits::*;
use crate::xml::{self, ContentTypes, DECL, REL_BASE, REL_CORE};
use crate::zip::ZipWriter;

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// The closed list of cell formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    General,
    Text,
    Integer,
    Number,
    Number2,
    Percent,
    Currency,
    Date,
    DateTime,
}

const FORMATS: [Format; 9] = [
    Format::General,
    Format::Text,
    Format::Integer,
    Format::Number,
    Format::Number2,
    Format::Percent,
    Format::Currency,
    Format::Date,
    Format::DateTime,
];

/// The names a column's `format` may have.
pub const FORMAT_NAMES: &str = "text, integer, number, number2, percent, currency, date, datetime";

impl Format {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "text" => Self::Text,
            "integer" => Self::Integer,
            "number" => Self::Number,
            "number2" => Self::Number2,
            "percent" => Self::Percent,
            "currency" => Self::Currency,
            "date" => Self::Date,
            "datetime" => Self::DateTime,
            _ => return None,
        })
    }

    fn index(self) -> usize {
        FORMATS.iter().position(|f| *f == self).unwrap_or(0)
    }

    fn num_fmt_id(self) -> u32 {
        match self {
            Self::General => 0,
            Self::Text => 49,
            Self::Integer => 1,
            Self::Number => 3,
            Self::Number2 => 4,
            Self::Percent => 10,
            Self::Currency => 164,
            Self::Date => 165,
            Self::DateTime => 166,
        }
    }
}

/// The style index of a cell of `f`, bold or not. Index 0 is the default.
fn xf(f: Format, bold: bool) -> usize {
    f.index() * 2 + usize::from(bold)
}

/// The header cells' style: bold on a pale fill.
const XF_HEADER: usize = FORMATS.len() * 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetReport {
    /// The name the sheet has in the file (cleaned and made unique).
    pub name: String,
    /// The name the model asked for, when it had to change.
    pub renamed_from: Option<String>,
    /// Rows written, header included.
    pub rows: usize,
    pub cols: usize,
    pub formulas: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub sheets: Vec<SheetReport>,
    pub formulas: usize,
    /// Strings in a date column that are not ISO dates and so stay text.
    pub dates_left_as_text: usize,
}

#[derive(Debug)]
pub struct Built {
    pub bytes: Vec<u8>,
    pub report: Report,
}

fn short(s: &str) -> String {
    harness_core::text::clean_line(s, 40)
}

/// `A`, `B`, … `Z`, `AA` for a 0-based column.
pub fn col_letters(mut c: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (c % 26) as u8);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// The address of a 0-based column and row.
fn addr(col: usize, row: usize) -> String {
    format!("{}{}", col_letters(col), row + 1)
}

/// The sheet name Excel takes: at most 31 characters, none of `: \ / ? * [ ]`,
/// not starting or ending with `'`, not empty, not `History`, and different
/// from every earlier one even by letter case.
fn sheet_name(asked: &str, n: usize, taken: &[String]) -> String {
    let mut s: String = xml::clean(&harness_core::text::clean_line(asked, 200))
        .chars()
        .map(|c| if ":\\/?*[]".contains(c) { '_' } else { c })
        .collect();
    s = s.trim_matches('\'').trim().to_string();
    s = s
        .chars()
        .take(MAX_SHEET_NAME)
        .collect::<String>()
        .trim()
        .to_string();
    if s.is_empty() {
        s = format!("Sheet{n}");
    }
    if s.eq_ignore_ascii_case("history") {
        s.push('_');
    }
    let mut cand = s.clone();
    let mut k = 2;
    while taken.iter().any(|t| t.eq_ignore_ascii_case(&cand)) {
        let suffix = format!(" ({k})");
        let keep: String = s
            .chars()
            .take(MAX_SHEET_NAME.saturating_sub(suffix.chars().count()))
            .collect();
        cand = format!("{}{suffix}", keep.trim_end());
        k += 1;
    }
    cand
}

/// An ISO date, `2026-12-01`, or date and time, `2026-12-01T09:30[:00]` (a
/// space may stand for the `T`, a trailing `Z` is ignored), as an Excel
/// serial number (1900 date system), or `None`.
pub fn iso_serial(s: &str) -> Option<f64> {
    let s = s.trim();
    let s = s.strip_suffix('Z').unwrap_or(s);
    let (d, t) = match s.split_once(['T', ' ']) {
        Some((d, t)) => (d, Some(t)),
        None => (s, None),
    };
    let mut dp = d.split('-');
    let (y, m, day) = (dp.next()?, dp.next()?, dp.next()?);
    if dp.next().is_some() || y.len() != 4 || m.len() != 2 || day.len() != 2 {
        return None;
    }
    let (y, m, day): (i64, i64, i64) = (y.parse().ok()?, m.parse().ok()?, day.parse().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) {
        return None;
    }
    let days = crate::days_from_civil(y, m, day);
    // Reject days a month does not have (31 February): round-trip the date.
    if crate::civil_from_days(days) != (y, m, day) {
        return None;
    }
    // Excel's 1900 date system counts a 29 February 1900 that never was:
    // serials of earlier dates would be off by one, so they are not made.
    let serial = days - crate::days_from_civil(1899, 12, 30);
    if !(61..=2_958_465).contains(&serial) {
        return None;
    }
    let mut frac = 0.0;
    if let Some(t) = t {
        let mut tp = t.split(':');
        let h: u32 = tp.next()?.parse().ok()?;
        let mi: u32 = tp.next()?.parse().ok()?;
        let sec: u32 = match tp.next() {
            Some(x) => x.parse().ok()?,
            None => 0,
        };
        if tp.next().is_some() || h > 23 || mi > 59 || sec > 59 {
            return None;
        }
        frac = f64::from(h * 3600 + mi * 60 + sec) / 86_400.0;
    }
    Some(serial as f64 + frac)
}

struct Col {
    header: Option<String>,
    width: Option<f64>,
    format: Format,
}

fn only<'a>(
    o: &'a Map<String, Value>,
    allowed: &[&str],
    what: &str,
) -> Result<&'a Map<String, Value>> {
    for k in o.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(format!(
                "{what} has a field `{}` that is not part of the spec (fields: {})",
                short(k),
                allowed.join(", ")
            ));
        }
    }
    Ok(o)
}

fn flag(o: &Map<String, Value>, key: &str, default: bool, what: &str) -> Result<bool> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        _ => Err(format!("{what}: `{key}` is true or false")),
    }
}

fn currency(v: Option<&Value>) -> Result<String> {
    let Some(v) = v.filter(|v| !v.is_null()) else {
        return Ok("$".into());
    };
    let s = v
        .as_str()
        .ok_or("`currency` is a short text such as \"$\" or \"EUR\"")?;
    let s = xml::clean(s);
    let ok = !s.is_empty()
        && s.chars().count() <= 3
        && s.chars()
            .all(|c| c.is_alphanumeric() || "$€£¥₹₽₩₺₴₫₪₦".contains(c));
    if !ok {
        return Err(
            "`currency` is one to three letters or currency signs, such as \"$\" or \"EUR\"".into(),
        );
    }
    Ok(s)
}

struct Sheet {
    name: String,
    renamed_from: Option<String>,
    cols: Vec<Col>,
    has_header: bool,
    rows: Vec<Vec<Value>>,
    freeze: bool,
    filter: bool,
    bold_last: bool,
}

fn parse(spec: &Value, taken: &mut Vec<String>) -> Result<(Vec<Sheet>, String)> {
    let top = spec
        .as_object()
        .ok_or("the spec is an object: {\"sheets\": [...]}")?;
    only(top, &["sheets", "currency"], "the spec")?;
    let cur = currency(top.get("currency"))?;
    let sheets = top
        .get("sheets")
        .and_then(Value::as_array)
        .ok_or("`sheets` is a list of sheets")?;
    if sheets.is_empty() {
        return Err("`sheets` has no sheet".into());
    }
    if sheets.len() > MAX_SHEETS {
        return Err(format!("a workbook has at most {MAX_SHEETS} sheets"));
    }
    let mut out = Vec::new();
    let mut cells = 0usize;
    for (si, s) in sheets.iter().enumerate() {
        let what = format!("sheet {}", si + 1);
        let o = s
            .as_object()
            .ok_or_else(|| format!("{what} is an object"))?;
        only(
            o,
            &[
                "name",
                "columns",
                "rows",
                "freeze_header",
                "autofilter",
                "bold_last_row",
            ],
            &what,
        )?;
        let asked = match o.get("name") {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(n)) => n.clone(),
            _ => return Err(format!("{what}: `name` is text")),
        };
        let name = sheet_name(&asked, si + 1, taken);
        taken.push(name.clone());
        let renamed_from = (!asked.is_empty()
            && harness_core::text::clean_line(&asked, 200) != name)
            .then(|| harness_core::text::clean_line(&asked, 60));
        let mut cols = Vec::new();
        match o.get("columns") {
            None | Some(Value::Null) => {}
            Some(Value::Array(a)) => {
                if a.len() > MAX_COLS {
                    return Err(format!("{what} has more than {MAX_COLS} columns"));
                }
                for (ci, c) in a.iter().enumerate() {
                    let cw = format!("{what}, column {}", col_letters(ci));
                    let co = c
                        .as_object()
                        .ok_or_else(|| format!("{cw} is an object with a `header`"))?;
                    only(co, &["header", "width", "format"], &cw)?;
                    let header = match co.get("header") {
                        None | Some(Value::Null) => None,
                        Some(Value::String(h)) => {
                            let h = xml::clean(h);
                            if h.chars().count() > MAX_HEADER_CHARS {
                                return Err(format!(
                                    "{cw}: a header is at most {MAX_HEADER_CHARS} characters"
                                ));
                            }
                            Some(h)
                        }
                        _ => return Err(format!("{cw}: `header` is text")),
                    };
                    let width = match co.get("width") {
                        None | Some(Value::Null) => None,
                        Some(Value::Number(n)) => {
                            let w = n.as_f64().unwrap_or(0.0);
                            if !(w > 0.0 && w <= MAX_COL_WIDTH) {
                                return Err(format!(
                                    "{cw}: `width` is a number of characters, above 0 and at most {MAX_COL_WIDTH}"
                                ));
                            }
                            Some(w)
                        }
                        _ => return Err(format!("{cw}: `width` is a number")),
                    };
                    let format = match co.get("format") {
                        None | Some(Value::Null) => Format::General,
                        Some(Value::String(f)) => Format::parse(f)
                            .ok_or_else(|| format!("{cw}: `format` is one of {FORMAT_NAMES}"))?,
                        _ => return Err(format!("{cw}: `format` is text")),
                    };
                    cols.push(Col {
                        header,
                        width,
                        format,
                    });
                }
            }
            _ => return Err(format!("{what}: `columns` is a list")),
        }
        let has_header = !cols.is_empty() && cols.iter().any(|c| c.header.is_some());
        let rows = match o.get("rows") {
            None | Some(Value::Null) => vec![],
            Some(Value::Array(a)) => {
                let mut rows = Vec::with_capacity(a.len().min(MAX_ROWS));
                for (ri, r) in a.iter().enumerate() {
                    let arr = r.as_array().ok_or_else(|| {
                        format!("{what}, row {}: a row is a list of cells", ri + 1)
                    })?;
                    if arr.len() > MAX_COLS {
                        return Err(format!("{what} has a row with more than {MAX_COLS} cells"));
                    }
                    cells += arr.len();
                    if cells > MAX_CELLS {
                        return Err(format!(
                            "the workbook has more than {MAX_CELLS} cells; split it"
                        ));
                    }
                    if ri + usize::from(has_header) >= MAX_ROWS {
                        return Err(format!("{what} has more than {MAX_ROWS} rows; split it"));
                    }
                    rows.push(arr.clone());
                }
                rows
            }
            _ => return Err(format!("{what}: `rows` is a list of rows")),
        };
        cells += if has_header { cols.len() } else { 0 };
        if cells > MAX_CELLS {
            return Err(format!(
                "the workbook has more than {MAX_CELLS} cells; split it"
            ));
        }
        out.push(Sheet {
            name,
            renamed_from,
            has_header,
            freeze: flag(o, "freeze_header", has_header, &what)? && has_header,
            filter: flag(o, "autofilter", has_header, &what)? && has_header,
            bold_last: flag(o, "bold_last_row", false, &what)?,
            cols,
            rows,
        });
    }
    Ok((out, cur))
}

fn num(n: &serde_json::Number) -> Option<String> {
    let f = n.as_f64()?;
    f.is_finite().then(|| format!("{f}"))
}

fn text_cell(a: &str, style: usize, s: &str) -> String {
    format!(
        "<c r=\"{a}\" s=\"{style}\" t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
        xml::text(s)
    )
}

struct Done {
    xml: String,
    rows: usize,
    cols: usize,
    formulas: usize,
    dates_text: usize,
}

fn sheet_xml(s: &Sheet, first: bool) -> Result<Done> {
    let mut data = String::new();
    let mut formulas = 0;
    let mut dates_text = 0;
    let mut ncols = s.cols.len();
    let header_rows = usize::from(s.has_header);
    let total_rows = header_rows + s.rows.len();
    let mut widths: Vec<usize> = vec![0; s.cols.len().max(1)];
    if s.has_header {
        data.push_str("<row r=\"1\">");
        for (ci, c) in s.cols.iter().enumerate() {
            if let Some(h) = &c.header {
                data.push_str(&text_cell(&addr(ci, 0), XF_HEADER, h));
                widths[ci] = widths[ci].max(h.chars().count());
            }
        }
        data.push_str("</row>");
    }
    for (ri, r) in s.rows.iter().enumerate() {
        let row = ri + header_rows;
        let last = ri + 1 == s.rows.len();
        let bold = s.bold_last && last;
        ncols = ncols.max(r.len());
        if r.is_empty() {
            continue;
        }
        data.push_str(&format!("<row r=\"{}\">", row + 1));
        for (ci, v) in r.iter().enumerate() {
            let a = addr(ci, row);
            let fmt = s.cols.get(ci).map_or(Format::General, |c| c.format);
            let st = xf(fmt, bold);
            match v {
                Value::Null => {}
                Value::Bool(b) => data.push_str(&format!(
                    "<c r=\"{a}\" s=\"{st}\" t=\"b\"><v>{}</v></c>",
                    u8::from(*b)
                )),
                Value::Number(n) => {
                    let t = num(n).ok_or_else(|| {
                        format!("cell {a} of {}: a number Excel cannot hold", short(&s.name))
                    })?;
                    if ci < widths.len() && ri < 200 {
                        widths[ci] = widths[ci].max(t.len());
                    }
                    data.push_str(&format!("<c r=\"{a}\" s=\"{st}\"><v>{t}</v></c>"));
                }
                Value::String(t) => {
                    let t = xml::clean(t);
                    if t.chars().count() > MAX_CELL_CHARS {
                        return Err(format!(
                            "cell {a} of sheet \"{}\" has more than {MAX_CELL_CHARS} characters",
                            short(&s.name)
                        ));
                    }
                    if matches!(fmt, Format::Date | Format::DateTime) {
                        if let Some(serial) = iso_serial(&t) {
                            data.push_str(&format!("<c r=\"{a}\" s=\"{st}\"><v>{serial}</v></c>"));
                            if ci < widths.len() && ri < 200 {
                                widths[ci] =
                                    widths[ci].max(if fmt == Format::Date { 10 } else { 16 });
                            }
                            continue;
                        }
                        if !t.trim().is_empty() {
                            dates_text += 1;
                        }
                    }
                    if ci < widths.len() && ri < 200 {
                        widths[ci] = widths[ci].max(t.chars().count());
                    }
                    data.push_str(&text_cell(&a, st, &t));
                }
                Value::Object(o) => {
                    only(o, &["f"], &format!("cell {a}"))?;
                    let f = o.get("f").and_then(Value::as_str).ok_or_else(|| {
                        format!(
                            "cell {a} of sheet \"{}\": `f` is the formula text",
                            short(&s.name)
                        )
                    })?;
                    let f = formula::check(f)
                        .map_err(|e| format!("cell {a} of sheet \"{}\": {e}", short(&s.name)))?;
                    formulas += 1;
                    if ci < widths.len() && ri < 200 {
                        widths[ci] = widths[ci].max(10);
                    }
                    data.push_str(&format!(
                        "<c r=\"{a}\" s=\"{st}\"><f>{}</f></c>",
                        xml::text(&f)
                    ));
                }
                Value::Array(_) => {
                    return Err(format!(
                        "cell {a} of sheet \"{}\" is a list: a cell is a number, text, true or false, null or {{\"f\": ...}}",
                        short(&s.name)
                    ));
                }
            }
        }
        data.push_str("</row>");
    }
    let ncols = ncols.max(1);
    let last_ref = addr(ncols - 1, total_rows.max(1) - 1);
    let mut x = String::from(DECL);
    x.push_str(&format!("<worksheet xmlns=\"{MAIN}\" xmlns:r=\"{R}\">"));
    x.push_str(&format!("<dimension ref=\"A1:{last_ref}\"/>"));
    x.push_str(&format!(
        "<sheetViews><sheetView workbookViewId=\"0\"{}>",
        if first { " tabSelected=\"1\"" } else { "" }
    ));
    if s.freeze {
        x.push_str(
            "<pane ySplit=\"1\" topLeftCell=\"A2\" activePane=\"bottomLeft\" state=\"frozen\"/>\
             <selection pane=\"bottomLeft\" activeCell=\"A2\" sqref=\"A2\"/>",
        );
    }
    x.push_str("</sheetView></sheetViews><sheetFormatPr defaultRowHeight=\"15\"/>");
    x.push_str("<cols>");
    for ci in 0..ncols {
        let w = match s.cols.get(ci).and_then(|c| c.width) {
            Some(w) => w,
            None => {
                let l = widths.get(ci).copied().unwrap_or(0) as f64;
                (l * 1.1 + 2.0).clamp(8.43, 60.0)
            }
        };
        x.push_str(&format!(
            "<col min=\"{0}\" max=\"{0}\" width=\"{w:.2}\" customWidth=\"1\"/>",
            ci + 1
        ));
    }
    x.push_str("</cols><sheetData>");
    x.push_str(&data);
    x.push_str("</sheetData>");
    if s.filter {
        x.push_str(&format!(
            "<autoFilter ref=\"A1:{}\"/>",
            addr(s.cols.len().max(1) - 1, total_rows.max(1) - 1)
        ));
    }
    x.push_str(
        "<pageMargins left=\"0.7\" right=\"0.7\" top=\"0.75\" bottom=\"0.75\" header=\"0.3\" footer=\"0.3\"/></worksheet>",
    );
    Ok(Done {
        xml: x,
        rows: total_rows,
        cols: ncols,
        formulas,
        dates_text,
    })
}

fn styles_xml(cur: &str) -> String {
    let mut s = String::from(DECL);
    s.push_str(&format!("<styleSheet xmlns=\"{MAIN}\">"));
    s.push_str(&format!(
        "<numFmts count=\"3\"><numFmt numFmtId=\"164\" formatCode=\"&quot;{}&quot;#,##0.00\"/>\
         <numFmt numFmtId=\"165\" formatCode=\"yyyy\\-mm\\-dd\"/>\
         <numFmt numFmtId=\"166\" formatCode=\"yyyy\\-mm\\-dd\\ hh:mm\"/></numFmts>",
        xml::attr(cur)
    ));
    s.push_str(
        "<fonts count=\"2\"><font><sz val=\"11\"/><name val=\"Calibri\"/><family val=\"2\"/></font>\
         <font><b/><sz val=\"11\"/><name val=\"Calibri\"/><family val=\"2\"/></font></fonts>\
         <fills count=\"3\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill>\
         <fill><patternFill patternType=\"solid\"><fgColor rgb=\"FFD9E2F3\"/><bgColor indexed=\"64\"/></patternFill></fill></fills>\
         <borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders>\
         <cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>",
    );
    s.push_str(&format!("<cellXfs count=\"{}\">", XF_HEADER + 1));
    for f in FORMATS {
        for bold in [false, true] {
            s.push_str(&format!(
                "<xf numFmtId=\"{}\" fontId=\"{}\" fillId=\"0\" borderId=\"0\" xfId=\"0\"{}{}/>",
                f.num_fmt_id(),
                u8::from(bold),
                if f.num_fmt_id() != 0 {
                    " applyNumberFormat=\"1\""
                } else {
                    ""
                },
                if bold { " applyFont=\"1\"" } else { "" },
            ));
        }
    }
    s.push_str(
        "<xf numFmtId=\"0\" fontId=\"1\" fillId=\"2\" borderId=\"0\" xfId=\"0\" applyFont=\"1\" applyFill=\"1\"/></cellXfs>\
         <cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles></styleSheet>",
    );
    s
}

/// Build the `.xlsx` of a sheet spec.
pub fn build(spec: &Value, created: i64) -> Result<Built> {
    let mut taken = Vec::new();
    let (sheets, cur) = parse(spec, &mut taken)?;
    let mut z = ZipWriter::new(MAX_OUTPUT_BYTES);
    let mut ct = ContentTypes::new();
    ct.part(
        "xl/workbook.xml",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml",
    );
    ct.part(
        "xl/styles.xml",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml",
    );
    ct.part(
        "docProps/core.xml",
        "application/vnd.openxmlformats-package.core-properties+xml",
    );
    let mut report = Report::default();
    let mut parts: Vec<(String, String)> = Vec::new();
    let mut wb_sheets = String::new();
    let mut names = String::new();
    let mut wrels = Vec::new();
    for (i, s) in sheets.iter().enumerate() {
        let d = sheet_xml(s, i == 0)?;
        let part = format!("xl/worksheets/sheet{}.xml", i + 1);
        ct.part(
            &part,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml",
        );
        wb_sheets.push_str(&format!(
            "<sheet name=\"{}\" sheetId=\"{}\" r:id=\"rId{}\"/>",
            xml::attr(&s.name),
            i + 1,
            i + 1
        ));
        wrels.push((
            format!("rId{}", i + 1),
            format!("{REL_BASE}/worksheet"),
            format!("worksheets/sheet{}.xml", i + 1),
            false,
        ));
        if s.filter {
            let q = s.name.replace('\'', "''");
            names.push_str(&format!(
                "<definedName name=\"_xlnm._FilterDatabase\" localSheetId=\"{i}\" hidden=\"1\">'{}'!$A$1:${}${}</definedName>",
                xml::text(&q),
                col_letters(s.cols.len().max(1) - 1),
                d.rows.max(1)
            ));
        }
        report.formulas += d.formulas;
        report.dates_left_as_text += d.dates_text;
        report.sheets.push(SheetReport {
            name: s.name.clone(),
            renamed_from: s.renamed_from.clone(),
            rows: d.rows,
            cols: d.cols,
            formulas: d.formulas,
        });
        parts.push((part, d.xml));
    }
    wrels.push((
        format!("rId{}", sheets.len() + 1),
        format!("{REL_BASE}/styles"),
        "styles.xml".to_string(),
        false,
    ));
    let mut wb = String::from(DECL);
    wb.push_str(&format!(
        "<workbook xmlns=\"{MAIN}\" xmlns:r=\"{R}\"><workbookPr/><bookViews><workbookView xWindow=\"0\" yWindow=\"0\" windowWidth=\"28800\" windowHeight=\"12300\"/></bookViews>\
         <sheets>{wb_sheets}</sheets>"
    ));
    if !names.is_empty() {
        wb.push_str(&format!("<definedNames>{names}</definedNames>"));
    }
    // A full calculation when the file is opened: no formula has a cached
    // value, and a viewer with no engine shows none.
    wb.push_str("<calcPr calcId=\"191029\" fullCalcOnLoad=\"1\"/></workbook>");
    let root_rels = vec![
        (
            "rId1".to_string(),
            format!("{REL_BASE}/officeDocument"),
            "xl/workbook.xml".to_string(),
            false,
        ),
        (
            "rId2".to_string(),
            REL_CORE.to_string(),
            "docProps/core.xml".to_string(),
            false,
        ),
    ];
    z.add("[Content_Types].xml", ct.xml().as_bytes())?;
    z.add("_rels/.rels", xml::rels(&root_rels).as_bytes())?;
    z.add("xl/workbook.xml", wb.as_bytes())?;
    z.add("xl/_rels/workbook.xml.rels", xml::rels(&wrels).as_bytes())?;
    for (p, x) in &parts {
        z.add(p, x.as_bytes())?;
    }
    z.add("xl/styles.xml", styles_xml(&cur).as_bytes())?;
    z.add(
        "docProps/core.xml",
        xml::core_props(None, &crate::iso_utc(created)).as_bytes(),
    )?;
    Ok(Built {
        bytes: z.finish()?,
        report,
    })
}
