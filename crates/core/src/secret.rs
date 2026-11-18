use std::fmt;
use std::sync::{Arc, RwLock};

pub mod schema;

/// A value that must never appear in logs. `Debug` and `Display` are redacted.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secret<T>(T);

impl<T> Secret<T> {
    pub fn new(value: T) -> Self {
        Self(value)
    }

    /// Explicit access; grep for `expose` to audit secret use.
    pub fn expose(&self) -> &T {
        &self.0
    }
}

impl Secret<String> {
    /// An empty key, or one of spaces only: no key at all. A provider
    /// request sends no key header for it (a router that refuses unknown
    /// keys refuses `Bearer ` as well).
    pub fn is_blank(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

impl<T> fmt::Display for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Secrets shared by every scrubber in the process. Adding one (e.g. an API
/// key typed into onboarding) takes effect everywhere immediately.
#[derive(Clone, Default)]
pub struct SecretSet(Arc<RwLock<Vec<Secret<String>>>>);

impl SecretSet {
    pub fn new(secrets: Vec<Secret<String>>) -> Self {
        Self(Arc::new(RwLock::new(secrets)))
    }

    pub fn add(&self, s: Secret<String>) {
        let mut v = self.0.write().unwrap_or_else(|p| p.into_inner());
        if !v.iter().any(|x| x.expose() == s.expose()) {
            v.push(s);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.read().unwrap_or_else(|p| p.into_inner()).is_empty()
    }

    pub fn scrub(&self, text: &str) -> String {
        scrub(text, &self.0.read().unwrap_or_else(|p| p.into_inner()))
    }

    /// Scrub every string (and object key) of a JSON value in place. This is
    /// the way to scrub structured data: scrubbing the serialized text would
    /// miss a secret holding `"` or `\` (escaped there) and could break the
    /// JSON with the replacement.
    pub fn scrub_value(&self, v: &mut serde_json::Value) {
        use serde_json::Value;
        match v {
            Value::String(s) => {
                let clean = self.scrub(s);
                if clean != *s {
                    *s = clean;
                }
            }
            Value::Array(a) => a.iter_mut().for_each(|x| self.scrub_value(x)),
            Value::Object(m) => {
                let keys: Vec<String> =
                    m.keys().filter(|k| self.scrub(k) != **k).cloned().collect();
                for k in keys {
                    if let Some(mut val) = m.remove(&k) {
                        self.scrub_value(&mut val);
                        m.insert(self.scrub(&k), val);
                    }
                }
                m.values_mut().for_each(|x| self.scrub_value(x));
            }
            _ => {}
        }
    }
}

/// Secrets shorter than this are not redacted (too many false hits).
const MIN_SECRET_LEN: usize = 8;
/// Upper bound on what a [`StreamScrubber`] holds. Keys are far shorter; a
/// longer secret is still scrubbed whole but may leak if split by a flush.
pub const MAX_HOLD_BACK: usize = 4096;
/// Secrets at least this long are "long": a stream holds text back only
/// where it could begin one, judged on the first [`SEEN_PREFIX`] bytes.
/// Shorter ones keep a constant hold of `longest short − 1` (at most 30)
/// bytes: a few bytes of one would be a good part of it.
pub const LONG_SECRET_LEN: usize = 32;
/// How much of a long secret's start decides what a stream holds back
/// (PLAN.md T75). Where the stream cuts is seen by whoever watches it, and
/// whoever writes into it (a prompt, a command's output) chooses the text:
/// a hold that followed the whole secret would answer "does a secret start
/// with this?" byte by byte. Judged on these bytes and the longest long
/// secret's length only, the cut tells at most the first 4 bytes of each
/// long secret, whatever is written and however often (often only a
/// public prefix such as `sk-a`); everything else it depends on is whether
/// a whole secret was written, which every scrubber tells by replacing it.
pub const SEEN_PREFIX: usize = 4;

/// Scrubs a stream that arrives in pieces; what it returns, put together,
/// is [`scrub`] of the whole stream, so a secret split across pieces is
/// replaced as if it had come in one. It holds back, unscrubbed, the end of
/// the text from the first place a secret could still begin:
/// - any of the last `longest short secret − 1` bytes (a constant hold);
/// - a place within the last `longest long secret − 1` bytes where the text
///   agrees with a long secret's first [`SEEN_PREFIX`] bytes (or with as
///   many of them as the text has left).
///
/// Other text comes out of the `push` that brought it. The held bytes are
/// bounded by [`MAX_HOLD_BACK`]; the hold follows the secret set as it
/// grows (a secret added mid-stream is caught from the next piece on).
pub struct StreamScrubber {
    secrets: SecretSet,
    /// Raw text not yet released: never scrubbed twice, so a replacement
    /// marker never joins later text into a match.
    held: String,
}

impl StreamScrubber {
    pub fn new(secrets: SecretSet) -> Self {
        Self {
            secrets,
            held: String::new(),
        }
    }

    /// Add a piece; returns the text that is now safe to emit (possibly empty).
    pub fn push(&mut self, piece: &str) -> String {
        self.held.push_str(piece);
        let set = self.secrets.0.read().unwrap_or_else(|p| p.into_inner());
        let live = live(&set);
        let stop = release_point(&self.held, &live);
        let (out, end) = scan(&self.held, &live, stop);
        drop(set);
        self.held.drain(..end);
        out
    }

    /// The stream ended (or was cancelled or cut): release what is held.
    pub fn finish(&mut self) -> String {
        let held = std::mem::take(&mut self.held);
        self.secrets.scrub(&held)
    }
}

/// What a secret is replaced with.
pub const MARKER: &str = "[REDACTED]";

/// `value` is part of [`MARKER`] (and long enough to count): it cannot be a
/// secret, and redacting it would rewrite every earlier redaction, so a
/// second pass over scrubbed text would change it again. Never redacted;
/// a node names such a configured value once at start.
pub fn is_marker_part(value: &str) -> bool {
    value.len() >= MIN_SECRET_LEN && MARKER.contains(value)
}

/// The secrets that are redacted, as bytes.
fn live(secrets: &[Secret<String>]) -> Vec<&[u8]> {
    secrets
        .iter()
        .map(|s| s.expose().as_str())
        .filter(|v| v.len() >= MIN_SECRET_LEN && !is_marker_part(v))
        .map(str::as_bytes)
        .collect()
}

/// Where `text` may still hold the start of a secret that continues in a
/// later piece: everything before it can be scrubbed for good. Depends on
/// the secrets only through the longest short and long lengths and each
/// long secret's first [`SEEN_PREFIX`] bytes (T75). Work: at most
/// `held + piece` positions, each against every long secret's first bytes.
fn release_point(text: &str, live: &[&[u8]]) -> usize {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let short = live
        .iter()
        .map(|s| s.len())
        .filter(|&n| n < LONG_SECRET_LEN)
        .max();
    let mut at = len - short.map_or(0, |n| n - 1).min(len);
    let long: Vec<&[u8]> = live
        .iter()
        .copied()
        .filter(|s| s.len() >= LONG_SECRET_LEN)
        .collect();
    if let Some(max) = long.iter().map(|s| s.len()).max() {
        let from = len.saturating_sub((max - 1).min(MAX_HOLD_BACK));
        if let Some(i) = (from..at).find(|&i| {
            let k = SEEN_PREFIX.min(len - i);
            long.iter().any(|s| bytes[i..i + k] == s[..k])
        }) {
            at = i;
        }
    }
    // A secret begins on a character boundary; the constant hold may not.
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// [`scrub`]'s pass over `text` up to `stop`: returns the scrubbed text and
/// the raw position the pass ended at (`stop`, or past it where a secret
/// that began before it ends).
fn scan(text: &str, live: &[&[u8]], stop: usize) -> (String, usize) {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(stop);
    let (mut i, mut copied) = (0, 0);
    while i < stop {
        let best = live
            .iter()
            .filter(|v| bytes[i..].starts_with(v))
            .map(|v| v.len())
            .max();
        match best {
            // Secrets are whole UTF-8 strings, so a match starts and ends on
            // character boundaries.
            Some(n) => {
                out.push_str(&text[copied..i]);
                out.push_str(MARKER);
                i += n;
                copied = i;
            }
            None => i += 1,
        }
    }
    out.push_str(&text[copied..i]);
    (out, i)
}

impl fmt::Debug for SecretSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n = self.0.read().map(|v| v.len()).unwrap_or(0);
        write!(f, "SecretSet({n} redacted)")
    }
}

/// Replace every occurrence of any non-trivial secret in `text`. One pass,
/// leftmost match first and the longest of those, so a secret that is a
/// prefix of another, or overlaps one, leaves no remainder behind.
pub fn scrub(text: &str, secrets: &[Secret<String>]) -> String {
    let live = live(secrets);
    if live.is_empty() {
        return text.to_string();
    }
    scan(text, &live, text.len()).0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_is_redacted() {
        let s = Secret::new("sk-very-secret".to_string());
        assert_eq!(format!("{s:?}"), "Secret(<redacted>)");
        assert_eq!(format!("{s}"), "<redacted>");
    }

    #[test]
    fn secret_set_is_shared_and_redacted() {
        let a = SecretSet::default();
        let b = a.clone();
        assert_eq!(b.scrub("key sk-added-later-1"), "key sk-added-later-1");
        a.add(Secret::new("sk-added-later-1".into()));
        a.add(Secret::new("sk-added-later-1".into()));
        assert_eq!(b.scrub("key sk-added-later-1"), "key [REDACTED]");
        assert_eq!(format!("{a:?}"), "SecretSet(1 redacted)");
    }

    #[test]
    fn scrub_replaces_secrets() {
        let s = vec![Secret::new("sk-abcdefgh123".to_string())];
        assert_eq!(scrub("key=sk-abcdefgh123!", &s), "key=[REDACTED]!");
        let short = vec![Secret::new("abc".to_string())];
        assert_eq!(scrub("abc", &short), "abc");
    }

    fn set(v: &[&str]) -> SecretSet {
        SecretSet::new(v.iter().map(|s| Secret::new(s.to_string())).collect())
    }

    fn stream(set: &SecretSet, pieces: &[&str]) -> String {
        let mut sc = StreamScrubber::new(set.clone());
        let mut out: String = pieces.iter().map(|p| sc.push(p)).collect();
        out.push_str(&sc.finish());
        out
    }

    #[test]
    fn split_secret_is_redacted_at_every_offset() {
        let set = set(&["sk-abcdefgh123"]);
        let text = "key=sk-abcdefgh123!";
        for at in (0..=text.len()).filter(|&i| text.is_char_boundary(i)) {
            let got = stream(&set, &[&text[..at], &text[at..]]);
            assert_eq!(got, "key=[REDACTED]!", "split at {at}");
        }
    }

    #[test]
    fn one_byte_chunks_and_multibyte_text() {
        let set = set(&["sk-abcdefgh123", "pässwörd-ünï-1"]);
        let text = "é key=sk-abcdefgh123 and pässwörd-ünï-1 ✓ end";
        let pieces: Vec<String> = text.chars().map(String::from).collect();
        let refs: Vec<&str> = pieces.iter().map(String::as_str).collect();
        assert_eq!(stream(&set, &refs), "é key=[REDACTED] and [REDACTED] ✓ end");
        // Output never splits a character and nothing is lost or repeated.
        assert_eq!(stream(&set, &["plain ünï text ✓"]), "plain ünï text ✓");
    }

    #[test]
    fn whole_chunk_prefix_and_overlapping_secrets() {
        let set = set(&["abcdefgh", "abcdefghij", "ghijklmn"]);
        assert_eq!(stream(&set, &["abcdefgh"]), "[REDACTED]");
        assert_eq!(stream(&set, &["xx abcdefghij yy"]), "xx [REDACTED] yy");
        assert_eq!(stream(&set, &["abcdefg", "hij!"]), "[REDACTED]!");
        // Overlap: the leftmost secret wins, the rest is plain text.
        assert_eq!(stream(&set, &["abcdefghijklmn"]), "[REDACTED]klmn");
    }

    #[test]
    fn hold_back_follows_the_set_and_is_bounded() {
        let set = SecretSet::default();
        let mut sc = StreamScrubber::new(set.clone());
        assert_eq!(sc.push("nothing to hold"), "nothing to hold");
        set.add(Secret::new("sk-added-later-1".into()));
        assert_eq!(sc.push("abcdefghijklmnopqrstuvwxyz"), "abcdefghijk");
        assert_eq!(sc.push("sk-added"), "lmnopqrs");
        // The hold counts raw bytes: the secret completed in this piece
        // comes out replaced, and what follows it is held.
        assert_eq!(sc.push("-later-1 x"), "tuvwxyz[REDACTED]");
        assert_eq!(sc.finish(), " x");
        let big = SecretSet::new(vec![Secret::new("k".repeat(1 << 20))]);
        let mut sc = StreamScrubber::new(big);
        sc.push(&"a".repeat(1 << 16));
        assert!(sc.held.len() <= MAX_HOLD_BACK);
    }

    // ---- The hold-back: lag, exactness, and the side channel (T75) ----

    /// The bytes of a long secret's start the hold-back may depend on, and
    /// the length from which a secret is long (the code's `SEEN_PREFIX` and
    /// `LONG_SECRET_LEN`; spelled out so these tests also run on the code
    /// before them).
    const SEEN: usize = 4;
    const LONG: usize = 32;
    /// A 64-character hex token, the shape of the master's API token.
    const TOKEN: &str = "4f1c9e2ab7d03c58e6f19a2b4c7d8e0f1a2b3c4d5e6f708192a3b4c5d6e7f809";

    /// xorshift64*: deterministic randomness for the property tests.
    struct Rng(std::cell::Cell<u64>);
    impl Rng {
        fn next(&self) -> u64 {
            let mut x = self.0.get();
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0.set(x);
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
        fn below(&self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    /// Characters from `alphabet` until at least `bytes` long.
    fn word(rng: &Rng, alphabet: &[char], bytes: usize) -> String {
        let mut s = String::new();
        while s.len() < bytes {
            s.push(alphabet[rng.below(alphabet.len())]);
        }
        s
    }

    /// A char-boundary prefix of `s` of about `n` bytes (at least 1 char).
    fn cut(s: &str, n: usize) -> &str {
        let mut at = n.clamp(1, s.len());
        while !s.is_char_boundary(at) {
            at += 1;
        }
        &s[..at]
    }

    /// `text` cut at random char boundaries, one-byte pieces included.
    fn split_random(rng: &Rng, text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut at = 0;
        while at < text.len() {
            let max = if rng.below(4) == 0 {
                1
            } else {
                1 + rng.below(24)
            };
            let mut end = (at + max).min(text.len());
            while !text.is_char_boundary(end) {
                end += 1;
            }
            out.push(text[at..end].to_string());
            at = end;
        }
        out
    }

    /// Every chunk `push` and `finish` return for these pieces.
    fn chunks<S: AsRef<str>>(set: &SecretSet, pieces: &[S]) -> Vec<String> {
        let mut sc = StreamScrubber::new(set.clone());
        let mut out: Vec<String> = pieces.iter().map(|p| sc.push(p.as_ref())).collect();
        out.push(sc.finish());
        out
    }

    fn secrets_of(v: &[String]) -> Vec<Secret<String>> {
        v.iter().map(|s| Secret::new(s.clone())).collect()
    }

    /// The obvious fix, kept here as the attack's target: hold exactly the
    /// longest suffix of the scrubbed text that is a proper prefix of a
    /// secret. Shows no part of a secret, and answers "does a secret start
    /// with this?" through where it cuts.
    struct ExactHold {
        secrets: Vec<Secret<String>>,
        held: String,
    }

    impl ExactHold {
        fn push(&mut self, piece: &str) -> String {
            self.held.push_str(piece);
            let mut text = scrub(&self.held, &self.secrets);
            let k = (1..text.len())
                .rev()
                .filter(|&k| text.is_char_boundary(text.len() - k))
                .find(|&k| {
                    let tail = &text.as_bytes()[text.len() - k..];
                    self.secrets.iter().any(|s| {
                        let s = s.expose().as_bytes();
                        s.len() >= MIN_SECRET_LEN && s.len() > k && s.starts_with(tail)
                    })
                })
                .unwrap_or(0);
            self.held = text.split_off(text.len() - k);
            text
        }
        fn finish(&mut self) -> String {
            scrub(&std::mem::take(&mut self.held), &self.secrets)
        }
    }

    /// An adaptive prober that sees only the chunks a stream returns for
    /// the text it chose (`observe` runs one fresh stream on `> ` and a
    /// guess). A guess is a hit when the stream released nothing of it
    /// before its end while the same guess ending in a character that is
    /// never in a secret was released. Extends `known` one character per
    /// hit; returns what it learned and how many streams it used.
    fn probe(
        observe: &mut dyn FnMut(&str) -> Vec<String>,
        alphabet: &[char],
        known: &str,
        max: usize,
    ) -> (String, usize) {
        // Bytes released before the stream's end.
        let cut_at = |v: Vec<String>| v[..v.len() - 1].iter().map(String::len).sum::<usize>();
        let mut known = known.to_string();
        let mut streams = 1;
        while known.len() < max {
            if cut_at(observe(&format!("> {known}\u{1}"))) <= 2 {
                break;
            }
            let hit = alphabet.iter().find(|&&c| {
                streams += 1;
                cut_at(observe(&format!("> {known}{c}"))) <= 2
            });
            streams += 1;
            match hit {
                Some(&c) => known.push(c),
                None => break,
            }
        }
        (known, streams)
    }

    const B64: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    /// A 40-byte key with a public 13-byte prefix, the shape of a provider
    /// key.
    fn key40(rng: &Rng) -> String {
        let alpha: Vec<char> = B64.chars().collect();
        format!("sk-ant-api03-{}", word(rng, &alpha, 27))
    }

    /// The finding that sets the design: with the obvious fix the cut is a
    /// yes/no answer to "does a secret start with this?", and an adaptive
    /// prober reads a 40-byte key with a few streams per character.
    #[test]
    fn the_obvious_fix_is_an_oracle() {
        let rng = Rng(std::cell::Cell::new(0x0dd5_eed5));
        let key = key40(&rng);
        let alpha: Vec<char> = B64.chars().collect();
        let set = vec![Secret::new(key.clone())];
        let mut observe = |text: &str| {
            let mut sc = ExactHold {
                secrets: set.clone(),
                held: String::new(),
            };
            vec![sc.push(text), sc.finish()]
        };
        // From nothing, and from the key's public prefix.
        let (all, n_all) = probe(&mut observe, &alpha, "", key.len() - 1);
        let (rest, n_rest) = probe(&mut observe, &alpha, "sk-ant-api03-", key.len() - 1);
        eprintln!(
            "obvious fix: {} of 40 bytes in {n_all} streams from nothing, {} in {n_rest} from the public prefix",
            all.len(),
            rest.len()
        );
        // The last byte is the whole-secret test every scrubber answers.
        assert_eq!(all, key[..key.len() - 1]);
        assert_eq!(rest, key[..key.len() - 1]);
    }

    /// The same prober against the stream scrubber, in every way of feeding
    /// it a guess: one piece, one byte per piece, the guess cut from its
    /// lead, a short secret beside the long one. It learns at most the
    /// first `SEEN` bytes, and from a key's public prefix nothing at all.
    #[test]
    fn the_prober_learns_at_most_the_first_bytes() {
        let rng = Rng(std::cell::Cell::new(0x0dd5_eed5));
        let key = key40(&rng);
        let alpha: Vec<char> = B64.chars().collect();
        for short in [None, Some("pass-word-1234")] {
            let set = SecretSet::new(
                std::iter::once(key.clone())
                    .chain(short.map(String::from))
                    .map(Secret::new)
                    .collect(),
            );
            type Feed = fn(&str) -> Vec<String>;
            let feeds: [Feed; 3] = [
                |t| vec![t.to_string()],
                |t| t.chars().map(String::from).collect(),
                |t| vec![t[..2].to_string(), t[2..].to_string()],
            ];
            for feed in feeds {
                let mut observe = |text: &str| chunks(&set, &feed(text));
                let (all, _) = probe(&mut observe, &alpha, "", key.len() - 1);
                assert!(
                    all.len() <= SEEN && key.starts_with(&all),
                    "learned {all:?}"
                );
                let (rest, _) = probe(&mut observe, &alpha, "sk-ant-api03-", key.len() - 1);
                assert_eq!(rest, "sk-ant-api03-", "learned past the public prefix");
            }
        }
    }

    /// The bound, independent of any strategy: two long secrets of one
    /// length that agree on their first `SEEN` bytes give every input the
    /// same chunks, cut for cut, as long as neither appears whole (a whole
    /// secret is replaced: the test every scrubber answers). Random texts
    /// built from both secrets' starts, random cuts, one-byte pieces, a
    /// short and another long secret beside them, a secret added mid-stream.
    /// The obvious fix fails the same check (the test has teeth).
    #[test]
    fn what_a_watcher_sees_depends_only_on_a_long_secrets_first_bytes() {
        let rng = Rng(std::cell::Cell::new(0xb0_0d5));
        let alpha = ['a', 'b', '-'];
        let mut exact_differs = 0;
        for round in 0..3000 {
            let len = LONG + rng.below(40);
            let a = word(&rng, &alpha, len);
            let b = format!("{}{}", &a[..SEEN], word(&rng, &alpha, len - SEEN));
            if a == b {
                continue;
            }
            let other = word(&rng, &['a', 'c'], LONG + rng.below(8));
            let short = word(&rng, &alpha, 8 + rng.below(20));
            let mut text = String::new();
            for _ in 0..1 + rng.below(8) {
                match rng.below(5) {
                    0 => text.push_str(cut(&a, rng.below(a.len()))),
                    1 => text.push_str(cut(&b, rng.below(b.len()))),
                    2 => text.push_str(cut(&other, rng.below(other.len() + 1))),
                    3 => text.push_str(&short),
                    _ => text.push_str(&word(&rng, &['a', 'b', '-', 'x', 'é'], rng.below(9))),
                }
            }
            if text.contains(&a) || text.contains(&b) {
                continue;
            }
            let pieces = split_random(&rng, &text);
            let late = rng.below(pieces.len() + 1);
            let mut extra = vec![other.clone()];
            if rng.below(2) == 0 {
                extra.push(short.clone());
            }
            let run = |s: &str| {
                let set = SecretSet::new(secrets_of(&[s.to_string()]));
                let mut sc = StreamScrubber::new(set.clone());
                let mut out = Vec::new();
                for (i, p) in pieces.iter().enumerate() {
                    if i == late {
                        extra.iter().for_each(|e| set.add(Secret::new(e.clone())));
                    }
                    out.push(sc.push(p));
                }
                out.push(sc.finish());
                out
            };
            assert_eq!(run(&a), run(&b), "round {round}: {a:?} {b:?} {pieces:?}");
            let exact = |s: &str| {
                let mut sc = ExactHold {
                    secrets: secrets_of(&[s.to_string()]),
                    held: String::new(),
                };
                let mut out: Vec<String> = pieces.iter().map(|p| sc.push(p)).collect();
                out.push(sc.finish());
                out
            };
            exact_differs += usize::from(exact(&a) != exact(&b));
        }
        assert!(exact_differs > 100, "{exact_differs}");
    }

    /// Exactness: for random secrets (short and long, one a prefix of
    /// another, overlapping, starting with a two-byte character), random
    /// texts made of their pieces and random cuts, everything returned is
    /// `scrub` of the whole text, so no chunk holds part of a secret a
    /// later chunk completes.
    #[test]
    fn the_stream_is_scrub_of_the_whole_text() {
        let rng = Rng(std::cell::Cell::new(0x5eed_1234_abcd));
        let alpha = ['a', 'b', '-', 'é'];
        for round in 0..4000 {
            let mut secrets: Vec<String> = Vec::new();
            for _ in 0..1 + rng.below(4) {
                let fresh = |rng: &Rng| {
                    let n = if rng.below(2) == 0 {
                        MIN_SECRET_LEN + rng.below(LONG - MIN_SECRET_LEN)
                    } else {
                        LONG + rng.below(40)
                    };
                    word(rng, &alpha, n)
                };
                let s = match (secrets.last().cloned(), rng.below(5)) {
                    // A longer secret that begins with another.
                    (Some(p), 0) => format!("{p}{}", word(&rng, &alpha, 1 + rng.below(30))),
                    // A secret that is a prefix of another.
                    (Some(p), 1) if p.len() > MIN_SECRET_LEN => {
                        cut(&p, MIN_SECRET_LEN + rng.below(p.len() - MIN_SECRET_LEN)).to_string()
                    }
                    // One that begins with another's end (they overlap).
                    (Some(p), 2) => {
                        let mut k = 1 + rng.below(p.len() - 1);
                        while !p.is_char_boundary(k) {
                            k += 1;
                        }
                        format!(
                            "{}{}",
                            &p[k..],
                            word(&rng, &alpha, MIN_SECRET_LEN + rng.below(30))
                        )
                    }
                    _ => fresh(&rng),
                };
                secrets.push(s);
            }
            let mut text = String::new();
            for _ in 0..1 + rng.below(10) {
                let s = &secrets[rng.below(secrets.len())];
                match rng.below(5) {
                    0 | 1 => text.push_str(s),
                    2 => text.push_str(cut(s, rng.below(s.len()))),
                    3 => text.push_str("[REDACTED]"),
                    _ => text.push_str(&word(&rng, &['a', 'b', '-', 'é', ' ', '✓'], rng.below(12))),
                }
            }
            let pieces = split_random(&rng, &text);
            let set = SecretSet::new(secrets_of(&secrets));
            let got = chunks(&set, &pieces);
            assert_eq!(
                got.concat(),
                scrub(&text, &secrets_of(&secrets)),
                "round {round}: {secrets:?} {pieces:?} {got:?}"
            );
        }
    }

    /// Found by the property test above on the old code: a secret that is
    /// the start of a longer one was replaced the moment it was whole, and
    /// the longer one's rest came out as text. The stream now holds the raw
    /// text, so the longer secret is replaced as `scrub` replaces it, and a
    /// replacement marker never joins later text into a match.
    #[test]
    fn a_longer_secret_completing_after_a_shorter_one_is_replaced_whole() {
        let set1 = set(&["abcdefgh", "abcdefghij"]);
        assert_eq!(stream(&set1, &["abcdefgh", "ij!"]), "[REDACTED]!");
        // The old code re-read its own marker: "…D]" + "sk-two-123" made
        // the second secret out of text that never held it.
        let set2 = set(&["sk-one-12345", "D]sk-two-123"]);
        assert_eq!(
            stream(&set2, &["sk-one-12345", "sk-two-123 ok"]),
            "[REDACTED]sk-two-123 ok"
        );
    }

    /// The lag: ordinary text comes out of the `push` that brought it when
    /// only long secrets are known; with a short one it is at most 31 bytes
    /// behind. The old code held `longest − 1` bytes (63 for the token).
    #[test]
    fn ordinary_text_is_not_held_back() {
        let set = set(&[TOKEN]);
        let mut sc = StreamScrubber::new(set.clone());
        for piece in ["Hello, ", "this is ", "an answer, ", "slowly. "] {
            assert_eq!(sc.push(piece), piece);
        }
        // A short secret: its constant hold, `length − 1` bytes.
        set.add(Secret::new("pass-word-1234".into()));
        let plain = "and more plain text here, long enough to pass by. ";
        assert_eq!(sc.push(plain), plain[..plain.len() - 13]);
        // The longest short secret (31 bytes) holds 30.
        set.add(Secret::new(format!("{}q", "z".repeat(LONG - 2))));
        assert_eq!(
            sc.push(plain),
            format!(
                "{}{}",
                &plain[plain.len() - 13..],
                &plain[..plain.len() - 30]
            )
        );
        assert_eq!(sc.finish(), plain[plain.len() - 30..]);
    }

    /// A long secret's start is held until it is complete (replaced) or
    /// can no longer be one (as many bytes as the longest long secret);
    /// what comes before it is not.
    #[test]
    fn a_long_secrets_start_is_held_until_it_cannot_be_one() {
        let set = set(&[TOKEN]);
        let mut sc = StreamScrubber::new(set.clone());
        assert_eq!(sc.push("key: 4f"), "key: ");
        assert_eq!(sc.push("1c9e2ab7"), "");
        assert_eq!(sc.push(&TOKEN[10..40]), "");
        assert_eq!(
            sc.push(&format!("{} done", &TOKEN[40..])),
            "[REDACTED] done"
        );
        // A start that diverges after its first bytes is held as long as
        // the secret could be: that is what keeps the cut blind to them.
        let near = format!("{}zz", &TOKEN[..SEEN]);
        assert_eq!(sc.push(&format!("x {near}")), "x ");
        let fill = "y".repeat(TOKEN.len() - 1 - near.len());
        assert_eq!(sc.push(&fill), "");
        assert_eq!(sc.push("!"), format!("{near}{fill}!"));
        assert_eq!(sc.finish(), "");
    }

    /// A secret added between pieces is caught from the next piece on, in
    /// both modes.
    #[test]
    fn a_secret_added_mid_stream_is_caught_from_the_next_piece() {
        for s in [TOKEN, "sk-short-12345"] {
            let set = SecretSet::default();
            let mut sc = StreamScrubber::new(set.clone());
            let mut out = sc.push("before ");
            set.add(Secret::new(s.into()));
            let (a, b) = s.split_at(5);
            let (b, c) = b.split_at(b.len() / 2);
            for p in [&format!("after: {a}"), b, &format!("{c} end")] {
                out.push_str(&sc.push(p));
                assert!(!out.contains(a) || !out.contains(b), "{out}");
            }
            out.push_str(&sc.finish());
            assert_eq!(out, "before after: [REDACTED] end");
        }
    }

    /// A long secret that begins with a two-byte character, split inside
    /// that character's neighbours and one byte at a time.
    #[test]
    fn a_long_secret_starting_with_a_wide_character() {
        let secret = format!("élan-{}", &TOKEN[..40]);
        let set = set(&[&secret]);
        let text = format!("ü é {secret} ✓");
        for at in (0..=text.len()).filter(|&i| text.is_char_boundary(i)) {
            assert_eq!(
                stream(&set, &[&text[..at], &text[at..]]),
                "ü é [REDACTED] ✓",
                "{at}"
            );
        }
        let pieces: Vec<String> = text.chars().map(String::from).collect();
        let got = chunks(&set, &pieces);
        assert_eq!(got.concat(), "ü é [REDACTED] ✓");
        assert_eq!(
            got[..4],
            ["ü", " ", "", "é "],
            "only what could begin it is held"
        );
    }

    /// The work per piece (run with `--ignored --nocapture`): 50 secrets of
    /// 200 bytes, 1 MB of text in 20-byte pieces, plain and with a secret's
    /// first bytes every 100 bytes, against the constant hold it replaces.
    #[test]
    #[ignore = "measurement"]
    fn measure_push_cost() {
        let rng = Rng(std::cell::Cell::new(42));
        let alpha: Vec<char> = B64.chars().collect();
        let secrets: Vec<String> = (0..50).map(|_| word(&rng, &alpha, 200)).collect();
        let set = SecretSet::new(secrets_of(&secrets));
        let plain = word(&rng, &['a', 'b', ' ', 'x', 'y', '.'], 1 << 20);
        let mut hostile = String::new();
        while hostile.len() < 1 << 20 {
            hostile.push_str(&secrets[rng.below(50)][..SEEN]);
            hostile.push_str(&word(&rng, &['a', ' '], 96));
        }
        for (name, text) in [("plain", &plain), ("hostile", &hostile)] {
            let pieces: Vec<&str> = text
                .as_bytes()
                .chunks(20)
                .map(|c| std::str::from_utf8(c).unwrap())
                .collect();
            let t = std::time::Instant::now();
            let out = chunks(&set, &pieces);
            let new = t.elapsed();
            let t = std::time::Instant::now();
            let mut held = String::new();
            let hb = secrets.iter().map(String::len).max().unwrap() - 1;
            for p in &pieces {
                held.push_str(p);
                let mut s = set.scrub(&held);
                held = s.split_off(s.len() - hb.min(s.len()));
            }
            let old = t.elapsed();
            eprintln!(
                "{name}: {} pushes, this code {new:?}, constant hold {old:?}",
                pieces.len()
            );
            assert_eq!(out.concat().len(), text.len());
        }
    }

    #[test]
    fn json_scrubs_fields_not_serialized_text() {
        let set = set(&["pa\"ss\\word-123"]);
        let mut v = serde_json::json!({"a": "x pa\"ss\\word-123 y", "pa\"ss\\word-123": [1, "pa\"ss\\word-123"]});
        // The serialized form escapes the secret, so text scrubbing misses it.
        assert_eq!(set.scrub(&v.to_string()), v.to_string());
        set.scrub_value(&mut v);
        assert_eq!(
            v,
            serde_json::json!({"a": "x [REDACTED] y", "[REDACTED]": [1, "[REDACTED]"]})
        );
    }
}
