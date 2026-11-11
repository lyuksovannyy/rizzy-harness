//! Server hardening (FIX_PLAN 3.6, AUDIT L3, L4, L6): provider tests reach
//! public addresses only, a 500 says nothing but a request id, cookie
//! writes need an allowed `Origin` with the server's scheme, API answers
//! are not stored, HSTS only with `--tls`.

mod common;
use common::Login as _;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::routing::{get, post};
use harness_agent_native::{NativeBackend, NativeConfig};
use harness_core::event::Profile;
use harness_core::{Secret, SecretSet};
use harness_master::auth::{Auth, Claim};
use harness_master::config::Config;
use harness_master::outbound::Outbound;
use harness_master::providers::ProviderRuntime;
use harness_master::{Master, router};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

struct T {
    addr: std::net::SocketAddr,
    base: String,
    dir: tempfile::TempDir,
    master: Master,
    admin: String,
}

/// A server; `tls` only sets what the server believes about itself (it
/// serves plain HTTP here), `extra` adds allowed hosts.
async fn start(tls: bool, extra: &[&str]) -> T {
    let dir = tempfile::tempdir().unwrap();
    let cfg = Config::load(&dir.path().join("config.toml")).unwrap();
    let secrets = SecretSet::default();
    let (rt, _env) = ProviderRuntime::start(cfg, dir.path(), secrets.clone()).unwrap();
    let rt = Arc::new(rt);
    let chat = NodeRuntime::start(NodeConfig {
        node_id: "master".into(),
        hostname: "h".into(),
        policy: NodePolicy::default(),
        store: Store::open_in_memory().unwrap(),
        backend: Arc::new(NativeBackend::new(
            rt.provider.clone(),
            Vec::new(),
            NativeConfig {
                secrets: secrets.clone(),
                ..Default::default()
            },
        )),
        default_model: "demo::mock".into(),
        secrets,
        profile: Profile::Chat,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    })
    .await
    .unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let mut hosts = vec![addr.to_string()];
    hosts.extend(extra.iter().map(|h| h.to_string()));
    let master = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("tok".into()), hosts, tls).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: dir.path().join("claim-code"),
        }),
    );
    master.set_provider_runtime(rt);
    master.add_link(chat.attach().await, true).await.unwrap();
    let app = router(master.clone());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let base = format!("http://{addr}");
    // A script's setup: no cookie, no Origin.
    let r = reqwest::Client::new()
        .post(format!("{base}/api/setup"))
        .json(&json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let admin = common::signed_in(r).await;
    T {
        addr,
        base,
        dir,
        master,
        admin,
    }
}

/// A request as the admin's browser on this page would send it.
fn browser(t: &T, method: Method, path: &str) -> reqwest::RequestBuilder {
    reqwest::Client::new()
        .request(method, format!("{}{path}", t.base))
        .login(&t.admin)
        .header("origin", &t.base)
}

async fn json_of(r: reqwest::Response) -> (StatusCode, Value) {
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

/// An OpenAI-compatible fake that counts the requests it gets.
async fn fake_openai(hits: Arc<AtomicUsize>) -> std::net::SocketAddr {
    let (a, b) = (hits.clone(), hits);
    let app = axum::Router::new()
        .route(
            "/v1/models",
            get(move || async move {
                a.fetch_add(1, Ordering::SeqCst);
                axum::Json(json!({"data": [{"id": "m1"}]}))
            }),
        )
        .route(
            "/v1/chat/completions",
            post(move || async move {
                b.fetch_add(1, Ordering::SeqCst);
                axum::Json(json!({"choices": [{"message": {"content": "OK"}}]}))
            }),
        );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    addr
}

/// A TCP port that only counts connections (https to it never gets far).
async fn counting_port(hits: Arc<AtomicUsize>) -> std::net::SocketAddr {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((s, _)) = l.accept().await {
            hits.fetch_add(1, Ordering::SeqCst);
            drop(s);
        }
    });
    addr
}

async fn model_test(t: &T, base_url: &str) -> Value {
    let r = browser(t, Method::POST, "/api/admin/providers/models/test")
        .json(&json!({"base_url": base_url, "model": "m1"}))
        .send()
        .await
        .unwrap();
    let (st, v) = json_of(r).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v
}

async fn provider_test(t: &T, base_url: &str) -> Value {
    let r = browser(t, Method::POST, "/api/admin/providers/test")
        .json(&json!({"preset": "custom_openai", "base_url": base_url}))
        .send()
        .await
        .unwrap();
    let (st, v) = json_of(r).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v
}

