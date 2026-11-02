//! Per-user preferences, stored in the mirror's kv table as
//! `settings:<user_id>` (the API token uses `settings:api-token`).

use serde::{Deserialize, Serialize};

use crate::Master;

pub const DEFAULT_SEARCH_URL: &str = "https://duckduckgo.com/?q={q}";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prefs {
    /// Where Search mode sends the query; `{q}` is replaced (URL-encoded).
    pub search_url: String,
    #[serde(default)]
    pub default_agent: Option<String>,
    #[serde(default)]
    pub default_model: Option<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            search_url: DEFAULT_SEARCH_URL.into(),
            default_agent: None,
            default_model: None,
        }
    }
}

fn key(user: Option<&str>) -> String {
    format!("settings:{}", user.unwrap_or("api-token"))
}

impl Prefs {
    pub fn validate(&self, m: &Master) -> Result<(), String> {
        let u = self.search_url.trim();
        if !u.starts_with("https://") || !u.contains("{q}") || u.len() > 2048 {
            return Err("search_url must be an https:// URL containing {q}".into());
        }
        if let Some(a) = &self.default_agent
            && m.agents().get(a).is_none()
        {
            return Err(format!("unknown agent {a:?}"));
        }
        if let Some(model) = &self.default_model
            && !m.models().iter().any(|x| &x.id == model)
        {
            return Err(format!("unknown model {model:?}"));
        }
        Ok(())
    }

    pub async fn load(m: &Master, user: Option<&str>) -> Prefs {
        let k = key(user);
        m.inner
            .mirror
            .call(move |s| s.kv_get(&k))
            .await
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default()
    }

    pub async fn save(
        &self,
        m: &Master,
        user: Option<&str>,
    ) -> Result<(), harness_store::StoreError> {
        let (k, v) = (
            key(user),
            serde_json::to_string(self).expect("prefs serialize"),
        );
        m.inner.mirror.call(move |s| s.kv_set(&k, &v)).await
    }
}
