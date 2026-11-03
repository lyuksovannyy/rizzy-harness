//! The model provider the whole server uses, swappable at runtime so that
//! onboarding and settings can change it without a restart.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use harness_agent_native::Provider;
use harness_agent_native::llm::{ApiKind, LlmProvider, SwappableProvider};
use harness_agent_native::mock::Mock;
use harness_core::{Secret, SecretSet};

use crate::ModelInfo;
use crate::config::{Config, ModelConfig, ProviderKind};

/// A built provider plus the key it uses and env vars to hide from tools.
pub struct Built {
    pub provider: Arc<dyn Provider>,
    pub key: Option<Secret<String>>,
    pub env_remove: Vec<String>,
}

pub fn api_kind(kind: ProviderKind) -> Option<ApiKind> {
    match kind {
        ProviderKind::Openai => Some(ApiKind::Openai),
        ProviderKind::Anthropic => Some(ApiKind::Anthropic),
        ProviderKind::Mock => None,
    }
}

/// Resolve the configured key (env var or `secrets.toml`).
pub fn resolve_key(
    cfg: &Config,
    data_dir: &Path,
) -> anyhow::Result<(Option<Secret<String>>, Vec<String>)> {
    let p = &cfg.provider;
    Ok(match (&p.api_key_env, &p.api_key_secret) {
        (Some(var), _) => {
            let v = std::env::var(var)
                .with_context(|| format!("environment variable {var} is not set"))?;
            (Some(Secret::new(v)), vec![var.clone()])
        }
        (None, Some(name)) => {
            let path = data_dir.join("secrets.toml");
            let v = crate::secrets::get(&path, name)?
                .with_context(|| format!("secret {name:?} not found in {}", path.display()))?;
            (Some(v), vec![])
        }
        (None, None) => (None, vec![]),
    })
}

/// The web tools for the chat runtime, from `[web]`. Returns the tools and
/// any key that must be added to the redaction set.
pub fn web_tools(
    cfg: &Config,
    data_dir: &Path,
) -> anyhow::Result<(harness_tools::ToolSet, Option<Secret<String>>)> {
    use crate::config::SearchKind;
    use harness_web::{SearchBackend, WebFetch, WebSearch};
    let w = &cfg.web;
    let mut tools: harness_tools::ToolSet = Vec::new();
    let mut key = None;
    match w.search {
        SearchKind::None => {}
        SearchKind::Searxng => tools.push(Arc::new(WebSearch::new(SearchBackend::Searxng {
            base_url: w.searxng_url.clone(),
        }))),
        SearchKind::Brave => {
            let k = match (&w.brave_key_env, &w.brave_key_secret) {
                (Some(var), _) => Secret::new(
                    std::env::var(var)
                        .with_context(|| format!("environment variable {var} is not set"))?,
                ),
                (None, Some(name)) => {
                    let path = data_dir.join("secrets.toml");
                    crate::secrets::get(&path, name)?.with_context(|| {
                        format!("secret {name:?} not found in {}", path.display())
                    })?
                }
                (None, None) => anyhow::bail!("[web] brave needs a key"),
            };
            tools.push(Arc::new(WebSearch::new(SearchBackend::Brave {
                api_key: k.clone(),
            })));
            key = Some(k);
        }
    }
    if w.fetch {
        tools.push(Arc::new(WebFetch::new()));
    }
    Ok((tools, key))
}

pub fn build(cfg: &Config, data_dir: &Path) -> anyhow::Result<Built> {
    match api_kind(cfg.provider.kind) {
        None => Ok(Built {
            provider: Arc::new(Mock {
                delay: Duration::from_millis(cfg.provider.mock_delay_ms),
            }),
            key: None,
            env_remove: vec![],
        }),
        Some(kind) => {
            let (key, env_remove) = resolve_key(cfg, data_dir)?;
            Ok(Built {
                provider: Arc::new(LlmProvider::new(kind, cfg.base_url(), key.clone())?),
                key,
                env_remove,
            })
        }
    }
}

/// The default model plus every model listed in `[[models]]`.
pub fn model_list(cfg: &Config, provider: &str) -> Vec<ModelInfo> {
    let mut out: Vec<ModelInfo> = cfg
        .models
        .iter()
        .map(|m| ModelInfo {
            id: m.id.clone(),
            provider: provider.to_string(),
            effort: m.effort,
            vision: m.vision,
            default: m.id == cfg.provider.model,
        })
        .collect();
    if !out.iter().any(|m| m.default) {
        out.insert(
            0,
            ModelInfo {
                id: cfg.provider.model.clone(),
                provider: provider.to_string(),
                effort: false,
                vision: false,
                default: true,
            },
        );
    }
    out
}

/// Owns the live provider, its config file and the shared secret set.
pub struct ProviderRuntime {
    pub provider: Arc<SwappableProvider>,
    pub secrets: SecretSet,
    data_dir: PathBuf,
    config: Mutex<Config>,
}

