//! Full-text index over chats: filled on append, incognito and sub-agent
//! chats excluded, rebuildable from the log, filtered by owner.

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
    let mut p = json!({"title": title, "cwd": "", "model": "m", "backend": "native", "node_id": "master", "profile": "chat", "approval_mode": "ask"});
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
    s.claim_session(sid, Some(owner), None).unwrap();
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
    let hits = s.search("\"tokio\"", None, None, 10).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].session_id, a, "title match ranks first");
    assert_eq!(hits[1].session_id, b);
    assert!(hits[0].snippet.contains(HIT_START) && hits[0].snippet.contains(HIT_END));
    // Assistant text is indexed too.
    let hits = s.search("\"builder\"", None, None, 10).unwrap();
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
    assert!(s.search("\"secret\"", None, None, 10).unwrap().is_empty());
    let hits = s.search("\"plan\"", Some("u1"), None, 10).unwrap();
    assert_eq!(
        hits.iter().map(|h| h.session_id).collect::<Vec<_>>(),
        vec![mine]
    );
    assert_eq!(s.search("\"plan\"", None, None, 10).unwrap().len(), 2);
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
    let before = s.search("\"lifetimes\"", None, None, 10).unwrap();
    s.rebuild_search_index().unwrap();
    let after = s.search("\"lifetimes\"", None, None, 10).unwrap();
    assert_eq!(before, after);
    assert_eq!(after.len(), 1);
}

#[test]
fn latest_message_texts_skip_empty_messages() {
    let s = Store::open_in_memory().unwrap();
    let answered = chat(&s, "u1", "t", &["first question", "the answer"], false);
    // A tool-only turn ends in an assistant message without text.
    let tools = chat(&s, "u1", "t", &["run the build", "", "  "], false);
    let none = chat(&s, "u1", "t", &[], false);
    // Only the last four messages are read: older text is not looked for.
    let quiet = chat(&s, "u1", "t", &["old words", "", "", "", ""], false);
    let got = s
        .latest_message_texts(&[answered, tools, none, quiet])
        .unwrap();
    assert_eq!(got.get(&answered).map(String::as_str), Some("the answer"));
    assert_eq!(got.get(&tools).map(String::as_str), Some("run the build"));
    assert!(!got.contains_key(&none));
    assert!(!got.contains_key(&quiet));
    assert_eq!(got.len(), 2);
}

#[test]
fn search_never_finds_sub_agent_or_incognito_chats() {
    let s = Store::open_in_memory().unwrap();
    let parent = chat(&s, "u1", "plan the garden", &["beans and kale"], false);
    chat(&s, "u1", "secret garden", &["hidden beans"], true);
    // A delegate's session, named after its parent (as the node writes it).
    let sub = SessionId::new();
    s.append(
        "master",
        &ev(
            sub,
            1,
            "session_created",
            json!({"title": "@scout: beans", "cwd": "", "model": "m", "backend": "native", "node_id": "master", "profile": "chat", "approval_mode": "ask", "parent_session": parent.to_string()}),
        ),
    )
    .unwrap();
    s.append(
        "master",
        &ev(
            sub,
            2,
            "assistant_message",
            json!({"turn_id": "t", "message_id": "m", "text": "beans grow fast"}),
        ),
    )
    .unwrap();
    assert_eq!(
        s.session(sub).unwrap().unwrap().parent_id,
        Some(parent.to_string())
    );
    // Indexed, but its parent chat is the hit; nothing else.
    assert!(s.session_search_text(sub).unwrap().contains("grow fast"));
    let hits = s.search("\"beans\"", Some("u1"), None, 10).unwrap();
    assert_eq!(
        hits.iter().map(|h| h.session_id).collect::<Vec<_>>(),
        vec![parent]
    );
    assert!(s.search("\"grow\"", None, None, 10).unwrap().is_empty());
}

