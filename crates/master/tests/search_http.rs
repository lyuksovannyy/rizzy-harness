//! Chat search, similar-chat suggestions and the classifier store.

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
        checkpoints_dir: None,
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
            ..Default::default()
        },
        ModelInfo {
            id: "thinker".into(),
            provider: "mock".into(),
            effort: true,
            vision: false,
            default: false,
            ..Default::default()
        },
    ]);
    master.add_link(chat.attach().await, true).await.unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
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

async fn login_admin(t: &T) -> String {
    let (_, c, _) = req(
        t,
        Method::POST,
        "/api/setup",
        Some(json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"})),
        None,
    )
    .await;
    c.unwrap()
}

async fn chat(t: &T, cookie: &str, text: &str) -> String {
    let (st, _, v) = req(
        t,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat", "title": text})),
        Some(cookie),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap().to_string();
    req(
        t,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({"text": text})),
        Some(cookie),
    )
    .await;
    // Wait for the mock reply to be mirrored (and indexed).
    for _ in 0..200 {
        let (_, _, row) = req(
            t,
            Method::GET,
            &format!("/api/sessions/{sid}"),
            None,
            Some(cookie),
        )
        .await;
        if row["running"] == false && row["head_seq"].as_u64().unwrap_or(0) > 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    sid
}

async fn results(t: &T, cookie: &str, q: &str, suggest: bool) -> Vec<String> {
    let path = format!(
        "/api/search?q={}{}",
        q.replace(' ', "%20")
            .replace('"', "%22")
            .replace('*', "%2A")
            .replace('(', "%28")
            .replace(')', "%29")
            .replace(':', "%3A"),
        if suggest { "&suggest=true" } else { "" }
    );
    let (st, _, v) = req(t, Method::GET, &path, None, Some(cookie)).await;
    assert_eq!(st, StatusCode::OK, "{q}: {v}");
    v["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["session_id"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn similar_chats_are_strict_safe_and_private() {
    let t = start().await;
    let admin = login_admin(&t).await;
    let tokio_sid = chat(
        &t,
        &admin,
        "how do I configure tokio runtime worker threads",
    )
    .await;
    let _other = chat(&t, &admin, "best pasta recipe with garlic").await;

    // Similar wording, typed as you go: suggested.
    assert_eq!(
        results(&t, &admin, "configure tokio runtime thr", true).await,
        std::slice::from_ref(&tokio_sid)
    );
    // Too short, unrelated, or time-sensitive: nothing.
    assert!(results(&t, &admin, "tokio", true).await.is_empty());
    assert!(
        results(&t, &admin, "garlic bread with butter tonight", true)
            .await
            .is_empty()
    );
    assert!(
        results(&t, &admin, "tokio runtime news today please", true)
            .await
            .is_empty()
    );
    // Sidebar search: all words must match.
    assert_eq!(results(&t, &admin, "pasta garlic", false).await.len(), 1);
    assert!(results(&t, &admin, "pasta tokio", false).await.is_empty());
    // FTS5 syntax in the input is harmless.
    for q in [
        "\"tokio",
        "tokio*",
        "NEAR(tokio runtime)",
        "-tokio",
        "title:tokio",
        "\"\" OR \"\"",
        ")(",
    ] {
        results(&t, &admin, q, false).await;
        results(&t, &admin, q, true).await;
    }

    // Another user cannot find alex's chats.
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
    assert!(
        results(&t, &bc, "configure tokio runtime thr", true)
            .await
            .is_empty()
    );
    assert!(results(&t, &bc, "tokio", false).await.is_empty());
}

#[tokio::test]
async fn classifier_state_is_validated_per_user() {
    let t = start().await;
    let admin = login_admin(&t).await;
    let (st, _, v) = req(&t, Method::GET, "/api/me/classifier", None, Some(&admin)).await;
    assert_eq!(st, StatusCode::OK);
    assert!(v.is_null());
    let good = json!({"version": 1, "weights": vec![0.0; 4096 * 3], "bias": [0.0, 0.0, 0.0], "labels": 3, "rules_window": [1, 0, 1], "model_window": [1, 1, 0]});
    let (st, _, _) = req(
        &t,
        Method::PUT,
        "/api/me/classifier",
        Some(good.clone()),
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_, _, v) = req(&t, Method::GET, "/api/me/classifier", None, Some(&admin)).await;
    assert_eq!(v["labels"], 3);
    for bad in [
        json!({"version": 1, "weights": [0.0], "bias": [0.0, 0.0, 0.0], "labels": 0, "rules_window": [], "model_window": []}),
        json!({"version": 2, "weights": vec![0.0; 4096 * 3], "bias": [0.0, 0.0, 0.0], "labels": 0, "rules_window": [], "model_window": []}),
        json!({"version": 1, "weights": vec![0.0; 4096 * 3], "bias": [0.0, 0.0, 0.0], "labels": 0, "rules_window": vec![1; 101], "model_window": []}),
        json!({"version": 1, "weights": vec![1e9; 4096 * 3], "bias": [0.0, 0.0, 0.0], "labels": 0, "rules_window": [], "model_window": []}),
    ] {
        let (st, _, _) = req(
            &t,
            Method::PUT,
            "/api/me/classifier",
            Some(bad),
            Some(&admin),
        )
        .await;
        assert!(st.is_client_error(), "{st}");
    }
}
