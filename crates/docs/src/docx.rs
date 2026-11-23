//! Markdown to `.docx` (PLAN.md T97, report 4.1): the parts of a Word file
//! are string templates, and only `word/document.xml` varies. The walk over
//! `pulldown-cmark`'s events is iterative (a million nested `>` is a counter,
//! not a recursion), every text goes through [`crate::xml`], and a property
//! block is emitted from a fixed, ordered template (Word refuses children out
//! of schema order). Raw HTML is dropped, an image is not embedded (its
//! alternative text stays), a link is kept only to http, https or mailto, and
//! the result says what was dropped.
//!
//! Parts: `[Content_Types].xml`, `_rels/.rels`, `word/document.xml`,
//! `word/styles.xml` (a fixed style set), `word/numbering.xml` (bullets, and
//! one numbering per ordered list so each restarts), `word/_rels/
//! document.xml.rels` (the hyperlinks) and `docProps/core.xml`.

use pulldown_cmark::{Alignment, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::Result;
use crate::limits::*;
use crate::xml::{self, ContentTypes, DECL, REL_BASE, REL_CORE};
use crate::zip::ZipWriter;

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// A page size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    A4,
    Letter,
}

/// What the model chose besides the text.
#[derive(Debug, Clone, Default)]
pub struct DocOptions {
    pub page: Page,
    pub landscape: bool,
    /// A property of the file, not a heading in it.
    pub title: Option<String>,
    /// The creation time written into the file's properties (Unix seconds).
    pub created: i64,
}

/// What came out, and what was left out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub paragraphs: usize,
    pub tables: usize,
    pub lists: usize,
    pub links: usize,
    /// Raw HTML fragments left out.
    pub html_dropped: usize,
    /// Images not embedded (their alternative text is kept).
    pub images_dropped: usize,
    /// Links to an address that is not http, https or mailto: text only.
    pub links_dropped: usize,
}

/// The finished file and its report.
#[derive(Debug)]
pub struct Built {
    pub bytes: Vec<u8>,
    pub report: Report,
}

/// Page size and content width in twentieths of a point.
fn page_dims(o: &DocOptions) -> (u32, u32, u32) {
    let (w, h) = match o.page {
        Page::A4 => (11_906, 16_838),
        Page::Letter => (12_240, 15_840),
    };
    let (w, h) = if o.landscape { (h, w) } else { (w, h) };
    (w, h, w - 2 * 1440)
}

#[derive(Clone, Copy, Default)]
struct Fmt {
    bold: u32,
    italic: u32,
    strike: u32,
    code: bool,
    link: bool,
}

struct ListCtx {
    num_id: u32,
}

struct Par {
    style: Option<&'static str>,
    num: Option<(u32, usize)>,
    ind: Option<u32>,
    jc: Option<&'static str>,
    runs: String,
    from_item: bool,
}

struct Cell {
    runs: String,
    header: bool,
    align: Option<&'static str>,
    chars: usize,
}

struct Table {
    aligns: Vec<Alignment>,
    rows: Vec<Vec<Cell>>,
    in_head: bool,
}

struct Gen {
    /// The document part, written in place: its start, then every block.
    body: String,
    /// Bytes held: the body, the open paragraph's runs and each table
    /// cell's, and a count of what each block adds around them.
    used: usize,
    /// Link targets: relationship `rId{3 + index}` (1 and 2 are the styles
    /// and the numbering).
    rels: Vec<String>,
    content_width: u32,
    in_html: bool,
    lists: Vec<ListCtx>,
    /// The start number of each ordered list, by order (numId 2 + index).
    ordered: Vec<u64>,
    par: Option<Par>,
    item_pending: bool,
    item_emitted: Vec<bool>,
    fmt: Fmt,
    quote: u32,
    table: Option<Table>,
    code: Option<String>,
    in_image: u32,
    link_open: bool,
    report: Report,
    cells: usize,
    err: Option<String>,
}

fn run_props(f: Fmt) -> String {
    let mut p = String::new();
    if f.link {
        p.push_str("<w:rStyle w:val=\"Hyperlink\"/>");
    } else if f.code {
        p.push_str("<w:rStyle w:val=\"InlineCode\"/>");
    }
    if f.bold > 0 {
        p.push_str("<w:b/>");
    }
    if f.italic > 0 {
        p.push_str("<w:i/>");
    }
    if f.strike > 0 {
        p.push_str("<w:strike/>");
    }
    if p.is_empty() {
        p
    } else {
        format!("<w:rPr>{p}</w:rPr>")
    }
}

