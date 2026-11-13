//! Provider setup by admins: presets, test, save, delete, cached model
//! listings, model tests, per-agent default models. Keys are stored per
//! provider (`provider-<id>`) and only ever sent to the base URL they were
//! saved for.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use harness_core::Secret;
use serde::Deserialize;
use serde_json::json;

use super::{ApiError, ApiResult, require_admin, runtime};
use crate::Master;
use crate::avatars::AgentAvatar;
use crate::config::{AgentChain, Config, ModelConfig, ProviderConfig, ProviderKind, check_chain};
use crate::presets::{self, Preset};
use crate::providers::{ProviderRuntime, secret_name};

const MAX_NAME: usize = 64;
const MAX_TOKENS: u64 = 100_000_000;

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

fn key_state(p: &ProviderConfig) -> String {
    match (&p.api_key_env, &p.api_key_secret) {
        (Some(v), _) => format!("env:{v}"),
        (None, Some(_)) => "stored".to_string(),
        (None, None) => "none".to_string(),
    }
}

/// Every field, unreported ones as `null`.
fn model_json(m: &ModelConfig) -> serde_json::Value {
    json!({
        "id": m.id, "name": m.name, "context": m.context, "max_output": m.max_output,
        "effort": m.effort, "vision": m.vision, "tools": m.tools,
        "custom": m.custom, "unavailable": m.unavailable, "missing_since": m.missing_since,
    })
}

