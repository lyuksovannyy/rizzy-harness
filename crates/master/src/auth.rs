//! Authentication and browser hardening (PLAN.md T1–T3, §0 accounts).
//!
//! * People log in with username + password (Argon2id). A successful login
//!   sets a random session id in an `HttpOnly; SameSite=Strict` cookie; only
//!   its SHA-256 is stored, so a leaked database does not leak sessions.
//! * The first account (the owner) is created with a one-time **claim code**
//!   from `<data>/claim-code` (0600), so whoever reaches a fresh server first
//!   cannot take it over. The file is deleted once claimed.
//! * A 256-bit **API token** in `<data>/api-token` (0600) is accepted as
//!   `Authorization: Bearer` for scripts and tests; it acts as the owner.
//! * No secret is ever logged or accepted in a URL.
//! * Every request's `Host` must be allowlisted (DNS-rebinding defence), and
//!   any `Origin` sent must match an allowed host (CSRF / cross-site WS).

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash};
use harness_store::{Role, UserRow};
use sha2::{Digest, Sha256};

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use harness_core::{APP_NAME, Secret};
use harness_store::Store;
use subtle::ConstantTimeEq;

use crate::Master;

pub fn cookie_name() -> String {
    format!("{APP_NAME}_session")
}

/// UI sessions last this long.
const SESSION_TTL_MS: i64 = 30 * 24 * 3600 * 1000;

#[derive(Clone)]
pub struct Auth {
    token: Secret<String>,
    /// `host[:port]` values accepted in `Host` and `Origin`.
    allowed_hosts: Vec<String>,
    secure_cookie: bool,
    claim: Arc<tokio::sync::Mutex<Option<Claim>>>,
}

/// The one-time code that creates the owner account.
pub struct Claim {
    pub code: Secret<String>,
    /// Deleted after a successful claim.
    pub path: PathBuf,
}

/// Who is making a request.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Principal {
    /// `None` for the API token.
    pub user_id: Option<String>,
    pub username: String,
    pub role: Role,
}

impl Principal {
    fn from_user(u: UserRow) -> Self {
        Self {
            user_id: Some(u.user_id),
            username: u.username,
            role: u.role,
        }
    }

    /// May this principal see a session owned by `owner_id`?
    pub fn can_see(&self, owner_id: Option<&str>) -> bool {
        self.role == Role::Owner || (owner_id.is_some() && owner_id == self.user_id.as_deref())
    }
}

pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("OS randomness");
    hex::encode(buf)
}

/// Read a 0600 secret file, creating it with `make()` on first run.
pub fn load_or_create_secret(
    path: &Path,
    make: impl FnOnce() -> String,
) -> std::io::Result<Secret<String>> {
    match std::fs::read_to_string(path) {
        Ok(s) if !s.trim().is_empty() => return Ok(Secret::new(s.trim().to_string())),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let token = make();
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    f.write_all(token.as_bytes())?;
    Ok(Secret::new(token))
}

/// The API token (64 hex chars).
pub fn load_or_create_token(path: &Path) -> std::io::Result<Secret<String>> {
    load_or_create_secret(path, || random_hex(32))
}

/// A claim code that is easy to type: 4 groups of 5 base32 characters (~100 bits).
pub fn new_claim_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let mut buf = [0u8; 20];
    getrandom::fill(&mut buf).expect("OS randomness");
    let chars: Vec<char> = buf
        .iter()
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char)
        .collect();
    chars
        .chunks(5)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("-")
}

fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && bool::from(a.ct_eq(b))
}

pub fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

pub fn validate_username(u: &str) -> Result<(), &'static str> {
    let ok_len = (1..=32).contains(&u.len());
    let ok_chars = u
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b));
    if ok_len && ok_chars {
        Ok(())
    } else {
        Err("username must be 1-32 characters: a-z, 0-9, _ . -")
    }
}

pub fn validate_password(p: &str) -> Result<(), &'static str> {
    if p.chars().count() < 10 {
        Err("password must be at least 10 characters")
    } else if p.len() > 1024 {
        Err("password is too long")
    } else {
        Ok(())
    }
}

/// Argon2id with default parameters; runs off the async runtime.
pub async fn hash_password(password: String) -> String {
    tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes())
            .expect("argon2 hashing")
            .to_string()
    })
    .await
    .expect("hash task")
}

