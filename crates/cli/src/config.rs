//! `harness.toml`: master-side configuration. Secrets are referenced by
//! environment-variable name, never stored in the file.

use std::path::Path;

use anyhow::{Context, bail};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[derive(Default)]
pub struct Config {
    pub provider: ProviderConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// Any OpenAI-compatible chat-completions endpoint.
    Openai,
    /// Built-in scripted provider (no network, for testing the UI).
    Mock,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub base_url: String,
    pub model: String,
    /// Name of the environment variable holding the API key.
    pub api_key_env: Option<String>,
    /// Delay between mock chunks, in milliseconds.
    pub mock_delay_ms: u64,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            kind: ProviderKind::Mock,
            base_url: "https://api.openai.com/v1".into(),
            model: "mock".into(),
            api_key_env: None,
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

    fn validate(&self) -> anyhow::Result<()> {
        if self.provider.kind == ProviderKind::Openai {
            if self.provider.model.is_empty() || self.provider.model == "mock" {
                bail!("[provider] model must be set for kind = \"openai\"");
            }
            let u = &self.provider.base_url;
            let local = u.starts_with("http://127.0.0.1")
                || u.starts_with("http://localhost")
                || u.starts_with("http://[::1]");
            if !u.starts_with("https://") && !local {
                bail!("[provider] base_url must be https:// (plain http only for localhost)");
            }
        }
        Ok(())
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
}
