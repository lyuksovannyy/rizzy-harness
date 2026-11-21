//! Logs a rewind can be in, and logs it cannot (a corrupt or hostile
//! `conversation_rewound`): the one rule in `harness_core::rewind` against
//! an independent oracle, `visible` idempotent, and the same view in the
//! web fold (`web/src/rewindfuzz.test.ts` reads the fixture).
//! Regenerate with `UPDATE_FIXTURES=1 cargo test -p harness-core --test rewind_fuzz`.
//!
//! The rule: a rewind counts only when its range is not empty and its
//! `message_seq` is the seq of a (not queued) `user_message` of its
//! `turn_id`; one that does not counts for nothing, hides nothing (not its
//! turn's `turn_started` either), and is not listed.

use harness_core::rewind;
use harness_core::view::SessionView;
use harness_core::{RawEvent, SessionId};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

struct Rng(u64);
impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

struct Log(Vec<RawEvent>);
impl Log {
    fn push(&mut self, kind: &str, payload: Value) -> u64 {
        let seq = self.0.len() as u64 + 1;
        self.0.push(RawEvent {
            session_id: "01a19bf2-0000-7000-8000-00000000f1f2"
                .parse::<SessionId>()
                .unwrap(),
            seq,
            ts_ms: 0,
            kind: kind.into(),
            payload,
        });
        seq
    }
}

fn gen_log(seed: u64) -> Vec<RawEvent> {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let hostile = !seed.is_multiple_of(3);
    let mut l = Log(vec![]);
    let mut msgs: Vec<(u64, String)> = vec![];
    let mut rewinds: Vec<u64> = vec![];
    let mut n = 0;
    for _ in 0..(6 + rng.below(24)) {
        match rng.below(11) {
            0..=3 => {
                n += 1;
                let t = format!("t{n}");
                l.push("turn_started", json!({"turn_id": t}));
                let m = l.push(
                    "user_message",
                    json!({"turn_id": t, "text": format!("MSG{n}")}),
                );
                msgs.push((m, t.clone()));
                l.push(
                    "assistant_message",
                    json!({"turn_id": t, "message_id": format!("m{n}"), "text": format!("ANS{n}")}),
                );
                if rng.below(3) == 0 {
                    l.push(
                        "todo_updated",
                        json!({"turn_id": t, "call_id": "c", "items": [{"content": format!("todo{n}"), "status": "pending"}]}),
                    );
                }
                if rng.below(4) == 0 {
                    l.push(
                        "background_started",
                        json!({"bg_id": format!("bg-{n}"), "call_id": "c", "turn_id": t, "timeout_s": 5, "ends_by_ms": 0}),
                    );
                }
                l.push("turn_ended", json!({"turn_id": t, "reason": "completed"}));
            }
            4 if n > 0 => {
                l.push(
                    "user_message",
                    json!({"turn_id": format!("t{n}"), "text": "queued", "queued": true}),
                );
            }
            5 if n > 0 => {
                let bg = format!("bg-{}", 1 + rng.below(n));
                l.push(
                    "background_ended",
                    json!({"bg_id": bg, "reason": "exited", "confirmed": "ended", "ran_ms": 1, "output_bytes": 0}),
                );
            }
            6..=7 if !msgs.is_empty() => {
                let (m, t) = msgs[rng.below(msgs.len() as u64) as usize].clone();
                let r = l.push(
                    "conversation_rewound",
                    json!({"message_seq": m, "turn_id": t, "files": false}),
                );
                rewinds.push(r);
            }
            8 if hostile => {
                // A message_seq no message sits at, one in the future, the
                // seq of a queued message or of another turn's message,
                // with a real turn's id.
                let len = l.0.len() as u64;
                let bad = [0, 1, len + 1, len + 50, len / 2];
                let m = bad[rng.below(5) as usize];
                let t = format!("t{}", 1 + rng.below(n.max(1)));
                let r = l.push(
                    "conversation_rewound",
                    json!({"message_seq": m, "turn_id": t, "files": false}),
                );
                rewinds.push(r);
            }
            9 if !rewinds.is_empty() => {
                let r = rewinds[rng.below(rewinds.len() as u64) as usize];
                l.push("conversation_rewind_undone", json!({"rewind_seq": r}));
            }
            10 if hostile => {
                let r = rng.below(l.0.len() as u64 + 3);
                l.push("conversation_rewind_undone", json!({"rewind_seq": r}));
            }
            _ => {}
        }
    }
    l.0
}

