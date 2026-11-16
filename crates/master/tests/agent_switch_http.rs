//! Switching a chat's agent over HTTP (README "Agents"): the pick is the
//! chat's state on the master (its session row, on disk), not an event;
//! the switch is recorded by the node at the next message. While the chat
//! works the node refuses the change and the master answers 409 with its
//! reason. Only the chat's owner may switch it: every other account,
//! admins included, gets the 404 of a chat that does not exist.

mod common;
use common::{Login as _, SameOrigin as _};

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::event::Profile;
use harness_core::{RawEvent, Secret};
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, ModelInfo, router};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::{Durability, Store};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

struct T {
    base: String,
    alex: String,
    db: std::path::PathBuf,
    master: Master,
    _node: NodeRuntime,
    _dir: tempfile::TempDir,
}

/// A master on a database file with its chat runtime (the scripted model)
/// and alex, its claimed admin.
async fn start() -> T {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, "CLAIM").unwrap();
    let auth =
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim_path,
        });
    let db = dir.path().join("master.db");
    let master = Master::new(Store::open(&db, Durability::Full).unwrap(), auth);
    master.set_models(vec![ModelInfo {
        id: "mock".into(),
        provider: "mock".into(),
        default: true,
        ..Default::default()
    }]);
    let node = NodeRuntime::start(NodeConfig {
        node_id: "master".into(),
        hostname: "h".into(),
        policy: NodePolicy {
            workspace_roots: vec![dir.path().into()],
            ..Default::default()
        },
        store: Store::open_in_memory().unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(5),
            }),
            Vec::new(),
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
    .unwrap();
    master.add_link(node.attach().await, true).await.unwrap();
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
        alex,
        db,
        master,
        _node: node,
        _dir: dir,
    }
}

