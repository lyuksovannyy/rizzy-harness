//! Files a chat without a folder may read through `attached_file` (PLAN.md
//! T95): a row per chat and file on the owner's blob, served only once the
//! chat's own log names the file, copied to a fork, gone with the chat, and
//! keeping its blob alive until then.

use harness_core::event::*;
use harness_core::fork;
use harness_core::policy::ApprovalMode;
use harness_core::{RawEvent, SessionId};
use harness_store::blobs::{Limits, NewBlob};
use harness_store::chat_uploads::{NewUpload, Put, TakenUpload};
use harness_store::{AppendOutcome, NewUser, Role, Store};
use serde_json::json;

const NODE: &str = "n1";

fn store() -> Store {
    let s = Store::open_in_memory().unwrap();
    for id in ["u1", "u2"] {
        s.create_user(&NewUser {
            user_id: id.into(),
            username: id.into(),
            password_hash: "x".into(),
            role: Role::Member,
            exec_ceiling: harness_core::policy::ExecLevel::None,
            must_change_password: false,
        })
        .unwrap();
    }
    s
}

/// An upload of `owner`, taken, ready for a send.
fn taken(s: &Store, id: &str, owner: &str, sha: &str) -> TakenUpload {
    let blob = NewBlob {
        blob_id: format!("b{id}"),
        owner_id: Some(owner.into()),
        sha256: sha.into(),
        size: 10,
        kind: "other".into(),
        format: "bin".into(),
        mime: "application/octet-stream".into(),
        display: "download".into(),
        key: vec![7; 32],
        width: None,
        height: None,
    };
    let up = NewUpload {
        upload_id: id.into(),
        owner_id: Some(owner.into()),
        name: format!("{id}.zip"),
        mime: "application/zip".into(),
        session_id: None,
    };
    let r = s
        .chat_upload_put(
            &up,
            &blob,
            Limits {
                account: 0,
                server: 0,
            },
            10,
        )
        .unwrap();
    assert!(matches!(r, Put::Added { .. }));
    s.chat_uploads_take(&[id.to_string()], Some(owner))
        .unwrap()
        .unwrap()
        .remove(0)
}

struct Log {
    sid: SessionId,
    events: Vec<RawEvent>,
}

impl Log {
    fn new() -> Log {
        let mut l = Log {
            sid: SessionId::new(),
            events: vec![],
        };
        l.push(EventKind::SessionCreated(SessionCreated {
            forked: None,
            title: "Chat".into(),
            cwd: "/w".into(),
            model: "m".into(),
            backend: "native".into(),
            node_id: NODE.into(),
            approval_mode: ApprovalMode::AcceptEdits,
            profile: Profile::Chat,
            agent_id: None,
            incognito: false,
            parent_session: None,
            no_callbacks: false,
            exec_mode: None,
            sub_folder: None,
        }));
        l
    }
    fn push(&mut self, k: EventKind) {
        let (kind, payload) = k.into_parts();
        self.raw(&kind, payload);
    }
    fn raw(&mut self, kind: &str, payload: serde_json::Value) {
        let seq = self.events.len() as u64 + 1;
        self.events.push(RawEvent {
            session_id: self.sid,
            seq,
            ts_ms: 1_000 + seq as i64,
            kind: kind.into(),
            payload,
        });
    }
    /// A finished turn whose message names `files`.
    fn turn(&mut self, id: &str, files: &[(&str, &str)]) {
        self.raw("turn_started", json!({"turn_id": id}));
        let attached: Vec<_> = files
            .iter()
            .map(|(f, sha)| {
                json!({"file_id": f, "name": format!("{f}.zip"), "bytes": 10, "sha256": sha,
                       "mime": "application/zip"})
            })
            .collect();
        self.raw(
            "user_message",
            json!({"turn_id": id, "text": "look", "attached": attached}),
        );
        self.raw(
            "assistant_message",
            json!({"turn_id": id, "message_id": format!("m-{id}"), "text": "ok"}),
        );
        self.raw(
            "turn_ended",
            json!({"turn_id": id, "reason": "completed", "usage": {
                "input_tokens": 10, "output_tokens": 1, "context_tokens": 10}}),
        );
    }
}

fn append_all(s: &Store, node: &str, evs: &[RawEvent]) {
    for e in evs {
        assert_eq!(
            s.append(node, e).unwrap(),
            AppendOutcome::Applied,
            "{}",
            e.seq
        );
    }
}

fn blob_gone(s: &Store, id: &str) -> bool {
    s.blob(id).unwrap().is_none()
}

