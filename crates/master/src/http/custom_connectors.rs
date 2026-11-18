//! A workspace's custom connectors over HTTP (ADOPTION_PLAN X.35 step 1,
//! PLAN.md T84, README "Custom connectors"; `crate::connectors`):
//!
//! * `GET /api/workspaces/{id}/connectors`: for every member, the
//!   workspace's connectors (name, address, how one signs in, whether a
//!   node announces a server of the same name), whether the caller has
//!   connected an account to each, and what the caller's last check found
//!   (held in memory a few minutes). Never a token.
//! * `POST …/connectors` `{"name", "url", "auth", "header"?}`, `PUT
//!   …/connectors/{cid}` `{"url", "auth", "header"?}`, `DELETE
//!   …/connectors/{cid}`: the workspace's `settings` permission. A new URL
//!   or sign-in removes every account of the connector before anything is
//!   sent to the new address (their tokens were given for the old one).
//!   The name does not change (it is what Shared / Assigned and, later,
//!   the agents' tool names are keyed by).
//! * `PUT …/connectors/{cid}/account` `{"token"}`: connect or replace the
//!   caller's own account: the token is checked by speaking MCP to the
//!   connector with it and stored sealed only if that works.
//! * `DELETE …/connectors/{cid}/account`: disconnect it.
//! * `POST …/connectors/{cid}/check`: see what it offers now, with the
//!   caller's account (or none, for a connector without sign-in).
//!
//! An account is its person's alone: every query names the caller and the
//! workspace, so another member, a workspace or server admin and the API
//! token (which acts as an owner for the connector's definition but has no
//! account, so it can neither connect, check nor disconnect) neither see,
//! use nor delete it. Ids are looked up by (workspace, id); another
//! workspace's id is 404. Bodies are parsed here, so a malformed one gets
//! a fixed message that quotes nothing of it.

use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, put};
use harness_core::Secret;
use harness_store::custom_connectors::{AccountPut, ConnectorAdd, ConnectorRow};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, require};
use crate::connectors::{self, Checked, Offer};
use crate::sealed::Binding;
use crate::workspaces::{Access, Perm, access};
use crate::{Audience, Master, forge};

/// A body is a name, an address and a header name, or a token: far below.
const BODY_LIMIT: usize = 8192;
pub(crate) const TABLE: &str = "connector_accounts";

pub(super) fn routes() -> Router<Master> {
    Router::new()
        .route(
            "/api/workspaces/{id}/connectors",
            get(list)
                .post(create)
                .layer(DefaultBodyLimit::max(BODY_LIMIT)),
        )
        .route(
            "/api/workspaces/{id}/connectors/{cid}",
            put(edit)
                .delete(remove)
                .layer(DefaultBodyLimit::max(BODY_LIMIT)),
        )
        .route(
            "/api/workspaces/{id}/connectors/{cid}/account",
            put(connect)
                .delete(disconnect)
                .layer(DefaultBodyLimit::max(BODY_LIMIT)),
        )
        .route(
            "/api/workspaces/{id}/connectors/{cid}/check",
            axum::routing::post(check).layer(DefaultBodyLimit::max(BODY_LIMIT)),
        )
}

fn err(code: StatusCode, msg: impl Into<String>) -> ApiError {
    ApiError(code, msg.into())
}

fn parse<T: for<'de> Deserialize<'de>>(body: &[u8]) -> ApiResult<T> {
    serde_json::from_slice(body).map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "the body must be JSON with the fields this route takes",
        )
    })
}

/// The caller in workspace `id`, their account id when they have one, and
/// whether they may see the local node's servers (the Plugins rule).
async fn caller(
    m: &Master,
    headers: &HeaderMap,
    id: &str,
) -> ApiResult<(Access, Option<String>, bool)> {
    let who = require(m, headers).await?;
    let a = access(m, &who, id).await?;
    let local_ok = crate::workspaces::may_use_local_node(m, &who);
    Ok((a, who.user_id, local_ok))
}

/// The caller's own account id: an account belongs to a person.
fn person(me: Option<String>) -> ApiResult<String> {
    me.ok_or_else(|| {
        err(
            StatusCode::FORBIDDEN,
            "a connector account belongs to a person; the API token has none",
        )
    })
}

