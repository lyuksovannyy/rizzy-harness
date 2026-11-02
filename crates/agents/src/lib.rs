//! Agent definitions (PLAN.md §0 "Agents"): markdown files with YAML
//! frontmatter. Built-ins ship in the binary; users override or add agents in
//! `<data>/agents/<id>.md`. The file stem is the agent id.
//!
//! ```text
//! ---
//! name: Coder
//! description: Software engineer that edits and tests code.
//! tools: [bash, read, write, edit, grep, glob]   # or: all
//! model: optional-model-id
//! effort: high                                    # low | medium | high | max
//! budget: standard                                # lean | standard
//! ---
//! System prompt in markdown...
//! ```

use std::path::Path;

use harness_core::agent::{AgentSpec, Budget, ToolSelection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const DEFAULT_AGENT: &str = "general";

const BUILTIN: &[(&str, &str)] = &[
    ("general", include_str!("../builtin/general.md")),
    ("coder", include_str!("../builtin/coder.md")),
    ("writer", include_str!("../builtin/writer.md")),
    ("scout", include_str!("../builtin/scout.md")),
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
}

/// An agent as listed in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub description: String,
    pub builtin: bool,
    #[serde(skip)]
    pub spec: AgentSpec,
}

fn valid_id(id: &str) -> bool {
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
    if let Some(e) = &fm.effort
        && !matches!(e.as_str(), "low" | "medium" | "high" | "max")
    {
        return Err(AgentError::BadEffort {
            id: id.into(),
            got: e.clone(),
        });
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
    ] {
        h.update(part.as_bytes());
        h.update([0u8]);
    }
    let prompt_hash = hex::encode(h.finalize())[..16].to_string();
    Ok(Agent {
        id: id.into(),
        name: fm.name.clone(),
        description: fm.description,
        builtin,
        spec: AgentSpec {
            agent_id: id.into(),
            name: fm.name,
            system_prompt,
            tools: fm.tools,
            model: fm.model,
            effort: fm.effort,
            budget: fm.budget,
            prompt_hash,
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
