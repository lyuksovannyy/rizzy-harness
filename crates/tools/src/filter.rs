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

/// The most characters of a terminal string (OSC, DCS, APC, PM, SOS) the
/// stripper holds while it waits for its end, and the longest an open CSI
/// or escape is followed. A string that is not closed within it, or before
/// a line break, is not a sequence: its payload is shown as text.
pub const SEQUENCE_MAX: usize = 4096;

/// Per line, the most characters removed inside terminated string sequences
/// without a trace; past it each removal leaves a marker
/// ([`AnsiStripper`]).
pub const SILENT_PER_LINE: usize = 256;

/// The longest run of parameter characters a C1 CSI (`U+009B`) takes.
const C1_CSI_MAX: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Text,
    /// After ESC.
    Esc,
    /// After ESC and some intermediate bytes (0x20..=0x2f): a final byte follows.
    EscInter(usize),
    /// A CSI's parameters: the final byte is 0x40..=0x7e.
    Csi(usize),
    /// After `U+009B`: parameters (0x30..=0x3f, held) and a final byte; with
    /// none of them, or anything else, only the one character was dropped.
    C1Csi,
    /// A string's payload (held in `buf`): ends at BEL (OSC only) or ST.
    Str {
        bel_ends: bool,
    },
    /// After an ESC inside a string: `\` closes it, anything else means it
    /// never was closed.
    StrEsc {
        bel_ends: bool,
    },
}

