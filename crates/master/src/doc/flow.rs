//! What a document's text becomes for a model: paragraphs in order, headings
//! as `# `, list items as `- `, a table as rows of cells joined with ` | `.
//! The XML readers (Word, OpenDocument, PowerPoint) translate their own
//! elements into the calls of [`Flow`]; nothing here knows a format.
//!
//! Every string from the file goes through
//! [`harness_core::text::clean_line`]: hidden characters (right-to-left
//! overrides, zero-width), controls and every newline are gone, so a line is
//! one line and a paragraph cannot open a line of its own that poses as the
//! harness (the attachment is quoted line by line behind that anyway).

use harness_core::text::clean_line;

/// Characters kept of one paragraph, one table cell or one sheet name.
pub const LINE_CHARS: usize = 20_000;
/// Characters kept of one spreadsheet cell.
pub const CELL_CHARS: usize = 1_000;
/// Cells of one table row that are kept; the rest are counted, not stored.
pub const MAX_CELLS: usize = super::MAX_COLS;
/// Raw bytes a paragraph collects before it stops taking more: what
/// [`LINE_CHARS`] can need, with room for what cleaning drops.
const RAW_MAX: usize = LINE_CHARS * 4 + 4096;

/// The output: lines, at most `cap` characters in all.
pub struct Out {
    pub s: String,
    chars: usize,
    cap: usize,
    /// Something hidden in the document was left out (hidden text, rows,
    /// columns, sheets, slides): the text ends with a line saying so.
    pub hidden: bool,
}

/// Append at most `max` bytes in all to `dst`, cutting `src` while it is
/// appended (at a character boundary), so one huge text event is never held
/// whole by the reader.
pub fn push_capped(dst: &mut String, src: &str, max: usize) {
    let room = max.saturating_sub(dst.len());
    if src.len() <= room {
        dst.push_str(src);
        return;
    }
    let mut end = room;
    while !src.is_char_boundary(end) {
        end -= 1;
    }
    dst.push_str(&src[..end]);
}

impl Out {
    pub fn new(cap: usize) -> Self {
        Self {
            s: String::new(),
            chars: 0,
            cap,
            hidden: false,
        }
    }

    /// Where the output is now, to cut back to ([`Self::rewind`]).
    pub fn mark(&self) -> (usize, usize) {
        (self.s.len(), self.chars)
    }

    /// Take back what was written since `mark`.
    pub fn rewind(&mut self, mark: (usize, usize)) {
        self.s.truncate(mark.0);
        self.chars = mark.1;
    }

    /// Enough has been written: the readers stop.
    pub fn full(&self) -> bool {
        self.chars >= self.cap
    }

    /// One line (already cleaned), cut at the cap.
    pub fn line(&mut self, line: &str) {
        let room = self.cap.saturating_sub(self.chars);
        if room == 0 {
            return;
        }
        let end = line.char_indices().nth(room).map_or(line.len(), |(i, _)| i);
        self.s.push_str(&line[..end]);
        self.chars += line[..end].chars().count();
        if self.chars < self.cap {
            self.s.push('\n');
            self.chars += 1;
        }
    }
}

#[derive(Default)]
struct Para {
    text: String,
    heading: Option<u8>,
    list: Option<u8>,
}

#[derive(Default)]
struct Table {
    cells: Vec<String>,
    /// Cells past [`MAX_CELLS`] in this row: counted, not stored.
    extra: usize,
    cell: Option<String>,
}

/// Paragraphs and tables being read, nested as the file nests them.
#[derive(Default)]
pub struct Flow {
    paras: Vec<Para>,
    tables: Vec<Table>,
}

impl Flow {
    pub fn begin_para(&mut self) {
        self.paras.push(Para::default());
    }

    pub fn heading(&mut self, level: u8) {
        if let Some(p) = self.paras.last_mut() {
            p.heading = Some(level.clamp(1, 6));
        }
    }

    pub fn list(&mut self, level: u8) {
        if let Some(p) = self.paras.last_mut() {
            p.list = Some(level.min(8));
        }
    }

    /// Text of the innermost open paragraph; nothing without one.
    pub fn text(&mut self, t: &str) {
        if let Some(p) = self.paras.last_mut() {
            push_capped(&mut p.text, t, RAW_MAX);
        }
    }

    pub fn end_para(&mut self, out: &mut Out) {
        let Some(p) = self.paras.pop() else { return };
        let text = clean_line(&p.text, LINE_CHARS);
        if text.is_empty() {
            return;
        }
        // In a table cell the text is the cell's: no marker of its own.
        if let Some(cell) = self.tables.last_mut().and_then(|t| t.cell.as_mut()) {
            if !cell.is_empty() {
                push_capped(cell, " ", RAW_MAX);
            }
            push_capped(cell, &text, RAW_MAX);
            return;
        }
        let line = match (p.heading, p.list) {
            (Some(h), _) => format!("{} {text}", "#".repeat(usize::from(h))),
            (None, Some(l)) => format!("{}- {text}", "  ".repeat(usize::from(l))),
            (None, None) => text,
        };
        out.line(&line);
    }