/// A renamed chat lists and is found under its new title, not its old one,
/// also after the index is rebuilt from the log (which keeps the title the
/// chat was created with); an incognito chat's new title is not indexed.
#[test]
fn a_rename_changes_the_row_and_the_index() {
    let s = Store::open_in_memory().unwrap();
    let a = chat(&s, "u1", "Lunch ideas", &["pasta"], false);
    assert!(s.rename_session(a, "Quarterly zeppelin budget").unwrap());
    assert_eq!(
        s.session(a).unwrap().unwrap().title,
        "Quarterly zeppelin budget"
    );
    let found = |q: &str| s.search(q, None, None, 10).unwrap().len();
    assert_eq!(found("\"zeppelin\""), 1);
    assert_eq!(found("\"lunch\""), 0);
    assert_eq!(found("\"pasta\""), 1, "the messages stay indexed");
    s.rebuild_search_index().unwrap();
    assert_eq!(found("\"zeppelin\""), 1, "after a rebuild");
    assert_eq!(found("\"lunch\""), 0, "after a rebuild");
    // Cut to the row's length.
    assert!(s.rename_session(a, &"x".repeat(500)).unwrap());
    assert_eq!(
        s.session(a).unwrap().unwrap().title.chars().count(),
        harness_store::MAX_TITLE_CHARS
    );
    let inc = chat(&s, "u1", "Private", &[], true);
    assert!(s.rename_session(inc, "Zeppelin secret").unwrap());
    assert_eq!(found("\"secret\""), 0);
    assert!(
        !s.rename_session(SessionId::new(), "x").unwrap(),
        "no such chat"
    );
}

/// A failed turn and the person's Retry of it (X.21): the retry writes no
/// second message, so the index holds the message once and a search finds
/// the chat once; the retry's `turn_started` (its error included) is not
/// indexed.
#[test]
fn a_retry_indexes_its_message_once() {
    let s = Store::open_in_memory().unwrap();
    let sid = chat(&s, "u1", "Schema work", &[], false);
    let evs = [
        ("turn_started", json!({"turn_id": "t1"})),
        (
            "user_message",
            json!({"turn_id": "t1", "text": "migrate the invoices table"}),
        ),
        (
            "error",
            json!({"turn_id": "t1", "message": "invoices unavailable"}),
        ),
        ("turn_ended", json!({"turn_id": "t1", "reason": "error"})),
        (
            "turn_started",
            json!({"turn_id": "t2", "retry": {"turn_id": "t1", "reason": "error", "error": "invoices unavailable"}}),
        ),
        (
            "assistant_message",
            json!({"turn_id": "t2", "message_id": "m1", "text": "Done."}),
        ),
        (
            "turn_ended",
            json!({"turn_id": "t2", "reason": "completed"}),
        ),
    ];
    for (i, (kind, p)) in evs.into_iter().enumerate() {
        s.append("master", &ev(sid, i as u64 + 2, kind, p)).unwrap();
    }
    let text = s.session_search_text(sid).unwrap();
    assert_eq!(text.matches("invoices").count(), 1, "{text}");
    assert_eq!(s.search("\"invoices\"", None, None, 10).unwrap().len(), 1);
}

/// A user `u1` and their team workspace.
fn team(s: &Store) -> String {
    s.create_user(&harness_store::NewUser {
        user_id: "u1".into(),
        username: "u1".into(),
        password_hash: "$argon2id$x".into(),
        role: harness_store::Role::Member,
        exec_ceiling: harness_core::policy::ExecLevel::None,
        must_change_password: false,
    })
    .unwrap();
    s.create_workspace("Team", "u1").unwrap().workspace_id
}

