//! Problems that last until someone fixes them, shown under the home
//! screen's search bar. Today one kind: a model in use that its provider no
//! longer lists. Admins see every one (and can fix it in Settings); other
//! users only those that touch them: an agent's main model, the global
//! default, or their own default.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::Master;
use crate::auth::Principal;
use crate::prefs::Prefs;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Notice {
    pub id: String,
    /// Always `model_unavailable` for now.
    pub kind: &'static str,
    /// Prefixed.
    pub model: String,
    /// The provider's display name.
    pub provider: String,
    /// Agents using it as their main model or a fallback.
    pub agents: Vec<String>,
    /// It is the viewer's own default model.
    pub your_default: bool,
    pub global_default: bool,
    /// The viewer is an admin.
    pub can_fix: bool,
}

/// Where an unavailable model is used.
#[derive(Default)]
struct Uses {
    /// Agent ids: main model.
    main: BTreeSet<String>,
    /// Agent ids: fallback only.
    fallback: BTreeSet<String>,
    global_default: bool,
    your_default: bool,
    /// Some user's default (admins see these).
    others_default: bool,
}

pub async fn for_viewer(m: &Master, who: &Principal) -> Vec<Notice> {
    let Some(cfg) = m.provider_runtime().map(|rt| rt.config()) else {
        return Vec::new();
    };
    let admin = who.is_admin();
    let mut uses: BTreeMap<String, Uses> = BTreeMap::new();
    let mut mark = |id: &str, f: &mut dyn FnMut(&mut Uses)| {
        if let Some(c) = cfg.canonical_model(id)
            && cfg.model_unavailable(&c)
        {
            f(uses.entry(c).or_default());
        }
    };
    for a in m.agents().list() {
        if let Some(main) = m.agent_main_model(a) {
            mark(&main, &mut |u| {
                u.main.insert(a.id.clone());
            });
        }
        for f in m
            .agent_chain(&a.id)
            .map(|c| c.fallbacks)
            .unwrap_or_default()
        {
            mark(&f, &mut |u| {
                u.fallback.insert(a.id.clone());
            });
        }
    }
    if let Some(d) = cfg.effective_default() {
        mark(&d, &mut |u| u.global_default = true);
    }
    let own = Prefs::load(m, who.user_id.as_deref()).await.default_model;
    if let Some(d) = &own {
        mark(d, &mut |u| u.your_default = true);
    }
    if admin {
        let all = m
            .inner
            .mirror
            .call(|s| s.kv_prefix("settings:"))
            .await
            .unwrap_or_default();
        for (_, v) in all {
            let Ok(p) = serde_json::from_str::<Prefs>(&v) else {
                continue;
            };
            if let Some(d) = p.default_model {
                mark(&d, &mut |u| u.others_default = true);
            }
        }
    }
    uses.into_iter()
        .filter(|(_, u)| {
            let theirs = !u.main.is_empty() || u.global_default || u.your_default;
            theirs || (admin && (!u.fallback.is_empty() || u.others_default))
        })
        .map(|(model, u)| Notice {
            id: format!("model_unavailable:{model}"),
            kind: "model_unavailable",
            provider: cfg
                .resolve_model(&model)
                .map(|(p, _)| p.name.clone())
                .unwrap_or_default(),
            agents: u.main.union(&u.fallback).cloned().collect(),
            your_default: u.your_default,
            global_default: u.global_default,
            can_fix: admin,
            model,
        })
        .collect()
}
