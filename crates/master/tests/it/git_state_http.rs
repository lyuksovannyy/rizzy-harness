//! The header's git chip over HTTP (X.64, PLAN.md T19): owner only, every
//! other account gets 404 (admins and workspace admins included, T10), and
//! so does a plain chat, with the node never asked; the master asks the
//! owning node once and passes its answer on, bounded again (the fake node
//! here is hostile); a node that is away, too old, slow or has nothing to
//! show is `null`, never an error the page must word; a node that is busy is
//! a 503. The real node's reading is `crates/node/src/git/state.rs`.

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
    CAP_DIFF, CAP_GIT_STATE, GitState, MasterToNode, NodeToMaster, PROTOCOL_VERSION,
};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

const TOKEN: &str = "api-secret";

/// What a hostile node sends: a branch of 4,000 characters with control and
/// line-separator characters in it, a count past the cap.
fn hostile() -> GitState {
    GitState {
        branch: Some(format!("a\u{1b}[31m\nb\u{2028}{}", "é".repeat(2000))),
        commit: Some("abcdef0".into()),
        changed: Some(u32::MAX),
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
                MasterToNode::GitState { req_id, .. } => Some(req_id.clone()),
                _ => None,
            })
    }

    /// The request id of the `n`th `session.git_state` this node got.
    fn seen_req(&self, n: usize) -> String {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                MasterToNode::GitState { req_id, .. } => Some(req_id.clone()),
                _ => None,
            })
            .nth(n)
            .unwrap()
    }

    /// The chat of every `session.git_state` this node got.
    fn asks(&self) -> Vec<SessionId> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                MasterToNode::GitState { session_id, .. } => Some(*session_id),
                _ => None,
            })
            .collect()
    }
}

