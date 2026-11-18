//! A person's `/compact` over HTTP (X.20, PLAN.md T64, README "Context and
//! summaries"): the chat's owner only, every other account gets 404
//! (admins and workspace admins included, T10); relayed as
//! `session.compact` only to a node that announced `compact`; the node's
//! refusal (a turn runs, too short) comes back as it said it. The node's
//! side is `crates/node/tests/compact.rs`.
//!
//! The node here is a fake that opens chats, keeps every command it is sent
//! and acks, refusing a `/compact` whose focus is "refuse".

mod common;
use common::{Login as _, SameOrigin as _};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use harness_core::event::{EventKind, Profile, SessionCreated};
use harness_core::policy::ApprovalMode;
use harness_core::{RawEvent, Secret, SessionId};
use harness_master::auth::{Auth, Claim};
use harness_master::router;
use harness_node::NodeLink;
use harness_protocol::{MasterToNode, NodeToMaster, PROTOCOL_VERSION};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

type Seen = Arc<Mutex<Vec<MasterToNode>>>;

fn fake(id: &str, caps: &[&str]) -> (NodeLink, Seen) {
    let (to_node, mut commands) = mpsc::channel::<MasterToNode>(1024);
    let (tx, from_node) = mpsc::channel::<NodeToMaster>(1024);
    let mut capabilities = vec![
        "native".to_string(),
        "profile:agentic".to_string(),
        "exec:sandbox".to_string(),
    ];
    capabilities.extend(caps.iter().map(|c| c.to_string()));
    tx.try_send(NodeToMaster::Hello {
        protocol_version: PROTOCOL_VERSION,
        node_id: id.into(),
        hostname: "h".into(),
        version: "1".into(),
        capabilities,
        sessions: vec![],
    })
    .unwrap();
    let seen: Seen = Arc::default();
    let (node, keep) = (id.to_string(), seen.clone());
    tokio::spawn(async move {
        let mut seqs: HashMap<SessionId, u64> = HashMap::new();
        while let Some(msg) = commands.recv().await {
            keep.lock().unwrap().push(msg.clone());
            if let MasterToNode::Create {
                session_id,
                exec_mode,
                ..
            } = &msg
            {
                let seq = seqs.entry(*session_id).or_insert(0);
                *seq += 1;
                let (kind, payload) = EventKind::SessionCreated(SessionCreated {
                    exec_mode: *exec_mode,
                    no_callbacks: false,
                    title: "t".into(),
                    cwd: "/w".into(),
                    model: "demo::mock".into(),
                    backend: "native".into(),
                    node_id: node.clone(),
                    approval_mode: ApprovalMode::Ask,
                    profile: Profile::Agentic,
                    agent_id: None,
                    incognito: false,
                    parent_session: None,
                    owner_id: None,
                })
                .into_parts();
                let _ = tx.try_send(NodeToMaster::Event {
                    event: RawEvent {
                        session_id: *session_id,
                        seq: *seq,
                        ts_ms: harness_core::now_ms(),
                        kind,
                        payload,
                    },
                });
            }
            if let MasterToNode::Compact { req_id, focus, .. } = &msg
                && focus.as_deref() == Some("refuse")
            {
                let _ = tx.try_send(NodeToMaster::Ack {
                    req_id: req_id.clone(),
                    ok: false,
                    error: Some(harness_protocol::COMPACT_TURN_RUNNING.into()),
                });
                continue;
            }
            if let Some(req_id) = msg.req_id() {
                let _ = tx.try_send(NodeToMaster::Ack {
                    req_id: req_id.into(),
                    ok: true,
                    error: None,
                });
            }
        }
    });
    (NodeLink { to_node, from_node }, seen)
}

/// The focus of every `session.compact` the node was sent, by chat.
fn compacts(seen: &Seen) -> Vec<(SessionId, Option<String>)> {
    seen.lock()
        .unwrap()
        .iter()
        .filter_map(|m| match m {
            MasterToNode::Compact {
                session_id, focus, ..
            } => Some((*session_id, focus.clone())),
            _ => None,
        })
        .collect()
}

struct T {
    base: String,
    alex: String,
    local: Seen,
    old: Seen,
    _dir: tempfile::TempDir,
}

