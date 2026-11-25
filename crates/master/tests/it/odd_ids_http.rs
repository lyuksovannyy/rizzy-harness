//! Review F1 of FIX_PLAN 4.5 slice 4: the destructive set reads (member
//! removal, workspace deletion, the revocation sweep, account deletion)
//! act on a member's chat whose stored id is in another spelling (upper
//! case here; `SessionId` parses it), exactly as on any other chat of
//! theirs: its node is told to stop, cancel or delete it, and an account's
//! deletion removes its mirror row. A remaining member's chat is not
//! touched.
//!
//! No writer stores such an id (every one stores `SessionId::to_string()`);
//! it comes from a hand edit or a corrupt file, made here with raw SQL.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::event::{EventKind, Profile, SessionCreated, TurnStarted};
use harness_core::{RawEvent, Secret, SessionId};
use harness_master::Master;
use harness_master::auth::Auth;
use harness_master::router;
use harness_node::NodeLink;
use harness_protocol::{MasterToNode, NodeToMaster, PROTOCOL_VERSION};
use harness_store::{NewUser, Role, Store};
use reqwest::StatusCode;
use tokio::sync::mpsc;

const TOKEN: &str = "api-secret";

/// What the fake node was sent: (command, session).
type Seen = Arc<Mutex<Vec<(&'static str, SessionId)>>>;

struct T {
    base: String,
    store: Store,
    m: Master,
    seen: Seen,
    tx: mpsc::Sender<NodeToMaster>,
    team: String,
    /// Bob's chat in the team, its stored id upper-cased.
    odd: SessionId,
    /// Carol's chat in the team (she stays).
    carols: SessionId,
}

fn account(store: &Store, id: &str) {
    store
        .create_user(&NewUser {
            user_id: id.into(),
            username: id.into(),
            password_hash: "$argon2id$x".into(),
            role: Role::Member,
            exec_ceiling: harness_core::policy::ExecLevel::Host,
            must_change_password: false,
        })
        .unwrap();
    store.personal_workspace(id).unwrap();
}

fn created(sid: SessionId) -> RawEvent {
    let (kind, payload) = EventKind::SessionCreated(SessionCreated {
        forked: None,
        exec_mode: Some(harness_core::policy::ExecMode::Sandbox),
        no_callbacks: false,
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
        sub_folder: None,
    })
    .into_parts();
    RawEvent {
        session_id: sid,
        seq: 1,
        ts_ms: 1,
        kind,
        payload,
    }
}

fn started(sid: SessionId) -> RawEvent {
    let (kind, payload) = EventKind::TurnStarted(TurnStarted {
        cause: None,
        turn_id: "t1".into(),
        agent: None,
        environment: None,
        continued_by: None,
        retry: None,
    })
    .into_parts();
    RawEvent {
        session_id: sid,
        seq: 2,
        ts_ms: 2,
        kind,
        payload,
    }
}

async fn wait_for(mut f: impl FnMut() -> bool, what: &str) {
    for _ in 0..500 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{what}");
}

/// A running agentic chat of `owner` in `team`, mirrored from node n1.
async fn running(t: &T, owner: &str) -> SessionId {
    let sid = SessionId::new();
    for e in [created(sid), started(sid)] {
        t.tx.send(NodeToMaster::Event { event: e }).await.unwrap();
    }
    let store = t.store.clone();
    wait_for(
        || store.head_seq(sid).unwrap() == 2,
        "the chat was not mirrored",
    )
    .await;
    t.store
        .claim_session(sid, Some(owner), Some(&t.team))
        .unwrap();
    assert!(t.store.session(sid).unwrap().unwrap().running);
    sid
}

async fn start() -> T {
    let store = Store::open_in_memory().unwrap();
    for u in ["bob", "carol"] {
        account(&store, u);
    }
    let team = store
        .create_workspace("Team", "carol")
        .unwrap()
        .workspace_id;
    store.add_member(&team, "bob", "member").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let m = Master::new(
        store.clone(),
        Auth::new(Secret::new(TOKEN.into()), vec![addr.to_string()], false),
    );
    tokio::spawn({
        let m = m.clone();
        async move { axum::serve(listener, router(m)).await.unwrap() }
    });
    // The fake node: records every command and acknowledges it.
    let (tx, from_node) = mpsc::channel::<NodeToMaster>(64);
    let (to_node, mut commands) = mpsc::channel::<MasterToNode>(1024);
    tx.try_send(NodeToMaster::Hello {
        protocol_version: PROTOCOL_VERSION,
        node_id: "n1".into(),
        hostname: "h".into(),
        version: "1".into(),
        capabilities: vec!["exec:sandbox".into()],
        sessions: vec![],
    })
    .unwrap();
    m.add_link(NodeLink { to_node, from_node }, false)
        .await
        .unwrap();
    let seen: Seen = Arc::default();
    tokio::spawn({
        let (seen, tx) = (seen.clone(), tx.clone());
        async move {
            while let Some(c) = commands.recv().await {
                let (what, sid, req) = match c {
                    MasterToNode::Cancel { req_id, session_id } => ("cancel", session_id, req_id),
                    MasterToNode::Delete { req_id, session_id } => ("delete", session_id, req_id),
                    MasterToNode::StopBackground {
                        req_id, session_id, ..
                    } => ("stop", session_id, req_id),
                    _ => continue,
                };
                seen.lock().unwrap().push((what, sid));
                let _ = tx
                    .send(NodeToMaster::Ack {
                        req_id: req,
                        ok: true,
                        error: None,
                    })
                    .await;
            }
        }
    });
    let mut t = T {
        base: format!("http://{addr}"),
        store,
        m,
        seen,
        tx,
        team,
        odd: SessionId::new(),
        carols: SessionId::new(),
    };
    let odd = running(&t, "bob").await;
    t.carols = running(&t, "carol").await;
    // Bob's chat, as a hand edit left it: its id upper-cased everywhere
    // the mirror keys it.
    let low = odd.to_string();
    let up = low.to_uppercase();
    t.store
        .execute_batch_for_test(&format!(
            "UPDATE sessions SET session_id = '{up}' WHERE session_id = '{low}';
             UPDATE session_workspaces SET session_id = '{up}' WHERE session_id = '{low}';
             UPDATE events SET session_id = '{up}' WHERE session_id = '{low}';"
        ))
        .unwrap();
    assert!(t.store.session(odd).unwrap().is_none());
    assert_eq!(raw_count(&t, &up), 1);
    t.odd = odd;
    t
}

/// Rows of `sessions` whose stored id is exactly `stored`.
fn raw_count(t: &T, stored: &str) -> i64 {
    t.store
        .query_i64_for_test(&format!(
            "SELECT COUNT(*) FROM sessions WHERE session_id = '{stored}'"
        ))
        .unwrap()
}

/// What the node was sent for `sid`, after waiting `ms` for more.
async fn sent(t: &T, sid: SessionId, ms: u64) -> Vec<&'static str> {
    tokio::time::sleep(Duration::from_millis(ms)).await;
    t.seen
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, s)| *s == sid)
        .map(|(w, _)| *w)
        .collect()
}

