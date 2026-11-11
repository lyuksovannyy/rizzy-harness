//! Incognito chats and deleting chats: nothing may be left behind, not even
//! in free pages or the WAL of either database.

mod common;
use common::{Login as _, SameOrigin as _};

use std::path::Path;
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

async fn chat(t: &T, cookie: &str, text: &str, incognito: bool) -> String {
    let (st, v) = req(
        t,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat", "title": "t", "incognito": incognito})),
        cookie,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap().to_string();
    let (st, v) = req(
        t,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({"text": text})),
        cookie,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    for _ in 0..300 {
        let (_, row) = req(
            t,
            Method::GET,
            &format!("/api/sessions/{sid}"),
            None,
            cookie,
        )
        .await;
        if row["running"] == false && row["head_seq"].as_u64().unwrap_or(0) > 4 {
            return sid;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("turn did not finish");
}

/// True if `needle` occurs anywhere in the database files (main, WAL, SHM).
fn on_disk(dir: &Path, needle: &str) -> bool {
    std::fs::read_dir(dir).unwrap().any(|e| {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        name.contains(".db")
            && std::fs::read(&p)
                .map(|b| b.windows(needle.len()).any(|w| w == needle.as_bytes()))
                .unwrap_or(false)
    })
}

#[tokio::test]
async fn incognito_leaves_no_rows_after_ttl() {
    let t = start(Some(Duration::from_millis(300))).await;
    let c = login(&t).await;
    let kept = chat(&t, &c, "normal chat about zebrafish", false).await;
    let gone = chat(&t, &c, "secret plan okapimarker", true).await;

    // Listed (flagged), but never searchable.
    let (_, v) = req(&t, Method::GET, "/api/sessions", None, &c).await;
    let row = v["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["session_id"] == gone.as_str())
        .unwrap()
        .clone();
    assert_eq!(row["incognito"], true);
    let (_, v) = req(&t, Method::GET, "/api/search?q=okapimarker", None, &c).await;
    assert!(v["results"].as_array().unwrap().is_empty());

    // Agentic sessions cannot be incognito.
    let (st, _) = req(
        &t,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "agentic", "cwd": "/tmp", "incognito": true})),
        &c,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // The node purges by itself once the TTL passes; the master follows.
    t.master.start_incognito_purge(Duration::from_millis(300));
    for _ in 0..300 {
        let (st, _) = req(&t, Method::GET, &format!("/api/sessions/{gone}"), None, &c).await;
        if st == StatusCode::NOT_FOUND {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let (st, _) = req(&t, Method::GET, &format!("/api/sessions/{gone}"), None, &c).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let sid: harness_core::SessionId = gone.parse().unwrap();
    assert!(t.chat.store().session(sid).unwrap().is_none());
    assert!(t.chat.store().events_after(sid, 0, 10).unwrap().is_empty());
    assert!(
        !on_disk(t.dir.path(), "okapimarker"),
        "incognito text left on disk"
    );

    // The normal chat is untouched.
    let (st, _) = req(&t, Method::GET, &format!("/api/sessions/{kept}"), None, &c).await;
    assert_eq!(st, StatusCode::OK);
}

#[tokio::test]
async fn deleting_a_chat_removes_it_everywhere() {
    let t = start(None).await;
    let c = login(&t).await;
    let sid = chat(&t, &c, "remember the capybaramarker", false).await;
    let (_, v) = req(&t, Method::GET, "/api/search?q=capybaramarker", None, &c).await;
    assert_eq!(v["results"].as_array().unwrap().len(), 1);
    assert_eq!(v["results"][0]["id"], sid.as_str());
    t.chat.store().checkpoint().unwrap();
    assert!(
        on_disk(t.dir.path(), "capybaramarker"),
        "the check itself must see live text"
    );

    let (st, v) = req(
        &t,
        Method::DELETE,
        &format!("/api/sessions/{sid}"),
        None,
        &c,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, _) = req(&t, Method::GET, &format!("/api/sessions/{sid}"), None, &c).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (_, v) = req(&t, Method::GET, "/api/search?q=capybaramarker", None, &c).await;
    assert!(v["results"].as_array().unwrap().is_empty());
    let id: harness_core::SessionId = sid.parse().unwrap();
    assert!(t.chat.store().session(id).unwrap().is_none());
    assert!(
        !on_disk(t.dir.path(), "capybaramarker"),
        "deleted text left on disk"
    );
}

async fn upload(t: &T, cookie: &str, name: &str, body: &[u8]) -> (StatusCode, Value) {
    let r = reqwest::Client::new()
        .post(format!("{}/api/attachments?name={name}", t.base))
        .login(cookie)
        .same_origin()
        .body(body.to_vec())
        .send()
        .await
        .unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn attachments_are_owned_and_used_once() {
    let t = start(None).await;
    let alex = login(&t).await;
    let (st, a) = upload(&t, &alex, "notes.md", b"# hi\nbuy milk").await;
    assert_eq!(st, StatusCode::OK, "{a}");
    assert_eq!(a["name"], "notes.md");
    let (st, _) = upload(&t, &alex, "x.bin", &[0, 1, 2]).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    let id = a["attachment_id"].as_str().unwrap().to_string();

    // Another account cannot use alex's upload.
    let pw = req(
        &t,
        Method::POST,
        "/api/users",
        Some(json!({"username": "bob"})),
        &alex,
    )
    .await
    .1["password"]
        .as_str()
        .unwrap()
        .to_string();
    let r = reqwest::Client::new()
        .post(format!("{}/api/login", t.base))
        .json(&json!({"username": "bob", "password": pw}))
        .send()
        .await
        .unwrap();
    let bob = common::signed_in(r).await;
    let r = reqwest::Client::new()
        .post(format!("{}/api/me/password", t.base))
        .login(&bob)
        .same_origin()
        .json(&json!({"current": pw, "new": "bobs own password"}))
        .send()
        .await
        .unwrap();
    let bob = common::signed_in(r).await;
    let (_, v) = req(
        &t,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat"})),
        &bob,
    )
    .await;
    let bsid = v["session_id"].as_str().unwrap().to_string();
    let (st, _) = req(
        &t,
        Method::POST,
        &format!("/api/sessions/{bsid}/prompt"),
        Some(json!({"text": "read it", "attachments": [id]})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // Alex sends it: the text is in the event; the upload is consumed.
    let (_, v) = req(
        &t,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat"})),
        &alex,
    )
    .await;
    let sid = v["session_id"].as_str().unwrap().to_string();
    let (st, v) = req(
        &t,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({"text": "read it", "attachments": [id]})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, _) = req(
        &t,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({"text": "again", "attachments": [id]})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let mut found = false;
    for _ in 0..200 {
        let (_, v) = req(
            &t,
            Method::GET,
            &format!("/api/sessions/{sid}/events"),
            None,
            &alex,
        )
        .await;
        let evs = v["events"].as_array().unwrap();
        if let Some(u) = evs.iter().find(|e| e["kind"] == "user_message") {
            assert_eq!(u["payload"]["attachments"][0]["text"], "# hi\nbuy milk");
        }
        if evs.iter().any(|e| {
            e["kind"] == "assistant_message"
                && e["payload"]["text"]
                    .as_str()
                    .unwrap_or("")
                    .contains("buy milk")
        }) {
            found = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(found, "the model saw the attachment text");
}
