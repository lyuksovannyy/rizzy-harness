//! A position's class does not depend on which payload of its kind the
//! process met first (PLAN.md T3; the defect of `artifact_staged.packed`,
//! see `scrub_class_null_first.rs`).
//!
//! The scrubber learns classes once per process, so every run here is a
//! child process of this test binary (a fresh cache each), and the parent
//! compares what the children stored. For every kind: the full instance,
//! and one instance per position the typed event can write as `null`,
//! `{}` or `[]` (see `order_support`), each with the secret in every
//! string left. Whatever the order they are met in, and however many
//! threads meet them, every instance must come out the same, the secret
//! must be gone from the free text, and the result must decode.

use std::collections::BTreeMap;
use std::process::Command;
use std::sync::{Arc, Barrier};

use harness_core::event::EventKind;
use harness_core::secret::schema::KEPT;
use serde_json::{Value, json};

mod common;
mod order_support;
use order_support::*;

const MARK: &str = "ORDER-RESULT:";

/// Run this binary's `child` test with `env`, and read what it stored.
fn spawn(env: &[(&str, &str)]) -> BTreeMap<String, Value> {
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["child", "--exact", "--nocapture", "--test-threads=1"])
        .envs(env.iter().copied())
        .output()
        .expect("spawn the child");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "child {env:?} failed:\n{text}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let line = text
        .lines()
        .find_map(|l| l.split_once(MARK).map(|(_, r)| r))
        .unwrap_or_else(|| panic!("child {env:?} printed no result:\n{text}"));
    serde_json::from_str(line).unwrap()
}

fn sequential(order: Order) -> BTreeMap<String, Value> {
    let set = set();
    let mut out = BTreeMap::new();
    for group in instances().values() {
        for i in order.arrange(group) {
            out.insert(i.label.clone(), scrub(&set, i));
        }
    }
    out
}

/// The kind of the instance `first` met before every instance of its kind.
fn first_then_all(first: &str) -> BTreeMap<String, Value> {
    let set = set();
    let all = instances();
    let group = all
        .values()
        .find(|g| g.iter().any(|i| i.label == first))
        .expect("label");
    let start = group.iter().find(|i| i.label == first).unwrap();
    scrub(&set, start);
    group
        .iter()
        .map(|i| (i.label.clone(), scrub(&set, i)))
        .collect()
}

/// Every instance of the one kind `kind` scrubbed by `threads` threads at
/// once, each starting at another instance, twice round.
fn concurrent(kind: &str, threads: usize) -> BTreeMap<String, Value> {
    let set = Arc::new(set());
    let group = Arc::new(instances().remove(kind).expect("kind"));
    let gate = Arc::new(Barrier::new(threads));
    let handles: Vec<_> = (0..threads)
        .map(|t| {
            let (set, group, gate) = (set.clone(), group.clone(), gate.clone());
            std::thread::spawn(move || {
                gate.wait();
                let n = group.len();
                let mut seen = BTreeMap::new();
                for r in 0..2 * n {
                    let i = &group[(t * 7 + r) % n];
                    seen.insert(i.label.clone(), scrub(&set, i));
                }
                seen
            })
        })
        .collect();
    let mut all: BTreeMap<String, Value> = BTreeMap::new();
    for h in handles {
        for (k, v) in h.join().unwrap() {
            if let Some(was) = all.get(&k) {
                assert_eq!(*was, v, "two threads stored {k} differently");
            }
            all.insert(k, v);
        }
    }
    all
}

/// Not a test of its own: the body of every child.
#[test]
fn child() {
    let Ok(mode) = std::env::var("ORDER_MODE") else {
        return;
    };
    let out = match mode.as_str() {
        "sequential" => sequential(Order::parse(&std::env::var("ORDER").unwrap())),
        "first" => first_then_all(&std::env::var("ORDER_FIRST").unwrap()),
        "concurrent" => concurrent(&std::env::var("ORDER_KIND").unwrap(), 8),
        other => panic!("mode {other}"),
    };
    println!("{MARK}{}", serde_json::to_string(&out).unwrap());
}

#[test]
fn every_kind_has_positions_that_can_be_empty() {
    let all = instances();
    let emptied: Vec<_> = all
        .values()
        .flatten()
        .filter(|i| i.emptied.is_some())
        .collect();
    for i in all.values().flatten().filter(|i| i.label.contains(' ')) {
        println!("variant: {}", i.label);
    }
    assert!(
        emptied.iter().any(|i| i.label.contains("packed -> null")),
        "the packed position of artifact_staged is among them"
    );
    for (kind, group) in &all {
        assert!(!group.is_empty(), "{kind}");
    }
}

