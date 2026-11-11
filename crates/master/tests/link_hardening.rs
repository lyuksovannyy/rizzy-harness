//! What the master accepts from a remote node's hello (docs/security-node-link.md).

use std::time::Duration;

use harness_core::event::{AssistantDelta, EventKind, Profile, SessionCreated};
use harness_core::{RawEvent, Secret, SessionId};
use harness_master::Master;
use harness_master::auth::Auth;
use harness_node::NodeLink;
use harness_protocol::{MasterToNode, NodeToMaster, PROTOCOL_VERSION, SessionHead};
use harness_store::Store;
use tokio::sync::mpsc;

fn link(id: &str, hostname: &str, caps: &[&str]) -> (NodeLink, mpsc::Receiver<MasterToNode>) {
    let (to_node, commands) = mpsc::channel::<MasterToNode>(1024);
    let (tx, from_node) = mpsc::channel::<NodeToMaster>(1024);
    tx.try_send(NodeToMaster::Hello {
        protocol_version: PROTOCOL_VERSION,
        node_id: id.into(),
        hostname: hostname.into(),
        version: "1".repeat(200),
        capabilities: caps.iter().map(|c| c.to_string()).collect(),
        sessions: vec![],
    })
    .unwrap();
    // Keep the sender alive so the link stays open.
    std::mem::forget(tx);
    (NodeLink { to_node, from_node }, commands)
}

fn master() -> Master {
    Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("t".into()), vec![], false),
    )
}

#[tokio::test]
async fn a_remote_node_cannot_be_the_chat_runtime() {
    let m = master();
    let (l, _c) = link("evil", "h", &["profile:chat"]);
    let err = m.add_link(l, false).await.unwrap_err();
    assert!(err.contains("chat profile"), "{err}");
    assert!(m.nodes().iter().all(|n| n.node_id != "evil"));
    // The master's own runtime (embedded) still may.
    let (l, _c2) = link("master", "h", &["profile:chat"]);
    m.add_link(l, true).await.unwrap();
    assert_eq!(
        m.default_node(harness_core::event::Profile::Chat)
            .as_deref(),
        Some("master")
    );
}

#[tokio::test]
async fn what_a_remote_node_says_about_itself_is_cleaned() {
    let m = master();
    let (l, _c) = link("n1", &format!("box\u{1b}[31m{}", "x".repeat(100)), &[]);
    m.add_link(l, false).await.unwrap();
    let n = m.nodes().into_iter().find(|n| n.node_id == "n1").unwrap();
    assert!(
        !n.hostname.chars().any(char::is_control),
        "{:?}",
        n.hostname
    );
    assert_eq!(n.hostname.chars().count(), 64);
    assert_eq!(n.version.chars().count(), 64);
}

// --- One writer per session: divergent seq, revoked frames (PLAN.md §3, T7) ---

/// A link whose sending half the test keeps, as a connection authenticated
/// as `id` that has said hello with `heads`.
fn open_link(
    id: &str,
    heads: Vec<SessionHead>,
) -> (
    NodeLink,
    mpsc::Receiver<MasterToNode>,
    mpsc::Sender<NodeToMaster>,
) {
    // The size of the master's queue per node (PLAN.md §3, FROM_NODE_QUEUE).
    let (tx, from_node) = mpsc::channel::<NodeToMaster>(9);
    let (to_node, commands) = mpsc::channel::<MasterToNode>(1024);
    tx.try_send(NodeToMaster::Hello {
        protocol_version: PROTOCOL_VERSION,
        node_id: id.into(),
        hostname: "h".into(),
        version: "1".into(),
        capabilities: vec![],
        sessions: heads,
    })
    .unwrap();
    (NodeLink { to_node, from_node }, commands, tx)
}

fn event(sid: SessionId, seq: u64, text: &str) -> RawEvent {
    let (kind, payload) = if seq == 1 {
        EventKind::SessionCreated(SessionCreated {
            profile: Profile::Agentic,
            agent_id: None,
            incognito: false,
            parent_session: None,
            title: "t".into(),
            cwd: "/w".into(),
            model: "m".into(),
            backend: "native".into(),
            node_id: "n1".into(),
            approval_mode: Default::default(),
        })
    } else {
        EventKind::AssistantDelta(AssistantDelta {
            author: None,
            variant: None,
            turn_id: "t1".into(),
            message_id: "m1".into(),
            text: text.into(),
        })
    }
    .into_parts();
    RawEvent {
        session_id: sid,
        seq,
        ts_ms: seq as i64,
        kind,
        payload,
    }
}

async fn send(tx: &mpsc::Sender<NodeToMaster>, e: RawEvent) {
    tx.send(NodeToMaster::Event { event: e }).await.unwrap();
}

async fn wait_for(mut f: impl FnMut() -> bool) {
    for _ in 0..500 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("condition not reached");
}

/// Every frame sent before this one on `tx` has been through `apply`: a
/// link is read in order, and this frame starts a session of its own.
async fn barrier(store: &Store, tx: &mpsc::Sender<NodeToMaster>) {
    let marker = SessionId::new();
    send(tx, event(marker, 1, "")).await;
    wait_for(|| store.head_seq(marker).unwrap() == 1).await;
}

fn master_on(store: &Store) -> Master {
    Master::new(
        store.clone(),
        Auth::new(Secret::new("t".into()), vec![], false),
    )
}

