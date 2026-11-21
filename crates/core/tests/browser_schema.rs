//! The event schema as the browser's fold guard must know it
//! (`web/src/fold.ts`, `KINDS` and `STRUCTS`): every field of every kind,
//! found by serde itself on the kinds `common::every_kind` builds in full.
//! For each position: its JSON type, whether serde refuses the payload
//! without it (required), whether serde refuses an arbitrary string there
//! (an enum or an id), and an enum's values. `web/src/schema.test.ts`
//! compares the guard's table with it, so a field or kind added in Rust
//! fails there until the table knows it.
//! Regenerate with `UPDATE_FIXTURES=1 cargo test -p harness-core --test browser_schema`.

mod common;

use std::collections::BTreeMap;

use common::{KINDS, alternatives, every_kind, slot};
use harness_core::artifacts::StagedBy;
use harness_core::event::*;
use harness_core::policy::{ApprovalMode, Decision, ExecMode};
use harness_core::todo::TodoStatus;
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Clone, Debug)]
enum Seg {
    Key(String),
    Index(usize),
}

fn at<'v>(v: &'v mut Value, ptr: &[Seg]) -> &'v mut Value {
    let mut v = v;
    for s in ptr {
        v = match s {
            Seg::Key(k) => v.get_mut(k.as_str()).unwrap(),
            Seg::Index(i) => v.get_mut(*i).unwrap(),
        };
    }
    v
}

fn parses(kind: &str, payload: Value) -> bool {
    EventKind::from_parts(kind, payload).is_ok()
}

/// The payload with `f` applied at `ptr`, through serde and back (`None`
/// when serde refuses it).
fn round(kind: &str, payload: &Value, ptr: &[Seg], f: impl FnOnce(&mut Value)) -> Option<Value> {
    let mut p = payload.clone();
    f(at(&mut p, ptr));
    EventKind::from_parts(kind, p)
        .ok()
        .map(|k| k.into_parts().1)
}

#[derive(Serialize, Clone, Debug, PartialEq)]
struct Field {
    path: String,
    /// string, number, boolean, array, object (a struct), map, any.
    #[serde(rename = "type")]
    ty: &'static str,
    required: bool,
    /// serde refuses an arbitrary string here: an enum (see `values`) or an id.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    restricted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    values: Option<Vec<String>>,
}

const PROBE: &str = "zz-not-a-value";

/// Every field below `ptr` (a struct, an array or a map), as `path` prefix.
fn walk(
    kind: &str,
    payload: &Value,
    ptr: &mut Vec<Seg>,
    prefix: &str,
    out: &mut BTreeMap<String, Field>,
) {
    let node = {
        let mut p = payload.clone();
        at(&mut p, ptr).clone()
    };
    match &node {
        Value::Object(map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            for k in &keys {
                let path = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                ptr.push(Seg::Key(k.clone()));
                let required = round(kind, payload, &ptr[..ptr.len() - 1], |o| {
                    o.as_object_mut().unwrap().remove(k);
                })
                .is_none();
                leaf_or_deeper(kind, payload, ptr, &path, required, out);
                ptr.pop();
            }
        }
        _ => unreachable!("walk is called on objects only"),
    }
}