/// A chat with two finished turns, and the events a rewind to the second
/// message adds. Returns (chat, seq of the second message, next seq).
fn two_turns(s: &Store, incognito: bool, ws: &str) -> (SessionId, u64, u64) {
    let sid = SessionId::new();
    let mut p = json!({"title": "Schema work", "cwd": "", "model": "m", "backend": "native", "node_id": "master", "profile": "chat", "approval_mode": "ask"});
    if incognito {
        p["incognito"] = json!(true);
    }
    s.append("master", &ev(sid, 1, "session_created", p))
        .unwrap();
    s.claim_session(sid, Some("u1"), Some(ws)).unwrap();
    let evs = [
        ("turn_started", json!({"turn_id": "t1"})),
        (
            "user_message",
            json!({"turn_id": "t1", "text": "keep the invoices table"}),
        ),
        (
            "assistant_message",
            json!({"turn_id": "t1", "message_id": "m1", "text": "Kept it."}),
        ),
        (
            "turn_ended",
            json!({"turn_id": "t1", "reason": "completed"}),
        ),
        ("turn_started", json!({"turn_id": "t2"})),
        (
            "user_message",
            json!({"turn_id": "t2", "text": "now drop the zebrafish column"}),
        ),
        (
            "assistant_message",
            json!({"turn_id": "t2", "message_id": "m2", "text": "Dropped zebrafish."}),
        ),
        (
            "turn_ended",
            json!({"turn_id": "t2", "reason": "completed"}),
        ),
    ];
    for (i, (kind, p)) in evs.into_iter().enumerate() {
        s.append("master", &ev(sid, i as u64 + 2, kind, p)).unwrap();
    }
    // seqs: created 1, t1 start 2, message 3, reply 4, end 5, t2 start 6,
    // message 7, reply 8, end 9.
    (sid, 7, 10)
}

/// A rewind takes the rewound turns' text out of search and recall (the
/// model must not reach it through another chat); the undo puts it back;
/// what was before the cut stays throughout (X.46, T90).
#[test]
fn a_rewind_takes_its_text_out_of_the_index_and_an_undo_puts_it_back() {
    let s = Store::open_in_memory().unwrap();
    let ws = team(&s);
    let (sid, msg, next) = two_turns(&s, false, &ws);
    let hits = |q: &str| s.search(q, None, None, 10).unwrap().len();
    let recall = |q: &str| {
        s.recall_search(
            &[q.to_string()],
            "u1",
            &ws,
            [SessionId::new(), SessionId::new()],
            10,
        )
        .unwrap()
        .len()
    };
    assert_eq!(hits("\"zebrafish\""), 1);
    assert_eq!(recall("\"zebrafish\""), 1);
    s.append(
        "master",
        &ev(
            sid,
            next,
            "conversation_rewound",
            json!({"message_seq": msg, "turn_id": "t2", "files": false}),
        ),
    )
    .unwrap();
    assert_eq!(hits("\"zebrafish\""), 0, "search");
    assert_eq!(recall("\"zebrafish\""), 0, "recall");
    assert_eq!(hits("\"invoices\""), 1, "before the cut stays");
    assert_eq!(hits("\"Schema\""), 1, "the title stays");
    assert!(!s.session_search_text(sid).unwrap().contains("zebrafish"));
    // The index equals a rebuild from the log.
    let live = s.session_search_text(sid).unwrap();
    s.rebuild_search_index().unwrap();
    assert_eq!(s.session_search_text(sid).unwrap(), live);
    assert_eq!(hits("\"zebrafish\""), 0);

    s.append(
        "master",
        &ev(
            sid,
            next + 1,
            "conversation_rewind_undone",
            json!({"rewind_seq": next}),
        ),
    )
    .unwrap();
    assert_eq!(hits("\"zebrafish\""), 1, "undone");
    assert_eq!(recall("\"zebrafish\""), 1, "recall, undone");
    assert_eq!(hits("\"invoices\""), 1);
    // A message after the rewind is indexed as it is appended.
    s.append(
        "master",
        &ev(
            sid,
            next + 2,
            "user_message",
            json!({"turn_id": "t3", "text": "mention the quokka"}),
        ),
    )
    .unwrap();
    assert_eq!(hits("\"quokka\""), 1);
}

/// An incognito chat is never indexed, a rewind or not.
#[test]
fn a_rewind_does_not_index_an_incognito_chat() {
    let s = Store::open_in_memory().unwrap();
    let ws = team(&s);
    let (sid, msg, next) = two_turns(&s, true, &ws);
    for (i, (k, p)) in [
        (
            "conversation_rewound",
            json!({"message_seq": msg, "turn_id": "t2", "files": false}),
        ),
        ("conversation_rewind_undone", json!({"rewind_seq": next})),
    ]
    .into_iter()
    .enumerate()
    {
        s.append("master", &ev(sid, next + i as u64, k, p)).unwrap();
        assert_eq!(s.search("\"invoices\"", None, None, 10).unwrap().len(), 0);
        assert_eq!(s.session_search_text(sid).unwrap(), "");
    }
}
