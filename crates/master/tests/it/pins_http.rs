//! The owner's pin of a message over HTTP (docs/pins.md, README "Pinned
//! messages"): `POST /api/sessions/{id}/pins` with `{message_seq, pinned}`.
//! The chat's owner only: every other account gets the 404 of a chat that
//! does not exist (admins and workspace admins included, T10), the API token
//! too on a chat somebody owns, and the node is told nothing; the same text
//! for "not yours" and "does not exist". A sub-agent's chat and a routine's
//! chat the caller owns can be pinned in (it is a bookmark). The rule is
//! `harness_core::pins::check`, applied to the mirror first; relayed as
//! `session.pin`, and only to a node that announced `pin` (an older node is
//! told to update). The node's side is `crates/node/tests/pins.rs`.
//!
//! The node here is a fake: it opens chats, runs a turn for a prompt
//! (`child`: also a sub-agent's chat), writes `message_pinned` when told to,
//! and keeps every command.

use crate::common;
use common::{Login as _, SameOrigin as _};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use harness_core::event::*;
use harness_core::policy::ApprovalMode;
use harness_core::{RawEvent, Secret, SessionId};
use harness_master::auth::{Auth, Claim};
use harness_master::router;
use harness_node::NodeLink;
use harness_protocol::{MasterToNode, NodeToMaster, PROTOCOL_VERSION, PromptMsg};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

type Seen = Arc<Mutex<Vec<MasterToNode>>>;
type Children = Arc<Mutex<HashMap<SessionId, SessionId>>>;

struct Fake {
    tx: mpsc::Sender<NodeToMaster>,
    seqs: HashMap<SessionId, u64>,
    /// The turn each message seq opened.
    turn_of: HashMap<(SessionId, u64), String>,
    node: String,
    profile: Profile,
    turns: u32,
}

impl Fake {
    fn emit(&mut self, sid: SessionId, k: EventKind) -> u64 {
        let seq = self.seqs.entry(sid).or_insert(0);
        *seq += 1;
        let at = *seq;
        let (kind, payload) = k.into_parts();
        let _ = self.tx.try_send(NodeToMaster::Event {
            event: RawEvent {
                session_id: sid,
                seq: at,
                ts_ms: harness_core::now_ms(),
                kind,
                payload,
            },
        });
        at
    }

    fn created(&mut self, sid: SessionId, parent: Option<SessionId>) {
        let k = EventKind::SessionCreated(SessionCreated {
            forked: None,
            exec_mode: Some(harness_core::policy::ExecMode::Sandbox),
            no_callbacks: false,
            title: "t".into(),
            cwd: "/w".into(),
            model: "demo::mock".into(),
            backend: "native".into(),
            node_id: self.node.clone(),
            approval_mode: ApprovalMode::Ask,
            profile: self.profile,
            agent_id: None,
            incognito: false,
            parent_session: parent,
        });
        self.emit(sid, k);
    }

    /// One finished turn of `text`; its id.
    fn turn(&mut self, sid: SessionId, text: &str) -> String {
        self.turns += 1;
        let t = format!("t{}", self.turns);
        self.emit(
            sid,
            EventKind::TurnStarted(TurnStarted {
                turn_id: t.clone(),
                agent: None,
                cause: None,
                environment: None,
                retry: None,
                continued_by: None,
            }),
        );
        let at = self.emit(
            sid,
            EventKind::UserMessage(
                serde_json::from_value(json!({"turn_id": t, "text": text})).unwrap(),
            ),
        );
        self.turn_of.insert((sid, at), t.clone());
        self.emit(
            sid,
            EventKind::AssistantMessage(
                serde_json::from_value(
                    json!({"turn_id": t, "message_id": format!("m-{t}"), "text": format!("answer to {text}")}),
                )
                .unwrap(),
            ),
        );
        self.emit(
            sid,
            EventKind::TurnEnded(TurnEnded {
                turn_id: t.clone(),
                reason: TurnEndReason::Completed,
                usage: None,
            }),
        );
        t
    }
}

