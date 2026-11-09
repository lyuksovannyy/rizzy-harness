//! Authentication and browser hardening (PLAN.md T1–T3, §0 accounts).
//!
//! * People log in with username + password (Argon2id). A successful login
//!   sets two `HttpOnly; SameSite=Strict` cookies holding signed JWTs
//!   (`jwt.rs`): a short-lived **access token** (every request; checked by
//!   signature, no session table) and a **refresh token** (sent only to
//!   `/api/auth/refresh`) that is rotated on each use. Refresh tokens are
//!   tracked in the KV store (`kv.rs`): a refresh token used again after
//!   its short grace period is treated as stolen and its whole login (the
//!   token family) is revoked. Logout revokes the family too, so its access
//!   tokens stop at once. Logging a user out everywhere raises their
//!   session version, which every token carries.
//! * The first account (an admin) is created with a one-time **claim code**
//!   from `<data>/claim-code` (0600), so whoever reaches a fresh server first
//!   cannot take it over. The file is deleted once claimed.
//! * Only admins create further accounts (no self-signup). New accounts get a
//!   temporary password and must change it at first login.
//! * Everyone, admins included, sees only their own conversations.
//! * Agentic sessions run shell commands on a worker node, so they need the
//!   `can_agentic` grant (admins always have it).
//! * A 256-bit **API token** in `<data>/api-token` (0600) is accepted as
//!   `Authorization: Bearer` for scripts and tests. It is the machine owner's
//!   root key: admin rights and access to every session.
//! * Failed logins are throttled per username.
//! * No secret is ever logged or accepted in a URL.
//! * Every request's `Host` must be allowlisted (DNS-rebinding defence), and
//!   any `Origin` sent must match an allowed host (CSRF / cross-site WS).

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash};
use harness_store::{NewUser, Role, UserRow};
use sha2::{Digest, Sha256};

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use harness_core::{APP_NAME, Secret};
use harness_store::Store;
use subtle::ConstantTimeEq;

use crate::Master;
use crate::jwt::{Claims, Use};

/// The pre-JWT session cookie; only ever cleared now.
pub fn cookie_name() -> String {
    format!("{APP_NAME}_session")
}

pub fn access_cookie_name() -> String {
    format!("{APP_NAME}_access")
}

pub fn refresh_cookie_name() -> String {
    format!("{APP_NAME}_refresh")
}

/// The only path the refresh cookie is sent to.
pub const REFRESH_PATH: &str = "/api/auth";

/// Defaults; `HARNESS_ACCESS_TTL_SECS` / `HARNESS_REFRESH_TTL_DAYS` change them.
pub const ACCESS_TTL_S: i64 = 15 * 60;
pub const REFRESH_TTL_S: i64 = 30 * 24 * 3600;

/// A rotated refresh token still works this long (two tabs refreshing at
/// once, a response lost on the way); after that, using it again revokes
/// the login.
pub const REFRESH_GRACE: std::time::Duration = std::time::Duration::from_secs(30);

fn now_s() -> i64 {
    harness_core::now_ms() / 1000
}

fn refresh_key(jti: &str) -> String {
    format!("auth.refresh.{jti}")
}

fn family_key(fam: &str) -> String {
    format!("auth.family.{fam}")
}