/// Runs for `s`: tabs and line breaks become their own elements, every
/// other character is text. `None` when they would pass `max` bytes: it
/// stops there, so the amount built is bounded by `max` and not by what the
/// text multiplies into.
fn runs(s: &str, f: Fmt, max: usize) -> Option<String> {
    let props = run_props(f);
    let mut out = String::new();
    let mut buf = String::new();
    let flush = |buf: &mut String, out: &mut String| {
        if !buf.is_empty() {
            out.push_str("<w:r>");
            out.push_str(&props);
            out.push_str("<w:t xml:space=\"preserve\">");
            out.push_str(&xml::text(buf));
            out.push_str("</w:t></w:r>");
            buf.clear();
        }
    };
    for c in xml::clean(s).chars() {
        match c {
            '\t' | '\n' => {
                flush(&mut buf, &mut out);
                out.push_str("<w:r>");
                out.push_str(&props);
                out.push_str(if c == '\t' {
                    "<w:tab/></w:r>"
                } else {
                    "<w:br/></w:r>"
                });
            }
            '\r' => flush(&mut buf, &mut out),
            c => buf.push(c),
        }
        if out.len() + buf.len() > max {
            return None;
        }
    }
    flush(&mut buf, &mut out);
    (out.len() <= max).then_some(out)
}

fn ppr(p: &Par) -> String {
    let mut s = String::new();
    if let Some(st) = p.style {
        s.push_str(&format!("<w:pStyle w:val=\"{st}\"/>"));
    }
    if let Some((id, lvl)) = p.num {
        s.push_str(&format!(
            "<w:numPr><w:ilvl w:val=\"{lvl}\"/><w:numId w:val=\"{id}\"/></w:numPr>"
        ));
    }
    if let Some(i) = p.ind {
        s.push_str(&format!("<w:ind w:left=\"{i}\"/>"));
    }
    if let Some(j) = p.jc {
        s.push_str(&format!("<w:jc w:val=\"{j}\"/>"));
    }
    if s.is_empty() {
        s
    } else {
        format!("<w:pPr>{s}</w:pPr>")
    }
}

impl Gen {
    fn new(content_width: u32, start: String) -> Self {
        Self {
            used: start.len(),
            body: start,
            rels: vec![],
            content_width,
            in_html: false,
            lists: vec![],
            ordered: vec![],
            par: None,
            item_pending: false,
            item_emitted: vec![],
            fmt: Fmt::default(),
            quote: 0,
            table: None,
            code: None,
            in_image: 0,
            link_open: false,
            report: Report::default(),
            cells: 0,
            err: None,
        }
    }

    /// Count `n` more bytes held; false (and the build fails) past the cap.
    fn spend(&mut self, n: usize) -> bool {
        self.used = self.used.saturating_add(n);
        if self.used > MAX_DOC_PART_BYTES {
            self.fail(&format!(
                "the document would be larger than {} MiB as a Word file; make it shorter or split it",
                MAX_DOC_PART_BYTES >> 20
            ));
            return false;
        }
        true
    }

    fn room(&self) -> usize {
        MAX_DOC_PART_BYTES.saturating_sub(self.used)
    }

    fn fail(&mut self, e: &str) {
        if self.err.is_none() {
            self.err = Some(e.into());
        }
    }

    fn depth(&self) -> usize {
        self.lists.len().saturating_sub(1).min(2)
    }

    /// The numbering of a paragraph that is an item's own, where it is.
    fn item_num(&self) -> Option<(u32, usize)> {
        self.lists.last().map(|l| (l.num_id, self.depth()))
    }

    /// The paragraph style and indent a plain paragraph gets where it is.
    fn open(&mut self, style: Option<&'static str>) {
        self.close();
        let in_item = !self.lists.is_empty();
        let (style, ind) = match style {
            Some(s) => (Some(s), None),
            None if self.quote > 0 && !in_item => (Some("Quote"), Some(720 * self.quote.min(8))),
            None if in_item => (Some("ListParagraph"), None),
            None => (None, None),
        };
        self.par = Some(Par {
            style,
            num: None,
            ind,
            jc: None,
            runs: String::new(),
            from_item: false,
        });
    }

