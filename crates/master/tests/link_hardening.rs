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

/// A running agentic turn of `owner` (an account with the agentic grant)
/// on node `n1`, mirrored through `tx`.
async fn running_turn(store: &Store, tx: &mpsc::Sender<NodeToMaster>, owner: &str) -> SessionId {
    use harness_core::event::TurnStarted;
    let sid = SessionId::new();
    store
        .create_user(&harness_store::NewUser {
            user_id: owner.into(),
            username: owner.into(),
            password_hash: "$argon2id$x".into(),
            role: harness_store::Role::Member,
            can_agentic: true,
            must_change_password: false,
        })
        .unwrap();
    send(tx, event(sid, 1, "")).await;
    let (kind, payload) = EventKind::TurnStarted(TurnStarted {
        turn_id: "t1".into(),
        agent: None,
    })
    .into_parts();
    send(
        tx,
        RawEvent {
            session_id: sid,
            seq: 2,
            ts_ms: 2,
            kind,
            payload,
        },
    )
    .await;
    barrier(store, tx).await;
    store.claim_session(sid, Some(owner), None).unwrap();
    assert!(store.session(sid).unwrap().unwrap().running);
    sid
}

fn revoke_grant(store: &Store, user: &str) {
    store
        .update_user(
            user,
            &harness_store::UserPatch {
                can_agentic: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
}

/// The next `Cancel` for `sid` on `c`, within `secs`.
async fn cancel_within(c: &mut mpsc::Receiver<MasterToNode>, sid: SessionId, secs: u64) -> bool {
    let end = tokio::time::Instant::now() + Duration::from_secs(secs);
    loop {
        match tokio::time::timeout_at(end, c.recv()).await {
            Ok(Some(MasterToNode::Cancel { session_id, .. })) if session_id == sid => return true,
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => return false,
        }
    }
}

/// Gate 3 review P-3: the grant is revoked while the node's link is down;
/// the sweep then cannot reach it. When the node is back (after its
/// hello), the turn that lost its access is cancelled.
#[tokio::test]
async fn a_turn_that_lost_its_grant_while_its_node_was_away_is_cancelled_on_return() {
    let store = Store::open_in_memory().unwrap();
    let m = master_on(&store);
    let (l1, c1, tx1) = open_link("n1", vec![]);
    m.add_link(l1, false).await.unwrap();
    let sid = running_turn(&store, &tx1, "bob").await;
    drop((tx1, c1));
    wait_for(|| m.nodes().iter().all(|n| !n.connected)).await;

    revoke_grant(&store, "bob");
    harness_master::workspaces::cancel_revoked(&m, &["bob".to_string()]).await;

    let (l2, mut c2, _tx2) = open_link(
        "n1",
        vec![SessionHead {
            session_id: sid,
            head_seq: 2,
        }],
    );
    m.add_link(l2, false).await.unwrap();
    assert!(
        cancel_within(&mut c2, sid, 5).await,
        "the running turn was not cancelled after the node came back"
    );
}

/// Gate 3 fix review 2, R2-2: the sweep after a hello read the mirror once,
/// before the replay; a turn that started while the link was down, so that
/// only the replay says it runs, was never cancelled.
#[tokio::test]
async fn a_turn_known_only_from_the_replay_is_cancelled_when_it_lands() {
    use harness_core::event::TurnStarted;
    let store = Store::open_in_memory().unwrap();
    let m = master_on(&store);
    let (l1, c1, tx1) = open_link("n1", vec![]);
    m.add_link(l1, false).await.unwrap();
    // Bob's chat, mirrored up to its creation, idle.
    store
        .create_user(&harness_store::NewUser {
            user_id: "bob".into(),
            username: "bob".into(),
            password_hash: "$argon2id$x".into(),
            role: harness_store::Role::Member,
            can_agentic: true,
            must_change_password: false,
        })
        .unwrap();
    let sid = SessionId::new();
    send(&tx1, event(sid, 1, "")).await;
    barrier(&store, &tx1).await;
    store.claim_session(sid, Some("bob"), None).unwrap();
    drop((tx1, c1));
    wait_for(|| m.nodes().iter().all(|n| !n.connected)).await;

    // While the node is away bob loses the grant, and the turn starts.
    revoke_grant(&store, "bob");
    harness_master::workspaces::cancel_revoked(&m, &["bob".to_string()]).await;
    let (l2, mut c2, tx2) = open_link(
        "n1",
        vec![SessionHead {
            session_id: sid,
            head_seq: 2,
        }],
    );
    m.add_link(l2, false).await.unwrap();
    let last_seq = loop {
        match tokio::time::timeout(Duration::from_secs(5), c2.recv()).await {
            Ok(Some(MasterToNode::Resume {
                session_id,
                last_seq,
            })) if session_id == sid => break last_seq,
            Ok(Some(_)) => {}
            other => panic!("no resume: {other:?}"),
        }
    };
    assert_eq!(last_seq, 1);
    let (kind, payload) = EventKind::TurnStarted(TurnStarted {
        turn_id: "t1".into(),
        agent: None,
    })
    .into_parts();
    tx2.send(NodeToMaster::Snapshot {
        session_id: sid,
        head_seq: 2,
        events: vec![RawEvent {
            session_id: sid,
            seq: 2,
            ts_ms: 2,
            kind,
            payload,
        }],
    })
    .await
    .unwrap();
    assert!(
        cancel_within(&mut c2, sid, 5).await,
        "the turn the replay revealed was not cancelled"
    );
}

/// The `session.create` a link received for the next create: (request id,
/// session id).
async fn next_create(c: &mut mpsc::Receiver<MasterToNode>) -> (String, SessionId) {
    loop {
        match tokio::time::timeout(Duration::from_secs(30), c.recv()).await {
            Ok(Some(MasterToNode::Create {
                req_id, session_id, ..
            })) => return (req_id, session_id),
            Ok(Some(_)) => {}
            other => panic!("no create: {other:?}"),
        }
    }
}

fn create_spec(owner: &str) -> harness_master::CreateSpec {
    harness_master::CreateSpec {
        node_id: Some("n1".into()),
        title: "t".into(),
        cwd: "/w".into(),
        owner_id: Some(owner.into()),
        ..Default::default()
    }
}

/// Gate 3 review P-6: a create whose acknowledgement was late (or came
/// before the node's `session_created`) left a chat nobody owned; only the
/// API token could see or delete it. The master records the intended owner
/// before it sends the create, and the late chat is that owner's.
#[tokio::test(start_paused = true)]
async fn a_late_session_created_is_claimed_by_whoever_asked_for_it() {
    let store = Store::open_in_memory().unwrap();
    let m = master_on(&store);
    m.set_agents(harness_agents::AgentSet::load(std::path::Path::new("/nonexistent")).0);
    let (l, mut c, tx) = open_link("n1", vec![]);
    m.add_link(l, false).await.unwrap();

    // The node answers the create before it says the chat exists.
    let m2 = m.clone();
    let made = tokio::spawn(async move { m2.create_session(create_spec("bob")).await });
    let (req, sid) = next_create(&mut c).await;
    tx.send(NodeToMaster::Ack {
        req_id: req,
        ok: true,
        error: None,
    })
    .await
    .unwrap();
    assert_eq!(made.await.unwrap().unwrap(), sid);
    send(&tx, event(sid, 1, "")).await;
    barrier(&store, &tx).await;
    assert_eq!(
        store.session(sid).unwrap().unwrap().owner_id.as_deref(),
        Some("bob"),
        "an ack before session_created left the chat without an owner"
    );

    // The node does not answer in time; the chat turns up later.
    let m2 = m.clone();
    let made = tokio::spawn(async move { m2.create_session(create_spec("carol")).await });
    let (_, sid) = next_create(&mut c).await;
    assert!(matches!(
        made.await.unwrap(),
        Err(harness_master::CommandError::Timeout)
    ));
    send(&tx, event(sid, 1, "")).await;
    barrier(&store, &tx).await;
    assert_eq!(
        store.session(sid).unwrap().unwrap().owner_id.as_deref(),
        Some("carol"),
        "a create that timed out left the chat without an owner"
    );
}

/// Gate 3 fix review 6, N-2: a create that timed out and never landed left
/// its intent row for good. An intent lives for `CREATE_INTENT_TTL`: an
/// older one is removed by the next create and claims nothing.
#[tokio::test]
async fn a_create_intent_that_never_lands_is_removed_and_claims_nothing() {
    let store = Store::open_in_memory().unwrap();
    let m = master_on(&store);
    m.set_agents(harness_agents::AgentSet::load(std::path::Path::new("/nonexistent")).0);
    let (l, mut c, tx) = open_link("n1", vec![]);
    m.add_link(l, false).await.unwrap();
    let stale = SessionId::new();
    let old = harness_core::now_ms() - harness_master::CREATE_INTENT_TTL_MS - 1;
    store
        .kv_set(
            &format!("create-intent:{stale}"),
            &serde_json::json!({"node": "n1", "owner": "mallory", "workspace": null, "at": old})
                .to_string(),
        )
        .unwrap();
    // The next create clears it.
    let m2 = m.clone();
    let made = tokio::spawn(async move { m2.create_session(create_spec("bob")).await });
    let (req, sid) = next_create(&mut c).await;
    send(&tx, event(sid, 1, "")).await;
    tx.send(NodeToMaster::Ack {
        req_id: req,
        ok: true,
        error: None,
    })
    .await
    .unwrap();
    made.await.unwrap().unwrap();
    assert_eq!(
        store.kv_get(&format!("create-intent:{stale}")).unwrap(),
        None,
        "the stale intent row stayed"
    );
    // And a chat that turns up that late is nobody's claim.
    send(&tx, event(stale, 1, "")).await;
    barrier(&store, &tx).await;
    assert_eq!(store.session(stale).unwrap().unwrap().owner_id, None);
}

/// Gate 3 review N-5: a command whose caller went away (an HTTP request
/// that timed out) left its entry in the master's table of pending acks;
/// and a model at create was forwarded to the node unchecked.
#[tokio::test]
async fn an_abandoned_command_leaves_no_ack_and_an_unknown_model_is_refused() {
    let store = Store::open_in_memory().unwrap();
    let m = master_on(&store);
    m.set_agents(harness_agents::AgentSet::load(std::path::Path::new("/nonexistent")).0);
    let (l, mut c, _tx) = open_link("n1", vec![]);
    m.add_link(l, false).await.unwrap();
    let pending = tokio::time::timeout(
        Duration::from_millis(200),
        m.create_session(create_spec("bob")),
    )
    .await;
    assert!(pending.is_err(), "the node never answers");
    next_create(&mut c).await;
    assert_eq!(m.pending_acks(), 0, "the abandoned command's ack stayed");

    let mut spec = create_spec("bob");
    spec.model = Some("nope::not-a-model".into());
    let e = m.create_session(spec).await.unwrap_err();
    assert!(e.to_string().contains("unknown model"), "{e}");
    assert!(
        tokio::time::timeout(Duration::from_millis(200), next_create(&mut c))
            .await
            .is_err(),
        "the unknown model went to the node"
    );
}

/// Gate 3 review P-3: a cancel that could not be queued (the node's queue
/// is full) is tried again, so the turn does not outlive the revocation.
#[tokio::test]
async fn a_revocation_cancel_that_found_the_queue_full_is_retried() {
    let store = Store::open_in_memory().unwrap();
    let m = master_on(&store);
    // A queue of one, which the master's welcome fills.
    let (tx, from_node) = mpsc::channel::<NodeToMaster>(9);
    let (to_node, mut commands) = mpsc::channel::<MasterToNode>(1);
    tx.try_send(NodeToMaster::Hello {
        protocol_version: PROTOCOL_VERSION,
        node_id: "n1".into(),
        hostname: "h".into(),
        version: "1".into(),
        capabilities: vec![],
        sessions: vec![],
    })
    .unwrap();
    m.add_link(NodeLink { to_node, from_node }, false)
        .await
        .unwrap();
    let sid = running_turn(&store, &tx, "bob").await;
    revoke_grant(&store, "bob");
    harness_master::workspaces::cancel_revoked(&m, &["bob".to_string()]).await;
    // The node reads its queue only now.
    assert!(
        cancel_within(&mut commands, sid, 10).await,
        "the cancel was dropped for good"
    );
}

/// Gate 3 review P-7: on a diverged chat an allow is refused (it would
/// make the node write a turn nobody sees), but a deny is how a person
/// stops that turn, like cancel: it goes through.
#[tokio::test]
async fn a_diverged_chat_refuses_an_allow_and_takes_a_deny() {
    use harness_core::policy::Decision;
    let store = Store::open_in_memory().unwrap();
    let m = master_on(&store);
    let sid = SessionId::new();
    let (l1, _c1, tx1) = open_link("n1", vec![]);
    m.add_link(l1, false).await.unwrap();
    for seq in 1..=3 {
        send(&tx1, event(sid, seq, "first")).await;
    }
    barrier(&store, &tx1).await;
    let (l2, mut c2, tx2) = open_link(
        "n1",
        vec![SessionHead {
            session_id: sid,
            head_seq: 2,
        }],
    );
    m.add_link(l2, false).await.unwrap();
    send(&tx2, event(sid, 3, "second copy")).await;
    barrier(&store, &tx2).await;
    assert_eq!(store.diverged_seq(sid).unwrap(), Some(3));

    let err = m
        .respond_approval(sid, "a1".into(), Decision::Allow)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("diverged"), "{err}");

    let m2 = m.clone();
    let deny =
        tokio::spawn(async move { m2.respond_approval(sid, "a1".into(), Decision::Deny).await });
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), c2.recv())
            .await
            .expect("the deny never reached the node")
            .expect("link open");
        if let MasterToNode::ApprovalRespond {
            req_id, decision, ..
        } = msg
        {
            assert!(!decision.allows());
            tx2.send(NodeToMaster::Ack {
                req_id,
                ok: true,
                error: None,
            })
            .await
            .unwrap();
            break;
        }
    }
    deny.await.unwrap().unwrap();
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

