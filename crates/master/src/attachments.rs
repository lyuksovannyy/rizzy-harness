//! Document attachments: text is extracted on the master at upload time and
//! travels inside the user's message event, so the log stays self-contained.

use harness_core::event::Attachment;

/// Largest upload accepted.
pub const MAX_UPLOAD: usize = 10 * 1024 * 1024;
/// Extracted text kept per document (about 25k tokens).
pub const MAX_TEXT_CHARS: usize = 100_000;
/// Attachments per message.
pub const MAX_PER_MESSAGE: usize = 5;

/// Extract text from an upload. Runs PDF parsing in a blocking task: the
/// parser can panic on malformed files, which becomes an error here.
pub async fn extract(name: &str, bytes: Vec<u8>) -> Result<Attachment, String> {
    let name = clean_name(name);
    if bytes.is_empty() {
        return Err("the file is empty".into());
    }
    let is_pdf = bytes.starts_with(b"%PDF") || name.to_ascii_lowercase().ends_with(".pdf");
    let (mime, text) = if is_pdf {
        let text = tokio::task::spawn_blocking(move || pdf_extract::extract_text_from_mem(&bytes))
            .await
            .map_err(|_| "this PDF could not be read".to_string())?
            .map_err(|e| format!("this PDF could not be read: {e}"))?;
        if text.trim().is_empty() {
            return Err("this PDF has no text layer (scanned images are not supported yet)".into());
        }
        ("application/pdf".to_string(), text)
    } else {
        let body = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
        if body.contains(&0) {
            return Err(unsupported());
        }
        let text = String::from_utf8(body.to_vec()).map_err(|_| unsupported())?;
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

    #[tokio::test]
    async fn text_files_and_rejections() {
        let a = extract("../../etc/notes.md", b"\xEF\xBB\xBF# Title\nbody".to_vec())
            .await
            .unwrap();
        assert_eq!(a.name, "notes.md");
        assert_eq!(a.mime, "text/markdown");
        assert_eq!(a.text, "# Title\nbody");
        assert!(!a.truncated);
        let a = extract("data.json", b"{\"a\":1}".to_vec()).await.unwrap();
        assert_eq!(a.mime, "application/json");
        let a = extract("main.rs", b"fn main() {}".to_vec()).await.unwrap();
        assert_eq!(a.text, "fn main() {}");
        assert!(extract("x.bin", vec![0, 1, 2]).await.is_err());
        assert!(extract("x.txt", vec![0xff, 0xfe, 0x41]).await.is_err());
        assert!(extract("empty.txt", vec![]).await.is_err());
        let big = "a".repeat(MAX_TEXT_CHARS + 10);
        let a = extract("big.txt", big.into_bytes()).await.unwrap();
        assert!(a.truncated && a.text.len() == MAX_TEXT_CHARS);
    }

    #[tokio::test]
    async fn broken_pdf_is_an_error_not_a_crash() {
        let e = extract("x.pdf", b"%PDF-1.4 garbage".to_vec()).await;
        assert!(e.is_err());
    }
}
