use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use harness_core::SessionId;
use harness_core::policy::{ApprovalMode, Decision};
use serde::Deserialize;
use serde_json::json;

use crate::workspaces::{self as ws, WsError};
use crate::{CommandError, Master, auth};

mod agent_edits;
mod artifacts;
mod hooks;
mod media;
mod projects;
mod providers;
mod routines;
mod storage;
mod workspaces;

pub fn router(master: Master) -> Router {
    let api = Router::new()
        .route("/api/setup", get(setup_status).post(setup))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/auth/refresh", post(refresh_tokens))
        // Under /api/auth the refresh cookie comes along, so logout finds
        // the login even after the access cookie expired.
        .route("/api/auth/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/me/password", post(change_password))
        .route(
            "/api/me/workspace",
            axum::routing::put(workspaces::set_active),
        )
        .route("/api/me/settings", get(get_settings).put(put_settings))
        .route(
            "/api/me/classifier",
            get(get_classifier).put(put_classifier),
        )
        .route("/api/search", get(search))
        .route("/api/favicon", get(crate::favicon::favicon))
        .route("/api/admin/providers", get(providers::get_providers))
        .route(
            "/api/admin/providers/test",
            post(providers::test_provider)
                .put(providers::put_test)
                .delete(providers::delete_test),
        )
        .route(
            "/api/admin/providers/demo",
            post(providers::use_demo)
                .put(providers::put_demo)
                .delete(providers::delete_demo),
        )
        .route(
            "/api/admin/providers/{id}",
            axum::routing::put(providers::put_provider).delete(providers::delete_provider),
        )
        .route(
            "/api/admin/providers/models/test",
            post(providers::test_model),
        )
        .route(
            "/api/admin/providers/{id}/models",
            get(providers::get_models),
        )
        .route(
            "/api/admin/providers/{id}/models/refresh",
            post(providers::refresh_models).delete(providers::delete_refresh),
        )
        .route(
            "/api/admin/providers/{id}/models/{raw}",
            axum::routing::delete(providers::delete_model),
        )
        .route(
            "/api/admin/agent-models",
            axum::routing::put(providers::put_agent_models),
        )
        .route(
            "/api/admin/agents/{id}/avatar",
            axum::routing::put(providers::put_agent_avatar),
        )
        .route("/api/notices", get(notices))
        .route(
            "/api/workspaces",
            get(workspaces::list).post(workspaces::create),
        )
        .route(
            "/api/workspaces/{id}",
            get(workspaces::get_one)
                .patch(workspaces::patch)
                .delete(workspaces::delete),
        )
        .route(
            "/api/workspaces/{id}/memory",
            axum::routing::put(workspaces::set_memory),
        )
        .route(
            "/api/workspaces/{id}/members",
            get(workspaces::members).post(workspaces::add_member),
        )
        .route(
            "/api/workspaces/{id}/members/{user_id}",
            axum::routing::patch(workspaces::set_role).delete(workspaces::remove_member),
        )
        .route("/api/admin/workspaces", get(workspaces::admin_list))
        .route(
            "/api/admin/workspaces/policy",
            axum::routing::put(workspaces::admin_policy),
        )
        .route(
            "/api/admin/workspaces/{id}",
            axum::routing::patch(workspaces::admin_patch).delete(workspaces::admin_delete),
        )
        .route(
            "/api/admin/workspaces/{id}/members",
            get(workspaces::admin_members),
        )
        .route("/api/users", get(list_users).post(create_user))
        .route(
            "/api/users/{id}",
            axum::routing::patch(update_user).delete(delete_user),
        )
        .route("/api/nodes", get(nodes))
        .route("/api/admin/nodes", get(admin_nodes))
        .route("/api/admin/nodes/join", post(node_join_token))
        .route("/api/admin/provision/probe", post(provision_probe))
        .route("/api/admin/node-setups", get(list_setups))
        .route(
            "/api/admin/node-setups/{id}",
            axum::routing::patch(update_setup).delete(forget_setup),
        )
        .route("/api/admin/node-setups/{id}/check", post(check_setup))
        .route("/api/admin/node-setups/{id}/firewall", post(setup_firewall))
        .route("/api/admin/provision", post(provision_start))
        .route("/api/admin/provision/{id}", get(provision_state))
        .route("/api/admin/provision/{id}/answer", post(provision_answer))
        .route("/api/admin/provision/{id}/recheck", post(provision_recheck))
        .route("/api/admin/nodes/{id}", axum::routing::delete(revoke_node))
        .route("/api/models", get(models))
        .route("/api/agents", get(agents).post(agent_edits::create_agent))
        .route(
            "/api/agents/{id}",
            get(agent_edits::get_agent)
                .patch(agent_edits::update_agent)
                .delete(agent_edits::delete_agent),
        )
        .route("/api/agent-summary", post(agent_edits::generate_summary))
        .route(
            "/api/attachments",
            post(upload).layer(axum::extract::DefaultBodyLimit::max(
                crate::attachments::MAX_UPLOAD,
            )),
        )
        .route("/api/memory", get(list_memory).post(add_memory))
        .route("/api/features", get(features))
        .route(
            "/api/transcribe",
            post(transcribe).layer(axum::extract::DefaultBodyLimit::max(
                crate::voice::MAX_AUDIO,
            )),
        )
        .route("/api/admin/memory", get(get_memory_cfg).put(put_memory_cfg))
        .route("/api/admin/savings", get(get_savings).put(put_savings))
        .route("/api/admin/graphify", get(get_graphify).put(put_graphify))
        .route("/api/admin/graphify/install", post(install_graphify))
        .route(
            "/api/memory/{id}",
            axum::routing::patch(update_memory).delete(delete_memory),
        )
        .route("/api/attention", get(attention))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route("/api/sessions/{id}/seen", post(mark_seen))
        .route(
            "/api/sessions/{id}",
            get(get_session).delete(delete_session),
        )
        .route("/api/sessions/{id}/events", get(events))
        .route("/api/sessions/{id}/prompt", post(prompt))
        .route("/api/sessions/{id}/cancel", post(cancel))
        .route("/api/sessions/{id}/agent", post(set_agent))
        .route("/api/sessions/{id}/variants", post(select_variant))
        .route(
            "/api/sessions/{id}/turns/{turn}/restore",
            post(restore_turn),
        )
        .route("/api/sessions/{id}/approvals/{approval_id}", post(approval))
        .route("/api/ws", get(crate::ws::upgrade))
        .merge(projects::routes())
        .merge(routines::routes())
        .merge(hooks::routes())
        .merge(media::routes())
        .merge(artifacts::routes())
        .merge(storage::routes());
    api.fallback(crate::assets::serve)
        .layer(axum::middleware::from_fn_with_state(
            master.clone(),
            auth::guard,
        ))
        .with_state(master)
}

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<CommandError> for ApiError {
    fn from(e: CommandError) -> Self {
        let code = match &e {
            CommandError::UnknownSession => StatusCode::NOT_FOUND,
            CommandError::NodeOffline(_) => StatusCode::SERVICE_UNAVAILABLE,
            CommandError::Timeout => StatusCode::GATEWAY_TIMEOUT,
            CommandError::Rejected(_) => StatusCode::CONFLICT,
            CommandError::Store(_) => StatusCode::INTERNAL_SERVER_ERROR,
            CommandError::UnknownAgent(_) => StatusCode::BAD_REQUEST,
            CommandError::TooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
        };
        ApiError(code, e.to_string())
    }
}

impl From<harness_store::StoreError> for ApiError {
    fn from(e: harness_store::StoreError) -> Self {
        use harness_store::StoreError;
        let code = match &e {
            // Refusals are written for users (a workspace left without an
            // owner, ...); conflicts are uniqueness.
            StoreError::Refused(_) | StoreError::Conflict(_) => StatusCode::CONFLICT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        ApiError(code, e.to_string())
    }
}

impl From<WsError> for ApiError {
    fn from(e: WsError) -> Self {
        match e {
            WsError::NotFound => ApiError(StatusCode::NOT_FOUND, e.to_string()),
            WsError::Forbidden(m) => ApiError(StatusCode::FORBIDDEN, m),
            WsError::Store(e) => e.into(),
        }
    }
}

type ApiResult<T> = Result<T, ApiError>;

/// Any logged-in principal, even one that must change its password first.
async fn require_any(master: &Master, headers: &HeaderMap) -> ApiResult<auth::Principal> {
    auth::authenticate(master, headers)
        .await
        .ok_or_else(|| ApiError(StatusCode::UNAUTHORIZED, "not logged in".into()))
}

/// A logged-in principal with no pending password change.
async fn require(master: &Master, headers: &HeaderMap) -> ApiResult<auth::Principal> {
    let who = require_any(master, headers).await?;
    if who.must_change_password {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "change your password first".into(),
        ));
    }
    Ok(who)
}