// --- What a node may set in a chat's row (Gate 3 review L-1 to L-3) ---

fn created_by(sid: SessionId, seq: u64, parent: Option<SessionId>) -> RawEvent {
    let (kind, payload) = EventKind::SessionCreated(SessionCreated {
        profile: Profile::Agentic,
        agent_id: None,
        incognito: false,
        parent_session: parent,
        title: "t".into(),
        cwd: "/w".into(),
        model: "m".into(),
        backend: "native".into(),
        node_id: "n1".into(),
        approval_mode: Default::default(),
    })
    .into_parts();
    RawEvent {
        session_id: sid,
        seq,
        ts_ms: seq as i64,
        kind,
        payload,
    }
}

/// Wait for a `session.cancel` of `sid` on `c`.
async fn cancelled(c: &mut mpsc::Receiver<MasterToNode>, sid: SessionId) -> bool {
    for _ in 0..200 {
        match c.try_recv() {
            Ok(MasterToNode::Cancel { session_id, .. }) if session_id == sid => return true,
            Ok(_) => {}
            Err(_) => tokio::time::sleep(Duration::from_millis(10)).await,
        }
    }
    false
}

#[tokio::test]
async fn a_second_session_created_moves_nothing_and_freezes_the_chat() {
    let store = Store::open_in_memory().unwrap();
    let m = master_on(&store);
    let (alice, bob) = (SessionId::new(), SessionId::new());
    let (l, mut c, tx) = open_link("n1", vec![]);
    m.add_link(l, false).await.unwrap();
    send(&tx, created_by(alice, 1, None)).await;
    send(&tx, created_by(bob, 1, None)).await;
    barrier(&store, &tx).await;
    store.claim_session(alice, Some("alice"), None).unwrap();
    store.claim_session(bob, Some("bob"), None).unwrap();

    // Alice's chat names Bob's as its parent: under the old rule it became
    // Bob's.
    send(&tx, created_by(alice, 2, Some(bob))).await;
    barrier(&store, &tx).await;
    let row = store.session(alice).unwrap().unwrap();
    assert_eq!(row.owner_id.as_deref(), Some("alice"));
    assert_eq!(row.parent_id, None);
    assert_eq!(row.head_seq, 1, "the event is not stored");
    assert_eq!(store.diverged_seq(alice).unwrap(), Some(2));
    assert!(cancelled(&mut c, alice).await, "the node is told to stop");
    // Bob's chat is untouched and still takes events.
    send(&tx, event(bob, 2, "fine")).await;
    barrier(&store, &tx).await;
    assert_eq!(store.head_seq(bob).unwrap(), 2);
}

