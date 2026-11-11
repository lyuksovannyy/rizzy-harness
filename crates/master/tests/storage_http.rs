//! Storage over HTTP: each account's own meter and the server admin's
//! numbers and limits (never names or contents).

mod common;
use common::SameOrigin as _;

use std::sync::Arc;

use harness_core::Secret;
use harness_master::auth::{Auth, Claim};
use harness_master::config::Config;
use harness_master::providers::ProviderRuntime;
use harness_master::{Master, router};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

struct T {
    base: String,
    dir: tempfile::TempDir,
    _master: Master,
}

async fn start() -> T {
    let dir = tempfile::tempdir().unwrap();
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
    let mut cfg = Config::default();
    cfg.media.enabled = true;
    let (rt, _) = ProviderRuntime::start(cfg, dir.path(), Default::default()).unwrap();
    master.set_provider_runtime(Arc::new(rt));
    harness_master::storage::open(&master, dir.path()).unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        dir,
        _master: master,
    }
}

async fn send(
    t: &T,
    method: Method,
    path: &str,
    body: Option<Value>,
    c: &str,
) -> (StatusCode, Value) {
    let mut rb = reqwest::Client::new().request(method, format!("{}{path}", t.base));
    rb = if c.starts_with("Bearer ") {
        rb.header("authorization", c)
    } else {
        rb.header("cookie", c).same_origin()
    };
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    let r = rb.send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

fn cookie(r: &reqwest::Response) -> String {
    r.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

async fn post_anon(t: &T, path: &str, body: Value, c: Option<&str>) -> reqwest::Response {
    let mut rb = reqwest::Client::new()
        .post(format!("{}{path}", t.base))
        .json(&body);
    if let Some(c) = c {
        rb = rb.header("cookie", c).same_origin();
    }
    rb.send().await.unwrap()
}

/// The server admin "alex" and a member "bob".
async fn users(t: &T) -> [String; 2] {
    let alex = cookie(
        &post_anon(
            t,
            "/api/setup",
            json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}),
            None,
        )
        .await,
    );
    let body = json!({"username": "bob"});
    let pw = send(t, Method::POST, "/api/users", Some(body), &alex)
        .await
        .1["password"]
        .as_str()
        .unwrap()
        .to_string();
    let first = post_anon(
        t,
        "/api/login",
        json!({"username": "bob", "password": pw}),
        None,
    )
    .await;
    let bob = cookie(
        &post_anon(
            t,
            "/api/me/password",
            json!({"current": pw, "new": "bobs own password"}),
            Some(&cookie(&first)),
        )
        .await,
    );
    [alex, bob]
}

fn png(tail: &[u8]) -> Vec<u8> {
    let mut b =
        b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\0\0\0\0".to_vec();
    b.extend(tail);
    b
}

