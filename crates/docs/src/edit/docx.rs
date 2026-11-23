//! `doc_view` and `doc_edit` of a Word file (PLAN.md T98, report 3.2 and
//! 3.4). `word/document.xml` is the one part that changes.
//!
//! A paragraph is addressed by a tag a view printed: `p12#a3f` is the
//! twelfth paragraph directly in the body and `a3f…` the short hash of its
//! visible text; `t2r3c1#9bd` is the paragraph of cell 1 of row 3 of table 2
//! (`t2r3c1p2#…` when the cell has several). A hash that no longer matches
//! refuses the edit: the document changed since the view.

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use quick_xml::events::Event;

use super::para::{self, Kind, OBJ};
use super::pkg::{Rewrite, Source};
use super::scan::{NS_W, Patches, Scan, attr};
use crate::Result;
use crate::limits::*;
use crate::xml;

const DOC: &str = "word/document.xml";

/// Where a paragraph is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tag {
    /// The nth paragraph directly in the body.
    P(u32),
    /// A paragraph of a cell of a table directly in the body; `p` when the
    /// cell holds several.
    Cell {
        t: u32,
        r: u32,
        c: u32,
        p: Option<u32>,
    },
}

impl Tag {
    fn key(&self) -> String {
        match self {
            Tag::P(n) => format!("p{n}"),
            Tag::Cell { t, r, c, p: None } => format!("t{t}r{r}c{c}"),
            Tag::Cell {
                t,
                r,
                c,
                p: Some(p),
            } => format!("t{t}r{r}c{c}p{p}"),
        }
    }
}

/// A paragraph of the document, as the index holds it.
#[derive(Debug, Clone)]
pub struct Para {
    pub start: usize,
    pub end: usize,
    pub tag: Option<Tag>,
    /// What it is when it has no tag: shown, not editable.
    pub text: String,
    pub hash: u16,
    pub style: Option<String>,
    pub list: bool,
    pub sect: bool,
    /// The last paragraph of its cell, or of the body.
    pub last: bool,
    /// The table it is in (a table directly in the body), for the view.
    pub table: Option<u32>,
    pub hidden: bool,
    pub tracked: bool,
    pub field: bool,
}

pub struct Index {
    pub paras: Vec<Para>,
    pub prefix: String,
}

/// The short hash of a paragraph's text (FNV-1a, 16 bits): enough to know a
/// paragraph changed since a view, not a security property.
pub fn hash(text: &str) -> u16 {
    let mut h: u32 = 0x811c_9dc5;
    for b in text.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    ((h >> 16) ^ h) as u16
}

enum C {
    Doc,
    Body,
    Tbl { top: bool, t: u32, rows: u32 },
    Tr { r: u32, cols: u32 },
    Tc { first: usize, c: u32 },
    Wrap,
}

/// Read the paragraphs of `word/document.xml`.
pub fn index(part: &[u8]) -> Result<Index> {
    let mut s = Scan::new(part, NS_W, MAX_EDIT_EVENTS)?;
    let mut paras: Vec<Para> = vec![];
    let mut stack: Vec<C> = vec![];
    let (mut np, mut nt) = (0u32, 0u32);
    let mut text_total = 0usize;
    let mut saw_doc = false;
    loop {
        let Some(t) = s.next()? else { break };
        let (e, empty) = match &t.ev {
            Event::Start(e) => (e, false),
            Event::Empty(e) => (e, true),
            Event::End(_) => {
                if let Some(C::Tc { first, c }) = stack.pop() {
                    // The tags of the cell's paragraphs (a cell of a table
                    // directly in the body; the others have none).
                    let (tt, rr) = match (stack.iter().rev().nth(1), stack.last()) {
                        (Some(C::Tbl { top: true, t, .. }), Some(C::Tr { r, .. })) => (*t, *r),
                        _ => continue,
                    };
                    let cell: Vec<usize> = (first..paras.len())
                        .filter(|i| paras[*i].tag.is_some())
                        .collect();
                    let n = cell.len();
                    for (k, i) in cell.iter().enumerate() {
                        paras[*i].tag = Some(Tag::Cell {
                            t: tt,
                            r: rr,
                            c,
                            p: if n > 1 { Some(k as u32 + 1) } else { None },
                        });
                        paras[*i].table = Some(tt);
                        paras[*i].last = k + 1 == n;
                    }
                }
                continue;
            }
            _ => continue,
        };
        let local = s.local(e.name());
        if !saw_doc {
            if local != Some("document") {
                return Err("this is not a Word document: its main part is not a document".into());
            }
            saw_doc = true;
            if !empty {
                stack.push(C::Doc);
            }
            continue;
        }
        let parent = stack.last();
        match (parent, local) {
            (Some(C::Doc), Some("body")) if !empty => stack.push(C::Body),
            (Some(C::Body | C::Tc { .. } | C::Wrap), Some("p")) => {
                let end = if empty {
                    t.end
                } else {
                    s.skip_to_end(t.depth)?
                };
                if paras.len() >= MAX_EDIT_PARAS {
                    return Err("the document has too many paragraphs to be edited".into());
                }
                if end - t.start > MAX_EDIT_PARA_BYTES {
                    return Err("a paragraph of the document is too large to be edited".into());
                }
                let ps = para::scan(&part[t.start..end], s.prefix(), NS_W)?;
                let text = ps.text();
                text_total += text.len();
                if text_total > MAX_EDIT_PART_BYTES {
                    return Err("the document's text is too large to be edited".into());
                }
                let tag = match parent {
                    Some(C::Body) => {
                        np += 1;
                        Some(Tag::P(np))
                    }
                    // Filled in when the cell ends.
                    Some(C::Tc { .. })
                        if matches!(stack.iter().rev().nth(2), Some(C::Tbl { top: true, .. })) =>
                    {
                        Some(Tag::Cell {
                            t: 0,
                            r: 0,
                            c: 0,
                            p: None,
                        })
                    }
                    _ => None,
                };
                paras.push(Para {
                    start: t.start,
                    end,
                    tag,
                    hash: hash(&text),
                    text,
                    style: ps.style.clone(),
                    list: ps.numpr,
                    sect: ps.sect,
                    last: false,
                    table: None,
                    hidden: ps.hidden,
                    tracked: ps.tracked,
                    field: ps.field,
                });
            }
            (Some(C::Body | C::Tc { .. }), Some("tbl")) if !empty => {
                let top = matches!(parent, Some(C::Body));
                if top {
                    nt += 1;
                }
                stack.push(C::Tbl {
                    top,
                    t: nt,
                    rows: 0,
                });
            }
            (Some(C::Tbl { .. }), Some("tr")) if !empty => {
                let r = match stack.last_mut() {
                    Some(C::Tbl { rows, .. }) => {
                        *rows += 1;
                        *rows
                    }
                    _ => 0,
                };
                stack.push(C::Tr { r, cols: 0 });
            }
            (Some(C::Tr { .. }), Some("tc")) if !empty => {
                let c = match stack.last_mut() {
                    Some(C::Tr { cols, .. }) => {
                        *cols += 1;
                        *cols
                    }
                    _ => 0,
                };
                stack.push(C::Tc {
                    first: paras.len(),
                    c,
                });
            }
            (
                Some(C::Body | C::Tc { .. } | C::Wrap),
                Some("sdt" | "sdtContent" | "ins" | "customXml" | "smartTag"),
            ) if !empty => stack.push(C::Wrap),
            (_, _) => {
                if !empty {
                    s.skip_to_end(t.depth)?;
                }
            }
        }
    }
    if !saw_doc {
        return Err("this is not a Word document".into());
    }
    // The last paragraph of the body.
    if let Some(l) = paras.iter().rposition(|p| matches!(p.tag, Some(Tag::P(_)))) {
        paras[l].last = true;
    }
    Ok(Index {
        paras,
        prefix: s.prefix().to_string(),
    })
}

