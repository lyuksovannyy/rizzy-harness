//! Editing existing Word and Excel files with their formatting kept
//! (ADOPTION_PLAN X.57, PLAN.md T98). One part of the package is patched by
//! a streaming pass that copies every byte it does not change; every other
//! part is copied as its original compressed bytes. See [`scan`].
//!
//! The input is someone's file, hostile: this library is called only from
//! the confined helper (`rizzy-harness doc-edit`), never in a server's or a
//! node's own process.

pub mod check;
pub mod docx;
pub mod para;
pub mod pkg;
pub mod preview;
pub mod scan;
pub mod serve;
pub mod xlsx;

#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(test)]
mod samples;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_docx;
#[cfg(test)]
mod tests_fix;
#[cfg(test)]
mod tests_fuzz;
#[cfg(test)]
mod tests_preview;
#[cfg(test)]
mod tests_xlsx;

// The refusals that mean "past a limit of the reader" (the preview says so
// in a word of its own, `preview::Why::TooLarge`): one text each, shared by
// the place that refuses and the place that tells them from a damaged file.
pub(crate) const E_EVENTS: &str = "a part of the file has too many elements to be edited";
pub(crate) const E_PARAS: &str = "the document has too many paragraphs to be edited";
pub(crate) const E_PARA_BYTES: &str = "a paragraph of the document is too large to be edited";
pub(crate) const E_TEXT: &str = "the document's text is too large to be edited";
pub(crate) const E_SHEETS: &str = "the workbook has too many sheets to be edited";
pub(crate) const E_STRINGS: &str = "the workbook's strings are too large to be read";
pub(crate) const E_ROWS: &str = "the sheet has too many rows to be edited";
pub(crate) const E_PARTS: &str = "the file has too many parts to be edited";
pub(crate) const LIMIT_REFUSALS: [&str; 8] = [
    E_EVENTS,
    E_PARAS,
    E_PARA_BYTES,
    E_TEXT,
    E_SHEETS,
    E_STRINGS,
    E_ROWS,
    E_PARTS,
];

/// A view cut to [`crate::limits::MAX_VIEW_BYTES`] bytes whatever it holds.
pub(crate) fn cap_view(mut s: String) -> String {
    use crate::limits::MAX_VIEW_BYTES;
    if s.len() > MAX_VIEW_BYTES {
        let mut end = MAX_VIEW_BYTES;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s.push_str("\n[cut: the view is too long; ask for a later page with `from`]\n");
    }
    s
}

/// What a call did: per edit, one sentence (applied, or why not); the file
/// only when every edit applied; notes (warnings) for the person.
#[derive(Debug)]
pub struct Outcome {
    pub results: Vec<std::result::Result<String, String>>,
    pub bytes: Option<Vec<u8>>,
    pub notes: Vec<String>,
}