    /// Where inline text goes: the open paragraph, or one opened for it
    /// (tight list items and quotes have no paragraph tag of their own).
    fn ensure(&mut self) {
        if self.table.is_some() || self.par.is_some() {
            return;
        }
        self.open(None);
        if self.item_pending {
            self.item_pending = false;
            let num = self.item_num();
            if let (Some(num), Some(p)) = (num, self.par.as_mut()) {
                p.num = Some(num);
                p.from_item = true;
            }
            if let Some(e) = self.item_emitted.last_mut() {
                *e = true;
            }
        } else if !self.lists.is_empty() {
            // A further paragraph of an item: indented under its text.
            let ind = 720 * (self.depth() as u32 + 1);
            if let Some(p) = self.par.as_mut() {
                p.ind = Some(ind);
            }
        }
    }

    fn push_inline(&mut self, xml_runs: String) {
        if !self.spend(xml_runs.len()) {
            return;
        }
        if let Some(t) = self.table.as_mut() {
            if let Some(c) = t.rows.last_mut().and_then(|r| r.last_mut()) {
                c.runs.push_str(&xml_runs);
            }
            return;
        }
        self.ensure();
        if let Some(p) = self.par.as_mut() {
            p.runs.push_str(&xml_runs);
        }
    }

    fn text(&mut self, s: &str) {
        let f = self.fmt;
        if let Some(t) = self.table.as_mut()
            && let Some(c) = t.rows.last_mut().and_then(|r| r.last_mut())
        {
            c.chars += s.chars().count();
        }
        // `push_inline` counts what is added; the room is what is left.
        match runs(s, f, self.room()) {
            Some(r) => self.push_inline(r),
            None => {
                self.spend(usize::MAX / 2);
            }
        }
    }

    /// Finish the open paragraph.
    fn close(&mut self) {
        let Some(p) = self.par.take() else { return };
        if p.from_item {
            // Written even when empty: an item is a bullet.
        } else if p.runs.is_empty() && p.num.is_none() {
            return;
        }
        self.emit_par(&p);
    }

