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
use reqwest::StatusCode;
use serde_json::{Value, json};

struct Server {
    base: String,
    claim_path: std::path::PathBuf,
    ws: std::path::PathBuf,
    _dir: tempfile::TempDir,
    mirror: Store,
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
        secrets: vec![],
        profile: harness_core::event::Profile::Agentic,
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
    let master = Master::new(mirror.clone(), auth);
    master.add_link(node.attach().await, true).await.unwrap();
    tokio::spawn(async move { axum::serve(listener, router(master)).await.unwrap() });
    Server {
        base: format!("http://{addr}"),
        claim_path,
        ws,
        _dir: dir,
        mirror,
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

#[tokio::test]
async fn claim_login_logout_and_api_token() {
    let s = start().await;
    let (_, v) = get(&s, "/api/setup", None, None).await;
    assert_eq!(v["claimed"], false);
    assert_eq!(
        get(&s, "/api/me", None, None).await.0,
        StatusCode::UNAUTHORIZED
    );

    // Wrong code, then a weak password, then success (code is case/dash-insensitive).
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
    let (st, cookie, _) = post(&s, "/api/setup", json!({"claim_code": "abcde fghjk mnpqr stuvw", "username": "alex", "password": "long enough pw"}), None).await;
    assert_eq!(st, StatusCode::OK);
    let cookie = cookie.expect("session cookie");
    assert!(!s.claim_path.exists(), "claim file must be deleted");
    let (st, me) = get(&s, "/api/me", Some(&cookie), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(me["user"]["username"], "alex");
    assert_eq!(me["user"]["role"], "owner");

    // Claiming again is refused.
    let (st, _, _) = post(&s, "/api/setup", json!({"claim_code": "ABCDE-FGHJK-MNPQR-STUVW", "username": "eve", "password": "long enough pw"}), None).await;
    assert_eq!(st, StatusCode::CONFLICT);

    // Login: wrong password and unknown user give the same answer.
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
    let (st, c2, _) = post(
        &s,
        "/api/login",
        json!({"username": "Alex", "password": "long enough pw"}),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let c2 = c2.unwrap();

    // Logout revokes that session only.
    post(&s, "/api/logout", json!({}), Some(&c2)).await;
    assert_eq!(
        get(&s, "/api/me", Some(&c2), None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        get(&s, "/api/me", Some(&cookie), None).await.0,
        StatusCode::OK
    );

    // The cookie value is never stored in clear.
    let raw = cookie.split_once('=').unwrap().1.to_string();
    let dump = format!("{:?}", s.mirror.kv_get(&raw));
    assert!(dump.contains("None"));

    // API token works as the owner.
    let (st, me) = get(&s, "/api/me", None, Some("api-secret")).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(me["user"]["role"], "owner");
    assert_eq!(
        get(&s, "/api/me", None, Some("bad")).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn sessions_record_owner_and_members_see_only_their_own() {
    let s = start().await;
    let (_, cookie, _) = post(&s, "/api/setup", json!({"claim_code": "ABCDE-FGHJK-MNPQR-STUVW", "username": "alex", "password": "long enough pw"}), None).await;
    let cookie = cookie.unwrap();
    let (st, _, v) = post(
        &s,
        "/api/sessions",
        json!({"cwd": s.ws.display().to_string()}),
        Some(&cookie),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap().to_string();
    let (_, list) = get(&s, "/api/sessions", Some(&cookie), None).await;
    assert_eq!(list["sessions"][0]["session_id"], sid);
    assert!(list["sessions"][0]["owner_id"].is_string());

    // A member (teams, later) cannot see the owner's session, even by id.
    let phc = harness_master::auth::hash_password("member password".into()).await;
    s.mirror
        .create_user("m1", "bob", &phc, harness_store::Role::Member)
        .unwrap();
    let (_, mc, _) = post(
        &s,
        "/api/login",
        json!({"username": "bob", "password": "member password"}),
        None,
    )
    .await;
    let mc = mc.unwrap();
    let (_, list) = get(&s, "/api/sessions", Some(&mc), None).await;
    assert_eq!(list["sessions"].as_array().unwrap().len(), 0);
    let (st, _) = get(&s, &format!("/api/sessions/{sid}/events"), Some(&mc), None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _, _) = post(
        &s,
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": "hi"}),
        Some(&mc),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}
