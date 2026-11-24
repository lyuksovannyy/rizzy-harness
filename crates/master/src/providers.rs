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
                .with_context(|| format!("secret {name:?} not found in secrets.toml"))?;
            (Some(v), vec![])
        }
        (None, None) => (None, vec![]),
    })
    .map(|(k, env)| (k.filter(|k| !k.is_blank()), env))
}

/// The web tools for the chat runtime, from `[web]`. Returns the tools and
/// any key that must be added to the redaction set.
pub fn web_tools(
    cfg: &Config,
    data_dir: &Path,
) -> anyhow::Result<(harness_tools::ToolSet, Option<Secret<String>>)> {
    web_tools_from(&cfg.web, data_dir)
}

/// The web tools one `[web]` section asks for (config.toml's, or a remote
/// node's in node.toml: ADOPTION_PLAN T.3), and the key to add to the
/// redaction set of whatever runs them. A key that cannot be found is an
/// error naming the variable or the secret, never its value.
pub fn web_tools_from(
    w: &crate::config::WebConfig,
    data_dir: &Path,
) -> anyhow::Result<(harness_tools::ToolSet, Option<Secret<String>>)> {
    use crate::config::SearchKind;
    use harness_web::{SearchBackend, WebFetch, WebSearch};
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
                    crate::secrets::get(&path, name)?
                        .with_context(|| format!("secret {name:?} not found in secrets.toml"))?
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
            None => {
                let mock = Mock {
                    delay: Duration::from_millis(p.mock_delay_ms.unwrap_or(40)),
                };
                if p.mock_obeys_output {
                    Arc::new(harness_agent_native::mock::Obedient(mock))
                } else {
                    Arc::new(mock)
                }
            }
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
                    price: m.price.clone().map(|price| crate::pricing::PriceInfo {
                        price,
                        currency: cfg.pricing.currency.clone(),
                    }),
                }
            })
        })
        .collect()
}

/// Why [`ProviderRuntime::save`] or [`ProviderRuntime::apply`] failed.
/// Callers answer the two differently: a refused change is the admin's to
/// fix (a 400 with the sentence), a file that could not be written is the
/// server's trouble (a 500, whose text holds a server path and goes to the
/// log, never to the client).
#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    /// The change is refused: the environment, `validate` or building the
    /// providers said why.
    #[error("{0}")]
    Invalid(anyhow::Error),
    /// `config.toml` could not be written (or read back for the settings
    /// the environment fixes).
    #[error("{0:#}")]
    Io(anyhow::Error),
}

/// The lock that serialises changes to `config.toml` and `secrets.toml`.
pub type EditGuard = tokio::sync::OwnedMutexGuard<()>;

/// Proof, inside [`ProviderRuntime::section`], that the edit lock is held.
pub struct Held(());

/// The task that carried a change out stopped without an answer (it
/// panicked, or the runtime is shutting down).
#[derive(Debug, thiserror::Error)]
#[error("the change did not finish")]
pub struct SectionLost;

impl From<SectionLost> for SaveError {
    fn from(e: SectionLost) -> Self {
        SaveError::Io(e.into())
    }
}

/// Owns the live provider, its config file and the shared secret set.
pub struct ProviderRuntime {
    pub provider: Arc<SwappableProvider>,
    pub secrets: SecretSet,
    data_dir: PathBuf,
    config: Mutex<Config>,
    /// Held by every config change from its `config()` read through its
    /// `save` / `apply`, so no change writes back a stale snapshot.
    edits: Arc<tokio::sync::Mutex<()>>,
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
                edits: Arc::new(tokio::sync::Mutex::new(())),
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
    /// settings the environment fixes. The one place the running server
    /// writes the file: on the blocking pool, so a slow disk stalls a
    /// thread meant for it and not the async workers (FIX_PLAN 4.2).
    async fn persist(&self, cfg: &Config) -> anyhow::Result<()> {
        let path = self.data_dir.join(harness_core::CONFIG_FILE);
        let (env, cfg) = (self.env.clone(), cfg.clone());
        tokio::task::spawn_blocking(move || env.for_file(&cfg, &path)?.save(&path))
            .await
            .context("the config write did not finish")?
    }