fn state_json(cfg: &Config) -> serde_json::Value {
    json!({
        "providers": cfg.providers.iter().map(|p| json!({
            "id": p.id,
            "name": p.name,
            "preset": p.preset,
            "kind": p.kind,
            "base_url": p.base_url,
            "key": key_state(p),
            "models": p.models.iter().map(model_json).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "presets": presets::PRESETS,
        "default_model": cfg.effective_default(),
        "agent_models": cfg.agent_models,
        "configured": cfg.configured(),
    })
}

pub(super) async fn get_providers(
    State(m): State<Master>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    Ok(Json(state_json(&runtime(&m)?.config())))
}

fn preset(id: &str) -> ApiResult<&'static Preset> {
    presets::get(id).ok_or_else(|| bad(format!("unknown preset {id:?}")))
}

/// The URL a preset uses: its own for fixed presets (a client URL is
/// ignored), else the client's, else the preset's default.
fn base_url(p: &Preset, given: Option<&str>) -> ApiResult<String> {
    let given = given
        .map(|u| u.trim().trim_end_matches('/'))
        .filter(|u| !u.is_empty());
    let url = match (p.url_editable, given, p.base_url) {
        (true, Some(u), _) => u.to_string(),
        (_, _, Some(u)) => u.to_string(),
        (_, None, None) | (false, Some(_), None) => {
            return Err(bad("enter the endpoint's base URL"));
        }
    };
    if !crate::config::url_allowed(&url) {
        return Err(bad(
            "base URL must be https:// (plain http only for localhost)",
        ));
    }
    Ok(url)
}

fn new_key(k: &Option<String>) -> Option<Secret<String>> {
    k.as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(|k| Secret::new(k.to_string()))
}

/// The saved key of provider `id`, only if it was saved for `base`.
fn stored_key(rt: &ProviderRuntime, id: Option<&str>, base: &str) -> Option<Secret<String>> {
    let cfg = rt.config();
    let p = cfg.provider(id?)?;
    if !presets::same_url(&p.base_url, base) {
        return None;
    }
    crate::providers::resolve_key(p, rt.data_dir()).ok()?.0
}

#[derive(Deserialize)]
pub(super) struct TestBody {
    preset: String,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
    /// Reuse this provider's saved key (same base URL only).
    #[serde(default)]
    provider_id: Option<String>,
}

pub(super) async fn test_provider(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<TestBody>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    let rt = runtime(&m)?;
    // Always 200: a bad preset or URL is a failed test like any other.
    let checked = preset(&b.preset).and_then(|p| Ok((p, base_url(p, b.base_url.as_deref())?)));
    let (p, base) = match checked {
        Ok(v) => v,
        Err(ApiError(_, e)) => return Ok(Json(json!({ "ok": false, "error": e }))),
    };
    let typed = new_key(&b.api_key);
    let saved = typed
        .is_none()
        .then(|| stored_key(&rt, b.provider_id.as_deref(), &base))
        .flatten();
    // Only a listing made the way the saved provider lists (its preset, its
    // own key or none) is its listing; a draft key's or preset's is not.
    let own = typed.is_none()
        && b.provider_id.as_deref().is_some_and(|id| {
            rt.config().provider(id).is_some_and(|sp| {
                sp.preset == p.id
                    && (saved.is_some()
                        || (sp.api_key_env.is_none() && sp.api_key_secret.is_none()))
            })
        });
    let key = typed.or(saved);
    let ollama = p.id == "ollama";
    match crate::providers::list_models(p.kind, &base, key.as_ref(), ollama, &m.outbound()).await {
        Ok(models) => {
            // A saved provider at this URL keeps it as its listing.
            if own
                && let Some(id) = b.provider_id.as_deref()
                && let Err(e) = m.record_listing(id, p.kind, &base, models.clone()).await
            {
                tracing::warn!(provider = %id, "keeping a model listing: {e:#}");
            }
            Ok(Json(json!({
                "ok": true,
                "models": models.iter().map(model_json).collect::<Vec<_>>(),
            })))
        }
        Err(e) => Ok(Json(json!({ "ok": false, "error": e }))),
    }
}

/// A model as the client sends it back (the test's `ProviderModel`).
#[derive(Deserialize)]
pub(super) struct ModelBody {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    context: Option<u64>,
    #[serde(default)]
    max_output: Option<u64>,
    #[serde(default)]
    effort: Option<bool>,
    #[serde(default)]
    vision: Option<bool>,
    #[serde(default)]
    tools: Option<bool>,
    /// Added by hand rather than picked from a listing.
    #[serde(default)]
    custom: bool,
}

#[derive(Deserialize)]
pub(super) struct ProviderBody {
    /// Custom presets only; the others are named after the preset.
    #[serde(default)]
    name: String,
    preset: String,
    #[serde(default)]
    base_url: Option<String>,
    /// A new key; omitted = keep the saved one if the URL is unchanged.
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    models: Vec<ModelBody>,
}

fn clean_name(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME)
        .collect::<String>()
        .trim()
        .to_string()
}

fn clean_models(models: Vec<ModelBody>, kind: ProviderKind) -> Vec<ModelConfig> {
    let tokens = |n: Option<u64>| n.filter(|n| (1..=MAX_TOKENS).contains(n));
    let mut out: Vec<ModelConfig> = Vec::new();
    for m in models {
        let id = m.id.trim().to_string();
        if id.is_empty()
            || id.len() > 256
            || id.contains("::")
            || id.chars().any(char::is_control)
            || out.iter().any(|x| x.id == id)
        {
            continue;
        }
        out.push(ModelConfig {
            name: m
                .name
                .as_deref()
                .map(|n| clean_name(n).chars().take(120).collect::<String>())
                .filter(|n| !n.is_empty()),
            context: tokens(m.context),
            max_output: tokens(m.max_output),
            // Never for Anthropic until thinking blocks are replayed
            // (PLAN.md R8); the model list reports `false` for the same reason.
            effort: if kind == ProviderKind::Anthropic {
                Some(false)
            } else {
                m.effort
            },
            vision: m.vision,
            tools: m.tools,
            custom: m.custom,
            // Availability is the server's to mark (see `put_provider`).
            unavailable: false,
            missing_since: None,
            id,
        });
    }
    out
}

fn secrets_path(rt: &ProviderRuntime) -> std::path::PathBuf {
    rt.data_dir().join("secrets.toml")
}

/// Store `key` under `name` for `f` (which builds and persists the config
/// that uses it), and put the previous value back when `f` fails, so a
/// failed save never pairs the new key with the old URL.
pub(super) fn with_secret<T>(
    path: &std::path::Path,
    name: &str,
    key: Option<&Secret<String>>,
    f: impl FnOnce() -> ApiResult<T>,
) -> ApiResult<T> {
    let Some(k) = key else { return f() };
    let internal = |e: crate::secrets::SecretsError| {
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    };
    let prev = crate::secrets::get(path, name).map_err(internal)?;
    crate::secrets::set(path, name, Some(k)).map_err(internal)?;
    let r = f();
    if r.is_err()
        && let Err(e) = crate::secrets::set(path, name, prev.as_ref())
    {
        tracing::error!("restoring secret {name:?} after a failed save: {e}");
    }
    r
}

/// Validate, persist, switch, and refresh the model list and the memory
/// embedder.
pub(super) fn apply(m: &Master, rt: &ProviderRuntime, mut cfg: Config) -> ApiResult<()> {
    cfg.normalize();
    rt.apply(cfg).map_err(|e| bad(format!("{e:#}")))?;
    m.set_models(rt.models());
    super::refresh_embedder(m, rt);
    Ok(())
}

/// Drop a stored key nothing uses any more (only our own `provider-<id>`
/// names; a key referenced by hand under another name is left alone).
fn forget_key(rt: &ProviderRuntime, old: Option<&ProviderConfig>) {
    let Some(old) = old else { return };
    let name = secret_name(&old.id);
    let in_use = rt
        .config()
        .providers
        .iter()
        .any(|p| p.api_key_secret.as_deref() == Some(name.as_str()));
    if old.api_key_secret.as_deref() == Some(name.as_str()) && !in_use {
        let _ = crate::secrets::set(&secrets_path(rt), &name, None);
    }
}

pub(super) async fn put_provider(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<ProviderBody>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    let rt = runtime(&m)?;
    if !crate::config::valid_provider_id(&id) {
        return Err(bad(
            "the prefix must be 1-32 of a-z, 0-9, _ and - (starting with a letter or digit)",
        ));
    }
    if presets::RESERVED_IDS.contains(&id.as_str()) {
        return Err(bad(format!("the prefix {id:?} is reserved")));
    }
    let p = preset(&b.preset)?;
    let base = base_url(p, b.base_url.as_deref())?;
    let name = if p.custom {
        let n = clean_name(&b.name);
        if n.is_empty() {
            return Err(bad("give the provider a name"));
        }
        n
    } else {
        p.label.to_string()
    };
    let _edits = rt.lock_edits().await;
    let mut cfg = rt.config();
    let old = cfg.provider(&id).cloned();
    // One prefix, one provider: a preset never silently replaces another
    // provider that holds its id (custom entries may switch API kind).
    if let Some(o) = &old {
        let custom = |preset: &str| presets::get(preset).is_some_and(|x| x.custom);
        if o.preset != p.id && !(custom(&o.preset) && p.custom) {
            return Err(ApiError(
                StatusCode::CONFLICT,
                format!(
                    "the prefix {id:?} is already used by {:?}; remove that provider first",
                    o.name
                ),
            ));
        }
    }
    let key = new_key(&b.api_key);
    // A saved key stays only while the URL it was saved for does.
    let (api_key_env, api_key_secret) = match (&key, &old) {
        (Some(_), _) => (None, Some(secret_name(&id))),
        (None, Some(o)) if presets::same_url(&o.base_url, &base) => {
            (o.api_key_env.clone(), o.api_key_secret.clone())
        }
        _ => (None, None),
    };
    if p.needs_key && api_key_env.is_none() && api_key_secret.is_none() {
        return Err(bad("an API key is required"));
    }
    // No chat model is fine: a provider may serve memory embeddings only.
    let mut models = clean_models(b.models, p.kind);
    // Marks stay while the URL does; the next listing re-checks them.
    let same_url = old
        .as_ref()
        .filter(|o| presets::same_url(&o.base_url, &base));
    for x in models.iter_mut().filter(|x| !x.custom) {
        if let Some(o) = same_url.and_then(|o| o.model(&x.id)) {
            x.unavailable = o.unavailable;
            x.missing_since = o.missing_since;
        }
    }
    let moved = old.is_some() && same_url.is_none();
    let entry = ProviderConfig {
        id: id.clone(),
        name,
        preset: p.id.to_string(),
        kind: p.kind,
        base_url: base,
        api_key_env,
        api_key_secret,
        effort: old.as_ref().and_then(|o| o.effort),
        mock_delay_ms: None,
        mock_obeys_output: false,
        models,
    };
    match cfg.providers.iter_mut().find(|x| x.id == id) {
        Some(slot) => *slot = entry,
        None => cfg.providers.push(entry),
    }
    // A real provider with a chat model replaces the demo one.
    if cfg.has_chat_model() {
        cfg.providers.retain(|x| x.kind != ProviderKind::Mock);
    }
    cfg.normalize();
    cfg.validate().map_err(|e| bad(format!("{e:#}")))?;
    with_secret(&secrets_path(&rt), &secret_name(&id), key.as_ref(), || {
        apply(&m, &rt, cfg)
    })?;
    forget_key(&rt, old.as_ref());
    if moved {
        m.forget_listing(&id).await;
    }
    tracing::info!(provider = %id, preset = %p.id, "provider saved");
    Ok(Json(state_json(&rt.config())))
}

pub(super) async fn delete_provider(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    let rt = runtime(&m)?;
    let _edits = rt.lock_edits().await;
    let mut cfg = rt.config();
    let old = cfg.provider(&id).cloned();
    if old.is_none() {
        return Err(ApiError(StatusCode::NOT_FOUND, "unknown provider".into()));
    }
    cfg.providers.retain(|p| p.id != id);
    apply(&m, &rt, cfg)?;
    forget_key(&rt, old.as_ref());
    m.forget_listing(&id).await;
    tracing::info!(provider = %id, "provider deleted");
    Ok(Json(state_json(&rt.config())))
}

/// `PUT` / `DELETE` on the action paths (`/test`, `/demo`), which axum
/// routes away from `{id}`: saving under them is refused with a reason, and
/// a provider named so by hand can still be removed.
pub(super) async fn put_test(
    s: State<Master>,
    h: HeaderMap,
    b: Json<ProviderBody>,
) -> ApiResult<Json<serde_json::Value>> {
    put_provider(s, h, Path("test".into()), b).await
}

pub(super) async fn put_demo(
    s: State<Master>,
    h: HeaderMap,
    b: Json<ProviderBody>,
) -> ApiResult<Json<serde_json::Value>> {
    put_provider(s, h, Path(presets::DEMO.into()), b).await
}

pub(super) async fn delete_test(
    s: State<Master>,
    h: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    delete_provider(s, h, Path("test".into())).await
}

pub(super) async fn delete_demo(
    s: State<Master>,
    h: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    delete_provider(s, h, Path(presets::DEMO.into())).await
}

/// Switch to the built-in demo provider (onboarding's "Skip").
pub(super) async fn use_demo(
    State(m): State<Master>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    let rt = runtime(&m)?;
    let _edits = rt.lock_edits().await;
    let mut cfg = rt.config();
    if cfg.provider(presets::DEMO).is_none() {
        cfg.providers.push(ProviderConfig::demo(40));
    }
    cfg.default_model = Some(format!("{}::mock", presets::DEMO));
    apply(&m, &rt, cfg)?;
    Ok(Json(state_json(&rt.config())))
}

/// `Some(None)` for an explicit `null`, `None` when the field is missing.
fn present<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(d).map(Some)
}