const TOKEN: &str = "api-secret";

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
    let master = harness_master::Master::new(Store::open_in_memory().unwrap(), auth);
    let (l, local) = fake("local", &[harness_protocol::CAP_COMPACT]);
    master.add_link(l, true).await.unwrap();
    let (l, old) = fake("old", &[]);
    master.add_link(l, false).await.unwrap();
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
        local,
        old,
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

/// An account with its own password, logged in; and its id.
async fn account(t: &T, name: &str, role: &str) -> (String, String) {
    let (st, v) = req(
        t,
        Method::POST,
        "/api/users",
        Some(json!({"username": name, "role": role, "exec_ceiling": "host"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let pw = v["password"].as_str().unwrap().to_string();
    let id = v["user"]["user_id"]
        .as_str()
        .or(v["user_id"].as_str())
        .unwrap_or_default()
        .to_string();
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
    (common::signed_in(r).await, id)
}

async fn chat(t: &T, who: &str, node: &str, workspace: Option<&str>) -> SessionId {
    let mut body = json!({"cwd": "/w", "title": "t", "node_id": node});
    if let Some(w) = workspace {
        body["workspace_id"] = json!(w);
    }
    let (st, v) = req(t, Method::POST, "/api/sessions", Some(body), who).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["session_id"].as_str().unwrap().parse().unwrap()
}

fn path(sid: SessionId) -> String {
    format!("/api/sessions/{sid}/compact")
}

/// The owner's `/compact` goes to the chat's node with their focus; an
/// empty focus is none.
#[tokio::test]
async fn the_owner_compacts_with_a_focus() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    let body = json!({"focus": "keep the database migration details"});
    let (st, v) = req(&t, Method::POST, &path(sid), Some(body), &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = req(
        &t,
        Method::POST,
        &path(sid),
        Some(json!({"focus": "  "})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = req(&t, Method::POST, &path(sid), Some(json!({})), &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        compacts(&t.local),
        [
            (sid, Some("keep the database migration details".into())),
            (sid, None),
            (sid, None)
        ]
    );
}

/// Another account cannot summarise someone's chat: a member, a server
/// admin and an admin of the chat's workspace all get the 404 of a chat
/// that does not exist, and the node is told nothing.
#[tokio::test]
async fn nobody_else_may_compact_not_even_an_admin() {
    let t = start().await;
    let (bob, _) = account(&t, "bob", "member").await;
    let (carol, _) = account(&t, "carol", "admin").await;
    let (dana, _) = account(&t, "dana", "member").await;
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "team"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let team = v["workspace_id"].as_str().unwrap().to_string();
    let (st, v) = req(
        &t,
        Method::POST,
        &format!("/api/workspaces/{team}/members"),
        Some(json!({"username": "dana", "role": "admin"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = chat(&t, &t.alex, "local", Some(&team)).await;
    for (who, name) in [
        (&bob, "another member"),
        (&carol, "a server admin"),
        (&dana, "a workspace admin"),
    ] {
        let (st, v) = req(&t, Method::POST, &path(sid), Some(json!({})), who).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{name}: {v}");
        assert_eq!(v["error"], "unknown session", "{name}");
    }
    assert!(compacts(&t.local).is_empty());
}

/// The node's refusal is the person's answer, in its words; a node that
/// did not announce `compact` is never sent it; a focus past the master's
/// bound is no request.
#[tokio::test]
async fn refusals_are_said_plainly() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    let (st, v) = req(
        &t,
        Method::POST,
        &path(sid),
        Some(json!({"focus": "refuse"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"], harness_protocol::COMPACT_TURN_RUNNING);
    let long = "x".repeat(70 * 1024);
    let (st, v) = req(
        &t,
        Method::POST,
        &path(sid),
        Some(json!({"focus": long})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE, "{v}");
    assert_eq!(compacts(&t.local).len(), 1, "only the refused one was sent");
    let old = chat(&t, &t.alex, "old", None).await;
    let (st, v) = req(&t, Method::POST, &path(old), Some(json!({})), &t.alex).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"].as_str().unwrap().contains("cannot summarise"),
        "{v}"
    );
    assert!(compacts(&t.old).is_empty());
}
