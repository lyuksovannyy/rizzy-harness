//! Document attachments: text is extracted on the master at upload time and
//! travels inside the user's message event, so the log stays self-contained.
//!
//! Unsent uploads wait in master.db (`attachments`), at most [`MAX_UNSENT`]
//! and [`MAX_UNSENT_BYTES`] per account, and expire with the incognito TTL
//! (`Master::start_incognito_purge`).

use harness_core::event::Attachment;

use crate::pdf::DocError;

/// Largest upload accepted.
pub const MAX_UPLOAD: usize = 10 * 1024 * 1024;
/// Extracted text kept per document (about 25k tokens).
pub const MAX_TEXT_CHARS: usize = 100_000;
/// Attachments per message.
pub const MAX_PER_MESSAGE: usize = 10;

/// Unsent uploads one account may hold (count): four full messages.
pub const MAX_UNSENT: usize = 2 * MAX_PER_MESSAGE;
/// Unsent uploads one account may hold (bytes of extracted text). A full
/// attachment is at most about 400 KB of UTF-8, so the count is the limit
/// ordinary use meets first.
pub const MAX_UNSENT_BYTES: usize = 8 * 1024 * 1024;

/// Why an upload gives no attachment. `Display` is the user's message.
#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    /// Empty (422).
    #[error("{0}")]
    Unsupported(String),
    /// Not text and not a PDF (422): a coding chat may save it into the
    /// chat's folder instead (PLAN.md T94, `crate::uploads`).
    #[error("{}", unsupported())]
    Binary,
    /// The PDF reader's answer (`PdfError::Busy` is 503, the rest 422).
    #[error("{0}")]
    Pdf(crate::pdf::PdfError),
    /// An office document the reader found and could not read (503 when
    /// busy, the rest 422): kept as a plain file where files are kept, with
    /// this sentence (X.55, T96).
    #[error("{0}")]
    Doc(crate::pdf::DocError),
    /// The reader could not say whether a zip is a document (busy, timed
    /// out, crashed, could not confine itself): not worth a sentence about
    /// documents. Kept as a plain file where files are kept, as before
    /// documents were read (503 when busy and nothing is kept).
    #[error("{0}")]
    Unsure(crate::pdf::DocError),
}

impl ExtractError {
    pub fn busy(&self) -> bool {
        matches!(
            self,
            Self::Pdf(crate::pdf::PdfError::Busy) | Self::Unsure(crate::pdf::DocError::Busy)
        )
    }
}

/// Whether `mime` is one an office document is read as (`crate::doc`).
pub fn is_document_mime(mime: &str) -> bool {
    crate::doc::Format::from_mime(mime).is_some()
}

/// The id a document's kept file is stored under, next to its text
/// attachment `text_id`: the message that names the text takes the file too
/// (X.55). Derived, never stored: no schema, and an id is only ever one an
/// upload of the same person made.
pub fn file_id_of(text_id: &str) -> String {
    use sha2::{Digest, Sha256};
    // The node takes an upload id only as 32 lowercase hex characters.
    let h = Sha256::digest(format!("document-file:{text_id}"));
    hex::encode(&h[..16])
}

