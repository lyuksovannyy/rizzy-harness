//! X.39: what a running turn has used shows in the chat's figures. One rule,
//! in Rust and in the browser (`web/src/liveusage.test.ts` reads the same
//! file): the chat shows its finished turns plus the running turn's latest
//! `turn_usage`; `turn_ended.usage` replaces that figure (never adds to it);
//! a `turn_ended` without usage keeps it; a `turn_usage` that is a
//! duplicate, lower, for another turn or late changes nothing.

use harness_core::view::SessionView;
use harness_core::{RawEvent, SessionId};
use serde_json::Value;

fn fixture() -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../web/src/__fixtures__/live_usage.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn events() -> Vec<RawEvent> {
    serde_json::from_value(fixture()["events"].clone()).unwrap()
}

fn figures(v: &SessionView) -> Value {
    serde_json::json!({
        "input": v.usage.input_tokens,
        "output": v.usage.output_tokens,
        "context": v.context_tokens,
        "by_model_input": v.usage.by_model.get("p::m").map_or(0, |t| t.input_tokens),
        "turn": v.turn_usage.as_ref().map(|u| vec![u.input_tokens, u.output_tokens]),
    })
}

#[test]
fn the_chats_figures_follow_the_rule_after_every_event() {
    let f = fixture();
    let expected = f["expected"].as_array().unwrap();
    let events = events();
    assert_eq!(expected.len(), events.len());
    let mut v = SessionView::default();
    for (e, want) in events.iter().zip(expected) {
        v.apply(e).unwrap();
        let mut want = want.clone();
        let seq = want.as_object_mut().unwrap().remove("seq").unwrap();
        assert_eq!(seq, e.seq, "the fixture lists its events in order");
        assert_eq!(figures(&v), want, "after seq {}", e.seq);
    }
}

#[test]
fn folding_the_same_events_twice_gives_the_same_view() {
    let events = events();
    let a = SessionView::fold(&events).unwrap();
    let b = SessionView::fold(&events).unwrap();
    assert_eq!(a, b);
    // The last turn ended: nothing of it is still "running".
    assert_eq!(a.running_turn, None);
    let _: SessionId = a.session_id.unwrap();
}
