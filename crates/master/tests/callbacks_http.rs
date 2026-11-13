//! The master's side of callback turns (T.8d step 3, docs/background-
//! commands.md §4.4 steps 1 to 4, §13): the watcher sees an end that may
//! call back, admits it as a routine fire is admitted (less the `routines`
//! permission), and asks the owning node with `session.callback`; several
//! ends are one request; an end older than 60 minutes, an owner who lost
//! access, a disabled account and a chat that left its workspace get none,
//! each with an attention item for the owner only; the watcher's position
//! is the log, so a restarted master asks once and loses nothing.
//!
//! The node here is a fake that opens chats, appends what a test tells it
//! (with the time it says), keeps every command, and answers a callback as
//! the real node does: a turn with `cause`, or the refusal a test set. The
//! real node's side is `crates/cli/tests/callbacks.rs`.

mod common;
use common::{Login as _, SameOrigin as _};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::event::{
    BackgroundConfirmed, BackgroundEnded, BackgroundReason, BackgroundStarted, EventKind, Profile,
    SessionCreated, TurnCause, TurnEndReason, TurnEnded, TurnStarted,
};
use harness_core::policy::ApprovalMode;
use harness_core::{RawEvent, Secret, SessionId};
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, router};
use harness_node::NodeLink;
use harness_protocol::{MasterToNode, NodeToMaster, PROTOCOL_VERSION};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

const TOKEN: &str = "api-secret";
const MIN: i64 = 60_000;

enum Do {
    Append(SessionId, Box<EventKind>, i64),
    /// Send this event, then drop the link: a node that went away.
    AppendThenLeave(SessionId, Box<EventKind>),
    /// Send a raw event (a kind this build may not type).
    Raw(SessionId, String, Value),
}

#[derive(Clone)]
struct Fake {
    seen: Arc<Mutex<Vec<MasterToNode>>>,
    tx: mpsc::UnboundedSender<Do>,
    /// The answer to the next callbacks (`None`: start the turn).
    refuse: Arc<Mutex<Option<String>>>,
    /// The answer to the next prompts (`None`: start the person's turn).
    refuse_prompts: Arc<Mutex<Option<String>>>,
    /// A started turn (a callback's or a person's) stays running: its
    /// `turn_ended` is the test's to send.
    hold: Arc<Mutex<bool>>,
}

impl Fake {
    fn callbacks(&self) -> Vec<(SessionId, Vec<String>)> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                MasterToNode::Callback {
                    session_id, bg_ids, ..
                } => Some((*session_id, bg_ids.clone())),
                _ => None,
            })
            .collect()
    }

    fn append_at(&self, sid: SessionId, k: EventKind, ts: i64) {
        self.tx.send(Do::Append(sid, Box::new(k), ts)).unwrap();
    }

    fn append(&self, sid: SessionId, k: EventKind) {
        self.append_at(sid, k, harness_core::now_ms());
    }

    fn append_then_leave(&self, sid: SessionId, k: EventKind) {
        self.tx.send(Do::AppendThenLeave(sid, Box::new(k))).unwrap();
    }

    fn raw(&self, sid: SessionId, kind: &str, payload: Value) {
        self.tx.send(Do::Raw(sid, kind.into(), payload)).unwrap();
    }
}

/// An agentic node `id` announcing `background` and `callback`.
fn fake(id: &str, sessions: Vec<harness_protocol::SessionHead>) -> (NodeLink, Fake) {
    fake_with(id, sessions, true)
}

