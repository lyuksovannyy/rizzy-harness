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

/// The most a terminal string (OSC, DCS, APC, PM, SOS), an open CSI or an
/// open escape hides: one never closed ends at a newline or after this many
/// characters, so a command cannot blank what it prints after it.
pub const SEQUENCE_MAX: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Text,
    /// After ESC.
    Esc,
    /// After ESC and some intermediate bytes (0x20..=0x2f): a final byte follows.
    EscInter(usize),
    /// A CSI's parameters: the final byte is 0x40..=0x7e.
    Csi(usize),
    /// A string to its terminator: BEL (OSC only), ST (`ESC \` or U+009C).
    Str {
        bel_ends: bool,
        n: usize,
    },
}

/// Removes terminal escape sequences from text that arrives in pieces: CSI,
/// OSC, DCS, APC, PM and SOS strings, other escapes (with their
/// intermediate bytes) and the C1 controls (U+0080..=U+009F, whose U+009B
/// opens a CSI and U+009D an OSC). The state is kept between pieces, so a
/// sequence cut by a read is removed whole and a secret it split is whole
/// again for the scrubber (`grep --color=always` puts a colour code inside
/// the match). Nothing is buffered: each piece comes out at once. Bounded:
/// a sequence that is never closed ends at a newline, at a control
/// character that cannot be part of it, or after [`SEQUENCE_MAX`]
/// characters, and what ends it is kept as text. Text with no escape in it
/// comes out unchanged, and `\r` and `\n` are never taken.
#[derive(Debug)]
pub struct AnsiStripper {
    state: State,
}

impl Default for AnsiStripper {
    fn default() -> Self {
        Self::new()
    }
}

fn is_c1(c: char) -> bool {
    ('\u{80}'..='\u{9f}').contains(&c)
}

impl AnsiStripper {
    pub fn new() -> Self {
        Self { state: State::Text }
    }

    /// The next piece, without its escape sequences.
    pub fn push(&mut self, s: &str) -> String {
        if self.state == State::Text && !s.contains(|c| c == '\u{1b}' || is_c1(c)) {
            return s.to_string();
        }
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            self.step(c, &mut out);
        }
        out
    }

    fn step(&mut self, c: char, out: &mut String) {
        use State::*;
        match self.state {
            Text => match c {
                '\u{1b}' => self.state = Esc,
                '\u{9b}' => self.state = Csi(0),
                '\u{9d}' => {
                    self.state = Str {
                        bel_ends: true,
                        n: 0,
                    }
                }
                '\u{90}' | '\u{98}' | '\u{9e}' | '\u{9f}' => {
                    self.state = Str {
                        bel_ends: false,
                        n: 0,
                    }
                }
                c if is_c1(c) => {}
                c => out.push(c),
            },
            Esc => match c {
                '[' => self.state = Csi(0),
                ']' => {
                    self.state = Str {
                        bel_ends: true,
                        n: 0,
                    }
                }
                'P' | '_' | '^' | 'X' => {
                    self.state = Str {
                        bel_ends: false,
                        n: 0,
                    }
                }
                '\u{1b}' => {}
                '\u{20}'..='\u{2f}' => self.state = EscInter(1),
                '\n' | '\r' => {
                    self.state = Text;
                    out.push(c);
                }
                _ => self.state = Text,
            },
            EscInter(n) => match c {
                '\u{20}'..='\u{2f}' if n < SEQUENCE_MAX => self.state = EscInter(n + 1),
                '\u{30}'..='\u{7e}' => self.state = Text,
                // Not a final byte: the escape ends, the character is text.
                _ => {
                    self.state = Text;
                    self.step(c, out);
                }
            },
            Csi(n) => match c {
                '\u{40}'..='\u{7e}' => self.state = Text,
                '\u{20}'..='\u{3f}' if n < SEQUENCE_MAX => self.state = Csi(n + 1),
                _ => {
                    self.state = Text;
                    self.step(c, out);
                }
            },
            Str { bel_ends, n } => match c {
                '\u{1b}' => self.state = Esc,
                '\u{9c}' => self.state = Text,
                '\u{7}' if bel_ends => self.state = Text,
                '\n' => {
                    self.state = Text;
                    out.push(c);
                }
                _ if n >= SEQUENCE_MAX => {
                    self.state = Text;
                    self.step(c, out);
                }
                _ => self.state = Str { bel_ends, n: n + 1 },
            },
        }
    }
}

