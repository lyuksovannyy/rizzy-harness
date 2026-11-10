//! `config.toml`: master-side configuration. API keys are never stored here:
//! they are referenced by environment-variable name (`api_key_env`) or by name
//! in the 0600 `secrets.toml` (`api_key_secret`).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

/// The whole file. Loading repairs stale references (see `normalize`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "RawConfig")]
pub struct Config {
    /// Model providers. Model ids are `<provider id>::<raw id>`.
    pub providers: Vec<ProviderConfig>,
    /// The global default model (prefixed). Unset or stale: agent
    /// `general`'s model, then the first listed model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    /// Admin-assigned model per agent id, with its fallbacks (prefixed).
    /// One model may serve many agents; an agent has one model.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub agent_models: BTreeMap<String, AgentChain>,
    /// How agents look; an agent missing here gets one derived from its id.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub agent_avatars: BTreeMap<String, crate::avatars::AgentAvatar>,
    pub chat: ChatConfig,
    pub web: WebConfig,
    pub memory: MemoryConfig,
    pub voice: VoiceConfig,
    /// Token-saving methods, each one switchable.
    pub savings: harness_core::savings::TokenSavings,
    /// Written only when changed, so config.toml (and the copy remote nodes
    /// get) has no `[workspaces]` section until an admin sets one.
    #[serde(skip_serializing_if = "WorkspacesConfig::is_default")]
    pub workspaces: WorkspacesConfig,
    /// `[nodes]`: who may use the embedded node; written only when changed.
    #[serde(skip_serializing_if = "NodesConfig::is_default")]
    pub nodes: NodesConfig,
    // Each feature's section lives in its own module, written only when
    // changed like `[workspaces]`.
    #[serde(skip_serializing_if = "crate::routines::RoutinesConfig::is_default")]
    pub routines: crate::routines::RoutinesConfig,
    #[serde(skip_serializing_if = "crate::storage::StorageConfig::is_default")]
    pub storage: crate::storage::StorageConfig,
    #[serde(skip_serializing_if = "crate::media::MediaConfig::is_default")]
    pub media: crate::media::MediaConfig,
    #[serde(skip_serializing_if = "crate::artifacts::ArtifactsConfig::is_default")]
    pub artifacts: crate::artifacts::ArtifactsConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            providers: vec![ProviderConfig::demo(40)],
            default_model: Some(format!("{}::mock", crate::presets::DEMO)),
            agent_models: BTreeMap::new(),
            agent_avatars: BTreeMap::new(),
            chat: Default::default(),
            web: Default::default(),
            memory: Default::default(),
            voice: Default::default(),
            savings: Default::default(),
            workspaces: Default::default(),
            nodes: Default::default(),
            routines: Default::default(),
            storage: Default::default(),
            media: Default::default(),
            artifacts: Default::default(),
        }
    }
}

/// What the file may contain; any part may be missing.
#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct RawConfig {
    providers: Vec<ProviderConfig>,
    default_model: Option<String>,
    agent_models: BTreeMap<String, AgentChain>,
    agent_avatars: BTreeMap<String, crate::avatars::AgentAvatar>,
    chat: ChatConfig,
    web: WebConfig,
    memory: MemoryConfig,
    voice: VoiceConfig,
    savings: harness_core::savings::TokenSavings,
    workspaces: WorkspacesConfig,
    nodes: NodesConfig,
    routines: crate::routines::RoutinesConfig,
    storage: crate::storage::StorageConfig,
    media: crate::media::MediaConfig,
    artifacts: crate::artifacts::ArtifactsConfig,
}

impl From<RawConfig> for Config {
    fn from(r: RawConfig) -> Self {
        let mut c = Config {
            providers: r.providers,
            default_model: r.default_model,
            agent_models: r.agent_models,
            agent_avatars: r.agent_avatars,
            chat: r.chat,
            web: r.web,
            memory: r.memory,
            voice: r.voice,
            savings: r.savings,
            workspaces: r.workspaces,
            nodes: r.nodes,
            routines: r.routines,
            storage: r.storage,
            media: r.media,
            artifacts: r.artifacts,
        };
        c.normalize();
        c
    }
}

/// `[workspaces]`: the server's policy for team workspaces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkspacesConfig {
    /// Accounts other than server admins may create team workspaces
    /// (server admins always may).
    pub members_can_create: bool,
}

impl Default for WorkspacesConfig {
    fn default() -> Self {
        Self {
            members_can_create: true,
        }
    }
}

impl WorkspacesConfig {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// `[nodes]`: the server's policy for its own embedded worker node.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NodesConfig {
    /// Who may run agentic sessions on the embedded `local` node, which
    /// runs as the server's own OS user next to its data directory.
    pub local_node_users: LocalNodeUsers,
}