/// `fake`, announcing `callback` only when `callback`.
fn fake_with(
    id: &str,
    sessions: Vec<harness_protocol::SessionHead>,
    callback: bool,
) -> (NodeLink, Fake) {
    let (to_node, mut commands) = mpsc::channel::<MasterToNode>(1024);
    let (tx, from_node) = mpsc::channel::<NodeToMaster>(1024);
    tx.try_send(NodeToMaster::Hello {
        protocol_version: PROTOCOL_VERSION,
        node_id: id.into(),
        hostname: "h".into(),
        version: "1".into(),
        capabilities: [
            "native",
            "profile:agentic",
            harness_protocol::CAP_BACKGROUND,
            harness_protocol::CAP_CALLBACK,
        ]
        .into_iter()
        .filter(|c| callback || *c != harness_protocol::CAP_CALLBACK)
        .map(String::from)
        .collect(),
        sessions: sessions.clone(),
    })
    .unwrap();
    let seen: Arc<Mutex<Vec<MasterToNode>>> = Arc::default();
    let refuse: Arc<Mutex<Option<String>>> = Arc::default();
    let refuse_prompts: Arc<Mutex<Option<String>>> = Arc::default();
    let hold: Arc<Mutex<bool>> = Arc::default();
    let (do_tx, mut do_rx) = mpsc::unbounded_channel::<Do>();
    let (node, keep, no) = (id.to_string(), seen.clone(), refuse.clone());
    let (no_prompt, held) = (refuse_prompts.clone(), hold.clone());
    tokio::spawn(async move {
        let mut seqs: HashMap<SessionId, u64> = sessions
            .iter()
            .map(|h| (h.session_id, h.head_seq))
            .collect();
        let send = |seqs: &mut HashMap<SessionId, u64>, sid: SessionId, k: EventKind, ts: i64| {
            let seq = seqs.entry(sid).or_insert(0);
            *seq += 1;
            let (kind, payload) = k.into_parts();
            let _ = tx.try_send(NodeToMaster::Event {
                event: RawEvent {
                    session_id: sid,
                    seq: *seq,
                    ts_ms: ts,
                    kind,
                    payload,
                },
            });
        };
        loop {
            tokio::select! {
                msg = commands.recv() => {
                    let Some(msg) = msg else { return };
                    keep.lock().unwrap().push(msg.clone());
                    let now = harness_core::now_ms();
                    if let MasterToNode::Create { session_id, agent, .. } = &msg {
                        let created = EventKind::SessionCreated(SessionCreated {
                            title: "t".into(),
                            cwd: "/w".into(),
                            model: "demo::mock".into(),
                            backend: "native".into(),
                            node_id: node.clone(),
                            approval_mode: ApprovalMode::Auto,
                            profile: Profile::Agentic,
                            agent_id: agent.as_ref().map(|a| a.agent_id.clone()),
                            incognito: false,
                            parent_session: None,
                            no_callbacks: false,
                        });
                        send(&mut seqs, *session_id, created, now);
                    }
                    let mut error = None;
                    if let MasterToNode::Callback { session_id, bg_ids, .. } = &msg {
                        error = no.lock().unwrap().clone();
                        if error.is_none() {
                            send(&mut seqs, *session_id, EventKind::TurnStarted(TurnStarted {
                                turn_id: "cb".into(),
                                agent: None,
                                cause: Some(TurnCause::Callback(bg_ids.clone())),
                            }), now);
                            if !*held.lock().unwrap() {
                                send(&mut seqs, *session_id, EventKind::TurnEnded(TurnEnded {
                                    turn_id: "cb".into(),
                                    reason: TurnEndReason::Completed,
                                    usage: None,
                                }), now);
                            }
                        }
                    }
                    if let MasterToNode::Prompt { session_id, text, .. } = &msg {
                        error = no_prompt.lock().unwrap().clone();
                        if error.is_none() {
                            send(&mut seqs, *session_id, EventKind::TurnStarted(TurnStarted {
                                turn_id: "p".into(),
                                agent: None,
                                cause: None,
                            }), now);
                            send(&mut seqs, *session_id, EventKind::UserMessage(harness_core::event::UserMessage {
                                turn_id: "p".into(),
                                text: text.clone(),
                                attachments: vec![],
                                reply: None,
                                images: vec![],
                            }), now);
                            if !*held.lock().unwrap() {
                                send(&mut seqs, *session_id, EventKind::TurnEnded(TurnEnded {
                                    turn_id: "p".into(),
                                    reason: TurnEndReason::Completed,
                                    usage: None,
                                }), now);
                            }
                        }
                    }
                    if let Some(req_id) = msg.req_id() {
                        let _ = tx.try_send(NodeToMaster::Ack {
                            req_id: req_id.into(),
                            ok: error.is_none(),
                            error,
                        });
                    }
                }
                d = do_rx.recv() => match d {
                    Some(Do::Append(sid, k, ts)) => send(&mut seqs, sid, *k, ts),
                    Some(Do::AppendThenLeave(sid, k)) => {
                        send(&mut seqs, sid, *k, harness_core::now_ms());
                        return;
                    }
                    Some(Do::Raw(sid, kind, payload)) => {
                        let seq = seqs.entry(sid).or_insert(0);
                        *seq += 1;
                        let _ = tx.try_send(NodeToMaster::Event {
                            event: RawEvent { session_id: sid, seq: *seq, ts_ms: harness_core::now_ms(), kind, payload },
                        });
                    }
                    None => return,
                },
            }
        }
    });
    (
        NodeLink { to_node, from_node },
        Fake {
            seen,
            tx: do_tx,
            refuse,
            refuse_prompts,
            hold,
        },
    )
}

struct T {
    base: String,
    master: Master,
    alex: String,
    node: Fake,
}

fn auth(dir: &std::path::Path, addr: &str) -> Auth {
    let claim_path = dir.join("claim-code");
    std::fs::write(&claim_path, "CLAIM").unwrap();
    Auth::new(Secret::new(TOKEN.into()), vec![addr.to_string()], false).with_claim(Claim {
        code: Secret::new("CLAIM".into()),
        path: claim_path,
    })
}