/// `s` without escape sequences, as one piece ([`AnsiStripper`]).
pub fn strip_ansi(s: &str) -> String {
    AnsiStripper::new().push(s)
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
    fn sequences_of_every_kind_come_out() {
        for (raw, want) in [
            ("a\u{1b}[0mb", "ab"),
            ("a\u{1b}[01;31m\u{1b}[Kb", "ab"),
            ("a\u{1b}]0;t\u{7}b", "ab"),
            ("a\u{1b}]8;;http://x\u{1b}\\b", "ab"),
            ("a\u{1b}Pq\u{1b}\\b", "ab"),
            ("a\u{1b}_x\u{1b}\\b", "ab"),
            ("a\u{1b}(Bb", "ab"),
            ("a\u{1b}#8b", "ab"),
            ("a\u{1b}Zb\n", "ab\n"),
            // SOS is a string: it ends at the newline.
            ("a\u{1b}Xb\nc", "a\nc"),
            ("a\u{9b}0mb", "ab"),
            ("a\u{9d}0;t\u{7}b", "ab"),
            ("a\u{9d}0;t\u{9c}b", "ab"),
            ("a\u{85}b", "ab"),
            // A newline is never taken; an open sequence ends at it.
            ("a\u{1b}]0;title\nb", "a\nb"),
            ("a\u{1b}[1;2;3\nb", "a\nb"),
            ("a\u{1b}\nb", "a\nb"),
            // A control character that cannot be part of a CSI ends it.
            ("a\u{1b}[1\u{7}b", "a\u{7}b"),
            // A new escape cancels an open string.
            ("a\u{1b}]0;t\u{1b}[0mb", "ab"),
            // \r and plain text are not touched.
            ("10%\r50%\r100%\r\n", "10%\r50%\r100%\r\n"),
            ("é€😀 plain", "é€😀 plain"),
        ] {
            assert_eq!(strip_ansi(raw), want, "{raw:?}");
        }
    }

    #[test]
    fn a_sequence_that_never_ends_hides_at_most_a_bounded_run() {
        for open in ["\u{1b}]", "\u{1b}P", "\u{9d}", "\u{1b}[", "\u{1b}("] {
            let fill = if open == "\u{1b}(" { " " } else { "1" };
            let raw = format!("{open}{}TAIL", fill.repeat(SEQUENCE_MAX * 2));
            let t = strip_ansi(&raw);
            assert!(
                t.ends_with("TAIL") && t.len() <= SEQUENCE_MAX + 4 + 1,
                "{open:?}: {}",
                t.len()
            );
        }
    }

    /// Small deterministic generator (no new dependency).
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    const SEQS: &[&str] = &[
        "\u{1b}[0m",
        "\u{1b}[01;31m\u{1b}[K",
        "\u{1b}]0;t\u{7}",
        "\u{1b}]8;;http://x\u{1b}\\",
        "\u{1b}Pq\u{1b}\\",
        "\u{1b}(B",
        "\u{9b}1m",
        "\u{9d}0;t\u{9c}",
        // From here on, open at the end.
        "\u{1b}X",
        "\u{1b}",
        "\u{1b}[",
        "\u{1b}]",
        "\u{9b}",
    ];

    /// The sequences before the open ones in [`SEQS`].
    const CLOSED: usize = 8;

    /// Cut `s` at random character boundaries.
    fn pieces<'a>(s: &'a str, r: &mut Rng) -> Vec<&'a str> {
        let cuts: Vec<usize> = s.char_indices().map(|(i, _)| i).chain([s.len()]).collect();
        let mut at = 0;
        let mut out = vec![];
        while at < s.len() {
            let to = cuts[cuts.iter().position(|&c| c > at).unwrap()..]
                .iter()
                .copied()
                .nth(r.below(6))
                .unwrap_or(s.len());
            out.push(&s[at..to]);
            at = to;
        }
        out
    }

    /// Random text with escape sequences put in at random places (inside
    /// the secret too), cut at random places (an escape cut in two too):
    /// the pieces put through one stripper are the whole through another,
    /// and neither holds the secret once scrubbed, nor leaves its second
    /// half. With no escape in the text, nothing changes.
    #[test]
    fn property_a_secret_with_escapes_inside_never_survives_the_strip_and_scrub() {
        use harness_core::secret::StreamScrubber;
        use harness_core::{Secret, SecretSet};
        const KEY: &str = "sk-test-0123456789abcdef";
        let set = SecretSet::new(vec![Secret::new(KEY.into())]);
        let mut r = Rng(0x9e37_79b9_7f4a_7c15);
        for case in 0..3000 {
            let mut text = String::new();
            for _ in 0..r.below(6) {
                match r.below(4) {
                    0 => text.push_str("word é€ \n"),
                    1 => {
                        text.push_str(SEQS[r.below(SEQS.len())]);
                        // An open sequence ends at a newline: it hides text
                        // after it on its line (any command may print that).
                        text.push('\n');
                    }
                    2 => text.push_str("10%\r50%\r"),
                    _ => {
                        for (i, ch) in KEY.chars().enumerate() {
                            if i > 0 && r.below(4) == 0 {
                                // Only whole sequences: an open one inside a
                                // key hides a part of it, as `cut` would.
                                text.push_str(SEQS[r.below(CLOSED)]);
                            }
                            text.push(ch);
                        }
                        text.push('\n');
                    }
                }
            }
            let whole = strip_ansi(&text);
            let mut st = AnsiStripper::new();
            let mut parts = String::new();
            let mut scrubbed = StreamScrubber::new(set.clone());
            let mut streamed = String::new();
            for p in pieces(&text, &mut r) {
                let c = st.push(p);
                parts.push_str(&c);
                streamed.push_str(&scrubbed.push(&c));
            }
            streamed.push_str(&scrubbed.finish());
            assert_eq!(parts, whole, "case {case}: {text:?}");
            let after = set.scrub(&whole);
            for shown in [&after, &streamed] {
                assert!(
                    !shown.contains(KEY) && !shown.contains("456789abcdef"),
                    "case {case}: {text:?} -> {shown:?}"
                );
            }
            // An open escape at the end of the text is not a leak either.
            assert!(!whole.contains('\u{1b}'), "case {case}");
            if !text.contains('\u{1b}') && !text.contains(|c| ('\u{80}'..='\u{9f}').contains(&c)) {
                assert_eq!(whole, text, "case {case}");
            }
        }
    }

    /// A flood: linear, and no text is held back.
    #[test]
    fn a_flood_is_stripped_in_one_pass_with_nothing_held() {
        let mut st = AnsiStripper::new();
        let piece = "x\u{1b}[31my\u{1b}]0;t\u{7}z".repeat(2000);
        let started = std::time::Instant::now();
        let mut n = 0;
        for _ in 0..200 {
            n += st.push(&piece).len();
        }
        assert_eq!(n, 200 * 2000 * 3);
        assert!(started.elapsed().as_secs() < 20);
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