async fn require_admin(master: &Master, headers: &HeaderMap) -> ApiResult<auth::Principal> {
    let who = require(master, headers).await?;
    if !who.is_admin() {
        return Err(ApiError(StatusCode::FORBIDDEN, "admins only".into()));
    }
    Ok(who)
}

impl From<auth::AccountError> for ApiError {
    fn from(e: auth::AccountError) -> Self {
        let code = match &e {
            auth::AccountError::Invalid(_) => StatusCode::BAD_REQUEST,
            auth::AccountError::Taken => StatusCode::CONFLICT,
            auth::AccountError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        ApiError(code, e.to_string())
    }
}

/// Resolve a session the principal may see (404 otherwise, so ids of other
/// users' sessions are not confirmed).
async fn visible_session(
    m: &Master,
    who: &auth::Principal,
    id: &str,
) -> ApiResult<harness_store::SessionRow> {
    let sid = parse_sid(id)?;
    match m.inner.mirror.call(move |s| s.session(sid)).await? {
        Some(r) if who.can_see(r.owner_id.as_deref()) => Ok(r),
        _ => Err(ApiError(StatusCode::NOT_FOUND, "unknown session".into())),
    }
}

/// Refuse (403) to make an agentic chat do work when its owner may no
/// longer run agentic sessions there: the account's grant and the
/// workspace's `agentic` permission, checked on every action that starts
/// work, since either can be taken away after the chat was created.
async fn agentic_gate(
    m: &Master,
    who: &auth::Principal,
    row: &harness_store::SessionRow,
) -> ApiResult<()> {
    match ws::agentic_blocked(m, who, row).await {
        Some(why) => Err(ApiError(StatusCode::FORBIDDEN, why)),
        None => Ok(()),
    }
}

fn parse_sid(id: &str) -> ApiResult<SessionId> {
    id.parse()
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "bad session id".into()))
}

async fn setup_status(State(m): State<Master>) -> ApiResult<Json<serde_json::Value>> {
    let users = m.inner.mirror.call(|s| s.user_count()).await?;
    Ok(Json(json!({ "claimed": users > 0 })))
}

#[derive(Deserialize)]
struct SetupBody {
    claim_code: String,
    username: String,
    password: String,
}

async fn setup(State(m): State<Master>, Json(b): Json<SetupBody>) -> ApiResult<Response> {
    use auth::ClaimError;
    let user = m
        .inner
        .auth
        .claim(&m.inner.mirror, &b.claim_code, b.username, b.password)
        .await
        .map_err(|e| {
            let code = match &e {
                ClaimError::AlreadyClaimed => StatusCode::CONFLICT,
                ClaimError::BadCode => StatusCode::FORBIDDEN,
                ClaimError::Invalid(_) => StatusCode::BAD_REQUEST,
                ClaimError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            };
            ApiError(code, e.to_string())
        })?;
    tracing::info!(username = %user.username, "server claimed by owner");
    session_response(&m, &user.user_id).await
}

/// A new login for `user_id`: both token cookies.
async fn session_response(m: &Master, user_id: &str) -> ApiResult<Response> {
    let t = m
        .inner
        .auth
        .issue(&m.inner.mirror, user_id, None)
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(with_cookies(
        &m.inner.auth.session_cookies(&t),
        Json(json!({"ok": true})),
    ))
}

fn with_cookies(cookies: &[axum::http::HeaderValue], body: impl IntoResponse) -> Response {
    let mut r = body.into_response();
    for c in cookies {
        r.headers_mut().append(header::SET_COOKIE, c.clone());
    }
    r
}

/// Rotate the refresh cookie and get a new access cookie. The web client
/// calls this before the access token runs out and after any 401.
async fn refresh_tokens(State(m): State<Master>, headers: HeaderMap) -> Response {
    let auth = &m.inner.auth;
    let Some(token) = auth::named_cookie(&headers, &auth::refresh_cookie_name()) else {
        return ApiError(StatusCode::UNAUTHORIZED, "not logged in".into()).into_response();
    };
    match auth.refresh(&m.inner.mirror, &token).await {
        Ok(t) => with_cookies(&auth.session_cookies(&t), Json(json!({"ok": true}))),
        Err(auth::RefreshError::Internal(e)) => {
            tracing::warn!(error = %e, "refresh failed");
            ApiError(StatusCode::SERVICE_UNAVAILABLE, "try again".into()).into_response()
        }
        Err(e) => with_cookies(
            &auth.clear_cookies(),
            ApiError(StatusCode::UNAUTHORIZED, e.to_string()),
        ),
    }
}

#[derive(Deserialize)]
struct LoginBody {
    username: String,
    password: String,
}

async fn login(State(m): State<Master>, Json(b): Json<LoginBody>) -> ApiResult<Response> {
    let name = b.username.trim().to_ascii_lowercase();
    let now = harness_core::now_ms();
    if m.inner.auth.login_throttled(&name, now) {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "too many failed attempts; try again in 15 minutes".into(),
        ));
    }
    let lookup = name.clone();
    let user = m
        .inner
        .mirror
        .call(move |s| s.user_by_username(&lookup))
        .await?;
    let phc = user.as_ref().map(|u| u.password_hash.clone());
    // Verified even for unknown users, so timing does not reveal usernames;
    // disabled accounts get the same answer as a wrong password.
    let ok = auth::verify_password(b.password, phc).await;
    let user = match user {
        Some(u) if ok && !u.disabled => u,
        _ => {
            m.inner.auth.record_login_failure(&name, now);
            return Err(ApiError(
                StatusCode::UNAUTHORIZED,
                "wrong username or password".into(),
            ));
        }
    };
    m.inner.auth.clear_login_failures(&name);
    session_response(&m, &user.user_id).await
}

async fn logout(State(m): State<Master>, headers: HeaderMap) -> Response {
    // The whole login ends: its refresh token and, at once, its access token.
    if let Some(fam) = m.inner.auth.family_of(&headers) {
        m.inner.auth.revoke_family(&fam).await;
    }
    with_cookies(&m.inner.auth.clear_cookies(), Json(json!({"ok": true})))
}

async fn me(State(m): State<Master>, headers: HeaderMap) -> ApiResult<Json<serde_json::Value>> {
    let who = require_any(&m, &headers).await?;
    // The active workspace comes along so the UI can key on it at once.
    let workspace = workspaces::active_summary(&m, &who).await?;
    Ok(Json(
        json!({ "ok": true, "app": harness_core::APP_NAME, "user": who, "workspace": workspace }),
    ))
}

async fn nodes(State(m): State<Master>, headers: HeaderMap) -> ApiResult<Json<serde_json::Value>> {
    require(&m, &headers).await?;
    Ok(Json(json!({ "nodes": m.nodes() })))
}

/// Which settings a read of models, agents or notices follows.
#[derive(Deserialize)]
struct ScopeQuery {
    /// A workspace the caller is a member of (users: defaults to the
    /// active one; the API token: the server's without it).
    #[serde(default)]
    workspace: Option<String>,
    /// `server`: the server's own values, without any workspace's.
    #[serde(default)]
    scope: Option<String>,
}

/// The workspace whose settings apply, or `None` for the server's own.
async fn settings_scope(
    m: &Master,
    who: &auth::Principal,
    q: &ScopeQuery,
) -> ApiResult<Option<ws::Access>> {
    match q.scope.as_deref() {
        None => Ok(ws::scope(m, who, q.workspace.as_deref()).await?),
        Some("server") if q.workspace.is_none() => Ok(None),
        Some("server") => Err(ApiError(
            StatusCode::BAD_REQUEST,
            "ask for a workspace or for the server scope, not both".into(),
        )),
        Some(_) => Err(ApiError(
            StatusCode::BAD_REQUEST,
            "scope must be server".into(),
        )),
    }
}

async fn scoped_resolver(
    m: &Master,
    who: &auth::Principal,
    q: &ScopeQuery,
) -> ApiResult<crate::Resolver> {
    Ok(match settings_scope(m, who, q).await? {
        Some(a) => a.resolver(m),
        None => m.resolver(None),
    })
}

/// The listed models; `default` is the scope's default model.
async fn models(
    State(m): State<Master>,
    headers: HeaderMap,
    Query(q): Query<ScopeQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let r = scoped_resolver(&m, &who, &q).await?;
    Ok(Json(json!({ "models": r.models() })))
}

