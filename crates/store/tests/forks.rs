//! What a fork changes in the store (ADOPTION_PLAN C.2): the lineage row,
//! the search index and the usage sums that skip what a fork copied, and
//! writing a fork's events and images in one transaction.

use std::time::Duration;

use harness_core::event::*;
use harness_core::fork;
use harness_core::policy::ApprovalMode;
use harness_core::{RawEvent, SessionId};
use harness_store::session_images::SessionImage;
use harness_store::{AppendOutcome, NewUser, Role, Store};
use serde_json::json;

const NODE: &str = "n1";

fn store() -> Store {
    let s = Store::open_in_memory().unwrap();
    s.create_user(&NewUser {
        user_id: "u1".into(),
        username: "alex".into(),
        password_hash: "x".into(),
        role: Role::Member,
        exec_ceiling: harness_core::policy::ExecLevel::None,
        must_change_password: false,
    })
    .unwrap();
    s
}

struct Log {
    sid: SessionId,
    events: Vec<RawEvent>,
}

impl Log {
    fn push(&mut self, k: EventKind) -> u64 {
        let (kind, payload) = k.into_parts();
        let seq = self.events.len() as u64 + 1;
        self.events.push(RawEvent {
            session_id: self.sid,
            seq,
            ts_ms: 1_000 + seq as i64,
            kind,
            payload,
        });
        seq
    }
    fn raw(&mut self, kind: &str, payload: serde_json::Value) -> u64 {
        let seq = self.events.len() as u64 + 1;
        self.events.push(RawEvent {
            session_id: self.sid,
            seq,
            ts_ms: 1_000 + seq as i64,
            kind: kind.into(),
            payload,
        });
        seq
    }
    /// A finished turn with one message and one answer; returns the message's seq.
    fn turn(&mut self, id: &str, said: &str, answered: &str, input: u64) -> u64 {
        self.raw("turn_started", json!({"turn_id": id}));
        let m = self.raw("user_message", json!({"turn_id": id, "text": said}));
        self.raw(
            "assistant_message",
            json!({"turn_id": id, "message_id": format!("m-{id}"), "text": answered}),
        );
        self.raw(
            "turn_ended",
            json!({"turn_id": id, "reason": "completed", "usage": {
                "input_tokens": input, "output_tokens": input / 10, "context_tokens": input}}),
        );
        m
    }
}

fn parent_log() -> Log {
    let mut l = Log {
        sid: SessionId::new(),
        events: vec![],
    };
    l.push(EventKind::SessionCreated(SessionCreated {
        forked: None,
        title: "Parent chat".into(),
        cwd: "/w".into(),
        model: "m".into(),
        backend: "native".into(),
        node_id: NODE.into(),
        approval_mode: ApprovalMode::AcceptEdits,
        profile: Profile::Agentic,
        agent_id: None,
        incognito: false,
        parent_session: None,
        no_callbacks: false,
        exec_mode: None,
    }));
    l.turn("t1", "inheritedneedle question", "inheritedanswer", 100);
    l
}

/// The parent mirrored, then a fork of it at its first turn.
fn mirrored(s: &Store) -> (Log, Vec<RawEvent>) {
    let p = parent_log();
    for e in &p.events {
        assert_eq!(s.append(NODE, e).unwrap(), AppendOutcome::Applied);
    }
    s.claim_session(p.sid, Some("u1"), None).unwrap();
    let cut = fork::check(&p.events, "t1").unwrap();
    let f = fork::build(
        &p.events,
        &cut,
        SessionId::new(),
        &fork::title("Parent chat"),
        "/w",
        5_000,
    )
    .unwrap();
    (p, f)
}

fn append_all(s: &Store, evs: &[RawEvent]) {
    for e in evs {
        assert_eq!(
            s.append(NODE, e).unwrap(),
            AppendOutcome::Applied,
            "{}",
            e.seq
        );
    }
}

#[test]
fn a_forks_lineage_is_written_with_its_first_event_and_goes_with_the_fork() {
    let s = store();
    let (p, f) = mirrored(&s);
    assert!(s.fork_of(p.sid).unwrap().is_none());
    append_all(&s, &f);
    let row = s.fork_of(f[0].session_id).unwrap().expect("a lineage row");
    assert_eq!(row.parent, p.sid);
    assert_eq!(row.through_seq, 5);
    assert_eq!(row.created_ms, 5_000);
    // The parent may be deleted: the fork lives on, its row too.
    s.delete_session(p.sid, true).unwrap();
    assert!(s.session(f[0].session_id).unwrap().is_some());
    assert_eq!(s.fork_of(f[0].session_id).unwrap().unwrap().parent, p.sid);
    // The fork's own deletion takes its row.
    s.delete_session(f[0].session_id, true).unwrap();
    assert!(s.fork_of(f[0].session_id).unwrap().is_none());
}

#[test]
fn a_chat_that_is_not_a_fork_has_no_lineage_and_a_resend_adds_none() {
    let s = store();
    let (p, f) = mirrored(&s);
    assert!(s.fork_of(p.sid).unwrap().is_none());
    append_all(&s, &f);
    for e in &f {
        assert_eq!(s.append(NODE, e).unwrap(), AppendOutcome::Duplicate);
    }
    assert_eq!(s.fork_of(f[0].session_id).unwrap().unwrap().through_seq, 5);
}

