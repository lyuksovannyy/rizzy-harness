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
