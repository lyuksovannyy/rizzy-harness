//! A workspace's custom connectors and each person's account for one
//! (ADOPTION_PLAN X.35 step 1, PLAN.md T84): capped, unique by name, read
//! by (workspace, id) and (person, workspace, id) only; an edited URL or
//! sign-in removes every account in the same transaction; accounts go
//! with their connector, membership, person and workspace.

use harness_store::custom_connectors::{AccountPut, ConnectorAdd, ConnectorRow};
use harness_store::{NewUser, Role, Store};

fn user(s: &Store, id: &str) {
    s.create_user(&NewUser {
        user_id: id.into(),
        username: id.into(),
        password_hash: "$argon2id$x".into(),
        role: Role::Member,
        exec_ceiling: harness_core::policy::ExecLevel::None,
        must_change_password: false,
    })
    .unwrap();
}

fn connector(id: &str, ws: &str, name: &str, url: &str) -> ConnectorRow {
    ConnectorRow {
        connector_id: id.into(),
        workspace_id: ws.into(),
        name: name.into(),
        url: url.into(),
        auth: "token".into(),
        header: None,
        created_ms: 0,
        updated_ms: 0,
    }
}

const U1: &str = "https://mcp.example.com/a";
const U2: &str = "https://mcp.example.com/b";

/// (store, team workspace, other team workspace) with alice and bob in the
/// first, carol in the second.
fn setup() -> (Store, String, String) {
    let s = Store::open_in_memory().unwrap();
    for u in ["alice", "bob", "carol"] {
        user(&s, u);
    }
    let team = s.create_workspace("Team", "alice").unwrap().workspace_id;
    let other = s.create_workspace("Other", "carol").unwrap().workspace_id;
    assert!(s.add_member(&team, "bob", "member").unwrap());
    (s, team, other)
}

#[test]
fn connectors_are_unique_capped_and_kept_per_workspace() {
    let (s, team, other) = setup();
    assert_eq!(
        s.connector_add(&connector("k1", &team, "linear", U1), 2)
            .unwrap(),
        ConnectorAdd::Added
    );
    assert_eq!(
        s.connector_add(&connector("k2", &team, "linear", U2), 2)
            .unwrap(),
        ConnectorAdd::NameTaken
    );
    assert_eq!(
        s.connector_add(&connector("k2", &team, "notion", U2), 2)
            .unwrap(),
        ConnectorAdd::Added
    );
    assert_eq!(
        s.connector_add(&connector("k3", &team, "third", U2), 2)
            .unwrap(),
        ConnectorAdd::OverCap
    );
    // Another workspace has its own names and its own cap.
    assert_eq!(
        s.connector_add(&connector("k4", &other, "linear", U1), 2)
            .unwrap(),
        ConnectorAdd::Added
    );
    assert_eq!(
        s.connector_add(&connector("k5", "nope", "x", U1), 2)
            .unwrap(),
        ConnectorAdd::NoWorkspace
    );
    assert_eq!(s.connectors_of(&team).unwrap().len(), 2);
    // By (workspace, id): another workspace's id is nobody's here.
    assert!(s.connector(&team, "k4").unwrap().is_none());
    assert!(!s.connector_delete(&team, "k4").unwrap());
    assert!(
        s.connector_update(&team, "k4", U2, "none", None)
            .unwrap()
            .is_none()
    );
    assert!(s.connector(&other, "k4").unwrap().is_some());
    // The table refuses what the master refuses.
    for bad in [
        connector("x1", &team, "", U1),
        connector("x2", &team, &"n".repeat(33), U1),
        ConnectorRow {
            auth: "oauth".into(),
            ..connector("x3", &team, "x3", U1)
        },
        connector(
            "x4",
            &team,
            "x4",
            &format!("https://e.com/{}", "a".repeat(520)),
        ),
    ] {
        assert!(s.connector_add(&bad, 50).is_err(), "{bad:?}");
    }
}