#[test]
fn what_a_fork_copied_is_not_indexed_again_but_its_title_and_own_text_are() {
    let s = store();
    let (p, mut f) = mirrored(&s);
    append_all(&s, &f);
    let fork_id = f[0].session_id;
    s.claim_session(fork_id, Some("u1"), None).unwrap();
    // The fork's own message and answer.
    let mut own = Log {
        sid: fork_id,
        events: std::mem::take(&mut f),
    };
    own.turn("u1", "ownneedle question", "ownanswer", 40);
    for e in &own.events[(own.events.len() - 4)..] {
        assert_eq!(s.append(NODE, e).unwrap(), AppendOutcome::Applied);
    }
    let ids = |q: &str| -> Vec<SessionId> {
        s.search(q, Some("u1"), None, 10)
            .unwrap()
            .into_iter()
            .map(|h| h.session_id)
            .collect()
    };
    assert_eq!(
        ids("inheritedneedle"),
        vec![p.sid],
        "the parent has it, once"
    );
    assert_eq!(ids("ownneedle"), vec![fork_id]);
    assert!(ids("fork").contains(&fork_id), "found by its title");
    // A rebuild of the whole index follows the same rule.
    s.rebuild_search_index().unwrap();
    assert_eq!(ids("inheritedneedle"), vec![p.sid]);
    assert_eq!(ids("ownneedle"), vec![fork_id]);
    // So does the per-chat re-index a rewind in the fork triggers.
    let m = own
        .events
        .iter()
        .rposition(|e| e.kind == "user_message")
        .unwrap() as u64
        + 1;
    let turn = own
        .events
        .iter()
        .filter(|e| e.kind == "turn_started")
        .count();
    assert_eq!(turn, 2);
    own.raw(
        "conversation_rewound",
        json!({"message_seq": m, "turn_id": "u1", "files": false}),
    );
    s.append(NODE, own.events.last().unwrap()).unwrap();
    assert_eq!(ids("inheritedneedle"), vec![p.sid], "still only the parent");
    assert!(ids("ownneedle").is_empty(), "rewound text is not found");
    assert!(ids("fork").contains(&fork_id));
}

#[test]
fn usage_counts_what_a_fork_spent_and_not_what_it_copied() {
    let s = store();
    let (p, f) = mirrored(&s);
    append_all(&s, &f);
    let fork_id = f[0].session_id;
    s.claim_session(fork_id, Some("u1"), None).unwrap();
    let mut own = Log {
        sid: fork_id,
        events: f,
    };
    own.turn("u1", "more", "ok", 40);
    for e in &own.events[(own.events.len() - 4)..] {
        s.append(NODE, e).unwrap();
    }
    let t = s.usage_totals(0, 100_000, Duration::from_secs(60)).unwrap();
    let all: u64 = t.people.iter().map(|p| p.bucket.tally.input_tokens).sum();
    assert_eq!(all, 100 + 40, "the parent's turn once, the fork's own once");
    let _ = p;
}

/// The master's callback watcher reads a chat's log from its last person's
/// turn: a fork's divider is such a boundary, so an end the parent had not
/// been resumed for at the cut is not waiting in the fork.
#[test]
fn the_callback_watcher_starts_after_the_divider() {
    let s = store();
    let mut p = parent_log();
    p.raw("turn_started", json!({"turn_id": "t2"}));
    p.raw("user_message", json!({"turn_id": "t2", "text": "go"}));
    p.raw(
        "background_started",
        json!({"bg_id": "bg-1", "call_id": "c", "turn_id": "t2", "timeout_s": 60, "ends_by_ms": 1}),
    );
    p.raw(
        "background_ended",
        json!({"bg_id": "bg-1", "reason": "exited", "exit_code": 0, "confirmed": "ended",
               "ran_ms": 1, "output_bytes": 0}),
    );
    p.raw(
        "turn_ended",
        json!({"turn_id": "t2", "reason": "completed"}),
    );
    for e in &p.events {
        s.append(NODE, e).unwrap();
    }
    let kinds = ["background_ended", "turn_started"];
    let waiting = |sid| {
        let ends = s.events_since_last_person_turn(sid, &kinds).unwrap();
        harness_core::event::waiting_ends(&ends).len()
    };
    assert_eq!(waiting(p.sid), 1, "the parent still waits for it");
    let cut = fork::check(&p.events, "t2").unwrap();
    let f = fork::build(
        &p.events,
        &cut,
        SessionId::new(),
        &fork::title("Parent chat"),
        "/w",
        9_000,
    )
    .unwrap();
    append_all(&s, &f);
    assert_eq!(waiting(f[0].session_id), 0, "the fork does not");
}

