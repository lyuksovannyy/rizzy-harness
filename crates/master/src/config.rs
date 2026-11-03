//! `harness.toml`: master-side configuration. API keys are never stored here:
//! they are referenced by environment-variable name (`api_key_env`) or by name
//! in the 0600 `secrets.toml` (`api_key_secret`).

use std::path::Path;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[derive(Default)]
pub struct Config {
    pub provider: ProviderConfig,
    /// Known models and what they support. Unlisted models are assumed to
    /// support tools but not effort, so effort is never sent blindly.
    pub models: Vec<ModelConfig>,
    pub chat: ChatConfig,
    pub web: WebConfig,
    pub memory: MemoryConfig,
    pub voice: VoiceConfig,
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

/// Long-term memory. Embedding settings apply live; the backend needs a restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MemoryConfig {
    pub backend: MemoryBackend,
    /// OpenAI-compatible base URL serving `/embeddings` (e.g. Ollama's
    /// `http://localhost:11434/v1`). Empty: keyword recall only.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub embeddings_url: String,
    /// Empty: the preset's model.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub embeddings_model: String,
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
        Self {
            backend: MemoryBackend::Builtin,
            embeddings_url: String::new(),
            embeddings_model: String::new(),
            preset: harness_memory::Preset::Balanced,
            embeddings_key_secret: None,
            mem0_url: String::new(),
            mem0_key_secret: None,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    pub id: String,
    #[serde(default)]
    pub effort: bool,
    #[serde(default)]
    pub vision: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    /// Empty means the kind's official endpoint.
    pub base_url: String,
    pub model: String,
    /// Name of the environment variable holding the API key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    /// Name of the key in `secrets.toml`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_secret: Option<String>,
    /// Default reasoning effort, sent only if the model lists `effort = true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<harness_agent_native::Effort>,
    /// Delay between mock chunks, in milliseconds.
    pub mock_delay_ms: u64,
}

impl Default for ProviderConfig {
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
        let p = &self.provider;
        if p.api_key_env.is_some() && p.api_key_secret.is_some() {
            bail!("[provider] set api_key_env or api_key_secret, not both");
        }
        if self.memory.backend == MemoryBackend::Mem0 && self.memory.mem0_url.is_empty() {
            bail!("[memory] backend = \"mem0\" needs mem0_url");
        }
        let eu = &self.memory.embeddings_url;
        if !eu.is_empty() && !eu.starts_with("http://") && !eu.starts_with("https://") {
            bail!("[memory] embeddings_url must be an http(s) URL");
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
        if p.kind != ProviderKind::Mock {
            if p.model.is_empty() || p.model == "mock" {
                bail!("[provider] model must be set for a real provider");
            }
            if p.kind == ProviderKind::Anthropic
                && p.api_key_env.is_none()
                && p.api_key_secret.is_none()
            {
                bail!("[provider] Anthropic needs an API key (api_key_env or api_key_secret)");
            }
            let u = self.base_url();
            let local = u.starts_with("http://127.0.0.1")
                || u.starts_with("http://localhost")
                || u.starts_with("http://[::1]");
            if !u.starts_with("https://") && !local {
                bail!("[provider] base_url must be https:// (plain http only for localhost)");
            }
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

    /// The configured base URL, or the kind's official endpoint.
    pub fn base_url(&self) -> &str {
        match (self.provider.base_url.as_str(), self.provider.kind) {
            ("", ProviderKind::Anthropic) => "https://api.anthropic.com/v1",
            ("", _) => "https://api.openai.com/v1",
            (u, _) => u,
        }
    }

    /// Whether `model` is known to accept a reasoning-effort setting.
    pub fn supports_effort(&self, model: &str) -> bool {
        self.models.iter().any(|m| m.id == model && m.effort)
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
        assert_eq!(c.provider.api_key_env.as_deref(), Some("DEEPSEEK_API_KEY"));
        let bad: Config = toml::from_str(
            "[provider]\nkind = \"openai\"\nbase_url = \"http://example.com\"\nmodel = \"x\"\n",
        )
        .unwrap();
        assert!(bad.validate().is_err());
        assert!(toml::from_str::<Config>("[provider]\napi_key = \"sk-x\"\n").is_err());
    }

    #[test]
    fn anthropic_needs_key_and_defaults_endpoint() {
        let c: Config = toml::from_str(
            "[provider]\nkind = \"anthropic\"\nmodel = \"claude-x\"\napi_key_secret = \"anthropic\"\neffort = \"high\"\n\n[[models]]\nid = \"claude-x\"\neffort = true\n",
        )
        .unwrap();
        c.validate().unwrap();
        assert_eq!(c.base_url(), "https://api.anthropic.com/v1");
        assert!(c.supports_effort("claude-x"));
        assert!(!c.supports_effort("other"));
        let nokey: Config =
            toml::from_str("[provider]\nkind = \"anthropic\"\nmodel = \"claude-x\"\n").unwrap();
        assert!(nokey.validate().is_err());
        let both: Config = toml::from_str(
            "[provider]\nkind = \"openai\"\nmodel = \"m\"\napi_key_env = \"A\"\napi_key_secret = \"b\"\n",
        )
        .unwrap();
        assert!(both.validate().is_err());
    }
}
