//! Gate 3 review P-7: a deny and a cancel are always safe, so an account
//! that must change its password first (an admin just reset it) can still
//! stop its own running work; it cannot allow or prompt.

mod common;
use common::{Login as _, SameOrigin as _};

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::Secret;
use harness_core::event::Profile;
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, router};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

async fn chat_node(root: &std::path::Path) -> NodeRuntime {
    NodeRuntime::start(NodeConfig {
        node_id: "master".into(),
        hostname: "h".into(),
        policy: NodePolicy {
            workspace_roots: vec![root.into()],
            ..Default::default()
        },
        store: Store::open_in_memory().unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(1),
            }),
            vec![],
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: Profile::Chat,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    })
    .await
    .unwrap()
}

struct T {
    base: String,
    _dir: tempfile::TempDir,
}

async fn start() -> T {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim = dir.path().join("claim-code");
    std::fs::write(&claim, "CLAIM").unwrap();
    let master = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim,
        }),
    );
    master.set_agents(harness_agents::AgentSet::load(&dir.path().join("none")).0);
    master
        .add_link(chat_node(dir.path()).await.attach().await, true)
        .await
        .unwrap();
    tokio::spawn(async move { axum::serve(listener, router(master)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        _dir: dir,
    }
}

async fn req(
    t: &T,
    m: Method,
    path: &str,
    body: Option<Value>,
    c: Option<&str>,
) -> reqwest::Response {
    let mut rb = reqwest::Client::new().request(m, format!("{}{path}", t.base));
    if let Some(c) = c {
        rb = rb.login(c).same_origin();
    }
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    rb.send().await.unwrap()
}

async fn status(t: &T, m: Method, path: &str, body: Option<Value>, c: &str) -> StatusCode {
    req(t, m, path, body, Some(c)).await.status()
}

async fn login(t: &T, user: &str, pw: &str) -> String {
    let r = req(
        t,
        Method::POST,
        "/api/login",
        Some(json!({"username": user, "password": pw})),
        None,
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    common::signed_in(r).await
}

#[tokio::test]
async fn an_account_that_must_change_its_password_can_still_deny_and_cancel() {
    let t = start().await;
    let alex = common::signed_in(
        req(
            &t,
            Method::POST,
            "/api/setup",
            Some(json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"})),
            None,
        )
        .await,
    )
    .await;
    let made: Value = req(
        &t,
        Method::POST,
        "/api/users",
        Some(json!({"username": "bob"})),
        Some(&alex),
    )
    .await
    .json()
    .await
    .unwrap();
    let bob_id = made["user_id"].as_str().unwrap().to_string();
    let first = login(&t, "bob", made["password"].as_str().unwrap()).await;
    let r = req(
        &t,
        Method::POST,
        "/api/me/password",
        Some(json!({"current": made["password"], "new": "bobs own password"})),
        Some(&first),
    )
    .await;
    let bob = common::signed_in(r).await;
    let sid = req(
        &t,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat"})),
        Some(&bob),
    )
    .await
    .json::<Value>()
    .await
    .unwrap()["session_id"]
        .as_str()
        .unwrap()
        .to_string();

    // The admin resets bob's password: bob must change it first.
    let reset: Value = req(
        &t,
        Method::POST,
        &format!("/api/users/{bob_id}/password"),
        None,
        Some(&alex),
    )
    .await
    .json()
    .await
    .unwrap();
    let bob = login(&t, "bob", reset["password"].as_str().unwrap()).await;
    let me: Value = req(&t, Method::GET, "/api/me", None, Some(&bob))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(me["user"]["must_change_password"], true);

    // Work is refused...
    let prompt = format!("/api/sessions/{sid}/prompt");
    let approval = format!("/api/sessions/{sid}/approvals/a1");
    assert_eq!(
        status(&t, Method::POST, &prompt, Some(json!({"text": "hi"})), &bob).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(
            &t,
            Method::POST,
            &approval,
            Some(json!({"decision": "allow"})),
            &bob
        )
        .await,
        StatusCode::FORBIDDEN
    );
    // ...stopping it is not. (The node knows no approval a1: it says so,
    // which is not the password gate's 403.)
    let cancel = format!("/api/sessions/{sid}/cancel");
    assert_eq!(
        status(&t, Method::POST, &cancel, None, &bob).await,
        StatusCode::OK
    );
    let deny = status(
        &t,
        Method::POST,
        &approval,
        Some(json!({"decision": "deny"})),
        &bob,
    )
    .await;
    assert!(
        deny != StatusCode::FORBIDDEN && deny != StatusCode::UNAUTHORIZED,
        "{deny}"
    );
    // Still only the owner's own chat.
    let other = format!("/api/sessions/{}/cancel", harness_core::SessionId::new());
    assert_eq!(
        status(&t, Method::POST, &other, None, &bob).await,
        StatusCode::NOT_FOUND
    );
}