#[test]
fn a_forks_row_is_idle_after_the_copy_whatever_the_parent_was_doing() {
    let s = store();
    let mut p = parent_log();
    // A turn that asked for an approval nobody answered before it ended,
    // and ended with a question.
    p.raw("turn_started", json!({"turn_id": "t2"}));
    p.raw("user_message", json!({"turn_id": "t2", "text": "go"}));
    p.raw(
        "approval_requested",
        json!({"approval_id": "a1", "call_id": "c", "tool": "bash", "input": {}, "reason": "r"}),
    );
    p.raw(
        "assistant_message",
        json!({"turn_id": "t2", "message_id": "m2", "text": "Shall I?"}),
    );
    p.raw(
        "turn_ended",
        json!({"turn_id": "t2", "reason": "completed"}),
    );
    for e in &p.events {
        s.append(NODE, e).unwrap();
    }
    s.claim_session(p.sid, Some("u1"), None).unwrap();
    assert_eq!(
        s.attention(Some("u1"), "u1").unwrap().len(),
        1,
        "the parent waits"
    );
    let cut = fork::check(&p.events, "t2").unwrap();
    let f = fork::build(
        &p.events,
        &cut,
        SessionId::new(),
        &fork::title("Parent chat"),
        "/w",
        9_000,
    )
    .unwrap();
    append_all(&s, &f);
    let id = f[0].session_id;
    s.claim_session(id, Some("u1"), None).unwrap();
    let att = s.attention(Some("u1"), "u1").unwrap();
    assert!(att.iter().all(|a| a.session_id != id), "{att:?}");
    assert!(!s.unread(Some("u1"), "u1").unwrap().contains(&id));
    let row = s.session(id).unwrap().unwrap();
    assert!(!row.running);
    assert_eq!(row.head_seq, f.len() as u64);
    assert_eq!(row.title, "Parent chat (fork)");
    assert_eq!(row.parent_id, None, "a fork is not a sub-agent");
}

#[test]
fn a_forks_events_and_images_are_written_in_one_transaction() {
    let s = store();
    let mut p = parent_log();
    p.raw(
        "user_message",
        json!({"turn_id": "t1", "text": "look", "images": [
            {"media_id": "m", "sha256": "aa", "mime": "image/png", "name": "a.png", "width": 1, "height": 1}]}),
    );
    p.raw(
        "turn_ended",
        json!({"turn_id": "t1", "reason": "completed"}),
    );
    for e in &p.events {
        s.append(NODE, e).unwrap();
    }
    let image = |sha: &str| SessionImage {
        sha256: sha.into(),
        mime: "image/png".into(),
        name: format!("{sha}.png"),
        bytes: vec![1, 2, 3],
    };
    // One the copied events name, one a later message of the parent's does.
    s.session_images_put(&p.sid.to_string(), &[image("aa"), image("bb")])
        .unwrap();
    let cut = fork::check(&p.events, "t1").unwrap();
    // The cut is the first turn end; the later one is the parent's own.
    assert_eq!(cut.through_seq, 5);
    let f = fork::build(
        &p.events,
        &cut,
        SessionId::new(),
        &fork::title("Parent chat"),
        "/w",
        5,
    )
    .unwrap();
    // `aa` is named by event 6, after the cut: not copied.
    s.write_fork(NODE, p.sid, &f).unwrap();
    let id = f[0].session_id.to_string();
    assert_eq!(s.head_seq(f[0].session_id).unwrap(), f.len() as u64);
    assert!(s.session_images(&id, &["aa".into()]).unwrap().is_empty());

    // A cut after the image: both its events and its bytes arrive.
    let cut = fork::Cut {
        through_seq: p.events.len() as u64,
        turn_id: "t1".into(),
    };
    let f2 = fork::build(
        &p.events,
        &cut,
        SessionId::new(),
        &fork::title("Parent chat"),
        "/w",
        6,
    )
    .unwrap();
    s.write_fork(NODE, p.sid, &f2).unwrap();
    let id2 = f2[0].session_id.to_string();
    let got = s.session_images(&id2, &["aa".into(), "bb".into()]).unwrap();
    assert_eq!(got.len(), 1, "only the image the copied events name");
    assert_eq!(got[0].sha256, "aa");
    assert_eq!(got[0].bytes, vec![1, 2, 3]);
    // The parent keeps its own.
    assert_eq!(
        s.session_images(&p.sid.to_string(), &["aa".into(), "bb".into()])
            .unwrap()
            .len(),
        2
    );

    // All or nothing: a gap in the middle writes nothing, no row, no image.
    let mut bad = fork::build(
        &p.events,
        &cut,
        SessionId::new(),
        &fork::title("Parent chat"),
        "/w",
        7,
    )
    .unwrap();
    bad.remove(3);
    let bad_id = bad[0].session_id;
    assert!(s.write_fork(NODE, p.sid, &bad).is_err());
    assert!(s.session(bad_id).unwrap().is_none());
    assert!(s.fork_of(bad_id).unwrap().is_none());
    assert!(
        s.session_images(&bad_id.to_string(), &["aa".into()])
            .unwrap()
            .is_empty()
    );
    // Writing the same fork twice is refused and changes nothing.
    assert!(s.write_fork(NODE, p.sid, &f2).is_err());
    assert_eq!(s.head_seq(f2[0].session_id).unwrap(), f2.len() as u64);
}
