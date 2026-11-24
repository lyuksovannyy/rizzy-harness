use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use harness_core::SessionId;
use harness_core::policy::{ApprovalMode, Decision};
use serde::Deserialize;
use serde_json::json;

use crate::workspaces::{self as ws, WsError};
use crate::{CommandError, Master, auth, forge_repos};
pub(crate) use caller::ByteCaller;
use caller::{Admin, Caller, User};
use memory::{memory_err, memory_store};

mod account;
mod agent_edits;
mod approval_mode;
mod artifacts;
mod attachments;
mod background;
mod bg_output;
mod caller;
mod chat_allows;
mod chat_diff;
mod classifier;
mod compact;
mod connection_repos;
mod connections;
mod custom_connectors;
mod diff;
mod doc_page_edit;
mod doc_preview;
mod exec_mode;
mod files;
mod folder_file;
mod folder_page_edit;
mod folder_preview;
mod fork;
mod git_state;
mod hooks;
mod levels;
mod links;
mod made_files;
mod media;
mod memory;
mod nodes;
mod pins;
mod plugins;
mod pricing;
mod profile;
mod projects;
mod providers;
mod push;
mod retry;
mod rewind;
mod routines;
mod search;
mod server_settings;
mod sessions;
mod skills;
mod skills_menu;
mod step_limit;
mod storage;
mod usage;
pub(crate) use usage::TIME_LIMIT as USAGE_TIME_LIMIT;
mod users;
mod voice;
mod workspaces;

pub fn router(master: Master) -> Router {
    router_with(master, crate::limits::Timeouts::default())
}

/// `router` with explicit request timeouts (tests shorten them).
pub fn router_with(master: Master, timeouts: crate::limits::Timeouts) -> Router {
    let private = private(&master);
    assemble(master, timeouts, private)
}

fn assemble(master: Master, timeouts: crate::limits::Timeouts, private: Router<Master>) -> Router {
    // `private` already carries its caller layer; `public()` must not.
    let api = private
        .merge(public())
        // Matched routes only (the embedded UI files are not handlers); which
        // routes get which limit is the table in `limits.rs`.
        .route_layer(axum::middleware::from_fn_with_state(
            timeouts,
            crate::limits::enforce,
        ));
    api.fallback(crate::assets::serve)
        .layer(axum::middleware::from_fn_with_state(
            master.clone(),
            auth::guard,
        ))
        // Outermost: request ids, 500 bodies and cache/transport headers
        // for every response, the guard's refusals included.
        .layer(axum::middleware::from_fn_with_state(
            master.clone(),
            crate::response::finish,
        ))
        .with_state(master)
}

/// The routes anyone may call: each checks its caller itself, or has none
/// (the route walk's `PUBLIC` list says why for each). Nothing else goes
/// here.
fn public() -> Router<Master> {
    Router::new()
        .route(
            "/api/setup",
            get(account::setup_status)
                .post(account::setup)
                .layer(axum::extract::DefaultBodyLimit::max(auth::AUTH_BODY_LIMIT)),
        )
        .route(
            "/api/login",
            post(account::login).layer(axum::extract::DefaultBodyLimit::max(auth::AUTH_BODY_LIMIT)),
        )
        .route("/api/logout", post(account::logout))
        .route("/api/ws", get(crate::ws::upgrade))
        .merge(hooks::routes())
}

/// Every other route: the caller is identified (`caller::identify`) before
/// the handler reads anything of the request.
fn private(master: &Master) -> Router<Master> {
    private_routes().route_layer(axum::middleware::from_fn_with_state(
        master.clone(),
        caller::identify,
    ))
}

