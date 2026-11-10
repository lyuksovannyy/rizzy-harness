//! Instance settings from the environment (a `.env` file or the process
//! environment), above `config.toml`.
//!
//! * **Precedence:** process environment > `.env` > `config.toml` > defaults.
//!   `dotenvy` never overrides a variable that is already set, so the
//!   process environment wins over the file.
//! * **Locked:** a setting given here is what the server runs with; the UI
//!   shows it read-only and the API refuses to change it.
//! * **`config.toml` is not overwritten:** a value the file already has stays
//!   in the file as it is (the environment only wins at runtime). A setting
//!   the file lacks is written there once (`seed_file`), so removing the
//!   variable later keeps the instance as it was.
//! * Only essential instance settings live here: storage endpoints, memory
//!   and embeddings, web search, dictation, the default model, and a first
//!   provider for onboarding. Everything else stays in `config.toml`.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, bail};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::config::{Config, ProviderConfig, ProviderKind};

/// A `config.toml` setting that an environment variable can set.
struct Var {
    name: &'static str,
    /// `section.field` (or `field` at the top level) in `config.toml`.
    key: &'static str,
}

const VARS: &[Var] = &[
    Var {
        name: "HARNESS_DEFAULT_MODEL",
        key: "default_model",
    },
    Var {
        name: "HARNESS_MEMORY_BACKEND",
        key: "memory.backend",
    },
    Var {
        name: "HARNESS_MEMORY_RECALL",
        key: "memory.recall",
    },
    Var {
        name: "HARNESS_EMBEDDINGS_SOURCE",
        key: "memory.source",
    },
    Var {
        name: "HARNESS_EMBEDDINGS_PROVIDER",
        key: "memory.provider_id",
    },
    Var {
        name: "HARNESS_EMBEDDINGS_URL",
        key: "memory.embeddings_url",
    },
    Var {
        name: "HARNESS_EMBEDDINGS_MODEL",
        key: "memory.embeddings_model",
    },
    Var {
        name: "HARNESS_MEM0_URL",
        key: "memory.mem0_url",
    },
    Var {
        name: "HARNESS_QDRANT_URL",
        key: "memory.qdrant_url",
    },
    Var {
        name: "HARNESS_QDRANT_COLLECTION",
        key: "memory.qdrant_collection",
    },
    Var {
        name: "HARNESS_WEB_SEARCH",
        key: "web.search",
    },
    Var {
        name: "HARNESS_SEARXNG_URL",
        key: "web.searxng_url",
    },
    Var {
        name: "HARNESS_VOICE_URL",
        key: "voice.url",
    },
    Var {
        name: "HARNESS_VOICE_MODEL",
        key: "voice.model",
    },
];

/// A first provider, written to `config.toml` only while it has no real
/// provider (onboarding then starts with it filled in). Not locked: it is
/// an ordinary provider afterwards.
pub const PROVIDER_VARS: &[&str] = &[
    "HARNESS_PROVIDER_PRESET",
    "HARNESS_PROVIDER_ID",
    "HARNESS_PROVIDER_NAME",
    "HARNESS_PROVIDER_KIND",
    "HARNESS_PROVIDER_BASE_URL",
    "HARNESS_PROVIDER_API_KEY",
    "HARNESS_PROVIDER_MODELS",
];

/// The variable that holds the first provider's key. The provider refers
/// to it by name (`api_key_env`), so the key itself is never written to disk.
pub const PROVIDER_KEY_VAR: &str = "HARNESS_PROVIDER_API_KEY";

/// Variables whose values are secrets: added to the redaction set.
pub const SECRET_VARS: &[&str] = &[
    PROVIDER_KEY_VAR,
    "HARNESS_QDRANT_API_KEY",
    "HARNESS_JOIN_TOKEN",
];

/// One locked setting, for the UI.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Locked {
    pub key: &'static str,
    pub var: &'static str,
}

/// The settings the environment gives, read once at start.
#[derive(Debug, Clone, Default)]
pub struct EnvSettings {
    /// Set, non-empty variables of `VARS` and `PROVIDER_VARS`.
    vars: BTreeMap<&'static str, String>,
}

impl EnvSettings {
    pub fn from_env() -> Self {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let names = VARS
            .iter()
            .map(|v| v.name)
            .chain(PROVIDER_VARS.iter().copied());
        let vars = names
            .filter_map(|n| {
                let v = get(n)?;
                let v = v.trim();
                (!v.is_empty()).then(|| (n, v.to_string()))
            })
            .collect();
        Self { vars }
    }

    /// Settings the environment fixes, in `VARS` order.
    pub fn locked(&self) -> Vec<Locked> {
        VARS.iter()
            .filter(|v| self.vars.contains_key(v.name))
            .map(|v| Locked {
                key: v.key,
                var: v.name,
            })
            .collect()
    }