impl NodesConfig {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// `local_node_users` in `[nodes]`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalNodeUsers {
    /// Server admins (and the API token) only; other accounts with the
    /// agentic grant use a remote node. Until the agent shell is sandboxed,
    /// an agent on this node can read whatever the server's user can.
    #[default]
    Admins,
    /// Every account with the agentic grant. Only for installs where each
    /// such account is trusted like an admin.
    Granted,
}

/// Fallbacks per agent.
pub const MAX_FALLBACKS: usize = 8;

/// Check an agent's chain before it is saved: a known agent, known models,
/// at most `MAX_FALLBACKS` fallbacks, none repeating the main model or each
/// other. The server's `[agent_models]` and workspace settings both use it,
/// so the two never drift apart.
pub fn check_chain(
    agents: &harness_agents::AgentSet,
    knows_model: &dyn Fn(&str) -> bool,
    agent: &str,
    chain: &AgentChain,
) -> Result<(), String> {
    if agents.get(agent).is_none() {
        return Err(format!("unknown agent {agent:?}"));
    }
    if chain.fallbacks.len() > MAX_FALLBACKS {
        return Err(format!("{agent}: at most {MAX_FALLBACKS} fallbacks"));
    }
    let known = |id: &String| {
        knows_model(id)
            .then_some(())
            .ok_or_else(|| format!("unknown model {id:?}"))
    };
    known(&chain.model)?;
    for (i, f) in chain.fallbacks.iter().enumerate() {
        known(f)?;
        if *f == chain.model || chain.fallbacks[..i].contains(f) {
            return Err(format!(
                "{agent}: fallbacks must not repeat the main model or each other ({f})"
            ));
        }
    }
    Ok(())
}

/// An agent's model and, in order, the models tried when it is unavailable
/// or fails before answering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentChain {
    pub model: String,
    #[serde(default)]
    pub fallbacks: Vec<String>,
}

impl AgentChain {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            fallbacks: Vec::new(),
        }
    }

    /// The main model, then the fallbacks.
    pub fn models(&self) -> impl Iterator<Item = &String> {
        std::iter::once(&self.model).chain(&self.fallbacks)
    }
}

/// Dictation: audio goes to an OpenAI-compatible `/audio/transcriptions`
/// endpoint (a local Whisper server, or a cloud API). Off when `url` is empty.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VoiceConfig {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_secret: Option<String>,
    /// Dictations per account per UTC day (default 200; 0 = unlimited).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daily_requests: Option<u32>,
    /// MiB of audio per account per UTC day (default 100; 0 = unlimited).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daily_mb: Option<u64>,
}

impl VoiceConfig {
    pub fn limits(&self) -> crate::voice::Limits {
        crate::voice::Limits {
            daily_requests: self
                .daily_requests
                .unwrap_or(crate::voice::DEFAULT_DAILY_REQUESTS),
            daily_bytes: self
                .daily_mb
                .unwrap_or(crate::voice::DEFAULT_DAILY_MB)
                .saturating_mul(1024 * 1024),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryBackend {
    /// SQLite in the data dir; keyword recall, plus vectors if embeddings are set.
    #[default]
    Builtin,
    /// A self-hosted mem0 server (restart to switch).
    Mem0,
    /// The built-in store with its vectors in a Qdrant server
    /// (`qdrant_url`); needs meaning recall (an embeddings model).
    Qdrant,
    Off,
}

impl MemoryBackend {
    /// Facts live in the built-in SQLite store (embeddings settings apply).
    pub fn is_builtin(self) -> bool {
        matches!(self, Self::Builtin | Self::Qdrant)
    }
}

/// The Qdrant collection prefix when none is set; the vector size is
/// appended (`harness_memory_768`), so a new embeddings model with another
/// size gets its own collection.
pub const QDRANT_COLLECTION: &str = "harness_memory";

/// How the built-in memory finds relevant facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Recall {
    /// Full-text search only.
    #[default]
    Keywords,
    /// Vectors from an embeddings endpoint, on top of keywords.
    Meaning,
}

/// Where embeddings come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbedSource {
    /// A configured model provider (`provider_id`): its URL and its key,
    /// resolved live, never copied.
    #[default]
    Provider,
    /// Ollama at `embeddings_url` (default [`OLLAMA_URL`]), no key.
    Ollama,
}

pub const OLLAMA_URL: &str = "http://localhost:11434/v1";

/// Long-term memory. Embedding settings apply live; the backend needs a restart.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MemoryConfig {
    pub backend: MemoryBackend,
    pub recall: Recall,
    pub source: EmbedSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    /// Ollama's base URL (Ollama source only).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub embeddings_url: String,
    /// Empty: the preset's model (Ollama only; a provider source needs one).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub embeddings_model: String,
    pub preset: harness_memory::Preset,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub mem0_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem0_key_secret: Option<String>,
    /// Qdrant's base URL (Qdrant backend). Its API key, if any, comes from
    /// `HARNESS_QDRANT_API_KEY`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub qdrant_url: String,
    /// Collection prefix; empty: [`QDRANT_COLLECTION`].
    #[serde(skip_serializing_if = "String::is_empty")]
    pub qdrant_collection: String,
}

