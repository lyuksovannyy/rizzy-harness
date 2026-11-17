//! The project board tools on the master (ADOPTION_PLAN X.32, PLAN.md
//! T78): a node asks for the session it runs; the master finds the
//! project, the person and what they may do from that session alone. The
//! nodes here are fakes the test drives, so a request can be forged at
//! will.

mod common;
use common::{Login as _, SameOrigin as _};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::board::{BoardAnswer, BoardRequest, CardChat};
use harness_core::{RawEvent, Secret, SessionId};
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, PromptOpts, router};
use harness_node::NodeLink;
use harness_protocol::{MasterToNode, NodeToMaster, PROTOCOL_VERSION};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

type Answers = Arc<Mutex<HashMap<String, Vec<(Option<BoardAnswer>, Option<String>)>>>>;
type Seen = Arc<Mutex<Vec<MasterToNode>>>;

/// A fake node: acks every command, mirrors a `session_created` for each
/// chat it is asked to create, keeps the seq of each session so the test can
/// append events, and records each `board.result`.
struct Fake {
    tx: mpsc::Sender<NodeToMaster>,
    seqs: Arc<Mutex<HashMap<SessionId, u64>>>,
    answers: Answers,
    seen: Seen,
}

fn fake_node(node_id: &str, caps: &[&str]) -> (NodeLink, Fake) {
    let (to_node, mut commands) = mpsc::channel::<MasterToNode>(1024);
    let (to_master, from_node) = mpsc::channel::<NodeToMaster>(1024);
    to_master
        .try_send(NodeToMaster::Hello {
            protocol_version: PROTOCOL_VERSION,
            node_id: node_id.into(),
            hostname: "h".into(),
            version: "0".into(),
            capabilities: caps.iter().map(|c| c.to_string()).collect(),
            sessions: vec![],
        })
        .unwrap();
    let fake = Fake {
        tx: to_master.clone(),
        seqs: Arc::default(),
        answers: Arc::default(),
        seen: Arc::default(),
    };
    let (seqs, answers, seen) = (fake.seqs.clone(), fake.answers.clone(), fake.seen.clone());
    let node = node_id.to_string();
    tokio::spawn(async move {
        while let Some(msg) = commands.recv().await {
            if let MasterToNode::Create {
                session_id,
                title,
                cwd,
                incognito,
                ..
            } = &msg
            {
                let payload = json!({"title": title, "cwd": cwd, "model": "m", "backend": "native",
                    "node_id": node, "profile": "chat", "approval_mode": "ask",
                    "incognito": incognito});
                seqs.lock().unwrap().insert(*session_id, 1);
                let event = RawEvent {
                    session_id: *session_id,
                    seq: 1,
                    ts_ms: 1,
                    kind: "session_created".into(),
                    payload,
                };
                to_master.try_send(NodeToMaster::Event { event }).unwrap();
            }
            if let MasterToNode::BoardResult {
                req_id,
                answer,
                error,
            } = &msg
            {
                answers
                    .lock()
                    .unwrap()
                    .entry(req_id.clone())
                    .or_default()
                    .push((answer.clone(), error.clone()));
                continue;
            }
            let req_id = msg.req_id().map(str::to_string);
            seen.lock().unwrap().push(msg);
            if let Some(req_id) = req_id {
                let ack = NodeToMaster::Ack {
                    req_id,
                    ok: true,
                    error: None,
                };
                to_master.try_send(ack).unwrap();
            }
        }
    });
    (NodeLink { to_node, from_node }, fake)
}

impl Fake {
    async fn emit(&self, sid: SessionId, kind: &str, payload: Value) {
        let seq = {
            let mut s = self.seqs.lock().unwrap();
            let n = s.entry(sid).or_insert(0);
            *n += 1;
            *n
        };
        let event = RawEvent {
            session_id: sid,
            seq,
            ts_ms: 10 + seq as i64,
            kind: kind.into(),
            payload,
        };
        self.tx.send(NodeToMaster::Event { event }).await.unwrap();
    }

    /// Send one request with `req_id` on behalf of `sid`.
    async fn send(&self, sid: SessionId, req_id: &str, request: BoardRequest) {
        self.tx
            .send(NodeToMaster::Board {
                req_id: req_id.into(),
                session_id: sid,
                request,
            })
            .await
            .unwrap();
    }

