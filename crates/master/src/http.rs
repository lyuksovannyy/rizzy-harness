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
        .route("/api/nodes", get(nodes))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route("/api/sessions/{id}", get(get_session))
        .route("/api/sessions/{id}/events", get(events))
        .route("/api/sessions/{id}/prompt", post(prompt))
        .route("/api/sessions/{id}/cancel", post(cancel))
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

async fn require(master: &Master, headers: &HeaderMap) -> ApiResult<auth::Principal> {
    auth::authenticate(master, headers)
        .await
        .ok_or_else(|| ApiError(StatusCode::UNAUTHORIZED, "not logged in".into()))
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
    let user = m
        .inner
        .mirror
        .call(move |s| s.user_by_username(&name))
        .await?;
    let phc = user.as_ref().map(|u| u.password_hash.clone());
    // Verified even for unknown users, so timing does not reveal usernames.
    if !auth::verify_password(b.password, phc).await {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "wrong username or password".into(),
        ));
    }
    let user = user.expect("verified implies user exists");
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
    let who = require(&m, &headers).await?;
    Ok(Json(
        json!({ "ok": true, "app": harness_core::APP_NAME, "user": who }),
    ))
}

async fn nodes(State(m): State<Master>, headers: HeaderMap) -> ApiResult<Json<serde_json::Value>> {
    require(&m, &headers).await?;
    Ok(Json(json!({ "nodes": m.nodes() })))
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
    cwd: String,
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
    let title = b
        .title
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| "New session".into());
    let sid = m
        .create_session(b.node_id, title, b.cwd, b.model, b.approval_mode)
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
    m.prompt(sid, b.text).await?;
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