#[derive(Deserialize)]
pub(super) struct AgentModelsBody {
    /// Agent id -> its model and fallbacks; `null` removes the agent's
    /// assignment.
    #[serde(default)]
    agent_models: BTreeMap<String, Option<AgentChain>>,
    /// Missing: unchanged. `null`: falls back (agent `general`, first model).
    #[serde(default, deserialize_with = "present")]
    default_model: Option<Option<String>>,
}

pub(super) async fn put_agent_models(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<AgentModelsBody>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    let rt = runtime(&m)?;
    let _edits = rt.lock_edits().await;
    let mut cfg = rt.config();
    let agents = m.agents();
    let model = |cfg: &Config, id: &str| {
        cfg.knows_model(id)
            .then(|| id.to_string())
            .ok_or_else(|| bad(format!("unknown model {id:?}")))
    };
    for (agent, choice) in b.agent_models {
        match choice {
            Some(c) => {
                check_chain(&agents, &|id| cfg.knows_model(id), &agent, &c).map_err(bad)?;
                cfg.agent_models.insert(agent, c);
            }
            None if agents.get(&agent).is_none() => {
                return Err(bad(format!("unknown agent {agent:?}")));
            }
            None => {
                cfg.agent_models.remove(&agent);
            }
        }
    }
    match b.default_model {
        Some(Some(id)) => cfg.default_model = Some(model(&cfg, &id)?),
        Some(None) => cfg.default_model = None,
        None => {}
    }
    apply(&m, &rt, cfg)?;
    Ok(Json(state_json(&rt.config())))
}

