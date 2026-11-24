//! A background command's output over HTTP (X.62, PLAN.md T8D): owner only,
//! every other account gets 404 (admins and workspace admins included, T10)
//! with the node never asked; the master asks the owning node once and
//! passes its answer on, held to the protocol's size; a node that is away,
//! too old or has forgotten the command is a sentence, never a 500; one
//! account cannot hold every waiting place. The node here is a fake; the
//! real node's table, scrubbing and chat scoping are
//! `crates/node/tests/background_output.rs`.

use crate::common;
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
    BackgroundOutput, CAP_BG_OUTPUT, MAX_BG_OUTPUT_BYTES, MasterToNode, NodeToMaster,
    PROTOCOL_VERSION,
};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

const TOKEN: &str = "api-secret";
const FORGOTTEN: &str = "no background command bg-9 in this chat: the node forgets one 30 \
                         minutes after it ended, or when it restarts";

fn out(text: &str) -> BackgroundOutput {
    BackgroundOutput {
        running: true,
        ran_ms: 1500,
        printed: text.len() as u64,
        text: text.into(),
        ..Default::default()
    }
}

struct Fake {
    seen: Arc<Mutex<Vec<MasterToNode>>>,
    leave: mpsc::UnboundedSender<()>,
    say: mpsc::UnboundedSender<NodeToMaster>,
}

impl Fake {
    fn last_req(&self) -> Option<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|m| match m {
                MasterToNode::BackgroundOutput { req_id, .. } => Some(req_id.clone()),
                _ => None,
            })
    }

    /// (chat, command) of every `session.background_output` this node got.
    fn asks(&self) -> Vec<(SessionId, String)> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                MasterToNode::BackgroundOutput {
                    session_id, bg_id, ..
                } => Some((*session_id, bg_id.clone())),
                _ => None,
            })
            .collect()
    }
}

/// Chats a fake node leaves unanswered. Session ids are unique to a test.
static HELD: Mutex<Vec<SessionId>> = Mutex::new(Vec::new());

/// A node `id` announcing `caps`: it opens chats, acks every command, and
/// answers `bg-1` with a line, `bg-2` with far more text than it may send,
/// and any other id with the node's words for a command it has forgotten,
/// except in a chat in `HELD`, which it never answers.
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
                                forked: None,
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
                                sub_folder: None,
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
                        MasterToNode::BackgroundOutput { req_id, bg_id, session_id }
                            if !HELD.lock().unwrap().contains(session_id) =>
                        {
                            let (output, error) = match bg_id.as_str() {
                                "bg-1" => (Some(out("listening on :3000\n")), None),
                                "bg-2" => (Some(out(&"x".repeat(MAX_BG_OUTPUT_BYTES * 3))), None),
                                _ => (None, Some(FORGOTTEN.to_string())),
                            };
                            let _ = tx.try_send(NodeToMaster::BackgroundOutputResult {
                                req_id: req_id.clone(),
                                output,
                                error,
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

/// A master with the fake nodes `local` (announcing the capability), `old`
/// (a node from before it), and alex, its claimed admin.
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
    let (l, local) = fake("local", &[CAP_BG_OUTPUT]);
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
        "" => rb,
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

async fn gone(t: &T, node: &str, f: &Fake) {
    f.leave.send(()).unwrap();
    for _ in 0..400 {
        if !t.master.is_connected(node) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{node} still connected");
}

fn url(sid: SessionId, bg: &str) -> String {
    format!("/api/sessions/{sid}/background/{bg}/output")
}

#[tokio::test]
async fn the_owner_gets_the_nodes_answer_as_it_came() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    let (st, v) = req(&t, Method::GET, &url(sid, "bg-1"), None, &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        v,
        json!({"running": true, "ran_ms": 1500, "printed": 19, "text": "listening on :3000\n"})
    );
    assert_eq!(t.local.asks(), vec![(sid, "bg-1".to_string())]);
}

/// Abuse case 1: another member, a server admin and a workspace admin get
/// the 404 of a chat that does not exist; the node is never asked.
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
    for (who, role) in [("bob", "member"), ("dana", "admin")] {
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
        let (st, v) = req(&t, Method::GET, &url(sid, "bg-1"), None, who).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{name}: {v}");
        assert_eq!(v["error"], "unknown session", "{name}: the same as no chat");
    }
    let (st, v) = req(
        &t,
        Method::GET,
        &url(SessionId::new(), "bg-1"),
        None,
        &t.alex,
    )
    .await;
    assert_eq!(
        (st, v["error"].as_str()),
        (StatusCode::NOT_FOUND, Some("unknown session"))
    );
    let (st, _) = req(&t, Method::GET, &url(sid, "bg-1"), None, "").await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    assert!(t.local.asks().is_empty(), "{:?}", t.local.asks());
}

/// The id is the node's shape only (no path, no query, no number past nine
/// digits) and is refused before the node is asked.
#[tokio::test]
async fn a_bad_id_is_refused_before_the_node_is_asked() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    for bad in [
        "bg-",
        "bg-x",
        "x",
        "bg-1234567890",
        "..",
        "bg-1%2f..",
        "BG-1",
    ] {
        let (st, _) = req(&t, Method::GET, &url(sid, bad), None, &t.alex).await;
        assert!(
            st == StatusCode::BAD_REQUEST || st == StatusCode::NOT_FOUND,
            "{bad}: {st}"
        );
    }
    assert!(t.local.asks().is_empty(), "{:?}", t.local.asks());
}

/// Abuse case 4, the master's part: a node that sends more than the
/// protocol's bound is cut from the front, on a boundary, and it says so.
#[tokio::test]
async fn an_answer_past_the_bound_is_cut_and_says_so() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    let (st, v) = req(&t, Method::GET, &url(sid, "bg-2"), None, &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["text"].as_str().unwrap().len(), MAX_BG_OUTPUT_BYTES);
    assert_eq!(v["cut"], true);
}

