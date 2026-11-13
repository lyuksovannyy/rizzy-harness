//! The person's stop of a chat's background commands over HTTP (T.8d step
//! 2, docs/background-commands.md §4.7, §4.9): owner only, every other
//! account gets 404 (admins and workspace admins included, T10), with the
//! login key and the page's Origin like every other cookie write (Gate 3
//! P-1, AUDIT L6). The master runs nothing: it relays
//! `session.stop_background { by: user }` to the owning node, answers an
//! end the log already shows without asking the node, passes the node's
//! refusal on, and says plainly when the node is away or cannot do it.
//!
//! The node here is a fake that opens chats, appends the background events
//! a test asks for, keeps every command it is sent and acks; the real
//! node's side (the end recorded as `stopped_by_user`, the processes gone)
//! is `crates/cli/tests/background.rs`.

mod common;
use common::{Login as _, SameOrigin as _};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::event::{
    BackgroundConfirmed, BackgroundEnded, BackgroundReason, BackgroundStarted, EventKind, Profile,
    SessionCreated,
};
use harness_core::policy::ApprovalMode;
use harness_core::{RawEvent, Secret, SessionId};
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, router};
use harness_node::NodeLink;
use harness_protocol::{MasterToNode, NodeToMaster, PROTOCOL_VERSION, StopBackgroundBy};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

const TOKEN: &str = "api-secret";

/// What a test asks the fake node to do.
enum Do {
    Append(SessionId, Box<EventKind>),
    /// Drop the link, as a node that went away.
    Leave,
}

struct Fake {
    seen: Arc<Mutex<Vec<MasterToNode>>>,
    tx: mpsc::UnboundedSender<Do>,
}

impl Fake {
    fn stops(&self) -> Vec<(SessionId, Option<String>, StopBackgroundBy)> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                MasterToNode::StopBackground {
                    session_id,
                    bg_id,
                    by,
                    ..
                } => Some((*session_id, bg_id.clone(), *by)),
                _ => None,
            })
            .collect()
    }

    fn append(&self, sid: SessionId, k: EventKind) {
        self.tx.send(Do::Append(sid, Box::new(k))).unwrap();
    }
}