/// Server names the nodes announce, as the Plugins screen shows them to
/// the caller (the local node's only to those who may use it): a clash is
/// told about nothing the caller could not see there.
fn announced(m: &Master, local_ok: bool) -> std::collections::BTreeSet<String> {
    m.announcers()
        .into_iter()
        .filter(|n| local_ok || !m.is_local_node(&n.node_id))
        .flat_map(|n| n.plugins.mcp_servers)
        .collect()
}

/// What a person saw of a connector before sending a token or checking:
/// changes with its URL, its sign-in and its header.
fn stamp(r: &ConnectorRow) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for part in [
        r.url.as_str(),
        r.auth.as_str(),
        r.header.as_deref().unwrap_or(""),
    ] {
        h.update((part.len() as u32).to_be_bytes());
        h.update(part.as_bytes());
    }
    hex::encode(h.finalize())[..16].to_string()
}

/// `seen` must be the connector's [`stamp`] now: else 409, before anything
/// is sent anywhere.
fn same(r: &ConnectorRow, seen: &str) -> ApiResult<()> {
    if stamp(r) == seen {
        Ok(())
    } else {
        Err(err(
            StatusCode::CONFLICT,
            "this connector changed since you opened the page: look again",
        ))
    }
}

fn host_of(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default()
}

fn row_json(r: &ConnectorRow, clash: bool, connected: Option<bool>, offer: Option<Offer>) -> Value {
    json!({
        "id": r.connector_id,
        "name": r.name,
        "url": r.url,
        "host": host_of(&r.url),
        "auth": r.auth,
        "header": r.header.clone().unwrap_or_else(|| "authorization".into()),
        "clash": clash,
        "connected": connected,
        "offer": offer,
        "updated_ms": r.updated_ms,
        "stamp": stamp(r),
    })
}

async fn row(m: &Master, ws: &str, cid: &str) -> ApiResult<ConnectorRow> {
    let (w, c) = (ws.to_string(), cid.to_string());
    m.inner
        .mirror
        .call(move |s| s.connector(&w, &c))
        .await?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such connector in this workspace"))
}

/// Whether a token pasted now could be stored (nothing is made by asking).
async fn available(m: &Master) -> ApiResult<bool> {
    Ok(match m.sealer() {
        Some(s) => s.check(&forge::stored(m).await?).is_ok(),
        None => false,
    })
}