#[tokio::test]
async fn an_oversized_event_from_a_node_is_refused_and_freezes_the_chat() {
    let store = Store::open_in_memory().unwrap();
    let m = master_on(&store);
    let sid = SessionId::new();
    let (l, mut c, tx) = open_link("n1", vec![]);
    m.add_link(l, false).await.unwrap();
    send(&tx, created_by(sid, 1, None)).await;
    // A large legitimate event is taken...
    send(&tx, event(sid, 2, &"x".repeat(3 << 20))).await;
    barrier(&store, &tx).await;
    assert_eq!(store.head_seq(sid).unwrap(), 2);
    // ...one over the cap is not, and nothing after it either.
    send(
        &tx,
        event(sid, 3, &"x".repeat(harness_store::MAX_EVENT_BYTES)),
    )
    .await;
    send(&tx, event(sid, 4, "after")).await;
    barrier(&store, &tx).await;
    assert_eq!(store.head_seq(sid).unwrap(), 2);
    assert_eq!(store.diverged_seq(sid).unwrap(), Some(3));
    assert!(cancelled(&mut c, sid).await);

    // Refused at its first event: a frozen empty row, so later frames are
    // refused too instead of asking for the session again.
    let other = SessionId::new();
    let mut first = created_by(other, 1, None);
    first.payload["cwd"] = serde_json::json!("/".repeat(harness_store::MAX_CWD_BYTES + 1));
    send(&tx, first).await;
    send(&tx, event(other, 2, "next")).await;
    barrier(&store, &tx).await;
    assert_eq!(store.head_seq(other).unwrap(), 0);
    assert_eq!(store.diverged_seq(other).unwrap(), Some(1));
}