/// The routes of [`private`], without its layer.
fn private_routes() -> Router<Master> {
    Router::new()
        .route(
            "/api/me/workspace",
            axum::routing::put(workspaces::set_active),
        )
        .route("/api/favicon", get(crate::favicon::favicon))
        .route("/api/admin/providers", get(providers::get_providers))
        .route(
            "/api/admin/pricing",
            axum::routing::put(pricing::put_pricing),
        )
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
        .route("/api/workspaces/{id}/plugins", get(plugins::list))
        .route(
            "/api/workspaces/{id}/plugins/{kind}/{name}",
            axum::routing::put(plugins::put),
        )
        .route(
            "/api/workspaces/{id}/skills",
            axum::routing::post(skills::create),
        )
        .route(
            "/api/workspaces/{id}/skills/{scope}/{name}",
            get(skills::get_one).put(skills::put).delete(skills::delete),
        )
        .route(
            "/api/workspaces/{id}/members",
            get(workspaces::members).post(workspaces::add_member),
        )
        .route(
            "/api/workspaces/{id}/members/{user_id}",
            axum::routing::patch(workspaces::set_role).delete(workspaces::remove_member),
        )
        .route(
            "/api/workspaces/{id}/members/{user_id}/exec-level",
            axum::routing::put(levels::set_member_level),
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
        .route(
            "/api/users",
            get(users::list_users).post(users::create_user),
        )
        .route(
            "/api/users/{id}",
            axum::routing::patch(users::update_user).delete(users::delete_user),
        )
        .route("/api/users/{id}/password", post(users::reset_password))
        .route(
            "/api/users/{id}/exec-ceiling",
            axum::routing::put(levels::set_ceiling),
        )
        .route("/api/admin/memory", get(get_memory_cfg).put(put_memory_cfg))
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
        .merge(projects::routes())
        .merge(memory::routes())
        .merge(routines::routes())
        .merge(attachments::routes())
        .merge(media::routes())
        .merge(artifacts::routes())
        .merge(background::routes())
        .merge(bg_output::routes())
        .merge(diff::routes())
        .merge(chat_diff::routes())
        .merge(files::routes())
        .merge(made_files::routes())
        .merge(doc_preview::routes())
        .merge(doc_page_edit::routes())
        .merge(folder_page_edit::routes())
        .merge(folder_preview::routes())
        .merge(folder_file::routes())
        .merge(git_state::routes())
        .merge(push::routes())
        .merge(skills_menu::routes())
        .merge(approval_mode::routes())
        .merge(exec_mode::routes())
        .merge(chat_allows::routes())
        .merge(step_limit::routes())
        .merge(compact::routes())
        .merge(retry::routes())
        .merge(pins::routes())
        .merge(rewind::routes())
        .merge(fork::routes())
        .merge(storage::routes())
        .merge(usage::routes())
        .merge(links::routes())
        .merge(profile::routes())
        .merge(connections::routes())
        .merge(connection_repos::routes())
        .merge(custom_connectors::routes())
        .merge(sessions::routes())
        .merge(account::routes())
        .merge(nodes::routes())
        .merge(server_settings::routes())
        .merge(voice::routes())
        .merge(search::routes())
        .merge(classifier::routes())
        .merge(agent_edits::routes())
}

pub(crate) struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let busy = self.0 == StatusCode::TOO_MANY_REQUESTS && self.1 == auth::BUSY;
        // The one 429 a client may retry says so in a field of its own.
        let body = if self.0 == StatusCode::TOO_MANY_REQUESTS && self.1 == forge_repos::LISTING_BUSY
        {
            json!({ "error": self.1, "code": forge_repos::LISTING_BUSY_CODE })
        } else {
            json!({ "error": self.1 })
        };
        let mut resp = (self.0, Json(body)).into_response();
        if busy {
            // No password-hash permit came free in time (auth::Hasher).
            resp.headers_mut()
                .insert(header::RETRY_AFTER, auth::BUSY_RETRY_AFTER_S.into());
        }
        resp
    }
}

impl From<auth::HashError> for ApiError {
    fn from(e: auth::HashError) -> Self {
        auth::AccountError::from(e).into()
    }
}

