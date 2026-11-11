//! Accounts over HTTP: claim, login, cookie sessions, logout, API token,
//! and per-user visibility.

mod common;
use common::{Login as _, SameOrigin as _};

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
    master: Master,
    claim_path: std::path::PathBuf,
    ws: std::path::PathBuf,
    /// The embedded node's own store.
    node_store: Store,
    _dir: tempfile::TempDir,
}

async fn start() -> Server {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let node_store = Store::open_in_memory().unwrap();
    let node = NodeRuntime::start(NodeConfig {
        node_id: "local".into(),
        hostname: "h".into(),
        policy: NodePolicy {
            workspace_roots: vec![dir.path().into()],
            ..Default::default()
        },
        store: node_store.clone(),
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
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
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
    // Non-admins with the agentic grant use the embedded node here; the
    // default (admins only) has its own tests in local_node_http.rs.
    let mut cfg = harness_master::config::Config::default();
    cfg.nodes.local_node_users = harness_master::config::LocalNodeUsers::Granted;
    let (rt, _) =
        harness_master::providers::ProviderRuntime::start(cfg, dir.path(), Default::default())
            .unwrap();
    master.set_provider_runtime(Arc::new(rt));
    master.add_link(node.attach().await, true).await.unwrap();
    let served = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(served)).await.unwrap() });
    Server {
        base: format!("http://{addr}"),
        master,
        claim_path,
        ws,
        node_store,
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
        rb = rb.login(c).same_origin();
    }
    let r = rb.send().await.unwrap();
    let status = r.status();
    let cookie = r
        .headers()
        .get("set-cookie")
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
    let body = r.json().await.unwrap_or(Value::Null);
    if let Some(c) = &cookie {
        common::remember(c, &body);
    }
    (status, cookie, body)
}

