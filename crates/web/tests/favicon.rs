//! Favicon fetching against local fake sites.

use axum::Router;
use axum::response::{Html, Redirect};
use axum::routing::get;
use harness_web::favicon::{FaviconFetcher, MAX_ICON};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
const ICO: &[u8] = &[0, 0, 1, 0, 1, 0, 16, 16];

async fn site(app: Router) -> url::Url {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url::Url::parse(&format!("http://{addr}/")).unwrap()
}

async fn fetch(app: Router) -> Option<harness_web::favicon::Favicon> {
    let origin = site(app).await;
    FaviconFetcher::allowing_private().fetch(&origin).await
}

#[tokio::test]
async fn link_icon_after_redirect() {
    let icon = fetch(
        Router::new()
            .route("/", get(|| async { Redirect::temporary("/home/") }))
            .route(
                "/home/",
                get(|| async {
                    Html(r#"<head><link rel="icon" sizes="32x32" href="big.png"><link rel="icon" sizes="16x16" href="small.png"></head>"#)
                }),
            )
            .route("/home/small.png", get(|| async { ([("content-type", "text/plain")], PNG) })),
    )
    .await
    .unwrap();
    // Typed by the bytes, not by what the server claimed.
    assert_eq!(icon.content_type, "image/png");
    assert_eq!(icon.bytes, PNG);
}

#[tokio::test]
async fn falls_back_to_favicon_ico() {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#;
    let icon = fetch(
        Router::new()
            .route(
                "/",
                get(|| async {
                    Html(
                        r#"<link rel="icon" href="/bad.png"><link rel="icon" href="/missing.png">"#,
                    )
                }),
            )
            .route(
                "/bad.png",
                get(move || async move { ([("content-type", "image/png")], svg) }),
            )
            .route("/favicon.ico", get(|| async { ICO })),
    )
    .await
    .unwrap();
    assert_eq!(icon.content_type, "image/x-icon");
}

#[tokio::test]
async fn html_svg_and_oversized_icons_are_refused() {
    let none = fetch(
        Router::new()
            .route(
                "/",
                get(|| async { Html(r#"<link rel="icon" href="/big.png">"#) }),
            )
            .route(
                "/big.png",
                get(|| async {
                    let mut b = PNG.to_vec();
                    b.resize(MAX_ICON + 1, 0);
                    b
                }),
            )
            .route(
                "/favicon.ico",
                get(|| async { Html("<html>not an icon</html>") }),
            ),
    )
    .await;
    assert_eq!(none, None);
    // A site with no page and no icon at all.
    assert_eq!(fetch(Router::new()).await, None);
}

#[tokio::test]
async fn private_sites_are_refused_by_default() {
    let origin = site(Router::new().route("/favicon.ico", get(|| async { ICO }))).await;
    assert_eq!(FaviconFetcher::new().fetch(&origin).await, None);
}
