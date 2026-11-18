//! A person's forge connections over HTTP (ADOPTION_PLAN X.27 step 1,
//! PLAN.md T83, README "Connections"; `crate::forge`, `crate::sealed`):
//!
//! * `GET /api/connections`: the hosts this server allows, with where a
//!   token is made and the scopes later steps need, and the caller's own
//!   connections. Never a token.
//! * `POST /api/connections` `{"host", "token"}`: check the token with the
//!   forge and keep the account with the token sealed; the same account
//!   again replaces its token.
//! * `PUT /api/connections/{id}/token` `{"token"}`: replace the token of
//!   one connection; refused when the new token is another account's.
//! * `DELETE /api/connections/{id}`.
//! * `GET /api/connections/{id}/repositories` (X.27 step 2):
//!   `http/connection_repos.rs`.
//!
//! A connection is its person's alone: every query names the caller, so
//! another user, a server admin and the API token (which has no account)
//! neither see, use nor delete it; an id that is not the caller's is 404.
//! Bodies are parsed here, so a malformed one gets a fixed message that
//! quotes nothing of it.

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, put};
use axum::{Json, Router};
use harness_core::Secret;
use harness_store::connections::{ConnectionPut, ConnectionRow, NewConnection};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, require};
use crate::forge::{self, ForgeHost};
use crate::sealed::Binding;
use crate::{Audience, Master};

/// A body is a host name and a token: far below this.
const BODY_LIMIT: usize = 4096;
const TABLE: &str = "user_connections";

pub(super) fn routes() -> Router<Master> {
    Router::new()
        .route(
            "/api/connections",
            get(list)
                .post(create)
                .layer(DefaultBodyLimit::max(BODY_LIMIT)),
        )
        .route("/api/connections/{id}", axum::routing::delete(remove))
        .route(
            "/api/connections/{id}/token",
            put(replace).layer(DefaultBodyLimit::max(BODY_LIMIT)),
        )
}

pub(super) fn err(code: StatusCode, msg: impl Into<String>) -> ApiError {
    ApiError(code, msg.into())
}

/// The caller's account id: connections belong to a person.
pub(super) async fn person(m: &Master, headers: &HeaderMap) -> ApiResult<String> {
    let who = require(m, headers).await?;
    who.user_id.ok_or_else(|| {
        err(
            StatusCode::FORBIDDEN,
            "connections belong to a person; the API token has none",
        )
    })
}

fn parse<T: for<'de> Deserialize<'de>>(body: &[u8]) -> ApiResult<T> {
    serde_json::from_slice(body).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "the body must be JSON with the fields this route takes",
        )
    })
}

fn host_json(h: &ForgeHost) -> Value {
    let (token_page, scopes) = forge::token_help(h);
    json!({
        "host": h.host,
        "kind": h.kind.name(),
        "builtin": forge::builtins().iter().any(|b| b.host == h.host),
        "token_page": token_page,
        "scopes_needed": scopes,
    })
}

fn row_json(r: &ConnectionRow, hosts: &[ForgeHost]) -> Value {
    let h = hosts.iter().find(|h| h.host == r.host);
    json!({
        "id": r.connection_id,
        "host": r.host,
        "kind": h.map(|h| h.kind.name()),
        "allowed": h.is_some(),
        "account_id": r.account_id,
        "login": r.login,
        "display_name": r.display_name,
        "commit_email": r.commit_email,
        "scopes": r.scopes,
        "scopes_more": r.scopes_more,
        "created_ms": r.created_ms,
        "updated_ms": r.updated_ms,
    })
}

async fn list(State(m): State<Master>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let me = person(&m, &headers).await?;
    let hosts = m.forge_hosts();
    let rows = m.inner.mirror.call(move |s| s.connections_of(&me)).await?;
    // Whether a token pasted now could be stored: the key is there (or can
    // be made) and opens what is stored. Nothing is made by listing.
    let available = match m.sealer() {
        Some(s) => {
            let st = forge::stored(&m).await?;
            s.check(&st).is_ok()
        }
        None => false,
    };
    Ok(Json(json!({
        "hosts": hosts.iter().map(host_json).collect::<Vec<_>>(),
        "connections": rows.iter().map(|r| row_json(r, &hosts)).collect::<Vec<_>>(),
        "max": forge::MAX_PER_USER,
        "available": available,
    })))
}

/// What a person sees when the key file needs the admin; the reason goes
/// to the log (it names a path, never a secret).
pub(super) fn sealing_failed(e: crate::sealed::SealError) -> ApiError {
    tracing::error!(error = %e, "sealed secrets: refused");
    err(
        StatusCode::SERVICE_UNAVAILABLE,
        "this server cannot store or open tokens right now: its key file for stored secrets needs \
         the server's admin (the server log says why)",
    )
}

fn unavailable() -> ApiError {
    err(
        StatusCode::SERVICE_UNAVAILABLE,
        "this server has no sealed storage set up, so it keeps no tokens",
    )
}

/// Get the sealing key now (made if nothing is stored, proven against what
/// is): before a token is sent anywhere, so a paste that could not be
/// stored never leaves the machine.
async fn ready(m: &Master) -> ApiResult<()> {
    let sealer = m.sealer().ok_or_else(unavailable)?.clone();
    let st = forge::stored(m).await?;
    sealer.ready(&st).map_err(sealing_failed)
}

/// Check `token` with `h` for `me`: the attempt limits first, then the
/// forge's answer; a failure counts against the host's budget.
async fn check(
    m: &Master,
    me: &str,
    h: &ForgeHost,
    token: &Secret<String>,
) -> ApiResult<forge::Account> {
    m.forge_attempt(me, &h.host)
        .map_err(|e| err(StatusCode::TOO_MANY_REQUESTS, e))?;
    forge::who_am_i(h, token, m.forge_timeout(), &m.forge_lookup())
        .await
        .map_err(|e| {
            m.forge_failed(&h.host);
            tracing::info!(host = %h.host, error = %e, "a forge token check failed");
            err(StatusCode::BAD_GATEWAY, e.to_string())
        })
}

