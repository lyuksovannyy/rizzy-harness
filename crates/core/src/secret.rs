use std::fmt;

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

/// Replace every occurrence of any non-trivial secret in `text`.
pub fn scrub(text: &str, secrets: &[Secret<String>]) -> String {
    let mut out = text.to_string();
    for s in secrets {
        let v = s.expose();
        if v.len() >= 8 && out.contains(v.as_str()) {
            out = out.replace(v.as_str(), "[REDACTED]");
        }
    }
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
    fn scrub_replaces_secrets() {
        let s = vec![Secret::new("sk-abcdefgh123".to_string())];
        assert_eq!(scrub("key=sk-abcdefgh123!", &s), "key=[REDACTED]!");
        let short = vec![Secret::new("abc".to_string())];
        assert_eq!(scrub("abc", &short), "abc");
    }
}
