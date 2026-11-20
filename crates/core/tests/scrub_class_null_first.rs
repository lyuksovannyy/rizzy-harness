//! A position's class does not depend on the first payload of its kind the
//! process saw (PLAN.md T3). This file is its own program, so the
//! scrubber's per-process class cache is empty when its one test starts,
//! and that test is the only one here: nothing else touches the kind first.
//!
//! The defect it pins: `artifact_staged.packed` is `Option<String>` written
//! as an explicit `null` when `None`. The class learned from that `null`
//! was "sequence" for the life of the process, so a later string there was
//! left as it was.

use harness_core::event::EventKind;
use harness_core::{Secret, SecretSet};
use serde_json::{Value, json};

mod common;

const SECRET: &str = "k9Qz7WvR2pLx8TnB4cYh6JmD1sFa3GuE5oKiNwXe";

fn staged(packed: Value) -> Value {
    let k = common::every_kind("filler")
        .into_iter()
        .find(|k| k.name() == "artifact_staged")
        .expect("artifact_staged is built");
    let (_, mut p) = k.into_parts();
    p["packed"] = packed;
    p
}

#[test]
fn a_null_first_does_not_teach_a_wrong_class() {
    let set = SecretSet::new(vec![Secret::new(SECRET.into())]);
    // The first artifact_staged of the process that has something to scrub
    // (a payload with nothing in it is never probed): `packed` is null.
    let mut first = staged(Value::Null);
    first["name"] = json!(format!("report {SECRET}"));
    set.scrub_event("artifact_staged", &mut first);
    assert_eq!(first["packed"], Value::Null);
    assert_eq!(first["name"], json!("report [REDACTED]"));
    // A later one: a string with the secret in it.
    let mut later = staged(json!(format!("tar.gz and {SECRET}")));
    set.scrub_event("artifact_staged", &mut later);
    assert_eq!(later["packed"], json!("tar.gz and [REDACTED]"), "{later}");
    assert!(
        EventKind::from_parts("artifact_staged", later).is_ok(),
        "still decodes"
    );
}
