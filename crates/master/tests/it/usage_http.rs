//! Token totals for server admins (`GET /api/admin/usage`): numbers per
//! person and model, never anything about one chat; admins only; a bounded
//! period; a time limit that says it cut.

use crate::common;
use common::{Login as _, SameOrigin as _};

use std::time::Duration;

use harness_core::event::*;
use harness_core::{RawEvent, Secret, SessionId};
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, router};
use harness_store::Store;
use reqwest::StatusCode;
use serde_json::{Value, json};

const DAY: i64 = 86_400_000;
const TOKEN: &str = "api-secret";

struct T {
    base: String,
    store: Store,
    master: Master,
    alex: String,
    _dir: tempfile::TempDir,
}

async fn start() -> T {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, "CLAIM").unwrap();
    let auth =
        Auth::new(Secret::new(TOKEN.into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim_path,
        });
    let store = Store::open_in_memory().unwrap();
    let master = Master::new(store.clone(), auth);
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    let base = format!("http://{addr}");
    let r = reqwest::Client::new()
        .post(format!("{base}/api/setup"))
        .json(&json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let alex = common::signed_in(r).await;
    T {
        base,
        store,
        master,
        alex,
        _dir: dir,
    }
}

async fn get(t: &T, path: &str, c: Option<&str>) -> (StatusCode, Value, String) {
    let mut rb = reqwest::Client::new().get(format!("{}{path}", t.base));
    rb = match c {
        Some(c) if c.starts_with("Bearer ") => rb.header("authorization", c),
        Some(c) => rb.login(c).same_origin(),
        None => rb,
    };
    let r = rb.send().await.unwrap();
    let st = r.status();
    let text = r.text().await.unwrap();
    (st, serde_json::from_str(&text).unwrap_or(Value::Null), text)
}

async fn post(t: &T, path: &str, body: Value, c: &str) -> Value {
    let r = reqwest::Client::new()
        .post(format!("{}{path}", t.base))
        .json(&body)
        .login(c)
        .same_origin()
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    r.json().await.unwrap_or(Value::Null)
}

/// An account with its own password, logged in; its cookie.
async fn account(t: &T, name: &str) -> String {
    let v = post(
        t,
        "/api/users",
        json!({"username": name, "role": "member"}),
        &t.alex,
    )
    .await;
    let pw = v["password"].as_str().unwrap().to_string();
    let r = reqwest::Client::new()
        .post(format!("{}/api/login", t.base))
        .json(&json!({"username": name, "password": pw}))
        .send()
        .await
        .unwrap();
    let temp = common::signed_in(r).await;
    let r = reqwest::Client::new()
        .post(format!("{}/api/me/password", t.base))
        .json(&json!({"current": pw, "new": "a password of my own"}))
        .login(&temp)
        .same_origin()
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    common::signed_in(r).await
}

fn usage(input: u64, output: u64, model: &str) -> Value {
    json!({"input_tokens": input, "output_tokens": output, "context_tokens": 1,
           "by_model": {model: {"input_tokens": input, "output_tokens": output}}})
}

/// A chat of `owner` with one finished turn, a title, a folder and a node
/// that must never reach the answer. Returns its id.
fn chat(t: &T, owner: &str, parent: Option<SessionId>, ts: i64, u: Value) -> SessionId {
    let owner = t.store.user_by_username(owner).unwrap().unwrap().user_id;
    let sid = SessionId::new();
    let mut seq = 0;
    let mut push = |ts: i64, kind: &str, payload: Value| {
        seq += 1;
        t.store
            .append(
                "node-xyz-secret",
                &RawEvent {
                    session_id: sid,
                    seq,
                    ts_ms: ts,
                    kind: kind.into(),
                    payload,
                },
            )
            .unwrap();
    };
    let (kind, payload) = EventKind::SessionCreated(SessionCreated {
        exec_mode: None,
        no_callbacks: false,
        profile: Profile::Agentic,
        agent_id: None,
        incognito: false,
        parent_session: parent,
        title: "Quarterly layoffs plan".into(),
        cwd: "/home/secret-folder".into(),
        model: "m".into(),
        backend: "native".into(),
        node_id: "node-xyz-secret".into(),
        approval_mode: Default::default(),
    })
    .into_parts();
    push(ts, &kind, payload);
    t.store.claim_session(sid, Some(&owner), None).unwrap();
    push(ts + 1, "turn_started", json!({"turn_id": "t1"}));
    push(
        ts + 2,
        "turn_ended",
        json!({"turn_id": "t1", "reason": "completed", "usage": u}),
    );
    sid
}

