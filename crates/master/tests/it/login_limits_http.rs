//! Pre-authentication limits on `/api/login` and `/api/setup` (AUDIT S4,
//! FIX_PLAN 3.1): username format checked first, reserve-then-release
//! throttle, a bounded failure map and a cap of 4 Argon2 runs at once.

use harness_core::Secret;
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, router};
use harness_store::Store;
use reqwest::StatusCode;
use serde_json::{Value, json};

const CLAIM: &str = "ABCDE-FGHJK-MNPQR-STUVW";

struct Server {
    base: String,
    auth: Auth,
    _dir: tempfile::TempDir,
}

async fn start() -> Server {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, CLAIM).unwrap();
    let auth = Auth::new(
        Secret::new("api-secret".into()),
        vec![addr.to_string()],
        false,
    )
    .with_claim(Claim {
        code: Secret::new(CLAIM.into()),
        path: claim_path,
    });
    let master = Master::new(Store::open_in_memory().unwrap(), auth.clone());
    tokio::spawn(async move { axum::serve(listener, router(master)).await.unwrap() });
    Server {
        base: format!("http://{addr}"),
        auth,
        _dir: dir,
    }
}

async fn post(s: &Server, path: &str, body: Value) -> (StatusCode, Option<String>, Value) {
    let r = reqwest::Client::new()
        .post(format!("{}{path}", s.base))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = r.status();
    let retry = r
        .headers()
        .get("retry-after")
        .map(|v| v.to_str().unwrap().to_string());
    (status, retry, r.json().await.unwrap_or(Value::Null))
}

async fn claim_admin(s: &Server) {
    let (st, _, v) = post(
        s,
        "/api/setup",
        json!({"claim_code": CLAIM, "username": "alex", "password": "admin password"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

async fn login(s: &Server, user: &str, pw: &str) -> (StatusCode, Option<String>, Value) {
    post(s, "/api/login", json!({"username": user, "password": pw})).await
}

/// The check and the count happen under one lock, so a burst cannot pass
/// the "is throttled?" check before the first failure is recorded.
#[tokio::test]
async fn fifty_parallel_wrong_passwords_record_five_failures() {
    let s = start().await;
    claim_admin(&s).await;
    let base = s.base.clone();
    let tasks: Vec<_> = (0..50)
        .map(|_| {
            let base = base.clone();
            tokio::spawn(async move {
                reqwest::Client::new()
                    .post(format!("{base}/api/login"))
                    .json(&json!({"username": "alex", "password": "wrong password!"}))
                    .send()
                    .await
                    .unwrap()
                    .status()
            })
        })
        .collect();
    let mut wrong = 0;
    let mut refused = 0;
    for t in tasks {
        match t.await.unwrap() {
            StatusCode::UNAUTHORIZED => wrong += 1,
            StatusCode::TOO_MANY_REQUESTS => refused += 1,
            other => panic!("unexpected {other}"),
        }
    }
    assert_eq!(
        wrong,
        harness_master::auth::THROTTLE_MAX_FAILURES,
        "exactly 5 guesses checked"
    );
    assert_eq!(refused, 50 - wrong);
    // Locked: even the right password waits out the window.
    let (st, retry, _) = login(&s, "alex", "admin password").await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS);
    let secs: u64 = retry.expect("Retry-After").parse().unwrap();
    assert!((1..=15 * 60).contains(&secs), "{secs}");
    assert_eq!(s.auth.login_throttle_len(), 1);
}

/// Different usernames are not throttled per name, but Argon2 runs at most
/// 4 at a time whatever the username.
#[tokio::test]
async fn at_most_four_hashes_run_at_once() {
    let s = start().await;
    let base = s.base.clone();
    let tasks: Vec<_> = (0..24)
        .map(|i| {
            let base = base.clone();
            tokio::spawn(async move {
                reqwest::Client::new()
                    .post(format!("{base}/api/login"))
                    .json(&json!({"username": format!("user{i}"), "password": "wrong password!"}))
                    .send()
                    .await
                    .unwrap()
                    .status()
            })
        })
        .collect();
    for t in tasks {
        let st = t.await.unwrap();
        assert!(
            st == StatusCode::UNAUTHORIZED || st == StatusCode::TOO_MANY_REQUESTS,
            "{st}"
        );
    }
    let peak = s.auth.hash_peak();
    assert!(peak <= 4, "{peak} hashes ran at once");
    assert!(peak >= 2, "the requests did overlap ({peak})");
}

/// A username that cannot exist gets the same 401 as an unknown one, but
/// costs no hash and leaves nothing in the failure map.
#[tokio::test]
async fn malformed_usernames_get_the_same_401_and_leave_no_trace() {
    let s = start().await;
    let (st, _, unknown) = login(&s, "nobody", "wrong password!").await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    assert_eq!(s.auth.login_throttle_len(), 1);
    let hashes = s.auth.hashes_started();
    let long = "a".repeat(8 * 1024);
    for name in ["", "   ", &"a".repeat(33), "bad name!", "ünï", "a/b", &long] {
        let (st, _, body) = login(&s, name, "wrong password!").await;
        assert_eq!(st, StatusCode::UNAUTHORIZED, "{name:.40}");
        assert_eq!(body, unknown, "{name:.40}");
    }
    // A password that could never have been set is refused the same way.
    let (st, _, body) = login(&s, "nobody", &"p".repeat(1025)).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    assert_eq!(body, unknown);
    assert_eq!(s.auth.login_throttle_len(), 1, "no new entries");
    assert_eq!(s.auth.hashes_started(), hashes, "no hash ran");
}

/// The body is bounded before it is parsed: a 1 MiB username is never
/// deserialised, hashed or stored.
#[tokio::test]
async fn a_one_mib_username_is_refused_before_parsing() {
    let s = start().await;
    let hashes = s.auth.hashes_started();
    let huge = "a".repeat(1024 * 1024);
    for path in ["/api/login", "/api/setup"] {
        let (st, _, _) = post(
            &s,
            path,
            json!({"claim_code": CLAIM, "username": huge, "password": "wrong password!"}),
        )
        .await;
        assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE, "{path}");
    }
    assert_eq!(s.auth.login_throttle_len(), 0);
    assert_eq!(s.auth.hashes_started(), hashes);
}

/// Setup: a malformed username or a wrong code costs no hash; the error
/// body never echoes what was sent.
#[tokio::test]
async fn setup_checks_before_hashing_and_echoes_nothing() {
    let s = start().await;
    let hashes = s.auth.hashes_started();
    let (st, _, v) = post(
        &s,
        "/api/setup",
        json!({"claim_code": CLAIM, "username": "b".repeat(40), "password": "admin password"}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert!(!v.to_string().contains("bbbb"), "{v}");
    let (st, _, v) = post(
        &s,
        "/api/setup",
        json!({"claim_code": "WRONG-WRONG-WRONG-WRONG", "username": "alex", "password": "admin password"}),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    assert!(!v.to_string().contains("WRONG"), "{v}");
    assert!(!v.to_string().contains("admin password"), "{v}");
    assert_eq!(s.auth.hashes_started(), hashes);
    assert_eq!(s.auth.login_throttle_len(), 0);
    claim_admin(&s).await;
}
