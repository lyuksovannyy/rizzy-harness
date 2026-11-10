//! The effective configuration in one scope: the server's own, or a team
//! workspace's default agent and model chains on top of it. Everything that
//! picks an agent, a model or its fallbacks for a chat goes through one, so
//! the lists the UI shows and the turns the server runs agree.

use harness_agents::{Agent, AgentSet, DEFAULT_AGENT};
use harness_core::agent::AgentSpec;
use harness_core::savings::TokenSavings;

use crate::config::{AgentChain, Config};
use crate::workspaces::WorkspaceSettings;
use crate::{CommandError, Master, ModelInfo};

/// One scope's agents, models and chains.
#[derive(Debug, Clone)]
pub struct Resolver {
    /// The server's config with the workspace's chains applied (`None`
    /// without a provider runtime: tests).
    cfg: Option<Config>,
    /// The listed models, `default` flagged for this scope.
    models: Vec<ModelInfo>,
    agents: AgentSet,
    /// The workspace's default agent while it exists, else the server's.
    default_agent: String,
}

/// Put a workspace's chains on the server's config. Chains whose main model
/// the server does not know are skipped and unknown fallbacks dropped, but
/// nothing is promoted (unlike `Config::normalize`): a workspace chain whose
/// main model went away gives the agent back the server's chain. General's
/// chain is the workspace default model, as on the server.
fn overlay(cfg: &mut Config, ws: &WorkspaceSettings) {
    for (agent, chain) in &ws.agent_models {
        if !cfg.knows_model(&chain.model) {
            continue;
        }
        let fallbacks = chain
            .fallbacks
            .iter()
            .filter(|f| cfg.knows_model(f))
            .cloned()
            .collect();
        if agent == DEFAULT_AGENT {
            cfg.default_model = Some(chain.model.clone());
        }
        cfg.agent_models.insert(
            agent.clone(),
            AgentChain {
                model: chain.model.clone(),
                fallbacks,
            },
        );
    }
}

impl Master {
    /// The effective configuration with workspace settings `ws` on top of
    /// the server's (`None`: the server's own).
    pub fn resolver(&self, ws: Option<&WorkspaceSettings>) -> Resolver {
        let agents = self.agents();
        let mut models = self.models();
        let cfg = self.provider_runtime().map(|rt| {
            let mut cfg = rt.config();
            if let Some(ws) = ws {
                overlay(&mut cfg, ws);
            }
            cfg
        });
        // Without a config the cached flags stand, except that a workspace
        // default model (General's chain) still moves the flag.
        let default = match &cfg {
            Some(cfg) => Some(cfg.effective_default()),
            None => ws
                .and_then(|w| w.agent_models.get(DEFAULT_AGENT))
                .filter(|c| models.iter().any(|m| m.id == c.model))
                .map(|c| Some(c.model.clone())),
        };
        if let Some(default) = default {
            for m in &mut models {
                m.default = default.as_deref() == Some(m.id.as_str());
            }
        }
        let default_agent = ws
            .and_then(|w| w.default_agent.clone())
            .filter(|a| agents.get(a).is_some())
            .unwrap_or_else(|| agents.default_id().to_string());
        Resolver {
            cfg,
            models,
            agents,
            default_agent,
        }
    }

    /// The effective configuration in a workspace: a team workspace's
    /// settings, stale references dropped, on top of the server's. Personal
    /// workspaces (they have no settings of their own), unknown ones and
    /// none get the server's.
    pub async fn resolver_for(&self, workspace_id: Option<&str>) -> Resolver {
        let Some(id) = workspace_id.map(str::to_string) else {
            return self.resolver(None);
        };
        let settings = match self.inner.mirror.call(move |s| s.workspace(&id)).await {
            Ok(Some(w)) if !w.personal() => Some(WorkspaceSettings::read(&w).normalized(self)),
            Ok(_) => None,
            Err(e) => {
                tracing::warn!(error = %e, "could not load workspace settings; using the server's");
                None
            }
        };
        self.resolver(settings.as_ref())
    }
}

impl Resolver {
    /// The server's config with this scope's chains, when the master
    /// manages its providers.
    pub fn config(&self) -> Option<&Config> {
        self.cfg.as_ref()
    }

    /// Every listed model; `default` marks this scope's default.
    pub fn models(&self) -> &[ModelInfo] {
        &self.models
    }

    pub fn agents(&self) -> &AgentSet {
        &self.agents
    }

    /// A listed model by its `<provider>::<raw>` id.
    pub fn find_model(&self, id: &str) -> Option<&ModelInfo> {
        self.models.iter().find(|m| m.id == id)
    }

    /// The model chats get when nobody chose one.
    pub fn default_model(&self) -> Option<String> {
        self.models.iter().find(|m| m.default).map(|m| m.id.clone())
    }

    /// The agent chats get when nobody chose one.
    pub fn default_agent(&self) -> &str {
        &self.default_agent
    }