/// The two tokens of a login.
pub struct Tokens {
    pub access: String,
    pub refresh: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RefreshError {
    #[error("not logged in")]
    Invalid,
    /// A rotated refresh token came back after its grace period.
    #[error("this login was revoked; log in again")]
    Reused,
    #[error("{0}")]
    Internal(String),
}

#[derive(Clone)]
pub struct Auth {
    token: Secret<String>,
    /// `host[:port]` values accepted in `Host` and `Origin`.
    allowed_hosts: Vec<String>,
    secure_cookie: bool,
    claim: Arc<tokio::sync::Mutex<Option<Claim>>>,
    /// Lowercase username -> recent failure times (ms).
    failures: Failures,
    /// Caller (user id, or `api-token`) -> recent failed lookups by
    /// username (adding a workspace member).
    lookups: Failures,
    jwt: crate::jwt::JwtKey,
    /// Refresh tokens and revoked token families.
    kv: Arc<dyn crate::kv::Kv>,
    access_ttl_s: i64,
    refresh_ttl_s: i64,
}

type Failures = Arc<std::sync::Mutex<std::collections::HashMap<String, Vec<i64>>>>;

/// Failed logins allowed per username within `THROTTLE_WINDOW_MS`.
pub const THROTTLE_MAX_FAILURES: usize = 5;
pub const THROTTLE_WINDOW_MS: i64 = 15 * 60 * 1000;
/// Failed member adds allowed per caller within `THROTTLE_WINDOW_MS`: the
/// "no such user" answer tells whether a username exists (PLAN.md T12).
pub const LOOKUP_MAX_FAILURES: usize = 10;

/// How many of `key`'s failures are still inside the window (older ones
/// are dropped).
fn recent_failures(map: &Failures, key: &str, now_ms: i64) -> usize {
    let mut map = map.lock().unwrap_or_else(|p| p.into_inner());
    let list = map.entry(key.to_string()).or_default();
    list.retain(|t| now_ms - *t < THROTTLE_WINDOW_MS);
    list.len()
}

fn record_failure(map: &Failures, key: &str, now_ms: i64) {
    map.lock()
        .unwrap_or_else(|p| p.into_inner())
        .entry(key.to_string())
        .or_default()
        .push(now_ms);
}

/// The one-time code that creates the first admin account.
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
    /// Effective: admins always may.
    pub can_agentic: bool,
    pub must_change_password: bool,
}

impl Principal {
    pub fn from_user(u: UserRow) -> Self {
        Self {
            can_agentic: u.role == Role::Admin || u.can_agentic,
            must_change_password: u.must_change_password,
            user_id: Some(u.user_id),
            username: u.username,
            role: u.role,
        }
    }

    fn api_token() -> Self {
        Self {
            user_id: None,
            username: "api-token".into(),
            role: Role::Admin,
            can_agentic: true,
            must_change_password: false,
        }
    }

    pub fn is_admin(&self) -> bool {
        self.role == Role::Admin
    }

    /// May this principal see a session owned by `owner_id`? Only its own;
    /// the API token (machine owner's key) sees everything.
    pub fn can_see(&self, owner_id: Option<&str>) -> bool {
        match &self.user_id {
            None => true,
            Some(me) => owner_id == Some(me.as_str()),
        }
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

/// Create an account after validating the username and password.
pub async fn create_account(
    store: &Store,
    username: String,
    password: String,
    role: Role,
    can_agentic: bool,
    must_change_password: bool,
) -> Result<UserRow, AccountError> {
    validate_username(&username).map_err(AccountError::Invalid)?;
    validate_password(&password).map_err(AccountError::Invalid)?;
    let user = NewUser {
        user_id: harness_core::new_id(),
        username,
        password_hash: hash_password(password).await,
        role,
        can_agentic,
        must_change_password,
    };
    let uid = user.user_id.clone();
    store
        .call(move |s| s.create_user(&user))
        .await
        .map_err(|e| match e {
            harness_store::StoreError::Conflict(_) => AccountError::Taken,
            other => AccountError::Internal(other.to_string()),
        })?;
    store
        .call(move |s| s.user_by_id(&uid))
        .await
        .ok()
        .flatten()
        .ok_or_else(|| AccountError::Internal("user vanished".into()))
}

/// Set a password. `must_change` forces another change at next login (admin
/// resets). Signs the user out everywhere.
pub async fn set_password(
    store: &Store,
    user_id: &str,
    password: String,
    must_change: bool,
) -> Result<(), AccountError> {
    validate_password(&password).map_err(AccountError::Invalid)?;
    let phc = hash_password(password).await;
    let uid = user_id.to_string();
    store
        .call(move |s| {
            s.set_password_hash(&uid, &phc, must_change)?;
            s.ui_sessions_delete_for_user(&uid)
        })
        .await
        .map(|_| ())
        .map_err(|e| AccountError::Internal(e.to_string()))
}

#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("that username is taken")]
    Taken,
    #[error("internal error: {0}")]
    Internal(String),
}

impl Auth {
    pub fn new(token: Secret<String>, allowed_hosts: Vec<String>, secure_cookie: bool) -> Self {
        Self {
            token,
            allowed_hosts,
            secure_cookie,
            claim: Arc::new(tokio::sync::Mutex::new(None)),
            failures: Arc::default(),
            lookups: Arc::default(),
            jwt: crate::jwt::JwtKey::ephemeral(),
            kv: Arc::new(crate::kv::MemoryKv::default()),
            access_ttl_s: ACCESS_TTL_S,
            refresh_ttl_s: REFRESH_TTL_S,
        }
    }

