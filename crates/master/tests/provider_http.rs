//! Provider setup by an admin: test, apply, persist, hot-swap, scrub.

use std::sync::Arc;
use std::time::Duration;

use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use harness_agent_native::{NativeBackend, NativeConfig};
use harness_core::event::Profile;
use harness_core::{RawEvent, Secret, SecretSet};
use harness_master::auth::{Auth, Claim};
use harness_master::config::Config;
use harness_master::providers::ProviderRuntime;
use harness_master::{Master, router};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

const KEY: &str = "sk-fake-key-123456";

/// A minimal OpenAI-compatible server that checks the key and echoes it back
/// (to prove scrubbing).
async fn fake_openai() -> String {
    let ok = |h: &HeaderMap| {
        h.get("authorization").and_then(|v| v.to_str().ok()) == Some(&format!("Bearer {KEY}"))
    };
    let app = axum::Router::new()
        .route(
            "/v1/models",
            get(move |h: HeaderMap| async move {
                if ok(&h) {
                    (
                        StatusCode::OK,
                        r#"{"data":[{"id":"fake-1"},{"id":"fake-2"}]}"#,
                    )
                        .into_response()
                } else {
                    (StatusCode::UNAUTHORIZED, "bad key").into_response()
                }
            }),
        )
        .route(
            "/v1/chat/completions",
            post(move |h: HeaderMap| async move {
                if !ok(&h) {
                    return (StatusCode::UNAUTHORIZED, "bad key").into_response();
                }
                let chunk = |t: &str| {
                    format!(
                        "data: {}\n\n",
                        json!({"choices":[{"index":0,"delta":{"content":t}}]})
                    )
                };
                let body = chunk("Hello from fake, my key is ") + &chunk(KEY) + "data: [DONE]\n\n";
                ([("content-type", "text/event-stream")], body).into_response()
            }),
        );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    base
}

struct T {
    base: String,
    dir: tempfile::TempDir,
}

async fn start() -> T {
    let dir = tempfile::tempdir().unwrap();
    let secrets = SecretSet::default();
    let (rt, _env) =
        ProviderRuntime::start(Config::default(), dir.path(), secrets.clone()).unwrap();
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
        default_model: "mock".into(),
        secrets,
        profile: Profile::Chat,
        incognito_ttl: None,
        checkpoints_dir: None,
    })
    .await
    .unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let claim = dir.path().join("claim-code");
    std::fs::write(&claim, "X").unwrap();
    let master = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim,
        }),
    );
    master.set_provider_runtime(rt);
    master.add_link(chat.attach().await, true).await.unwrap();
    tokio::spawn(async move { axum::serve(l, router(master)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        dir,
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
    if let Some(c) = cookie {
        rb = rb.header("cookie", c);
    }
    let r = rb.send().await.unwrap();
    let st = r.status();
    let c = r
        .headers()
        .get("set-cookie")
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
    (st, c, r.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn admin_configures_provider_live() {
    let fake = fake_openai().await;
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

    let (_, _, p) = req(&t, Method::GET, "/api/admin/provider", None, Some(&admin)).await;
    assert_eq!(p["configured"], false);
    assert_eq!(p["kind"], "mock");

    // Testing: wrong key fails without leaking it; right key lists models.
    let (_, _, v) = req(
        &t,
        Method::POST,
        "/api/admin/provider/test",
        Some(json!({"kind": "openai", "base_url": fake, "api_key": "sk-wrong-000000"})),
        Some(&admin),
    )
    .await;
    assert_eq!(v["ok"], false);
    assert!(v["error"].as_str().unwrap().contains("401"));
    assert!(!v.to_string().contains("sk-wrong"));
    let (_, _, v) = req(
        &t,
        Method::POST,
        "/api/admin/provider/test",
        Some(json!({"kind": "openai", "base_url": fake, "api_key": KEY})),
        Some(&admin),
    )
    .await;
    assert_eq!(v["models"], json!(["fake-1", "fake-2"]));

    // Apply.
    let (st, _, v) = req(
        &t,
        Method::PUT,
        "/api/admin/provider",
        Some(json!({
            "kind": "openai", "base_url": fake, "api_key": KEY, "model": "fake-1",
            "models": [{"id": "fake-1"}, {"id": "fake-2", "effort": true}]
        })),
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["key"], "stored");
    assert!(!v.to_string().contains(KEY), "key never returned");

    // Files: config names the secret, the secret file holds it (0600).
    let toml = std::fs::read_to_string(t.dir.path().join("harness.toml")).unwrap();
    assert!(
        toml.contains("api_key_secret = \"openai\"") && toml.contains("fake-1"),
        "{toml}"
    );
    assert!(!toml.contains(KEY));
    let secrets = t.dir.path().join("secrets.toml");
    assert!(std::fs::read_to_string(&secrets).unwrap().contains(KEY));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&secrets).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let reloaded = Config::load(&t.dir.path().join("harness.toml")).unwrap();
    assert_eq!(reloaded.provider.model, "fake-1");

    // Models endpoint reflects it.
    let (_, _, v) = req(&t, Method::GET, "/api/models", None, Some(&admin)).await;
    let ids: Vec<&str> = v["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["fake-1", "fake-2"]);

    // A chat turn now goes to the fake server, and the echoed key is scrubbed.
    let (_, _, v) = req(
        &t,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat"})),
        Some(&admin),
    )
    .await;
    let sid = v["session_id"].as_str().unwrap().to_string();
    req(
        &t,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({"text": "hi"})),
        Some(&admin),
    )
    .await;
    let mut text = String::new();
    for _ in 0..200 {
        let (_, _, v) = req(
            &t,
            Method::GET,
            &format!("/api/sessions/{sid}/events"),
            None,
            Some(&admin),
        )
        .await;
        let evs: Vec<RawEvent> = serde_json::from_value(v["events"].clone()).unwrap();
        if let Some(e) = evs.iter().find(|e| e.kind == "assistant_message") {
            text = e.payload["text"].as_str().unwrap().to_string();
            assert!(
                !serde_json::to_string(&evs).unwrap().contains(KEY),
                "key in the event log"
            );
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(text, "Hello from fake, my key is [REDACTED]");

    // Re-applying without a key keeps the stored one.
    let (st, _, v) = req(&t, Method::PUT, "/api/admin/provider", Some(json!({"kind": "openai", "base_url": fake, "model": "fake-2", "models": [{"id": "fake-2"}]})), Some(&admin)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["key"], "stored");
    let (_, _, v) = req(
        &t,
        Method::POST,
        "/api/admin/provider/test",
        Some(json!({"kind": "openai", "base_url": fake})),
        Some(&admin),
    )
    .await;
    assert_eq!(v["ok"], true, "stored key reused for tests: {v}");

    // Members cannot see or change the provider.
    req(
        &t,
        Method::POST,
        "/api/users",
        Some(json!({"username": "bob", "password": "temporary pw"})),
        Some(&admin),
    )
    .await;
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
    let bc = bc.unwrap();
    for (m, path) in [
        (Method::GET, "/api/admin/provider"),
        (Method::PUT, "/api/admin/provider"),
        (Method::POST, "/api/admin/provider/test"),
    ] {
        let (st, _, _) = req(&t, m, path, Some(json!({"kind": "mock"})), Some(&bc)).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{path}");
    }
}