#[tokio::test]
async fn a_second_writer_under_one_node_key_freezes_the_mirror_instead_of_mixing_logs() {
    let store = Store::open_in_memory().unwrap();
    let m = master_on(&store);
    let sid = SessionId::new();

    let (l1, _c1, tx1) = open_link("n1", vec![]);
    m.add_link(l1, false).await.unwrap();
    for seq in 1..=7 {
        send(&tx1, event(sid, seq, &format!("first copy {seq}"))).await;
    }
    barrier(&store, &tx1).await;
    let before = store.events_after(sid, 0, 1000).unwrap();
    assert_eq!(before.len(), 7);

    // A second process holding the same key: its log has another seq 7.
    let (l2, mut c2, tx2) = open_link(
        "n1",
        vec![SessionHead {
            session_id: sid,
            head_seq: 6,
        }],
    );
    m.add_link(l2, false).await.unwrap();
    send(&tx2, event(sid, 7, "second copy 7")).await;
    // ...and then an in-order frame, which must not be appended either.
    send(&tx2, event(sid, 8, "second copy 8")).await;
    barrier(&store, &tx2).await;

    assert_eq!(store.events_after(sid, 0, 1000).unwrap(), before);
    assert_eq!(store.head_seq(sid).unwrap(), 7);
    assert_eq!(store.diverged_seq(sid).unwrap(), Some(7));

    // The original writer's next frame is refused as well: the master
    // cannot tell which copy is right.
    send(&tx1, event(sid, 8, "first copy 8")).await;
    barrier(&store, &tx1).await;
    assert_eq!(store.events_after(sid, 0, 1000).unwrap(), before);

    // The node is told to stop the session (cancel, on its live link).
    let mut cancelled = false;
    for _ in 0..200 {
        match c2.try_recv() {
            Ok(MasterToNode::Cancel { session_id, .. }) if session_id == sid => {
                cancelled = true;
                break;
            }
            Ok(_) => {}
            Err(_) => tokio::time::sleep(Duration::from_millis(10)).await,
        }
    }
    assert!(cancelled, "the node was not told to stop the session");

    // New work in that session is refused before it reaches a node.
    let err = m
        .prompt(sid, "go on".into(), Default::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("diverged"), "{err}");
}

#[tokio::test]
async fn frames_queued_from_a_node_that_is_then_revoked_are_not_applied() {
    let store = Store::open_in_memory().unwrap();
    store.join_token_create("h", i64::MAX, None).unwrap();
    let id = store.node_join("h", 0, "key", "box").unwrap().unwrap();
    let m = master_on(&store);
    let sid = SessionId::new();

    let (l, _c, tx) = open_link(&id, vec![]);
    m.add_link(l, false).await.unwrap();
    send(&tx, event(sid, 1, "")).await;
    wait_for(|| store.head_seq(sid).unwrap() == 1).await;

    assert!(m.revoke_node(&id).await.unwrap());
    // Eight frames that were read off the socket before the revoke and
    // wait in the master's queue (in process nothing stops the sender, so
    // they are put there now; `apply` cannot tell the difference).
    for seq in 2..=9 {
        tx.try_send(NodeToMaster::Event {
            event: event(sid, seq, "late"),
        })
        .unwrap();
    }
    // A session the revoked node never mirrored is refused too.
    let other = SessionId::new();
    send(&tx, event(other, 1, "")).await;
    // The queue drains, then the link closes.
    drop(tx);
    wait_for(|| m.nodes().iter().all(|n| n.node_id != id || !n.connected)).await;

    assert_eq!(store.head_seq(sid).unwrap(), 1);
    assert_eq!(store.events_after(sid, 0, 100).unwrap().len(), 1);
    assert_eq!(store.head_seq(other).unwrap(), 0);
}

#[tokio::test]
async fn a_reconnect_that_resends_acknowledged_events_carries_on() {
    let store = Store::open_in_memory().unwrap();
    let m = master_on(&store);
    let sid = SessionId::new();

    let (l1, _c1, tx1) = open_link("n1", vec![]);
    m.add_link(l1, false).await.unwrap();
    for seq in 1..=5 {
        send(&tx1, event(sid, seq, &format!("d{seq}"))).await;
    }
    barrier(&store, &tx1).await;
    drop(tx1);

    // The node comes back and replays from before the mirror's head (the
    // events re-read from its own log), then carries on.
    let (l2, _c2, tx2) = open_link(
        "n1",
        vec![SessionHead {
            session_id: sid,
            head_seq: 6,
        }],
    );
    m.add_link(l2, false).await.unwrap();
    tx2.send(NodeToMaster::Snapshot {
        session_id: sid,
        head_seq: 6,
        events: (3..=6).map(|s| event(sid, s, &format!("d{s}"))).collect(),
    })
    .await
    .unwrap();
    send(&tx2, event(sid, 7, "d7")).await;
    barrier(&store, &tx2).await;

    assert_eq!(store.head_seq(sid).unwrap(), 7);
    assert_eq!(store.diverged_seq(sid).unwrap(), None);
    let texts: Vec<String> = store
        .events_after(sid, 1, 100)
        .unwrap()
        .iter()
        .map(|e| e.payload["text"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(texts, ["d2", "d3", "d4", "d5", "d6", "d7"]);
}