// --- Revocation and the link (Gate 3 review L-4) ---

/// A registered node id in `store`.
fn registered(store: &Store, n: usize) -> String {
    let token = format!("tok-{n}");
    store.join_token_create(&token, i64::MAX, None).unwrap();
    store
        .node_join(&token, 0, &format!("key-{n}"), &format!("box{n}"))
        .unwrap()
        .unwrap()
}

/// A session of node `id` in the mirror, for commands to aim at.
fn session_on(store: &Store, id: &str) -> SessionId {
    let sid = SessionId::new();
    store.append(id, &created_by(sid, 1, None)).unwrap();
    sid
}

/// Whether a command for `sid` reaches its node (the fake never acks, so a
/// command that is sent times out; one that is not is `NodeOffline` at once).
async fn reachable(m: &Master, sid: SessionId) -> bool {
    !matches!(
        m.cancel(sid).await,
        Err(harness_master::CommandError::NodeOffline(_))
    )
}

#[tokio::test]
async fn a_revoked_node_cannot_link_and_gets_no_commands() {
    let store = Store::open_in_memory().unwrap();
    let m = master_on(&store);
    let id = registered(&store, 0);
    let sid = session_on(&store, &id);

    // Linked, then revoked: the link is down and nothing is sent to it.
    let (l, mut c, _tx) = open_link(&id, vec![]);
    m.add_link(l, false).await.unwrap();
    assert!(m.is_connected(&id));
    assert!(m.revoke_node(&id).await.unwrap());
    assert!(!m.is_connected(&id));
    assert!(!reachable(&m, sid).await);
    while let Ok(msg) = c.try_recv() {
        assert!(
            matches!(msg, MasterToNode::Welcome { .. }),
            "sent after the revoke: {msg:?}"
        );
    }

    // Linking again is refused, in this process and in a new one (the
    // store's record).
    let (l, _c, _tx) = open_link(&id, vec![]);
    let err = m.add_link(l, false).await.unwrap_err();
    assert!(err.contains("revoked"), "{err}");
    assert!(!m.is_connected(&id));
    let fresh = master_on(&store);
    let (l, _c, _tx) = open_link(&id, vec![]);
    assert!(
        fresh
            .add_link(l, false)
            .await
            .unwrap_err()
            .contains("revoked")
    );
    assert!(!fresh.is_connected(&id));
}

