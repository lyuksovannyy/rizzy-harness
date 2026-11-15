//! The owner's change of a chat's step limit over HTTP (X.14, README "Step
//! and time limits"): owner only, every other account gets 404 (admins and
//! workspace admins included, T10); a raise through the execution-level
//! check; relayed as `session.set_step_limit` only to a node that announced
//! `steps-max:*`; the master passes the number on and the node clamps and
//! records (`crates/node/tests/step_limit.rs`).
//!
//! The node here is a fake that opens chats, keeps every command it is sent
//! and acks.

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

/// An agentic node `id` announcing `caps` on top of the usual ones.
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

fn changes(seen: &Seen) -> Vec<(SessionId, i64)> {
    seen.lock()
        .unwrap()
        .iter()
        .filter_map(|m| match m {
            MasterToNode::SetStepLimit {
                session_id,
                max_steps,
                ..
            } => Some((*session_id, *max_steps)),
            _ => None,
        })
        .collect()
}

struct T {
    base: String,
    alex: String,
    local: Seen,
    remote: Seen,
    old: Seen,
    _dir: tempfile::TempDir,
}

const TOKEN: &str = "api-secret";

/// A master with the fake nodes `local` (embedded, announcing its approval
/// modes, ceiling `auto`), `remote` (ceiling `accept_edits`) and `old` (a
/// node from before T50), and alex, its claimed admin.
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
    let (l, local) = fake(
        "local",
        &["approval-max:auto", "approval-default:ask", "steps-max:50"],
    );
    master.add_link(l, true).await.unwrap();
    let (l, remote) = fake(
        "remote",
        &[
            "approval-max:accept_edits",
            "approval-default:ask",
            "steps-max:20",
        ],
    );
    master.add_link(l, false).await.unwrap();
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
        remote,
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
    format!("/api/sessions/{sid}/step-limit")
}

/// The owner's numbers go to the node as they are (the node clamps them:
/// 0, 1, its maximum, above it, a negative one); what is not an integer is
/// no request at all. The node list says each node's maximum.
#[tokio::test]
async fn the_owner_changes_the_limit_and_the_node_clamps_it() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    for n in [0, 1, 50, 99, -3] {
        let (st, v) = req(
            &t,
            Method::POST,
            &path(sid),
            Some(json!({ "max_steps": n })),
            &t.alex,
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{n}: {v}");
    }
    assert_eq!(
        changes(&t.local),
        [(sid, 0), (sid, 1), (sid, 50), (sid, 99), (sid, -3)]
    );
    for bad in [
        json!({"max_steps": "ten"}),
        json!({"max_steps": 2.5}),
        json!({}),
        json!({"max_steps": 3, "also": 1}),
    ] {
        let (st, _) = req(&t, Method::POST, &path(sid), Some(bad.clone()), &t.alex).await;
        assert!(st.is_client_error(), "{bad}: {st}");
    }
    assert_eq!(changes(&t.local).len(), 5);
    let (st, v) = req(&t, Method::GET, "/api/nodes", None, &t.alex).await;
    assert_eq!(st, StatusCode::OK);
    let nodes = v["nodes"].as_array().unwrap();
    let of = |id: &str| nodes.iter().find(|n| n["node_id"] == id).unwrap().clone();
    assert_eq!(of("local")["max_steps"], 50);
    assert!(of("old")["max_steps"].is_null());
}

/// Another account cannot change someone's chat: a member, a server admin
/// and an admin of the chat's workspace all get the 404 of a chat that
/// does not exist, and the node is told nothing.
#[tokio::test]
async fn nobody_else_may_change_it_not_even_an_admin() {
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
        let (st, v) = req(
            &t,
            Method::POST,
            &path(sid),
            Some(json!({"max_steps": 10})),
            who,
        )
        .await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{name}: {v}");
        assert_eq!(v["error"], "unknown session", "{name}");
    }
    assert!(changes(&t.local).is_empty());
}

/// A node from before chat step limits is never sent the command: a plain
/// refusal.
#[tokio::test]
async fn a_node_without_step_limits_is_not_sent_it() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "old", None).await;
    let (st, v) = req(
        &t,
        Method::POST,
        &path(sid),
        Some(json!({"max_steps": 5})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"]
            .as_str()
            .unwrap()
            .contains("cannot change its step limit"),
        "{v}"
    );
    assert!(changes(&t.old).is_empty());
}

/// An owner whose level no longer allows the chat's execution mode cannot
/// raise its limit (the same check as for a prompt); a lowering is never
/// refused.
#[tokio::test]
async fn a_raise_needs_the_level_a_lowering_does_not() {
    let t = start().await;
    let (bob, bob_id) = account(&t, "bob", "member").await;
    let sid = chat(&t, &bob, "remote", None).await;
    let (st, v) = req(
        &t,
        Method::PUT,
        &format!("/api/users/{bob_id}/exec-ceiling"),
        Some(json!({"level": "none"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = req(
        &t,
        Method::POST,
        &path(sid),
        Some(json!({"max_steps": 30})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert!(changes(&t.remote).is_empty());
    let (st, v) = req(
        &t,
        Method::POST,
        &path(sid),
        Some(json!({"max_steps": 5})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(changes(&t.remote), [(sid, 5)]);
}