impl From<CommandError> for ApiError {
    fn from(e: CommandError) -> Self {
        let code = match &e {
            CommandError::UnknownSession => StatusCode::NOT_FOUND,
            CommandError::NodeOffline(_) | CommandError::NodeStopping => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            CommandError::Timeout => StatusCode::GATEWAY_TIMEOUT,
            CommandError::Forbidden(_) => StatusCode::FORBIDDEN,
            CommandError::Rejected(_)
            | CommandError::AgentGone(_)
            | CommandError::BadDefault(_) => StatusCode::CONFLICT,
            CommandError::Store(_) => StatusCode::INTERNAL_SERVER_ERROR,
            CommandError::UnknownAgent(_) | CommandError::SubagentOnly { .. } => {
                StatusCode::BAD_REQUEST
            }
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

impl From<auth::AccountError> for ApiError {
    fn from(e: auth::AccountError) -> Self {
        let code = match &e {
            auth::AccountError::Invalid(_) => StatusCode::BAD_REQUEST,
            auth::AccountError::Taken => StatusCode::CONFLICT,
            auth::AccountError::Busy => StatusCode::TOO_MANY_REQUESTS,
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

/// [`visible_session`] for a request that changes the chat or what its
/// turns read (a message, an answer, a decision, a setting): the ONE place
/// such requests resolve their caller. A caller who is not the chat's owner
/// (the API token: a person sees only their own chats) marks the chat
/// before anything is forwarded, for good: custom connectors are not used
/// in it again (PLAN.md T85). A mark that cannot be written refuses the
/// request.
async fn acting_session(
    m: &Master,
    who: &auth::Principal,
    id: &str,
) -> ApiResult<harness_store::SessionRow> {
    let row = visible_session(m, who, id).await?;
    // A chat nobody owns (made with the API token) has no account to use.
    if row.owner_id.is_some() && row.owner_id != who.user_id {
        m.mark_foreign(&row).await.map_err(|e| {
            tracing::warn!(error = %e, "could not mark a chat another caller acted in");
            ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "this action could not be recorded; nothing was sent".into(),
            )
        })?;
    }
    Ok(row)
}

/// Refuse (403) to make an agentic chat do work when its owner may no
/// longer run its mode there: their effective level (N.10a part 2) and the
/// node rule, checked on every action that starts work, since either can
/// be lowered after the chat was created. `Master::send_command` asks the
/// same again for every such command; this answers first, before a
/// request takes its uploads.
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

fn runtime(m: &Master) -> ApiResult<std::sync::Arc<crate::providers::ProviderRuntime>> {
    m.provider_runtime().ok_or_else(|| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "provider is not managed by this server".into(),
        )
    })
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
    User(who): User,
    State(m): State<Master>,
    Query(q): Query<SessionsQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let scope = ws::scope(&m, &who, q.workspace.as_deref()).await?;
    // Every listed chat is in the scope's workspace: the caller's level
    // there, against each chat's own mode (reading stays allowed).
    let local_ok = ws::may_use_local_node(&m, &who);
    let in_scope = |r: &&harness_store::SessionRow| match &scope {
        Some(a) => r.workspace_id.as_deref() == Some(a.id()),
        None => true,
    };
    let owner = who.user_id.clone();
    let viewer = owner.clone().unwrap_or_default();
    let (rows, unread) = m
        .inner
        .mirror
        .call(move |s| {
            Ok::<_, harness_store::StoreError>((
                s.sessions_of(owner.as_deref())?,
                s.unread(owner.as_deref(), &viewer)?,
            ))
        })
        .await?;
    let links = crate::projects::links(&m).await?;
    let mut list: Vec<serde_json::Value> = rows
        .iter()
        .filter(|r| who.can_see(r.owner_id.as_deref()))
        .filter(in_scope)
        .map(|r| {
            let on_local = (!local_ok && m.is_local_node(&r.node_id))
                .then(|| ws::LOCAL_NODE_ADMINS_ONLY.to_string());
            let blocked = ws::row_refusal(&who, scope.as_ref(), r);
            let mut v = session_json(r, blocked.or(on_local).as_deref());
            v["project_id"] = json!(crate::projects::effective(&links, r));
            v["unread"] = json!(unread.contains(&r.session_id));
            v
        })
        .collect();
    crate::routines::annotate(&m, &mut list, q.routine_runs).await?;
    // A routine's run says it is done through the routine's own results.
    for v in &mut list {
        if !v["routine"].is_null() {
            v["unread"] = json!(false);
        }
    }
    m.annotate_background(&mut list);
    Ok(Json(json!({ "sessions": list })))
}

/// Any of the caller's chats, whatever its workspace (the UI follows a
/// link to a chat into its workspace).
async fn get_session(
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let row = visible_session(&m, &who, &id).await?;
    let blocked = ws::agentic_blocked(&m, &who, &row).await;
    let mut v = session_json(&row, blocked.as_deref());
    v["project_id"] = json!(crate::projects::effective(
        &crate::projects::links(&m).await?,
        &row
    ));
    crate::routines::annotate_one(&m, &mut v).await?;
    m.annotate_background(std::slice::from_mut(&mut v));
    // Whether custom connectors are off in this chat and why (`this_chat`,
    // `parent_chat`, `event_data`, `no_owner`, `unknown`), so a screen can
    // say it (PLAN.md T85).
    v["connectors_off"] = json!(m.connectors_off(&row).await);
    Ok(Json(v))
}

#[derive(Deserialize)]
struct DeleteSessionQuery {
    /// Also delete the caller's Media items that came from this chat.
    #[serde(default, deserialize_with = "query_flag")]
    media: bool,
}

async fn delete_session(
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
    Query(q): Query<DeleteSessionQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let sid = acting_session(&m, &who, &id).await?.session_id;
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
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
    Query(q): Query<EventsQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let sid = visible_session(&m, &who, &id).await?.session_id;
    let limit = q.limit.unwrap_or(1000).min(5000);
    let evs = m
        .inner
        .mirror
        .call(move |s| s.events_after(sid, q.after, limit))
        .await?;
    // No signature or redacted data reaches a browser (X.25, T73).
    let evs: Vec<serde_json::Value> = evs
        .iter()
        .map(harness_core::RawEvent::for_browser)
        .collect();
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
    /// How its commands run (N.10a): a request, checked against the
    /// caller's level (`levels::create_mode`) and then by the node.
    #[serde(default)]
    exec_mode: Option<harness_core::policy::ExecMode>,
}

async fn create_session(
    User(who): User,
    State(m): State<Master>,
    Json(b): Json<CreateBody>,
) -> ApiResult<Json<serde_json::Value>> {
    // Every request for the host shell is audited, whatever refuses it.
    let host = b.exec_mode == Some(harness_core::policy::ExecMode::Host);
    let mut seen = ws::HostAudit::default();
    let out = create_checked(&m, &who, b, &mut seen).await;
    if host {
        let res = out.as_ref().map(|s| *s).map_err(|e| e.1.as_str());
        ws::audit_host_create(&m, &who, &seen, res).await;
    }
    Ok(Json(json!({ "session_id": out? })))
}

async fn create_checked(
    m: &Master,
    who: &crate::auth::Principal,
    mut b: CreateBody,
    seen: &mut ws::HostAudit,
) -> ApiResult<SessionId> {
    let (m, who) = (m.clone(), who.clone());
    seen.node = b.node_id.clone();
    if b.incognito && b.profile != Some(harness_core::event::Profile::Chat) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "incognito is only available for chats".into(),
        ));
    }
    let scope = ws::scope(&m, &who, b.workspace_id.as_deref()).await?;
    seen.workspace = scope.as_ref().map(|a| a.id().to_string());
    // Agentic sessions run shell commands on a worker node: the mode asked
    // for (or the most confined one) must be within the caller's level
    // here (N.10a part 2). `Master::create_session` checks it again.
    if b.profile.unwrap_or(harness_core::event::Profile::Agentic)
        == harness_core::event::Profile::Agentic
    {
        let mode = crate::levels::create_mode(&who, scope.as_ref(), b.exec_mode);
        b.exec_mode = Some(mode.map_err(|why| ApiError(StatusCode::FORBIDDEN, why))?);
    }
    let project = projects::for_new_chat(&m, &who, &mut b, scope.as_ref()).await?;
    // And the node must be one they may use (the embedded node is for
    // admins by default): checked after the project may have chosen it;
    // without a choice, only nodes they may use are candidates.
    let agentic = b.profile.unwrap_or(harness_core::event::Profile::Agentic)
        == harness_core::event::Profile::Agentic;
    if agentic {
        let local_ok = ws::may_use_local_node(&m, &who);
        if b.node_id.is_none() && !local_ok {
            b.node_id = m.default_node_where(harness_core::event::Profile::Agentic, false);
            if b.node_id.is_none() {
                return Err(ApiError(
                    StatusCode::FORBIDDEN,
                    ws::LOCAL_NODE_ADMINS_ONLY.into(),
                ));
            }
        }
        if let Some(why) = b
            .node_id
            .as_deref()
            .and_then(|n| ws::node_refusal(&m, &who, n))
        {
            return Err(ApiError(StatusCode::FORBIDDEN, why));
        }
        // The node as `create_session` will pick it, for the audit line.
        b.node_id = b
            .node_id
            .or_else(|| m.default_node(harness_core::event::Profile::Agentic));
        seen.node = b.node_id.clone();
    } else if b.exec_mode.is_some() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "a chat runs no commands and has no execution mode".into(),
        ));
    }
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
            no_callbacks: false,
            exec_mode: b.exec_mode,
        })
        .await?;
    Ok(sid)
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
    /// Paths the person mentioned with `@path` (`files::check_mentions`).
    #[serde(default)]
    files: Vec<String>,
}

