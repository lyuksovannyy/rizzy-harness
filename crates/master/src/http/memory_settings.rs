//! The admin's memory settings: where embeddings come from, tested with one
//! embedding call before they change. Routes for the memory configuration go
//! here (the memory store itself is `memory.rs`).

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use super::caller::Admin;
use super::{ApiError, ApiResult, runtime};
use crate::Master;

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/admin/memory", get(get_memory_cfg).put(put_memory_cfg))
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
pub(super) fn refresh_embedder(m: &Master, rt: &crate::providers::ProviderRuntime) {
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
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if let Some(bm) = m.builtin_memory() {
        bm.set_embedder(emb);
    }
    Ok(Json(memory_cfg_json(&m, &latest)))
}