/// An oracle written apart from `Ranges`.
fn oracle(events: &[RawEvent]) -> (HashSet<u64>, HashSet<u64>) {
    let mut msgs: HashMap<u64, String> = HashMap::new();
    let mut active: Vec<(u64, u64)> = vec![];
    for e in events {
        match e.kind.as_str() {
            "user_message" if e.payload.get("queued") != Some(&json!(true)) => {
                msgs.insert(e.seq, e.payload["turn_id"].as_str().unwrap().to_string());
            }
            "conversation_rewound" => {
                let m = e.payload["message_seq"].as_u64().unwrap();
                let t = e.payload["turn_id"].as_str().unwrap();
                if m < e.seq && msgs.get(&m).is_some_and(|x| x == t) {
                    active.retain(|a| a.0 != e.seq);
                    active.push((e.seq, m));
                }
            }
            "conversation_rewind_undone" => {
                let r = e.payload["rewind_seq"].as_u64().unwrap();
                active.retain(|a| a.0 != r);
            }
            _ => {}
        }
    }
    let hidden = events
        .iter()
        .filter(|e| active.iter().any(|(r, m)| *m <= e.seq && e.seq < *r))
        .map(|e| e.seq)
        .collect();
    let listed = events
        .iter()
        .filter(|e| e.kind == "conversation_rewound")
        .filter(|e| {
            let m = e.payload["message_seq"].as_u64().unwrap();
            m < e.seq
                && msgs
                    .get(&m)
                    .is_some_and(|x| x == e.payload["turn_id"].as_str().unwrap())
        })
        .map(|e| e.seq)
        .collect();
    (hidden, listed)
}

#[test]
fn the_rule_agrees_with_an_oracle_and_visible_is_idempotent() {
    for seed in 1..2000u64 {
        let evs = gen_log(seed);
        let (hidden, listed) = oracle(&evs);
        let ranges = rewind::rewound(&evs);
        for e in &evs {
            assert_eq!(
                ranges.holds(e.seq),
                hidden.contains(&e.seq),
                "seed {seed} seq {}",
                e.seq
            );
        }
        let once = rewind::visible(&evs).into_owned();
        let twice = rewind::visible(&once).into_owned();
        let seqs = |v: &[RawEvent]| v.iter().map(|e| e.seq).collect::<Vec<_>>();
        assert_eq!(
            seqs(&once),
            seqs(&twice),
            "seed {seed}: visible is not idempotent"
        );
        // A turn_started goes only with a rewind that counts.
        if hidden.is_empty() {
            assert_eq!(
                once.len(),
                evs.len(),
                "seed {seed}: nothing rewound, nothing dropped"
            );
        }
        let view = SessionView::fold(&evs).unwrap();
        let shown: Vec<u64> = view.rewinds.iter().map(|r| r.seq).collect();
        let mut want: Vec<u64> = listed.into_iter().collect();
        want.sort();
        assert_eq!(shown, want, "seed {seed}: the rewinds the view lists");
    }
}

#[test]
fn a_rewind_naming_no_message_of_its_turn_hides_nothing() {
    for (bad, what) in [(0u64, "seq 0"), (50, "the future"), (1, "a turn_started")] {
        let mut l = Log(vec![]);
        l.push("turn_started", json!({"turn_id": "t1"}));
        l.push("user_message", json!({"turn_id": "t1", "text": "one"}));
        l.push(
            "assistant_message",
            json!({"turn_id": "t1", "message_id": "m", "text": "ans"}),
        );
        l.push(
            "turn_ended",
            json!({"turn_id": "t1", "reason": "completed"}),
        );
        l.push(
            "conversation_rewound",
            json!({"message_seq": bad, "turn_id": "t1", "files": false}),
        );
        let ranges = rewind::rewound(&l.0);
        assert!(ranges.is_empty(), "{what}");
        assert!(!ranges.is_target("t1"), "{what}");
        let v = rewind::visible(&l.0);
        assert_eq!(v.len(), 5, "{what}: its turn_started stays");
        assert!(
            SessionView::fold(&l.0).unwrap().rewinds.is_empty(),
            "{what}"
        );
    }
    // The right seq with another turn's id, and a queued message's seq.
    let mut l = Log(vec![]);
    l.push("turn_started", json!({"turn_id": "t1"}));
    let m = l.push("user_message", json!({"turn_id": "t1", "text": "one"}));
    let q = l.push(
        "user_message",
        json!({"turn_id": "t1", "text": "q", "queued": true}),
    );
    l.push(
        "conversation_rewound",
        json!({"message_seq": m, "turn_id": "t2", "files": false}),
    );
    l.push(
        "conversation_rewound",
        json!({"message_seq": q, "turn_id": "t1", "files": false}),
    );
    assert!(rewind::rewound(&l.0).is_empty());
    // The same rewind with the right turn counts.
    l.push(
        "conversation_rewound",
        json!({"message_seq": m, "turn_id": "t1", "files": false}),
    );
    assert!(rewind::rewound(&l.0).holds(m));
}

#[test]
fn fixture_is_current() {
    let logs: Vec<Value> = (1..=40u64)
        .map(|seed| {
            let evs = gen_log(seed);
            let view = serde_json::to_value(SessionView::fold(&evs).unwrap()).unwrap();
            let sent: Vec<Value> = evs.iter().map(RawEvent::for_browser).collect();
            json!({
                "seed": seed,
                "events": sent,
                "items": view["items"],
                "rewinds": view.get("rewinds").cloned().unwrap_or(json!([])),
                "todo": view.get("todo").cloned().unwrap_or(json!([])),
            })
        })
        .collect();
    let doc = Value::Array(logs);
    let text = serde_json::to_string(&doc).unwrap() + "\n";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../web/src/__fixtures__/rewind_fuzz.json");
    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    let on_disk: Value = serde_json::from_str(
        &std::fs::read_to_string(&path).expect("fixture missing; run with UPDATE_FIXTURES=1"),
    )
    .unwrap();
    assert_eq!(on_disk, doc, "fixture is stale; run with UPDATE_FIXTURES=1");
}
