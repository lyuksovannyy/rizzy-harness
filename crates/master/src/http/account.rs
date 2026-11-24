//! The caller's own account: the first-run claim, login and logout (their
//! routes stay on `public()` in `http.rs`), `/api/me` and the password
//! change. New handlers for the caller's own account go here.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use super::caller::Caller;
use super::{ApiError, ApiResult, workspaces};
use crate::{Master, auth};

pub(super) fn routes() -> Router<Master> {
    Router::new()
        .route("/api/me", get(me))
        .route("/api/me/password", post(change_password))
}

pub(super) async fn setup_status(State(m): State<Master>) -> ApiResult<Json<serde_json::Value>> {
    let users = m.inner.mirror.call(|s| s.user_count()).await?;
    Ok(Json(json!({ "claimed": users > 0 })))
}

#[derive(Deserialize)]
pub(super) struct SetupBody {
    claim_code: String,
    username: String,
    password: String,
}

pub(super) async fn setup(
    State(m): State<Master>,
    Json(b): Json<SetupBody>,
) -> ApiResult<Response> {
    let user = match m
        .inner
        .auth
        .claim(&m.inner.mirror, &b.claim_code, b.username, b.password)
        .await
    {
        Ok(u) => u,
        Err(e) => return Ok(e.into_response()),
    };
    tracing::info!(username = %user.username, "server claimed by owner");
    session_response(&m, &user.user_id).await
}

/// A new login for `user_id`: the session cookie, and the login key in
/// the body, this once (P-1, `auth`). Login, the first-run claim and a
/// password change are the only callers.
async fn session_response(m: &Master, user_id: &str) -> ApiResult<Response> {
    let login = auth::create_ui_session(&m.inner.mirror, user_id).await?;
    Ok((
        [(
            header::SET_COOKIE,
            m.inner.auth.session_cookie(login.cookie.expose()),
        )],
        Json(json!({"ok": true, "login_key": login.key.expose()})),
    )
        .into_response())
}

#[derive(Deserialize)]
pub(super) struct LoginBody {
    username: String,
    password: String,
}

pub(super) async fn login(
    State(m): State<Master>,
    Json(b): Json<LoginBody>,
) -> ApiResult<Response> {
    // Format, throttle, hash permit and verification: auth::Auth::login.
    match m
        .inner
        .auth
        .login(&m.inner.mirror, &b.username, b.password)
        .await
    {
        Ok(user) => session_response(&m, &user.user_id).await,
        Err(e) => Ok(e.into_response()),
    }
}

pub(super) async fn logout(State(m): State<Master>, headers: HeaderMap) -> Response {
    // The session row goes, so a copy of the cookie is useless from now on.
    // The cookie alone is enough (Gate 3 fix review 2, R2-3): a page that
    // lost its key (storage cleared) must be able to end its login, which
    // would otherwise live for 30 days. Ending a login never harms its
    // owner and reveals nothing (the answer is the same whether the cookie
    // was live, dead or made up); the worst a holder of the cookie (another
    // local port's server, P-1) can do is sign the person out, which it
    // could only repeat after they visit it again. It ends that one login,
    // never the person's others. The browser's cookie is cleared either way.
    if let Some(id) = auth::cookie_value(&headers) {
        auth::delete_ui_session(&m, &id).await;
    }
    (
        [(header::SET_COOKIE, m.inner.auth.clear_cookie())],
        Json(json!({"ok": true})),
    )
        .into_response()
}

async fn me(
    Caller(who): Caller,
    State(m): State<Master>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    // The active workspace comes along so the UI can key on it at once.
    let workspace = workspaces::active_summary(&m, &who).await?;
    let body = Json(
        json!({ "ok": true, "app": harness_core::APP_NAME, "user": who, "workspace": workspace }),
    );
    // The server-side expiry slides with use; the browser's copy of the
    // cookie has to slide too, so it is sent again here (the UI asks for
    // /api/me each time it loads).
    Ok(match auth::cookie_value(&headers) {
        Some(id) if who.user_id.is_some() => (
            [(header::SET_COOKIE, m.inner.auth.session_cookie(&id))],
            body,
        )
            .into_response(),
        _ => body.into_response(),
    })
}

#[derive(Deserialize)]
struct PasswordBody {
    current: String,
    new: String,
}

async fn change_password(
    Caller(who): Caller,
    State(m): State<Master>,
    Json(b): Json<PasswordBody>,
) -> ApiResult<Response> {
    let Some(uid) = who.user_id.clone() else {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "the API token has no password".into(),
        ));
    };
    let lookup = uid.clone();
    let phc = m
        .inner
        .mirror
        .call(move |s| s.user_by_id(&lookup))
        .await?
        .map(|u| u.password_hash);
    let hasher = m.inner.auth.hasher();
    // 403, not 401: a 401 means "not logged in", which signs the page out
    // and forgets its login key.
    if !auth::verify_password(hasher, b.current, phc).await? {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "current password is wrong".into(),
        ));
    }
    auth::set_password(&m, &uid, b.new, false).await?;
    // Every session was revoked; give this browser a fresh one.
    session_response(&m, &uid).await
}
