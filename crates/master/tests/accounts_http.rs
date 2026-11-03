//! Accounts over HTTP: claim, login, cookie sessions, logout, API token,
//! and per-user visibility.

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::Secret;
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, router};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

struct Server {
    base: String,
    claim_path: std::path::PathBuf,
    ws: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

async fn start() -> Server {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let node = NodeRuntime::start(NodeConfig {
        node_id: "local".into(),
        hostname: "h".into(),
        policy: NodePolicy {
            workspace_roots: vec![dir.path().into()],
            ..Default::default()
        },
        store: Store::open_in_memory().unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(1),
            }),
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: harness_core::event::Profile::Agentic,
        incognito_ttl: None,
    })
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, "ABCDE-FGHJK-MNPQR-STUVW").unwrap();
    let mirror = Store::open_in_memory().unwrap();
    let auth = Auth::new(
        Secret::new("api-secret".into()),
        vec![addr.to_string()],
        false,
    )
    .with_claim(Claim {
        code: Secret::new("ABCDE-FGHJK-MNPQR-STUVW".into()),
        path: claim_path.clone(),
    });
    let master = Master::new(mirror, auth);
    master.add_link(node.attach().await, true).await.unwrap();
    tokio::spawn(async move { axum::serve(listener, router(master)).await.unwrap() });
    Server {
        base: format!("http://{addr}"),
        claim_path,
        ws,
        _dir: dir,
    }
}

async fn post(
    s: &Server,
    path: &str,
    body: Value,
    cookie: Option<&str>,
) -> (StatusCode, Option<String>, Value) {
    let mut rb = reqwest::Client::new()
        .post(format!("{}{path}", s.base))
        .json(&body);
    if let Some(c) = cookie {
        rb = rb.header("cookie", c);
    }
    let r = rb.send().await.unwrap();
    let status = r.status();
    let cookie = r
        .headers()
        .get("set-cookie")
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
    (status, cookie, r.json().await.unwrap_or(Value::Null))
}

async fn get(
    s: &Server,
    path: &str,
    cookie: Option<&str>,
    bearer: Option<&str>,
) -> (StatusCode, Value) {
    let mut rb = reqwest::Client::new().get(format!("{}{path}", s.base));
    if let Some(c) = cookie {
        rb = rb.header("cookie", c);
    }
    if let Some(b) = bearer {
        rb = rb.bearer_auth(b);
    }
    let r = rb.send().await.unwrap();
    (r.status(), r.json().await.unwrap_or(Value::Null))
}

const CLAIM: &str = "ABCDE-FGHJK-MNPQR-STUVW";

/// Generic JSON call with an optional cookie.
async fn req(
    s: &Server,
    method: Method,
    path: &str,
    body: Option<Value>,
    cookie: Option<&str>,
) -> (StatusCode, Option<String>, Value) {
    let mut rb = reqwest::Client::new().request(method, format!("{}{path}", s.base));
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    if let Some(c) = cookie {
        rb = rb.header("cookie", c);
    }
    let r = rb.send().await.unwrap();
    let status = r.status();
    let cookie = r
        .headers()
        .get("set-cookie")
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
    (status, cookie, r.json().await.unwrap_or(Value::Null))
}