/// AUDIT L3: an admin's test call is not a way into the server's network.
#[tokio::test]
async fn provider_tests_reach_public_addresses_only() {
    let t = start(false, &[]).await;
    let hits = Arc::new(AtomicUsize::new(0));
    let fake = fake_openai(hits.clone()).await;
    let tls_hits = Arc::new(AtomicUsize::new(0));
    let tls_port = counting_port(tls_hits.clone()).await;
    let port = fake.port();

    // Unlisted: refused before connecting, with the variable to set.
    for (url, host) in [
        (format!("http://127.0.0.1:{port}/v1"), "127.0.0.1"),
        (format!("http://localhost:{port}/v1"), "localhost"),
    ] {
        let v = model_test(&t, &url).await;
        assert_eq!(v["ok"], false, "{url}: {v}");
        let e = v["error"].as_str().unwrap();
        assert!(
            e.contains(&format!("HARNESS_PLAIN_HTTP_HOSTS={host}")),
            "{url}: {e}"
        );
        let v = provider_test(&t, &url).await;
        assert_eq!(v["ok"], false, "{url}: {v}");
        assert!(
            v["error"]
                .as_str()
                .unwrap()
                .contains("HARNESS_PLAIN_HTTP_HOSTS"),
            "{v}"
        );
    }
    // https to a private address passes `url_allowed` but not this.
    let v = model_test(&t, &format!("https://127.0.0.1:{}/v1", tls_port.port())).await;
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(hits.load(Ordering::SeqCst), 0, "nothing reached the fake");
    assert_eq!(tls_hits.load(Ordering::SeqCst), 0, "no connection at all");

    // Listed, the same host works.
    t.master
        .set_outbound(Outbound::new(["127.0.0.1".to_string()]));
    let v = model_test(&t, &format!("http://127.0.0.1:{port}/v1")).await;
    assert_eq!(v["ok"], true, "{v}");
    let v = provider_test(&t, &format!("http://127.0.0.1:{port}/v1")).await;
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["models"][0]["id"], "m1");
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    // Listing one host opens no other.
    let v = model_test(&t, &format!("http://localhost:{port}/v1")).await;
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(hits.load(Ordering::SeqCst), 2);
}