impl MemoryConfig {
    pub fn qdrant_collection(&self) -> &str {
        if self.qdrant_collection.is_empty() {
            QDRANT_COLLECTION
        } else {
            &self.qdrant_collection
        }
    }

    pub fn model(&self) -> String {
        if self.embeddings_model.is_empty() {
            self.preset.ollama_model().to_string()
        } else {
            self.embeddings_model.clone()
        }
    }

    /// The Ollama base URL (Ollama source).
    pub fn ollama_url(&self) -> &str {
        if self.embeddings_url.is_empty() {
            OLLAMA_URL
        } else {
            &self.embeddings_url
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchKind {
    #[default]
    None,
    /// Self-hosted SearXNG (JSON format enabled). Keeps queries local.
    Searxng,
    /// Brave Search API (key required).
    Brave,
}

/// Web tools for chats. Changing them needs a restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WebConfig {
    pub search: SearchKind,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub searxng_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brave_key_env: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brave_key_secret: Option<String>,
    /// Offer `web_fetch` (reads pages the user or a search result linked).
    pub fetch: bool,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            search: SearchKind::None,
            searxng_url: String::new(),
            brave_key_env: None,
            brave_key_secret: None,
            fetch: true,
        }
    }
}

/// Chat runtime behaviour.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ChatConfig {
    /// Incognito chats idle this long are deleted (master and chat runtime).
    pub incognito_ttl_minutes: u64,
    /// When the estimated history passes this many tokens, older turns are
    /// replaced by a summary (0 disables summarising).
    pub summarize_after_tokens: usize,
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            incognito_ttl_minutes: 24 * 60,
            summarize_after_tokens: 24_000,
        }
    }
}

impl ChatConfig {
    pub fn incognito_ttl(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.incognito_ttl_minutes.max(1) * 60)
    }
}

/// A model as the provider reported it, or as an admin described it
/// (`custom`). `None`: not reported (treated as unsupported at runtime).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    /// The provider's raw id (no prefix).
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<bool>,
    /// Added by an admin with hand-set parameters: listings never change
    /// it or mark it unavailable.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub custom: bool,
    /// Missing from the provider's latest listing. Kept (sessions and
    /// agents may still name it) until an admin removes it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unavailable: bool,
    /// When a listing first missed it (ms since the epoch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing_since: Option<i64>,
}

impl From<harness_agent_native::models::ModelMeta> for ModelConfig {
    fn from(m: harness_agent_native::models::ModelMeta) -> Self {
        Self {
            id: m.id,
            name: m.name,
            context: m.context,
            max_output: m.max_output,
            effort: m.effort,
            vision: m.vision,
            tools: m.tools,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// Any OpenAI-compatible chat-completions endpoint.
    Openai,
    /// Anthropic Messages API, API key only (no subscription OAuth: ToS).
    Anthropic,
    /// Built-in scripted provider (no network, for testing the UI).
    Mock,
}

/// One `[[providers]]` entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    /// Also the model-id prefix: `^[a-z0-9][a-z0-9_-]{0,31}$`.
    pub id: String,
    pub name: String,
    /// The preset it was made from (`presets.rs`), `demo` for the mock.
    pub preset: String,
    pub kind: ProviderKind,
    /// Empty for the mock.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub base_url: String,
    /// Name of the environment variable holding the API key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    /// Name of the key in `secrets.toml`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_secret: Option<String>,
    /// Default reasoning effort, sent only to models with `effort = true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<harness_agent_native::Effort>,
    /// Delay between mock chunks, in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mock_delay_ms: Option<u64>,
    #[serde(default)]
    pub models: Vec<ModelConfig>,
}

impl ProviderConfig {
    pub fn demo(delay_ms: u64) -> Self {
        Self {
            id: crate::presets::DEMO.into(),
            name: "Demo".into(),
            preset: crate::presets::DEMO.into(),
            kind: ProviderKind::Mock,
            base_url: String::new(),
            api_key_env: None,
            api_key_secret: None,
            effort: None,
            mock_delay_ms: Some(delay_ms),
            models: vec![ModelConfig {
                id: "mock".into(),
                ..Default::default()
            }],
        }
    }

