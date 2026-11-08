//! Event sources: what can fire a routine besides its schedules, and which
//! `event.*` variables its prompt may use. v1 has one source, the incoming
//! webhook (`http/hooks.rs`). A new source is one more `impl EventSource`
//! plus its trigger kind (docs/routines.md).
//!
//! An event's data is what the template sees under `event.*`:
//! `{body | text, received_at, content_type}`.

use serde_json::{Map, Value, json};

use super::template;

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

/// Something that fired a routine.
#[derive(Debug, Clone)]
pub struct Event {
    /// The source's kind (`"webhook"`).
    pub kind: &'static str,
    /// What `event.*` reads.
    pub data: Value,
    /// The SHA-256 hex of an `Idempotency-Key`.
    pub dedupe_key: Option<String>,
    /// The raw body's size and SHA-256 hex, for the audit line (which
    /// never carries the body itself).
    pub size: usize,
    pub sha256: String,
}

impl Event {
    /// The event of a JSON body.
    pub fn json(body: Value, received_at: String) -> Value {
        json!({ "body": body, "received_at": received_at, "content_type": "application/json" })
    }

    /// The event of a text body.
    pub fn text(text: String, received_at: String) -> Value {
        json!({ "text": text, "received_at": received_at, "content_type": "text/plain" })
    }

    /// A sample event typed in the editor or sent with Run now: a JSON
    /// string stands for a text body, anything else for a JSON body.
    pub fn sample(sample: Value, received_at: String) -> Value {
        match sample {
            Value::String(t) => Self::text(t, received_at),
            body => Self::json(body, received_at),
        }
    }
}

/// The largest body a webhook takes (and the largest sample event).
pub const MAX_BODY: usize = 64 * 1024;
/// The deepest a JSON body may nest.
pub const MAX_DEPTH: usize = 32;

/// How deep `v` nests: a scalar is 0, `[1]` is 1.
pub fn depth(v: &Value) -> usize {
    match v {
        Value::Array(a) => 1 + a.iter().map(depth).max().unwrap_or(0),
        Value::Object(o) => 1 + o.values().map(depth).max().unwrap_or(0),
        _ => 0,
    }
}

/// The most of an event a run keeps (`routine_runs.event`).
pub const MAX_STORED: usize = 16 * 1024;
/// Strings kept in a stored event that had to be cut.
const STORED_STRING: usize = 200;
/// Paths offered from a received event.
const MAX_PATHS: usize = 100;

/// The copy of `data` a run keeps, as JSON within `MAX_STORED`: as it is
/// when it fits; else with every string cut short; else without its body,
/// marked `truncated`. The run itself renders from the whole event.
pub fn stored(data: &Value) -> String {
    let whole = data.to_string();
    if whole.len() <= MAX_STORED {
        return whole;
    }
    let short = shorten(data).to_string();
    if short.len() <= MAX_STORED {
        return short;
    }
    let mut bare: Map<String, Value> = data.as_object().cloned().unwrap_or_default();
    bare.remove("body");
    bare.remove("text");
    bare.insert("truncated".into(), Value::Bool(true));
    Value::Object(bare).to_string()
}

/// `v` with every string longer than `STORED_STRING` cut.
fn shorten(v: &Value) -> Value {
    match v {
        Value::String(s) if s.chars().count() > STORED_STRING => {
            let mut cut: String = s.chars().take(STORED_STRING).collect();
            cut.push('…');
            Value::String(cut)
        }
        Value::Array(a) => Value::Array(a.iter().map(shorten).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), shorten(v))).collect()),
        other => other.clone(),
    }
}

/// The variables a template could use from event `data`, for the picker:
/// `event.text` for a text body, else the paths of the body's values
/// (`event.body.build.id`; an array by its first item), at most 100. Keys a
/// template cannot name are left out.
pub fn paths(data: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if data.get("text").is_some_and(Value::is_string) {
        out.push("event.text".to_string());
    }
    if let Some(body) = data.get("body") {
        let mut path = vec!["event".to_string(), "body".to_string()];
        walk_paths(body, &mut path, &mut out);
    }
    out
}

