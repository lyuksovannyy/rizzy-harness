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
            exec_mode: None,
            no_callbacks: false,
            profile: Profile::Agentic,
            agent_id: None,
            incognito: false,
            parent_session: None,
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
            cause: None,
            agent: None,
            turn_id: "t1".into(),
        }),
    ));
    for seq in 3..=n {
        v.push(ev(
            sid,
            seq,
            EventKind::AssistantDelta(AssistantDelta {
                author: None,
                variant: None,
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

#[test]
fn deleted_sessions_stay_deleted() {
    let s = Store::open_in_memory().unwrap();
    let sid = SessionId::new();
    for e in script(sid, 5) {
        s.append("n", &e).unwrap();
    }
    // Node-side delete (no tombstone): the id could be reused by replay.
    let other = SessionId::new();
    s.append("n", &created(other)).unwrap();
    assert!(s.delete_session(other, false).unwrap());
    assert_eq!(
        s.append("n", &created(other)).unwrap(),
        AppendOutcome::Applied
    );

    // Mirror-side delete: a late replay from an offline node is refused.
    assert!(s.delete_session(sid, true).unwrap());
    assert!(s.is_deleted(sid).unwrap());
    assert!(s.events_after(sid, 0, 100).unwrap().is_empty());
    for e in script(sid, 5) {
        assert_eq!(s.append("n", &e).unwrap(), AppendOutcome::Deleted);
    }
    assert!(s.session(sid).unwrap().is_none());
    assert!(!s.delete_session(sid, true).unwrap());
}

#[test]
fn expired_incognito_only_selects_idle_incognito() {
    let s = Store::open_in_memory().unwrap();
    let inc = SessionId::new();
    let mut e = created(inc);
    e.payload["incognito"] = json!(true);
    s.append("n", &e).unwrap();
    let normal = SessionId::new();
    s.append("n", &created(normal)).unwrap();
    assert_eq!(s.expired_incognito(1_000).unwrap(), vec![inc]);
    assert!(s.expired_incognito(0).unwrap().is_empty());
    // A running turn is never purged underneath the user.
    s.append(
        "n",
        &ev(
            inc,
            2,
            EventKind::TurnStarted(TurnStarted {
                cause: None,
                agent: None,
                turn_id: "t".into(),
            }),
        ),
    )
    .unwrap();
    assert!(s.expired_incognito(1_000).unwrap().is_empty());
}

/// A finished turn whose last message holds a question or form the UI
/// shows waits for the user, like one ending with "?"; a block Rust
/// rejected is code, not a question.
#[test]
fn a_shown_input_block_needs_attention() {
    let s = Store::open_in_memory().unwrap();
    let needs_you = |text: &str| -> bool {
        let sid = SessionId::new();
        let blocks = harness_core::ui::extract(text, harness_core::agent::UiLevel::Rich);
        let kinds = [
            EventKind::TurnStarted(TurnStarted {
                cause: None,
                agent: None,
                turn_id: "t1".into(),
            }),
            EventKind::AssistantMessage(AssistantMessage {
                turn_id: "t1".into(),
                message_id: "m1".into(),
                text: text.into(),
                tool_calls: vec![],
                author: None,
                variant: None,
                blocks,
            }),
            EventKind::TurnEnded(TurnEnded {
                turn_id: "t1".into(),
                reason: TurnEndReason::Completed,
                usage: None,
            }),
        ];
        s.append("local", &created(sid)).unwrap();
        for (i, k) in kinds.into_iter().enumerate() {
            s.append("local", &ev(sid, i as u64 + 2, k)).unwrap();
        }
        s.attention(None, "viewer")
            .unwrap()
            .iter()
            .any(|a| a.session_id == sid && a.reason == "question")
    };
    let fence = |body: &str| format!("Pick one.\n\n```ui:choices\n{body}\n```\n");
    assert!(needs_you(&fence(
        r#"{"id": "c", "question": "Which?", "options": ["a", "b"]}"#
    )));
    assert!(!needs_you(&fence(r#"{"id": "c", "question": "Which?"}"#)));
    assert!(!needs_you("Done."));
    assert!(needs_you("Shall I go on?"));
}

#[test]
fn an_open_message_names_the_agent_that_wrote_it() {
    let s = Store::open_in_memory().unwrap();
    let sid = SessionId::new();
    let said = |seq, id: &str, author: Option<&str>| {
        ev(
            sid,
            seq,
            EventKind::AssistantMessage(AssistantMessage {
                turn_id: "t".into(),
                message_id: id.into(),
                text: "Which?".into(),
                tool_calls: vec![],
                author: author.map(Into::into),
                variant: None,
                blocks: vec![],
            }),
        )
    };
    let user = ev(
        sid,
        3,
        EventKind::UserMessage(UserMessage {
            turn_id: "t2".into(),
            text: "hi".into(),
            attachments: vec![],
            reply: None,
            images: vec![],
            queued: false,
        }),
    );
    for e in [
        created(sid),
        said(2, "old", Some("coder")),
        user,
        said(4, "m1", Some("writer")),
        said(5, "m2", None),
    ] {
        s.append("local", &e).unwrap();
    }
    let author = |id: &str| s.open_message_author(sid, id).unwrap();
    assert_eq!(author("m1").as_deref(), Some("writer"));
    assert_eq!(author("m2"), None);
    // Before the last user message (no longer open), or not there at all.
    assert_eq!(author("old"), None);
    assert_eq!(author("zz"), None);
    assert_eq!(s.open_message_author(SessionId::new(), "m1").unwrap(), None);
}

/// What the store holds for `sid`, in full.
fn dump(s: &Store, sid: SessionId) -> Vec<RawEvent> {
    s.events_after(sid, 0, 1000).unwrap()
}

fn delta(sid: SessionId, seq: u64, text: &str) -> RawEvent {
    ev(
        sid,
        seq,
        EventKind::AssistantDelta(AssistantDelta {
            author: None,
            variant: None,
            turn_id: "t1".into(),
            message_id: "m1".into(),
            text: text.into(),
        }),
    )
}

#[test]
fn a_repeated_seq_with_different_content_diverges_the_session_for_good() {
    let s = Store::open_in_memory().unwrap();
    let sid = SessionId::new();
    let evs = script(sid, 7);
    for e in &evs {
        assert_eq!(s.append("n", e).unwrap(), AppendOutcome::Applied);
    }
    let before = dump(&s, sid);

    // An identical resend (a reconnect) is a duplicate, as before.
    assert_eq!(s.append("n", &evs[6]).unwrap(), AppendOutcome::Duplicate);
    assert_eq!(s.diverged_seq(sid).unwrap(), None);

    // Same seq, other content: a second writer.
    let other = delta(sid, 7, "not what the first copy said");
    assert_eq!(
        s.append("n", &other).unwrap(),
        AppendOutcome::Diverged {
            seq: 7,
            first: true
        }
    );
    assert_eq!(dump(&s, sid), before);
    assert_eq!(s.diverged_seq(sid).unwrap(), Some(7));

    // Sticky: the copy's next in-order frame is refused, and so is an
    // identical resend and anything from the original writer.
    let next = delta(sid, 8, "chunk8");
    for e in [&next, &evs[6], &evs[2]] {
        assert_eq!(
            s.append("n", e).unwrap(),
            AppendOutcome::Diverged {
                seq: 7,
                first: false
            }
        );
    }
    assert_eq!(dump(&s, sid), before);
    assert_eq!(s.head_seq(sid).unwrap(), 7);

    // Deleting the session is the way out; its tombstone then applies.
    assert!(s.delete_session(sid, true).unwrap());
    assert_eq!(s.diverged_seq(sid).unwrap(), None);
    assert_eq!(s.append("n", &next).unwrap(), AppendOutcome::Deleted);
}

#[test]
fn a_different_timestamp_or_kind_under_a_used_seq_diverges() {
    for change in 0..2 {
        let s = Store::open_in_memory().unwrap();
        let sid = SessionId::new();
        let evs = script(sid, 3);
        for e in &evs {
            s.append("n", e).unwrap();
        }
        let mut other = evs[2].clone();
        if change == 0 {
            other.ts_ms += 1;
        } else {
            other.kind = "assistant_message".into();
        }
        assert!(
            matches!(
                s.append("n", &other).unwrap(),
                AppendOutcome::Diverged { seq: 3, .. }
            ),
            "change {change}"
        );
    }
}

#[test]
fn key_order_and_float_round_trips_do_not_count_as_divergence() {
    let s = Store::open_in_memory().unwrap();
    let sid = SessionId::new();
    s.append("n", &created(sid)).unwrap();
    // Floats as a model may write them in tool arguments. The live frame
    // carries the node's in-memory value, a resend the value re-read from
    // the node's log; both cross the wire as text. Without serde_json's
    // `float_roundtrip` some of these come back a bit different.
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let floats: Vec<f64> = (0..2000)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            f64::from_bits((x >> 2) | 0x3000_0000_0000_0000)
        })
        .collect();
    let live = RawEvent {
        session_id: sid,
        seq: 2,
        ts_ms: 2,
        kind: "tool_call_started".into(),
        payload: json!({"a": 1, "b": {"x": [1, 2], "y": "z"}, "f": floats}),
    };
    let wire = |e: &RawEvent| -> RawEvent {
        serde_json::from_str(&serde_json::to_string(e).unwrap()).unwrap()
    };
    assert_eq!(s.append("n", &wire(&live)).unwrap(), AppendOutcome::Applied);
    // The node re-reads its own log (text to value) and sends it again,
    // with the members in another order.
    let mut resent = wire(&wire(&live));
    resent.payload = serde_json::from_str(&format!(
        r#"{{"f": {}, "b": {{"y": "z", "x": [1, 2]}}, "a": 1}}"#,
        serde_json::to_string(&resent.payload["f"]).unwrap()
    ))
    .unwrap();
    assert_eq!(s.append("n", &resent).unwrap(), AppendOutcome::Duplicate);
    assert_eq!(s.diverged_seq(sid).unwrap(), None);
}

#[test]
fn a_revoked_nodes_frames_are_refused_at_append() {
    let s = Store::open_in_memory().unwrap();
    s.join_token_create("h", i64::MAX, None).unwrap();
    let id = s.node_join("h", 0, "key", "box").unwrap().unwrap();
    let sid = SessionId::new();
    let evs = script(sid, 4);
    s.append(&id, &evs[0]).unwrap();
    assert!(s.node_revoke(&id, 1).unwrap());
    for e in &evs[1..] {
        assert_eq!(s.append(&id, e).unwrap(), AppendOutcome::Revoked);
    }
    // A new session from the revoked node is refused as well.
    assert_eq!(
        s.append(&id, &created(SessionId::new())).unwrap(),
        AppendOutcome::Revoked
    );
    assert_eq!(s.head_seq(sid).unwrap(), 1);
    // A writer with no row (the embedded node, a node's own log) is not.
    assert_eq!(
        s.append("local", &created(SessionId::new())).unwrap(),
        AppendOutcome::Applied
    );
}

// --- What a node may set in the sessions row (Gate 3 review L-1, L-2, L-3) ---

fn raw(sid: SessionId, seq: u64, kind: &str, payload: serde_json::Value) -> RawEvent {
    RawEvent {
        session_id: sid,
        seq,
        ts_ms: seq as i64,
        kind: kind.into(),
        payload,
    }
}

fn created_json(title: &str, parent: Option<SessionId>) -> serde_json::Value {
    let mut p = json!({"title": title, "cwd": "/w", "model": "m", "backend": "native",
        "node_id": "n1", "profile": "agentic", "approval_mode": "ask"});
    if let Some(parent) = parent {
        p["parent_session"] = json!(parent);
    }
    p
}

/// A chat on node `node`, created through the master by `owner`.
fn chat_of(s: &Store, node: &str, owner: &str) -> SessionId {
    let sid = SessionId::new();
    assert_eq!(
        s.append(
            node,
            &raw(sid, 1, "session_created", created_json("t", None))
        )
        .unwrap(),
        AppendOutcome::Applied
    );
    s.claim_session(sid, Some(owner), None).unwrap();
    sid
}

/// L-2. The row is set by the session's first event only. A second
/// `session_created` from the node (here naming another user's chat on the
/// same node as parent, then an unknown parent) is not applied: the owner,
/// the parent, `incognito` and `profile` stay what the creation set.
#[test]
fn the_row_is_set_at_seq_1_and_its_owner_never_changes() {
    let s = Store::open_in_memory().unwrap();
    let alice = chat_of(&s, "n1", "alice");
    let bob = chat_of(&s, "n1", "bob");
    let mut moved = created_json("t", Some(bob));
    moved["incognito"] = json!(true);
    moved["profile"] = json!("chat");
    let out = s
        .append("n1", &raw(alice, 2, "session_created", moved))
        .unwrap();
    assert_ne!(
        out,
        AppendOutcome::Applied,
        "a second session_created is refused"
    );
    let row = s.session(alice).unwrap().unwrap();
    assert_eq!(row.owner_id.as_deref(), Some("alice"));
    assert_eq!(row.parent_id, None);
    assert!(!row.incognito);
    assert_eq!(row.profile.as_deref(), Some("agentic"));
    let out = s
        .append(
            "n1",
            &raw(
                bob,
                2,
                "session_created",
                created_json("t", Some(SessionId::new())),
            ),
        )
        .unwrap();
    assert_ne!(out, AppendOutcome::Applied);
    assert_eq!(
        s.session(bob).unwrap().unwrap().owner_id.as_deref(),
        Some("bob")
    );
}

/// L-1, the store half. A parent is stored only when it is a session of
/// the same node with the same owner: another node's chat is dropped at
/// append, another user's chat on the same node when the master stamps
/// the owner it recorded for the create.
#[test]
fn a_parent_is_kept_only_under_the_same_node_and_owner() {
    let s = Store::open_in_memory().unwrap();
    let victim = chat_of(&s, "n-victim", "alice");
    let sid = SessionId::new();
    s.append(
        "n-hostile",
        &raw(sid, 1, "session_created", created_json("t", Some(victim))),
    )
    .unwrap();
    // What `Master::create_session` does after the node's ack.
    s.claim_session(sid, Some("mallory"), None).unwrap();
    let row = s.session(sid).unwrap().unwrap();
    assert_eq!(row.owner_id.as_deref(), Some("mallory"));
    assert_eq!(row.parent_id, None, "another node's chat is no parent");

    // Same node, other owner: alice's chat on the hostile node.
    let alices = chat_of(&s, "n-hostile", "alice");
    let sid = SessionId::new();
    s.append(
        "n-hostile",
        &raw(sid, 1, "session_created", created_json("t", Some(alices))),
    )
    .unwrap();
    s.claim_session(sid, Some("mallory"), None).unwrap();
    let row = s.session(sid).unwrap().unwrap();
    assert_eq!(row.owner_id.as_deref(), Some("mallory"));
    assert_eq!(row.parent_id, None, "another user's chat is no parent");
}

/// L-3. A node's title is cut to a few hundred characters in the row, so
/// a listing never carries megabytes per chat.
#[test]
fn a_nodes_title_is_capped_in_the_row() {
    let s = Store::open_in_memory().unwrap();
    // Megabytes, yet under the event cap: the event is taken, the row cut.
    let big = "A".repeat(3 << 20);
    for _ in 0..3 {
        let sid = SessionId::new();
        assert_eq!(
            s.append(
                "n1",
                &raw(sid, 1, "session_created", created_json(&big, None)),
            )
            .unwrap(),
            AppendOutcome::Applied
        );
    }
    let rows = s.sessions().unwrap();
    assert_eq!(rows.len(), 3);
    assert!(
        rows.iter()
            .all(|r| r.title.chars().count() <= harness_store::MAX_TITLE_CHARS)
    );
    // Cut on a character boundary, and a short title is left as it is.
    let sid = SessionId::new();
    let wide = "é".repeat(300);
    s.append(
        "n1",
        &raw(sid, 1, "session_created", created_json(&wide, None)),
    )
    .unwrap();
    let title = s.session(sid).unwrap().unwrap().title;
    assert_eq!(title, "é".repeat(harness_store::MAX_TITLE_CHARS));
    let sid = SessionId::new();
    let long_but_fine = "a legitimate, long title ".repeat(7);
    s.append(
        "n1",
        &raw(
            sid,
            1,
            "session_created",
            created_json(&long_but_fine, None),
        ),
    )
    .unwrap();
    assert_eq!(s.session(sid).unwrap().unwrap().title, long_but_fine);
}

/// L-3. An event larger than `MAX_EVENT_BYTES` is refused, with nothing
/// stored; a large legitimate one (3 MiB, more than today's largest) is
/// taken.
#[test]
fn an_event_over_the_size_cap_is_refused() {
    let s = Store::open_in_memory().unwrap();
    let sid = SessionId::new();
    s.append(
        "n1",
        &raw(sid, 1, "session_created", created_json("t", None)),
    )
    .unwrap();
    let message = |seq, n: usize| {
        raw(
            sid,
            seq,
            "user_message",
            json!({"turn_id": "t1", "text": "x".repeat(n)}),
        )
    };
    assert_eq!(
        s.append("n1", &message(2, 3 << 20)).unwrap(),
        AppendOutcome::Applied
    );
    let out = s
        .append("n1", &message(3, harness_store::MAX_EVENT_BYTES))
        .unwrap();
    assert!(
        matches!(out, AppendOutcome::Refused { seq: 3, reason } if reason.contains("too large")),
        "{out:?}"
    );
    assert_eq!(s.head_seq(sid).unwrap(), 2);
    assert_eq!(s.events_after(sid, 2, 10).unwrap(), vec![]);
    // Refused, not frozen: the store leaves that to its caller (the
    // master freezes; a node's own log just fails the append).
    assert_eq!(s.diverged_seq(sid).unwrap(), None);
    assert_eq!(
        s.append("n1", &message(3, 10)).unwrap(),
        AppendOutcome::Applied
    );
}

/// L-3. Identifiers and paths a node supplies are refused past their
/// limits rather than cut; ordinary ones are taken.
#[test]
fn identifiers_and_paths_past_their_limits_are_refused() {
    let s = Store::open_in_memory().unwrap();
    let cases = [
        (
            "cwd",
            json!(format!("/{}", "d".repeat(harness_store::MAX_CWD_BYTES))),
        ),
        (
            "model",
            json!("m".repeat(harness_store::MAX_MODEL_BYTES + 1)),
        ),
        (
            "agent_id",
            json!("a".repeat(harness_store::MAX_AGENT_ID_BYTES + 1)),
        ),
        ("profile", json!("root")),
    ];
    for (field, value) in cases {
        let sid = SessionId::new();
        let mut p = created_json("t", None);
        p[field] = value;
        let out = s.append("n1", &raw(sid, 1, "session_created", p)).unwrap();
        assert!(
            matches!(out, AppendOutcome::Refused { seq: 1, .. }),
            "{field}: {out:?}"
        );
        assert!(s.session(sid).unwrap().is_none(), "{field}: no row");
    }
    let sid = SessionId::new();
    let mut p = created_json("t", None);
    p["cwd"] = json!(format!("/{}", "d".repeat(300)));
    p["model"] = json!("openrouter/anthropic/claude-some-long-model-name-20991231");
    assert_eq!(
        s.append("n1", &raw(sid, 1, "session_created", p)).unwrap(),
        AppendOutcome::Applied
    );
    let changed = raw(
        sid,
        2,
        "agent_changed",
        json!({"agent": {"agent_id": "x".repeat(1000)}}),
    );
    assert!(matches!(
        s.append("n1", &changed).unwrap(),
        AppendOutcome::Refused { seq: 2, .. }
    ));
}

/// L-3. One account's listing is filtered in SQL: it holds only that
/// account's rows; `None` (the API token) holds everyone's.
#[test]
fn a_listing_of_one_owner_reads_only_their_rows() {
    let s = Store::open_in_memory().unwrap();
    let alice = [chat_of(&s, "n1", "alice"), chat_of(&s, "n1", "alice")];
    let bob = chat_of(&s, "n1", "bob");
    let ids = |rows: Vec<harness_store::SessionRow>| {
        let mut v: Vec<SessionId> = rows.into_iter().map(|r| r.session_id).collect();
        v.sort_by_key(|s| s.to_string());
        v
    };
    let mut want = alice.to_vec();
    want.sort_by_key(|s| s.to_string());
    assert_eq!(ids(s.sessions_of(Some("alice")).unwrap()), want);
    assert_eq!(ids(s.sessions_of(Some("bob")).unwrap()), vec![bob]);
    assert!(s.sessions_of(Some("carol")).unwrap().is_empty());
    assert_eq!(s.sessions_of(None).unwrap().len(), 3);
}

/// Review S3-3: the callback watcher's read of a chat's last turn takes
/// only the kinds it asks for, in the SQL: the rest of a long turn is never
/// read or parsed. A turn's other events here include one whose payload is
/// not JSON at all; a read that parses every event of the turn fails on it.
#[test]
fn the_watchers_read_of_a_long_last_turn_is_its_ends_only() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let path = dir.path().join("m.db");
    let s = Store::open(&path, Durability::Full).unwrap();
    let sid = SessionId::new();
    assert_eq!(
        s.append("n", &created(sid)).unwrap(),
        AppendOutcome::Applied
    );
    let started = ev(
        sid,
        2,
        EventKind::TurnStarted(TurnStarted {
            turn_id: "t".into(),
            agent: None,
            cause: None,
        }),
    );
    assert_eq!(s.append("n", &started).unwrap(), AppendOutcome::Applied);
    for seq in 3..503 {
        let d = ev(
            sid,
            seq,
            EventKind::AssistantDelta(AssistantDelta {
                turn_id: "t".into(),
                message_id: "m".into(),
                text: "x".repeat(1000),
                author: None,
                variant: None,
            }),
        );
        assert_eq!(s.append("n", &d).unwrap(), AppendOutcome::Applied);
    }
    let end = ev(
        sid,
        503,
        EventKind::BackgroundEnded(BackgroundEnded {
            bg_id: "bg-1".into(),
            reason: BackgroundReason::Exited,
            exit_code: Some(0),
            signal: None,
            confirmed: BackgroundConfirmed::Ended,
            ran_ms: 1,
            output_bytes: 0,
            tail: String::new(),
            tail_cut: false,
            delivered: false,
        }),
    );
    assert_eq!(s.append("n", &end).unwrap(), AppendOutcome::Applied);
    drop(s);
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute(
        "UPDATE events SET payload = 'not json' WHERE session_id = ?1 AND seq = 3",
        [sid.to_string()],
    )
    .unwrap();
    drop(raw);
    let s = Store::open(&path, Durability::Full).unwrap();
    let got = s
        .events_since_last_turn(sid, &["background_ended"])
        .unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].seq, 503);
}

/// N.10a: the mode `session_created` recorded is on the session's row, so
/// the master can check who may drive a `host` chat on every action; a log
/// without one leaves it empty.
#[test]
fn the_recorded_execution_mode_is_on_the_row() {
    let s = Store::open_in_memory().unwrap();
    let (host, none) = (SessionId::new(), SessionId::new());
    let mut e = created(host);
    e.payload["exec_mode"] = json!("host");
    s.append("local", &e).unwrap();
    s.append("local", &created(none)).unwrap();
    assert_eq!(
        s.session(host).unwrap().unwrap().exec_mode.as_deref(),
        Some("host")
    );
    assert_eq!(s.session(none).unwrap().unwrap().exec_mode, None);
}
