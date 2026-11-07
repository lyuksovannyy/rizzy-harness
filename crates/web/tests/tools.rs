//! web_search / web_fetch against local fake servers.

use std::sync::Arc;

use axum::Router;
use axum::response::{Html, IntoResponse, Redirect};
use axum::routing::get;
use harness_core::event::Source;
use harness_core::refs::RefBook;
use harness_tools::{Tool, ToolCtx};
use harness_web::{SearchBackend, WebFetch, WebSearch};
use serde_json::json;

async fn server() -> String {
    let app = Router::new()
        .route(
            "/search",
            get(|| async {
                axum::Json(json!({"results": [
                    {"title": "Tokio docs", "url": "https://tokio.rs/tokio/tutorial", "content": "An async runtime"},
                    {"title": "No url"}
                ]}))
            }),
        )
        .route(
            "/brave",
            get(|headers: axum::http::HeaderMap| async move {
                if headers.get("x-subscription-token").map(|v| v.as_bytes()) != Some(b"bk") {
                    return axum::http::StatusCode::UNAUTHORIZED.into_response();
                }
                axum::Json(json!({"web": {"results": [
                    {"title": "Brave hit", "url": "https://example.com/b", "description": "with <strong>bold</strong> &amp; more"}
                ]}}))
                .into_response()
            }),
        )
        .route(
            "/page",
            get(|| async {
                Html("<html><head><title>T</title><script>var secret=1</script></head><body><h1>Hello page</h1><p>Body text here.</p></body></html>")
            }),
        )
        .route(
            "/article",
            get(|| async {
                let para = "Tokio is an asynchronous runtime for the Rust programming language. \
                    It provides the building blocks needed for writing network applications. ";
                Html(format!(
                    "<html><head><title>Tokio guide</title></head><body>\
                     <nav><a href='/a'>Home</a> <a href='/b'>Blog</a> <a href='/c'>Shop</a> NAVJUNK</nav>\
                     <aside>Subscribe to our newsletter! ASIDEJUNK</aside>\
                     <article><h1>Tokio guide</h1><p>{p}</p><p>{p}</p><h2>Spawning</h2>\
                     <p>Use <code>tokio::spawn</code> to run tasks. {p}</p>\
                     <ul><li>first item</li><li>second item</li></ul></article>\
                     <footer>Copyright FOOTERJUNK</footer></body></html>",
                    p = para
                ))
            }),
        )
        .route("/bin", get(|| async { ([("content-type", "application/octet-stream")], vec![0u8; 10]) }))
        .route("/hop", get(|| async { Redirect::temporary("http://169.254.169.254/latest") }));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    format!("http://{addr}")
}

fn ctx(observed: &[&str]) -> ToolCtx {
    let mut c = ToolCtx::new(std::env::temp_dir());
    c.observed = Arc::new(observed.iter().map(|s| s.to_string()).collect());
    c
}

#[tokio::test]
async fn searxng_and_brave() {
    let base = server().await;
    let s = WebSearch::new(SearchBackend::Searxng {
        base_url: base.clone(),
    });
    let out = s.run(&ctx(&[]), json!({"query": "tokio"})).await;
    assert!(out.ok, "{}", out.output);
    assert_eq!(
        out.output,
        "[S1] Tokio docs\n    https://tokio.rs/tokio/tutorial\n    An async runtime"
    );
    assert_eq!(
        out.sources,
        [Source {
            id: "S1".into(),
            url: "https://tokio.rs/tokio/tutorial".into(),
            title: "Tokio docs".into(),
        }]
    );

    let b = WebSearch::new(SearchBackend::Brave {
        api_key: harness_core::Secret::new("bk".into()),
    })
    .with_brave_url(&format!("{base}/brave"));
    assert!(
        !format!(
            "{:?}",
            SearchBackend::Brave {
                api_key: harness_core::Secret::new("bk".into())
            }
        )
        .contains("bk")
    );
    let out = b.run(&ctx(&[]), json!({"query": "x"})).await;
    assert!(out.ok, "{}", out.output);
    assert!(out.output.contains("with bold & more"), "{}", out.output);
    let bad = WebSearch::new(SearchBackend::Brave {
        api_key: harness_core::Secret::new("wrong".into()),
    })
    .with_brave_url(&format!("{base}/brave"));
    assert!(!bad.run(&ctx(&[]), json!({"query": "x"})).await.ok);
}

