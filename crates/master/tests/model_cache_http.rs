//! Cached model listings and their unavailable marks, the admin's override,
//! custom models, model tests (chat and embeddings), providers without chat
//! models; agent fallbacks, notices about unavailable models, avatars.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use harness_core::{Secret, SecretSet};
use harness_master::auth::{Auth, Claim};
use harness_master::config::Config;
use harness_master::providers::ProviderRuntime;
use harness_master::{Master, router};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

const KEY: &str = "sk-fake-key-123456";

/// A provider whose listing can change and fail, counting listings. Chat
/// answers "OK" (`missing` is unknown, `reasoner` refuses `max_tokens`),
/// embeddings have 3 dimensions; OpenAI and Anthropic auth both work.
#[derive(Clone, Default)]
struct Fake {
    listed: Arc<Mutex<Vec<&'static str>>>,
    lists: Arc<AtomicUsize>,
    down: Arc<AtomicBool>,
    last_chat: Arc<Mutex<Value>>,
}

impl Fake {
    fn list(&self, ids: &[&'static str]) {
        *self.listed.lock().unwrap() = ids.to_vec();
    }
    fn lists(&self) -> usize {
        self.lists.load(Ordering::SeqCst)
    }
}

fn authed(h: &HeaderMap) -> bool {
    let v = |n: &str| h.get(n).and_then(|v| v.to_str().ok()).map(str::to_string);
    v("authorization") == Some(format!("Bearer {KEY}")) || v("x-api-key").as_deref() == Some(KEY)
}

async fn fake(f: Fake) -> String {
    let (f1, f2, f3) = (f.clone(), f.clone(), f.clone());
    let app = axum::Router::new()
        .route(
            "/v1/models",
            get(move |h: HeaderMap| {
                let f = f1.clone();
                async move {
                    f.lists.fetch_add(1, Ordering::SeqCst);
                    if f.down.load(Ordering::SeqCst) {
                        return (StatusCode::SERVICE_UNAVAILABLE, "down").into_response();
                    }
                    if !authed(&h) {
                        return (StatusCode::UNAUTHORIZED, "bad key").into_response();
                    }
                    let data: Vec<Value> = f
                        .listed
                        .lock()
                        .unwrap()
                        .iter()
                        .map(|m| json!({"id": m, "context_length": 4096}))
                        .collect();
                    axum::Json(json!({ "data": data })).into_response()
                }
            }),
        )
        .route(
            "/v1/chat/completions",
            post(move |h: HeaderMap, body: String| {
                let f = f2.clone();
                async move {
                    if !authed(&h) {
                        // Like some providers: the wrong key, echoed.
                        let sent = h.get("authorization").and_then(|v| v.to_str().ok());
                        let msg = format!("Incorrect API key provided: {}", sent.unwrap_or(""));
                        return (StatusCode::UNAUTHORIZED, msg).into_response();
                    }
                    let req: Value = serde_json::from_str(&body).unwrap();
                    *f.last_chat.lock().unwrap() = req.clone();
                    match req["model"].as_str().unwrap() {
                        "missing" => (StatusCode::NOT_FOUND, "model not found").into_response(),
                        "reasoner" if req.get("max_tokens").is_some() => (
                            StatusCode::BAD_REQUEST,
                            "Unsupported parameter: 'max_tokens'. Use 'max_completion_tokens'.",
                        )
                            .into_response(),
                        _ => axum::Json(json!({
                            "choices": [{"message": {"role": "assistant", "content": "OK\n\u{202E}!"}}]
                        }))
                        .into_response(),
                    }
                }
            }),
        )
        .route(
            "/v1/messages",
            post(move |h: HeaderMap| async move {
                if !authed(&h) {
                    return (StatusCode::UNAUTHORIZED, "bad key").into_response();
                }
                axum::Json(json!({"content": [{"type": "text", "text": "OK."}]})).into_response()
            }),
        )
        .route(
            "/v1/embeddings",
            post(move |h: HeaderMap| {
                let _f = f3.clone();
                async move {
                    if !authed(&h) {
                        return (StatusCode::UNAUTHORIZED, "bad key").into_response();
                    }
                    axum::Json(json!({"data": [{"embedding": [0.1, 0.2, 0.3]}]})).into_response()
                }
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
    master: Master,
}

async fn start() -> T {
    let dir = tempfile::tempdir().unwrap();
    let cfg = Config::load(&dir.path().join("config.toml")).unwrap();
    let (rt, _env) = ProviderRuntime::start(cfg, dir.path(), SecretSet::default()).unwrap();
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
    master.set_provider_runtime(Arc::new(rt));
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(l, router(m2)).await.unwrap() });
    let mut t = T {
        base: format!("http://{addr}"),
        dir,
        admin: String::new(),
        master,
    };
    let r = reqwest::Client::new()
        .post(format!("{}/api/setup", t.base))
        .json(&json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    t.admin = r.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    t
}

async fn call(t: &T, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut rb = reqwest::Client::new()
        .request(method, format!("{}{path}", t.base))
        .header("cookie", &t.admin);
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    let r = rb.send().await.unwrap();
    (r.status(), r.json().await.unwrap_or(Value::Null))
}

async fn put_local(t: &T, url: &str, models: Value) -> (StatusCode, Value) {
    call(
        t,
        Method::PUT,
        "/api/admin/providers/loc",
        Some(
            json!({"preset": "custom_openai", "name": "Local", "base_url": url,
                    "api_key": KEY, "models": models}),
        ),
    )
    .await
}

fn ids(v: &Value) -> Vec<&str> {
    v["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect()
}

fn saved(state: &Value, raw: &str) -> Value {
    state["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "loc")
        .and_then(|p| {
            p["models"]
                .as_array()
                .unwrap()
                .iter()
                .find(|m| m["id"] == raw)
        })
        .cloned()
        .unwrap_or(Value::Null)
}

/// GET fetches once and then serves the cache; refresh fetches, marks a
/// model gone from the listing (kept), clears it when it is back, never
/// marks a custom model, and a failed fetch changes nothing.
#[tokio::test]
async fn listing_cache_marks_and_clears() {
    let f = Fake::default();
    f.list(&["a", "b", "c"]);
    let url = fake(f.clone()).await;
    let t = start().await;
    let mine = json!({"id": "mine", "custom": true, "context": 1000, "max_output": 50,
                      "vision": true, "tools": true, "effort": true});
    let (st, v) = put_local(&t, &url, json!([{"id": "a"}, {"id": "b"}, mine])).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(saved(&v, "mine")["custom"], true);

    let (st, v) = call(&t, Method::GET, "/api/admin/providers/loc/models", None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(f.lists(), 1, "no cache: fetched once");
    assert!(v["fetched_ms"].as_i64().unwrap() > 0);
    // Listed first, then saved models the listing lacks.
    assert_eq!(ids(&v), ["a", "b", "c", "mine"]);
    assert_eq!(v["models"][0]["context"], 4096);
    assert_eq!(v["models"][3]["custom"], true);
    let (_, again) = call(&t, Method::GET, "/api/admin/providers/loc/models", None).await;
    assert_eq!(f.lists(), 1, "served from the cache");
    assert_eq!(again["fetched_ms"], v["fetched_ms"]);

    // `b` leaves the listing: marked, kept, everywhere.
    f.list(&["a", "c"]);
    let (st, v) = call(
        &t,
        Method::POST,
        "/api/admin/providers/loc/models/refresh",
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(f.lists(), 2);
    assert_eq!(ids(&v), ["a", "c", "b", "mine"]);
    assert_eq!(v["models"][2]["unavailable"], true);
    let (_, state) = call(&t, Method::GET, "/api/admin/providers", None).await;
    let b = saved(&state, "b");
    assert_eq!(b["unavailable"], true);
    let since = b["missing_since"].as_i64().expect("missing_since");
    assert_eq!(saved(&state, "a")["unavailable"], false);
    assert_eq!(saved(&state, "a")["missing_since"], Value::Null);
    let custom = saved(&state, "mine");
    assert_eq!(
        (
            &custom["unavailable"],
            &custom["context"],
            &custom["effort"]
        ),
        (&json!(false), &json!(1000), &json!(true)),
        "custom models keep their hand-set parameters and are never marked"
    );
    let (_, models) = call(&t, Method::GET, "/api/models", None).await;
    let flag = |id: &str| {
        models["models"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == id)
            .unwrap()["unavailable"]
            .clone()
    };
    assert_eq!(
        (flag("loc::a"), flag("loc::b"), flag("loc::mine")),
        (json!(false), json!(true), json!(false))
    );
    let file = std::fs::read_to_string(t.dir.path().join("config.toml")).unwrap();
    assert!(file.contains("unavailable = true"), "{file}");

    // Saving the provider again keeps the mark and its time; the client's
    // own flags are ignored.
    let (st, v) = put_local(
        &t,
        &url,
        json!([{"id": "a", "unavailable": true}, {"id": "b"}, mine]),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(saved(&v, "a")["unavailable"], false);
    assert_eq!(saved(&v, "b")["missing_since"], since);

    // A failed fetch is an error and changes nothing.
    f.down.store(true, Ordering::SeqCst);
    f.list(&["a", "b"]);
    let (st, v) = call(
        &t,
        Method::POST,
        "/api/admin/providers/loc/models/refresh",
        None,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_GATEWAY, "{v}");
    let (_, state) = call(&t, Method::GET, "/api/admin/providers", None).await;
    assert_eq!(saved(&state, "b")["unavailable"], true);
    let (_, cached) = call(&t, Method::GET, "/api/admin/providers/loc/models", None).await;
    assert_eq!(
        ids(&cached),
        ["a", "c", "b", "mine"],
        "the old listing stays"
    );

    // Back again: cleared.
    f.down.store(false, Ordering::SeqCst);
    let (_, v) = call(
        &t,
        Method::POST,
        "/api/admin/providers/loc/models/refresh",
        None,
    )
    .await;
    assert_eq!(ids(&v), ["a", "b", "mine"]);
    let (_, state) = call(&t, Method::GET, "/api/admin/providers", None).await;
    assert_eq!(saved(&state, "b")["unavailable"], false);
    assert_eq!(saved(&state, "b")["missing_since"], Value::Null);

    // A provider test for a saved provider at its URL keeps the listing.
    f.list(&["a"]);
    let (_, v) = call(
        &t,
        Method::POST,
        "/api/admin/providers/test",
        Some(json!({"preset": "custom_openai", "base_url": url, "provider_id": "loc"})),
    )
    .await;
    assert_eq!(v["ok"], true, "{v}");
    let n = f.lists();
    let (_, v) = call(&t, Method::GET, "/api/admin/providers/loc/models", None).await;
    assert_eq!(f.lists(), n, "the test's listing is the cache");
    assert_eq!(ids(&v), ["a", "b", "mine"]);
    let (_, state) = call(&t, Method::GET, "/api/admin/providers", None).await;
    assert_eq!(saved(&state, "b")["unavailable"], true);

    // A test with a draft key or another preset is only returned: the saved
    // provider's cache and marks stay as they are.
    f.list(&["a", "b"]);
    let (_, v) = call(
        &t,
        Method::POST,
        "/api/admin/providers/loc/models/refresh",
        None,
    )
    .await;
    assert_eq!(ids(&v), ["a", "b", "mine"]);
    f.list(&["c"]);
    for body in [
        json!({"preset": "custom_openai", "base_url": url, "provider_id": "loc", "api_key": KEY}),
        json!({"preset": "ollama", "base_url": url, "provider_id": "loc"}),
    ] {
        let (_, v) = call(&t, Method::POST, "/api/admin/providers/test", Some(body)).await;
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(ids(&v), ["c"]);
    }
    let (_, v) = call(&t, Method::GET, "/api/admin/providers/loc/models", None).await;
    assert_eq!(ids(&v), ["a", "b", "mine"], "the cache is unchanged");
    let (_, state) = call(&t, Method::GET, "/api/admin/providers", None).await;
    assert_eq!(saved(&state, "a")["unavailable"], false);
    assert_eq!(saved(&state, "b")["unavailable"], false);

    // Unknown providers, and admins only.
    let (st, _) = call(&t, Method::GET, "/api/admin/providers/nope/models", None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let st = reqwest::get(format!("{}/api/admin/providers/loc/models", t.base))
        .await
        .unwrap()
        .status();
    assert_eq!(st, StatusCode::UNAUTHORIZED);

    // Moving the provider drops its listing; deleting it too.
    let other = fake(Fake::default()).await;
    let (st, v) = put_local(&t, &other, json!([])).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, v) = call(&t, Method::GET, "/api/admin/providers/loc/models", None).await;
    assert_eq!(ids(&v), Vec::<&str>::new(), "fetched from the new URL");
}

/// The daily task fetches a missing listing at start and marks models.
#[tokio::test]
async fn background_refresh_marks() {
    let f = Fake::default();
    f.list(&["a", "b"]);
    let url = fake(f.clone()).await;
    let t = start().await;
    put_local(&t, &url, json!([{"id": "a"}, {"id": "b"}])).await;
    f.list(&["a"]);
    t.master.start_model_refresh(Duration::from_millis(300));
    for _ in 0..200 {
        let (_, state) = call(&t, Method::GET, "/api/admin/providers", None).await;
        if saved(&state, "b")["unavailable"] == true {
            // Fresh listings are not fetched again before they are due.
            let n = f.lists();
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert!(f.lists() <= n + 1);
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("not refreshed");
}

/// The override removes a model and every reference to it; raw ids with a
/// slash are percent-encoded.
#[tokio::test]
async fn delete_override_cleans_references() {
    let f = Fake::default();
    f.list(&["a"]);
    let url = fake(f.clone()).await;
    let t = start().await;
    let (st, v) = put_local(
        &t,
        &url,
        json!([{"id": "a"}, {"id": "org/b"}, {"id": "refresh"}]),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = call(
        &t,
        Method::PUT,
        "/api/admin/agent-models",
        Some(json!({"agent_models": {
                        "general": {"model": "loc::a", "fallbacks": ["loc::org/b"]},
                        "writer": {"model": "loc::org/b", "fallbacks": ["loc::a"]},
                        "coder": {"model": "loc::org/b"}},
                    "default_model": "loc::org/b"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    call(
        &t,
        Method::POST,
        "/api/admin/providers/loc/models/refresh",
        None,
    )
    .await;

    let (st, v) = call(
        &t,
        Method::DELETE,
        "/api/admin/providers/loc/models/org%2Fb",
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(saved(&v, "org/b"), Value::Null);
    // Gone from every fallback list; a fallback takes a main model's place.
    let a = json!({"model": "loc::a", "fallbacks": []});
    assert_eq!(v["agent_models"], json!({"general": a, "writer": a}));
    assert_eq!(v["default_model"], "loc::a");
    let (_, models) = call(&t, Method::GET, "/api/models", None).await;
    assert!(!models.to_string().contains("org/b"));
    let file = std::fs::read_to_string(t.dir.path().join("config.toml")).unwrap();
    assert!(!file.contains("org/b"), "{file}");

    let (st, v) = call(
        &t,
        Method::DELETE,
        "/api/admin/providers/loc/models/refresh",
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "a model named like the action: {v}");
    assert_eq!(saved(&v, "refresh"), Value::Null);
    let (st, _) = call(
        &t,
        Method::DELETE,
        "/api/admin/providers/loc/models/zzz",
        None,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _) = call(
        &t,
        Method::DELETE,
        "/api/admin/providers/nope/models/a",
        None,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // The last model can go too: a provider may have none.
    let (st, v) = call(
        &t,
        Method::DELETE,
        "/api/admin/providers/loc/models/a",
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["providers"][0]["models"], json!([]));
}

/// A provider with no chat model (memory embeddings only) saves and loads,
/// and leaves the demo as the chat model: after onboarding's Skip, and when
/// the last chat provider is deleted.
#[tokio::test]
async fn provider_without_models() {
    let url = fake(Fake::default()).await;
    let t = start().await;
    let (st, v) = call(&t, Method::POST, "/api/admin/providers/demo", None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = put_local(&t, &url, json!([])).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let chat_left = |v: &Value, models: &Value| {
        let local = v["providers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == "loc")
            .unwrap();
        assert_eq!(local["models"], json!([]));
        assert_eq!(v["configured"], false, "{v}");
        assert_eq!(v["default_model"], "demo::mock", "{v}");
        let ids: Vec<&str> = models["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["demo::mock"]);
    };
    let (_, models) = call(&t, Method::GET, "/api/models", None).await;
    chat_left(&v, &models);
    let cfg = Config::load(&t.dir.path().join("config.toml")).unwrap();
    assert!(cfg.provider("loc").unwrap().models.is_empty());
    assert!(cfg.provider("demo").is_some());

    // A chat provider replaces the demo; deleting it brings the demo back.
    let (st, v) = call(
        &t,
        Method::PUT,
        "/api/admin/providers/chat",
        Some(
            json!({"preset": "custom_openai", "name": "Chat", "base_url": url,
                    "api_key": KEY, "models": [{"id": "a"}]}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["configured"], true);
    assert_eq!(v["default_model"], "chat::a");
    assert!(
        !v["providers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "demo")
    );
    let (st, v) = call(&t, Method::DELETE, "/api/admin/providers/chat", None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, models) = call(&t, Method::GET, "/api/models", None).await;
    chat_left(&v, &models);
}

/// Model tests: chat through a saved provider's key, a reasoning model's
/// token limit, Anthropic, embeddings; failures without the key.
#[tokio::test]
async fn model_test_chat_and_embeddings() {
    let f = Fake::default();
    f.list(&["a"]);
    let url = fake(f.clone()).await;
    let other = fake(Fake::default()).await;
    let t = start().await;
    put_local(&t, &url, json!([{"id": "a"}])).await;
    let test = |body: Value| {
        let t = &t;
        async move {
            let (st, v) = call(
                t,
                Method::POST,
                "/api/admin/providers/models/test",
                Some(body),
            )
            .await;
            assert_eq!(st, StatusCode::OK, "{v}");
            assert!(!v.to_string().contains(KEY), "{v}");
            v
        }
    };

    let v = test(json!({"model": "a", "provider_id": "loc"})).await;
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["reply"], "OK !", "one line, no bidi characters");
    assert!(v["ms"].is_u64());
    let sent = f.last_chat.lock().unwrap().clone();
    assert_eq!(sent["messages"][0]["content"], "Reply with OK.");
    assert!(sent["max_tokens"].as_u64().unwrap() <= 64);
    // A prefixed id is accepted for the provider it names.
    let v = test(json!({"model": "loc::a", "provider_id": "loc"})).await;
    assert_eq!(v["ok"], true, "{v}");
    let v = test(json!({"model": "reasoner", "provider_id": "loc"})).await;
    assert_eq!(v["ok"], true, "{v}");
    assert!(f.last_chat.lock().unwrap()["max_completion_tokens"].is_u64());
    let v = test(json!({"model": "missing", "provider_id": "loc"})).await;
    assert_eq!(v["ok"], false);
    assert!(v["error"].as_str().unwrap().contains("404"), "{v}");

    // A draft provider with its own key, either API kind.
    let v =
        test(json!({"model": "x", "preset": "custom_openai", "base_url": other, "api_key": KEY}))
            .await;
    assert_eq!(v["ok"], true, "{v}");
    let v = test(json!({"model": "claude-x", "preset": "custom_anthropic", "base_url": other, "api_key": KEY})).await;
    assert_eq!(
        (&v["ok"], &v["reply"]),
        (&json!(true), &json!("OK.")),
        "{v}"
    );
    // The saved key never goes to another URL; a wrong key's echo is scrubbed.
    let v = test(
        json!({"model": "x", "preset": "custom_openai", "base_url": other, "provider_id": "loc"}),
    )
    .await;
    assert_eq!(v["ok"], false, "{v}");
    let v = test(json!({"model": "x", "preset": "custom_openai", "base_url": other, "api_key": "sk-wrong-777"})).await;
    assert_eq!(v["ok"], false, "{v}");
    assert!(v["error"].as_str().unwrap().contains("[redacted]"), "{v}");
    assert!(!v.to_string().contains("sk-wrong-777"), "{v}");

    // Embeddings.
    let v = test(json!({"model": "embed-1", "provider_id": "loc", "memory": true})).await;
    assert_eq!(
        (&v["ok"], &v["reply"]),
        (&json!(true), &json!("3 dimensions")),
        "{v}"
    );
    let v = test(json!({"model": "embed-1", "base_url": other, "api_key": KEY, "kind": "openai", "memory": true})).await;
    assert_eq!(v["reply"], "3 dimensions", "{v}");
    let v = test(json!({"model": "embed-1", "base_url": other, "memory": true})).await;
    assert_eq!(v["ok"], false, "no key: {v}");

    // Bad input is a failed test, not an error.
    for body in [
        json!({"model": "", "provider_id": "loc"}),
        json!({"model": "a"}),
        json!({"model": "a", "provider_id": "nope"}),
        json!({"model": "a", "preset": "nope"}),
        json!({"model": "a", "base_url": "http://example.com/v1", "api_key": KEY, "memory": true}),
    ] {
        let v = test(body.clone()).await;
        assert_eq!(v["ok"], false, "{body}: {v}");
    }
}

async fn call_as(
    t: &T,
    cookie: &str,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut rb = reqwest::Client::new()
        .request(method, format!("{}{path}", t.base))
        .header("cookie", cookie);
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    let r = rb.send().await.unwrap();
    (r.status(), r.json().await.unwrap_or(Value::Null))
}

/// A member with their own password, logged in: their cookie.
async fn member(t: &T, name: &str) -> String {
    let (st, v) = call(
        t,
        Method::POST,
        "/api/users",
        Some(json!({"username": name, "password": "temporary pw"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let cookie = |r: &reqwest::Response| {
        r.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string()
    };
    let r = reqwest::Client::new()
        .post(format!("{}/api/login", t.base))
        .json(&json!({"username": name, "password": "temporary pw"}))
        .send()
        .await
        .unwrap();
    let first = cookie(&r);
    let r = reqwest::Client::new()
        .post(format!("{}/api/me/password", t.base))
        .header("cookie", &first)
        .json(&json!({"current": "temporary pw", "new": "a password of my own"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    cookie(&r)
}

async fn set_default_model(t: &T, cookie: &str, model: &str) {
    let (_, mut p) = call_as(t, cookie, Method::GET, "/api/me/settings", None).await;
    p["default_model"] = json!(model);
    let (st, v) = call_as(t, cookie, Method::PUT, "/api/me/settings", Some(p)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

fn agent<'a>(agents: &'a Value, id: &str) -> &'a Value {
    agents["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == id)
        .unwrap()
}

/// Fallback lists are checked on save; agents report their unavailable
/// models; notices go to admins for every use and to members only for an
/// agent's main model, the global default and their own default.
#[tokio::test]
async fn fallbacks_and_notices() {
    let f = Fake::default();
    f.list(&["a", "b", "c", "d", "e"]);
    let url = fake(f.clone()).await;
    let t = start().await;
    let ms = json!([{"id": "a"}, {"id": "b"}, {"id": "c"}, {"id": "d"}, {"id": "e"}]);
    let (st, v) = put_local(&t, &url, ms).await;
    assert_eq!(st, StatusCode::OK, "{v}");

    for (body, why) in [
        (
            json!({"general": {"model": "loc::a", "fallbacks": ["loc::a"]}}),
            "repeats main",
        ),
        (
            json!({"general": {"model": "loc::a", "fallbacks": ["loc::b", "loc::b"]}}),
            "repeats a fallback",
        ),
        (
            json!({"general": {"model": "a", "fallbacks": []}}),
            "no provider prefix",
        ),
        (
            json!({"general": {"model": "loc::a", "fallbacks": ["loc::nope"]}}),
            "unknown fallback",
        ),
        (
            json!({"nobody": {"model": "loc::a", "fallbacks": []}}),
            "unknown agent",
        ),
    ] {
        let (st, v) = call(
            &t,
            Method::PUT,
            "/api/admin/agent-models",
            Some(json!({ "agent_models": body })),
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{why}: {v}");
    }
    let (st, v) = call(
        &t,
        Method::PUT,
        "/api/admin/agent-models",
        Some(json!({
            "agent_models": {
                "general": {"model": "loc::a", "fallbacks": ["loc::b"]},
                "coder": {"model": "loc::c", "fallbacks": []},
                // One model may serve several agents.
                "writer": {"model": "loc::a", "fallbacks": []},
            },
            "default_model": "loc::a",
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        v["agent_models"]["general"],
        json!({"model": "loc::a", "fallbacks": ["loc::b"]})
    );
    let file = std::fs::read_to_string(t.dir.path().join("config.toml")).unwrap();
    assert!(file.contains("[agent_models.general]"), "{file}");

    let bob = member(&t, "bob").await;
    set_default_model(&t, &bob, "loc::d").await;
    set_default_model(&t, &t.admin, "loc::e").await;
    let (_, n) = call(&t, Method::GET, "/api/notices", None).await;
    assert_eq!(n["notices"], json!([]), "all available");

    // Everything but `a` leaves the listing.
    f.list(&["a"]);
    let (st, _) = call(
        &t,
        Method::POST,
        "/api/admin/providers/loc/models/refresh",
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let (_, agents) = call(&t, Method::GET, "/api/agents", None).await;
    let general = agent(&agents, "general");
    assert_eq!(general["default_model"], "loc::a");
    assert_eq!(general["fallbacks"], json!(["loc::b"]));
    assert_eq!(general["unavailable"], json!(["loc::b"]));
    assert_eq!(agent(&agents, "coder")["unavailable"], json!(["loc::c"]));
    assert_eq!(agent(&agents, "writer")["unavailable"], json!([]));
    assert_eq!(agent(&agents, "scout")["default_model"], Value::Null);
    // It runs on General's main model, with General's fallbacks.
    assert_eq!(agent(&agents, "scout")["main_model"], "loc::a");
    assert_eq!(agent(&agents, "scout")["fallbacks"], json!(["loc::b"]));

    let models = |n: &Value| -> Vec<String> {
        n["notices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["model"].as_str().unwrap().to_string())
            .collect()
    };
    let (st, n) = call(&t, Method::GET, "/api/notices", None).await;
    assert_eq!(st, StatusCode::OK, "{n}");
    assert_eq!(models(&n), ["loc::b", "loc::c", "loc::d", "loc::e"]);
    assert_eq!(
        n["notices"][0],
        json!({"id": "model_unavailable:loc::b", "kind": "model_unavailable",
               "model": "loc::b", "provider": "Local", "agents": ["general"],
               "your_default": false, "global_default": false, "can_fix": true})
    );
    assert_eq!(n["notices"][3]["your_default"], true);

    let (st, n) = call_as(&t, &bob, Method::GET, "/api/notices", None).await;
    assert_eq!(st, StatusCode::OK, "{n}");
    // Not `b` (a fallback only) nor `e` (someone else's default).
    assert_eq!(models(&n), ["loc::c", "loc::d"]);
    assert_eq!(n["notices"][0]["agents"], json!(["coder"]));
    assert_eq!(n["notices"][0]["can_fix"], false);
    assert_eq!(n["notices"][1]["your_default"], true);

    // The global default counts for everyone.
    call(
        &t,
        Method::PUT,
        "/api/admin/agent-models",
        Some(json!({"agent_models": {}, "default_model": "loc::c"})),
    )
    .await;
    let (_, n) = call_as(&t, &bob, Method::GET, "/api/notices", None).await;
    assert_eq!(n["notices"][0]["global_default"], true, "{n}");
    // Agents without a model of their own run on it.
    assert!(
        n["notices"][0]["agents"]
            .as_array()
            .unwrap()
            .contains(&json!("scout")),
        "{n}"
    );

    // The override clears the notice.
    let (st, _) = call(
        &t,
        Method::DELETE,
        "/api/admin/providers/loc/models/b",
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_, n) = call(&t, Method::GET, "/api/notices", None).await;
    assert!(!models(&n).contains(&"loc::b".to_string()), "{n}");
    let (st, _) = call(&t, Method::GET, "/api/notices", None).await;
    assert_eq!(st, StatusCode::OK);
}

/// Agents without a chain of their own, and sub-agents without a model of
/// their own, run on the default with its fallbacks; an effort goes only to
/// the models of the chain that take one.
#[tokio::test]
async fn inherited_fallbacks_and_effort() {
    let f = Fake::default();
    f.list(&["a", "b", "c"]);
    let url = fake(f.clone()).await;
    let t = start().await;
    let ms = json!([{"id": "a", "effort": true}, {"id": "b"}, {"id": "c"}]);
    let (st, v) = put_local(&t, &url, ms).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let assign = |general: Value| {
        call(
            &t,
            Method::PUT,
            "/api/admin/agent-models",
            Some(json!({"agent_models": {"general": general}})),
        )
    };

    // Main takes an effort, the fallback does not.
    let (st, v) = assign(json!({"model": "loc::a", "fallbacks": ["loc::b"]})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let spec = t
        .master
        .agent_spec_with(Some("general"), None, Some("high"))
        .unwrap();
    assert_eq!(spec.effort.as_deref(), Some("high"));
    assert_eq!(spec.no_effort, ["loc::b"]);
    // Only a fallback takes one: kept for it.
    assign(json!({"model": "loc::c", "fallbacks": ["loc::a"]})).await;
    let spec = t
        .master
        .agent_spec_with(Some("general"), None, Some("high"))
        .unwrap();
    assert_eq!(spec.effort.as_deref(), Some("high"));
    assert_eq!(spec.no_effort, ["loc::c"]);

    // General's main model becomes unavailable.
    assign(json!({"model": "loc::a", "fallbacks": ["loc::b"]})).await;
    f.list(&["b", "c"]);
    let (st, _) = call(
        &t,
        Method::POST,
        "/api/admin/providers/loc/models/refresh",
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let coder = t.master.agent_spec(Some("coder"), None).unwrap();
    assert_eq!(coder.model.as_deref(), Some("loc::a"));
    assert_eq!(coder.fallbacks, ["loc::b"], "General's, inherited");
    assert_eq!(coder.unavailable, ["loc::a"]);
    let ultra = t.master.agent_spec(Some("ultracode"), None).unwrap();
    assert_eq!(ultra.fallbacks, ["loc::b"]);
    assert!(!ultra.delegates.is_empty());
    for d in &ultra.delegates {
        assert_eq!(d.model.as_deref(), Some("loc::a"), "{}", d.agent_id);
        assert_eq!(d.fallbacks, ["loc::b"], "{}", d.agent_id);
        assert_eq!(d.unavailable, ["loc::a"], "{}", d.agent_id);
    }

    // A chain of its own wins; a model other than General's main inherits
    // nothing.
    let (st, v) = call(
        &t,
        Method::PUT,
        "/api/admin/agent-models",
        Some(json!({"agent_models": {
            "general": {"model": "loc::a", "fallbacks": ["loc::b"]},
            "coder": {"model": "loc::c", "fallbacks": []},
        }})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let coder = t.master.agent_spec(Some("coder"), None).unwrap();
    assert_eq!(coder.model.as_deref(), Some("loc::c"));
    assert!(coder.fallbacks.is_empty());
    let scout = t.master.agent_spec(Some("scout"), Some("loc::c")).unwrap();
    assert!(scout.fallbacks.is_empty());
}

/// Every agent has a stable avatar from its id until an admin sets one,
/// which is saved in config.toml; bad bodies, unknown agents and members
/// are refused.
#[tokio::test]
async fn agent_avatars() {
    let t = start().await;
    let (_, a1) = call(&t, Method::GET, "/api/agents", None).await;
    let (_, a2) = call(&t, Method::GET, "/api/agents", None).await;
    let before = agent(&a1, "coder")["avatar"].clone();
    assert_eq!(before, agent(&a2, "coder")["avatar"], "stable");
    for k in ["shape", "eyes", "accent"] {
        assert!(before[k].is_string(), "{before}");
    }

    let mine = json!({"shape": "hexagon", "eyes": "sleepy", "accent": "green"});
    let (st, v) = call(
        &t,
        Method::PUT,
        "/api/admin/agents/coder/avatar",
        Some(mine.clone()),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!((&v["id"], &v["avatar"]), (&json!("coder"), &mine));
    let (_, agents) = call(&t, Method::GET, "/api/agents", None).await;
    assert_eq!(agent(&agents, "coder")["avatar"], mine);
    let file = std::fs::read_to_string(t.dir.path().join("config.toml")).unwrap();
    assert!(file.contains("[agent_avatars.coder]"), "{file}");
    assert!(file.contains("shape = \"hexagon\""), "{file}");

    for (path, body, want) in [
        (
            "coder",
            json!({"shape": "star", "eyes": "round", "accent": "blue"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            "coder",
            json!({"shape": "circle", "eyes": "round"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            "coder",
            json!({"shape": "circle", "eyes": "round", "accent": "red"}),
            StatusCode::BAD_REQUEST,
        ),
        ("nobody", mine.clone(), StatusCode::NOT_FOUND),
    ] {
        let (st, v) = call(
            &t,
            Method::PUT,
            &format!("/api/admin/agents/{path}/avatar"),
            Some(body),
        )
        .await;
        assert_eq!(st, want, "{v}");
    }
    let bob = member(&t, "bob").await;
    let (st, _) = call_as(
        &t,
        &bob,
        Method::PUT,
        "/api/admin/agents/coder/avatar",
        Some(mine),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}
