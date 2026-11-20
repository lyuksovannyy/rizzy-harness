//! Which turns read as undone after a chain of undos and put-backs (X.42).
//! Restoring a saved id returns the folder to what it was just before the
//! restore that saved it, so the undone flags return too. The reviewer's
//! sequences S1 to S5, with the flags expected after every event; the web
//! fold reads the same fixture (`web/src/restoreflags.test.ts`).
//! Regenerate with `UPDATE_FIXTURES=1 cargo test -p harness-core --test restore_flags`.

use harness_core::event::*;
use harness_core::view::SessionView;
use harness_core::{RawEvent, SessionId};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// `b:t` before, `a:t` after, `br:R` before_restore, `r:t` restored.
fn events(tokens: &[&str]) -> Vec<RawEvent> {
    let sid: SessionId = "01a19bf2-0000-7000-8000-00000000f1f0".parse().unwrap();
    tokens
        .iter()
        .enumerate()
        .map(|(i, tok)| {
            let (p, id) = tok.split_once(':').unwrap();
            let phase = match p {
                "b" => CheckpointPhase::Before,
                "a" => CheckpointPhase::After,
                "br" => CheckpointPhase::BeforeRestore,
                "r" => CheckpointPhase::Restored,
                _ => panic!("{tok}"),
            };
            // A distinct ref per event, so a before/after mix-up shows.
            let (kind, payload) = EventKind::Checkpoint(Checkpoint {
                turn_id: id.into(),
                phase,
                git_ref: format!("ref{}-{tok}", i + 1),
                files: vec!["a".into()],
                excluded: vec![],
                excluded_more: 0,
                head_moved: None,
            })
            .into_parts();
            RawEvent {
                session_id: sid,
                seq: i as u64 + 1,
                ts_ms: 0,
                kind,
                payload,
            }
        })
        .collect()
}

/// The turns read as undone, with the id each says it saved.
fn flags(v: &SessionView) -> BTreeMap<String, Option<String>> {
    v.checkpoints
        .iter()
        .filter(|(_, c)| c.restored)
        .map(|(k, c)| (k.clone(), c.saved.clone()))
        .collect()
}

fn want(pairs: &[(&str, &str)]) -> Value {
    json!(
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), json!(v)))
            .collect::<serde_json::Map<_, _>>()
    )
}

/// (name, tokens, the expected flags after the event at each index where they change).
type Case = (&'static str, Vec<&'static str>, Vec<(usize, Value)>);

fn cases() -> Vec<Case> {
    vec![
        (
            "S1 undo t2, undo t1, put back R1",
            vec![
                "b:t1", "a:t1", "b:t2", "a:t2", "br:R1", "a:R1", "r:t2", "br:R2", "a:R2", "r:t1",
                "br:R3", "a:R3", "r:R1",
            ],
            vec![
                (6, want(&[("t2", "R1")])),
                (9, want(&[("t1", "R2"), ("t2", "R1")])),
                (12, want(&[])),
            ],
        ),
        (
            "S2 rewind, put back, undo again",
            vec![
                "b:t1", "a:t1", "b:t2", "a:t2", "br:R1", "a:R1", "r:t1", "r:t2", "br:R2", "a:R2",
                "r:R1", "br:R3", "a:R3", "r:t1",
            ],
            vec![
                (6, want(&[("t1", "R1")])),
                (7, want(&[("t1", "R1"), ("t2", "R1")])),
                (10, want(&[])),
                (13, want(&[("t1", "R3")])),
            ],
        ),
        (
            "S3 a failed undo saved an id, then an undo",
            vec![
                "b:t1", "a:t1", "b:t2", "a:t2", "br:Rd", "br:R2", "a:R2", "r:t1",
            ],
            vec![(7, want(&[("t1", "R2")]))],
        ),
        (
            "S4 put back, then undo the put-back",
            vec![
                "b:t1", "a:t1", "b:t2", "a:t2", "br:R1", "a:R1", "r:t1", "br:R2", "a:R2", "r:R1",
                "br:R3", "a:R3", "r:R2",
            ],
            vec![
                (6, want(&[("t1", "R1")])),
                (9, want(&[])),
                (12, want(&[("t1", "R1")])),
            ],
        ),
        (
            "S5 a failed restore between an undo and the next",
            vec![
                "b:t1", "a:t1", "b:t2", "a:t2", "br:R1", "a:R1", "r:t1", "br:Rx", "br:R2", "a:R2",
                "r:t2",
            ],
            vec![
                (6, want(&[("t1", "R1")])),
                (10, want(&[("t1", "R1"), ("t2", "R2")])),
            ],
        ),
        (
            "C undo, put back, undo the put-back, put it back again",
            vec![
                "b:t1", "a:t1", "br:R1", "a:R1", "r:t1", "br:R2", "a:R2", "r:R1", "br:R3", "a:R3",
                "r:R2", "br:R4", "a:R4", "r:R3",
            ],
            vec![
                (4, want(&[("t1", "R1")])),
                (7, want(&[])),
                (10, want(&[("t1", "R1")])),
                (13, want(&[])),
            ],
        ),
    ]
}