/// An agent as the UI lists it in a scope: its definition, the model
/// assigned to it there, the model it runs on and the fallbacks it has
/// there (its own chain's, else the ones it inherits with the default),
/// which of those are unavailable, and its avatar (server-wide).
fn agent_json(m: &Master, r: &crate::Resolver, a: &harness_agents::Agent) -> serde_json::Value {
    let chain = r.agent_chain(&a.id);
    let main = r.agent_main_model(a);
    let fallbacks = r.agent_fallbacks(&a.id, main.as_deref());
    let mut unavailable: Vec<String> = Vec::new();
    for x in main.iter().chain(&fallbacks) {
        if r.model_unavailable(x) && !unavailable.contains(x) {
            unavailable.push(x.clone());
        }
    }
    let mut v = json!(a);
    v["default_model"] = json!(chain.map(|c| c.model));
    v["main_model"] = json!(main);
    v["fallbacks"] = json!(fallbacks);
    v["unavailable"] = json!(unavailable);
    v["avatar"] = json!(m.agent_avatar(&a.id));
    v
}

async fn agents(
    State(m): State<Master>,
    headers: HeaderMap,
    Query(q): Query<ScopeQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let r = scoped_resolver(&m, &who, &q).await?;
    let agents: Vec<serde_json::Value> = r
        .agents()
        .list()
        .iter()
        .map(|a| agent_json(&m, &r, a))
        .collect();
    Ok(Json(
        json!({ "agents": agents, "default": r.default_agent() }),
    ))
}

async fn notices(
    State(m): State<Master>,
    headers: HeaderMap,
    Query(q): Query<ScopeQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let scope = settings_scope(&m, &who, &q).await?;
    Ok(Json(
        json!({ "notices": crate::notices::for_viewer(&m, &who, scope.as_ref()).await }),
    ))
}

#[derive(Deserialize)]
struct PasswordBody {
    current: String,
    new: String,
}

async fn change_password(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<PasswordBody>,
) -> ApiResult<Response> {
    let who = require_any(&m, &headers).await?;
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
    if !auth::verify_password(b.current, phc).await {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "current password is wrong".into(),
        ));
    }
    auth::set_password(&m.inner.mirror, &uid, b.new, false).await?;
    // Every session was revoked; give this browser a fresh one.
    session_response(&m, &uid).await
}

async fn get_settings(
    State(m): State<Master>,
    headers: HeaderMap,
) -> ApiResult<Json<crate::prefs::Prefs>> {
    let who = require(&m, &headers).await?;
    Ok(Json(
        crate::prefs::Prefs::load(&m, who.user_id.as_deref()).await,
    ))
}

async fn put_settings(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(p): Json<crate::prefs::Prefs>,
) -> ApiResult<Json<crate::prefs::Prefs>> {
    let who = require(&m, &headers).await?;
    p.validate(&m)
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))?;
    p.save(&m, who.user_id.as_deref()).await?;
    Ok(Json(p))
}

#[derive(Deserialize)]
struct SearchQuery {
    /// Empty: the newest items.
    #[serde(default)]
    q: String,
    /// `all`, `chats`, `projects`, `media` or `artifacts`; `chats` when left
    /// out (the panel always names it).
    #[serde(default, rename = "in")]
    kind: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    /// The previous page's `next`.
    #[serde(default)]
    cursor: Option<String>,
    /// Similar-chat suggestions for the bar (strict; may return nothing).
    #[serde(default, deserialize_with = "query_flag")]
    suggest: bool,
    /// Users: defaults to the active workspace. The API token: no filter.
    #[serde(default)]
    workspace: Option<String>,
}

/// The search panel (docs/sidebar-search.md): one tab's page, or All. Chats
/// are the caller's own for everyone, admins included (T10); everything is
/// in the one workspace asked for (members only).
async fn search(
    State(m): State<Master>,
    headers: HeaderMap,
    Query(q): Query<SearchQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let scope = ws::scope(&m, &who, q.workspace.as_deref()).await?;
    let kinds = search_kinds(&m, &who, scope.as_ref()).await;
    let owner = who.user_id.clone();
    let workspace = scope.map(|a| a.workspace.workspace_id);
    let text: String = q.q.chars().take(500).collect();
    if q.suggest {
        let results = m
            .inner
            .mirror
            .call(move |s| crate::search::suggest(s, &text, owner.as_deref(), workspace.as_deref()))
            .await?;
        return Ok(Json(
            json!({ "results": results, "next": null, "kinds": kinds }),
        ));
    }
    let asked = q.kind.as_deref().unwrap_or("chats");
    let all = asked == "all";
    if !all && !crate::search::KINDS.contains(&asked) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "in must be all, chats, projects, media or artifacts".into(),
        ));
    }
    let offset = match q.cursor.as_deref() {
        None | Some("") => 0,
        Some(c) => c
            .parse::<u32>()
            .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "bad cursor".into()))?
            as usize,
    };
    // A kind whose area the viewer does not have answers nothing.
    let searched = kinds
        .iter()
        .copied()
        .filter(|k| all || *k == asked)
        .collect();
    let r = crate::search::Request {
        q: text,
        kinds: searched,
        all,
        owner,
        workspace,
        limit: q.limit.unwrap_or(20).clamp(1, 50),
        offset,
    };
    let page = m
        .inner
        .mirror
        .call(move |s| crate::search::run(s, &r))
        .await?;
    Ok(Json(json!({
        "results": page.results,
        "next": page.next.map(|n| n.to_string()),
        "kinds": kinds,
    })))
}

/// The search tabs `who` has in `scope`: chats and projects always, media
/// and artifacts while their areas show (the same flags `/api/features`
/// sends).
async fn search_kinds(
    m: &Master,
    who: &auth::Principal,
    scope: Option<&ws::Access>,
) -> Vec<&'static str> {
    let media = crate::media::features(m, who, scope).await;
    let artifacts = crate::artifacts::features(m, who, scope).await;
    kinds_shown(&media, &artifacts)
}

fn kinds_shown(media: &serde_json::Value, artifacts: &serde_json::Value) -> Vec<&'static str> {
    let mut kinds = vec!["chats", "projects"];
    for (kind, block) in [("media", media), ("artifacts", artifacts)] {
        if block["show"] == true {
            kinds.push(kind);
        }
    }
    kinds
}

/// The bar's mode classifier, per user. Only weights and accuracy windows:
/// no typed text is ever stored.
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassifierState {
    version: u32,
    weights: Vec<f32>,
    bias: Vec<f32>,
    labels: u32,
    /// Last <= 100 outcomes (1 = right) for the rules and for the model.
    rules_window: Vec<u8>,
    model_window: Vec<u8>,
}

const CLASSIFIER_BUCKETS: usize = 4096;
const CLASSIFIER_CLASSES: usize = 3;

impl ClassifierState {
    fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("unsupported classifier version".into());
        }
        if self.weights.len() != CLASSIFIER_BUCKETS * CLASSIFIER_CLASSES
            || self.bias.len() != CLASSIFIER_CLASSES
        {
            return Err("classifier weights have the wrong shape".into());
        }
        if !self
            .weights
            .iter()
            .chain(&self.bias)
            .all(|w| w.is_finite() && w.abs() < 1e4)
        {
            return Err("classifier weights must be finite and bounded".into());
        }
        for w in [&self.rules_window, &self.model_window] {
            if w.len() > 100 || w.iter().any(|x| *x > 1) {
                return Err("accuracy windows hold at most 100 values of 0 or 1".into());
            }
        }
        Ok(())
    }
}

fn classifier_key(who: &auth::Principal) -> String {
    format!(
        "classifier:{}",
        who.user_id.as_deref().unwrap_or("api-token")
    )
}

async fn get_classifier(
    State(m): State<Master>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let key = classifier_key(&who);
    let v = m.inner.mirror.call(move |s| s.kv_get(&key)).await?;
    Ok(Json(
        match v.and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok()) {
            Some(state) => state,
            None => serde_json::Value::Null,
        },
    ))
}

async fn put_classifier(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(state): Json<ClassifierState>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    state
        .validate()
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))?;
    let (key, v) = (
        classifier_key(&who),
        serde_json::to_string(&state).expect("serialize"),
    );
    m.inner.mirror.call(move |s| s.kv_set(&key, &v)).await?;
    Ok(Json(json!({ "ok": true })))
}

fn runtime(m: &Master) -> ApiResult<std::sync::Arc<crate::providers::ProviderRuntime>> {
    m.provider_runtime().ok_or_else(|| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "provider is not managed by this server".into(),
        )
    })
}