    /// The variables that lock keys starting with `prefix` (e.g. `memory.`).
    pub fn locked_under(&self, prefix: &str) -> Vec<Locked> {
        self.locked()
            .into_iter()
            .filter(|l| l.key.starts_with(prefix))
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.vars.is_empty()
    }

    /// Put the environment's values on `cfg` (the config the server runs with).
    pub fn apply(&self, cfg: &mut Config) -> anyhow::Result<()> {
        for v in VARS {
            if let Some(val) = self.vars.get(v.name) {
                set(cfg, v.key, val).with_context(|| format!("{} in the environment", v.name))?;
            }
        }
        Ok(())
    }

    /// The config as `config.toml` should hold it after a change made in the
    /// UI: locked settings keep the file's own values (the environment's
    /// value goes in only where the file has none).
    pub fn for_file(&self, effective: &Config, path: &Path) -> anyhow::Result<Config> {
        if self.locked().is_empty() {
            return Ok(effective.clone());
        }
        let (table, file) = read_file(path)?;
        let mut out = effective.clone();
        for l in self.locked() {
            if has_key(&table, l.key) {
                copy(&file, &mut out, l.key);
            }
        }
        Ok(out)
    }

    /// Write the environment's settings into `config.toml` where the file has
    /// no value for them, and a first provider when it has no real one.
    /// Returns whether the file changed.
    pub fn seed_file(&self, path: &Path) -> anyhow::Result<bool> {
        if self.is_empty() {
            return Ok(false);
        }
        let (table, mut cfg) = read_file(path)?;
        let mut changed = false;
        if let Some(p) = self.first_provider()?
            && !cfg.providers.iter().any(|q| q.kind != ProviderKind::Mock)
        {
            if cfg.provider(&p.id).is_some() {
                bail!(
                    "HARNESS_PROVIDER_ID {:?} is already used in config.toml",
                    p.id
                );
            }
            let first = p.models.first().map(|m| format!("{}::{}", p.id, m.id));
            cfg.providers.push(p);
            if !has_key(&table, "default_model") && first.is_some() {
                cfg.default_model = first;
            }
            changed = true;
        }
        for v in VARS {
            if let Some(val) = self.vars.get(v.name)
                && !has_key(&table, v.key)
            {
                set(&mut cfg, v.key, val)
                    .with_context(|| format!("{} in the environment", v.name))?;
                changed = true;
            }
        }
        if changed {
            cfg.normalize();
            cfg.save(path)?;
        }
        Ok(changed)
    }

    /// The provider `HARNESS_PROVIDER_*` describe, if any.
    fn first_provider(&self) -> anyhow::Result<Option<ProviderConfig>> {
        let get = |k: &str| self.vars.get(k).map(String::as_str);
        let (preset_id, base_url) = (
            get("HARNESS_PROVIDER_PRESET"),
            get("HARNESS_PROVIDER_BASE_URL"),
        );
        if preset_id.is_none() && base_url.is_none() {
            return Ok(None);
        }
        let preset = match preset_id {
            Some(id) => Some(
                crate::presets::get(id)
                    .with_context(|| format!("HARNESS_PROVIDER_PRESET: unknown preset {id:?}"))?,
            ),
            None => None,
        };
        let kind = match get("HARNESS_PROVIDER_KIND") {
            Some(k) => parse_enum::<ProviderKind>(k).context("HARNESS_PROVIDER_KIND")?,
            None => preset.map(|p| p.kind).unwrap_or(ProviderKind::Openai),
        };
        // A fixed preset pins its URL, as in the UI.
        let base_url = match preset {
            Some(p) if !p.url_editable && p.base_url.is_some() => p.base_url.unwrap_or_default(),
            _ => base_url
                .or(preset.and_then(|p| p.base_url))
                .unwrap_or_default(),
        }
        .trim_end_matches('/')
        .to_string();
        if base_url.is_empty() {
            bail!("HARNESS_PROVIDER_BASE_URL is required for this preset");
        }
        let id = get("HARNESS_PROVIDER_ID")
            .map(str::to_string)
            .or_else(|| preset.filter(|p| !p.custom).map(|p| p.id.to_string()))
            .unwrap_or_else(|| "main".into());
        if !crate::config::valid_provider_id(&id)
            || crate::presets::RESERVED_IDS.contains(&id.as_str())
        {
            bail!("HARNESS_PROVIDER_ID {id:?} is not a usable provider id");
        }
        let models = get("HARNESS_PROVIDER_MODELS")
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(|m| crate::config::ModelConfig {
                id: m.to_string(),
                ..Default::default()
            })
            .collect();
        Ok(Some(ProviderConfig {
            name: get("HARNESS_PROVIDER_NAME")
                .map(str::to_string)
                .or_else(|| preset.map(|p| p.label.to_string()))
                .unwrap_or_else(|| id.clone()),
            id,
            preset: preset.map(|p| p.id).unwrap_or("custom_openai").to_string(),
            kind,
            base_url,
            api_key_env: get(PROVIDER_KEY_VAR).map(|_| PROVIDER_KEY_VAR.to_string()),
            api_key_secret: None,
            effort: None,
            mock_delay_ms: None,
            models,
        }))
    }
}

