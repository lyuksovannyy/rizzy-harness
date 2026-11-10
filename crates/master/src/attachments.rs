//! Document attachments: text is extracted on the master at upload time and
//! travels inside the user's message event, so the log stays self-contained.
//!
//! Unsent uploads wait in master.db (`attachments`), at most [`MAX_UNSENT`]
//! and [`MAX_UNSENT_BYTES`] per account, and expire with the incognito TTL
//! (`Master::start_incognito_purge`).

use harness_core::event::Attachment;

/// Largest upload accepted.
pub const MAX_UPLOAD: usize = 10 * 1024 * 1024;
/// Extracted text kept per document (about 25k tokens).
pub const MAX_TEXT_CHARS: usize = 100_000;
/// Attachments per message.
pub const MAX_PER_MESSAGE: usize = 5;

/// Unsent uploads one account may hold (count): four full messages.
pub const MAX_UNSENT: usize = 4 * MAX_PER_MESSAGE;
/// Unsent uploads one account may hold (bytes of extracted text). A full
/// attachment is at most about 400 KB of UTF-8, so the count is the limit
/// ordinary use meets first.
pub const MAX_UNSENT_BYTES: usize = 8 * 1024 * 1024;

/// Why an upload gives no attachment. `Display` is the user's message.
#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    /// Not a file we read (422).
    #[error("{0}")]
    Unsupported(String),
    /// The PDF reader's answer (`PdfError::Busy` is 503, the rest 422).
    #[error("{0}")]
    Pdf(crate::pdf::PdfError),
}

impl ExtractError {
    pub fn busy(&self) -> bool {
        matches!(self, Self::Pdf(crate::pdf::PdfError::Busy))
    }
}

/// Extract text from an upload. A file is a PDF only by its `%PDF-` header,
/// never by its name; PDFs go to the out-of-process reader (`crate::pdf`),
/// and are refused where there is none.
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
    } else {
        let body = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
        if body.contains(&0) {
            return Err(ExtractError::Unsupported(unsupported()));
        }
        let text = String::from_utf8(body.to_vec())
            .map_err(|_| ExtractError::Unsupported(unsupported()))?;
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
    "unsupported file: text, Markdown, code, CSV, JSON and PDF work".into()
}

fn cap(text: String) -> (String, bool) {
    if text.chars().count() <= MAX_TEXT_CHARS {
        return (text, false);
    }
    (text.chars().take(MAX_TEXT_CHARS).collect(), true)
}

/// A display-safe file name: last path segment, no control characters, short.
pub fn clean_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let s: String = base.chars().filter(|c| !c.is_control()).take(120).collect();
    let s = s.trim().to_string();
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
        assert!(extract("x.bin", vec![0, 1, 2], None).await.is_err());
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
}
