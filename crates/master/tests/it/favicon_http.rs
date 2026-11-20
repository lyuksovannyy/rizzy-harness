//! `/api/favicon`: logged-in users only, public https origins only, 404
//! when a site has no icon. Fetching itself is tested in harness-web.

use harness_core::Secret;
use harness_master::auth::Auth;
use harness_master::{Master, router};
use harness_store::Store;
use reqwest::StatusCode;

#[tokio::test]
async fn favicon_requires_login_and_public_https() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let master = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false),
    );
    tokio::spawn(async move { axum::serve(listener, router(master)).await.unwrap() });
    let c = reqwest::Client::new();
    let get = |url: &str, auth: bool| {
        let u =
            reqwest::Url::parse_with_params(&format!("http://{addr}/api/favicon"), [("url", url)])
                .unwrap();
        let rb = c.get(u);
        let rb = if auth { rb.bearer_auth("tok") } else { rb };
        async move { rb.send().await.unwrap().status() }
    };

    assert_eq!(
        get("https://example.com/", false).await,
        StatusCode::UNAUTHORIZED
    );
    for bad in [
        "http://example.com/",
        "file:///etc/passwd",
        "https://127.0.0.1/",
        "https://localhost:8080/",
        "https://user:pw@example.com/",
        "not a url",
        "",
    ] {
        assert_eq!(get(bad, true).await, StatusCode::BAD_REQUEST, "{bad}");
    }
    // `.invalid` never resolves: no icon, and the miss is remembered.
    for _ in 0..2 {
        assert_eq!(
            get("https://no-such-site.invalid/some/page", true).await,
            StatusCode::NOT_FOUND
        );
    }
}
