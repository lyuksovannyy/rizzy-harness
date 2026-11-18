//! A turn's changes over HTTP (X.12, PLAN.md T53): owner only, every other
//! account gets 404 (admins and workspace admins included, T10); the
//! master asks the owning node once and passes its answer on; a node that
//! is away, too old or has nothing to show is a sentence, never a 500.
//!
//! The node here is a fake that opens chats and answers `session.diff`
//! with a fixed diff, or with the real node's words for a pruned turn; the
//! real node's side is `crates/node/tests/turn_diff.rs`.

mod common;
use common::{Login as _, SameOrigin as _};

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::event::{EventKind, Profile, SessionCreated};
use harness_core::policy::ApprovalMode;
use harness_core::{RawEvent, Secret, SessionId};
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, router};
use harness_node::NodeLink;
use harness_protocol::{
    CAP_DIFF, DiffContent, DiffFile, DiffHunk, DiffStatus, MasterToNode, NodeToMaster,
    PROTOCOL_VERSION, TurnDiff,
};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

const TOKEN: &str = "api-secret";
const PRUNED: &str = "this checkpoint is no longer kept (only the latest 10 changes are), so its \
                      changes cannot be shown";

fn sample() -> TurnDiff {
    TurnDiff {
        files: vec![DiffFile {
            path: "a.txt".into(),
            old_path: None,
            status: DiffStatus::Modified,
            content: DiffContent::Text,
            old_mode: Some("100644".into()),
            new_mode: Some("100644".into()),
            old_size: Some(3),
            new_size: Some(6),
            added: 1,
            removed: 0,
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 2,
                section: String::new(),
                lines: vec![" v1".into(), "+v2".into()],
            }],
            cut: false,
        }],
        ..Default::default()
    }
}

struct Fake {
    seen: Arc<Mutex<Vec<MasterToNode>>>,
    leave: mpsc::UnboundedSender<()>,
    /// Send this as the node (a forged or late answer).
    say: mpsc::UnboundedSender<NodeToMaster>,
}

impl Fake {
    /// The request id of the latest `session.diff` this node was sent.
    fn last_req(&self) -> Option<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|m| match m {
                MasterToNode::Diff { req_id, .. } => Some(req_id.clone()),
                _ => None,
            })
    }

    fn diffs(&self) -> Vec<(SessionId, String, bool)> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                MasterToNode::Diff {
                    session_id,
                    turn_id,
                    rewind,
                    ..
                } => Some((*session_id, turn_id.clone(), *rewind)),
                _ => None,
            })
            .collect()
    }
}

/// An agentic node `id` announcing `caps`: it opens chats, acks every
/// command, and answers a diff of turn `pruned` with the node's words.
fn fake(id: &str, caps: &[&str]) -> (NodeLink, Fake) {
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
    let seen: Arc<Mutex<Vec<MasterToNode>>> = Arc::default();
    let (leave, mut left) = mpsc::unbounded_channel::<()>();
    let (say, mut said) = mpsc::unbounded_channel::<NodeToMaster>();
    let (node, keep) = (id.to_string(), seen.clone());
    tokio::spawn(async move {
        loop {
            tokio::select! {
                msg = commands.recv() => {
                    let Some(msg) = msg else { return };
                    keep.lock().unwrap().push(msg.clone());
                    match &msg {
                        MasterToNode::Create { session_id, exec_mode, .. } => {
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
                                    seq: 1,
                                    ts_ms: harness_core::now_ms(),
                                    kind,
                                    payload,
                                },
                            });
                        }
                        // Held: the test answers it (or not) itself.
                        MasterToNode::Diff { turn_id, .. } if turn_id == "hold" => {}
                        MasterToNode::Diff { req_id, turn_id, .. } => {
                            let pruned = turn_id == "pruned";
                            let _ = tx.try_send(NodeToMaster::DiffResult {
                                req_id: req_id.clone(),
                                diff: (!pruned).then(|| Box::new(sample())),
                                error: pruned.then(|| PRUNED.into()),
                            });
                        }
                        _ => {}
                    }
                    if let Some(req_id) = msg.req_id() {
                        let _ = tx.try_send(NodeToMaster::Ack { req_id: req_id.into(), ok: true, error: None });
                    }
                }
                m = said.recv() => if let Some(m) = m { let _ = tx.try_send(m); },
                _ = left.recv() => return,
            }
        }
    });
    (NodeLink { to_node, from_node }, Fake { seen, leave, say })
}