async fn req(
    t: &T,
    method: Method,
    path: &str,
    body: Option<Value>,
    c: &str,
) -> (StatusCode, Value) {
    let mut rb = reqwest::Client::new().request(method, format!("{}{path}", t.base));
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    let r = rb.login(c).same_origin().send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

async fn account(t: &T, name: &str, role: &str) -> String {
    let (st, v) = req(
        t,
        Method::POST,
        "/api/users",
        Some(json!({"username": name, "role": role})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
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

async fn chat(t: &T) -> String {
    let (st, v) = req(
        t,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["session_id"].as_str().unwrap().to_string()
}

async fn events(t: &T, sid: &str) -> Vec<RawEvent> {
    let (st, v) = req(
        t,
        Method::GET,
        &format!("/api/sessions/{sid}/events"),
        None,
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    serde_json::from_value(v["events"].clone()).unwrap()
}

async fn events_until(t: &T, sid: &str, pred: impl Fn(&[RawEvent]) -> bool) -> Vec<RawEvent> {
    for _ in 0..500 {
        let evs = events(t, sid).await;
        if pred(&evs) {
            return evs;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("condition not reached");
}

fn ended(evs: &[RawEvent]) -> usize {
    evs.iter().filter(|e| e.kind == "turn_ended").count()
}

async fn pick(t: &T, sid: &str, agent: &str, who: &str) -> (StatusCode, Value) {
    req(
        t,
        Method::POST,
        &format!("/api/sessions/{sid}/agent"),
        Some(json!({ "agent_id": agent })),
        who,
    )
    .await
}

async fn prompt(t: &T, sid: &str, text: &str) {
    let (st, v) = req(
        t,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({ "text": text })),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

async fn row_agent(t: &T, sid: &str) -> Value {
    let (_, row) = req(
        t,
        Method::GET,
        &format!("/api/sessions/{sid}"),
        None,
        &t.alex,
    )
    .await;
    row["agent_id"].clone()
}

/// General → Writer → Researcher → Writer with no message: the log is as
/// it was, the chat's row (on disk) says Writer, and the next message runs
/// the Writer under one `agent_changed` naming it and its model.
#[tokio::test]
async fn a_pick_is_state_and_is_noted_at_the_next_message() {
    let t = start().await;
    let sid = chat(&t).await;
    prompt(&t, &sid, "hello").await;
    let before = events_until(&t, &sid, |e| ended(e) == 1).await;
    for agent in ["writer", "researcher", "writer"] {
        let (st, v) = pick(&t, &sid, agent, &t.alex).await;
        assert_eq!(st, StatusCode::OK, "{agent}: {v}");
    }
    // Nothing reaches the log; give a late event the time to land.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let after = events(&t, &sid).await;
    assert_eq!(
        after.last().map(|e| e.seq),
        before.last().map(|e| e.seq),
        "{:?}",
        after.iter().map(|e| &e.kind).collect::<Vec<_>>()
    );
    assert_eq!(row_agent(&t, &sid).await, "writer");
    // The pick is on disk: a restarted master reads it from the same file.
    let reopened = Store::open(&t.db, Durability::Full).unwrap();
    let row = reopened.session(sid.parse().unwrap()).unwrap().unwrap();
    assert_eq!(row.agent_id.as_deref(), Some("writer"));
    drop(reopened);

    prompt(&t, &sid, "two").await;
    let evs = events_until(&t, &sid, |e| ended(e) == 2).await;
    let switches: Vec<&RawEvent> = evs.iter().filter(|e| e.kind == "agent_changed").collect();
    assert_eq!(switches.len(), 1, "{switches:?}");
    assert_eq!(switches[0].payload["agent"]["agent_id"], "writer");
    assert_eq!(switches[0].payload["model"], "mock");
    let at = evs.iter().position(|e| e.kind == "agent_changed").unwrap();
    assert_eq!(evs[at + 1].kind, "turn_started");
    assert_eq!(evs[at + 1].payload["agent"]["agent_id"], "writer");
    assert_eq!(evs[at + 2].kind, "user_message");
}

/// While a turn runs the switch is refused with 409 and the node's reason,
/// and nothing changes; after Stop it works.
#[tokio::test]
async fn a_switch_while_working_is_409_and_works_after_stop() {
    let t = start().await;
    let sid = chat(&t).await;
    prompt(&t, &sid, "slow: 4000").await;
    events_until(&t, &sid, |e| e.iter().any(|x| x.kind == "turn_started")).await;
    let (st, v) = pick(&t, &sid, "writer", &t.alex).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"].as_str().unwrap_or_default().contains("working"),
        "{v}"
    );
    assert_eq!(row_agent(&t, &sid).await, "general");
    let (st, v) = req(
        &t,
        Method::POST,
        &format!("/api/sessions/{sid}/cancel"),
        None,
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let evs = events_until(&t, &sid, |e| ended(e) == 1).await;
    assert!(!evs.iter().any(|e| e.kind == "agent_changed"));
    let (st, v) = pick(&t, &sid, "writer", &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(row_agent(&t, &sid).await, "writer");
}

/// Another member and a server admin get 404; the chat's agent stays.
#[tokio::test]
async fn only_the_owner_may_switch_not_even_an_admin() {
    let t = start().await;
    let sid = chat(&t).await;
    let bob = account(&t, "bob", "member").await;
    let carol = account(&t, "carol", "admin").await;
    for (who, name) in [(&bob, "a member"), (&carol, "a server admin")] {
        let (st, v) = pick(&t, &sid, "writer", who).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{name}: {v}");
        assert_eq!(v["error"], "unknown session", "{name}");
    }
    assert_eq!(row_agent(&t, &sid).await, "general");
}

/// The agent of every turn as `turn_started` names it, and of every switch.
fn agents_of(evs: &[RawEvent]) -> (Vec<String>, Vec<String>) {
    let of = |kind: &str| {
        evs.iter()
            .filter(|e| e.kind == kind)
            .map(|e| {
                e.payload["agent"]["agent_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string()
            })
            .collect::<Vec<_>>()
    };
    (of("turn_started"), of("agent_changed"))
}

/// Review of PR 135 (F1): a pick that lands while a prompt is being
/// dispatched (after the prompt read the row, before the node took it)
/// is never lost: it waits for the dispatch (one per chat at a time), then
/// the node answers it. The row and the log agree throughout, and every
/// turn runs under the agent its `turn_started` names.
#[tokio::test]
async fn a_pick_racing_a_prompt_is_never_lost() {
    let t = start().await;
    let sid = chat(&t).await;
    prompt(&t, &sid, "hello").await;
    events_until(&t, &sid, |e| ended(e) == 1).await;
    // A pending pick (no message yet): the row says writer, the log general.
    let (st, v) = pick(&t, &sid, "writer", &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");

    // The prompt reads the row (writer), then waits; the pick of the
    // researcher comes in meanwhile.
    t.master.set_dispatch_delay(Duration::from_millis(800));
    let sent = {
        let base = t.base.clone();
        let alex = t.alex.clone();
        let sid = sid.clone();
        tokio::spawn(async move {
            let r = reqwest::Client::new()
                .post(format!("{base}/api/sessions/{sid}/prompt"))
                .json(&json!({ "text": "two" }))
                .login(&alex)
                .same_origin()
                .send()
                .await
                .unwrap();
            r.status()
        })
    };
    tokio::time::sleep(Duration::from_millis(250)).await;
    let (picked, v) = pick(&t, &sid, "researcher", &t.alex).await;
    assert_eq!(sent.await.unwrap(), StatusCode::OK);
    t.master.set_dispatch_delay(Duration::ZERO);
    let evs = events_until(&t, &sid, |e| ended(e) == 2).await;
    let (turns, switches) = agents_of(&evs);
    // The prompt ran what it read: the writer, noted once.
    assert_eq!(turns, ["general", "writer"], "{evs:#?}");
    assert_eq!(switches, ["writer"]);
    // The pick was refused (the turn was still running) or kept: never
    // accepted and then overwritten by the turn's switch.
    let row = row_agent(&t, &sid).await;
    match picked {
        StatusCode::OK => assert_eq!(row, "researcher", "an accepted pick was lost"),
        StatusCode::CONFLICT => assert_eq!(row, "writer", "{v}"),
        other => panic!("{other}: {v}"),
    }
}

/// The other order: the pick first, then the prompt, which runs it.
#[tokio::test]
async fn a_pick_before_a_prompt_runs_at_it() {
    let t = start().await;
    let sid = chat(&t).await;
    prompt(&t, &sid, "hello").await;
    events_until(&t, &sid, |e| ended(e) == 1).await;
    let (st, v) = pick(&t, &sid, "writer", &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    t.master.set_dispatch_delay(Duration::from_millis(300));
    prompt(&t, &sid, "two").await;
    t.master.set_dispatch_delay(Duration::ZERO);
    let evs = events_until(&t, &sid, |e| ended(e) == 2).await;
    let (turns, switches) = agents_of(&evs);
    assert_eq!(turns, ["general", "writer"]);
    assert_eq!(switches, ["writer"]);
    assert_eq!(row_agent(&t, &sid).await, "writer");
}