async fn get(
    s: &Server,
    path: &str,
    cookie: Option<&str>,
    bearer: Option<&str>,
) -> (StatusCode, Value) {
    let mut rb = reqwest::Client::new().get(format!("{}{path}", s.base));
    if let Some(c) = cookie {
        rb = rb.login(c).same_origin();
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
        rb = rb.login(c).same_origin();
    }
    let r = rb.send().await.unwrap();
    let status = r.status();
    let cookie = r
        .headers()
        .get("set-cookie")
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
    let body = r.json().await.unwrap_or(Value::Null);
    if let Some(c) = &cookie {
        common::remember(c, &body);
    }
    (status, cookie, body)
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
        Some(json!({"username": "bob"})),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (st, _, bob) = req(
        &s,
        Method::POST,
        "/api/users",
        Some(json!({"username": "Bob"})),
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{bob}");
    assert_eq!(bob["username"], "bob");
    assert_eq!(bob["role"], "member");
    assert_eq!(bob["can_agentic"], false);
    assert_eq!(bob["must_change_password"], true);
    assert!(bob.get("password_hash").is_none());
    let pw = bob["password"].as_str().unwrap().to_string();
    let (st, _, _) = req(
        &s,
        Method::POST,
        "/api/users",
        Some(json!({"username": "bob"})),
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT);

    // First login: everything but /api/me* is blocked until the password changes.
    let (st, bc) = login(&s, "bob", &pw).await;
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
    assert_eq!(
        st,
        StatusCode::FORBIDDEN,
        "a wrong password is not a lost login"
    );
    let (st, nc, _) = req(
        &s,
        Method::POST,
        "/api/me/password",
        Some(json!({"current": pw, "new": "bobs own password"})),
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
        Some(json!({"username": "bob"})),
        Some(&admin),
    )
    .await;
    let bob_id = bob["user_id"].as_str().unwrap().to_string();
    let pw = bob["password"].as_str().unwrap().to_string();
    let (_, bc) = login(&s, "bob", &pw).await;
    let (_, bc, _) = req(
        &s,
        Method::POST,
        "/api/me/password",
        Some(json!({"current": pw, "new": "bobs own password"})),
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
        Some(json!({"username": "bob"})),
        Some(&admin),
    )
    .await;
    let bob_id = bob["user_id"].as_str().unwrap().to_string();
    let pw = bob["password"].as_str().unwrap().to_string();
    let (_, bc) = login(&s, "bob", &pw).await;
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
    assert_eq!(login(&s, "bob", &pw).await.0, StatusCode::UNAUTHORIZED);
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
        Method::POST,
        &format!("/api/users/{bob_id}/password"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["must_change_password"], true);
    let again = v["password"].as_str().unwrap().to_string();
    assert_eq!(login(&s, "bob", &again).await.0, StatusCode::OK);

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
        Some(json!({"username": "bob", "password": again})),
        None,
    )
    .await;
    let (_, bc2, _) = req(
        &s,
        Method::POST,
        "/api/me/password",
        Some(json!({"current": again, "new": "bobs own password"})),
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

fn unauthorized(st: StatusCode) {
    assert_eq!(st, StatusCode::UNAUTHORIZED);
}

async fn me_status(s: &Server, cookie: &str) -> StatusCode {
    get(s, "/api/me", Some(cookie), None).await.0
}

/// Admin `alex` plus member `bob` with two logins (two browsers).
async fn admin_and_bob(s: &Server) -> (String, String, [String; 2]) {
    let admin = claim_admin(s).await;
    let (st, _, bob) = req(
        s,
        Method::POST,
        "/api/users",
        Some(json!({"username": "bob"})),
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{bob}");
    let bob_id = bob["user_id"].as_str().unwrap().to_string();
    let pw = bob["password"].as_str().unwrap().to_string();
    let a = login(s, "bob", &pw).await.1.unwrap();
    let b = login(s, "bob", &pw).await.1.unwrap();
    assert_ne!(a, b, "each login is its own session");
    for c in [&a, &b] {
        assert_eq!(me_status(s, c).await, StatusCode::OK);
    }
    (admin, bob_id, [a, b])
}

#[tokio::test]
async fn login_sets_one_opaque_session_cookie() {
    let s = start().await;
    claim_admin(&s).await;
    let r = reqwest::Client::new()
        .post(format!("{}/api/login", s.base))
        .json(&json!({"username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let set: Vec<String> = r
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .collect();
    assert_eq!(set.len(), 1, "{set:?}");
    let name = harness_master::auth::cookie_name();
    let (pair, attrs) = set[0].split_once("; ").unwrap();
    common::remember(pair, &r.json::<Value>().await.unwrap());
    assert_eq!(attrs, "HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000");
    let id = pair.strip_prefix(&format!("{name}=")).unwrap();
    assert_eq!(id.len(), 64, "256 bits, hex");
    assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
    // Only its SHA-256 is stored: the cookie value itself opens nothing
    // when it is all an attacker reads from the database.
    let store = s.master.inner_mirror();
    let hash = harness_master::auth::sha256_hex(id);
    let now = harness_core::now_ms();
    assert!(
        store
            .ui_session_user(&hash, now, 0, |_| true)
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .ui_session_user(id, now, 0, |_| true)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        me_status(&s, &format!("{name}={hash}")).await,
        StatusCode::UNAUTHORIZED,
        "the stored hash is not a login"
    );
    // The token endpoints of the previous scheme are gone.
    for path in ["/api/auth/refresh", "/api/auth/logout"] {
        let (st, _, _) = post(&s, path, json!({}), Some(pair)).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{path}");
    }
    assert_eq!(me_status(&s, pair).await, StatusCode::OK);
}

#[tokio::test]
async fn logout_ends_that_session_only_and_a_stolen_cookie_is_useless() {
    let s = start().await;
    let (admin, _, [a, b]) = admin_and_bob(&s).await;
    // Someone copied cookie `a` before bob logged out.
    let stolen = a.clone();
    let (st, cleared, _) = post(&s, "/api/logout", json!({}), Some(&a)).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        cleared.unwrap(),
        format!("{}=", harness_master::auth::cookie_name()),
        "the browser's cookie is cleared"
    );
    unauthorized(me_status(&s, &a).await);
    unauthorized(me_status(&s, &stolen).await);
    // Other logins are untouched: bob's other browser, and the admin.
    assert_eq!(me_status(&s, &b).await, StatusCode::OK);
    assert_eq!(me_status(&s, &admin).await, StatusCode::OK);
    // Logging out twice, or with no cookie, is not an error.
    assert_eq!(
        post(&s, "/api/logout", json!({}), Some(&a)).await.0,
        StatusCode::OK
    );
    assert_eq!(
        post(&s, "/api/logout", json!({}), None).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn disabling_a_user_ends_every_session_of_theirs() {
    let s = start().await;
    let (admin, bob_id, bobs) = admin_and_bob(&s).await;
    let (st, _, _) = req(
        &s,
        Method::PATCH,
        &format!("/api/users/{bob_id}"),
        Some(json!({"disabled": true})),
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    for c in &bobs {
        unauthorized(me_status(&s, c).await);
    }
    // Enabled again: the old cookies stay dead (the rows are gone, not hidden).
    req(
        &s,
        Method::PATCH,
        &format!("/api/users/{bob_id}"),
        Some(json!({"disabled": false})),
        Some(&admin),
    )
    .await;
    for c in &bobs {
        unauthorized(me_status(&s, c).await);
    }
    assert_eq!(me_status(&s, &admin).await, StatusCode::OK);
}

#[tokio::test]
async fn a_password_reset_ends_every_session_of_that_user() {
    let s = start().await;
    let (admin, bob_id, bobs) = admin_and_bob(&s).await;
    let (st, _, v) = req(
        &s,
        Method::POST,
        &format!("/api/users/{bob_id}/password"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let again = v["password"].as_str().unwrap().to_string();
    for c in &bobs {
        unauthorized(me_status(&s, c).await);
    }
    assert_eq!(me_status(&s, &admin).await, StatusCode::OK);

    // Changing your own password: every other browser is signed out and
    // this one gets a new session.
    let one = login(&s, "bob", &again).await.1.unwrap();
    let other = login(&s, "bob", &again).await.1.unwrap();
    let (st, fresh, _) = req(
        &s,
        Method::POST,
        "/api/me/password",
        Some(json!({"current": again, "new": "bobs own password"})),
        Some(&one),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let fresh = fresh.expect("a new session cookie");
    unauthorized(me_status(&s, &one).await);
    unauthorized(me_status(&s, &other).await);
    assert_eq!(me_status(&s, &fresh).await, StatusCode::OK);
}

#[tokio::test]
async fn deleting_a_user_ends_every_session_of_theirs() {
    let s = start().await;
    let (admin, bob_id, bobs) = admin_and_bob(&s).await;
    let (st, _, _) = req(
        &s,
        Method::DELETE,
        &format!("/api/users/{bob_id}"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    for c in &bobs {
        unauthorized(me_status(&s, c).await);
    }
    assert_eq!(s.master.inner_mirror().ui_session_count().unwrap(), 1);
}

#[tokio::test]
async fn an_expired_session_is_refused_and_one_in_use_slides() {
    use harness_master::auth::{SESSION_TTL_MS, cookie_name, sha256_hex};
    let s = start().await;
    let admin = claim_admin(&s).await;
    let (_, me) = get(&s, "/api/me", Some(&admin), None).await;
    let uid = me["user"]["user_id"].as_str().unwrap().to_string();
    let store = s.master.inner_mirror();
    let now = harness_core::now_ms();
    let day = 24 * 3600 * 1000;

    // Ran out a second ago.
    let expired = "e1".repeat(32);
    store
        .ui_session_create(
            &sha256_hex(&expired),
            &sha256_hex("k"),
            &uid,
            now - 31 * day,
            now - 1000,
        )
        .unwrap();
    unauthorized(me_status(&s, &format!("{}={expired}", cookie_name())).await);

    // Last used 29 days ago, one day left: using it moves the expiry to 30
    // days from now, and /api/me sends the cookie again so the browser's
    // copy lasts as long.
    let old = "0a".repeat(32);
    let key = "0b".repeat(32);
    let hash = sha256_hex(&old);
    store
        .ui_session_create(&hash, &sha256_hex(&key), &uid, now - 29 * day, now + day)
        .unwrap();
    let old_cookie = format!("{}={old}", cookie_name());
    common::remember(&old_cookie, &json!({"login_key": key}));
    let r = reqwest::Client::new()
        .get(format!("{}/api/me", s.base))
        .login(&old_cookie)
        .same_origin()
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(
        r.headers()["set-cookie"].to_str().unwrap(),
        format!(
            "{}={old}; HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000",
            cookie_name()
        )
    );
    let later = now + 2 * day;
    assert!(
        store
            .ui_session_user(&hash, later, SESSION_TTL_MS, |_| true)
            .unwrap()
            .is_some(),
        "still valid after its first expiry"
    );
    // The API token gets no cookie.
    let r = reqwest::Client::new()
        .get(format!("{}/api/me", s.base))
        .bearer_auth("api-secret")
        .send()
        .await
        .unwrap();
    assert!(r.headers().get("set-cookie").is_none());
}

/// Gate 3 review P-6: a deleted user's chats stayed on the master and on
/// the node, and only the API token could delete them. Deleting the user
/// deletes them on both.
#[tokio::test]
async fn deleting_a_user_deletes_their_chats_on_the_master_and_the_node() {
    let s = start().await;
    let admin = claim_admin(&s).await;
    let (_, _, bob) = req(
        &s,
        Method::POST,
        "/api/users",
        Some(json!({"username": "bob"})),
        Some(&admin),
    )
    .await;
    let bob_id = bob["user_id"].as_str().unwrap().to_string();
    let sid = s
        .master
        .create_session(harness_master::CreateSpec {
            profile: Some(harness_core::event::Profile::Agentic),
            title: "a chat".into(),
            cwd: s.ws.display().to_string(),
            owner_id: Some(bob_id.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(s.node_store.head_seq(sid).unwrap() >= 1);
    let (st, _, v) = req(
        &s,
        Method::DELETE,
        &format!("/api/users/{bob_id}"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let mut gone = false;
    for _ in 0..500 {
        let mirror = s.master.inner_mirror().session(sid).unwrap();
        if mirror.is_none() && s.node_store.head_seq(sid).unwrap() == 0 {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        gone,
        "a deleted user's chat is still there (mirror: {}, node head: {})",
        s.master.inner_mirror().session(sid).unwrap().is_some(),
        s.node_store.head_seq(sid).unwrap()
    );
}