/// The files for the folder a refused message held can be sent again.
async fn give_back_files(m: &Master, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    let ids = ids.to_vec();
    if let Err(e) = m
        .inner
        .mirror
        .call(move |s| s.chat_uploads_give_back(&ids))
        .await
    {
        tracing::warn!(error = %e, "could not give a refused message's files back");
    }
}

async fn prompt(
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
    Json(b): Json<PromptBody>,
) -> ApiResult<Json<serde_json::Value>> {
    if b.text.trim().is_empty() && b.reply.is_none() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "empty prompt".into()));
    }
    if let Some(r) = &b.reply {
        check_reply_body(r, &b)?;
    }
    // One count for everything a message carries: text, PDFs and saved
    // files (the attachments) and images together.
    if b.attachments.len() + b.images.len() > crate::attachments::MAX_PER_MESSAGE {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            format!(
                "at most {} files and images per message",
                crate::attachments::MAX_PER_MESSAGE
            ),
        ));
    }
    let row = acting_session(&m, &who, &id).await?;
    // Before the uploads are taken, so a refusal leaves them for later.
    agentic_gate(&m, &who, &row).await?;
    files::check_mentions(&m, &row, &b.files)?;
    let images = crate::media::prompt_images(&m, &who, &row, &b.images)
        .await
        .map_err(|(c, e)| ApiError(c, e))?;
    // A chat a routine started from an external event stays untrusted for
    // every turn the owner sends there.
    let flags = crate::routines::on_owner_prompt(&m, row.session_id).await;
    let sid = row.session_id;
    // The take, the send and what is done with the uploads after it run in
    // a task of their own: a request the route's time limit (or a client
    // that went away) drops cannot cancel the bookkeeping half way and leave
    // a file held for a message the node never got (T94).
    let task = tokio::spawn(async move {
        // The ids name text attachments and files for the folder alike; the
        // files are held (not consumed) until the node has taken the message.
        let (file_ids, text_ids): (Vec<String>, Vec<String>) = if b.attachments.is_empty() {
            (vec![], vec![])
        } else {
            let ids = b.attachments.clone();
            let files = m
                .inner
                .mirror
                .call(move |s| s.chat_uploads_among(&ids))
                .await?;
            let text = b
                .attachments
                .iter()
                .filter(|i| !files.contains(i))
                .cloned()
                .collect();
            (files, text)
        };
        let unknown = || {
            ApiError(
                StatusCode::BAD_REQUEST,
                "unknown or already used attachment; upload it again".into(),
            )
        };
        let files = if file_ids.is_empty() {
            Vec::new()
        } else {
            let (ids, owner) = (file_ids.clone(), who.user_id.clone());
            m.inner
                .mirror
                .call(move |s| s.chat_uploads_take(&ids, owner.as_deref()))
                .await?
                .ok_or_else(unknown)?
        };
        let attachments = if text_ids.is_empty() {
            Vec::new()
        } else {
            let (ids, owner) = (text_ids.clone(), who.user_id.clone());
            let taken = m
                .inner
                .mirror
                .call(move |s| s.attachments_take_owned(&ids, owner.as_deref()))
                .await?;
            match taken {
                Some(t) => t,
                None => {
                    give_back_files(&m, &file_ids).await;
                    return Err(unknown());
                }
            }
        };
        // A document's text came with its file (X.55): the message that
        // takes the text takes the file too, when it can still be taken.
        let optional = if text_ids.is_empty() {
            Vec::new()
        } else {
            let ids: Vec<String> = text_ids
                .iter()
                .map(|i| crate::attachments::file_id_of(i))
                .collect();
            let owner = who.user_id.clone();
            m.inner
                .mirror
                .call(move |s| s.chat_uploads_take_available(&ids, owner.as_deref()))
                .await?
        };
        let file_ids: Vec<String> = file_ids
            .into_iter()
            .chain(optional.iter().map(|u| u.upload_id.clone()))
            .collect();
        let taken = (!attachments.is_empty()).then(|| (text_ids.clone(), attachments.clone()));
        let attachments = attachments.into_iter().map(|(a, _)| a).collect();
        let sent = m
            .prompt(
                sid,
                b.text,
                crate::PromptOpts {
                    agent_id: b.agent_id,
                    effort: b.effort,
                    context: Some(
                        crate::prefs::Prefs::load(&m, who.user_id.as_deref())
                            .await
                            .for_turn(flags.untrusted)
                            .model_context(
                                &who.username,
                                jiff::Timestamp::now(),
                                row.profile.as_deref() == Some("chat"),
                            ),
                    ),
                    attachments,
                    compare: b.compare,
                    reply: b.reply,
                    untrusted: flags.untrusted,
                    images,
                    callback: vec![],
                    by_owner: who.user_id.is_some() && who.user_id == row.owner_id,
                    // Never from a request: only the routine runner sets it.
                    routine_run: None,
                    files: b.files,
                    uploads: files,
                    optional_uploads: optional,
                    retry: None,
                    retry_images: false,
                },
            )
            .await;
        // A prompt the node certainly did not take gives its uploads back, so
        // "send this again" works (review A19-8): a refusal (a stopping node,
        // a turn already running) or one never sent (the node away, too
        // large). Not after a timeout: the node may have taken it and only its
        // answer was lost, and giving them back could let one file go twice.
        // The files for the folder follow the same rule; once the node took
        // the message they are done.
        match &sent {
            Ok(()) => {
                if !file_ids.is_empty() {
                    let ids = file_ids.clone();
                    if let Err(e) = m
                        .inner
                        .mirror
                        .call(move |s| s.chat_uploads_done(&ids))
                        .await
                    {
                        tracing::warn!(error = %e, "could not clear the files a message carried");
                    }
                }
            }
            Err(e) if !matches!(e, CommandError::Timeout) => {
                give_back_files(&m, &file_ids).await;
                if let Some((ids, atts)) = taken
                    && let Err(e) = m
                        .inner
                        .mirror
                        .call(move |s| s.attachments_give_back(&ids, &atts))
                        .await
                {
                    tracing::warn!(error = %e, "could not give a refused message's uploads back");
                }
            }
            Err(_) => {}
        }
        sent?;
        Ok::<(), ApiError>(())
    });
    task.await.map_err(|e| {
        tracing::error!(error = %e, "a prompt's task failed");
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, "the send failed".into())
    })??;
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
    if !b.files.is_empty() {
        return bad("an answer cannot mention files");
    }
    if harness_core::ui::too_large(r) {
        return bad(harness_core::ui::TOO_LARGE);
    }
    Ok(())
}