/// Set how an agent looks. The body must name a shape, eyes and an accent
/// from the fixed sets.
pub(super) async fn put_agent_avatar(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: String,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    let set = m.agents();
    let agent = set
        .get(&id)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("unknown agent {id:?}")))?;
    let avatar: AgentAvatar =
        serde_json::from_str(&body).map_err(|e| bad(format!("bad avatar: {e}")))?;
    let rt = runtime(&m)?;
    let _edits = rt.lock_edits().await;
    let mut cfg = rt.config();
    cfg.agent_avatars.insert(id, avatar);
    apply(&m, &rt, cfg)?;
    // Server settings show the server's own chains.
    Ok(Json(super::agent_json(&m, &m.resolver(None), agent)))
}

/// A listing for the picker: the listed models, then the provider's saved
/// ones the listing lacks (custom and unavailable models), flagged.
fn listing_json(cfg: &Config, id: &str, l: &crate::model_cache::Listing) -> serde_json::Value {
    let saved = cfg
        .provider(id)
        .map(|p| p.models.as_slice())
        .unwrap_or_default();
    let extra = saved
        .iter()
        .filter(|s| !l.models.iter().any(|m| m.id == s.id));
    json!({
        "models": l.models.iter().chain(extra).map(model_json).collect::<Vec<_>>(),
        "fetched_ms": l.fetched_ms,
    })
}