async fn claim_admin(s: &Server) -> String {
    let (st, c, v) = post(
        s,
        "/api/setup",
        json!({"claim_code": CLAIM, "username": "alex", "password": "admin password"}),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    c.unwrap()
}

async fn login(s: &Server, user: &str, pw: &str) -> (StatusCode, Option<String>) {
    let (st, c, _) = post(
        s,
        "/api/login",
        json!({"username": user, "password": pw}),
        None,
    )
    .await;
    (st, c)
}

#[tokio::test]
async fn claim_creates_admin_and_logins_work() {
    let s = start().await;
    let (_, v) = get(&s, "/api/setup", None, None).await;
    assert_eq!(v["claimed"], false);
    assert_eq!(
        get(&s, "/api/me", None, None).await.0,
        StatusCode::UNAUTHORIZED
    );

    let (st, _, _) = post(
        &s,
        "/api/setup",
        json!({"claim_code": "WRONG", "username": "alex", "password": "long enough pw"}),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _, _) = post(
        &s,
        "/api/setup",
        json!({"claim_code": "abcdefghjkmnpqrstuvw", "username": "alex", "password": "short"}),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, cookie, _) = post(&s, "/api/setup", json!({"claim_code": "abcde fghjk mnpqr stuvw", "username": "alex", "password": "admin password"}), None).await;
    assert_eq!(st, StatusCode::OK);
    let cookie = cookie.unwrap();
    assert!(!s.claim_path.exists(), "claim file must be deleted");
    let (_, me) = get(&s, "/api/me", Some(&cookie), None).await;
    assert_eq!(me["user"]["role"], "admin");
    assert_eq!(me["user"]["can_agentic"], true);

    let (st, _, _) = post(
        &s,
        "/api/setup",
        json!({"claim_code": CLAIM, "username": "eve", "password": "long enough pw"}),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT);

    // Same answer for a wrong password and an unknown user.
    let (st1, _, e1) = post(
        &s,
        "/api/login",
        json!({"username": "alex", "password": "nope nope nope"}),
        None,
    )
    .await;
    let (st2, _, e2) = post(
        &s,
        "/api/login",
        json!({"username": "ghost", "password": "nope nope nope"}),
        None,
    )
    .await;
    assert_eq!(
        (st1, st2),
        (StatusCode::UNAUTHORIZED, StatusCode::UNAUTHORIZED)
    );
    assert_eq!(e1, e2);
    let (st, c2) = login(&s, "Alex", "admin password").await;
    assert_eq!(st, StatusCode::OK);
    let c2 = c2.unwrap();
    post(&s, "/api/logout", json!({}), Some(&c2)).await;
    assert_eq!(
        get(&s, "/api/me", Some(&c2), None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        get(&s, "/api/me", Some(&cookie), None).await.0,
        StatusCode::OK
    );

    // API token: admin rights.
    let (st, me) = get(&s, "/api/me", None, Some("api-secret")).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(me["user"]["role"], "admin");
    assert_eq!(
        get(&s, "/api/me", None, Some("bad")).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn admin_creates_member_who_must_change_password() {
    let s = start().await;
    let admin = claim_admin(&s).await;
    // Members cannot sign themselves up; only admins create accounts.
    let (st, _, _) = req(
        &s,
        Method::POST,
        "/api/users",
        Some(json!({"username": "bob", "password": "temporary pw"})),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (st, _, bob) = req(
        &s,
        Method::POST,
        "/api/users",
        Some(json!({"username": "Bob", "password": "temporary pw"})),
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{bob}");
    assert_eq!(bob["username"], "bob");
    assert_eq!(bob["role"], "member");
    assert_eq!(bob["can_agentic"], false);
    assert_eq!(bob["must_change_password"], true);
    assert!(bob.get("password_hash").is_none());
    let (st, _, _) = req(
        &s,
        Method::POST,
        "/api/users",
        Some(json!({"username": "bob", "password": "temporary pw"})),
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT);

    // First login: everything but /api/me* is blocked until the password changes.
    let (st, bc) = login(&s, "bob", "temporary pw").await;
    assert_eq!(st, StatusCode::OK);
    let bc = bc.unwrap();
    let (_, me) = get(&s, "/api/me", Some(&bc), None).await;
    assert_eq!(me["user"]["must_change_password"], true);
    assert_eq!(
        get(&s, "/api/sessions", Some(&bc), None).await.0,
        StatusCode::FORBIDDEN
    );
    let (st, _, _) = req(
        &s,
        Method::POST,
        "/api/me/password",
        Some(json!({"current": "wrong", "new": "bobs own password"})),
        Some(&bc),
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (st, nc, _) = req(
        &s,
        Method::POST,
        "/api/me/password",
        Some(json!({"current": "temporary pw", "new": "bobs own password"})),
        Some(&bc),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let nc = nc.expect("fresh cookie");
    assert_eq!(
        get(&s, "/api/me", Some(&bc), None).await.0,
        StatusCode::UNAUTHORIZED,
        "old sessions revoked"
    );
    assert_eq!(
        get(&s, "/api/sessions", Some(&nc), None).await.0,
        StatusCode::OK
    );

    // Members cannot manage users.
    assert_eq!(
        get(&s, "/api/users", Some(&nc), None).await.0,
        StatusCode::FORBIDDEN
    );
    let (_, list) = get(&s, "/api/users", Some(&admin), None).await;
    assert_eq!(list["users"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn agentic_needs_grant_and_chats_are_private() {
    let s = start().await;
    let admin = claim_admin(&s).await;
    let (_, _, bob) = req(
        &s,
        Method::POST,
        "/api/users",
        Some(json!({"username": "bob", "password": "temporary pw"})),
        Some(&admin),
    )
    .await;
    let bob_id = bob["user_id"].as_str().unwrap().to_string();
    let (_, bc) = login(&s, "bob", "temporary pw").await;
    let (_, bc, _) = req(
        &s,
        Method::POST,
        "/api/me/password",
        Some(json!({"current": "temporary pw", "new": "bobs own password"})),
        Some(&bc.unwrap()),
    )
    .await;
    let bc = bc.unwrap();

    // No grant: an agentic session (shell on the node) is refused.
    let ws = json!({"cwd": s.ws.display().to_string()});
    let (st, _, v) = req(
        &s,
        Method::POST,
        "/api/sessions",
        Some(ws.clone()),
        Some(&bc),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    // Admin grants it.
    let (st, _, v) = req(
        &s,
        Method::PATCH,
        &format!("/api/users/{bob_id}"),
        Some(json!({"can_agentic": true})),
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["can_agentic"], true);
    let (st, _, v) = req(
        &s,
        Method::POST,
        "/api/sessions",
        Some(ws.clone()),
        Some(&bc),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let bob_sid = v["session_id"].as_str().unwrap().to_string();

    // The admin's own session; neither can see the other's.
    let (_, _, v) = req(&s, Method::POST, "/api/sessions", Some(ws), Some(&admin)).await;
    let admin_sid = v["session_id"].as_str().unwrap().to_string();
    let ids = |v: &Value| -> Vec<String> {
        v["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["session_id"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(
        ids(&get(&s, "/api/sessions", Some(&bc), None).await.1),
        std::slice::from_ref(&bob_sid)
    );
    assert_eq!(
        ids(&get(&s, "/api/sessions", Some(&admin), None).await.1),
        std::slice::from_ref(&admin_sid)
    );
    assert_eq!(
        get(
            &s,
            &format!("/api/sessions/{bob_sid}/events"),
            Some(&admin),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (st, _, _) = post(
        &s,
        &format!("/api/sessions/{admin_sid}/prompt"),
        json!({"text": "hi"}),
        Some(&bc),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // The API token (machine owner) sees both.
    assert_eq!(
        ids(&get(&s, "/api/sessions", None, Some("api-secret")).await.1).len(),
        2
    );
}

#[tokio::test]
async fn disable_reset_delete_and_last_admin_guard() {
    let s = start().await;
    let admin = claim_admin(&s).await;
    let (_, me) = get(&s, "/api/me", Some(&admin), None).await;
    let admin_id = me["user"]["user_id"].as_str().unwrap().to_string();
    let (_, _, bob) = req(
        &s,
        Method::POST,
        "/api/users",
        Some(json!({"username": "bob", "password": "temporary pw"})),
        Some(&admin),
    )
    .await;
    let bob_id = bob["user_id"].as_str().unwrap().to_string();
    let (_, bc) = login(&s, "bob", "temporary pw").await;
    let bc = bc.unwrap();

    // Disabling logs bob out at once and blocks new logins.
    let (st, _, _) = req(
        &s,
        Method::PATCH,
        &format!("/api/users/{bob_id}"),
        Some(json!({"disabled": true})),
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        get(&s, "/api/me", Some(&bc), None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login(&s, "bob", "temporary pw").await.0,
        StatusCode::UNAUTHORIZED
    );
    req(
        &s,
        Method::PATCH,
        &format!("/api/users/{bob_id}"),
        Some(json!({"disabled": false})),
        Some(&admin),
    )
    .await;

    // Admin password reset forces another change.
    let (st, _, v) = req(
        &s,
        Method::PATCH,
        &format!("/api/users/{bob_id}"),
        Some(json!({"password": "another temp pw"})),
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["must_change_password"], true);
    assert_eq!(login(&s, "bob", "another temp pw").await.0, StatusCode::OK);

    // You cannot demote, disable or delete yourself, and the last admin stays.
    for (m, body) in [
        (Method::PATCH, Some(json!({"role": "member"}))),
        (Method::PATCH, Some(json!({"disabled": true}))),
        (Method::DELETE, None),
    ] {
        let (st, _, _) = req(&s, m, &format!("/api/users/{admin_id}"), body, Some(&admin)).await;
        assert_eq!(st, StatusCode::CONFLICT);
    }
    // Promote bob, then bob may demote alex: there is still one admin.
    req(
        &s,
        Method::PATCH,
        &format!("/api/users/{bob_id}"),
        Some(json!({"role": "admin"})),
        Some(&admin),
    )
    .await;
    let (_, bc2, _) = req(
        &s,
        Method::POST,
        "/api/login",
        Some(json!({"username": "bob", "password": "another temp pw"})),
        None,
    )
    .await;
    let (_, bc2, _) = req(
        &s,
        Method::POST,
        "/api/me/password",
        Some(json!({"current": "another temp pw", "new": "bobs own password"})),
        Some(&bc2.unwrap()),
    )
    .await;
    let bc2 = bc2.unwrap();
    let (st, _, _) = req(
        &s,
        Method::PATCH,
        &format!("/api/users/{admin_id}"),
        Some(json!({"role": "member"})),
        Some(&bc2),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    // Now bob is the last admin; alex (a member) cannot touch users at all.
    assert_eq!(
        get(&s, "/api/users", Some(&admin), None).await.0,
        StatusCode::FORBIDDEN
    );

    let (st, _, _) = req(
        &s,
        Method::DELETE,
        &format!("/api/users/{admin_id}"),
        None,
        Some(&bc2),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        get(&s, "/api/me", Some(&admin), None).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn login_throttle_after_repeated_failures() {
    let s = start().await;
    claim_admin(&s).await;
    for _ in 0..harness_master::auth::THROTTLE_MAX_FAILURES {
        assert_eq!(
            login(&s, "alex", "wrong password!").await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    // Even the right password is refused while throttled.
    assert_eq!(
        login(&s, "alex", "admin password").await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
}
