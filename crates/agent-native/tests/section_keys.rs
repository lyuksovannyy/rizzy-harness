//! A context section's key is read back from the log to tell what the
//! model's history holds (`context::had`), so the event scrubber keeps the
//! harness's own keys as they are and a subfolder file's prefix (PLAN.md
//! T3). Its list lives in harness-core; this keeps it alike with
//! `SectionKey`.

use harness_agent_native::context::{self, SectionKey};
use harness_core::backend::StandingKey;
use harness_core::event::{ContextFrame, ContextSent, EventKind, SentSection, TurnStarted};
use harness_core::secret::schema::SECTION_KEYS;
use harness_core::{RawEvent, Secret, SecretSet, SessionId};

fn own_keys() -> Vec<SectionKey> {
    let mut keys = vec![
        SectionKey::UserInstructions,
        SectionKey::Instructions,
        SectionKey::Skills,
    ];
    keys.extend(StandingKey::ALL.map(SectionKey::Master));
    keys
}

/// Every key the harness writes, besides a subfolder file's, is one the
/// scrubber keeps: in `SECTION_KEYS`, or a master's key (which it reads
/// from `StandingKey`).
#[test]
fn the_scrubbers_section_keys_are_the_harnesss() {
    for k in own_keys() {
        let s = k.as_str();
        let standing = StandingKey::ALL
            .iter()
            .any(|m| serde_json::to_value(m).unwrap() == s.as_ref());
        assert!(
            SECTION_KEYS.contains(&s.as_ref()) || standing,
            "{s} is not kept by the scrubber"
        );
    }
    assert!(SECTION_KEYS.contains(&context::TURN));
}

/// With configured values equal to the harness's section words, a turn's
/// record still reads back as the same sections: the history's standing
/// sections compare from one turn to the next.
#[test]
fn what_the_history_holds_survives_section_words_as_secrets() {
    let mut words: Vec<String> = own_keys().iter().map(|k| k.as_str().into_owned()).collect();
    words.push(context::NESTED_PREFIX.into());
    words.push("AGENTS.md".into());
    let set = SecretSet::new(words.into_iter().map(Secret::new).collect());
    let mut keys = own_keys();
    keys.push(SectionKey::Nested("web/AGENTS.md".into()));
    let sid = SessionId::new();
    let mut log = vec![];
    for (seq, k) in [
        EventKind::TurnStarted(TurnStarted {
            turn_id: "t1".into(),
            agent: None,
            cause: None,
            environment: None,
            continued_by: None,
            retry: None,
        }),
        EventKind::ContextSent(ContextSent {
            turn_id: "t1".into(),
            frame: ContextFrame::Person,
            sections: keys
                .iter()
                .map(|k| SentSection {
                    key: k.as_str().into_owned(),
                    digest: format!("d-{}", k.as_str()),
                    text: "x".into(),
                    note: None,
                })
                .collect(),
        }),
    ]
    .into_iter()
    .enumerate()
    {
        let (kind, mut payload) = k.into_parts();
        set.scrub_event(&kind, &mut payload);
        log.push(RawEvent {
            session_id: sid,
            seq: seq as u64 + 1,
            ts_ms: 0,
            kind,
            payload,
        });
    }
    let had = context::had(&log);
    for k in &keys {
        // `AGENTS.md` is a secret here: the subfolder file's key reads as
        // a key still, with its path scrubbed the same way every turn.
        let want = match k {
            SectionKey::Nested(_) => SectionKey::Nested("web/[REDACTED]".into()),
            k => k.clone(),
        };
        assert_eq!(
            had.get(&want),
            Some(&format!("d-{}", k.as_str())),
            "{want:?} in {had:?}"
        );
    }
}
