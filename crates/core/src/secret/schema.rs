//! Scrubbing an event without touching its schema (PLAN.md T3).
//!
//! A configured secret can be any string, including a word the harness
//! writes itself: a field name (`approval_mode`), a type tag, an enum value
//! (`accept_edits`), an id, the chat's folder or a model's name. Scrubbing
//! such a position breaks the event (it no longer decodes, and a node whose
//! `session_created` does not decode cannot restart) or rewrites what the
//! harness relies on. So only free text is scrubbed: what a model, a tool,
//! a provider or a person wrote. Which positions are free is decided by the
//! event's own types, so a kind or field added later is covered without a
//! list to keep:
//! - a position whose change the typed event does not take back unchanged
//!   (a struct's field name, a tag, an enum value, a number, a UUID) is
//!   schema;
//! - a `String` field, a `serde_json::Value` (a tool's input, a ui spec, a
//!   form's values) and a map's keys are free, except the `String` fields
//!   named in [`HARNESS_FIELDS`], which hold what the harness wrote.

use std::collections::HashMap;

use serde_json::Value;

use super::SecretSet;
use crate::event::EventKind;

/// `String` fields of the event schema whose value the harness writes from
/// its own state (ids it made, the chat's folder, the configured model and
/// agent, hashes): never scrubbed, wherever they are in an event. Only as a
/// typed field: the same name as a key of a tool's JSON input is free.
/// Values a model or a tool chose (call ids, tool names, ui block ids,
/// paths, titles, mime types) are not here: they stay scrubbed. A field
/// added under one of these names must hold harness-written values only.
pub const HARNESS_FIELDS: &[&str] = &[
    "agent_id",
    "approval_id",
    "backend",
    "base",
    "bg_id",
    "bg_ids",
    // A callback turn's `cause`: the ids of the background commands.
    "callback",
    "continued_by",
    "cwd",
    "digest",
    "effort",
    "failed_model",
    "from",
    "git_ref",
    "grant_id",
    "group",
    "hash",
    "key",
    "media_id",
    "message_id",
    "model",
    "node_id",
    "prompt_hash",
    "sha256",
    "stage_id",
    "turn_id",
    "withheld",
];

#[derive(Clone, Debug)]
enum Seg {
    Key(String),
    Index(usize),
}

/// One change [`SecretSet::scrub_value`] would make: a string value at
/// `path` replaced, or (`key`) a key of the object at `path` renamed.
struct Edit {
    path: Vec<Seg>,
    key: Option<String>,
    to: String,
}

impl Edit {
    /// What makes edits alike: the same position in every element of an
    /// array (indices left out) and the same original text. Alike edits are
    /// schema or free together (one `Vec<T>`'s elements share a type).
    fn class(&self, from: &str) -> String {
        let mut s = String::new();
        for seg in &self.path {
            match seg {
                Seg::Key(k) => {
                    s.push('.');
                    s.push_str(k);
                }
                Seg::Index(_) => s.push_str("[]"),
            }
        }
        if self.key.is_some() {
            s.push_str(" key");
        }
        s.push('\0');
        s.push_str(from);
        s
    }

    /// The field name the value sits under (an array's elements under the
    /// array's), for [`HARNESS_FIELDS`].
    fn field(&self) -> Option<&str> {
        if self.key.is_some() {
            return None;
        }
        self.path.iter().rev().find_map(|s| match s {
            Seg::Key(k) => Some(k.as_str()),
            Seg::Index(_) => None,
        })
    }
}

fn collect(set: &SecretSet, v: &Value, path: &mut Vec<Seg>, out: &mut Vec<(Edit, String)>) {
    match v {
        Value::String(s) => {
            let to = set.scrub(s);
            if to != *s {
                out.push((
                    Edit {
                        path: path.clone(),
                        key: None,
                        to,
                    },
                    s.clone(),
                ));
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                path.push(Seg::Index(i));
                collect(set, x, path, out);
                path.pop();
            }
        }
        Value::Object(m) => {
            for (k, x) in m {
                let to = set.scrub(k);
                if to != *k {
                    out.push((
                        Edit {
                            path: path.clone(),
                            key: Some(k.clone()),
                            to,
                        },
                        k.clone(),
                    ));
                }
                path.push(Seg::Key(k.clone()));
                collect(set, x, path, out);
                path.pop();
            }
        }
        _ => {}
    }
}

fn at<'a>(v: &'a mut Value, path: &[Seg]) -> Option<&'a mut Value> {
    path.iter().try_fold(v, |v, seg| match seg {
        Seg::Key(k) => v.get_mut(k.as_str()),
        Seg::Index(i) => v.get_mut(*i),
    })
}

