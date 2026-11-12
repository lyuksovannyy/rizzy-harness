//! A node with another protocol version is told why, before `Accepted`
//! (ADOPTION_PLAN A.19, docs/node-update.md §8.3): the master compares the
//! version the node's `auth` hello declares once the node proved its key,
//! and answers `rejected` naming both versions and which side is older. A
//! peer that has not proved a key learns nothing of the master's version.
//!
//! The frames are written as JSON by hand, so the test speaks for a node of
//! any build (one before A.19 sends no version at all).

use std::sync::Arc;
use std::time::Duration;

use harness_core::Secret;
use harness_link::client::join;
use harness_link::wire::{ClientStream, connect_with, recv_json, send_json};
use harness_link::{MasterCert, NodeKey, Timing};
use harness_master::Master;
use harness_master::auth::Auth;
use harness_protocol::PROTOCOL_VERSION;
use harness_store::Store;
use serde_json::{Value, json};
use tokio::net::TcpListener;

struct Env {
    master: Master,
    url: String,
    fp: String,
    dir: tempfile::TempDir,
}

async fn master() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let master = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("tok".into()), vec![], false),
    );
    master.set_agents(harness_agents::AgentSet::load(&dir.path().join("none")).0);
    let cert = Arc::new(MasterCert::load_or_create(dir.path()).unwrap());
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let fp = cert.fingerprint();
    master.set_node_listener(port, fp.clone());
    master.serve_nodes(l, cert).unwrap();
    Env {
        master,
        url: format!("wss://127.0.0.1:{port}"),
        fp,
        dir,
    }
}

/// One handshake with `extra` merged into the `auth` hello, signed with
/// `key`: the master's answer to it, and the stream (for what follows).
async fn auth(env: &Env, node_id: &str, key: &NodeKey, extra: Value) -> (Value, ClientStream) {
    let mut ws = connect_with(&env.url, &env.fp, Timing::default())
        .await
        .unwrap();
    let challenge: Value = recv_json(&mut ws).await.unwrap().unwrap();
    assert_eq!(challenge["type"], "challenge", "{challenge}");
    let nonce = challenge["nonce"].as_str().unwrap();
    let signature = key.sign_hex(&harness_link::identity::auth_message(nonce, &env.fp));
    let mut hello = json!({"type": "auth", "node_id": node_id, "signature": signature});
    for (k, v) in extra.as_object().unwrap() {
        hello[k] = v.clone();
    }
    send_json(&mut ws, &hello).await.unwrap();
    let answer: Value = tokio::time::timeout(Duration::from_secs(10), recv_json(&mut ws))
        .await
        .expect("no answer to the hello")
        .unwrap()
        .unwrap_or(Value::Null);
    (answer, ws)
}

async fn joined(env: &Env, name: &str) -> (String, NodeKey) {
    let key = NodeKey::load_or_create(&env.dir.path().join(format!("{name}.key"))).unwrap();
    let token = env.master.create_join_token(None).await.unwrap();
    let id = join(&env.url, &env.fp, &token, name, &key).await.unwrap();
    (id, key)
}

#[tokio::test]
async fn a_protocol_mismatch_is_refused_before_accepted_with_the_reason() {
    let env = master().await;
    let (id, key) = joined(&env, "box").await;
    let newer = PROTOCOL_VERSION + 1;
    let (answer, _ws) = auth(&env, &id, &key, json!({"protocol_version": newer})).await;
    assert_eq!(answer["type"], "rejected", "answered {answer}");
    let reason = answer["reason"].as_str().unwrap();
    assert!(
        reason.contains(&format!("protocol {newer}"))
            && reason.contains(&format!("protocol {PROTOCOL_VERSION}")),
        "both versions are named: {reason}"
    );
    assert!(
        reason.contains("the master is older"),
        "the older side is named: {reason}"
    );

    // An older node: the other side is named.
    let older = PROTOCOL_VERSION - 1;
    let (answer, _ws) = auth(&env, &id, &key, json!({"protocol_version": older})).await;
    assert_eq!(answer["type"], "rejected", "answered {answer}");
    let reason = answer["reason"].as_str().unwrap();
    assert!(reason.contains("this node is older"), "{reason}");
    assert!(reason.contains(&format!("protocol {older}")), "{reason}");

    // It was never linked.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !env.master.nodes().iter().any(|n| n.node_id == id),
        "a refused node was registered as a link"
    );

    // The same node with the master's version is accepted.
    let (answer, _ws) = auth(
        &env,
        &id,
        &key,
        json!({"protocol_version": PROTOCOL_VERSION}),
    )
    .await;
    assert_eq!(answer["type"], "accepted", "answered {answer}");
}

/// A node built before the field existed sends no version: it speaks
/// protocol 1, the only one there was, and is taken as such.
#[tokio::test]
async fn a_hello_without_a_version_is_protocol_1() {
    let env = master().await;
    let (id, key) = joined(&env, "old").await;
    let (answer, _ws) = auth(&env, &id, &key, json!({})).await;
    if PROTOCOL_VERSION == 1 {
        assert_eq!(answer["type"], "accepted", "answered {answer}");
    } else {
        assert_eq!(answer["type"], "rejected", "answered {answer}");
        let reason = answer["reason"].as_str().unwrap();
        assert!(reason.contains("protocol 1"), "{reason}");
    }
}

/// Whoever has not proved a registered key gets the one refusal anyone
/// gets ("unknown or revoked node"), whatever version it claims: the
/// master's version is told only to its own nodes.
#[tokio::test]
async fn an_unauthenticated_peer_does_not_learn_the_version() {
    let env = master().await;
    let (id, _key) = joined(&env, "box").await;
    let stranger = NodeKey::load_or_create(&env.dir.path().join("stranger.key")).unwrap();
    let plain = auth(&env, &id, &stranger, json!({})).await.0;
    assert_eq!(plain["type"], "rejected", "{plain}");
    for claim in [
        json!({"protocol_version": PROTOCOL_VERSION + 7}),
        json!({"protocol_version": 0}),
    ] {
        for node in [id.as_str(), "no-such-node"] {
            let (answer, _ws) = auth(&env, node, &stranger, claim.clone()).await;
            assert_eq!(answer, plain, "{node} claiming {claim}");
        }
    }
    let reason = plain["reason"].as_str().unwrap();
    assert!(!reason.contains("protocol"), "{reason}");
}
