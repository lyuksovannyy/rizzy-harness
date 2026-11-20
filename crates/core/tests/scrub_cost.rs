//! What scrubbing an event costs (PLAN.md T3), next to the old scrubber
//! that rewrote every string and key (`scrub_value`). A measurement, not a
//! check, in a release build, alone:
//! `cargo test --release -p harness-core --test scrub_cost -- --ignored --nocapture --test-threads=1`.
//! The cases are the review's (r-scrubber R4, R4b): an `assistant_message`
//! whose tool input holds `n` objects with the configured value, under a
//! free key and under a key named like a kept field, with the value alone
//! and with every schema word configured as well.

use harness_core::event::*;
use harness_core::{Secret, SecretSet};
use serde_json::{Value, json};

fn set_of(words: &[&str]) -> SecretSet {
    SecretSet::new(words.iter().map(|w| Secret::new(w.to_string())).collect())
}

const W: &str = "deepseek-chat-v3";

/// Field names and enum values of 8+ bytes, as configured values.
const SCHEMA_WORDS: &[&str] = &[
    "approval_mode",
    "message_id",
    "tool_calls",
    "assistant_message",
    "session_created",
    "turn_started",
    "thinking",
    "variant",
    "by_model",
    "accept_edits",
    "in_progress",
    "completed",
    "redacted",
];

fn big_message(n: usize, key: &str) -> (String, Value) {
    let mut input = serde_json::Map::new();
    for i in 0..n {
        input.insert(format!("k{i}"), json!({ key: format!("{W} #{i}") }));
    }
    EventKind::AssistantMessage(AssistantMessage {
        turn_id: "t".into(),
        message_id: "m".into(),
        text: format!("using {W}"),
        tool_calls: vec![ToolCallRef {
            call_id: "c".into(),
            tool: "bash".into(),
            input: Value::Object(input),
        }],
        author: None,
        variant: Some(Variant {
            group: "g".into(),
            index: 0,
            model: Some(W.into()),
        }),
        blocks: vec![],
        thinking: Some(Thinking {
            model: W.into(),
            blocks: vec![ThinkingBlock::Text { text: "hm".into() }],
            prefix: String::new(),
            unsent: false,
        }),
    })
    .into_parts()
}

fn ms(f: &mut dyn FnMut()) -> f64 {
    let t = std::time::Instant::now();
    f();
    t.elapsed().as_secs_f64() * 1000.0
}

fn median_ms(f: &mut dyn FnMut()) -> f64 {
    let mut v: Vec<f64> = (0..5).map(|_| ms(f)).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[2]
}

fn status_kb(field: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with(field))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// The peak resident memory one scrub adds (VmHWM reset first).
fn peak_kb(f: &mut dyn FnMut()) -> u64 {
    if let Err(e) = std::fs::write("/proc/self/clear_refs", "5") {
        // Without the reset VmHWM is the process's all-time peak: the kB
        // columns would be wrong, not just missing. Say so once (a
        // sandboxed shell refuses this write; CI and a plain shell do not).
        static SAID: std::sync::Once = std::sync::Once::new();
        SAID.call_once(|| eprintln!("peak-memory columns are NOT valid: cannot reset VmHWM ({e})"));
    }
    let base = status_kb("VmRSS");
    f();
    status_kb("VmHWM").saturating_sub(base)
}

#[test]
#[ignore]
fn cost() {
    let plain = set_of(&[W]);
    let mut all: Vec<&str> = SCHEMA_WORDS.to_vec();
    all.push(W);
    let hostile = set_of(&all);
    let mut first = true;
    for n in [1_000usize, 10_000, 50_000] {
        for (label, key, set) in [
            ("free key `name`, set {W}", "name", &plain),
            ("key `model`, set {W}", "model", &plain),
            ("key `model`, every schema word + W", "model", &hostile),
        ] {
            let (kind, p) = big_message(n, key);
            let bytes = p.to_string().len();
            if first {
                // The first event of the process meets every path: probed.
                let t = ms(&mut || {
                    let mut x = p.clone();
                    std::hint::black_box(set.scrub_event(&kind, &mut x));
                });
                println!("first scrub of the process (paths probed): {t:.1} ms");
                first = false;
            }
            let new = median_ms(&mut || {
                let mut x = p.clone();
                std::hint::black_box(set.scrub_event(&kind, &mut x));
            });
            let old = median_ms(&mut || {
                let mut x = p.clone();
                set.scrub_value(&mut x);
                std::hint::black_box(x);
            });
            let clone = median_ms(&mut || {
                std::hint::black_box(p.clone());
            });
            let mut x = p.clone();
            let new_kb = peak_kb(&mut || {
                set.scrub_event(&kind, &mut x);
            });
            let mut y = p.clone();
            let old_kb = peak_kb(&mut || set.scrub_value(&mut y));
            println!(
                "n={n} bytes={bytes} [{label}]: scrub_event {new:.1} ms (+{new_kb} kB peak), \
                 scrub_value {old:.1} ms (+{old_kb} kB), the test's own clone {clone:.1} ms"
            );
        }
    }
}
