//! Accounts and UI sessions (PLAN.md §0: single owner now, teams later).

use harness_core::{RawEvent, SessionId};
use harness_store::{Role, Store, StoreError};
use serde_json::json;

#[test]
fn users_are_unique_and_counted() {
    let s = Store::open_in_memory().unwrap();
    assert_eq!(s.user_count().unwrap(), 0);
    s.create_user("u1", "alex", "$argon2id$x", Role::Owner)
        .unwrap();
    assert_eq!(s.user_count().unwrap(), 1);
    let dup = s.create_user("u2", "alex", "$argon2id$y", Role::Member);
    assert!(matches!(dup, Err(StoreError::Conflict(_))), "{dup:?}");
    let u = s.user_by_username("alex").unwrap().unwrap();
    assert_eq!(u.user_id, "u1");
    assert_eq!(u.role, Role::Owner);
    assert!(s.user_by_username("nobody").unwrap().is_none());
}

#[test]
fn ui_sessions_expire_and_revoke() {
    let s = Store::open_in_memory().unwrap();
    s.create_user("u1", "alex", "h", Role::Owner).unwrap();
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
    s.set_session_owner(sid, "u1").unwrap();
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
        &ev(1, "session_created", json!({"title": "t", "cwd": "", "model": "m", "backend": "native", "node_id": "master", "profile": "chat", "agent_id": "general"})),
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

/// A master.db created before accounts existed gains the new column on open.
#[test]
fn migrates_v1_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("master.db");
    {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch(
            "CREATE TABLE sessions (session_id TEXT PRIMARY KEY, node_id TEXT NOT NULL,
             title TEXT NOT NULL DEFAULT '', cwd TEXT NOT NULL DEFAULT '', model TEXT NOT NULL DEFAULT '',
             created_ms INTEGER NOT NULL, updated_ms INTEGER NOT NULL, head_seq INTEGER NOT NULL,
             running INTEGER NOT NULL DEFAULT 0);
             INSERT INTO sessions VALUES ('0193f2a0-0000-7000-8000-000000000001','local','old','/','m',1,1,0,0);",
        )
        .unwrap();
    }
    let s = Store::open(&path, harness_store::Durability::Normal).unwrap();
    let rows = s.sessions().unwrap();
    assert_eq!(rows[0].title, "old");
    assert_eq!(rows[0].owner_id, None);
    // Reopening is idempotent.
    drop(s);
    Store::open(&path, harness_store::Durability::Normal).unwrap();
}