async fn cancel(
    Caller(who): Caller,
    State(m): State<Master>,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    // Stopping work is always allowed, even before a pending password
    // change (Gate 3 review P-7): only the owner's own chat, as ever.
    let sid = acting_session(&m, &who, &id).await?.session_id;
    m.cancel(sid).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct AgentBody {
    agent_id: String,
}

async fn set_agent(
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
    Json(b): Json<AgentBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let row = acting_session(&m, &who, &id).await?;
    agentic_gate(&m, &who, &row).await?;
    m.set_agent(row.session_id, &b.agent_id).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ApprovalBody {
    decision: Decision,
    /// The person's note with a denial (ignored with an allow).
    #[serde(default)]
    note: Option<String>,
}

async fn approval(
    Caller(who): Caller,
    State(m): State<Master>,
    Path((id, approval_id)): Path<(String, String)>,
    Json(b): Json<ApprovalBody>,
) -> ApiResult<Json<serde_json::Value>> {
    // Allowing makes the chat do work; denying stops it, like cancel, so a
    // user who lost access, or must change their password first, can
    // still refuse (Gate 3 review P-7).
    if b.decision.allows() {
        caller::settled(&who)?;
    }
    // An id outside the harness's id rule names no card (review N4): the
    // same answer as an unknown session, before anything is relayed.
    if !harness_core::valid_event_id(&approval_id) {
        return Err(ApiError(StatusCode::NOT_FOUND, "unknown approval".into()));
    }
    let row = acting_session(&m, &who, &id).await?;
    if b.decision.allows() {
        agentic_gate(&m, &who, &row).await?;
    }
    if b.note
        .as_ref()
        .is_some_and(|n| n.chars().count() > harness_core::policy::MAX_DENY_NOTE)
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            format!(
                "the note is longer than {} characters",
                harness_core::policy::MAX_DENY_NOTE
            ),
        ));
    }
    // With an allow a note means nothing: it is dropped, not refused.
    let note = b.note.filter(|_| !b.decision.allows());
    // A push card's Allow by the chat's owner (X.27 step 4): recorded
    // before it is relayed, since the node asks for the credential at once.
    if b.decision.allows() {
        m.note_push_click(who.user_id.as_deref(), &row, &approval_id)
            .await;
    }
    m.respond_approval(row.session_id, approval_id, b.decision, note)
        .await?;
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