/// A node reconnects while it is revoked, over and over. Whatever the
/// order in which `add_link` and `revoke_node` run, it never ends with a
/// live link. Both start from a barrier on a multi-threaded runtime, so
/// different interleavings occur across the iterations; which ones occur
/// is up to the scheduler. What makes every interleaving safe is that
/// `add_link` registers and `revoke_node` marks under the same lock; this
/// test shows the outcome for the ones that happened, it cannot force the
/// narrowest one (a revoke landing between `add_link`'s store read and its
/// registration), and it does not cover `node_connection`'s own second
/// look, which needs a TLS handshake (`remote_nodes.rs`).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_node_revoked_while_it_reconnects_never_stays_linked() {
    let store = Store::open_in_memory().unwrap();
    let m = master_on(&store);
    let (mut linked_first, mut refused) = (0, 0);
    for n in 0..150 {
        let id = registered(&store, n);
        let sid = session_on(&store, &id);
        let (l, _c, tx) = open_link(&id, vec![]);
        let gate = std::sync::Arc::new(tokio::sync::Barrier::new(2));
        let (m1, g1) = (m.clone(), gate.clone());
        let link = tokio::spawn(async move {
            g1.wait().await;
            m1.add_link(l, false).await
        });
        let (m2, g2, id2) = (m.clone(), gate.clone(), id.clone());
        let revoke = tokio::spawn(async move {
            g2.wait().await;
            m2.revoke_node(&id2).await.unwrap()
        });
        let linked = link.await.unwrap();
        assert!(revoke.await.unwrap());
        match linked {
            Ok(_) => linked_first += 1,
            Err(_) => refused += 1,
        }
        assert!(!m.is_connected(&id), "iteration {n}: still linked");
        assert!(!reachable(&m, sid).await, "iteration {n}: still reachable");
        drop(tx);
    }
    // Not asserted (the scheduler decides), but printed so a run shows
    // that both orders occurred.
    eprintln!("linked before the revoke: {linked_first}, refused: {refused}");
}