/// Every `HARNESS_*` variable in the process environment, and every name a
/// `.env` file loaded: none of them reach the agents' shells.
pub fn hidden_from_tools(loaded: &[String]) -> Vec<String> {
    let mut v: Vec<String> = std::env::vars_os()
        .filter_map(|(k, _)| k.into_string().ok())
        .filter(|k| k.starts_with("HARNESS_"))
        .chain(loaded.iter().cloned())
        .collect();
    v.sort();
    v.dedup();
    v
}

/// The values of `SECRET_VARS` that are set, for the redaction set.
pub fn secret_values() -> Vec<harness_core::Secret<String>> {
    SECRET_VARS
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .filter(|v| v.len() >= 8)
        .map(harness_core::Secret::new)
        .collect()
}

fn read_file(path: &Path) -> anyhow::Result<(toml::Table, Config)> {
    match std::fs::read_to_string(path) {
        Ok(s) => {
            let table: toml::Table =
                toml::from_str(&s).with_context(|| format!("parsing {}", path.display()))?;
            let cfg: Config =
                toml::from_str(&s).with_context(|| format!("parsing {}", path.display()))?;
            Ok((table, cfg))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok((toml::Table::new(), Config::default()))
        }
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

fn has_key(t: &toml::Table, key: &str) -> bool {
    match key.split_once('.') {
        None => t.contains_key(key),
        Some((section, field)) => t
            .get(section)
            .and_then(|s| s.as_table())
            .is_some_and(|s| s.contains_key(field)),
    }
}

/// `text` as a lowercase serde enum value (`"qdrant"`, `"meaning"`, ...).
fn parse_enum<T: DeserializeOwned>(text: &str) -> anyhow::Result<T> {
    serde_json::from_value(serde_json::Value::String(text.trim().to_ascii_lowercase()))
        .map_err(|_| anyhow::anyhow!("{text:?} is not one of the accepted values"))
}

fn http_url(v: &str) -> anyhow::Result<String> {
    let v = v.trim().trim_end_matches('/');
    if !(v.starts_with("http://") || v.starts_with("https://")) {
        bail!("{v:?} must be an http(s) URL");
    }
    Ok(v.to_string())
}

fn set(cfg: &mut Config, key: &str, v: &str) -> anyhow::Result<()> {
    let m = &mut cfg.memory;
    match key {
        "default_model" => cfg.default_model = Some(v.to_string()),
        "memory.backend" => m.backend = parse_enum(v)?,
        "memory.recall" => m.recall = parse_enum(v)?,
        "memory.source" => m.source = parse_enum(v)?,
        "memory.provider_id" => m.provider_id = Some(v.to_string()),
        "memory.embeddings_url" => m.embeddings_url = http_url(v)?,
        "memory.embeddings_model" => m.embeddings_model = v.to_string(),
        "memory.mem0_url" => m.mem0_url = http_url(v)?,
        "memory.qdrant_url" => m.qdrant_url = http_url(v)?,
        "memory.qdrant_collection" => {
            if !v
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                bail!("use letters, digits, _ and - only");
            }
            m.qdrant_collection = v.to_string()
        }
        "web.search" => cfg.web.search = parse_enum(v)?,
        "web.searxng_url" => cfg.web.searxng_url = http_url(v)?,
        "voice.url" => cfg.voice.url = http_url(v)?,
        "voice.model" => cfg.voice.model = v.to_string(),
        other => bail!("unknown setting {other}"),
    }
    Ok(())
}

fn copy(from: &Config, to: &mut Config, key: &str) {
    let (f, t) = (&from.memory, &mut to.memory);
    match key {
        "default_model" => to.default_model = from.default_model.clone(),
        "memory.backend" => t.backend = f.backend,
        "memory.recall" => t.recall = f.recall,
        "memory.source" => t.source = f.source,
        "memory.provider_id" => t.provider_id = f.provider_id.clone(),
        "memory.embeddings_url" => t.embeddings_url = f.embeddings_url.clone(),
        "memory.embeddings_model" => t.embeddings_model = f.embeddings_model.clone(),
        "memory.mem0_url" => t.mem0_url = f.mem0_url.clone(),
        "memory.qdrant_url" => t.qdrant_url = f.qdrant_url.clone(),
        "memory.qdrant_collection" => t.qdrant_collection = f.qdrant_collection.clone(),
        "web.search" => to.web.search = from.web.search,
        "web.searxng_url" => to.web.searxng_url = from.web.searxng_url.clone(),
        "voice.url" => to.voice.url = from.voice.url.clone(),
        "voice.model" => to.voice.model = from.voice.model.clone(),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MemoryBackend, Recall, SearchKind};

    fn env(pairs: &[(&str, &str)]) -> EnvSettings {
        let m: BTreeMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        EnvSettings::from_lookup(|k| m.get(k).cloned())
    }

    #[test]
    fn env_wins_at_runtime_and_is_locked() {
        let e = env(&[
            ("HARNESS_MEMORY_RECALL", "Meaning"),
            ("HARNESS_EMBEDDINGS_SOURCE", "ollama"),
            ("HARNESS_EMBEDDINGS_URL", "http://ollama:11434/v1/"),
            ("HARNESS_WEB_SEARCH", "searxng"),
            ("HARNESS_SEARXNG_URL", "http://searxng:8080"),
            ("HARNESS_UNRELATED", "x"),
            ("HARNESS_VOICE_MODEL", "  "),
        ]);
        let mut c = Config::default();
        e.apply(&mut c).unwrap();
        assert_eq!(c.memory.recall, Recall::Meaning);
        assert_eq!(c.memory.embeddings_url, "http://ollama:11434/v1");
        assert_eq!(c.web.search, SearchKind::Searxng);
        let keys: Vec<_> = e.locked().into_iter().map(|l| l.key).collect();
        assert_eq!(
            keys,
            [
                "memory.recall",
                "memory.source",
                "memory.embeddings_url",
                "web.search",
                "web.searxng_url"
            ],
            "blank values lock nothing"
        );
        assert_eq!(e.locked_under("web.").len(), 2);
        assert!(
            env(&[("HARNESS_MEMORY_BACKEND", "sqlite")])
                .apply(&mut c)
                .is_err()
        );
        assert!(
            env(&[("HARNESS_QDRANT_URL", "qdrant:6333")])
                .apply(&mut c)
                .is_err()
        );
    }

    #[test]
    fn seeding_fills_only_what_the_file_lacks() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "[memory]\nbackend = \"off\"\n").unwrap();
        let e = env(&[
            ("HARNESS_MEMORY_BACKEND", "qdrant"),
            ("HARNESS_QDRANT_URL", "http://qdrant:6333"),
        ]);
        assert!(e.seed_file(&p).unwrap());
        let file = Config::load_raw(&p).unwrap();
        assert_eq!(
            file.memory.backend,
            MemoryBackend::Off,
            "the file's own value stays"
        );
        assert_eq!(
            file.memory.qdrant_url, "http://qdrant:6333",
            "a missing one is written"
        );
        assert!(!e.seed_file(&p).unwrap(), "nothing left to write");

        // A UI save keeps the file's value for a locked setting.
        let mut eff = file.clone();
        e.apply(&mut eff).unwrap();
        assert_eq!(eff.memory.backend, MemoryBackend::Qdrant);
        eff.chat.summarize_after_tokens = 1234;
        let out = e.for_file(&eff, &p).unwrap();
        assert_eq!(out.memory.backend, MemoryBackend::Off);
        assert_eq!(out.chat.summarize_after_tokens, 1234);
    }

    #[test]
    fn first_provider_is_written_once_with_its_key_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        let e = env(&[
            ("HARNESS_PROVIDER_PRESET", "openai"),
            ("HARNESS_PROVIDER_API_KEY", "sk-test-not-real-0000"),
            ("HARNESS_PROVIDER_MODELS", "gpt-a, gpt-b"),
        ]);
        assert!(e.seed_file(&p).unwrap());
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(
            !text.contains("sk-test"),
            "the key is referenced, never written"
        );
        let c = Config::load(&p).unwrap();
        let prov = c.provider("openai").expect("seeded");
        assert_eq!(prov.api_key_env.as_deref(), Some(PROVIDER_KEY_VAR));
        assert_eq!(prov.models.len(), 2);
        assert_eq!(c.default_model.as_deref(), Some("openai::gpt-a"));
        assert!(!e.seed_file(&p).unwrap(), "a real provider exists now");
        assert!(
            env(&[("HARNESS_PROVIDER_PRESET", "nope")])
                .seed_file(&p)
                .is_err()
        );
    }
}