fn user_json(u: &harness_store::UserRow) -> serde_json::Value {
    json!({
        "user_id": u.user_id,
        "username": u.username,
        "role": u.role,
        "disabled": u.disabled,
        "can_agentic": u.role == harness_store::Role::Admin || u.can_agentic,
        "must_change_password": u.must_change_password,
        "created_ms": u.created_ms,
    })
}

async fn list_users(
    State(m): State<Master>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    let users = m.inner.mirror.call(|s| s.list_users()).await?;
    Ok(Json(
        json!({ "users": users.iter().map(user_json).collect::<Vec<_>>() }),
    ))
}

#[derive(Deserialize)]
struct NewUserBody {
    username: String,
    /// Temporary; the user must change it at first login.
    password: String,
    #[serde(default = "member")]
    role: harness_store::Role,
    #[serde(default)]
    can_agentic: bool,
}

fn member() -> harness_store::Role {
    harness_store::Role::Member
}

async fn create_user(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<NewUserBody>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    let u = auth::create_account(
        &m.inner.mirror,
        b.username.trim().to_ascii_lowercase(),
        b.password,
        b.role,
        b.can_agentic,
        true,
    )
    .await?;
    Ok(Json(user_json(&u)))
}

#[derive(Deserialize)]
struct UserPatchBody {
    #[serde(default)]
    role: Option<harness_store::Role>,
    #[serde(default)]
    disabled: Option<bool>,
    #[serde(default)]
    can_agentic: Option<bool>,
    /// New temporary password; forces a change at next login.
    #[serde(default)]
    password: Option<String>,
}

/// Refuse changes that would lock everyone out or edit your own access.
async fn guard_admin_change(
    m: &Master,
    who: &auth::Principal,
    target: &harness_store::UserRow,
    removes_admin: bool,
) -> ApiResult<()> {
    if who.user_id.as_deref() == Some(target.user_id.as_str()) {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "you cannot change your own role or access, or delete yourself".into(),
        ));
    }
    if removes_admin && target.role == harness_store::Role::Admin && !target.disabled {
        let admins = m.inner.mirror.call(|s| s.active_admin_count()).await?;
        if admins <= 1 {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "this is the last active admin".into(),
            ));
        }
    }
    Ok(())
}

async fn load_user(m: &Master, id: &str) -> ApiResult<harness_store::UserRow> {
    let id = id.to_string();
    m.inner
        .mirror
        .call(move |s| s.user_by_id(&id))
        .await?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "unknown user".into()))
}

async fn update_user(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<UserPatchBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require_admin(&m, &headers).await?;
    let target = load_user(&m, &id).await?;
    let changes_access = b.role.is_some() || b.disabled.is_some() || b.can_agentic.is_some();
    if changes_access {
        let removes_admin = b.disabled == Some(true) || b.role == Some(harness_store::Role::Member);
        guard_admin_change(&m, &who, &target, removes_admin).await?;
        let patch = harness_store::UserPatch {
            role: b.role,
            disabled: b.disabled,
            can_agentic: b.can_agentic,
        };
        let uid = target.user_id.clone();
        let disable = b.disabled == Some(true);
        m.inner
            .mirror
            .call(move |s| {
                s.update_user(&uid, &patch)?;
                if disable {
                    s.ui_sessions_delete_for_user(&uid)?;
                }
                Ok::<_, harness_store::StoreError>(())
            })
            .await?;
        // Disabled owners stop counting as owners, and whether the user may
        // drive their agentic chats (`blocked`) may have changed: running
        // ones that lost their access stop.
        let _ = m.inner.live.send(crate::Live::WorkspacesChanged);
        let _ = m.inner.live.send(crate::Live::SessionsChanged);
        ws::sweep_revoked(&m, vec![target.user_id.clone()]);
    }
    if let Some(pw) = b.password {
        auth::set_password(&m.inner.mirror, &target.user_id, pw, true).await?;
    }
    Ok(Json(user_json(&load_user(&m, &id).await?)))
}

async fn delete_user(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require_admin(&m, &headers).await?;
    let target = load_user(&m, &id).await?;
    guard_admin_change(&m, &who, &target, true).await?;
    let uid = target.user_id.clone();
    // Refused (409) while they are the only owner of a team workspace with
    // other members; their chats leave every team workspace. The projects of
    // the workspaces that go with them go too, and their facts.
    let projects = crate::projects::ids(&m).await;
    m.inner.mirror.call(move |s| s.delete_user(&uid)).await?;
    // Their stored files lost their rows with the account; unlink them.
    crate::storage::on_user_deleted(&m).await;
    crate::projects::forget_gone(&m, projects).await;
    let _ = m.inner.live.send(crate::Live::WorkspacesChanged);
    let _ = m.inner.live.send(crate::Live::SessionsChanged);
    // Their running agentic chats stop (the account is gone).
    ws::sweep_revoked(&m, vec![target.user_id]);
    Ok(Json(json!({ "ok": true })))
}

/// A chat as the UI lists it. `blocked`: why the caller may not make this
/// agentic chat do work (always `null` for chats).
fn session_json(r: &harness_store::SessionRow, blocked: Option<&str>) -> serde_json::Value {
    let blocked = blocked.filter(|_| r.profile.as_deref() != Some("chat"));
    json!({
        "session_id": r.session_id,
        "node_id": r.node_id,
        "title": r.title,
        "cwd": r.cwd,
        "model": r.model,
        "created_ms": r.created_ms,
        "updated_ms": r.updated_ms,
        "head_seq": r.head_seq,
        "running": r.running,
        "owner_id": r.owner_id,
        "agent_id": r.agent_id,
        "profile": r.profile,
        "incognito": r.incognito,
        "parent_id": r.parent_id,
        "workspace_id": r.workspace_id,
        "blocked": blocked,
    })
}

#[derive(Deserialize)]
struct WorkspaceQuery {
    /// Users: defaults to the active workspace. The API token: no filter.
    #[serde(default)]
    workspace: Option<String>,
}

/// A query flag: `1` or `true` (links and the web app send `=1`).
fn query_flag<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    match String::deserialize(d)?.as_str() {
        "1" | "true" => Ok(true),
        "0" | "false" | "" => Ok(false),
        _ => Err(serde::de::Error::custom("expected 1 or 0")),
    }
}

#[derive(Deserialize)]
struct SessionsQuery {
    #[serde(default)]
    workspace: Option<String>,
    /// Also the routine run chats the list leaves out once seen.
    #[serde(default, deserialize_with = "query_flag")]
    routine_runs: bool,
}

/// The caller's own chats in one workspace (the API token: every chat,
/// optionally in one workspace). The workspace is an extra filter: other
/// members' chats stay private (T10).
async fn list_sessions(
    State(m): State<Master>,
    headers: HeaderMap,
    Query(q): Query<SessionsQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let scope = ws::scope(&m, &who, q.workspace.as_deref()).await?;
    // Every listed chat is in the scope's workspace, so one answer fits all.
    let blocked = ws::agentic_refusal(&who, scope.as_ref());
    let in_scope = |r: &&harness_store::SessionRow| match &scope {
        Some(a) => r.workspace_id.as_deref() == Some(a.id()),
        None => true,
    };
    let rows = m.inner.mirror.call(|s| s.sessions()).await?;
    let links = crate::projects::links(&m).await?;
    let mut list: Vec<serde_json::Value> = rows
        .iter()
        .filter(|r| who.can_see(r.owner_id.as_deref()))
        .filter(in_scope)
        .map(|r| {
            let mut v = session_json(r, blocked.as_deref());
            v["project_id"] = json!(crate::projects::effective(&links, r));
            v
        })
        .collect();
    crate::routines::annotate(&m, &mut list, q.routine_runs).await?;
    Ok(Json(json!({ "sessions": list })))
}

/// Any of the caller's chats, whatever its workspace (the UI follows a
/// link to a chat into its workspace).
async fn get_session(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let row = visible_session(&m, &who, &id).await?;
    let blocked = ws::agentic_blocked(&m, &who, &row).await;
    let mut v = session_json(&row, blocked.as_deref());
    v["project_id"] = json!(crate::projects::effective(
        &crate::projects::links(&m).await?,
        &row
    ));
    crate::routines::annotate_one(&m, &mut v).await?;
    Ok(Json(v))
}

#[derive(Deserialize)]
struct DeleteSessionQuery {
    /// Also delete the caller's Media items that came from this chat.
    #[serde(default, deserialize_with = "query_flag")]
    media: bool,
}