/// alex: two chats (models a and b) and a sub-agent chat; bob: one chat.
async fn fixture(t: &T) -> Vec<SessionId> {
    let _bob = account(t, "bob").await;
    let t0 = 10 * DAY;
    let a = chat(t, "alex", None, t0, usage(100, 10, "a"));
    let b = chat(t, "alex", None, t0 + 10, usage(40, 4, "b"));
    let child = chat(t, "alex", Some(a), t0 + 20, usage(30, 3, "a"));
    let c = chat(t, "bob", None, t0 + 30, usage(11, 2, "a"));
    vec![a, b, child, c]
}

const PERIOD: &str = "/api/admin/usage?from=0&to=1000000000";

#[tokio::test]
async fn an_admin_reads_tokens_per_person_and_model() {
    let t = start().await;
    fixture(&t).await;
    let (st, v, _) = get(&t, PERIOD, Some(&t.alex)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["from"], 0);
    assert_eq!(v["to"], 1_000_000_000);
    assert!(v["cut"].is_null());
    let alex = t.store.user_by_username("alex").unwrap().unwrap().user_id;
    let bob = t.store.user_by_username("bob").unwrap().unwrap().user_id;
    // The shape, in full: the sub-agent's own chat is not added (its cost is
    // in its parent's turn).
    assert_eq!(
        v["people"],
        json!([
            {"user_id": alex, "username": "alex",
             "input_tokens": 140, "output_tokens": 14,
             "cache_read_tokens": 0, "cache_write_tokens": 0, "turns": 2,
             "models": [
                {"model": "a", "input_tokens": 100, "output_tokens": 10,
                 "cache_read_tokens": 0, "cache_write_tokens": 0, "turns": 1},
                {"model": "b", "input_tokens": 40, "output_tokens": 4,
                 "cache_read_tokens": 0, "cache_write_tokens": 0, "turns": 1}]},
            {"user_id": bob, "username": "bob",
             "input_tokens": 11, "output_tokens": 2,
             "cache_read_tokens": 0, "cache_write_tokens": 0, "turns": 1,
             "models": [
                {"model": "a", "input_tokens": 11, "output_tokens": 2,
                 "cache_read_tokens": 0, "cache_write_tokens": 0, "turns": 1}]}
        ])
    );
    assert_eq!(v["workspaces"].as_array().unwrap().len(), 1);
    assert_eq!(v["workspaces"][0]["name"], Value::Null);
    assert_eq!(v["workspaces"][0]["input_tokens"], 151);
}

#[tokio::test]
async fn nothing_about_a_chat_is_in_the_answer() {
    let t = start().await;
    let sids = fixture(&t).await;
    let titles = ["Quarterly layoffs plan", "secret-folder", "node-xyz-secret"];
    for q in [PERIOD, "/api/admin/usage?from=0&to=1"] {
        let (st, _, text) = get(&t, q, Some(&t.alex)).await;
        assert_eq!(st, StatusCode::OK);
        for sid in &sids {
            assert!(!text.contains(&sid.to_string()), "session id in {text}");
        }
        for s in titles {
            assert!(!text.contains(s), "{s} in {text}");
        }
        // Only these keys, at every level.
        let v: Value = serde_json::from_str(&text).unwrap();
        let keys = |o: &Value| {
            let mut k: Vec<_> = o.as_object().unwrap().keys().cloned().collect();
            k.sort();
            k
        };
        assert_eq!(keys(&v), ["cut", "from", "people", "to", "workspaces"]);
        for p in v["people"].as_array().unwrap() {
            assert_eq!(
                keys(p),
                [
                    "cache_read_tokens",
                    "cache_write_tokens",
                    "input_tokens",
                    "models",
                    "output_tokens",
                    "turns",
                    "user_id",
                    "username"
                ]
            );
        }
    }
}