fn leaf_or_deeper(
    kind: &str,
    payload: &Value,
    ptr: &mut Vec<Seg>,
    path: &str,
    required: bool,
    out: &mut BTreeMap<String, Field>,
) {
    let v = {
        let mut p = payload.clone();
        at(&mut p, ptr).clone()
    };
    let any = round(kind, payload, ptr, |x| *x = json!([[1]])).is_some()
        && round(kind, payload, ptr, |x| *x = json!(PROBE)).is_some();
    let mut f = Field {
        path: path.to_string(),
        ty: "any",
        required,
        restricted: false,
        values: None,
    };
    if any && !matches!(v, Value::Null) {
        merge(out, f);
        return; // a `serde_json::Value`: kept as sent, nothing inside is schema
    }
    match &v {
        Value::String(_) => {
            f.ty = "string";
            f.restricted = round(kind, payload, ptr, |x| *x = json!(PROBE)).is_none();
            merge(out, f);
        }
        Value::Number(_) => {
            f.ty = "number";
            merge(out, f);
        }
        Value::Bool(_) => {
            f.ty = "boolean";
            merge(out, f);
        }
        Value::Null => panic!("{kind}.{path} is null: every_kind must set every optional part"),
        Value::Array(items) => {
            assert!(
                !items.is_empty(),
                "{kind}.{path} is empty: every_kind must fill every list"
            );
            f.ty = "array";
            merge(out, f);
            // Elements may differ (a tagged union): a path is required only
            // when every element has it and needs it.
            let mut per: Vec<BTreeMap<String, Field>> = Vec::new();
            for i in 0..items.len() {
                let mut one = BTreeMap::new();
                ptr.push(Seg::Index(i));
                leaf_or_deeper(kind, payload, ptr, &format!("{path}[]"), true, &mut one);
                ptr.pop();
                per.push(one);
            }
            let mut all: BTreeMap<String, Field> = BTreeMap::new();
            for one in &per {
                for (p, g) in one {
                    let everywhere = per.iter().all(|o| o.get(p).is_some_and(|h| h.required));
                    let mut g = g.clone();
                    g.required = everywhere;
                    all.insert(p.clone(), g);
                }
            }
            for (_, g) in all {
                merge(out, g);
            }
        }
        Value::Object(map) => {
            // A map keeps a key it has never seen; a struct drops it.
            let first = map.values().next().cloned().unwrap_or(json!(null));
            let kept = round(kind, payload, ptr, |x| {
                x.as_object_mut()
                    .unwrap()
                    .insert("zz-new-key".into(), first.clone());
            })
            .is_some_and(|back| {
                let mut b = back;
                at(&mut b, ptr).get("zz-new-key").is_some()
            });
            if kept {
                f.ty = "map";
                merge(out, f);
                ptr.push(Seg::Key(map.keys().next().unwrap().clone()));
                leaf_or_deeper(kind, payload, ptr, &format!("{path}{{}}"), true, out);
                ptr.pop();
            } else {
                f.ty = "object";
                merge(out, f);
                walk(kind, payload, ptr, path, out);
            }
        }
    }
}

fn merge(out: &mut BTreeMap<String, Field>, f: Field) {
    match out.get_mut(&f.path) {
        Some(g) => {
            g.required &= f.required;
            g.restricted |= f.restricted;
        }
        None => {
            out.insert(f.path.clone(), f);
        }
    }
}

/// An enum's values as serde writes them. The `match` has no wildcard arm:
/// a new variant does not compile until it is listed.
macro_rules! variants {
    ($($v:path),+ $(,)?) => {{
        let all = vec![$($v),+];
        for x in &all {
            match x {
                $($v)|+ => {}
            }
        }
        all.iter().map(|x| serde_json::to_value(x).unwrap().as_str().unwrap().to_string()).collect::<Vec<String>>()
    }};
}