// --- The hello, snapshots and acks (Gate 3 review L-5, L-6, L-8, L-9) ---

fn heads(n: usize) -> Vec<SessionHead> {
    (0..n)
        .map(|_| SessionHead {
            session_id: SessionId::new(),
            head_seq: 1,
        })
        .collect()
}

fn resumes(c: &mut mpsc::Receiver<MasterToNode>) -> usize {
    let mut n = 0;
    while let Ok(m) = c.try_recv() {
        n += usize::from(matches!(m, MasterToNode::Resume { .. }));
    }
    n
}

/// L-5. A hello listing 60,000 sessions starts a long pass over them; the
/// node reconnects meanwhile. The old link's pass stops at its next batch
/// and sends nothing on the new link, so passes never stack.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reconnect_stops_the_old_hellos_pass() {
    let store = Store::open_in_memory().unwrap();
    let m = master_on(&store);
    const N: usize = 60_000;
    // Room for every resume, so the count below shows how far it got.
    let (tx1, from_node) = mpsc::channel::<NodeToMaster>(9);
    let (to_node, mut c1) = mpsc::channel::<MasterToNode>(N + 16);
    tx1.try_send(NodeToMaster::Hello {
        protocol_version: PROTOCOL_VERSION,
        node_id: "n1".into(),
        hostname: "h".into(),
        version: "1".into(),
        capabilities: vec![],
        sessions: heads(N),
    })
    .unwrap();
    let l1 = NodeLink { to_node, from_node };
    let m1 = m.clone();
    let first = tokio::spawn(async move { m1.add_link(l1, false).await });
    // The first pass is under way: resumes reach the first link.
    wait_for(|| {
        if let Ok(MasterToNode::Resume { .. }) = c1.try_recv() {
            return true;
        }
        false
    })
    .await;
    let (l2, mut c2, _tx2) = open_link("n1", vec![]);
    m.add_link(l2, false).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), first)
        .await
        .expect("the old pass stops")
        .unwrap()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(resumes(&mut c2), 0, "the old pass sent on the new link");
    // The first pass was cut short (timing: it needs a few hundred batches,
    // the second link a few milliseconds).
    let sent = 1 + resumes(&mut c1);
    assert!(sent < N, "the old pass ran to the end ({sent})");
    drop(tx1);
}