/// Labels continue from the session's earlier ones, a page keeps its label
/// across tools, and every label in the output has its source.
#[tokio::test]
async fn labels_are_stable_across_calls() {
    let base = server().await;
    let s = WebSearch::new(SearchBackend::Searxng {
        base_url: base.clone(),
    });
    let mut c = ctx(&[]);
    let mut book = RefBook::default();
    for u in ["https://a.example/", "https://b.example/"] {
        book.label(u);
    }
    c.refs = std::sync::Arc::new(std::sync::Mutex::new(book));
    let out = s.run(&c, json!({"query": "tokio"})).await;
    assert!(
        out.output.starts_with("[S3] Tokio docs\n"),
        "{}",
        out.output
    );
    let again = s.run(&c, json!({"query": "tokio"})).await;
    assert_eq!(again.output, out.output);

    let f = WebFetch::allowing_private();
    let page = format!("{base}/page");
    c.observed = Arc::new(vec![page.clone()]);
    let out = f.run(&c, json!({"url": page})).await;
    assert!(out.ok, "{}", out.output);
    assert!(
        out.output.starts_with(&format!("[S4] T\nURL: {page}\n\n")),
        "{}",
        out.output
    );
    assert_eq!(
        out.sources,
        [Source {
            id: "S4".into(),
            url: page.clone(),
            title: "T".into(),
        }]
    );
    // Read again (even with a fragment): the same label.
    let out = f.run(&c, json!({"url": format!("{page}#top")})).await;
    assert!(out.output.starts_with("[S4] T\n"), "{}", out.output);
    assert_eq!(out.sources[0].url, page);
}

#[tokio::test]
async fn fetch_reads_text_and_respects_provenance() {
    let base = server().await;
    let f = WebFetch::allowing_private();
    let page = format!("{base}/page");
    let out = f
        .run(&ctx(&[&format!("see {page}")]), json!({"url": page}))
        .await;
    assert!(out.ok, "{}", out.output);
    assert!(out.output.contains("Hello page") && out.output.contains("Body text here."));
    assert!(!out.output.contains("var secret"), "scripts are not text");

    // Not given by the user or a tool: refused before any request.
    let out = f.run(&ctx(&["nothing here"]), json!({"url": page})).await;
    assert!(!out.ok && out.output.contains("did not come from"));

    let bin = format!("{base}/bin");
    let out = f.run(&ctx(&[&bin]), json!({"url": bin})).await;
    assert!(!out.ok && out.output.contains("unsupported content type"));
}

#[tokio::test]
async fn fetch_refuses_private_targets() {
    let base = server().await;
    let f = WebFetch::new();
    let port = base.rsplit(':').next().unwrap();
    for url in [
        format!("{base}/page"),
        format!("http://localhost:{port}/page"),
        format!("http://localhost.:{port}/page"),
        "http://169.254.169.254/latest/meta-data".to_string(),
        "file:///etc/passwd".to_string(),
    ] {
        let out = f.run(&ctx(&[&url]), json!({"url": url})).await;
        assert!(!out.ok, "{url} was fetched: {}", out.output);
    }
    // A public-looking start that redirects inward is stopped at the hop.
    let f = WebFetch::allowing_private_start();
    let hop = format!("{base}/hop");
    let out = f.run(&ctx(&[&hop]), json!({"url": hop})).await;
    assert!(
        !out.ok && out.output.contains("private and local addresses"),
        "{}",
        out.output
    );
}

#[tokio::test]
async fn fetch_readable_markdown_vs_all_text() {
    let base = server().await;
    let url = format!("{base}/article");
    let f = WebFetch::allowing_private();
    let mut c = ctx(&[&url]);
    let all = f.run(&c, json!({"url": url})).await;
    assert!(all.ok, "{}", all.output);
    assert!(all.output.contains("NAVJUNK") && all.output.contains("FOOTERJUNK"));

    c.savings.web_readability = true;
    let md = f.run(&c, json!({"url": url})).await;
    assert!(md.ok, "{}", md.output);
    for gone in ["NAVJUNK", "ASIDEJUNK", "FOOTERJUNK"] {
        assert!(!md.output.contains(gone), "{gone} kept:\n{}", md.output);
    }
    assert!(md.output.contains("`tokio::spawn`"), "{}", md.output);
    // The readable title labels the page.
    assert!(
        md.output.starts_with("[S1] Tokio guide\nURL: "),
        "{}",
        md.output
    );
    assert!(md.output.contains("Spawning"), "{}", md.output);
    assert!(md.output.contains("first item"), "{}", md.output);
    assert!(md.output.len() < all.output.len());
}
