//! The last model listing of every provider, kept in the store (so restarts
//! keep it) and refreshed by an admin or once a day. Each listing re-marks
//! the provider's saved models: one missing from it becomes `unavailable`
//! (kept, never removed here), one that is back is cleared, and custom
//! models are never marked. A failed fetch changes nothing.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::Master;
use crate::config::{ModelConfig, ProviderConfig, ProviderKind};
use crate::presets::same_url;

const PREFIX: &str = "model-listing:";

/// How old a listing may get before the background task fetches it again.
pub const REFRESH_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// One provider's listing as it was fetched.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Listing {
    /// The URL it came from: it never applies to the provider at another.
    pub base_url: String,
    pub fetched_ms: i64,
    pub models: Vec<ModelConfig>,
}

fn key(provider_id: &str) -> String {
    format!("{PREFIX}{provider_id}")
}

/// Mark `p`'s saved models against `listed` at `now`. Returns whether any
/// mark changed.
pub fn mark(p: &mut ProviderConfig, listed: &[ModelConfig], now: i64) -> bool {
    let mut changed = false;
    for m in &mut p.models {
        let missing = !m.custom && !listed.iter().any(|l| l.id == m.id);
        if missing && !m.unavailable {
            m.unavailable = true;
            m.missing_since = Some(now);
            changed = true;
        } else if !missing && (m.unavailable || m.missing_since.is_some()) {
            m.unavailable = false;
            m.missing_since = None;
            changed = true;
        }
    }
    changed
}

impl Master {
    /// Provider `id`'s cached listing, if it was fetched from `base_url`.
    pub async fn cached_listing(&self, id: &str, base_url: &str) -> Option<Listing> {
        let k = key(id);
        let v = self.inner.mirror.call(move |s| s.kv_get(&k)).await.ok()??;
        let l: Listing = serde_json::from_str(&v).ok()?;
        same_url(&l.base_url, base_url).then_some(l)
    }

    /// Drop provider `id`'s cached listing (it was removed or moved).
    pub(crate) async fn forget_listing(&self, id: &str) {
        let k = key(id);
        if let Err(e) = self.inner.mirror.call(move |s| s.kv_delete(&k)).await {
            tracing::warn!(provider = %id, "dropping a model listing: {e}");
        }
    }

    /// Keep `models` as the listing of provider `id` at `base_url` and
    /// re-mark its saved models, while the provider still has that URL and
    /// kind (else the listing is only returned). Takes the config edit lock
    /// for the store write and the re-marking, never for a fetch.
    pub async fn record_listing(
        &self,
        id: &str,
        kind: ProviderKind,
        base_url: &str,
        models: Vec<ModelConfig>,
    ) -> anyhow::Result<Listing> {
        let l = Listing {
            base_url: base_url.trim_end_matches('/').to_string(),
            fetched_ms: harness_core::now_ms(),
            models,
        };
        let Some(rt) = self.provider_runtime() else {
            return Ok(l);
        };
        let _edits = rt.lock_edits().await;
        let mut cfg = rt.config();
        let Some(p) = cfg
            .providers
            .iter_mut()
            .find(|p| p.id == id && p.kind == kind && same_url(&p.base_url, base_url))
        else {
            return Ok(l);
        };
        let (k, v) = (key(id), serde_json::to_string(&l)?);
        self.inner.mirror.call(move |s| s.kv_set(&k, &v)).await?;
        if mark(p, &l.models, l.fetched_ms) {
            rt.save(cfg)?;
            self.set_models(rt.models());
        }
        Ok(l)
    }

    /// Fetch provider `id`'s listing now with its saved key, and record it.
    pub async fn refresh_listing(&self, id: &str) -> Result<Listing, String> {
        let rt = self
            .provider_runtime()
            .ok_or("provider is not managed by this server")?;
        let p = rt
            .config()
            .provider(id)
            .cloned()
            .ok_or("unknown provider")?;
        let (key, _) =
            crate::providers::resolve_key(&p, rt.data_dir()).map_err(|e| format!("{e:#}"))?;
        let ollama = p.preset == "ollama";
        let models = crate::providers::list_models(
            p.kind,
            &p.base_url,
            key.as_ref(),
            ollama,
            &self.outbound(),
        )
        .await?;
        self.record_listing(id, p.kind, &p.base_url, models)
            .await
            .map_err(|e| format!("{e:#}"))
    }

    /// Fetch every real provider's listing when the cached one is older
    /// than `every` or missing (so once at start, then once per `every`).
    /// A provider whose fetch failed is tried again after `every` too.
    pub fn start_model_refresh(&self, every: Duration) {
        let this = self.clone();
        let tick = (every / 24).clamp(Duration::from_millis(50), Duration::from_secs(3600));
        tokio::spawn(async move {
            let mut tried: HashMap<String, Instant> = HashMap::new();
            loop {
                if let Some(rt) = this.provider_runtime() {
                    let now = harness_core::now_ms();
                    let every_ms = every.as_millis() as i64;
                    for p in rt.config().providers {
                        if p.kind == ProviderKind::Mock
                            || tried.get(&p.id).is_some_and(|t| t.elapsed() < every)
                        {
                            continue;
                        }
                        let fresh = this
                            .cached_listing(&p.id, &p.base_url)
                            .await
                            .is_some_and(|l| now - l.fetched_ms < every_ms);
                        if fresh {
                            continue;
                        }
                        tried.insert(p.id.clone(), Instant::now());
                        match this.refresh_listing(&p.id).await {
                            Ok(l) => tracing::info!(
                                provider = %p.id,
                                models = l.models.len(),
                                "model listing refreshed"
                            ),
                            Err(e) => {
                                tracing::warn!(provider = %p.id, "model listing refresh failed: {e}")
                            }
                        }
                    }
                }
                tokio::time::sleep(tick).await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str) -> ModelConfig {
        ModelConfig {
            id: id.into(),
            ..Default::default()
        }
    }

    #[test]
    fn marks_and_clears() {
        let mut p = ProviderConfig::demo(1);
        p.models = vec![
            model("a"),
            model("b"),
            ModelConfig {
                custom: true,
                ..model("mine")
            },
        ];
        assert!(mark(&mut p, &[model("a")], 5));
        assert_eq!(
            p.models
                .iter()
                .map(|m| (m.unavailable, m.missing_since))
                .collect::<Vec<_>>(),
            [(false, None), (true, Some(5)), (false, None)]
        );
        // Still missing: the first time it went missing is kept.
        assert!(!mark(&mut p, &[model("a")], 9));
        assert_eq!(p.models[1].missing_since, Some(5));
        // Back again.
        assert!(mark(&mut p, &[model("a"), model("b")], 10));
        assert!(
            p.models
                .iter()
                .all(|m| !m.unavailable && m.missing_since.is_none())
        );
    }
}
