//! The model providers the whole server uses, behind one router that is
//! swappable at runtime so that onboarding and settings can change them
//! without a restart.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use harness_agent_native::Provider;
use harness_agent_native::llm::{ApiKind, LlmProvider, ModelRouter, Route, SwappableProvider};
use harness_agent_native::mock::Mock;
use harness_core::{Secret, SecretSet};

use crate::ModelInfo;
use crate::config::{Config, ModelConfig, ProviderConfig, ProviderKind};

/// The built router plus the keys it uses and env vars to hide from tools.
pub struct Built {
    pub provider: Arc<dyn Provider>,
    pub keys: Vec<Secret<String>>,
    pub env_remove: Vec<String>,
}

pub fn api_kind(kind: ProviderKind) -> Option<ApiKind> {
    match kind {
        ProviderKind::Openai => Some(ApiKind::Openai),
        ProviderKind::Anthropic => Some(ApiKind::Anthropic),
        ProviderKind::Mock => None,
    }
}

/// The `secrets.toml` name of a provider's stored key.
pub fn secret_name(provider_id: &str) -> String {
    format!("provider-{provider_id}")
}

/// Resolve a provider's key (env var or `secrets.toml`).
pub fn resolve_key(
    p: &ProviderConfig,
    data_dir: &Path,
) -> anyhow::Result<(Option<Secret<String>>, Vec<String>)> {
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
    let (mut keys, mut env_remove, mut routes) = (Vec::new(), Vec::new(), Vec::new());
    for p in &cfg.providers {
        let provider: Arc<dyn Provider> = match api_kind(p.kind) {
            None => Arc::new(Mock {
                delay: Duration::from_millis(p.mock_delay_ms.unwrap_or(40)),
            }),
            Some(kind) => {
                let (key, env) =
                    resolve_key(p, data_dir).with_context(|| format!("provider {:?}", p.id))?;
                keys.extend(key.clone());
                env_remove.extend(env);
                Arc::new(LlmProvider::new(kind, &p.base_url, key)?)
            }
        };
        routes.push(Route {
            prefix: p.id.clone(),
            provider,
        });
    }
    Ok(Built {
        provider: Arc::new(ModelRouter::new(routes)),
        keys,
        env_remove,
    })
}

/// Every listed model of every provider, prefixed, in config order.
pub fn model_list(cfg: &Config) -> Vec<ModelInfo> {
    let default = cfg.effective_default();
    cfg.providers
        .iter()
        .flat_map(|p| {
            let default = default.clone();
            p.models.iter().map(move |m| {
                let id = format!("{}::{}", p.id, m.id);
                ModelInfo {
                    default: default.as_deref() == Some(id.as_str()),
                    id,
                    raw: m.id.clone(),
                    provider: p.name.clone(),
                    provider_id: p.id.clone(),
                    name: m.name.clone(),
                    context: m.context,
                    max_output: m.max_output,
                    effort: crate::config::effort_allowed(p, m),
                    vision: m.vision == Some(true),
                    tools: m.tools == Some(true),
                    unavailable: m.unavailable,
                }
            })
        })
        .collect()
}

/// Owns the live provider, its config file and the shared secret set.
pub struct ProviderRuntime {
    pub provider: Arc<SwappableProvider>,
    pub secrets: SecretSet,
    data_dir: PathBuf,
    config: Mutex<Config>,
    /// Held by every config change from its `config()` read through its
    /// `save` / `apply`, so no change writes back a stale snapshot.
    edits: tokio::sync::Mutex<()>,
    /// Settings the environment fixes (`.env`): they win over every change
    /// and are never written over `config.toml`'s own values.
    env: crate::env::EnvSettings,
}

impl ProviderRuntime {
    /// Build from config. Returns the runtime and env vars to hide from tools.
    pub fn start(
        cfg: Config,
        data_dir: &Path,
        secrets: SecretSet,
    ) -> anyhow::Result<(Self, Vec<String>)> {
        let built = build(&cfg, data_dir)?;
        for k in built.keys {
            secrets.add(k);
        }
        Ok((
            Self {
                provider: Arc::new(SwappableProvider::new(built.provider)),
                secrets,
                data_dir: data_dir.to_path_buf(),
                config: Mutex::new(cfg),
                edits: tokio::sync::Mutex::new(()),
                env: crate::env::EnvSettings::default(),
            },
            built.env_remove,
        ))
    }

    /// Keep `env`'s settings on top of every later change.
    pub fn with_env(mut self, env: crate::env::EnvSettings) -> Self {
        self.env = env;
        self
    }

    pub fn env(&self) -> &crate::env::EnvSettings {
        &self.env
    }

    /// Write `cfg` to `config.toml`, keeping the file's own values for the
    /// settings the environment fixes.
    fn persist(&self, cfg: &Config) -> anyhow::Result<()> {
        let path = self.data_dir.join(harness_core::CONFIG_FILE);
        self.env.for_file(cfg, &path)?.save(&path)
    }