/// A file is readable only from the message that names it on, in its own
/// chat, with the hash the master sent.
#[test]
fn a_file_is_served_once_the_chats_log_names_it_and_only_there() {
    let s = store();
    let mut l = Log::new();
    append_all(&s, NODE, &l.events);
    let sid = l.sid.to_string();
    let f = taken(&s, "f1", "u1", "aa");
    s.session_files_hold(&sid, &[f]).unwrap();
    // Held for the send, the log does not name it yet.
    assert!(s.session_file(&sid, "f1").unwrap().is_none());
    // A message naming it with another hash logs nothing.
    l.turn("t0", &[("f1", "wrong")]);
    append_all(&s, NODE, &l.events[1..]);
    assert!(s.session_file(&sid, "f1").unwrap().is_none());
    // The real one.
    let from = l.events.len();
    l.turn("t1", &[("f1", "aa")]);
    append_all(&s, NODE, &l.events[from..]);
    let got = s.session_file(&sid, "f1").unwrap().unwrap();
    assert_eq!(
        (got.blob_id.as_str(), got.size, got.owner_id.as_deref()),
        ("bf1", 10, Some("u1"))
    );
    assert_eq!(got.sha256, "aa");
    // Never another chat's, never an id nobody sent.
    let other = Log::new();
    append_all(&s, NODE, &other.events);
    assert!(
        s.session_file(&other.sid.to_string(), "f1")
            .unwrap()
            .is_none()
    );
    assert!(s.session_file(&sid, "invented").unwrap().is_none());
    // A message that names a file the master never sent this chat makes no row.
    let mut m = Log::new();
    append_all(&s, NODE, &m.events);
    m.turn("t1", &[("f1", "aa")]);
    append_all(&s, NODE, &m.events[1..]);
    assert!(s.session_file(&m.sid.to_string(), "f1").unwrap().is_none());
}

#[test]
fn a_released_hold_goes_but_a_logged_row_stays() {
    let s = store();
    let mut l = Log::new();
    append_all(&s, NODE, &l.events);
    let sid = l.sid.to_string();
    s.session_files_hold(
        &sid,
        &[taken(&s, "f1", "u1", "aa"), taken(&s, "f2", "u1", "bb")],
    )
    .unwrap();
    let from = l.events.len();
    l.turn("t1", &[("f2", "bb")]);
    append_all(&s, NODE, &l.events[from..]);
    s.session_files_release(&sid, &["f1".into(), "f2".into()])
        .unwrap();
    assert!(
        s.session_file(&sid, "f2").unwrap().is_some(),
        "logged: kept"
    );
    // f1 is gone: no row refers to its blob (the upload row still does until sent).
    s.chat_uploads_done(&["f1".into()]).unwrap();
    assert_eq!(
        s.blobs_unreferenced(i64::MAX).unwrap(),
        vec!["bf1".to_string()]
    );
}

/// The sweep: a blob lives while a chat's row refers to it, also after
/// the upload row is gone, and is swept once the last such chat is
/// deleted. A held row that was never logged keeps it too (the message may
/// have been taken).
#[test]
fn the_blob_lives_as_long_as_a_chat_refers_to_it() {
    let s = store();
    let mut l = Log::new();
    append_all(&s, NODE, &l.events);
    let sid = l.sid.to_string();
    let f = taken(&s, "f1", "u1", "aa");
    s.session_files_hold(&sid, &[f]).unwrap();
    s.chat_uploads_done(&["f1".into()]).unwrap();
    let from = l.events.len();
    l.turn("t1", &[("f1", "aa")]);
    append_all(&s, NODE, &l.events[from..]);
    // No upload row, a chat row: not swept.
    assert!(s.blobs_unreferenced(i64::MAX).unwrap().is_empty());
    assert!(s.blob_delete(&["bf1".into()]).unwrap().is_empty());
    assert!(!blob_gone(&s, "bf1"));
    // The chat goes: its rows go and the blob is a leftover the sweep takes.
    assert!(s.delete_session(l.sid, true).unwrap());
    assert_eq!(
        s.blobs_unreferenced(i64::MAX).unwrap(),
        vec!["bf1".to_string()]
    );
    assert_eq!(
        s.blob_delete(&["bf1".into()]).unwrap(),
        vec!["bf1".to_string()]
    );
    assert!(blob_gone(&s, "bf1"));
    assert!(s.session_file(&sid, "f1").unwrap().is_none());
}