async fn get_memory_cfg(_: Admin, State(m): State<Master>) -> ApiResult<Json<serde_json::Value>> {
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
    _: Admin,
    State(m): State<Master>,
    Json(b): Json<MemoryCfgBody>,
) -> ApiResult<Json<serde_json::Value>> {
    use crate::config::{EmbedSource, ProviderKind, Recall};
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

#[derive(Deserialize)]
struct RestoreBody {
    #[serde(default)]
    force: bool,
    #[serde(default)]
    rewind: bool,
}

async fn restore_turn(
    User(who): User,
    State(m): State<Master>,
    Path((id, turn)): Path<(String, String)>,
    Json(b): Json<RestoreBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let row = acting_session(&m, &who, &id).await?;
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
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
    Json(b): Json<VariantBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let row = acting_session(&m, &who, &id).await?;
    agentic_gate(&m, &who, &row).await?;
    m.select_variant(row.session_id, b.group, b.message_id)
        .await?;
    Ok(Json(json!({ "ok": true })))
}

/// Chats waiting for the caller (approvals, unanswered questions).
async fn attention(User(who): User, State(m): State<Master>) -> ApiResult<Json<serde_json::Value>> {
    let owner = who.user_id.clone();
    let viewer = owner.clone().unwrap_or_default();
    let (o, v) = (owner.clone(), viewer.clone());
    let items = m
        .inner
        .mirror
        .call(move |s| s.attention(o.as_deref(), &v))
        .await?;
    // Callback turns not seen yet, and why the agent was not resumed (T.8d
    // step 3): rows of the same shape, after the chats that wait.
    let mut items = serde_json::to_value(items).unwrap_or_default();
    if let Some(list) = items.as_array_mut() {
        list.extend(m.callback_attention(owner, viewer).await?);
    }
    let routines = crate::routines::attention(&m, &who).await?;
    Ok(Json(json!({ "items": items, "routines": routines })))
}

#[derive(Deserialize)]
struct SeenBody {
    seq: u64,
}

async fn mark_seen(
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
    Json(b): Json<SeenBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let sid = visible_session(&m, &who, &id).await?.session_id;
    let viewer = who.user_id.clone().unwrap_or_default();
    m.inner
        .mirror
        .call(move |s| s.mark_seen(&viewer, sid, b.seq))
        .await?;
    m.nudge(crate::Live::SessionsChanged(crate::Audience::Chat(sid)));
    Ok(Json(json!({ "ok": true })))
}
