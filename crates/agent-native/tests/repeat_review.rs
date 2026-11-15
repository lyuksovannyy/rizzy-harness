//! The review of pull request 111 (second repeated-call rule), kept as
//! regression tests: its probes, asserting the fixed behaviour.
use harness_agent_native::repeat::{Admission, RepeatGuard, TOOL_REFUSE_AT};
use serde_json::json;

fn k(i: u32) -> harness_tools::CallKey {
    harness_tools::call_key(std::path::Path::new("/w"), "bash", &json!({ "command": i }))
}

const DENIED: &str = "The user denied this tool call. Ask how to proceed or try something else.";

/// F1: after the refusal the model reads a file, then runs a sound `bash`
/// command in a later message: it runs (the trial), and its success resets
/// the rule. The turn does not end.
#[test]
fn a_sound_call_in_a_later_message_runs_after_the_refusal() {
    let mut g = RepeatGuard::default();
    for i in 1..TOOL_REFUSE_AT {
        g.record(k(i), "bash", false, false, "error: network unreachable");
    }
    match g.admit(k(99), "bash") {
        Admission::Refuse { note, stop, .. } => {
            assert!(!stop);
            assert!(note.contains("is run once"), "{note}");
        }
        a => panic!("{a:?}"),
    }
    g.next_step();
    let r = harness_tools::call_key(std::path::Path::new("/w"), "read", &json!({"path": "x"}));
    assert_eq!(g.admit(r, "read"), Admission::Run);
    g.record(r, "read", false, true, "contents");
    g.next_step();
    assert_eq!(g.admit(k(200), "bash"), Admission::Run);
    assert!(g.record(k(200), "bash", false, true, "ok").is_none());
    assert!(!g.ended());
    assert_eq!(g.admit(k(201), "bash"), Admission::Run);
}

/// F2: seven denials of seven different `bash` commands do not count: the
/// eighth is offered to the person.
#[test]
fn person_denials_do_not_count_toward_the_second_rule() {
    let mut g = RepeatGuard::default();
    for i in 1..TOOL_REFUSE_AT {
        assert_eq!(g.admit(k(i), "bash"), Admission::Run);
        g.record_denied(k(i), DENIED);
    }
    assert_eq!(g.admit(k(50), "bash"), Admission::Run);
}