async fn upload(t: &T, c: &str, bytes: Vec<u8>) -> StatusCode {
    reqwest::Client::new()
        .post(format!("{}/api/media", t.base))
        .header("cookie", c)
        .same_origin()
        .header("x-file-name", "a.png")
        .body(bytes)
        .send()
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn each_account_sees_its_own_meter() {
    let t = start().await;
    let [alex, bob] = users(&t).await;
    let a = png(&[1; 1000]);
    let b = png(&[2; 300]);
    assert_eq!(upload(&t, &alex, a.clone()).await, StatusCode::CREATED);
    assert_eq!(upload(&t, &bob, b.clone()).await, StatusCode::CREATED);
    let (st, v) = send(&t, Method::GET, "/api/me/storage", None, &alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        v,
        json!({
            "used": a.len(),
            "quota": 4096u64 << 20,
            "media": { "bytes": a.len(), "items": 1 },
            "artifacts": { "bytes": 0, "versions": 0 },
        })
    );
    let (_, v) = send(&t, Method::GET, "/api/me/storage", None, &bob).await;
    assert_eq!(v["used"], json!(b.len()));
    // The API token has no account quota.
    let (_, v) = send(&t, Method::GET, "/api/me/storage", None, "Bearer tok").await;
    assert_eq!(
        (v["used"].as_u64(), v["quota"].as_u64()),
        (Some(0), Some(0))
    );
}

#[tokio::test]
async fn admins_read_numbers_and_set_limits() {
    let t = start().await;
    let [alex, bob] = users(&t).await;
    let a = png(&[1; 2000]);
    assert_eq!(upload(&t, &bob, a.clone()).await, StatusCode::CREATED);
    let (st, _) = send(&t, Method::GET, "/api/admin/storage", None, &bob).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, v) = send(&t, Method::GET, "/api/admin/storage", None, &alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        v["settings"],
        json!({"max_file_mb": 200, "account_quota_mb": 4096, "server_quota_mb": 20480, "min_free_mb": 1024})
    );
    let usage = v["usage"].as_array().unwrap();
    assert_eq!(usage.len(), 2);
    assert_eq!(usage[0]["username"], "bob");
    assert_eq!(
        (usage[0]["used"].as_u64(), usage[0]["media_items"].as_u64()),
        (Some(a.len() as u64), Some(1))
    );
    assert_eq!(usage[0]["artifact_versions"], 0);
    assert_eq!(usage[1]["used"], 0);
    assert_eq!(v["total"], json!(a.len()));
    assert!(v["disk"]["free"].as_u64().unwrap() > 0);
    assert!(v["disk"]["total"].as_u64().unwrap() >= v["disk"]["free"].as_u64().unwrap());
    // Numbers only: no file names anywhere.
    assert!(!v.to_string().contains("a.png"));
    let (st, _) = send(&t, Method::GET, "/api/admin/storage", None, "Bearer tok").await;
    assert_eq!(st, StatusCode::OK);

    let set =
        json!({"max_file_mb": 1, "account_quota_mb": 0, "server_quota_mb": 100, "min_free_mb": 0});
    let (st, _) = send(
        &t,
        Method::PUT,
        "/api/admin/storage",
        Some(set.clone()),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    for bad in [
        json!({"max_file_mb": 0, "account_quota_mb": 0, "server_quota_mb": 100, "min_free_mb": 0}),
        json!({"max_file_mb": -1, "account_quota_mb": 0, "server_quota_mb": 100, "min_free_mb": 0}),
        json!({"max_file_mb": 1, "server_quota_mb": 100, "nonsense": 1}),
        json!({"max_file_mb": 1, "account_quota_mb": 0, "server_quota_mb": 1u64 << 40, "min_free_mb": 0}),
    ] {
        let (st, v) = send(
            &t,
            Method::PUT,
            "/api/admin/storage",
            Some(bad.clone()),
            &alex,
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{bad} → {v}");
    }
    let (st, v) = send(
        &t,
        Method::PUT,
        "/api/admin/storage",
        Some(set.clone()),
        &alex,
    )
    .await;
    assert_eq!((st, &v), (StatusCode::OK, &set));
    let (_, v) = send(&t, Method::GET, "/api/admin/storage", None, &alex).await;
    assert_eq!(v["settings"], set);
    let toml = std::fs::read_to_string(t.dir.path().join(harness_core::CONFIG_FILE)).unwrap();
    assert!(
        toml.contains("[storage]") && toml.contains("max_file_mb = 1"),
        "{toml}"
    );
    // The next upload follows the new limit; unlimited accounts show 0.
    assert_eq!(
        upload(&t, &bob, png(&vec![0; 2 << 20])).await,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let (_, v) = send(&t, Method::GET, "/api/me/storage", None, &bob).await;
    assert_eq!(v["quota"], 0);
    let (_, f) = send(&t, Method::GET, "/api/features", None, &bob).await;
    assert_eq!(f["media"]["max_file"], json!(1u64 << 20));
}
