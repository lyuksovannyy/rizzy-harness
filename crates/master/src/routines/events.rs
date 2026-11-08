//! Event sources: what can fire a routine besides its schedules, and which
//! `event.*` variables its prompt may use. v1 has one source, the incoming
//! webhook (its endpoint, `http/hooks.rs`, lands in Phase B). A new source
//! is one more `impl EventSource` plus its trigger kind (docs/routines.md).

use serde_json::Value;

use crate::auth::Principal;
use crate::workspaces::Access;

/// A fixed variable a source offers, with an example for the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VarSpec {
    pub path: &'static str,
    pub example: &'static str,
}

pub trait EventSource: Send + Sync {
    /// The trigger kind it fires (`routine_triggers.kind`).
    fn kind(&self) -> &'static str;
    /// Its fixed `event.*` variables.
    fn vars(&self) -> &'static [VarSpec];
    /// A prefix under which any path is allowed (a JSON body's fields).
    fn free_prefix(&self) -> Option<&'static str>;
    /// Whether `who` may fire routines from it in `ws`.
    fn may_use(&self, who: &Principal, ws: &Access) -> Result<(), String>;
    /// A trigger config from a request, checked and normalized.
    fn check_config(&self, cfg: &Value) -> Result<Value, String>;
}

/// `POST /api/hooks/{trigger_id}` with a bearer secret.
pub struct Webhook;

const WEBHOOK_VARS: &[VarSpec] = &[
    VarSpec {
        path: "event.text",
        example: "Build 812 failed",
    },
    VarSpec {
        path: "event.received_at",
        example: "2026-11-08 09:30",
    },
    VarSpec {
        path: "event.content_type",
        example: "application/json",
    },
];

impl EventSource for Webhook {
    fn kind(&self) -> &'static str {
        "webhook"
    }

    fn vars(&self) -> &'static [VarSpec] {
        WEBHOOK_VARS
    }

    fn free_prefix(&self) -> Option<&'static str> {
        Some("event.body")
    }

    fn may_use(&self, _who: &Principal, _ws: &Access) -> Result<(), String> {
        Ok(())
    }

    fn check_config(&self, cfg: &Value) -> Result<Value, String> {
        match cfg.as_object() {
            Some(o) if o.is_empty() => Ok(Value::Object(Default::default())),
            _ => Err("a webhook trigger takes no settings".into()),
        }
    }
}

/// The source behind trigger kind `kind`, if it is an event source.
pub fn source(kind: &str) -> Option<&'static dyn EventSource> {
    match kind {
        "webhook" => Some(&Webhook),
        _ => None,
    }
}
