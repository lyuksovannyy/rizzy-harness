//! A.19 re-check: is there any bound on how often one routine run is put
//! back? Not part of the test suite: `run_rust_proofs.sh loop` copies this
//! to `crates/master/tests/` for the run and removes the copy.
//!
//! A node that acks every command and ends every turn it is given with
//! `turn_ended{reason: "stopped"}` (a hostile node, or one with a bug; here
//! a protocol-level fake standing in for the chat runtime, as
//! `hooks_http.rs` uses one, so that no grants are needed: `finalize` and
//! `put_back` are the same code for every node). One "run now"; then count
//! the prompts and the chats for five seconds.

mod common;
use common::{Login as _, SameOrigin as _};

use std::collections::HashMap;
use std::sync::atomic::AtomicI64;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use harness_core::{RawEvent, Secret, SessionId};
use harness_master::auth::{Auth, Claim};
use harness_master::routines::{self, Clock};
use harness_master::{Master, router};
use harness_node::NodeLink;
use harness_protocol::{MasterToNode, NodeToMaster, PROTOCOL_VERSION};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

#[derive(Clone)]
struct Fake {
    to_master: mpsc::Sender<NodeToMaster>,
    seqs: Arc<Mutex<HashMap<SessionId, u64>>>,
    prompts: Arc<Mutex<usize>>,
    creates: Arc<Mutex<usize>>,
}

impl Fake {
    async fn emit(&self, sid: SessionId, kind: &str, payload: Value) {
        let seq = {
            let mut seqs = self.seqs.lock().unwrap();
            let n = seqs.entry(sid).or_default();
            *n += 1;
            *n
        };
        let event = RawEvent {
            session_id: sid,
            seq,
            ts_ms: seq as i64,
            kind: kind.into(),
            payload,
        };
        let _ = self.to_master.send(NodeToMaster::Event { event }).await;
    }
}

/// The node: every turn it is given ends `stopped` at once.
fn stopping_node() -> (NodeLink, Fake) {
    let (to_node, mut commands) = mpsc::channel::<MasterToNode>(1024);
    let (to_master, from_node) = mpsc::channel::<NodeToMaster>(4096);
    to_master
        .try_send(NodeToMaster::Hello {
            protocol_version: PROTOCOL_VERSION,
            node_id: "master".into(),
            hostname: "h".into(),
            version: "0".into(),
            capabilities: vec!["profile:chat".into()],
            sessions: vec![],
        })
        .unwrap();
    let fake = Fake {
        to_master,
        seqs: Arc::default(),
        prompts: Arc::default(),
        creates: Arc::default(),
    };
    let node = fake.clone();
    tokio::spawn(async move {
        while let Some(msg) = commands.recv().await {
            let req_id = msg.req_id().map(str::to_string);
            let mut then: Option<SessionId> = None;
            match &msg {
                MasterToNode::Create {
                    session_id, title, ..
                } => {
                    *node.creates.lock().unwrap() += 1;
                    let payload = json!({"title": title, "cwd": "", "model": "m", "backend": "native",
                        "node_id": "master", "profile": "chat", "approval_mode": "ask"});
                    node.emit(*session_id, "session_created", payload).await;
                }
                MasterToNode::Prompt { session_id, .. } => {
                    *node.prompts.lock().unwrap() += 1;
                    then = Some(*session_id);
                }
                _ => {}
            }
            if let Some(req_id) = req_id {
                let ack = NodeToMaster::Ack {
                    req_id,
                    ok: true,
                    error: None,
                };
                let _ = node.to_master.send(ack).await;
            }
            if let Some(sid) = then {
                let turn = harness_core::new_id();
                node.emit(sid, "turn_started", json!({"turn_id": turn})).await;
                node.emit(
                    sid,
                    "turn_ended",
                    json!({"turn_id": turn, "reason": "stopped"}),
                )
                .await;
            }
        }
    });
    (NodeLink { to_node, from_node }, fake)
}

async fn req(base: &str, method: Method, path: &str, body: Option<Value>, c: &str) -> (StatusCode, Value) {
    let mut rb = reqwest::Client::new()
        .request(method, format!("{base}{path}"))
        .login(c)
        .same_origin();
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    let r = rb.send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_node_that_ends_every_turn_stopped_gets_the_same_run_for_ever() {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let path = dir.path().join("claim-code");
    std::fs::write(&path, "X").unwrap();
    let auth = Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false).with_claim(Claim {
        code: Secret::new("CLAIM".into()),
        path,
    });
    let master = Master::new(Store::open_in_memory().unwrap(), auth);
    master.set_agents(harness_agents::AgentSet::load(&dir.path().join("none")).0);
    let (link, fake) = stopping_node();
    master.add_link(link, true).await.unwrap();
    let clock = Arc::new(AtomicI64::new(
        "2026-11-08T09:30:00Z".parse::<jiff::Timestamp>().unwrap().as_millisecond(),
    ));
    routines::start_with(&master, Clock::manual(clock.clone()), false);
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    let base = format!("http://{addr}");

    let body = json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"});
    let r = reqwest::Client::new()
        .post(format!("{base}/api/setup"))
        .json(&body)
        .send()
        .await
        .unwrap();
    let alex = common::signed_in(r).await;
    let spec = json!({"profile": "chat", "agent_id": "general", "prompt": "hello"});
    let body = json!({"name": "Loop", "spec": spec, "triggers": [{"kind": "webhook", "config": {}}]});
    let (st, v) = req(&base, Method::POST, "/api/routines", Some(body), &alex).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let id = v["routine"]["routine_id"].as_str().unwrap().to_string();
    let (st, v) = req(&base, Method::POST, &format!("/api/routines/{id}/run"), Some(json!({})), &alex).await;
    assert_eq!(st, StatusCode::ACCEPTED, "{v}");
    let run = v["run_id"].as_str().unwrap().to_string();

    let t = Instant::now();
    for at in [1u64, 2, 3, 4, 5] {
        tokio::time::sleep(Duration::from_secs(at).saturating_sub(t.elapsed())).await;
        let (_, v) = req(&base, Method::GET, &format!("/api/routines/{id}/runs/{run}"), None, &alex).await;
        let (_, list) = req(&base, Method::GET, &format!("/api/routines/{id}/runs?limit=200"), None, &alex).await;
        println!(
            "after {at} s: prompts sent for the one run: {}; chats made: {}; the run: {} attempt {}; runs listed: {}",
            fake.prompts.lock().unwrap(),
            fake.creates.lock().unwrap(),
            v["status"],
            v["attempt"],
            list["runs"].as_array().map_or(0, Vec::len),
        );
    }
    let (_, r) = req(&base, Method::GET, &format!("/api/routines/{id}"), None, &alex).await;
    println!(
        "the routine: paused_reason {}, enabled {}",
        r["routine"]["paused_reason"], r["routine"]["enabled"]
    );
    let n = *fake.prompts.lock().unwrap();
    println!("{n} launches of one run in 5 s; nothing counted, nothing stops it");
}
