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
    master: Master,
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
    let app = router(master.clone());
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
    T {
        base,
        dir,
        admin,
        master,
    }
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

/// An OpenAI-compatible fake that lists one model the saved provider does
/// not have, so that refreshing marks the saved one and saves the config.
async fn fake_lister() -> String {
    let app = axum::Router::new().route(
        "/v1/models",
        axum::routing::get(|| async { axum::Json(json!({"data": [{"id": "zzz"}]})) }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    base
}

/// Review round: the model-listing routes sent a failed write of
/// `config.toml` as a 502 with the server's path (the response layer
/// replaces only 500s). It is a 500 now; a provider that does not answer
/// keeps its sentence.
#[cfg(unix)]
#[tokio::test]
async fn a_listing_that_cannot_be_recorded_is_a_500_without_the_path() {
    use std::os::unix::fs::PermissionsExt;
    let t = start("save-errors-listing-", Config::default()).await;
    t.master
        .set_outbound(harness_master::outbound::Outbound::new([
            "127.0.0.1".to_string()
        ]));
    let fake = fake_lister().await;
    let (st, _, v) = send(
        &t,
        Method::PUT,
        "/api/admin/providers/fake",
        Some(&provider_at(&fake)),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let dir = t.dir.path().to_path_buf();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    let (st, id, v) = send(
        &t,
        Method::POST,
        "/api/admin/providers/fake/models/refresh",
        None,
    )
    .await;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(st, StatusCode::INTERNAL_SERVER_ERROR, "{v}");
    assert_eq!(v, json!({"error": "internal error", "request_id": id}));
    let lines = lines_of(&id);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("config.toml"), "{}", lines[0]);
    // A provider that does not answer is still the admin's news.
    let (st, _, v) = send(
        &t,
        Method::PUT,
        "/api/admin/providers/dead",
        Some(&provider_at("http://127.0.0.1:1/v1")),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, _, v) = send(
        &t,
        Method::POST,
        "/api/admin/providers/dead/models/refresh",
        None,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_GATEWAY, "{v}");
    assert!(v["error"].as_str().is_some_and(|e| !e.is_empty()), "{v}");
}

fn provider_at(url: &str) -> Value {
    json!({
        "name": "Fake", "preset": "custom_openai", "base_url": url, "api_key": KEY,
        "models": [{"id": "m1"}]
    })
}

/// Review round: `toml` quotes the line it cannot read, and in
/// `secrets.toml` that line holds a key. A broken `secrets.toml` reaches
/// neither a response nor a log line with its content; the position is
/// given in the log.
#[cfg(unix)]
#[tokio::test]
async fn a_broken_secrets_file_shows_no_key_and_no_line() {
    const LEAK: &str = "sk-leak-probe-0003";
    let t = start("save-errors-broken-", Config::default()).await;
    let (st, _, v) = send(
        &t,
        Method::PUT,
        "/api/admin/providers/fake",
        Some(&provider(KEY)),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let secrets = t.dir.path().join("secrets.toml");
    std::fs::write(
        &secrets,
        format!("[keys]\n\"provider-fake\" = \"{LEAK} oops\n"),
    )
    .unwrap();
    let (st, id, v) = send(
        &t,
        Method::PUT,
        "/api/admin/pricing",
        Some(&json!({"currency": "EUR"})),
    )
    .await;
    assert_eq!(st, StatusCode::INTERNAL_SERVER_ERROR, "{v}");
    assert_eq!(v, json!({"error": "internal error", "request_id": id}));
    let lines = lines_of(&id);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        !lines[0].contains(LEAK) && !lines[0].contains("oops"),
        "{}",
        lines[0]
    );
    assert!(
        lines[0].contains("is not valid TOML at line 2, column"),
        "{}",
        lines[0]
    );
}

/// The same for `config.toml` (an inline key, a URL with credentials, a
/// value of the wrong type is quoted by serde too).
#[test]
fn a_broken_config_file_shows_its_position_and_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("config.toml");
    for (name, text) in [
        (
            "syntax",
            "[[providers]]\nid = \"x\"\nbase_url = \"https://user:pass-leak-0004@example.com/v1\" oops\n",
        ),
        (
            "type",
            "[[providers]]\nid = \"x\"\nbase_url = 5\n[chat]\nmodel_first_byte_timeout_s = \"pass-leak-0004\"\n",
        ),
        ("unknown", "pass-leak-0004 = 1\n"),
    ] {
        std::fs::write(&file, text).unwrap();
        let e = format!("{:#}", Config::load(&file).unwrap_err());
        assert!(
            !e.contains("pass-leak") && !e.contains("oops"),
            "{name}: {e}"
        );
        assert!(e.contains("is not valid TOML at line"), "{name}: {e}");
    }
}

/// A `secrets.toml` open to others, found while building the providers, is
/// the server's trouble (500, the path only in the log), not a 400 with the
/// path.
#[cfg(unix)]
#[tokio::test]
async fn a_secrets_file_open_to_others_found_in_apply_is_a_500() {
    use std::os::unix::fs::PermissionsExt;
    let t = start("save-errors-mode-", Config::default()).await;
    let (st, _, v) = send(
        &t,
        Method::PUT,
        "/api/admin/providers/fake",
        Some(&provider(KEY)),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    std::fs::set_permissions(
        t.dir.path().join("secrets.toml"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let (st, id, v) = send(
        &t,
        Method::PUT,
        "/api/admin/pricing",
        Some(&json!({"currency": "EUR"})),
    )
    .await;
    assert_eq!(st, StatusCode::INTERNAL_SERVER_ERROR, "{v}");
    assert_eq!(v, json!({"error": "internal error", "request_id": id}));
    assert_eq!(lines_of(&id).len(), 1);
}

/// The one refusal that still reaches the admin names the secret but not
/// the server's path.
#[cfg(unix)]
#[tokio::test]
async fn a_missing_named_secret_is_named_without_the_path() {
    let t = start("save-errors-missing-", Config::default()).await;
    let (st, _, v) = send(
        &t,
        Method::PUT,
        "/api/admin/providers/fake",
        Some(&provider(KEY)),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    std::fs::write(t.dir.path().join("secrets.toml"), "[keys]\n").unwrap();
    let (st, _, v) = send(
        &t,
        Method::PUT,
        "/api/admin/pricing",
        Some(&json!({"currency": "EUR"})),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(
        v,
        json!({"error": "provider \"fake\": secret \"provider-fake\" not found in secrets.toml"})
    );
}

/// Review round: a write that never finishes holds the edit lock for good,
/// and the background refresh loop must not wait on it for good, silently.
/// It waits a bounded time, skips the round with one log line, and tries
/// again on the next round. The write is held up on a named pipe.
#[cfg(unix)]
#[tokio::test]
async fn the_listing_loop_skips_a_round_while_the_config_is_being_written() {
    use harness_master::model_cache::ListingError;
    let t = start("save-errors-busy-", Config::default()).await;
    t.master
        .set_outbound(harness_master::outbound::Outbound::new([
            "127.0.0.1".to_string()
        ]));
    let fake = fake_lister().await;
    let (st, _, v) = send(
        &t,
        Method::PUT,
        "/api/admin/providers/busyfake",
        Some(&provider_at(&fake)),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    t.master
        .set_listing_lock_wait(std::time::Duration::from_millis(300));

    // A save that blocks inside its write of config.toml.tmp.
    let tmp = t.dir.path().join("config.toml.tmp");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&tmp)
            .status()
            .unwrap()
            .success()
    );
    let (base, admin) = (t.base.clone(), t.admin.clone());
    let save = tokio::spawn(async move {
        reqwest::Client::new()
            .put(format!("{base}/api/admin/storage"))
            .login(&admin)
            .header("origin", &base)
            .json(&json!({"max_file_mb": 5, "account_quota_mb": 0, "server_quota_mb": 100, "min_free_mb": 0}))
            .send()
            .await
            .unwrap()
            .status()
    });
    // Wait (bounded only to fail a hang) until the section holds the lock.
    let rt = t.master.provider_runtime().unwrap();
    let began = std::time::Instant::now();
    while tokio::time::timeout(std::time::Duration::from_millis(50), rt.lock_edits())
        .await
        .is_ok()
    {
        assert!(
            began.elapsed().as_secs() < 30,
            "the save never took the lock"
        );
    }

    // The attempt returns within its bound with the busy answer.
    let at = std::time::Instant::now();
    let r = t.master.refresh_listing("busyfake").await;
    assert!(matches!(r, Err(ListingError::Busy(_))), "{r:?}");
    assert!(at.elapsed().as_secs() < 20, "{:?}", at.elapsed());

    // The loop's own round logs one line and does not wait for good.
    t.master
        .start_model_refresh(std::time::Duration::from_secs(24 * 3600));
    let needle = "the model listing was not recorded";
    let lines = || {
        let buf = logs().lock().unwrap().clone();
        String::from_utf8_lossy(&buf)
            .lines()
            .filter(|l| l.contains(needle) && l.contains("busyfake"))
            .count()
    };
    let began = std::time::Instant::now();
    while lines() == 0 {
        assert!(began.elapsed().as_secs() < 30, "the loop logged nothing");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(lines(), 1);

    // Let the write end.
    let p = tmp.clone();
    let reader = std::thread::spawn(move || std::fs::read_to_string(p));
    assert_eq!(save.await.unwrap(), StatusCode::OK);
    let _ = tokio::task::spawn_blocking(move || reader.join()).await;
}
