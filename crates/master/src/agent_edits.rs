//! The agents the master uses: the loaded definitions (built-ins and
//! `<data>/agents/*.md`) with the Agents page's edits applied
//! (docs/agent-pages.md). Edits live in the `kv` table as JSON, one value
//! per agent, under a scope: `server` until workspaces bring their own.

use std::sync::Arc;

use harness_agents::edits::{Custom, Edits, Overlay};
use harness_agents::{Agent, AgentSet, Origin};
use harness_store::StoreError;

use crate::{Master, lock};

/// The only scope until workspaces land: edits for the whole server.
const SCOPE: &str = "server";
/// `agent-edit:<scope>:<id>`: summary and additional instructions of a
/// built-in or file agent.
const OVERLAY: &str = "agent-edit";
/// `agent-custom:<scope>:<id>`: an agent made on the page.
const CUSTOM: &str = "agent-custom";

/// `new` is the page that makes an agent (`#/agents/new`).
const RESERVED: &[&str] = &["new"];
const MAX_NAME: usize = 64;
const MAX_SUMMARY: usize = 300;
const MAX_PROMPT: usize = 20_000;
const MAX_INSTRUCTIONS: usize = 8_000;

fn key(kind: &str, id: &str) -> String {
    format!("{kind}:{SCOPE}:{id}")
}

pub(crate) struct Agents {
    /// As loaded: built-ins, overridden or extended by files.
    base: AgentSet,
    edits: Edits,
    /// `base` with `edits` applied: what everything else uses.
    set: AgentSet,
    /// Held by each edit from its checks to its write, so two edits of the
    /// same agent cannot interleave (std mutexes cannot be held across awaits).
    writes: Arc<tokio::sync::Mutex<()>>,
}

impl Agents {
    pub(crate) fn new(base: AgentSet) -> Self {
        let mut a = Self {
            set: base.clone(),
            base,
            edits: Edits::default(),
            writes: Arc::default(),
        };
        a.compose();
        a
    }