async fn delete_session(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<DeleteSessionQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let sid = visible_session(&m, &who, &id).await?.session_id;
    // Before the chat goes, while its items still name it.
    if q.media {
        crate::media::on_chat_deleted(&m, &who, sid).await?;
    }
    m.delete_session(sid).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct EventsQuery {
    #[serde(default)]
    after: u64,
    #[serde(default)]
    limit: Option<usize>,
}

async fn events(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<EventsQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let sid = visible_session(&m, &who, &id).await?.session_id;
    let limit = q.limit.unwrap_or(1000).min(5000);
    let evs = m
        .inner
        .mirror
        .call(move |s| s.events_after(sid, q.after, limit))
        .await?;
    Ok(Json(json!({ "events": evs })))
}

#[derive(Deserialize)]
struct CreateBody {
    #[serde(default)]
    title: Option<String>,
    /// Required for agentic sessions; ignored for chat.
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    profile: Option<harness_core::event::Profile>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    approval_mode: Option<ApprovalMode>,
    #[serde(default)]
    incognito: bool,
    /// Users: defaults to the active workspace; members only. The API
    /// token: any existing workspace, or none.
    #[serde(default)]
    workspace_id: Option<String>,
    /// Start the chat in this project (docs/projects.md).
    #[serde(default)]
    project_id: Option<String>,
}

async fn create_session(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(mut b): Json<CreateBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    if b.incognito && b.profile != Some(harness_core::event::Profile::Chat) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "incognito is only available for chats".into(),
        ));
    }
    let scope = ws::scope(&m, &who, b.workspace_id.as_deref()).await?;
    // Agentic sessions run shell commands on a worker node: they need the
    // account's grant and the workspace's permission.
    if b.profile.unwrap_or(harness_core::event::Profile::Agentic)
        == harness_core::event::Profile::Agentic
        && let Some(why) = ws::agentic_refusal(&who, scope.as_ref())
    {
        return Err(ApiError(StatusCode::FORBIDDEN, why));
    }
    let project = projects::for_new_chat(&m, &who, &mut b, scope.as_ref()).await?;
    let title = b
        .title
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| "New session".into());
    // The master stamps owner, workspace and project once the node has the
    // chat.
    let sid = m
        .create_session(crate::CreateSpec {
            profile: b.profile,
            node_id: b.node_id,
            title,
            cwd: b.cwd,
            model: b.model,
            approval_mode: b.approval_mode,
            agent_id: b.agent_id,
            incognito: b.incognito,
            owner_id: who.user_id.clone(),
            workspace_id: scope.map(|a| a.workspace.workspace_id),
            project_id: project,
        })
        .await?;
    Ok(Json(json!({ "session_id": sid })))
}

#[derive(Deserialize)]
struct PromptBody {
    text: String,
    /// Run this turn as another agent (mention popup); the session's agent
    /// stays unchanged.
    #[serde(default)]
    agent_id: Option<String>,
    /// Reasoning effort for this message only (if the model supports it).
    #[serde(default)]
    effort: Option<String>,
    /// Upload ids from `POST /api/attachments`; consumed by this message.
    #[serde(default)]
    attachments: Vec<String>,
    /// Parallel answers: model ids (2-4).
    #[serde(default)]
    compare: Vec<String>,
    /// An answer to a choices or form block; `text` is then an optional
    /// note.
    #[serde(default)]
    reply: Option<harness_core::event::FormReply>,
    /// Media item ids of images for this message (`media::prompt_images`).
    #[serde(default)]
    images: Vec<String>,
}

#[derive(Deserialize)]
struct UploadQuery {
    name: String,
}

/// Raw file body, name in the query. Returns the extracted size for the chip.
async fn upload(
    State(m): State<Master>,
    headers: HeaderMap,
    Query(q): Query<UploadQuery>,
    body: axum::body::Bytes,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let a = crate::attachments::extract(&q.name, body.to_vec())
        .await
        .map_err(|e| ApiError(StatusCode::UNPROCESSABLE_ENTITY, e))?;
    let id = harness_core::new_id();
    let (aid, owner, att) = (id.clone(), who.user_id.clone(), a.clone());
    m.inner
        .mirror
        .call(move |s| s.attachment_put(&aid, owner.as_deref(), &att))
        .await?;
    Ok(Json(json!({
        "attachment_id": id,
        "name": a.name,
        "mime": a.mime,
        "chars": a.text.chars().count(),
        "truncated": a.truncated,
    })))
}

async fn prompt(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<PromptBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    if b.text.trim().is_empty() && b.reply.is_none() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "empty prompt".into()));
    }
    if let Some(r) = &b.reply {
        check_reply_body(r, &b)?;
    }
    if b.attachments.len() > crate::attachments::MAX_PER_MESSAGE {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            format!(
                "at most {} attachments per message",
                crate::attachments::MAX_PER_MESSAGE
            ),
        ));
    }
    let row = visible_session(&m, &who, &id).await?;
    // Before the uploads are taken, so a refusal leaves them for later.
    agentic_gate(&m, &who, &row).await?;
    let images = crate::media::prompt_images(&m, &who, &row, &b.images)
        .await
        .map_err(|(c, e)| ApiError(c, e))?;
    // A chat a routine started from an external event stays untrusted for
    // every turn the owner sends there.
    let flags = crate::routines::on_owner_prompt(&m, row.session_id).await;
    let sid = row.session_id;
    let attachments = if b.attachments.is_empty() {
        Vec::new()
    } else {
        let (ids, owner) = (b.attachments.clone(), who.user_id.clone());
        m.inner
            .mirror
            .call(move |s| s.attachments_take(&ids, owner.as_deref()))
            .await?
            .ok_or_else(|| {
                ApiError(
                    StatusCode::BAD_REQUEST,
                    "unknown or already used attachment; upload it again".into(),
                )
            })?
    };
    m.prompt(
        sid,
        b.text,
        crate::PromptOpts {
            agent_id: b.agent_id,
            effort: b.effort,
            context: Some(
                crate::prefs::Prefs::load(&m, who.user_id.as_deref())
                    .await
                    .model_context(&who.username, jiff::Timestamp::now()),
            ),
            attachments,
            compare: b.compare,
            reply: b.reply,
            untrusted: flags.untrusted,
            images,
        },
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

/// What an answer may come with. Whether it answers anything is the node's
/// to say, against its log; this only keeps the request plain.
fn check_reply_body(r: &harness_core::event::FormReply, b: &PromptBody) -> ApiResult<()> {
    let bad = |m: &str| Err(ApiError(StatusCode::BAD_REQUEST, m.into()));
    if !b.attachments.is_empty() {
        return bad("an answer cannot carry attachments");
    }
    if !b.compare.is_empty() {
        return bad("an answer cannot ask for parallel answers");
    }
    if !b.images.is_empty() {
        return bad("an answer cannot carry images");
    }
    if harness_core::ui::too_large(r) {
        return bad(harness_core::ui::TOO_LARGE);
    }
    Ok(())
}

async fn cancel(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let sid = visible_session(&m, &who, &id).await?.session_id;
    m.cancel(sid).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct AgentBody {
    agent_id: String,
}

async fn set_agent(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<AgentBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let sid = visible_session(&m, &who, &id).await?.session_id;
    m.set_agent(sid, &b.agent_id).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ApprovalBody {
    decision: Decision,
}

async fn approval(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, approval_id)): Path<(String, String)>,
    Json(b): Json<ApprovalBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let row = visible_session(&m, &who, &id).await?;
    // Allowing makes the chat do work; denying stops it, like cancel, so a
    // user who lost access can still refuse.
    if b.decision.allows() {
        agentic_gate(&m, &who, &row).await?;
    }
    m.respond_approval(row.session_id, approval_id, b.decision)
        .await?;
    Ok(Json(json!({ "ok": true })))
}

fn memory_store(m: &Master) -> ApiResult<std::sync::Arc<dyn harness_memory::MemoryStore>> {
    m.memory().ok_or_else(|| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "memory is not set up on this server".into(),
        )
    })
}