async fn list(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let (a, me, local) = caller(&m, &headers, &id).await?;
    let ws = a.id().to_string();
    let w = ws.clone();
    let rows = m.inner.mirror.call(move |s| s.connectors_of(&w)).await?;
    let mine: Vec<String> = match &me {
        Some(u) => {
            let (u, w) = (u.clone(), ws.clone());
            m.inner
                .mirror
                .call(move |s| s.connector_accounts_of(&u, &w))
                .await?
                .into_iter()
                .map(|r| r.connector_id)
                .collect()
        }
        None => Vec::new(),
    };
    let names = announced(&m, local);
    let list: Vec<Value> = rows
        .iter()
        .map(|r| {
            let connected = (r.auth == "token").then(|| mine.contains(&r.connector_id));
            let offer = me
                .as_deref()
                .and_then(|u| m.connector_offer(u, &r.connector_id));
            row_json(r, names.contains(&r.name), connected, offer)
        })
        .collect();
    Ok(Json(json!({
        "connectors": list,
        "can_edit": a.can(Perm::Settings),
        "person": me.is_some(),
        "available": available(&m).await?,
        "max": connectors::MAX_PER_WORKSPACE,
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    #[serde(default)]
    name: Option<String>,
    url: String,
    auth: String,
    #[serde(default)]
    header: Option<String>,
}

/// The checked (url, auth, header) of a definition.
fn checked(d: &Definition) -> ApiResult<(String, String, Option<String>)> {
    let url = connectors::check_url(&d.url).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    let (auth, header) = match d.auth.as_str() {
        "none" => {
            if d.header.as_deref().is_some_and(|h| !h.trim().is_empty()) {
                return Err(err(
                    StatusCode::BAD_REQUEST,
                    "a connector without sign-in takes no header",
                ));
            }
            ("none", None)
        }
        "token" => {
            let h = match d.header.as_deref().map(str::trim) {
                None | Some("") => None,
                Some(h) => {
                    Some(connectors::check_header(h).map_err(|e| err(StatusCode::BAD_REQUEST, e))?)
                }
            };
            // Authorization is the default: stored as none.
            ("token", h.filter(|h| h != "authorization"))
        }
        "oauth" => {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "sign-in through a browser (OAuth) is not built yet; use a token, or none",
            ));
        }
        _ => {
            return Err(err(StatusCode::BAD_REQUEST, "sign-in is none or token"));
        }
    };
    Ok((url, auth.into(), header))
}

async fn audit(m: &Master, detail: Value) {
    let detail = detail.to_string();
    if let Err(e) = m
        .inner
        .mirror
        .call(move |s| s.audit("custom_connector", &detail))
        .await
    {
        tracing::warn!(error = %e, "could not write a connector audit line");
    }
}

async fn create(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    let (a, me, local) = caller(&m, &headers, &id).await?;
    a.require(Perm::Settings)?;
    let d: Definition = parse(&body)?;
    let name = d.name.clone().unwrap_or_default();
    connectors::check_name(&name).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    let (url, auth, header) = checked(&d)?;
    if announced(&m, local).contains(&name) {
        return Err(err(
            StatusCode::CONFLICT,
            format!("a node of this server has its own MCP server named {name}; pick another name"),
        ));
    }
    let r = ConnectorRow {
        connector_id: harness_core::new_id(),
        workspace_id: a.id().to_string(),
        name: name.clone(),
        url,
        auth,
        header,
        created_ms: 0,
        updated_ms: 0,
    };
    let r2 = r.clone();
    match m
        .inner
        .mirror
        .call(move |s| s.connector_add(&r2, connectors::MAX_PER_WORKSPACE))
        .await?
    {
        ConnectorAdd::Added => {}
        ConnectorAdd::NameTaken => {
            return Err(err(
                StatusCode::CONFLICT,
                format!("this workspace has a connector named {name} already"),
            ));
        }
        ConnectorAdd::OverCap => {
            return Err(err(
                StatusCode::CONFLICT,
                format!(
                    "this workspace has {} connectors, the most it keeps; delete one first",
                    connectors::MAX_PER_WORKSPACE
                ),
            ));
        }
        ConnectorAdd::NoWorkspace => {
            return Err(err(StatusCode::NOT_FOUND, "no such workspace"));
        }
    }
    audit(
        &m,
        json!({"action": "add", "by": me, "workspace": a.id(), "connector": r.connector_id,
               "name": r.name, "host": host_of(&r.url), "auth": r.auth}),
    )
    .await;
    m.changed("connectors", Audience::Workspace(a.id().to_string()));
    let r = row(&m, a.id(), &r.connector_id).await?;
    Ok(Json(row_json(
        &r,
        false,
        (r.auth == "token").then_some(false),
        None,
    )))
}

async fn edit(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, cid)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    let (a, me, local) = caller(&m, &headers, &id).await?;
    a.require(Perm::Settings)?;
    let d: Definition = parse(&body)?;
    let old = row(&m, a.id(), &cid).await?;
    if d.name.as_ref().is_some_and(|n| *n != old.name) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "a connector's name does not change; add it again under the new name",
        ));
    }
    let (url, auth, header) = checked(&d)?;
    let (w, c, u, au, h) = (a.id().to_string(), cid.clone(), url, auth, header);
    let (r, removed) = m
        .inner
        .mirror
        .call(move |s| s.connector_update(&w, &c, &u, &au, h.as_deref()))
        .await?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such connector in this workspace"))?;
    if r.url != old.url || r.auth != old.auth || r.header != old.header {
        m.connector_offer_forget(None, &cid);
    }
    audit(
        &m,
        json!({"action": "edit", "by": me, "workspace": a.id(), "connector": cid,
               "name": r.name, "host": host_of(&r.url), "auth": r.auth,
               "accounts_removed": removed}),
    )
    .await;
    m.changed("connectors", Audience::Workspace(a.id().to_string()));
    let mut v = row_json(&r, announced(&m, local).contains(&r.name), None, None);
    v["disconnected"] = json!(removed);
    Ok(Json(v))
}

