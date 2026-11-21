//! A person's Retry over HTTP (ADOPTION_PLAN X.21, README "Retry"):
//! `POST /api/sessions/{id}/turns/{turn}/retry`. The chat's owner only:
//! every other account gets the 404 of a chat that does not exist (admins
//! and workspace admins included, T10), a disabled owner nothing; only the
//! failed last turn of a person's, never one that completed, one with a
//! turn after it, a running one, a sub-agent's or a routine's own run;
//! relayed as `session.retry` with the chat's agent and the person's
//! context, never with a message, and only to a node that announced
//! `retry`. The node's side is `crates/node/tests/retry.rs`.
//!
//! The node here is a fake: it opens chats and runs a turn for a prompt
//! as its text says (`fail`: an error; `hang`: never ends; `child`: also a
//! sub-agent's chat whose turn fails; anything else: an answer), starts a
//! retry's turn without ending it, and keeps every command it is sent.

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
use harness_protocol::{MasterToNode, NodeToMaster, PROTOCOL_VERSION, PromptMsg, RetryMsg};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

type Seen = Arc<Mutex<Vec<MasterToNode>>>;
type Children = Arc<Mutex<HashMap<SessionId, SessionId>>>;

struct Fake {
    tx: mpsc::Sender<NodeToMaster>,
    seqs: HashMap<SessionId, u64>,
    node: String,
    profile: Profile,
    turns: u32,
}

impl Fake {
    fn emit(&mut self, sid: SessionId, k: EventKind) {
        let seq = self.seqs.entry(sid).or_insert(0);
        *seq += 1;
        let (kind, payload) = k.into_parts();
        let _ = self.tx.try_send(NodeToMaster::Event {
            event: RawEvent {
                session_id: sid,
                seq: *seq,
                ts_ms: harness_core::now_ms(),
                kind,
                payload,
            },
        });
    }

