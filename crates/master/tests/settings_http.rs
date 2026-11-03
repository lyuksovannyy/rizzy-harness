//! Per-user preferences (`/api/me/settings`) and per-message effort.

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::Secret;
use harness_core::event::Profile;
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, ModelInfo, router};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

struct T {
    base: String,
    master: Master,
    _dir: tempfile::TempDir,
}

async fn start() -> T {
    let dir = tempfile::tempdir().unwrap();
    let chat = NodeRuntime::start(NodeConfig {
        node_id: "master".into(),
        hostname: "h".into(),
        policy: NodePolicy::default(),
        store: Store::open_in_memory().unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(1),
            }),
            Vec::new(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: Profile::Chat,
        incognito_ttl: None,
    })
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, "X").unwrap();
    let master = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim_path,
        }),
    );
    master.set_models(vec![
        ModelInfo {
            id: "mock".into(),
            provider: "mock".into(),
            effort: false,
            vision: false,
            default: true,
        },
        ModelInfo {
            id: "thinker".into(),
            provider: "mock".into(),
            effort: true,
            vision: false,
            default: false,
        },
    ]);
    master.add_link(chat.attach().await, true).await.unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        master,
        _dir: dir,
    }
}

async fn req(
    t: &T,
    method: Method,
    path: &str,
    body: Option<Value>,
    cookie: Option<&str>,
) -> (StatusCode, Option<String>, Value) {
    let mut rb = reqwest::Client::new().request(method, format!("{}{path}", t.base));
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    rb = match cookie {
        Some(c) => rb.header("cookie", c),
        None => rb.bearer_auth("tok"),
    };
    let r = rb.send().await.unwrap();
    let status = r.status();
    let c = r
        .headers()
        .get("set-cookie")
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
    (status, c, r.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn settings_defaults_validation_and_isolation() {
    let t = start().await;
    let (_, admin, _) = req(
        &t,
        Method::POST,
        "/api/setup",
        Some(json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"})),
        None,
    )
    .await;
    let admin = admin.unwrap();

    let (st, _, v) = req(&t, Method::GET, "/api/me/settings", None, Some(&admin)).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["search_url"], "https://duckduckgo.com/?q={q}");
    assert!(v["default_agent"].is_null());

    for bad in [
        json!({"search_url": "http://example.com/?q={q}"}),
        json!({"search_url": "https://example.com/search"}),
        json!({"search_url": "https://e.com/?q={q}", "default_agent": "nope"}),
        json!({"search_url": "https://e.com/?q={q}", "default_model": "nope"}),
    ] {
        let (st, _, v) = req(
            &t,
            Method::PUT,
            "/api/me/settings",
            Some(bad.clone()),
            Some(&admin),
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{bad} -> {v}");
    }
    let good = json!({"search_url": "https://www.google.com/search?q={q}", "default_agent": "writer", "default_model": "thinker"});
    let (st, _, _) = req(
        &t,
        Method::PUT,
        "/api/me/settings",
        Some(good.clone()),
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_, _, v) = req(&t, Method::GET, "/api/me/settings", None, Some(&admin)).await;
    for k in ["search_url", "default_agent", "default_model"] {
        assert_eq!(v[k], good[k], "{k}");
    }
    assert_eq!(v["onboarded"], false);

    // Profile fields: valid ones stick, bad ones are refused.
    let profile = json!({"search_url": "https://e.com/?q={q}", "display_name": "Alex", "timezone": "Europe/Riga", "country": "Latvia", "city": "Riga", "accent": "green", "onboarded": true});
    let (st, _, _) = req(
        &t,
        Method::PUT,
        "/api/me/settings",
        Some(profile.clone()),
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_, _, v) = req(&t, Method::GET, "/api/me/settings", None, Some(&admin)).await;
    assert_eq!(v["timezone"], "Europe/Riga");
    assert_eq!(v["onboarded"], true);
    for bad in [
        json!({"search_url": "https://e.com/?q={q}", "timezone": "Europe/Riga; DROP"}),
        json!({"search_url": "https://e.com/?q={q}", "accent": "neon"}),
        json!({"search_url": "https://e.com/?q={q}", "display_name": "x".repeat(65)}),
        json!({"search_url": "https://e.com/?q={q}", "city": "  "}),
    ] {
        let (st, _, v) = req(
            &t,
            Method::PUT,
            "/api/me/settings",
            Some(bad.clone()),
            Some(&admin),
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{bad} -> {v}");
    }

    // Another user keeps their own defaults.
    let (_, _, bob) = req(
        &t,
        Method::POST,
        "/api/users",
        Some(json!({"username": "bob", "password": "temporary pw"})),
        Some(&admin),
    )
    .await;
    assert_eq!(bob["username"], "bob");
    let (_, bc, _) = req(
        &t,
        Method::POST,
        "/api/login",
        Some(json!({"username": "bob", "password": "temporary pw"})),
        None,
    )
    .await;
    let (_, bc, _) = req(
        &t,
        Method::POST,
        "/api/me/password",
        Some(json!({"current": "temporary pw", "new": "bobs own password"})),
        Some(&bc.unwrap()),
    )
    .await;
    let (_, _, v) = req(
        &t,
        Method::GET,
        "/api/me/settings",
        None,
        Some(&bc.unwrap()),
    )
    .await;
    assert_eq!(v["search_url"], "https://duckduckgo.com/?q={q}");
}

#[test]
fn effort_override_is_validated_and_gated() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let t = rt.block_on(start());
    // Model without effort support: the override is dropped.
    assert_eq!(
        t.master
            .agent_spec_with(None, Some("mock"), Some("high"))
            .unwrap()
            .effort,
        None
    );
    // Model with support: kept.
    assert_eq!(
        t.master
            .agent_spec_with(None, Some("thinker"), Some("high"))
            .unwrap()
            .effort
            .as_deref(),
        Some("high")
    );
    assert!(
        t.master
            .agent_spec_with(None, Some("thinker"), Some("turbo"))
            .is_err()
    );
}
