//! `harness.toml`: master-side configuration. API keys are never stored here:
//! they are referenced by environment-variable name (`api_key_env`) or by name
//! in the 0600 `secrets.toml` (`api_key_secret`).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

/// The whole file. Older files with a single `[provider]` and a global
/// `[[models]]` still load: they become one entry of `[[providers]]` (see
/// `RawConfig`), and the next save writes the new shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "RawConfig")]
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
        }
    }
}

/// What the file may contain, old shape included.
#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct RawConfig {
    provider: Option<LegacyProvider>,
    models: Vec<ModelConfig>,
    providers: Vec<ProviderConfig>,
    default_model: Option<String>,
    agent_models: BTreeMap<String, AgentChain>,
    agent_avatars: BTreeMap<String, crate::avatars::AgentAvatar>,
    chat: ChatConfig,
    web: WebConfig,
    memory: MemoryConfig,
    voice: VoiceConfig,
    savings: harness_core::savings::TokenSavings,
}

impl TryFrom<RawConfig> for Config {
    type Error = String;

    fn try_from(r: RawConfig) -> Result<Self, String> {
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
        };
        match r.provider {
            Some(_) if !c.providers.is_empty() => {
                return Err("use [[providers]] or the older [provider], not both".into());
            }
            Some(legacy) => {
                let (p, model) = legacy.migrate(r.models);
                if c.default_model.is_none() {
                    c.default_model = Some(format!("{}::{model}", p.id));
                }
                c.providers.push(p);
            }
            None if !r.models.is_empty() => {
                return Err("[[models]] needs [provider]; list models under [[providers]]".into());
            }
            None => {}
        }
        c.normalize();
        Ok(c)
    }
}

/// An agent's model and, in order, the models tried when it is unavailable
/// or fails before answering. Files before fallbacks existed (up to commit
/// 2a501fe) hold a bare model id: `general = "a::m1"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "RawChain")]
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

#[derive(Deserialize)]
#[serde(untagged)]
enum RawChain {
    Bare(String),
    Full {
        model: String,
        #[serde(default)]
        fallbacks: Vec<String>,
    },
}

impl From<RawChain> for AgentChain {
    fn from(r: RawChain) -> Self {
        match r {
            RawChain::Bare(model) => AgentChain::new(model),
            RawChain::Full { model, fallbacks } => AgentChain { model, fallbacks },
        }
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryBackend {
    /// SQLite in the data dir; keyword recall, plus vectors if embeddings are set.
    #[default]
    Builtin,
    /// A self-hosted mem0 server (restart to switch).
    Mem0,
    Off,
}

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
    /// Any endpoint with its own key (`embeddings_key_secret`), sent the
    /// way `api_style` says.
    Custom,
}

pub const OLLAMA_URL: &str = "http://localhost:11434/v1";

/// The `secrets.toml` name of a custom embeddings endpoint's key.
pub const EMBEDDINGS_SECRET: &str = "embeddings";

/// Long-term memory. Embedding settings apply live; the backend needs a restart.
/// Files from before `source` existed are read through `RawMemory`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "RawMemory")]
pub struct MemoryConfig {
    pub backend: MemoryBackend,
    pub recall: Recall,
    pub source: EmbedSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    /// Base URL serving `/embeddings` (Ollama and custom sources).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub embeddings_url: String,
    /// Empty: the preset's model (Ollama only; the others need one).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub embeddings_model: String,
    /// Custom sources only; a provider source follows the provider's kind.
    pub api_style: harness_memory::ApiStyle,
    pub preset: harness_memory::Preset,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embeddings_key_secret: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub mem0_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem0_key_secret: Option<String>,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        RawMemory::default().into()
    }
}

