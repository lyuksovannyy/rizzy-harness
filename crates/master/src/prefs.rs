//! Per-user preferences, stored in the mirror's kv table as
//! `settings:<user_id>` (the API token uses `settings:api-token`).

use serde::{Deserialize, Serialize};

use crate::Master;

pub const DEFAULT_SEARCH_URL: &str = "https://duckduckgo.com/?q={q}";
pub const DEFAULT_IMAGE_SEARCH_URL: &str = "https://duckduckgo.com/?q={q}&iax=images&ia=images";
/// Custom search engines a user may define.
pub const MAX_CUSTOM_ENGINES: usize = 20;

/// A user-defined search engine. Its abilities are the URLs it has: a text
/// URL makes it a web search, an image URL an image search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomEngine {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
}

fn check_search_url(what: &str, u: &str) -> Result<(), String> {
    let u = u.trim();
    if !u.starts_with("https://") || !u.contains("{q}") || u.len() > 2048 {
        return Err(format!("{what} must be an https:// URL containing {{q}}"));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prefs {
    /// Where Search mode sends the query; `{q}` is replaced (URL-encoded).
    pub search_url: String,
    /// The same for image searches.
    #[serde(default = "default_image_search")]
    pub image_search_url: String,
    #[serde(default)]
    pub custom_engines: Vec<CustomEngine>,
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

fn default_image_search() -> String {
    DEFAULT_IMAGE_SEARCH_URL.into()
}

pub const ACCENTS: &[&str] = &["blue", "violet", "green", "orange", "pink", "gray"];

impl Default for Prefs {
    fn default() -> Self {
        Self {
            search_url: DEFAULT_SEARCH_URL.into(),
            image_search_url: DEFAULT_IMAGE_SEARCH_URL.into(),
            custom_engines: Vec::new(),
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
        check_search_url("search_url", &self.search_url)?;
        check_search_url("image_search_url", &self.image_search_url)?;
        if self.custom_engines.len() > MAX_CUSTOM_ENGINES {
            return Err(format!(
                "at most {MAX_CUSTOM_ENGINES} custom search engines"
            ));
        }
        for e in &self.custom_engines {
            short_text("engine name", &Some(e.name.clone()))?;
            if e.text_url.is_none() && e.image_url.is_none() {
                return Err(format!(
                    "search engine {:?} needs a text or an image URL",
                    e.name
                ));
            }
            if let Some(u) = &e.text_url {
                check_search_url(&format!("{} text URL", e.name), u)?;
            }
            if let Some(u) = &e.image_url {
                check_search_url(&format!("{} image URL", e.name), u)?;
            }
        }
        short_text("display_name", &self.display_name)?;
        short_text("country", &self.country)?;
        short_text("city", &self.city)?;
        if let Some(tz) = &self.timezone
            && (tz.len() > 64 || jiff::tz::TimeZone::get(tz).is_err())
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

    /// A short context block for the model: who the user is, their local
    /// date and time, and (if given) where they are. Empty fields are left out.
    pub fn model_context(&self, username: &str, now: jiff::Timestamp) -> String {
        let opt = |o: &Option<String>| o.as_deref().map(str::trim).unwrap_or("").to_string();
        let display = opt(&self.display_name);
        let name = if display.is_empty() {
            username
        } else {
            display.as_str()
        };
        let tz = self
            .timezone
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .and_then(|t| jiff::tz::TimeZone::get(t).ok().map(|z| (t, z)));
        let mut out = match tz {
            Some((zone_name, zone)) => format!(
                "Context: the user's name is {name}. Their local time is {} ({zone_name}).",
                now.to_zoned(zone).strftime("%A %Y-%m-%d %H:%M"),
            ),
            // No time zone given: say so instead of passing UTC off as theirs.
            None => format!(
                "Context: the user's name is {name}. Their time zone is not known; it is {} UTC.",
                now.to_zoned(jiff::tz::TimeZone::UTC)
                    .strftime("%A %Y-%m-%d %H:%M")
            ),
        };
        let place: Vec<String> = [opt(&self.city), opt(&self.country)]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect();
        if !place.is_empty() {
            out.push_str(&format!(
                " They are in {}; use it for local questions (transit, shops, news, weather).",
                place.join(", ")
            ));
        }
        out
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

#[cfg(test)]
mod context_tests {
    use super::*;

    #[test]
    fn model_context_uses_local_time_and_place() {
        let p = Prefs {
            display_name: Some("Alex".into()),
            timezone: Some("Europe/Riga".into()),
            city: Some("Riga".into()),
            country: Some("Latvia".into()),
            ..Default::default()
        };
        // 2026-01-15 22:30 UTC is 00:30 the next day in Riga (UTC+2).
        let now: jiff::Timestamp = "2026-01-15T22:30:00Z".parse().unwrap();
        let c = p.model_context("alex", now);
        assert!(c.contains("Alex"), "{c}");
        assert!(c.contains("Friday 2026-01-16 00:30 (Europe/Riga)"), "{c}");
        assert!(c.contains("Riga, Latvia"), "{c}");
        let bare = Prefs::default().model_context("bob", now);
        assert!(
            bare.contains("bob") && !bare.contains("They are in"),
            "{bare}"
        );
        // No time zone: UTC is not passed off as the user's local time.
        assert!(
            bare.contains("time zone is not known; it is Thursday 2026-01-15 22:30 UTC"),
            "{bare}"
        );
        assert!(!bare.contains("local time"), "{bare}");
    }

    #[test]
    fn timezone_must_exist() {
        assert!(jiff::tz::TimeZone::get("Europe/Riga").is_ok());
        assert!(jiff::tz::TimeZone::get("Mars/Olympus").is_err());
    }
}