/// Seal `token` for (`me`, host, account) and store the connection.
async fn store(
    m: &Master,
    me: &str,
    h: &ForgeHost,
    a: forge::Account,
    token: &Secret<String>,
    new_id: String,
) -> ApiResult<ConnectionPut> {
    let sealer = m.sealer().ok_or_else(unavailable)?.clone();
    let st = forge::stored(m).await?;
    let fields = [h.host.as_str(), a.account_id.as_str()];
    let at = Binding {
        table: TABLE,
        owner: me,
        fields: &fields,
    };
    let sealed = sealer.seal(&at, token, &st).map_err(sealing_failed)?;
    let c = NewConnection {
        connection_id: new_id,
        user_id: me.to_string(),
        host: h.host.clone(),
        account_id: a.account_id,
        login: a.login,
        display_name: a.display_name,
        commit_email: a.commit_email,
        scopes: a.scopes,
        scopes_more: a.scopes_more,
        sealed_token: sealed,
    };
    let put = m
        .inner
        .mirror
        .call(move |s| s.connection_put(&c, forge::MAX_PER_USER))
        .await?;
    Ok(put)
}

async fn audit(m: &Master, action: &str, me: &str, host: &str, login: &str) {
    let detail = json!({"action": action, "user": me, "host": host, "login": login}).to_string();
    if let Err(e) = m
        .inner
        .mirror
        .call(move |s| s.audit("forge_connection", &detail))
        .await
    {
        tracing::warn!(error = %e, "could not write a connection audit line");
    }
}

/// The connection `id` of `me`, as the answer shows it.
async fn shown(m: &Master, me: &str, id: &str) -> ApiResult<Json<Value>> {
    let (me, id) = (me.to_string(), id.to_string());
    let r = m
        .inner
        .mirror
        .call(move |s| s.connection(&me, &id))
        .await?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such connection"))?;
    Ok(Json(row_json(&r, &m.forge_hosts())))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateBody {
    host: String,
    token: String,
}

async fn create(
    State(m): State<Master>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    let me = person(&m, &headers).await?;
    let b: CreateBody = parse(&body)?;
    // The exact listed name: no URL, port, case or dot is folded into one.
    let h = m
        .forge_hosts()
        .into_iter()
        .find(|h| h.host == b.host)
        .ok_or_else(|| {
            err(
                StatusCode::BAD_REQUEST,
                "that is not a forge host this server allows (its admin lists them in config.toml)",
            )
        })?;
    let token = forge::clean_token(&b.token).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    drop(b);
    ready(&m).await?;
    let a = check(&m, &me, &h, &token).await?;
    let login = a.login.clone();
    let id = harness_core::new_id();
    match store(&m, &me, &h, a, &token, id).await? {
        ConnectionPut::OverCap => Err(err(
            StatusCode::CONFLICT,
            format!(
                "you have {} connections, the most this server keeps; disconnect one first",
                forge::MAX_PER_USER
            ),
        )),
        ConnectionPut::Added(id) | ConnectionPut::Replaced(id) => {
            audit(&m, "connect", &me, &h.host, &login).await;
            tracing::info!(user = %me, host = %h.host, "forge connection stored");
            m.changed("connections", Audience::User(me.clone()));
            shown(&m, &me, &id).await
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenBody {
    token: String,
}

async fn replace(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    let me = person(&m, &headers).await?;
    let b: TokenBody = parse(&body)?;
    let token = forge::clean_token(&b.token).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    drop(b);
    let (u, i) = (me.clone(), id.clone());
    let row = m
        .inner
        .mirror
        .call(move |s| s.connection(&u, &i))
        .await?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such connection"))?;
    let h = m
        .forge_hosts()
        .into_iter()
        .find(|h| h.host == row.host)
        .ok_or_else(|| {
            err(
                StatusCode::CONFLICT,
                "this server no longer allows that forge host; disconnect it instead",
            )
        })?;
    ready(&m).await?;
    let a = check(&m, &me, &h, &token).await?;
    if a.account_id != row.account_id {
        return Err(err(
            StatusCode::CONFLICT,
            format!(
                "this token belongs to @{}, not @{}; use Connect to add it as another account",
                a.login, row.login
            ),
        ));
    }
    let login = a.login.clone();
    match store(&m, &me, &h, a, &token, id.clone()).await? {
        ConnectionPut::Replaced(id) | ConnectionPut::Added(id) => {
            audit(&m, "replace", &me, &h.host, &login).await;
            m.changed("connections", Audience::User(me.clone()));
            shown(&m, &me, &id).await
        }
        ConnectionPut::OverCap => Err(err(StatusCode::CONFLICT, "no such connection")),
    }
}

async fn remove(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let me = person(&m, &headers).await?;
    let (u, i) = (me.clone(), id.clone());
    let row = m.inner.mirror.call(move |s| s.connection(&u, &i)).await?;
    let (u, i) = (me.clone(), id.clone());
    if !m
        .inner
        .mirror
        .call(move |s| s.connection_delete(&u, &i))
        .await?
    {
        return Err(err(StatusCode::NOT_FOUND, "no such connection"));
    }
    // Its cached listings go with it.
    m.forget_repos(&me, &id);
    if let Some(r) = row {
        audit(&m, "disconnect", &me, &r.host, &r.login).await;
    }
    m.changed("connections", Audience::User(me));
    Ok(Json(json!({"ok": true})))
}
