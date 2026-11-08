//! Routines: saved instructions that start a chat on a schedule or when a
//! webhook is called (docs/routines.md). Owned by the routines backend.
//! Phase 0 holds the signatures the rest of the master calls, with bodies
//! that do nothing, so each side is built on its own.

use harness_core::SessionId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::Master;
use crate::auth::Principal;
use crate::workspaces::Access;

/// `[routines]` in config.toml; left out of the file while it is the default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RoutinesConfig {
    pub enabled: bool,
}

impl Default for RoutinesConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl RoutinesConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// The running scheduler, in `MasterInner.routines`.
pub struct RoutinesHandle {}

/// What a routine run decides about the owner's own turns in its chat.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PromptFlags {
    /// The chat holds data from an external event: every later turn there
    /// runs untrusted too (`PromptOpts::untrusted`).
    pub untrusted: bool,
}

/// Install the scheduler with the wall clock and start its loops.
pub fn start(m: &Master) {
    let _ = m.inner.routines.set(RoutinesHandle {});
}

/// The `routines` block of `/api/features` for `who` in `scope`.
pub async fn features(_m: &Master, _who: &Principal, _scope: Option<&Access>) -> Value {
    json!({ "enabled": false, "show": false, "webhooks": false, "why": null })
}

/// Give each chat row its `routine` mark, and leave out the run chats the
/// sessions list hides unless `include_runs`.
// A `Vec`, not a slice: the filter removes rows (the stub does not yet).
#[allow(clippy::ptr_arg)]
pub async fn annotate(
    _m: &Master,
    rows: &mut Vec<Value>,
    _include_runs: bool,
) -> harness_store::Result<()> {
    for row in rows.iter_mut() {
        row["routine"] = Value::Null;
    }
    Ok(())
}

/// `annotate` for one chat row.
pub async fn annotate_one(_m: &Master, row: &mut Value) -> harness_store::Result<()> {
    row["routine"] = Value::Null;
    Ok(())
}

/// Called before every prompt the owner sends: marks a run's chat as
/// continued and says whether the turn must run untrusted.
pub async fn on_owner_prompt(_m: &Master, _sid: SessionId) -> PromptFlags {
    PromptFlags { untrusted: false }
}

/// The `routines` rows of `GET /api/attention` for `who`.
pub async fn attention(_m: &Master, _who: &Principal) -> harness_store::Result<Value> {
    Ok(json!([]))
}

/// The hourly part of routines: run retention and purges.
pub async fn hourly(_m: &Master) {}
