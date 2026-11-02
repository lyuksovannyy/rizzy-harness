//! Full-text index over chats: filled on append, incognito excluded,
//! rebuildable from the log, filtered by owner.

use harness_core::{RawEvent, SessionId};
use harness_store::{HIT_END, HIT_START, Store};
use serde_json::{Value, json};

fn ev(sid: SessionId, seq: u64, kind: &str, payload: Value) -> RawEvent {
    RawEvent {
        session_id: sid,
        seq,
        ts_ms: seq as i64,
        kind: kind.into(),
        payload,
    }
}

fn chat(s: &Store, owner: &str, title: &str, msgs: &[&str], incognito: bool) -> SessionId {
    let sid = SessionId::new();
    let mut p = json!({"title": title, "cwd": "", "model": "m", "backend": "native", "node_id": "master", "profile": "chat"});
    if incognito {
        p["incognito"] = json!(true);
    }
    s.append("master", &ev(sid, 1, "session_created", p))
        .unwrap();
    for (i, m) in msgs.iter().enumerate() {
        let kind = if i % 2 == 0 {
            "user_message"
        } else {
            "assistant_message"
        };
        s.append(
            "master",
            &ev(
                sid,
                i as u64 + 2,
                kind,
                json!({"turn_id": "t", "message_id": format!("m{i}"), "text": m}),
            ),
        )
        .unwrap();
    }
    s.set_session_owner(sid, owner).unwrap();
    sid
}

#[test]
fn indexes_on_append_and_ranks_titles_higher() {
    let s = Store::open_in_memory().unwrap();
    let a = chat(
        &s,
        "u1",
        "Tokio runtime config",
        &["how many worker threads", "use the builder"],
        false,
    );
    let b = chat(
        &s,
        "u1",
        "Lunch ideas",
        &["something with tokio mentioned in passing"],
        false,
    );
    let hits = s.search("\"tokio\"", None, 10).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].session_id, a, "title match ranks first");
    assert_eq!(hits[1].session_id, b);
    assert!(hits[0].snippet.contains(HIT_START) && hits[0].snippet.contains(HIT_END));
    // Assistant text is indexed too.
    let hits = s.search("\"builder\"", None, 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].session_id, a);
    assert!(s.session_search_text(a).unwrap().contains("worker threads"));
}

#[test]
fn incognito_never_indexed_and_owner_filter() {
    let s = Store::open_in_memory().unwrap();
    chat(&s, "u1", "secret plan", &["the secret plan is here"], true);
    let mine = chat(&s, "u1", "public plan", &["nothing to hide"], false);
    chat(&s, "u2", "another plan", &["bob's plan"], false);
    assert!(s.search("\"secret\"", None, 10).unwrap().is_empty());
    let hits = s.search("\"plan\"", Some("u1"), 10).unwrap();
    assert_eq!(
        hits.iter().map(|h| h.session_id).collect::<Vec<_>>(),
        vec![mine]
    );
    assert_eq!(s.search("\"plan\"", None, 10).unwrap().len(), 2);
}

#[test]
fn rebuild_matches_live_index() {
    let s = Store::open_in_memory().unwrap();
    chat(
        &s,
        "u1",
        "Rust lifetimes",
        &["explain lifetimes", "they track borrows"],
        false,
    );
    chat(&s, "u1", "hidden", &["lifetimes secret"], true);
    let before = s.search("\"lifetimes\"", None, 10).unwrap();
    s.rebuild_search_index().unwrap();
    let after = s.search("\"lifetimes\"", None, 10).unwrap();
    assert_eq!(before, after);
    assert_eq!(after.len(), 1);
}