struct T {
    base: String,
    master: Master,
    alex: String,
    local: Fake,
    old: Fake,
    _dir: tempfile::TempDir,
}

/// A master with the fake node `local` (announcing `diff`) and `old` (a
/// node from before X.12), and alex, its claimed admin.
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
    let master = Master::new(Store::open_in_memory().unwrap(), auth);
    let (l, local) = fake("local", &[CAP_DIFF]);
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
        master,
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
    rb = match c {
        TOKEN => rb.bearer_auth(TOKEN),
        c => rb.login(c).same_origin(),
    };
    let r = rb.send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

/// An account with its own password, logged in (its cookie).
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

fn diff(sid: SessionId, turn: &str) -> String {
    format!("/api/sessions/{sid}/turns/{turn}/diff")
}

#[tokio::test]
async fn the_owner_sees_the_nodes_diff_as_it_came() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    let (st, v) = req(&t, Method::GET, &diff(sid, "t1"), None, &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v, serde_json::to_value(sample()).unwrap());
    let (st, _) = req(
        &t,
        Method::GET,
        &format!("{}?rewind=1", diff(sid, "t1")),
        None,
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        t.local.diffs(),
        vec![
            (sid, "t1".to_string(), false),
            (sid, "t1".to_string(), true)
        ]
    );
}

#[tokio::test]
async fn nobody_else_sees_it_not_even_an_admin() {
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
    for who in ["bob", "dana"] {
        let role = if who == "dana" { "admin" } else { "member" };
        let (st, v) = req(
            &t,
            Method::POST,
            &format!("/api/workspaces/{team}/members"),
            Some(json!({"username": who, "role": role})),
            &t.alex,
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }
    let sid = chat(&t, &t.alex, "local", Some(&team)).await;
    for (who, name) in [
        (&bob, "another member"),
        (&carol, "a server admin"),
        (&dana, "a workspace admin"),
    ] {
        let (st, v) = req(&t, Method::GET, &diff(sid, "t1"), None, who).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{name}: {v}");
        assert_eq!(v["error"], "unknown session", "{name}: the same as no chat");
    }
    // A chat that does not exist: the same 404.
    let (st, v) = req(
        &t,
        Method::GET,
        &diff(SessionId::new(), "t1"),
        None,
        &t.alex,
    )
    .await;
    assert_eq!(
        (st, v["error"].as_str()),
        (StatusCode::NOT_FOUND, Some("unknown session"))
    );
    assert!(t.local.diffs().is_empty(), "{:?}", t.local.diffs());
}

#[tokio::test]
async fn nothing_to_show_is_a_sentence_never_a_500() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    // The node's own words.
    let (st, v) = req(&t, Method::GET, &diff(sid, "pruned"), None, &t.alex).await;
    assert_eq!(
        (st, v["error"].as_str()),
        (StatusCode::CONFLICT, Some(PRUNED))
    );
    // A turn id no node makes is not sent.
    let (st, _) = req(&t, Method::GET, &diff(sid, "a%20b"), None, &t.alex).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert_eq!(t.local.diffs().len(), 1);
    // A node from before X.12 is sent nothing.
    let old = chat(&t, &t.alex, "old", None).await;
    let (st, v) = req(&t, Method::GET, &diff(old, "t1"), None, &t.alex).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("older"), "{v}");
    assert!(t.old.diffs().is_empty());
    // A node that is away.
    t.local.leave.send(()).unwrap();
    for _ in 0..400 {
        if !t.master.is_connected("local") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let (st, v) = req(&t, Method::GET, &diff(sid, "t1"), None, &t.alex).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert!(v["error"].as_str().unwrap().contains("away"), "{v}");
}