/// A fork keeps the files of the messages it copies, as rows of its own for
/// the same blob: deleting the parent leaves the fork's file readable and
/// the blob; deleting the last chat frees it.
#[test]
fn a_fork_shares_the_blob_and_the_last_chat_frees_it() {
    let s = store();
    let mut p = Log::new();
    p.turn("t1", &[("f1", "aa")]);
    p.turn("t2", &[]);
    append_all(&s, NODE, &p.events[..1]);
    s.claim_session(p.sid, Some("u1"), None).unwrap();
    let psid = p.sid.to_string();
    s.session_files_hold(&psid, &[taken(&s, "f1", "u1", "aa")])
        .unwrap();
    s.chat_uploads_done(&["f1".into()]).unwrap();
    append_all(&s, NODE, &p.events[1..]);
    assert!(s.session_file(&psid, "f1").unwrap().is_some());

    // Fork after the first turn: the copy names f1.
    let cut = fork::check(&p.events, "t1").unwrap();
    let f = fork::build(
        &p.events,
        &cut,
        SessionId::new(),
        "Chat (fork)",
        "/w",
        5_000,
    )
    .unwrap();
    s.write_fork(NODE, p.sid, &f).unwrap();
    let fsid = f[0].session_id.to_string();
    let got = s
        .session_file(&fsid, "f1")
        .unwrap()
        .expect("the fork's own row");
    assert_eq!(got.blob_id, "bf1");

    // The parent goes: the fork still reads it, the blob stays.
    assert!(s.delete_session(p.sid, true).unwrap());
    assert!(s.blobs_unreferenced(i64::MAX).unwrap().is_empty());
    assert!(s.session_file(&fsid, "f1").unwrap().is_some());
    assert!(s.session_file(&psid, "f1").unwrap().is_none());
    // The last one goes: the blob is freed.
    assert!(s.delete_session(f[0].session_id, true).unwrap());
    assert_eq!(
        s.blobs_unreferenced(i64::MAX).unwrap(),
        vec!["bf1".to_string()]
    );
}

/// A node that says its new chat is a fork of another node's chat (or of
/// a chat that never had the file) gets no file by it.
#[test]
fn a_fork_claim_cannot_reach_another_nodes_chat_or_an_unlogged_file() {
    let s = store();
    let mut p = Log::new();
    p.turn("t1", &[("f1", "aa")]);
    append_all(&s, NODE, &p.events[..1]);
    s.claim_session(p.sid, Some("u1"), None).unwrap();
    let psid = p.sid.to_string();
    s.session_files_hold(&psid, &[taken(&s, "f1", "u1", "aa")])
        .unwrap();
    append_all(&s, NODE, &p.events[1..]);
    assert!(s.session_file(&psid, "f1").unwrap().is_some());
    let cut = fork::check(&p.events, "t1").unwrap();
    let f = fork::build(&p.events, &cut, SessionId::new(), "x", "/w", 5_000).unwrap();
    // Another node writes the "fork" of this node's chat.
    s.write_fork("evil", p.sid, &f).unwrap();
    assert!(
        s.session_file(&f[0].session_id.to_string(), "f1")
            .unwrap()
            .is_none()
    );
    // A fork of a parent whose row was only held (never logged there).
    let mut h = Log::new();
    h.turn("t1", &[("f9", "zz")]);
    append_all(&s, NODE, &h.events[..1]);
    s.session_files_hold(&h.sid.to_string(), &[taken(&s, "f9", "u1", "zz")])
        .unwrap();
    let cut = fork::check(&h.events, "t1").unwrap();
    let f = fork::build(&h.events, &cut, SessionId::new(), "x", "/w", 5_000).unwrap();
    s.write_fork(NODE, h.sid, &f).unwrap();
    assert!(
        s.session_file(&f[0].session_id.to_string(), "f9")
            .unwrap()
            .is_none()
    );
}

/// Another account's blob is not reachable by claiming its upload id: the
/// row records the owner of the blob, which the master compares with the
/// chat's owner when it serves.
#[test]
fn the_row_records_whose_blob_it_is() {
    let s = store();
    let mut l = Log::new();
    append_all(&s, NODE, &l.events);
    let sid = l.sid.to_string();
    s.session_files_hold(&sid, &[taken(&s, "f1", "u2", "aa")])
        .unwrap();
    let from = l.events.len();
    l.turn("t1", &[("f1", "aa")]);
    append_all(&s, NODE, &l.events[from..]);
    assert_eq!(
        s.session_file(&sid, "f1")
            .unwrap()
            .unwrap()
            .owner_id
            .as_deref(),
        Some("u2")
    );
}

/// The tool is offered only for a chat that has a file its log names: a row
/// held by a send and never logged offers nothing.
#[test]
fn a_held_but_unlogged_row_does_not_offer_the_tool() {
    let s = store();
    let mut l = Log::new();
    append_all(&s, NODE, &l.events);
    let sid = l.sid.to_string();
    assert!(!s.session_files_any(&sid).unwrap());
    s.session_files_hold(&sid, &[taken(&s, "f1", "u1", "aa")])
        .unwrap();
    assert!(!s.session_files_any(&sid).unwrap(), "held, not logged");
    let from = l.events.len();
    l.turn("t1", &[("f1", "aa")]);
    append_all(&s, NODE, &l.events[from..]);
    assert!(s.session_files_any(&sid).unwrap());
}
