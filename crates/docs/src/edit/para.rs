//! One Word paragraph read for an edit (PLAN.md T98, report 3.4 docx 1, 3).
//! What a person sees of a paragraph is its visible text: the text of the
//! runs, a tab as a tab, a line break as a line break. Everything else a
//! paragraph can hold is not text and is never matched or cut: a picture,
//! a footnote mark, a field's code, a field's result, a tracked deletion,
//! hidden text. A character of the visible text carries the region it sits
//! in (inside a hyperlink, a tracked insertion, a content control, or none)
//! and whether it is locked: an edit that would cut across two regions, or
//! touch a locked character, is refused.

use quick_xml::events::Event;

use super::scan::{Scan, attr, event_text};
use crate::Result;
use crate::limits::MAX_EDIT_EVENTS;

/// The stand-in for something in the text that is not text (a picture, a
/// footnote mark): never matched by a `find`, never cut.
pub const OBJ: char = '\u{FFFC}';

/// A `w:t` element.
#[derive(Debug, Clone)]
pub struct TNode {
    /// The element: start tag start .. end tag end (or the whole `<w:t/>`).
    pub start: usize,
    pub end: usize,
    /// Where its start tag ends (the content begins).
    pub open_end: usize,
    pub empty: bool,
    pub preserve: bool,
    pub text: String,
    pub dirty: bool,
}

/// A step of the visible text.
#[derive(Debug, Clone)]
pub enum Kind {
    /// The text of `tnodes[i]`.
    T(usize),
    /// One character that is not in a `w:t` (a tab, a break, [`OBJ`]).
    Ch(char),
}

#[derive(Debug, Clone)]
pub struct Piece {
    /// Hidden text lies between this piece and the one before it.
    pub gap: bool,
    pub kind: Kind,
    pub region: u32,
    pub locked: bool,
}

/// A run directly inside the paragraph.
#[derive(Debug, Clone)]
pub struct Run {
    pub start: usize,
    pub end: usize,
    /// End of the start tag and start of the end tag.
    pub open_end: usize,
    pub close_start: usize,
    pub rpr: Option<(usize, usize)>,
    pub has_text: bool,
    pub hidden: bool,
}

#[derive(Debug, Default, Clone)]
pub struct ParaScan {
    pub tnodes: Vec<TNode>,
    pub pieces: Vec<Piece>,
    pub runs: Vec<Run>,
    /// The paragraph's `pPr` children: local name and byte range.
    pub ppr: Vec<(String, usize, usize)>,
    pub ppr_range: Option<(usize, usize)>,
    pub style: Option<String>,
    pub numpr: bool,
    /// What the paragraph holds besides plain runs.
    pub sect: bool,
    pub barrier: bool,
    pub field: bool,
    pub wrapper: bool,
    pub bookmark: bool,
    pub comment: bool,
    pub tracked: bool,
    pub hidden: bool,
    /// The start tag's end and the end tag's start (equal to `end` and
    /// `start` of a `<w:p/>`: `self_closing`).
    pub open_end: usize,
    pub close_start: usize,
    pub self_closing: bool,
    /// Fields open at the end of the paragraph (a field can run over several
    /// paragraphs: a table of contents).
    pub field_out: i32,
    /// The paragraph starts inside a field's result.
    pub in_field: bool,
}

impl ParaScan {
    /// The visible text.
    pub fn text(&self) -> String {
        let mut s = String::new();
        for p in &self.pieces {
            match &p.kind {
                Kind::T(i) => s.push_str(&self.tnodes[*i].text),
                Kind::Ch(c) => s.push(*c),
            }
        }
        s
    }

    /// Nothing in it but plain runs, text and bookmarks: what `set_text`
    /// can rewrite without losing something the person put there.
    pub fn plain(&self) -> bool {
        !(self.barrier
            || self.field
            || self.in_field
            || self.wrapper
            || self.tracked
            || self.hidden
            || self.sect)
    }

    /// What `delete` may remove: plain, and no anchor that another part of
    /// the document points at.
    pub fn removable(&self) -> bool {
        self.plain() && !self.bookmark && !self.comment
    }
}

fn is_off(v: Option<String>) -> bool {
    matches!(v.as_deref(), Some("0" | "false" | "off"))
}

enum Ctx {
    P,
    Run(usize),
    T(usize),
    Wrap { simple: bool },
}