fn result(req_id: &str, path: &str) -> NodeToMaster {
    let mut d = sample();
    d.files[0].path = path.into();
    NodeToMaster::DiffResult {
        req_id: req_id.into(),
        diff: Some(Box::new(d)),
        error: None,
    }
}

/// Review D10, the link's abuse cases: an answer from another node, for an
/// unknown id, a duplicate and a late one are ignored; a request whose node
/// drops its link ends as "away" at once, not at the timeout.
#[tokio::test]
async fn only_the_asked_node_answers_once() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    let base = t.base.clone();
    let alex = t.alex.clone();
    let get = |turn: &'static str| {
        let (base, alex) = (base.clone(), alex.clone());
        tokio::spawn(async move {
            let r = reqwest::Client::new()
                .get(format!("{base}/api/sessions/{sid}/turns/{turn}/diff"))
                .login(&alex)
                .same_origin()
                .send()
                .await
                .unwrap();
            let st = r.status();
            (st, r.json::<Value>().await.unwrap_or(Value::Null))
        })
    };
    let wait_req = || async {
        for _ in 0..400 {
            if let Some(r) = t.local.last_req() {
                return r;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no session.diff reached the node");
    };
    let pending = get("hold");
    let req = wait_req().await;
    // Another node answering this request, and an answer to no request.
    t.old.say.send(result(&req, "forged-by-old")).unwrap();
    t.local
        .say
        .send(result("no-such-request", "unknown"))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !pending.is_finished(),
        "a foreign or unknown answer ended the request"
    );
    // The node asked answers, then again (duplicate, late): the first wins.
    t.local.say.send(result(&req, "the-real-one")).unwrap();
    t.local.say.send(result(&req, "duplicate")).unwrap();
    let (st, v) = pending.await.unwrap();
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["files"][0]["path"], "the-real-one");
    // The link still works after the late answer.
    let (st, _) = req_ok(&t, sid).await;
    assert_eq!(st, StatusCode::OK);
    // A held request ends as "away" when the link drops.
    let before = t.local.diffs().len();
    let pending = get("hold");
    for _ in 0..400 {
        if t.local.diffs().len() > before {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let at = std::time::Instant::now();
    t.local.leave.send(()).unwrap();
    let (st, v) = pending.await.unwrap();
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert!(v["error"].as_str().unwrap().contains("away"), "{v}");
    assert!(at.elapsed() < Duration::from_secs(10), "{:?}", at.elapsed());
}

async fn req_ok(t: &T, sid: SessionId) -> (StatusCode, Value) {
    req(t, Method::GET, &diff(sid, "t1"), None, &t.alex).await
}

/// Review D9: one account cannot take every waiting place.
#[tokio::test]
async fn one_account_has_a_few_waiting_places() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    let mut held = Vec::new();
    for _ in 0..4 {
        let (base, alex) = (t.base.clone(), t.alex.clone());
        held.push(tokio::spawn(async move {
            reqwest::Client::new()
                .get(format!("{base}/api/sessions/{sid}/turns/hold/diff"))
                .login(&alex)
                .same_origin()
                .send()
                .await
                .unwrap()
                .status()
        }));
    }
    for _ in 0..400 {
        if t.local.diffs().len() >= 4 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(t.local.diffs().len(), 4);
    let (st, v) = req(&t, Method::GET, &diff(sid, "t1"), None, &t.alex).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert_eq!(t.local.diffs().len(), 4, "the fifth was not sent");
    // Another account still gets a place (an admin: only admins may open
    // chats on the server's own node).
    let bob = account(&t, "bob", "admin").await;
    let bob_sid = chat(&t, &bob, "local", None).await;
    let (st, v) = req(&t, Method::GET, &diff(bob_sid, "t1"), None, &bob).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    t.local.leave.send(()).unwrap();
    for h in held {
        assert_eq!(h.await.unwrap(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
