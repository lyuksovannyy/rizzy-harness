//! Agent definitions (PLAN.md §0 "Agents"): markdown files with YAML
//! frontmatter. Built-ins ship in the binary; users override or add agents in
//! `<data>/agents/<id>.md`. The file stem is the agent id.
//!
//! ```text
//! ---
//! name: Coder
//! description: Software engineer that edits and tests code.
//! tools: [bash, read, write, edit, grep, glob]   # or: all
//! model: openrouter::openai/gpt-4o                # optional, <provider>::<raw>
//! effort: high                                    # low | medium | high | max
//! budget: standard                                # lean | standard
//! ui: rich                                        # rich | display | off
//! delegates: [coder, scout]                       # may hand sub-tasks to these
//! max_subagents: 4                                # per turn (default 4)
//! max_subagent_tokens: 200000                     # per turn, all sub-agents
//! memory: true                                    # recall and `remember` (default true)
//! project_memory: true                            # a project's facts, the same (default true)
//! available_as: [agent, subagent]                 # default both
//! ---
//! System prompt in markdown...
//! ```
//!
//! `ui` is which `ui:` blocks the agent's replies may show (default `rich`):
//! `rich` also allows questions and forms the user answers, `display` only
//! progress and charts, `off` none (fences stay code). A delegated sub-task
//! runs at `display` at most.
//!
//! `available_as` says where the agent may be used: `agent` puts it in the
//! picker and `@mentions`, `subagent` lets agents that delegate hand it
//! sub-tasks (one that is not is dropped from every `delegates`).
//!
//! Agents made on the Agents page live in the master's database, not here:
//! see [`edits`], which also applies the summary and additional
//! instructions people give built-in and file agents.

pub mod edits;

use std::path::Path;

use harness_core::agent::{AgentSpec, Budget, ToolSelection, UiLevel};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const DEFAULT_AGENT: &str = "general";

const BUILTIN: &[(&str, &str)] = &[
    ("general", include_str!("../builtin/general.md")),
    ("coder", include_str!("../builtin/coder.md")),
    ("writer", include_str!("../builtin/writer.md")),
    ("scout", include_str!("../builtin/scout.md")),
    ("ultracode", include_str!("../builtin/ultracode.md")),
];

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("{0}: missing `---` frontmatter")]
    NoFrontmatter(String),
    #[error("{id}: bad frontmatter: {msg}")]
    Frontmatter { id: String, msg: String },
    #[error("{0}: agent ids must be 1-32 characters: a-z, 0-9, -")]
    BadId(String),
    #[error("{0}: system prompt is empty")]
    EmptyPrompt(String),
    #[error("{id}: effort must be low, medium, high or max, not {got:?}")]
    BadEffort { id: String, got: String },
    #[error("{0}: {1}")]
    Io(String, std::io::Error),
    #[error("{0}: available_as must name agent, subagent or both")]
    Unavailable(String),
    #[error("{0}: a built-in or file agent has this id")]
    Shadowed(String),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Frontmatter {
    name: String,
    #[serde(default)]
    description: String,
    tools: ToolSelection,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    effort: Option<String>,
    #[serde(default)]
    budget: Budget,
    #[serde(default)]
    ui: UiLevel,
    #[serde(default)]
    delegates: Vec<String>,
    #[serde(default)]
    max_subagents: Option<u32>,
    #[serde(default)]
    max_subagent_tokens: Option<u64>,
    #[serde(default = "yes")]
    memory: bool,
    #[serde(default = "yes")]
    project_memory: bool,
    #[serde(default = "both")]
    available_as: Vec<Use>,
}

fn yes() -> bool {
    true
}

fn both() -> Vec<Use> {
    vec![Use::Agent, Use::Subagent]
}

/// Where an agent may be used (`available_as`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Use {
    Agent,
    Subagent,
}

/// Where a definition comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Ships in the binary.
    Builtin,
    /// `<data>/agents/<id>.md`, written by whoever runs the server.
    File,
    /// Made on the Agents page (stored by the master).
    Custom,
}

// Tools each feature switch stands for. A built-in or file agent has a
// feature when its tool list allows any of them.
const WEB_TOOLS: &[&str] = &["web_search", "web_fetch"];
const READ_TOOLS: &[&str] = &["read", "grep", "glob", "outline", "graph_query"];
const EDIT_TOOLS: &[&str] = &["write", "edit"];
const COMMAND_TOOLS: &[&str] = &["bash", "tool_output"];
const MEMORY_TOOL: &str = "remember";