    fn compose(&mut self) {
        let (set, errors) = self.base.with_edits(&self.edits);
        for e in errors {
            match e {
                harness_agents::AgentError::AliasTaken { .. } => {
                    tracing::warn!(error = %e, "dropping an agent alias")
                }
                _ => tracing::warn!(error = %e, "skipping an agent edit"),
            }
        }
        self.set = set;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AgentEditError {
    #[error("unknown agent {0:?}")]
    NotFound(String),
    /// The change is well-formed but not allowed (409).
    #[error("{0}")]
    Refused(String),
    #[error("{0}")]
    Bad(String),
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Trimmed, at most `max` characters, no control characters (new lines and
/// tabs only where `multiline`).
fn text(field: &str, s: &str, max: usize, multiline: bool) -> Result<String, AgentEditError> {
    let s = s.trim().replace("\r\n", "\n");
    if s.chars().count() > max {
        return Err(AgentEditError::Bad(format!(
            "{field} is longer than {max} characters"
        )));
    }
    if s.chars()
        .any(|c| c.is_control() && !(multiline && matches!(c, '\n' | '\t')))
    {
        return Err(AgentEditError::Bad(format!(
            "{field} has control characters"
        )));
    }
    Ok(s)
}

fn locked(a: &Agent) -> AgentEditError {
    AgentEditError::Refused(match a.origin {
        Origin::File => format!(
            "{} is defined in a file on the server (agents/{}.md): only its summary, look and \
             additional instructions change here",
            a.name, a.id
        ),
        _ => format!(
            "{} is built in: only its summary, look and additional instructions change; \
             duplicate it to change the rest",
            a.name
        ),
    })
}

impl Master {
    /// Load agent definitions (at startup); the current edits apply to them.
    pub fn set_agents(&self, agents: AgentSet) {
        let mut a = lock(&self.inner.agents);
        a.base = agents;
        a.compose();
    }

    /// The agents in use, edits applied.
    pub fn agents(&self) -> AgentSet {
        lock(&self.inner.agents).set.clone()
    }

    /// The agents as defined, before edits.
    pub fn agent_definitions(&self) -> AgentSet {
        lock(&self.inner.agents).base.clone()
    }

    pub fn agent_edits(&self) -> Edits {
        lock(&self.inner.agents).edits.clone()
    }

    /// Read the saved edits and apply them. Unreadable values are skipped.
    pub async fn load_agent_edits(&self) -> Result<(), StoreError> {
        let read = |kind: &'static str| {
            let prefix = format!("{kind}:{SCOPE}:");
            async move {
                let rows = self
                    .inner
                    .mirror
                    .call(move |s| s.kv_prefix(&prefix))
                    .await?;
                Ok::<_, StoreError>(rows)
            }
        };
        let mut edits = Edits::default();
        for (k, v) in read(OVERLAY).await? {
            match serde_json::from_str(&v) {
                Ok(o) => _ = edits.overlays.insert(tail(&k), o),
                Err(e) => tracing::warn!(key = k, error = %e, "unreadable agent edit"),
            }
        }
        for (k, v) in read(CUSTOM).await? {
            match serde_json::from_str(&v) {
                Ok(c) => _ = edits.custom.insert(tail(&k), c),
                Err(e) => tracing::warn!(key = k, error = %e, "unreadable custom agent"),
            }
        }
        let mut a = lock(&self.inner.agents);
        a.edits = edits;
        a.compose();
        Ok(())
    }

    fn agent_writes(&self) -> Arc<tokio::sync::Mutex<()>> {
        lock(&self.inner.agents).writes.clone()
    }

    /// Save the summary and additional instructions of a built-in or file
    /// agent. Both empty removes the edit.
    pub async fn save_agent_overlay(&self, id: &str, o: Overlay) -> Result<(), AgentEditError> {
        let o = Overlay {
            summary: text("the summary", &o.summary, MAX_SUMMARY, false)?,
            instructions: text(
                "the additional instructions",
                &o.instructions,
                MAX_INSTRUCTIONS,
                true,
            )?,
        };
        let writes = self.agent_writes();
        let _w = writes.lock().await;
        if self.agent_definitions().get(id).is_none() {
            return Err(self.edit_refusal(id));
        }
        let k = key(OVERLAY, id);
        if o == Overlay::default() {
            self.inner.mirror.call(move |s| s.kv_delete(&k)).await?;
        } else {
            let v = serde_json::to_string(&o).expect("serializes");
            self.inner.mirror.call(move |s| s.kv_set(&k, &v)).await?;
        }
        let mut a = lock(&self.inner.agents);
        if o == Overlay::default() {
            a.edits.overlays.remove(id);
        } else {
            a.edits.overlays.insert(id.into(), o);
        }
        a.compose();
        Ok(())
    }

    /// Make (`create`) or change an agent of the page's own.
    pub async fn save_custom_agent(
        &self,
        id: &str,
        c: Custom,
        create: bool,
    ) -> Result<(), AgentEditError> {
        let name = text("the name", &c.name, MAX_NAME, false)?;
        if name.is_empty() {
            return Err(AgentEditError::Bad("give the agent a name".into()));
        }
        let prompt = text("the instructions", &c.prompt, MAX_PROMPT, true)?;
        let category = text(
            "the category",
            c.category.as_deref().unwrap_or(""),
            64,
            false,
        )?;
        let aliases: Vec<String> = c.aliases.iter().map(|a| a.trim().to_string()).collect();
        // Aliases and category are checked like a file's (`labels`).
        let (aliases, _) = harness_agents::labels(id, &aliases, Some(&category))
            .map_err(|e| AgentEditError::Bad(e.to_string()))?;
        let c = Custom {
            name,
            summary: text("the summary", &c.summary, MAX_SUMMARY, false)?,
            prompt,
            features: c.features,
            aliases,
            category: Some(category).filter(|c| !c.is_empty()),
        };
        // The rest of what a definition must be (an id, a prompt, somewhere
        // to be used) is checked by building it.
        harness_agents::custom(id, &c.name, &c.summary, &c.prompt, c.features, vec![])
            .map_err(|e| AgentEditError::Bad(e.to_string()))?;
        let writes = self.agent_writes();
        let _w = writes.lock().await;
        let exists = self.agent_edits().custom.contains_key(id);
        if create {
            if RESERVED.contains(&id) || self.handle_taken(id, None) || exists {
                return Err(AgentEditError::Refused(format!(
                    "the handle @{id} is taken: choose another"
                )));
            }
        } else if !exists {
            return Err(self.edit_refusal(id));
        }
        // Ids and aliases are one namespace, built-ins' and files' included
        // (also an alias a custom agent's id pushed out of the set).
        if let Some(a) = c
            .aliases
            .iter()
            .find(|a| RESERVED.contains(&a.as_str()) || self.handle_taken(a, Some(id)))
        {
            return Err(AgentEditError::Refused(format!(
                "the handle @{a} is taken: choose another alias"
            )));
        }
        let (k, v) = (
            key(CUSTOM, id),
            serde_json::to_string(&c).expect("serializes"),
        );
        self.inner.mirror.call(move |s| s.kv_set(&k, &v)).await?;
        let mut a = lock(&self.inner.agents);
        a.edits.custom.insert(id.into(), c);
        a.compose();
        Ok(())
    }

    /// Whether `handle` is an id or alias of an agent other than `except`:
    /// in use, or declared by a built-in or file agent.
    fn handle_taken(&self, handle: &str, except: Option<&str>) -> bool {
        let other = |a: Option<&Agent>| a.is_some_and(|a| Some(a.id.as_str()) != except);
        other(self.agents().resolve(handle)) || other(self.agent_definitions().resolve(handle))
    }

    /// Delete an agent made on the page. Built-in and file agents stay.
    pub async fn delete_custom_agent(&self, id: &str) -> Result<(), AgentEditError> {
        let writes = self.agent_writes();
        let _w = writes.lock().await;
        if !self.agent_edits().custom.contains_key(id) {
            return Err(self.edit_refusal(id));
        }
        let k = key(CUSTOM, id);
        self.inner.mirror.call(move |s| s.kv_delete(&k)).await?;
        let mut a = lock(&self.inner.agents);
        a.edits.custom.remove(id);
        a.compose();
        Ok(())
    }

    /// Why `id` cannot be changed like an agent made here: it is built in
    /// or from a file, or it does not exist.
    pub fn edit_refusal(&self, id: &str) -> AgentEditError {
        match self.agent_definitions().get(id) {
            Some(a) => locked(a),
            None => AgentEditError::NotFound(id.into()),
        }
    }
}

/// The agent id at the end of a `<kind>:<scope>:<id>` key.
fn tail(k: &str) -> String {
    k.rsplit(':').next().unwrap_or_default().to_string()
}
