//! The agents the master uses: the loaded definitions (built-ins and
//! `<data>/agents/*.md`) with the Agents page's edits applied
//! (docs/agent-pages.md). The server's edits live in the `kv` table as JSON,
//! one value per agent (`agent-edit:server:<id>`, `agent-custom:server:<id>`);
//! a workspace's own agents and its edits of the server's in
//! `workspace_agents` (ADOPTION_PLAN X.33, PLAN.md T79). The set a chat sees
//! is the definitions, then the server's edits, then its workspace's.

use std::collections::HashMap;
use std::sync::Arc;

use harness_agents::edits::{Custom, Edits, Overlay};
use harness_agents::{Agent, AgentSet, Origin};
use harness_store::StoreError;

use crate::{Master, lock};

/// The server's scope in the kv keys.
const SCOPE: &str = "server";
/// `agent-edit:<scope>:<id>`: summary and additional instructions of a
/// built-in or file agent.
const OVERLAY: &str = "agent-edit";
/// `agent-custom:<scope>:<id>`: an agent made on the page.
const CUSTOM: &str = "agent-custom";
/// `workspace_agents.kind` of the same two.
const WS_OVERLAY: &str = "overlay";
const WS_CUSTOM: &str = "custom";

/// `new` is the page that makes an agent (`#/agents/new`).
const RESERVED: &[&str] = &["new"];
const MAX_NAME: usize = 64;
const MAX_SUMMARY: usize = 300;
/// Most agents one agent's `delegate_to` list may name.
pub const MAX_DELEGATE_TO: usize = 32;
const MAX_PROMPT: usize = 20_000;
const MAX_INSTRUCTIONS: usize = 8_000;
/// Agents one workspace may make (PLAN.md T79). Edits of the server's
/// agents are bounded by the server's agents.
pub const MAX_WORKSPACE_AGENTS: usize = 30;
/// One stored agent, as JSON: the store refuses more too.
pub const MAX_DOC_BYTES: usize = harness_store::workspace_agents::MAX_DOC_BYTES;
/// Workspaces whose sets are kept; past it the cache starts over.
const CACHED_WORKSPACES: usize = 32;

fn key(kind: &str, id: &str) -> String {
    format!("{kind}:{SCOPE}:{id}")
}

/// Where an edit goes: the server's own set, or one workspace's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    Server,
    Workspace(String),
}

/// A workspace's own edits and the set its chats see.
#[derive(Debug)]
pub(crate) struct Layer {
    pub(crate) edits: Edits,
    pub(crate) set: AgentSet,
}

pub(crate) struct Agents {
    /// As loaded: built-ins, overridden or extended by files.
    base: AgentSet,
    /// The server's edits.
    edits: Edits,
    /// `base` with `edits` applied: the server's set.
    set: AgentSet,
    /// Held by each edit from its checks to its write, so two edits cannot
    /// interleave (std mutexes cannot be held across awaits).
    writes: Arc<tokio::sync::Mutex<()>>,
    /// Workspaces' layers, read from the store on first use and dropped by
    /// a write to that workspace or to the server's scope.
    workspaces: HashMap<String, Arc<Layer>>,
    /// Bumped by every write: a load that began before one is not kept.
    generation: u64,
}

impl Agents {
    pub(crate) fn new(base: AgentSet) -> Self {
        let mut a = Self {
            set: base.clone(),
            base,
            edits: Edits::default(),
            writes: Arc::default(),
            workspaces: HashMap::new(),
            generation: 0,
        };
        a.compose();
        a
    }

    fn compose(&mut self) {
        let (set, errors) = self.base.with_edits(&self.edits);
        report(None, errors);
        self.set = set;
        // Every workspace's set is built on this one.
        self.workspaces.clear();
        self.generation += 1;
    }

    fn forget(&mut self, ws: &str) {
        self.workspaces.remove(ws);
        self.generation += 1;
    }
}