#[test]
fn accounts_are_per_person_checked_against_the_url_and_capped() {
    let (s, team, other) = setup();
    s.connector_add(&connector("k1", &team, "linear", U1), 50)
        .unwrap();
    s.connector_add(&connector("k2", &team, "notion", U1), 50)
        .unwrap();
    s.connector_add(&connector("k9", &other, "linear", U1), 50)
        .unwrap();
    let put = |u: &str, ws: &str, id: &str, url: &str, b: &[u8], cap| {
        s.connector_account_put(u, ws, id, url, b, cap).unwrap()
    };
    assert_eq!(put("alice", &team, "k1", U1, b"a1", 1), AccountPut::Added);
    assert_eq!(
        put("alice", &team, "k1", U1, b"a2", 1),
        AccountPut::Replaced
    );
    assert_eq!(put("alice", &team, "k2", U1, b"a3", 1), AccountPut::OverCap);
    assert_eq!(put("bob", &team, "k1", U1, b"b1", 5), AccountPut::Added);
    // A token checked against another URL is not stored.
    assert_eq!(put("bob", &team, "k2", U2, b"b2", 5), AccountPut::Changed);
    // Not a member there; another workspace's connector.
    assert_eq!(
        put("carol", &team, "k1", U1, b"c", 5),
        AccountPut::NotMember
    );
    assert_eq!(put("alice", &team, "k9", U1, b"x", 5), AccountPut::Changed);
    assert_eq!(
        put("alice", &other, "k9", U1, b"x", 5),
        AccountPut::NotMember
    );
    // Read by the person only.
    assert_eq!(
        s.connector_account_sealed("alice", &team, "k1").unwrap(),
        Some(b"a2".to_vec())
    );
    assert_eq!(s.connector_accounts_of("alice", &team).unwrap().len(), 1);
    assert!(
        s.connector_account_sealed("carol", &team, "k1")
            .unwrap()
            .is_none()
    );
    assert!(!s.connector_account_delete("carol", &team, "k1").unwrap());
    assert!(
        s.connector_account_sealed("alice", &other, "k1")
            .unwrap()
            .is_none()
    );
    assert_eq!(s.sealed_count().unwrap(), 2);
    let samples = s.sealed_samples(5).unwrap();
    let a = samples.iter().find(|x| x.owner == "alice").unwrap();
    assert_eq!(a.table, "connector_accounts");
    assert_eq!(a.fields, [team.clone(), "k1".to_string(), U1.to_string()]);
    // A connector without sign-in takes no token.
    s.connector_update(&team, "k2", U1, "none", None).unwrap();
    assert_eq!(put("bob", &team, "k2", U1, b"b3", 5), AccountPut::Changed);
}

#[test]
fn an_edited_url_or_sign_in_removes_every_account_at_once() {
    let (s, team, _) = setup();
    s.connector_add(&connector("k1", &team, "linear", U1), 50)
        .unwrap();
    for u in ["alice", "bob"] {
        s.connector_account_put(u, &team, "k1", U1, b"t", 50)
            .unwrap();
    }
    // The same definition again keeps them; a new header does not.
    let (_, removed) = s
        .connector_update(&team, "k1", U1, "token", None)
        .unwrap()
        .unwrap();
    assert_eq!(removed, 0);
    let (r, removed) = s
        .connector_update(&team, "k1", U1, "token", Some("x-api-key"))
        .unwrap()
        .unwrap();
    assert_eq!((removed, r.header.as_deref()), (2, Some("x-api-key")));
    assert_eq!(s.sealed_count().unwrap(), 0);
    for u in ["alice", "bob"] {
        s.connector_account_put(u, &team, "k1", U1, b"t", 50)
            .unwrap();
    }
    let (r, removed) = s
        .connector_update(&team, "k1", U2, "token", None)
        .unwrap()
        .unwrap();
    assert_eq!((removed, r.url.as_str()), (2, U2));
    assert_eq!(s.sealed_count().unwrap(), 0);
    s.connector_account_put("alice", &team, "k1", U2, b"t", 50)
        .unwrap();
    let (_, removed) = s
        .connector_update(&team, "k1", U2, "none", None)
        .unwrap()
        .unwrap();
    assert_eq!(removed, 1);
}