    fn created(
        &mut self,
        sid: SessionId,
        parent: Option<SessionId>,
        exec_mode: Option<harness_core::policy::ExecMode>,
    ) {
        let k = EventKind::SessionCreated(SessionCreated {
            exec_mode,
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

    /// One turn of `text`, as the fake's rules say; its id.
    fn turn(&mut self, sid: SessionId, text: &str, retry: Option<RetryOf>) -> String {
        self.turns += 1;
        let t = format!("t{}", self.turns);
        let has_retry = retry.is_some();
        self.emit(
            sid,
            EventKind::TurnStarted(TurnStarted {
                turn_id: t.clone(),
                agent: None,
                cause: None,
                environment: None,
                retry,
                continued_by: None,
            }),
        );
        if has_retry {
            return t;
        }
        // `image`: the message carries an image (a reference, as the node
        // records it).
        let mut m = json!({"turn_id": t, "text": text});
        if text.contains("image") {
            m["images"] = json!([{"media_id": "m1", "sha256": "ab", "mime": "image/png",
                "name": "shot.png", "width": 1, "height": 1}]);
        }
        self.emit(
            sid,
            EventKind::UserMessage(serde_json::from_value(m).unwrap()),
        );
        if text.contains("hang") {
            return t;
        }
        let reason = if text.contains("fail") || text.contains("child") {
            self.emit(
                sid,
                EventKind::Error(ErrorEvent {
                    notice: false,
                    turn_id: Some(t.clone()),
                    message: "the model is not available".into(),
                }),
            );
            TurnEndReason::Error
        } else {
            TurnEndReason::Completed
        };
        self.emit(
            sid,
            EventKind::TurnEnded(TurnEnded {
                turn_id: t.clone(),
                reason,
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
        node: id.to_string(),
        profile,
        turns: 0,
    };
    tokio::spawn(async move {
        while let Some(msg) = commands.recv().await {
            keep.lock().unwrap().push(msg.clone());
            match &msg {
                MasterToNode::Create {
                    session_id,
                    exec_mode,
                    ..
                } => f.created(*session_id, None, *exec_mode),
                MasterToNode::Prompt(PromptMsg {
                    session_id, text, ..
                }) => {
                    if text.contains("child") {
                        let child = SessionId::new();
                        f.created(
                            child,
                            Some(*session_id),
                            Some(harness_core::policy::ExecMode::Sandbox),
                        );
                        f.turn(child, text, None);
                        kids.lock().unwrap().insert(*session_id, child);
                    }
                    f.turn(*session_id, text, None);
                }
                MasterToNode::Retry(RetryMsg {
                    session_id,
                    turn_id,
                    ..
                }) => {
                    let r = RetryOf {
                        turn_id: turn_id.clone(),
                        reason: TurnEndReason::Error,
                        error: None,
                        failed_model: None,
                        model: None,
                    };
                    f.turn(*session_id, "", Some(r));
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

/// Every `session.retry` the node was sent: chat and turn.
fn retries(seen: &Seen) -> Vec<(SessionId, String)> {
    seen.lock()
        .unwrap()
        .iter()
        .filter_map(|m| match m {
            MasterToNode::Retry(RetryMsg {
                session_id,
                turn_id,
                ..
            }) => Some((*session_id, turn_id.clone())),
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
    let (l, local, children) = fake("local", Profile::Agentic, &[harness_protocol::CAP_RETRY]);
    master.add_link(l, true).await.unwrap();
    let (l, old, _) = fake("old", Profile::Agentic, &[]);
    master.add_link(l, false).await.unwrap();
    let (l, chats, _) = fake("master", Profile::Chat, &[harness_protocol::CAP_RETRY]);
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

/// The chat's mirrored events.
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

/// Send `text` and wait until the mirror holds its turn's start; the id of
/// the chat's last turn then.
async fn send(t: &T, who: &str, sid: SessionId, text: &str) -> String {
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
    let want = if text.contains("hang") { 2 } else { 3 };
    last_turn(t, who, sid, before + want).await
}

/// The id of the chat's last turn once the mirror holds `at_least` events.
async fn last_turn(t: &T, who: &str, sid: SessionId, at_least: usize) -> String {
    for _ in 0..500 {
        let evs = events(t, who, sid).await;
        if evs.len() >= at_least {
            return evs
                .iter()
                .rev()
                .find(|e| e["kind"] == "turn_started")
                .map(|e| e["payload"]["turn_id"].as_str().unwrap().to_string())
                .unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!(
        "the mirror never caught up: {:#?}",
        events(t, who, sid).await
    );
}

fn path(sid: SessionId, turn: &str) -> String {
    format!("/api/sessions/{sid}/turns/{turn}/retry")
}

/// The owner's Retry of the failed last turn goes to the chat's node as
/// `session.retry` naming it, with the chat's agent and the person's
/// context, and no message; a second click while it runs is refused here,
/// and the node is sent one.
#[tokio::test]
async fn the_owner_retries_the_failed_last_turn_once() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    let failed = send(&t, &t.alex, sid, "please fail").await;
    let (st, v) = req(&t, Method::POST, &path(sid, &failed), None, &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(retries(&t.local), [(sid, failed.clone())]);
    let sent = t
        .local
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|m| matches!(m, MasterToNode::Retry(_)))
        .cloned()
        .unwrap();
    let MasterToNode::Retry(RetryMsg { agent, context, .. }) = sent else {
        unreachable!()
    };
    assert!(agent.is_some(), "the chat's agent, resolved now");
    let turn = context.and_then(|c| c.turn).unwrap_or_default();
    assert!(turn.contains("alex"), "the person's context: {turn}");
    // The retry's turn runs (the fake never ends it): no second one.
    let n = events(&t, &t.alex, sid).await.len();
    last_turn(&t, &t.alex, sid, n).await;
    for _ in 0..50 {
        if events(&t, &t.alex, sid)
            .await
            .iter()
            .any(|e| e["kind"] == "turn_started" && e["payload"]["retry"].is_object())
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let (st, v) = req(&t, Method::POST, &path(sid, &failed), None, &t.alex).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"]
            .as_str()
            .unwrap()
            .starts_with(harness_protocol::RETRY_REFUSED),
        "{v}"
    );
    assert_eq!(retries(&t.local).len(), 1);
    // No message was written for it, here or on the node.
    let evs = events(&t, &t.alex, sid).await;
    assert_eq!(
        evs.iter().filter(|e| e["kind"] == "user_message").count(),
        1
    );
}

/// Another account cannot retry someone's turn: a member, a server admin
/// and an admin of the chat's workspace all get the 404 of a chat that does
/// not exist; a disabled owner gets nothing; the node is told nothing.
#[tokio::test]
async fn nobody_else_may_retry_not_even_an_admin() {
    let t = start().await;
    let (bob, _) = account(&t, "bob", "member").await;
    let (carol, _) = account(&t, "carol", "admin").await;
    let (dana, _) = account(&t, "dana", "member").await;
    let (erin, erin_id) = account(&t, "erin", "admin").await;
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
    for (name, role) in [("dana", "admin"), ("erin", "member")] {
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
    let failed = send(&t, &t.alex, sid, "fail").await;
    for (who, name) in [
        (&bob, "another member"),
        (&carol, "a server admin"),
        (&dana, "a workspace admin"),
    ] {
        let (st, v) = req(&t, Method::POST, &path(sid, &failed), None, who).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{name}: {v}");
        assert_eq!(v["error"], "unknown session", "{name}");
    }
    // Erin's own failed chat, then Erin is disabled.
    let hers = chat(&t, &erin, "local", None).await;
    let her_failed = send(&t, &erin, hers, "fail").await;
    let (st, v) = req(
        &t,
        Method::PATCH,
        &format!("/api/users/{erin_id}"),
        Some(json!({"disabled": true})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, _) = req(&t, Method::POST, &path(hers, &her_failed), None, &erin).await;
    assert!(
        st == StatusCode::UNAUTHORIZED || st == StatusCode::FORBIDDEN,
        "a disabled owner: {st}"
    );
    assert!(retries(&t.local).is_empty());
}

/// Only the failed last turn of a person's: not one that completed, not
/// one with a turn after it, not one still running, not an unknown one, not
/// a sub-agent's chat; and never on a node that did not announce `retry`.
#[tokio::test]
async fn only_the_failed_last_turn_is_retried() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    let failed = send(&t, &t.alex, sid, "fail").await;
    let done = send(&t, &t.alex, sid, "hello").await;
    for (turn, why) in [
        (&failed, "last turn"),
        (&done, "answer"),
        (&"nope".to_string(), "last turn"),
    ] {
        let (st, v) = req(&t, Method::POST, &path(sid, turn), None, &t.alex).await;
        assert_eq!(st, StatusCode::CONFLICT, "{turn}: {v}");
        assert!(v["error"].as_str().unwrap().contains(why), "{turn}: {v}");
    }
    let running = send(&t, &t.alex, sid, "hang").await;
    let (st, v) = req(&t, Method::POST, &path(sid, &running), None, &t.alex).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("not ended"), "{v}");

    // A sub-agent's chat: its parent retries it, the person does not.
    let parent = chat(&t, &t.alex, "local", None).await;
    send(&t, &t.alex, parent, "child").await;
    let child = *t.children.lock().unwrap().get(&parent).unwrap();
    let child_turn = last_turn(&t, &t.alex, child, 4).await;
    let (st, v) = req(&t, Method::POST, &path(child, &child_turn), None, &t.alex).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("sub-agent"), "{v}");
    assert!(retries(&t.local).is_empty());

    let old = chat(&t, &t.alex, "old", None).await;
    let old_failed = send(&t, &t.alex, old, "fail").await;
    let (st, v) = req(&t, Method::POST, &path(old, &old_failed), None, &t.alex).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("cannot retry"), "{v}");
    assert!(retries(&t.old).is_empty());
}

/// A routine's own run is retried by the routine's rules, not this route;
/// the owner's later turn in the run's chat may be.
#[tokio::test]
async fn a_routines_own_run_is_not_retried_here() {
    let t = start().await;
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/routines",
        Some(json!({
            "name": "nightly",
            "spec": {"profile": "chat", "agent_id": "general", "prompt": "fail tonight"},
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
    let first = last_turn(&t, &t.alex, sid, 4).await;
    let (st, v) = req(&t, Method::POST, &path(sid, &first), None, &t.alex).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("routine"), "{v}");
    // The owner carries on in the run's chat; that turn fails; it may be
    // retried.
    let later = send(&t, &t.alex, sid, "fail again").await;
    let (st, v) = req(&t, Method::POST, &path(sid, &later), None, &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

/// The message a retry runs again had an image: the model it would now run
/// on cannot see images (an unlisted model counts as not seeing them), so
/// the retry is refused with the prompt route's words and nothing is sent.
#[tokio::test]
async fn a_retry_with_images_needs_a_model_that_sees_them() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    let failed = send(&t, &t.alex, sid, "fail with an image").await;
    let (st, v) = req(&t, Method::POST, &path(sid, &failed), None, &t.alex).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"].as_str().unwrap().contains("can't see images"),
        "{v}"
    );
    assert!(retries(&t.local).is_empty());
}

/// A chat a webhook routine started holds data from an external event: the
/// owner's later turn there runs untrusted, and so does its retry (the
/// untrusted note, no web tools, no `remember`, no `recall`).
#[tokio::test]
async fn a_retry_in_a_webhook_chat_stays_untrusted() {
    let t = start().await;
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/routines",
        Some(json!({
            "name": "hooked",
            "spec": {"profile": "chat", "agent_id": "general", "prompt": "Got {{event.body.id}}"},
            "triggers": [{"kind": "webhook", "config": {}}],
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
        Some(json!({"event": {"id": 7}})),
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
    last_turn(&t, &t.alex, sid, 4).await;
    let later = send(&t, &t.alex, sid, "fail please").await;
    let (st, v) = req(&t, Method::POST, &path(sid, &later), None, &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sent = t
        .chats
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|m| matches!(m, MasterToNode::Retry(_)))
        .cloned()
        .expect("the retry went to the chat's node");
    let MasterToNode::Retry(RetryMsg { agent, context, .. }) = sent else {
        unreachable!()
    };
    let turn = context.and_then(|c| c.turn).unwrap_or_default();
    assert!(turn.contains("come from an external event"), "{turn}");
    let deny = agent.unwrap().deny_tools;
    for tool in ["web_fetch", "remember", "recall"] {
        assert!(deny.iter().any(|t| t == tool), "{tool}: {deny:?}");
    }
}