/// A listed provider that redirects elsewhere is not followed there.
#[tokio::test]
async fn a_listed_provider_cannot_redirect_inward() {
    let t = start(false, &[]).await;
    let inner_hits = Arc::new(AtomicUsize::new(0));
    let inner = fake_openai(inner_hits.clone()).await;
    let to = format!("http://localhost:{}/v1/models", inner.port());
    let app = axum::Router::new().route(
        "/v1/models",
        get(move || {
            let to = to.clone();
            async move { axum::response::Redirect::temporary(&to) }
        }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let outer = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    t.master
        .set_outbound(Outbound::new(["127.0.0.1".to_string()]));
    let v = provider_test(&t, &format!("http://127.0.0.1:{}/v1", outer.port())).await;
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(inner_hits.load(Ordering::SeqCst), 0);
}

/// AUDIT L4: a handler that fails for real (a group-readable secrets.toml
/// while saving a key) answers with a fixed body and a request id; the
/// detail, which names the data directory, stays out of it.
#[tokio::test]
async fn internal_errors_carry_only_a_request_id() {
    let t = start(false, &[]).await;
    let secrets = t.dir.path().join("secrets.toml");
    std::fs::write(&secrets, "[keys]\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&secrets, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    let r = browser(&t, Method::PUT, "/api/admin/providers/fake")
        .json(&json!({
            "name": "Fake", "preset": "custom_openai",
            "base_url": "https://api.example.com/v1", "api_key": "sk-test-123",
            "models": [{"id": "m1"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let id = r.headers()["x-request-id"].to_str().unwrap().to_string();
    assert_eq!(r.headers()["cache-control"], "no-store");
    let body = r.text().await.unwrap();
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v, json!({"error": "internal error", "request_id": id}));
    let dir = t.dir.path().display().to_string();
    assert!(!body.contains(&dir) && !body.contains("secrets"), "{body}");
    assert_eq!(id.len(), 16);
    assert!(id.bytes().all(|b| b.is_ascii_hexdigit()), "{id}");

    // Every response has one, and no two are the same.
    let mut seen = std::collections::HashSet::new();
    seen.insert(id);
    for path in ["/api/me", "/api/nope", "/"] {
        let r = browser(&t, Method::GET, path).send().await.unwrap();
        let id = r.headers()["x-request-id"].to_str().unwrap().to_string();
        assert!(seen.insert(id), "{path}: ids repeat");
    }
    let r = reqwest::Client::new()
        .get(format!("{}/api/me", t.base))
        .header("host", "evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    assert!(r.headers().contains_key("x-request-id"), "the guard's too");
}

/// AUDIT L6: cookie writes need an allowed Origin with the server's scheme.
#[tokio::test]
async fn cookie_writes_need_this_servers_origin() {
    let t = start(false, &[]).await;
    let send = |origin: Option<String>| {
        let mut rb = reqwest::Client::new()
            .post(format!("{}/api/logout", t.base))
            .login(&t.admin);
        if let Some(o) = origin {
            rb = rb.header("origin", o);
        }
        rb.send()
    };
    let refused = [
        None,
        Some("null".to_string()),
        Some(format!("https://{}", t.addr)),
        Some(format!("http://127.0.0.1:{}", t.addr.port() + 1)),
        Some("http://evil.example".into()),
        Some(format!("{}/", t.base)),
    ];
    for o in refused {
        let r = send(o.clone()).await.unwrap();
        assert_eq!(r.status(), StatusCode::FORBIDDEN, "{o:?}");
    }
    // None of them reached the handler: still logged in.
    let r = browser(&t, Method::GET, "/api/me").send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    // A cookie GET needs none (a link, an image).
    let r = reqwest::Client::new()
        .get(format!("{}/api/me", t.base))
        .login(&t.admin)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    // The API token needs none.
    let r = reqwest::Client::new()
        .put(format!("{}/api/me/settings", t.base))
        .bearer_auth("tok")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_ne!(r.status(), StatusCode::FORBIDDEN);
    // A login without a cookie needs none either (scripts, curl).
    let r = reqwest::Client::new()
        .post(format!("{}/api/login", t.base))
        .json(&json!({"username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    // A cookie WebSocket without Origin is refused; with ours it opens.
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let ws = |origin: Option<&str>| {
        let mut r = format!("ws://{}/api/ws", t.addr)
            .into_client_request()
            .unwrap();
        r.headers_mut().insert("cookie", t.admin.parse().unwrap());
        if let Some(o) = origin {
            r.headers_mut().insert("origin", o.parse().unwrap());
        }
        tokio_tungstenite::connect_async(r)
    };
    assert!(ws(None).await.is_err());
    assert!(ws(Some(&format!("https://{}", t.addr))).await.is_err());
    assert!(ws(Some(&t.base)).await.is_ok());
    // And the page's own logout works.
    let r = browser(&t, Method::POST, "/api/logout")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let r = browser(&t, Method::GET, "/api/me").send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}

/// With `--tls` the scheme is https: an http page is refused; a TLS proxy
/// is named as an origin in the allowed hosts.
#[tokio::test]
async fn the_origin_scheme_follows_the_server() {
    let t = start(true, &["https://chat.example"]).await;
    for (host, origin, ok) in [
        (t.addr.to_string(), format!("https://{}", t.addr), true),
        (t.addr.to_string(), format!("http://{}", t.addr), false),
        ("chat.example".into(), "https://chat.example".into(), true),
        ("chat.example".into(), "http://chat.example".into(), false),
    ] {
        let r = reqwest::Client::new()
            .put(format!("{}/api/me/settings", t.base))
            .header("host", &host)
            .login(&t.admin)
            .header("origin", &origin)
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status() != StatusCode::FORBIDDEN, ok, "{origin}");
    }
}

/// AUDIT L6: API answers are never stored by the browser; the UI's files
/// keep their caching; HSTS only when this server serves TLS.
#[tokio::test]
async fn cache_and_transport_headers() {
    let t = start(false, &["harness.example"]).await;
    let me = browser(&t, Method::GET, "/api/me").send().await.unwrap();
    let anon = reqwest::Client::new()
        .get(format!("{}/api/me", t.base))
        .send()
        .await
        .unwrap();
    assert_eq!(anon.status(), StatusCode::UNAUTHORIZED);
    let missing = browser(&t, Method::GET, "/api/nope").send().await.unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    // Stored files (which set their own) are checked in media_http.rs.
    for r in [&me, &anon, &missing] {
        assert_eq!(r.headers()["cache-control"], "no-store", "{}", r.url());
        assert!(!r.headers().contains_key("strict-transport-security"));
    }
    let page = reqwest::Client::new()
        .get(format!("{}/", t.base))
        .send()
        .await
        .unwrap();
    assert_ne!(
        page.headers()
            .get("cache-control")
            .map(|v| v.to_str().unwrap()),
        Some("no-store"),
        "the UI is not an API answer"
    );
    // Plain HTTP: never HSTS, whatever the host.
    let r = reqwest::Client::new()
        .get(format!("{}/api/me", t.base))
        .header("host", "harness.example")
        .send()
        .await
        .unwrap();
    assert!(!r.headers().contains_key("strict-transport-security"));

    let tls = start(true, &["harness.example"]).await;
    let get = |host: String| {
        reqwest::Client::new()
            .get(format!("{}/", tls.base))
            .header("host", host)
            .send()
    };
    let r = get("harness.example".into()).await.unwrap();
    assert_eq!(r.headers()["strict-transport-security"], "max-age=15552000");
    // Not for localhost names or addresses (one name, every port).
    let r = get(tls.addr.to_string()).await.unwrap();
    assert!(!r.headers().contains_key("strict-transport-security"));
}
