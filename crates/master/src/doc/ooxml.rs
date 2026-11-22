//! Word, Excel and PowerPoint files (Office Open XML): zips of XML parts.
//! Parts are found through the package's own relationships, and only ever
//! by name inside the zip.

use std::collections::HashMap;

use harness_attached::{Package, PartError};
use harness_core::text::clean_line;
use quick_xml::events::BytesStart;

use super::flow::{CELL_CHARS, Flow, LINE_CHARS, Out};
use super::xml::{Ev, Xml, attr, end_name, name};
use super::{MAX_COLS, MAX_ROWS, Res, fail, fail_part};

/// Shared strings kept: a workbook of a million strings reads the first
/// part of them (a cell past it is empty).
const MAX_SHARED: usize = 100_000;
/// Characters kept of one shared string, before the cell cap.
const SHARED_CHARS: usize = 2_000;
/// Bytes of shared strings kept in all.
const SHARED_BYTES: usize = 16 << 20;
/// The largest relationships or styles part read.
const SMALL: usize = 4 << 20;

fn open<'p>(pkg: &'p Package<'p>, part: &str) -> Result<Xml<'p>, PartError> {
    Ok(Xml::new(pkg.read(part)?))
}

/// `target` as a part name, relative to the folder `base` (`xl/`), the
/// package root for a leading `/`. `.` and `..` are resolved inside the
/// package: a name can only ever pick a part of it.
fn resolve(base: &str, target: &str) -> String {
    let mut parts: Vec<&str> = if target.starts_with('/') {
        Vec::new()
    } else {
        base.split('/').filter(|s| !s.is_empty()).collect()
    };
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// A relationship: its id, its type's last word and its target (an internal
/// one, as a part name).
struct Rel {
    id: String,
    kind: String,
    target: String,
}

/// The relationships of `part` (`xl/workbook.xml` →
/// `xl/_rels/workbook.xml.rels`); none when it has no such part.
fn relationships(pkg: &Package<'_>, part: &str) -> Result<Vec<Rel>, String> {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    let rel = if dir.is_empty() {
        format!("_rels/{file}.rels")
    } else {
        format!("{dir}/_rels/{file}.rels")
    };
    let bytes = match pkg.read_small(&rel, SMALL) {
        Ok(b) => b,
        Err(PartError::Missing) => return Ok(Vec::new()),
        Err(e) => return Err(fail_part(pkg, e)),
    };
    let mut x = Xml::new(Box::new(&bytes[..]));
    let mut out = Vec::new();
    loop {
        match x.next().map_err(|f| fail(pkg, f))? {
            Ev::Open(e) if name(&e) == "Relationship" => {
                if attr(&e, "TargetMode").as_deref() == Some("External") {
                    continue;
                }
                let (Some(id), Some(target)) = (attr(&e, "Id"), attr(&e, "Target")) else {
                    continue;
                };
                let kind = attr(&e, "Type").unwrap_or_default();
                let kind = kind.rsplit('/').next().unwrap_or("").to_string();
                out.push(Rel {
                    id,
                    kind,
                    target: resolve(dir, &target),
                });
            }
            Ev::Eof => return Ok(out),
            _ => {}
        }
    }
}