#[test]
fn accounts_go_with_their_connector_membership_person_and_workspace() {
    let (s, team, other) = setup();
    s.connector_add(&connector("k1", &team, "linear", U1), 50)
        .unwrap();
    s.connector_add(&connector("k2", &team, "notion", U1), 50)
        .unwrap();
    s.connector_add(&connector("k9", &other, "linear", U1), 50)
        .unwrap();
    for (u, ws, k) in [
        ("alice", &team, "k1"),
        ("alice", &team, "k2"),
        ("bob", &team, "k1"),
        ("bob", &team, "k2"),
        ("carol", &other, "k9"),
    ] {
        assert_eq!(
            s.connector_account_put(u, ws, k, U1, b"t", 50).unwrap(),
            AccountPut::Added
        );
    }
    assert!(s.connector_delete(&team, "k2").unwrap());
    assert_eq!(s.sealed_count().unwrap(), 3);
    assert!(s.remove_member(&team, "bob", None).unwrap());
    assert!(s.connector_accounts_of("bob", &team).unwrap().is_empty());
    assert_eq!(s.sealed_count().unwrap(), 2);
    s.delete_user("carol").unwrap();
    assert_eq!(s.sealed_count().unwrap(), 1);
    assert!(s.delete_workspace(&team).unwrap());
    assert!(s.connectors_of(&team).unwrap().is_empty());
    assert_eq!(s.sealed_count().unwrap(), 0);
}

fn chat_with_parent(s: &Store, parent: Option<harness_core::SessionId>) -> harness_core::SessionId {
    let sid = harness_core::SessionId::new();
    let mut p = serde_json::json!({"title": "t", "cwd": "", "model": "m", "backend": "native",
        "node_id": "n", "profile": "agentic", "approval_mode": "ask"});
    if let Some(parent) = parent {
        p["parent_session"] = serde_json::json!(parent.to_string());
    }
    s.append(
        "n",
        &harness_core::RawEvent {
            session_id: sid,
            seq: 1,
            ts_ms: 1,
            kind: "session_created".into(),
            payload: p,
        },
    )
    .unwrap();
    sid
}

/// Review 4 of PR 157 (M2, L6): a mark reaches every ancestor up to the
/// root, and a chat is foreign when it or any ancestor is marked or holds
/// external event data, at any depth.
#[test]
fn the_mark_and_the_untrusted_data_follow_every_ancestor() {
    let (s, _, _) = setup();
    let root = chat_with_parent(&s, None);
    let kid = chat_with_parent(&s, Some(root));
    let gk = chat_with_parent(&s, Some(kid));
    let ggk = chat_with_parent(&s, Some(gk));
    let other = chat_with_parent(&s, None);
    // A mark three levels down reaches the root.
    s.chat_mark_foreign(ggk).unwrap();
    for c in [ggk, gk, kid, root] {
        assert!(s.chat_foreign(c).unwrap(), "{c}");
    }
    assert!(!s.chat_foreign(other).unwrap());
    // A sub-agent of a marked root, two levels down, is foreign by its
    // ancestors alone (nothing marked it).
    let root2 = chat_with_parent(&s, None);
    let k2 = chat_with_parent(&s, Some(root2));
    let gk2 = chat_with_parent(&s, Some(k2));
    s.chat_mark_foreign(root2).unwrap();
    assert!(s.chat_foreign(gk2).unwrap(), "by its ancestor");
    assert_eq!(
        s.chat_foreign_reason(gk2).unwrap(),
        Some(harness_store::custom_connectors::ForeignReason::ParentChat)
    );
    assert_eq!(
        s.chat_foreign_reason(root2).unwrap(),
        Some(harness_store::custom_connectors::ForeignReason::ThisChat)
    );
    // External event data on an ancestor two levels up.
    let root3 = chat_with_parent(&s, None);
    let k3 = chat_with_parent(&s, Some(root3));
    let gk3 = chat_with_parent(&s, Some(k3));
    s.mark_untrusted_for_test(root3).unwrap();
    assert!(s.chat_untrusted(gk3).unwrap(), "webhook data two levels up");
    assert!(s.chat_foreign(gk3).unwrap());
    assert_eq!(
        s.chat_foreign_reason(gk3).unwrap(),
        Some(harness_store::custom_connectors::ForeignReason::EventData)
    );
    assert!(!s.chat_foreign(other).unwrap());
    assert_eq!(s.chat_foreign_reason(other).unwrap(), None);
}

