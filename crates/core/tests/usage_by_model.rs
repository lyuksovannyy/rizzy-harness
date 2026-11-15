//! Which model used which tokens: each model call's usage is stamped with
//! its model (`by_model`, set by the provider router), and `Usage::add`
//! keeps the per-model sums as turns, sub-agents, summaries and hand-off
//! notes are added up. Tokens in no bucket are unattributed (never priced).

use std::collections::BTreeMap;

use harness_core::event::{MAX_USAGE_MODELS, ModelTokens, Usage};
use harness_core::view::SessionView;

fn call(model: &str, input: u64, output: u64, read: u64, write: u64) -> Usage {
    Usage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: read,
        cache_write_tokens: write,
        by_model: BTreeMap::from([(
            model.to_string(),
            ModelTokens {
                input_tokens: input,
                output_tokens: output,
                cache_read_tokens: read,
                cache_write_tokens: write,
            },
        )]),
        ..Default::default()
    }
}

#[test]
fn add_keeps_per_model_sums() {
    let mut turn = Usage {
        model: Some("p::main".into()),
        ..Default::default()
    };
    turn.add(&call("p::main", 1000, 100, 600, 200));
    turn.add(&call("p::main", 1500, 50, 1000, 0));
    // A sub-agent's whole turn on the small model, already broken down.
    let mut child = Usage::default();
    child.add(&call("p::small", 300, 30, 0, 0));
    child.add(&call("p::small", 400, 40, 100, 0));
    turn.add(&child);
    assert_eq!(turn.input_tokens, 3200);
    assert_eq!(turn.output_tokens, 220);
    assert_eq!(turn.cache_read_tokens, 1700);
    assert_eq!(turn.cache_write_tokens, 200);
    assert_eq!(
        turn.by_model,
        BTreeMap::from([
            (
                "p::main".to_string(),
                ModelTokens {
                    input_tokens: 2500,
                    output_tokens: 150,
                    cache_read_tokens: 1600,
                    cache_write_tokens: 200,
                }
            ),
            (
                "p::small".to_string(),
                ModelTokens {
                    input_tokens: 700,
                    output_tokens: 70,
                    cache_read_tokens: 100,
                    cache_write_tokens: 0,
                }
            ),
        ])
    );
}

#[test]
fn a_usage_without_a_breakdown_stays_unattributed() {
    // A call no router stamped (or a log written before `by_model`): its
    // tokens count in the totals, in no model's bucket, whatever `model`
    // (the turn's window label) says.
    let mut turn = Usage::default();
    turn.add(&call("p::main", 100, 10, 0, 0));
    turn.add(&Usage {
        input_tokens: 50,
        output_tokens: 5,
        model: Some("p::main".into()),
        ..Default::default()
    });
    assert_eq!(turn.input_tokens, 150);
    assert_eq!(turn.by_model["p::main"].input_tokens, 100);
    assert_eq!(turn.by_model.len(), 1);
}

#[test]
fn the_breakdown_is_capped_and_the_rest_is_unattributed() {
    let mut u = Usage::default();
    for i in 0..MAX_USAGE_MODELS + 3 {
        u.add(&call(&format!("p::m{i:02}"), 10, 1, 0, 0));
    }
    assert_eq!(u.by_model.len(), MAX_USAGE_MODELS);
    assert_eq!(u.input_tokens, 10 * (MAX_USAGE_MODELS as u64 + 3));
    let attributed: u64 = u.by_model.values().map(|t| t.input_tokens).sum();
    assert_eq!(attributed, 10 * MAX_USAGE_MODELS as u64);
    // A model already in the map still adds up past the cap.
    let first = u.by_model.keys().next().unwrap().clone();
    u.add(&call(&first, 5, 0, 0, 0));
    assert_eq!(u.by_model[&first].input_tokens, 15);
}

#[test]
fn the_breakdown_round_trips_and_is_omitted_when_empty() {
    let u = call("p::main", 100, 10, 40, 0);
    let v = serde_json::to_value(&u).unwrap();
    assert_eq!(v["by_model"]["p::main"]["input_tokens"], 100);
    assert!(v["by_model"]["p::main"].get("cache_write_tokens").is_none());
    assert_eq!(serde_json::from_value::<Usage>(v).unwrap(), u);
    let v = serde_json::to_value(Usage::default()).unwrap();
    assert!(v.get("by_model").is_none());
}

#[test]
fn the_view_keeps_the_chat_and_the_last_turn() {
    use harness_core::event::*;
    let sid = SessionId::new();
    let ev = |seq: u64, kind: EventKind| {
        let (k, p) = kind.into_parts();
        RawEvent {
            session_id: sid,
            seq,
            ts_ms: 0,
            kind: k,
            payload: p,
        }
    };
    let mut first = call("p::main", 1000, 100, 0, 0);
    first.context_tokens = 1000;
    let mut second = call("p::main", 2000, 200, 0, 0);
    second.add(&call("p::small", 300, 30, 0, 0));
    second.context_tokens = 2000;
    let events = vec![
        ev(
            1,
            EventKind::TurnEnded(TurnEnded {
                turn_id: "t1".into(),
                reason: TurnEndReason::Completed,
                usage: Some(first),
            }),
        ),
        ev(
            2,
            EventKind::TurnEnded(TurnEnded {
                turn_id: "t2".into(),
                reason: TurnEndReason::Completed,
                usage: Some(second.clone()),
            }),
        ),
        // A hand-off note's cost is the chat's, not the last turn's.
        ev(
            3,
            EventKind::HandoffWritten(HandoffWritten {
                through_seq: 2,
                text: Some("note".into()),
                model: Some("p::small".into()),
                failed: vec![],
                usage: Some(call("p::small", 900, 60, 0, 0)),
            }),
        ),
    ];
    let view = SessionView::fold(&events).unwrap();
    assert_eq!(view.usage.by_model["p::main"].input_tokens, 3000);
    assert_eq!(view.usage.by_model["p::small"].input_tokens, 1200);
    assert_eq!(view.turn_usage, Some(second));
}
