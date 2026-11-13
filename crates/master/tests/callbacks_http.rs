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
}

#[derive(Clone)]
struct Fake {
    seen: Arc<Mutex<Vec<MasterToNode>>>,
    tx: mpsc::UnboundedSender<Do>,
    /// The answer to the next callbacks (`None`: start the turn).
    refuse: Arc<Mutex<Option<String>>>,
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
}

/// An agentic node `id` announcing `background` and `callback`.
fn fake(id: &str, sessions: Vec<harness_protocol::SessionHead>) -> (NodeLink, Fake) {
    let (to_node, mut commands) = mpsc::channel::<MasterToNode>(1024);
    let (tx, from_node) = mpsc::channel::<NodeToMaster>(1024);
    tx.try_send(NodeToMaster::Hello {
        protocol_version: PROTOCOL_VERSION,
        node_id: id.into(),
        hostname: "h".into(),
        version: "1".into(),
        capabilities: vec![
            "native".into(),
            "profile:agentic".into(),
            harness_protocol::CAP_BACKGROUND.into(),
            harness_protocol::CAP_CALLBACK.into(),
        ],
        sessions: sessions.clone(),
    })
    .unwrap();
    let seen: Arc<Mutex<Vec<MasterToNode>>> = Arc::default();
    let refuse: Arc<Mutex<Option<String>>> = Arc::default();
    let (do_tx, mut do_rx) = mpsc::unbounded_channel::<Do>();
    let (node, keep, no) = (id.to_string(), seen.clone(), refuse.clone());
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
                    if let MasterToNode::Create { session_id, .. } = &msg {
                        let created = EventKind::SessionCreated(SessionCreated {
                            title: "t".into(),
                            cwd: "/w".into(),
                            model: "demo::mock".into(),
                            backend: "native".into(),
                            node_id: node.clone(),
                            approval_mode: ApprovalMode::Auto,
                            profile: Profile::Agentic,
                            agent_id: None,
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
                            send(&mut seqs, *session_id, EventKind::TurnEnded(TurnEnded {
                                turn_id: "cb".into(),
                                reason: TurnEndReason::Completed,
                                usage: None,
                            }), now);
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
        },
    )
}

struct T {
    base: String,
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
    T { base, alex, node }
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

/// The 60-minute rule, and a node's refusals: an end recorded more than 60
/// minutes before the master sees it starts nothing and says so; a node
/// that finds nothing to resume is not the owner's business; a node that
/// refuses on its rules recorded it in the chat, where the owner sees it.
#[tokio::test]
async fn an_old_end_and_the_nodes_refusals() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let t = start(Store::open_in_memory().unwrap(), dir.path()).await;
    let sid = chat(&t, &t.alex, None).await;
    t.node.append(sid, started("bg-1"));
    t.node.append_at(
        sid,
        ended("bg-1", BackgroundReason::Exited, false),
        harness_core::now_ms() - 61 * MIN,
    );
    let row = attention_of(&t.base, &t.alex, sid).await;
    assert_eq!(row["reason"], "not_resumed", "{row}");
    assert!(
        row["detail"]
            .as_str()
            .unwrap()
            .contains("more than 60 minutes"),
        "{row}"
    );
    assert!(t.node.callbacks().is_empty());

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

/// §7 test 6 at the master: an owner who left the chat's workspace, and a
/// disabled account, get no callback turn; each refusal is an attention
/// row; nothing is sent to the node.
#[tokio::test]
async fn a_removed_owner_and_a_disabled_account_get_none() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let t = start(Store::open_in_memory().unwrap(), dir.path()).await;
    let (bob, bob_id) = account(&t, "bob", "member").await;
    let (carol, carol_id) = account(&t, "carol", "member").await;
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
    // Let the watcher note where it started.
    tokio::task::yield_now().await;
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
    let row = attention_of(&t.base, &bob, sid).await;
    assert_eq!(row["reason"], "not_resumed", "{row}");
    assert!(
        row["detail"]
            .as_str()
            .unwrap()
            .contains("no longer in the workspace"),
        "{row}"
    );
    // A disabled account.
    let sid2 = chat(&t, &carol, None).await;
    t.node.append(sid2, started("bg-1"));
    mirrored(&t, sid2, 2).await;
    let (st, v) = req(
        &t.base,
        Method::PATCH,
        &format!("/api/users/{carol_id}"),
        Some(json!({"disabled": true})),
        &t.alex,
    )
    .await;
    assert!(st.is_success(), "{st} {v}");
    t.node
        .append(sid2, ended("bg-1", BackgroundReason::Exited, false));
    let row = attention_of(&t.base, TOKEN, sid2).await;
    assert!(
        row["detail"].as_str().unwrap().contains("disabled"),
        "{row}"
    );
    assert!(t.node.callbacks().is_empty(), "{:?}", t.node.callbacks());
    let _ = carol;
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