fn unknown_provider() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "unknown provider".into())
}

/// The cached listing; fetched once when there is none yet.
pub(super) async fn get_models(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    let rt = runtime(&m)?;
    let base = rt
        .config()
        .provider(&id)
        .map(|p| p.base_url.clone())
        .ok_or_else(unknown_provider)?;
    let l = match m.cached_listing(&id, &base).await {
        Some(l) => l,
        None => m
            .refresh_listing(&id)
            .await
            .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))?,
    };
    Ok(Json(listing_json(&rt.config(), &id, &l)))
}

/// Fetch the listing now and re-mark the saved models.
pub(super) async fn refresh_models(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    let rt = runtime(&m)?;
    if rt.config().provider(&id).is_none() {
        return Err(unknown_provider());
    }
    let l = m
        .refresh_listing(&id)
        .await
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(listing_json(&rt.config(), &id, &l)))
}

/// The admin's override for a model: remove it (and its mark) from the
/// provider and from every agent that uses it.
pub(super) async fn delete_model(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, raw)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    let rt = runtime(&m)?;
    let _edits = rt.lock_edits().await;
    let mut cfg = rt.config();
    let p = cfg
        .providers
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(unknown_provider)?;
    let before = p.models.len();
    p.models.retain(|x| x.id != raw);
    if p.models.len() == before {
        return Err(ApiError(StatusCode::NOT_FOUND, "unknown model".into()));
    }
    // Agents and defaults that named it are repaired by `apply`: a fallback
    // takes the place of a main model, a default falls back.
    apply(&m, &rt, cfg)?;
    tracing::info!(provider = %id, model = %raw, "model removed");
    Ok(Json(state_json(&rt.config())))
}

/// `DELETE …/models/refresh`, which axum routes to the refresh action: the
/// model whose raw id is `refresh`.
pub(super) async fn delete_refresh(
    s: State<Master>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    delete_model(s, h, Path((id, "refresh".into()))).await
}