    fn emit_par(&mut self, p: &Par) {
        if self.report.paragraphs >= MAX_PARAGRAPHS {
            self.fail(&format!(
                "the document has more than {MAX_PARAGRAPHS} paragraphs; split it into several files"
            ));
            return;
        }
        let head = ppr(p);
        // The runs were counted when they were made; this is what wraps them.
        if !self.spend(head.len() + 16) {
            return;
        }
        self.report.paragraphs += 1;
        self.body.push_str("<w:p>");
        self.body.push_str(&head);
        self.body.push_str(&p.runs);
        self.body.push_str("</w:p>");
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {
                self.close();
                self.ensure();
            }
            Tag::Heading { level, .. } => {
                self.close();
                let style = match level {
                    HeadingLevel::H1 => "Heading1",
                    HeadingLevel::H2 => "Heading2",
                    HeadingLevel::H3 => "Heading3",
                    _ => "Heading4",
                };
                self.open(Some(style));
            }
            Tag::BlockQuote(_) => {
                self.close();
                self.quote += 1;
            }
            Tag::CodeBlock(_) => {
                self.close();
                self.code = Some(String::new());
            }
            Tag::List(start) => {
                self.close();
                let ordered = start.is_some();
                let num_id = if ordered {
                    if self.ordered.len() >= MAX_LISTS {
                        self.fail(&format!(
                            "the document has more than {MAX_LISTS} numbered lists"
                        ));
                    }
                    self.ordered.push(start.unwrap_or(1));
                    self.report.lists += 1;
                    1 + self.ordered.len() as u32
                } else {
                    1
                };
                self.lists.push(ListCtx { num_id });
            }
            Tag::Item => {
                self.close();
                self.item_pending = true;
                self.item_emitted.push(false);
            }
            Tag::Table(aligns) => {
                self.close();
                if self.report.tables >= MAX_TABLES {
                    self.fail(&format!("the document has more than {MAX_TABLES} tables"));
                }
                if aligns.len() > MAX_TABLE_COLS {
                    self.fail(&format!("a table has more than {MAX_TABLE_COLS} columns"));
                }
                self.table = Some(Table {
                    aligns,
                    rows: vec![],
                    in_head: false,
                });
            }
            Tag::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    t.in_head = true;
                    t.rows.push(vec![]);
                }
                self.fmt.bold += 1;
            }
            Tag::TableRow => {
                if let Some(t) = self.table.as_mut() {
                    t.rows.push(vec![]);
                }
            }
            Tag::TableCell => {
                self.cells += 1;
                // The cell's own markup (`w:tc`, its properties, a paragraph).
                self.spend(220);
                if self.cells > MAX_TABLE_CELLS {
                    self.fail(&format!(
                        "the tables have more than {MAX_TABLE_CELLS} cells in all"
                    ));
                }
                if let Some(t) = self.table.as_mut() {
                    let i = t.rows.last().map_or(0, Vec::len);
                    let align = match t.aligns.get(i) {
                        Some(Alignment::Center) => Some("center"),
                        Some(Alignment::Right) => Some("right"),
                        _ => None,
                    };
                    let header = t.in_head;
                    if let Some(r) = t.rows.last_mut()
                        && r.len() < MAX_TABLE_COLS
                    {
                        r.push(Cell {
                            runs: String::new(),
                            header,
                            align,
                            chars: 0,
                        });
                    }
                }
            }
            Tag::Emphasis => self.fmt.italic += 1,
            Tag::Strong => self.fmt.bold += 1,
            Tag::Strikethrough => self.fmt.strike += 1,
            Tag::Link { dest_url, .. } => {
                let url = xml::clean(&dest_url);
                let lower = url.trim().to_ascii_lowercase();
                let ok = ["http://", "https://", "mailto:"]
                    .iter()
                    .any(|s| lower.starts_with(s))
                    && url.len() <= MAX_URL_BYTES
                    && !url.chars().any(|c| c.is_control() || c.is_whitespace())
                    && self.report.links < MAX_LINKS;
                if ok {
                    let id = match self.rels.iter().position(|u| *u == url) {
                        Some(i) => format!("rId{}", 3 + i),
                        None => {
                            self.rels.push(url.clone());
                            format!("rId{}", 2 + self.rels.len())
                        }
                    };
                    self.report.links += 1;
                    self.push_inline(format!(
                        "<w:hyperlink r:id=\"{}\" w:history=\"1\">",
                        xml::attr(&id)
                    ));
                    self.link_open = true;
                    self.fmt.link = true;
                } else {
                    self.report.links_dropped += 1;
                }
            }
            Tag::Image { .. } => {
                self.report.images_dropped += 1;
                self.in_image += 1;
            }
            Tag::HtmlBlock => {
                self.report.html_dropped += 1;
                self.in_html = true;
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Heading(_) => self.close(),
            TagEnd::BlockQuote(_) => {
                self.close();
                self.quote = self.quote.saturating_sub(1);
            }
            TagEnd::CodeBlock => {
                if let Some(code) = self.code.take() {
                    let code = code.strip_suffix('\n').unwrap_or(&code);
                    let lines: Vec<&str> = if code.is_empty() {
                        vec![]
                    } else {
                        code.split('\n').collect()
                    };
                    for line in lines {
                        let p = Par {
                            style: Some("Code"),
                            num: None,
                            ind: None,
                            jc: None,
                            runs: match runs(
                                line.trim_end_matches('\r'),
                                Fmt::default(),
                                self.room(),
                            ) {
                                Some(r) if self.spend(r.len()) => r,
                                _ => {
                                    self.spend(usize::MAX / 2);
                                    break;
                                }
                            },
                            from_item: false,
                        };
                        self.emit_par(&p);
                        if self.err.is_some() {
                            break;
                        }
                    }
                    if let Some(e) = self.item_emitted.last_mut() {
                        *e = true;
                    }
                }
            }
            TagEnd::List(_) => {
                self.close();
                self.lists.pop();
                self.item_pending = false;
            }
            TagEnd::Item => {
                let emitted = self.item_emitted.pop().unwrap_or(false);
                if self.par.is_none() && !emitted && self.item_pending {
                    // An empty item is an empty bullet.
                    self.item_pending = false;
                    if let Some(num) = self.item_num() {
                        let p = Par {
                            style: Some("ListParagraph"),
                            num: Some(num),
                            ind: None,
                            jc: None,
                            runs: String::new(),
                            from_item: true,
                        };
                        self.emit_par(&p);
                    }
                }
                self.close();
                self.item_pending = false;
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.emit_table(t);
                }
            }
            TagEnd::TableHead => {
                self.fmt.bold = self.fmt.bold.saturating_sub(1);
                if let Some(t) = self.table.as_mut() {
                    t.in_head = false;
                }
            }
            TagEnd::Emphasis => self.fmt.italic = self.fmt.italic.saturating_sub(1),
            TagEnd::Strong => self.fmt.bold = self.fmt.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.fmt.strike = self.fmt.strike.saturating_sub(1),
            TagEnd::Link => {
                if self.link_open {
                    self.push_inline("</w:hyperlink>".into());
                    self.link_open = false;
                    self.fmt.link = false;
                }
            }
            TagEnd::Image => self.in_image = self.in_image.saturating_sub(1),
            TagEnd::HtmlBlock => self.in_html = false,
            _ => {}
        }
    }

    fn emit_table(&mut self, t: Table) {
        let cols = t.rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
        let content = self.content_width;
        self.report.tables += 1;
        // Column widths: by the longest text, within bounds, summing to the
        // content width exactly (Word lays a fixed table out by these).
        let mut weights = vec![4usize; cols];
        for r in &t.rows {
            for (i, c) in r.iter().enumerate() {
                weights[i] = weights[i].max(c.chars.clamp(4, 40));
            }
        }
        let total: usize = weights.iter().sum();
        let mut widths: Vec<u32> = weights
            .iter()
            .map(|w| (content as usize * w / total) as u32)
            .collect();
        let used: u32 = widths.iter().sum();
        if let Some(last) = widths.last_mut() {
            *last += content - used;
        }
        let mut s = String::from("<w:tbl><w:tblPr><w:tblStyle w:val=\"TableGrid\"/>");
        s.push_str(&format!(
            "<w:tblW w:w=\"{content}\" w:type=\"dxa\"/><w:tblLayout w:type=\"fixed\"/>\
             <w:tblLook w:val=\"04A0\" w:firstRow=\"1\" w:lastRow=\"0\" w:firstColumn=\"0\" w:lastColumn=\"0\" w:noHBand=\"0\" w:noVBand=\"1\"/></w:tblPr><w:tblGrid>"
        ));
        for w in &widths {
            s.push_str(&format!("<w:gridCol w:w=\"{w}\"/>"));
        }
        s.push_str("</w:tblGrid>");
        for r in &t.rows {
            s.push_str("<w:tr>");
            let header = r.first().is_some_and(|c| c.header);
            if header {
                s.push_str("<w:trPr><w:tblHeader/></w:trPr>");
            }
            for (i, w) in widths.iter().enumerate() {
                let (runs_xml, align) = match r.get(i) {
                    Some(c) => (c.runs.as_str(), c.align),
                    None => ("", None),
                };
                s.push_str(&format!(
                    "<w:tc><w:tcPr><w:tcW w:w=\"{w}\" w:type=\"dxa\"/>"
                ));
                if header {
                    s.push_str("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"D9E2F3\"/>");
                }
                s.push_str("</w:tcPr><w:p>");
                if let Some(j) = align {
                    s.push_str(&format!("<w:pPr><w:jc w:val=\"{j}\"/></w:pPr>"));
                }
                s.push_str(runs_xml);
                s.push_str("</w:p></w:tc>");
            }
            s.push_str("</w:tr>");
        }
        s.push_str("</w:tbl>");
        self.body.push_str(&s);
        // Word needs a paragraph between two tables and after the last one.
        self.body.push_str("<w:p/>");
    }
}