/// What an agent can do, as the Agents page shows it. A custom agent is
/// made from these; for the others they are read off the definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Features {
    /// The user's memory: facts recalled into its turns, `remember`.
    pub memory: bool,
    /// In a project's chats, the project's facts recalled into its turns and
    /// `remember` for new ones (docs/projects.md). The project's
    /// instructions and board reach every agent: they are its brief.
    #[serde(default = "yes")]
    pub project_memory: bool,
    /// `web_search` and `web_fetch`.
    pub web: bool,
    /// Read and search files.
    pub read_files: bool,
    /// Write and edit files.
    pub edit_files: bool,
    /// Run shell commands.
    pub commands: bool,
    /// Hand sub-tasks to the agents available as sub-agents.
    pub delegate: bool,
    /// In the agent picker and `@mentions`.
    pub as_agent: bool,
    /// May be handed sub-tasks.
    pub as_subagent: bool,
}

impl Features {
    /// The tools a custom agent with these features asks for.
    pub fn tools(&self) -> Vec<String> {
        let groups = [
            // The master offers it per turn only for the memory it may write.
            (self.memory || self.project_memory, &[MEMORY_TOOL][..]),
            (self.web, WEB_TOOLS),
            (self.read_files, READ_TOOLS),
            (self.edit_files, EDIT_TOOLS),
            (self.commands, COMMAND_TOOLS),
        ];
        groups
            .iter()
            .filter(|(on, _)| *on)
            .flat_map(|(_, tools)| tools.iter().map(|t| t.to_string()))
            .collect()
    }

    /// Work that runs long gets the standard budget; the rest stays lean.
    fn budget(&self) -> Budget {
        if self.edit_files || self.commands || self.delegate {
            Budget::Standard
        } else {
            Budget::Lean
        }
    }
}

/// An agent as listed in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
    /// The summary under its name (one or two sentences).
    pub description: String,
    pub builtin: bool,
    pub origin: Origin,
    pub features: Features,
    /// Agent ids this one may delegate to (resolved by the master).
    pub delegates: Vec<String>,
    #[serde(skip)]
    pub spec: AgentSpec,
}

