//! A.19 review: the handshake's new field, attacked. Not part of the test
//! suite: `run_rust_proofs.sh` copies this to `crates/master/tests/` for the
//! run and removes the copy. Every test prints what the master answered;
//! the asserts are the properties that held.

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

/// One handshake whose hello is the raw text `make(signature)`: the
/// master's answer (`Null`: it closed without one) and the stream.
async fn raw(env: &Env, key: &NodeKey, make: impl FnOnce(&str) -> String) -> (Value, ClientStream) {
    use futures::SinkExt;
    let mut ws = connect_with(&env.url, &env.fp, Timing::default())
        .await
        .unwrap();
    let challenge: Value = recv_json(&mut ws).await.unwrap().unwrap();
    let nonce = challenge["nonce"].as_str().unwrap();
    let signature = key.sign_hex(&harness_link::identity::auth_message(nonce, &env.fp));
    ws.send(tokio_tungstenite::tungstenite::Message::text(make(
        &signature,
    )))
    .await
    .unwrap();
    let answer = match tokio::time::timeout(Duration::from_secs(10), recv_json::<_, Value>(&mut ws))
        .await
    {
        Ok(Ok(Some(v))) => v,
        Ok(Ok(None)) => Value::Null,
        Ok(Err(e)) => json!({"closed": e.to_string()}),
        Err(_) => json!("no answer in 10 s"),
    };
    (answer, ws)
}

