//! Who is calling, settled before anything of the request is read (FIX_PLAN
//! 4.1, `docs/auth-by-construction.md`).
//!
//! [`identify`] is a route layer on every non-public route (`http::private`).
//! It runs after routing and before the handler's first extractor, so a
//! caller who is nobody gets 401 before a path, a query or a body is
//! parsed, and the body is never read. It finds the principal once and
//! leaves it in the request's extensions; the handler takes it with one of
//! the extractors below, which only read it and add the route's own rule:
//!
//! | extractor | who passes | replaces |
//! |---|---|---|
//! | [`Caller`] | any login with its key, or the API token | `require_any` |
//! | [`User`] | the same, with no pending password change | `require` |
//! | [`Admin`] | a [`User`] who is a server admin | `require_admin` |
//! | [`ByteCaller`] | a [`User`], or a live login's cookie with a link minted for this URL (GET and HEAD only) | `require_bytes` |
//!
//! The answers are today's, word for word: 401 "not logged in", 403
//! "change your password first", 403 "admins only".

use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::ApiError;
use crate::Master;
use crate::auth::{self, Principal};

/// What [`identify`] found: the principal, and whether only a link (no
/// login key, no token) vouched for it.
#[derive(Clone)]
struct Identified {
    who: Principal,
    by_link: bool,
}

fn not_logged_in() -> ApiError {
    ApiError(StatusCode::UNAUTHORIZED, "not logged in".into())
}

/// The layer: 401 unless the request carries the API token, a login with
/// its key, or (for a GET or HEAD) a live login's cookie and a link minted
/// for it and this exact URL (`auth::links`, which also limits links to the
/// byte routes).
pub(super) async fn identify(State(m): State<Master>, mut req: Request, next: Next) -> Response {
    let found = match auth::authenticate(&m, req.headers()).await {
        Some(who) => Identified {
            who,
            by_link: false,
        },
        None if matches!(*req.method(), Method::GET | Method::HEAD) => {
            match auth::link_principal(&m, req.headers(), req.uri()).await {
                Some(who) => Identified { who, by_link: true },
                None => return not_logged_in().into_response(),
            }
        }
        None => return not_logged_in().into_response(),
    };
    req.extensions_mut().insert(found);
    next.run(req).await
}

/// The principal [`identify`] left, or 401: a route outside the layer that
/// takes one of these extractors refuses everyone, so the mistake shows at
/// once instead of opening the route.
fn found(parts: &Parts) -> Result<&Identified, ApiError> {
    parts
        .extensions
        .get::<Identified>()
        .ok_or_else(not_logged_in)
}

fn settled(who: &Principal) -> Result<(), ApiError> {
    if who.must_change_password {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "change your password first".into(),
        ));
    }
    Ok(())
}

/// Any logged-in principal, even one that must change its password first.
pub(super) struct Caller(pub Principal);

/// A logged-in principal with no pending password change.
pub(super) struct User(pub Principal);

/// A [`User`] who is a server admin (the API token is one).
// The admin's name is read once handlers that log it convert.
#[allow(dead_code)]
pub(super) struct Admin(pub Principal);

/// A [`User`], or a login vouched for by a link: what serves bytes to an
/// element or a download.
pub(super) struct ByteCaller(pub Principal);

impl<S: Send + Sync> FromRequestParts<S> for Caller {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, ApiError> {
        let f = found(parts)?;
        if f.by_link {
            return Err(not_logged_in());
        }
        Ok(Caller(f.who.clone()))
    }
}

impl<S: Send + Sync> FromRequestParts<S> for User {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, s: &S) -> Result<Self, ApiError> {
        let Caller(who) = Caller::from_request_parts(parts, s).await?;
        settled(&who)?;
        Ok(User(who))
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Admin {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, s: &S) -> Result<Self, ApiError> {
        let User(who) = User::from_request_parts(parts, s).await?;
        if !who.is_admin() {
            return Err(ApiError(StatusCode::FORBIDDEN, "admins only".into()));
        }
        Ok(Admin(who))
    }
}

impl<S: Send + Sync> FromRequestParts<S> for ByteCaller {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, ApiError> {
        let f = found(parts)?;
        settled(&f.who)?;
        Ok(ByteCaller(f.who.clone()))
    }
}
