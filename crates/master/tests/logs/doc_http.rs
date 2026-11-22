//! Nothing about an office document is logged (X.55, T96): not its name, not
//! its bytes, not its text, not what the reader said when it failed.

use harness_core::Secret;
use harness_master::auth::Auth;
use harness_master::pdf::PdfReader;
use harness_master::{Master, router};
use harness_store::Store;

use crate::capture::logs;

const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

async fn start(script: &str) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let master = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false),
    );
    master.set_pdf_reader(PdfReader::with_command(
        "/bin/sh".into(),
        vec!["-c".into(), script.into()],
    ));
    tokio::spawn(async move { axum::serve(listener, router(master)).await.unwrap() });
    format!("http://{addr}")
}

async fn upload(base: &str, name: &str, body: Vec<u8>) -> u16 {
    reqwest::Client::new()
        .post(format!("{base}/api/attachments?name={name}"))
        .bearer_auth("tok")
        .body(body)
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

#[tokio::test]
async fn a_document_leaves_no_trace_in_the_logs() {
    let buf = logs();
    // Read: the reader's text carries a marker.
    let ok = start(&format!(
        "cat >/dev/null; printf '{DOCX}\\nDOCTEXT-MARKER-7731\\n'"
    ))
    .await;
    let mut zip = harness_attached::testkit::zip(&[("a", b"x")]);
    zip.extend_from_slice(b"DOCBYTES-MARKER-5512");
    assert_eq!(
        upload(&ok, "DOCNAME-MARKER-9981.docx", zip.clone()).await,
        200
    );
    // Failed: the reader's own words carry another.
    let bad = start("cat >/dev/null; printf 'DOCFAIL-MARKER-3340'; exit 3").await;
    assert_eq!(
        upload(&bad, "DOCNAME-MARKER-9981.docx", zip.clone()).await,
        422
    );
    // Killed on its timeout (the helper never answers) and over its output cap.
    let sleepy = start("sleep 0.2; printf 'DOCSLOW-MARKER-1127'").await;
    let _ = upload(&sleepy, "DOCNAME-MARKER-9981.docx", zip).await;
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).to_string();
    for marker in [
        "DOCTEXT-MARKER-7731",
        "DOCBYTES-MARKER-5512",
        "DOCNAME-MARKER-9981",
        "DOCFAIL-MARKER-3340",
        "DOCSLOW-MARKER-1127",
    ] {
        assert!(!log.contains(marker), "{marker} reached the log");
    }
}
