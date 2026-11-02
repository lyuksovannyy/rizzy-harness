use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use harness_core::SessionId;
use harness_core::policy::{ApprovalMode, Decision};
use serde::Deserialize;
use serde_json::json;

use crate::{CommandError, Master, auth};

pub fn router(master: Master) -> Router {
    let api = Router::new()
        .route("/api/setup", get(setup_status).post(setup))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/me/password", post(change_password))
        .route("/api/me/settings", get(get_settings).put(put_settings))
        .route(
            "/api/me/classifier",
            get(get_classifier).put(put_classifier),
        )
        .route("/api/search", get(search))
        .route("/api/users", get(list_users).post(create_user))
        .route(
            "/api/users/{id}",
            axum::routing::patch(update_user).delete(delete_user),
        )
        .route("/api/nodes", get(nodes))
        .route("/api/models", get(models))
        .route("/api/agents", get(agents))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route("/api/sessions/{id}", get(get_session))
        .route("/api/sessions/{id}/events", get(events))
        .route("/api/sessions/{id}/prompt", post(prompt))
        .route("/api/sessions/{id}/cancel", post(cancel))
        .route("/api/sessions/{id}/agent", post(set_agent))
        .route("/api/sessions/{id}/approvals/{approval_id}", post(approval))
        .route("/api/ws", get(crate::ws::upgrade));
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
        };
        ApiError(code, e.to_string())
    }
}

impl From<harness_store::StoreError> for ApiError {
    fn from(e: harness_store::StoreError) -> Self {
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
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

async fn session_response(m: &Master, user_id: &str) -> ApiResult<Response> {
    let id = auth::create_ui_session(&m.inner.mirror, user_id).await?;
    Ok((
        [(header::SET_COOKIE, m.inner.auth.session_cookie(&id))],
        Json(json!({"ok": true})),
    )
        .into_response())
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
    if let Some(id) = auth::cookie_value(&headers) {
        auth::delete_ui_session(&m.inner.mirror, &id).await;
    }
    (
        [(header::SET_COOKIE, m.inner.auth.clear_cookie())],
        Json(json!({"ok": true})),
    )
        .into_response()
}

async fn me(State(m): State<Master>, headers: HeaderMap) -> ApiResult<Json<serde_json::Value>> {
    let who = require_any(&m, &headers).await?;
    Ok(Json(
        json!({ "ok": true, "app": harness_core::APP_NAME, "user": who }),
    ))
}

async fn nodes(State(m): State<Master>, headers: HeaderMap) -> ApiResult<Json<serde_json::Value>> {
    require(&m, &headers).await?;
    Ok(Json(json!({ "nodes": m.nodes() })))
}

async fn models(State(m): State<Master>, headers: HeaderMap) -> ApiResult<Json<serde_json::Value>> {
    require(&m, &headers).await?;
    Ok(Json(json!({ "models": m.models() })))
}

async fn agents(State(m): State<Master>, headers: HeaderMap) -> ApiResult<Json<serde_json::Value>> {
    require(&m, &headers).await?;
    let set = m.agents();
    Ok(Json(
        json!({ "agents": set.list(), "default": set.default_id() }),
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
    q: String,
    #[serde(default)]
    limit: Option<usize>,
    /// Similar-chat suggestions for the bar (strict; may return nothing).
    #[serde(default)]
    suggest: bool,
}

async fn search(
    State(m): State<Master>,
    headers: HeaderMap,
    Query(q): Query<SearchQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let owner = who.user_id.clone();
    let limit = q.limit.unwrap_or(20).clamp(1, 50);
    let text = q.q.chars().take(500).collect::<String>();
    let hits = m
        .inner
        .mirror
        .call(move |s| {
            if q.suggest {
                crate::search::suggest(s, &text, owner.as_deref())
            } else {
                crate::search::search(s, &text, owner.as_deref(), limit)
            }
        })
        .await?;
    Ok(Json(json!({
        "results": hits.iter().map(|h| json!({
            "session_id": h.session_id,
            "title": h.title,
            "snippet": h.snippet,
            "score": h.score,
            "updated_ms": h.updated_ms,
        })).collect::<Vec<_>>()
    })))
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
    m.inner.mirror.call(move |s| s.delete_user(&uid)).await?;
    Ok(Json(json!({ "ok": true })))
}

fn session_json(r: &harness_store::SessionRow) -> serde_json::Value {
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
    })
}

async fn list_sessions(
    State(m): State<Master>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    let rows = m.inner.mirror.call(|s| s.sessions()).await?;
    Ok(Json(json!({
        "sessions": rows
            .iter()
            .filter(|r| who.can_see(r.owner_id.as_deref()))
            .map(session_json)
            .collect::<Vec<_>>()
    })))
}

async fn get_session(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    Ok(Json(session_json(&visible_session(&m, &who, &id).await?)))
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
}

async fn create_session(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<CreateBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    // Agentic sessions run shell commands on a worker node.
    if b.profile.unwrap_or(harness_core::event::Profile::Agentic)
        == harness_core::event::Profile::Agentic
        && !who.can_agentic
    {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "agentic sessions are not enabled for this account; ask an admin".into(),
        ));
    }
    let title = b
        .title
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| "New session".into());
    let sid = m
        .create_session(crate::CreateSpec {
            profile: b.profile,
            node_id: b.node_id,
            title,
            cwd: b.cwd,
            model: b.model,
            approval_mode: b.approval_mode,
            agent_id: b.agent_id,
        })
        .await?;
    if let Some(uid) = who.user_id {
        // The node's session_created event is mirrored before the create ack
        // arrives (same ordered link), so the row exists here.
        m.inner
            .mirror
            .call(move |s| s.set_session_owner(sid, &uid))
            .await?;
    }
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
}

async fn prompt(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<PromptBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    if b.text.trim().is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "empty prompt".into()));
    }
    let sid = visible_session(&m, &who, &id).await?.session_id;
    m.prompt(
        sid,
        b.text,
        crate::PromptOpts {
            agent_id: b.agent_id,
            effort: b.effort,
        },
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
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
    let sid = visible_session(&m, &who, &id).await?.session_id;
    m.respond_approval(sid, approval_id, b.decision).await?;
    Ok(Json(json!({ "ok": true })))
}