fn styles_xml() -> String {
    let mut s = String::from(DECL);
    s.push_str(&format!("<w:styles xmlns:w=\"{W}\">"));
    s.push_str(
        "<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\" w:eastAsia=\"Calibri\" w:cs=\"Calibri\"/>\
         <w:sz w:val=\"22\"/><w:szCs w:val=\"22\"/><w:lang w:val=\"en-US\" w:eastAsia=\"en-US\" w:bidi=\"ar-SA\"/></w:rPr></w:rPrDefault>\
         <w:pPrDefault><w:pPr><w:spacing w:after=\"160\" w:line=\"259\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>",
    );
    let heading = |n: u8, size: u32, before: u32, extra: &str| {
        format!(
            "<w:style w:type=\"paragraph\" w:styleId=\"Heading{n}\"><w:name w:val=\"heading {n}\"/><w:basedOn w:val=\"Normal\"/>\
             <w:next w:val=\"Normal\"/><w:uiPriority w:val=\"9\"/><w:qFormat/>\
             <w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"{before}\" w:after=\"120\"/><w:outlineLvl w:val=\"{}\"/></w:pPr>\
             <w:rPr><w:b/><w:bCs/>{extra}<w:sz w:val=\"{size}\"/><w:szCs w:val=\"{size}\"/></w:rPr></w:style>",
            n - 1
        )
    };
    s.push_str(
        "<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style>\
         <w:style w:type=\"character\" w:default=\"1\" w:styleId=\"DefaultParagraphFont\"><w:name w:val=\"Default Paragraph Font\"/><w:uiPriority w:val=\"1\"/><w:semiHidden/><w:unhideWhenUsed/></w:style>\
         <w:style w:type=\"table\" w:default=\"1\" w:styleId=\"TableNormal\"><w:name w:val=\"Normal Table\"/><w:uiPriority w:val=\"99\"/><w:semiHidden/><w:unhideWhenUsed/>\
         <w:tblPr><w:tblInd w:w=\"0\" w:type=\"dxa\"/><w:tblCellMar><w:top w:w=\"0\" w:type=\"dxa\"/><w:left w:w=\"108\" w:type=\"dxa\"/><w:bottom w:w=\"0\" w:type=\"dxa\"/><w:right w:w=\"108\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr></w:style>\
         <w:style w:type=\"numbering\" w:default=\"1\" w:styleId=\"NoList\"><w:name w:val=\"No List\"/><w:uiPriority w:val=\"99\"/><w:semiHidden/><w:unhideWhenUsed/></w:style>",
    );
    s.push_str(&heading(1, 36, 360, "<w:color w:val=\"1F3864\"/>"));
    s.push_str(&heading(2, 30, 280, "<w:color w:val=\"1F3864\"/>"));
    s.push_str(&heading(3, 26, 240, "<w:color w:val=\"2F5496\"/>"));
    s.push_str(&heading(4, 23, 200, "<w:color w:val=\"2F5496\"/>"));
    s.push_str(
        "<w:style w:type=\"paragraph\" w:styleId=\"Code\"><w:name w:val=\"Code\"/><w:basedOn w:val=\"Normal\"/><w:qFormat/>\
         <w:pPr><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F2F2F2\"/><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr>\
         <w:rPr><w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\" w:cs=\"Consolas\"/><w:sz w:val=\"20\"/><w:szCs w:val=\"20\"/></w:rPr></w:style>\
         <w:style w:type=\"character\" w:styleId=\"InlineCode\"><w:name w:val=\"Inline Code\"/><w:basedOn w:val=\"DefaultParagraphFont\"/><w:qFormat/>\
         <w:rPr><w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\" w:cs=\"Consolas\"/><w:sz w:val=\"20\"/><w:szCs w:val=\"20\"/><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F2F2F2\"/></w:rPr></w:style>\
         <w:style w:type=\"paragraph\" w:styleId=\"Quote\"><w:name w:val=\"Quote\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/>\
         <w:pPr><w:pBdr><w:left w:val=\"single\" w:sz=\"18\" w:space=\"8\" w:color=\"A6A6A6\"/></w:pBdr><w:ind w:left=\"720\"/></w:pPr>\
         <w:rPr><w:i/><w:iCs/><w:color w:val=\"404040\"/></w:rPr></w:style>\
         <w:style w:type=\"paragraph\" w:styleId=\"ListParagraph\"><w:name w:val=\"List Paragraph\"/><w:basedOn w:val=\"Normal\"/><w:uiPriority w:val=\"34\"/><w:qFormat/>\
         <w:pPr><w:spacing w:after=\"60\"/><w:ind w:left=\"720\"/></w:pPr></w:style>\
         <w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/><w:basedOn w:val=\"DefaultParagraphFont\"/><w:uiPriority w:val=\"99\"/><w:unhideWhenUsed/>\
         <w:rPr><w:color w:val=\"0563C1\"/><w:u w:val=\"single\"/></w:rPr></w:style>\
         <w:style w:type=\"table\" w:styleId=\"TableGrid\"><w:name w:val=\"Table Grid\"/><w:basedOn w:val=\"TableNormal\"/><w:uiPriority w:val=\"39\"/>\
         <w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr>\
         <w:tblPr><w:tblBorders><w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>\
         <w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>\
         <w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/></w:tblBorders></w:tblPr></w:style>",
    );
    s.push_str("</w:styles>");
    s
}