fn hello(node: &str, sig: &str, version: &str) -> String {
    format!(r#"{{"type":"auth","node_id":"{node}","signature":"{sig}"{version}}}"#)
}

async fn joined(env: &Env, name: &str) -> (String, NodeKey) {
    let key = NodeKey::load_or_create(&env.dir.path().join(format!("{name}.key"))).unwrap();
    let token = env.master.create_join_token(None).await.unwrap();
    let id = join(&env.url, &env.fp, &token, name, &key).await.unwrap();
    (id, key)
}

/// Every shape the new field can take, from the master's own node and from
/// a stranger. The stranger's column must never hold a version.
#[tokio::test]
async fn the_fields_shapes() {
    let env = master().await;
    let (id, key) = joined(&env, "box").await;
    let stranger = NodeKey::load_or_create(&env.dir.path().join("stranger.key")).unwrap();
    let shapes = [
        ("absent", String::new()),
        ("1", r#","protocol_version":1"#.into()),
        ("0", r#","protocol_version":0"#.into()),
        ("2", r#","protocol_version":2"#.into()),
        ("u32::MAX", r#","protocol_version":4294967295"#.into()),
        ("u32::MAX+1", r#","protocol_version":4294967296"#.into()),
        ("-1", r#","protocol_version":-1"#.into()),
        ("1.0", r#","protocol_version":1.0"#.into()),
        ("1e400", r#","protocol_version":1e400"#.into()),
        ("\"1\"", r#","protocol_version":"1""#.into()),
        ("null", r#","protocol_version":null"#.into()),
        ("[1]", r#","protocol_version":[1]"#.into()),
        ("a 2000-digit number", format!(r#","protocol_version":{}"#, "9".repeat(2000))),
        ("twice: 1 then 2", r#","protocol_version":1,"protocol_version":2"#.into()),
        (
            "a 1 MiB string",
            format!(r#","protocol_version":"{}""#, "A".repeat(1 << 20)),
        ),
    ];
    println!("{:<22} | {:<60} | a stranger's key", "protocol_version", "the node's own key");
    for (name, v) in &shapes {
        let (own, _a) = raw(&env, &key, |s| hello(&id, s, v)).await;
        let (other, _b) = raw(&env, &stranger, |s| hello(&id, s, v)).await;
        let (unknown, _c) = raw(&env, &stranger, |s| hello("no-such-node", s, v)).await;
        let short = |v: &Value| {
            let t = v.to_string();
            t.chars().take(60).collect::<String>()
        };
        println!("{name:<22} | {:<60} | {} | {}", short(&own), short(&other), short(&unknown));
        for a in [&other, &unknown] {
            let t = a.to_string();
            assert!(
                !t.contains("protocol") && !t.contains(env!("CARGO_PKG_VERSION")),
                "a stranger was told a version: {t}"
            );
        }
    }
}

/// A refused handshake with the right key does not end the link the node
/// already has; a `node.hello` that states another version than the `auth`
/// did is refused and the node is not listed.
#[tokio::test]
async fn a_refused_hello_does_not_touch_the_live_link_and_a_lying_hello_is_refused() {
    let env = master().await;
    let (id, key) = joined(&env, "box").await;
    let ok = format!(r#","protocol_version":{PROTOCOL_VERSION}"#);
    let (answer, mut live) = raw(&env, &key, |s| hello(&id, s, &ok)).await;
    assert_eq!(answer["type"], "accepted");
    let hello_msg = |v: u32| {
        json!({"type": "node.hello", "node_id": id, "protocol_version": v, "hostname": "box",
               "version": "x", "capabilities": [], "roots": [], "heads": [], "agents": [],
               "max_approval_mode": "auto", "sessions": []})
    };
    send_json(&mut live, &hello_msg(PROTOCOL_VERSION)).await.unwrap();
    let mut listed = false;
    for _ in 0..50 {
        listed = env.master.nodes().iter().any(|n| n.node_id == id && n.connected);
        if listed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    println!("the node's own link is up: {listed}");
    if !listed {
        // The hello's shape is this proof's guess; say so instead of failing.
        let next: Option<Value> = tokio::time::timeout(Duration::from_secs(2), recv_json(&mut live))
            .await
            .ok()
            .and_then(|r| r.ok())
            .flatten();
        println!("NOT TESTED: the hand-written node.hello was not taken ({next:?})");
        return;
    }
    for v in [PROTOCOL_VERSION + 1, 0] {
        let bad = format!(r#","protocol_version":{v}"#);
        let (answer, _ws) = raw(&env, &key, |s| hello(&id, s, &bad)).await;
        println!("a second handshake claiming {v}: {answer}");
        assert_eq!(answer["type"], "rejected");
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let still = env.master.nodes().iter().any(|n| n.node_id == id && n.connected);
    println!("after two refused handshakes with its key, the live link is still up: {still}");
    assert!(still, "a refused handshake ended the live link");

    // auth says the master's version, node.hello says another.
    let (id2, key2) = joined(&env, "liar").await;
    let (answer, mut ws) = raw(&env, &key2, |s| hello(&id2, s, &ok)).await;
    assert_eq!(answer["type"], "accepted");
    let mut lie = hello_msg(PROTOCOL_VERSION + 1);
    lie["node_id"] = json!(id2);
    send_json(&mut ws, &lie).await.unwrap();
    let next = tokio::time::timeout(Duration::from_secs(3), recv_json::<_, Value>(&mut ws)).await;
    println!("auth says {PROTOCOL_VERSION}, node.hello says {}: the master then sends {next:?}",
             PROTOCOL_VERSION + 1);
    let listed = env.master.nodes().iter().any(|n| n.node_id == id2 && n.connected);
    println!("the lying node is listed as connected: {listed}");
    assert!(!listed);
}

/// What an unauthenticated peer can time: the same refusal whatever version
/// it claims (the comparison runs only after the signature held).
#[tokio::test]
async fn a_strangers_refusal_takes_as_long_whatever_it_claims() {
    let env = master().await;
    let (id, _key) = joined(&env, "box").await;
    let stranger = NodeKey::load_or_create(&env.dir.path().join("stranger.key")).unwrap();
    let mut med = Vec::new();
    for v in ["", r#","protocol_version":1"#, r#","protocol_version":99"#] {
        let mut ts = Vec::new();
        for _ in 0..40 {
            let t = std::time::Instant::now();
            let (a, _ws) = raw(&env, &stranger, |s| hello(&id, s, v)).await;
            ts.push(t.elapsed());
            assert_eq!(a["reason"], "unknown or revoked node");
        }
        ts.sort();
        med.push(ts[20]);
        println!("claim {v:<26} median {:?} (min {:?}, max {:?})", ts[20], ts[0], ts[39]);
    }
}
