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

async fn require(master: &Master, headers: &HeaderMap) -> ApiResult<()> {
    if auth::is_authenticated(master, headers).await {
        Ok(())
    } else {
        Err(ApiError(StatusCode::UNAUTHORIZED, "not logged in".into()))
    }
}

fn parse_sid(id: &str) -> ApiResult<SessionId> {
    id.parse()
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "bad session id".into()))
}

#[derive(Deserialize)]
struct LoginBody {
    token: String,
}

async fn login(State(m): State<Master>, Json(body): Json<LoginBody>) -> ApiResult<Response> {
    if !m.inner.auth.token_matches(body.token.trim()) {
        // Constant-ish delay to blunt online guessing.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        return Err(ApiError(StatusCode::UNAUTHORIZED, "invalid token".into()));
    }
    let id = auth::create_ui_session(&m.inner.mirror).await?;
    Ok((
        [(header::SET_COOKIE, m.inner.auth.session_cookie(&id))],
        Json(json!({"ok": true})),
    )
        .into_response())
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
    require(&m, &headers).await?;
    Ok(Json(json!({ "ok": true, "app": harness_core::APP_NAME })))
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
    })
}

async fn list_sessions(
    State(m): State<Master>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    require(&m, &headers).await?;
    let rows = m.inner.mirror.call(|s| s.sessions()).await?;
    Ok(Json(json!({
        "sessions": rows.iter().map(session_json).collect::<Vec<_>>()
    })))
}

async fn get_session(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    require(&m, &headers).await?;
    let sid = parse_sid(&id)?;
    match m.inner.mirror.call(move |s| s.session(sid)).await? {
        Some(r) => Ok(Json(session_json(&r))),
        None => Err(ApiError(StatusCode::NOT_FOUND, "unknown session".into())),
    }
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
    require(&m, &headers).await?;
    let sid = parse_sid(&id)?;
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
    require(&m, &headers).await?;
    let title = b
        .title
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| "New session".into());
    let sid = m
        .create_session(b.node_id, title, b.cwd, b.model, b.approval_mode)
        .await?;
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
    require(&m, &headers).await?;
    if b.text.trim().is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "empty prompt".into()));
    }
    m.prompt(parse_sid(&id)?, b.text).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn cancel(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    require(&m, &headers).await?;
    m.cancel(parse_sid(&id)?).await?;
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
    require(&m, &headers).await?;
    m.respond_approval(parse_sid(&id)?, approval_id, b.decision)
        .await?;
    Ok(Json(json!({ "ok": true })))
}
