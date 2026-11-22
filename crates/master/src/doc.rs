//! Office documents as text, out of process (ADOPTION_PLAN X.55, PLAN.md
//! T96): Word, Excel, PowerPoint and OpenDocument files, read for the model
//! the way a PDF is (`crate::pdf`).
//!
//! The parsing never runs in the server. The server starts its own binary as
//! `rizzy-harness doc-text` ([`helper_main`]), which is `pdf-text`'s twin: the
//! same Landlock confinement (no filesystem, no TCP), the same address-space
//! cap, the same empty environment, and on the server side the same
//! supervisor ([`crate::pdf::PdfReader`]: slots, wall time, output cap,
//! process-group kill, nothing logged).
//!
//! A file is a document by its parts, never by its name: a zip with a
//! `mimetype` entry naming an OpenDocument type and a `content.xml`, or a
//! `[Content_Types].xml` and the main part of Word (`word/document.xml`),
//! Excel (`xl/workbook.xml`) or PowerPoint (`ppt/presentation.xml`). Anything
//! else, an encrypted file included, is [`crate::pdf::EXIT_NOT_DOCUMENT`]:
//! it stays what it was.
//!
//! Inside the helper: the bounded zip reader of `harness-attached` (nothing is
//! allocated from a size the archive states; everything unpacked from one
//! file stops at 64 MiB or at the deadline) and one XML pull parser that
//! processes no DTD and expands no entity the file defines (`xml.rs`). The
//! output is plain text shaped for a model (`flow.rs`), at most
//! [`crate::pdf::HELPER_CHARS`] characters: the helper stops reading as soon
//! as it has them.

mod flow;
mod odf;
mod ooxml;
mod xml;

use std::time::{Duration, Instant};

use harness_attached::{Package, PartError};

use crate::pdf::{HELPER_CHARS, Verdict, confined_helper};
use flow::Out;

/// Rows of a sheet that are read.
pub const MAX_ROWS: usize = 10_000;
/// Columns of a sheet that are read.
pub const MAX_COLS: usize = 256;
/// How long the helper reads before it gives up itself (the server kills it
/// at [`crate::pdf::TIMEOUT`]).
const DEADLINE: Duration = Duration::from_secs(15);
/// The largest `mimetype` / manifest entry looked at.
const SMALL: usize = 1 << 20;

pub(crate) type Res = Result<(), String>;

/// A kind of office document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Docx,
    Xlsx,
    Pptx,
    Odt,
    Ods,
    Odp,
}

impl Format {
    const ALL: [Format; 6] = [
        Format::Docx,
        Format::Xlsx,
        Format::Pptx,
        Format::Odt,
        Format::Ods,
        Format::Odp,
    ];

    /// The media type, which is also what the attachment records.
    pub fn mime(self) -> &'static str {
        match self {
            Self::Docx => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            Self::Xlsx => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            Self::Pptx => {
                "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            }
            Self::Odt => "application/vnd.oasis.opendocument.text",
            Self::Ods => "application/vnd.oasis.opendocument.spreadsheet",
            Self::Odp => "application/vnd.oasis.opendocument.presentation",
        }
    }

    pub fn from_mime(m: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.mime() == m)
    }
}

/// Whether `bytes` start like a zip (the only thing the server looks at
/// before it asks the helper).
pub fn is_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
}

/// What the files are, by their parts. `Ok(None)`: not a document.
fn classify(pkg: &Package<'_>) -> Option<Format> {
    if pkg.has("mimetype") {
        // OpenDocument: the type is the first entry's content.
        let kind = match pkg.read_small("mimetype", 128) {
            Ok(b) => String::from_utf8_lossy(&b).trim().to_string(),
            Err(_) => return None,
        };
        let format = match kind.as_str() {
            "application/vnd.oasis.opendocument.text" => Format::Odt,
            "application/vnd.oasis.opendocument.spreadsheet" => Format::Ods,
            "application/vnd.oasis.opendocument.presentation" => Format::Odp,
            _ => return None,
        };
        if !pkg.has("content.xml") || odf_encrypted(pkg) {
            return None;
        }
        return Some(format);
    }
    if pkg.has("[Content_Types].xml") {
        for (part, f) in [
            ("word/document.xml", Format::Docx),
            ("xl/workbook.xml", Format::Xlsx),
            ("ppt/presentation.xml", Format::Pptx),
        ] {
            if pkg.has(part) {
                return Some(f);
            }
        }
    }
    None
}

/// An OpenDocument file that encrypts its parts says so in its manifest.
fn odf_encrypted(pkg: &Package<'_>) -> bool {
    match pkg.read_small("META-INF/manifest.xml", SMALL) {
        Ok(b) => String::from_utf8_lossy(&b).contains("encryption-data"),
        // Too large or damaged to look at: not read as a document.
        Err(PartError::Missing) => false,
        Err(_) => true,
    }
}

/// The helper's reading: the document's type on the first line, then its
/// text.
fn read(bytes: &[u8]) -> Verdict {
    let deadline = Instant::now() + DEADLINE;
    let pkg = match Package::open(bytes, deadline) {
        Ok(p) => p,
        // Not a zip it can read: not a document.
        Err(_) => return Verdict::NotDocument,
    };
    let Some(format) = classify(&pkg) else {
        return Verdict::NotDocument;
    };
    let mut out = Out::new(HELPER_CHARS);
    let done = match format {
        Format::Docx => ooxml::docx(&pkg, &mut out),
        Format::Xlsx => ooxml::xlsx(&pkg, &mut out),
        Format::Pptx => ooxml::pptx(&pkg, &mut out),
        Format::Odt => odf::text(&pkg, false, &mut out),
        Format::Odp => odf::text(&pkg, true, &mut out),
        Format::Ods => odf::sheets(&pkg, &mut out),
    };
    match done {
        Err(m) if out.s.trim().is_empty() => {
            if m == ENCRYPTED {
                Verdict::NotDocument
            } else {
                Verdict::Unreadable(m)
            }
        }
        // What was read before a limit hit is still the document's start,
        // and the text says it stops there.
        Err(m) => {
            out.line(&format!("[cut: reading stopped here: {m}]"));
            Verdict::Text(format!("{}\n{}", format.mime(), out.s).into_bytes())
        }
        Ok(()) => Verdict::Text(format!("{}\n{}", format.mime(), out.s).into_bytes()),
    }
}

/// The sentence for an encrypted part, which makes the file no document.
const ENCRYPTED: &str = "a part of the document is encrypted";

/// A failed XML stream, as a sentence for the person.
pub(crate) fn fail(pkg: &Package<'_>, f: xml::Fail) -> String {
    match f {
        xml::Fail::Deep => "the document nests its elements too deeply".into(),
        xml::Fail::Syntax => "the document's XML is damaged".into(),
        xml::Fail::Io(e) => fail_part(pkg, pkg.failed(&e)),
    }
}

/// A part that could not be opened, as a sentence.
pub(crate) fn fail_part(_: &Package<'_>, e: PartError) -> String {
    match e {
        PartError::Missing => "a part of the document is missing".into(),
        PartError::Encrypted => ENCRYPTED.into(),
        PartError::Failed(m) => m,
    }
}

/// `rizzy-harness doc-text`: read an office document from stdin, write its
/// type and its text to stdout. Returns the exit code.
pub fn helper_main() -> i32 {
    confined_helper(read)
}

#[cfg(test)]
mod tests;
