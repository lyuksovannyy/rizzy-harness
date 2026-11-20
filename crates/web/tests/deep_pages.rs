//! A hostile page must not pin a CPU or overflow a stack (review of PR 179,
//! H2): pages nested past the limit are refused before any parser runs, and
//! every page the caps let through is read in well under a second (release)
//! without recursing past its thread's stack (debug too).
//! Loopback fake only; the pages are built here.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::response::Html;
use axum::routing::get;
use harness_tools::{Tool, ToolCtx};
use harness_web::WebFetch;
use serde_json::json;

async fn fetch(html: String, readable: bool) -> (bool, String, Duration) {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let a = l.local_addr().unwrap();
    let app = Router::new().route(
        "/p",
        get(move || {
            let b = html.clone();
            async move { Html(b) }
        }),
    );
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let u = format!("http://{a}/p");
    let mut c = ToolCtx::new(std::env::temp_dir());
    c.observed = Arc::new(vec![u.clone()]);
    c.savings.web_readability = readable;
    let t = Instant::now();
    let o = WebFetch::allowing_private_start()
        .run(&c, json!({"url": u}))
        .await;
    (o.ok, o.output, t.elapsed())
}

fn page(open: &str, n: usize) -> String {
    format!("<html><body>{}text</body></html>", open.repeat(n))
}

/// Deep nesting is refused with a plain sentence, whatever the tag, for the
/// all-text path and the readability path; the time is generous (assert on
/// the refusal, not the speed).
#[tokio::test(flavor = "multi_thread")]
async fn a_page_nested_too_deeply_is_refused() {
    for open in [
        "<div>",
        "<span>",
        "<b>",
        "<blockquote>",
        "<ul><li>",
        "<table><tr><td>",
    ] {
        for readable in [false, true] {
            let (ok, out, took) = fetch(page(open, 20_000), readable).await;
            assert!(!ok && out.contains("nested too deeply"), "{open}: {out}");
            assert!(took < Duration::from_secs(10), "{open}: {took:?}");
        }
    }
    // Depth hidden behind an attribute value that holds a `>` and a closer.
    let hidden = format!(
        "<html><body>{}x</body></html>",
        "<div><br t=\"> </div> \">".repeat(20_000)
    );
    let (ok, out, _) = fetch(hidden, false).await;
    assert!(!ok && out.contains("nested too deeply"), "{out}");
}

/// Pages inside the limits are read: at the deepest nesting allowed, with
/// the readability pass and without, and many other shapes at the byte
/// caps. None may take long or overflow the stack.
#[tokio::test(flavor = "multi_thread")]
async fn pages_inside_the_caps_are_read_quickly() {
    let max = 500usize;
    let big = 1024 * 1024usize;
    let cases: Vec<(&str, String)> = vec![
        ("deepest allowed div", page("<div>", max)),
        ("deepest allowed table", page("<table><tr><td>", max / 3)),
        ("deepest allowed blockquote", page("<blockquote>", max)),
        (
            "an article at the readability cap",
            format!(
                "<html><body><article>{}</article></body></html>",
                "<div><p>Some real words in a paragraph here.</p></div>".repeat(5_000)
            ),
        ),
        (
            "a million-ish siblings",
            format!("<html><body>{}</body></html>", "<b></b>".repeat(big / 7)),
        ),
        (
            "many paragraphs, unclosed",
            format!("<html><body>{}</body></html>", "<p>word ".repeat(big / 8)),
        ),
        (
            "a huge attribute",
            format!(
                "<html><body><div a=\"{}\">hi</div></body></html>",
                "a".repeat(big)
            ),
        ),
        (
            "a huge text node",
            format!(
                "<html><body><p>{}</p></body></html>",
                "word ".repeat(big / 5)
            ),
        ),
        (
            "unclosed attribute",
            format!("<html><body><div a=\"{}", "x".repeat(big)),
        ),
        (
            "many comments",
            format!(
                "<html><body>{}ok</body></html>",
                "<!-- c -->".repeat(big / 10)
            ),
        ),
        (
            "many tags no text",
            format!(
                "<html><body>{}ok</body></html>",
                "<i></i><a href=x></a>".repeat(big / 22)
            ),
        ),
        (
            "two megabytes of divs, closed",
            format!(
                "<html><body>{}</body></html>",
                "<div>x</div>".repeat(2 * big / 12)
            ),
        ),
    ];
    for (what, html) in cases {
        for readable in [false, true] {
            let (ok, out, took) = fetch(html.clone(), readable).await;
            assert!(ok, "{what} (readable={readable}): {out}");
            assert!(took < Duration::from_secs(20), "{what}: {took:?}");
            println!("TIMING {what} readable={readable}: {took:?}");
        }
    }
}