impl ProviderRuntime {
    /// Build from config. Returns the runtime and env vars to hide from tools.
    pub fn start(
        cfg: Config,
        data_dir: &Path,
        secrets: SecretSet,
    ) -> anyhow::Result<(Self, Vec<String>)> {
        let built = build(&cfg, data_dir)?;
        if let Some(k) = built.key {
            secrets.add(k);
        }
        Ok((
            Self {
                provider: Arc::new(SwappableProvider::new(built.provider)),
                secrets,
                data_dir: data_dir.to_path_buf(),
                config: Mutex::new(cfg),
            },
            built.env_remove,
        ))
    }

    pub fn config(&self) -> Config {
        self.config
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn models(&self) -> Vec<ModelInfo> {
        model_list(&self.config(), self.provider.name())
    }

    /// Validate and persist a config change that does not touch the provider.
    pub fn save(&self, cfg: Config) -> anyhow::Result<()> {
        cfg.validate()?;
        cfg.save(&self.data_dir.join("harness.toml"))?;
        *self.config.lock().unwrap_or_else(|p| p.into_inner()) = cfg;
        Ok(())
    }

    /// Validate, build, persist to `harness.toml`, then switch. Running turns
    /// finish on the old provider; the next turn uses the new one.
    pub fn apply(&self, cfg: Config) -> anyhow::Result<()> {
        cfg.validate()?;
        let built = build(&cfg, &self.data_dir)?;
        cfg.save(&self.data_dir.join("harness.toml"))?;
        if let Some(k) = built.key {
            self.secrets.add(k);
        }
        self.provider.set(built.provider);
        *self.config.lock().unwrap_or_else(|p| p.into_inner()) = cfg;
        Ok(())
    }
}

/// Models listed by a provider's `/models`, for the onboarding picker.
pub async fn list_models(
    kind: ProviderKind,
    base_url: &str,
    key: Option<&Secret<String>>,
) -> Result<Vec<String>, String> {
    match api_kind(kind) {
        None => Ok(vec!["mock".into()]),
        Some(k) => harness_agent_native::llm::list_models(k, base_url, key)
            .await
            .map_err(|e| e.to_string()),
    }
}

pub fn model_config(id: &str, effort: bool, vision: bool) -> ModelConfig {
    ModelConfig {
        id: id.to_string(),
        effort,
        vision,
    }
}

fn secret(data_dir: &Path, name: &Option<String>) -> anyhow::Result<Option<Secret<String>>> {
    match name {
        None => Ok(None),
        Some(n) => {
            let path = data_dir.join("secrets.toml");
            Ok(Some(crate::secrets::get(&path, n)?.with_context(|| {
                format!("secret {n:?} not found in {}", path.display())
            })?))
        }
    }
}

/// The embedder for `[memory]`, if one is configured.
pub fn embedder(
    cfg: &Config,
    data_dir: &Path,
    secrets: &SecretSet,
) -> anyhow::Result<Option<harness_memory::Embedder>> {
    let m = &cfg.memory;
    if m.embeddings_url.is_empty() {
        return Ok(None);
    }
    let key = secret(data_dir, &m.embeddings_key_secret)?;
    if let Some(k) = &key {
        secrets.add(k.clone());
    }
    Ok(Some(harness_memory::Embedder::new(
        &m.embeddings_url,
        &m.model(),
        key,
    )))
}

/// The configured store, and the built-in one again when that is it.
pub type MemorySetup = (
    Option<Arc<dyn harness_memory::MemoryStore>>,
    Option<Arc<harness_memory::SqliteMemory>>,
);

/// The memory store for `[memory]`. The built-in store is also returned on
/// its own so its embedder can be switched live.
pub fn memory(cfg: &Config, data_dir: &Path, secrets: &SecretSet) -> anyhow::Result<MemorySetup> {
    use crate::config::MemoryBackend;
    Ok(match cfg.memory.backend {
        MemoryBackend::Off => (None, None),
        MemoryBackend::Mem0 => {
            let key = secret(data_dir, &cfg.memory.mem0_key_secret)?;
            if let Some(k) = &key {
                secrets.add(k.clone());
            }
            let m: Arc<dyn harness_memory::MemoryStore> =
                Arc::new(harness_memory::Mem0Memory::new(&cfg.memory.mem0_url, key));
            (Some(m), None)
        }
        MemoryBackend::Builtin => {
            let s = Arc::new(
                harness_memory::SqliteMemory::open(&data_dir.join("memory.db"))
                    .map_err(|e| anyhow::anyhow!("opening memory.db: {e}"))?,
            );
            s.set_embedder(embedder(cfg, data_dir, secrets)?);
            let m: Arc<dyn harness_memory::MemoryStore> = s.clone();
            (Some(m), Some(s))
        }
    })
}