fn expected_after(changes: &[(usize, Value)], i: usize) -> Value {
    changes
        .iter()
        .rev()
        .find(|(at, _)| *at <= i)
        .map_or(want(&[]), |(_, v)| v.clone())
}

#[test]
fn the_undone_flags_follow_what_a_restore_means() {
    for (name, tokens, changes) in cases() {
        let evs = events(&tokens);
        for i in 0..evs.len() {
            let v = SessionView::fold(&evs[..=i]).unwrap();
            let got: Value = json!(
                flags(&v)
                    .into_iter()
                    .map(|(k, s)| (k, json!(s.unwrap_or_default())))
                    .collect::<serde_json::Map<_, _>>()
            );
            assert_eq!(
                got,
                expected_after(&changes, i),
                "{name}, after event {i} ({})",
                tokens[i]
            );
        }
    }
}

/// Past 100 saved ids the oldest remembered state is dropped; restoring it
/// then marks it as a turn would be, and nothing panics.
#[test]
fn only_the_last_hundred_saved_ids_are_remembered() {
    let mut tokens: Vec<String> = ["b:t1", "a:t1", "br:R1", "a:R1", "r:t1"]
        .map(String::from)
        .to_vec();
    // 109 put-backs in a chain: R(n) restores R(n-1), saving R(n+1).
    for n in 2..=110 {
        tokens.extend([
            format!("br:R{n}"),
            format!("a:R{n}"),
            format!("r:R{}", n - 1),
        ]);
    }
    let t: Vec<&str> = tokens.iter().map(String::as_str).collect();
    let v = SessionView::fold(&events(&t)).unwrap();
    // 110 restores: t1 undone by the first, then flipped every time.
    assert!(!v.checkpoints["t1"].restored);
    // R1 is no longer remembered: restoring it changes no flag but its own.
    let mut more = t.clone();
    more.extend(["br:R111", "a:R111", "r:R1"]);
    let w = SessionView::fold(&events(&more)).unwrap();
    assert!(!w.checkpoints["t1"].restored);
    // A recent one still works: R109's state was "undone".
    let mut recent = t.clone();
    recent.extend(["br:R111", "a:R111", "r:R109"]);
    let x = SessionView::fold(&events(&recent)).unwrap();
    assert!(x.checkpoints["t1"].restored == flags_before(&t, "R109"));
}

/// Whether t1 was undone just before the restore that saved `id`.
fn flags_before(t: &[&str], id: &str) -> bool {
    let at = t.iter().position(|x| *x == format!("br:{id}")).unwrap();
    SessionView::fold(&events(&t[..at])).unwrap().checkpoints["t1"].restored
}

#[test]
fn fixture_is_current() {
    let doc: Vec<Value> = cases()
        .into_iter()
        .map(|(name, tokens, changes)| {
            let evs = events(&tokens);
            let after: Vec<Value> = (0..evs.len()).map(|i| expected_after(&changes, i)).collect();
            json!({
                "name": name,
                "events": evs.iter().map(|e| json!({"kind": e.kind, "payload": e.payload, "seq": e.seq, "session_id": e.session_id, "ts_ms": e.ts_ms})).collect::<Vec<_>>(),
                "after": after,
            })
        })
        .collect();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../web/src/__fixtures__/restore_flags.json");
    let text = serde_json::to_string_pretty(&doc).unwrap() + "\n";
    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    let on_disk: Value = serde_json::from_str(
        &std::fs::read_to_string(&path).expect("fixture missing; run with UPDATE_FIXTURES=1"),
    )
    .unwrap();
    assert_eq!(
        on_disk,
        Value::Array(doc),
        "restore_flags.json is stale; run with UPDATE_FIXTURES=1"
    );
}
