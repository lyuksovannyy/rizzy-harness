//! A person's fork of a chat over HTTP (ADOPTION_PLAN C.2, README "Fork a
//! chat"): `POST /api/sessions/{id}/fork`. The chat's owner only (everyone
//! else, admins and the API token on an owned chat included, gets the 404 of
//! a chat that does not exist); a finished turn of a chat that is no
//! sub-agent's; relayed as `session.fork` to a node that announced `fork`;
//! the new chat is the owner's, in the parent's workspace.
//!
//! And the guard in `Master::apply`: what the master did when it first
//! applied an event of the parent (filed a memory fact, started an artifact
//! version, resumed the chat for a background end) it does not do again for
//! the copy in a fork. One test per side effect.
//!
//! The node here is a fake: it opens chats, runs a turn for a prompt, writes
//! what a test tells it to (`Do::Put`), and forks as the real node does
//! (`harness_core::fork::build`). The real node's side is
//! `crates/node/tests/fork.rs`.

use crate::common;
use common::{Login as _, SameOrigin as _};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::event::*;
use harness_core::policy::ApprovalMode;
use harness_core::{RawEvent, Secret, SessionId};
use harness_master::auth::{Auth, Claim};
use harness_master::router;
use harness_memory::{MemoryStore, SqliteMemory};
use harness_node::NodeLink;
use harness_protocol::{CallbackMsg, MasterToNode, NodeToMaster, PROTOCOL_VERSION, PromptMsg};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

type Seen = Arc<Mutex<Vec<MasterToNode>>>;

enum Do {
    /// Append an event to a chat of the node's, at the time given.
    Put(SessionId, String, Value, i64),
}

struct Node {
    seen: Seen,
    put: mpsc::UnboundedSender<Do>,
    /// The sub-agents' chats the node opened.
    children: Arc<Mutex<Vec<SessionId>>>,
    /// While set, the node writes a fork but never acks it (a late ack).
    mute_fork_ack: Arc<std::sync::atomic::AtomicBool>,
}

impl Node {
    fn put(&self, sid: SessionId, kind: &str, payload: Value) {
        self.put
            .send(Do::Put(sid, kind.into(), payload, harness_core::now_ms()))
            .unwrap();
    }

    fn forks(&self) -> Vec<(SessionId, SessionId, String, String)> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                MasterToNode::Fork {
                    session_id,
                    parent,
                    turn_id,
                    title,
                    ..
                } => Some((*session_id, *parent, turn_id.clone(), title.clone())),
                _ => None,
            })
            .collect()
    }

    fn callbacks(&self) -> Vec<(SessionId, Vec<String>)> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                MasterToNode::Callback(CallbackMsg {
                    session_id, bg_ids, ..
                }) => Some((*session_id, bg_ids.clone())),
                _ => None,
            })
            .collect()
    }
}

struct Fake {
    tx: mpsc::Sender<NodeToMaster>,
    logs: HashMap<SessionId, Vec<RawEvent>>,
    node: String,
    turns: u32,
}

impl Fake {
    fn push(&mut self, sid: SessionId, kind: String, payload: Value, ts: i64) -> u64 {
        let log = self.logs.entry(sid).or_default();
        let seq = log.len() as u64 + 1;
        let e = RawEvent {
            session_id: sid,
            seq,
            ts_ms: ts,
            kind,
            payload,
        };
        log.push(e.clone());
        let _ = self.tx.try_send(NodeToMaster::Event { event: e });
        seq
    }

    fn emit(&mut self, sid: SessionId, k: EventKind) -> u64 {
        let (kind, payload) = k.into_parts();
        self.push(sid, kind, payload, harness_core::now_ms())
    }

