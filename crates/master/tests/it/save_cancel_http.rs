//! FIX_PLAN 4.2, "Errors", review round: a request that is cut by the
//! route timeout (`limits::enforce`, 30 s for these routes) must not cut a
//! save in half. The write of `config.toml` runs on a blocking thread, and
//! a dropped handler future does not stop that thread; so the whole
//! critical section (the secret write, the file write, the switch of the
//! running configuration, the restore on failure) runs in a task of its
//! own that owns the edit lock, and the handler only waits for it.
//!
//! The write is held up for as long as the test wants by making
//! `config.toml.tmp` a named pipe: opening it for writing blocks until the
//! test opens it for reading. That makes the timeout land in the middle of
//! the write every time, without a race.

#![cfg(unix)]

use crate::common;
use common::Login as _;

use std::os::unix::fs::FileTypeExt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use harness_core::Secret;
use harness_master::auth::{Auth, Claim};
use harness_master::config::Config;
use harness_master::limits::Timeouts;
use harness_master::providers::ProviderRuntime;
use harness_master::{Master, router_with};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// How long the cut request waits before the route timeout cuts it. It is
/// the only thing on a clock; long enough that the request reaches the held
/// write first on any machine.
const CUT_AFTER: Duration = Duration::from_secs(4);

const K1: &str = "sk-made-up-first-key-0001";
const K2: &str = "sk-made-up-second-key-0002";
const U1: &str = "https://first.example.com/v1";
const U2: &str = "https://second.example.com/v1";

struct T {
    /// The router with the normal timeouts: every request that is expected
    /// to succeed goes here, so nothing but the cut request is on a clock.
    base: String,
    /// The router with the short timeout, over the same `Master`: only the
    /// request that is meant to be cut goes here. It blocks on the named
    /// pipe, so its 504 is certain whatever the machine's speed.
    cut: String,
    dir: tempfile::TempDir,
    admin: String,
    rt: Arc<ProviderRuntime>,
}

async fn start(short: Duration) -> T {
    let dir = tempfile::tempdir().unwrap();
    let (rt, _env) =
        ProviderRuntime::start(Config::default(), dir.path(), Default::default()).unwrap();
    let rt = Arc::new(rt);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let l2 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr2 = l2.local_addr().unwrap();
    let master = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(
            Secret::new("tok".into()),
            vec![addr.to_string(), addr2.to_string()],
            false,
        )
        .with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: dir.path().join("claim-code"),
        }),
    );
    master.set_provider_runtime(rt.clone());
    let app = router_with(master.clone(), Timeouts::default());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let app = router_with(master, Timeouts { short, long: short });
    tokio::spawn(async move { axum::serve(l2, app).await.unwrap() });
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
        cut: format!("http://{addr2}"),
        dir,
        admin,
        rt,
    }
}

async fn put(t: &T, path: &str, body: Value) -> StatusCode {
    send(t, &t.base, path, body).await
}

/// The one request that is meant to be cut.
async fn put_cut(t: &T, path: &str, body: Value) -> StatusCode {
    send(t, &t.cut, path, body).await
}

async fn send(t: &T, base: &str, path: &str, body: Value) -> StatusCode {
    reqwest::Client::new()
        .request(Method::PUT, format!("{base}{path}"))
        .login(&t.admin)
        .header("origin", base)
        .json(&body)
        .send()
        .await
        .unwrap()
        .status()
}

fn provider(url: &str, key: &str) -> Value {
    json!({"name": "Fake", "preset": "custom_openai", "base_url": url, "api_key": key,
           "models": [{"id": "m1"}]})
}

fn storage(mb: u64) -> Value {
    json!({"max_file_mb": mb, "account_quota_mb": 0, "server_quota_mb": 100, "min_free_mb": 0})
}

fn mkfifo(p: &Path) {
    let s = std::process::Command::new("mkfifo")
        .arg(p)
        .status()
        .unwrap();
    assert!(s.success());
}

/// Let the held-up writer go: read what it writes. Returns what it wrote.
fn release(tmp: &Path) -> std::thread::JoinHandle<String> {
    let p = tmp.to_path_buf();
    std::thread::spawn(move || std::fs::read_to_string(p).unwrap())
}