fn memory_err(e: harness_memory::MemoryError) -> ApiError {
    match e {
        harness_memory::MemoryError::Invalid(msg) => ApiError(StatusCode::BAD_REQUEST, msg),
        other => ApiError(StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
    }
}

async fn list_memory(
    State(m): State<Master>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let mem = memory_store(&m)?;
    let facts = mem.list(who.user_id.as_deref()).await.map_err(memory_err)?;
    Ok(Json(json!({ "memories": facts, "backend": mem.name() })))
}

#[derive(Deserialize)]
struct NewMemoryBody {
    text: String,
    #[serde(default)]
    kind: harness_memory::FactKind,
}

/// Added by the user directly: active at once.
async fn add_memory(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<NewMemoryBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let mem = memory_store(&m)?;
    let f = mem
        .add(harness_memory::NewFact {
            owner_id: who.user_id.clone(),
            text: b.text,
            kind: b.kind,
            status: harness_memory::Status::Active,
            source_session: None,
            dedupe_key: None,
        })
        .await
        .map_err(memory_err)?;
    let _ = m.inner.live.send(crate::Live::MemoryChanged);
    Ok(Json(json!(f)))
}

/// The fact, if it belongs to the caller (404 otherwise, never 403).
async fn own_fact(
    m: &Master,
    who: &auth::Principal,
    id: &str,
) -> ApiResult<(
    std::sync::Arc<dyn harness_memory::MemoryStore>,
    harness_memory::Fact,
)> {
    let mem = memory_store(m)?;
    match mem.get(id).await.map_err(memory_err)? {
        Some(f) if f.owner_id == who.user_id => Ok((mem, f)),
        _ => Err(ApiError(StatusCode::NOT_FOUND, "no such memory".into())),
    }
}

#[derive(Deserialize)]
struct MemoryPatch {
    #[serde(default)]
    text: Option<String>,
    /// `active` confirms a proposal.
    #[serde(default)]
    status: Option<harness_memory::Status>,
}

async fn update_memory(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<MemoryPatch>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let (mem, _) = own_fact(&m, &who, &id).await?;
    let f = mem
        .update(&id, b.text, b.status)
        .await
        .map_err(memory_err)?;
    let _ = m.inner.live.send(crate::Live::MemoryChanged);
    Ok(Json(json!(f)))
}

async fn delete_memory(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let (mem, _) = own_fact(&m, &who, &id).await?;
    mem.delete(&id).await.map_err(memory_err)?;
    let _ = m.inner.live.send(crate::Live::MemoryChanged);
    Ok(Json(json!({ "ok": true })))
}

fn memory_cfg_json(m: &Master, cfg: &crate::config::Config) -> serde_json::Value {
    use crate::config::{EmbedSource, ProviderKind};
    use harness_memory::Preset;
    let mc = &cfg.memory;
    // Until a provider is chosen, suggest the default model's provider (else
    // the first real one), or Ollama when there is none.
    let suggested = || {
        let default = cfg.effective_default();
        let by_default = default
            .as_deref()
            .and_then(|d| cfg.resolve_model(d))
            .map(|(p, _)| p);
        by_default
            .into_iter()
            .chain(&cfg.providers)
            .find(|p| p.kind != ProviderKind::Mock)
    };
    let (source, provider) = match mc.source {
        EmbedSource::Provider => match cfg.embed_provider().or_else(suggested) {
            Some(p) => (EmbedSource::Provider, Some(p)),
            None => (EmbedSource::Ollama, None),
        },
        EmbedSource::Ollama => (EmbedSource::Ollama, None),
    };
    let (url, model) = match source {
        EmbedSource::Ollama => (mc.ollama_url(), mc.model()),
        EmbedSource::Provider => (mc.embeddings_url.as_str(), mc.embeddings_model.clone()),
    };
    let presets: Vec<serde_json::Value> = [Preset::Fast, Preset::Balanced, Preset::Best]
        .iter()
        .map(|p| {
            json!({
                "id": p,
                "model": p.ollama_model(),
                "size": match p { Preset::Fast => "~50 MB", Preset::Balanced => "~280 MB", Preset::Best => "~670 MB" },
            })
        })
        .collect();
    json!({
        "backend": mc.backend,
        "recall": mc.recall,
        "source": source,
        "provider_id": provider.map(|p| &p.id),
        "embeddings_url": url,
        "embeddings_model": mc.embeddings_model,
        "preset": mc.preset,
        "model": model,
        "active_model": m.builtin_memory().and_then(|b| b.embedder_model()),
        "presets": presets,
        // Settings `.env` fixes: shown read-only.
        "locked": m.provider_runtime().map(|rt| rt.env().locked_under("memory.")).unwrap_or_default(),
    })
}

/// Rebuild the live embedder after a provider change, when memory takes
/// its embeddings from a provider (its URL or key may have changed).
fn refresh_embedder(m: &Master, rt: &crate::providers::ProviderRuntime) {
    let cfg = rt.config();
    if cfg.memory.source != crate::config::EmbedSource::Provider {
        return;
    }
    let Some(bm) = m.builtin_memory() else { return };
    match crate::providers::embedder(&cfg, rt.data_dir(), &rt.secrets) {
        Ok(e) => bm.set_embedder(e),
        Err(e) => {
            tracing::warn!("memory embeddings off: {e:#}");
            bm.set_embedder(None);
        }
    }
}

async fn get_memory_cfg(
    State(m): State<Master>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    Ok(Json(memory_cfg_json(&m, &runtime(&m)?.config())))
}

#[derive(Deserialize)]
struct MemoryCfgBody {
    recall: crate::config::Recall,
    /// Required for meaning recall.
    #[serde(default)]
    source: Option<crate::config::EmbedSource>,
    #[serde(default)]
    provider_id: Option<String>,
    /// Ollama source only.
    #[serde(default)]
    embeddings_url: Option<String>,
    #[serde(default)]
    embeddings_model: Option<String>,
    #[serde(default)]
    preset: Option<harness_memory::Preset>,
}

/// A failed test embedding, worded for the admin.
fn embed_failure(
    cfg: &crate::config::Config,
    e: &harness_memory::Embedder,
    err: harness_memory::MemoryError,
) -> ApiError {
    use harness_memory::MemoryError;
    let who = match cfg.embed_provider() {
        Some(p) if cfg.memory.source == crate::config::EmbedSource::Provider => p.name.clone(),
        _ => "The endpoint".to_string(),
    };
    let msg = match &err {
        // Ollama answers 404 for a missing model too, and says so.
        MemoryError::EmbedHttp {
            status: status @ (404 | 405),
            detail,
        } if !detail.to_lowercase().contains("model") => {
            let detail = if detail.is_empty() {
                String::new()
            } else {
                format!(": {detail}")
            };
            format!(
                "{who} has no embeddings endpoint (POST {}/embeddings answered {status}{detail})",
                e.base_url()
            )
        }
        MemoryError::EmbedHttp {
            status: 401 | 403, ..
        } => format!("{err} (check the API key)"),
        MemoryError::EmbedHttp { .. } => {
            format!("{err} (is the model `{}` available there?)", e.model)
        }
        _ => err.to_string(),
    };
    ApiError(StatusCode::BAD_GATEWAY, msg)
}

/// Change embeddings live. Keywords keeps the source settings as they are;
/// meaning recall is tested with one embedding call first, so a typo cannot
/// silently degrade recall.
async fn put_memory_cfg(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<MemoryCfgBody>,
) -> ApiResult<Json<serde_json::Value>> {
    use crate::config::{EmbedSource, ProviderKind, Recall};
    require_admin(&m, &headers).await?;
    let rt = runtime(&m)?;
    let locked = rt.env().locked_under("memory.");
    if !locked.is_empty() {
        let vars: Vec<_> = locked.iter().map(|l| l.var).collect();
        return Err(ApiError(
            StatusCode::CONFLICT,
            format!(
                "memory is set by the server's environment ({})",
                vars.join(", ")
            ),
        ));
    }
    let mut cfg = rt.config();
    if !cfg.memory.backend.is_builtin() {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "embeddings settings apply to the built-in memory only".into(),
        ));
    }
    let bad = |msg: &str| ApiError(StatusCode::BAD_REQUEST, msg.to_string());
    let old = cfg.memory.clone();
    cfg.memory.recall = b.recall;
    if let Some(p) = b.preset {
        cfg.memory.preset = p;
    }
    if b.recall == Recall::Meaning {
        let source = b
            .source
            .ok_or_else(|| bad("choose where embeddings come from"))?;
        let provider_id = match source {
            EmbedSource::Provider => {
                let id = b.provider_id.as_deref().unwrap_or("").trim();
                match cfg.provider(id) {
                    None if id.is_empty() => return Err(bad("choose a provider")),
                    None => return Err(bad("unknown provider")),
                    Some(p) if p.kind == ProviderKind::Mock => {
                        return Err(bad("the demo provider has no embeddings"));
                    }
                    Some(p) => Some(p.id.clone()),
                }
            }
            EmbedSource::Ollama => None,
        };
        let url = b.embeddings_url.as_deref().unwrap_or("").trim();
        let mc = &mut cfg.memory;
        mc.source = source;
        mc.provider_id = provider_id;
        mc.embeddings_url = match source {
            EmbedSource::Provider => String::new(),
            EmbedSource::Ollama => url.trim_end_matches('/').to_string(),
        };
        mc.embeddings_model = b
            .embeddings_model
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string();
    }
    cfg.validate()
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    let emb = crate::providers::embedder(&cfg, rt.data_dir(), &rt.secrets)
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, format!("{e:#}")))?;
    if let Some(e) = &emb {
        e.embed(&["test".to_string()])
            .await
            .map_err(|err| embed_failure(&cfg, e, err))?;
    }
    // The test took a while: save onto the config as it is now, and only
    // if what the test relied on is unchanged.
    let _edits = rt.lock_edits().await;
    let mut latest = rt.config();
    let provider_of = |c: &crate::config::Config| {
        c.memory
            .provider_id
            .as_deref()
            .and_then(|id| c.provider(id))
            .map(|p| {
                (
                    p.kind,
                    p.base_url.clone(),
                    p.api_key_env.clone(),
                    p.api_key_secret.clone(),
                )
            })
    };
    let mut probe = latest.clone();
    probe.memory = cfg.memory.clone();
    let memory_moved = serde_json::to_value(&latest.memory).ok() != serde_json::to_value(&old).ok();
    if memory_moved || provider_of(&probe) != provider_of(&cfg) {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "the settings changed while testing; test again".into(),
        ));
    }
    latest.memory = cfg.memory;
    latest
        .validate()
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    rt.save(latest.clone())
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if let Some(bm) = m.builtin_memory() {
        bm.set_embedder(emb);
    }
    Ok(Json(memory_cfg_json(&m, &latest)))
}