async fn delete(t: &T, path: &str) -> StatusCode {
    reqwest::Client::new()
        .delete(format!("{}{path}", t.base))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn a_removed_members_odd_id_chat_is_stopped_and_the_others_is_not() {
    let t = start().await;
    let st = delete(&t, &format!("/api/workspaces/{}/members/bob", t.team)).await;
    assert_eq!(st, StatusCode::OK);
    let bob = sent(&t, t.odd, 800).await;
    assert!(
        bob.contains(&"cancel"),
        "bob's chat was not stopped: {bob:?}"
    );
    assert!(
        sent(&t, t.carols, 0).await.is_empty(),
        "carol's chat was touched"
    );
}

#[tokio::test]
async fn a_deleted_workspaces_odd_id_chat_is_stopped() {
    let t = start().await;
    let st = delete(&t, &format!("/api/workspaces/{}", t.team)).await;
    assert_eq!(st, StatusCode::OK);
    let bob = sent(&t, t.odd, 800).await;
    assert!(
        bob.contains(&"cancel"),
        "bob's chat was not stopped: {bob:?}"
    );
}

#[tokio::test]
async fn the_revocation_sweep_cancels_an_odd_id_chat_and_only_the_revoked_owners() {
    let t = start().await;
    use harness_core::policy::ExecLevel;
    assert!(
        t.store
            .set_exec_ceiling("bob", ExecLevel::Host, ExecLevel::None, None, "{}")
            .unwrap()
    );
    harness_master::workspaces::cancel_revoked(&t.m, &["bob".to_string()]).await;
    let bob = sent(&t, t.odd, 800).await;
    assert!(
        bob.contains(&"cancel"),
        "bob's turn was not cancelled: {bob:?}"
    );
    assert!(
        sent(&t, t.carols, 0).await.is_empty(),
        "carol's chat was touched"
    );
}

#[tokio::test]
async fn a_deleted_accounts_odd_id_chat_is_deleted_on_its_node_and_in_the_mirror() {
    let t = start().await;
    let up = t.odd.to_string().to_uppercase();
    let st = delete(&t, "/api/users/bob").await;
    assert_eq!(st, StatusCode::OK);
    let bob = sent(&t, t.odd, 800).await;
    assert!(
        bob.contains(&"delete"),
        "bob's chat was not deleted on its node: {bob:?}"
    );
    let store = t.store.clone();
    let mut gone = false;
    for _ in 0..200 {
        if raw_count(&t, &up) == 0 {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(gone, "bob's chat is still in the mirror");
    assert!(!sent(&t, t.carols, 0).await.contains(&"delete"));
    assert!(
        store.session(t.carols).unwrap().is_some(),
        "carol's chat went"
    );
}