/// A master with the remote fake node "n1", and alex, its claimed admin.
async fn start(store: Store, dir: &std::path::Path) -> T {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let master = Master::new(store, auth(dir, &addr.to_string()));
    master.set_memory(Arc::new(
        harness_memory::SqliteMemory::open_in_memory().unwrap(),
    ));
    let (l, node) = fake("n1", vec![]);
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
        node,
    }
}

async fn req(
    base: &str,
    method: Method,
    path: &str,
    body: Option<Value>,
    c: &str,
) -> (StatusCode, Value) {
    let mut rb = reqwest::Client::new().request(method, format!("{base}{path}"));
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    rb = match c {
        TOKEN => rb.bearer_auth(TOKEN),
        c => rb.login(c).same_origin(),
    };
    let r = rb.send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

/// An account with its own password, logged in (its cookie).
async fn account(t: &T, name: &str, role: &str) -> (String, String) {
    let (st, v) = req(
        &t.base,
        Method::POST,
        "/api/users",
        Some(json!({"username": name, "role": role, "can_agentic": true})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let id = v["user"]["user_id"]
        .as_str()
        .or(v["user_id"].as_str())
        .unwrap()
        .to_string();
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
    (common::signed_in(r).await, id)
}

async fn chat(t: &T, who: &str, workspace: Option<&str>) -> SessionId {
    let mut body = json!({"cwd": "/w", "title": "t", "node_id": "n1"});
    if let Some(w) = workspace {
        body["workspace_id"] = json!(w);
    }
    let (st, v) = req(&t.base, Method::POST, "/api/sessions", Some(body), who).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["session_id"].as_str().unwrap().parse().unwrap()
}

fn started(bg: &str) -> EventKind {
    EventKind::BackgroundStarted(BackgroundStarted {
        bg_id: bg.into(),
        call_id: format!("call-{bg}"),
        turn_id: "t1".into(),
        timeout_s: 600,
        ends_by_ms: harness_core::now_ms() + 600_000,
    })
}

fn ended(bg: &str, reason: BackgroundReason, delivered: bool) -> EventKind {
    EventKind::BackgroundEnded(BackgroundEnded {
        bg_id: bg.into(),
        reason,
        exit_code: Some(1),
        signal: None,
        confirmed: BackgroundConfirmed::Ended,
        ran_ms: 1000,
        output_bytes: 0,
        tail: String::new(),
        tail_cut: false,
        delivered,
    })
}

async fn mirrored(t: &T, sid: SessionId, n: usize) {
    for _ in 0..500 {
        let (_, v) = req(
            &t.base,
            Method::GET,
            &format!("/api/sessions/{sid}/events"),
            None,
            TOKEN,
        )
        .await;
        if v["events"].as_array().is_some_and(|e| e.len() >= n) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the mirror of {sid} never held {n} events");
}

/// `who`'s attention row of `sid`, once there is one.
async fn attention_of(base: &str, who: &str, sid: SessionId) -> Value {
    for _ in 0..500 {
        let (_, v) = req(base, Method::GET, "/api/attention", None, who).await;
        if let Some(i) = v["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["session_id"] == sid.to_string() && i["reason"] != "question")
        {
            return i.clone();
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no attention row for {sid}");
}

async fn rows_of(base: &str, who: &str, sid: SessionId) -> Vec<Value> {
    let (_, v) = req(base, Method::GET, "/api/attention", None, who).await;
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["session_id"] == sid.to_string())
        .cloned()
        .collect()
}

async fn until_callbacks(node: &Fake, n: usize) -> Vec<(SessionId, Vec<String>)> {
    for _ in 0..500 {
        let c = node.callbacks();
        if c.len() >= n {
            return c;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{n} callbacks never came: {:?}", node.callbacks());
}

fn listed_background(v: &Value, sid: SessionId) -> Value {
    v["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["session_id"] == sid.to_string())
        .unwrap()["background"]
        .clone()
}

/// §7 test 5 at the master: ends that come together are one request with
/// every id; stops, restarts and ends a step already delivered are none.
/// The owner, and nobody else, has an attention row until they look; the
/// chat list counts the commands running.
#[tokio::test]
async fn ends_together_are_one_callback_and_only_the_owner_sees_it() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let t = start(Store::open_in_memory().unwrap(), dir.path()).await;
    let (bob, _) = account(&t, "bob", "member").await;
    let sid = chat(&t, &t.alex, None).await;
    for bg in ["bg-1", "bg-2", "bg-3", "bg-4", "bg-5"] {
        t.node.append(sid, started(bg));
    }
    mirrored(&t, sid, 6).await;
    let (_, list) = req(&t.base, Method::GET, "/api/sessions", None, &t.alex).await;
    assert_eq!(listed_background(&list, sid), 5);
    // None of these resumes anything.
    t.node
        .append(sid, ended("bg-3", BackgroundReason::StoppedByUser, false));
    t.node
        .append(sid, ended("bg-4", BackgroundReason::NodeRestarted, false));
    t.node
        .append(sid, ended("bg-5", BackgroundReason::Exited, true));
    // Two that do, back to back.
    t.node
        .append(sid, ended("bg-1", BackgroundReason::Exited, false));
    t.node
        .append(sid, ended("bg-2", BackgroundReason::TimedOut, false));
    let cbs = until_callbacks(&t.node, 1).await;
    assert_eq!(
        cbs,
        vec![(sid, vec!["bg-1".to_string(), "bg-2".to_string()])]
    );
    let row = attention_of(&t.base, &t.alex, sid).await;
    assert_eq!(row["reason"], "resumed", "{row}");
    assert_eq!(
        row["detail"],
        "bg-1, bg-2 ended: the agent was resumed without you"
    );
    assert!(
        rows_of(&t.base, &bob, sid).await.is_empty(),
        "another member sees it"
    );
    let (_, list) = req(&t.base, Method::GET, "/api/sessions", None, &t.alex).await;
    assert_eq!(listed_background(&list, sid), 0);
    // Seen: gone.
    let (st, _) = req(
        &t.base,
        Method::POST,
        &format!("/api/sessions/{sid}/seen"),
        Some(json!({"seq": row["seq"]})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(rows_of(&t.base, &t.alex, sid).await.is_empty());
    // A later end is a later request, for it alone.
    t.node.append(sid, started("bg-6"));
    t.node
        .append(sid, ended("bg-6", BackgroundReason::Exited, false));
    let cbs = until_callbacks(&t.node, 2).await;
    assert_eq!(cbs[1], (sid, vec!["bg-6".to_string()]));
}

/// The 60-minute rule, on the master's clock (S3-5): an end this master
/// first saw more than 60 minutes ago starts nothing and says so. Here the
/// master that mirrored it could not ask (its node did not offer
/// callbacks), and the master that starts next, on the same store, ages it
/// by its `ts_ms`, the only time it has.
#[tokio::test]
async fn an_end_older_than_60_minutes_starts_nothing() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let store = Store::open(
        &dir.path().join("mirror.db"),
        harness_store::Durability::Full,
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let m1 = Master::new(store, auth(dir.path(), &addr.to_string()));
    let (l, n1) = fake_with("n1", vec![], false);
    m1.add_link(l, false).await.unwrap();
    let m = m1.clone();
    tokio::spawn(async move { axum::serve(listener, router(m)).await.unwrap() });
    let base = format!("http://{addr}");
    let r = reqwest::Client::new()
        .post(format!("{base}/api/setup"))
        .json(&json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    let alex = common::signed_in(r).await;
    let (st, v) = req(
        &base,
        Method::POST,
        "/api/sessions",
        Some(json!({"cwd": "/w", "title": "t", "node_id": "n1"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid: SessionId = v["session_id"].as_str().unwrap().parse().unwrap();
    let old = harness_core::now_ms() - 61 * MIN;
    n1.append_at(sid, started("bg-1"), old);
    n1.append_at(sid, ended("bg-1", BackgroundReason::Exited, false), old);
    for _ in 0..500 {
        if m1.inner_mirror().head_seq(sid).unwrap() >= 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    drop((m1, n1));
    let store = Store::open(
        &dir.path().join("mirror.db"),
        harness_store::Durability::Full,
    )
    .unwrap();
    let m2 = Master::new(store, auth(dir.path(), "127.0.0.1:1"));
    let (l, n2) = fake(
        "n1",
        vec![harness_protocol::SessionHead {
            session_id: sid,
            head_seq: 3,
        }],
    );
    m2.add_link(l, false).await.unwrap();
    m2.start_callbacks();
    let key = format!("callback-attention:{sid}");
    let mut mark = None;
    for _ in 0..500 {
        mark = m2.inner_mirror().kv_get(&key).unwrap();
        if mark.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let mark: Value = serde_json::from_str(&mark.expect("no refusal recorded")).unwrap();
    assert_eq!(mark["kind"], "not_resumed", "{mark}");
    assert!(
        mark["reason"]
            .as_str()
            .unwrap()
            .contains("more than 60 minutes"),
        "{mark}"
    );
    assert!(n2.callbacks().is_empty());
}

/// A node's refusals: one that finds nothing to resume is not the owner's
/// business; one on its rules recorded it in the chat, where the owner
/// sees it.
#[tokio::test]
async fn the_nodes_refusals() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let t = start(Store::open_in_memory().unwrap(), dir.path()).await;
    // Nothing to resume: quiet.
    let other = chat(&t, &t.alex, None).await;
    *t.node.refuse.lock().unwrap() = Some(format!(
        "{}: a turn is already running",
        harness_protocol::CALLBACK_NOTHING
    ));
    t.node.append(other, started("bg-1"));
    t.node
        .append(other, ended("bg-1", BackgroundReason::Exited, false));
    until_callbacks(&t.node, 1).await;
    // Its refusal on its rules: the chat's own event is the row.
    *t.node.refuse.lock().unwrap() = Some(format!(
        "{}: 3 turns in a row without you (node limit)",
        harness_protocol::CALLBACK_REFUSED
    ));
    t.node.append(other, started("bg-2"));
    t.node
        .append(other, ended("bg-2", BackgroundReason::Exited, false));
    until_callbacks(&t.node, 2).await;
    t.node.append(
        other,
        EventKind::CallbackRefused(harness_core::event::CallbackRefused {
            bg_ids: vec!["bg-2".into()],
            reason: "3 turns in a row without you (node limit)".into(),
        }),
    );
    let row = attention_of(&t.base, &t.alex, other).await;
    assert_eq!(
        row["detail"],
        "bg-2 ended; the agent was not resumed: 3 turns in a row without you (node limit)"
    );
    assert_eq!(rows_of(&t.base, &t.alex, other).await.len(), 1);
}

/// Admission is exactly what the owner could do themselves now (the
/// review's decision on item 9): an owner removed from a team workspace
/// keeps the chat in their Personal workspace and may go on in it, so it is
/// resumed; a disabled account, a password to change, a lost agentic grant,
/// a chat without an owner and a sub-agent's chat get none, each with an
/// attention row and nothing sent to the node.
#[tokio::test]
async fn admission_is_what_the_owner_could_do_now() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let t = start(Store::open_in_memory().unwrap(), dir.path()).await;
    let (bob, bob_id) = account(&t, "bob", "member").await;
    let (st, v) = req(
        &t.base,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "team"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let team = v["workspace_id"].as_str().unwrap().to_string();
    let (st, v) = req(
        &t.base,
        Method::POST,
        &format!("/api/workspaces/{team}/members"),
        Some(json!({"username": "bob", "role": "member"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = chat(&t, &bob, Some(&team)).await;
    t.node.append(sid, started("bg-1"));
    mirrored(&t, sid, 2).await;
    let (st, v) = req(
        &t.base,
        Method::DELETE,
        &format!("/api/workspaces/{team}/members/{bob_id}"),
        None,
        &t.alex,
    )
    .await;
    assert!(st.is_success(), "{st} {v}");
    t.node
        .append(sid, ended("bg-1", BackgroundReason::Exited, false));
    let cbs = until_callbacks(&t.node, 1).await;
    assert_eq!(
        cbs,
        vec![(sid, vec!["bg-1".to_string()])],
        "the owner could go on in Personal"
    );

    // Each refusal: an account that cannot act, or a chat that is not one.
    let refused = |sid: SessionId, who: String, want: &'static str| {
        let base = t.base.clone();
        async move {
            let row = attention_of(&base, &who, sid).await;
            assert_eq!(row["reason"], "not_resumed", "{row}");
            assert!(
                row["detail"].as_str().unwrap().contains(want),
                "{want}: {row}"
            );
        }
    };
    let end_in = |sid: SessionId| {
        t.node.append(sid, started("bg-1"));
        t.node
            .append(sid, ended("bg-1", BackgroundReason::Exited, false));
    };
    // Disabled.
    let (carol, carol_id) = account(&t, "carol", "member").await;
    let c = chat(&t, &carol, None).await;
    let (st, _) = req(
        &t.base,
        Method::PATCH,
        &format!("/api/users/{carol_id}"),
        Some(json!({"disabled": true})),
        &t.alex,
    )
    .await;
    assert!(st.is_success());
    end_in(c);
    refused(c, TOKEN.into(), "disabled").await;
    // A password to change (an admin reset it).
    let (dana, dana_id) = account(&t, "dana", "member").await;
    let d = chat(&t, &dana, None).await;
    let (st, _) = req(
        &t.base,
        Method::POST,
        &format!("/api/users/{dana_id}/password"),
        None,
        &t.alex,
    )
    .await;
    assert!(st.is_success());
    end_in(d);
    refused(d, TOKEN.into(), "password").await;
    // A lost agentic grant.
    let (erin, erin_id) = account(&t, "erin", "member").await;
    let e = chat(&t, &erin, None).await;
    let (st, _) = req(
        &t.base,
        Method::PATCH,
        &format!("/api/users/{erin_id}"),
        Some(json!({"can_agentic": false})),
        &t.alex,
    )
    .await;
    assert!(st.is_success());
    end_in(e);
    let row = attention_of(&t.base, TOKEN, e).await;
    assert_eq!(row["reason"], "not_resumed", "{row}");
    // No owner: a chat the API token made.
    let o = chat(&t, TOKEN, None).await;
    end_in(o);
    refused(o, TOKEN.into(), "no owner account").await;
    // A sub-agent's chat (the node names alex's chat as its parent).
    let parent = chat(&t, &t.alex, None).await;
    mirrored(&t, parent, 1).await;
    let kid = SessionId::new();
    t.node.raw(
        kid,
        "session_created",
        json!({"title": "@scout", "cwd": "/w", "model": "m", "backend": "native", "node_id": "n1",
               "profile": "agentic", "approval_mode": "auto", "parent_session": parent}),
    );
    mirrored(&t, kid, 1).await;
    end_in(kid);
    refused(kid, t.alex.clone(), "sub-agent").await;
    assert_eq!(t.node.callbacks().len(), 1, "{:?}", t.node.callbacks());
    let _ = (bob, carol, dana, erin);
}

/// The master restarts mid-way: its position is the log. Ends a master
/// saw but could not have resumed yet (the node had a turn running) are
/// asked for once by the master that starts next on the same store; a
/// third start asks nothing for what those turns covered, and still asks
/// for an end that waits.
#[tokio::test]
async fn a_restarted_master_asks_once_and_loses_nothing() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let store = Store::open(
        &dir.path().join("mirror.db"),
        harness_store::Durability::Full,
    )
    .unwrap();
    let t = start(store, dir.path()).await;
    let busy = || {
        Some(format!(
            "{}: a turn is already running",
            harness_protocol::CALLBACK_NOTHING
        ))
    };
    let (a, b) = (chat(&t, &t.alex, None).await, chat(&t, &t.alex, None).await);
    *t.node.refuse.lock().unwrap() = busy();
    for sid in [a, b] {
        t.node.append(sid, started("bg-1"));
        t.node
            .append(sid, ended("bg-1", BackgroundReason::Exited, false));
    }
    until_callbacks(&t.node, 2).await;
    mirrored(&t, a, 3).await;
    mirrored(&t, b, 3).await;
    drop(t);

    // Started again on the same store; the node's hello names the heads
    // the mirror has, so nothing is replayed to the watcher.
    let again = |dir: std::path::PathBuf| async move {
        let store = Store::open(&dir.join("mirror.db"), harness_store::Durability::Full).unwrap();
        let heads = [a, b]
            .into_iter()
            .map(|sid| harness_protocol::SessionHead {
                session_id: sid,
                head_seq: store.head_seq(sid).unwrap(),
            })
            .collect();
        let master = Master::new(store, auth(&dir, "127.0.0.1:1"));
        let (l, node) = fake("n1", heads);
        master.add_link(l, false).await.unwrap();
        master.start_callbacks();
        (master, node)
    };
    let (m2, n2) = again(dir.path().to_path_buf()).await;
    let mut cbs = until_callbacks(&n2, 2).await;
    cbs.sort();
    let mut want = vec![(a, vec!["bg-1".to_string()]), (b, vec!["bg-1".to_string()])];
    want.sort();
    assert_eq!(cbs, want, "each waiting end once");
    // Both turns are in the log (the fake ran them); then b gets a new end
    // its node cannot take yet.
    for sid in [a, b] {
        for _ in 0..500 {
            if m2
                .inner_mirror()
                .events_after(sid, 0, 100)
                .unwrap()
                .iter()
                .any(|e| e.kind == "turn_ended")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    *n2.refuse.lock().unwrap() = busy();
    n2.append(b, started("bg-2"));
    n2.append(b, ended("bg-2", BackgroundReason::Exited, false));
    until_callbacks(&n2, 3).await;
    for _ in 0..500 {
        if m2.inner_mirror().head_seq(b).unwrap() >= 7 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    drop((m2, n2));

    // A third start: b's new end is asked for; a, whose end its turn
    // covered, is not asked again. (b's request is the sign the start's
    // checks ran: each chat's check reads the log first, and a's has
    // nothing to send.)
    let (_m3, n3) = again(dir.path().to_path_buf()).await;
    let cbs = until_callbacks(&n3, 1).await;
    assert_eq!(cbs[0], (b, vec!["bg-2".to_string()]));
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(n3.callbacks().len(), 1, "asked again: {:?}", n3.callbacks());
}

/// A `remember` call of `turn` in `sid`, then a barrier event: the mirror
/// takes events in order and files a proposal while it takes the call, so
/// once the barrier is mirrored the call has been decided.
async fn remember_in(t: &T, node: &Fake, sid: SessionId, turn: &str, text: &str, n: usize) {
    node.raw(
        sid,
        "tool_call_started",
        json!({"turn_id": turn, "call_id": format!("call-{text}"), "tool": "remember",
               "input": {"text": text}}),
    );
    node.raw(sid, "error", json!({"message": "barrier"}));
    mirrored(t, sid, n).await;
}

/// Alex's agentic chat with the General agent, whose memory is on (the
/// Software Developer's is off, which would file nothing either way).
async fn memory_chat(t: &T) -> SessionId {
    let body = json!({"cwd": "/w", "title": "t", "node_id": "n1", "agent_id": "general"});
    let (st, v) = req(&t.base, Method::POST, "/api/sessions", Some(body), &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["session_id"].as_str().unwrap().parse().unwrap()
}

/// The memory facts (proposals included) of `sid`'s owner.
async fn facts(m: &Master, sid: SessionId) -> Vec<String> {
    let owner = m.inner_mirror().session(sid).unwrap().unwrap().owner_id;
    m.memory()
        .unwrap()
        .list(owner.as_deref())
        .await
        .unwrap()
        .into_iter()
        .map(|f| f.text)
        .collect()
}

/// Review S3-1: a callback turn's `remember` is never filed: not after the
/// owner's message was refused while it ran ("a turn is already running"),
/// not after a master restart (no recorded memory); and a callback the node
/// answers "nothing to resume" leaves the person's running turn its own.
#[tokio::test]
async fn a_callback_turns_remember_is_never_filed_and_the_persons_turn_keeps_its_own() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let store = Store::open(
        &dir.path().join("mirror.db"),
        harness_store::Durability::Full,
    )
    .unwrap();
    let t = start(store, dir.path()).await;
    *t.node.hold.lock().unwrap() = true;
    let sid = memory_chat(&t).await;
    t.node.append(sid, started("bg-1"));
    t.node
        .append(sid, ended("bg-1", BackgroundReason::Exited, false));
    until_callbacks(&t.node, 1).await;
    mirrored(&t, sid, 4).await;
    // The owner writes while the callback turn runs: refused by the node.
    *t.node.refuse_prompts.lock().unwrap() = Some("a turn is already running".into());
    let (st, v) = req(
        &t.base,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({"text": "remember nothing"})),
        &t.alex,
    )
    .await;
    assert!(!st.is_success(), "{st} {v}");
    remember_in(&t, &t.node, sid, "cb", "steered-after-refusal", 6).await;
    assert!(
        !facts(&t.master, sid)
            .await
            .iter()
            .any(|f| f.contains("steered")),
        "filed after the refused message"
    );

    // The reverse: the person's turn runs, a callback is answered "nothing
    // to resume"; the person's turn still files its proposal.
    let p = memory_chat(&t).await;
    *t.node.refuse_prompts.lock().unwrap() = None;
    let (st, v) = req(
        &t.base,
        Method::POST,
        &format!("/api/sessions/{p}/prompt"),
        Some(json!({"text": "keep notes"})),
        &t.alex,
    )
    .await;
    assert!(st.is_success(), "{st} {v}");
    mirrored(&t, p, 3).await;
    *t.node.refuse.lock().unwrap() = Some(format!(
        "{}: a turn is already running",
        harness_protocol::CALLBACK_NOTHING
    ));
    t.node.append(p, started("bg-1"));
    t.node
        .append(p, ended("bg-1", BackgroundReason::Exited, false));
    until_callbacks(&t.node, 2).await;
    remember_in(&t, &t.node, p, "p", "persons-own-fact", 7).await;
    assert!(
        facts(&t.master, p)
            .await
            .iter()
            .any(|f| f == "persons-own-fact"),
        "the person's turn lost its proposal: {:?}",
        facts(&t.master, p).await
    );

    // A restart while the callback turn runs: no recorded memory, and still
    // nothing is filed (the log says whose turn it is).
    let head = t.master.inner_mirror().head_seq(sid).unwrap();
    drop(t);
    let store = Store::open(
        &dir.path().join("mirror.db"),
        harness_store::Durability::Full,
    )
    .unwrap();
    let m2 = Master::new(store, auth(dir.path(), "127.0.0.1:1"));
    m2.set_memory(Arc::new(
        harness_memory::SqliteMemory::open_in_memory().unwrap(),
    ));
    let (l, n2) = fake(
        "n1",
        vec![harness_protocol::SessionHead {
            session_id: sid,
            head_seq: head,
        }],
    );
    m2.add_link(l, false).await.unwrap();
    n2.raw(
        sid,
        "tool_call_started",
        json!({"turn_id": "cb", "call_id": "call-r", "tool": "remember",
               "input": {"text": "steered-after-restart"}}),
    );
    n2.raw(sid, "error", json!({"message": "barrier"}));
    for _ in 0..500 {
        if m2.inner_mirror().head_seq(sid).unwrap() >= head + 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(m2.inner_mirror().head_seq(sid).unwrap() >= head + 2);
    assert!(
        facts(&m2, sid).await.is_empty(),
        "filed after a restart: {:?}",
        facts(&m2, sid).await
    );
}

/// Review S3-5: the 60-minute rule ages an end by the master's clock: an
/// end stamped two hours behind by its node's clock is resumed, and one
/// stamped two hours ahead too.
#[tokio::test]
async fn the_age_of_an_end_is_the_masters_clock_not_the_nodes() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let t = start(Store::open_in_memory().unwrap(), dir.path()).await;
    let now = harness_core::now_ms();
    let (behind, ahead) = (chat(&t, &t.alex, None).await, chat(&t, &t.alex, None).await);
    t.node.append_at(behind, started("bg-1"), now - 120 * MIN);
    t.node.append_at(
        behind,
        ended("bg-1", BackgroundReason::Exited, false),
        now - 120 * MIN,
    );
    t.node.append_at(ahead, started("bg-1"), now + 120 * MIN);
    t.node.append_at(
        ahead,
        ended("bg-1", BackgroundReason::Exited, false),
        now + 120 * MIN,
    );
    let mut cbs = until_callbacks(&t.node, 2).await;
    cbs.sort();
    let mut want = vec![
        (behind, vec!["bg-1".to_string()]),
        (ahead, vec!["bg-1".to_string()]),
    ];
    want.sort();
    assert_eq!(cbs, want);
}

/// A node that is stopping, then one that went away, is asked again once it
/// is back (every 15 s); a node without `CAP_CALLBACK` is never asked.
#[tokio::test]
async fn a_busy_or_away_node_is_asked_again_and_one_without_the_capability_never() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let t = start(Store::open_in_memory().unwrap(), dir.path()).await;
    let sid = chat(&t, &t.alex, None).await;
    *t.node.refuse.lock().unwrap() = Some(harness_protocol::NODE_STOPPING.into());
    t.node.append(sid, started("bg-1"));
    t.node
        .append(sid, ended("bg-1", BackgroundReason::Exited, false));
    until_callbacks(&t.node, 1).await;
    *t.node.refuse.lock().unwrap() = None;
    let cbs = until_callbacks_within(&t.node, 2, 40).await;
    assert_eq!(
        cbs[1],
        (sid, vec!["bg-1".to_string()]),
        "asked again once it was back"
    );
    for _ in 0..500 {
        if rows_of(&t.base, &t.alex, sid)
            .await
            .iter()
            .any(|r| r["reason"] == "resumed")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // Away: the end arrives and the link drops before the master asks.
    let head = t.master.inner_mirror().head_seq(sid).unwrap();
    t.node.append(sid, started("bg-2"));
    t.node
        .append_then_leave(sid, ended("bg-2", BackgroundReason::Exited, false));
    for _ in 0..500 {
        if t.master.inner_mirror().head_seq(sid).unwrap() >= head + 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let (l, back) = fake(
        "n1",
        vec![harness_protocol::SessionHead {
            session_id: sid,
            head_seq: head + 2,
        }],
    );
    t.master.add_link(l, false).await.unwrap();
    let cbs = until_callbacks_within(&back, 1, 40).await;
    assert_eq!(cbs[0], (sid, vec!["bg-2".to_string()]));

    // A node that does not announce callbacks.
    let (l, old) = fake_with("n2", vec![], false);
    t.master.add_link(l, false).await.unwrap();
    let (st, v) = req(
        &t.base,
        Method::POST,
        "/api/sessions",
        Some(json!({"cwd": "/w", "title": "t", "node_id": "n2"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let o: SessionId = v["session_id"].as_str().unwrap().parse().unwrap();
    old.append(o, started("bg-1"));
    old.append(o, ended("bg-1", BackgroundReason::Exited, false));
    // The check runs within its settle; afterwards nothing was sent and the
    // owner has no row (the end waits for them, as before step 3).
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(old.callbacks().is_empty());
    assert!(rows_of(&t.base, &t.alex, o).await.is_empty());
}

async fn until_callbacks_within(node: &Fake, n: usize, secs: u64) -> Vec<(SessionId, Vec<String>)> {
    let by = std::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let c = node.callbacks();
        if c.len() >= n {
            return c;
        }
        assert!(
            std::time::Instant::now() < by,
            "{n} callbacks never came: {c:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// `PromptOpts.callback` carries no person's input: with text or context
/// it is refused before anything is sent.
#[tokio::test]
async fn a_callback_with_text_or_context_is_refused() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let t = start(Store::open_in_memory().unwrap(), dir.path()).await;
    let sid = chat(&t, &t.alex, None).await;
    for (text, context) in [("do it", None), ("", Some("<page>".to_string()))] {
        let e = t
            .master
            .prompt(
                sid,
                text.into(),
                harness_master::PromptOpts {
                    callback: vec!["bg-1".into()],
                    context,
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert!(e.to_string().contains("carries no message"), "{e}");
    }
    assert!(t.node.callbacks().is_empty());
}
