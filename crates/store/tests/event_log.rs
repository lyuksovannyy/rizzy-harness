//! Event-log contract tests. These define the replication semantics everything
//! else relies on; see PLAN.md §2–3.

use harness_core::event::*;
use harness_core::view::SessionView;
use harness_core::{RawEvent, SessionId};
use harness_store::{AppendOutcome, Durability, Store, StoreError};
use serde_json::json;

fn ev(sid: SessionId, seq: u64, k: EventKind) -> RawEvent {
    let (kind, payload) = k.into_parts();
    RawEvent {
        session_id: sid,
        seq,
        ts_ms: seq as i64,
        kind,
        payload,
    }
}

fn created(sid: SessionId) -> RawEvent {
    ev(
        sid,
        1,
        EventKind::SessionCreated(SessionCreated {
            title: "t".into(),
            cwd: "/w".into(),
            model: "m".into(),
            backend: "native".into(),
            node_id: "local".into(),
            approval_mode: Default::default(),
        }),
    )
}

/// A realistic session log of `n` events (n >= 2).
fn script(sid: SessionId, n: u64) -> Vec<RawEvent> {
    let mut v = vec![created(sid)];
    v.push(ev(
        sid,
        2,
        EventKind::TurnStarted(TurnStarted {
            turn_id: "t1".into(),
        }),
    ));
    for seq in 3..=n {
        v.push(ev(
            sid,
            seq,
            EventKind::AssistantDelta(AssistantDelta {
                turn_id: "t1".into(),
                message_id: "m1".into(),
                text: format!("chunk{seq} "),
            }),
        ));
    }
    v
}

#[test]
fn append_is_contiguous_and_idempotent() {
    let s = Store::open_in_memory().unwrap();
    let sid = SessionId::new();
    let evs = script(sid, 5);
    for e in &evs {
        assert_eq!(s.append("local", e).unwrap(), AppendOutcome::Applied);
    }
    // Replaying any already-stored seq is a no-op.
    for e in &evs {
        assert_eq!(s.append("local", e).unwrap(), AppendOutcome::Duplicate);
    }
    assert_eq!(s.head_seq(sid).unwrap(), 5);
    assert_eq!(s.events_after(sid, 0, 100).unwrap(), evs);
}

#[test]
fn gap_is_reported_not_stored() {
    let s = Store::open_in_memory().unwrap();
    let sid = SessionId::new();
    let evs = script(sid, 4);
    s.append("local", &evs[0]).unwrap();
    assert_eq!(
        s.append("local", &evs[2]).unwrap(),
        AppendOutcome::Gap { head: 1 }
    );
    assert_eq!(s.head_seq(sid).unwrap(), 1);
    // First event seen for a session that is not seq 1 is also a gap.
    let other = SessionId::new();
    let o = script(other, 3);
    assert_eq!(
        s.append("local", &o[1]).unwrap(),
        AppendOutcome::Gap { head: 0 }
    );
}

#[test]
fn ownership_is_enforced() {
    let s = Store::open_in_memory().unwrap();
    let sid = SessionId::new();
    let evs = script(sid, 3);
    s.append("node-a", &evs[0]).unwrap();
    let err = s.append("node-b", &evs[1]).unwrap_err();
    assert!(matches!(err, StoreError::WrongOwner { .. }), "{err:?}");
}

#[test]
fn cursor_pagination() {
    let s = Store::open_in_memory().unwrap();
    let sid = SessionId::new();
    let evs = script(sid, 50);
    for e in &evs {
        s.append("local", e).unwrap();
    }
    let mut got = Vec::new();
    let mut cursor = 0;
    loop {
        let page = s.events_after(sid, cursor, 7).unwrap();
        if page.is_empty() {
            break;
        }
        cursor = page.last().unwrap().seq;
        got.extend(page);
    }
    assert_eq!(got, evs);
    assert!(s.events_after(sid, 50, 10).unwrap().is_empty());
}

