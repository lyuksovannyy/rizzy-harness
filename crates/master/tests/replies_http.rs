//! Answering a ui:choices block over HTTP (rich_plan §7.1): the prompt
//! route takes a `reply` with an empty note, refuses what an answer cannot
//! carry, and the node's check comes back as 409.

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

struct T {
    base: String,
    _dir: tempfile::TempDir,
    _chat: NodeRuntime,
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
    let claim = dir.path().join("claim-code");
    std::fs::write(&claim, "X").unwrap();
    let master = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim,
        }),
    );
    master.add_link(chat.attach().await, true).await.unwrap();
    tokio::spawn(async move { axum::serve(listener, router(master)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        _dir: dir,
        _chat: chat,
    }
}

async fn req(
    t: &T,
    method: Method,
    path: &str,
    body: Option<Value>,
    cookie: &str,
) -> (StatusCode, Value) {
    let mut rb = reqwest::Client::new()
        .request(method, format!("{}{path}", t.base))
        .header("cookie", cookie);
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    let r = rb.send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

fn cookie(r: &reqwest::Response) -> String {
    let c = r.headers()["set-cookie"].to_str().unwrap();
    c.split(';').next().unwrap().to_string()
}

async fn accounts(t: &T) -> (String, String) {
    let c = reqwest::Client::new();
    let r = c
        .post(format!("{}/api/setup", t.base))
        .json(&json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    let alex = cookie(&r);
    let body = json!({"username": "bob", "password": "temporary pw"});
    req(t, Method::POST, "/api/users", Some(body.clone()), &alex).await;
    let r = c
        .post(format!("{}/api/login", t.base))
        .json(&body)
        .send()
        .await
        .unwrap();
    let r = c
        .post(format!("{}/api/me/password", t.base))
        .header("cookie", cookie(&r))
        .json(&json!({"current": "temporary pw", "new": "bobs own password"}))
        .send()
        .await
        .unwrap();
    (alex, cookie(&r))
}

/// Wait until the session's turn is over; returns its events.
async fn settled(t: &T, sid: &str, ck: &str, users: usize) -> Vec<Value> {
    for _ in 0..500 {
        let (_, v) = req(
            t,
            Method::GET,
            &format!("/api/sessions/{sid}/events"),
            None,
            ck,
        )
        .await;
        let events = v["events"].as_array().cloned().unwrap_or_default();
        let count = |k: &str| events.iter().filter(|e| e["kind"] == k).count();
        if count("user_message") == users && count("turn_ended") == users {
            return events;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("turn did not finish");
}

#[tokio::test]
async fn replies_over_http() {
    let t = start().await;
    let (alex, bob) = accounts(&t).await;
    let (_, v) = req(
        &t,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat"})),
        &alex,
    )
    .await;
    let sid = v["session_id"].as_str().unwrap().to_string();
    let prompt = format!("/api/sessions/{sid}/prompt");
    let (st, _) = req(
        &t,
        Method::POST,
        &prompt,
        Some(json!({"text": "ui: choices"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let events = settled(&t, &sid, &alex, 1).await;
    let asked = events
        .iter()
        .find(|e| e["kind"] == "assistant_message" && e["payload"]["blocks"].is_array())
        .unwrap()["payload"]["message_id"]
        .as_str()
        .unwrap()
        .to_string();
    let reply = |pick: &str| json!({"message_id": asked, "block_id": "colour", "values": {"selected": [pick]}});

    // What an answer may not come with, before anything reaches the node.
    let big = json!({"message_id": asked, "block_id": "colour", "values": {"selected": ["blue"], "other": "x".repeat(33 * 1024)}});
    for (body, err) in [
        (
            json!({"text": "", "reply": reply("blue"), "compare": ["mock", "mock-b"]}),
            "parallel answers",
        ),
        (
            json!({"text": "", "reply": reply("blue"), "attachments": ["a1"]}),
            "attachments",
        ),
        (json!({"text": "", "reply": big}), "32 KiB"),
        (json!({"text": " "}), "empty prompt"),
    ] {
        let (st, v) = req(&t, Method::POST, &prompt, Some(body), &alex).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
        assert!(v["error"].as_str().unwrap().contains(err), "{v}");
    }
    // Another account cannot even confirm the session exists.
    let (st, _) = req(
        &t,
        Method::POST,
        &prompt,
        Some(json!({"text": "", "reply": reply("blue")})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // An answer needs no note; the node records the canonical text.
    let (st, v) = req(
        &t,
        Method::POST,
        &prompt,
        Some(json!({"text": "", "reply": reply("blue")})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let events = settled(&t, &sid, &alex, 2).await;
    let answer = events
        .iter()
        .rfind(|e| e["kind"] == "user_message")
        .unwrap();
    assert_eq!(answer["payload"]["text"], "Choice answer · colour: blue");
    assert_eq!(answer["payload"]["reply"], reply("blue"));

    // Answered once: the next answer is stale.
    let (st, v) = req(
        &t,
        Method::POST,
        &prompt,
        Some(json!({"text": "", "reply": reply("red")})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"], harness_core::ui::NOT_OPEN);
}