async fn admin_nodes(
    State(m): State<Master>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    let listener = m.node_listener();
    Ok(Json(json!({
        "nodes": m.registered_nodes().await?,
        "listener": listener.map(|(port, fp)| json!({ "port": port, "fingerprint": fp })),
    })))
}

/// A join token plus everything the node needs, except the host name the
/// node should dial (the browser knows that better than the server).
async fn node_join_token(
    State(m): State<Master>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require_admin(&m, &headers).await?;
    let Some((port, fingerprint)) = m.node_listener() else {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "remote nodes are off: start the server with --node-listen 0.0.0.0:7443".into(),
        ));
    };
    let token = m.create_join_token(who.user_id.clone()).await?;
    Ok(Json(json!({
        "token": token,
        "port": port,
        "fingerprint": fingerprint,
        "expires_in_s": crate::nodes::JOIN_TOKEN_TTL.as_secs(),
    })))
}

async fn revoke_node(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    if !m.revoke_node(&id).await? {
        return Err(ApiError(StatusCode::NOT_FOUND, "no such node".into()));
    }
    crate::artifacts::on_node_revoked(&m, &id).await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct RestoreBody {
    #[serde(default)]
    force: bool,
    #[serde(default)]
    rewind: bool,
}

async fn restore_turn(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, turn)): Path<(String, String)>,
    Json(b): Json<RestoreBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let row = visible_session(&m, &who, &id).await?;
    agentic_gate(&m, &who, &row).await?;
    m.restore_turn(row.session_id, turn, b.force, b.rewind)
        .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct VariantBody {
    group: String,
    message_id: String,
}

async fn select_variant(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<VariantBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let sid = visible_session(&m, &who, &id).await?.session_id;
    m.select_variant(sid, b.group, b.message_id).await?;
    Ok(Json(json!({ "ok": true })))
}

/// What optional services this server has, and what the caller may use in
/// a workspace (the sidebar shows an entry only when its `show` is true).
async fn features(
    State(m): State<Master>,
    headers: HeaderMap,
    Query(q): Query<WorkspaceQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let scope = ws::scope(&m, &who, q.workspace.as_deref()).await?;
    let scope = scope.as_ref();
    let routines = crate::routines::features(&m, &who, scope).await;
    let media = crate::media::features(&m, &who, scope).await;
    let artifacts = crate::artifacts::features(&m, &who, scope).await;
    let kinds = kinds_shown(&media, &artifacts);
    Ok(Json(json!({
        "voice": m.transcriber().is_some(),
        "memory": m.memory().is_some(),
        "routines": routines,
        "media": media,
        "artifacts": artifacts,
        "search": { "kinds": kinds },
    })))
}

async fn transcribe(
    State(m): State<Master>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> ApiResult<Json<serde_json::Value>> {
    require(&m, &headers).await?;
    let t = m.transcriber().ok_or_else(|| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "voice is not set up on this server".into(),
        )
    })?;
    if body.is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "no audio".into()));
    }
    let mime = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .filter(|v| v.starts_with("audio/"))
        .unwrap_or("audio/webm")
        .to_string();
    let text = t
        .transcribe(body.to_vec(), &mime)
        .await
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(json!({ "text": text })))
}

/// Chats waiting for the caller (approvals, unanswered questions).
async fn attention(
    State(m): State<Master>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let owner = who.user_id.clone();
    let viewer = owner.clone().unwrap_or_default();
    let items = m
        .inner
        .mirror
        .call(move |s| s.attention(owner.as_deref(), &viewer))
        .await?;
    // Server problems from the node watchdog, for admins.
    let nodes = if who.is_admin() {
        crate::node_setups::attention(&m).await
    } else {
        vec![]
    };
    let routines = crate::routines::attention(&m, &who).await?;
    Ok(Json(
        json!({ "items": items, "nodes": nodes, "routines": routines }),
    ))
}

#[derive(Deserialize)]
struct SeenBody {
    seq: u64,
}

async fn mark_seen(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<SeenBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let sid = visible_session(&m, &who, &id).await?.session_id;
    let viewer = who.user_id.clone().unwrap_or_default();
    m.inner
        .mirror
        .call(move |s| s.mark_seen(&viewer, sid, b.seq))
        .await?;
    let _ = m.inner.live.send(crate::Live::SessionsChanged);
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ProbeBody {
    host: String,
    #[serde(default = "ssh_port")]
    port: u16,
}

fn ssh_port() -> u16 {
    22
}

fn valid_host(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 253
        && h.chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-:[]".contains(c))
}

/// The server's SSH host key fingerprint, for the user to compare with the
/// provider's console before sending any credential.
async fn provision_probe(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<ProbeBody>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    if !valid_host(&b.host) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid host".into()));
    }
    let fp = harness_provision::ssh::probe(&b.host, b.port)
        .await
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e.to_string()))?;
    Ok(Json(json!({ "fingerprint": fp })))
}

/// Credentials are moved into `Secret`s at once and only used for the first
/// login; this struct is never logged.
#[derive(Deserialize)]
struct ProvisionBody {
    host: String,
    #[serde(default = "ssh_port")]
    port: u16,
    user: String,
    /// From the probe, confirmed by the user.
    fingerprint: String,
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    private_key: Option<String>,
    #[serde(default)]
    passphrase: Option<String>,
    mode: crate::provision::Mode,
    /// The user's own public key, kept on the server (automatic mode).
    #[serde(default)]
    own_pubkey: Option<String>,
    /// Copy this server's model provider and key to the node.
    #[serde(default)]
    copy_provider: bool,
    #[serde(default)]
    engine: harness_provision::facts::Engine,
    /// Automatic mode: keep harness's passwordless sudo afterwards.
    #[serde(default)]
    keep_admin_sudo: bool,
    /// Automatic mode: key-only SSH and the firewall (opt-out, default on).
    #[serde(default = "yes")]
    protect_ssh: bool,
    #[serde(default = "yes")]
    protect_firewall: bool,
}

fn yes() -> bool {
    true
}

async fn provision_start(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<ProvisionBody>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    let ProvisionBody {
        host,
        port,
        user,
        fingerprint,
        password,
        private_key,
        passphrase,
        mode,
        own_pubkey,
        copy_provider,
        engine,
        keep_admin_sudo,
        protect_ssh,
        protect_firewall,
    } = b;
    let bad = |m: &str| Err(ApiError(StatusCode::BAD_REQUEST, m.into()));
    if !valid_host(&host) {
        return bad("invalid host");
    }
    // Automatic setup creates harness's own user, which takes root.
    if mode == crate::provision::Mode::Auto && user != "root" {
        return bad(
            "automatic setup needs the root login (user `root`); harness then creates its own user. \
             Use manual setup to keep your own user.",
        );
    }
    if user.is_empty()
        || !user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    {
        return bad("invalid user name");
    }
    if !fingerprint.starts_with("SHA256:") {
        return bad("confirm the server's host key fingerprint first");
    }
    let own_pubkey = own_pubkey
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty());
    if let Some(k) = &own_pubkey
        && (!k.starts_with("ssh-") && !k.starts_with("ecdsa-")
            || k.contains('\n')
            || k.contains('\''))
    {
        return bad("your public key must be one line starting with ssh- or ecdsa-");
    }
    let auth = match (password, private_key) {
        (Some(p), None) if !p.is_empty() => {
            harness_provision::ssh::Auth::Password(harness_core::Secret::new(p))
        }
        (None, Some(k)) if !k.trim().is_empty() => harness_provision::ssh::Auth::Key(
            harness_core::Secret::new(k),
            passphrase.map(harness_core::Secret::new),
        ),
        _ => return bad("give a password or a private key"),
    };
    let job = m
        .start_provision(crate::provision::StartRequest {
            host,
            port,
            user,
            fingerprint,
            auth,
            mode,
            own_pubkey,
            copy_provider,
            engine,
            keep_admin_sudo: keep_admin_sudo && mode == crate::provision::Mode::Auto,
            protect_ssh,
            protect_firewall,
        })
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({ "id": job.id })))
}