async fn remove(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, cid)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let (a, me, _) = caller(&m, &headers, &id).await?;
    a.require(Perm::Settings)?;
    let old = row(&m, a.id(), &cid).await?;
    let (w, c) = (a.id().to_string(), cid.clone());
    if !m
        .inner
        .mirror
        .call(move |s| s.connector_delete(&w, &c))
        .await?
    {
        return Err(err(
            StatusCode::NOT_FOUND,
            "no such connector in this workspace",
        ));
    }
    m.connector_offer_forget(None, &cid);
    audit(
        &m,
        json!({"action": "delete", "by": me, "workspace": a.id(), "connector": cid,
               "name": old.name, "host": host_of(&old.url)}),
    )
    .await;
    m.changed("connectors", Audience::Workspace(a.id().to_string()));
    Ok(Json(json!({"ok": true})))
}

/// What a person sees when the key file needs the admin; the reason goes
/// to the log (it names a path, never a secret).
fn sealing_failed(e: crate::sealed::SealError) -> ApiError {
    tracing::error!(error = %e, "sealed secrets: refused");
    err(
        StatusCode::SERVICE_UNAVAILABLE,
        "this server cannot store or open tokens right now: its key file for stored secrets needs \
         the server's admin (the server log says why)",
    )
}

fn no_sealing() -> ApiError {
    err(
        StatusCode::SERVICE_UNAVAILABLE,
        "this server has no sealed storage set up, so it keeps no tokens",
    )
}

fn binding<'a>(me: &'a str, fields: &'a [&'a str; 3]) -> Binding<'a> {
    Binding {
        table: TABLE,
        owner: me,
        fields,
    }
}

