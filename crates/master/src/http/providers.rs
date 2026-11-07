//! Provider setup by admins: presets, test, save, delete, per-agent default
//! models. Keys are stored per provider (`provider-<id>`) and only ever sent
//! to the base URL they were saved for.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use harness_core::Secret;
use serde::Deserialize;
use serde_json::json;

use super::{ApiError, ApiResult, require_admin, runtime};
use crate::Master;
use crate::config::{Config, ModelConfig, ProviderConfig, ProviderKind};
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
    /// Reuse the custom embeddings key saved in `[memory]` (same URL only).
    #[serde(default)]
    memory: bool,
}

/// The custom embeddings key `[memory]` keeps, only if it was saved for
/// `base`.
fn memory_key(rt: &ProviderRuntime, base: &str) -> Option<Secret<String>> {
    let cfg = rt.config();
    let mc = &cfg.memory;
    if mc.source != crate::config::EmbedSource::Custom
        || !presets::same_url(&mc.embeddings_url, base)
    {
        return None;
    }
    crate::secrets::get(&secrets_path(rt), mc.embeddings_key_secret.as_deref()?)
        .ok()
        .flatten()
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
    let key = new_key(&b.api_key)
        .or_else(|| stored_key(&rt, b.provider_id.as_deref(), &base))
        .or_else(|| b.memory.then(|| memory_key(&rt, &base)).flatten());
    let ollama = p.id == "ollama";
    match crate::providers::list_models(p.kind, &base, key.as_ref(), ollama).await {
        Ok(models) => Ok(Json(json!({
            "ok": true,
            "models": models.iter().map(model_json).collect::<Vec<_>>(),
        }))),
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
fn apply(m: &Master, rt: &ProviderRuntime, mut cfg: Config) -> ApiResult<()> {
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
    let models = clean_models(b.models, p.kind);
    if models.is_empty() {
        return Err(bad("choose at least one model"));
    }
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
        models,
    };
    match cfg.providers.iter_mut().find(|x| x.id == id) {
        Some(slot) => *slot = entry,
        None => cfg.providers.push(entry),
    }
    // A real provider replaces the demo one.
    cfg.providers.retain(|x| x.kind != ProviderKind::Mock);
    cfg.normalize();
    cfg.validate().map_err(|e| bad(format!("{e:#}")))?;
    with_secret(&secrets_path(&rt), &secret_name(&id), key.as_ref(), || {
        apply(&m, &rt, cfg)
    })?;
    forget_key(&rt, old.as_ref());
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
    /// Agent id -> model id; `null` removes the agent's default.
    #[serde(default)]
    agent_models: BTreeMap<String, Option<String>>,
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
        cfg.canonical_model(id)
            .ok_or_else(|| bad(format!("unknown model {id:?}")))
    };
    for (agent, choice) in b.agent_models {
        if agents.get(&agent).is_none() {
            return Err(bad(format!("unknown agent {agent:?}")));
        }
        match choice {
            Some(id) => {
                let id = model(&cfg, &id)?;
                cfg.agent_models.insert(agent, id);
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