fn walk_paths(v: &Value, path: &mut Vec<String>, out: &mut Vec<String>) {
    if out.len() >= MAX_PATHS {
        return;
    }
    let children: Vec<(String, &Value)> = match v {
        Value::Object(o) => o.iter().map(|(k, v)| (k.clone(), v)).collect(),
        Value::Array(a) => a
            .first()
            .map(|v| ("0".to_string(), v))
            .into_iter()
            .collect(),
        // A leaf: `event.body` alone is no variable.
        _ if path.len() > 2 => {
            out.push(path.join("."));
            return;
        }
        _ => return,
    };
    for (key, child) in children {
        if path.len() >= template::MAX_SEGMENTS || !template::segment_ok(&key) {
            continue;
        }
        path.push(key);
        walk_paths(child, path, out);
        path.pop();
    }
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
        match cfg {
            Value::Null => Ok(json!({})),
            Value::Object(o) if o.is_empty() => Ok(json!({})),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_and_events() {
        let e = Event::sample(json!({"a": 1}), "2026-11-08 09:30".into());
        assert_eq!(e["body"]["a"], 1);
        assert_eq!(e["content_type"], "application/json");
        let e = Event::sample(json!("hello"), "t".into());
        assert_eq!(e["text"], "hello");
        assert_eq!(e["content_type"], "text/plain");
    }

    #[test]
    fn depth_counts_nesting() {
        assert_eq!(depth(&json!(1)), 0);
        assert_eq!(depth(&json!([])), 1);
        assert_eq!(depth(&json!({"a": [1, {"b": {}}]})), 4);
    }

    #[test]
    fn paths_follow_the_body() {
        let body = json!({"build": {"id": 812, "tags": ["x", "y"], "steps": [{"name": "a"}]},
                          "bad key": 1, "ok": null, "empty": {}});
        let mut p = paths(&Event::json(body, "t".into()));
        p.sort();
        assert_eq!(
            p,
            [
                "event.body.build.id",
                "event.body.build.steps.0.name",
                "event.body.build.tags.0",
                "event.body.ok",
            ]
        );
        assert_eq!(paths(&Event::text("x".into(), "t".into())), ["event.text"]);
        // A body that is not an object or array offers nothing to name.
        assert!(paths(&Event::json(json!(5), "t".into())).is_empty());
        let deep = json!({"a": {"b": {"c": {"d": {"e": {"f": {"g": 1}}}}}}});
        assert!(
            paths(&Event::json(deep, "t".into())).is_empty(),
            "past 8 parts"
        );
        let many: Map<String, Value> = (0..300).map(|i| (format!("k{i}"), json!(i))).collect();
        assert_eq!(
            paths(&Event::json(Value::Object(many), "t".into())).len(),
            MAX_PATHS
        );
    }

    #[test]
    fn stored_copies_fit() {
        let small = Event::json(json!({"a": "b"}), "t".into());
        assert_eq!(stored(&small), small.to_string());
        // Long strings are cut first, so the shape (and its paths) stay.
        let long = Event::json(json!({"log": "x".repeat(40_000), "n": 1}), "t".into());
        let kept: Value = serde_json::from_str(&stored(&long)).unwrap();
        assert_eq!(kept["body"]["n"], 1);
        assert_eq!(
            kept["body"]["log"].as_str().unwrap().chars().count(),
            STORED_STRING + 1
        );
        // Too many values even then: the body goes, the rest stays.
        let wide: Vec<String> = (0..2000).map(|i| format!("value {i}")).collect();
        let wide = Event::json(json!(wide), "t".into());
        let s = stored(&wide);
        assert!(s.len() <= MAX_STORED);
        let kept: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(kept["truncated"], true);
        assert_eq!(kept["received_at"], "t");
        assert!(kept.get("body").is_none());
    }
}
