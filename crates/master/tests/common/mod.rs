//! Shared by the HTTP tests.
// Each test binary uses a part of this.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// The header a browser login's key travels in (Gate 3 P-1).
pub const LOGIN_HEADER: &str = "x-harness-login";

/// Send `Origin` as the page served from the request's own URL would: the
/// server refuses cookie-authenticated writes and WebSockets without one
/// (AUDIT L6). Call it after the URL and before a streaming body; a request
/// that already names an Origin keeps it.
pub trait SameOrigin {
    fn same_origin(self) -> Self;
}

impl SameOrigin for reqwest::RequestBuilder {
    fn same_origin(self) -> Self {
        let Some(req) = self.try_clone().and_then(|b| b.build().ok()) else {
            return self;
        };
        if req.headers().contains_key(reqwest::header::ORIGIN) {
            return self;
        }
        let origin = req.url().origin().ascii_serialization();
        self.header(reqwest::header::ORIGIN, origin)
    }
}

/// The login keys of the browser logins this test binary made, by cookie
/// (`name=value`). A browser login is the cookie and its key (P-1); the
/// tests pass the cookie around and [`Login::login`] adds the key.
static KEYS: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);

/// Record the key a login's answer (`body`) carried for `cookie`.
pub fn remember(cookie: &str, body: &serde_json::Value) {
    if let Some(k) = body["login_key"].as_str() {
        KEYS.lock()
            .unwrap()
            .insert(cookie.to_string(), k.to_string());
    }
}

/// The login key of `cookie`, if a login of this binary made it.
pub fn key_of(cookie: &str) -> Option<String> {
    KEYS.lock().unwrap().get(cookie).cloned()
}

/// The cookie (`name=value`) of a `Set-Cookie`, if the answer set one.
pub fn set_cookie(r: &reqwest::Response) -> Option<String> {
    r.headers()
        .get("set-cookie")
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string())
}

/// The cookie a login's answer set (login, setup, a password change), its
/// key recorded for [`Login::login`], and the body.
pub async fn signed_in_with(r: reqwest::Response) -> (String, serde_json::Value) {
    let cookie = set_cookie(&r).expect("the answer set a session cookie");
    let body: serde_json::Value = r.json().await.unwrap_or(serde_json::Value::Null);
    remember(&cookie, &body);
    (cookie, body)
}

/// [`signed_in_with`] without the body.
pub async fn signed_in(r: reqwest::Response) -> String {
    signed_in_with(r).await.0
}

/// Send a browser login: its cookie and, as the page does, its key.
pub trait Login {
    fn login(self, cookie: &str) -> Self;
}

impl Login for reqwest::RequestBuilder {
    fn login(self, cookie: &str) -> Self {
        let rb = self.header(reqwest::header::COOKIE, cookie);
        match key_of(cookie) {
            Some(k) => rb.header(LOGIN_HEADER, k),
            None => rb,
        }
    }
}

/// The first message a browser's socket sends: its login key (P-1).
/// Empty when `cookie` is no login of this binary.
pub fn socket_login(cookie: &str) -> String {
    serde_json::json!({"type": "login", "key": key_of(cookie).unwrap_or_default()}).to_string()
}