/// Verify against a PHC string. When the user does not exist, pass `None`: a
/// dummy hash is still checked so response time does not reveal usernames.
pub async fn verify_password(password: String, phc: Option<String>) -> bool {
    static DUMMY: OnceLock<String> = OnceLock::new();
    tokio::task::spawn_blocking(move || {
        let real = phc.is_some();
        let phc = phc.unwrap_or_else(|| {
            DUMMY
                .get_or_init(|| {
                    Argon2::default()
                        .hash_password(b"dummy password for timing")
                        .expect("argon2 hashing")
                        .to_string()
                })
                .clone()
        });
        let ok = PasswordHash::new(&phc)
            .map(|h| {
                Argon2::default()
                    .verify_password(password.as_bytes(), &h)
                    .is_ok()
            })
            .unwrap_or(false);
        ok && real
    })
    .await
    .unwrap_or(false)
}

impl Auth {
    pub fn new(token: Secret<String>, allowed_hosts: Vec<String>, secure_cookie: bool) -> Self {
        Self {
            token,
            allowed_hosts,
            secure_cookie,
            claim: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// Enable first-run claiming with this code.
    pub fn with_claim(self, claim: Claim) -> Self {
        Self {
            claim: Arc::new(tokio::sync::Mutex::new(Some(claim))),
            ..self
        }
    }

    pub fn token_matches(&self, candidate: &str) -> bool {
        ct_eq(self.token.expose(), candidate)
    }

    /// Create the owner if `code` matches the pending claim. Serialized so two
    /// racing claims cannot both succeed.
    pub async fn claim(
        &self,
        store: &Store,
        code: &str,
        username: String,
        password: String,
    ) -> Result<UserRow, ClaimError> {
        let mut guard = self.claim.lock().await;
        let Some(claim) = guard.as_ref() else {
            return Err(ClaimError::AlreadyClaimed);
        };
        let users = store
            .call(|s| s.user_count())
            .await
            .map_err(|e| ClaimError::Internal(e.to_string()))?;
        if users > 0 {
            *guard = None;
            return Err(ClaimError::AlreadyClaimed);
        }
        // Normalise: case-insensitive, dashes and spaces optional.
        let norm = |s: &str| -> String {
            s.chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .map(|c| c.to_ascii_uppercase())
                .collect()
        };
        if !ct_eq(&norm(claim.code.expose()), &norm(code)) {
            return Err(ClaimError::BadCode);
        }
        validate_username(&username).map_err(ClaimError::Invalid)?;
        validate_password(&password).map_err(ClaimError::Invalid)?;
        let phc = hash_password(password).await;
        let user_id = harness_core::new_id();
        let (uid, uname) = (user_id.clone(), username.clone());
        store
            .call(move |s| s.create_user(&uid, &uname, &phc, Role::Owner))
            .await
            .map_err(|e| ClaimError::Internal(e.to_string()))?;
        let _ = std::fs::remove_file(&claim.path);
        *guard = None;
        let uid = user_id.clone();
        store
            .call(move |s| s.user_by_id(&uid))
            .await
            .ok()
            .flatten()
            .ok_or_else(|| ClaimError::Internal("user vanished".into()))
    }

    fn host_allowed(&self, host: &str) -> bool {
        self.allowed_hosts
            .iter()
            .any(|h| h.eq_ignore_ascii_case(host))
    }

    fn origin_allowed(&self, origin: &str) -> bool {
        let rest = origin
            .strip_prefix("http://")
            .or_else(|| origin.strip_prefix("https://"));
        rest.is_some_and(|h| self.host_allowed(h))
    }

    pub fn session_cookie(&self, id: &str) -> HeaderValue {
        let secure = if self.secure_cookie { "; Secure" } else { "" };
        HeaderValue::from_str(&format!(
            "{}={id}; HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000{secure}",
            cookie_name()
        ))
        .expect("ascii cookie")
    }

    pub fn clear_cookie(&self) -> HeaderValue {
        HeaderValue::from_str(&format!(
            "{}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
            cookie_name()
        ))
        .expect("ascii cookie")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ClaimError {
    #[error("this server has already been claimed")]
    AlreadyClaimed,
    #[error("wrong claim code")]
    BadCode,
    #[error("{0}")]
    Invalid(&'static str),
    #[error("internal error: {0}")]
    Internal(String),
}

pub fn cookie_value(headers: &HeaderMap) -> Option<String> {
    let name = cookie_name();
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
}

/// Start a UI session for `user_id`; returns the cookie value.
pub async fn create_ui_session(
    store: &Store,
    user_id: &str,
) -> Result<String, harness_store::StoreError> {
    let id = random_hex(32);
    let (hash, uid) = (sha256_hex(&id), user_id.to_string());
    let expires = harness_core::now_ms() + SESSION_TTL_MS;
    store
        .call(move |s| s.ui_session_create(&hash, &uid, expires))
        .await?;
    Ok(id)
}

pub async fn delete_ui_session(store: &Store, id: &str) {
    let hash = sha256_hex(id);
    let _ = store.call(move |s| s.ui_session_delete(&hash)).await;
}

/// The principal behind a request, if any.
pub async fn authenticate(master: &Master, headers: &HeaderMap) -> Option<Principal> {
    if let Some(bearer) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        return master
            .inner
            .auth
            .token_matches(bearer.trim())
            .then(|| Principal {
                user_id: None,
                username: "api-token".into(),
                role: Role::Owner,
            });
    }
    let id = cookie_value(headers)?;
    // Session ids are 64 hex chars; reject anything else before touching the DB.
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let hash = sha256_hex(&id);
    let now = harness_core::now_ms();
    master
        .inner
        .mirror
        .call(move |s| s.ui_session_user(&hash, now))
        .await
        .ok()
        .flatten()
        .map(Principal::from_user)
}

pub async fn is_authenticated(master: &Master, headers: &HeaderMap) -> bool {
    authenticate(master, headers).await.is_some()
}

/// Host/Origin checks for every request, plus security headers.
pub async fn guard(State(master): State<Master>, req: Request, next: Next) -> Response {
    let auth = &master.inner.auth;
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if !auth.host_allowed(host) {
        return (StatusCode::FORBIDDEN, "host not allowed").into_response();
    }
    if let Some(origin) = req.headers().get(header::ORIGIN) {
        let ok = origin.to_str().is_ok_and(|o| auth.origin_allowed(o));
        if !ok {
            return (StatusCode::FORBIDDEN, "origin not allowed").into_response();
        }
    }
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; connect-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
        ),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_file_is_private_and_stable() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("ui-token");
        let a = load_or_create_token(&p).unwrap();
        let b = load_or_create_token(&p).unwrap();
        assert_eq!(a.expose(), b.expose());
        assert_eq!(a.expose().len(), 64);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&p).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn host_and_origin_checks() {
        let a = Auth::new(
            Secret::new("t".into()),
            vec!["127.0.0.1:7777".into(), "localhost:7777".into()],
            false,
        );
        assert!(a.host_allowed("localhost:7777"));
        assert!(!a.host_allowed("evil.example:7777"));
        assert!(a.origin_allowed("http://127.0.0.1:7777"));
        assert!(!a.origin_allowed("http://evil.example"));
        assert!(!a.origin_allowed("null"));
        assert!(a.token_matches("t"));
        assert!(!a.token_matches("u"));
    }

    #[test]
    fn claim_code_shape_and_validation() {
        let c = new_claim_code();
        assert_eq!(c.len(), 23);
        assert_eq!(c.matches('-').count(), 3);
        assert!(validate_username("alex").is_ok());
        assert!(validate_username("Alex").is_err());
        assert!(validate_username("").is_err());
        assert!(validate_password("short").is_err());
        assert!(validate_password("long enough pw").is_ok());
    }

    #[tokio::test]
    async fn password_hash_roundtrip() {
        let phc = hash_password("correct horse battery".into()).await;
        assert!(phc.starts_with("$argon2id$"));
        assert!(verify_password("correct horse battery".into(), Some(phc.clone())).await);
        assert!(!verify_password("wrong".into(), Some(phc)).await);
        assert!(!verify_password("anything".into(), None).await);
    }

    #[test]
    fn cookie_parsing() {
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("a=1; {}=abc; b=2", cookie_name())).unwrap(),
        );
        assert_eq!(cookie_value(&h).as_deref(), Some("abc"));
    }
}