/// 1-32 characters: a-z, 0-9, `-`.
pub fn valid_id(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Parse one definition. `id` comes from the file name.
pub fn parse(id: &str, text: &str, builtin: bool) -> Result<Agent, AgentError> {
    if !valid_id(id) {
        return Err(AgentError::BadId(id.into()));
    }
    let text = text.replace("\r\n", "\n");
    let rest = text
        .strip_prefix("---\n")
        .ok_or_else(|| AgentError::NoFrontmatter(id.into()))?;
    let (yaml, body) = match rest.find("\n---\n") {
        Some(i) => (&rest[..i], &rest[i + 5..]),
        None => match rest.strip_suffix("\n---") {
            Some(y) => (y, ""),
            None => return Err(AgentError::NoFrontmatter(id.into())),
        },
    };
    let fm: Frontmatter = serde_saphyr::from_str(yaml).map_err(|e| AgentError::Frontmatter {
        id: id.into(),
        msg: e.to_string(),
    })?;
    let origin = if builtin {
        Origin::Builtin
    } else {
        Origin::File
    };
    build(id, fm, body, origin)
}

/// An agent made on the Agents page, from its features. `delegates` are
/// the agents it may hand sub-tasks to (the master works them out: every
/// agent available as a sub-agent but this one, when `delegate` is on).
pub fn custom(
    id: &str,
    name: &str,
    summary: &str,
    prompt: &str,
    features: Features,
    delegates: Vec<String>,
) -> Result<Agent, AgentError> {
    if !valid_id(id) {
        return Err(AgentError::BadId(id.into()));
    }
    let available_as = [
        (features.as_agent, Use::Agent),
        (features.as_subagent, Use::Subagent),
    ]
    .into_iter()
    .filter_map(|(on, u)| on.then_some(u))
    .collect();
    let fm = Frontmatter {
        name: name.into(),
        description: summary.into(),
        tools: ToolSelection::List(features.tools()),
        model: None,
        effort: None,
        budget: features.budget(),
        ui: UiLevel::Rich,
        delegates: if features.delegate { delegates } else { vec![] },
        max_subagents: None,
        max_subagent_tokens: None,
        memory: features.memory,
        project_memory: features.project_memory,
        available_as,
    };
    let mut agent = build(id, fm, prompt, Origin::Custom)?;
    // Kept as chosen: with no sub-agent to call it has no `delegate` tool,
    // but the switch stays on.
    agent.features.delegate = features.delegate;
    Ok(agent)
}

fn build(id: &str, fm: Frontmatter, body: &str, origin: Origin) -> Result<Agent, AgentError> {
    if let Some(e) = &fm.effort
        && !matches!(e.as_str(), "low" | "medium" | "high" | "max")
    {
        return Err(AgentError::BadEffort {
            id: id.into(),
            got: e.clone(),
        });
    }
    if fm.available_as.is_empty() {
        return Err(AgentError::Unavailable(id.into()));
    }
    let system_prompt = body.trim().to_string();
    if system_prompt.is_empty() {
        return Err(AgentError::EmptyPrompt(id.into()));
    }
    let mut h = Sha256::new();
    for part in [
        system_prompt.as_str(),
        &serde_json_like(&fm.tools),
        fm.model.as_deref().unwrap_or(""),
        fm.effort.as_deref().unwrap_or(""),
        &fm.delegates.join(","),
        &format!("{:?}/{:?}", fm.max_subagents, fm.max_subagent_tokens),
        fm.ui.as_str(),
        if fm.memory { "" } else { "no-memory" },
    ] {
        h.update(part.as_bytes());
        h.update([0u8]);
    }
    // Only when off, so the hashes of agents from before this switch stay.
    if !fm.project_memory {
        h.update(b"no-project-memory\0");
    }
    let prompt_hash = hex::encode(h.finalize())[..16].to_string();
    for d in &fm.delegates {
        if !valid_id(d) || d == id {
            return Err(AgentError::Frontmatter {
                id: id.into(),
                msg: format!("bad delegate {d:?}"),
            });
        }
    }
    let caps = (!fm.delegates.is_empty()).then(|| {
        let d = harness_core::agent::DelegateCaps::default();
        harness_core::agent::DelegateCaps {
            max_subagents: fm.max_subagents.unwrap_or(d.max_subagents).clamp(1, 16),
            max_tokens: fm.max_subagent_tokens.unwrap_or(d.max_tokens),
        }
    });
    let any = |tools: &[&str]| tools.iter().any(|t| fm.tools.allows(t));
    let features = Features {
        memory: fm.memory,
        project_memory: fm.project_memory,
        web: any(WEB_TOOLS),
        read_files: any(READ_TOOLS),
        edit_files: any(EDIT_TOOLS),
        commands: any(COMMAND_TOOLS),
        delegate: !fm.delegates.is_empty(),
        as_agent: fm.available_as.contains(&Use::Agent),
        as_subagent: fm.available_as.contains(&Use::Subagent),
    };
    Ok(Agent {
        id: id.into(),
        name: fm.name.clone(),
        description: fm.description,
        builtin: origin == Origin::Builtin,
        origin,
        features,
        delegates: fm.delegates,
        spec: AgentSpec {
            agent_id: id.into(),
            name: fm.name,
            system_prompt,
            tools: fm.tools,
            model: fm.model,
            fallbacks: vec![],
            unavailable: vec![],
            no_effort: vec![],
            effort: fm.effort,
            budget: fm.budget,
            ui: fm.ui,
            prompt_hash,
            deny_tools: vec![],
            delegates: vec![],
            caps,
        },
    })
}

/// Stable text form of a tool selection for hashing.
fn serde_json_like(t: &ToolSelection) -> String {
    match t {
        ToolSelection::All(_) => "all".into(),
        ToolSelection::List(l) => l.join(","),
    }
}

pub fn builtins() -> Vec<Agent> {
    BUILTIN
        .iter()
        .map(|(id, text)| parse(id, text, true).expect("built-in agents are valid"))
        .collect()
}

/// The agents available on this master.
#[derive(Debug, Clone)]
pub struct AgentSet {
    agents: Vec<Agent>,
    default_id: String,
}

impl AgentSet {
    /// Built-ins, overridden or extended by `<dir>/*.md`. A missing directory is
    /// fine. Invalid user files are skipped and reported, never fatal.
    pub fn load(dir: &Path) -> (Self, Vec<AgentError>) {
        let mut agents = builtins();
        let mut errors = Vec::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e.flatten().collect::<Vec<_>>(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                errors.push(AgentError::Io(dir.display().to_string(), e));
                Vec::new()
            }
        };
        let mut files: Vec<_> = entries
            .into_iter()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "md"))
            .collect();
        files.sort();
        for path in files {
            let id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            let parsed = std::fs::read_to_string(&path)
                .map_err(|e| AgentError::Io(path.display().to_string(), e))
                .and_then(|t| parse(&id, &t, false));
            match parsed {
                Ok(a) => match agents.iter_mut().find(|x| x.id == a.id) {
                    Some(slot) => *slot = a,
                    None => agents.push(a),
                },
                Err(e) => errors.push(e),
            }
        }
        (Self::from_agents(agents, DEFAULT_AGENT), errors)
    }

    pub fn from_agents(agents: Vec<Agent>, default_id: &str) -> Self {
        let default_id = if agents.iter().any(|a| a.id == default_id) {
            default_id.to_string()
        } else {
            agents.first().map(|a| a.id.clone()).unwrap_or_default()
        };
        Self { agents, default_id }
    }

    pub fn list(&self) -> &[Agent] {
        &self.agents
    }

    pub fn get(&self, id: &str) -> Option<&Agent> {
        self.agents.iter().find(|a| a.id == id)
    }

    pub fn default_agent(&self) -> Option<&Agent> {
        self.get(&self.default_id)
    }

    pub fn default_id(&self) -> &str {
        &self.default_id
    }
}