    pub fn model(&self, raw: &str) -> Option<&ModelConfig> {
        self.models.iter().find(|m| m.id == raw)
    }
}

/// Whether `m` of `p` takes a reasoning effort. Never for Anthropic until
/// signed thinking blocks are replayed (PLAN.md R8), whatever the file says.
pub fn effort_allowed(p: &ProviderConfig, m: &ModelConfig) -> bool {
    p.kind != ProviderKind::Anthropic && m.effort == Some(true)
}

/// Whether `id` may be a provider id (it is the visible model prefix).
pub fn valid_provider_id(id: &str) -> bool {
    let b = id.as_bytes();
    (1..=32).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-')
}

/// https, or plain http to this machine only; never with userinfo (which
/// would hide the real host behind a `localhost` prefix).
pub fn url_allowed(u: &str) -> bool {
    let Ok(p) = reqwest::Url::parse(u) else {
        return false;
    };
    let no_userinfo = p.username().is_empty() && p.password().is_none();
    no_userinfo && p.host_str().is_some() && (p.scheme() == "https" || is_local_url(&p))
}

fn is_local_url(p: &reqwest::Url) -> bool {
    p.scheme() == "http"
        && p.username().is_empty()
        && p.password().is_none()
        && p.host_str().is_some_and(|h| {
            matches!(h, "localhost" | "127.0.0.1" | "[::1]")
                || plain_http_hosts().iter().any(|a| a.eq_ignore_ascii_case(h))
        })
}

/// Host names the operator allows over plain http besides this machine
/// (`HARNESS_PLAIN_HTTP_HOSTS=ollama,vllm`): services on a private
/// container network, which have no certificate. Read once.
fn plain_http_hosts() -> &'static [String] {
    static HOSTS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    HOSTS.get_or_init(|| {
        std::env::var("HARNESS_PLAIN_HTTP_HOSTS")
            .unwrap_or_default()
            .split(',')
            .map(|h| h.trim().to_string())
            .filter(|h| !h.is_empty())
            .collect()
    })
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let c = Self::load_raw(path)?;
        c.validate()?;
        Ok(c)
    }

    /// Parse without validating (the environment may complete it).
    pub fn load_raw(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(s) => toml::from_str(&s).with_context(|| format!("parsing {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    /// The config the server runs with: `config.toml` with the
    /// environment's settings on top (see `env.rs`), after writing the ones
    /// the file lacks into it.
    pub fn load_with_env(path: &Path, env: &crate::env::EnvSettings) -> anyhow::Result<Self> {
        env.seed_file(path)?;
        let mut c = Self::load_raw(path)?;
        env.apply(&mut c)?;
        c.normalize();
        c.validate()?;
        Ok(c)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.providers.is_empty() {
            bail!("no providers");
        }
        for (i, p) in self.providers.iter().enumerate() {
            let at = format!("[[providers]] {:?}", p.id);
            if !valid_provider_id(&p.id) {
                bail!(
                    "{at}: id must be 1-32 of a-z, 0-9, _ and - (starting with a letter or digit)"
                );
            }
            if self.providers[..i].iter().any(|q| q.id == p.id) {
                bail!("{at}: the id is used twice");
            }
            if p.api_key_env.is_some() && p.api_key_secret.is_some() {
                bail!("{at}: set api_key_env or api_key_secret, not both");
            }
            if p.models
                .iter()
                .any(|m| m.id.trim().is_empty() || m.id.contains("::"))
            {
                bail!("{at}: model ids must be non-empty and must not contain \"::\"");
            }
            if p.kind == ProviderKind::Mock {
                continue;
            }
            // A provider may list no chat model (memory embeddings only).
            if p.kind == ProviderKind::Anthropic
                && p.api_key_env.is_none()
                && p.api_key_secret.is_none()
            {
                bail!("{at}: Anthropic needs an API key (api_key_env or api_key_secret)");
            }
            let u = p.base_url.as_str();
            if !url_allowed(u) {
                bail!("{at}: base_url must be https:// (plain http only for localhost)");
            }
        }
        for (what, m) in self
            .default_model
            .iter()
            .map(|m| ("default_model", m))
            .chain(
                self.agent_models
                    .values()
                    .flat_map(AgentChain::models)
                    .map(|m| ("[agent_models]", m)),
            )
        {
            if self.resolve_model(m).is_none() {
                bail!("{what}: unknown model {m:?}");
            }
        }
        if self.memory.backend == MemoryBackend::Mem0 && self.memory.mem0_url.is_empty() {
            bail!("[memory] backend = \"mem0\" needs mem0_url");
        }
        if self.memory.backend == MemoryBackend::Qdrant {
            let u = &self.memory.qdrant_url;
            if !u.starts_with("http://") && !u.starts_with("https://") {
                bail!("[memory] backend = \"qdrant\" needs an http(s) qdrant_url");
            }
            if self.memory.recall != Recall::Meaning {
                bail!(
                    "[memory] backend = \"qdrant\" stores vectors: set recall = \"meaning\" and an embeddings model"
                );
            }
        }
        let mc = &self.memory;
        let eu = &mc.embeddings_url;
        if !eu.is_empty() && !eu.starts_with("http://") && !eu.starts_with("https://") {
            bail!("[memory] embeddings_url must be an http(s) URL");
        }
        if mc.recall == Recall::Meaning && mc.source == EmbedSource::Provider {
            if self.embed_provider().is_none() {
                bail!("[memory] provider_id must name a configured (non-demo) provider");
            }
            if mc.embeddings_model.is_empty() {
                bail!("[memory] embeddings_model is required unless source = \"ollama\"");
            }
        }
        match self.web.search {
            SearchKind::Searxng if self.web.searxng_url.is_empty() => {
                bail!("[web] search = \"searxng\" needs searxng_url")
            }
            SearchKind::Brave
                if self.web.brave_key_env.is_none() && self.web.brave_key_secret.is_none() =>
            {
                bail!("[web] search = \"brave\" needs brave_key_env or brave_key_secret")
            }
            _ => {}
        }
        Ok(())
    }

    /// Write atomically (temp file + rename). Holds no secrets.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let text = format!(
            "# Written by {} (onboarding / settings). Keys live in secrets.toml.\n{}",
            harness_core::APP_NAME,
            toml::to_string(self).context("serializing config")?
        );
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
        Ok(())
    }

    pub fn provider(&self, id: &str) -> Option<&ProviderConfig> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// A `<provider>::<raw>` model id to its provider and model, matched
    /// exactly.
    pub fn resolve_model(&self, id: &str) -> Option<(&ProviderConfig, &ModelConfig)> {
        let (prefix, raw) = harness_agent_native::llm::split_model(id)?;
        let p = self.provider(prefix)?;
        Some((p, p.model(raw)?))
    }

    /// Whether `id` names a configured model.
    pub fn knows_model(&self, id: &str) -> bool {
        self.resolve_model(id).is_some()
    }

    /// The model sessions get when nobody chose one: `default_model`, else
    /// agent `general`'s, else the first listed model (an available one
    /// first).
    pub fn effective_default(&self) -> Option<String> {
        let first = |available: bool| {
            self.providers.iter().find_map(|p| {
                p.models
                    .iter()
                    .find(|m| !(available && m.unavailable))
                    .map(|m| format!("{}::{}", p.id, m.id))
            })
        };
        self.default_model
            .iter()
            .chain(
                self.agent_models
                    .get(harness_agents::DEFAULT_AGENT)
                    .map(|c| &c.model),
            )
            .find(|m| self.knows_model(m))
            .cloned()
            .or_else(|| first(true))
            .or_else(|| first(false))
    }

    /// The admin's default for `agent`, falling back to the global default.
    pub fn agent_default(&self, agent: &str) -> Option<String> {
        self.agent_models
            .get(agent)
            .map(|c| &c.model)
            .filter(|m| self.knows_model(m))
            .cloned()
            .or_else(|| self.effective_default())
    }

    /// Whether `model` is saved but missing from its provider's latest
    /// listing.
    pub fn model_unavailable(&self, model: &str) -> bool {
        self.resolve_model(model)
            .is_some_and(|(_, m)| m.unavailable)
    }

    /// `chain` without repeats and models that no longer resolve. A main
    /// model that is gone gives way to the first fallback left; `None` when
    /// nothing is left.
    fn repair_chain(&self, chain: AgentChain) -> Option<AgentChain> {
        let mut ids: Vec<String> = Vec::new();
        for m in chain.models() {
            if self.knows_model(m) && !ids.contains(m) {
                ids.push(m.clone());
            }
        }
        let mut ids = ids.into_iter();
        Some(AgentChain {
            model: ids.next()?,
            fallbacks: ids.collect(),
        })
    }

    /// Repair references after providers or models changed: drop agent
    /// models that no longer resolve (a fallback takes a main model's
    /// place), fall back for a stale global
    /// default, and keep the demo provider when nothing else is left.
    pub fn normalize(&mut self) {
        // Without a chat model anywhere, the demo stays the one to chat with.
        if !self.has_chat_model() && !self.providers.iter().any(|p| p.kind == ProviderKind::Mock) {
            self.providers.push(ProviderConfig::demo(40));
        }
        let agent_models = std::mem::take(&mut self.agent_models);
        self.agent_models = agent_models
            .into_iter()
            .filter_map(|(a, c)| self.repair_chain(c).map(|c| (a, c)))
            .collect();
        if self
            .default_model
            .as_ref()
            .is_some_and(|m| !self.knows_model(m))
        {
            self.default_model = None;
        }
        if self.default_model.is_none() {
            self.default_model = self.effective_default();
        }
        // Embeddings from a provider that is gone fall back to keywords.
        let mc = &self.memory;
        if mc.recall == Recall::Meaning
            && mc.source == EmbedSource::Provider
            && self.embed_provider().is_none()
        {
            self.memory.recall = Recall::Keywords;
        }
    }

    /// The provider `[memory]` names for embeddings, unless it is the demo.
    pub fn embed_provider(&self) -> Option<&ProviderConfig> {
        self.provider(self.memory.provider_id.as_deref()?)
            .filter(|p| p.kind != ProviderKind::Mock)
    }

    /// Whether `model` is known to accept a reasoning-effort setting.
    pub fn supports_effort(&self, model: &str) -> bool {
        self.resolve_model(model)
            .is_some_and(|(p, m)| effort_allowed(p, m))
    }

    /// At least one real (non-mock) provider with a chat model (one that
    /// serves memory embeddings only is not enough).
    pub fn configured(&self) -> bool {
        self.has_chat_model()
    }

    /// Whether a real (non-mock) provider has at least one chat model.
    pub fn has_chat_model(&self) -> bool {
        self.providers
            .iter()
            .any(|p| p.kind != ProviderKind::Mock && !p.models.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEEPSEEK: &str = concat!(
        "[[providers]]\nid = \"ds\"\nname = \"DeepSeek\"\npreset = \"custom_openai\"\n",
        "kind = \"openai\"\nmodels = [{ id = \"deepseek-chat\" }]\n",
    );

    #[test]
    fn parses_and_validates() {
        let c: Config = toml::from_str(&format!(
            "{DEEPSEEK}base_url = \"https://api.deepseek.com\"\napi_key_env = \"DEEPSEEK_API_KEY\"\n"
        ))
        .unwrap();
        c.validate().unwrap();
        let p = &c.providers[0];
        assert_eq!(p.api_key_env.as_deref(), Some("DEEPSEEK_API_KEY"));
        assert_eq!(c.default_model.as_deref(), Some("ds::deepseek-chat"));
        assert!(c.configured());
        for base in ["http://example.com", "http://localhost.evil.com/v1", ""] {
            let bad: Config =
                toml::from_str(&format!("{DEEPSEEK}base_url = \"{base}\"\n")).unwrap();
            assert!(bad.validate().is_err(), "{base:?}");
        }
        assert!(
            toml::from_str::<Config>(&format!(
                "{DEEPSEEK}base_url = \"https://api.deepseek.com\"\napi_key = \"sk-x\"\n"
            ))
            .is_err(),
            "no inline keys"
        );
        let mock: Config = toml::from_str(
            "[[providers]]\nid = \"demo\"\nname = \"Demo\"\npreset = \"demo\"\nkind = \"mock\"\nmock_delay_ms = 5\nmodels = [{ id = \"mock\" }]\n",
        )
        .unwrap();
        assert_eq!(mock.providers[0].mock_delay_ms, Some(5));
        assert_eq!(mock.default_model.as_deref(), Some("demo::mock"));
        assert!(!mock.configured());
        let empty: Config = toml::from_str("").unwrap();
        assert_eq!(empty.default_model.as_deref(), Some("demo::mock"));
        assert!(!empty.configured());
    }

    /// Plain http only to this machine: a `localhost` prefix hiding the
    /// real host in the userinfo does not count, and https takes none.
    #[test]
    fn url_allowed_parses_the_host() {
        for ok in [
            "https://api.example.com/v1",
            "http://localhost:11434/v1",
            "http://localhost",
            "http://127.0.0.1:8080/v1",
            "http://[::1]:8000/v1",
        ] {
            assert!(url_allowed(ok), "{ok}");
        }
        for bad in [
            "http://localhost:x@evil.example/v1",
            "http://127.0.0.1:1@evil.example",
            "http://[::1]:1@evil.example/",
            "http://localhost@evil.example",
            "http://localhost.evil.com/v1",
            "https://user:pw@api.example.com/v1",
            "http://example.com/v1",
            "ftp://localhost/",
            "not a url",
        ] {
            assert!(!url_allowed(bad), "{bad}");
        }
    }

    #[test]
    fn anthropic_needs_a_key_and_takes_no_effort() {
        let ant = concat!(
            "[[providers]]\nid = \"anthropic\"\nname = \"Anthropic\"\npreset = \"anthropic\"\n",
            "kind = \"anthropic\"\nbase_url = \"https://api.anthropic.com/v1\"\neffort = \"high\"\n",
            "models = [{ id = \"claude-x\", effort = true }]\n",
        );
        let c: Config =
            toml::from_str(&format!("{ant}api_key_secret = \"provider-anthropic\"\n")).unwrap();
        c.validate().unwrap();
        // Listed with effort, but never for Anthropic (PLAN.md R8).
        assert!(!c.supports_effort("anthropic::claude-x"));
        assert!(!c.supports_effort("other"));
        assert!(
            crate::providers::model_list(&c).iter().all(|m| !m.effort),
            "the model list agrees"
        );
        let nokey: Config = toml::from_str(ant).unwrap();
        assert!(nokey.validate().is_err());
        let both: Config = toml::from_str(&format!(
            "{DEEPSEEK}base_url = \"https://api.deepseek.com\"\napi_key_env = \"A\"\napi_key_secret = \"b\"\n"
        ))
        .unwrap();
        assert!(both.validate().is_err());
    }

    /// Agent chains round-trip, and a main model that is gone gives way
    /// to its first fallback.
    #[test]
    fn agent_chains_round_trip_and_repair() {
        let providers = concat!(
            "[[providers]]\nid = \"a\"\nname = \"A\"\npreset = \"custom_openai\"\nkind = \"openai\"\n",
            "base_url = \"http://localhost:8000/v1\"\nmodels = [{ id = \"m1\" }, { id = \"m2\" }, { id = \"m3\" }]\n",
        );
        let c: Config = toml::from_str(&format!(
            "[agent_models.coder]\nmodel = \"a::m2\"\n\n[agent_models.general]\nmodel = \"a::m1\"\n\n{providers}"
        ))
        .unwrap();
        assert_eq!(c.agent_models["general"], AgentChain::new("a::m1"));
        assert_eq!(c.agent_models["coder"], AgentChain::new("a::m2"));

        let mut c = c.clone();
        c.agent_models.insert(
            "writer".into(),
            AgentChain {
                model: "a::m3".into(),
                fallbacks: vec![
                    "a::m1".into(),
                    "a::gone".into(),
                    "a::m1".into(),
                    "a::m2".into(),
                ],
            },
        );
        c.agent_avatars.insert(
            "coder".into(),
            crate::avatars::AgentAvatar::default_for("x"),
        );
        c.normalize();
        assert_eq!(c.agent_models["writer"].fallbacks, ["a::m1", "a::m2"]);
        let text = toml::to_string(&c).unwrap();
        assert!(text.contains("[agent_models.writer]"), "{text}");
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.agent_models, c.agent_models);
        assert_eq!(back.agent_avatars, c.agent_avatars);

        let mut gone = back;
        gone.providers[0].models.retain(|m| m.id != "m3");
        gone.normalize();
        assert_eq!(
            gone.agent_models["writer"],
            AgentChain {
                model: "a::m1".into(),
                fallbacks: vec!["a::m2".into()],
            }
        );
        // Validation checks fallbacks too.
        let mut bad = gone.clone();
        bad.agent_models.get_mut("coder").unwrap().fallbacks = vec!["a::nope".into()];
        assert!(bad.validate().is_err());
    }

    #[test]
    fn defaults_and_ids() {
        let mut c: Config = toml::from_str(concat!(
            "default_model = \"gone::x\"\n",
            "[agent_models.coder]\nmodel = \"b::m2\"\n\n[agent_models.general]\nmodel = \"a::m1\"\n\n",
            "[agent_models.writer]\nmodel = \"gone::y\"\n\n",
            "[[providers]]\nid = \"a\"\nname = \"A\"\npreset = \"custom_openai\"\nkind = \"openai\"\n",
            "base_url = \"http://localhost:8000/v1\"\nmodels = [{ id = \"m1\" }]\n\n",
            "[[providers]]\nid = \"b\"\nname = \"B\"\npreset = \"custom_openai\"\nkind = \"openai\"\n",
            "base_url = \"http://localhost:8001/v1\"\nmodels = [{ id = \"m2\", context = 8192 }, { id = \"m1\" }]\n",
        ))
        .unwrap();
        c.validate().unwrap();
        // The stale default falls back to agent general's model.
        assert_eq!(c.default_model.as_deref(), Some("a::m1"));
        assert!(!c.agent_models.contains_key("writer"));
        assert_eq!(c.agent_default("coder").as_deref(), Some("b::m2"));
        assert_eq!(c.agent_default("writer").as_deref(), Some("a::m1"));
        assert!(c.resolve_model("b::m1").is_some());
        assert!(c.resolve_model("b::m3").is_none());
        assert!(
            c.resolve_model("m1").is_none(),
            "an id needs its provider prefix"
        );
        c.providers[1].id = "a".into();
        assert!(c.validate().is_err(), "duplicate id");
        for (id, ok) in [
            ("nim", true),
            ("x_1-2", true),
            ("-x", false),
            ("Up", false),
            ("", false),
        ] {
            assert_eq!(valid_provider_id(id), ok, "{id}");
        }
        assert!(!valid_provider_id(&"a".repeat(33)));
    }

    #[test]
    fn local_node_is_for_admins_unless_granted() {
        let c = Config::default();
        assert_eq!(c.nodes.local_node_users, LocalNodeUsers::Admins);
        assert!(!toml::to_string(&c).unwrap().contains("[nodes]"));
        let c: Config = toml::from_str("[nodes]\nlocal_node_users = \"granted\"\n").unwrap();
        assert_eq!(c.nodes.local_node_users, LocalNodeUsers::Granted);
        let text = toml::to_string(&c).unwrap();
        assert!(text.contains("local_node_users = \"granted\""), "{text}");
        let c: Config = toml::from_str("[nodes]\nlocal_node_users = \"admins\"\n").unwrap();
        assert_eq!(c.nodes.local_node_users, LocalNodeUsers::Admins);
        assert!(toml::from_str::<Config>("[nodes]\nlocal_node_users = \"everyone\"\n").is_err());
        assert!(toml::from_str::<Config>("[nodes]\nnope = 1\n").is_err());
    }

    #[test]
    fn workspaces_policy_is_written_only_when_changed() {
        let c = Config::default();
        assert!(c.workspaces.members_can_create);
        assert!(!toml::to_string(&c).unwrap().contains("[workspaces]"));
        let mut c: Config = toml::from_str("[workspaces]\nmembers_can_create = false\n").unwrap();
        assert!(!c.workspaces.members_can_create);
        let text = toml::to_string(&c).unwrap();
        assert!(text.contains("[workspaces]"), "{text}");
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.workspaces, c.workspaces);
        c.workspaces.members_can_create = true;
        assert!(!toml::to_string(&c).unwrap().contains("[workspaces]"));
        assert!(toml::from_str::<Config>("[workspaces]\nnope = 1\n").is_err());
    }

    #[test]
    fn memory_sources() {
        let m = toml::from_str::<Config>("[memory]\npreset = \"best\"\n")
            .unwrap()
            .memory;
        assert_eq!(
            (m.recall, m.source),
            (Recall::Keywords, EmbedSource::Provider)
        );
        assert_eq!(Config::default().memory.recall, Recall::Keywords);
        // Ollama: no model needed (the preset's), and it round-trips.
        let mut c = Config::default();
        c.memory.recall = Recall::Meaning;
        c.memory.source = EmbedSource::Ollama;
        c.memory.embeddings_url = "http://localhost:11434/v1".into();
        c.validate().unwrap();
        assert_eq!(c.memory.model(), c.memory.preset.ollama_model());
        let back: Config = toml::from_str(&toml::to_string(&c).unwrap()).unwrap();
        assert_eq!(back.memory.source, EmbedSource::Ollama);
        assert_eq!(back.memory.ollama_url(), "http://localhost:11434/v1");
        c.memory.embeddings_url = "localhost:11434".into();
        assert!(c.validate().is_err(), "not an http(s) URL");
        // Provider source: a real provider and a model.
        let mut c: Config = toml::from_str(&format!(
            "{DEEPSEEK}base_url = \"https://api.deepseek.com\"\n\n[memory]\nrecall = \"meaning\"\nprovider_id = \"ds\"\n"
        ))
        .unwrap();
        assert!(c.validate().is_err(), "a provider source needs a model");
        c.memory.embeddings_model = "e".into();
        c.validate().unwrap();
        // The demo is no provider for embeddings, and a provider that is
        // gone falls back to keywords.
        let mut c = Config::default();
        c.memory.recall = Recall::Meaning;
        c.memory.source = EmbedSource::Provider;
        c.memory.provider_id = Some(crate::presets::DEMO.into());
        c.memory.embeddings_model = "e".into();
        assert!(c.validate().is_err());
        c.normalize();
        assert_eq!(c.memory.recall, Recall::Keywords);
        c.validate().unwrap();
    }
}