/// Read the element at `src` (a `w:p`; positions are relative to `src`).
pub fn scan(src: &[u8], prefix: &str, ns: &'static str, field_in: i32) -> Result<ParaScan> {
    let mut s = Scan::within(src, ns, prefix, MAX_EDIT_EVENTS)?;
    let mut out = ParaScan::default();
    let mut stack: Vec<Ctx> = vec![];
    let mut region = 0u32;
    let mut next_region = 1u32;
    let mut field_depth = field_in;
    let mut simple_depth = 0i32;
    let mut run_hidden = false;
    let mut pending_gap = false;
    let mut first = true;
    while let Some(t) = s.next()? {
        let (e, empty) = match &t.ev {
            Event::Start(e) => (Some(e), false),
            Event::Empty(e) => (Some(e), true),
            _ => (None, false),
        };
        if first {
            first = false;
            let Some(e) = e else {
                return Err("not a paragraph".into());
            };
            if s.local(e.name()) != Some("p") {
                return Err("not a paragraph".into());
            }
            out.self_closing = empty;
            out.open_end = t.end;
            out.close_start = if empty { t.end } else { 0 };
            if !empty {
                stack.push(Ctx::P);
            }
            continue;
        }
        if let Event::End(_) = &t.ev {
            match stack.pop() {
                Some(Ctx::Run(i)) => {
                    out.runs[i].end = t.end;
                    out.runs[i].close_start = t.start;
                    run_hidden = false;
                }
                Some(Ctx::T(i)) => {
                    // The content is replaced as a whole: its end is here.
                    let n = &mut out.tnodes[i];
                    n.end = t.end;
                }
                Some(Ctx::Wrap { simple }) => {
                    if simple {
                        simple_depth -= 1;
                    }
                    region = next_region;
                    next_region += 1;
                }
                Some(Ctx::P) | None => {
                    out.close_start = t.start;
                    break;
                }
            }
            continue;
        }
        if let Some(txt) = event_text(&t.ev) {
            if let Some(Ctx::T(i)) = stack.last() {
                out.tnodes[*i].text.push_str(&txt);
            }
            continue;
        }
        let Some(e) = e else { continue };
        let local = s.local(e.name());
        let parent = stack.last();
        // Directly in a run.
        if matches!(parent, Some(Ctx::Run(_))) {
            let hide = run_hidden;
            let locked = field_depth > 0 || simple_depth > 0;
            match local {
                Some("rPr") => {
                    let ri = match parent {
                        Some(Ctx::Run(i)) => *i,
                        _ => 0,
                    };
                    out.runs[ri].rpr = Some((t.start, t.end));
                    if !empty {
                        let d = t.depth;
                        loop {
                            let Some(c) = s.next()? else {
                                return Err("a part of the file is cut short".into());
                            };
                            if let Event::End(_) = c.ev
                                && c.depth == d
                            {
                                out.runs[ri].rpr = Some((t.start, c.end));
                                break;
                            }
                            if let Event::Start(ce) | Event::Empty(ce) = &c.ev
                                && c.depth
                                    == if matches!(c.ev, Event::Empty(_)) {
                                        d
                                    } else {
                                        d + 1
                                    }
                                && s.local(ce.name()) == Some("vanish")
                                && !is_off(attr(ce, "val")?)
                            {
                                run_hidden = true;
                                out.runs[ri].hidden = true;
                                out.hidden = true;
                            }
                            if let Event::Start(ce) = &c.ev
                                && c.depth == d + 1
                            {
                                // A property with children: read over it.
                                let _ = ce;
                                s.skip_to_end(c.depth)?;
                            }
                        }
                    }
                }
                Some("t") => {
                    let ri = match parent {
                        Some(Ctx::Run(i)) => *i,
                        _ => 0,
                    };
                    let preserve = attr(e, "space")?.as_deref() == Some("preserve");
                    if hide {
                        pending_gap = true;
                        if !empty {
                            s.skip_to_end(t.depth)?;
                        }
                        continue;
                    }
                    out.runs[ri].has_text = true;
                    out.tnodes.push(TNode {
                        start: t.start,
                        end: t.end,
                        open_end: t.end,
                        empty,
                        preserve,
                        text: String::new(),
                        dirty: false,
                    });
                    let i = out.tnodes.len() - 1;
                    out.pieces.push(Piece {
                        gap: std::mem::take(&mut pending_gap),
                        kind: Kind::T(i),
                        region,
                        locked,
                    });
                    if !empty {
                        stack.push(Ctx::T(i));
                    }
                }
                Some("tab") | Some("br") | Some("cr") | Some("noBreakHyphen") => {
                    if !hide {
                        let kind = attr(e, "type")?;
                        let c = match local {
                            Some("tab") => '\t',
                            Some("br") if matches!(kind.as_deref(), Some("page" | "column")) => OBJ,
                            Some("br") | Some("cr") => '\n',
                            _ => '\u{2011}',
                        };
                        if c == OBJ {
                            out.barrier = true;
                        }
                        out.pieces.push(Piece {
                            gap: std::mem::take(&mut pending_gap),
                            kind: Kind::Ch(c),
                            region,
                            locked: true,
                        });
                    }
                    if !empty {
                        s.skip_to_end(t.depth)?;
                    }
                }
                Some(
                    "delText"
                    | "instrText"
                    | "delInstrText"
                    | "lastRenderedPageBreak"
                    | "softHyphen"
                    | "annotationRef",
                ) => {
                    if !empty {
                        s.skip_to_end(t.depth)?;
                    }
                }
                Some("fldChar") => {
                    out.field = true;
                    match attr(e, "fldCharType")?.as_deref() {
                        Some("begin") => field_depth += 1,
                        Some("end") => field_depth = (field_depth - 1).max(0),
                        _ => {}
                    }
                    if !empty {
                        s.skip_to_end(t.depth)?;
                    }
                }
                _ => {
                    // A picture, a footnote mark, a symbol, anything else: in
                    // the text as a thing that is not text.
                    if !hide {
                        out.barrier = true;
                        out.pieces.push(Piece {
                            gap: std::mem::take(&mut pending_gap),
                            kind: Kind::Ch(OBJ),
                            region,
                            locked: true,
                        });
                    }
                    if !empty {
                        s.skip_to_end(t.depth)?;
                    }
                }
            }
            continue;
        }
        // Directly in the paragraph, or in something that wraps runs.
        match local {
            Some("pPr") if matches!(parent, Some(Ctx::P)) => {
                out.ppr_range = Some((t.start, t.end));
                if !empty {
                    let d = t.depth;
                    loop {
                        let Some(c) = s.next()? else {
                            return Err("a part of the file is cut short".into());
                        };
                        if let Event::End(_) = c.ev
                            && c.depth == d
                        {
                            out.ppr_range = Some((t.start, c.end));
                            break;
                        }
                        let (ce, cempty) = match &c.ev {
                            Event::Start(x) => (x, false),
                            Event::Empty(x) => (x, true),
                            _ => continue,
                        };
                        if c.depth != d + 1 && !(cempty && c.depth == d) {
                            continue;
                        }
                        let name = s.local(ce.name()).unwrap_or("?").to_string();
                        let mut end = c.end;
                        if !cempty {
                            end = s.skip_to_end(c.depth)?;
                        }
                        match name.as_str() {
                            "pStyle" => out.style = attr(ce, "val")?,
                            "numPr" => out.numpr = true,
                            "sectPr" => out.sect = true,
                            _ => {}
                        }
                        out.ppr.push((name, c.start, end));
                    }
                }
            }
            Some("r") => {
                out.runs.push(Run {
                    start: t.start,
                    end: t.end,
                    open_end: t.end,
                    close_start: t.end,
                    rpr: None,
                    has_text: false,
                    hidden: false,
                });
                if !empty {
                    // A run inside a wrapper still holds text; `set_text`
                    // refuses a paragraph with a wrapper, so it never sees one.
                    stack.push(Ctx::Run(out.runs.len() - 1));
                    run_hidden = false;
                }
            }
            Some(
                "hyperlink" | "ins" | "moveTo" | "smartTag" | "dir" | "bdo" | "customXml"
                | "sdtContent",
            ) => {
                out.wrapper = true;
                if local == Some("ins") || local == Some("moveTo") {
                    out.tracked = true;
                }
                if !empty {
                    stack.push(Ctx::Wrap { simple: false });
                    region = next_region;
                    next_region += 1;
                }
            }
            Some("sdt") => {
                out.wrapper = true;
                if !empty {
                    stack.push(Ctx::Wrap { simple: false });
                    region = next_region;
                    next_region += 1;
                }
            }
            Some("fldSimple") => {
                out.field = true;
                out.wrapper = true;
                if !empty {
                    simple_depth += 1;
                    stack.push(Ctx::Wrap { simple: true });
                    region = next_region;
                    next_region += 1;
                }
            }
            Some("del" | "moveFrom") => {
                out.tracked = true;
                if !empty {
                    s.skip_to_end(t.depth)?;
                }
            }
            Some("bookmarkStart" | "bookmarkEnd") => {
                out.bookmark = true;
                if !empty {
                    s.skip_to_end(t.depth)?;
                }
            }
            Some("commentRangeStart" | "commentRangeEnd") => {
                out.comment = true;
                if !empty {
                    s.skip_to_end(t.depth)?;
                }
            }
            Some("sdtPr" | "sdtEndPr" | "proofErr" | "permStart" | "permEnd") => {
                if !empty {
                    s.skip_to_end(t.depth)?;
                }
            }
            _ => {
                out.barrier = true;
                out.pieces.push(Piece {
                    gap: std::mem::take(&mut pending_gap),
                    kind: Kind::Ch(OBJ),
                    region,
                    locked: true,
                });
                if !empty {
                    s.skip_to_end(t.depth)?;
                }
            }
        }
    }
    out.field_out = field_depth;
    out.in_field = field_in > 0;
    Ok(out)
}
