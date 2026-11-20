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

// ---- review of PR 179: caps, redirects, error text ----------------------

fn ctx_with_pages(strong: &[&str], pages: &[&str]) -> ToolCtx {
    let mut c = ToolCtx::new(std::env::temp_dir());
    let all: Vec<String> = strong
        .iter()
        .map(|s| s.to_string())
        .chain(std::iter::once(harness_tools::PAGE_MARK.to_string()))
        .chain(pages.iter().map(|s| s.to_string()))
        .collect();
    c.observed = Arc::new(all);
    c
}

/// A URL that only a fetched page lists counts against the turn's cap of
/// `MAX_PAGE_VOUCHED_FETCHES`; the person's own URL and a search result's
/// do not; a turn's counter is shared by its calls, and a new turn starts
/// at zero.
#[tokio::test]
async fn fetches_vouched_only_by_a_page_are_capped_per_turn() {
    let base = server().await;
    let f = WebFetch::allowing_private();
    let page = format!("{base}/page");
    let mine = format!("{base}/page?mine=1");
    let found = format!("{base}/page?found=1");
    let c = ctx_with_pages(&[&format!("see {mine}"), &format!("hit {found}")], &[&page]);
    for i in 0..20 {
        let o = f.run(&c, json!({"url": page})).await;
        assert!(o.ok, "fetch {i}: {}", o.output);
    }
    let o = f.run(&c, json!({"url": page})).await;
    assert!(
        !o.ok && o.output.contains("only because another page linked"),
        "{}",
        o.output
    );
    // Not counted: the person's URL, a search result's.
    for u in [&mine, &found] {
        let o = f.run(&c, json!({"url": u})).await;
        assert!(o.ok, "{u}: {}", o.output);
    }
    // A URL no source vouches for keeps the old refusal.
    let o = f.run(&c, json!({"url": format!("{base}/bin")})).await;
    assert!(o.output.contains("did not come from"), "{}", o.output);
    // A new turn (a new counter) starts again.
    let again = ToolCtx {
        observed: c.observed.clone(),
        ..ToolCtx::new(std::env::temp_dir())
    };
    assert!(f.run(&again, json!({"url": page})).await.ok);
}

/// A search query may have at most `MAX_QUERY_CHARS` characters (longer is
/// refused, not cut) and a turn at most `MAX_SEARCHES_PER_TURN` searches.
#[tokio::test]
async fn searches_are_bounded_in_length_and_number() {
    let base = server().await;
    let s = WebSearch::new(SearchBackend::Searxng { base_url: base });
    let c = ctx(&[]);
    let long = "q".repeat(harness_web::MAX_QUERY_CHARS + 1);
    let o = s.run(&c, json!({"query": long})).await;
    assert!(!o.ok && o.output.contains("at most 400"), "{}", o.output);
    let edge = "q".repeat(harness_web::MAX_QUERY_CHARS);
    assert!(s.run(&c, json!({"query": edge})).await.ok);
    // A refused query used no search (one was spent above on the edge).
    for i in 1..harness_web::MAX_SEARCHES_PER_TURN {
        let o = s.run(&c, json!({"query": format!("q{i}")})).await;
        assert!(o.ok, "search {i}: {}", o.output);
    }
    let o = s.run(&c, json!({"query": "one more"})).await;
    assert!(
        !o.ok && o.output.contains("already made 20 searches"),
        "{}",
        o.output
    );
    assert!(s.run(&ctx(&[]), json!({"query": "new turn"})).await.ok);
}

/// The search client follows no redirect: the Brave key header never goes
/// to a second origin.
#[tokio::test]
async fn the_search_client_follows_no_redirect() {
    let hits: Arc<std::sync::Mutex<Vec<Option<String>>>> = Arc::default();
    let h2 = hits.clone();
    let second = Router::new().route(
        "/x",
        get(move |headers: axum::http::HeaderMap| async move {
            h2.lock().unwrap().push(
                headers
                    .get("x-subscription-token")
                    .map(|v| v.to_str().unwrap().to_string()),
            );
            axum::Json(json!({"web": {"results": []}}))
        }),
    );
    let l2 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let a2 = l2.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l2, second).await.unwrap() });
    let target = format!("http://{a2}/x");
    let first = Router::new().route(
        "/brave",
        get(move || {
            let t = target.clone();
            async move { Redirect::temporary(&t) }
        }),
    );
    let l1 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let a1 = l1.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l1, first).await.unwrap() });
    let b = WebSearch::new(SearchBackend::Brave {
        api_key: harness_core::Secret::new("bk-redirect-key".into()),
    })
    .with_brave_url(&format!("http://{a1}/brave"));
    let o = b.run(&ctx(&[]), json!({"query": "x"})).await;
    assert!(!o.ok && o.output.contains("307"), "{}", o.output);
    assert!(
        hits.lock().unwrap().is_empty(),
        "the second origin was asked"
    );
}

/// A backend error names the backend by scheme, host and port only: not the
/// path, query or credentials of the address the owner configured.
#[tokio::test]
async fn a_search_error_names_the_backend_by_host_only() {
    let s = WebSearch::new(SearchBackend::Searxng {
        base_url: "http://user:hunter2@127.0.0.1:9/secret/path?token=abc123".into(),
    });
    let o = s.run(&ctx(&[]), json!({"query": "x"})).await;
    assert!(!o.ok, "{}", o.output);
    assert!(o.output.contains("http://127.0.0.1:9"), "{}", o.output);
    for leak in ["hunter2", "user", "secret", "path", "token", "abc123"] {
        assert!(!o.output.contains(leak), "{leak} in: {}", o.output);
    }
    // And the status error of a backend that answers.
    let base = server().await;
    let s = WebSearch::new(SearchBackend::Searxng {
        base_url: format!("{base}/nope?k=v"),
    });
    let o = s.run(&ctx(&[]), json!({"query": "x"})).await;
    assert!(
        !o.ok && !o.output.contains("nope") && !o.output.contains("k=v"),
        "{}",
        o.output
    );
}

/// `localhost.` is `localhost`.
#[test]
fn a_trailing_dot_does_not_hide_localhost() {
    for u in [
        "http://localhost./",
        "http://LOCALHOST./x",
        "http://app.localhost./",
        "http://localhost../",
    ] {
        assert!(
            harness_web::check_public_url(&url::Url::parse(u).unwrap()).is_err(),
            "{u}"
        );
    }
    assert!(
        harness_web::check_public_url(&url::Url::parse("http://example.com./").unwrap()).is_ok()
    );
}

/// The client allows five requests in a chain (the policy refuses the
/// redirect that would be the fifth): a chain of that length is followed, one
/// redirect longer is refused with the existing message.
#[tokio::test]
async fn the_redirect_hop_limit_is_five() {
    let app = Router::new().route(
        "/c/{n}",
        get(
            |axum::extract::Path(n): axum::extract::Path<u32>| async move {
                if n == 0 {
                    Html("<html><body>end of the chain</body></html>").into_response()
                } else {
                    Redirect::temporary(&format!("/c/{}", n - 1)).into_response()
                }
            },
        ),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let a = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let f = WebFetch::allowing_private();
    let ok = format!("http://{a}/c/4");
    let o = f.run(&ctx(&[&ok]), json!({"url": ok})).await;
    assert!(
        o.ok && o.output.contains("end of the chain"),
        "{}",
        o.output
    );
    let too_far = format!("http://{a}/c/5");
    let o = f.run(&ctx(&[&too_far]), json!({"url": too_far})).await;
    assert!(
        !o.ok && o.output.contains("too many redirects"),
        "{}",
        o.output
    );
}
