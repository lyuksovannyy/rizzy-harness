//! Domain types shared by every harness crate: the event schema, the session
//! fold, approval policy types and small utilities.

pub mod backend;
pub mod event;
pub mod policy;
pub mod secret;
pub mod view;

pub use event::{EventKind, RawEvent, SessionId};
pub use secret::Secret;

/// The product's working name. Change it here (and in Cargo package names).
pub const APP_NAME: &str = "harness";

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