    /// Sign tokens with `key` (by default a key that dies with the process).
    pub fn with_jwt(self, key: crate::jwt::JwtKey) -> Self {
        Self { jwt: key, ..self }
    }

    /// Keep refresh tokens in `kv` (by default in this process only).
    pub fn with_kv(self, kv: Arc<dyn crate::kv::Kv>) -> Self {
        Self { kv, ..self }
    }

    pub fn with_ttls(self, access_s: i64, refresh_s: i64) -> Self {
        Self {
            access_ttl_s: access_s.max(30),
            refresh_ttl_s: refresh_s.max(access_s.max(30)),
            ..self
        }
    }

    pub fn kv_name(&self) -> &'static str {
        self.kv.name()
    }

    fn refresh_ttl(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.refresh_ttl_s as u64)
    }

    /// New tokens for `user_id`, in login `fam` (a new login when `None`).
    pub async fn issue(
        &self,
        store: &Store,
        user_id: &str,
        fam: Option<String>,
    ) -> Result<Tokens, RefreshError> {
        let uid = user_id.to_string();
        let sv = store
            .call(move |s| s.session_version(&uid))
            .await
            .map_err(|e| RefreshError::Internal(e.to_string()))?;
        let fam = fam.unwrap_or_else(|| random_hex(16));
        let now = now_s();
        let access = Claims::new(user_id, Use::Access, sv, &fam, now, self.access_ttl_s);
        let refresh = Claims::new(user_id, Use::Refresh, sv, &fam, now, self.refresh_ttl_s);
        self.kv
            .set(&refresh_key(&refresh.jti), "live", self.refresh_ttl())
            .await
            .map_err(|e| RefreshError::Internal(e.to_string()))?;
        Ok(Tokens {
            access: self.jwt.sign(&access),
            refresh: self.jwt.sign(&refresh),
        })
    }

    async fn family_revoked(&self, fam: &str) -> Result<bool, crate::kv::KvError> {
        Ok(self.kv.get(&family_key(fam)).await?.is_some())
    }

    /// Revoke a whole login: its refresh tokens and, at once, its access tokens.
    pub async fn revoke_family(&self, fam: &str) {
        if let Err(e) = self
            .kv
            .set(&family_key(fam), "revoked", self.refresh_ttl())
            .await
        {
            tracing::warn!(error = %e, "could not revoke a login");
        }
    }

    /// Trade a refresh token for new tokens (rotation).
    pub async fn refresh(&self, store: &Store, token: &str) -> Result<Tokens, RefreshError> {
        let c: Claims = self.jwt.verify(token).ok_or(RefreshError::Invalid)?;
        if !c.valid(Use::Refresh, now_s()) {
            return Err(RefreshError::Invalid);
        }
        let internal = |e: crate::kv::KvError| RefreshError::Internal(e.to_string());
        if self.family_revoked(&c.fam).await.map_err(internal)? {
            return Err(RefreshError::Invalid);
        }
        let uid = c.sub.clone();
        let current = store
            .call(move |s| s.user_for_token(&uid))
            .await
            .map_err(|e| RefreshError::Internal(e.to_string()))?;
        match current {
            Some((_, sv)) if sv == c.sv => {}
            _ => return Err(RefreshError::Invalid),
        }
        match self
            .kv
            .get(&refresh_key(&c.jti))
            .await
            .map_err(internal)?
            .as_deref()
        {
            Some("live") => {
                self.kv
                    .set(&refresh_key(&c.jti), "rotated", REFRESH_GRACE)
                    .await
                    .map_err(internal)?;
            }
            Some(_) => {} // rotated moments ago: still accepted
            None => {
                self.revoke_family(&c.fam).await;
                let detail = serde_json::json!({ "user_id": c.sub }).to_string();
                let _ = store
                    .call(move |s| s.audit("auth.refresh_reused", &detail))
                    .await;
                return Err(RefreshError::Reused);
            }
        }
        self.issue(store, &c.sub, Some(c.fam)).await
    }

    /// The login family behind a request's tokens, for logout.
    pub fn family_of(&self, headers: &HeaderMap) -> Option<String> {
        let read = |name: &str, want: Use| {
            let t = named_cookie(headers, name)?;
            let c: Claims = self.jwt.verify(&t)?;
            (c.token_use == want).then_some(c.fam)
        };
        read(&access_cookie_name(), Use::Access)
            .or_else(|| read(&refresh_cookie_name(), Use::Refresh))
    }

    fn access_claims(&self, headers: &HeaderMap) -> Option<Claims> {
        let t = named_cookie(headers, &access_cookie_name())?;
        let c: Claims = self.jwt.verify(&t)?;
        c.valid(Use::Access, now_s()).then_some(c)
    }

    /// Is this username currently locked out by repeated failures?
    pub fn login_throttled(&self, username: &str, now_ms: i64) -> bool {
        recent_failures(&self.failures, &username.to_ascii_lowercase(), now_ms)
            >= THROTTLE_MAX_FAILURES
    }

    pub fn record_login_failure(&self, username: &str, now_ms: i64) {
        record_failure(&self.failures, &username.to_ascii_lowercase(), now_ms);
    }

    pub fn clear_login_failures(&self, username: &str) {
        self.failures
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&username.to_ascii_lowercase());
    }

    /// Take one of `caller`'s username lookups, or `false` when they have
    /// used up their failures. Checking and counting happen under one lock,
    /// so a burst of parallel adds cannot all pass before the first failure
    /// is recorded: the taken slot counts as a failure until
    /// `release_lookup` gives it back.
    pub fn try_reserve_lookup(&self, caller: &str, now_ms: i64) -> bool {
        let mut map = self.lookups.lock().unwrap_or_else(|p| p.into_inner());
        let list = map.entry(caller.to_string()).or_default();
        list.retain(|t| now_ms - *t < THROTTLE_WINDOW_MS);
        if list.len() >= LOOKUP_MAX_FAILURES {
            return false;
        }
        list.push(now_ms);
        true
    }

    /// Give back the slot `try_reserve_lookup` took at `now_ms`: the
    /// username existed, or the store could not tell.
    pub fn release_lookup(&self, caller: &str, now_ms: i64) {
        let mut map = self.lookups.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(list) = map.get_mut(caller)
            && let Some(i) = list.iter().position(|t| *t == now_ms)
        {
            list.swap_remove(i);
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

    /// Create the first admin if `code` matches the pending claim. Serialized so two
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
        let user = create_account(store, username, password, Role::Admin, true, false)
            .await
            .map_err(|e| match e {
                AccountError::Invalid(m) => ClaimError::Invalid(m),
                other => ClaimError::Internal(other.to_string()),
            })?;
        let _ = std::fs::remove_file(&claim.path);
        *guard = None;
        Ok(user)
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

    fn cookie(&self, name: &str, value: &str, path: &str, max_age: i64) -> HeaderValue {
        let secure = if self.secure_cookie { "; Secure" } else { "" };
        HeaderValue::from_str(&format!(
            "{name}={value}; HttpOnly; SameSite=Strict; Path={path}; Max-Age={max_age}{secure}"
        ))
        .expect("ascii cookie")
    }

    /// `Set-Cookie` values for a login: the access cookie first.
    pub fn session_cookies(&self, t: &Tokens) -> [HeaderValue; 2] {
        [
            self.cookie(&access_cookie_name(), &t.access, "/", self.access_ttl_s),
            self.cookie(
                &refresh_cookie_name(),
                &t.refresh,
                REFRESH_PATH,
                self.refresh_ttl_s,
            ),
        ]
    }

    /// `Set-Cookie` values that end a login in this browser (the pre-JWT
    /// session cookie too).
    pub fn clear_cookies(&self) -> [HeaderValue; 3] {
        [
            self.cookie(&access_cookie_name(), "", "/", 0),
            self.cookie(&refresh_cookie_name(), "", REFRESH_PATH, 0),
            self.cookie(&cookie_name(), "", "/", 0),
        ]
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

/// The access cookie's value.
pub fn cookie_value(headers: &HeaderMap) -> Option<String> {
    named_cookie(headers, &access_cookie_name())
}

pub fn named_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
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
            .then(Principal::api_token);
    }
    // Signature and expiry first: a forged or stale token costs no lookup.
    let auth = &master.inner.auth;
    let c = auth.access_claims(headers)?;
    // A logged-out login stops at once, not when its token expires.
    match auth.family_revoked(&c.fam).await {
        Ok(false) => {}
        Ok(true) => return None,
        Err(e) => {
            tracing::warn!(error = %e, "cannot check logins; refusing the request");
            return None;
        }
    }
    // The account as it is now: disabled, demoted or logged out everywhere
    // since the token was issued all count.
    let uid = c.sub.clone();
    let (user, sv) = master
        .inner
        .mirror
        .call(move |s| s.user_for_token(&uid))
        .await
        .ok()
        .flatten()?;
    (sv == c.sv).then(|| Principal::from_user(user))
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
    security_headers(resp.headers_mut());
    resp
}

/// The app's CSP, for every response that does not bring its own.
const APP_CSP: &str = "default-src 'self'; connect-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

/// Headers every response gets. A CSP the handler set stays: a stored file
/// is served with its own stricter, sandboxed one.
fn security_headers(h: &mut HeaderMap) {
    h.entry(header::CONTENT_SECURITY_POLICY)
        .or_insert(HeaderValue::from_static(APP_CSP));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_file_is_private_and_stable() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("api-token");
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
    fn throttle_window() {
        let a = Auth::new(Secret::new("t".into()), vec![], false);
        for i in 0..THROTTLE_MAX_FAILURES as i64 {
            assert!(!a.login_throttled("Bob", i));
            a.record_login_failure("bob", i);
        }
        assert!(a.login_throttled("BOB", 10));
        assert!(!a.login_throttled("alice", 10));
        assert!(
            !a.login_throttled("bob", THROTTLE_WINDOW_MS + 10),
            "window expired"
        );
        a.record_login_failure("carol", 0);
        a.clear_login_failures("Carol");
        assert!(!a.login_throttled("carol", 1));
        // Username lookups count per caller, separately from logins.
        for i in 0..LOOKUP_MAX_FAILURES as i64 {
            assert!(a.try_reserve_lookup("u1", i));
        }
        assert!(!a.try_reserve_lookup("u1", 20));
        assert!(!a.login_throttled("u1", 20));
        assert!(a.try_reserve_lookup("u2", 20));
        // A slot given back (the username existed) is free again.
        a.release_lookup("u1", 3);
        assert!(a.try_reserve_lookup("u1", 21));
        assert!(!a.try_reserve_lookup("u1", 22));
        a.release_lookup("u1", 99);
        assert!(!a.try_reserve_lookup("u1", 23), "only taken slots go back");
        assert!(a.try_reserve_lookup("u1", THROTTLE_WINDOW_MS + 30));
    }

    #[test]
    fn visibility_is_own_only() {
        let admin = Principal {
            user_id: Some("a".into()),
            username: "a".into(),
            role: Role::Admin,
            can_agentic: true,
            must_change_password: false,
        };
        assert!(admin.can_see(Some("a")));
        assert!(
            !admin.can_see(Some("b")),
            "admins do not read other people's chats"
        );
        assert!(!admin.can_see(None));
        assert!(Principal::api_token().can_see(Some("b")));
    }

    #[test]
    fn guard_keeps_a_handlers_csp() {
        let mut own = HeaderMap::new();
        own.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'none'; sandbox"),
        );
        security_headers(&mut own);
        assert_eq!(
            own[header::CONTENT_SECURITY_POLICY],
            "default-src 'none'; sandbox"
        );
        assert_eq!(own[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        let mut plain = HeaderMap::new();
        security_headers(&mut plain);
        assert_eq!(plain[header::CONTENT_SECURITY_POLICY], APP_CSP);
        assert_eq!(plain[header::X_FRAME_OPTIONS], "DENY");
    }

    #[test]
    fn cookie_parsing() {
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("a=1; {}=abc; b=2", access_cookie_name())).unwrap(),
        );
        assert_eq!(cookie_value(&h).as_deref(), Some("abc"));
        assert_eq!(named_cookie(&h, "b").as_deref(), Some("2"));
    }
}
