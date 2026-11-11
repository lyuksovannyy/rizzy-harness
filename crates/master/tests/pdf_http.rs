//! FIX_PLAN 3.4 through the HTTP API: PDFs by header only, a stuck PDF
//! reader does not stall the server, unsent uploads are capped, and
//! dictation is one at a time per account.

use std::time::{Duration, Instant};

use harness_core::Secret;
use harness_master::auth::Auth;
use harness_master::pdf::PdfReader;
use harness_master::voice::Transcriber;
use harness_master::{Master, router};
use harness_store::Store;
use serde_json::{Value, json};

async fn start() -> (Master, String) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let master = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false),
    );
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    (master, format!("http://{addr}"))
}

async fn upload(base: &str, name: &str, body: &[u8]) -> (u16, Value) {
    let r = reqwest::Client::new()
        .post(format!("{base}/api/attachments?name={name}"))
        .bearer_auth("tok")
        .body(body.to_vec())
        .send()
        .await
        .unwrap();
    let st = r.status().as_u16();
    (st, r.json().await.unwrap_or(Value::Null))
}

/// A PDF reader that never finishes (the supervision of a looping parser:
/// a real parser-looping PDF is not something we can craft reliably) is
/// killed at its timeout; meanwhile the server answers other requests at
/// once. Without a reader at all, a PDF is refused, never parsed here.
#[tokio::test]
async fn a_stuck_pdf_reader_is_killed_and_the_server_stays_responsive() {
    let (master, base) = start().await;
    let (st, v) = upload(&base, "a.pdf", b"%PDF-1.4\n").await;
    assert_eq!(st, 422, "{v}");
    assert!(
        v["error"].as_str().unwrap().contains("cannot read PDFs"),
        "{v}"
    );

    master.set_pdf_reader(
        PdfReader::with_command(
            "/bin/sh".into(),
            vec!["-c".into(), "while :; do :; done".into()],
        )
        .with_limits(Duration::from_millis(1500), Duration::from_secs(1)),
    );
    let b2 = base.clone();
    let stuck = tokio::spawn(async move { upload(&b2, "a.pdf", b"%PDF-1.4\n").await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let t = Instant::now();
    let r = reqwest::Client::new()
        .get(format!("{base}/api/features"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert!(
        t.elapsed() < Duration::from_millis(500),
        "{:?}",
        t.elapsed()
    );
    let (st, v) = stuck.await.unwrap();
    assert_eq!(st, 422, "{v}");
    assert!(v["error"].as_str().unwrap().contains("too long"), "{v}");
    assert_eq!(
        master.pdf_reader().unwrap().free_slots(),
        harness_master::pdf::CONCURRENT
    );
}

/// The header decides, never the name.
#[tokio::test]
async fn a_pdf_named_text_file_is_text() {
    let (_master, base) = start().await;
    let (st, v) = upload(&base, "notes.pdf", b"plain notes").await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["mime"], "text/plain");
    assert_eq!(v["chars"], 11);
}

#[tokio::test]
async fn unsent_uploads_are_capped() {
    let (_master, base) = start().await;
    for n in 0..harness_master::attachments::MAX_UNSENT {
        let (st, v) = upload(&base, &format!("n{n}.txt"), b"x").await;
        assert_eq!(st, 200, "{v}");
    }
    let (st, v) = upload(&base, "more.txt", b"x").await;
    assert_eq!(st, 429, "{v}");
    assert!(
        v["error"].as_str().unwrap().contains("waiting to be sent"),
        "{v}"
    );
}

/// A second dictation while the first runs is refused at once (429, not
/// queued); the daily cap from config refuses the rest.
#[tokio::test]
async fn dictation_is_one_at_a_time_and_capped_daily() {
    let fake = axum::Router::new().route(
        "/v1/audio/transcriptions",
        axum::routing::post(|| async {
            tokio::time::sleep(Duration::from_millis(600)).await;
            axum::Json(json!({ "text": "ok" }))
        }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fa = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, fake).await.unwrap() });
    let (master, base) = start().await;
    master.set_transcriber(
        Transcriber::new(&format!("http://{fa}/v1"), "", None).with_limits(
            harness_master::voice::Limits {
                daily_requests: 2,
                daily_bytes: 0,
            },
        ),
    );
    let send = |base: String| async move {
        let r = reqwest::Client::new()
            .post(format!("{base}/api/transcribe"))
            .bearer_auth("tok")
            .header("content-type", "audio/webm")
            .body(vec![1u8; 100])
            .send()
            .await
            .unwrap();
        let st = r.status().as_u16();
        (st, r.json::<Value>().await.unwrap_or(Value::Null))
    };
    let first = tokio::spawn(send(base.clone()));
    tokio::time::sleep(Duration::from_millis(200)).await;
    let t = Instant::now();
    let (st, v) = send(base.clone()).await;
    assert_eq!(st, 429, "{v}");
    assert!(
        v["error"]
            .as_str()
            .unwrap()
            .contains("one dictation at a time")
    );
    // Answered while the first still runs (it takes 600 ms): not queued.
    assert!(!first.is_finished(), "{:?}", t.elapsed());
    assert_eq!(first.await.unwrap().0, 200);
    assert_eq!(send(base.clone()).await.0, 200);
    let (st, v) = send(base.clone()).await;
    assert_eq!(st, 429, "{v}");
    assert!(v["error"].as_str().unwrap().contains("daily"), "{v}");
}