    pub fn begin_table(&mut self) {
        self.tables.push(Table::default());
    }

    pub fn begin_row(&mut self) {
        if let Some(t) = self.tables.last_mut() {
            t.cells.clear();
            t.extra = 0;
            t.cell = None;
        }
    }

    pub fn begin_cell(&mut self) {
        if let Some(t) = self.tables.last_mut() {
            t.cell = Some(String::new());
        }
    }

    pub fn end_cell(&mut self) {
        if let Some(t) = self.tables.last_mut()
            && let Some(c) = t.cell.take()
        {
            if t.cells.len() < MAX_CELLS {
                t.cells.push(c);
            } else {
                t.extra += 1;
            }
        }
    }

    /// A finished row is a line (a row of a table inside a cell becomes
    /// part of that cell's text); a row with no text at all is none.
    pub fn end_row(&mut self, out: &mut Out) {
        let Some(t) = self.tables.last_mut() else {
            return;
        };
        let cells = std::mem::take(&mut t.cells);
        let extra = std::mem::take(&mut t.extra);
        if extra == 0 && cells.iter().all(String::is_empty) {
            return;
        }
        let mut line = cells.join(" | ");
        if extra > 0 {
            line.push_str(&format!(" | [+{extra} cells]"));
        }
        match self
            .tables
            .len()
            .checked_sub(2)
            .map(|i| &mut self.tables[i])
        {
            Some(parent) if parent.cell.is_some() => {
                let c = parent.cell.get_or_insert_with(String::new);
                if !c.is_empty() {
                    push_capped(c, " ", RAW_MAX);
                }
                push_capped(c, &line, RAW_MAX);
            }
            _ => out.line(&line),
        }
    }

    pub fn end_table(&mut self) {
        self.tables.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_stop_at_the_cap() {
        let mut o = Out::new(8);
        o.line("abc");
        assert_eq!(o.s, "abc\n");
        o.line("defghij");
        assert_eq!(o.s, "abc\ndefg");
        assert!(o.full());
        o.line("more");
        assert_eq!(o.s, "abc\ndefg");
    }

    /// What the reader holds is bounded by its own caps, whatever the file
    /// holds: cells of a row past the cap are counted, a cell's text and a
    /// paragraph stop growing, and one text event is cut as it is taken.
    #[test]
    fn what_is_held_is_bounded_by_the_readers_own_caps() {
        let mut f = Flow::default();
        let mut o = Out::new(1_000_000);
        f.begin_table();
        f.begin_row();
        for _ in 0..100_000 {
            f.begin_cell();
            f.end_cell();
        }
        assert_eq!(f.tables[0].cells.len(), MAX_CELLS);
        assert_eq!(f.tables[0].extra, 100_000 - MAX_CELLS);
        f.begin_cell();
        f.begin_para();
        f.text("x");
        f.end_para(&mut o);
        f.end_cell();
        f.end_row(&mut o);
        assert!(
            o.s.contains("[+99745 cells]"),
            "{:?}",
            &o.s[o.s.len().saturating_sub(40)..]
        );
        // A cell's text stops growing across paragraphs.
        f.begin_row();
        f.begin_cell();
        for _ in 0..100_000 {
            f.begin_para();
            f.text(&"abcdefghij".repeat(10));
            f.end_para(&mut o);
        }
        assert!(f.tables[0].cell.as_ref().unwrap().len() <= RAW_MAX + 1);
        // One huge text event is cut while it is taken in.
        f.begin_para();
        f.text(&"é".repeat(10 << 20));
        assert!(f.paras[0].text.len() <= RAW_MAX);
        assert!(f.paras[0].text.is_char_boundary(f.paras[0].text.len()));
    }

    #[test]
    fn headings_lists_and_tables() {
        let mut f = Flow::default();
        let mut o = Out::new(1000);
        f.begin_para();
        f.heading(2);
        f.text("Title\u{202e} here");
        f.end_para(&mut o);
        f.begin_para();
        f.list(1);
        f.text("item\none");
        f.end_para(&mut o);
        f.begin_table();
        f.begin_row();
        for c in ["a", "b"] {
            f.begin_cell();
            f.begin_para();
            f.text(c);
            f.end_para(&mut o);
            f.end_cell();
        }
        f.end_row(&mut o);
        f.end_table();
        assert_eq!(o.s, "## Title here\n  - item one\na | b\n");
    }
}
