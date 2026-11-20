//! `recall` on the master (PLAN.md T55): a node asks for the session it
//! runs; the master decides the scope from that session alone. The nodes
//! here are fakes the test drives, so a request can be forged at will.

use crate::common;
use common::{Login as _, SameOrigin as _};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::recall::{HitKind, RecallHit, RecallIn, RecallQuery};
use harness_core::{RawEvent, Secret, SessionId};
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, PromptOpts, router};
use harness_memory::SqliteMemory;
use harness_node::NodeLink;
use harness_protocol::{MasterToNode, NodeToMaster, PROTOCOL_VERSION, PromptMsg};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

type Answers = Arc<Mutex<HashMap<String, (Vec<RecallHit>, Option<String>)>>>;
type Seen = Arc<Mutex<Vec<MasterToNode>>>;

/// A fake node: acks every command, mirrors a `session_created` for each
/// chat it is asked to create, keeps the seq of each session so the test can
/// append events, and records each `recall.result`.
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
            if let MasterToNode::RecallResult {
                req_id,
                hits,
                error,
            } = &msg
            {
                answers
                    .lock()
                    .unwrap()
                    .insert(req_id.clone(), (hits.clone(), error.clone()));
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

    /// A finished exchange: the person said `said`, the agent answered.
    async fn exchange(&self, sid: SessionId, turn: &str, said: &str) {
        self.emit(sid, "turn_started", json!({"turn_id": turn}))
            .await;
        self.emit(sid, "user_message", json!({"turn_id": turn, "text": said}))
            .await;
        self.emit(
            sid,
            "assistant_message",
            json!({"turn_id": turn, "message_id": format!("m-{turn}"), "text": "Noted."}),
        )
        .await;
        self.emit(
            sid,
            "turn_ended",
            json!({"turn_id": turn, "reason": "completed"}),
        )
        .await;
    }

    /// Ask on behalf of `sid` and wait for the answer.
    async fn recall(&self, sid: SessionId, query: &str) -> Result<Vec<RecallHit>, String> {
        let req_id = harness_core::new_id();
        self.tx
            .send(NodeToMaster::Recall {
                req_id: req_id.clone(),
                session_id: sid,
                query: RecallQuery {
                    query: query.into(),
                    scope: RecallIn::Both,
                    limit: None,
                },
            })
            .await
            .unwrap();
        for _ in 0..200 {
            if let Some((hits, error)) = self.answers.lock().unwrap().remove(&req_id) {
                return match error {
                    Some(e) => Err(e),
                    None => Ok(hits),
                };
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no recall.result for {req_id}");
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
    master.set_memory(Arc::new(SqliteMemory::open_in_memory().unwrap()));
    let (link, chat) = fake_node(
        "master",
        &[
            "profile:chat",
            harness_protocol::CAP_REMEMBER,
            harness_protocol::CAP_RECALL,
        ],
    );
    master.add_link(link, true).await.unwrap();
    let (link, other) = fake_node("other", &[harness_protocol::CAP_RECALL, "exec:sandbox"]);
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
            "what did we decide?".into(),
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
            MasterToNode::Prompt(PromptMsg {
                session_id, agent, ..
            }) if *session_id == sid => agent.as_ref().map(|a| a.agent_ref()),
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

fn chat_ids(hits: &[RecallHit]) -> Vec<String> {
    hits.iter()
        .filter(|h| h.kind == HitKind::Chat)
        .map(|h| h.id.clone())
        .collect()
}

/// A fact said in one chat is found from another chat of the same owner in
/// the same workspace; nothing of another member's chats, of a chat in
/// another workspace, of a temporary chat or of the asking chat itself.
#[tokio::test]
async fn finds_the_owners_own_earlier_chats_only() {
    let t = start().await;
    let [alex, bob] = users(&t).await;
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "team"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let team = v["workspace_id"].as_str().unwrap().to_string();

    let a = chat(&t, &alex, json!({})).await;
    t.chat
        .exchange(a, "t1", "We chose the zebrafish codename for the launch.")
        .await;
    let elsewhere = chat(&t, &alex, json!({"workspace_id": team})).await;
    t.chat
        .exchange(elsewhere, "t2", "The zebrafish budget is secret.")
        .await;
    let temp = chat(&t, &alex, json!({"incognito": true})).await;
    t.chat
        .exchange(temp, "t3", "zebrafish temporary note")
        .await;
    let bobs = chat(&t, &bob, json!({})).await;
    t.chat
        .exchange(bobs, "t4", "Bob thinks zebrafish is a bad codename.")
        .await;
    let b = chat(&t, &alex, json!({})).await;
    t.chat
        .exchange(b, "t5", "zebrafish in the asking chat")
        .await;
    for s in [a, elsewhere, temp, bobs, b] {
        landed(&t, s).await;
    }

    // Outside a turn: refused.
    assert!(t.chat.recall(b, "zebrafish codename").await.is_err());

    running_turn(&t, b, "t6").await;
    let hits = t.chat.recall(b, "zebrafish codename").await.unwrap();
    assert_eq!(chat_ids(&hits), [a.to_string()], "{hits:?}");
    let a_hit = &hits[0];
    assert!(a_hit.excerpt.contains("codename"), "{a_hit:?}");
    assert!(a_hit.when_ms > 0);

    // The same words from Bob's running turn find only Bob's chat... none
    // of his other chats exist, and never Alex's.
    running_turn(&t, bobs, "t7").await;
    let hits = t.chat.recall(bobs, "zebrafish codename").await.unwrap();
    assert!(chat_ids(&hits).is_empty(), "{hits:?}");
}

/// A node that does not run the session cannot search for it; nor can a
/// request name another user or workspace (the query has no such field).
#[tokio::test]
async fn forged_requests_are_refused() {
    let t = start().await;
    let [alex, _bob] = users(&t).await;
    let a = chat(&t, &alex, json!({})).await;
    t.chat
        .exchange(a, "t1", "The kiwi deploy key rotates monthly.")
        .await;
    let b = chat(&t, &alex, json!({})).await;
    landed(&t, a).await;
    running_turn(&t, b, "t2").await;
    // Another node names Alex's running chat.
    let e = t.other.recall(b, "kiwi deploy").await.unwrap_err();
    assert!(e.contains("no such session"), "{e}");
    // An unknown session.
    let e = t.chat.recall(SessionId::new(), "kiwi").await.unwrap_err();
    assert!(e.contains("no such session"), "{e}");
    // The query takes words only.
    for forged in [
        json!({"query": "kiwi", "user": "bob"}),
        json!({"query": "kiwi", "workspace_id": "w"}),
        json!({"query": "kiwi", "session_id": a.to_string()}),
    ] {
        assert!(serde_json::from_value::<RecallQuery>(forged).is_err());
    }
    // The owner's own chat still answers.
    assert_eq!(
        chat_ids(&t.chat.recall(b, "kiwi deploy").await.unwrap()),
        [a.to_string()]
    );
}

/// Turns that may not search are not offered the tool, and their requests
/// are refused: untrusted (webhook) turns, temporary chats; an agent
/// without the switch is not offered it either.
#[tokio::test]
async fn untrusted_turns_temporary_chats_and_agents_without_the_switch() {
    let t = start().await;
    let [alex, _bob] = users(&t).await;
    let a = chat(&t, &alex, json!({})).await;
    t.chat
        .exchange(a, "t1", "The heron server lives in Oslo.")
        .await;
    landed(&t, a).await;
    let last_spec = |t: &T| {
        t.chat
            .seen
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|m| match m {
                MasterToNode::Prompt(PromptMsg { agent, context, .. }) => Some((
                    agent.clone().unwrap(),
                    context.as_ref().map(|c| c.joined()).unwrap_or_default(),
                )),
                _ => None,
            })
            .unwrap()
    };

    let b = chat(&t, &alex, json!({})).await;
    // Trusted, with the switch: offered, and told in one line.
    running_turn(&t, b, "t2").await;
    let (spec, ctx) = last_spec(&t);
    assert!(spec.allows_tool("recall"), "{spec:?}");
    assert!(ctx.contains("`recall`"), "{ctx}");
    assert!(t.chat.recall(b, "heron server").await.is_ok());

    // Untrusted: withheld (and from sub-agents), and refused if asked.
    t.master
        .prompt(
            b,
            "event text".into(),
            PromptOpts {
                agent_id: Some("teamlead".into()),
                untrusted: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let (spec, ctx) = last_spec(&t);
    assert!(!spec.allows_tool("recall"));
    assert!(spec.delegates.iter().all(|d| !d.allows_tool("recall")));
    assert!(!ctx.contains("`recall`"), "{ctx}");
    // Its turn, once it runs, is refused: the log records `recall` withheld.
    t.chat
        .emit(
            b,
            "turn_ended",
            json!({"turn_id": "t2", "reason": "completed"}),
        )
        .await;
    start_turn(&t, b, "w").await;
    let e = t.chat.recall(b, "heron server").await.unwrap_err();
    assert!(e.contains("not available in this turn"), "{e}");

    // Smol has no switch: not offered.
    t.master
        .prompt(
            b,
            "hi".into(),
            PromptOpts {
                agent_id: Some("smol".into()),
                by_owner: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let (spec, ctx) = last_spec(&t);
    assert!(!spec.allows_tool("recall"));
    assert!(!ctx.contains("`recall`"), "{ctx}");
    t.chat
        .emit(
            b,
            "turn_ended",
            json!({"turn_id": "w", "reason": "completed"}),
        )
        .await;
    start_turn(&t, b, "s").await;
    let e = t.chat.recall(b, "heron server").await.unwrap_err();
    assert!(e.contains("not available to this agent"), "{e}");

    // A temporary chat: withheld, refused.
    let temp = chat(&t, &alex, json!({"incognito": true})).await;
    running_turn(&t, temp, "t3").await;
    let (spec, _) = last_spec(&t);
    assert!(!spec.allows_tool("recall"));
    assert!(t.chat.recall(temp, "heron server").await.is_err());
}

/// Per-turn cap: the ninth search of one turn is refused; a new turn
/// starts again.
#[tokio::test]
async fn searches_are_capped_per_turn() {
    let t = start().await;
    let [alex, _bob] = users(&t).await;
    let b = chat(&t, &alex, json!({})).await;
    running_turn(&t, b, "t1").await;
    for _ in 0..harness_master::recall::PER_TURN {
        t.chat.recall(b, "anything").await.unwrap();
    }
    let e = t.chat.recall(b, "anything").await.unwrap_err();
    assert!(e.contains("already searched"), "{e}");
    t.chat
        .emit(
            b,
            "turn_ended",
            json!({"turn_id": "t1", "reason": "completed"}),
        )
        .await;
    running_turn(&t, b, "t2").await;
    assert!(t.chat.recall(b, "anything").await.is_ok());
}

/// Memory comes from the places the chat reads anyway: the owner's facts
/// are found from their chat, never another member's.
#[tokio::test]
async fn memory_hits_are_the_owners() {
    let t = start().await;
    let [alex, bob] = users(&t).await;
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/memory",
        Some(json!({"text": "Alex waters the ferns on Fridays"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/memory",
        Some(json!({"text": "Bob waters the ferns on Mondays"})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let b = chat(&t, &alex, json!({})).await;
    running_turn(&t, b, "t1").await;
    let hits = t.chat.recall(b, "ferns watered").await.unwrap();
    let facts: Vec<_> = hits.iter().filter(|h| h.kind == HitKind::Memory).collect();
    assert!(
        facts.iter().any(|h| h.excerpt.contains("Fridays")),
        "{hits:?}"
    );
    assert!(
        !hits.iter().any(|h| h.excerpt.contains("Mondays")),
        "{hits:?}"
    );
}

/// Review R1 (P1): an untrusted (webhook) turn runs and the owner queues a
/// prompt behind it: the running untrusted turn still may not search.
#[tokio::test]
async fn a_queued_prompt_does_not_open_a_running_untrusted_turn() {
    let t = start().await;
    let [alex, _bob] = users(&t).await;
    let a = chat(&t, &alex, json!({})).await;
    t.chat
        .exchange(a, "t1", "The heron server lives in Oslo.")
        .await;
    landed(&t, a).await;
    let b = chat(&t, &alex, json!({})).await;
    let untrusted = PromptOpts {
        untrusted: true,
        ..Default::default()
    };
    t.master
        .prompt(b, "event text".into(), untrusted)
        .await
        .unwrap();
    start_turn(&t, b, "w").await;
    assert!(t.chat.recall(b, "heron server").await.is_err());
    let owner = PromptOpts {
        by_owner: true,
        ..Default::default()
    };
    t.master.prompt(b, "later".into(), owner).await.unwrap();
    let e = t.chat.recall(b, "heron server").await.unwrap_err();
    assert!(e.contains("not available in this turn"), "{e}");
}

/// Review R6 (P2): a queued untrusted prompt does not take the tool from
/// the granted turn that is running.
#[tokio::test]
async fn a_queued_untrusted_prompt_leaves_the_running_turn_its_grant() {
    let t = start().await;
    let [alex, _bob] = users(&t).await;
    let a = chat(&t, &alex, json!({})).await;
    t.chat
        .exchange(a, "t1", "The heron server lives in Oslo.")
        .await;
    landed(&t, a).await;
    let b = chat(&t, &alex, json!({})).await;
    running_turn(&t, b, "t2").await;
    assert!(t.chat.recall(b, "heron server").await.is_ok());
    let untrusted = PromptOpts {
        untrusted: true,
        ..Default::default()
    };
    t.master.prompt(b, "event".into(), untrusted).await.unwrap();
    assert_eq!(
        chat_ids(&t.chat.recall(b, "heron server").await.unwrap()),
        [a.to_string()]
    );
}

/// Review R1 (P3): a grant does not outlive its turn: a turn the master
/// never prompted (no agent recorded) is refused, as is the time between
/// turns.
#[tokio::test]
async fn a_grant_does_not_outlive_its_turn() {
    let t = start().await;
    let [alex, _bob] = users(&t).await;
    let a = chat(&t, &alex, json!({})).await;
    t.chat
        .exchange(a, "t1", "The heron server lives in Oslo.")
        .await;
    landed(&t, a).await;
    let b = chat(&t, &alex, json!({})).await;
    running_turn(&t, b, "t2").await;
    t.chat
        .emit(
            b,
            "turn_ended",
            json!({"turn_id": "t2", "reason": "completed"}),
        )
        .await;
    landed(&t, b).await;
    assert!(t.chat.recall(b, "heron server").await.is_err());
    t.chat
        .emit(b, "turn_started", json!({"turn_id": "t3"}))
        .await;
    landed(&t, b).await;
    let e = t.chat.recall(b, "heron server").await.unwrap_err();
    assert!(e.contains("not available to this agent"), "{e}");
}

/// Review R5 (P4): natural questions and other languages find the chat;
/// query syntax has no effect; a query with no words is refused and not
/// counted.
#[tokio::test]
async fn natural_queries_find_the_chat() {
    let t = start().await;
    let [alex, _bob] = users(&t).await;
    let a = chat(&t, &alex, json!({})).await;
    t.chat
        .exchange(a, "t1", "We made the decision: deploy the Größe service to staging; деплой в пятницу. 東京サーバー. Cost was $1,200 (approx).")
        .await;
    landed(&t, a).await;
    let b = chat(&t, &alex, json!({})).await;
    running_turn(&t, b, "t2").await;
    let mut n = 0;
    for q in [
        "what decisions did we make about deploying?",
        "decision deploy",
        "decisions",
        "deploying staging",
        "\"deploy the\" AND NOT staging",
        "staging OR (x) NEAR(a b) ^col: * -y",
        "Größe",
        "деплой",
        "пятница",
        "東京",
        "1,200 cost",
    ] {
        if n == harness_master::recall::PER_TURN {
            n = 0;
            running_turn(&t, b, &format!("t{q}")).await;
        }
        n += 1;
        let hits = t.chat.recall(b, q).await.unwrap();
        assert_eq!(chat_ids(&hits), [a.to_string()], "{q:?}");
    }
    // No words: refused, and not counted.
    running_turn(&t, b, "t-empty").await;
    for _ in 0..harness_master::recall::PER_TURN + 2 {
        let e = t.chat.recall(b, "a* ?").await.unwrap_err();
        assert!(e.contains("distinctive words"), "{e}");
    }
    // Only common words: searched by them rather than refused.
    t.chat.recall(b, "what is it?").await.unwrap();
    for _ in 1..harness_master::recall::PER_TURN {
        t.chat.recall(b, "heron").await.unwrap();
    }
    let e = t.chat.recall(b, "heron").await.unwrap_err();
    assert!(e.contains("already searched"), "{e}");
}

/// Review R7: a long asking chat does not crowd the others out.
#[tokio::test]
async fn a_long_asking_chat_does_not_crowd_out_the_others() {
    let t = start().await;
    let [alex, _bob] = users(&t).await;
    let a = chat(&t, &alex, json!({})).await;
    t.chat.exchange(a, "t1", "pelican budget").await;
    landed(&t, a).await;
    let b = chat(&t, &alex, json!({})).await;
    for i in 0..harness_store::SEARCH_WINDOW + 20 {
        t.chat
            .emit(
                b,
                "user_message",
                json!({"turn_id": "x", "text": format!("pelican pelican note {i}")}),
            )
            .await;
    }
    running_turn(&t, b, "t2").await;
    assert_eq!(
        chat_ids(&t.chat.recall(b, "pelican").await.unwrap()),
        [a.to_string()]
    );
}

async fn user_id(t: &T, c: &str) -> String {
    let (_, me) = req(t, Method::GET, "/api/me", None, c).await;
    me["user"]["user_id"].as_str().unwrap().to_string()
}

async fn personal_ws(t: &T, c: &str) -> String {
    let (_, v) = req(t, Method::GET, "/api/workspaces", None, c).await;
    v["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["personal"] == true)
        .unwrap()["workspace_id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn memory_texts(hits: &[RecallHit]) -> Vec<String> {
    let mut v: Vec<String> = hits
        .iter()
        .filter(|h| h.kind == HitKind::Memory)
        .map(|h| h.excerpt.clone())
        .collect();
    v.sort();
    v
}

/// Review R4: memory comes from exactly the places the turn reads
/// (`memory::turn_places`): a team member gets the team's shared memory,
/// never a colleague's own facts or Personal workspace, nor their own
/// facts after answering "no"; a Personal chat gets the person's own facts
/// and their private-details place; retired facts and waiting proposals
/// never come back.
#[tokio::test]
async fn memory_comes_from_the_places_the_turn_reads() {
    use harness_memory::{FactKind, NewFact, Status};
    let t = start().await;
    let [alex, bob] = users(&t).await;
    let (alex_id, bob_id) = (user_id(&t, &alex).await, user_id(&t, &bob).await);
    let (alex_home, bob_home) = (personal_ws(&t, &alex).await, personal_ws(&t, &bob).await);
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "team"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let team = v["workspace_id"].as_str().unwrap().to_string();
    let (st, v) = req(
        &t,
        Method::POST,
        &format!("/api/workspaces/{team}/members"),
        Some(json!({"username": "bob", "role": "member"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // Bob keeps his own facts out of the team.
    let (st, v) = req(
        &t,
        Method::PUT,
        &format!("/api/workspaces/{team}/memory"),
        Some(json!({"use_memory": false})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let mem = t.master.memory().unwrap();
    for (owner, text, status) in [
        (
            format!("workspace:{team}"),
            "heron team shared",
            Status::Active,
        ),
        (
            format!("workspace:{team}"),
            "heron team retired",
            Status::Retired,
        ),
        (
            format!("workspace:{team}"),
            "heron team proposal",
            Status::Proposed,
        ),
        (alex_id.clone(), "heron alex own", Status::Active),
        (
            format!("workspace:{alex_home}"),
            "heron alex private",
            Status::Active,
        ),
        (bob_id.clone(), "heron bob own", Status::Active),
        (bob_id.clone(), "heron bob proposal", Status::Proposed),
        (
            format!("workspace:{bob_home}"),
            "heron bob private",
            Status::Active,
        ),
    ] {
        mem.add(NewFact {
            owner_id: Some(owner),
            text: text.into(),
            kind: FactKind::Fact,
            status,
            source_session: None,
            dedupe_key: None,
        })
        .await
        .unwrap();
    }
    let bobs = chat(&t, &bob, json!({"workspace_id": team})).await;
    running_turn(&t, bobs, "t1").await;
    let hits = t.chat.recall(bobs, "heron").await.unwrap();
    assert_eq!(memory_texts(&hits), ["heron team shared"], "{hits:?}");

    let home = chat(&t, &alex, json!({})).await;
    running_turn(&t, home, "t2").await;
    let hits = t.chat.recall(home, "heron").await.unwrap();
    assert_eq!(
        memory_texts(&hits),
        ["heron alex own", "heron alex private"],
        "{hits:?}"
    );
}
