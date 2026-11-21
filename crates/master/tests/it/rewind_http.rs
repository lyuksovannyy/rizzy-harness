//! A person's rewind of the conversation over HTTP (ADOPTION_PLAN X.46,
//! README "Rewind"): `POST /api/sessions/{id}/rewind` and
//! `POST /api/sessions/{id}/rewind/{seq}/undo`. The chat's owner only:
//! every other account gets the 404 of a chat that does not exist (admins
//! and workspace admins included, T10) and the node is told nothing; only a
//! message that opened a turn and is not rewound already, never a
//! sub-agent's chat or a routine's own first message; an undo only before
//! anything new; relayed as `session.rewind` and `session.rewind_undo`, and
//! only to a node that announced `rewind`. The node's side is
//! `crates/node/tests/rewind.rs`.
//!
//! The node here is a fake: it opens chats, runs a turn for a prompt
//! (`child`: also a sub-agent's chat), writes `conversation_rewound` and
//! `conversation_rewind_undone` when told to, and keeps every command.

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
                MasterToNode::Rewind {
                    session_id,
                    message_seq,
                    files,
                    ..
                } => {
                    let turn_id = f
                        .turn_of
                        .get(&(*session_id, *message_seq))
                        .cloned()
                        .unwrap_or_default();
                    f.emit(
                        *session_id,
                        EventKind::ConversationRewound(ConversationRewound {
                            message_seq: *message_seq,
                            turn_id,
                            files: *files,
                        }),
                    );
                }
                MasterToNode::RewindUndo {
                    session_id,
                    rewind_seq,
                    ..
                } => {
                    f.emit(
                        *session_id,
                        EventKind::ConversationRewindUndone(ConversationRewindUndone {
                            rewind_seq: *rewind_seq,
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

/// Every `session.rewind` the node was sent: chat, message, files, force.
fn rewinds(seen: &Seen) -> Vec<(SessionId, u64, bool, bool)> {
    seen.lock()
        .unwrap()
        .iter()
        .filter_map(|m| match m {
            MasterToNode::Rewind {
                session_id,
                message_seq,
                files,
                force,
                ..
            } => Some((*session_id, *message_seq, *files, *force)),
            _ => None,
        })
        .collect()
}

fn undos(seen: &Seen) -> Vec<(SessionId, u64)> {
    seen.lock()
        .unwrap()
        .iter()
        .filter_map(|m| match m {
            MasterToNode::RewindUndo {
                session_id,
                rewind_seq,
                ..
            } => Some((*session_id, *rewind_seq)),
            _ => None,
        })
        .collect()
}

struct T {
    base: String,
    alex: String,
    local: Seen,
    chats: Seen,
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
    let (l, local, children) = fake("local", Profile::Agentic, &[harness_protocol::CAP_REWIND]);
    master.add_link(l, true).await.unwrap();
    let (l, old, _) = fake("old", Profile::Agentic, &[]);
    master.add_link(l, false).await.unwrap();
    let (l, chats, _) = fake("master", Profile::Chat, &[harness_protocol::CAP_REWIND]);
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
        chats,
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

/// Send `text` (a turn of three events) and return the seq of its message.
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
    let evs = caught_up(t, who, sid, before + 3).await;
    evs.iter().rfind(|e| e["kind"] == "user_message").unwrap()["seq"]
        .as_u64()
        .unwrap()
}

fn path(sid: SessionId) -> String {
    format!("/api/sessions/{sid}/rewind")
}

fn undo_path(sid: SessionId, seq: u64) -> String {
    format!("/api/sessions/{sid}/rewind/{seq}/undo")
}

/// The owner's rewind goes to the chat's node as `session.rewind` with
/// the message, the files part and force, and is in the mirror; to the
/// same message again it is refused here and the node is sent nothing; the
/// undo goes the same way, once.
#[tokio::test]
async fn the_owner_rewinds_and_puts_it_back_once() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    send(&t, &t.alex, sid, "first").await;
    let second = send(&t, &t.alex, sid, "second").await;
    let (st, v) = req(
        &t,
        Method::POST,
        &path(sid),
        Some(json!({"message_seq": second, "files": true, "force": true})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(rewinds(&t.local), [(sid, second, true, true)]);
    let evs = caught_up(&t, &t.alex, sid, 8).await;
    let rewound = evs
        .iter()
        .find(|e| e["kind"] == "conversation_rewound")
        .expect("in the mirror");
    assert_eq!(rewound["payload"]["message_seq"], second);
    // To the same message again, and to one inside the range: refused here.
    for seq in [second, second + 1] {
        let (st, v) = req(
            &t,
            Method::POST,
            &path(sid),
            Some(json!({"message_seq": seq})),
            &t.alex,
        )
        .await;
        assert_eq!(st, StatusCode::CONFLICT, "{v}");
        assert!(
            v["error"]
                .as_str()
                .unwrap()
                .starts_with(harness_protocol::REWIND_REFUSED),
            "{v}"
        );
    }
    assert_eq!(rewinds(&t.local).len(), 1);
    // Undo, once.
    let at = rewound["seq"].as_u64().unwrap();
    let (st, v) = req(&t, Method::POST, &undo_path(sid, at), None, &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(undos(&t.local), [(sid, at)]);
    caught_up(&t, &t.alex, sid, 9).await;
    let (st, v) = req(&t, Method::POST, &undo_path(sid, at), None, &t.alex).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert_eq!(undos(&t.local).len(), 1);
}

/// After a new message an undo is refused here, and a message that is not
/// a message (an unknown seq, a turn's start) is not a target.
#[tokio::test]
async fn an_undo_after_something_new_and_a_bad_target_are_refused() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    let first = send(&t, &t.alex, sid, "first").await;
    let (st, _) = req(
        &t,
        Method::POST,
        &path(sid),
        Some(json!({"message_seq": first})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let evs = caught_up(&t, &t.alex, sid, 5).await;
    let at = evs
        .iter()
        .find(|e| e["kind"] == "conversation_rewound")
        .unwrap()["seq"]
        .as_u64()
        .unwrap();
    send(&t, &t.alex, sid, "second").await;
    let (st, v) = req(&t, Method::POST, &undo_path(sid, at), None, &t.alex).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("no longer"), "{v}");
    for seq in [1u64, 9999] {
        let (st, v) = req(
            &t,
            Method::POST,
            &path(sid),
            Some(json!({"message_seq": seq})),
            &t.alex,
        )
        .await;
        assert_eq!(st, StatusCode::CONFLICT, "{seq}: {v}");
    }
    assert_eq!(rewinds(&t.local).len(), 1);
    assert!(undos(&t.local).is_empty());
}

/// Another account cannot rewind someone's chat: a member, a server admin
/// and an admin of the chat's workspace all get the 404 of a chat that does
/// not exist, for the rewind and its undo, and the node is told nothing.
#[tokio::test]
async fn nobody_else_may_rewind_not_even_an_admin() {
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
    let first = send(&t, &t.alex, sid, "first").await;
    for (who, name) in [
        (&bob, "another member"),
        (&carol, "a server admin"),
        (&dana, "a workspace admin"),
    ] {
        let (st, v) = req(
            &t,
            Method::POST,
            &path(sid),
            Some(json!({"message_seq": first})),
            who,
        )
        .await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{name}: {v}");
        assert_eq!(v["error"], "unknown session", "{name}");
        let (st, v) = req(&t, Method::POST, &undo_path(sid, 4), None, who).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{name}: {v}");
    }
    assert!(rewinds(&t.local).is_empty() && undos(&t.local).is_empty());
    assert_eq!(events(&t, &t.alex, sid).await.len(), 4);
}

/// A sub-agent's chat is rewound by the agent that delegated it, never by
/// the person; a node that did not announce `rewind` is told to update.
#[tokio::test]
async fn a_sub_agents_chat_and_an_old_node_are_refused() {
    let t = start().await;
    let parent = chat(&t, &t.alex, "local", None).await;
    send(&t, &t.alex, parent, "child please").await;
    let child = *t.children.lock().unwrap().get(&parent).unwrap();
    let evs = caught_up(&t, &t.alex, child, 4).await;
    let msg = evs.iter().find(|e| e["kind"] == "user_message").unwrap()["seq"]
        .as_u64()
        .unwrap();
    let (st, v) = req(
        &t,
        Method::POST,
        &path(child),
        Some(json!({"message_seq": msg})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("sub-agent"), "{v}");
    assert!(rewinds(&t.local).is_empty());

    let old = chat(&t, &t.alex, "old", None).await;
    let first = send(&t, &t.alex, old, "first").await;
    let (st, v) = req(
        &t,
        Method::POST,
        &path(old),
        Some(json!({"message_seq": first})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"].as_str().unwrap().contains("cannot rewind"),
        "{v}"
    );
    let (st, _) = req(&t, Method::POST, &undo_path(old, 4), None, &t.alex).await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert!(rewinds(&t.old).is_empty() && undos(&t.old).is_empty());
}

/// A routine's own first message is the routine's prompt, not the
/// person's to take back; the owner's later message in the run's chat is.
#[tokio::test]
async fn a_routines_own_message_is_not_rewound_to_but_the_owners_is() {
    let t = start().await;
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/routines",
        Some(json!({
            "name": "nightly",
            "spec": {"profile": "chat", "agent_id": "general", "prompt": "tonight"},
            "triggers": [{"kind": "schedule", "config": {"type": "interval", "every_minutes": 60}}],
        })),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let id = v["routine"]["routine_id"].as_str().unwrap().to_string();
    let (st, v) = req(
        &t,
        Method::POST,
        &format!("/api/routines/{id}/run"),
        None,
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::ACCEPTED, "{v}");
    let run = v["run_id"].as_str().unwrap().to_string();
    let mut sid: Option<SessionId> = None;
    for _ in 0..500 {
        let (_, r) = req(
            &t,
            Method::GET,
            &format!("/api/routines/{id}/runs/{run}"),
            None,
            &t.alex,
        )
        .await;
        if let Some(s) = r["session_id"].as_str() {
            sid = Some(s.parse().unwrap());
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let sid = sid.expect("the run made a chat");
    let evs = caught_up(&t, &t.alex, sid, 4).await;
    let first = evs.iter().find(|e| e["kind"] == "user_message").unwrap()["seq"]
        .as_u64()
        .unwrap();
    let (st, v) = req(
        &t,
        Method::POST,
        &path(sid),
        Some(json!({"message_seq": first})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("routine"), "{v}");
    let later = send(&t, &t.alex, sid, "and later").await;
    let (st, v) = req(
        &t,
        Method::POST,
        &path(sid),
        Some(json!({"message_seq": later})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(rewinds(&t.chats).len(), 1);
}