fn tag_text(p: &Para) -> Option<String> {
    p.tag
        .as_ref()
        .map(|t| format!("{}#{:04x}", t.key(), p.hash))
}

/// A line of text for a model: control characters and what is not text made
/// visible, cut at `max` characters.
fn shown(text: &str, max: usize) -> String {
    let mut o = String::new();
    let mut n = 0;
    for c in text.chars() {
        if n >= max {
            let more = text.chars().count() - n;
            o.push_str(&format!(" [+{more} characters]"));
            break;
        }
        n += 1;
        match c {
            '\t' => o.push_str("\\t"),
            '\n' => o.push_str("\\n"),
            OBJ => o.push_str("[obj]"),
            c if c.is_control() => o.push(' '),
            c => o.push(c),
        }
    }
    o
}

/// The styles of `word/styles.xml`: id to name, name to id (lower case,
/// paragraph styles only).
#[derive(Default)]
pub struct Styles {
    pub by_id: HashMap<String, String>,
    pub by_name: HashMap<String, String>,
}

pub fn styles(src: &Source<'_>) -> Result<Styles> {
    let mut st = Styles::default();
    if !src.has("word/styles.xml") {
        return Ok(st);
    }
    let part = src.read("word/styles.xml", MAX_EDIT_PART_BYTES)?;
    let mut s = Scan::new(&part, NS_W, MAX_EDIT_EVENTS)?;
    let mut cur: Option<String> = None;
    loop {
        let Some(t) = s.next()? else { break };
        match &t.ev {
            Event::Start(e) if s.local(e.name()) == Some("style") => {
                cur = if attr(e, "type")?.as_deref() == Some("paragraph") {
                    attr(e, "styleId")?
                } else {
                    None
                };
            }
            Event::End(e) if s.local(e.name()) == Some("style") => cur = None,
            Event::Start(e) | Event::Empty(e) if s.local(e.name()) == Some("name") => {
                if let (Some(id), Some(n)) = (&cur, attr(e, "val")?) {
                    let n = n.to_lowercase();
                    st.by_name.entry(n.clone()).or_insert_with(|| id.clone());
                    st.by_id.insert(id.clone(), n);
                }
            }
            _ => {}
        }
    }
    Ok(st)
}

