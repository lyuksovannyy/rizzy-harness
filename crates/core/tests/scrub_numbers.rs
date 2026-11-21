//! A secret written as a JSON number (PLAN.md T3). A configured value made
//! of digits (a PIN, a numeric token) can reach an event as a number, not
//! a string: a model writes `{"pin": 12345678901}` as a tool's input, a
//! person fills a form's number field. In free JSON such a number becomes
//! the scrubbed text; a number the event's types fix (an exit code, a
//! count) cannot hold the marker and stays as it is.

use harness_core::event::*;
use harness_core::secret::schema::Outcome;
use harness_core::{Secret, SecretSet};
use serde_json::{Value, json};

const PIN: &str = "12345678901";

fn set() -> SecretSet {
    SecretSet::new(vec![Secret::new(PIN.into())])
}

fn started(input: Value) -> (String, Value) {
    EventKind::ToolCallStarted(ToolCallStarted {
        turn_id: "t".into(),
        call_id: "c".into(),
        tool: "x".into(),
        input,
        author: None,
    })
    .into_parts()
}

#[test]
fn a_number_in_a_tools_input_is_redacted() {
    let (kind, mut p) = started(json!({
        "pin": 12345678901u64,
        "text": "12345678901",
        "small": 7,
        "neg": -12345678901i64,
        "deep": { "list": [1, 12345678901u64, 991234567890155u64], "f": 12345678901.5 },
    }));
    assert_eq!(set().scrub_event(&kind, &mut p), Outcome::Scrubbed);
    assert_eq!(
        p["input"],
        json!({
            "pin": "[REDACTED]",
            "text": "[REDACTED]",
            "small": 7,
            "neg": "-[REDACTED]",
            "deep": { "list": [1, "[REDACTED]", "99[REDACTED]55"], "f": "[REDACTED].5" },
        })
    );
    assert!(EventKind::from_parts(&kind, p.clone()).is_ok(), "{p}");
    assert!(!p.to_string().contains(PIN), "{p}");
}

/// With nothing else in the event to scrub: the number alone is a hit.
#[test]
fn an_input_that_is_one_number_is_redacted() {
    let (kind, mut p) = started(json!(12345678901u64));
    assert_eq!(set().scrub_event(&kind, &mut p), Outcome::Scrubbed);
    assert_eq!(p["input"], json!("[REDACTED]"));
}

/// A form's answer: the values are free JSON under names a model chose, a
/// number field is answered with a number.
#[test]
fn a_number_in_a_forms_answer_is_redacted() {
    let kind = "user_message";
    for first in ["number", "text"] {
        let number = json!({ "pin": 12345678901u64, "n": 3 });
        let text = json!({ "pin": "12345678901", "n": "three" });
        let payload = |values: &Value| {
            json!({
                "turn_id": "t",
                "text": "sent",
                "reply": { "message_id": "m", "block_id": "b", "values": values },
            })
        };
        let order = if first == "number" {
            [&number, &text]
        } else {
            [&text, &number]
        };
        for values in order {
            let mut p = payload(values);
            assert_eq!(
                set().scrub_event(kind, &mut p),
                Outcome::Scrubbed,
                "{first} first: {p}"
            );
            assert_eq!(p["reply"]["values"]["pin"], "[REDACTED]", "{first} first");
            assert!(EventKind::from_parts(kind, p.clone()).is_ok(), "{p}");
        }
    }
}

/// A number the types fix stays, and the event is not taken for one that
/// failed to decode: the text beside it is scrubbed as ever.
#[test]
fn a_typed_number_with_the_same_digits_stays() {
    let set = SecretSet::new(vec![Secret::new("1234567890".into())]);
    let finished = |output: &str| {
        EventKind::ToolCallFinished(ToolCallFinished {
            call_id: "c".into(),
            ok: false,
            output: output.into(),
            exit_code: Some(1234567890),
            truncated: false,
            sources: vec![],
            note: None,
            seen: None,
            image: None,
        })
        .into_parts()
    };
    let (kind, mut p) = finished("the pin is 1234567890");
    assert_eq!(set.scrub_event(&kind, &mut p), Outcome::Scrubbed);
    assert_eq!(p["output"], "the pin is [REDACTED]");
    assert_eq!(p["exit_code"], 1234567890);

    let (kind, mut p) = finished("nothing here");
    let before = p.clone();
    assert_eq!(set.scrub_event(&kind, &mut p), Outcome::Clean);
    assert_eq!(p, before);
}

/// A set with no value that could be a number leaves every number alone
/// (and does not look at them).
#[test]
fn a_secret_that_is_not_a_number_leaves_numbers_alone() {
    let set = SecretSet::new(vec![Secret::new("12345678-abc".into())]);
    let (kind, mut p) = started(json!({ "n": 12345678, "s": "12345678-abc" }));
    assert_eq!(set.scrub_event(&kind, &mut p), Outcome::Scrubbed);
    assert_eq!(p["input"], json!({ "n": 12345678, "s": "[REDACTED]" }));
}

/// A kind this build does not know: its types are not known, so a number
/// is not replaced (a string there could break the build that knows it).
#[test]
fn an_unknown_kinds_number_is_left() {
    let mut p = json!({ "pin": 12345678901u64, "s": "12345678901" });
    assert_eq!(
        set().scrub_event("from_a_newer_node", &mut p),
        Outcome::Unknown
    );
    assert_eq!(p, json!({ "pin": 12345678901u64, "s": "[REDACTED]" }));
}