#[test]
fn persists_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("node.db");
    let sid = SessionId::new();
    let evs = script(sid, 10);
    {
        let s = Store::open(&path, Durability::Full).unwrap();
        for e in &evs[..6] {
            s.append("local", e).unwrap();
        }
    }
    let s = Store::open(&path, Durability::Full).unwrap();
    assert_eq!(s.head_seq(sid).unwrap(), 6);
    for e in &evs {
        s.append("local", e).unwrap();
    }
    assert_eq!(s.events_after(sid, 0, 100).unwrap(), evs);
}

#[test]
fn unknown_kinds_are_preserved_verbatim() {
    let s = Store::open_in_memory().unwrap();
    let sid = SessionId::new();
    s.append("local", &created(sid)).unwrap();
    let future = RawEvent {
        session_id: sid,
        seq: 2,
        ts_ms: 9,
        kind: "hologram".into(),
        payload: json!({"deep": {"nested": [1, 2, 3]}}),
    };
    s.append("local", &future).unwrap();
    assert_eq!(s.events_after(sid, 1, 10).unwrap(), vec![future]);
}

#[test]
fn session_index_tracks_metadata_and_running() {
    let s = Store::open_in_memory().unwrap();
    let sid = SessionId::new();
    for e in script(sid, 3) {
        s.append("local", &e).unwrap();
    }
    let rows = s.sessions().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "t");
    assert_eq!(rows[0].node_id, "local");
    assert_eq!(rows[0].head_seq, 3);
    assert!(rows[0].running);
    s.append(
        "local",
        &ev(
            sid,
            4,
            EventKind::TurnEnded(TurnEnded {
                turn_id: "t1".into(),
                reason: TurnEndReason::Completed,
                usage: None,
            }),
        ),
    )
    .unwrap();
    assert!(!s.sessions().unwrap()[0].running);
    assert_eq!(s.heads().unwrap(), vec![(sid, 4)]);
}

/// Simulate an unreliable link: the mirror receives a stream with drops,
/// duplicates and reordering, and recovers only through `Gap` -> resume from
/// the source. It must end byte-identical to the source with an equal fold.
#[test]
fn mirror_converges_under_lossy_delivery() {
    let mut rng = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };
    for round in 0..50 {
        let source = Store::open_in_memory().unwrap();
        let mirror = Store::open_in_memory().unwrap();
        let sid = SessionId::new();
        let evs = script(sid, 20 + round);
        for e in &evs {
            source.append("n", e).unwrap();
        }
        // Lossy live stream.
        let mut stream: Vec<RawEvent> = Vec::new();
        for e in &evs {
            match next() % 10 {
                0 | 1 => {}                                 // drop
                2 => stream.extend([e.clone(), e.clone()]), // dup
                3 if !stream.is_empty() => {
                    let i = (next() as usize) % stream.len();
                    stream.insert(i, e.clone()) // reorder
                }
                _ => stream.push(e.clone()),
            }
        }
        let mut resumes = 0;
        let deliver = |e: &RawEvent, resumes: &mut u32| {
            if let AppendOutcome::Gap { head } = mirror.append("n", e).unwrap() {
                *resumes += 1;
                for r in source.events_after(sid, head, 1000).unwrap() {
                    mirror.append("n", &r).unwrap();
                }
            }
        };
        for e in &stream {
            deliver(e, &mut resumes);
        }
        // Reconnect: the mirror announces its head and the source replays.
        let head = mirror.head_seq(sid).unwrap();
        for r in source.events_after(sid, head, 1000).unwrap() {
            assert_eq!(mirror.append("n", &r).unwrap(), AppendOutcome::Applied);
        }
        let got = mirror.events_after(sid, 0, 1000).unwrap();
        assert_eq!(got, evs, "round {round}");
        assert_eq!(
            SessionView::fold(&got).unwrap(),
            SessionView::fold(&evs).unwrap()
        );
    }
}

#[test]
fn audit_log_records() {
    let s = Store::open_in_memory().unwrap();
    s.audit("session.prompt", "{\"session\":\"x\"}").unwrap();
    assert_eq!(s.audit_count().unwrap(), 1);
}

#[tokio::test]
async fn async_call_helper() {
    let s = Store::open_in_memory().unwrap();
    let sid = SessionId::new();
    let e = created(sid);
    let out = s.call(move |s| s.append("local", &e)).await.unwrap();
    assert_eq!(out, AppendOutcome::Applied);
}