/// Review 5 of PR 157, L1: a chain longer than the walk reads as foreign
/// all along it from the 16th level down, and a mark in it is written to
/// every chat the walk reached and then refused, so the caller forwards
/// nothing (no chain leaves a root unmarked after a forwarded action).
#[test]
fn a_chain_too_deep_to_walk_is_foreign_and_its_mark_is_refused() {
    let (s, _, _) = setup();
    let mut chain = vec![chat_with_parent(&s, None)];
    for _ in 0..17 {
        let up = *chain.last().unwrap();
        chain.push(chat_with_parent(&s, Some(up)));
    }
    assert!(
        !s.chat_foreign(chain[15]).unwrap(),
        "15 levels: walked whole"
    );
    assert!(s.chat_foreign(chain[16]).unwrap(), "16 levels: too deep");
    assert!(s.chat_foreign(chain[17]).unwrap());
    // The clean part of the chain can still be marked.
    let short = chat_with_parent(&s, Some(chain[3]));
    s.chat_mark_foreign(short).unwrap();
    assert!(s.chat_foreign(chain[0]).unwrap());
    let (s, _, _) = setup();
    let mut chain = vec![chat_with_parent(&s, None)];
    for _ in 0..17 {
        let up = *chain.last().unwrap();
        chain.push(chat_with_parent(&s, Some(up)));
    }
    assert!(s.chat_mark_foreign(chain[17]).is_err(), "refused");
    assert!(
        s.chat_foreign_reason(chain[1]).unwrap()
            == Some(harness_store::custom_connectors::ForeignReason::ThisChat)
    );
    assert!(
        !s.chat_foreign(chain[0]).unwrap(),
        "the walk never reached it"
    );
}

/// Review 5 of PR 157, L2: deleting a chat in the middle of a chain hands
/// down what its sub-agents' chats inherited through it, by the ancestor
/// rule: a mark or external event data above the deleted chat.
#[test]
fn deleting_a_middle_chat_keeps_what_its_sub_agents_inherited() {
    let (s, _, _) = setup();
    // A mark on the root, inherited by the grandchild through the child.
    let root = chat_with_parent(&s, None);
    let kid = chat_with_parent(&s, Some(root));
    let gk = chat_with_parent(&s, Some(kid));
    s.chat_mark_foreign(root).unwrap();
    assert!(s.chat_foreign(gk).unwrap());
    assert!(s.delete_session(kid, false).unwrap());
    assert!(
        s.chat_foreign(gk).unwrap(),
        "the mark above stays inherited"
    );
    // External event data on the root, the same.
    let root = chat_with_parent(&s, None);
    let kid = chat_with_parent(&s, Some(root));
    let gk = chat_with_parent(&s, Some(kid));
    s.mark_untrusted_for_test(root).unwrap();
    assert!(s.delete_session(kid, false).unwrap());
    assert!(
        s.chat_untrusted(gk).unwrap(),
        "the event data stays inherited"
    );
    assert!(s.chat_foreign(gk).unwrap());
    // A clean chain stays clean.
    let root = chat_with_parent(&s, None);
    let kid = chat_with_parent(&s, Some(root));
    let gk = chat_with_parent(&s, Some(kid));
    assert!(s.delete_session(kid, false).unwrap());
    assert!(!s.chat_foreign(gk).unwrap());
    assert!(!s.chat_untrusted(gk).unwrap());
}