fn fake(id: &str, profile: Profile, caps: &[&str]) -> (NodeLink, Seen, Children) {
    let (to_node, mut commands) = mpsc::channel::<MasterToNode>(1024);
    let (tx, from_node) = mpsc::channel::<NodeToMaster>(1024);
    let mut capabilities = vec![
        "native".to_string(),
        match profile {
            Profile::Agentic => "profile:agentic".to_string(),
            Profile::Chat => "profile:chat".to_string(),
        },
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
    let children: Children = Arc::default();
    let (keep, kids) = (seen.clone(), children.clone());
    let mut f = Fake {
        tx: tx.clone(),
        seqs: HashMap::new(),
        turn_of: HashMap::new(),
        node: id.to_string(),
        profile,
        turns: 0,
    };
    tokio::spawn(async move {
        while let Some(msg) = commands.recv().await {
            keep.lock().unwrap().push(msg.clone());
            match &msg {
                MasterToNode::Create { session_id, .. } => f.created(*session_id, None),
                MasterToNode::Prompt(PromptMsg {
                    session_id, text, ..
                }) => {
                    if text.contains("child") {
                        let child = SessionId::new();
                        f.created(child, Some(*session_id));
                        f.turn(child, text);
                        kids.lock().unwrap().insert(*session_id, child);
                    }
                    f.turn(*session_id, text);
                }
                MasterToNode::Pin {
                    session_id,
                    message_seq,
                    pinned,
                    ..
                } => {
                    f.emit(
                        *session_id,
                        EventKind::MessagePinned(MessagePinned {
                            message_seq: *message_seq,
                            pinned: *pinned,
                        }),
                    );
                }
                _ => {}
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
    (NodeLink { to_node, from_node }, seen, children)
}

/// Every `session.pin` the node was sent: chat, message, pinned.
fn pins(seen: &Seen) -> Vec<(SessionId, u64, bool)> {
    seen.lock()
        .unwrap()
        .iter()
        .filter_map(|m| match m {
            MasterToNode::Pin {
                session_id,
                message_seq,
                pinned,
                ..
            } => Some((*session_id, *message_seq, *pinned)),
            _ => None,
        })
        .collect()
}

struct T {
    base: String,
    alex: String,
    local: Seen,
    children: Children,
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
    master.set_agents(harness_agents::AgentSet::load(&dir.path().join("none")).0);
    let (l, local, children) = fake("local", Profile::Agentic, &[harness_protocol::CAP_PIN]);
    master.add_link(l, true).await.unwrap();
    let (l, old, _) = fake("old", Profile::Agentic, &[]);
    master.add_link(l, false).await.unwrap();
    let (l, _, _) = fake("master", Profile::Chat, &[harness_protocol::CAP_PIN]);
    master.add_link(l, true).await.unwrap();
    harness_master::routines::start_with(
        &master,
        harness_master::routines::Clock::manual(Arc::new(std::sync::atomic::AtomicI64::new(
            harness_core::now_ms(),
        ))),
        false,
    );
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
        children,
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

/// An account with its own password, logged in.
async fn account(t: &T, name: &str, role: &str) -> String {
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

async fn chat(t: &T, who: &str, node: &str, workspace: Option<&str>) -> SessionId {
    let mut body = json!({"cwd": "/w", "title": "t", "node_id": node});
    if let Some(w) = workspace {
        body["workspace_id"] = json!(w);
    }
    let (st, v) = req(t, Method::POST, "/api/sessions", Some(body), who).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["session_id"].as_str().unwrap().parse().unwrap()
}

async fn events(t: &T, who: &str, sid: SessionId) -> Vec<Value> {
    let (st, v) = req(
        t,
        Method::GET,
        &format!("/api/sessions/{sid}/events?after=0"),
        None,
        who,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["events"].as_array().cloned().unwrap_or_default()
}

/// Wait until the mirror holds `at_least` events of the chat.
async fn caught_up(t: &T, who: &str, sid: SessionId, at_least: usize) -> Vec<Value> {
    for _ in 0..500 {
        let evs = events(t, who, sid).await;
        if evs.len() >= at_least {
            return evs;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!(
        "the mirror never caught up: {:#?}",
        events(t, who, sid).await
    );
}

/// Send `text` (a turn of four events) and return the seq of the assistant's answer.
async fn send(t: &T, who: &str, sid: SessionId, text: &str) -> u64 {
    let before = events(t, who, sid).await.len();
    let (st, v) = req(
        t,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({"text": text})),
        who,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let evs = caught_up(t, who, sid, before + 4).await;
    evs.iter()
        .rfind(|e| e["kind"] == "assistant_message")
        .unwrap()["seq"]
        .as_u64()
        .unwrap()
}

fn path(sid: SessionId) -> String {
    format!("/api/sessions/{sid}/pins")
}

fn body(seq: u64, pinned: bool) -> Option<Value> {
    Some(json!({"message_seq": seq, "pinned": pinned}))
}

#[tokio::test]
async fn the_owner_pins_and_unpins_and_the_node_is_asked() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    let answer = send(&t, &t.alex, sid, "first").await;
    let (st, v) = req(&t, Method::POST, &path(sid), body(answer, true), &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(pins(&t.local), [(sid, answer, true)]);
    let evs = caught_up(&t, &t.alex, sid, 6).await;
    assert_eq!(evs.last().unwrap()["kind"], "message_pinned");
    // Pinned already: refused here, the node is sent nothing more.
    let (st, v) = req(&t, Method::POST, &path(sid), body(answer, true), &t.alex).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().starts_with(PIN_REFUSED), "{v}");
    assert!(
        v["error"].as_str().unwrap().contains("already pinned"),
        "{v}"
    );
    assert_eq!(pins(&t.local).len(), 1);
    let (st, v) = req(&t, Method::POST, &path(sid), body(answer, false), &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(pins(&t.local).len(), 2);
}

const PIN_REFUSED: &str = "This message can't be pinned";

#[tokio::test]
async fn a_seq_that_is_no_message_is_refused_here() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    send(&t, &t.alex, sid, "first").await;
    for seq in [0u64, 1, 2, 9999] {
        let (st, v) = req(&t, Method::POST, &path(sid), body(seq, true), &t.alex).await;
        assert_eq!(st, StatusCode::CONFLICT, "{seq}: {v}");
    }
    assert!(pins(&t.local).is_empty());
}

/// Another account cannot pin in someone's chat: a member, a server admin
/// and an admin of the chat's workspace all get the 404 of a chat that does
/// not exist, with the very text a chat that does not exist gets, and the
/// node is told nothing.
#[tokio::test]
async fn nobody_else_may_pin_not_even_an_admin() {
    let t = start().await;
    let bob = account(&t, "bob", "member").await;
    let carol = account(&t, "carol", "admin").await;
    let dana = account(&t, "dana", "member").await;
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
    let answer = send(&t, &t.alex, sid, "first").await;
    let (_, nothing) = req(
        &t,
        Method::POST,
        &path(SessionId::new()),
        body(answer, true),
        &bob,
    )
    .await;
    for (who, name) in [
        (&bob, "another member"),
        (&carol, "a server admin"),
        (&dana, "a workspace admin"),
    ] {
        let (st, v) = req(&t, Method::POST, &path(sid), body(answer, true), who).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{name}: {v}");
        assert_eq!(
            v, nothing,
            "{name}: the same answer as a chat that does not exist"
        );
        assert_eq!(v["error"], "unknown session", "{name}");
    }
    assert!(pins(&t.local).is_empty());
    assert_eq!(events(&t, &t.alex, sid).await.len(), 5);
}

async fn token_req(t: &T, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut rb = reqwest::Client::new()
        .request(method, format!("{}{path}", t.base))
        .bearer_auth(TOKEN);
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    let r = rb.send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

/// The API token is no account: on a chat somebody owns it gets the 404 of
/// a chat that does not exist and the node is sent nothing.
#[tokio::test]
async fn the_api_token_cannot_pin_in_a_chat_somebody_owns() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    let answer = send(&t, &t.alex, sid, "first").await;
    let (st, v) = token_req(&t, Method::POST, &path(sid), body(answer, true)).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{v}");
    assert_eq!(v["error"], "unknown session");
    let (st, missing) = token_req(
        &t,
        Method::POST,
        &path(SessionId::new()),
        body(answer, true),
    )
    .await;
    assert_eq!(
        (st, &missing["error"]),
        (StatusCode::NOT_FOUND, &v["error"])
    );
    assert!(pins(&t.local).is_empty());
}

/// A sub-agent's chat the caller owns can be pinned in (it is a bookmark,
/// not a change to the chat); a node that did not announce `pin` is told
/// to update and is sent nothing.
#[tokio::test]
async fn a_sub_agents_chat_can_be_pinned_and_an_old_node_is_refused() {
    let t = start().await;
    let parent = chat(&t, &t.alex, "local", None).await;
    send(&t, &t.alex, parent, "child please").await;
    let child = *t.children.lock().unwrap().get(&parent).unwrap();
    let evs = caught_up(&t, &t.alex, child, 4).await;
    let answer = evs
        .iter()
        .rfind(|e| e["kind"] == "assistant_message")
        .unwrap()["seq"]
        .as_u64()
        .unwrap();
    let (st, v) = req(&t, Method::POST, &path(child), body(answer, true), &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(pins(&t.local), [(child, answer, true)]);
    // An old node.
    let old = chat(&t, &t.alex, "old", None).await;
    let a = send(&t, &t.alex, old, "first").await;
    let (st, v) = req(&t, Method::POST, &path(old), body(a, true), &t.alex).await;
    assert_ne!(st, StatusCode::OK, "{v}");
    assert!(v["error"].as_str().unwrap().contains("cannot pin"), "{v}");
    assert!(pins(&t.old).is_empty());
}

/// A chat that does not exist is a 404 and the node hears nothing.
#[tokio::test]
async fn a_chat_that_does_not_exist_is_a_404_with_no_node_traffic() {
    let t = start().await;
    let (st, v) = req(
        &t,
        Method::POST,
        &path(SessionId::new()),
        body(3, true),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{v}");
    assert!(pins(&t.local).is_empty());
}