#[tokio::test]
async fn only_server_admins_may_read_it() {
    let t = start().await;
    fixture(&t).await;
    let bob = account(&t, "bob2").await;
    // dana: an admin of a workspace, a member of the server.
    let dana = account(&t, "dana").await;
    let w = post(&t, "/api/workspaces", json!({"name": "team"}), &t.alex).await;
    let team = w["workspace_id"].as_str().unwrap().to_string();
    post(
        &t,
        &format!("/api/workspaces/{team}/members"),
        json!({"username": "dana", "role": "admin"}),
        &t.alex,
    )
    .await;
    for (who, name) in [(&bob, "a member"), (&dana, "a workspace admin")] {
        let (st, v, _) = get(&t, PERIOD, Some(who)).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{name}: {v}");
    }
    let (st, _, _) = get(&t, PERIOD, None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    // The API token is the machine owner's key and is an admin here, as it
    // is for /api/admin/storage.
    let (st, _, _) = get(&t, "/api/admin/storage", Some(&format!("Bearer {TOKEN}"))).await;
    let (st2, v, _) = get(&t, PERIOD, Some(&format!("Bearer {TOKEN}"))).await;
    assert_eq!(st2, st, "{v}");
}

#[tokio::test]
async fn the_period_is_required_and_bounded() {
    let t = start().await;
    let max = 366 * DAY;
    for (q, want) in [
        ("/api/admin/usage", StatusCode::BAD_REQUEST),
        ("/api/admin/usage?from=5", StatusCode::BAD_REQUEST),
        ("/api/admin/usage?from=5&to=5", StatusCode::BAD_REQUEST),
        ("/api/admin/usage?from=9&to=5", StatusCode::BAD_REQUEST),
        ("/api/admin/usage?from=-1&to=5", StatusCode::BAD_REQUEST),
        ("/api/admin/usage?from=abc&to=5", StatusCode::BAD_REQUEST),
        (
            &format!("/api/admin/usage?from=0&to={}", max + 1),
            StatusCode::BAD_REQUEST,
        ),
        (&format!("/api/admin/usage?from=0&to={max}"), StatusCode::OK),
    ] {
        let (st, v, _) = get(&t, q, Some(&t.alex)).await;
        assert_eq!(st, want, "{q}: {v}");
    }
}

#[tokio::test]
async fn the_period_and_a_deleted_chat_decide_what_is_counted() {
    let t = start().await;
    let sids = fixture(&t).await;
    let t0 = 10 * DAY;
    // Only bob's chat (ts t0+30..t0+32) is in this period.
    let q = format!("/api/admin/usage?from={}&to={}", t0 + 30, t0 + 33);
    let (_, v, _) = get(&t, &q, Some(&t.alex)).await;
    assert_eq!(v["people"].as_array().unwrap().len(), 1);
    assert_eq!(v["people"][0]["username"], "bob");
    // The end is exclusive: a period ending at the turn's end leaves it out.
    let q = format!("/api/admin/usage?from={}&to={}", t0 + 30, t0 + 32);
    let (_, v, _) = get(&t, &q, Some(&t.alex)).await;
    assert!(v["people"].as_array().unwrap().is_empty(), "{v}");
    // Deleting the chat takes its usage with it.
    t.store.delete_session(sids[3], false).unwrap();
    let (_, v, _) = get(&t, PERIOD, Some(&t.alex)).await;
    let names: Vec<_> = v["people"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["username"].clone())
        .collect();
    assert_eq!(names, [json!("alex")]);
}

#[tokio::test]
async fn a_spent_time_limit_is_said() {
    let t = start().await;
    fixture(&t).await;
    t.master.set_usage_time_limit(Duration::from_micros(1));
    let (st, v, text) = get(&t, PERIOD, Some(&t.alex)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["cut"]["covered_percent"], 0, "{text}");
    assert!(v["people"].as_array().unwrap().is_empty());
}