/// `base` with `edits` made; with `probe`, each value replaced by a number
/// instead (a typed `String` does not take one, free JSON does).
fn apply(base: &Value, edits: &[&Edit], probe: bool) -> Value {
    let mut v = base.clone();
    // Values first, by paths of the original keys; then keys, deepest
    // first, so a renamed key never hides a path still to be followed.
    for e in edits.iter().filter(|e| e.key.is_none()) {
        if let Some(x) = at(&mut v, &e.path) {
            *x = if probe {
                Value::from(0)
            } else {
                Value::String(e.to.clone())
            };
        }
    }
    let mut keys: Vec<&&Edit> = edits.iter().filter(|e| e.key.is_some()).collect();
    keys.sort_by_key(|e| std::cmp::Reverse(e.path.len()));
    for e in keys {
        if let (Some(Value::Object(m)), Some(k)) = (at(&mut v, &e.path), &e.key)
            && let Some(x) = m.remove(k)
        {
            m.insert(e.to.clone(), x);
        }
    }
    v
}

/// `payload` is a `kind` exactly as the typed event writes it: it decodes,
/// as that kind, and encodes back to itself.
fn holds(kind: &str, payload: &Value) -> bool {
    EventKind::from_parts(kind, payload.clone()).is_ok_and(|e| {
        let (k, p) = e.into_parts();
        k == kind && p == *payload
    })
}

/// The groups of `groups` for which `test` holds, found by halving: one
/// test when it holds for all, about `k·log2(n)` when it fails for `k`.
fn passing(groups: &[usize], test: &mut dyn FnMut(&[usize]) -> bool, out: &mut Vec<usize>) {
    if groups.is_empty() {
        return;
    }
    if test(groups) {
        out.extend_from_slice(groups);
        return;
    }
    if groups.len() == 1 {
        return;
    }
    let (a, b) = groups.split_at(groups.len() / 2);
    passing(a, test, out);
    passing(b, test, out);
}

impl SecretSet {
    /// Scrub the payload of an event of `kind` in place, its free text only
    /// (module docs): whatever the set holds, the result decodes as the same
    /// kind, with every schema position as it was. A payload with nothing to
    /// scrub is left untouched, byte for byte. A kind this build does not
    /// know has no schema to keep: every string and key is scrubbed
    /// ([`SecretSet::scrub_value`]).
    pub fn scrub_event(&self, kind: &str, payload: &mut Value) {
        if self.is_empty() {
            return;
        }
        let mut found = vec![];
        collect(self, payload, &mut vec![], &mut found);
        if found.is_empty() {
            return;
        }
        let Ok(typed) = EventKind::from_parts(kind, payload.clone()) else {
            self.scrub_value(payload);
            return;
        };
        let (_, base) = typed.into_parts();
        if base != *payload {
            // Not as the typed event writes it (never from `append`): start
            // from what it does write, so every check below compares alike.
            *payload = base;
            found.clear();
            collect(self, payload, &mut vec![], &mut found);
        }
        let base = &*payload;

        // Alike edits in groups.
        let mut classes: HashMap<String, usize> = HashMap::new();
        let mut groups: Vec<Vec<usize>> = vec![];
        for (i, (e, from)) in found.iter().enumerate() {
            let g = *classes.entry(e.class(from)).or_insert_with(|| {
                groups.push(vec![]);
                groups.len() - 1
            });
            groups[g].push(i);
        }
        let edits = |gs: &[usize]| -> Vec<&Edit> {
            gs.iter()
                .flat_map(|g| groups[*g].iter().map(|i| &found[*i].0))
                .collect()
        };

        // A harness field's value is kept where it is a typed `String`
        // (a number in its place does not decode); in free JSON it is free.
        let named: Vec<usize> = (0..groups.len())
            .filter(|g| {
                found[groups[*g][0]]
                    .0
                    .field()
                    .is_some_and(|f| HARNESS_FIELDS.contains(&f))
            })
            .collect();
        let mut untyped = vec![];
        passing(
            &named,
            &mut |gs| holds(kind, &apply(base, &edits(gs), true)),
            &mut untyped,
        );
        let mut typed_field = vec![false; groups.len()];
        named.iter().for_each(|g| typed_field[*g] = true);
        untyped.iter().for_each(|g| typed_field[*g] = false);
        let open: Vec<usize> = (0..groups.len()).filter(|g| !typed_field[*g]).collect();

        // Of the rest, the edits the typed event takes back unchanged.
        let mut ok = vec![];
        passing(
            &open,
            &mut |gs| holds(kind, &apply(base, &edits(gs), false)),
            &mut ok,
        );
        if ok.is_empty() {
            return;
        }
        let all = apply(base, &edits(&ok), false);
        if holds(kind, &all) {
            *payload = all;
            return;
        }
        // Groups that pass alone but not together (none known): add them
        // one at a time, each checked, so what is stored always decodes.
        let mut kept: Vec<usize> = vec![];
        for g in ok {
            kept.push(g);
            if !holds(kind, &apply(base, &edits(&kept), false)) {
                kept.pop();
            }
        }
        *payload = apply(base, &edits(&kept), false);
    }
}
