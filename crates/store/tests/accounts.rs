//! Accounts and UI sessions (PLAN.md §0: single owner now, teams later).

use harness_core::{RawEvent, SessionId};
use harness_store::{NewUser, Role, Store, StoreError, UserPatch};
use serde_json::json;

fn user(id: &str, name: &str, role: Role) -> NewUser {
    NewUser {
        user_id: id.into(),
        username: name.into(),
        password_hash: "$argon2id$x".into(),
        role,
        can_agentic: false,
        must_change_password: false,
    }
}

#[test]
fn users_are_unique_and_counted() {
    let s = Store::open_in_memory().unwrap();
    assert_eq!(s.user_count().unwrap(), 0);
    s.create_user(&user("u1", "alex", Role::Admin)).unwrap();
    assert_eq!(s.user_count().unwrap(), 1);
    let dup = s.create_user(&user("u2", "alex", Role::Member));
    assert!(matches!(dup, Err(StoreError::Conflict(_))), "{dup:?}");
    let u = s.user_by_username("alex").unwrap().unwrap();
    assert_eq!(u.user_id, "u1");
    assert_eq!(u.role, Role::Admin);
    assert!(s.user_by_username("nobody").unwrap().is_none());
}

#[test]
fn ui_sessions_expire_and_revoke() {
    let s = Store::open_in_memory().unwrap();
    s.create_user(&user("u1", "alex", Role::Admin)).unwrap();
    s.ui_session_create("hash-a", "u1", 1_000).unwrap();
    assert_eq!(
        s.ui_session_user("hash-a", 999).unwrap().unwrap().username,
        "alex"
    );
    assert!(
        s.ui_session_user("hash-a", 1_000).unwrap().is_none(),
        "expired"
    );
    s.ui_session_create("hash-b", "u1", 5_000).unwrap();
    s.ui_session_delete("hash-b").unwrap();
    assert!(s.ui_session_user("hash-b", 0).unwrap().is_none());
}

#[test]
fn session_owner_is_recorded() {
    let s = Store::open_in_memory().unwrap();
    let sid = SessionId::new();
    s.append(
        "local",
        &RawEvent {
            session_id: sid,
            seq: 1,
            ts_ms: 1,
            kind: "turn_started".into(),
            payload: json!({"turn_id": "t"}),
        },
    )
    .unwrap();
    assert_eq!(s.sessions().unwrap()[0].owner_id, None);
    s.claim_session(sid, Some("u1"), None).unwrap();
    assert_eq!(s.sessions().unwrap()[0].owner_id.as_deref(), Some("u1"));
}

#[test]
fn session_row_tracks_agent_and_profile() {
    let s = Store::open_in_memory().unwrap();
    let sid = SessionId::new();
    let ev = |seq: u64, kind: &str, payload: serde_json::Value| RawEvent {
        session_id: sid,
        seq,
        ts_ms: seq as i64,
        kind: kind.into(),
        payload,
    };
    s.append(
        "master",
        &ev(1, "session_created", json!({"title": "t", "cwd": "", "model": "m", "backend": "native", "node_id": "master", "profile": "chat", "agent_id": "general", "approval_mode": "ask"})),
    )
    .unwrap();
    let r = &s.sessions().unwrap()[0];
    assert_eq!(r.agent_id.as_deref(), Some("general"));
    assert_eq!(r.profile.as_deref(), Some("chat"));
    s.append(
        "master",
        &ev(
            2,
            "agent_changed",
            json!({"agent": {"agent_id": "writer", "name": "Writer", "prompt_hash": "h"}}),
        ),
    )
    .unwrap();
    assert_eq!(s.sessions().unwrap()[0].agent_id.as_deref(), Some("writer"));
}

#[test]
fn user_crud_flags_and_admin_count() {
    let s = Store::open_in_memory().unwrap();
    s.create_user(&user("a1", "admin", Role::Admin)).unwrap();
    s.create_user(&NewUser {
        must_change_password: true,
        ..user("m1", "bob", Role::Member)
    })
    .unwrap();
    assert_eq!(s.active_admin_count().unwrap(), 1);
    let bob = s.user_by_id("m1").unwrap().unwrap();
    assert!(bob.must_change_password && !bob.can_agentic && !bob.disabled);

    s.update_user(
        "m1",
        &UserPatch {
            can_agentic: Some(true),
            role: Some(Role::Admin),
            ..Default::default()
        },
    )
    .unwrap();
    let bob = s.user_by_id("m1").unwrap().unwrap();
    assert!(bob.can_agentic);
    assert_eq!(bob.role, Role::Admin);
    assert_eq!(s.active_admin_count().unwrap(), 2);

    s.update_user(
        "m1",
        &UserPatch {
            disabled: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        s.active_admin_count().unwrap(),
        1,
        "disabled admins do not count"
    );

    s.set_password_hash("m1", "$argon2id$new", false).unwrap();
    let bob = s.user_by_id("m1").unwrap().unwrap();
    assert_eq!(bob.password_hash, "$argon2id$new");
    assert!(!bob.must_change_password);

    let names: Vec<_> = s
        .list_users()
        .unwrap()
        .into_iter()
        .map(|u| u.username)
        .collect();
    assert_eq!(names, ["admin", "bob"]);
    assert!(s.update_user("nope", &UserPatch::default()).is_err());
    s.delete_user("m1").unwrap();
    assert!(s.user_by_id("m1").unwrap().is_none());
}

#[test]
fn disabled_or_deleted_users_lose_sessions() {
    let s = Store::open_in_memory().unwrap();
    s.create_user(&user("m1", "bob", Role::Member)).unwrap();
    s.ui_session_create("h1", "m1", i64::MAX).unwrap();
    s.ui_session_create("h2", "m1", i64::MAX).unwrap();
    s.update_user(
        "m1",
        &UserPatch {
            disabled: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        s.ui_session_user("h1", 0).unwrap().is_none(),
        "disabled user"
    );
    s.update_user(
        "m1",
        &UserPatch {
            disabled: Some(false),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(s.ui_session_user("h1", 0).unwrap().is_some());
    assert_eq!(s.ui_sessions_delete_for_user("m1").unwrap(), 2);
    assert!(s.ui_session_user("h1", 0).unwrap().is_none());
    s.ui_session_create("h3", "m1", i64::MAX).unwrap();
    s.delete_user("m1").unwrap();
    assert!(s.ui_session_user("h3", 0).unwrap().is_none(), "cascade");
}