/// Extract text from an upload. A file is a PDF only by its `%PDF-` header,
/// never by its name; PDFs go to the out-of-process reader (`crate::pdf`),
/// and are refused where there is none. A zip goes to the same reader's
/// document mode (`crate::doc`): it comes back as text when its parts say it
/// is a Word, Excel, PowerPoint or OpenDocument file, and as a plain binary
/// file when they do not.
pub async fn extract(
    name: &str,
    bytes: Vec<u8>,
    pdf: Option<&crate::pdf::PdfReader>,
) -> Result<Attachment, ExtractError> {
    let name = clean_name(name);
    if bytes.is_empty() {
        return Err(ExtractError::Unsupported("the file is empty".into()));
    }
    let (mime, text) = if crate::pdf::is_pdf(&bytes) {
        let reader = pdf.ok_or(ExtractError::Pdf(crate::pdf::PdfError::NotSetUp))?;
        let text = reader.extract(bytes).await.map_err(ExtractError::Pdf)?;
        ("application/pdf".to_string(), text)
    } else if let (true, Some(reader)) = (crate::doc::is_zip(&bytes), pdf) {
        match reader.extract_doc(bytes).await {
            Ok(Some((mime, text))) => (mime, text),
            // Not a document by its parts, or encrypted: whatever it was.
            Ok(None) => return Err(ExtractError::Binary),
            // Only what the reader positively found to be a document gets a
            // document's sentence.
            Err(e @ (DocError::Unreadable(_) | DocError::NoText)) => {
                return Err(ExtractError::Doc(e));
            }
            Err(e) => return Err(ExtractError::Unsure(e)),
        }
    } else {
        let body = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
        if body.contains(&0) {
            return Err(ExtractError::Binary);
        }
        let text = String::from_utf8(body.to_vec()).map_err(|_| ExtractError::Binary)?;
        // The name only picks a text type; `.pdf` is not one.
        let mime = mime_guess::from_path(&name)
            .first()
            .map(|m| m.essence_str().to_string())
            .filter(|m| m.starts_with("text/") || m.contains("json") || m.contains("xml"))
            .unwrap_or_else(|| "text/plain".into());
        (mime, text)
    };
    let (text, truncated) = cap(text);
    Ok(Attachment {
        name,
        mime,
        text,
        truncated,
    })
}

fn unsupported() -> String {
    "unsupported file: text, Markdown, code, CSV, JSON, PDF and Office documents (Word, Excel, PowerPoint, OpenDocument) work".into()
}

/// Why a chat without tools takes no other file (T94): said at attach
/// time, in the words of the plan.
pub const NO_TOOLS: &str = "unsupported file: this chat's agent cannot open files; switch to \
     Agentic (next to Ask under the box) to attach any file, or attach text, a PDF, an Office document or an image";

fn cap(text: String) -> (String, bool) {
    if text.chars().count() <= MAX_TEXT_CHARS {
        return (text, false);
    }
    (text.chars().take(MAX_TEXT_CHARS).collect(), true)
}

