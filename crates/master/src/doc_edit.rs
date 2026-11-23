//! `rizzy-harness doc-edit`: Word and Excel files edited out of process
//! (ADOPTION_PLAN X.57, PLAN.md T98), `doc-text`'s sibling. The file is
//! someone's, hostile: it is parsed only here, after the same confinement as
//! `pdf-text` and `doc-text` (Landlock with no filesystem and no TCP, an
//! address-space cap, no core, an empty environment, one thread) and under
//! the same supervisor (`crate::pdf::PdfReader::edit`: slots, wall time,
//! output cap, process-group kill). The engine is `harness_docs::edit`.
//!
//! Stdin: a 4-byte length, a JSON request, the file. Stdout: a 4-byte length,
//! a JSON reply, and the new file when every edit applied.

use crate::pdf::{Verdict, confined_helper_cap};

/// What the helper reads from stdin at most: the largest upload and the
/// largest request.
pub const MAX_INPUT: usize =
    crate::attachments::MAX_UPLOAD + harness_docs::edit::serve::MAX_HEADER_BYTES + 4;

/// `rizzy-harness doc-edit`. Returns the exit code.
pub fn helper_main() -> i32 {
    confined_helper_cap(MAX_INPUT, |input| {
        match harness_docs::edit::serve::serve(input) {
            Ok(out) => Verdict::Text(out),
            Err(e) => Verdict::Unreadable(e),
        }
    })
}
