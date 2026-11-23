//! The memory one build may use (PLAN.md T97). No allocator can be written
//! here (`unsafe` is forbidden), so each hostile input is built in a child
//! process, the test binary itself, and the child reports the growth of its
//! peak resident set (`VmHWM` of /proc/self/status) over what it had before
//! the build. That is the real peak, page granularity, Linux only.

use harness_docs::docx::{self, DocOptions};
use harness_docs::limits::MAX_MARKDOWN_BYTES;

/// The most one build may add to the process's peak, in KiB.
const PEAK_KIB: u64 = 64 << 10;

fn hwm_kib() -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    s.lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
        .unwrap_or(0)
}

fn fill(prefix: &str, unit: &str, suffix: &str) -> String {
    let mut s = String::from(prefix);
    while s.len() + unit.len() + suffix.len() <= MAX_MARKDOWN_BYTES {
        s.push_str(unit);
    }
    s.push_str(suffix);
    s
}

/// The four inputs a review measured at 137 to 269 MB.
fn case(n: usize) -> String {
    match n {
        0 => fill("x[***~~a", "\t", "a~~***](http://a)"),
        1 => {
            let half = "[".repeat(MAX_MARKDOWN_BYTES / 2);
            half + &"]".repeat(MAX_MARKDOWN_BYTES / 2)
        }
        2 => fill("", "[a](", ""),
        3 => fill("", "*a", ""),
        // Nesting that is not inline markup: quotes and list markers.
        4 => fill("", ">", " deep"),
        5 => fill("", "- ", "x"),
        _ => fill("", "1. ", "x"),
    }
}

/// Runs in the child only.
#[test]
fn child_builds_one_case() {
    let Ok(n) = std::env::var("HARNESS_DOCS_MEMORY_CASE") else {
        return;
    };
    let md = case(n.parse().unwrap());
    assert!(md.len() <= MAX_MARKDOWN_BYTES);
    let before = hwm_kib();
    if std::env::var("HARNESS_DOCS_MEMORY_PARSE").is_ok() {
        let n = pulldown_cmark::Parser::new_ext(
            &md,
            pulldown_cmark::Options::ENABLE_TABLES | pulldown_cmark::Options::ENABLE_STRIKETHROUGH,
        )
        .count();
        println!(
            "RESULT grew_kib={} parse only, {n} events",
            hwm_kib().saturating_sub(before)
        );
        return;
    }
    let r = docx::build(&md, &DocOptions::default());
    let grew = hwm_kib().saturating_sub(before);
    let outcome = match &r {
        Ok(b) => format!("built {} bytes", b.bytes.len()),
        Err(e) => format!("refused: {e}"),
    };
    println!("RESULT grew_kib={grew} {outcome}");
}

#[test]
fn one_build_stays_under_its_memory_bound() {
    for n in 0..7 {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "child_builds_one_case",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HARNESS_DOCS_MEMORY_CASE", n.to_string())
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success(),
            "case {n}: {text} {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let line = text
            .lines()
            .find(|l| l.contains("RESULT grew_kib="))
            .unwrap_or_else(|| panic!("case {n}: no result: {text}"));
        let grew: u64 = line
            .split("grew_kib=")
            .nth(1)
            .and_then(|r| r.split(' ').next())
            .and_then(|v| v.parse().ok())
            .unwrap();
        println!("case {n}: {line}");
        assert!(
            grew <= PEAK_KIB,
            "case {n} grew the peak by {} MiB: {line}",
            grew >> 10
        );
    }
}