/// Abuse case 6 and the old node: sentences with a status that says whose
/// they are, never a 500.
#[tokio::test]
async fn nothing_to_show_is_a_sentence_never_a_500() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    let (st, v) = req(&t, Method::GET, &url(sid, "bg-9"), None, &t.alex).await;
    assert_eq!(
        (st, v["error"].as_str()),
        (StatusCode::CONFLICT, Some(FORGOTTEN))
    );
    let old = chat(&t, &t.alex, "old", None).await;
    let (st, v) = req(&t, Method::GET, &url(old, "bg-1"), None, &t.alex).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("older"), "{v}");
    assert!(t.old.asks().is_empty());
    gone(&t, "local", &t.local).await;
    let (st, v) = req(&t, Method::GET, &url(sid, "bg-1"), None, &t.alex).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert!(v["error"].as_str().unwrap().contains("away"), "{v}");
}

fn answer(req_id: &str, text: &str) -> NodeToMaster {
    NodeToMaster::BackgroundOutputResult {
        req_id: req_id.into(),
        output: Some(out(text)),
        error: None,
    }
}

/// Abuse case 5 and the link's: one account holds at most a few places (the
/// next is refused at once), an answer from another node, for an unknown id,
/// a duplicate and a late one are ignored, and a node that drops its link
/// ends its requests as "away" at once.
#[tokio::test]
async fn places_are_bounded_per_account_and_only_the_asked_node_answers_once() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    HELD.lock().unwrap().push(sid);
    let get = || {
        let (base, alex) = (t.base.clone(), t.alex.clone());
        tokio::spawn(async move {
            let r = reqwest::Client::new()
                .get(format!("{base}/api/sessions/{sid}/background/bg-1/output"))
                .login(&alex)
                .same_origin()
                .send()
                .await
                .unwrap();
            let st = r.status();
            (st, r.json::<Value>().await.unwrap_or(Value::Null))
        })
    };
    let first = get();
    for _ in 0..400 {
        if t.local.last_req().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let req = t.local.last_req().expect("no request reached the node");
    // Three more places (four in all), then the fifth is refused at once.
    let more: Vec<_> = (0..3).map(|_| get()).collect();
    for _ in 0..400 {
        if t.local.asks().len() >= 4 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(t.local.asks().len(), 4);
    let (st, v) = req_busy(&t, sid).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert!(v["error"].as_str().unwrap().contains("waiting"), "{v}");
    assert_eq!(t.local.asks().len(), 4, "the refused one reached the node");
    // Foreign and unknown answers do not end a request.
    t.old.say.send(answer(&req, "forged-by-old")).unwrap();
    t.local
        .say
        .send(answer("no-such-request", "unknown"))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !first.is_finished(),
        "a foreign or unknown answer ended the request"
    );
    // The node asked answers, then again: the first wins.
    t.local.say.send(answer(&req, "the-real-one")).unwrap();
    t.local.say.send(answer(&req, "duplicate")).unwrap();
    let (st, v) = first.await.unwrap();
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["text"], "the-real-one");
    // The rest end as "away" at once when the link drops.
    let at = std::time::Instant::now();
    t.local.leave.send(()).unwrap();
    for p in more {
        let (st, v) = p.await.unwrap();
        assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{v}");
        assert!(v["error"].as_str().unwrap().contains("away"), "{v}");
    }
    assert!(at.elapsed() < Duration::from_secs(10), "{:?}", at.elapsed());
}

async fn req_busy(t: &T, sid: SessionId) -> (StatusCode, Value) {
    req(t, Method::GET, &url(sid, "bg-1"), None, &t.alex).await
}
