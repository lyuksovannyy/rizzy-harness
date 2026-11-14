//! Renaming a chat: its owner only (anyone else gets the 404 of a chat
//! that does not exist, admins too), cleaned and capped, shown in the list
//! and found by search; the log keeps the title it was created with.

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
use harness_store::{Durability, Store};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

#[allow(dead_code)]
struct T {
    base: String,
    dir: tempfile::TempDir,
    master: Master,
    chat: NodeRuntime,
}

async fn start(node_ttl: Option<Duration>) -> T {
    let dir = tempfile::tempdir().unwrap();
    let chat = NodeRuntime::start(NodeConfig {
        node_id: "master".into(),
        hostname: "h".into(),
        policy: NodePolicy::default(),
        store: Store::open(&dir.path().join("chat.db"), Durability::Full).unwrap(),
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
        incognito_ttl: node_ttl,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    })
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, "X").unwrap();
    let master = Master::new(
        Store::open(&dir.path().join("master.db"), Durability::Normal).unwrap(),
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim_path,
        }),
    );
    master.add_link(chat.attach().await, true).await.unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        dir,
        master,
        chat,
    }
}

async fn req(
    t: &T,
    method: Method,
    path: &str,
    body: Option<Value>,
    cookie: &str,
) -> (StatusCode, Value) {
    let mut rb = reqwest::Client::new().request(method, format!("{}{path}", t.base));
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    let r = rb.login(cookie).same_origin().send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

async fn login(t: &T) -> String {
    let r = reqwest::Client::new()
        .post(format!("{}/api/setup", t.base))
        .json(&json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    common::signed_in(r).await
}

async fn user(t: &T, admin: &str, name: &str) -> String {
    let pw = req(
        t,
        Method::POST,
        "/api/users",
        Some(json!({"username": name})),
        admin,
    )
    .await
    .1["password"]
        .as_str()
        .unwrap()
        .to_string();
    let r = reqwest::Client::new()
        .post(format!("{}/api/login", t.base))
        .json(&json!({"username": name, "password": pw}))
        .send()
        .await
        .unwrap();
    let c = common::signed_in(r).await;
    let r = reqwest::Client::new()
        .post(format!("{}/api/me/password", t.base))
        .login(&c)
        .same_origin()
        .json(&json!({"current": pw, "new": "their own password"}))
        .send()
        .await
        .unwrap();
    common::signed_in(r).await
}

#[tokio::test]
async fn only_the_owner_renames_and_the_title_is_cleaned() {
    let t = start(None).await;
    let alex = login(&t).await;
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat", "title": "Lunch ideas"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap().to_string();
    let path = format!("/api/sessions/{sid}/title");
    // The row exists once session_created is mirrored.
    for _ in 0..300 {
        if req(
            &t,
            Method::GET,
            &format!("/api/sessions/{sid}"),
            None,
            &alex,
        )
        .await
        .0 == StatusCode::OK
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let (st, v) = req(
        &t,
        Method::PUT,
        &path,
        Some(json!({"title": "  Zeppelin\u{0007} budget\u{202e} \n"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["title"], "Zeppelin budget");
    let (_, row) = req(
        &t,
        Method::GET,
        &format!("/api/sessions/{sid}"),
        None,
        &alex,
    )
    .await;
    assert_eq!(row["title"], "Zeppelin budget");
    let (_, list) = req(&t, Method::GET, "/api/sessions", None, &alex).await;
    assert!(
        list["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["title"] == "Zeppelin budget")
    );
    // The log keeps what it was created with.
    let (_, evs) = req(
        &t,
        Method::GET,
        &format!("/api/sessions/{sid}/events"),
        None,
        &alex,
    )
    .await;
    assert_eq!(evs["events"][0]["payload"]["title"], "Lunch ideas");

    for bad in [
        json!({"title": "  \u{0001} "}),
        json!({"title": "x".repeat(201)}),
    ] {
        let (st, v) = req(&t, Method::PUT, &path, Some(bad), &alex).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    }
    assert_eq!(
        req(
            &t,
            Method::PUT,
            &path,
            Some(json!({"title": "x".repeat(200)})),
            &alex
        )
        .await
        .0,
        StatusCode::OK
    );

    // Another account, an admin's account included, does not see it.
    let bob = user(&t, &alex, "bob").await;
    let (st, _) = req(
        &t,
        Method::PUT,
        &path,
        Some(json!({"title": "mine now"})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (bsid, _) = {
        let (_, v) = req(
            &t,
            Method::POST,
            "/api/sessions",
            Some(json!({"profile": "chat"})),
            &bob,
        )
        .await;
        (v["session_id"].as_str().unwrap().to_string(), ())
    };
    for _ in 0..300 {
        if req(
            &t,
            Method::GET,
            &format!("/api/sessions/{bsid}"),
            None,
            &bob,
        )
        .await
        .0 == StatusCode::OK
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let (st, _) = req(
        &t,
        Method::PUT,
        &format!("/api/sessions/{bsid}/title"),
        Some(json!({"title": "admin was here"})),
        &alex,
    )
    .await;
    assert_eq!(
        st,
        StatusCode::NOT_FOUND,
        "an admin cannot rename another's chat"
    );
    let (st, _) = req(
        &t,
        Method::PUT,
        "/api/sessions/not-a-uuid/title",
        Some(json!({"title": "x"})),
        &alex,
    )
    .await;
    assert!(
        st == StatusCode::BAD_REQUEST || st == StatusCode::NOT_FOUND,
        "{st}"
    );
    drop(t.chat);
}