    /// The answers that came for `req_id`, once `n` have.
    async fn answers(&self, req_id: &str, n: usize) -> Vec<Result<BoardAnswer, String>> {
        for _ in 0..300 {
            if let Some(a) = self.answers.lock().unwrap().get(req_id)
                && a.len() >= n
            {
                return a
                    .iter()
                    .map(|(a, e)| match e {
                        Some(e) => Err(e.clone()),
                        None => Ok(a.clone().expect("an answer")),
                    })
                    .collect();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no board.result for {req_id}");
    }

    /// Ask on behalf of `sid` and wait for the answer.
    async fn board(&self, sid: SessionId, request: BoardRequest) -> Result<BoardAnswer, String> {
        let req_id = harness_core::new_id();
        self.send(sid, &req_id, request).await;
        self.answers(&req_id, 1).await.remove(0)
    }
}

struct T {
    base: String,
    master: Master,
    chat: Fake,
    other: Fake,
    _dir: tempfile::TempDir,
}

async fn start() -> T {
    start_with(&[harness_protocol::CAP_BOARD]).await
}

/// A master whose chat node announces `caps` (and the chat profile).
async fn start_with(caps: &[&str]) -> T {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, "X").unwrap();
    let master = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim_path,
        }),
    );
    let mut chat_caps = vec!["profile:chat"];
    chat_caps.extend_from_slice(caps);
    let (link, chat) = fake_node("master", &chat_caps);
    master.add_link(link, true).await.unwrap();
    let (link, other) = fake_node("other", &[harness_protocol::CAP_BOARD, "exec:sandbox"]);
    master.add_link(link, false).await.unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        master,
        chat,
        other,
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

async fn post_anon(t: &T, path: &str, body: Value, c: Option<&str>) -> reqwest::Response {
    let mut rb = reqwest::Client::new()
        .post(format!("{}{path}", t.base))
        .json(&body);
    if let Some(c) = c {
        rb = rb.login(c).same_origin();
    }
    rb.send().await.unwrap()
}

/// The admin "alex" and a member "bob".
async fn users(t: &T) -> [String; 2] {
    let alex = common::signed_in(
        post_anon(
            t,
            "/api/setup",
            json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}),
            None,
        )
        .await,
    )
    .await;
    let pw = req(
        t,
        Method::POST,
        "/api/users",
        Some(json!({"username": "bob"})),
        &alex,
    )
    .await
    .1["password"]
        .as_str()
        .unwrap()
        .to_string();
    let first = common::signed_in(
        post_anon(
            t,
            "/api/login",
            json!({"username": "bob", "password": pw}),
            None,
        )
        .await,
    )
    .await;
    let bob = common::signed_in(
        post_anon(
            t,
            "/api/me/password",
            json!({"current": pw, "new": "bobs own password"}),
            Some(&first),
        )
        .await,
    )
    .await;
    [alex, bob]
}

async fn chat(t: &T, c: &str, extra: Value) -> SessionId {
    let mut body = json!({"profile": "chat"});
    for (k, v) in extra.as_object().unwrap() {
        body[k] = v.clone();
    }
    let (st, v) = req(t, Method::POST, "/api/sessions", Some(body), c).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["session_id"].as_str().unwrap().parse().unwrap()
}

