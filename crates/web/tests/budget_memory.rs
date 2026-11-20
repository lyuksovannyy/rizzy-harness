//! The tree builder can make far more nodes than a page has tags: text that
//! follows closed blocks re-opens every formatting element still listed, and
//! attributes that differ defeat its limit of three alike (review of PR 179,
//! round 3). `<div>` x M, then `<b a=i>` for i in 0..N, then `</div>x` x M
//! makes N x M elements from N + M tags: 156 million at the caps, tens of GB.
//!
//! Every shape of it, at a size inside the byte and start-tag caps, through
//! the tool's real entry point: refused, within a time bound, and the
//! process's peak memory (`VmHWM`) grows by less than 64 MB. The only test in
//! this file: the peak is the whole process's.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::response::Html;
use axum::routing::get;
use harness_tools::{Tool, ToolCtx};
use harness_web::WebFetch;
use serde_json::json;

fn hwm_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
        .unwrap()
}

fn attrs(tag: &str, attr: &str, n: usize) -> String {
    (0..n).map(|i| format!("<{tag} {attr}={i}>")).collect()
}

/// (name, page, the word the refusal must contain)
fn pages() -> Vec<(&'static str, String, &'static str)> {
    let mut v: Vec<(&'static str, String, &'static str)> = Vec::new();
    // The reviewer's page, N = M = 12,500 (25,000 start tags + the end ones):
    // deep, so it is stopped on depth inside the tree builder.
    for (name, tag, attr) in [
        ("div / b a", "b", "a"),
        ("div / a href", "a", "href"),
        ("div / font color", "font", "color"),
        ("div / i id", "i", "id"),
    ] {
        v.push((
            name,
            format!(
                "{}{}{}",
                "<div>".repeat(12_000),
                attrs(tag, attr, 12_000),
                "</div>x".repeat(12_000)
            ),
            "nested too deeply",
        ));
    }
    // Under the nesting limit: 500 formatting elements re-opened by each of
    // 24,000 paragraphs, no end tag anywhere (12 million nodes).
    for (name, tag, attr) in [
        ("p / b a", "b", "a"),
        ("p / font color", "font", "color"),
        ("p / i id", "i", "id"),
    ] {
        v.push((
            name,
            format!("<p>{}{}", attrs(tag, attr, 500), "<p>x".repeat(24_000)),
            "too many elements",
        ));
    }
    // `<a>` inside `<a>` is closed by the tree builder (the adoption agency):
    // no blow-up, so this page is read, as text (72,000 nodes).
    v.push((
        "p / a href",
        format!("<p>{}{}", attrs("a", "href", 500), "<p>x".repeat(24_000)),
        "",
    ));
    // Foster parenting: formatting elements in a table, text after each row.
    // (A table cell puts a marker in the list of formatting elements, so these
    // do not multiply; they are read, and must cost nothing either way.)
    v.push((
        "table foster b",
        format!(
            "<table>{}{}",
            attrs("b", "a", 500),
            "<tr><td>x".repeat(12_000)
        ),
        "",
    ));
    v.push((
        "table cells b",
        format!(
            "<table><tr><td>{}{}",
            attrs("b", "a", 500),
            "</td></tr><tr><td>x".repeat(11_000)
        ),
        "",
    ));
    // `<a>` in `<a>` (the adoption agency closes the first) and a template:
    // read or refused, at no cost.
    v.push((
        "a inside a",
        format!("{}{}", attrs("a", "href", 500), "<p>x".repeat(24_000)),
        "",
    ));
    v.push((
        "template + b",
        format!(
            "<template>{}{}</template>",
            attrs("b", "a", 400),
            "<p>x".repeat(20_000)
        ),
        "",
    ));
    // Formatting elements listed but popped, then reconstructed all at once.
    v.push((
        "listed and popped",
        format!(
            "{}x",
            "<div><b a=1></div><b a=2><i x=3><u y=4></div>".repeat(5_000)
        ),
        "",
    ));
    v
}

#[tokio::test(flavor = "multi_thread")]
async fn a_page_that_makes_millions_of_elements_is_refused_in_bounded_memory() {
    let all = pages();
    let served: Vec<Arc<String>> = all.iter().map(|(_, p, _)| Arc::new(p.clone())).collect();
    let app = Router::new().route(
        "/p/{i}",
        get(move |axum::extract::Path(i): axum::extract::Path<usize>| {
            let page = served[i].clone();
            async move { Html((*page).clone()) }
        }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let a = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let f = WebFetch::allowing_private_start();
    let before = hwm_kb();
    for (i, (name, page, word)) in all.iter().enumerate() {
        let u = format!("http://{a}/p/{i}");
        let mut c = ToolCtx::new(std::env::temp_dir());
        c.observed = Arc::new(vec![u.clone()]);
        c.savings.web_readability = i % 2 == 0;
        let t = Instant::now();
        let o = f.run(&c, json!({"url": u})).await;
        let took = t.elapsed();
        println!(
            "MEM {name:<20} {:>7} bytes {:>8.1?} ok={} | grown {} kB",
            page.len(),
            took,
            o.ok,
            hwm_kb() - before
        );
        if word.is_empty() {
            // Either way: refused or read, never a failure of another kind.
            assert!(
                o.ok || o.output.contains("nested too deeply") || o.output.contains("too many"),
                "{name}: {}",
                o.output
            );
        } else {
            assert!(!o.ok && o.output.contains(word), "{name}: {}", o.output);
        }
        assert!(took < Duration::from_secs(30), "{name} took {took:?}");
    }
    let grown = hwm_kb() - before;
    assert!(grown < 64 * 1024, "peak memory grew by {grown} kB");
}
