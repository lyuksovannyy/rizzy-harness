//! At most two pages are parsed at once in the process, and a parse keeps its
//! slot until its thread really ends, not until the caller stops waiting
//! (review of PR 179, round 2): abandoned parses cannot pile up. Uses the
//! crate's test hooks to slow a parse down (no hostile page); in a binary of
//! its own, since the slots are process-wide.

use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

use axum::Router;
use axum::response::Html;
use axum::routing::get;
use harness_tools::{Tool, ToolCtx};
use harness_web::WebFetch;
use serde_json::json;

async fn fetch(url: &str) -> (bool, String) {
    let mut c = ToolCtx::new(std::env::temp_dir());
    c.observed = Arc::new(vec![url.to_string()]);
    let o = WebFetch::allowing_private_start()
        .run(&c, json!({"url": url}))
        .await;
    (o.ok, o.output)
}

#[tokio::test(flavor = "multi_thread")]
async fn abandoned_parses_keep_their_slots_until_they_end() {
    let app = Router::new().route(
        "/p",
        get(|| async { Html("<html><body><p>hello</p></body></html>") }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let a = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let url = format!("http://{a}/p");

    // Each parse takes 3 s; a caller waits 200 ms for it and 200 ms for a slot.
    harness_web::TEST_PARSE_DELAY_MS.store(3_000, Relaxed);
    harness_web::TEST_PARSE_WAIT_MS.store(200, Relaxed);
    harness_web::TEST_SLOT_WAIT_MS.store(200, Relaxed);
    let started = Instant::now();
    // Two fetches take the two slots and give up waiting.
    let (one, two) = tokio::join!(fetch(&url), fetch(&url));
    for (ok, out) in [&one, &two] {
        assert!(!ok && out.contains("took too long"), "{out}");
    }
    // Their threads are still parsing: a third has no slot, and says so.
    let (ok, out) = fetch(&url).await;
    assert!(!ok && out.contains("busy reading other pages"), "{out}");
    // Once those threads end, the slots are free again.
    tokio::time::sleep(Duration::from_millis(3_300).saturating_sub(started.elapsed())).await;
    harness_web::TEST_PARSE_WAIT_MS.store(10_000, Relaxed);
    harness_web::TEST_SLOT_WAIT_MS.store(10_000, Relaxed);
    harness_web::TEST_PARSE_DELAY_MS.store(0, Relaxed);
    let (ok, out) = fetch(&url).await;
    assert!(ok && out.contains("hello"), "{out}");
}