/// Wait until the mirror has `sid` at `seq`.
async fn landed(t: &T, sid: SessionId) {
    let want = *t.chat.seqs.lock().unwrap().get(&sid).unwrap();
    for _ in 0..200 {
        if t.master.inner_mirror().head_seq(sid).unwrap_or(0) >= want {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("events of {sid} did not land");
}

/// Prompt `sid` as its owner would, then mark its turn running on the node.
async fn running_turn(t: &T, sid: SessionId, turn: &str) {
    t.master
        .prompt(
            sid,
            "what is on the board?".into(),
            PromptOpts {
                by_owner: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    start_turn(t, sid, turn).await;
}

/// The node starts a turn of the newest prompt `sid` got, recording its
/// agent as an honest node does (`AgentSpec::agent_ref`, with what the
/// master withheld).
async fn start_turn(t: &T, sid: SessionId, turn: &str) {
    let agent = t
        .chat
        .seen
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find_map(|m| match m {
            MasterToNode::Prompt {
                session_id, agent, ..
            } if *session_id == sid => agent.as_ref().map(|a| a.agent_ref()),
            _ => None,
        })
        .expect("no prompt for the session");
    t.chat
        .emit(
            sid,
            "turn_started",
            json!({"turn_id": turn, "agent": agent}),
        )
        .await;
    landed(t, sid).await;
}

fn add(title: &str) -> BoardRequest {
    BoardRequest::Add {
        title: title.into(),
        notes: String::new(),
        column: None,
    }
}

fn list() -> BoardRequest {
    BoardRequest::List { column: None }
}

/// Every open card of a `list` answer: (column name, card id, title).
fn cards(a: &BoardAnswer) -> Vec<(String, String, String)> {
    match a {
        BoardAnswer::Board { columns, .. } => columns
            .iter()
            .flat_map(|c| {
                c.cards
                    .iter()
                    .map(|k| (c.name.clone(), k.card_id.clone(), k.title.clone()))
            })
            .collect(),
        other => panic!("not a board: {other:?}"),
    }
}

fn card_id(a: &BoardAnswer) -> String {
    match a {
        BoardAnswer::Done { card_id, .. } => card_id.clone(),
        other => panic!("not a change: {other:?}"),
    }
}

async fn new_project(t: &T, c: &str, body: Value) -> String {
    let (st, v) = req(t, Method::POST, "/api/projects", Some(body), c).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["project_id"].as_str().unwrap().to_string()
}

/// The board as people see it over HTTP.
async fn http_board(t: &T, c: &str, project: &str) -> Value {
    let (st, v) = req(
        t,
        Method::GET,
        &format!("/api/projects/{project}/board"),
        None,
        c,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v
}

/// A person's card in the first column, over HTTP.
async fn http_card(t: &T, c: &str, project: &str, title: &str, extra: Value) -> String {
    let b = http_board(t, c, project).await;
    let col = b["columns"][0]["column_id"].as_str().unwrap().to_string();
    let mut body = json!({"column_id": col, "title": title});
    for (k, v) in extra.as_object().unwrap() {
        body[k] = v.clone();
    }
    let (st, v) = req(
        t,
        Method::POST,
        &format!("/api/projects/{project}/board/cards"),
        Some(body),
        c,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["columns"][0]["cards"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["title"] == title)
        .unwrap()["card_id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// The agent spec of the newest prompt the chat node got.
fn last_spec(t: &T) -> harness_core::agent::AgentSpec {
    t.chat
        .seen
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find_map(|m| match m {
            MasterToNode::Prompt { agent, .. } => agent.clone(),
            _ => None,
        })
        .unwrap()
}

/// A session in project A reaches A's board and nothing of B's: the
/// request has no field to name another project, and a card id of B's is
/// "no such card" for every change. Its card says which agent made it, in
/// which chat, and the change is an audit line.
#[tokio::test]
async fn a_session_reaches_its_own_projects_board_only() {
    let t = start().await;
    let [alex, _bob] = users(&t).await;
    let a = new_project(&t, &alex, json!({"name": "Alpha"})).await;
    let b = new_project(&t, &alex, json!({"name": "Beta"})).await;
    let b_card = http_card(&t, &alex, &b, "Beta secret task", json!({})).await;
    let chat_a = chat(&t, &alex, json!({"project_id": a})).await;
    landed(&t, chat_a).await;

    // Outside a turn: refused.
    assert!(t.chat.board(chat_a, list()).await.is_err());

    running_turn(&t, chat_a, "t1").await;
    let spec = last_spec(&t);
    assert!(
        spec.allows_tool("board") && spec.allows_tool("board_edit"),
        "{:?}",
        spec.deny_tools
    );
    let made = card_id(
        &t.chat
            .board(chat_a, add("Write the release notes"))
            .await
            .unwrap(),
    );
    let seen = cards(&t.chat.board(chat_a, list()).await.unwrap());
    assert_eq!(
        seen,
        [(
            "To do".to_string(),
            made.clone(),
            "Write the release notes".to_string()
        )]
    );
    // B's card is not on A's board, for any change.
    for r in [
        BoardRequest::Move {
            card_id: b_card.clone(),
            column: "Done".into(),
            position: None,
        },
        BoardRequest::Note {
            card_id: b_card.clone(),
            text: "hijacked".into(),
        },
    ] {
        let e = t.chat.board(chat_a, r).await.unwrap_err();
        assert!(e.contains("no card with that id"), "{e}");
    }
    let beta = http_board(&t, &alex, &b).await;
    assert_eq!(beta["columns"][0]["cards"].as_array().unwrap().len(), 1);
    assert_eq!(beta["columns"][0]["cards"][0]["notes"], "");
    // The request names no place: a project field does not parse.
    assert!(
        serde_json::from_value::<BoardRequest>(json!({"action": "list", "project_id": b})).is_err()
    );

    // People see who made it, and the chat link (its owner: with title).
    let alpha = http_board(&t, &alex, &a).await;
    let card = &alpha["columns"][0]["cards"][0];
    assert_eq!(card["agent"], "Assistant", "{card}");
    assert_eq!(card["chat"]["session_id"], chat_a.to_string(), "{card}");
    // A move and a note: the note is appended with the agent's mark.
    t.chat
        .board(
            chat_a,
            BoardRequest::Move {
                card_id: made.clone(),
                column: "in progress".into(),
                position: Some(0),
            },
        )
        .await
        .unwrap();
    t.chat
        .board(
            chat_a,
            BoardRequest::Note {
                card_id: made.clone(),
                text: "Started on it.".into(),
            },
        )
        .await
        .unwrap();
    let alpha = http_board(&t, &alex, &a).await;
    let card = &alpha["columns"][1]["cards"][0];
    assert_eq!(card["card_id"], made.as_str());
    assert_eq!(card["notes"], "[agent Assistant] Started on it.");
    let audit = t.master.inner_mirror().audit_lines("board.agent").unwrap();
    assert_eq!(audit.len(), 3, "{audit:?}");
    assert!(
        audit
            .iter()
            .all(|(_, d)| d.contains(&made) && !d.contains("Started"))
    );
}

/// A member whose role may not change projects lists the board but every
/// change is refused, offered or forged; the tool is not offered to them.
#[tokio::test]
async fn a_member_without_settings_lists_but_cannot_change() {
    let t = start().await;
    let [alex, bob] = users(&t).await;
    let (st, w) = req(
        &t,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "Acme"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{w}");
    let acme = w["workspace_id"].as_str().unwrap().to_string();
    let (st, v) = req(
        &t,
        Method::POST,
        &format!("/api/workspaces/{acme}/members"),
        Some(json!({"username": "bob", "role": "member"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let p = new_project(&t, &alex, json!({"name": "Launch", "workspace_id": acme})).await;
    let k = http_card(&t, &alex, &p, "Ship it", json!({})).await;
    let bobs = chat(&t, &bob, json!({"workspace_id": acme, "project_id": p})).await;
    landed(&t, bobs).await;
    running_turn(&t, bobs, "t1").await;
    let spec = last_spec(&t);
    assert!(spec.allows_tool("board"), "{:?}", spec.deny_tools);
    assert!(!spec.allows_tool("board_edit"), "{:?}", spec.deny_tools);
    // Listing works.
    let seen = cards(&t.chat.board(bobs, list()).await.unwrap());
    assert_eq!(seen.len(), 1);
    // Every change is refused (a forged request: the tool was withheld).
    for r in [
        add("Bob's card"),
        BoardRequest::Move {
            card_id: k.clone(),
            column: "Done".into(),
            position: None,
        },
        BoardRequest::Note {
            card_id: k.clone(),
            text: "x".into(),
        },
    ] {
        let e = t.chat.board(bobs, r).await.unwrap_err();
        assert!(e.contains("not available in this turn"), "{e}");
    }
    let b = http_board(&t, &alex, &p).await;
    assert_eq!(b["columns"][0]["cards"].as_array().unwrap().len(), 1);
    assert_eq!(b["columns"][0]["cards"][0]["notes"], "");
}

/// The permission is checked when the change is asked for, not only when
/// the turn started: a role lowered during the turn is refused.
#[tokio::test]
async fn the_permission_is_checked_per_request() {
    let t = start().await;
    let [alex, bob] = users(&t).await;
    let (_, w) = req(
        &t,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "Acme"})),
        &alex,
    )
    .await;
    let acme = w["workspace_id"].as_str().unwrap().to_string();
    let (st, v) = req(
        &t,
        Method::POST,
        &format!("/api/workspaces/{acme}/members"),
        Some(json!({"username": "bob", "role": "admin"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let p = new_project(&t, &alex, json!({"name": "Launch", "workspace_id": acme})).await;
    let bobs = chat(&t, &bob, json!({"workspace_id": acme, "project_id": p})).await;
    landed(&t, bobs).await;
    running_turn(&t, bobs, "t1").await;
    assert!(last_spec(&t).allows_tool("board_edit"));
    t.chat.board(bobs, add("While an admin")).await.unwrap();
    let bob_id = user_id(&t, &bob).await;
    let (st, v) = req(
        &t,
        Method::PATCH,
        &format!("/api/workspaces/{acme}/members/{bob_id}"),
        Some(json!({"role": "member"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let e = t.chat.board(bobs, add("After")).await.unwrap_err();
    assert!(e.contains("cannot change boards"), "{e}");
    // Still lists.
    assert_eq!(cards(&t.chat.board(bobs, list()).await.unwrap()).len(), 1);
}

async fn user_id(t: &T, c: &str) -> String {
    let (_, me) = req(t, Method::GET, "/api/me", None, c).await;
    me["user"]["user_id"].as_str().unwrap().to_string()
}

/// A chat outside a project is not offered the tools and its requests are
/// refused; nor is an untrusted (webhook) turn of a project chat.
#[tokio::test]
async fn a_chat_outside_a_project_has_no_board() {
    let t = start().await;
    let [alex, _bob] = users(&t).await;
    let _p = new_project(&t, &alex, json!({"name": "Alpha"})).await;
    let plain = chat(&t, &alex, json!({})).await;
    landed(&t, plain).await;
    running_turn(&t, plain, "t1").await;
    let spec = last_spec(&t);
    assert!(!spec.allows_tool("board") && !spec.allows_tool("board_edit"));
    let e = t.chat.board(plain, list()).await.unwrap_err();
    assert!(e.contains("not available"), "{e}");
    // An untrusted (webhook) turn in a project chat: not offered either.
    let p = new_project(&t, &alex, json!({"name": "Beta"})).await;
    let pc = chat(&t, &alex, json!({"project_id": p})).await;
    landed(&t, pc).await;
    t.master
        .prompt(
            pc,
            "from a webhook".into(),
            PromptOpts {
                untrusted: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let spec = last_spec(&t);
    assert!(!spec.allows_tool("board") && !spec.allows_tool("board_edit"));
}

/// A node that does not run the session is refused; so is an unknown one.
#[tokio::test]
async fn another_nodes_session_is_refused() {
    let t = start().await;
    let [alex, _bob] = users(&t).await;
    let p = new_project(&t, &alex, json!({"name": "Alpha"})).await;
    let a = chat(&t, &alex, json!({"project_id": p})).await;
    landed(&t, a).await;
    running_turn(&t, a, "t1").await;
    let e = t.other.board(a, add("forged")).await.unwrap_err();
    assert!(e.contains("no such session"), "{e}");
    let e = t.chat.board(SessionId::new(), list()).await.unwrap_err();
    assert!(e.contains("no such session"), "{e}");
    let b = http_board(&t, &alex, &p).await;
    assert!(b["columns"][0]["cards"].as_array().unwrap().is_empty());
}

/// A node without the capability is not offered the tools, and its
/// requests are refused.
#[tokio::test]
async fn a_node_without_the_capability() {
    let t = start_with(&[]).await;
    let [alex, _bob] = users(&t).await;
    let p = new_project(&t, &alex, json!({"name": "Alpha"})).await;
    let a = chat(&t, &alex, json!({"project_id": p})).await;
    landed(&t, a).await;
    running_turn(&t, a, "t1").await;
    let spec = last_spec(&t);
    assert!(!spec.allows_tool("board") && !spec.allows_tool("board_edit"));
    let e = t.chat.board(a, list()).await.unwrap_err();
    assert!(e.contains("does not offer"), "{e}");
}

/// An agent without the switch is refused, whatever its turn asks.
#[tokio::test]
async fn an_agent_without_the_switch_is_refused() {
    let t = start().await;
    let [alex, _bob] = users(&t).await;
    let p = new_project(&t, &alex, json!({"name": "Alpha"})).await;
    let a = chat(&t, &alex, json!({"project_id": p})).await;
    landed(&t, a).await;
    t.chat
        .emit(
            a,
            "turn_started",
            json!({"turn_id": "t1", "agent": {"agent_id": "researcher", "name": "Researcher",
                   "prompt_hash": "h"}}),
        )
        .await;
    landed(&t, a).await;
    let e = t.chat.board(a, list()).await.unwrap_err();
    assert!(e.contains("not available to this agent"), "{e}");
    let e = t.chat.board(a, add("x")).await.unwrap_err();
    assert!(e.contains("not available to this agent"), "{e}");
}

/// At most `WRITES_PER_TURN` changes a turn, refused past it with a clear
/// message; a new turn may change again.
#[tokio::test]
async fn changes_are_capped_per_turn() {
    let t = start().await;
    let [alex, _bob] = users(&t).await;
    let p = new_project(&t, &alex, json!({"name": "Alpha"})).await;
    let a = chat(&t, &alex, json!({"project_id": p})).await;
    landed(&t, a).await;
    running_turn(&t, a, "t1").await;
    for i in 0..harness_core::board::WRITES_PER_TURN {
        t.chat.board(a, add(&format!("card {i}"))).await.unwrap();
    }
    let e = t.chat.board(a, add("one too many")).await.unwrap_err();
    assert!(e.contains("the most one turn may"), "{e}");
    // Listing is counted apart.
    t.chat.board(a, list()).await.unwrap();
    t.chat
        .emit(
            a,
            "turn_ended",
            json!({"turn_id": "t1", "reason": "completed"}),
        )
        .await;
    running_turn(&t, a, "t2").await;
    t.chat.board(a, add("next turn")).await.unwrap();
    let b = http_board(&t, &alex, &p).await;
    assert_eq!(
        b["columns"][0]["cards"].as_array().unwrap().len(),
        harness_core::board::WRITES_PER_TURN as usize + 1
    );
}

/// An append that would take the notes past 4,000 characters is refused,
/// not cut, and the notes stay as they were.
#[tokio::test]
async fn an_append_past_the_limit_is_refused() {
    let t = start().await;
    let [alex, _bob] = users(&t).await;
    let p = new_project(&t, &alex, json!({"name": "Alpha"})).await;
    let notes = "n".repeat(3900);
    let k = http_card(&t, &alex, &p, "Full", json!({"notes": notes})).await;
    let a = chat(&t, &alex, json!({"project_id": p})).await;
    landed(&t, a).await;
    running_turn(&t, a, "t1").await;
    let e = t
        .chat
        .board(
            a,
            BoardRequest::Note {
                card_id: k.clone(),
                text: "m".repeat(200),
            },
        )
        .await
        .unwrap_err();
    assert!(e.contains("at most 4000"), "{e}");
    let b = http_board(&t, &alex, &p).await;
    assert_eq!(b["columns"][0]["cards"][0]["notes"], notes.as_str());
    // A short one fits.
    t.chat
        .board(
            a,
            BoardRequest::Note {
                card_id: k,
                text: "ok".into(),
            },
        )
        .await
        .unwrap();
}

/// `list` never shows another member's linked chat title (T10): a card
/// linked to Alex's chat is "another member's chat" in Bob's list, and the
/// title is nowhere in the answer; Alex's own list shows it.
#[tokio::test]
async fn another_members_chat_title_is_not_listed() {
    let t = start().await;
    let [alex, bob] = users(&t).await;
    let (_, w) = req(
        &t,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "Acme"})),
        &alex,
    )
    .await;
    let acme = w["workspace_id"].as_str().unwrap().to_string();
    req(
        &t,
        Method::POST,
        &format!("/api/workspaces/{acme}/members"),
        Some(json!({"username": "bob", "role": "member"})),
        &alex,
    )
    .await;
    let p = new_project(&t, &alex, json!({"name": "Launch", "workspace_id": acme})).await;
    let alexs = chat(&t, &alex, json!({"workspace_id": acme, "project_id": p})).await;
    landed(&t, alexs).await;
    let (st, v) = req(
        &t,
        Method::PUT,
        &format!("/api/sessions/{alexs}/title"),
        Some(json!({"title": "Pelican merger terms"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    http_card(
        &t,
        &alex,
        &p,
        "Linked",
        json!({"session_id": alexs.to_string()}),
    )
    .await;
    let bobs = chat(&t, &bob, json!({"workspace_id": acme, "project_id": p})).await;
    landed(&t, bobs).await;
    running_turn(&t, bobs, "t1").await;
    let a = t.chat.board(bobs, list()).await.unwrap();
    assert!(!format!("{a:?}").contains("Pelican"), "{a:?}");
    let BoardAnswer::Board { columns, .. } = &a else {
        panic!()
    };
    assert_eq!(columns[0].cards[0].chat, Some(CardChat::Private));
    assert!(!harness_tools::render_board("Launch", columns).contains("Pelican"));
    // Alex's own chat sees its title.
    let other = chat(&t, &alex, json!({"workspace_id": acme, "project_id": p})).await;
    landed(&t, other).await;
    running_turn(&t, other, "t2").await;
    let a = t.chat.board(other, list()).await.unwrap();
    let BoardAnswer::Board { columns, .. } = &a else {
        panic!()
    };
    assert_eq!(
        columns[0].cards[0].chat,
        Some(CardChat::Own {
            title: "Pelican merger terms".into()
        })
    );
}

/// The same request id twice (a resent or duplicated message) adds one
/// card; the second gets the first answer.
#[tokio::test]
async fn a_repeated_request_id_adds_one_card() {
    let t = start().await;
    let [alex, _bob] = users(&t).await;
    let p = new_project(&t, &alex, json!({"name": "Alpha"})).await;
    let a = chat(&t, &alex, json!({"project_id": p})).await;
    landed(&t, a).await;
    running_turn(&t, a, "t1").await;
    t.chat.send(a, "same", add("Once")).await;
    let first = t.chat.answers("same", 1).await;
    t.chat.send(a, "same", add("Once")).await;
    let both = t.chat.answers("same", 2).await;
    assert!(first[0].is_ok(), "{first:?}");
    assert_eq!(both[0], both[1]);
    // Two at once as well: one card more, not two.
    t.chat.send(a, "twice", add("Twice")).await;
    t.chat.send(a, "twice", add("Twice")).await;
    t.chat.answers("twice", 2).await;
    let b = http_board(&t, &alex, &p).await;
    let titles: Vec<&str> = b["columns"][0]["cards"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Once", "Twice"]);
}

/// A turn as the node records it: `turn_started` of `turn` for agent `id`,
/// with what was withheld and, for a callback turn, its cause.
async fn turn_as(t: &T, sid: SessionId, turn: &str, id: &str, withheld: &[&str], cause: Value) {
    t.chat
        .emit(
            sid,
            "turn_started",
            json!({"turn_id": turn, "cause": cause,
                   "agent": {"agent_id": id, "name": id, "prompt_hash": "h", "withheld": withheld}}),
        )
        .await;
    landed(t, sid).await;
}

/// A routine of `owner` in `ws` and a running run of it whose chat is
/// `sid`, as the routine runner records them; its run id.
fn routine_run(t: &T, owner: &str, ws: &str, sid: SessionId) -> String {
    use harness_store::routines::{NewRoutine, NewRun, RoutineCaps, RunPatch};
    let s = t.master.inner_mirror();
    let rid = harness_core::new_id();
    s.routine_create(
        &NewRoutine {
            routine_id: rid.clone(),
            workspace_id: ws.into(),
            owner_id: owner.into(),
            name: format!("r-{rid}"),
            enabled: true,
            spec: "{}".into(),
            triggers: vec![],
        },
        RoutineCaps {
            per_user: 50,
            per_workspace: 50,
        },
        1,
    )
    .unwrap();
    let run = harness_core::new_id();
    s.run_start(&NewRun {
        run_id: run.clone(),
        routine_id: rid,
        trigger_id: None,
        trigger_kind: "manual".into(),
        scheduled_ms: None,
        started_ms: 1,
        status: "pending".into(),
        reason: None,
        repeats: 1,
        event: None,
    })
    .unwrap();
    assert!(
        s.run_update(
            &run,
            &RunPatch {
                status: Some("running".into()),
                session_id: Some(sid.to_string()),
                ..Default::default()
            },
        )
        .unwrap()
    );
    run
}

/// The prompt a routine's run sends (`routines/runner.rs`), for run `run`.
fn routine_prompt(run: &str) -> PromptOpts {
    PromptOpts {
        agent_id: Some("general".into()),
        context: Some("Started by routine.".into()),
        routine_run: Some(run.into()),
        ..Default::default()
    }
}

/// Review M1 and the owner's decision of 2026-11-18 ("Routines may
/// change"): an API-token turn can list the board and not change it
/// (`board_edit` withheld, a direct request refused from the turn as the
/// log records it); a run of the owner's own routine changes it, capped at
/// 10 like any turn, and its audit lines name the routine; a run id the
/// master has no record of for this chat gives nothing.
#[tokio::test]
async fn routine_runs_change_the_board_and_api_token_turns_only_list_it() {
    let t = start().await;
    let [alex, _bob] = users(&t).await;
    let p = new_project(&t, &alex, json!({"name": "Alpha"})).await;
    let a = chat(&t, &alex, json!({"project_id": p})).await;
    landed(&t, a).await;
    // The API token's prompt.
    t.master
        .prompt(a, "from the api token".into(), PromptOpts::default())
        .await
        .unwrap();
    let spec = last_spec(&t);
    assert!(spec.allows_tool("board") && !spec.allows_tool("board_edit"));
    start_turn(&t, a, "t1").await;
    let e = t
        .chat
        .board(a, add("in the owner's name"))
        .await
        .unwrap_err();
    assert!(e.contains("not available in this turn"), "{e}");
    t.chat.board(a, list()).await.unwrap();
    t.chat
        .emit(
            a,
            "turn_ended",
            json!({"turn_id": "t1", "reason": "completed"}),
        )
        .await;
    // A run id the store does not record for this chat.
    t.master
        .prompt(a, "forged".into(), routine_prompt("no-such-run"))
        .await
        .unwrap();
    assert!(!last_spec(&t).allows_tool("board_edit"));
    // A run of the owner's routine, its chat this one.
    let ws = projects_ws(&t, &alex, &p).await;
    let me = user_id(&t, &alex).await;
    let run = routine_run(&t, &me, &ws, a);
    t.master
        .prompt(a, "run".into(), routine_prompt(&run))
        .await
        .unwrap();
    assert!(last_spec(&t).allows_tool("board_edit"));
    start_turn(&t, a, "t2").await;
    for i in 0..harness_core::board::WRITES_PER_TURN {
        t.chat.board(a, add(&format!("routine {i}"))).await.unwrap();
    }
    let e = t.chat.board(a, add("one too many")).await.unwrap_err();
    assert!(e.contains("the most one turn may"), "{e}");
    let audit = t.master.inner_mirror().audit_lines("board.agent").unwrap();
    assert_eq!(audit.len(), harness_core::board::WRITES_PER_TURN as usize);
    assert!(audit.iter().all(|(_, d)| d.contains(&run)), "{audit:?}");
}

/// A routine whose owner lost the `settings` permission: no `board_edit`
/// for its run, and a request is refused.
#[tokio::test]
async fn a_routine_of_an_owner_without_settings_is_refused() {
    let t = start().await;
    let [alex, bob] = users(&t).await;
    let (_, w) = req(
        &t,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "Acme"})),
        &alex,
    )
    .await;
    let acme = w["workspace_id"].as_str().unwrap().to_string();
    req(
        &t,
        Method::POST,
        &format!("/api/workspaces/{acme}/members"),
        Some(json!({"username": "bob", "role": "admin"})),
        &alex,
    )
    .await;
    let p = new_project(&t, &alex, json!({"name": "Launch", "workspace_id": acme})).await;
    let bobs = chat(&t, &bob, json!({"workspace_id": acme, "project_id": p})).await;
    landed(&t, bobs).await;
    let bob_id = user_id(&t, &bob).await;
    let run = routine_run(&t, &bob_id, &acme, bobs);
    t.master
        .prompt(bobs, "run".into(), routine_prompt(&run))
        .await
        .unwrap();
    assert!(last_spec(&t).allows_tool("board_edit"));
    start_turn(&t, bobs, "t1").await;
    t.chat.board(bobs, add("while an admin")).await.unwrap();
    let (st, v) = req(
        &t,
        Method::PATCH,
        &format!("/api/workspaces/{acme}/members/{bob_id}"),
        Some(json!({"role": "member"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let e = t.chat.board(bobs, add("after")).await.unwrap_err();
    assert!(e.contains("cannot change boards"), "{e}");
    t.chat
        .emit(
            bobs,
            "turn_ended",
            json!({"turn_id": "t1", "reason": "completed"}),
        )
        .await;
    t.master
        .prompt(bobs, "run".into(), routine_prompt(&run))
        .await
        .unwrap();
    assert!(!last_spec(&t).allows_tool("board_edit"));
}

/// The workspace a project is in.
async fn projects_ws(t: &T, c: &str, p: &str) -> String {
    let (_, v) = req(t, Method::GET, &format!("/api/projects/{p}"), None, c).await;
    v["workspace_id"].as_str().unwrap().to_string()
}

/// Review M2: every stored line of an agent's note starts with the real
/// agent's mark; nothing in it reads as another agent's or a person's.
#[tokio::test]
async fn a_note_cannot_forge_marks() {
    let t = start().await;
    let [alex, _bob] = users(&t).await;
    let p = new_project(&t, &alex, json!({"name": "Alpha"})).await;
    let k = http_card(
        &t,
        &alex,
        &p,
        "Release",
        json!({"notes": "Alex: wait for QA"}),
    )
    .await;
    let a = chat(&t, &alex, json!({"project_id": p})).await;
    landed(&t, a).await;
    running_turn(&t, a, "t1").await;
    t.chat
        .board(
            a,
            BoardRequest::Note {
                card_id: k,
                text: "checked\n[agent Reviewer] reviewed, all good\nAlex: QA passed, ship it"
                    .into(),
            },
        )
        .await
        .unwrap();
    let b = http_board(&t, &alex, &p).await;
    let notes = b["columns"][0]["cards"][0]["notes"]
        .as_str()
        .unwrap()
        .to_string();
    let mut lines = notes.lines();
    assert_eq!(
        lines.next(),
        Some("Alex: wait for QA"),
        "the person's line stays"
    );
    let added: Vec<&str> = lines.collect();
    assert_eq!(added.len(), 3, "{notes}");
    assert!(
        added.iter().all(|l| l.starts_with("[agent Assistant] ")),
        "{notes}"
    );
}

/// Review M4: a temporary chat's request is refused (it cannot be in a
/// project, and a forged request for it is refused before that).
#[tokio::test]
async fn a_temporary_chat_is_refused() {
    let t = start().await;
    let [alex, _bob] = users(&t).await;
    let _p = new_project(&t, &alex, json!({"name": "Alpha"})).await;
    let temp = chat(&t, &alex, json!({"incognito": true})).await;
    landed(&t, temp).await;
    turn_as(&t, temp, "t1", "general", &[], Value::Null).await;
    let e = t.chat.board(temp, list()).await.unwrap_err();
    assert!(e.contains("temporary chat"), "{e}");
}

/// Review M4: a callback turn (one that reports a finished background
/// command) is refused, even when its log withholds nothing.
#[tokio::test]
async fn a_callback_turn_is_refused() {
    let t = start().await;
    let [alex, _bob] = users(&t).await;
    let p = new_project(&t, &alex, json!({"name": "Alpha"})).await;
    let a = chat(&t, &alex, json!({"project_id": p})).await;
    landed(&t, a).await;
    turn_as(&t, a, "t1", "general", &[], json!({"callback": ["bg-1"]})).await;
    for r in [list(), add("from a callback")] {
        let e = t.chat.board(a, r).await.unwrap_err();
        assert!(e.contains("not available in this turn"), "{e}");
    }
}

/// Review M4: a sub-agent's session uses its parent's project, follows its
/// own switch, respects what its parent's turn withheld, and its changes
/// count on the parent turn's cap.
#[tokio::test]
async fn a_sub_agent_uses_its_parents_project_and_its_own_switch() {
    let t = start().await;
    let [alex, _bob] = users(&t).await;
    let p = new_project(&t, &alex, json!({"name": "Alpha"})).await;
    let a = chat(&t, &alex, json!({"project_id": p})).await;
    landed(&t, a).await;
    running_turn(&t, a, "t1").await;
    let kid = SessionId::new();
    t.chat
        .emit(
            kid,
            "session_created",
            json!({"title": "@coder", "cwd": "/w", "model": "m", "backend": "native",
                   "node_id": "master", "profile": "chat", "approval_mode": "auto",
                   "parent_session": a}),
        )
        .await;
    landed(&t, kid).await;
    // Its own switch: Scout has none, whatever its parent has.
    turn_as(&t, kid, "k1", "scout", &[], Value::Null).await;
    let e = t.chat.board(kid, list()).await.unwrap_err();
    assert!(e.contains("not available to this agent"), "{e}");
    // The developer has it: the parent's project's board.
    turn_as(&t, kid, "k2", "coder", &[], Value::Null).await;
    let made = card_id(&t.chat.board(kid, add("from the sub-agent")).await.unwrap());
    let seen = cards(&t.chat.board(kid, list()).await.unwrap());
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].1, made);
    let b = http_board(&t, &alex, &p).await;
    assert_eq!(
        b["columns"][0]["cards"][0]["chat"]["session_id"],
        a.to_string()
    );
    // Its changes count on the parent's turn: 1 made, 9 more by the parent.
    for i in 0..harness_core::board::WRITES_PER_TURN - 1 {
        t.chat.board(a, add(&format!("parent {i}"))).await.unwrap();
    }
    let e = t.chat.board(kid, add("one too many")).await.unwrap_err();
    assert!(e.contains("the most one turn may"), "{e}");
    // A parent turn that withheld `board_edit` binds the sub-agent.
    t.chat
        .emit(
            a,
            "turn_ended",
            json!({"turn_id": "t1", "reason": "completed"}),
        )
        .await;
    turn_as(&t, a, "t2", "general", &["board_edit"], Value::Null).await;
    let e = t
        .chat
        .board(kid, add("parent withheld it"))
        .await
        .unwrap_err();
    assert!(e.contains("not available in this turn"), "{e}");
    t.chat.board(kid, list()).await.unwrap();
}