fn report(ws: Option<&str>, errors: Vec<harness_agents::AgentError>) {
    for e in errors {
        match e {
            harness_agents::AgentError::AliasTaken(_) => {
                tracing::warn!(workspace = ws, error = %e, "dropping an agent alias")
            }
            _ => tracing::warn!(workspace = ws, error = %e, "skipping an agent edit"),
        }
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

/// The JSON a workspace stores for one agent, within [`MAX_DOC_BYTES`].
fn doc(v: &impl serde::Serialize) -> Result<String, AgentEditError> {
    let doc = serde_json::to_string(v).expect("serializes");
    if doc.len() > MAX_DOC_BYTES {
        return Err(AgentEditError::Bad(format!(
            "this agent takes {} bytes stored; at most {MAX_DOC_BYTES} fit: shorten its \
             instructions",
            doc.len()
        )));
    }
    Ok(doc)
}

/// The store's refusals (the cap) are refusals here too.
fn stored(e: StoreError) -> AgentEditError {
    match e {
        StoreError::Refused(why) => AgentEditError::Refused(why),
        e => AgentEditError::Store(e),
    }
}

fn locked(a: &Agent, place: &Place) -> AgentEditError {
    AgentEditError::Refused(match (a.origin, place) {
        _ if a.scope == harness_agents::AgentScope::Workspace => format!(
            "{} was made in this workspace: it has no additional instructions, change its own",
            a.name
        ),
        (Origin::File, _) => format!(
            "{} is defined in a file on the server (agents/{}.md): only its summary, look and \
             additional instructions change here",
            a.name, a.id
        ),
        (Origin::Custom, Place::Workspace(_)) => format!(
            "{} is the server's: only its summary and additional instructions change in a \
             workspace; duplicate it to change the rest",
            a.name
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

    /// The server's agents, its edits applied: what a chat outside any
    /// workspace, the server's settings and the API token without a
    /// workspace see. A workspace's chats see [`Master::agents_in`].
    pub fn agents(&self) -> AgentSet {
        lock(&self.inner.agents).set.clone()
    }

    /// The agents as defined, before edits.
    pub fn agent_definitions(&self) -> AgentSet {
        lock(&self.inner.agents).base.clone()
    }

    /// The server's edits.
    pub fn agent_edits(&self) -> Edits {
        lock(&self.inner.agents).edits.clone()
    }

    /// The agents a chat in workspace `ws` sees (`None`: the server's):
    /// the server's set with that workspace's edits and agents on top. One
    /// path for the listing, the picker and every turn (PLAN.md T79).
    pub async fn agents_in(&self, ws: Option<&str>) -> AgentSet {
        match ws {
            None => self.agents(),
            Some(ws) => self.workspace_layer(ws).await.set.clone(),
        }
    }

    /// Workspace `ws`'s edits and set: cached, else read from the store. A
    /// store that cannot be read gives the server's set (its own agents are
    /// then unknown there: refused, never stood in for), uncached.
    pub(crate) async fn workspace_layer(&self, ws: &str) -> Arc<Layer> {
        let generation = {
            let a = lock(&self.inner.agents);
            if let Some(l) = a.workspaces.get(ws) {
                return l.clone();
            }
            a.generation
        };
        let id = ws.to_string();
        let rows = match self
            .inner
            .mirror
            .call(move |s| s.workspace_agents(&id))
            .await
        {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!(workspace = ws, error = %e, "could not read the workspace's agents");
                return Arc::new(Layer {
                    edits: Edits::default(),
                    set: self.agents(),
                });
            }
        };
        let mut edits = Edits::default();
        for r in rows {
            let read = match r.kind.as_str() {
                WS_OVERLAY => serde_json::from_str(&r.doc)
                    .map(|o| _ = edits.overlays.insert(r.agent_id.clone(), o)),
                _ => serde_json::from_str(&r.doc)
                    .map(|c| _ = edits.custom.insert(r.agent_id.clone(), c)),
            };
            if let Err(e) = read {
                tracing::warn!(workspace = ws, agent = r.agent_id, error = %e, "unreadable workspace agent");
            }
        }
        self.keep_layer(ws, generation, edits)
    }

    /// Compose workspace `ws`'s set from `edits`, read from the store when
    /// the cache was at `generation`, and keep it only if no write came in
    /// between: a load that began before a write holds what the write
    /// replaced, and caching it would serve stale agents until the next
    /// write (PLAN.md T79).
    fn keep_layer(&self, ws: &str, generation: u64, edits: Edits) -> Arc<Layer> {
        let mut a = lock(&self.inner.agents);
        let (set, errors) = a.base.with_layers(&[&a.edits, &edits]);
        report(Some(ws), errors);
        let layer = Arc::new(Layer { edits, set });
        if a.generation == generation {
            if a.workspaces.len() >= CACHED_WORKSPACES {
                a.workspaces.clear();
            }
            a.workspaces.insert(ws.to_string(), layer.clone());
        }
        layer
    }

    /// A deleted workspace's agents leave the cache (its rows went with it).
    pub fn forget_workspace_agents(&self, ws: &str) {
        lock(&self.inner.agents).forget(ws);
    }

    /// Read the server's saved edits and apply them. Unreadable values are
    /// skipped.
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

    /// The edits made at `place` and the set seen there.
    pub(crate) async fn place_edits(&self, place: &Place) -> (Edits, AgentSet) {
        match place {
            Place::Server => {
                let a = lock(&self.inner.agents);
                (a.edits.clone(), a.set.clone())
            }
            Place::Workspace(ws) => {
                let l = self.workspace_layer(ws).await;
                (l.edits.clone(), l.set.clone())
            }
        }
    }

    /// The agents `place`'s overlays may go on: at the server's scope the
    /// definitions, in a workspace the server's set (its own agents too).
    pub(crate) fn wider(&self, place: &Place) -> AgentSet {
        match place {
            Place::Server => self.agent_definitions(),
            Place::Workspace(_) => self.agents(),
        }
    }

    /// Save the summary and additional instructions of an agent `place`
    /// did not make. Both empty removes the edit.
    pub async fn save_agent_overlay(
        &self,
        place: &Place,
        id: &str,
        o: Overlay,
    ) -> Result<(), AgentEditError> {
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
        if self.wider(place).get(id).is_none() {
            return Err(self.edit_refusal(place, id).await);
        }
        let empty = o == Overlay::default();
        match place {
            Place::Server => {
                let k = key(OVERLAY, id);
                if empty {
                    self.inner.mirror.call(move |s| s.kv_delete(&k)).await?;
                } else {
                    let v = serde_json::to_string(&o).expect("serializes");
                    self.inner.mirror.call(move |s| s.kv_set(&k, &v)).await?;
                }
                let mut a = lock(&self.inner.agents);
                if empty {
                    a.edits.overlays.remove(id);
                } else {
                    a.edits.overlays.insert(id.into(), o);
                }
                a.compose();
            }
            Place::Workspace(ws) => {
                let (w, aid) = (ws.clone(), id.to_string());
                if empty {
                    self.inner
                        .mirror
                        .call(move |s| s.workspace_agent_delete(&w, &aid, WS_OVERLAY))
                        .await?;
                } else {
                    let d = doc(&o)?;
                    let put = self
                        .inner
                        .mirror
                        .call(move |s| {
                            s.workspace_agent_put(&w, &aid, WS_OVERLAY, &d, MAX_WORKSPACE_AGENTS)
                        })
                        .await
                        .map_err(stored)?;
                    if !put {
                        return Err(AgentEditError::NotFound(ws.clone()));
                    }
                }
                lock(&self.inner.agents).forget(ws);
            }
        }
        Ok(())
    }

    /// Make (`create`) or change an agent of `place`'s own.
    pub async fn save_custom_agent(
        &self,
        place: &Place,
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
        // Collisions inside the definition, said plainly before the
        // general checks.
        for (i, a) in aliases.iter().enumerate() {
            if a == id {
                return Err(AgentEditError::Bad(format!(
                    "`{a}` is this agent's own id: an alias must be another handle"
                )));
            }
            if aliases[..i].contains(a) {
                return Err(AgentEditError::Bad(format!(
                    "`{a}` is listed twice in this agent's aliases"
                )));
            }
        }
        // Aliases and category are checked like a file's (`labels`).
        let (aliases, _) = harness_agents::labels(id, &aliases, Some(&category))
            .map_err(|e| AgentEditError::Bad(e.to_string()))?;
        let delegates = self.checked_delegate_to(place, id, &c.delegates).await?;
        let c = Custom {
            name,
            summary: text("the summary", &c.summary, MAX_SUMMARY, false)?,
            prompt,
            features: c.features,
            aliases,
            category: Some(category).filter(|c| !c.is_empty()),
            delegates,
        };
        // The rest of what a definition must be (an id, a prompt, somewhere
        // to be used) is checked by building it.
        harness_agents::custom(id, &c.name, &c.summary, &c.prompt, c.features, vec![])
            .map_err(|e| AgentEditError::Bad(e.to_string()))?;
        let writes = self.agent_writes();
        let _w = writes.lock().await;
        let (own, seen) = self.place_edits(place).await;
        if create {
            if let Some(why) = self.handle_holder(&own, &seen, id, None) {
                return Err(AgentEditError::Refused(why));
            }
        } else if !own.custom.contains_key(id) {
            return Err(self.edit_refusal(place, id).await);
        }
        // Ids and aliases are one namespace, built-ins' and files' included
        // (also an alias a custom agent's id pushed out of the set).
        for a in &c.aliases {
            if let Some(why) = self.handle_holder(&own, &seen, a, Some(id)) {
                return Err(AgentEditError::Refused(why));
            }
        }
        // A name it already had stays (a server agent may have taken it
        // since); a new or changed name may not be a server agent's.
        let renamed = !own
            .custom
            .get(id)
            .is_some_and(|o| fold(&o.name) == fold(&c.name));
        if let Place::Workspace(_) = place
            && renamed
            && let Some(why) = self.name_holder(&c.name)
        {
            return Err(AgentEditError::Refused(why));
        }
        match place {
            Place::Server => {
                let (k, v) = (
                    key(CUSTOM, id),
                    serde_json::to_string(&c).expect("serializes"),
                );
                self.inner.mirror.call(move |s| s.kv_set(&k, &v)).await?;
                let mut a = lock(&self.inner.agents);
                a.edits.custom.insert(id.into(), c);
                a.compose();
            }
            Place::Workspace(ws) => {
                let d = doc(&c)?;
                let (w, aid) = (ws.clone(), id.to_string());
                let put = self
                    .inner
                    .mirror
                    .call(move |s| {
                        s.workspace_agent_put(&w, &aid, WS_CUSTOM, &d, MAX_WORKSPACE_AGENTS)
                    })
                    .await
                    .map_err(stored)?;
                if !put {
                    return Err(AgentEditError::NotFound(ws.clone()));
                }
                lock(&self.inner.agents).forget(ws);
            }
        }
        Ok(())
    }

    /// A `delegate_to` list as it may be stored for agent `id` of `place`:
    /// handles, no repeats, not the agent itself, at most
    /// [`MAX_DELEGATE_TO`]. An entry the agent already had is kept as it
    /// is (its agent may have gone since: that never blocks another
    /// change); a new one must be an agent `place` sees, offered as a
    /// sub-agent. The list only narrows at turn time whatever it holds.
    async fn checked_delegate_to(
        &self,
        place: &Place,
        id: &str,
        list: &[String],
    ) -> Result<Vec<String>, AgentEditError> {
        let bad = |m: String| Err(AgentEditError::Bad(m));
        if list.len() > MAX_DELEGATE_TO {
            return bad(format!(
                "an agent may name at most {MAX_DELEGATE_TO} agents to delegate to"
            ));
        }
        let (own, seen) = self.place_edits(place).await;
        let before = own.custom.get(id).map(|o| o.delegates.as_slice());
        let mut out: Vec<String> = Vec::new();
        for d in list {
            let d = d.trim();
            if !harness_agents::valid_id(d) {
                return bad(format!("`{d}` is not an agent handle"));
            }
            if d == id {
                return bad("an agent cannot delegate to itself".into());
            }
            if out.iter().any(|x| x == d) {
                return bad(format!("`{d}` is listed twice in the delegates"));
            }
            let kept = before.is_some_and(|b| b.iter().any(|x| x == d));
            if !kept && !seen.get(d).is_some_and(|a| a.features.as_subagent) {
                return bad(format!(
                    "`{d}` is not an agent that works as a sub-agent here"
                ));
            }
            out.push(d.to_string());
        }
        Ok(out)
    }

    /// Why `handle` cannot be a new id or alias for agent `except`: the
    /// words name the agent that holds it ("`ai` is already reserved for
    /// the Assistant agent (general)"). `None`: it is free. `own` and
    /// `seen` are the scope's edits and set. In a workspace the server's
    /// agents hold their handles. At the server's scope no workspace is
    /// looked at: a server agent wins over a workspace's agent with its id,
    /// which is then skipped in that workspace, as for a later file
    /// (PLAN.md T79), and a server admin learns nothing about workspaces.
    fn handle_holder(
        &self,
        own: &Edits,
        seen: &AgentSet,
        handle: &str,
        except: Option<&str>,
    ) -> Option<String> {
        if RESERVED.contains(&handle) {
            return Some(format!(
                "`{handle}` is reserved for the page that makes agents: choose another"
            ));
        }
        let holder = |a: Option<&Agent>| {
            a.filter(|a| Some(a.id.as_str()) != except)
                .map(|a| (a.name.clone(), a.id.clone()))
        };
        // A custom agent a wider one shadows is still stored under its id.
        let server = self.agent_edits();
        let shadowed = [&own.custom, &server.custom]
            .into_iter()
            .find_map(|m| m.get(handle))
            .filter(|_| Some(handle) != except)
            .map(|c| (c.name.clone(), handle.to_string()));
        let (name, id) = holder(seen.resolve(handle))
            .or_else(|| holder(self.agents().resolve(handle)))
            .or_else(|| holder(self.agent_definitions().resolve(handle)))
            .or(shadowed)?;
        Some(format!(
            "`{handle}` is already reserved for the {name} agent ({id}): choose another"
        ))
    }

    /// Why a workspace's own agent cannot be called `name`: a server agent
    /// is called that, or has it as its id or an alias, compared cleaned
    /// and case-folded (letters and digits only), so no workspace shows a
    /// second "Assistant" (PLAN.md T79). A server agent made later with a
    /// name a workspace agent has is not refused: both stay, the workspace's
    /// marked as the workspace's.
    fn name_holder(&self, name: &str) -> Option<String> {
        let key = fold(name);
        let server = self.agents();
        let a = server.list().iter().find(|a| {
            fold(&a.name) == key || fold(&a.id) == key || a.aliases.iter().any(|x| fold(x) == key)
        })?;
        Some(format!(
            "the server's {} agent (@{}) goes by that name: give this workspace's agent another",
            a.name, a.id
        ))
    }

    /// Delete an agent made at `place`. Built-in, file and wider agents
    /// stay. In a workspace its model chain there goes with it, and so do
    /// the Plugins items assigned to it (in every workspace, for a server
    /// agent): a later agent with its id starts with none (PLAN.md T80).
    pub async fn delete_custom_agent(&self, place: &Place, id: &str) -> Result<(), AgentEditError> {
        let writes = self.agent_writes();
        let _w = writes.lock().await;
        let (own, _) = self.place_edits(place).await;
        if !own.custom.contains_key(id) {
            return Err(self.edit_refusal(place, id).await);
        }
        match place {
            Place::Server => {
                // Workspaces' overlays on it go too: they were written for
                // this agent, not for a later one with its id (T79).
                let (k, aid) = (key(CUSTOM, id), id.to_string());
                let touched = self
                    .inner
                    .mirror
                    .call(move |s| {
                        s.kv_delete(&k)?;
                        s.delete_workspace_overlays(&aid)?;
                        // Nor what workspaces assigned to it (X.34, T80).
                        s.plugin_access_forget_agent(None, &aid)?;
                        // Its handle leaves every `delegate_to` list that
                        // could name it (the server's agents and every
                        // workspace's), so a later agent with the handle
                        // is not a delegate by accident (X.41).
                        let prefix = format!("{CUSTOM}:{SCOPE}:");
                        for (k, v) in s.kv_prefix(&prefix)? {
                            if let Some(d) = without_delegate(&v, &aid) {
                                s.kv_set(&k, &d)?;
                            }
                        }
                        let mut spaces = Vec::new();
                        for (w, a, v) in s.workspace_custom_docs_naming(&aid)? {
                            if let Some(d) = without_delegate(&v, &aid) {
                                s.workspace_agent_put(&w, &a, WS_CUSTOM, &d, MAX_WORKSPACE_AGENTS)?;
                                spaces.push(w);
                            }
                        }
                        Ok::<_, harness_store::StoreError>(spaces)
                    })
                    .await?;
                let mut a = lock(&self.inner.agents);
                a.edits.custom.remove(id);
                for c in a.edits.custom.values_mut() {
                    c.delegates.retain(|d| d != id);
                }
                for w in &touched {
                    a.forget(w);
                }
                a.compose();
            }
            Place::Workspace(ws) => {
                let (w, aid) = (ws.clone(), id.to_string());
                self.inner
                    .mirror
                    .call(move |s| {
                        s.workspace_agent_delete(&w, &aid, WS_CUSTOM)?;
                        s.plugin_access_forget_agent(Some(&w), &aid)?;
                        // Only this workspace's agents can have listed it.
                        for (x, a, v) in s.workspace_custom_docs_naming(&aid)? {
                            if x == w
                                && let Some(d) = without_delegate(&v, &aid)
                            {
                                s.workspace_agent_put(&x, &a, WS_CUSTOM, &d, MAX_WORKSPACE_AGENTS)?;
                            }
                        }
                        crate::workspaces::drop_agent_chain(s, &w, &aid)
                    })
                    .await?;
                lock(&self.inner.agents).forget(ws);
            }
        }
        Ok(())
    }

    /// Why `id` cannot be changed like an agent made at `place`: it is
    /// built in, from a file or a wider scope's, or it does not exist there.
    pub async fn edit_refusal(&self, place: &Place, id: &str) -> AgentEditError {
        let (_, seen) = self.place_edits(place).await;
        let wider = self.wider(place);
        match wider.get(id).or_else(|| match place {
            Place::Server => None,
            Place::Workspace(_) => seen.get(id),
        }) {
            Some(a) => locked(a, place),
            None => AgentEditError::NotFound(id.into()),
        }
    }
}

/// A stored custom agent (`doc`) without `id` among its `delegates`, when
/// it was there (X.41); `None` when nothing changes or it is unreadable.
fn without_delegate(doc: &str, id: &str) -> Option<String> {
    let mut c: Custom = serde_json::from_str(doc).ok()?;
    let before = c.delegates.len();
    c.delegates.retain(|d| d != id);
    (c.delegates.len() != before).then(|| serde_json::to_string(&c).expect("serializes"))
}

/// `name` as names are compared: letters and digits only, case-folded.
fn fold(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The agent id at the end of a `<kind>:<scope>:<id>` key.
fn tail(k: &str) -> String {
    k.rsplit(':').next().unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::Auth;
    use harness_core::Secret;
    use harness_store::{NewUser, Role, Store};

    /// A master with one team workspace, its id.
    async fn master() -> (Master, String) {
        let store = Store::open_in_memory().unwrap();
        store
            .create_user(&NewUser {
                user_id: "u1".into(),
                username: "bo".into(),
                password_hash: "$argon2id$x".into(),
                role: Role::Member,
                exec_ceiling: harness_core::policy::ExecLevel::None,
                must_change_password: false,
            })
            .unwrap();
        let ws = store.create_workspace("A", "u1").unwrap().workspace_id;
        let m = Master::new(store, Auth::new(Secret::new("t".into()), vec![], false));
        (m, ws)
    }

    fn rule(text: &str) -> Overlay {
        Overlay {
            summary: String::new(),
            instructions: text.into(),
        }
    }

    /// Point 8 of X.33: a workspace's set is built once and kept; its own
    /// writes and the server's drop it, so the next read has them at once.
    #[tokio::test]
    async fn a_workspace_set_is_cached_until_a_write_changes_it() {
        let (m, ws) = master().await;
        let first = m.workspace_layer(&ws).await;
        assert!(Arc::ptr_eq(&first, &m.workspace_layer(&ws).await));
        let at = Place::Workspace(ws.clone());
        m.save_agent_overlay(&at, "coder", rule("TEAM"))
            .await
            .unwrap();
        let second = m.workspace_layer(&ws).await;
        assert!(!Arc::ptr_eq(&first, &second));
        let p = |l: &Layer| l.set.get("coder").unwrap().spec.system_prompt.clone();
        assert!(p(&second).ends_with("TEAM"));
        assert!(Arc::ptr_eq(&second, &m.workspace_layer(&ws).await));
        m.save_agent_overlay(&Place::Server, "coder", rule("SERVER"))
            .await
            .unwrap();
        let third = m.workspace_layer(&ws).await;
        assert!(!Arc::ptr_eq(&second, &third));
        let prompt = p(&third);
        assert!(
            prompt.find("SERVER").unwrap() < prompt.find("TEAM").unwrap(),
            "{prompt}"
        );
        // The server's set never has the workspace's.
        assert!(
            !m.agents()
                .get("coder")
                .unwrap()
                .spec
                .system_prompt
                .contains("TEAM")
        );
        // Loading the definitions again (a restart's order) drops it too.
        m.set_agents(m.agent_definitions());
        assert!(!Arc::ptr_eq(&third, &m.workspace_layer(&ws).await));
        m.forget_workspace_agents(&ws);
        assert!(lock(&m.inner.agents).workspaces.is_empty());
    }

    /// L2 of the review: a load that read the store before a write and
    /// finishes after it is not kept, so the next read sees the write.
    #[tokio::test]
    async fn a_load_that_began_before_a_write_is_not_kept() {
        let (m, ws) = master().await;
        // A load starts: it notes the generation and reads (no rows yet).
        let before = lock(&m.inner.agents).generation;
        let stale = Edits::default();
        // A write lands while it is away.
        let at = Place::Workspace(ws.clone());
        m.save_agent_overlay(&at, "coder", rule("WRITTEN"))
            .await
            .unwrap();
        // The load finishes with what it read: served once, never kept.
        let served = m.keep_layer(&ws, before, stale);
        assert!(
            !served
                .set
                .get("coder")
                .unwrap()
                .spec
                .system_prompt
                .contains("WRITTEN")
        );
        let next = m.workspace_layer(&ws).await;
        assert!(
            next.set
                .get("coder")
                .unwrap()
                .spec
                .system_prompt
                .ends_with("WRITTEN"),
            "a stale load was cached"
        );
    }
}
