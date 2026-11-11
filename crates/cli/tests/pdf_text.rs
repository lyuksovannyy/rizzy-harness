//! The real `rizzy-harness pdf-text` helper (FIX_PLAN 3.4): it reads PDFs,
//! a malformed one is an error inside the helper, and a decompression bomb
//! dies on the helper's own memory limit instead of growing the server.
//!
//! The helper refuses to parse where it cannot confine itself (no Landlock,
//! or a kernel before ABI 4): these tests then skip with a printed reason,
//! but fail when `CI` is set.

use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use harness_master::pdf::{self, PdfError, PdfReader};

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

/// A one-page PDF whose page content is `content` (with its own dictionary
/// entries, e.g. a filter), xref offsets computed.
fn pdf_with(content_dict: &str, content: &[u8]) -> Vec<u8> {
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    let mut obj = |out: &mut Vec<u8>, body: &[u8]| {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    obj(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>");
    obj(&mut out, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    obj(
        &mut out,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 4 0 R \
          /Resources << /Font << /F1 5 0 R >> >> >>",
    );
    let mut stream =
        format!("<< /Length {}{content_dict} >>\nstream\n", content.len()).into_bytes();
    stream.extend_from_slice(content);
    stream.extend_from_slice(b"\nendstream");
    obj(&mut out, &stream);
    obj(
        &mut out,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
    );
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes(),
    );
    for o in &offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn hello() -> Vec<u8> {
    pdf_with("", b"BT /F1 12 Tf 20 200 Td (Hello from a PDF) Tj ET")
}

/// Run the helper directly: (status, stdout).
fn helper(input: &[u8]) -> (std::process::ExitStatus, Vec<u8>) {
    let mut c = Command::new(EXE)
        .arg(pdf::SUBCOMMAND)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = c.stdin.take().unwrap();
    let input = input.to_vec();
    let w = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let out = c.wait_with_output().unwrap();
    w.join().unwrap();
    (out.status, out.stdout)
}

/// Whether the helper can confine itself here; prints why not.
fn confined() -> bool {
    let (st, _) = helper(&hello());
    if st.code() == Some(pdf::EXIT_UNCONFINED) {
        assert!(
            std::env::var_os("CI").is_none(),
            "pdf-text cannot confine itself on this CI kernel"
        );
        eprintln!("skipped: pdf-text cannot confine itself here (Landlock ABI 4 needed)");
        return false;
    }
    true
}

#[tokio::test]
async fn the_helper_reads_a_pdf() {
    if !confined() {
        return;
    }
    let r = PdfReader::new(EXE.into());
    let text = r.extract(hello()).await.unwrap();
    assert!(text.contains("Hello from a PDF"), "{text:?}");
}

/// A malformed PDF is an error the helper reports, not a crash of anything.
#[tokio::test]
async fn a_malformed_pdf_is_an_error() {
    if !confined() {
        return;
    }
    let r = PdfReader::new(EXE.into());
    let e = r.extract(b"%PDF-1.4 garbage".to_vec()).await.unwrap_err();
    assert!(
        matches!(e, PdfError::Unreadable(_) | PdfError::Crashed),
        "{e:?}"
    );
    // Not a PDF on stdin: refused before parsing.
    let (st, _) = helper(b"hello");
    assert_eq!(st.code(), Some(pdf::EXIT_BAD_INPUT));
}

/// A page whose content inflates to 768 MiB: the helper's address-space
/// limit makes the allocation fail and the helper (not the caller) abort.
#[tokio::test]
async fn a_decompression_bomb_dies_inside_the_helper() {
    if !confined() {
        return;
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    let chunk = vec![b' '; 1 << 20];
    for _ in 0..768 {
        z.write_all(&chunk).unwrap();
    }
    let bomb = pdf_with(" /Filter /FlateDecode", &z.finish().unwrap());
    assert!(bomb.len() < harness_master::attachments::MAX_UPLOAD);
    let t = Instant::now();
    let (st, out) = helper(&bomb);
    assert!(!st.success(), "the bomb was read: {} bytes", out.len());
    assert_eq!(
        st.signal(),
        Some(nix::sys::signal::Signal::SIGABRT as i32),
        "expected an allocation failure, got {st:?}"
    );
    assert!(t.elapsed() < Duration::from_secs(20));
    // Through the reader: a clear error, the slot back.
    let r = PdfReader::new(EXE.into());
    assert_eq!(r.extract(bomb).await, Err(PdfError::Crashed));
    assert_eq!(r.free_slots(), pdf::CONCURRENT);
}