    pub fn config(&self) -> Config {
        self.config
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Serialise config changes: take this before reading the config you
    /// are about to change, and keep it until it is saved or applied. Never
    /// hold it across a network call.
    pub async fn lock_edits(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.edits.lock().await
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn models(&self) -> Vec<ModelInfo> {
        model_list(&self.config())
    }

    /// Validate and persist a config change that does not touch the provider.
    pub fn save(&self, mut cfg: Config) -> anyhow::Result<()> {
        self.env.apply(&mut cfg)?;
        cfg.validate()?;
        self.persist(&cfg)?;
        *self.config.lock().unwrap_or_else(|p| p.into_inner()) = cfg;
        Ok(())
    }

    /// Validate, build, persist to `config.toml`, then switch. Running turns
    /// finish on the old provider; the next turn uses the new one.
    pub fn apply(&self, mut cfg: Config) -> anyhow::Result<()> {
        self.env.apply(&mut cfg)?;
        cfg.validate()?;
        let built = build(&cfg, &self.data_dir)?;
        self.persist(&cfg)?;
        for k in built.keys {
            self.secrets.add(k);
        }
        self.provider.set(built.provider);
        *self.config.lock().unwrap_or_else(|p| p.into_inner()) = cfg;
        Ok(())
    }
}

/// Models a provider lists, with what it reports about each, for the
/// onboarding picker. Doubles as a key check. Only to a public address or
/// a host `out` lists (AUDIT L3).
pub async fn list_models(
    kind: ProviderKind,
    base_url: &str,
    key: Option<&Secret<String>>,
    ollama: bool,
    out: &crate::outbound::Outbound,
) -> Result<Vec<ModelConfig>, String> {
    match api_kind(kind) {
        None => Ok(vec![ModelConfig {
            id: "mock".into(),
            ..Default::default()
        }]),
        Some(k) => {
            let dns = out.provider_call(base_url).await?;
            harness_agent_native::models::list_models(k, base_url, key, ollama, &dns)
                .await
                .map(|v| v.into_iter().map(ModelConfig::from).collect())
                .map_err(|e| e.to_string())
        }
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

/// The embedder for `[memory]`, when meaning recall is on. A provider
/// source uses that provider's URL and its key as resolved now.
pub fn embedder(
    cfg: &Config,
    data_dir: &Path,
    secrets: &SecretSet,
) -> anyhow::Result<Option<harness_memory::Embedder>> {
    use crate::config::{EmbedSource, Recall};
    use harness_memory::ApiStyle;
    let m = &cfg.memory;
    if m.recall == Recall::Keywords {
        return Ok(None);
    }
    let (url, key, style) = match m.source {
        EmbedSource::Provider => {
            let p = cfg
                .embed_provider()
                .context("[memory] provider_id is not a configured provider")?;
            let (key, _) =
                resolve_key(p, data_dir).with_context(|| format!("provider {:?}", p.id))?;
            let style = match p.kind {
                ProviderKind::Anthropic => ApiStyle::Anthropic,
                _ => ApiStyle::Openai,
            };
            (p.base_url.clone(), key, style)
        }
        EmbedSource::Ollama => (m.ollama_url().to_string(), None, ApiStyle::Openai),
    };
    if key.is_some() && !crate::config::url_allowed(&url) {
        anyhow::bail!(
            "an embeddings URL that gets a key must be https:// (plain http only for localhost)"
        );
    }
    if let Some(k) = &key {
        secrets.add(k.clone());
    }
    Ok(Some(
        harness_memory::Embedder::new(&url, &m.model(), key).with_style(style),
    ))
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
        MemoryBackend::Builtin | MemoryBackend::Qdrant => {
            let s = Arc::new(
                harness_memory::SqliteMemory::open(&data_dir.join("memory.db"))
                    .map_err(|e| anyhow::anyhow!("opening memory.db: {e}"))?,
            );
            if cfg.memory.backend == MemoryBackend::Qdrant {
                let key = std::env::var("HARNESS_QDRANT_API_KEY")
                    .ok()
                    .filter(|k| !k.trim().is_empty())
                    .map(Secret::new);
                if key.is_some() && !crate::config::url_allowed(&cfg.memory.qdrant_url) {
                    anyhow::bail!(
                        "a Qdrant URL that gets an API key must be https:// (plain http only for \
                         localhost or HARNESS_PLAIN_HTTP_HOSTS)"
                    );
                }
                if let Some(k) = &key {
                    secrets.add(k.clone());
                }
                tracing::info!(url = %cfg.memory.qdrant_url, "memory vectors in qdrant");
                s.set_vector_index(Some(Arc::new(harness_memory::QdrantIndex::new(
                    &cfg.memory.qdrant_url,
                    cfg.memory.qdrant_collection(),
                    key,
                ))));
            }
            s.set_embedder(embedder(cfg, data_dir, secrets)?);
            let m: Arc<dyn harness_memory::MemoryStore> = s.clone();
            (Some(m), Some(s))
        }
    })
}

/// The dictation service from `[voice]`, if configured.
pub fn transcriber(
    cfg: &Config,
    data_dir: &Path,
    secrets: &SecretSet,
) -> anyhow::Result<Option<crate::voice::Transcriber>> {
    if cfg.voice.url.is_empty() {
        return Ok(None);
    }
    let key = secret(data_dir, &cfg.voice.key_secret)?;
    if let Some(k) = &key {
        secrets.add(k.clone());
    }
    Ok(Some(crate::voice::Transcriber::new(
        &cfg.voice.url,
        &cfg.voice.model,
        key,
    )))
}