/// `[memory]` as written, `recall` and `source` missing in older files.
#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct RawMemory {
    backend: MemoryBackend,
    recall: Option<Recall>,
    source: Option<EmbedSource>,
    provider_id: Option<String>,
    embeddings_url: String,
    embeddings_model: String,
    api_style: harness_memory::ApiStyle,
    preset: harness_memory::Preset,
    embeddings_key_secret: Option<String>,
    mem0_url: String,
    mem0_key_secret: Option<String>,
}

impl From<RawMemory> for MemoryConfig {
    /// Older files: a URL meant meaning recall, from Ollama when it is the
    /// local Ollama port without a key, else from a custom endpoint; no URL
    /// meant keywords only.
    fn from(r: RawMemory) -> Self {
        let url = r.embeddings_url.trim_end_matches('/').to_string();
        let (recall, source) = match r.source {
            Some(s) => (r.recall.unwrap_or_default(), s),
            None if url.is_empty() => (Recall::Keywords, EmbedSource::default()),
            None => {
                let ollama = is_local(&url)
                    && host_of(&url).ends_with(":11434")
                    && r.embeddings_key_secret.is_none();
                let s = if ollama {
                    EmbedSource::Ollama
                } else {
                    EmbedSource::Custom
                };
                (r.recall.unwrap_or(Recall::Meaning), s)
            }
        };
        // Older files left the model empty to mean the preset's model; a
        // custom source needs it written out.
        let mut embeddings_model = r.embeddings_model;
        if r.source.is_none() && source == EmbedSource::Custom && embeddings_model.is_empty() {
            embeddings_model = r.preset.ollama_model().to_string();
        }
        Self {
            backend: r.backend,
            recall,
            source,
            provider_id: r.provider_id,
            embeddings_url: url,
            embeddings_model,
            api_style: r.api_style,
            preset: r.preset,
            embeddings_key_secret: r.embeddings_key_secret,
            mem0_url: r.mem0_url,
            mem0_key_secret: r.mem0_key_secret,
        }
    }
}

impl MemoryConfig {
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

/// The single `[provider]` of older files.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct LegacyProvider {
    kind: ProviderKind,
    base_url: String,
    model: String,
    api_key_env: Option<String>,
    api_key_secret: Option<String>,
    effort: Option<harness_agent_native::Effort>,
    mock_delay_ms: u64,
}

impl Default for LegacyProvider {
    fn default() -> Self {
        Self {
            kind: ProviderKind::Mock,
            base_url: String::new(),
            model: "mock".into(),
            api_key_env: None,
            api_key_secret: None,
            effort: None,
            mock_delay_ms: 40,
        }
    }
}

impl LegacyProvider {
    /// One `[[providers]]` entry and its default model's raw id. The id is
    /// the preset whose URL matches, else one made from the host.
    fn migrate(self, models: Vec<ModelConfig>) -> (ProviderConfig, String) {
        if self.kind == ProviderKind::Mock {
            return (ProviderConfig::demo(self.mock_delay_ms), "mock".into());
        }
        let base = match (self.base_url.trim(), self.kind) {
            ("", ProviderKind::Anthropic) => "https://api.anthropic.com/v1".to_string(),
            ("", _) => "https://api.openai.com/v1".to_string(),
            (u, _) => u.to_string(),
        };
        let (id, name, preset) = match crate::presets::by_url(self.kind, &base) {
            Some(p) => (p.id.to_string(), p.label.to_string(), p.id.to_string()),
            None => (
                legacy_custom_id(&base),
                host_of(&base),
                crate::presets::custom_for(self.kind).to_string(),
            ),
        };
        let mut models = models;
        if !self.model.is_empty() && !models.iter().any(|m| m.id == self.model) {
            models.insert(
                0,
                ModelConfig {
                    id: self.model.clone(),
                    ..Default::default()
                },
            );
        }
        (
            ProviderConfig {
                id,
                name,
                preset,
                kind: self.kind,
                base_url: base,
                api_key_env: self.api_key_env,
                api_key_secret: self.api_key_secret,
                effort: self.effort,
                mock_delay_ms: None,
                models,
            },
            self.model,
        )
    }
}