/// The attribute whose local name is `local` and which has a prefix
/// (`r:id`, not the plain `id` next to it).
fn attr_prefixed(e: &BytesStart<'_>, local: &str) -> Option<String> {
    for a in e.attributes().with_checks(false).flatten() {
        if a.key.prefix().is_some() && a.key.local_name().into_inner() == local {
            return a
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()
                .map(std::borrow::Cow::into_owned);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Word

/// The heading level a paragraph style stands for, by the style's own name
/// (`heading 2`, `Title`), whatever the language of its id.
fn heading_styles(pkg: &Package<'_>) -> Result<HashMap<String, u8>, String> {
    let mut map = HashMap::new();
    let mut x = match open(pkg, "word/styles.xml") {
        Ok(x) => x,
        Err(PartError::Missing) => return Ok(map),
        Err(e) => return Err(fail_part(pkg, e)),
    };
    let mut id: Option<String> = None;
    loop {
        match x.next().map_err(|f| fail(pkg, f))? {
            Ev::Open(e) => match name(&e) {
                "style" => id = attr(&e, "styleId"),
                "name" => {
                    if let (Some(id), Some(v)) = (&id, attr(&e, "val"))
                        && map.len() < 2000
                    {
                        let v = v.to_lowercase();
                        let level = if v == "title" {
                            Some(1)
                        } else {
                            v.strip_prefix("heading ")
                                .and_then(|n| n.trim().parse::<u8>().ok())
                                .filter(|n| (1..=9).contains(n))
                        };
                        if let Some(l) = level {
                            map.insert(id.clone(), l);
                        }
                    }
                }
                _ => {}
            },
            Ev::Close(e) if end_name(&e) == "style" => id = None,
            Ev::Eof => return Ok(map),
            _ => {}
        }
    }
}

pub fn docx(pkg: &Package<'_>, out: &mut Out) -> Res {
    let styles = heading_styles(pkg)?;
    let mut x = open(pkg, "word/document.xml").map_err(|e| fail_part(pkg, e))?;
    let mut flow = Flow::default();
    // Text inside `w:t`; inside a subtree whose text is not the reader's
    // (a tracked deletion, a fallback copy of a drawing).
    let (mut in_t, mut skip) = (false, 0usize);
    let mut in_num = false;
    while !out.full() {
        match x.next().map_err(|f| fail(pkg, f))? {
            Ev::Open(e) => {
                let n = name(&e);
                if skip > 0 {
                    skip += 1;
                    continue;
                }
                match n {
                    "del" | "moveFrom" | "Fallback" | "instrText" | "delText" => skip = 1,
                    "p" => flow.begin_para(),
                    "pStyle" => {
                        if let Some(v) = attr(&e, "val") {
                            let level = styles.get(&v).copied().or_else(|| {
                                // The built-in ids, when styles.xml does not
                                // name them.
                                let l = v.to_lowercase();
                                if l == "title" {
                                    Some(1)
                                } else {
                                    l.strip_prefix("heading")
                                        .and_then(|n| n.trim().parse::<u8>().ok())
                                        .filter(|n| (1..=9).contains(n))
                                }
                            });
                            if let Some(l) = level {
                                flow.heading(l);
                            }
                        }
                    }
                    "numPr" => {
                        in_num = true;
                        flow.list(0);
                    }
                    "ilvl" if in_num => {
                        if let Some(l) = attr(&e, "val").and_then(|v| v.parse::<u8>().ok()) {
                            flow.list(l);
                        }
                    }
                    "t" => in_t = true,
                    "tab" | "br" | "cr" => flow.text(" "),
                    "noBreakHyphen" => flow.text("-"),
                    "tbl" => flow.begin_table(),
                    "tr" => flow.begin_row(),
                    "tc" => flow.begin_cell(),
                    _ => {}
                }
            }
            Ev::Close(e) => {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                match end_name(&e) {
                    "p" => flow.end_para(out),
                    "numPr" => in_num = false,
                    "t" => in_t = false,
                    "tc" => flow.end_cell(),
                    "tr" => flow.end_row(out),
                    "tbl" => flow.end_table(),
                    _ => {}
                }
            }
            Ev::Text(t) if in_t && skip == 0 => flow.text(&t),
            Ev::Eof => break,
            _ => {}
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Excel

/// The strings cells point at. Kept up to the caps; a cell past them reads
/// as empty, and the sheet says so.
struct Shared {
    strings: Vec<String>,
    cut: bool,
}

fn shared_strings(pkg: &Package<'_>, part: &str) -> Result<Shared, String> {
    let mut s = Shared {
        strings: Vec::new(),
        cut: false,
    };
    let mut x = match open(pkg, part) {
        Ok(x) => x,
        Err(PartError::Missing) => return Ok(s),
        Err(e) => return Err(fail_part(pkg, e)),
    };
    let (mut cur, mut in_si, mut in_t, mut phonetic) = (String::new(), false, false, 0usize);
    let mut bytes = 0usize;
    loop {
        match x.next().map_err(|f| fail(pkg, f))? {
            Ev::Open(e) => match name(&e) {
                "si" => {
                    in_si = true;
                    cur.clear();
                }
                "rPh" if in_si => phonetic += 1,
                "t" if in_si && phonetic == 0 => in_t = true,
                _ => {}
            },
            Ev::Close(e) => match end_name(&e) {
                "si" => {
                    in_si = false;
                    if s.strings.len() >= MAX_SHARED || bytes >= SHARED_BYTES {
                        s.cut = true;
                        return Ok(s);
                    }
                    let c = clean_line(&cur, SHARED_CHARS);
                    bytes += c.len();
                    s.strings.push(c);
                }
                "rPh" if phonetic > 0 => phonetic -= 1,
                "t" => in_t = false,
                _ => {}
            },
            Ev::Text(t) if in_t && cur.len() < SHARED_CHARS * 4 => cur.push_str(&t),
            Ev::Eof => return Ok(s),
            _ => {}
        }
    }
}

/// The 0-based column of a cell reference (`B7` is 1, `AA1` is 26); `None`
/// when it has no letters.
fn column(r: &str) -> Option<usize> {
    let mut n = 0usize;
    let mut any = false;
    for c in r.bytes() {
        if !c.is_ascii_alphabetic() {
            break;
        }
        any = true;
        n = n
            .saturating_mul(26)
            .saturating_add(usize::from(c.to_ascii_uppercase() - b'A') + 1);
    }
    any.then(|| n - 1)
}

pub fn xlsx(pkg: &Package<'_>, out: &mut Out) -> Res {
    struct Sheet {
        name: String,
        rid: String,
        hidden: bool,
    }
    let mut sheets: Vec<Sheet> = Vec::new();
    {
        let mut x = open(pkg, "xl/workbook.xml").map_err(|e| fail_part(pkg, e))?;
        loop {
            match x.next().map_err(|f| fail(pkg, f))? {
                Ev::Open(e) if name(&e) == "sheet" && sheets.len() < 1000 => {
                    sheets.push(Sheet {
                        name: attr(&e, "name").unwrap_or_default(),
                        rid: attr_prefixed(&e, "id").unwrap_or_default(),
                        hidden: attr(&e, "state").is_some_and(|s| s != "visible"),
                    });
                }
                Ev::Eof => break,
                _ => {}
            }
        }
    }
    let rels = relationships(pkg, "xl/workbook.xml")?;
    let sst = rels
        .iter()
        .find(|r| r.kind == "sharedStrings")
        .map_or("xl/sharedStrings.xml", |r| r.target.as_str());
    let shared = shared_strings(pkg, sst)?;
    for sh in sheets.iter().filter(|s| !s.hidden) {
        if out.full() {
            break;
        }
        let Some(rel) = rels.iter().find(|r| r.id == sh.rid) else {
            continue;
        };
        out.line(&format!("## Sheet: {}", clean_line(&sh.name, 100)));
        sheet(pkg, &rel.target, &shared, out)?;
    }
    Ok(())
}

fn sheet(pkg: &Package<'_>, part: &str, shared: &Shared, out: &mut Out) -> Res {
    let mut x = match open(pkg, part) {
        Ok(x) => x,
        // A sheet the workbook names and the zip lacks (a chart sheet, a
        // damaged file): no rows.
        Err(PartError::Missing) => return Ok(()),
        Err(e) => return Err(fail_part(pkg, e)),
    };
    let mut row: Vec<String> = Vec::new();
    let (mut rows, mut last_row) = (0usize, 0usize);
    let (mut col, mut ty) = (0usize, String::new());
    let (mut v, mut inline) = (String::new(), String::new());
    let (mut in_v, mut in_is, mut in_t, mut phonetic) = (false, false, false, 0usize);
    let mut row_no = 0usize;
    while !out.full() {
        match x.next().map_err(|f| fail(pkg, f))? {
            Ev::Open(e) => match name(&e) {
                "row" => {
                    row.clear();
                    row_no = attr(&e, "r")
                        .and_then(|r| r.parse::<usize>().ok())
                        .unwrap_or(row_no + 1);
                }
                "c" => {
                    col = attr(&e, "r")
                        .as_deref()
                        .and_then(column)
                        .unwrap_or(row.len());
                    ty = attr(&e, "t").unwrap_or_default();
                    v.clear();
                    inline.clear();
                }
                "v" => in_v = true,
                "is" => in_is = true,
                "rPh" if in_is => phonetic += 1,
                "t" if in_is && phonetic == 0 => in_t = true,
                _ => {}
            },
            Ev::Close(e) => match end_name(&e) {
                "v" => in_v = false,
                "is" => in_is = false,
                "rPh" if phonetic > 0 => phonetic -= 1,
                "t" => in_t = false,
                "c" => {
                    let value = match ty.as_str() {
                        "s" => v
                            .trim()
                            .parse::<usize>()
                            .ok()
                            .and_then(|i| shared.strings.get(i))
                            .cloned()
                            .unwrap_or_default(),
                        "inlineStr" => clean_line(&inline, CELL_CHARS),
                        "b" => (if v.trim() == "1" { "TRUE" } else { "FALSE" }).to_string(),
                        _ => clean_line(&v, CELL_CHARS),
                    };
                    if !value.is_empty() && col < MAX_COLS {
                        if row.len() <= col {
                            row.resize(col + 1, String::new());
                        }
                        row[col] = value;
                    }
                }
                "row" => {
                    while row.last().is_some_and(String::is_empty) {
                        row.pop();
                    }
                    if row.is_empty() {
                        continue;
                    }
                    if rows >= MAX_ROWS {
                        out.line(&format!("[cut: more rows follow after {MAX_ROWS}]"));
                        return Ok(());
                    }
                    // One blank line for a gap of empty rows, so blocks of
                    // a sheet stay apart.
                    if rows > 0 && row_no > last_row + 1 {
                        out.line("");
                    }
                    out.line(&row.join("\t"));
                    rows += 1;
                    last_row = row_no;
                }
                _ => {}
            },
            Ev::Text(t) => {
                if in_v && v.len() < CELL_CHARS * 4 {
                    v.push_str(&t);
                } else if in_t && inline.len() < CELL_CHARS * 4 {
                    inline.push_str(&t);
                }
            }
            Ev::Eof => break,
            _ => {}
        }
    }
    if shared.cut && !out.full() {
        out.line(
            "[cut: this workbook has more shared strings than are read; some cells are empty]",
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// PowerPoint

pub fn pptx(pkg: &Package<'_>, out: &mut Out) -> Res {
    let mut x = open(pkg, "ppt/presentation.xml").map_err(|e| fail_part(pkg, e))?;
    let mut order: Vec<String> = Vec::new();
    loop {
        match x.next().map_err(|f| fail(pkg, f))? {
            Ev::Open(e) if name(&e) == "sldId" && order.len() < 5000 => {
                if let Some(r) = attr_prefixed(&e, "id") {
                    order.push(r);
                }
            }
            Ev::Eof => break,
            _ => {}
        }
    }
    drop(x);
    let rels = relationships(pkg, "ppt/presentation.xml")?;
    for (n, rid) in order.iter().enumerate() {
        if out.full() {
            break;
        }
        let Some(rel) = rels.iter().find(|r| &r.id == rid) else {
            continue;
        };
        out.line(&format!("## Slide {}", n + 1));
        slide_text(pkg, &rel.target, false, out)?;
        let notes = relationships(pkg, &rel.target)?
            .into_iter()
            .find(|r| r.kind == "notesSlide");
        if let Some(nr) = notes {
            let mut tmp = Out::new(LINE_CHARS * 4);
            slide_text(pkg, &nr.target, true, &mut tmp)?;
            if !tmp.s.is_empty() {
                out.line("Notes:");
                for l in tmp.s.lines() {
                    out.line(l);
                }
            }
        }
    }
    Ok(())
}

/// The text of one slide, or of its notes (only the notes' body, not the
/// slide image or the number).
fn slide_text(pkg: &Package<'_>, part: &str, notes: bool, out: &mut Out) -> Res {
    let mut x = match open(pkg, part) {
        Ok(x) => x,
        Err(PartError::Missing) => return Ok(()),
        Err(e) => return Err(fail_part(pkg, e)),
    };
    let mut flow = Flow::default();
    let (mut in_t, mut skip) = (false, 0usize);
    // Whether the shape being read is one whose text counts.
    let mut shape_ok = !notes;
    while !out.full() {
        match x.next().map_err(|f| fail(pkg, f))? {
            Ev::Open(e) => {
                if skip > 0 {
                    skip += 1;
                    continue;
                }
                match name(&e) {
                    "Fallback" => skip = 1,
                    "sp" => shape_ok = !notes,
                    "ph" => {
                        let t = attr(&e, "type").unwrap_or_default();
                        shape_ok = if notes {
                            t == "body"
                        } else {
                            !matches!(t.as_str(), "sldNum" | "ftr" | "dt" | "hdr")
                        };
                    }
                    "p" => flow.begin_para(),
                    "t" => in_t = true,
                    "br" | "tab" => flow.text(" "),
                    "tbl" => flow.begin_table(),
                    "tr" => flow.begin_row(),
                    "tc" => flow.begin_cell(),
                    _ => {}
                }
            }
            Ev::Close(e) => {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                match end_name(&e) {
                    "p" => flow.end_para(out),
                    "t" => in_t = false,
                    "sp" => shape_ok = !notes,
                    "tc" => flow.end_cell(),
                    "tr" => flow.end_row(out),
                    "tbl" => flow.end_table(),
                    _ => {}
                }
            }
            Ev::Text(t) if in_t && skip == 0 && shape_ok => flow.text(&t),
            Ev::Eof => break,
            _ => {}
        }
    }
    Ok(())
}