/// The table: whichever instance of a kind comes first, every instance is
/// stored the same, the secret is gone from every free position (the
/// full-first order is the one `scrub_event.rs` checks against its
/// oracle), and each result decodes as its kind.
#[test]
fn the_order_instances_are_met_in_changes_nothing() {
    let reference = spawn(&[("ORDER_MODE", "sequential"), ("ORDER", "full")]);
    assert!(!reference.is_empty());
    for order in [Order::EmptiedFirst, Order::EmptiedFirstReversed] {
        let got = spawn(&[("ORDER_MODE", "sequential"), ("ORDER", order.name())]);
        let mut wrong = vec![];
        for (label, want) in &reference {
            if got.get(label) != Some(want) {
                wrong.push(format!(
                    "{label}\n  full first: {want}\n  {}: {}",
                    order.name(),
                    got.get(label).unwrap_or(&json!(null))
                ));
            }
        }
        assert!(
            wrong.is_empty(),
            "{} instances differ when {} come first:\n{}",
            wrong.len(),
            order.name(),
            wrong.join("\n")
        );
    }
    // The reference is itself scrubbed, and decodes.
    let by_label: BTreeMap<_, _> = instances()
        .into_values()
        .flatten()
        .map(|i| (i.label.clone(), i))
        .collect();
    let mut scrubbed = 0;
    for (label, r) in &reference {
        let i = &by_label[label];
        let p = r["payload"].clone();
        assert!(
            EventKind::from_parts(&i.kind, p.clone()).is_ok(),
            "{label} no longer decodes: {p}"
        );
        scrubbed += usize::from(p != i.payload);
        // The secret is gone: what is left of it is at a kept position.
        let outcome = r["outcome"].as_str().unwrap_or_default();
        assert!(
            outcome == "Scrubbed" || outcome == "Clean",
            "{label}: {outcome}"
        );
        let kept = KEPT
            .iter()
            .find(|(k, _)| *k == i.kind)
            .map_or(&[][..], |(_, paths)| *paths);
        let mut left = vec![];
        survivors(&p, &mut vec![], &mut left);
        left.retain(|at| !kept.contains(&at.as_str()));
        assert!(
            left.is_empty(),
            "{label}: the secret is still at {left:?}: {p}"
        );
    }
    assert!(scrubbed > reference.len() / 2, "most of them had text");
}

/// The positions of `v`, written as [`KEPT`] writes them, where a string
/// or a key still holds the secret.
fn survivors(v: &Value, path: &mut Vec<String>, out: &mut Vec<String>) {
    let at = |path: &[String]| path.join(".").replace(".[]", "[]");
    match v {
        Value::String(s) if s.contains(SECRET) => out.push(at(path)),
        Value::Array(a) => {
            for x in a {
                path.push("[]".into());
                survivors(x, path, out);
                path.pop();
            }
        }
        Value::Object(m) => {
            for (k, x) in m {
                let held = k.contains(SECRET);
                path.push(if held { "{}".into() } else { k.clone() });
                if held {
                    out.push(at(path));
                }
                survivors(x, path, out);
                path.pop();
            }
        }
        _ => {}
    }
}

/// The strictest table: each instance in turn is the very first of its
/// kind the process meets (a process of its own each), then all of the
/// kind follow. Every instance comes out as it does in the reference.
#[test]
fn whichever_instance_comes_first_every_instance_is_stored_the_same() {
    let reference = spawn(&[("ORDER_MODE", "sequential"), ("ORDER", "full")]);
    let mut wrong = vec![];
    let mut runs = 0;
    for i in instances().into_values().flatten() {
        let got = spawn(&[("ORDER_MODE", "first"), ("ORDER_FIRST", &i.label)]);
        runs += 1;
        for (label, v) in &got {
            if reference.get(label) != Some(v) {
                wrong.push(format!("first {} -> {label} differs: {v}", i.label));
            }
        }
    }
    assert!(runs > 20, "{runs} runs");
    assert!(
        wrong.is_empty(),
        "{} differences:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

/// No outcome depends on the interleaving: `artifact_staged` (the kind
/// with the most positions that can be empty) met by eight threads at
/// once, in a process of its own, 200 times: the same as one thread's.
#[test]
fn threads_meeting_a_kind_together_store_what_one_would() {
    let kind = "artifact_staged";
    let reference = spawn(&[("ORDER_MODE", "concurrent"), ("ORDER_KIND", kind)]);
    let alone: BTreeMap<_, _> = {
        let want = spawn(&[("ORDER_MODE", "sequential"), ("ORDER", "full")]);
        want.into_iter()
            .filter(|(k, _)| k.starts_with(kind))
            .collect()
    };
    assert_eq!(reference, alone, "the first run");
    for run in 1..200 {
        let got = spawn(&[("ORDER_MODE", "concurrent"), ("ORDER_KIND", kind)]);
        assert_eq!(got, alone, "run {run}");
    }
}
