//! Provider setup by an admin: test, apply, persist, hot-swap, scrub,
//! several providers routed by prefix, per-agent default models.

mod common;
use common::{Login as _, SameOrigin as _};

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

/// A minimal OpenAI-compatible server that checks the key, lists `models`
/// and answers `<tag> <model>, my key is <key>` (to prove routing and
/// scrubbing).
async fn fake_openai(tag: &'static str, models: &'static [&'static str]) -> String {
    let ok = |h: &HeaderMap| {
        h.get("authorization").and_then(|v| v.to_str().ok()) == Some(&format!("Bearer {KEY}"))
    };
    let app = axum::Router::new()
        .route(
            "/v1/models",
            get(move |h: HeaderMap| async move {
                if ok(&h) {
                    let data: Vec<Value> = models
                        .iter()
                        .map(|m| json!({"id": m, "context_length": 4096}))
                        .collect();
                    axum::Json(json!({ "data": data })).into_response()
                } else {
                    (StatusCode::UNAUTHORIZED, "bad key").into_response()
                }
            }),
        )
        .route(
            "/v1/chat/completions",
            post(move |h: HeaderMap, body: String| async move {
                if !ok(&h) {
                    return (StatusCode::UNAUTHORIZED, "bad key").into_response();
                }
                let req: Value = serde_json::from_str(&body).unwrap();
                let chunk = |t: &str| {
                    format!(
                        "data: {}\n\n",
                        json!({"choices":[{"index":0,"delta":{"content":t}}]})
                    )
                };
                let head = format!("{tag} {}, my key is ", req["model"].as_str().unwrap());
                let body = chunk(&head) + &chunk(KEY) + "data: [DONE]\n\n";
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
    admin: String,
}

/// A server whose data dir is prepared by `setup` before the config loads.
async fn start_with(setup: impl FnOnce(&std::path::Path)) -> T {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
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
        default_model: "stale".into(),
        secrets,
        profile: Profile::Chat,
        incognito_ttl: None,
        checkpoints_dir: None,
        artifacts: None,
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
    // The fake providers run on 127.0.0.1: listed, as an operator would
    // list a local model server (HARNESS_PLAIN_HTTP_HOSTS).
    master.set_outbound(harness_master::outbound::Outbound::new([
        "127.0.0.1".to_string()
    ]));
    master.add_link(chat.attach().await, true).await.unwrap();
    tokio::spawn(async move { axum::serve(l, router(master)).await.unwrap() });
    let mut t = T {
        base: format!("http://{addr}"),
        dir,
        admin: String::new(),
    };
    let (_, admin, _) = req(
        &t,
        Method::POST,
        "/api/setup",
        Some(json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"})),
        None,
    )
    .await;
    t.admin = admin.unwrap();
    t
}

async fn start() -> T {
    start_with(|_| {}).await
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
        rb = rb.login(c).same_origin();
    }
    let r = rb.send().await.unwrap();
    let st = r.status();
    let c = r
        .headers()
        .get("set-cookie")
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
    let body = r.json().await.unwrap_or(Value::Null);
    if let Some(c) = &c {
        common::remember(c, &body);
    }
    (st, c, body)
}

/// As the admin.
async fn admin(t: &T, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let (st, _, v) = req(t, method, path, body, Some(&t.admin)).await;
    (st, v)
}

/// Start a chat (optionally with a model), send `text`, return the answer.
async fn chat(t: &T, model: Option<&str>, text: &str) -> (String, String) {
    let mut body = json!({"profile": "chat"});
    if let Some(m) = model {
        body["model"] = json!(m);
    }
    let (_, v) = admin(t, Method::POST, "/api/sessions", Some(body)).await;
    let sid = v["session_id"]
        .as_str()
        .expect("session created")
        .to_string();
    admin(
        t,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({ "text": text })),
    )
    .await;
    for _ in 0..200 {
        let (_, v) = admin(t, Method::GET, &format!("/api/sessions/{sid}/events"), None).await;
        let evs: Vec<RawEvent> = serde_json::from_value(v["events"].clone()).unwrap();
        if let Some(e) = evs.iter().find(|e| e.kind == "assistant_message") {
            assert!(
                !serde_json::to_string(&evs).unwrap().contains(KEY),
                "key in the event log"
            );
            return (sid, e.payload["text"].as_str().unwrap().to_string());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("no answer");
}

fn ids(v: &Value) -> Vec<String> {
    v["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn admin_configures_provider_live() {
    let fake = fake_openai("fake", &["fake-2", "fake-1"]).await;
    let t = start().await;

    let (_, p) = admin(&t, Method::GET, "/api/admin/providers", None).await;
    assert_eq!(p["configured"], false);
    assert_eq!(p["providers"][0]["kind"], "mock");
    assert_eq!(p["default_model"], "demo::mock");
    let presets: Vec<&str> = p["presets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["id"].as_str().unwrap())
        .collect();
    // The admin's own endpoints first, then known platforms.
    assert_eq!(
        presets,
        [
            "custom_openai",
            "custom_anthropic",
            "ollama",
            "openai",
            "anthropic",
            "gemini",
            "mistral",
            "deepseek",
            "nvidia_nim",
            "moonshot",
            "zai",
            "xai",
            "groq",
            "openrouter",
            "together",
            "fireworks",
        ]
    );
    let groups: Vec<&str> = p["presets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["group"].as_str().unwrap())
        .collect();
    assert_eq!(groups[..4], ["own", "own", "own", "known"]);
    assert!(groups[3..].iter().all(|g| *g == "known"));
    assert_eq!(p["presets"][5]["label"], "Google AI Studio (Gemini)");
    let nim = &p["presets"][8];
    assert_eq!(nim["id"], "nvidia_nim");
    assert_eq!(nim["warning"]["kind"], "training_data");
    assert_eq!(nim["warning"]["wait_secs"], 10);
    assert_eq!(nim["url_editable"], false);

    // Testing: wrong key fails without leaking it; right key lists models
    // with what the provider reports, sorted by id.
    let test = |key: Option<&str>, provider_id: Option<&str>, base: &str| {
        let mut b = json!({"preset": "custom_openai", "base_url": base});
        if let Some(k) = key {
            b["api_key"] = json!(k);
        }
        if let Some(id) = provider_id {
            b["provider_id"] = json!(id);
        }
        b
    };
    let (_, v) = admin(
        &t,
        Method::POST,
        "/api/admin/providers/test",
        Some(test(Some("sk-wrong-000000"), None, &fake)),
    )
    .await;
    assert_eq!(v["ok"], false);
    assert!(v["error"].as_str().unwrap().contains("401"));
    assert!(!v.to_string().contains("sk-wrong"));
    let (_, v) = admin(
        &t,
        Method::POST,
        "/api/admin/providers/test",
        Some(test(Some(KEY), None, &fake)),
    )
    .await;
    assert_eq!(ids(&v), ["fake-1", "fake-2"]);
    assert_eq!(v["models"][0]["context"], 4096);
    assert_eq!(
        v["models"][0]["vision"],
        Value::Null,
        "unreported stays null"
    );

    // Apply.
    let (st, v) = admin(
        &t,
        Method::PUT,
        "/api/admin/providers/fake",
        Some(json!({
            "name": "Fake", "preset": "custom_openai", "base_url": fake, "api_key": KEY,
            "models": [{"id": "fake-1", "context": 4096}, {"id": "fake-2", "effort": true}]
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["configured"], true);
    assert_eq!(v["providers"].as_array().unwrap().len(), 1, "demo replaced");
    assert_eq!(v["providers"][0]["key"], "stored");
    assert_eq!(v["providers"][0]["name"], "Fake");
    assert_eq!(v["default_model"], "fake::fake-1");
    assert!(!v.to_string().contains(KEY), "key never returned");

    // Files: config names the secret, the secret file holds it (0600).
    let toml = std::fs::read_to_string(t.dir.path().join("config.toml")).unwrap();
    assert!(
        toml.contains("[[providers]]")
            && toml.contains("api_key_secret = \"provider-fake\"")
            && toml.contains("fake-1"),
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
    let reloaded = Config::load(&t.dir.path().join("config.toml")).unwrap();
    assert_eq!(reloaded.default_model.as_deref(), Some("fake::fake-1"));
    assert_eq!(reloaded.providers[0].models[0].context, Some(4096));

    // Models endpoint reflects it, prefixed, with the parameters.
    let (_, v) = admin(&t, Method::GET, "/api/models", None).await;
    assert_eq!(ids(&v), ["fake::fake-1", "fake::fake-2"]);
    let m = &v["models"][1];
    assert_eq!(
        (&m["raw"], &m["provider"], &m["provider_id"]),
        (&json!("fake-2"), &json!("Fake"), &json!("fake"))
    );
    assert_eq!(
        (&m["effort"], &m["tools"], &m["default"]),
        (&json!(true), &json!(false), &json!(false))
    );

    // A chat turn now goes to the fake server (prefix stripped), and the
    // echoed key is scrubbed.
    let (_, text) = chat(&t, None, "hi").await;
    assert_eq!(text, "fake fake-1, my key is [REDACTED]");

    // Re-applying without a key keeps the stored one (same URL).
    let (st, v) = admin(
        &t,
        Method::PUT,
        "/api/admin/providers/fake",
        Some(json!({"name": "Fake", "preset": "custom_openai", "base_url": fake, "models": [{"id": "fake-2"}]})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["providers"][0]["key"], "stored");
    let (_, v) = admin(
        &t,
        Method::POST,
        "/api/admin/providers/test",
        Some(test(None, Some("fake"), &fake)),
    )
    .await;
    assert_eq!(v["ok"], true, "stored key reused for the same URL: {v}");

    // Another URL never gets the stored key: not in a test...
    let other = fake_openai("other", &["o-1"]).await;
    let (_, v) = admin(
        &t,
        Method::POST,
        "/api/admin/providers/test",
        Some(test(None, Some("fake"), &other)),
    )
    .await;
    assert_eq!(v["ok"], false, "{v}");
    assert!(v["error"].as_str().unwrap().contains("401"), "{v}");
    // ...and not when the provider moves: it is dropped and deleted.
    let (st, v) = admin(
        &t,
        Method::PUT,
        "/api/admin/providers/fake",
        Some(json!({"name": "Fake", "preset": "custom_openai", "base_url": other, "models": [{"id": "o-1"}]})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["providers"][0]["key"], "none");
    assert!(!std::fs::read_to_string(&secrets).unwrap().contains(KEY));

    // Members cannot see or change providers.
    let pw = req(
        &t,
        Method::POST,
        "/api/users",
        Some(json!({"username": "bob"})),
        Some(&t.admin),
    )
    .await
    .2["password"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, bc, _) = req(
        &t,
        Method::POST,
        "/api/login",
        Some(json!({"username": "bob", "password": pw})),
        None,
    )
    .await;
    let (_, bc, _) = req(
        &t,
        Method::POST,
        "/api/me/password",
        Some(json!({"current": pw, "new": "bobs own password"})),
        Some(&bc.unwrap()),
    )
    .await;
    let bc = bc.unwrap();
    for (m, path) in [
        (Method::GET, "/api/admin/providers"),
        (Method::PUT, "/api/admin/providers/fake"),
        (Method::DELETE, "/api/admin/providers/fake"),
        (Method::POST, "/api/admin/providers/test"),
        (Method::POST, "/api/admin/providers/demo"),
        (Method::PUT, "/api/admin/agent-models"),
    ] {
        let body = json!({"preset": "openai", "name": "x", "agent_models": {}});
        let (st, _, _) = req(&t, m, path, Some(body), Some(&bc)).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{path}");
    }
}

/// Fixed presets keep their own URL and need a key; ids are checked.
#[tokio::test]
async fn fixed_presets_pin_their_url() {
    let fake = fake_openai("fake", &["m"]).await;
    let t = start().await;
    let put = |id: &str, body: Value| {
        let path = format!("/api/admin/providers/{id}");
        let t = &t;
        async move { admin(t, Method::PUT, &path, Some(body)).await }
    };
    let (st, v) = put(
        "openrouter",
        json!({"preset": "openrouter", "base_url": fake, "models": [{"id": "openai/gpt-4o"}]}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "a key is required: {v}");
    let (st, v) = put(
        "openrouter",
        json!({"preset": "openrouter", "name": "Mine", "base_url": fake, "api_key": KEY,
               "models": [{"id": "openai/gpt-4o"}]}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let p = &v["providers"][0];
    assert_eq!(
        p["base_url"], "https://openrouter.ai/api/v1",
        "client URL ignored"
    );
    assert_eq!(p["name"], "OpenRouter", "fixed presets keep their name");
    assert_eq!(v["default_model"], "openrouter::openai/gpt-4o");

    // Anthropic models never offer effort (PLAN.md R8).
    let (st, v) = put(
        "anthropic",
        json!({"preset": "anthropic", "api_key": KEY, "models": [{"id": "claude-x", "effort": true}]}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, m) = admin(&t, Method::GET, "/api/models", None).await;
    let claude = m["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == "anthropic::claude-x")
        .unwrap();
    assert_eq!(claude["effort"], false);

    for (id, body) in [
        (
            "Bad Id",
            json!({"preset": "openai", "api_key": KEY, "models": [{"id": "m"}]}),
        ),
        (
            "demo",
            json!({"preset": "openai", "api_key": KEY, "models": [{"id": "m"}]}),
        ),
        ("x", json!({"preset": "nope", "models": [{"id": "m"}]})),
        (
            "x",
            json!({"preset": "custom_openai", "base_url": fake, "models": [{"id": "m"}]}),
        ),
        (
            "x",
            json!({"preset": "custom_openai", "name": "X", "models": [{"id": "m"}]}),
        ),
        (
            "x",
            json!({"preset": "custom_openai", "name": "X", "base_url": "http://example.com/v1", "models": [{"id": "m"}]}),
        ),
        (
            "models",
            json!({"preset": "openai", "api_key": KEY, "models": [{"id": "m"}]}),
        ),
    ] {
        let (st, v) = put(id, body.clone()).await;
        assert!(st.is_client_error(), "{id} {body}: {v}");
        assert!(!v.to_string().contains(KEY));
    }
}

/// Two providers: prefixes route, agent defaults apply to new sessions, and
/// deleting cleans up.
#[tokio::test]
async fn several_providers_route_by_prefix() {
    let a = fake_openai("A", &["shared", "a-only"]).await;
    let b = fake_openai("B", &["shared", "b-only"]).await;
    let t = start().await;
    for (id, base, models) in [
        ("alpha", &a, json!([{"id": "shared"}, {"id": "a-only"}])),
        ("beta", &b, json!([{"id": "shared"}, {"id": "b-only"}])),
    ] {
        let (st, v) = admin(
            &t,
            Method::PUT,
            &format!("/api/admin/providers/{id}"),
            Some(json!({"name": id, "preset": "custom_openai", "base_url": base, "api_key": KEY, "models": models})),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }
    let (_, v) = admin(&t, Method::GET, "/api/models", None).await;
    assert_eq!(
        ids(&v),
        [
            "alpha::shared",
            "alpha::a-only",
            "beta::shared",
            "beta::b-only"
        ]
    );

    let (_, text) = chat(&t, Some("beta::shared"), "hi").await;
    assert_eq!(text, "B shared, my key is [REDACTED]");
    let (_, text) = chat(&t, Some("alpha::shared"), "hi").await;
    assert_eq!(text, "A shared, my key is [REDACTED]");

    // Per-agent defaults: unknown agents and models are refused.
    for body in [
        json!({"agent_models": {"nobody": {"model": "beta::b-only"}}}),
        json!({"agent_models": {"general": {"model": "beta::nope"}}}),
        json!({"agent_models": {}, "default_model": "gone::x"}),
    ] {
        let (st, _) = admin(&t, Method::PUT, "/api/admin/agent-models", Some(body)).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
    }
    let (st, v) = admin(
        &t,
        Method::PUT,
        "/api/admin/agent-models",
        Some(json!({"agent_models": {"general": {"model": "beta::b-only"}, "writer": {"model": "alpha::a-only"}}})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        v["agent_models"]["writer"],
        json!({"model": "alpha::a-only", "fallbacks": []}),
        "stored"
    );
    assert_eq!(v["default_model"], "alpha::shared", "kept while valid");
    let (_, agents) = admin(&t, Method::GET, "/api/agents", None).await;
    let general = agents["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "general")
        .unwrap();
    assert_eq!(general["default_model"], "beta::b-only");

    // A session created without a model gets its agent's default.
    let (sid, text) = chat(&t, None, "hi").await;
    assert_eq!(text, "B b-only, my key is [REDACTED]");
    let (_, s) = admin(&t, Method::GET, &format!("/api/sessions/{sid}"), None).await;
    assert_eq!(s["model"], "beta::b-only");
    // An explicit choice wins over the agent default.
    let (_, text) = chat(&t, Some("alpha::a-only"), "hi").await;
    assert_eq!(text, "A a-only, my key is [REDACTED]");

    // Deleting a provider drops its key and the defaults that named it.
    let (st, _) = admin(&t, Method::DELETE, "/api/admin/providers/nope", None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, v) = admin(&t, Method::DELETE, "/api/admin/providers/beta", None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["providers"].as_array().unwrap().len(), 1);
    assert_eq!(
        v["agent_models"],
        json!({"writer": {"model": "alpha::a-only", "fallbacks": []}})
    );
    let secrets = std::fs::read_to_string(t.dir.path().join("secrets.toml")).unwrap();
    assert!(secrets.contains("provider-alpha") && !secrets.contains("provider-beta"));
    // That session follows the defaults, so its next turn gets the new one.
    admin(
        &t,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({"text": "again"})),
    )
    .await;
    let mut answers = Vec::new();
    for _ in 0..200 {
        let (_, v) = admin(
            &t,
            Method::GET,
            &format!("/api/sessions/{sid}/events"),
            None,
        )
        .await;
        let evs: Vec<RawEvent> = serde_json::from_value(v["events"].clone()).unwrap();
        answers = evs
            .iter()
            .filter(|e| e.kind == "assistant_message")
            .map(|e| e.payload["text"].as_str().unwrap().to_string())
            .collect();
        if answers.len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(answers[1], "A shared, my key is [REDACTED]");

    // Clearing the default falls back to the first model; deleting the
    // last provider brings the demo back; "demo" switches to it.
    let (_, v) = admin(
        &t,
        Method::PUT,
        "/api/admin/agent-models",
        Some(json!({"agent_models": {"writer": null}, "default_model": null})),
    )
    .await;
    assert_eq!(v["default_model"], "alpha::shared");
    assert_eq!(v["agent_models"], json!({}));
    let (_, v) = admin(&t, Method::DELETE, "/api/admin/providers/alpha", None).await;
    assert_eq!(v["configured"], false);
    assert_eq!(v["default_model"], "demo::mock");
    let (_, text) = chat(&t, None, "hello").await;
    assert!(text.starts_with("You said"), "{text}");
    let (st, v) = admin(&t, Method::POST, "/api/admin/providers/demo", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["default_model"], "demo::mock");
}

/// One prefix, one provider: action paths and other presets' ids are
/// refused with a reason, the demo can be removed, and a failed save keeps
/// the key that goes with the saved URL.
#[tokio::test]
async fn prefixes_conflicts_and_failed_saves() {
    let fake = fake_openai("A", &["m"]).await;
    let other = fake_openai("B", &["m"]).await;
    let t = start().await;
    let custom = |url: &str, key: &str| {
        json!({"preset": "custom_openai", "name": "Local", "base_url": url, "api_key": key,
               "models": [{"id": "m"}]})
    };
    for id in ["test", "demo"] {
        let (st, v) = admin(
            &t,
            Method::PUT,
            &format!("/api/admin/providers/{id}"),
            Some(custom(&fake, KEY)),
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{id}: {v}");
        assert!(v.to_string().contains("reserved"), "{v}");
    }
    let (st, v) = admin(
        &t,
        Method::PUT,
        "/api/admin/providers/openai",
        Some(custom(&fake, KEY)),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // The OpenAI preset does not silently take over the custom "openai".
    let (st, v) = admin(
        &t,
        Method::PUT,
        "/api/admin/providers/openai",
        Some(json!({"preset": "openai", "api_key": KEY, "models": [{"id": "gpt-x"}]})),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    let (_, v) = admin(&t, Method::GET, "/api/admin/providers", None).await;
    assert_eq!(v["providers"][0]["base_url"], json!(fake));

    // The demo next to a real provider, then removed again.
    let (st, _) = admin(&t, Method::POST, "/api/admin/providers/demo", None).await;
    assert_eq!(st, StatusCode::OK);
    let (st, v) = admin(&t, Method::DELETE, "/api/admin/providers/demo", None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["providers"].as_array().unwrap().len(), 1);
    assert_eq!(v["configured"], true);

    // config.toml cannot be written: the save fails, and the stored key
    // is still the one for the URL that stays saved.
    let tmp = t.dir.path().join("config.toml.tmp");
    std::fs::create_dir(&tmp).unwrap();
    let (st, v) = admin(
        &t,
        Method::PUT,
        "/api/admin/providers/openai",
        Some(custom(&other, "sk-new-key-999")),
    )
    .await;
    assert!(st.is_client_error() || st.is_server_error(), "{v}");
    std::fs::remove_dir(&tmp).unwrap();
    let secrets = std::fs::read_to_string(t.dir.path().join("secrets.toml")).unwrap();
    assert!(
        secrets.contains(KEY) && !secrets.contains("sk-new-key-999"),
        "{secrets}"
    );
    let (_, text) = chat(&t, Some("openai::m"), "hi").await;
    assert_eq!(text, "A m, my key is [REDACTED]");
}

/// A session started on the model the client showed by default still
/// follows the defaults, like one started without a model.
#[tokio::test]
async fn default_model_sent_by_the_client_still_follows_defaults() {
    let a = fake_openai("A", &["m1"]).await;
    let b = fake_openai("B", &["m2"]).await;
    let t = start().await;
    for (id, url, model) in [("alpha", &a, "m1"), ("beta", &b, "m2")] {
        let (st, v) = admin(
            &t,
            Method::PUT,
            &format!("/api/admin/providers/{id}"),
            Some(
                json!({"preset": "custom_openai", "name": id, "base_url": url,
                        "api_key": KEY, "models": [{"id": model}]}),
            ),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }
    let (_, v) = admin(&t, Method::GET, "/api/admin/providers", None).await;
    assert_eq!(v["default_model"], "alpha::m1");
    let (sid, text) = chat(&t, Some("alpha::m1"), "hi").await;
    assert_eq!(text, "A m1, my key is [REDACTED]");
    // The admin moves the default; the session's next turn follows.
    let (st, _) = admin(
        &t,
        Method::PUT,
        "/api/admin/agent-models",
        Some(json!({"agent_models": {"general": {"model": "beta::m2"}}, "default_model": "beta::m2"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    admin(
        &t,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({"text": "again"})),
    )
    .await;
    let mut answers = Vec::new();
    for _ in 0..200 {
        let (_, v) = admin(
            &t,
            Method::GET,
            &format!("/api/sessions/{sid}/events"),
            None,
        )
        .await;
        let evs: Vec<RawEvent> = serde_json::from_value(v["events"].clone()).unwrap();
        answers = evs
            .iter()
            .filter(|e| e.kind == "assistant_message")
            .map(|e| e.payload["text"].as_str().unwrap().to_string())
            .collect();
        if answers.len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(answers[1], "B m2, my key is [REDACTED]");
    // A model the user picked over the default stays.
    let (sid, _) = chat(&t, Some("alpha::m1"), "hi").await;
    let (_, s) = admin(&t, Method::GET, &format!("/api/sessions/{sid}"), None).await;
    assert_eq!(s["model"], "alpha::m1");
}
