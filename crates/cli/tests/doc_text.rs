//! The real `rizzy-harness doc-text` helper (ADOPTION_PLAN X.55, T96): it
//! reads Word, Excel, PowerPoint and OpenDocument files confined like
//! `pdf-text`, tells a document by its parts, and neither a bomb, a hostile
//! entity nor a file it must not open gets anything out of it.
//!
//! Where the helper cannot confine itself (no Landlock, or a kernel before
//! ABI 4) these tests skip with a printed reason, but fail when `CI` is set.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use harness_attached::testkit::{
    Ent, docx, odf, p, pptx, sheet_xml, xlsx, xlsx_raw, zip, zip_with,
};
use harness_master::pdf::{self, DocError, PdfReader};

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

/// Run the helper directly: (status, stdout).
fn helper(input: &[u8]) -> (std::process::ExitStatus, Vec<u8>) {
    let mut c = Command::new(EXE)
        .arg(pdf::DOC_SUBCOMMAND)
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
    let (st, _) = helper(&docx(&p("x")));
    if st.code() == Some(pdf::EXIT_UNCONFINED) {
        assert!(
            std::env::var_os("CI").is_none(),
            "doc-text cannot confine itself on this CI kernel"
        );
        eprintln!("skipped: doc-text cannot confine itself here (Landlock ABI 4 needed)");
        return false;
    }
    true
}

