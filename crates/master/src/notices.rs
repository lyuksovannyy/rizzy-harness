//! Problems that last until someone fixes them, shown under the home
//! screen's search bar. Today one kind: a model in use that its provider no
//! longer lists. Notices follow a scope: a workspace (its chains on top of
//! the server's) or the server's own. Server admins see every one (and can
//! fix the server's in Settings); workspace admins also see those their
//! workspace's settings cause; everyone else only those that touch them: an
//! agent's main model, the default where the notice was computed, or their
//! own default.

use std::collections::{BTreeMap, BTreeSet};

use harness_agents::DEFAULT_AGENT;
use serde::Serialize;

use crate::Master;
use crate::auth::Principal;
use crate::prefs::Prefs;
use crate::workspaces::{Access, Perm};

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
    /// It is the server default and applies in the scope (no workspace
    /// default replaces it there), so a server admin fixes it.
    pub global_default: bool,
    /// It is the workspace's own default model (its General chain), which
    /// replaces the server default there.
    pub workspace_default: bool,
    /// The viewer is a server admin (fixed in the server's providers).
    pub can_fix: bool,
    /// The workspace's own settings use it (its default or an agent's
    /// chain there), so it is fixed in the workspace's settings.
    pub workspace: bool,
    /// The viewer may change the workspace's settings.
    pub can_fix_workspace: bool,
}

/// Where an unavailable model is used.
#[derive(Default)]
struct Uses {
    /// Agent ids: main model.
    main: BTreeSet<String>,
    /// Agent ids: fallback only.
    fallback: BTreeSet<String>,
    global_default: bool,
    workspace_default: bool,
    your_default: bool,
    /// Some user's default (server admins see these).
    others_default: bool,
}

/// The notices `who` sees in `scope` (`None`: the server's own values).
pub async fn for_viewer(m: &Master, who: &Principal, scope: Option<&Access>) -> Vec<Notice> {
    let settings = scope.and_then(|a| a.settings(m));
    let r = m.resolver(settings.as_ref());
    let Some(cfg) = r.config() else {
        return Vec::new();
    };
    let admin = who.is_admin();
    let can_fix_workspace = scope.is_some_and(|a| !a.personal && a.can(Perm::Settings));
    // The workspace's own default model (its General chain). It counts only
    // while it is the default in effect here: the overlay skips a model the
    // server does not know, and the server default then applies.
    let workspace_default = settings
        .as_ref()
        .and_then(|s| s.agent_models.get(DEFAULT_AGENT))
        .map(|c| c.model.clone());
    // Models the workspace's own chains name: fixed there, not on the server.
    let from_workspace = |id: &str| {
        settings
            .iter()
            .flat_map(|s| s.agent_models.values())
            .any(|c| c.models().any(|x| x == id))
    };
    let mut uses: BTreeMap<String, Uses> = BTreeMap::new();
    let mut mark = |id: &str, f: &mut dyn FnMut(&mut Uses)| {
        if cfg.model_unavailable(id) {
            f(uses.entry(id.to_string()).or_default());
        }
    };
    for a in r.agents().list() {
        if let Some(main) = r.agent_main_model(a) {
            mark(&main, &mut |u| {
                u.main.insert(a.id.clone());
            });
        }
        for f in r
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
        if workspace_default.as_deref() == Some(d.as_str()) {
            mark(&d, &mut |u| u.workspace_default = true);
        } else {
            mark(&d, &mut |u| u.global_default = true);
        }
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
        .filter(|(model, u)| {
            let theirs =
                !u.main.is_empty() || u.global_default || u.workspace_default || u.your_default;
            theirs
                || (admin && (!u.fallback.is_empty() || u.others_default))
                || (can_fix_workspace && from_workspace(model))
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
            workspace_default: u.workspace_default,
            can_fix: admin,
            workspace: from_workspace(&model),
            can_fix_workspace,
            model,
        })
        .collect()
}