    /// The model assigned to `agent_id` here, if one is.
    pub fn agent_model(&self, agent_id: &str) -> Option<String> {
        self.agent_chain(agent_id).map(|c| c.model)
    }

    /// The model assigned to `agent_id` here and its fallbacks, if any.
    pub fn agent_chain(&self, agent_id: &str) -> Option<AgentChain> {
        let cfg = self.cfg.as_ref()?;
        let c = cfg.agent_models.get(agent_id)?;
        Some(AgentChain {
            model: Some(c.model.clone()).filter(|m| cfg.knows_model(m))?,
            fallbacks: c
                .fallbacks
                .iter()
                .filter(|m| cfg.knows_model(m))
                .cloned()
                .collect(),
        })
    }

    /// The agent's main model: its file's `model`, else the one assigned to
    /// it, else the default.
    pub fn agent_main_model(&self, agent: &Agent) -> Option<String> {
        let cfg = self.cfg.as_ref()?;
        match &agent.spec.model {
            Some(m) => Some(m.clone()),
            None => cfg.agent_default(&agent.id),
        }
    }

    /// The fallbacks of `agent_id` running on `model`: its own chain's, else
    /// (an agent without one runs on the inherited default) those of the
    /// default agent's chain when `model` is that chain's main model. All of
    /// them, unavailable ones included; `set_fallbacks` filters.
    pub fn agent_fallbacks(&self, agent_id: &str, model: Option<&str>) -> Vec<String> {
        self.agent_chain(agent_id)
            .or_else(|| {
                self.agent_chain(DEFAULT_AGENT)
                    .filter(|g| Some(g.model.as_str()) == model)
            })
            .map(|c| c.fallbacks)
            .unwrap_or_default()
            .into_iter()
            .filter(|f| Some(f.as_str()) != model)
            .collect()
    }

    /// Whether `model` is saved but missing from its provider's latest
    /// listing.
    pub fn model_unavailable(&self, model: &str) -> bool {
        self.cfg
            .as_ref()
            .is_some_and(|cfg| cfg.model_unavailable(model))
    }

    /// The server's token-saving choice (no workspace changes it; `None`
    /// without a config: tests).
    pub fn token_savings(&self) -> Option<TokenSavings> {
        self.cfg.as_ref().map(|cfg| cfg.savings.clone())
    }

    /// Put the agent's fallbacks on `spec`, which runs on `model`: the
    /// available ones, in order, and `model` itself among the unavailable
    /// when its provider no longer lists it; and the models of that chain
    /// that take no reasoning effort.
    fn set_fallbacks(&self, spec: &mut AgentSpec, model: Option<&str>) {
        let Some(cfg) = &self.cfg else { return };
        let model = model.map(str::to_string);
        spec.fallbacks = self
            .agent_fallbacks(&spec.agent_id, model.as_deref())
            .into_iter()
            .filter(|f| !cfg.model_unavailable(f))
            .collect();
        spec.unavailable = model
            .clone()
            .filter(|m| cfg.model_unavailable(m))
            .into_iter()
            .collect();
        spec.no_effort = model
            .iter()
            .chain(&spec.fallbacks)
            .filter(|m| !self.find_model(m).is_some_and(|m| m.effort))
            .cloned()
            .collect();
        spec.context_windows = model
            .iter()
            .chain(&spec.fallbacks)
            .filter_map(|m| Some((m.clone(), self.find_model(m)?.context?)))
            .collect();
    }