    pub fn config(&self) -> Config {
        self.config
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Serialise config changes: take this before reading the config you
    /// are about to change, and hand it to [`Self::section`], [`Self::save`]
    /// or [`Self::apply`], which keep it until the change is written and
    /// switched. Never hold it across a network call.
    pub async fn lock_edits(&self) -> EditGuard {
        self.edits.clone().lock_owned().await
    }

    /// Run a change to the saved configuration to its end, whether or not
    /// the request that asked for it is still waiting.
    ///
    /// The route timeout (`limits::enforce`) and a client that hangs up drop
    /// the handler's future. A write on the blocking pool then keeps going
    /// without it, and with the lock gone the next change would start beside
    /// it on the same temp file, or the file would hold a value the running
    /// configuration never took. So the change runs in a task of its own that
    /// owns the lock `guard`; the handler waits for it, and dropping that wait
    /// stops nothing. Everything that must go together goes inside: the secret
    /// write, the file write, the switch of the running configuration, the
    /// restore of a previous secret on failure.
    pub async fn section<T, F, Fut>(
        self: &Arc<Self>,
        guard: EditGuard,
        f: F,
    ) -> Result<T, SectionLost>
    where
        T: Send + 'static,
        F: FnOnce(Arc<Self>, Held) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = T> + Send + 'static,
    {
        let rt = self.clone();
        tokio::spawn(async move {
            let _guard = guard;
            f(rt, Held(())).await
        })
        .await
        .map_err(|_| SectionLost)
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn models(&self) -> Vec<ModelInfo> {
        model_list(&self.config())
    }

    /// Validate and persist a config change that does not touch the provider.
    pub async fn save(self: &Arc<Self>, cfg: Config, guard: EditGuard) -> Result<(), SaveError> {
        self.section(guard, move |rt, held| async move {
            rt.save_in(cfg, &held).await
        })
        .await?
    }

    /// [`Self::save`] inside a [`Self::section`].
    pub async fn save_in(&self, mut cfg: Config, _: &Held) -> Result<(), SaveError> {
        self.env.apply(&mut cfg).map_err(SaveError::Invalid)?;
        cfg.validate().map_err(SaveError::Invalid)?;
        self.persist(&cfg).await.map_err(SaveError::Io)?;
        *self.config.lock().unwrap_or_else(|p| p.into_inner()) = cfg;
        Ok(())
    }

    /// Validate, build, persist to `config.toml`, then switch. Running turns
    /// finish on the old provider; the next turn uses the new one.
    pub async fn apply(self: &Arc<Self>, cfg: Config, guard: EditGuard) -> Result<(), SaveError> {
        self.section(guard, move |rt, held| async move {
            rt.apply_in(cfg, &held).await
        })
        .await?
    }

    /// [`Self::apply`] inside a [`Self::section`].
    pub async fn apply_in(&self, mut cfg: Config, _: &Held) -> Result<(), SaveError> {
        self.env.apply(&mut cfg).map_err(SaveError::Invalid)?;
        cfg.validate().map_err(SaveError::Invalid)?;
        let dir = self.data_dir.clone();
        let (cfg, built) = tokio::task::spawn_blocking(move || {
            let built = build(&cfg, &dir);
            (cfg, built)
        })
        .await
        .map_err(|e| SaveError::Io(e.into()))?;
        let built = built.map_err(|e| {
            // A secrets file that cannot be read is the server's trouble;
            // a provider that cannot be built is the change's.
            if e.chain().any(|c| c.is::<crate::secrets::SecretsError>()) {
                SaveError::Io(e)
            } else {
                SaveError::Invalid(e)
            }
        })?;
        self.persist(&cfg).await.map_err(SaveError::Io)?;
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
            let k: Secret<String> = crate::secrets::get(&path, n)?
                .with_context(|| format!("secret {n:?} not found in secrets.toml"))?;
            // An empty key is no key: nothing is sent for it.
            Ok(Some(k).filter(|k| !k.is_blank()))
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
    Ok(Some(
        crate::voice::Transcriber::new(&cfg.voice.url, &cfg.voice.model, key)
            .with_limits(cfg.voice.limits()),
    ))
}
