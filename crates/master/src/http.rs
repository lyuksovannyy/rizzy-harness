use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use harness_core::SessionId;
use serde::Deserialize;
use serde_json::json;

use crate::workspaces::{self as ws, WsError};
use crate::{CommandError, Master, auth, forge_repos};
use caller::Admin;
pub(crate) use caller::ByteCaller;
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
mod turns;
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
        .merge(turns::routes())
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