/// A node `id` announcing `caps`: it opens chats of `profile` (`incognito`:
/// temporary ones), acks every command, and answers a git state as above.
fn fake(id: &str, caps: &[&str], profile: Profile, incognito: bool) -> (NodeLink, Fake) {
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
                                profile,
                                agent_id: None,
                                incognito,
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
                        // A chat in HELD is never answered, one in BUSY gets
                        // `busy`, one in NONE gets no state; every other gets
                        // a hostile one (a long branch full of control
                        // characters, a count past the cap).
                        MasterToNode::GitState { req_id, session_id } => {
                            let sid = *session_id;
                            if !HELD.lock().unwrap().contains(&sid) {
                                let busy = BUSY.lock().unwrap().contains(&sid);
                                let none = NONE.lock().unwrap().contains(&sid);
                                let _ = tx.try_send(NodeToMaster::GitStateResult {
                                    req_id: req_id.clone(),
                                    state: (!busy && !none).then(hostile),
                                    busy,
                                });
                            }
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

/// Chats a fake node leaves unanswered, answers `busy` or with no state.
/// Session ids are unique to a test, so tests do not see each other's entries.
static HELD: Mutex<Vec<SessionId>> = Mutex::new(Vec::new());
static BUSY: Mutex<Vec<SessionId>> = Mutex::new(Vec::new());
static NONE: Mutex<Vec<SessionId>> = Mutex::new(Vec::new());

struct T {
    base: String,
    master: Master,
    alex: String,
    local: Fake,
    old: Fake,
    plain: Fake,
    inc: Fake,
    _dir: tempfile::TempDir,
}

/// A master with the fake nodes `local` (announcing `git-state`), `old` (a
/// node from before it), `plain` (plain chats),
/// `inc` (temporary chats), and alex, its claimed admin.
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
    let (l, local) = fake("local", &[CAP_GIT_STATE], Profile::Agentic, false);
    master.add_link(l, true).await.unwrap();
    let (l, old) = fake("old", &[CAP_DIFF], Profile::Agentic, false);
    master.add_link(l, false).await.unwrap();
    let (l, plain) = fake("plain", &[CAP_GIT_STATE], Profile::Chat, false);
    master.add_link(l, false).await.unwrap();
    let (l, inc) = fake("inc", &[CAP_GIT_STATE], Profile::Agentic, true);
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
        plain,
        inc,
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

fn url(sid: SessionId) -> String {
    format!("/api/sessions/{sid}/git")
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

#[tokio::test]
async fn the_owner_gets_what_the_node_says_bounded_and_the_node_is_asked_once() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    let (st, v) = req(&t, Method::GET, &url(sid), None, &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let branch = v["branch"].as_str().unwrap();
    assert!(branch.len() <= 200, "{}", branch.len());
    assert!(!branch.chars().any(char::is_control), "{branch:?}");
    assert!(!branch.contains('\u{2028}'), "{branch:?}");
    assert!(branch.starts_with("a\u{FFFD}[31m\u{FFFD}b"), "{branch:?}");
    assert_eq!(
        (&v["commit"], &v["changed"]),
        (&json!("abcdef0"), &json!(1000))
    );
    assert_eq!(t.local.asks(), vec![sid]);
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
        let (st, v) = req(&t, Method::GET, &url(sid), None, who).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{name}: {v}");
        assert_eq!(v["error"], "unknown session", "{name}: the same as no chat");
    }
    let (st, v) = req(&t, Method::GET, &url(SessionId::new()), None, &t.alex).await;
    assert_eq!(
        (st, v["error"].as_str()),
        (StatusCode::NOT_FOUND, Some("unknown session"))
    );
    let (st, _) = req(&t, Method::GET, &url(sid), None, "").await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    assert!(t.local.asks().is_empty(), "{:?}", t.local.asks());
}

#[tokio::test]
async fn a_plain_chat_is_404_and_asks_nothing_a_temporary_coding_chat_is_served() {
    let t = start().await;
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/sessions",
        Some(json!({"cwd": "/w", "title": "t", "node_id": "plain", "profile": "chat"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let plain: SessionId = v["session_id"].as_str().unwrap().parse().unwrap();
    let (st, v) = req(&t, Method::GET, &url(plain), None, &t.alex).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{v}");
    assert_eq!(v["error"], "unknown session");
    assert!(t.plain.asks().is_empty());
    let temp = chat(&t, &t.alex, "inc", None).await;
    let (st, v) = req(&t, Method::GET, &url(temp), None, &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["commit"], "abcdef0");
    assert_eq!(t.inc.asks(), vec![temp]);
}

#[tokio::test]
async fn nothing_to_show_is_null_and_a_busy_node_is_a_503_never_a_500() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    // A folder that is no repository the node reads.
    NONE.lock().unwrap().push(sid);
    let (st, v) = req(&t, Method::GET, &url(sid), None, &t.alex).await;
    assert_eq!((st, v), (StatusCode::OK, Value::Null));
    // A node that is busy: the page keeps what it shows.
    let busy = chat(&t, &t.alex, "local", None).await;
    BUSY.lock().unwrap().push(busy);
    let (st, v) = req(&t, Method::GET, &url(busy), None, &t.alex).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    // A node from before X.64 is sent nothing.
    let old = chat(&t, &t.alex, "old", None).await;
    let (st, v) = req(&t, Method::GET, &url(old), None, &t.alex).await;
    assert_eq!((st, v), (StatusCode::OK, Value::Null));
    assert!(t.old.asks().is_empty());
    // A node that is away.
    gone(&t, "local", &t.local).await;
    let (st, v) = req(&t, Method::GET, &url(sid), None, &t.alex).await;
    assert_eq!((st, v), (StatusCode::OK, Value::Null));
}

fn answer(req_id: &str, branch: &str) -> NodeToMaster {
    NodeToMaster::GitStateResult {
        req_id: req_id.into(),
        state: Some(GitState {
            branch: Some(branch.into()),
            commit: None,
            changed: None,
        }),
        busy: false,
    }
}

/// The link's abuse cases: an answer from another node, for an unknown id, a
/// duplicate and a late one are ignored; a request whose node drops its link
/// ends at once, not at the timeout.
#[tokio::test]
async fn only_the_asked_node_answers_once() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    HELD.lock().unwrap().push(sid);
    let get = || {
        let (base, alex) = (t.base.clone(), t.alex.clone());
        tokio::spawn(async move {
            let r = reqwest::Client::new()
                .get(format!("{base}/api/sessions/{sid}/git"))
                .login(&alex)
                .same_origin()
                .send()
                .await
                .unwrap();
            let st = r.status();
            (st, r.json::<Value>().await.unwrap_or(Value::Null))
        })
    };
    let pending = get();
    let mut req = None;
    for _ in 0..400 {
        if let Some(r) = t.local.last_req() {
            req = Some(r);
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let req = req.expect("no session.git_state reached the node");
    t.old.say.send(answer(&req, "forged-by-old")).unwrap();
    t.local
        .say
        .send(answer("no-such-request", "unknown"))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !pending.is_finished(),
        "a foreign or unknown answer ended the request"
    );
    t.local.say.send(answer(&req, "the-real-one")).unwrap();
    t.local.say.send(answer(&req, "duplicate")).unwrap();
    let (st, v) = pending.await.unwrap();
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["branch"], "the-real-one");
    // A held request ends when the link drops.
    let before = t.local.asks().len();
    let pending = get();
    for _ in 0..400 {
        if t.local.asks().len() > before {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let at = std::time::Instant::now();
    t.local.leave.send(()).unwrap();
    let (st, v) = pending.await.unwrap();
    assert_eq!((st, v), (StatusCode::OK, Value::Null));
    assert!(at.elapsed() < Duration::from_secs(10), "{:?}", at.elapsed());
}

/// One account has two waiting places: a slow node cannot be asked past them,
/// and the third is refused at once, not sent.
#[tokio::test]
async fn one_account_has_two_waiting_places() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    HELD.lock().unwrap().push(sid);
    let mut held = Vec::new();
    for _ in 0..2 {
        let (base, alex) = (t.base.clone(), t.alex.clone());
        held.push(tokio::spawn(async move {
            reqwest::Client::new()
                .get(format!("{base}/api/sessions/{sid}/git"))
                .login(&alex)
                .same_origin()
                .send()
                .await
                .unwrap()
                .json::<Value>()
                .await
                .unwrap()
        }));
    }
    for _ in 0..400 {
        if t.local.asks().len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(t.local.asks().len(), 2);
    let (st, v) = req(&t, Method::GET, &url(sid), None, &t.alex).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert_eq!(t.local.asks().len(), 2, "the third was not sent");
    t.local.leave.send(()).unwrap();
    for h in held {
        assert_eq!(h.await.unwrap(), Value::Null);
    }
}

/// A page that gives up (an abort, then a retry) does not free its place
/// while the node is still working: the places stay held until the node
/// answers or the link drops, so one account cannot keep the node busy by
/// aborting and asking again.
#[tokio::test]
async fn an_aborted_request_keeps_its_place_until_the_node_answers() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    HELD.lock().unwrap().push(sid);
    let mut held = Vec::new();
    for _ in 0..2 {
        let (base, alex) = (t.base.clone(), t.alex.clone());
        held.push(tokio::spawn(async move {
            reqwest::Client::new()
                .get(format!("{base}/api/sessions/{sid}/git"))
                .login(&alex)
                .same_origin()
                .send()
                .await
        }));
    }
    for _ in 0..400 {
        if t.local.asks().len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(t.local.asks().len(), 2);
    for h in &held {
        h.abort();
    }
    for h in held {
        let _ = h.await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (st, v) = req(&t, Method::GET, &url(sid), None, &t.alex).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert_eq!(t.local.asks().len(), 2, "no third request reached the node");
    // The node answers the first: its place is free again.
    let first = t.local.seen_req(0);
    t.local.say.send(answer(&first, "late")).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    HELD.lock().unwrap().retain(|s| *s != sid);
    let (st, v) = req(&t, Method::GET, &url(sid), None, &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
}