fn numbering_xml(ordered: &[u64]) -> String {
    let mut s = String::from(DECL);
    s.push_str(&format!("<w:numbering xmlns:w=\"{W}\">"));
    let bullets = ["\u{2022}", "\u{25E6}", "\u{25AA}"];
    s.push_str(
        "<w:abstractNum w:abstractNumId=\"0\"><w:multiLevelType w:val=\"hybridMultilevel\"/>",
    );
    for (i, b) in bullets.iter().enumerate() {
        s.push_str(&format!(
            "<w:lvl w:ilvl=\"{i}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/><w:lvlText w:val=\"{b}\"/><w:lvlJc w:val=\"left\"/>\
             <w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr>\
             <w:rPr><w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\" w:cs=\"Calibri\" w:hint=\"default\"/></w:rPr></w:lvl>",
            720 * (i + 1)
        ));
    }
    s.push_str("</w:abstractNum>");
    let fmts = [
        ("decimal", "%1."),
        ("lowerLetter", "%2."),
        ("lowerRoman", "%3."),
    ];
    s.push_str(
        "<w:abstractNum w:abstractNumId=\"1\"><w:multiLevelType w:val=\"hybridMultilevel\"/>",
    );
    for (i, (f, t)) in fmts.iter().enumerate() {
        s.push_str(&format!(
            "<w:lvl w:ilvl=\"{i}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{f}\"/><w:lvlText w:val=\"{t}\"/><w:lvlJc w:val=\"left\"/>\
             <w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr></w:lvl>",
            720 * (i + 1)
        ));
    }
    s.push_str("</w:abstractNum>");
    s.push_str("<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>");
    for (i, start) in ordered.iter().enumerate() {
        let start = (*start).min(1_000_000);
        s.push_str(&format!(
            "<w:num w:numId=\"{}\"><w:abstractNumId w:val=\"1\"/>\
             <w:lvlOverride w:ilvl=\"0\"><w:startOverride w:val=\"{start}\"/></w:lvlOverride>\
             <w:lvlOverride w:ilvl=\"1\"><w:startOverride w:val=\"1\"/></w:lvlOverride>\
             <w:lvlOverride w:ilvl=\"2\"><w:startOverride w:val=\"1\"/></w:lvlOverride></w:num>",
            2 + i
        ));
    }
    s.push_str("</w:numbering>");
    s
}

