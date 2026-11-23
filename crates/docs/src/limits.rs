//! Every limit of the create tools, in one place (PLAN.md T97). Each is
//! tested at its edge and one past it.

/// The Markdown of one `doc_create`, in bytes.
pub const MAX_MARKDOWN_BYTES: usize = 1 << 20;
/// Characters in the Markdown that start or end inline markup (`[ ] * _ ~ `
/// and `<`, `>`, `!`): the parser makes a node for each before the first one is
/// used, so its memory grows with their number (about 50 bytes each, a
/// million of them 55 MiB). Prose has a few thousand in a megabyte.
pub const MAX_INLINE_MARKERS: usize = 150_000;
/// Paragraphs of one document (code lines, list items and headings count;
/// table cells have their own count).
pub const MAX_PARAGRAPHS: usize = 20_000;
/// Tables of one document.
pub const MAX_TABLES: usize = 200;
/// Columns of one table.
pub const MAX_TABLE_COLS: usize = 30;
/// Cells of all tables of one document.
pub const MAX_TABLE_CELLS: usize = 20_000;
/// Ordered lists of one document (each gets its own numbering).
pub const MAX_LISTS: usize = 2_000;
/// Characters of a document title (a property of the file).
pub const MAX_TITLE_CHARS: usize = 200;
/// The longest link address kept, in bytes.
pub const MAX_URL_BYTES: usize = 2_000;
/// Links of one document.
pub const MAX_LINKS: usize = 2_000;

/// The sheet spec of one `sheet_create`, as JSON text, in bytes.
pub const MAX_SPEC_BYTES: usize = 4 << 20;
/// Sheets of one workbook.
pub const MAX_SHEETS: usize = 20;
/// Rows of one sheet, header included.
pub const MAX_ROWS: usize = 10_000;
/// Columns of one sheet.
pub const MAX_COLS: usize = 100;
/// Cells of one call, all sheets together.
pub const MAX_CELLS: usize = 200_000;
/// Characters of one text cell.
pub const MAX_CELL_CHARS: usize = 10_000;
/// Characters of one formula (Excel's own limit is 8,192).
pub const MAX_FORMULA_CHARS: usize = 8_192;
/// Characters of a sheet's name (Excel's limit).
pub const MAX_SHEET_NAME: usize = 31;
/// Characters of a column header.
pub const MAX_HEADER_CHARS: usize = 200;
/// The widest column asked for, in characters.
pub const MAX_COL_WIDTH: f64 = 100.0;

/// The finished file, in bytes (the upload limit).
pub const MAX_OUTPUT_BYTES: usize = 10 << 20;
/// One part of the package, unpacked, in bytes: the reader of
/// `harness-attached` cuts a part at four million XML events, and a part
/// this size is already past it.
pub const MAX_PART_BYTES: usize = 24 << 20;
/// The document part (`word/document.xml`) while it is built, in bytes: all
/// of it counted, the finished body, the paragraph being written and each
/// table cell, before anything is copied. A part past it is refused, so one
/// build holds a few times this and not an amount that grows with what the
/// Markdown can multiply into (a tab is thirty bytes of XML, and ninety in
/// bold italic struck text).
pub const MAX_DOC_PART_BYTES: usize = 16 << 20;
/// Characters of a file name, without its extension.
pub const MAX_NAME_CHARS: usize = 100;

// The edit tools (PLAN.md T98). Each is tested at its edge and one past it.

/// The part an edit patches (`word/document.xml`, a worksheet), unpacked, in
/// bytes. A larger part is refused: the whole of it is held, and so is what
/// the edit makes of it.
pub const MAX_EDIT_PART_BYTES: usize = 8 << 20;
/// The same part after the edit, in bytes: counted while it is written.
pub const MAX_EDIT_OUT_PART_BYTES: usize = 10 << 20;
/// Edits of one call.
pub const MAX_EDITS: usize = 200;
/// Characters of one text of an edit (a `find`, a replacement, a cell's text).
pub const MAX_EDIT_TEXT_CHARS: usize = 20_000;
/// The Markdown of one insert, in bytes.
pub const MAX_EDIT_MARKDOWN_BYTES: usize = 100_000;
/// Cells of one `set` or rows of one `append_rows`.
pub const MAX_EDIT_CELLS: usize = 10_000;
/// Paragraphs a document may have to be edited (each is indexed).
pub const MAX_EDIT_PARAS: usize = 200_000;
/// One paragraph's own XML, in bytes, for an edit inside it.
pub const MAX_EDIT_PARA_BYTES: usize = 1 << 20;
/// XML events read from one part.
pub const MAX_EDIT_EVENTS: usize = 4_000_000;
/// The view of a document, in bytes (the model reads at most this at once).
pub const MAX_VIEW_BYTES: usize = 30_000;
/// Items (paragraphs or rows) one view page shows at most.
pub const MAX_VIEW_ITEMS: usize = 400;
/// Wall time of one edit or view in the helper.
pub const EDIT_SECONDS: u64 = 15;