/// `api.deepseek.com` from `https://api.deepseek.com/v1` (with `:port` when
/// it is not the scheme's default). Never the userinfo.
pub fn host_of(url: &str) -> String {
    if let Ok(u) = reqwest::Url::parse(url)
        && let Some(h) = u.host_str()
    {
        return match u.port() {
            Some(p) => format!("{h}:{p}"),
            None => h.to_string(),
        };
    }
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let auth = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    auth.rsplit('@').next().unwrap_or(auth).to_string()
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

fn is_local(u: &str) -> bool {
    reqwest::Url::parse(u).is_ok_and(|p| is_local_url(&p))
}

fn is_local_url(p: &reqwest::Url) -> bool {
    p.scheme() == "http"
        && p.username().is_empty()
        && p.password().is_none()
        && matches!(p.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
}

/// A provider id (model prefix) for a migrated custom endpoint, from its
/// host: never a preset's id, so adding that preset later cannot replace it.
fn legacy_custom_id(base: &str) -> String {
    let id: String = host_of(base)
        .to_ascii_lowercase()
        .chars()
        .map(|c| match c {
            'a'..='z' | '0'..='9' | '_' | '-' => c,
            _ => '-',
        })
        .collect();
    let id: String = id.trim_matches(['-', '_']).chars().take(32).collect();
    let id = id.trim_end_matches(['-', '_']).to_string();
    let taken =
        crate::presets::get(&id).is_some() || crate::presets::RESERVED_IDS.contains(&id.as_str());
    if !valid_provider_id(&id) || taken {
        "custom".to_string()
    } else {
        id
    }
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(s) => {
                let c: Config =
                    toml::from_str(&s).with_context(|| format!("parsing {}", path.display()))?;
                c.validate()?;
                Ok(c)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
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
        let mc = &self.memory;
        let eu = &mc.embeddings_url;
        if !eu.is_empty() && !eu.starts_with("http://") && !eu.starts_with("https://") {
            bail!("[memory] embeddings_url must be an http(s) URL");
        }
        if mc.recall == Recall::Meaning {
            match mc.source {
                EmbedSource::Provider if self.embed_provider().is_none() => {
                    bail!("[memory] provider_id must name a configured (non-demo) provider")
                }
                EmbedSource::Custom if eu.is_empty() => {
                    bail!("[memory] source = \"custom\" needs embeddings_url")
                }
                EmbedSource::Custom if mc.embeddings_key_secret.is_some() && !url_allowed(eu) => {
                    bail!(
                        "[memory] embeddings_url must be https:// to send a key (plain http only for localhost)"
                    )
                }
                _ => {}
            }
            if mc.source != EmbedSource::Ollama && mc.embeddings_model.is_empty() {
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

    /// A model id to its provider and model. Prefixed ids must match
    /// exactly; a legacy unprefixed id is the first provider listing it.
    pub fn resolve_model(&self, id: &str) -> Option<(&ProviderConfig, &ModelConfig)> {
        match harness_agent_native::llm::split_model(id) {
            Some((prefix, raw)) => {
                let p = self.provider(prefix)?;
                Some((p, p.model(raw)?))
            }
            None => self
                .providers
                .iter()
                .find_map(|p| p.model(id).map(|m| (p, m))),
        }
    }

    /// The canonical (prefixed) form of a model id, if it is known.
    pub fn canonical_model(&self, id: &str) -> Option<String> {
        self.resolve_model(id)
            .map(|(p, m)| format!("{}::{}", p.id, m.id))
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
            .find_map(|m| self.canonical_model(m))
            .or_else(|| first(true))
            .or_else(|| first(false))
    }

    /// The admin's default for `agent`, falling back to the global default.
    pub fn agent_default(&self, agent: &str) -> Option<String> {
        self.agent_models
            .get(agent)
            .and_then(|c| self.canonical_model(&c.model))
            .or_else(|| self.effective_default())
    }

    /// Whether `model` is saved but missing from its provider's latest
    /// listing.
    pub fn model_unavailable(&self, model: &str) -> bool {
        self.resolve_model(model)
            .is_some_and(|(_, m)| m.unavailable)
    }

    /// `chain` with canonical ids, without repeats and models that no
    /// longer resolve. A main model that is gone gives way to the first
    /// fallback left; `None` when nothing is left.
    fn repair_chain(&self, chain: AgentChain) -> Option<AgentChain> {
        let mut ids: Vec<String> = Vec::new();
        for m in chain.models() {
            if let Some(c) = self.canonical_model(m)
                && !ids.contains(&c)
            {
                ids.push(c);
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
        if let Some(m) = &self.default_model {
            self.default_model = self.canonical_model(m);
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

    /// Whether `model` (prefixed or legacy) is known to accept a
    /// reasoning-effort setting.
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

    #[test]
    fn parses_and_validates() {
        let c: Config = toml::from_str(
            "[provider]\nkind = \"openai\"\nbase_url = \"https://api.deepseek.com\"\nmodel = \"deepseek-chat\"\napi_key_env = \"DEEPSEEK_API_KEY\"\n",
        )
        .unwrap();
        c.validate().unwrap();
        let p = &c.providers[0];
        assert_eq!(p.api_key_env.as_deref(), Some("DEEPSEEK_API_KEY"));
        // No preset has exactly this URL: a custom entry named after the
        // host, under an id no preset uses.
        assert_eq!(
            (p.id.as_str(), p.preset.as_str()),
            ("api-deepseek-com", "custom_openai")
        );
        assert_eq!(p.name, "api.deepseek.com");
        assert_eq!(
            c.default_model.as_deref(),
            Some("api-deepseek-com::deepseek-chat")
        );
        for (base, id) in [
            ("http://localhost:1234/v1", "localhost-1234"),
            ("https://api.openai.com/v1/", "openai"),
            ("https://OpenAI/v1", "custom"),
        ] {
            let c: Config = toml::from_str(&format!(
                "[provider]\nkind = \"openai\"\nbase_url = \"{base}\"\nmodel = \"m\"\napi_key_env = \"K\"\n"
            ))
            .unwrap();
            assert_eq!(c.providers[0].id, id, "{base}");
        }
        let bad: Config = toml::from_str(
            "[provider]\nkind = \"openai\"\nbase_url = \"http://example.com\"\nmodel = \"x\"\n",
        )
        .unwrap();
        assert!(bad.validate().is_err());
        assert!(toml::from_str::<Config>("[provider]\napi_key = \"sk-x\"\n").is_err());
        let local: Config = toml::from_str(
            "[provider]\nkind = \"openai\"\nbase_url = \"http://localhost.evil.com/v1\"\nmodel = \"x\"\n",
        )
        .unwrap();
        assert!(local.validate().is_err(), "not localhost");
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
        assert_eq!(
            host_of("http://localhost:x@evil.example/v1"),
            "evil.example"
        );
        assert_eq!(host_of("https://api.deepseek.com/v1"), "api.deepseek.com");
        assert_eq!(host_of("http://localhost:11434/v1"), "localhost:11434");
    }

    #[test]
    fn anthropic_needs_key_and_defaults_endpoint() {
        let c: Config = toml::from_str(
            "[provider]\nkind = \"anthropic\"\nmodel = \"claude-x\"\napi_key_secret = \"anthropic\"\neffort = \"high\"\n\n[[models]]\nid = \"claude-x\"\neffort = true\n",
        )
        .unwrap();
        c.validate().unwrap();
        let p = &c.providers[0];
        assert_eq!(
            (p.id.as_str(), p.preset.as_str()),
            ("anthropic", "anthropic")
        );
        assert_eq!(p.base_url, "https://api.anthropic.com/v1");
        assert_eq!(p.api_key_secret.as_deref(), Some("anthropic"));
        // Listed with effort, but never for Anthropic (PLAN.md R8).
        assert!(!c.supports_effort("claude-x"));
        assert!(!c.supports_effort("anthropic::claude-x"));
        assert!(!c.supports_effort("other"));
        assert!(
            crate::providers::model_list(&c).iter().all(|m| !m.effort),
            "the model list agrees"
        );
        let nokey: Config =
            toml::from_str("[provider]\nkind = \"anthropic\"\nmodel = \"claude-x\"\n").unwrap();
        assert!(nokey.validate().is_err());
        let both: Config = toml::from_str(
            "[provider]\nkind = \"openai\"\nmodel = \"m\"\napi_key_env = \"A\"\napi_key_secret = \"b\"\n",
        )
        .unwrap();
        assert!(both.validate().is_err());
    }

    /// An old file loads as one provider, keeps its models and key, and is
    /// saved in the new shape, which loads back the same.
    #[test]
    fn legacy_file_migrates_and_round_trips() {
        let c: Config = toml::from_str(concat!(
            "[provider]\nkind = \"openai\"\nbase_url = \"https://openrouter.ai/api/v1/\"\n",
            "model = \"openai/gpt-4o\"\napi_key_secret = \"openai\"\n\n",
            "[[models]]\nid = \"openai/gpt-4o\"\nvision = true\n\n",
            "[[models]]\nid = \"deepseek/r1\"\neffort = true\n\n",
            "[chat]\nsummarize_after_tokens = 0\n",
        ))
        .unwrap();
        c.validate().unwrap();
        assert_eq!(c.providers.len(), 1);
        let p = &c.providers[0];
        assert_eq!(
            (p.id.as_str(), p.name.as_str()),
            ("openrouter", "OpenRouter")
        );
        assert_eq!(p.api_key_secret.as_deref(), Some("openai"));
        assert_eq!(p.models.len(), 2);
        assert_eq!(p.models[0].vision, Some(true));
        assert_eq!(
            c.default_model.as_deref(),
            Some("openrouter::openai/gpt-4o")
        );
        assert!(c.supports_effort("openrouter::deepseek/r1"));
        assert_eq!(c.chat.summarize_after_tokens, 0);

        let text = toml::to_string(&c).unwrap();
        assert!(
            text.contains("[[providers]]") && !text.contains("[provider]\n"),
            "{text}"
        );
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.providers[0].models, p.models);
        assert_eq!(back.default_model, c.default_model);

        let mock: Config =
            toml::from_str("[provider]\nkind = \"mock\"\nmock_delay_ms = 5\n").unwrap();
        assert_eq!(mock.providers[0].id, "demo");
        assert_eq!(mock.providers[0].mock_delay_ms, Some(5));
        assert_eq!(mock.default_model.as_deref(), Some("demo::mock"));
        assert!(!mock.configured());
        let empty: Config = toml::from_str("").unwrap();
        assert_eq!(empty.default_model.as_deref(), Some("demo::mock"));
        assert!(toml::from_str::<Config>(
            "[provider]\nkind = \"mock\"\n\n[[providers]]\nid = \"demo\"\nname = \"D\"\npreset = \"demo\"\nkind = \"mock\"\n"
        )
        .is_err());
    }

    /// `[agent_models]` as commit 2a501fe wrote it (bare ids) loads as
    /// chains without fallbacks; the new shape round-trips, and a main
    /// model that is gone gives way to its first fallback.
    #[test]
    fn agent_chains_migrate_and_repair() {
        let providers = concat!(
            "[[providers]]\nid = \"a\"\nname = \"A\"\npreset = \"custom_openai\"\nkind = \"openai\"\n",
            "base_url = \"http://localhost:8000/v1\"\nmodels = [{ id = \"m1\" }, { id = \"m2\" }, { id = \"m3\" }]\n",
        );
        let old: Config = toml::from_str(&format!(
            "[agent_models]\ncoder = \"a::m2\"\ngeneral = \"m1\"\n\n{providers}"
        ))
        .unwrap();
        assert_eq!(old.agent_models["general"], AgentChain::new("a::m1"));
        assert_eq!(old.agent_models["coder"], AgentChain::new("a::m2"));

        let mut c = old.clone();
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
            "[agent_models]\ncoder = \"b::m2\"\ngeneral = \"a::m1\"\nwriter = \"gone::y\"\n\n",
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
        // Legacy unprefixed: the first provider listing it.
        assert_eq!(c.canonical_model("m1").as_deref(), Some("a::m1"));
        assert_eq!(c.canonical_model("m2").as_deref(), Some("b::m2"));
        assert_eq!(c.canonical_model("b::m3"), None);
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
    fn memory_legacy_and_sources() {
        let mem = |toml_text: &str| toml::from_str::<Config>(toml_text).unwrap().memory;
        // Older files: no `source`; the URL decides.
        let m = mem("[memory]\nembeddings_url = \"http://localhost:11434/v1/\"\n");
        assert_eq!((m.recall, m.source), (Recall::Meaning, EmbedSource::Ollama));
        assert_eq!(m.ollama_url(), "http://localhost:11434/v1");
        let m = mem(
            "[memory]\nembeddings_url = \"http://localhost:11434/v1\"\nembeddings_key_secret = \"k\"\n",
        );
        assert_eq!(m.source, EmbedSource::Custom, "a key means custom");
        let m = mem(
            "[memory]\nembeddings_url = \"https://api.example.com/v1\"\nembeddings_model = \"e\"\n",
        );
        assert_eq!((m.recall, m.source), (Recall::Meaning, EmbedSource::Custom));
        // An empty model meant the preset's; such files still load.
        for url in [
            "http://192.168.1.10:11434/v1",
            "http://ollama:11434/v1",
            "http://localhost:8080/v1",
        ] {
            let c: Config =
                toml::from_str(&format!("[memory]\nembeddings_url = \"{url}\"\n")).unwrap();
            c.validate().unwrap();
            assert_eq!(c.memory.source, EmbedSource::Custom);
            assert_eq!(c.memory.model(), c.memory.preset.ollama_model());
            assert_eq!(c.memory.embeddings_model, c.memory.preset.ollama_model());
        }
        let c: Config = toml::from_str(
            "[memory]\nembeddings_url = \"http://localhost:11434/v1\"\nembeddings_key_secret = \"k\"\n",
        )
        .unwrap();
        c.validate().unwrap();
        let m = mem("[memory]\npreset = \"best\"\n");
        assert_eq!(m.recall, Recall::Keywords);
        assert_eq!(Config::default().memory.recall, Recall::Keywords);
        // The new shape round-trips.
        let mut c = Config::default();
        c.memory.recall = Recall::Meaning;
        c.memory.source = EmbedSource::Custom;
        c.memory.embeddings_url = "https://e.example/v1".into();
        c.memory.embeddings_model = "embed-1".into();
        c.memory.api_style = harness_memory::ApiStyle::Anthropic;
        c.validate().unwrap();
        let back: Config = toml::from_str(&toml::to_string(&c).unwrap()).unwrap();
        assert_eq!(back.memory.source, EmbedSource::Custom);
        assert_eq!(back.memory.api_style, harness_memory::ApiStyle::Anthropic);
        // A key goes over https (or to localhost) only.
        c.memory.embeddings_url = "http://e.example/v1".into();
        c.memory.embeddings_key_secret = Some(EMBEDDINGS_SECRET.into());
        assert!(c.validate().is_err());
        c.memory.embeddings_model.clear();
        c.memory.embeddings_key_secret = None;
        assert!(c.validate().is_err(), "custom needs a model");
        // Provider source: the demo is no provider for embeddings, and a
        // provider that is gone falls back to keywords.
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
