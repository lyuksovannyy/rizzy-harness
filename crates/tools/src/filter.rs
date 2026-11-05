//! Shell output filtering: what the model needs from a command, not every
//! byte it printed. Ideas from rtk (https://github.com/rtk-ai/rtk); no code
//! copied. Lossy by design, so the full output is kept on the node and the
//! model is told how to get it (`tool_output`).
//!
//! Rules, in order: strip terminal colour codes; keep only the last state of
//! lines redrawn with `\r` (progress bars); drop build/download noise lines;
//! for test runners, replace passing-test lines by a count (failures, panics
//! and summaries stay); collapse runs of identical lines.

/// Lines that only report progress, for common tools.
const NOISE_PREFIXES: &[&str] = &[
    "Compiling ",
    "Checking ",
    "Downloaded ",
    "Downloading ",
    "Blocking waiting for file lock",
    "Updating crates.io index",
    "Locking ",
    "Fresh ",
    "Documenting ",
    "Collecting ",
    "Using cached ",
    "Requirement already satisfied",
    "npm http fetch",
    "Progress: resolved",
];

/// The filtered text and how many lines were dropped or folded.
#[derive(Debug, PartialEq)]
pub struct Filtered {
    pub text: String,
    pub removed_lines: usize,
}

pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match it.peek() {
            // CSI: ESC [ params final-byte(0x40..=0x7e)
            Some('[') => {
                it.next();
                for d in it.by_ref() {
                    if ('@'..='~').contains(&d) {
                        break;
                    }
                }
            }
            // OSC: ESC ] ... BEL or ESC \
            Some(']') => {
                it.next();
                while let Some(d) = it.next() {
                    if d == '\u{7}' {
                        break;
                    }
                    if d == '\u{1b}' && it.peek() == Some(&'\\') {
                        it.next();
                        break;
                    }
                }
            }
            _ => {
                it.next();
            }
        }
    }
    out
}

fn is_noise(line: &str) -> bool {
    let t = line.trim_start();
    NOISE_PREFIXES.iter().any(|p| t.starts_with(p))
}

/// A passing test line from cargo, pytest -v, go test -v, jest/vitest.
fn is_passing_test(line: &str) -> bool {
    let t = line.trim();
    t.ends_with(" ... ok")
        || t.ends_with(" PASSED")
        || t.contains(" PASSED [")
        || (t.starts_with("--- PASS: ")
            || t.starts_with("=== RUN ")
            || t.starts_with("=== PAUSE ")
            || t.starts_with("=== CONT "))
        || t.starts_with("✓ ")
        || t.starts_with("√ ")
        || t.starts_with("✔ ")
}

pub fn filter(raw: &str) -> Filtered {
    let clean = strip_ansi(raw);
    let mut lines: Vec<String> = Vec::new();
    let mut removed = 0usize;
    let mut passed = 0usize;
    for l in clean.split('\n') {
        // A progress bar redraws with \r: keep what was shown last.
        let l = l.trim_end_matches('\r');
        let l = l.rsplit('\r').next().unwrap_or(l);
        if is_noise(l) {
            removed += 1;
            continue;
        }
        if is_passing_test(l) {
            passed += 1;
            removed += 1;
            continue;
        }
        lines.push(l.to_string());
    }
    // Collapse runs of identical lines.
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let mut j = i + 1;
        while j < lines.len() && lines[j] == lines[i] {
            j += 1;
        }
        let n = j - i;
        if n > 2 && !lines[i].trim().is_empty() {
            out.push(format!("{} [repeated {n} times]", lines[i]));
            removed += n - 1;
        } else {
            out.extend(lines[i..j].iter().cloned());
        }
        i = j;
    }
    // Blank-line runs too.
    let mut text = String::new();
    let mut blank = 0;
    for l in out {
        if l.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                removed += 1;
                continue;
            }
        } else {
            blank = 0;
        }
        text.push_str(&l);
        text.push('\n');
    }
    let mut text = text.trim_end().to_string();
    if passed > 0 {
        text.push_str(&format!("\n[{passed} passing test lines not shown]"));
    }
    Filtered {
        text,
        removed_lines: removed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_and_progress_bars() {
        assert_eq!(strip_ansi("\u{1b}[1;31merror\u{1b}[0m: x"), "error: x");
        assert_eq!(strip_ansi("\u{1b}]0;title\u{7}ok"), "ok");
        let f = filter("10%\r50%\r100% done\nnext");
        assert_eq!(f.text, "100% done\nnext");
    }

    #[test]
    fn cargo_test_keeps_failures_and_summary() {
        let raw = "   Compiling foo v0.1.0 (/x)\n    Finished `test` profile\n     Running unittests\n\nrunning 3 tests\ntest a ... ok\ntest b ... ok\ntest c ... FAILED\n\nfailures:\n\n---- c stdout ----\nthread 'c' panicked at src/lib.rs:9:5:\nassertion failed: 1 == 2\n\ntest result: FAILED. 2 passed; 1 failed\n";
        let f = filter(raw);
        assert!(!f.text.contains("Compiling"));
        assert!(!f.text.contains("test a ... ok"));
        for keep in [
            "test c ... FAILED",
            "panicked at src/lib.rs:9:5",
            "assertion failed: 1 == 2",
            "test result: FAILED. 2 passed; 1 failed",
            "Finished",
        ] {
            assert!(f.text.contains(keep), "lost {keep:?}:\n{}", f.text);
        }
        assert!(f.text.ends_with("[2 passing test lines not shown]"));
        assert_eq!(f.removed_lines, 3);
    }

    #[test]
    fn pytest_and_repeats() {
        let raw = "tests/t.py::test_a PASSED [ 50%]\ntests/t.py::test_b FAILED [100%]\nE   AssertionError\nwarn\nwarn\nwarn\nwarn\n";
        let f = filter(raw);
        assert!(f.text.contains("test_b FAILED") && f.text.contains("AssertionError"));
        assert!(!f.text.contains("test_a"));
        assert!(f.text.contains("warn [repeated 4 times]"));
    }

    #[test]
    fn plain_output_is_untouched() {
        let raw = "a\nb\n\nc";
        let f = filter(raw);
        assert_eq!(f.text, raw);
        assert_eq!(f.removed_lines, 0);
    }
}
