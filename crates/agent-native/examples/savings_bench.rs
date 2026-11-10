//! Token-savings benchmark against a real model: the same bug-fix task run
//! with every method off and with the methods on, measuring tokens and
//! whether the task was solved.
//!
//! ```text
//! BENCH_BASE_URL=https://.../v1 BENCH_KEY=... BENCH_MODEL=... \
//!   cargo run -p harness-agent-native --example savings_bench -- 3
//! ```
//! The key is read from the environment only and never printed.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use harness_agent_native::llm::{ApiKind, LlmProvider};
use harness_agent_native::{NativeBackend, NativeConfig};
use harness_core::Secret;
use harness_core::backend::{AgentBackend, TurnCtx, TurnExtra, TurnHost};
use harness_core::event::{EventKind, TurnEndReason};
use harness_core::policy::{Decision, ToolRisk};
use harness_core::savings::{Savings, Style, TokenSavings};
use serde_json::Value;

struct Host {
    events: Mutex<Vec<EventKind>>,
}

#[async_trait::async_trait]
impl TurnHost for Host {
    async fn emit(&self, kind: EventKind) {
        self.events.lock().unwrap().push(kind);
    }
    async fn approve(&self, _: &str, _: &str, _: &Value, _: ToolRisk) -> Decision {
        Decision::Allow
    }
}

/// A small Python library with one bug, 40 unittest tests (one failing),
/// and enough code that reading everything is expensive.
fn fixture(dir: &Path) {
    let mut core = String::from(
        "\"\"\"Pricing helpers.\"\"\"\n\n\ndef percent(part, whole):\n    \
         \"\"\"Share of `whole` that `part` is, in percent.\"\"\"\n    if whole == 0:\n        \
         return 0.0\n    return part / whole * 10\n\n\ndef discount(price, pct):\n    \
         return round(price * (1 - pct / 100), 2)\n\n",
    );
    for i in 0..60 {
        core.push_str(&format!(
            "\ndef helper_{i}(values):\n    \"\"\"Sum of the values times {i}, ignoring None.\"\"\"\n    \
             total = 0\n    for v in values:\n        if v is None:\n            continue\n        \
             total += v * {i}\n    return total\n\n"
        ));
    }
    std::fs::create_dir_all(dir.join("calc")).unwrap();
    std::fs::create_dir_all(dir.join("tests")).unwrap();
    std::fs::write(dir.join("calc/__init__.py"), "").unwrap();
    std::fs::write(dir.join("calc/core.py"), core).unwrap();
    std::fs::write(dir.join("tests/__init__.py"), "").unwrap();
    let mut tests = String::from(
        "import unittest\nfrom calc.core import *\n\n\nclass T(unittest.TestCase):\n    \
         def test_percent(self):\n        self.assertEqual(percent(1, 4), 25.0)\n\n    \
         def test_discount(self):\n        self.assertEqual(discount(100, 15), 85.0)\n",
    );
    for i in 0..38 {
        tests.push_str(&format!(
            "\n    def test_helper_{i}(self):\n        self.assertEqual(helper_{i}([1, None, 2]), {})\n",
            3 * i
        ));
    }
    std::fs::write(dir.join("tests/test_core.py"), tests).unwrap();
    let readme: String = (0..80)
        .map(|i| format!("Line {i} of the project's long README about pricing.\n"))
        .collect();
    std::fs::write(dir.join("README.md"), readme).unwrap();
}

fn solved(dir: &Path) -> bool {
    std::process::Command::new("python3")
        .args(["-m", "unittest", "-q"])
        .current_dir(dir)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

const TASK: &str = "The test suite fails. Find the cause, fix the bug in the library (do not \
change the tests), and make all tests pass. Run the tests with `python3 -m unittest -v`.";

struct Run {
    input: u64,
    output: u64,
    cached: u64,
    steps: usize,
    solved: bool,
    secs: f64,
}

async fn one(provider: Arc<LlmProvider>, model: &str, savings: Savings) -> Run {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let backend = NativeBackend::new(
        provider,
        harness_tools::builtin(),
        NativeConfig {
            ..Default::default()
        },
    );
    let host = Host {
        events: Mutex::new(Vec::new()),
    };
    let t0 = Instant::now();
    let out = backend
        .run_turn(TurnCtx {
            session_id: harness_core::SessionId::new(),
            turn_id: "t".into(),
            history: vec![],
            user_text: TASK.into(),
            cwd: dir.path().canonicalize().unwrap(),
            model: model.into(),
            agent: None,
            compare: vec![],
            parent_session: None,
            extra: TurnExtra {
                savings,
                ..Default::default()
            },
            cancel: Default::default(),
            host: &host,
        })
        .await;
    if out.reason != TurnEndReason::Completed {
        eprintln!("  turn ended {:?}: {:?}", out.reason, out.error);
    }
    let u = out.usage.unwrap_or_default();
    let steps = host
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|e| matches!(e, EventKind::AssistantMessage(_)))
        .count();
    Run {
        input: u.input_tokens,
        output: u.output_tokens,
        cached: u.cache_read_tokens,
        steps,
        solved: solved(dir.path()),
        secs: t0.elapsed().as_secs_f64(),
    }
}

#[tokio::main]
async fn main() {
    let runs: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(2);
    let base = std::env::var("BENCH_BASE_URL").expect("BENCH_BASE_URL");
    let key = std::env::var("BENCH_KEY").expect("BENCH_KEY");
    let model = std::env::var("BENCH_MODEL").unwrap_or_else(|_| "gemini-flash-latest".into());
    let kind = match std::env::var("BENCH_KIND").as_deref() {
        Ok("anthropic") => ApiKind::Anthropic,
        _ => ApiKind::Openai,
    };
    let provider = Arc::new(LlmProvider::new(kind, &base, Some(Secret::new(key))).unwrap());
    let defaults = TokenSavings::default().for_turn(false);
    let all = TokenSavings {
        repo_map: true,
        agentic_style: Style::Full,
        ..Default::default()
    }
    .for_turn(false);
    let configs = [
        ("off", Savings::default()),
        ("defaults", defaults),
        ("all+full", all),
    ];
    println!("model {model}, {runs} run(s) per setting\n");
    println!(
        "{:<10} {:>4} {:>10} {:>8} {:>8} {:>6} {:>7} {:>6}",
        "setting", "run", "input", "output", "cached", "steps", "solved", "secs"
    );
    let mut summary = Vec::new();
    for (name, s) in configs {
        let (mut tin, mut tout, mut ok) = (0u64, 0u64, 0usize);
        for r in 0..runs {
            let x = one(provider.clone(), &model, s.clone()).await;
            println!(
                "{:<10} {:>4} {:>10} {:>8} {:>8} {:>6} {:>7} {:>6.0}",
                name,
                r + 1,
                x.input,
                x.output,
                x.cached,
                x.steps,
                x.solved,
                x.secs
            );
            tin += x.input;
            tout += x.output;
            ok += x.solved as usize;
        }
        summary.push((name, tin / runs as u64, tout / runs as u64, ok));
    }
    println!("\nmean per run:");
    let base_in = summary[0].1.max(1) as f64;
    for (name, i, o, ok) in summary {
        println!(
            "{name:<10} input {i:>8} ({:+.0}%)  output {o:>6}  solved {ok}/{runs}",
            (i as f64 / base_in - 1.0) * 100.0
        );
    }
}