/// Which enum each restricted position holds.
fn enums() -> Vec<(&'static [&'static str], Vec<String>)> {
    use ApprovalMode as A;
    use BackgroundReason as R;
    vec![
        (
            &[
                "session_created.approval_mode",
                "approval_mode_changed.mode",
                "approval_mode_changed.requested",
            ],
            variants!(A::Plan, A::AskAll, A::Ask, A::AcceptEdits, A::Auto),
        ),
        (
            &["session_created.profile"],
            variants!(Profile::Chat, Profile::Agentic),
        ),
        (
            &["session_created.exec_mode"],
            variants!(ExecMode::Sandbox, ExecMode::Host),
        ),
        (
            &["turn_started.retry.reason", "turn_ended.reason"],
            variants!(
                TurnEndReason::Completed,
                TurnEndReason::Cancelled,
                TurnEndReason::Error,
                TurnEndReason::Interrupted,
                TurnEndReason::Stopped,
                TurnEndReason::Limit
            ),
        ),
        (
            &["user_message.files[].state"],
            variants!(
                MentionState::File,
                MentionState::Cut,
                MentionState::Folder,
                MentionState::Missing,
                MentionState::Refused,
                MentionState::Binary,
                MentionState::TooLarge
            ),
        ),
        (
            &["assistant_message.blocks[].type"],
            variants!(
                UiKind::Choices,
                UiKind::Form,
                UiKind::Progress,
                UiKind::Chart
            ),
        ),
        (
            &["assistant_message.thinking.blocks[].type"],
            thought_kinds(),
        ),
        (
            &["tool_output_delta.stream"],
            variants!(OutputStream::Stdout, OutputStream::Stderr),
        ),
        (
            &["approval_resolved.decision"],
            variants!(
                Decision::Allow,
                Decision::Deny,
                Decision::AllowSession,
                Decision::AllowChat
            ),
        ),
        (
            &["approval_resolved.by"],
            variants!(
                ResolvedBy::User,
                ResolvedBy::Policy,
                ResolvedBy::Cancelled,
                ResolvedBy::Interrupted
            ),
        ),
        (
            &["checkpoint.phase"],
            variants!(
                CheckpointPhase::Before,
                CheckpointPhase::After,
                CheckpointPhase::Restored,
                CheckpointPhase::BeforeRestore
            ),
        ),
        (
            &["background_ended.reason"],
            variants!(
                R::Exited,
                R::TimedOut,
                R::StoppedByAgent,
                R::StoppedByUser,
                R::AccessRevoked,
                R::SessionDeleted,
                R::NodeStopping,
                R::NodeRestarted,
                R::Forked,
                R::Finished
            ),
        ),
        (
            &["background_ended.confirmed"],
            variants!(
                BackgroundConfirmed::Ended,
                BackgroundConfirmed::StillAlive,
                BackgroundConfirmed::Unconfirmed,
                BackgroundConfirmed::Unknown
            ),
        ),
        (
            &["model_retry.reason"],
            variants!(
                RetryReason::RateLimit,
                RetryReason::Overloaded,
                RetryReason::ServerError,
                RetryReason::Network,
                RetryReason::NoAnswer
            ),
        ),
        (
            &["artifact_staged.by"],
            variants!(StagedBy::Agent, StagedBy::User),
        ),
        (
            &[
                "todo_updated.items[].status",
                "context_shortened.folded.todo[].status",
            ],
            variants!(
                TodoStatus::Pending,
                TodoStatus::InProgress,
                TodoStatus::Done
            ),
        ),
        (
            &["turn_started.cause.mention.by"],
            variants!(CalledBy::Person),
        ),
        (
            &["peers_skipped.reason"],
            variants!(
                PeersSkipReason::Stopped,
                PeersSkipReason::Restarted,
                PeersSkipReason::Unavailable,
                PeersSkipReason::Forked
            ),
        ),
        (
            &["context_sent.frame"],
            variants!(
                ContextFrame::Person,
                ContextFrame::Task,
                ContextFrame::Callback,
                ContextFrame::Step
            ),
        ),
    ]
}