/// Run a check of `r` for `me` with `token`: the limits first; a failure
/// counts against the connector's budget and comes back in fixed words.
async fn run(
    m: &Master,
    me: &str,
    r: &ConnectorRow,
    token: Option<&Secret<String>>,
) -> ApiResult<Offer> {
    m.connector_attempt(me, &r.connector_id)
        .map_err(|e| err(StatusCode::TOO_MANY_REQUESTS, e))?;
    let header = match token {
        Some(t) => Some(
            connectors::token_header(r.header.as_deref(), t)
                .map_err(|e| err(StatusCode::BAD_REQUEST, e.to_string()))?,
        ),
        None => None,
    };
    let checked: Checked = connectors::check(
        m.connector_pool(),
        me,
        &r.name,
        &r.url,
        header,
        &m.connector_net(),
    )
    .await
    .map_err(|b| err(StatusCode::TOO_MANY_REQUESTS, b.to_string()))?;
    m.connector_checked(&checked);
    checked.result.map_err(|f| {
        m.connector_failed(&r.connector_id);
        tracing::info!(connector = %r.connector_id, error = %f, "a connector check failed");
        err(StatusCode::BAD_GATEWAY, format!("{}: {f}", host_of(&r.url)))
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenBody {
    token: String,
    /// The connector's stamp as the person saw it.
    seen: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckBody {
    seen: String,
}

async fn connect(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, cid)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    let (a, me, local) = caller(&m, &headers, &id).await?;
    let me = person(me)?;
    let b: TokenBody = parse(&body)?;
    let token = connectors::clean_token(&b.token).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    let seen = b.seen;
    let r = row(&m, a.id(), &cid).await?;
    same(&r, &seen)?;
    if r.auth != "token" {
        return Err(err(
            StatusCode::CONFLICT,
            "this connector needs no sign-in; it takes no token",
        ));
    }
    // The key now (made if nothing is stored, proven against what is):
    // before the token goes anywhere, so one that could not be stored
    // never leaves the machine.
    let sealer = m.sealer().ok_or_else(no_sealing)?.clone();
    sealer
        .ready(&forge::stored(&m).await?)
        .map_err(sealing_failed)?;
    let offer = run(&m, &me, &r, Some(&token)).await?;
    let st = forge::stored(&m).await?;
    let fields = [
        r.workspace_id.as_str(),
        r.connector_id.as_str(),
        r.url.as_str(),
    ];
    let sealed = sealer
        .seal(&binding(&me, &fields), &token, &st)
        .map_err(sealing_failed)?;
    drop(token);
    let (u, w, c, url) = (
        me.clone(),
        r.workspace_id.clone(),
        r.connector_id.clone(),
        r.url.clone(),
    );
    let put = m
        .inner
        .mirror
        .call(move |s| s.connector_account_put(&u, &w, &c, &url, &sealed, connectors::MAX_ACCOUNTS))
        .await?;
    let action = match put {
        AccountPut::Added => "connect",
        AccountPut::Replaced => "replace",
        AccountPut::OverCap => {
            return Err(err(
                StatusCode::CONFLICT,
                format!(
                    "you have {} connector accounts, the most this server keeps; disconnect one first",
                    connectors::MAX_ACCOUNTS
                ),
            ));
        }
        AccountPut::Changed => {
            return Err(err(
                StatusCode::CONFLICT,
                "the connector changed while the token was checked; nothing was stored, try again",
            ));
        }
        AccountPut::NotMember => {
            return Err(err(StatusCode::NOT_FOUND, "no such workspace"));
        }
    };
    m.connector_offer_put(&me, &r.connector_id, offer.clone());
    audit(
        &m,
        json!({"action": action, "user": me, "workspace": r.workspace_id,
               "connector": r.connector_id, "name": r.name}),
    )
    .await;
    m.changed("connectors", Audience::User(me.clone()));
    Ok(Json(row_json(
        &r,
        announced(&m, local).contains(&r.name),
        Some(true),
        Some(offer),
    )))
}

async fn disconnect(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, cid)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let (a, me, _) = caller(&m, &headers, &id).await?;
    let me = person(me)?;
    let (u, w, c) = (me.clone(), a.id().to_string(), cid.clone());
    if !m
        .inner
        .mirror
        .call(move |s| s.connector_account_delete(&u, &w, &c))
        .await?
    {
        return Err(err(
            StatusCode::NOT_FOUND,
            "you have no account for this connector",
        ));
    }
    m.connector_offer_forget(Some(&me), &cid);
    audit(
        &m,
        json!({"action": "disconnect", "user": me, "workspace": a.id(), "connector": cid}),
    )
    .await;
    m.changed("connectors", Audience::User(me));
    Ok(Json(json!({"ok": true})))
}

async fn check(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, cid)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    let (a, me, local) = caller(&m, &headers, &id).await?;
    let me = person(me)?;
    let b: CheckBody = parse(&body)?;
    let r = row(&m, a.id(), &cid).await?;
    same(&r, &b.seen)?;
    let token = if r.auth == "token" {
        let (u, w, c) = (me.clone(), r.workspace_id.clone(), r.connector_id.clone());
        let blob = m
            .inner
            .mirror
            .call(move |s| s.connector_account_sealed(&u, &w, &c))
            .await?
            .ok_or_else(|| {
                err(
                    StatusCode::CONFLICT,
                    "connect your account first: this connector signs in with a token",
                )
            })?;
        let sealer = m.sealer().ok_or_else(no_sealing)?.clone();
        let st = forge::stored(&m).await?;
        let fields = [
            r.workspace_id.as_str(),
            r.connector_id.as_str(),
            r.url.as_str(),
        ];
        Some(match sealer.open(&binding(&me, &fields), &blob, &st) {
            Ok(t) => t,
            Err(crate::sealed::SealError::Damaged) => {
                return Err(err(
                    StatusCode::CONFLICT,
                    "your stored token for this connector does not open; disconnect and connect \
                     again",
                ));
            }
            Err(e) => return Err(sealing_failed(e)),
        })
    } else {
        None
    };
    let offer = run(&m, &me, &r, token.as_ref()).await?;
    drop(token);
    m.connector_offer_put(&me, &r.connector_id, offer.clone());
    let connected = (r.auth == "token").then_some(true);
    Ok(Json(row_json(
        &r,
        announced(&m, local).contains(&r.name),
        connected,
        Some(offer),
    )))
}
