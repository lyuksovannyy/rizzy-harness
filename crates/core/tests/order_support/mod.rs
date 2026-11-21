//! Shared by `scrub_order.rs`: every kind built in full, and for each
//! position that the typed event can write as `null`, `{}` or `[]`, one
//! instance with that position emptied (the written form must decode and
//! come back byte for byte, so a field that is left out when `None` is not
//! one). All of them carry the secret in every string that is left.
#![allow(dead_code)]

use std::collections::BTreeMap;

use harness_core::event::EventKind;
use harness_core::{Secret, SecretSet};
use serde_json::{Value, json};

use super::common::{alternatives, every_kind};

pub const SECRET: &str = "k9Qz7WvR2pLx8TnB4cYh6JmD1sFa3GuE5oKiNwXe";

pub fn set() -> SecretSet {
    SecretSet::new(vec![Secret::new(SECRET.into())])
}

pub struct Instance {
    pub label: String,
    pub kind: String,
    pub payload: Value,
    /// The position emptied and what it was emptied to; none: the full one.
    pub emptied: Option<(String, Value)>,
}

fn written(kind: &str, p: &Value) -> bool {
    EventKind::from_parts(kind, p.clone()).is_ok_and(|e| e.into_parts().1 == *p)
}

fn nodes(v: &Value, path: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    match v {
        Value::String(_) => out.push(path.clone()),
        Value::Array(a) => {
            out.push(path.clone());
            for (i, x) in a.iter().enumerate() {
                path.push(i.to_string());
                nodes(x, path, out);
                path.pop();
            }
        }
        Value::Object(m) => {
            out.push(path.clone());
            for (k, x) in m {
                path.push(k.clone());
                nodes(x, path, out);
                path.pop();
            }
        }
        _ => {}
    }
}

fn filled_at<'a>(v: &'a Value, path: &[String]) -> Option<&'a Value> {
    path.iter().try_fold(v, |v, seg| match v {
        Value::Array(a) => a.get(seg.parse::<usize>().ok()?),
        other => other.get(seg),
    })
}

fn at_mut<'a>(v: &'a mut Value, path: &[String]) -> &'a mut Value {
    let mut cur = v;
    for seg in path {
        cur = match cur {
            Value::Array(a) => &mut a[seg.parse::<usize>().unwrap()],
            other => other.get_mut(seg).unwrap(),
        };
    }
    cur
}

fn put(v: &mut Value, path: &[String], to: Value) {
    let mut cur = v;
    for seg in path {
        cur = match cur {
            Value::Array(a) => &mut a[seg.parse::<usize>().unwrap()],
            other => other.get_mut(seg).unwrap(),
        };
    }
    *cur = to;
}

/// The full instances (every kind, and a kind's alternatives), each
/// followed by its emptied variants, grouped by kind.
pub fn instances() -> BTreeMap<String, Vec<Instance>> {
    let mut out: BTreeMap<String, Vec<Instance>> = BTreeMap::new();
    let all = every_kind(SECRET).into_iter().chain(alternatives(SECRET));
    for (n, k) in all.enumerate() {
        let (kind, filled) = k.into_parts();
        let group = out.entry(kind.clone()).or_default();
        group.push(Instance {
            label: format!("{kind}#{n}"),
            kind: kind.clone(),
            payload: filled.clone(),
            emptied: None,
        });
        let mut paths = vec![];
        nodes(&filled, &mut vec![], &mut paths);
        for path in paths {
            // Not below free JSON (a position that takes a number: a
            // `serde_json::Value`): its root is one position, whatever
            // is in it, and every key below it would be one more run.
            let below_free = (1..path.len()).any(|n| {
                let mut p = filled.clone();
                put(&mut p, &path[..n], json!(0));
                written(&kind, &p)
            });
            if path.is_empty() || below_free {
                continue;
            }
            // A map takes a renamed key and a struct does not: the
            // instance with the secret in its first key is one only where
            // the position is a map (a map's keys are free text).
            if let Some(Value::Object(m)) = filled_at(&filled, &path)
                && let Some(k) = m.keys().next()
            {
                let mut p = filled.clone();
                if let Value::Object(m) = at_mut(&mut p, &path) {
                    let v = m.remove(k).unwrap();
                    m.insert(format!("x{SECRET}{k}"), v);
                }
                if written(&kind, &p) {
                    let at = path.join(".");
                    group.push(Instance {
                        label: format!("{kind}#{n} {at} first key with the secret"),
                        kind: kind.clone(),
                        payload: p,
                        emptied: None,
                    });
                }
            }
            for empty in [Value::Null, json!({}), json!([])] {
                let mut p = filled.clone();
                put(&mut p, &path, empty.clone());
                if p == filled || !written(&kind, &p) {
                    continue;
                }
                let at = path.join(".");
                group.push(Instance {
                    label: format!("{kind}#{n} {at} -> {empty}"),
                    kind: kind.clone(),
                    payload: p,
                    emptied: Some((at, empty)),
                });
            }
        }
    }
    out
}

/// The order a kind's instances are met in.
#[derive(Clone, Copy, Debug)]
pub enum Order {
    FullFirst,
    EmptiedFirst,
    EmptiedFirstReversed,
}

impl Order {
    pub fn name(self) -> &'static str {
        match self {
            Order::FullFirst => "full",
            Order::EmptiedFirst => "emptied",
            Order::EmptiedFirstReversed => "reversed",
        }
    }

    pub fn parse(s: &str) -> Order {
        match s {
            "full" => Order::FullFirst,
            "emptied" => Order::EmptiedFirst,
            "reversed" => Order::EmptiedFirstReversed,
            other => panic!("order {other}"),
        }
    }

    pub fn arrange(self, group: &[Instance]) -> Vec<&Instance> {
        let full = |i: &&Instance| i.emptied.is_none();
        let (fulls, mut emptied): (Vec<_>, Vec<_>) = group.iter().partition(full);
        match self {
            Order::FullFirst => fulls.into_iter().chain(emptied).collect(),
            Order::EmptiedFirst => emptied.into_iter().chain(fulls).collect(),
            Order::EmptiedFirstReversed => {
                emptied.reverse();
                emptied.into_iter().chain(fulls).collect()
            }
        }
    }
}

/// Scrub one instance as a node would: the outcome and what is stored.
pub fn scrub(set: &SecretSet, i: &Instance) -> Value {
    let mut p = i.payload.clone();
    let outcome = set.scrub_event(&i.kind, &mut p);
    json!({ "outcome": format!("{outcome:?}"), "payload": p })
}