/// What a person sees of the document, with the tags the edit tool takes.
pub fn view(bytes: &[u8], from: usize, limit: usize) -> Result<String> {
    let src = Source::open(bytes)?;
    if !src.has(DOC) {
        return Err("this is not a Word document".into());
    }
    let part = src.read(DOC, MAX_EDIT_PART_BYTES)?;
    let idx = index(&part)?;
    let st = styles(&src)?;
    let limit = limit.clamp(1, MAX_VIEW_ITEMS);
    let tables = idx
        .paras
        .iter()
        .filter_map(|p| match p.tag {
            Some(Tag::Cell { t, .. }) => Some(t),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    let mut out = format!(
        "Word document: {} paragraphs, {} tables. Edit with doc_edit using the tag at the start of a line (`p12#a3f`: paragraph 12 of the body; `t2r3c1#9bd`: table 2, row 3, column 1); the hash after # must match, or the document changed since this view. In the text, \\t is a tab, \\n a line break and [obj] something that is not text (a picture, a note mark, a field); a line without a tag cannot be edited.\n",
        idx.paras.len(),
        tables
    );
    if idx.paras.iter().any(|p| p.hidden) {
        out.push_str("[left out: hidden text, which cannot be matched or edited]\n");
    }
    if idx.paras.iter().any(|p| p.tracked) {
        out.push_str("[tracked changes: deleted text is left out; inserted text is shown]\n");
    }
    let mut shown_n = 0usize;
    let mut last_table = None;
    let mut i = from.saturating_sub(1).min(idx.paras.len());
    let first = i;
    while i < idx.paras.len() {
        let p = &idx.paras[i];
        if shown_n >= limit || out.len() > MAX_VIEW_BYTES {
            break;
        }
        if p.table != last_table {
            if let Some(t) = p.table {
                out.push_str(&format!("-- table {t} --\n"));
            }
            last_table = p.table;
        }
        let mut line = match tag_text(p) {
            Some(t) => t,
            None => "(not editable)".into(),
        };
        if let Some(sid) = &p.style {
            let name = st
                .by_id
                .get(sid)
                .cloned()
                .unwrap_or_else(|| sid.to_lowercase());
            if name != "normal" {
                line.push_str(&format!(" [{name}]"));
            }
        }
        if p.list {
            line.push_str(" [list]");
        }
        if p.table.is_some() {
            line.push_str(" |");
        }
        line.push(' ');
        line.push_str(&shown(&p.text, 2_000));
        out.push_str(&line);
        out.push('\n');
        shown_n += 1;
        i += 1;
    }
    if i < idx.paras.len() {
        out.push_str(&format!(
            "[cut: showed items {}-{} of {}; call doc_view with from={} for the rest]\n",
            first + 1,
            i,
            idx.paras.len(),
            i + 1
        ));
    }
    Ok(out)
}

// -- edits ------------------------------------------------------------------

/// One edit of a Word document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocOp {
    ReplaceText {
        at: String,
        find: String,
        replace: String,
        all: bool,
    },
    SetText {
        at: String,
        text: String,
    },
    InsertAfter {
        at: String,
        markdown: String,
    },
    InsertBefore {
        at: String,
        markdown: String,
    },
    Delete {
        at: String,
    },
}

impl DocOp {
    fn at(&self) -> &str {
        match self {
            Self::ReplaceText { at, .. }
            | Self::SetText { at, .. }
            | Self::InsertAfter { at, .. }
            | Self::InsertBefore { at, .. }
            | Self::Delete { at } => at,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::ReplaceText { .. } => "replace_text",
            Self::SetText { .. } => "set_text",
            Self::InsertAfter { .. } => "insert_after",
            Self::InsertBefore { .. } => "insert_before",
            Self::Delete { .. } => "delete",
        }
    }

    /// An edit as the model wrote it.
    pub fn parse(v: &Value) -> Result<Self> {
        let o = v.as_object().ok_or("an edit is an object")?;
        let s = |k: &str| -> Result<String> {
            let t = o
                .get(k)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("this edit needs `{k}` as text"))?;
            if t.chars().count() > MAX_EDIT_TEXT_CHARS {
                return Err(format!(
                    "`{k}` is longer than {MAX_EDIT_TEXT_CHARS} characters"
                ));
            }
            Ok(t.to_string())
        };
        let op = o
            .get("op")
            .and_then(Value::as_str)
            .ok_or("an edit needs `op`")?;
        let known: &[&str] = match op {
            "replace_text" => &["op", "at", "find", "replace", "all"],
            "set_text" => &["op", "at", "text"],
            "insert_after" | "insert_before" => &["op", "at", "markdown"],
            "delete" => &["op", "at"],
            _ => {
                return Err(format!(
                    "`op` is replace_text, set_text, insert_after, insert_before or delete, not \"{}\"",
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
        let at = s("at")?;
        Ok(match op {
            "replace_text" => Self::ReplaceText {
                at,
                find: s("find")?,
                replace: s("replace")?,
                all: match o.get("all") {
                    None | Some(Value::Null) => false,
                    Some(Value::Bool(b)) => *b,
                    Some(_) => return Err("`all` is true or false".into()),
                },
            },
            "set_text" => Self::SetText {
                at,
                text: s("text")?,
            },
            "insert_after" | "insert_before" => {
                let markdown = o
                    .get("markdown")
                    .and_then(Value::as_str)
                    .ok_or("this edit needs `markdown` as text")?
                    .to_string();
                if markdown.len() > MAX_EDIT_MARKDOWN_BYTES {
                    return Err(format!(
                        "`markdown` is longer than {} KB; insert it in parts",
                        MAX_EDIT_MARKDOWN_BYTES / 1000
                    ));
                }
                if op == "insert_after" {
                    Self::InsertAfter { at, markdown }
                } else {
                    Self::InsertBefore { at, markdown }
                }
            }
            _ => Self::Delete { at },
        })
    }
}

pub use super::Outcome;

fn parse_at(at: &str) -> Option<(&str, u16)> {
    let (k, h) = at.rsplit_once('#')?;
    if h.len() != 4 {
        return None;
    }
    Some((k, u16::from_str_radix(h, 16).ok()?))
}

fn protected(src: &Source<'_>) -> Result<bool> {
    if !src.has("word/settings.xml") {
        return Ok(false);
    }
    let part = src.read("word/settings.xml", MAX_EDIT_PART_BYTES)?;
    let mut s = Scan::new(&part, NS_W, MAX_EDIT_EVENTS)?;
    loop {
        let Some(t) = s.next()? else { return Ok(false) };
        if let Event::Start(e) | Event::Empty(e) = &t.ev
            && s.local(e.name()) == Some("documentProtection")
            && matches!(
                attr(e, "enforcement")?.as_deref(),
                Some("1" | "true" | "on")
            )
        {
            return Ok(true);
        }
    }
}

fn needs_preserve(s: &str) -> bool {
    s.starts_with([' ', '\t', '\n']) || s.ends_with([' ', '\t', '\n']) || s.contains("  ")
}

/// The XML of `w:t`, `w:tab` and `w:br` children for `text`.
fn run_content(p: &str, text: &str) -> String {
    let mut o = String::new();
    let mut cur = String::new();
    let flush = |o: &mut String, cur: &mut String| {
        if !cur.is_empty() {
            o.push_str(&format!(
                "<{p}t xml:space=\"preserve\">{}</{p}t>",
                xml::text(cur)
            ));
            cur.clear();
        }
    };
    for c in text.chars() {
        match c {
            '\t' => {
                flush(&mut o, &mut cur);
                o.push_str(&format!("<{p}tab/>"));
            }
            '\n' => {
                flush(&mut o, &mut cur);
                o.push_str(&format!("<{p}br/>"));
            }
            c => cur.push(c),
        }
    }
    flush(&mut o, &mut cur);
    o
}

fn pfx(prefix: &str) -> String {
    if prefix.is_empty() {
        String::new()
    } else {
        format!("{prefix}:")
    }
}

fn short(s: &str) -> String {
    harness_core::text::clean_line(s, 60)
}

/// Replace `find` in the working copy of a paragraph. `Ok(n)` replacements.
fn replace_in(
    ps: &mut para::ParaScan,
    find: &str,
    replace: &str,
    all: bool,
    tag: &str,
) -> std::result::Result<usize, String> {
    if find.is_empty() {
        return Err("`find` is empty".into());
    }
    let text = ps.text();
    // Character index of every byte offset of a match.
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let matches: Vec<usize> = text.match_indices(find).map(|(i, _)| i).collect();
    // Non-overlapping, left to right (match_indices already is).
    if matches.is_empty() {
        return Err(format!(
            "no text \"{}\" in {tag}; its text is \"{}\"",
            short(find),
            short(&text)
        ));
    }
    if matches.len() > 1 && !all {
        return Err(format!(
            "\"{}\" occurs {} times in {tag}; make `find` longer so it matches once, or set `all`",
            short(find),
            matches.len()
        ));
    }
    // piece and offset of each character
    let mut map: Vec<(usize, usize)> = Vec::with_capacity(chars.len());
    for (pi, p) in ps.pieces.iter().enumerate() {
        match &p.kind {
            Kind::T(i) => {
                for k in 0..ps.tnodes[*i].text.chars().count() {
                    map.push((pi, k));
                }
            }
            Kind::Ch(_) => map.push((pi, 0)),
        }
    }
    let find_chars = find.chars().count();
    let mut spans = vec![];
    for b in &matches {
        let ci = chars.partition_point(|(o, _)| o < b);
        let span = &map[ci..ci + find_chars];
        let region = ps.pieces[span[0].0].region;
        for (pi, _) in span {
            let p = &ps.pieces[*pi];
            if p.locked {
                return Err(format!(
                    "\"{}\" in {tag} touches a field, a tab, a line break or a picture, which cannot be edited in place; use set_text on a paragraph without them, or edit around it",
                    short(find)
                ));
            }
            if p.region != region {
                return Err(format!(
                    "\"{}\" in {tag} runs across a link, a tracked insertion or a content control; edit the text on one side of it",
                    short(find)
                ));
            }
        }
        spans.push(span.to_vec());
    }
    // From the last match to the first, so earlier offsets stay right.
    for span in spans.iter().rev() {
        let mut first = true;
        // Cut the matched characters out of every node, from the last
        // character to the first, then put the replacement where the
        // first one was.
        let mut by_node: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (pi, off) in span {
            if let Kind::T(i) = ps.pieces[*pi].kind {
                by_node.entry(i).or_default().push(*off);
            }
        }
        for (i, offs) in &by_node {
            let n = &mut ps.tnodes[*i];
            let mut cs: Vec<char> = n.text.chars().collect();
            let lo = offs[0];
            let hi = offs[offs.len() - 1];
            cs.drain(lo..=hi);
            if first {
                let at = lo.min(cs.len());
                for (k, c) in replace.chars().enumerate() {
                    cs.insert(at + k, c);
                }
                first = false;
            }
            n.text = cs.into_iter().collect();
            n.dirty = true;
        }
    }
    Ok(spans.len())
}

/// One paragraph made by an insert, as XML and as the text it must read back as.
struct NewPara {
    xml: String,
    text: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Block {
    Para,
    Item,
    Head(u8),
}

/// The paragraphs of a Markdown fragment, formatted from the anchor: plain
/// paragraphs and headings (a style of THIS document, found by name), list
/// items that continue the anchor's list, bold, italic, strike and code.
/// Links, images, tables, quotes, code blocks, raw HTML and rules need parts
/// or properties an edit does not add, and are refused.
fn markdown_paras(
    md: &str,
    p: &str,
    anchor: &para::ParaScan,
    anchor_src: &[u8],
    st: &Styles,
) -> std::result::Result<Vec<NewPara>, String> {
    use pulldown_cmark::{Event as E, HeadingLevel, Options, Parser, Tag as T, TagEnd};
    let markers = md
        .bytes()
        .filter(|b| {
            matches!(
                b,
                b'[' | b']' | b'*' | b'_' | b'~' | b'`' | b'<' | b'>' | b'!'
            )
        })
        .count();
    if markers > MAX_INLINE_MARKERS {
        return Err("the Markdown has too many formatting marks".into());
    }
    let heading_like = |id: &str| {
        let n = st
            .by_id
            .get(id)
            .cloned()
            .unwrap_or_else(|| id.to_lowercase());
        n.starts_with("heading") || n == "title" || n == "subtitle"
    };
    // pPr children copied from the anchor, in their order.
    let copy = |names: &[&str]| -> String {
        let mut s = String::new();
        for (n, a, b) in &anchor.ppr {
            if names.contains(&n.as_str()) {
                s.push_str(&String::from_utf8_lossy(&anchor_src[*a..*b]));
            }
        }
        s
    };
    let anchor_head = anchor.style.as_deref().is_some_and(heading_like);
    let mut out: Vec<NewPara> = vec![];
    let mut block: Option<Block> = None;
    let mut runs = String::new();
    let mut text = String::new();
    let (mut b, mut i, mut st_k) = (0u32, 0u32, 0u32);
    let mut in_list = false;
    let mut last_list_item = false;
    let parser = Parser::new_ext(md, Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES);
    let rpr = |code: bool, b: u32, i: u32, k: u32| -> String {
        if !code && b == 0 && i == 0 && k == 0 {
            return String::new();
        }
        let mut s = format!("<{p}rPr>");
        if code {
            s.push_str(&format!(
                "<{p}rFonts {p}ascii=\"Consolas\" {p}hAnsi=\"Consolas\" {p}cs=\"Consolas\"/>"
            ));
        }
        if b > 0 {
            s.push_str(&format!("<{p}b/>"));
        }
        if i > 0 {
            s.push_str(&format!("<{p}i/>"));
        }
        if k > 0 {
            s.push_str(&format!("<{p}strike/>"));
        }
        s.push_str(&format!("</{p}rPr>"));
        s
    };
    for ev in parser {
        match ev {
            E::Start(T::Paragraph) => {
                // A paragraph inside a list item is the item itself.
                if !in_list {
                    block = Some(Block::Para);
                    runs.clear();
                    text.clear();
                }
            }
            E::Start(T::Heading { level, .. }) => {
                let n = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    _ => 4,
                };
                block = Some(Block::Head(n));
                runs.clear();
                text.clear();
            }
            E::Start(T::List(_)) => {
                if in_list {
                    return Err("a nested list needs levels this document's list may not have; insert the items one level at a time".into());
                }
                in_list = true;
            }
            E::Start(T::Item) => {
                block = Some(Block::Item);
                runs.clear();
                text.clear();
                last_list_item = true;
            }
            E::End(TagEnd::List(_)) => in_list = false,
            E::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item) => {
                if matches!(ev, E::End(TagEnd::Paragraph)) && in_list {
                    continue;
                }
                let Some(blk) = block.take() else { continue };
                let ppr = match blk {
                    Block::Head(n) => {
                        let want = format!("heading {n}");
                        let Some(id) = st.by_name.get(&want) else {
                            return Err(format!(
                                "this document has no style named \"{want}\"; use a plain paragraph"
                            ));
                        };
                        format!("<{p}pPr><{p}pStyle {p}val=\"{}\"/></{p}pPr>", xml::attr(id))
                    }
                    Block::Item => {
                        if !anchor.numpr {
                            return Err("the paragraph this goes next to is not a list item; a list needs numbering the file lacks. Insert plain paragraphs, or insert next to a list item".into());
                        }
                        format!(
                            "<{p}pPr>{}</{p}pPr>",
                            copy(&["pStyle", "numPr", "spacing", "ind", "contextualSpacing"])
                        )
                    }
                    Block::Para => {
                        let names: &[&str] = if anchor_head || anchor.numpr {
                            &["spacing", "ind", "jc"]
                        } else {
                            &["pStyle", "spacing", "ind", "contextualSpacing", "jc"]
                        };
                        let c = copy(names);
                        if c.is_empty() {
                            String::new()
                        } else {
                            format!("<{p}pPr>{c}</{p}pPr>")
                        }
                    }
                };
                if out.len() >= MAX_EDIT_PARAS_PER_INSERT {
                    return Err("too many paragraphs in one insert".into());
                }
                out.push(NewPara {
                    xml: format!("<{p}p>{ppr}{runs}</{p}p>"),
                    text: std::mem::take(&mut text),
                });
                runs.clear();
            }
            E::Start(T::Strong) => b += 1,
            E::End(TagEnd::Strong) => b = b.saturating_sub(1),
            E::Start(T::Emphasis) => i += 1,
            E::End(TagEnd::Emphasis) => i = i.saturating_sub(1),
            E::Start(T::Strikethrough) => st_k += 1,
            E::End(TagEnd::Strikethrough) => st_k = st_k.saturating_sub(1),
            E::Text(ref t) | E::Code(ref t) => {
                let code = matches!(ev, E::Code(_));
                let t = xml::clean(t);
                if block.is_none() {
                    return Err("text outside a paragraph".into());
                }
                if !t.is_empty() {
                    runs.push_str(&format!(
                        "<{p}r>{}<{p}t xml:space=\"preserve\">{}</{p}t></{p}r>",
                        rpr(code, b, i, st_k),
                        xml::text(&t)
                    ));
                    text.push_str(&t);
                }
            }
            E::SoftBreak => {
                runs.push_str(&format!(
                    "<{p}r><{p}t xml:space=\"preserve\"> </{p}t></{p}r>"
                ));
                text.push(' ');
            }
            E::HardBreak => {
                runs.push_str(&format!("<{p}r><{p}br/></{p}r>"));
                text.push('\n');
            }
            E::Start(T::Link { .. }) => {
                return Err("links need a relationship the file does not have; insert the text without the link".into());
            }
            E::Start(T::Image { .. }) => {
                return Err("images cannot be inserted".into());
            }
            E::Start(T::Table(_)) => return Err("tables cannot be inserted".into()),
            E::Start(T::BlockQuote(_)) => {
                return Err("block quotes cannot be inserted; use plain paragraphs".into());
            }
            E::Start(T::CodeBlock(_)) => {
                return Err(
                    "code blocks cannot be inserted; use `inline code` in a paragraph".into(),
                );
            }
            E::Rule => return Err("a rule cannot be inserted".into()),
            E::Html(_) | E::InlineHtml(_) | E::Start(T::HtmlBlock) => {
                return Err("raw HTML cannot be inserted".into());
            }
            E::Start(_) | E::End(_) => {
                return Err("this Markdown has something an insert cannot hold".into());
            }
            _ => return Err("this Markdown has something an insert cannot hold".into()),
        }
    }
    let _ = last_list_item;
    if out.is_empty() {
        return Err("the Markdown has no text".into());
    }
    Ok(out)
}

/// Paragraphs one insert may make.
pub const MAX_EDIT_PARAS_PER_INSERT: usize = 500;

fn write_t_patches(
    ps: &para::ParaScan,
    base: usize,
    prefix_colon: &str,
    patches: &mut Patches,
    src: &[u8],
) {
    for n in ps.tnodes.iter().filter(|n| n.dirty) {
        let esc = xml::text(&n.text);
        let preserve = needs_preserve(&n.text) && !n.preserve;
        if n.empty {
            // `<w:t/>` that gains text becomes a full element.
            let tag = String::from_utf8_lossy(&src[base + n.start..base + n.end]).into_owned();
            let name_end = tag
                .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
                .unwrap_or(tag.len());
            let name = &tag[1..name_end];
            let attrs = tag[name_end..].trim_end_matches("/>").trim_end();
            let ws = if preserve && !attrs.contains("space") {
                " xml:space=\"preserve\""
            } else {
                ""
            };
            patches.replace(
                base + n.start,
                base + n.end,
                format!("<{name}{attrs}{ws}>{esc}</{name}>").into_bytes(),
            );
        } else {
            if preserve {
                // Before the `>` of the start tag.
                patches.replace(
                    base + n.open_end - 1,
                    base + n.open_end - 1,
                    b" xml:space=\"preserve\"".to_vec(),
                );
            }
            patches.replace(
                base + n.open_end,
                base + n.end - closing_len(src, base + n.end),
                esc.into_bytes(),
            );
        }
    }
    let _ = prefix_colon;
}

/// The length of the end tag that ends at `end`.
fn closing_len(src: &[u8], end: usize) -> usize {
    let mut i = end;
    while i > 0 && src[i - 1] != b'<' {
        i -= 1;
    }
    end - (i - 1)
}

/// Edit a Word document.
pub fn edit(bytes: &[u8], ops: &[DocOp]) -> Result<Outcome> {
    if ops.is_empty() {
        return Err("give at least one edit".into());
    }
    if ops.len() > MAX_EDITS {
        return Err(format!("at most {MAX_EDITS} edits in one call"));
    }
    let src = Source::open(bytes)?;
    if !src.has(DOC) {
        return Err("this is not a Word document".into());
    }
    if protected(&src)? {
        return Err("this document is protected against editing (document protection is on); it is not edited".into());
    }
    let part = src.read(DOC, MAX_EDIT_PART_BYTES)?;
    let idx = index(&part)?;
    let st = styles(&src)?;
    let p = pfx(&idx.prefix);
    let by_key: HashMap<String, usize> = idx
        .paras
        .iter()
        .enumerate()
        .filter_map(|(i, x)| x.tag.as_ref().map(|t| (t.key(), i)))
        .collect();
    let mut errs: Vec<Option<String>> = vec![None; ops.len()];
    let mut target: Vec<usize> = vec![usize::MAX; ops.len()];
    for (k, op) in ops.iter().enumerate() {
        let Some((key, h)) = parse_at(op.at()) else {
            errs[k] = Some(format!(
                "`at` must be a tag from doc_view like p12#a3f, not \"{}\"",
                short(op.at())
            ));
            continue;
        };
        match by_key.get(key) {
            None => {
                errs[k] = Some(format!(
                    "there is no paragraph {key} (or it is not editable)"
                ))
            }
            Some(&i) if idx.paras[i].hash != h => {
                errs[k] = Some(format!(
                    "{key} changed since the view (its hash is now {:04x}, not {h:04x}); view the document again",
                    idx.paras[i].hash
                ));
            }
            Some(&i) => target[k] = i,
        }
    }
    // Who touches which paragraph.
    let mut touched: HashMap<usize, Vec<usize>> = HashMap::new();
    for (k, op) in ops.iter().enumerate() {
        if errs[k].is_some() {
            continue;
        }
        if !matches!(op, DocOp::InsertAfter { .. } | DocOp::InsertBefore { .. }) {
            touched.entry(target[k]).or_default().push(k);
        }
    }
    let mut patches = Patches::new();
    let mut done: Vec<Option<String>> = vec![None; ops.len()];
    // Expected text of every paragraph after the edit, and what was added.
    let mut new_text: HashMap<usize, String> = HashMap::new();
    let mut deleted: Vec<usize> = vec![];
    let mut inserts: Vec<(usize, usize, usize, Vec<String>)> = vec![];
    for (&pi, list) in &touched {
        let para = &idx.paras[pi];
        let tag = tag_text(para).unwrap_or_default();
        let key = tag.split('#').next().unwrap_or("").to_string();
        let whole = list
            .iter()
            .any(|k| matches!(ops[*k], DocOp::SetText { .. } | DocOp::Delete { .. }));
        if whole && list.len() > 1 {
            for k in list.iter().skip(1) {
                errs[*k] = Some(format!(
                    "{key} has another edit in this call; set_text and delete take a paragraph alone, so give one edit for it"
                ));
            }
        }
        let slice = &part[para.start..para.end];
        let mut ps = match para::scan(slice, &idx.prefix, NS_W) {
            Ok(x) => x,
            Err(e) => {
                for k in list {
                    errs[*k] = Some(e.clone());
                }
                continue;
            }
        };
        let mut set_done = false;
        for k in list {
            if errs[*k].is_some() {
                continue;
            }
            match &ops[*k] {
                DocOp::ReplaceText {
                    find, replace, all, ..
                } => {
                    let replace = xml::clean(replace);
                    if replace.contains(['\t', '\n', '\r']) {
                        errs[*k] = Some(
                            "`replace` cannot hold a tab or a line break; use set_text for that"
                                .into(),
                        );
                        continue;
                    }
                    match replace_in(&mut ps, &xml::clean(find), &replace, *all, &key) {
                        Ok(n) => {
                            done[*k] = Some(format!(
                                "{key}: replaced {n} occurrence{}",
                                if n == 1 { "" } else { "s" }
                            ))
                        }
                        Err(e) => errs[*k] = Some(e),
                    }
                }
                DocOp::SetText { text, .. } => {
                    let t = xml::clean(text).replace("\r\n", "\n").replace('\r', "\n");
                    if !ps.plain() {
                        errs[*k] = Some(format!(
                            "{key} holds {} that set_text would lose; use replace_text on the words, or insert a new paragraph and delete this one",
                            what_blocks(&ps)
                        ));
                        continue;
                    }
                    new_text.insert(pi, t.clone());
                    set_text_patches(&ps, &part, para.start, &p, &t, &mut patches);
                    set_done = true;
                    done[*k] = Some(format!("{key}: text set"));
                }
                DocOp::Delete { .. } => {
                    if para.sect {
                        errs[*k] = Some(format!(
                            "{key} carries the section's page setup (sectPr) and cannot be deleted"
                        ));
                    } else if para.last {
                        errs[*k] = Some(format!(
                            "{key} is the last paragraph of its body or cell, which must keep one; clear its text with set_text instead"
                        ));
                    } else if !ps.removable() {
                        errs[*k] = Some(format!(
                            "{key} holds {} that other parts may refer to; it is not deleted",
                            what_blocks_delete(&ps)
                        ));
                    } else {
                        patches.replace(para.start, para.end, vec![]);
                        deleted.push(pi);
                        done[*k] = Some(format!("{key}: deleted"));
                    }
                }
                _ => {}
            }
        }
        if !set_done && ps.tnodes.iter().any(|n| n.dirty) && list.iter().all(|k| errs[*k].is_none())
        {
            new_text.insert(pi, ps.text());
            write_t_patches(&ps, para.start, "", &mut patches, &part);
        } else if !set_done && ps.tnodes.iter().any(|n| n.dirty) {
            // Some edit of this paragraph failed: the call applies nothing,
            // so what the others would have done is only described.
        }
    }
    // Inserts.
    for (k, op) in ops.iter().enumerate() {
        if errs[k].is_some() {
            continue;
        }
        let (md, after) = match op {
            DocOp::InsertAfter { markdown, .. } => (markdown, true),
            DocOp::InsertBefore { markdown, .. } => (markdown, false),
            _ => continue,
        };
        let pi = target[k];
        let para = &idx.paras[pi];
        let key = tag_text(para).unwrap_or_default();
        let key = key.split('#').next().unwrap_or("").to_string();
        let slice = &part[para.start..para.end];
        let ps = match para::scan(slice, &idx.prefix, NS_W) {
            Ok(x) => x,
            Err(e) => {
                errs[k] = Some(e);
                continue;
            }
        };
        match markdown_paras(md, &p, &ps, slice, &st) {
            Err(e) => errs[k] = Some(format!("{key}: {e}")),
            Ok(np) => {
                let at = if after { para.end } else { para.start };
                let xml: String = np.iter().map(|n| n.xml.as_str()).collect();
                patches.replace(at, at, xml.into_bytes());
                inserts.push((at, k, pi, np.iter().map(|n| n.text.clone()).collect()));
                done[k] = Some(format!(
                    "{key}: inserted {} paragraph{} {}",
                    np.len(),
                    if np.len() == 1 { "" } else { "s" },
                    if after { "after it" } else { "before it" }
                ));
            }
        }
    }
    let results: Vec<std::result::Result<String, String>> = (0..ops.len())
        .map(|k| match (&errs[k], &done[k]) {
            (Some(e), _) => Err(format!("edit {} ({}): {e}", k + 1, ops[k].name())),
            (None, Some(d)) => Ok(format!("edit {} ({}): {d}", k + 1, ops[k].name())),
            (None, None) => Err(format!("edit {} ({}): not applied", k + 1, ops[k].name())),
        })
        .collect();
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
    let out_part = patches.apply(&part, MAX_EDIT_OUT_PART_BYTES)?;
    // Read it back.
    let again = index(&out_part).map_err(|e| {
        format!("the edited document did not pass its own check ({e}); nothing was written")
    })?;
    wellformed(&out_part)?;
    let mut expect: Vec<(usize, u8, usize, String)> = vec![];
    for (i, para) in idx.paras.iter().enumerate() {
        if deleted.contains(&i) {
            continue;
        }
        expect.push((
            para.start,
            1,
            0,
            new_text
                .get(&i)
                .cloned()
                .unwrap_or_else(|| para.text.clone()),
        ));
    }
    for (at, k, _, texts) in &inserts {
        for (j, t) in texts.iter().enumerate() {
            expect.push((*at, 0, *k * 1000 + j, t.clone()));
        }
    }
    expect.sort_by_key(|(a, r, o, _)| (*a, *r, *o));
    let got: Vec<&str> = again.paras.iter().map(|x| x.text.as_str()).collect();
    let want: Vec<&str> = expect.iter().map(|x| x.3.as_str()).collect();
    if got != want {
        return Err("the edited document did not read back as the edits say (a check of its own); nothing was written".into());
    }
    let mut rw = Rewrite::default();
    rw.replaced.insert(DOC.into(), out_part);
    let out = src.rewrite(&rw)?;
    super::check::package(bytes, &out)?;
    Ok(Outcome {
        results,
        bytes: Some(out),
        notes: vec![],
    })
}

fn what_blocks(ps: &para::ParaScan) -> &'static str {
    if ps.sect {
        "page setup"
    } else if ps.field {
        "a field"
    } else if ps.barrier {
        "a picture, note mark or page break"
    } else if ps.hidden {
        "hidden text"
    } else if ps.tracked {
        "tracked changes"
    } else {
        "a link or a content control"
    }
}

fn what_blocks_delete(ps: &para::ParaScan) -> &'static str {
    if ps.bookmark {
        "a bookmark"
    } else if ps.comment {
        "a comment"
    } else {
        what_blocks(ps)
    }
}

/// `set_text`: the paragraph's text becomes `text` in its first run, whose
/// properties it keeps; the other runs go; properties, bookmarks stay.
fn set_text_patches(
    ps: &para::ParaScan,
    part: &[u8],
    base: usize,
    p: &str,
    text: &str,
    patches: &mut Patches,
) {
    let content = run_content(p, text);
    let first = ps.runs.iter().position(|r| r.has_text);
    match first {
        Some(fi) => {
            let r = &ps.runs[fi];
            let rpr = r
                .rpr
                .map(|(a, b)| String::from_utf8_lossy(&part[base + a..base + b]).into_owned())
                .unwrap_or_default();
            let start_tag =
                String::from_utf8_lossy(&part[base + r.start..base + r.open_end]).into_owned();
            let end_tag =
                String::from_utf8_lossy(&part[base + r.close_start..base + r.end]).into_owned();
            patches.replace(
                base + r.start,
                base + r.end,
                format!("{start_tag}{rpr}{content}{end_tag}").into_bytes(),
            );
            for (i, r) in ps.runs.iter().enumerate() {
                // Every other run of the paragraph (they hold text, tabs or
                // breaks, or nothing at all).
                if i != fi {
                    patches.replace(base + r.start, base + r.end, vec![]);
                }
            }
        }
        None => {
            // No run with text: a new one, after the properties.
            let run = format!("<{p}r>{content}</{p}r>");
            if ps.self_closing {
                let tag = String::from_utf8_lossy(&part[base..base + ps.open_end]).into_owned();
                let name_end = tag
                    .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
                    .unwrap_or(tag.len());
                let name = tag[1..name_end].to_string();
                let attrs = tag[name_end..]
                    .trim_end_matches("/>")
                    .trim_end()
                    .to_string();
                patches.replace(
                    base,
                    base + ps.open_end,
                    format!("<{name}{attrs}>{run}</{name}>").into_bytes(),
                );
            } else {
                patches.replace(
                    base + ps.close_start,
                    base + ps.close_start,
                    run.into_bytes(),
                );
            }
        }
    }
}

/// Every element and attribute of the part parses.
pub fn wellformed(part: &[u8]) -> Result<()> {
    wellformed_ns(part, NS_W)
}

pub fn wellformed_ns(part: &[u8], ns: &'static str) -> Result<()> {
    let mut s = Scan::new(part, ns, MAX_EDIT_EVENTS)?;
    loop {
        let Some(t) = s.next()? else { return Ok(()) };
        if let Event::Start(e) | Event::Empty(e) = &t.ev {
            for a in e.attributes() {
                a.map_err(|_| "the edited part has a damaged attribute".to_string())?;
            }
        }
    }
}
