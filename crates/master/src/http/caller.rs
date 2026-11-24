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

/// The rule of a [`User`]: no pending password change. A handler whose
/// need depends on its body (a chat approval) applies it by hand.
pub(super) fn settled(who: &Principal) -> Result<(), ApiError> {
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
pub(crate) struct ByteCaller(pub Principal);

/// Whether the caller came by a link (no login key, no token), as the layer
/// found it: for a byte route that must tell the two apart (the latest
/// download's redirect carries a fresh link only for a link). It says
/// nothing about who may call; take a [`ByteCaller`] first.
pub(super) struct ViaLink(pub bool);

impl<S: Send + Sync> FromRequestParts<S> for ViaLink {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, ApiError> {
        Ok(ViaLink(found(parts)?.by_link))
    }
}

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
        // A link reads, never writes: the layer takes one only on a GET or
        // HEAD, and so does this, whatever route it is put on.
        if f.by_link && !matches!(parts.method, Method::GET | Method::HEAD) {
            return Err(not_logged_in());
        }
        settled(&f.who)?;
        Ok(ByteCaller(f.who.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(method: Method, by_link: bool) -> Parts {
        let (mut p, ()) = axum::http::Request::builder()
            .method(method)
            .uri("/api/media/m1/thumb")
            .body(())
            .unwrap()
            .into_parts();
        p.extensions.insert(Identified {
            who: Principal::api_token(),
            by_link,
        });
        p
    }

    /// A link passes `ByteCaller` on a GET or HEAD only, even if a write
    /// route took it; the key or the token pass on every method.
    #[tokio::test]
    async fn a_link_passes_the_byte_extractor_only_to_read() {
        for m in [Method::GET, Method::HEAD] {
            assert!(
                ByteCaller::from_request_parts(&mut parts(m, true), &())
                    .await
                    .is_ok()
            );
        }
        for m in [Method::POST, Method::PUT, Method::PATCH, Method::DELETE] {
            let e = ByteCaller::from_request_parts(&mut parts(m.clone(), true), &())
                .await
                .err()
                .expect("refused");
            assert_eq!(e.0, StatusCode::UNAUTHORIZED, "{m}");
            assert!(
                ByteCaller::from_request_parts(&mut parts(m, false), &())
                    .await
                    .is_ok()
            );
        }
        // Nothing found (a route outside the layer): nobody passes.
        let (mut bare, ()) = axum::http::Request::new(()).into_parts();
        assert!(User::from_request_parts(&mut bare, &()).await.is_err());
        // A link is never a `User` or a `Caller`.
        assert!(
            User::from_request_parts(&mut parts(Method::GET, true), &())
                .await
                .is_err()
        );
    }

    /// The layer's own verdict, seen by a handler that only reports what
    /// the layer left (no extractor checks again): a link counts on a GET
    /// or HEAD of a linkable URL with its login's cookie, and nowhere else;
    /// a cookie alone is nobody.
    #[tokio::test]
    async fn the_layer_takes_a_link_only_to_read() {
        use axum::Router;
        use axum::routing::{any, get};
        use harness_core::Secret;
        use harness_store::{NewUser, Role, Store};

        let store = Store::open_in_memory().unwrap();
        let m = Master::new(
            store.clone(),
            auth::Auth::new(Secret::new("t".into()), vec![], false),
        );
        let uid = harness_core::new_id();
        store
            .create_user(&NewUser {
                user_id: uid.clone(),
                username: "mo".into(),
                password_hash: "x".into(),
                role: Role::Member,
                exec_ceiling: harness_core::policy::ExecLevel::None,
                must_change_password: false,
            })
            .unwrap();
        let (cookie, key) = ("a1".repeat(32), "b2".repeat(32));
        let now = harness_core::now_ms();
        store
            .ui_session_create(
                &auth::sha256_hex(&cookie),
                &auth::sha256_hex(&key),
                &uid,
                now,
                now + 3_600_000,
            )
            .unwrap();
        let thumb = "/api/media/m1/thumb";
        let link = m
            .inner
            .auth
            .links()
            .mint(&auth::sha256_hex(&cookie), thumb, now)
            .unwrap();
        async fn probe(req: Request) -> String {
            match req.extensions().get::<Identified>() {
                Some(f) if f.by_link => "link".into(),
                Some(_) => "key".into(),
                None => "nothing".into(),
            }
        }
        let app = Router::new()
            .route("/api/media/{id}/thumb", any(probe))
            .route("/api/me", get(probe))
            .route_layer(axum::middleware::from_fn_with_state(m.clone(), identify))
            .with_state(m);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let c = reqwest::Client::new();
        let jar = format!("{}={cookie}", auth::cookie_name());
        let ask = |m: Method, url: String, with_key: bool| {
            let mut r = c.request(m, format!("{base}{url}")).header("cookie", &jar);
            if with_key {
                r = r.header(auth::LOGIN_HEADER, &key);
            }
            async move {
                let r = r.send().await.unwrap();
                (r.status().as_u16(), r.text().await.unwrap())
            }
        };
        assert_eq!(
            ask(Method::GET, link.clone(), false).await,
            (200, "link".into())
        );
        assert_eq!(ask(Method::HEAD, link.clone(), false).await.0, 200);
        for w in [Method::POST, Method::PUT, Method::PATCH, Method::DELETE] {
            assert_eq!(ask(w.clone(), link.clone(), false).await.0, 401, "{w}");
            assert_eq!(ask(w, thumb.into(), true).await, (200, "key".into()));
        }
        // A cookie alone, and a link carried to a URL it was not made for.
        assert_eq!(ask(Method::GET, thumb.into(), false).await.0, 401);
        let carried = format!("/api/me?{}", &link[link.find('?').unwrap() + 1..]);
        assert_eq!(ask(Method::GET, carried, false).await.0, 401);
        server.abort();
    }
}
