//! Nothing about an office document is logged (X.55, T96): not its name, not
//! its bytes, not its text, not what the reader said when it failed, not when
//! it was killed on its timeout or over its output cap, and not on the path
//! that keeps the file (the answer's `note`).

use std::time::Duration;

use harness_core::Secret;
use harness_master::auth::Auth;
use harness_master::pdf::PdfReader;
use harness_master::{Master, router};
use harness_store::Store;

use crate::capture::logs;

const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

/// A server with storage (so a file is kept) and the stand-in `script` for
/// the reader, killed after `timeout`.
async fn start(script: &str, timeout: Duration) -> (String, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let master = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false),
    );
    harness_master::storage::open(&master, dir.path()).unwrap();
    master.set_pdf_reader(
        PdfReader::with_command("/bin/sh".into(), vec!["-c".into(), script.into()])
            .with_limits(timeout, Duration::from_secs(1)),
    );
    tokio::spawn(async move { axum::serve(listener, router(master)).await.unwrap() });
    (format!("http://{addr}"), dir)
}

async fn upload(base: &str, name: &str, body: Vec<u8>) -> (u16, String) {
    let r = reqwest::Client::new()
        .post(format!("{base}/api/attachments?name={name}&agentic=1"))
        .bearer_auth("tok")
        .body(body)
        .send()
        .await
        .unwrap();
    (r.status().as_u16(), r.text().await.unwrap())
}

#[tokio::test]
async fn a_document_leaves_no_trace_in_the_logs() {
    let buf = logs();
    let mut zip = harness_attached::testkit::zip(&[("a", b"x")]);
    zip.extend_from_slice(b"DOCBYTES-MARKER-5512");
    let name = "DOCNAME-MARKER-9981.docx";
    let long = Duration::from_secs(20);
    // Read: the reader's text carries a marker.
    let (ok, _d1) = start(
        &format!("cat >/dev/null; printf '{DOCX}\\nDOCTEXT-MARKER-7731\\n'"),
        long,
    )
    .await;
    let (st, _) = upload(&ok, name, zip.clone()).await;
    assert_eq!(st, 200);
    // Failed: the reader's own words carry another, and the file is kept
    // with a sentence (the path with the note).
    let (bad, _d2) = start("cat >/dev/null; printf 'DOCFAIL-MARKER-3340'; exit 3", long).await;
    let (st, body) = upload(&bad, name, zip.clone()).await;
    assert_eq!(st, 200);
    assert!(
        body.contains("could not be read"),
        "the kept path with its note ran: {body}"
    );
    // Killed on its timeout: it prints its marker only after the limit.
    let (slow, _d3) = start(
        "cat >/dev/null; sleep 5; printf 'DOCSLOW-MARKER-1127'",
        Duration::from_millis(300),
    )
    .await;
    let t = std::time::Instant::now();
    let (st, body) = upload(&slow, name, zip.clone()).await;
    assert!(
        st == 200 && t.elapsed() < Duration::from_secs(4),
        "{st} {body} {:?}",
        t.elapsed()
    );
    // Over the output cap: far more than the cap, with a marker in it.
    let (big, _d4) = start("cat >/dev/null; yes DOCOVER-MARKER-6648", long).await;
    let (st, body) = upload(&big, name, zip.clone()).await;
    assert_eq!(st, 200, "{body}");
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).to_string();
    for marker in [
        "DOCTEXT-MARKER-7731",
        "DOCBYTES-MARKER-5512",
        "DOCNAME-MARKER-9981",
        "DOCFAIL-MARKER-3340",
        "DOCSLOW-MARKER-1127",
        "DOCOVER-MARKER-6648",
    ] {
        assert!(!log.contains(marker), "{marker} reached the log");
    }
}
