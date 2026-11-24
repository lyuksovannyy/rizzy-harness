//! FIX_PLAN 4.2, "Errors" (AUDIT L4; PLAN.md T3, T25):
//!
//! - the one line `response::finish` logs for a 500 goes through the
//!   master's secret scrubber (a provider key that reaches an error text
//!   shows as the marker in the log, and is not in the response);
//! - the four admin routes that save `config.toml` (token savings,
//!   storage limits, workspace policy, and the provider routes that go
//!   through `providers::apply`) answer a failed write as a 500 with the
//!   fixed sentence and no path, the cause in the log once, while every
//!   validation sentence they answer is unchanged.

use crate::capture::logs;
use crate::common;
use common::Login as _;

use std::sync::Arc;

use harness_core::{Secret, SecretSet};
use harness_master::auth::{Auth, Claim};
use harness_master::config::Config;
use harness_master::providers::ProviderRuntime;
use harness_master::{Master, router};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// Made up; long enough to be scrubbed.
const KEY: &str = "sk-made-up-key-for-save-errors-0001";

struct T {
    base: String,
    dir: tempfile::TempDir,
    admin: String,
}

/// A server whose data directory is named after `prefix` and whose runtime
/// starts with `cfg`.
async fn start(prefix: &str, cfg: Config) -> T {
    logs();
    let dir = tempfile::Builder::new().prefix(prefix).tempdir().unwrap();
    let secrets = SecretSet::default();
    let (rt, _env) = ProviderRuntime::start(cfg, dir.path(), secrets).unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let master = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: dir.path().join("claim-code"),
        }),
    );
    master.set_provider_runtime(Arc::new(rt));
    let app = router(master);
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let base = format!("http://{addr}");
    let r = reqwest::Client::new()
        .post(format!("{base}/api/setup"))
        .json(&json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let admin = common::signed_in(r).await;
    T { base, dir, admin }
}

fn browser(t: &T, method: Method, path: &str) -> reqwest::RequestBuilder {
    reqwest::Client::new()
        .request(method, format!("{}{path}", t.base))
        .login(&t.admin)
        .header("origin", &t.base)
}

