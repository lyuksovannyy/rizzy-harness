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
pub mod scan;

#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_docx;
