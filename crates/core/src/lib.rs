//! Domain types shared by every harness crate: the event schema, the session
//! fold, approval policy types and small utilities.

pub mod agent;
pub mod artifacts;
pub mod attached;
pub mod backend;
pub mod board;
pub mod connectors;
pub mod docs;
pub mod event;
pub mod exec_mode;
pub mod folder;
pub mod forge;
pub mod fork;
pub mod handoff;
pub mod history;
pub mod identity;
pub mod image;
pub mod mention;
pub mod pins;
pub mod policy;
pub mod recall;
pub mod refs;
pub mod retry;
pub mod rewind;
pub mod savings;
pub mod secret;
pub mod skills;
pub mod subagent;
pub mod text;
pub mod todo;
pub mod ui;
pub mod view;

pub use event::{EventKind, RawEvent, SessionId};
pub use secret::{Secret, SecretSet};

/// The product's working name. Change it here (and in Cargo package names).
pub const APP_NAME: &str = "harness";

/// The name outside the code: the command, the default data folder and
/// what gets installed on nodes. Code keeps the short [`APP_NAME`].
pub const BRAND: &str = "rizzy-harness";

/// The config file in the data directory.
pub const CONFIG_FILE: &str = "config.toml";

/// Environment variable prefix derived from [`APP_NAME`], e.g. `HARNESS_`.
pub fn env_prefix() -> String {
    format!("{}_", APP_NAME.to_ascii_uppercase())
}

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// A short random id (UUID v7 without dashes, 32 hex chars).
pub fn new_id() -> String {
    uuid::Uuid::now_v7().simple().to_string()
}

/// The most bytes an approval id or a call id may have to be drawn as a
/// card a person may allow (review N4).
pub const MAX_EVENT_ID: usize = 128;

/// Whether `id` has the shape of an approval id or a call id as the
/// harness and the providers make them: 1 to [`MAX_EVENT_ID`] of
/// `[A-Za-z0-9_-]`. [`new_id`] gives 32 lower-case hex digits (unchanged
/// since the first commit); call ids are the model provider's own
/// (`toolu_…`, `call_…`, the mock's `new_id`), which this alphabet covers.
/// An id outside it (`u1#x`, `x/../u1`, a space, 500 characters) is drawn
/// as an invalid card, Deny only, and answered 404 by the approval route:
/// it cannot be made to stand for another card in a URL (X.27 step 4,
/// review N4). The page mirrors this rule (`web/src/fold.ts` `validId`).
pub fn valid_event_id(id: &str) -> bool {
    (1..=MAX_EVENT_ID).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

#[cfg(test)]
mod id_tests {
    use super::*;

    #[test]
    fn event_ids() {
        for _ in 0..100 {
            assert!(valid_event_id(&new_id()));
        }
        for ok in [
            "a1",
            "toolu_01A09q90qw90lq917835lq9",
            "call_abc-1",
            &"a".repeat(MAX_EVENT_ID),
        ] {
            assert!(valid_event_id(ok), "{ok}");
        }
        for bad in [
            "",
            "u1#x",
            "u1?x",
            "x/../u1",
            "u1%2Fx",
            "a b",
            "ä",
            &"a".repeat(MAX_EVENT_ID + 1),
        ] {
            assert!(!valid_event_id(bad), "{bad}");
        }
    }
}