/// Status, request id and parsed body.
async fn send(
    t: &T,
    method: Method,
    path: &str,
    body: Option<&Value>,
) -> (StatusCode, String, Value) {
    let mut rb = browser(t, method, path);
    if let Some(b) = body {
        rb = rb.json(b);
    }
    let r = rb.send().await.unwrap();
    let st = r.status();
    let id = r.headers()["x-request-id"].to_str().unwrap().to_string();
    let text = r.text().await.unwrap();
    (
        st,
        id,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

/// The log lines that name this request.
fn lines_of(id: &str) -> Vec<String> {
    let buf = logs().lock().unwrap().clone();
    String::from_utf8_lossy(&buf)
        .lines()
        .filter(|l| l.contains(id))
        .map(str::to_string)
        .collect()
}

/// A provider body (no network is touched to save one).
fn provider(key: &str) -> Value {
    json!({
        "name": "Fake", "preset": "custom_openai",
        "base_url": "https://api.example.com/v1", "api_key": key,
        "models": [{"id": "m1"}]
    })
}

/// PLAN.md T3: an error text that holds a configured provider key (here the
/// data directory is named after it, so the path in a refusal carries it)
/// reaches the log with the marker, and the client gets neither.
#[cfg(unix)]
#[tokio::test]
async fn the_logged_server_error_is_scrubbed_of_a_provider_key() {
    use std::os::unix::fs::PermissionsExt;
    let t = start(&format!("{KEY}-"), Config::default()).await;
    // The key becomes a configured one.
    let (st, _, v) = send(
        &t,
        Method::PUT,
        "/api/admin/providers/fake",
        Some(&provider(KEY)),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // A secrets file open to others fails the next save with an error text
    // that names the data directory.
    let secrets = t.dir.path().join("secrets.toml");
    std::fs::set_permissions(&secrets, std::fs::Permissions::from_mode(0o644)).unwrap();
    let (st, id, v) = send(
        &t,
        Method::PUT,
        "/api/admin/providers/other",
        Some(&provider("sk-another-made-up-key-0002")),
    )
    .await;
    assert_eq!(st, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(v, json!({"error": "internal error", "request_id": id}));
    let lines = lines_of(&id);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("[REDACTED]"), "{}", lines[0]);
    assert!(
        !lines[0].contains(KEY),
        "the key is in the log: {}",
        lines[0]
    );
}

/// Every route that saves `config.toml` through `save`/`apply`, with a body
/// that is valid.
fn save_routes() -> Vec<(&'static str, &'static str, Value)> {
    vec![
        ("savings", "/api/admin/savings", json!({})),
        (
            "storage",
            "/api/admin/storage",
            json!({"max_file_mb": 1, "account_quota_mb": 0, "server_quota_mb": 100, "min_free_mb": 0}),
        ),
        (
            "workspace policy",
            "/api/admin/workspaces/policy",
            json!({"members_can_create": true}),
        ),
        (
            "providers (apply)",
            "/api/admin/pricing",
            json!({"currency": "EUR"}),
        ),
    ]
}

/// A failed write of `config.toml` is the server's trouble, not the admin's:
/// 500, the fixed sentence, no path; the cause (with the path) in the log,
/// once.
#[cfg(unix)]
#[tokio::test]
async fn a_failed_config_write_is_a_500_without_the_path() {
    use std::os::unix::fs::PermissionsExt;
    let t = start("save-errors-write-", Config::default()).await;
    let dir = t.dir.path().to_path_buf();
    let dir_text = dir.display().to_string();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    let mut results = Vec::new();
    for (what, path, body) in save_routes() {
        results.push((what, send(&t, Method::PUT, path, Some(&body)).await));
    }
    // So the directory can be removed, whatever the asserts say.
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    for (what, (st, id, v)) in results {
        assert_eq!(st, StatusCode::INTERNAL_SERVER_ERROR, "{what}: {v}");
        assert_eq!(
            v,
            json!({"error": "internal error", "request_id": id}),
            "{what}"
        );
        assert!(
            !v.to_string().contains(&dir_text),
            "{what}: path in the body"
        );
        let lines = lines_of(&id);
        assert_eq!(lines.len(), 1, "{what}: {lines:?}");
        assert!(
            lines[0].contains("config.toml"),
            "{what}: the cause is logged: {}",
            lines[0]
        );
    }
}

/// The validation sentences the four routes answer, byte for byte. The
/// runtime starts with a memory backend that has no address, so the config
/// it holds fails `validate` on every save (and none of these routes, nor
/// `normalize`, repairs it).
#[tokio::test]
async fn validation_sentences_of_the_save_routes_are_unchanged() {
    let mut cfg = Config::default();
    cfg.memory.backend = harness_master::config::MemoryBackend::Mem0;
    let t = start("save-errors-invalid-", cfg).await;
    let want = "[memory] backend = \"mem0\" needs mem0_url";
    for (what, path, body) in save_routes() {
        let (st, _, v) = send(&t, Method::PUT, path, Some(&body)).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{what}: {v}");
        assert_eq!(v, json!({"error": want}), "{what}");
    }
    // Nothing was written.
    assert!(!t.dir.path().join("config.toml").exists());
}

/// The write stays atomic: a temp file and a rename. A save that cannot
/// make its temp file leaves the old `config.toml` as it was, and a save
/// that works leaves no temp file behind.
#[tokio::test]
async fn a_failed_write_leaves_the_old_file_and_a_good_one_no_temp() {
    let t = start("save-errors-atomic-", Config::default()).await;
    let file = t.dir.path().join("config.toml");
    let tmp = t.dir.path().join("config.toml.tmp");
    let body = |mb: u64| json!({"max_file_mb": mb, "account_quota_mb": 0, "server_quota_mb": 100, "min_free_mb": 0});
    let (st, _, v) = send(&t, Method::PUT, "/api/admin/storage", Some(&body(3))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let first = std::fs::read_to_string(&file).unwrap();
    assert!(first.contains("max_file_mb = 3"), "{first}");
    assert!(!tmp.exists());
    // A directory where the temp file goes: the write fails.
    std::fs::create_dir(&tmp).unwrap();
    let (st, _, v) = send(&t, Method::PUT, "/api/admin/storage", Some(&body(4))).await;
    assert_eq!(st, StatusCode::INTERNAL_SERVER_ERROR, "{v}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), first);
}