#[derive(Deserialize)]
pub(super) struct ModelTestBody {
    /// The provider's raw id.
    model: String,
    /// A saved provider: its URL and kind unless a preset is given, and its
    /// key (same URL only).
    #[serde(default)]
    provider_id: Option<String>,
    #[serde(default)]
    preset: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
    /// The API kind of a bare `base_url`.
    #[serde(default)]
    kind: Option<ProviderKind>,
    /// An embeddings model: embed a word instead of chatting.
    #[serde(default)]
    memory: bool,
}

/// Where a model test goes: the API kind and base URL.
fn test_target(rt: &ProviderRuntime, b: &ModelTestBody) -> ApiResult<(ProviderKind, String)> {
    if let Some(id) = b.preset.as_deref() {
        let p = preset(id)?;
        return Ok((p.kind, base_url(p, b.base_url.as_deref())?));
    }
    if let Some(id) = b.provider_id.as_deref() {
        let p = rt
            .config()
            .provider(id)
            .cloned()
            .ok_or_else(unknown_provider)?;
        return Ok((p.kind, p.base_url));
    }
    let url = b
        .base_url
        .as_deref()
        .map(|u| u.trim().trim_end_matches('/'))
        .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
        .ok_or_else(|| bad("choose a provider or enter an http(s) base URL"))?;
    Ok((b.kind.unwrap_or(ProviderKind::Openai), url.to_string()))
}

/// One short request to a model: a chat that should answer "OK", or for
/// an embeddings model one embedding. Always 200, like `/providers/test`.
pub(super) async fn test_model(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<ModelTestBody>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    let rt = runtime(&m)?;
    let fail = |e: String| Ok(Json(json!({ "ok": false, "error": e })));
    let (kind, base) = match test_target(&rt, &b) {
        Ok(t) => t,
        Err(ApiError(_, e)) => return fail(e),
    };
    let mut model = b.model.trim();
    if let Some(rest) = b
        .provider_id
        .as_deref()
        .and_then(|p| model.strip_prefix(p)?.strip_prefix("::"))
    {
        model = rest;
    }
    if model.is_empty()
        || model.len() > 256
        || model.contains("::")
        || model.chars().any(char::is_control)
    {
        return fail("enter the provider's own model id".into());
    }
    let key = new_key(&b.api_key).or_else(|| stored_key(&rt, b.provider_id.as_deref(), &base));
    if key.is_some() && !crate::config::url_allowed(&base) {
        return fail("a key goes to https:// only (plain http only for localhost)".into());
    }
    let scrub = |e: String| match &key {
        Some(k) if !k.expose().is_empty() => e.replace(k.expose(), "[redacted]"),
        _ => e,
    };
    let started = std::time::Instant::now();
    let api = crate::providers::api_kind(kind);
    // Public addresses only, unless the operator listed the host (AUDIT L3).
    let dns = match api {
        None => None,
        Some(_) => match m.outbound().provider_call(&base).await {
            Ok(dns) => dns,
            Err(e) => return fail(e),
        },
    };
    let reply = match (api, b.memory) {
        (None, _) => Ok("OK".to_string()),
        (Some(k), false) => {
            harness_agent_native::models::test_chat(k, &base, key.as_ref(), model, &dns)
                .await
                .map_err(|e| e.to_string())
        }
        (Some(_), true) => {
            let style = match kind {
                ProviderKind::Anthropic => harness_memory::ApiStyle::Anthropic,
                _ => harness_memory::ApiStyle::Openai,
            };
            let mut embedder = harness_memory::Embedder::new(&base, model, key.clone());
            if let Some(r) = dns {
                embedder = embedder.with_resolver(r);
            }
            embedder
                .with_style(style)
                .embed(&["test".to_string()])
                .await
                .map_err(|e| e.to_string())
                .and_then(|v| match v.first() {
                    Some(e) if !e.is_empty() => Ok(format!("{} dimensions", e.len())),
                    _ => Err("the endpoint returned no embedding".to_string()),
                })
        }
    };
    match reply {
        Ok(r) => Ok(Json(json!({
            "ok": true,
            "reply": harness_agent_native::models::sanitize_reply(&scrub(r)),
            "ms": started.elapsed().as_millis() as u64,
        }))),
        Err(e) => fail(scrub(e)),
    }
}
