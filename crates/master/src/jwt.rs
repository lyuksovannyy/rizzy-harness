//! HS256 JSON Web Tokens, as narrow as the server needs them.
//!
//! The algorithm is fixed: a token is accepted only when its header is
//! exactly the one this module writes, so there is no `alg` negotiation
//! (no `none`, no RSA/HMAC confusion). HMAC-SHA256 comes from aws-lc-rs
//! (already the TLS provider) and is checked in constant time.
//!
//! The key is `HARNESS_JWT_SECRET` (at least 32 bytes), else a random one
//! kept in `<data>/keys/jwt.key` (0600). Several servers behind one address
//! must share it, which is what the environment variable is for.

use std::path::Path;

use aws_lc_rs::hmac;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// `{"alg":"HS256","typ":"JWT"}`, base64url.
const HEADER: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";

/// Shortest secret accepted from the environment.
pub const MIN_SECRET: usize = 32;

/// Issuer and audience of every token.
pub const ISSUER: &str = harness_core::APP_NAME;

#[derive(Clone)]
pub struct JwtKey {
    key: hmac::Key,
}

impl std::fmt::Debug for JwtKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JwtKey(..)")
    }
}

impl JwtKey {
    pub fn new(secret: &[u8]) -> Self {
        Self {
            key: hmac::Key::new(hmac::HMAC_SHA256, secret),
        }
    }

    /// A key that lives only as long as this process (tests, `Auth::new`).
    pub fn ephemeral() -> Self {
        let mut b = [0u8; 32];
        getrandom::fill(&mut b).expect("OS randomness");
        Self::new(&b)
    }

    /// `HARNESS_JWT_SECRET` if set, else `<data>/keys/jwt.key` (created on
    /// first use).
    pub fn load(data_dir: &Path) -> anyhow::Result<Self> {
        if let Ok(s) = std::env::var("HARNESS_JWT_SECRET") {
            let s = s.trim();
            if s.len() < MIN_SECRET {
                anyhow::bail!("HARNESS_JWT_SECRET must be at least {MIN_SECRET} characters");
            }
            return Ok(Self::new(s.as_bytes()));
        }
        let path = data_dir.join("keys/jwt.key");
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let s = crate::auth::load_or_create_secret(&path, || crate::auth::random_hex(32))?;
        Ok(Self::new(s.expose().as_bytes()))
    }

    pub fn sign<T: Serialize>(&self, claims: &T) -> String {
        let payload = B64.encode(serde_json::to_vec(claims).expect("claims serialize"));
        let signing_input = format!("{HEADER}.{payload}");
        let tag = hmac::sign(&self.key, signing_input.as_bytes());
        format!("{signing_input}.{}", B64.encode(tag.as_ref()))
    }

    /// The claims of a token this key signed, or `None`. Expiry and the
    /// token's purpose are the caller's to check (see [`Claims::valid`]).
    pub fn verify<T: DeserializeOwned>(&self, token: &str) -> Option<T> {
        if token.len() > 4096 {
            return None;
        }
        let mut parts = token.split('.');
        let (h, p, s) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() || h != HEADER {
            return None;
        }
        let sig = B64.decode(s).ok()?;
        let signing_input = &token[..h.len() + 1 + p.len()];
        hmac::verify(&self.key, signing_input.as_bytes(), &sig).ok()?;
        serde_json::from_slice(&B64.decode(p).ok()?).ok()
    }
}

/// What a token is for: an access token never refreshes, a refresh token
/// never authorises a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Use {
    Access,
    Refresh,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    pub iss: String,
    pub aud: String,
    /// The user id.
    pub sub: String,
    #[serde(rename = "use")]
    pub token_use: Use,
    /// The user's session version when issued (logging out everywhere
    /// raises it).
    pub sv: i64,
    /// The login this token descends from; refresh rotation stays in it and
    /// logout or a replayed refresh token revokes all of it.
    pub fam: String,
    /// Unique per token.
    pub jti: String,
    /// Seconds since the epoch.
    pub iat: i64,
    pub exp: i64,
}

impl Claims {
    pub fn new(user_id: &str, token_use: Use, sv: i64, fam: &str, now_s: i64, ttl_s: i64) -> Self {
        Self {
            iss: ISSUER.into(),
            aud: ISSUER.into(),
            sub: user_id.into(),
            token_use,
            sv,
            fam: fam.into(),
            jti: crate::auth::random_hex(16),
            iat: now_s,
            exp: now_s + ttl_s,
        }
    }

    /// Issued by this app, for `want`, and not expired at `now_s`.
    pub fn valid(&self, want: Use, now_s: i64) -> bool {
        self.iss == ISSUER && self.aud == ISSUER && self.token_use == want && now_s < self.exp
            // A token from the future (clock skew of more than a minute) is refused.
            && self.iat <= now_s + 60
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_constant_matches() {
        assert_eq!(B64.encode(br#"{"alg":"HS256","typ":"JWT"}"#), HEADER);
    }

    #[test]
    fn sign_verify_and_reject_tampering() {
        let k = JwtKey::ephemeral();
        let c = Claims::new("u1", Use::Access, 3, "fam", 1_000, 900);
        let t = k.sign(&c);
        assert_eq!(k.verify::<Claims>(&t), Some(c.clone()));
        assert!(c.valid(Use::Access, 1_899));
        assert!(!c.valid(Use::Access, 1_900), "expired");
        assert!(!c.valid(Use::Refresh, 1_000), "wrong use");

        // Another key, a changed payload, alg=none, extra segments.
        assert!(JwtKey::ephemeral().verify::<Claims>(&t).is_none());
        let mut parts: Vec<&str> = t.split('.').collect();
        let forged = B64.encode(br#"{"sub":"admin"}"#);
        parts[1] = &forged;
        assert!(k.verify::<Claims>(&parts.join(".")).is_none());
        let none_hdr = B64.encode(br#"{"alg":"none","typ":"JWT"}"#);
        let p = t.split('.').nth(1).unwrap();
        assert!(k.verify::<Claims>(&format!("{none_hdr}.{p}.")).is_none());
        assert!(k.verify::<Claims>(&format!("{t}.x")).is_none());
        assert!(k.verify::<Claims>("garbage").is_none());
    }

    #[test]
    fn key_file_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        // The test process must not see a secret from the developer's env.
        if std::env::var("HARNESS_JWT_SECRET").is_ok() {
            return;
        }
        let a = JwtKey::load(dir.path()).unwrap();
        let b = JwtKey::load(dir.path()).unwrap();
        let t = a.sign(&Claims::new("u", Use::Access, 0, "f", 0, 60));
        assert!(b.verify::<Claims>(&t).is_some());
    }
}