/// L-6. A hello entry is acted on only for the node's own sessions: another
/// node's live or deleted session gets no `resume` (which would reveal its
/// length) and no `session.delete`. The node's own deleted session is still
/// deleted again.
#[tokio::test]
async fn a_hello_naming_another_nodes_sessions_learns_nothing() {
    let store = Store::open_in_memory().unwrap();
    let m = master_on(&store);
    let (theirs, gone, mine) = (SessionId::new(), SessionId::new(), SessionId::new());
    for (node, sid) in [("n1", theirs), ("n1", gone), ("n2", mine)] {
        store.append(node, &created_by(sid, 1, None)).unwrap();
    }
    store.delete_session(gone, true).unwrap();
    store.delete_session(mine, true).unwrap();
    let named = |sid| SessionHead {
        session_id: sid,
        head_seq: 50,
    };
    let (l, mut c, _tx) = open_link("n2", vec![named(theirs), named(gone), named(mine)]);
    m.add_link(l, false).await.unwrap();
    let mut deleted = Vec::new();
    for _ in 0..50 {
        while let Ok(msg) = c.try_recv() {
            match msg {
                MasterToNode::Resume { session_id, .. } => {
                    panic!("resume for {session_id}")
                }
                MasterToNode::Delete { session_id, .. } => deleted.push(session_id),
                _ => {}
            }
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(deleted, vec![mine]);
    // Its events for the other node's deleted session are refused as for a
    // live one, without a delete either.
    let (l, mut c, tx) = open_link("n3", vec![]);
    m.add_link(l, false).await.unwrap();
    send(&tx, created_by(gone, 1, None)).await;
    barrier(&store, &tx).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    while let Ok(msg) = c.try_recv() {
        assert!(!matches!(msg, MasterToNode::Delete { .. }), "{msg:?}");
    }
}

/// L-8. A snapshot that does not reach the head it announced (here it
/// skips an event) used to leave the session marked as resuming, so no
/// later gap asked again. Now the next gap sends a new `resume`.
#[tokio::test]
async fn a_snapshot_that_falls_short_lets_the_next_gap_ask_again() {
    let store = Store::open_in_memory().unwrap();
    let m = master_on(&store);
    let sid = SessionId::new();
    let (l, mut c, tx) = open_link("n1", vec![]);
    m.add_link(l, false).await.unwrap();
    send(&tx, created_by(sid, 1, None)).await;
    send(&tx, event(sid, 2, "d2")).await;
    // A gap: the master asks to resume from 2.
    send(&tx, event(sid, 4, "d4")).await;
    barrier(&store, &tx).await;
    assert_eq!(resumes(&mut c), 1);
    // The answer skips seq 3.
    tx.send(NodeToMaster::Snapshot {
        session_id: sid,
        head_seq: 5,
        events: vec![event(sid, 4, "d4"), event(sid, 5, "d5")],
    })
    .await
    .unwrap();
    send(&tx, event(sid, 6, "d6")).await;
    barrier(&store, &tx).await;
    assert_eq!(store.head_seq(sid).unwrap(), 2);
    assert!(resumes(&mut c) >= 1, "the next gap asked again");
}

/// L-9. What a remote node says when it refuses a command reaches log
/// lines and API responses: one line, bounded, without control
/// characters. Its capabilities are bounded too.
#[tokio::test]
async fn what_a_remote_node_answers_is_cleaned() {
    let store = Store::open_in_memory().unwrap();
    let m = master_on(&store);
    let mut caps: Vec<String> = (0..1000).map(|i| format!("cap-{i}")).collect();
    caps.insert(0, "x".repeat(10_000));
    caps.insert(1, "artifacts".into());
    let (tx, from_node) = mpsc::channel::<NodeToMaster>(16);
    let (to_node, mut c) = mpsc::channel::<MasterToNode>(16);
    tx.try_send(NodeToMaster::Hello {
        protocol_version: PROTOCOL_VERSION,
        node_id: "n1".into(),
        hostname: "h".into(),
        version: "1".into(),
        capabilities: caps,
        sessions: vec![],
    })
    .unwrap();
    m.add_link(NodeLink { to_node, from_node }, false)
        .await
        .unwrap();
    assert!(m.node_has("n1", "artifacts"));
    assert!(m.node_has("n1", "cap-10"));
    assert!(!m.node_has("n1", "cap-900"), "at most 64 are kept");

    let sid = session_on(&store, "n1");
    let m2 = m.clone();
    let cancel = tokio::spawn(async move { m2.cancel(sid).await });
    let req_id = loop {
        match c.recv().await.unwrap() {
            MasterToNode::Cancel { req_id, .. } => break req_id,
            _ => continue,
        }
    };
    let noisy = format!("no\u{1b}[2J\nway {}", "z".repeat(1 << 20));
    tx.send(NodeToMaster::Ack {
        req_id,
        ok: false,
        error: Some(noisy),
    })
    .await
    .unwrap();
    let err = cancel.await.unwrap().unwrap_err().to_string();
    assert!(err.chars().count() <= 300, "{} chars", err.chars().count());
    assert!(!err.chars().any(char::is_control), "{err:?}");
    assert!(err.starts_with("no [2J way z"), "{err:?}");
}