    /// Resolve an agent id (or this scope's default agent) to the spec a
    /// node runs, with an optional per-message effort that replaces the
    /// agent's. Effort is dropped when no model of the chain supports it.
    ///
    /// The model is, in order: the agent file's `model` (a hard override),
    /// `session_model` (chosen for the session), the agent's chain here,
    /// the default here. Without a session model the result goes on the
    /// spec, so the session follows the current defaults.
    pub fn agent_spec_with(
        &self,
        agent_id: Option<&str>,
        session_model: Option<&str>,
        effort: Option<&str>,
    ) -> Result<AgentSpec, CommandError> {
        if let Some(e) = effort
            && !matches!(e, "low" | "medium" | "high" | "max")
        {
            return Err(CommandError::Rejected(format!(
                "effort must be low, medium, high or max, not {e:?}"
            )));
        }
        let set = &self.agents;
        let agent = match agent_id {
            Some(id) => set
                .get(id)
                .ok_or_else(|| CommandError::UnknownAgent(id.into()))?,
            None => set
                .get(&self.default_agent)
                .ok_or_else(|| CommandError::UnknownAgent("(default)".into()))?,
        };
        let mut spec = agent.spec.clone();
        if let Some(e) = effort {
            spec.effort = Some(e.to_string());
        }
        if spec.model.is_none() && session_model.is_none() {
            spec.model = self.agent_model(&agent.id).or_else(|| self.default_model());
        }
        let model = spec.model.clone().or(session_model.map(str::to_string));
        self.set_fallbacks(&mut spec, model.as_deref());
        // The first model of the chain that takes an effort; the node leaves
        // it off for the others (`no_effort`).
        let takes_effort = model
            .iter()
            .chain(&spec.fallbacks)
            .find(|m| self.find_model(m).is_some_and(|m| m.effort))
            .cloned();
        if takes_effort.is_none() {
            spec.effort = None;
        } else if spec.effort.is_none() {
            // The provider's default effort, for models that take one.
            spec.effort = self.cfg.as_ref().and_then(|cfg| {
                let (p, _) = cfg.resolve_model(takes_effort.as_deref()?)?;
                serde_json::to_value(p.effort?)
                    .ok()?
                    .as_str()
                    .map(str::to_string)
            });
        }
        // Sub-agents travel with the spec (nodes keep no definitions). Their
        // own delegates stay empty: delegation is one level deep. One with an
        // assigned model uses it; the rest inherit the parent's.
        spec.delegates = agent
            .delegates
            .iter()
            .filter_map(|d| set.get(d))
            .map(|d| {
                let mut c = d.spec.clone();
                c.effort = None;
                c.delegates.clear();
                c.caps = None;
                if c.model.is_none() {
                    c.model = self.agent_model(&d.id);
                }
                if c.model.is_some() {
                    let m = c.model.clone();
                    self.set_fallbacks(&mut c, m.as_deref());
                } else {
                    // A child without a model of its own runs on the
                    // parent's, with the parent's chain.
                    c.model = model.clone();
                    c.fallbacks = spec.fallbacks.clone();
                    c.unavailable = spec.unavailable.clone();
                    c.no_effort = spec.no_effort.clone();
                    c.context_windows = spec.context_windows.clone();
                }
                c
            })
            .collect();
        if spec.delegates.is_empty() {
            spec.caps = None;
        }
        Ok(spec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::Auth;
    use harness_core::Secret;
    use harness_store::Store;

    fn info(id: &str, default: bool) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            default,
            ..Default::default()
        }
    }

    fn master() -> Master {
        let m = Master::new(
            Store::open_in_memory().unwrap(),
            Auth::new(Secret::new("t".into()), vec![], false),
        );
        m.set_models(vec![info("p::a", true), info("p::b", false)]);
        m
    }

    /// Without a provider runtime, a workspace's General chain still moves
    /// the default, and its default agent applies while it exists.
    #[test]
    fn workspace_defaults_without_a_runtime() {
        let m = master();
        let server = m.resolver(None);
        assert_eq!(server.default_model().as_deref(), Some("p::a"));
        assert_eq!(server.default_agent(), DEFAULT_AGENT);
        let mut ws = WorkspaceSettings::default();
        ws.agent_models
            .insert(DEFAULT_AGENT.into(), AgentChain::new("p::b"));
        ws.default_agent = Some("coder".into());
        let r = m.resolver(Some(&ws));
        assert_eq!(r.default_model().as_deref(), Some("p::b"));
        assert_eq!(r.models().iter().filter(|m| m.default).count(), 1);
        assert_eq!(r.default_agent(), "coder");
        let spec = r.agent_spec_with(None, None, None).unwrap();
        assert_eq!(
            (spec.agent_id.as_str(), spec.model.as_deref()),
            ("coder", Some("p::b"))
        );
        // Stale references change nothing.
        ws.agent_models
            .insert(DEFAULT_AGENT.into(), AgentChain::new("p::gone"));
        ws.default_agent = Some("nobody".into());
        let r = m.resolver(Some(&ws));
        assert_eq!(r.default_model().as_deref(), Some("p::a"));
        assert_eq!(r.default_agent(), DEFAULT_AGENT);
    }

    /// With a config, workspace chains replace the server's, General's sets
    /// the default, and a chain whose main model is gone is skipped rather
    /// than repaired from its fallbacks.
    #[test]
    fn overlay_replaces_chains_without_promoting() {
        let mut cfg = Config::default();
        cfg.providers[0].models.push(crate::config::ModelConfig {
            id: "other".into(),
            ..Default::default()
        });
        let mut ws = WorkspaceSettings::default();
        ws.agent_models.insert(
            DEFAULT_AGENT.into(),
            AgentChain {
                model: "demo::other".into(),
                fallbacks: vec!["demo::gone".into(), "demo::mock".into()],
            },
        );
        ws.agent_models.insert(
            "coder".into(),
            AgentChain {
                model: "demo::gone".into(),
                fallbacks: vec!["demo::mock".into()],
            },
        );
        overlay(&mut cfg, &ws);
        assert_eq!(cfg.default_model.as_deref(), Some("demo::other"));
        assert_eq!(cfg.effective_default().as_deref(), Some("demo::other"));
        assert_eq!(cfg.agent_models["general"].fallbacks, ["demo::mock"]);
        assert!(!cfg.agent_models.contains_key("coder"));
    }
}