/// The tags of a thinking block (`ThinkingBlock`, tagged by `type`).
fn thought_kinds() -> Vec<String> {
    let all = [
        ThinkingBlock::Signed {
            text: String::new(),
            signature: String::new(),
        },
        ThinkingBlock::Redacted {
            data: String::new(),
        },
        ThinkingBlock::Text {
            text: String::new(),
        },
    ];
    all.iter()
        .map(|b| {
            match b {
                ThinkingBlock::Signed { .. }
                | ThinkingBlock::Redacted { .. }
                | ThinkingBlock::Text { .. } => {}
            }
            serde_json::to_value(b).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

/// Restricted strings that are ids (a `SessionId` is a UUID), not enums.
const IDS: &[&str] = &[
    "session_created.parent_session",
    "session_created.forked.parent",
    "session_forked.parent",
    "background_started.child.session",
    "background_ended.child.session",
];

/// `a.b[]` and `a.b.c` are under `a.b`; a top-level path has no parent.
fn parent_path(path: &str) -> &str {
    if let Some(p) = path.strip_suffix("[]") {
        return p;
    }
    path.rfind('.').map_or("", |i| &path[..i])
}

/// Whether `path` (`a.b[].c`) leads to a value in `v`.
fn has_path(v: &Value, path: &str) -> bool {
    let mut node = v;
    for seg in path.split('.') {
        let (name, arr) = seg.strip_suffix("[]").map_or((seg, false), |s| (s, true));
        let Some(n) = node.get(name) else {
            return false;
        };
        node = n;
        if arr {
            let Some(n) = node.get(0) else {
                return false;
            };
            node = n;
        }
    }
    true
}

fn schema() -> BTreeMap<String, Vec<Field>> {
    let enums = enums();
    let mut per_kind: BTreeMap<String, Vec<Vec<Field>>> = BTreeMap::new();
    let mut seen = [false; KINDS];
    // A kind with alternatives (an enum with data) is built once per
    // alternative; a path found in only some of them is optional.
    for k in every_kind("x").into_iter().chain(alternatives("x")) {
        seen[slot(&k)] = true;
        let (kind, payload) = k.into_parts();
        let mut out = BTreeMap::new();
        walk(&kind, &payload, &mut Vec::new(), "", &mut out);
        let mut fields: Vec<Field> = out.into_values().collect();
        for f in &mut fields {
            let full = format!("{kind}.{}", f.path);
            if let Some((_, values)) = enums
                .iter()
                .find(|(paths, _)| paths.contains(&full.as_str()))
            {
                assert!(
                    f.restricted,
                    "{full} is listed as an enum but takes any string"
                );
                f.values = Some(values.clone());
            } else {
                assert!(
                    !f.restricted || IDS.contains(&full.as_str()),
                    "{full} refuses an arbitrary string: list its enum in `enums` or it in `IDS`"
                );
            }
        }
        per_kind.entry(kind).or_default().push(fields);
    }
    let mut kinds = BTreeMap::new();
    for (kind, values) in per_kind {
        let mut all: BTreeMap<String, Field> = BTreeMap::new();
        for one in &values {
            for f in one {
                // Required where its parent exists: a path under an
                // alternative is required within it, optional as a whole.
                let parent = parent_path(&f.path);
                let everywhere = values
                    .iter()
                    .filter(|o| parent.is_empty() || o.iter().any(|g| g.path == parent))
                    .all(|o| o.iter().any(|g| g.path == f.path && g.required));
                let mut g = f.clone();
                g.required = everywhere;
                all.insert(f.path.clone(), g);
            }
        }
        kinds.insert(kind, all.into_values().collect::<Vec<_>>());
    }
    assert!(seen.iter().all(|s| *s), "kinds not built: {seen:?}");
    // Every listed value is one serde takes there (a thinking block's tag
    // changes which fields the block needs, so it is checked by its match).
    for (paths, values) in &enums {
        for p in paths
            .iter()
            .filter(|p| !p.ends_with("thinking.blocks[].type"))
        {
            let (kind, path) = p.split_once('.').unwrap();
            assert!(
                kinds
                    .get(kind)
                    .is_some_and(|f| f.iter().any(|f| f.path == path)),
                "{p} is not a position of {kind}"
            );
            for v in values {
                let mut k = every_kind("x")
                    .into_iter()
                    .chain(alternatives("x"))
                    .map(EventKind::into_parts)
                    .filter(|(n, _)| n == kind)
                    .map(|(_, v)| v)
                    .find(|v| has_path(v, path))
                    .unwrap();
                let mut node = &mut k;
                for seg in path.split('.') {
                    let (name, arr) = seg.strip_suffix("[]").map_or((seg, false), |s| (s, true));
                    node = node.get_mut(name).unwrap();
                    if arr {
                        node = node.get_mut(0).unwrap();
                    }
                }
                *node = json!(v);
                assert!(parses(kind, k), "{p} = {v} does not parse");
            }
        }
    }
    kinds
}

#[test]
fn browser_schema_is_current() {
    let doc = json!({ "kinds": schema() });
    let text = serde_json::to_string_pretty(&doc).unwrap() + "\n";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../web/src/__fixtures__/schema.json");
    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    let on_disk =
        std::fs::read_to_string(&path).expect("schema missing; run with UPDATE_FIXTURES=1");
    let on_disk: Value = serde_json::from_str(&on_disk).unwrap();
    assert_eq!(
        on_disk, doc,
        "web/src/__fixtures__/schema.json is stale; run with UPDATE_FIXTURES=1"
    );
}

/// What the list must say, so a broken walk cannot be regenerated into it.
#[test]
fn the_list_knows_what_serde_requires() {
    let s = schema();
    let f = |k: &str, p: &str| {
        s[k].iter()
            .find(|f| f.path == p)
            .cloned()
            .unwrap_or_else(|| panic!("{k}.{p}"))
    };
    assert!(f("user_message", "text").required);
    assert!(!f("user_message", "queued").required);
    assert!(f("user_message", "attachments[].mime").required);
    assert_eq!(f("tool_call_started", "input").ty, "any");
    assert_eq!(f("turn_ended", "usage.by_model").ty, "map");
    assert!(f("turn_ended", "usage.by_model{}.input_tokens").required);
    assert!(f("checkpoint", "phase").values.is_some());
    assert!(f("session_created", "parent_session").restricted);
    // A tagged union: `text` is not in every thinking block.
    assert!(!f("assistant_message", "thinking.blocks[].text").required);
    assert!(f("assistant_message", "thinking.blocks[].type").required);
    assert_eq!(s.len(), KINDS);
}