#[tokio::test]
async fn the_helper_reads_each_kind_of_document() {
    if !confined() {
        return;
    }
    let r = PdfReader::new(EXE.into());
    let (mime, text) = r
        .extract_doc(docx(&p("Hello from Word")))
        .await
        .unwrap()
        .unwrap();
    assert!(mime.ends_with("wordprocessingml.document"), "{mime}");
    assert_eq!(text, "Hello from Word\n");
    let (_, text) = r
        .extract_doc(xlsx(&[("Data", &[&["a", "1"], &["b", "2"]])]))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(text, "## Sheet: Data\na\t1\nb\t2\n");
    let (_, text) = r
        .extract_doc(pptx(&[(&["Title"], Some("notes"))]))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(text, "## Slide 1\nTitle\nNotes:\nnotes\n");
    let (mime, text) = r
        .extract_doc(odf(
            "text",
            "<office:text><text:p>Hi</text:p></office:text>",
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(mime, "application/vnd.oasis.opendocument.text");
    assert_eq!(text, "Hi\n");
    assert_eq!(r.free_slots(), pdf::CONCURRENT);
}

/// By its parts: a plain zip, a non-zip and an encrypted file are no
/// documents, and say nothing more.
#[tokio::test]
async fn what_is_not_a_document_is_not_one() {
    if !confined() {
        return;
    }
    let r = PdfReader::new(EXE.into());
    assert_eq!(r.extract_doc(zip(&[("readme.txt", b"hi")])).await, Ok(None));
    assert_eq!(
        r.extract_doc(b"PK\x03\x04 nothing".to_vec()).await,
        Ok(None)
    );
    assert_eq!(r.extract_doc(b"hello".to_vec()).await, Ok(None));
    let enc = zip_with(
        &[
            Ent {
                name: b"[Content_Types].xml",
                data: b"<Types/>",
                deflate: true,
            },
            Ent {
                name: b"word/document.xml",
                data: b"<w:document/>",
                deflate: true,
            },
        ],
        None,
        true,
    );
    assert_eq!(r.extract_doc(enc).await, Ok(None));
}

/// An external entity pointing at a real file is never read: the parser has
/// no entity table, and the process may open no file at all (Landlock).
#[tokio::test]
async fn an_external_entity_reads_nothing_of_the_machine() {
    if !confined() {
        return;
    }
    let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
    let first = passwd.lines().next().unwrap_or("root").to_string();
    let doc = "<?xml version=\"1.0\"?><!DOCTYPE w:document [<!ENTITY xxe SYSTEM \"file:///etc/passwd\">\
         <!ENTITY pe PUBLIC \"x\" \"file:///proc/self/environ\">]>\
         <w:document xmlns:w=\"w\"><w:body><w:p><w:r><w:t>a&xxe;b&pe;c</w:t></w:r></w:p></w:body></w:document>";
    let z = zip(&[
        ("[Content_Types].xml", b"<Types/>"),
        ("word/document.xml", doc.as_bytes()),
    ]);
    let (st, out) = helper(&z);
    assert!(st.success(), "{st:?}");
    let out = String::from_utf8(out).unwrap();
    assert!(out.ends_with("\nabc\n"), "{out:?}");
    assert!(!out.contains(&first), "{out:?}");
}

/// 200 MiB of spaces inside a docx, 200 KB of file: stopped at the
/// package's ceiling with a sentence, in bounded memory, never a crash and
/// never a long wait.
#[tokio::test]
async fn a_zip_bomb_inside_a_docx_is_stopped() {
    if !confined() {
        return;
    }
    let mut xml = String::from("<w:document xmlns:w=\"w\"><w:body><w:p><w:r><w:t>");
    xml.push_str(&" ".repeat(200 << 20));
    xml.push_str("</w:t></w:r></w:p></w:body></w:document>");
    let bomb = zip(&[
        ("[Content_Types].xml", b"<Types/>"),
        ("word/document.xml", xml.as_bytes()),
    ]);
    drop(xml);
    assert!(bomb.len() < 1 << 20);
    let r = PdfReader::new(EXE.into());
    let t = Instant::now();
    let e = r.extract_doc(bomb).await.unwrap_err();
    assert!(
        matches!(&e, DocError::Unreadable(m) if m.contains("MiB")),
        "{e:?}"
    );
    assert!(t.elapsed() < Duration::from_secs(20), "{:?}", t.elapsed());
    assert_eq!(r.free_slots(), pdf::CONCURRENT);
}

/// A single text node of hundreds of MiB (no tag in it to stop the parser at)
/// cannot grow the helper past its address-space limit: it ends as a
/// documented failure of the helper, not of the server.
#[tokio::test]
async fn a_huge_text_node_cannot_grow_the_helper() {
    if !confined() {
        return;
    }
    let mut xml = String::from("<w:document xmlns:w=\"w\"><w:body><w:p><w:r><w:t>");
    xml.push_str(&"a".repeat(60 << 20));
    xml.push_str("</w:t></w:r></w:p></w:body></w:document>");
    let z = zip(&[
        ("[Content_Types].xml", b"<Types/>"),
        ("word/document.xml", xml.as_bytes()),
    ]);
    drop(xml);
    let r = PdfReader::new(EXE.into());
    let t = Instant::now();
    let got = r.extract_doc(z).await;
    // Either read (capped) or refused: never a hang, never a killed server.
    assert!(
        matches!(
            got,
            Ok(Some(_)) | Err(DocError::Unreadable(_) | DocError::Crashed)
        ),
        "{got:?}"
    );
    assert!(t.elapsed() < Duration::from_secs(20));
}

#[tokio::test]
async fn deeply_nested_xml_and_a_sheet_of_a_million_rows_are_bounded() {
    if !confined() {
        return;
    }
    let r = PdfReader::new(EXE.into());
    let deep = docx(&format!(
        "{}x{}",
        "<w:p>".repeat(10_000),
        "</w:p>".repeat(10_000)
    ));
    let e = r.extract_doc(deep).await.unwrap_err();
    assert!(
        matches!(&e, DocError::Unreadable(m) if m.contains("too deeply")),
        "{e:?}"
    );
    let mut rows = String::new();
    for n in 1..=1_000_000u32 {
        rows.push_str(&format!(
            "<row r=\"{n}\"><c r=\"A{n}\"><v>{n}</v></c></row>"
        ));
    }
    let big = xlsx_raw(
        vec!["Big"],
        &[],
        vec![("xl/worksheets/sheet1.xml".into(), sheet_xml(&rows))],
    );
    let (_, text) = r.extract_doc(big).await.unwrap().unwrap();
    assert!(text.contains("[cut: more rows follow"), "{}", text.len());
    assert!(text.lines().count() < 10_100);
}
