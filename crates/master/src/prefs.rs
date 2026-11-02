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
    /// What the UI calls the user ("Good morning, <display_name>").
    #[serde(default)]
    pub display_name: Option<String>,
    /// IANA name, e.g. `Europe/Riga`; pre-filled from the browser.
    #[serde(default)]
    pub timezone: Option<String>,
    /// Optional; tells the AI where to look for local information.
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub city: Option<String>,
    /// One of `ACCENTS`.
    #[serde(default)]
    pub accent: Option<String>,
    /// Set when the user finished (or skipped) onboarding.
    #[serde(default)]
    pub onboarded: bool,
}

pub const ACCENTS: &[&str] = &["blue", "violet", "green", "orange", "pink", "gray"];

impl Default for Prefs {
    fn default() -> Self {
        Self {
            search_url: DEFAULT_SEARCH_URL.into(),
            default_agent: None,
            default_model: None,
            display_name: None,
            timezone: None,
            country: None,
            city: None,
            accent: None,
            onboarded: false,
        }
    }
}

fn short_text(name: &str, v: &Option<String>) -> Result<(), String> {
    match v {
        Some(t)
            if t.trim().is_empty() || t.chars().count() > 64 || t.chars().any(char::is_control) =>
        {
            Err(format!("{name} must be 1-64 printable characters"))
        }
        _ => Ok(()),
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
        short_text("display_name", &self.display_name)?;
        short_text("country", &self.country)?;
        short_text("city", &self.city)?;
        if let Some(tz) = &self.timezone
            && (tz.is_empty()
                || tz.len() > 64
                || !tz
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/_+-".contains(&b)))
        {
            return Err("timezone must be an IANA name such as Europe/Riga".into());
        }
        if let Some(a) = &self.accent
            && !ACCENTS.contains(&a.as_str())
        {
            return Err(format!("accent must be one of {}", ACCENTS.join(", ")));
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