/// An agentic node `id` announcing `caps`: it opens chats, appends what it
/// is told, acks every command, and refuses a stop of `bg-9` as the real
/// node words it.
fn fake(id: &str, caps: &[&str]) -> (NodeLink, Fake) {
    let (to_node, mut commands) = mpsc::channel::<MasterToNode>(1024);
    let (tx, from_node) = mpsc::channel::<NodeToMaster>(1024);
    let mut capabilities = vec!["native".to_string(), "profile:agentic".to_string()];
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
    let (do_tx, mut do_rx) = mpsc::unbounded_channel::<Do>();
    let (node, keep) = (id.to_string(), seen.clone());
    tokio::spawn(async move {
        let mut seqs: HashMap<SessionId, u64> = HashMap::new();
        let send = |seqs: &mut HashMap<SessionId, u64>, sid: SessionId, k: EventKind| {
            let seq = seqs.entry(sid).or_insert(0);
            *seq += 1;
            let (kind, payload) = k.into_parts();
            let _ = tx.try_send(NodeToMaster::Event {
                event: RawEvent {
                    session_id: sid,
                    seq: *seq,
                    ts_ms: harness_core::now_ms(),
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
                    if let MasterToNode::Create { session_id, .. } = &msg {
                        let created = EventKind::SessionCreated(SessionCreated {
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
                        });
                        send(&mut seqs, *session_id, created);
                    }
                    let refused = matches!(&msg, MasterToNode::StopBackground { bg_id: Some(b), .. } if b == "bg-9");
                    if let Some(req_id) = msg.req_id() {
                        let _ = tx.try_send(NodeToMaster::Ack {
                            req_id: req_id.into(),
                            ok: !refused,
                            error: refused.then(|| "no background command bg-9 in this chat (it has bg-1)".into()),
                        });
                    }
                }
                d = do_rx.recv() => match d {
                    Some(Do::Append(sid, k)) => send(&mut seqs, sid, *k),
                    Some(Do::Leave) | None => return,
                },
            }
        }
    });
    (NodeLink { to_node, from_node }, Fake { seen, tx: do_tx })
}

struct T {
    base: String,
    master: Master,
    alex: String,
    local: Fake,
    old: Fake,
    _dir: tempfile::TempDir,
}

/// A master with the fake agentic node `local` (announcing `background`)
/// and `old` (a node from before T.8d), and alex, its claimed admin.
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
    let (l, local) = fake("local", &[harness_protocol::CAP_BACKGROUND]);
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
        Some(json!({"username": name, "role": role, "can_agentic": true})),
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

/// An agentic chat of `who` on `node`, in `workspace` if named.
async fn chat(t: &T, who: &str, node: &str, workspace: Option<&str>) -> SessionId {
    let mut body = json!({"cwd": "/w", "title": "t", "node_id": node});
    if let Some(w) = workspace {
        body["workspace_id"] = json!(w);
    }
    let (st, v) = req(t, Method::POST, "/api/sessions", Some(body), who).await;
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

fn ended(bg: &str, reason: BackgroundReason) -> EventKind {
    EventKind::BackgroundEnded(BackgroundEnded {
        bg_id: bg.into(),
        reason,
        exit_code: Some(0),
        signal: None,
        confirmed: BackgroundConfirmed::Ended,
        ran_ms: 1000,
        output_bytes: 0,
        tail: String::new(),
        tail_cut: false,
        delivered: false,
    })
}

/// Until the master's mirror of `sid` holds `n` events.
async fn mirrored(t: &T, sid: SessionId, n: usize) {
    for _ in 0..400 {
        let (_, v) = req(
            t,
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

fn one(sid: SessionId, bg: &str) -> String {
    format!("/api/sessions/{sid}/background/{bg}/stop")
}

fn all(sid: SessionId) -> String {
    format!("/api/sessions/{sid}/background/stop")
}

#[tokio::test]
async fn the_owner_stops_one_and_all_and_the_node_is_told_it_is_the_person() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    t.local.append(sid, started("bg-1"));
    t.local.append(sid, started("bg-2"));
    mirrored(&t, sid, 3).await;

    let (st, v) = req(&t, Method::POST, &one(sid, "bg-1"), None, &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        t.local.stops(),
        vec![(sid, Some("bg-1".to_string()), StopBackgroundBy::User)]
    );

    // Stop all: one message for all of them.
    let (st, v) = req(&t, Method::POST, &all(sid), None, &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(t.local.stops()[1], (sid, None, StopBackgroundBy::User));
}

#[tokio::test]
async fn nobody_else_may_stop_them_not_even_an_admin() {
    let t = start().await;
    let bob = account(&t, "bob", "member").await;
    let carol = account(&t, "carol", "admin").await;
    let dana = account(&t, "dana", "member").await;
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
    t.local.append(sid, started("bg-1"));
    mirrored(&t, sid, 2).await;

    for (who, name) in [
        (&bob, "another member"),
        (&carol, "a server admin"),
        (&dana, "a workspace admin"),
    ] {
        for path in [one(sid, "bg-1"), all(sid)] {
            let (st, v) = req(&t, Method::POST, &path, None, who).await;
            assert_eq!(st, StatusCode::NOT_FOUND, "{name} {path}: {v}");
            assert_eq!(
                v["error"], "unknown session",
                "{name}: the same answer as for no chat"
            );
        }
    }
    // A chat that does not exist: the same 404.
    let none = SessionId::new();
    let (st, v) = req(&t, Method::POST, &one(none, "bg-1"), None, &t.alex).await;
    assert_eq!(
        (st, v["error"].as_str()),
        (StatusCode::NOT_FOUND, Some("unknown session"))
    );
    assert!(t.local.stops().is_empty(), "{:?}", t.local.stops());
}

#[tokio::test]
async fn a_stop_needs_the_login_key_and_the_pages_origin() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    t.local.append(sid, started("bg-1"));
    mirrored(&t, sid, 2).await;
    let url = format!("{}{}", t.base, one(sid, "bg-1"));
    let c = reqwest::Client::new();
    // No login at all.
    let r = c.post(&url).same_origin().send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    // The cookie without its login key (what the agent's own dev server
    // on this host would get from the browser).
    let r = c
        .post(&url)
        .header(reqwest::header::COOKIE, &t.alex)
        .same_origin()
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    // The full login, but no Origin, or another site's.
    let r = c.post(&url).login(&t.alex).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    let r = c
        .post(&url)
        .login(&t.alex)
        .header(reqwest::header::ORIGIN, "http://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    assert!(t.local.stops().is_empty(), "{:?}", t.local.stops());
    // The page's own request goes through.
    let r = c
        .post(&url)
        .login(&t.alex)
        .same_origin()
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
}

#[tokio::test]
async fn the_node_is_always_asked_and_its_refusal_is_the_answer() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    t.local.append(sid, started("bg-1"));
    t.local.append(sid, ended("bg-1", BackgroundReason::Exited));
    mirrored(&t, sid, 3).await;

    // A command the master's log shows ended is still the node's to
    // answer: the master reads no log for a stop (review S2-2: that read
    // scanned the chat's whole log under the mirror's lock).
    let (st, v) = req(&t, Method::POST, &one(sid, "bg-1"), None, &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        t.local.stops(),
        vec![(sid, Some("bg-1".to_string()), StopBackgroundBy::User)]
    );
    // Stop all when the log shows none running: the node is asked all the
    // same; Stop all must not be something a command can hide from.
    let (st, v) = req(&t, Method::POST, &all(sid), None, &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(t.local.stops()[1], (sid, None, StopBackgroundBy::User));

    // An id the node does not have: its refusal is the answer.
    let (st, v) = req(&t, Method::POST, &one(sid, "bg-9"), None, &t.alex).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"]
            .as_str()
            .unwrap()
            .contains("no background command bg-9"),
        "{v}"
    );
    assert_eq!(t.local.stops().len(), 3);

    // Not an id at all: refused here.
    for bad in ["bg-", "bg-1x", "1", "bg-12345678901", "..%2Fbg-1"] {
        let (st, v) = req(&t, Method::POST, &one(sid, bad), None, &t.alex).await;
        assert!(
            st == StatusCode::BAD_REQUEST || st == StatusCode::NOT_FOUND,
            "{bad}: {st} {v}"
        );
    }
    assert_eq!(t.local.stops().len(), 3);
}

#[tokio::test]
async fn a_stop_for_a_chat_on_an_away_node_says_so() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", None).await;
    t.local.append(sid, started("bg-1"));
    mirrored(&t, sid, 2).await;
    t.local.tx.send(Do::Leave).unwrap();
    for _ in 0..400 {
        if !t.master.is_connected("local") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!t.master.is_connected("local"));
    for path in [one(sid, "bg-1"), all(sid)] {
        let (st, v) = req(&t, Method::POST, &path, None, &t.alex).await;
        assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{v}");
        let e = v["error"].as_str().unwrap();
        assert!(e.contains("away") && e.contains("timeout"), "{e}");
    }
}

#[tokio::test]
async fn a_node_without_background_commands_is_sent_nothing() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "old", None).await;
    // Whatever such a node's log says.
    t.old.append(sid, started("bg-1"));
    mirrored(&t, sid, 2).await;
    let (st, v) = req(&t, Method::POST, &one(sid, "bg-1"), None, &t.alex).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"]
            .as_str()
            .unwrap()
            .contains("does not run background commands"),
        "{v}"
    );
    assert!(t.old.stops().is_empty());
}
