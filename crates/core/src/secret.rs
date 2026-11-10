use std::fmt;
use std::sync::{Arc, RwLock};

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

    /// Bytes a streaming scrubber must hold back: one less than the longest
    /// redactable secret, capped at [`MAX_HOLD_BACK`].
    fn hold_back(&self) -> usize {
        let v = self.0.read().unwrap_or_else(|p| p.into_inner());
        v.iter()
            .map(|s| s.expose().len())
            .filter(|&n| n >= MIN_SECRET_LEN)
            .max()
            .map_or(0, |n| (n - 1).min(MAX_HOLD_BACK))
    }
}

/// Secrets shorter than this are not redacted (too many false hits).
const MIN_SECRET_LEN: usize = 8;
/// Upper bound on what a [`StreamScrubber`] holds. Keys are far shorter; a
/// longer secret is still scrubbed whole but may leak if split by a flush.
pub const MAX_HOLD_BACK: usize = 4096;

/// Scrubs a stream that arrives in pieces. A secret split across two pieces
/// is still replaced: the last `longest secret - 1` bytes of what has been
/// seen are held back until the next piece (or [`finish`](Self::finish))
/// arrives, because only those can be the start of a secret that continues.
/// The held bytes are bounded by [`MAX_HOLD_BACK`]; the hold-back follows
/// the secret set as it grows.
pub struct StreamScrubber {
    secrets: SecretSet,
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
        let mut text = self.secrets.scrub(&self.held);
        let mut at = text.len() - self.secrets.hold_back().min(text.len());
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        self.held = text.split_off(at);
        text
    }

    /// The stream ended (or was cancelled or cut): release what is held.
    pub fn finish(&mut self) -> String {
        let held = std::mem::take(&mut self.held);
        self.secrets.scrub(&held)
    }
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
    let live: Vec<&[u8]> = secrets
        .iter()
        .map(|s| s.expose().as_bytes())
        .filter(|v| v.len() >= MIN_SECRET_LEN)
        .collect();
    if live.is_empty() {
        return text.to_string();
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let (mut i, mut copied) = (0, 0);
    while i < bytes.len() {
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
                out.push_str("[REDACTED]");
                i += n;
                copied = i;
            }
            None => i += 1,
        }
    }
    out.push_str(&text[copied..]);
    out
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
        assert_eq!(sc.push("-later-1 x"), "tuvw");
        assert_eq!(sc.finish(), "xyz[REDACTED] x");
        let big = SecretSet::new(vec![Secret::new("k".repeat(1 << 20))]);
        let mut sc = StreamScrubber::new(big);
        sc.push(&"a".repeat(1 << 16));
        assert!(sc.held.len() <= MAX_HOLD_BACK);
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