fn job_or_404(m: &Master, id: &str) -> ApiResult<std::sync::Arc<crate::provision::Job>> {
    m.provision_job(id)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "no such setup".into()))
}

async fn provision_state(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    Ok(Json(job_or_404(&m, &id)?.state()))
}

#[derive(Deserialize)]
struct AnswerBody {
    step: String,
    choice: String,
}

/// Answer a step that waits for the user (e.g. "create harness's own user").
async fn provision_answer(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<AnswerBody>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    let job = job_or_404(&m, &id)?;
    if !job.answer(&b.step, &b.choice) {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "that step is not waiting for this answer".into(),
        ));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn provision_recheck(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    let job = job_or_404(&m, &id)?;
    job.recheck()
        .await
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e.to_string()))?;
    Ok(Json(job.state()))
}

async fn setup_or_404(m: &Master, id: &str) -> ApiResult<crate::node_setups::SetupRecord> {
    m.setup(id)
        .await?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "no such server".into()))
}

/// Servers set up over SSH, with the commands for manual changes.
async fn list_setups(
    State(m): State<Master>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    let list: Vec<serde_json::Value> = m
        .setups()
        .await?
        .iter()
        .map(|r| json!({ "setup": r, "commands": crate::node_setups::commands(r) }))
        .collect();
    Ok(Json(json!({ "setups": list })))
}

#[derive(Deserialize)]
struct SetupPatch {
    #[serde(default)]
    mode: Option<crate::provision::Mode>,
    #[serde(default)]
    watch_ssh: Option<bool>,
    #[serde(default)]
    watch_firewall: Option<bool>,
}

/// Change the saved mode or what the watchdog looks at. Nothing on the
/// server changes.
async fn update_setup(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<SetupPatch>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    let mut r = setup_or_404(&m, &id).await?;
    if let Some(mode) = b.mode {
        r.mode = mode;
    }
    if let Some(v) = b.watch_ssh {
        r.watch_ssh = v;
    }
    if let Some(v) = b.watch_firewall {
        r.watch_firewall = v;
    }
    if !r.watch_ssh {
        r.problems.retain(|p| p.kind != "ssh");
    }
    if !r.watch_firewall {
        r.problems.retain(|p| p.kind != "firewall");
    }
    m.save_setup(&r).await?;
    let _ = m.inner.live.send(crate::Live::SessionsChanged);
    Ok(Json(json!(r)))
}

async fn forget_setup(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    if !m.forget_setup(&id).await? {
        return Err(ApiError(StatusCode::NOT_FOUND, "no such server".into()));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn check_setup(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    let r = setup_or_404(&m, &id).await?;
    let r = m
        .check_setup(r)
        .await
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e.to_string()))?;
    Ok(Json(json!(r)))
}

#[derive(Deserialize)]
struct FirewallBody {
    enable: bool,
    /// A root login for this action (unless harness kept sudo).
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    private_key: Option<String>,
    #[serde(default)]
    passphrase: Option<String>,
}

/// Automatic mode: turn the firewall on or off on the server.
async fn setup_firewall(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<FirewallBody>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    let r = setup_or_404(&m, &id).await?;
    if r.mode != crate::provision::Mode::Auto {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "manual mode: run the commands yourself, or switch this server to automatic".into(),
        ));
    }
    let root = match (b.password, b.private_key) {
        (Some(p), None) if !p.is_empty() => Some(harness_provision::ssh::Auth::Password(
            harness_core::Secret::new(p),
        )),
        (None, Some(k)) if !k.trim().is_empty() => Some(harness_provision::ssh::Auth::Key(
            harness_core::Secret::new(k),
            b.passphrase.map(harness_core::Secret::new),
        )),
        _ => None,
    };
    let r = m
        .set_firewall(r, b.enable, root)
        .await
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e.to_string()))?;
    Ok(Json(json!(r)))
}

// ---- token savings -------------------------------------------------------

async fn get_savings(
    State(m): State<Master>,
    headers: HeaderMap,
) -> ApiResult<Json<harness_core::savings::TokenSavings>> {
    require_admin(&m, &headers).await?;
    Ok(Json(runtime(&m)?.config().savings))
}

/// Replace the token-saving settings; the next turn uses them.
async fn put_savings(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<harness_core::savings::TokenSavings>,
) -> ApiResult<Json<harness_core::savings::TokenSavings>> {
    require_admin(&m, &headers).await?;
    let rt = runtime(&m)?;
    let _edits = rt.lock_edits().await;
    let mut cfg = rt.config();
    cfg.savings = b;
    rt.save(cfg.clone())
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(cfg.savings))
}

async fn graphify_json(m: &Master) -> serde_json::Value {
    let local = m.graphify();
    serde_json::json!({
        "local_node": local.is_some(),
        "allowed": local.as_ref().is_some_and(|(s, _)| s.allowed()),
        "installed": harness_tools::graphify::installed_version().await,
        "pinned_version": harness_tools::graphify::VERSION,
        "remote_setup": [
            "rizzy-harness graphify install",
            "echo 'graphify = true' >> <data dir>/node.toml   # then restart the node",
        ],
    })
}

async fn get_graphify(
    State(m): State<Master>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    Ok(Json(graphify_json(&m).await))
}

#[derive(Deserialize)]
struct GraphifyBody {
    allowed: bool,
}

/// Allow or forbid graphify on the built-in node (writes its node.toml).
/// Remote nodes decide in their own node.toml.
async fn put_graphify(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<GraphifyBody>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    let Some((state, path)) = m.graphify() else {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "this server runs no built-in node; allow graphify in each node's node.toml".into(),
        ));
    };
    set_toml_bool(&path, "graphify", b.allowed)
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    state.set_allowed(b.allowed);
    Ok(Json(graphify_json(&m).await))
}

/// Install the pinned graphify for the user running harness (built-in node).
async fn install_graphify(
    State(m): State<Master>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    if m.graphify().is_none() {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "this server runs no built-in node; run `rizzy-harness graphify install` on each node"
                .into(),
        ));
    }
    let (ok, log) = match harness_tools::graphify::install().await {
        Ok(l) => (true, l),
        Err(l) => (false, l),
    };
    let mut v = graphify_json(&m).await;
    v["install_ok"] = ok.into();
    v["install_log"] = log
        .chars()
        .rev()
        .take(4000)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>()
        .into();
    Ok(Json(v))
}

/// Set a top-level `key = true|false` in a TOML file, keeping the rest of
/// the file (and its comments) as it is.
fn set_toml_bool(path: &std::path::Path, key: &str, v: bool) -> Result<(), String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let line = format!("{key} = {v}");
    let mut found = false;
    let mut in_table = false;
    let mut out: Vec<String> = Vec::new();
    for l in text.lines() {
        let t = l.trim_start();
        if t.starts_with('[') {
            in_table = true;
        }
        let is_key = !in_table
            && t.strip_prefix(key)
                .is_some_and(|r| r.trim_start().starts_with('='));
        if is_key && !found {
            out.push(line.clone());
            found = true;
        } else if !is_key {
            out.push(l.to_string());
        }
    }
    if !found {
        // Top-level keys must come before any [table].
        let at = out
            .iter()
            .position(|l| l.trim_start().starts_with('['))
            .unwrap_or(out.len());
        out.insert(at, line);
    }
    let mut s = out.join("\n");
    s.push('\n');
    // Must still parse as a node policy before it replaces the file.
    toml::from_str::<toml::Value>(&s).map_err(|e| e.to_string())?;
    std::fs::write(path, s).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod savings_tests {
    #[test]
    fn toml_bool_is_set_in_place() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("node.toml");
        super::set_toml_bool(&p, "graphify", true).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "graphify = true\n");
        std::fs::write(
            &p,
            "# my roots\nworkspace_roots = [\"/w\"]\ngraphify = true\n",
        )
        .unwrap();
        super::set_toml_bool(&p, "graphify", false).unwrap();
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "# my roots\nworkspace_roots = [\"/w\"]\ngraphify = false\n"
        );
    }
}