/// Removes terminal escape sequences from text that arrives in pieces, for
/// command output that a scrubber reads next: a colour code inside a secret
/// (`grep --color=always`) must not keep it from being found. What it takes
/// out:
/// - CSI (`ESC [`), other escapes with their intermediate bytes, and
///   `U+009B` followed by parameters and a final byte;
/// - OSC, DCS, APC, PM and SOS strings that are closed (BEL for OSC, or ST,
///   `ESC \`).
///
/// What it does not take: a string that is never closed. It ends at a `\n`
/// or `\r` (which stay) or after [`SEQUENCE_MAX`] characters, and then only
/// its introducer is dropped: the payload comes out as text, so a command or
/// a file cannot make its own text vanish by opening a string. For that the
/// payload is held, **at most [`SEQUENCE_MAX`] characters per stream** while
/// a string is open, and [`Self::finish`] releases it at the end of the
/// stream. Removed payloads are counted per line (the count restarts at
/// each `\n`): the first [`SILENT_PER_LINE`] characters go without a trace,
/// then each removal, the one that crosses the bound included, leaves
/// `[N characters of an escape sequence not shown]`.
///
/// The other C1 code points (`U+0080..=U+009F`) are dropped one at a time
/// and open nothing (a UTF-8 terminal does not act on them). A CSI, an open
/// escape and a C1 CSI are bounded (4096 and 32 characters): what ends one
/// that is not a final byte (a line break, a control character) is text.
/// Text with no escape in it and no C1 code point comes out unchanged and
/// at once; state is kept between pieces, so a sequence cut by a read is
/// still one.
#[derive(Debug)]
pub struct AnsiStripper {
    state: State,
    /// The open string's payload (or the C1 CSI's parameters).
    buf: String,
    buf_chars: usize,
    /// Characters removed inside terminated strings on this line.
    removed: usize,
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
        Self {
            state: State::Text,
            buf: String::new(),
            buf_chars: 0,
            removed: 0,
        }
    }

    /// Characters held now (never more than [`SEQUENCE_MAX`]).
    pub fn held(&self) -> usize {
        self.buf_chars
    }

    /// The next piece, without its escape sequences.
    pub fn push(&mut self, s: &str) -> String {
        if self.state == State::Text && !s.contains(|c| c == '\u{1b}' || is_c1(c)) {
            if s.contains('\n') {
                self.removed = 0;
            }
            return s.to_string();
        }
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            self.step(c, &mut out);
        }
        out
    }

    /// The stream ended: what an open string, or C1 CSI, still held (it was
    /// never closed, so it is text).
    pub fn finish(&mut self) -> String {
        let mut out = String::new();
        if matches!(
            self.state,
            State::Str { .. } | State::StrEsc { .. } | State::C1Csi
        ) {
            self.flush(&mut out);
        }
        self.state = State::Text;
        out
    }

    fn flush(&mut self, out: &mut String) {
        out.push_str(&self.buf);
        self.buf.clear();
        self.buf_chars = 0;
    }

    fn open_string(&mut self, bel_ends: bool) {
        self.buf.clear();
        self.buf_chars = 0;
        self.state = State::Str { bel_ends };
    }

    /// The string was closed: its payload goes, and is counted.
    fn closed(&mut self, out: &mut String) {
        let n = self.buf_chars;
        self.buf.clear();
        self.buf_chars = 0;
        self.state = State::Text;
        self.removed += n;
        if self.removed > SILENT_PER_LINE {
            out.push_str(&format!("[{n} characters of an escape sequence not shown]"));
        }
    }

    fn emit(&mut self, c: char, out: &mut String) {
        if c == '\n' {
            self.removed = 0;
        }
        out.push(c);
    }

    fn step(&mut self, c: char, out: &mut String) {
        use State::*;
        match self.state {
            Text => match c {
                '\u{1b}' => self.state = Esc,
                '\u{9b}' => {
                    self.buf.clear();
                    self.buf_chars = 0;
                    self.state = C1Csi;
                }
                c if is_c1(c) => {}
                c => self.emit(c, out),
            },
            Esc => match c {
                '[' => self.state = Csi(0),
                ']' => self.open_string(true),
                'P' | '_' | '^' | 'X' => self.open_string(false),
                '\u{1b}' => {}
                '\u{20}'..='\u{2f}' => self.state = EscInter(1),
                '\n' | '\r' => {
                    self.state = Text;
                    self.emit(c, out);
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
            C1Csi => match c {
                '\u{30}'..='\u{3f}' if self.buf_chars < C1_CSI_MAX => {
                    self.buf.push(c);
                    self.buf_chars += 1;
                }
                '\u{40}'..='\u{7e}' if self.buf_chars > 0 => {
                    self.buf.clear();
                    self.buf_chars = 0;
                    self.state = Text;
                }
                _ => {
                    self.flush(out);
                    self.state = Text;
                    self.step(c, out);
                }
            },
            Str { bel_ends } => match c {
                '\u{1b}' => self.state = StrEsc { bel_ends },
                '\u{7}' if bel_ends => self.closed(out),
                // Never closed: the payload is text, the break stays.
                '\n' | '\r' => {
                    self.flush(out);
                    self.state = Text;
                    self.emit(c, out);
                }
                _ if self.buf_chars >= SEQUENCE_MAX => {
                    self.flush(out);
                    self.state = Text;
                    self.step(c, out);
                }
                c => {
                    self.buf.push(c);
                    self.buf_chars += 1;
                }
            },
            StrEsc { .. } if c == '\\' => self.closed(out),
            StrEsc { .. } => {
                // The ESC was no terminator: the string never was closed,
                // and that ESC begins a new escape.
                self.flush(out);
                self.state = Esc;
                self.step(c, out);
            }
        }
    }
}

/// `s` without escape sequences, as one piece ([`AnsiStripper`]).
pub fn strip_ansi(s: &str) -> String {
    let mut st = AnsiStripper::new();
    let mut out = st.push(s);
    out.push_str(&st.finish());
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
            ("a\u{9b}0mb", "ab"),
            // The other C1 code points: dropped alone, they open nothing.
            ("a\u{9d}0;t\u{7}b", "a0;t\u{7}b"),
            ("a\u{85}b", "ab"),
            // A string never closed is text, from its payload on.
            ("a\u{1b}]0;title\nb", "a0;title\nb"),
            ("a\u{1b}Xb\nc", "abnc".replace("bn", "b\n").as_str()),
            ("a\u{1b}]0;t\u{1b}[0mb", "a0;tb"),
            ("a\u{1b}]0;t", "a0;t"),
            // An open CSI or escape ends at a line break, which stays.
            ("a\u{1b}[1;2;3\nb", "a\nb"),
            ("a\u{1b}\nb", "a\nb"),
            // A control character that cannot be part of a CSI ends it.
            ("a\u{1b}[1\u{7}b", "a\u{7}b"),
            // \r and plain text are not touched.
            ("10%\r50%\r100%\r\n", "10%\r50%\r100%\r\n"),
            ("é€😀 plain", "é€😀 plain"),
        ] {
            assert_eq!(strip_ansi(raw), want, "{raw:?}");
        }
    }

    #[test]
    fn a_sequence_that_never_ends_hides_nothing_and_holds_a_bounded_run() {
        for open in ["\u{1b}]", "\u{1b}P", "\u{1b}_", "\u{1b}^", "\u{1b}X"] {
            let payload = "1".repeat(SEQUENCE_MAX * 2);
            let raw = format!("{open}{payload}TAIL");
            let mut st = AnsiStripper::new();
            let mut got = String::new();
            for p in raw.as_bytes().chunks(500) {
                got.push_str(&st.push(std::str::from_utf8(p).unwrap()));
                assert!(st.held() <= SEQUENCE_MAX);
            }
            got.push_str(&st.finish());
            assert_eq!(got, format!("{payload}TAIL"), "{open:?}");
        }
        // An escape or a CSI without a final byte hides at most a bounded run.
        for (open, fill) in [("\u{1b}[", "1"), ("\u{1b}(", " ")] {
            let raw = format!("{open}{}TAIL", fill.repeat(SEQUENCE_MAX * 2));
            let t = strip_ansi(&raw);
            assert!(
                t.ends_with("TAIL") && t.len() <= SEQUENCE_MAX + 5,
                "{open:?}: {}",
                t.len()
            );
        }
    }

    #[test]
    fn removed_payloads_are_counted_per_line() {
        let osc = |n: usize| format!("\u{1b}]8;;{}\u{7}", "u".repeat(n - 3));
        assert_eq!(strip_ansi(&format!("a{}b", osc(256))), "ab");
        assert_eq!(
            strip_ansi(&format!("a{}b", osc(257))),
            "a[257 characters of an escape sequence not shown]b"
        );
        assert_eq!(
            strip_ansi(&format!("{}{}\n{}", osc(200), osc(100), osc(100))),
            "[100 characters of an escape sequence not shown]\n"
        );
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

    /// Sequences that close, as (text, payload characters).
    const CLOSED: &[&str] = &[
        "\u{1b}[0m",
        "\u{1b}[01;31m\u{1b}[K",
        "\u{1b}(B",
        "\u{1b}#8",
        "\u{1b}7",
        "\u{9b}1;31m",
        "\u{85}",
    ];

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

    fn payload(r: &mut Rng, max: usize) -> String {
        (0..r.below(max + 1))
            .map(|_| ["a", "7", "é", "€", " ", ";"][r.below(6)])
            .collect()
    }

    /// Random text with sequences put in at random places (inside the
    /// secret too), cut at random places (a sequence cut in two too):
    /// - the pieces put through one stripper are the whole through another;
    /// - a key split only by closed sequences, or by terminated strings
    ///   under the per-line bound, is gone once scrubbed (and its tail);
    /// - no character outside an escape is ever lost: the output is exactly
    ///   what the generator says is visible (an unterminated string's
    ///   payload is visible, a terminated one's is not);
    /// - the stripper never holds more than its bound;
    /// - with no escape in the text, nothing changes.
    #[test]
    fn property_nothing_visible_is_lost_and_a_key_never_survives() {
        use harness_core::secret::StreamScrubber;
        use harness_core::{Secret, SecretSet};
        const KEY: &str = "sk-test-0123456789abcdef";
        let set = SecretSet::new(vec![Secret::new(KEY.into())]);
        let mut r = Rng(0x9e37_79b9_7f4a_7c15);
        for case in 0..4000 {
            let (mut input, mut visible) = (String::new(), String::new());
            let mut removed = 0usize;
            let line_end = |removed: &mut usize| *removed = 0;
            for _ in 0..r.below(8) {
                match r.below(6) {
                    0 => {
                        input.push_str("word é€ \n");
                        visible.push_str("word é€ \n");
                        line_end(&mut removed);
                    }
                    1 => input.push_str(CLOSED[r.below(CLOSED.len())]),
                    2 => {
                        input.push_str("10%\r50%\r");
                        visible.push_str("10%\r50%\r");
                    }
                    // A terminated string, kept under the per-line bound.
                    3 => {
                        let p = payload(&mut r, 120);
                        let n = p.chars().count();
                        if removed + n <= SILENT_PER_LINE {
                            removed += n;
                            let (open, end) = match r.below(4) {
                                0 => ("\u{1b}]", "\u{7}"),
                                1 => ("\u{1b}]", "\u{1b}\\"),
                                2 => ("\u{1b}P", "\u{1b}\\"),
                                _ => ("\u{1b}_", "\u{1b}\\"),
                            };
                            input.push_str(&format!("{open}{p}{end}"));
                        }
                    }
                    // An unterminated string: its payload is text, to a break.
                    4 => {
                        let p = payload(&mut r, 5000);
                        let open =
                            ["\u{1b}]", "\u{1b}P", "\u{1b}_", "\u{1b}^", "\u{1b}X"][r.below(5)];
                        let brk = ["\n", "\r"][r.below(2)];
                        input.push_str(&format!("{open}{p}{brk}"));
                        visible.push_str(&format!("{p}{brk}"));
                        if brk == "\n" {
                            line_end(&mut removed);
                        }
                    }
                    // The key, split by closed sequences and small strings.
                    _ => {
                        let mut line_removed = 0;
                        for (i, ch) in KEY.chars().enumerate() {
                            if i > 0 && r.below(4) == 0 {
                                if r.below(3) == 0 {
                                    let p = payload(&mut r, 8);
                                    let n = p.chars().count();
                                    if removed + line_removed + n <= SILENT_PER_LINE {
                                        line_removed += n;
                                        input.push_str(&format!("\u{1b}]{p}\u{7}"));
                                    }
                                } else {
                                    input.push_str(CLOSED[r.below(CLOSED.len())]);
                                }
                            }
                            input.push(ch);
                            visible.push(ch);
                        }
                        removed += line_removed;
                        input.push('\n');
                        visible.push('\n');
                        line_end(&mut removed);
                    }
                }
            }
            // The key appears whole in `input` only through sequences.
            let mut st = AnsiStripper::new();
            let mut got = String::new();
            let mut scrubbed = StreamScrubber::new(set.clone());
            let mut streamed = String::new();
            for p in pieces(&input, &mut r) {
                let c = st.push(p);
                assert!(st.held() <= SEQUENCE_MAX, "case {case}");
                got.push_str(&c);
                streamed.push_str(&scrubbed.push(&c));
            }
            let tail = st.finish();
            got.push_str(&tail);
            streamed.push_str(&scrubbed.push(&tail));
            streamed.push_str(&scrubbed.finish());
            assert_eq!(strip_ansi(&input), got, "case {case}: {input:?}");
            for shown in [set.scrub(&got), streamed] {
                assert!(
                    !shown.contains(KEY) && !shown.contains("456789abcdef"),
                    "case {case}: {input:?} -> {shown:?}"
                );
            }
            assert!(!got.contains('\u{1b}'), "case {case}");
            let want: String = visible.chars().filter(|c| *c != '\u{85}').collect();
            assert_eq!(got, want, "case {case}: {input:?}");
            if !input.contains('\u{1b}') && !input.contains(|c| ('\u{80}'..='\u{9f}').contains(&c))
            {
                assert_eq!(got, input, "case {case}");
            }
        }
    }

    /// A flood: linear, and nothing is held back while no string is open.
    #[test]
    fn a_flood_is_stripped_in_one_pass_with_nothing_held() {
        let mut st = AnsiStripper::new();
        let piece = "x\u{1b}[31my\u{1b}]0;t\u{7}z".repeat(2000);
        let started = std::time::Instant::now();
        let mut n = 0;
        for _ in 0..200 {
            n += st.push(&piece).len();
            assert_eq!(st.held(), 0);
        }
        // x, y and z stay; the OSC payloads (3 characters each) are
        // removed, and after 256 a line (the piece has no newline) each
        // leaves a marker.
        assert!(n > 200 * 2000 * 3);
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