    fn created(&mut self, sid: SessionId, parent: Option<SessionId>, title: &str) {
        self.emit(
            sid,
            EventKind::SessionCreated(SessionCreated {
                forked: None,
                exec_mode: Some(harness_core::policy::ExecMode::Sandbox),
                no_callbacks: false,
                title: title.into(),
                cwd: "/w".into(),
                model: "demo::mock".into(),
                backend: "native".into(),
                node_id: self.node.clone(),
                approval_mode: ApprovalMode::Ask,
                profile: Profile::Agentic,
                agent_id: None,
                incognito: false,
                parent_session: parent,
            }),
        );
    }

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
        self.emit(
            sid,
            EventKind::UserMessage(
                serde_json::from_value(json!({"turn_id": t, "text": text})).unwrap(),
            ),
        );
        self.emit(
            sid,
            EventKind::TurnEnded(TurnEnded {
                turn_id: t.clone(),
                reason: TurnEndReason::Completed,
                usage: Some(Usage {
                    input_tokens: 100,
                    output_tokens: 10,
                    ..Usage::default()
                }),
            }),
        );
        t
    }
}

fn fake(id: &str, caps: &[&str]) -> (NodeLink, Node) {
    let (to_node, mut commands) = mpsc::channel::<MasterToNode>(1024);
    let (tx, from_node) = mpsc::channel::<NodeToMaster>(4096);
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
    let keep = seen.clone();
    let children: Arc<Mutex<Vec<SessionId>>> = Arc::default();
    let kids = children.clone();
    let (put, mut puts) = mpsc::unbounded_channel::<Do>();
    let mute_fork_ack: Arc<std::sync::atomic::AtomicBool> = Arc::default();
    let mute = mute_fork_ack.clone();
    let mut f = Fake {
        tx: tx.clone(),
        logs: HashMap::new(),
        node: id.to_string(),
        turns: 0,
    };
    tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(Do::Put(sid, kind, payload, ts)) = puts.recv() => {
                    f.push(sid, kind, payload, ts);
                }
                msg = commands.recv() => {
                    let Some(msg) = msg else { return };
                    keep.lock().unwrap().push(msg.clone());
                    let mut nack: Option<String> = None;
                    match &msg {
                        MasterToNode::Create { session_id, title, .. } => {
                            f.created(*session_id, None, title);
                        }
                        MasterToNode::Prompt(PromptMsg { session_id, text, .. }) => {
                            if text.contains("child") {
                                let child = SessionId::new();
                                f.created(child, Some(*session_id), "child");
                                f.turn(child, text);
                                kids.lock().unwrap().push(child);
                            }
                            f.turn(*session_id, text);
                        }
                        MasterToNode::Resume { session_id, last_seq } => {
                            let log = f.logs.get(session_id).cloned().unwrap_or_default();
                            let events: Vec<RawEvent> = log
                                .iter()
                                .filter(|e| e.seq > *last_seq)
                                .cloned()
                                .collect();
                            let _ = tx.try_send(NodeToMaster::Snapshot {
                                session_id: *session_id,
                                head_seq: log.len() as u64,
                                events,
                            });
                        }
                        MasterToNode::Fork { session_id, parent, turn_id, title, .. } => {
                            let log = f.logs.get(parent).cloned().unwrap_or_default();
                            match harness_core::fork::check(&log, turn_id) {
                                Err(e) => nack = Some(e),
                                Ok(cut) => {
                                    let built = harness_core::fork::build(
                                        &log, &cut, *session_id, title, "/w", harness_core::now_ms(),
                                    )
                                    .unwrap();
                                    for e in built {
                                        f.push(*session_id, e.kind, e.payload, e.ts_ms);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                    let withheld = matches!(msg, MasterToNode::Fork { .. })
                        && mute.load(std::sync::atomic::Ordering::SeqCst);
                    if let Some(req_id) = msg.req_id().filter(|_| !withheld) {
                        let _ = tx.try_send(NodeToMaster::Ack {
                            req_id: req_id.into(),
                            ok: nack.is_none(),
                            error: nack,
                        });
                    }
                }
            }
        }
    });
    (
        NodeLink { to_node, from_node },
        Node {
            seen,
            put,
            children,
            mute_fork_ack,
        },
    )
}

struct T {
    base: String,
    alex: String,
    local: Node,
    old: Node,
    memory: Arc<SqliteMemory>,
    master: harness_master::Master,
    /// The master's mirror, to read what it kept.
    store: Store,
    /// The mirror's file, when the test asked for one (to break it).
    db: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

const TOKEN: &str = "api-secret";

async fn start() -> T {
    start_with(false).await
}

async fn start_with(file: bool) -> T {
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
    let db = dir.path().join("master.db");
    let store = if file {
        Store::open(&db, harness_store::Durability::Full).unwrap()
    } else {
        Store::open_in_memory().unwrap()
    };
    let master = harness_master::Master::new(store.clone(), auth);
    master.set_agents(harness_agents::AgentSet::load(&dir.path().join("none")).0);
    let memory = Arc::new(SqliteMemory::open_in_memory().unwrap());
    master.set_memory(memory.clone());
    let (l, local) = fake(
        "local",
        &[
            harness_protocol::CAP_FORK,
            harness_protocol::CAP_REMEMBER,
            harness_protocol::CAP_BACKGROUND,
            harness_protocol::CAP_CALLBACK,
            harness_protocol::CAP_ARTIFACTS,
        ],
    );
    master.add_link(l, true).await.unwrap();
    let (l, old) = fake("old", &[]);
    master.add_link(l, false).await.unwrap();
    harness_master::routines::start_with(
        &master,
        harness_master::routines::Clock::manual(Arc::new(std::sync::atomic::AtomicI64::new(
            harness_core::now_ms(),
        ))),
        false,
    );
    let m2 = master.clone();
    let master = master;
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
        master,
        base,
        alex,
        local,
        old,
        memory,
        store,
        db,
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

async fn chat(t: &T, who: &str, node: &str) -> SessionId {
    let (st, v) = req(
        t,
        Method::POST,
        "/api/sessions",
        Some(json!({"cwd": "/w", "title": "Parent", "node_id": node})),
        who,
    )
    .await;
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

async fn caught_up(t: &T, who: &str, sid: SessionId, at_least: usize) -> Vec<Value> {
    for _ in 0..1000 {
        let evs = events(t, who, sid).await;
        if evs.len() >= at_least {
            return evs;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "the mirror never caught up: {:#?}",
        events(t, who, sid).await
    );
}

/// Send `text` (a turn of three events); returns its turn id.
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
    let evs = caught_up(t, who, sid, before + 3).await;
    evs.iter().rfind(|e| e["kind"] == "turn_ended").unwrap()["payload"]["turn_id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn path(sid: SessionId) -> String {
    format!("/api/sessions/{sid}/fork")
}

async fn fork(t: &T, who: &str, sid: SessionId, turn: &str) -> (StatusCode, Value) {
    req(
        t,
        Method::POST,
        &path(sid),
        Some(json!({"turn_id": turn})),
        who,
    )
    .await
}

/// The owner forks a finished turn: the node is sent `session.fork` with
/// the parent, the turn and the title; the new chat is the owner's, has the
/// parent's events up to the cut (same seqs) and the divider, and the
/// parent's mirror is as it was.
#[tokio::test]
async fn the_owner_forks_a_finished_turn_and_the_new_chat_is_theirs() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local").await;
    let first = send(&t, &t.alex, sid, "first").await;
    send(&t, &t.alex, sid, "second").await;
    let parent = events(&t, &t.alex, sid).await;
    let (st, v) = fork(&t, &t.alex, sid, &first).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let new: SessionId = v["session_id"].as_str().unwrap().parse().unwrap();
    assert_ne!(new, sid);
    assert_eq!(
        t.local.forks(),
        vec![(new, sid, first.clone(), "Parent (fork)".to_string())]
    );
    // Turn one ends at seq 4: the copy and the divider.
    let evs = caught_up(&t, &t.alex, new, 5).await;
    assert_eq!(evs.len(), 5, "{evs:#?}");
    for (i, e) in evs.iter().enumerate() {
        assert_eq!(e["seq"], i as u64 + 1);
    }
    for i in 1..4 {
        assert_eq!(evs[i]["kind"], parent[i]["kind"]);
        assert_eq!(evs[i]["payload"], parent[i]["payload"]);
    }
    assert_eq!(evs[0]["payload"]["title"], "Parent (fork)");
    assert_eq!(evs[0]["payload"]["forked"]["parent"], json!(sid));
    assert_eq!(evs[4]["kind"], "session_forked");
    assert_eq!(
        events(&t, &t.alex, sid).await,
        parent,
        "the parent is as it was"
    );
    // It is the owner's: in their list, readable by them, a 404 for others.
    let (st, list) = req(&t, Method::GET, "/api/sessions", None, &t.alex).await;
    assert_eq!(st, StatusCode::OK);
    let row = list["sessions"]
        .as_array()
        .or(list.as_array())
        .unwrap()
        .iter()
        .find(|s| s["session_id"] == json!(new))
        .cloned()
        .expect("listed");
    assert_eq!(row["title"], "Parent (fork)");
    let bob = account(&t, "bob", "member").await;
    let (st, _) = req(
        &t,
        Method::GET,
        &format!("/api/sessions/{new}/events?after=0"),
        None,
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // The parent deleted afterwards: the fork opens as it did.
    let (st, v) = req(
        &t,
        Method::DELETE,
        &format!("/api/sessions/{sid}"),
        None,
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(events(&t, &t.alex, new).await.len(), 5);
    // A fork of the fork.
    let (st, v) = fork(&t, &t.alex, new, &first).await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

/// Nobody else may fork a chat: another member, a server admin and the API
/// token on an owned chat all get the 404 of a chat that does not exist, and
/// the node is told nothing.
#[tokio::test]
async fn nobody_else_may_fork_not_an_admin_and_not_the_api_token() {
    let t = start().await;
    let bob = account(&t, "bob", "member").await;
    let carol = account(&t, "carol", "admin").await;
    let sid = chat(&t, &t.alex, "local").await;
    let first = send(&t, &t.alex, sid, "first").await;
    for (who, name) in [(&bob, "a member"), (&carol, "a server admin")] {
        let (st, v) = fork(&t, who, sid, &first).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{name}: {v}");
        assert_eq!(v["error"], "unknown session", "{name}");
    }
    let (st, v) = token_req(
        &t,
        Method::POST,
        &path(sid),
        Some(json!({"turn_id": first})),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{v}");
    assert_eq!(v["error"], "unknown session");
    let (st, missing) = token_req(
        &t,
        Method::POST,
        &path(SessionId::new()),
        Some(json!({"turn_id": first})),
    )
    .await;
    assert_eq!(
        (st, &missing["error"]),
        (StatusCode::NOT_FOUND, &v["error"])
    );
    assert!(t.local.forks().is_empty());
}

/// A sub-agent's chat, a turn that has not ended and one that is not there
/// are refused here and the node is sent nothing; so is a chat on a node
/// that did not announce `fork`.
#[tokio::test]
async fn a_sub_agents_chat_a_running_turn_and_an_old_node_are_refused() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local").await;
    let first = send(&t, &t.alex, sid, "child please").await;
    // The sub-agent's chat the fake opened: owned by the parent's owner,
    // and still not a chat to fork.
    let child = t.local.children.lock().unwrap()[0];
    caught_up(&t, &t.alex, child, 4).await;
    let (st, v) = fork(&t, &t.alex, child, "t1").await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("sub-agent"), "{v}");
    // A turn still running: started, never ended.
    t.local
        .put(sid, "turn_started", json!({"turn_id": "running"}));
    caught_up(&t, &t.alex, sid, 5).await;
    let (st, v) = fork(&t, &t.alex, sid, "running").await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"].as_str().unwrap().contains("still running"),
        "{v}"
    );
    let (st, v) = fork(&t, &t.alex, sid, "no-such-turn").await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"]
            .as_str()
            .unwrap()
            .starts_with(harness_protocol::FORK_REFUSED),
        "{v}"
    );
    // The chat that ran on the old node.
    let old = chat(&t, &t.alex, "old").await;
    let turn = send(&t, &t.alex, old, "hello").await;
    let (st, v) = fork(&t, &t.alex, old, &turn).await;
    assert_ne!(st, StatusCode::OK, "{v}");
    assert!(t.old.forks().is_empty());
    assert!(t.local.forks().is_empty());
    // The one the fork would have been of stays forkable.
    let (st, _) = fork(&t, &t.alex, sid, &first).await;
    assert_eq!(st, StatusCode::OK);
}

/// A copied `remember` is not filed again: the dedupe key holds the chat's
/// id, so without the guard in `apply` the copy would file the same fact a
/// second time, for the fork.
#[tokio::test]
async fn a_copied_remember_is_filed_once() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local").await;
    t.local.put(sid, "turn_started", json!({"turn_id": "tm"}));
    t.local.put(
        sid,
        "user_message",
        json!({"turn_id": "tm", "text": "remember it"}),
    );
    t.local.put(
        sid,
        "tool_call_started",
        json!({"turn_id": "tm", "call_id": "c1", "tool": "remember",
               "input": {"text": "Alex rides the number 7 tram"}}),
    );
    t.local.put(
        sid,
        "tool_call_finished",
        json!({"call_id": "c1", "ok": true, "output": "saved", "truncated": false}),
    );
    t.local.put(
        sid,
        "turn_ended",
        json!({"turn_id": "tm", "reason": "completed"}),
    );
    caught_up(&t, &t.alex, sid, 6).await;
    let owner = account_id(&t).await;
    let count = |t: &T| {
        let m = t.memory.clone();
        let o = owner.clone();
        async move { m.list(Some(&o)).await.unwrap().len() }
    };
    for _ in 0..200 {
        if count(&t).await == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(count(&t).await, 1, "the parent's own call is filed");
    let (st, v) = fork(&t, &t.alex, sid, "tm").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let new: SessionId = v["session_id"].as_str().unwrap().parse().unwrap();
    caught_up(&t, &t.alex, new, 7).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(count(&t).await, 1, "the copy filed nothing");
}

async fn account_id(t: &T) -> String {
    let (st, v) = req(t, Method::GET, "/api/me", None, &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["user_id"]
        .as_str()
        .or_else(|| v["user"]["user_id"].as_str())
        .unwrap_or_else(|| panic!("{v}"))
        .to_string()
}

/// A copied `artifact_staged` makes no version for the fork: the mirror
/// holds one for the parent's own stage and none for the fork's copy of it.
#[tokio::test]
async fn a_copied_artifact_is_staged_once() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local").await;
    t.local.put(sid, "turn_started", json!({"turn_id": "ta"}));
    t.local.put(
        sid,
        "user_message",
        json!({"turn_id": "ta", "text": "publish it"}),
    );
    t.local.put(
        sid,
        "artifact_staged",
        json!({"stage_id": "01a19bf2000070008000000000000a01", "by": "user", "name": "app",
               "filename": "app", "size": 12, "sha256": "ab".repeat(32),
               "source_path": "/w/app", "meta": {}}),
    );
    t.local.put(
        sid,
        "turn_ended",
        json!({"turn_id": "ta", "reason": "completed"}),
    );
    caught_up(&t, &t.alex, sid, 5).await;
    let versions = |id: SessionId| {
        let s = t.store.clone();
        async move {
            s.call(move |s| s.artifact_session_versions(&id.to_string()))
                .await
                .unwrap()
                .len()
        }
    };
    let mut before = 0;
    for _ in 0..300 {
        before = versions(sid).await;
        if before >= 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(before, 1, "the parent's own stage is a version");
    let (st, v) = fork(&t, &t.alex, sid, "ta").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let new: SessionId = v["session_id"].as_str().unwrap().parse().unwrap();
    caught_up(&t, &t.alex, new, 6).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(versions(new).await, 0, "the copy made none for the fork");
    assert_eq!(versions(sid).await, 1);
}

/// A copied `background_ended` that would call back resumes nothing in the
/// fork: the parent's end waited for a callback turn at the cut, the fork's
/// divider clears it, and when the fork's own command ends the callback
/// names that command alone.
#[tokio::test]
async fn a_copied_background_end_calls_nothing_back_in_the_fork() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local").await;
    t.local.put(sid, "turn_started", json!({"turn_id": "tb"}));
    t.local.put(
        sid,
        "user_message",
        json!({"turn_id": "tb", "text": "start it"}),
    );
    t.local.put(
        sid,
        "background_started",
        json!({"bg_id": "bg-1", "call_id": "c", "turn_id": "tb", "timeout_s": 60,
               "ends_by_ms": harness_core::now_ms() + 60_000}),
    );
    // Ended while the turn ran, and not told to it: it calls back.
    t.local.put(
        sid,
        "background_ended",
        json!({"bg_id": "bg-1", "reason": "exited", "exit_code": 0, "confirmed": "ended",
               "ran_ms": 1, "output_bytes": 0, "tail": "", "tail_cut": false, "delivered": false}),
    );
    t.local.put(
        sid,
        "turn_ended",
        json!({"turn_id": "tb", "reason": "completed"}),
    );
    caught_up(&t, &t.alex, sid, 6).await;
    for _ in 0..300 {
        if !t.local.callbacks().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        t.local.callbacks(),
        vec![(sid, vec!["bg-1".to_string()])],
        "the parent is resumed for its own end"
    );
    let (st, v) = fork(&t, &t.alex, sid, "tb").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let new: SessionId = v["session_id"].as_str().unwrap().parse().unwrap();
    caught_up(&t, &t.alex, new, 7).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(
        t.local.callbacks().iter().all(|(s, _)| *s != new),
        "{:?}",
        t.local.callbacks()
    );
    // The fork's own command: its end is the only one that waits.
    t.local.put(
        new,
        "background_started",
        json!({"bg_id": "bg-2", "call_id": "c2", "turn_id": "tb", "timeout_s": 60,
               "ends_by_ms": harness_core::now_ms() + 60_000}),
    );
    t.local.put(
        new,
        "background_ended",
        json!({"bg_id": "bg-2", "reason": "exited", "exit_code": 0, "confirmed": "ended",
               "ran_ms": 1, "output_bytes": 0, "tail": "", "tail_cut": false, "delivered": false}),
    );
    for _ in 0..300 {
        if t.local.callbacks().iter().any(|(s, _)| *s == new) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let mine: Vec<_> = t
        .local
        .callbacks()
        .into_iter()
        .filter(|(s, _)| *s == new)
        .collect();
    assert_eq!(mine, vec![(new, vec!["bg-2".to_string()])]);
}

/// The page offers a fork only where the node announced `fork`: the nodes
/// list says which do.
#[tokio::test]
async fn the_nodes_list_says_which_nodes_fork() {
    let t = start().await;
    let (st, v) = req(&t, Method::GET, "/api/nodes", None, &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let nodes = v["nodes"].as_array().or(v.as_array()).unwrap();
    let fork_of = |id: &str| {
        nodes
            .iter()
            .find(|n| n["node_id"] == id)
            .unwrap_or_else(|| panic!("{v}"))["fork"]
            .clone()
    };
    assert_eq!(fork_of("local"), json!(true));
    assert_eq!(fork_of("old"), json!(false));
}

/// The first event of a fork the node writes by hand (`forked` naming a
/// parent the mirror may not even hold).
fn forked_created(parent: SessionId, through: u64) -> (String, Value) {
    EventKind::SessionCreated(SessionCreated {
        forked: Some(ForkedFrom {
            parent,
            through_seq: through,
        }),
        exec_mode: Some(harness_core::policy::ExecMode::Sandbox),
        no_callbacks: false,
        title: "Hand made (fork)".into(),
        cwd: "/w".into(),
        model: "demo::mock".into(),
        backend: "native".into(),
        node_id: "local".into(),
        approval_mode: ApprovalMode::Ask,
        profile: Profile::Agentic,
        agent_id: None,
        incognito: false,
        parent_session: None,
    })
    .into_parts()
}

fn break_table(t: &T, sql: &str) {
    let c = rusqlite::Connection::open(&t.db).unwrap();
    c.execute_batch(sql).unwrap();
}

/// A read of the lineage that fails is not an answer: the copied events
/// arriving meanwhile are not applied (not taken into the mirror, no side
/// effect), and not turned into "not a fork" for good. Once the read works
/// again the node's resume brings them, and they are the copies they are:
/// the copied `remember` files nothing.
#[tokio::test]
async fn a_failed_lineage_read_is_not_cached_and_the_copy_is_not_applied() {
    let t = start_with(true).await;
    let owner = account_id(&t).await;
    let parent = SessionId::new();
    let new = SessionId::new();
    let (kind, payload) = forked_created(parent, 6);
    t.local.put(new, &kind, payload);
    for _ in 0..500 {
        if t.store.session(new).unwrap().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    t.store.claim_session(new, Some(&owner), None).unwrap();
    assert!(
        t.store.fork_of(new).unwrap().is_some(),
        "the lineage is there"
    );
    // The lineage cannot be read now.
    break_table(
        &t,
        "ALTER TABLE session_forks RENAME TO session_forks_gone;",
    );
    t.local.put(new, "turn_started", json!({"turn_id": "tm"}));
    t.local.put(
        new,
        "user_message",
        json!({"turn_id": "tm", "text": "remember it"}),
    );
    t.local.put(
        new,
        "tool_call_started",
        json!({"turn_id": "tm", "call_id": "c1", "tool": "remember",
               "input": {"text": "Alex rides the number 9 tram"}}),
    );
    t.local.put(
        new,
        "tool_call_finished",
        json!({"call_id": "c1", "ok": true, "output": "saved", "truncated": false}),
    );
    t.local.put(
        new,
        "turn_ended",
        json!({"turn_id": "tm", "reason": "completed"}),
    );
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(
        events(&t, &t.alex, new).await.len(),
        1,
        "nothing was taken while the lineage could not be read"
    );
    assert_eq!(t.memory.list(Some(&owner)).await.unwrap().len(), 0);
    // The read works again; the divider arrives, the gap is resumed.
    break_table(
        &t,
        "ALTER TABLE session_forks_gone RENAME TO session_forks;",
    );
    t.local.put(
        new,
        "session_forked",
        json!({"parent": parent, "through_seq": 6, "turn_id": "tm"}),
    );
    caught_up(&t, &t.alex, new, 7).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        t.memory.list(Some(&owner)).await.unwrap().len(),
        0,
        "the copy filed nothing, as copies do not"
    );
}

/// The guard is the protection, not the divider: a long copy whose
/// `background_ended` (one that would call back) is applied well before the
/// divider arrives resumes nothing and records no refusal in the fork.
/// (With the guard off the callback watcher looks at the fork after its
/// settling time, finds the copied end waiting, and either resumes the fork
/// or records that it did not.)
#[tokio::test]
async fn a_copied_background_end_applied_before_the_divider_calls_nothing_back() {
    let t = start().await;
    let owner = account_id(&t).await;
    let parent = SessionId::new();
    let new = SessionId::new();
    let (kind, payload) = forked_created(parent, 6);
    t.local.put(new, &kind, payload);
    for _ in 0..500 {
        if t.store.session(new).unwrap().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    t.store.claim_session(new, Some(&owner), None).unwrap();
    t.local.put(new, "turn_started", json!({"turn_id": "tb"}));
    t.local.put(
        new,
        "user_message",
        json!({"turn_id": "tb", "text": "start it"}),
    );
    t.local.put(
        new,
        "background_started",
        json!({"bg_id": "bg-1", "call_id": "c", "turn_id": "tb", "timeout_s": 60,
               "ends_by_ms": harness_core::now_ms() + 60_000}),
    );
    t.local.put(
        new,
        "background_ended",
        json!({"bg_id": "bg-1", "reason": "exited", "exit_code": 0, "confirmed": "ended",
               "ran_ms": 1, "output_bytes": 0, "tail": "", "tail_cut": false, "delivered": false}),
    );
    t.local.put(
        new,
        "turn_ended",
        json!({"turn_id": "tb", "reason": "completed"}),
    );
    caught_up(&t, &t.alex, new, 6).await;
    // Long enough for the watcher's settling time and its look.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(
        t.local.callbacks().iter().all(|(s, _)| *s != new),
        "the copy resumed the fork: {:?}",
        t.local.callbacks()
    );
    let refused = t.store.audit_lines("callback.refused").unwrap();
    assert!(refused.is_empty(), "a refusal was recorded: {refused:?}");
    assert!(
        t.store
            .kv_get(&format!("callback-attention:{new}"))
            .unwrap()
            .is_none(),
        "the chat was marked as not resumed"
    );
    // The divider, late: still nothing.
    t.local.put(
        new,
        "session_forked",
        json!({"parent": parent, "through_seq": 6, "turn_id": "tb"}),
    );
    caught_up(&t, &t.alex, new, 7).await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(t.local.callbacks().iter().all(|(s, _)| *s != new));
}

/// A fork whose ack comes late still gets what the normal path gives it:
/// the intent the master recorded carries the parent's project and the
/// auto-model mark, and the late claim applies them.
#[tokio::test]
async fn a_fork_whose_ack_is_late_still_gets_its_project_and_the_auto_model() {
    let t = start().await;
    let (st, p) = req(
        &t,
        Method::POST,
        "/api/projects",
        Some(json!({"name": "Garden", "icon": "house", "accent": "green"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{p}");
    let project = p["project_id"].as_str().unwrap().to_string();
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/sessions",
        Some(json!({"cwd": "/w", "title": "Parent", "node_id": "local", "project_id": project})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid: SessionId = v["session_id"].as_str().unwrap().parse().unwrap();
    let first = send(&t, &t.alex, sid, "first").await;
    let linked = |id: SessionId| t.store.session_project(id).unwrap().map(|l| l.project_id);
    assert_eq!(linked(sid).as_deref(), Some(project.as_str()));
    let auto = |id: SessionId| t.store.kv_get(&format!("session-model-auto:{id}")).unwrap();
    assert!(auto(sid).is_some(), "the parent follows the defaults");
    // The node writes the fork and says nothing in time.
    t.master.set_ack_timeout_for_test(Duration::from_millis(50));
    t.local
        .mute_fork_ack
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let (st, v) = fork(&t, &t.alex, sid, &first).await;
    assert_ne!(st, StatusCode::OK, "the ack never came: {v}");
    let new = t.local.forks()[0].0;
    caught_up(&t, &t.alex, new, 5).await;
    assert!(
        t.store.session(new).unwrap().unwrap().owner_id.is_some(),
        "claimed for whoever asked"
    );
    assert_eq!(
        linked(new).as_deref(),
        Some(project.as_str()),
        "in the parent's project"
    );
    assert!(auto(new).is_some(), "following the defaults, as the parent");
}