/// A display-safe file name: last path segment, no control characters, short.
pub fn clean_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    // Hidden characters (a right-to-left override or isolate would show the
    // person's own file name reversed) and controls go, as the node does.
    let s = harness_core::text::clean_line(base, 120);
    if s.is_empty() { "document".into() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::pdf::{PdfError, PdfReader};

    #[tokio::test]
    async fn text_files_and_rejections() {
        let a = extract(
            "../../etc/notes.md",
            b"\xEF\xBB\xBF# Title\nbody".to_vec(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(a.name, "notes.md");
        assert_eq!(a.mime, "text/markdown");
        assert_eq!(a.text, "# Title\nbody");
        assert!(!a.truncated);
        let a = extract("data.json", b"{\"a\":1}".to_vec(), None)
            .await
            .unwrap();
        assert_eq!(a.mime, "application/json");
        let a = extract("main.rs", b"fn main() {}".to_vec(), None)
            .await
            .unwrap();
        assert_eq!(a.text, "fn main() {}");
        assert!(matches!(
            extract("x.bin", vec![0, 1, 2], None).await,
            Err(ExtractError::Binary)
        ));
        assert!(
            extract("x.txt", vec![0xff, 0xfe, 0x41], None)
                .await
                .is_err()
        );
        assert!(extract("empty.txt", vec![], None).await.is_err());
        let big = "a".repeat(MAX_TEXT_CHARS + 10);
        let a = extract("big.txt", big.into_bytes(), None).await.unwrap();
        assert!(a.truncated && a.text.len() == MAX_TEXT_CHARS);
    }

    /// A name shows as one line of the person's own: no bidi override or
    /// isolate, no zero-width character, no control, one segment.
    #[test]
    fn a_name_has_no_hidden_or_bidi_characters() {
        assert_eq!(clean_name("a\u{202e}fdp.exe"), "afdp.exe");
        assert_eq!(clean_name("x\u{2066}y\u{2069}\u{200b}.zip"), "xy.zip");
        assert_eq!(clean_name("..\\..\\a\u{202d}b\n.txt"), "ab .txt");
        assert_eq!(clean_name("\u{202e}"), "document");
    }

    #[tokio::test]
    async fn a_pdf_named_file_with_text_is_text() {
        let a = extract("notes.pdf", b"just some notes\n".to_vec(), None)
            .await
            .unwrap();
        assert_eq!(a.text, "just some notes\n");
        assert_eq!(a.mime, "text/plain");
    }

    /// The header decides: a PDF named `.txt` goes to the PDF reader, and
    /// without one it is refused, never parsed in the server.
    #[tokio::test]
    async fn a_pdf_named_txt_is_a_pdf() {
        let e = extract("x.txt", b"%PDF-1.4 garbage".to_vec(), None).await;
        assert!(
            matches!(e, Err(ExtractError::Pdf(PdfError::NotSetUp))),
            "{e:?}"
        );
        let r = PdfReader::with_command(
            "/bin/sh".into(),
            vec!["-c".into(), "cat >/dev/null; printf 'page text'".into()],
        );
        let a = extract("x.txt", b"%PDF-1.4 ...".to_vec(), Some(&r))
            .await
            .unwrap();
        assert_eq!(
            (a.mime.as_str(), a.text.as_str()),
            ("application/pdf", "page text")
        );
    }

    const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

    /// A zip goes to the document reader whatever it is called, and comes
    /// back as text only when the reader says it is a document.
    #[tokio::test]
    async fn a_zip_is_a_document_by_the_readers_word_not_by_its_name() {
        let doc = PdfReader::with_command(
            "/bin/sh".into(),
            vec![
                "-c".into(),
                format!("cat >/dev/null; printf '{DOCX}\\nthe words'").into(),
            ],
        );
        let zip = harness_attached::testkit::zip(&[("a", b"x")]);
        for name in ["report.docx", "report.bin", "../../etc/x.txt"] {
            let a = extract(name, zip.clone(), Some(&doc)).await.unwrap();
            assert_eq!(
                (a.mime.as_str(), a.text.as_str()),
                (DOCX, "the words"),
                "{name}"
            );
            assert!(is_document_mime(&a.mime));
        }
        let none = PdfReader::with_command(
            "/bin/sh".into(),
            vec!["-c".into(), "cat >/dev/null; exit 5".into()],
        );
        let e = extract("report.docx", zip.clone(), Some(&none)).await;
        assert!(matches!(e, Err(ExtractError::Binary)), "{e:?}");
        // Without a reader a zip is a binary file, never parsed here.
        let e = extract("report.docx", zip, None).await;
        assert!(matches!(e, Err(ExtractError::Binary)), "{e:?}");
        let bad = PdfReader::with_command(
            "/bin/sh".into(),
            vec![
                "-c".into(),
                "cat >/dev/null; printf 'broken'; exit 3".into(),
            ],
        );
        let e = extract(
            "a.docx",
            harness_attached::testkit::zip(&[("a", b"x")]),
            Some(&bad),
        )
        .await;
        assert!(
            matches!(e, Err(ExtractError::Doc(DocError::Unreadable(_)))),
            "{e:?}"
        );
        assert!(!is_document_mime("application/zip"));
        // Derived, the shape the node takes (32 lowercase hex), one per text id.
        let id = file_id_of("abc");
        assert!(id.len() == 32 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')));
        assert_eq!(id, file_id_of("abc"));
        assert_ne!(id, file_id_of("abd"));
    }
}
