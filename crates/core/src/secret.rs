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
}

impl fmt::Debug for SecretSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n = self.0.read().map(|v| v.len()).unwrap_or(0);
        write!(f, "SecretSet({n} redacted)")
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
}
