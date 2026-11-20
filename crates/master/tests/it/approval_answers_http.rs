//! A person's answers beyond allow and deny over HTTP (T51): a note with a
//! denial (capped, dropped with an allow), "allow for this chat" and its
//! removal; owner only, and `allow_chat` never sent to a node without the
//! `answers` capability. The node's side: `crates/node/tests/approval_modes.rs`.
//!
//! (Shares its fake node with `approval_mode_http.rs`.)
//! The owner's change of a chat's approval mode over HTTP (T50, README
//! "Approval modes"): owner only, every other account gets 404 (admins and
//! workspace admins included, T10); through the execution-level check every
//! command that makes a chat work passes; relayed as
//! `session.set_approval_mode` only to a node that announced its approval
//! modes; the master decides nothing else (the node clamps and records:
//! `crates/node/tests/approval_modes.rs`).
//!
//! The node here is a fake that opens chats, keeps every command it is sent
//! and acks.

use crate::common;
use common::{Login as _, SameOrigin as _};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use harness_core::event::{EventKind, Profile, SessionCreated};
use harness_core::policy::{ApprovalMode, Decision};
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

#[allow(dead_code)] // shared shape with approval_mode_http.rs
struct T {
    base: String,
    master: harness_master::Master,
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
        &["approval-max:auto", "approval-default:ask", "answers"],
    );
    master.add_link(l, true).await.unwrap();
    let (l, remote) = fake(
        "remote",
        &["approval-max:accept_edits", "approval-default:ask"],
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
        master,
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

fn answers(seen: &Seen) -> Vec<(Decision, Option<String>)> {
    seen.lock()
        .unwrap()
        .iter()
        .filter_map(|m| match m {
            MasterToNode::ApprovalRespond { decision, note, .. } => Some((*decision, note.clone())),
            _ => None,
        })
        .collect()
}

fn revokes(seen: &Seen) -> Vec<String> {
    seen.lock()
        .unwrap()
        .iter()
        .filter_map(|m| match m {
            MasterToNode::RevokeAllow { grant_id, .. } => Some(grant_id.clone()),
            _ => None,
        })
        .collect()
}

async fn answer(t: &T, who: &str, sid: SessionId, body: Value) -> (StatusCode, Value) {
    req(
        t,
        Method::POST,
        &format!("/api/sessions/{sid}/approvals/a1"),
        Some(body),
        who,
    )
    .await
}

#[tokio::test]
async fn a_note_goes_with_a_denial_only_and_is_capped() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    let (st, v) = answer(
        &t,
        &t.alex,
        sid,
        json!({"decision": "deny", "note": "not on main"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = answer(
        &t,
        &t.alex,
        sid,
        json!({"decision": "allow", "note": "ignored"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let long = "x".repeat(harness_core::policy::MAX_DENY_NOTE + 1);
    let (st, _) = answer(&t, &t.alex, sid, json!({"decision": "deny", "note": long})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert_eq!(
        answers(&t.local),
        [
            (Decision::Deny, Some("not on main".to_string())),
            (Decision::Allow, None)
        ]
    );
}

#[tokio::test]
async fn allow_for_this_chat_and_its_removal_are_the_owners_alone() {
    let t = start().await;
    let (bob, _) = account(&t, "bob", "admin").await;
    let (carol, _) = account(&t, "carol", "member").await;
    let (dana, _) = account(&t, "dana", "member").await;
    // A team workspace where dana is an admin, with alex's chat in it.
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
    for (name, role) in [("carol", "member"), ("dana", "admin")] {
        let (st, v) = req(
            &t,
            Method::POST,
            &format!("/api/workspaces/{team}/members"),
            Some(json!({"username": name, "role": role})),
            &t.alex,
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }
    let sid = chat(&t, &t.alex, "local", Some(&team)).await;
    let grant = "0193f2a000007000800000000000abcd";
    for (who, name) in [
        (&bob, "a server admin"),
        (&carol, "a member of the chat's workspace"),
        (&dana, "an admin of the chat's workspace"),
    ] {
        let (st, _) = answer(&t, who, sid, json!({"decision": "allow_chat"})).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{name}");
        let (st, _) = req(
            &t,
            Method::DELETE,
            &format!("/api/sessions/{sid}/allows/{grant}"),
            None,
            who,
        )
        .await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{name}");
    }
    assert!(answers(&t.local).is_empty() && revokes(&t.local).is_empty());
    let (st, v) = answer(&t, &t.alex, sid, json!({"decision": "allow_chat"})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = req(
        &t,
        Method::DELETE,
        &format!("/api/sessions/{sid}/allows/{grant}"),
        None,
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, _) = req(
        &t,
        Method::DELETE,
        &format!("/api/sessions/{sid}/allows/not-hex"),
        None,
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert_eq!(answers(&t.local), [(Decision::AllowChat, None)]);
    assert_eq!(revokes(&t.local), [grant.to_string()]);
}

/// A node from before these answers is never sent `allow_chat` (it would
/// drop the link) nor a note.
#[tokio::test]
async fn a_node_without_answers_gets_neither_allow_chat_nor_a_note() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "old", None).await;
    let (st, v) = answer(&t, &t.alex, sid, json!({"decision": "allow_chat"})).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    let (st, v) = answer(&t, &t.alex, sid, json!({"decision": "deny", "note": "why"})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(answers(&t.old), [(Decision::Deny, None)]);
}
