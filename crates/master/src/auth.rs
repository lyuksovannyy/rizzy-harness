//! UI authentication and browser hardening (PLAN.md T1–T3).
//!
//! * A 256-bit token lives in `<data>/ui-token` (0600). It is never logged and
//!   never accepted in a URL.
//! * `POST /api/login {token}` exchanges it for a random session id in an
//!   `HttpOnly; SameSite=Strict` cookie. Session ids are persisted (in the
//!   mirror's kv table) so a restart does not log the UI out.
//! * `Authorization: Bearer <token>` is accepted for scripts and tests.
//! * Every request's `Host` must be allowlisted (DNS-rebinding defence), and
//!   any `Origin` sent must match an allowed host (CSRF / cross-site WS).

use std::io::Write as _;
use std::path::Path;

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

#[derive(Clone)]
pub struct Auth {
    token: Secret<String>,
    /// `host[:port]` values accepted in `Host` and `Origin`.
    allowed_hosts: Vec<String>,
    secure_cookie: bool,
}

pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("OS randomness");
    hex::encode(buf)
}

/// Read the token file, creating it (0600) on first run.
pub fn load_or_create_token(path: &Path) -> std::io::Result<Secret<String>> {
    match std::fs::read_to_string(path) {
        Ok(s) if !s.trim().is_empty() => return Ok(Secret::new(s.trim().to_string())),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let token = random_hex(32);
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

impl Auth {
    pub fn new(token: Secret<String>, allowed_hosts: Vec<String>, secure_cookie: bool) -> Self {
        Self {
            token,
            allowed_hosts,
            secure_cookie,
        }
    }

    pub fn token_matches(&self, candidate: &str) -> bool {
        let a = self.token.expose().as_bytes();
        let b = candidate.as_bytes();
        a.len() == b.len() && bool::from(a.ct_eq(b))
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

fn kv_key(id: &str) -> String {
    format!("ui_session:{id}")
}

pub async fn create_ui_session(store: &Store) -> Result<String, harness_store::StoreError> {
    let id = random_hex(32);
    let key = kv_key(&id);
    store
        .call(move |s| s.kv_set(&key, &harness_core::now_ms().to_string()))
        .await?;
    Ok(id)
}

pub async fn delete_ui_session(store: &Store, id: &str) {
    let key = kv_key(id);
    let _ = store.call(move |s| s.kv_set(&key, "revoked")).await;
}

async fn ui_session_valid(store: &Store, id: &str) -> bool {
    // Session ids are 64 hex chars; reject anything else before touching the DB.
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    let key = kv_key(id);
    matches!(store.call(move |s| s.kv_get(&key)).await, Ok(Some(v)) if v != "revoked")
}

pub async fn is_authenticated(master: &Master, headers: &HeaderMap) -> bool {
    if let Some(bearer) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        return master.inner.auth.token_matches(bearer.trim());
    }
    match cookie_value(headers) {
        Some(id) => ui_session_valid(&master.inner.mirror, &id).await,
        None => false,
    }
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
    fn cookie_parsing() {
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("a=1; {}=abc; b=2", cookie_name())).unwrap(),
        );
        assert_eq!(cookie_value(&h).as_deref(), Some("abc"));
    }
}