/// Build the `.docx` of `markdown`.
pub fn build(markdown: &str, o: &DocOptions) -> Result<Built> {
    if markdown.len() > MAX_MARKDOWN_BYTES {
        return Err(format!(
            "the Markdown is larger than {} KiB; split the document into several files",
            MAX_MARKDOWN_BYTES >> 10
        ));
    }
    if markdown.trim().is_empty() {
        return Err("the document is empty: give its text as Markdown".into());
    }
    let markers = markdown
        .bytes()
        .filter(|b| {
            matches!(
                b,
                b'[' | b']' | b'*' | b'_' | b'~' | b'`' | b'<' | b'>' | b'!'
            )
        })
        .count();
    if markers > MAX_INLINE_MARKERS {
        return Err(format!(
            "the Markdown has more than {MAX_INLINE_MARKERS} characters of inline markup ([ ] * _ ~ ` < > !); write it as plain text or split it"
        ));
    }
    let (pw, ph, content) = page_dims(o);
    let start = format!("{DECL}<w:document xmlns:w=\"{W}\" xmlns:r=\"{R}\"><w:body>");
    let mut g = Gen::new(content, start);
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    for ev in Parser::new_ext(markdown, opts) {
        match ev {
            Event::Start(t) => g.start(t),
            Event::End(t) => g.end(t),
            Event::Text(t) => {
                if let Some(c) = g.code.as_mut() {
                    c.push_str(&t);
                } else if !g.in_html {
                    g.text(&t);
                }
            }
            Event::Code(t) => {
                let keep = g.fmt;
                g.fmt.code = true;
                g.text(&t);
                g.fmt = keep;
            }
            Event::Html(_) => {}
            Event::InlineHtml(_) => g.report.html_dropped += 1,
            Event::SoftBreak => g.text(" "),
            Event::HardBreak => g.push_inline("<w:r><w:br/></w:r>".into()),
            Event::Rule => {
                g.close();
                if g.report.paragraphs >= MAX_PARAGRAPHS {
                    g.fail(&format!(
                        "the document has more than {MAX_PARAGRAPHS} paragraphs"
                    ));
                } else {
                    if g.spend(60) {
                        g.report.paragraphs += 1;
                        g.body
                            .push_str("<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>");
                    }
                }
            }
            _ => {}
        }
        if g.err.is_some() {
            break;
        }
    }
    g.close();
    if let Some(e) = g.err.take() {
        return Err(e);
    }
    if g.report.paragraphs == 0 && g.report.tables == 0 {
        return Err(if g.report.html_dropped > 0 {
            "the document has no content: raw HTML is not used, write Markdown".into()
        } else {
            "the document has no content".into()
        });
    }
    let title: Option<String> = o
        .title
        .as_deref()
        .map(|t| xml::clean(&harness_core::text::clean_line(t, MAX_TITLE_CHARS)))
        .filter(|t| !t.is_empty());

    // The part was started in place and is finished in place: no second
    // copy of the whole body.
    let mut doc = std::mem::take(&mut g.body);
    doc.push_str(&format!(
        "<w:sectPr><w:pgSz w:w=\"{pw}\" w:h=\"{ph}\"{}/>\
         <w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>",
        if o.landscape { " w:orient=\"landscape\"" } else { "" }
    ));

    let mut ct = ContentTypes::new();
    ct.part(
        "word/document.xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
    );
    ct.part(
        "word/styles.xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml",
    );
    ct.part(
        "word/numbering.xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml",
    );
    ct.part(
        "docProps/core.xml",
        "application/vnd.openxmlformats-package.core-properties+xml",
    );
    let mut drels = vec![
        (
            "rId1".to_string(),
            format!("{REL_BASE}/styles"),
            "styles.xml".to_string(),
            false,
        ),
        (
            "rId2".to_string(),
            format!("{REL_BASE}/numbering"),
            "numbering.xml".to_string(),
            false,
        ),
    ];
    for (i, url) in g.rels.iter().enumerate() {
        drels.push((
            format!("rId{}", 3 + i),
            format!("{REL_BASE}/hyperlink"),
            url.clone(),
            true,
        ));
    }
    let root_rels = vec![
        (
            "rId1".to_string(),
            format!("{REL_BASE}/officeDocument"),
            "word/document.xml".to_string(),
            false,
        ),
        (
            "rId2".to_string(),
            REL_CORE.to_string(),
            "docProps/core.xml".to_string(),
            false,
        ),
    ];
    let mut z = ZipWriter::new(MAX_OUTPUT_BYTES);
    z.add("[Content_Types].xml", ct.xml().as_bytes())?;
    z.add("_rels/.rels", xml::rels(&root_rels).as_bytes())?;
    z.add("word/document.xml", doc.as_bytes())?;
    z.add("word/_rels/document.xml.rels", xml::rels(&drels).as_bytes())?;
    z.add("word/styles.xml", styles_xml().as_bytes())?;
    z.add("word/numbering.xml", numbering_xml(&g.ordered).as_bytes())?;
    z.add(
        "docProps/core.xml",
        xml::core_props(title.as_deref(), &crate::iso_utc(o.created)).as_bytes(),
    )?;
    Ok(Built {
        bytes: z.finish()?,
        report: g.report,
    })
}
