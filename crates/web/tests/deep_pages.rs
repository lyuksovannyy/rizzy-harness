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

/// The tests share the process's two parse slots and a debug build parses
/// slowly: one at a time, so that none waits for a slot another test holds.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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

/// Documents that hide their depth from any reading of the markup but the
/// real tree builder's: each class is a reduced reproducer of how a
/// hand-written scan disagreed with html5ever (review of PR 179, round 2).
/// `n` repeats the unit; the tree is thousands of levels deep at `n` = 3,000.
fn classes(n: usize) -> Vec<(&'static str, String)> {
    let r = |pre: &str, unit: &str| format!("<html><body>{pre}{}x</body></html>", unit.repeat(n));
    vec![
        ("plain divs", r("", "<div>")),
        (
            "noscript is markup with scripting off",
            r("<noscript>", "<div>"),
        ),
        (
            "foreignObject is an integration point",
            r("<svg><foreignObject>", "<section/>"),
        ),
        (
            "math mi is an integration point",
            r("<math><mi>", "<article/>"),
        ),
        ("math mtext", r("<math><mtext>", "<section/>")),
        ("svg desc", r("<svg><desc>", "<span/>")),
        (
            "foreignObject self-closing",
            r("<svg><foreignObject>", "<div/>"),
        ),
        ("optgroup nests", r("", "<optgroup>")),
        ("tbody in math nests", r("<math>", "<tbody>")),
        ("td in svg nests", r("<svg>", "<td>")),
        ("option in svg nests", r("<svg>", "<option>")),
        (
            "raw-text elements ignored in some modes",
            format!("<frameset><xmp>{}", "<frameset>".repeat(n)),
        ),
        (
            "frameset iframe",
            format!("<frameset><iframe>{}", "<frameset>".repeat(n)),
        ),
        (
            "font breaks out only with attributes",
            r("<svg><font><style>", "<g>"),
        ),
        ("math font iframe", r("<math><font><iframe>", "<mrow>")),
        ("close svg truncating too far", r("", "<svg><div></svg>")),
        ("math ul", r("", "<math><ul><ul></math>")),
        ("table td nest", r("", "<table><tr><td>")),
        (
            "closer hidden in an attribute value",
            r("", "<div><br t=\"> </div> \">"),
        ),
        ("closer hidden in a comment", r("", "<div><!-- </div> -->")),
        (
            "closer hidden in a script",
            r("", "<div><script></div></script>"),
        ),
        (
            "closer hidden in a textarea",
            r("", "<div><textarea></div></textarea>"),
        ),
    ]
}

/// Every class, through the tool's real entry point: refused as nested too
/// deeply, whatever the tag, with and without the readability pass.
#[tokio::test(flavor = "multi_thread")]
async fn a_page_nested_too_deeply_is_refused() {
    let _one_at_a_time = SERIAL.lock().await;
    for (what, html) in classes(3_000) {
        for readable in [false, true] {
            let (ok, out, took) = fetch(html.clone(), readable).await;
            assert!(!ok && out.contains("nested too deeply"), "{what}: {out}");
            assert!(took < Duration::from_secs(60), "{what}: {took:?}");
        }
    }
}

/// The same classes at a megabyte: more start tags than the tree builder is
/// given, so they are read as plain text, quickly, with no tree at all.
#[tokio::test(flavor = "multi_thread")]
async fn the_same_pages_at_a_megabyte_are_read_as_plain_text() {
    let _one_at_a_time = SERIAL.lock().await;
    for (what, html) in classes(1024 * 1024 / 40).into_iter().step_by(3) {
        let (ok, out, took) = fetch(html, true).await;
        assert!(ok, "{what}: {out}");
        assert!(took < Duration::from_secs(20), "{what}: {took:?}");
    }
}

/// Pages inside the limits are read: at the deepest nesting allowed, with
/// the readability pass and without, and many other shapes at the byte
/// caps. None may take long or overflow the stack.
#[tokio::test(flavor = "multi_thread")]
async fn pages_inside_the_caps_are_read_quickly() {
    let _one_at_a_time = SERIAL.lock().await;
    let max = 500usize;
    let big = 1024 * 1024usize;
    let cases: Vec<(&str, String)> = vec![
        ("deepest allowed div", page("<div>", max)),
        (
            "deepest allowed div, 24k other start tags",
            format!(
                "<html><body>{}{}x</body></html>",
                "<div>".repeat(max),
                "<i>w</i>".repeat(12_000)
            ),
        ),
        (
            "deepest allowed table",
            page("<table><tr><td>", max / 4 - 1),
        ),
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

/// The worst the tree builder is ever given: as many start tags as the cap
/// allows, nested as deep as its stack walks hurt most. It parses (about
/// 0.7 s in release) and is then refused as too deep, never read.
#[tokio::test(flavor = "multi_thread")]
async fn the_worst_pages_the_tree_builder_is_given_are_refused_in_time() {
    let _one_at_a_time = SERIAL.lock().await;
    // A debug build parses about thirty times slower and would overrun the
    // 15 s backstop at the cap: it takes a quarter of the cap; the release
    // run is the one that measures the cap itself.
    let n = if cfg!(debug_assertions) {
        6_000
    } else {
        24_900
    };
    for open in ["<ul>", "<div>", "<nav>"] {
        let (ok, out, took) = fetch(page(open, n), true).await;
        assert!(!ok && out.contains("nested too deeply"), "{open}: {out}");
        println!("TIMING cap {open}: {took:?}");
        assert!(took < Duration::from_secs(60), "{open}: {took:?}");
    }
}