/// After the writer was released: wait for every save section to end, let
/// the writer's rename happen, and put the text it wrote where the pipe
/// (renamed onto `config.toml`) is.
async fn settle(t: &T, wrote: std::thread::JoinHandle<String>) {
    let text = tokio::task::spawn_blocking(move || wrote.join().unwrap())
        .await
        .unwrap();
    // The section drops the lock when it ends, after the rename: taking the
    // lock is the signal. The bound is only there so a hang fails the test.
    let ended = tokio::time::timeout(Duration::from_secs(30), t.rt.lock_edits()).await;
    assert!(ended.is_ok(), "the save section did not end");
    drop(ended);
    let file = t.dir.path().join("config.toml");
    if std::fs::symlink_metadata(&file)
        .unwrap()
        .file_type()
        .is_fifo()
    {
        std::fs::remove_file(&file).unwrap();
        std::fs::write(&file, text).unwrap();
    }
}

/// The save that the timeout cut still finishes, and the running
/// configuration follows the file.
#[tokio::test]
async fn a_cut_save_finishes_and_the_running_config_follows_the_file() {
    let t = start(CUT_AFTER).await;
    assert_eq!(
        put(&t, "/api/admin/storage", storage(3)).await,
        StatusCode::OK
    );
    mkfifo(&t.dir.path().join("config.toml.tmp"));
    let st = put_cut(&t, "/api/admin/storage", storage(7)).await;
    assert_eq!(st, StatusCode::GATEWAY_TIMEOUT);
    let wrote = release(&t.dir.path().join("config.toml.tmp"));
    settle(&t, wrote).await;
    let file = std::fs::read_to_string(t.dir.path().join("config.toml")).unwrap();
    let running = t.rt.config().storage.max_file_mb;
    assert_eq!(
        (file.contains("max_file_mb = 7"), running),
        (true, 7),
        "the file and the running config disagree (running {running}):\n{file}"
    );
    assert!(!t.dir.path().join("config.toml.tmp").exists());
}

/// While the cut save is still writing, nobody else may start one: the edit
/// lock is still held (two writers would share the fixed temp file).
#[tokio::test]
async fn the_edit_lock_is_held_until_a_cut_save_ends() {
    let t = start(CUT_AFTER).await;
    assert_eq!(
        put(&t, "/api/admin/storage", storage(3)).await,
        StatusCode::OK
    );
    let tmp = t.dir.path().join("config.toml.tmp");
    mkfifo(&tmp);
    assert_eq!(
        put_cut(&t, "/api/admin/storage", storage(7)).await,
        StatusCode::GATEWAY_TIMEOUT
    );
    let free = tokio::time::timeout(Duration::from_millis(150), t.rt.lock_edits())
        .await
        .is_ok();
    let wrote = release(&tmp);
    settle(&t, wrote).await;
    assert!(
        !free,
        "the edit lock was free while the write was still running"
    );
}

/// A provider save cut by the timeout never leaves the new key with the old
/// URL: the key, the file and the running configuration agree afterwards.
#[tokio::test]
async fn a_cut_provider_save_never_pairs_the_new_key_with_the_old_url() {
    let t = start(CUT_AFTER).await;
    assert_eq!(
        put(&t, "/api/admin/providers/fake", provider(U1, K1)).await,
        StatusCode::OK
    );
    let tmp = t.dir.path().join("config.toml.tmp");
    mkfifo(&tmp);
    assert_eq!(
        put_cut(&t, "/api/admin/providers/fake", provider(U2, K2)).await,
        StatusCode::GATEWAY_TIMEOUT
    );
    let wrote = release(&tmp);
    settle(&t, wrote).await;
    let secrets = std::fs::read_to_string(t.dir.path().join("secrets.toml")).unwrap();
    let file = std::fs::read_to_string(t.dir.path().join("config.toml")).unwrap();
    let running = t.rt.config().provider("fake").unwrap().base_url.clone();
    let (new_key, new_file, new_running) = (secrets.contains(K2), file.contains(U2), running == U2);
    assert!(
        new_key == new_file && new_file == new_running,
        "key new: {new_key}, file new: {new_file}, running new: {new_running} ({running})"
    );
}

/// A provider save that fails to write puts the previous key back.
#[tokio::test]
async fn a_failed_provider_save_puts_the_previous_key_back() {
    let t = start(Duration::from_secs(30)).await;
    assert_eq!(
        put(&t, "/api/admin/providers/fake", provider(U1, K1)).await,
        StatusCode::OK
    );
    std::fs::create_dir(t.dir.path().join("config.toml.tmp")).unwrap();
    assert_eq!(
        put(&t, "/api/admin/providers/fake", provider(U2, K2)).await,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    let secrets = std::fs::read_to_string(t.dir.path().join("secrets.toml")).unwrap();
    assert!(secrets.contains(K1) && !secrets.contains(K2), "{secrets}");
    assert_eq!(t.rt.config().provider("fake").unwrap().base_url, U1);
}
