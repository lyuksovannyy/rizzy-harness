//! Agent definitions (PLAN.md §0 "Agents"): markdown files with YAML
//! frontmatter. Built-ins ship in the binary; users override or add agents in
//! `<data>/agents/<id>.md`. The file stem is the agent id.
//!
//! ```text
//! ---
//! name: Coder
//! description: Software engineer that edits and tests code.
//! aliases: [dev]                                  # more @handles (default none)
//! category: IT                                    # grouping in the UI (default General)
//! tools: [bash, read, write, edit, grep, glob]   # or: all
//! model: openrouter::openai/gpt-4o                # optional, <provider>::<raw>
//! effort: high                                    # low | medium | high | max
//! ui: rich                                        # rich | display | off
//! delegates: [coder, scout]                       # may hand sub-tasks to these
//! max_subagents: 4                                # per turn (default 4)
//! max_subagent_tokens: 200000                     # per turn, all sub-agents
//! background_subagents: false                     # delegate in the background (default false)
//! memory: true                                    # recall and `remember` (default true)
//! project_memory: true                            # a project's facts, the same (default true)
//! personalisation: true                           # the person's reply style and instructions (default true)
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
//! sub-tasks (one that is not is dropped from every `delegates`). The server
//! enforces both: an agent that is not available as an agent never runs a
//! chat or a turn, however it is named.
//!
//! Ids and `aliases` share one namespace: `@dev` and `"agent_id": "dev"`
//! resolve to the agent's id, which is what events and sessions store. An
//! id beats an alias, and the first agent to claim an alias keeps it
//! (built-ins, then files, then agents made on the page); a losing alias is
//! dropped and reported ([`AgentError::AliasTaken`]), never the agent.
//!
//! Agents made on the Agents page live in the master's database, not here:
//! see [`edits`], which also applies the summary and additional
//! instructions people give built-in and file agents.

pub mod edits;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use harness_core::agent::{AgentSpec, ToolSelection, UiLevel};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The default agent of an ordinary (chat-mode) chat.
pub const DEFAULT_AGENT: &str = "general";
/// The default agent of an Agentic chat: it needs file or command tools.
pub const DEFAULT_AGENTIC_AGENT: &str = "coder";
/// The category of a definition that names none.
pub const DEFAULT_CATEGORY: &str = "General";
/// Longest category, in characters.
pub const MAX_CATEGORY: usize = 32;
/// Most aliases one agent may have.
pub const MAX_ALIASES: usize = 8;

/// The built-in roster, in the order the UI lists it. `ultracode` was
/// removed: chats and routines that name it fall back like any unknown
/// agent (the master's `Resolver::session_agent`).
const BUILTIN: &[(&str, &str)] = &[
    ("general", include_str!("../builtin/general.md")),
    ("smol", include_str!("../builtin/smol.md")),
    ("researcher", include_str!("../builtin/researcher.md")),
    ("coder", include_str!("../builtin/coder.md")),
    ("teamlead", include_str!("../builtin/teamlead.md")),
    ("writer", include_str!("../builtin/writer.md")),
    ("scout", include_str!("../builtin/scout.md")),
    ("reviewer", include_str!("../builtin/reviewer.md")),
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
    /// `alias` is dropped from `id`: `by` has it as its id or claimed it
    /// first. The agent itself loads.
    #[error("{0}")]
    AliasTaken(Box<AliasClash>),
}

/// An alias dropped from agent `id` because agent `by` has it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "alias @{alias} of the {name} agent ({id}) dropped: the {by_name} agent ({by}) already has \
     it as its {how}"
)]
pub struct AliasClash {
    pub id: String,
    pub name: String,
    pub alias: String,
    pub by: String,
    pub by_name: String,
    /// `id` or `alias`: what `alias` is to `by`.
    pub how: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Frontmatter {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    category: Option<String>,
    tools: ToolSelection,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    effort: Option<String>,
    #[serde(default)]
    ui: UiLevel,
    #[serde(default)]
    delegates: Vec<String>,
    #[serde(default)]
    max_subagents: Option<u32>,
    #[serde(default)]
    max_subagent_tokens: Option<u64>,
    /// May start sub-agents in the background (ADOPTION_PLAN X.24).
    #[serde(default)]
    background_subagents: bool,
    #[serde(default = "yes")]
    memory: bool,
    #[serde(default = "yes")]
    project_memory: bool,
    #[serde(default = "yes")]
    personalisation: bool,
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
const COMMAND_TOOLS: &[&str] = &["bash", "bash_output", "bash_stop", "tool_output"];
/// "Read-only commands" (ADOPTION_PLAN T.7, PLAN.md T70): the same tools,
/// for an agent that changes nothing else, whose shell the node then runs
/// read-only.
const MEMORY_TOOL: &str = "remember";
/// Searching earlier chats and memory: its own switch.
const RECALL_TOOL: &str = "recall";

/// **The built-in agents that have "Earlier chats and memory search"**
/// (PLAN.md T55): the ones without commands, MCP servers or publishing,
/// so private data, untrusted input and a way out do not meet in one
/// agent without a person's choice. Built-ins are read-only on the Agents
/// page (Duplicate makes an editable copy), so this list IS the
/// permission for them: to change who has it, change this line.
pub const RECALL_BUILTINS: &[&str] = &["general", "researcher", "reviewer"];
/// The project's task board (ADOPTION_PLAN X.32, PLAN.md T78): `board`
/// lists it, `board_edit` adds, moves and notes cards. Its own switch.
const BOARD_TOOL: &str = harness_core::board::READ_TOOL;
const BOARD_EDIT_TOOL: &str = harness_core::board::EDIT_TOOL;
/// Offered with either switch that builds things; no switch of its own.
const PUBLISH_TOOL: &str = "publish";
/// The agent's own task list: with the same switches, for the same reason
/// (work that takes many steps).
const TODO_TOOL: &str = "todo";
/// Skills (instructions loaded by name): its own switch
/// (`Features::skills`). The node offers its skill folders only to an agent
/// that works on files or runs commands; the server's stored skills, which
/// are text, to any (X.34 step B).
const SKILL_TOOL: &str = "skill";

/// Which set an agent belongs to (X.33): the server's (built-ins, files and
/// agents made at the server's scope) or one workspace's own. Reported in
/// listings; the master keeps a workspace's agents out of every other.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentScope {
    #[default]
    Server,
    Workspace,
}

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
    /// The person's Personalisation (Settings → Personalisation, PLAN.md
    /// T74): the reply style they chose and their personal instructions,
    /// in their own chats with this agent. Off: neither (the name, place
    /// and time zone still come). On when absent.
    #[serde(default = "yes")]
    pub personalisation: bool,
    /// `recall`: searches the person's own earlier chats and memory
    /// (PLAN.md T55). Its own switch, off when absent.
    #[serde(default)]
    pub recall: bool,
    /// The project's task board, in a project's chats (PLAN.md T78):
    /// `board` lists it, `board_edit` adds, moves and notes cards (never
    /// deletes or overwrites). An agent that changes nothing else and has
    /// read-only commands gets `board` only, so it stays read-only. Its own
    /// switch, off when absent.
    #[serde(default)]
    pub board: bool,
    /// `web_search` and `web_fetch`.
    pub web: bool,
    /// Read and search files.
    pub read_files: bool,
    /// Write and edit files.
    pub edit_files: bool,
    /// Run shell commands.
    pub commands: bool,
    /// Run shell commands that can only read (T.7, PLAN.md T70): `bash` in
    /// the read-only shell, for an agent that changes nothing else by the
    /// shared rule. Its own switch, off when absent (a custom agent gets it
    /// when someone turns it on); with a switch on that changes things
    /// (files, commands, MCP) it adds nothing. Built-in Scout and Reviewer
    /// have it.
    #[serde(default)]
    pub read_only_commands: bool,
    /// The tools of the MCP servers the node runs for the chat (X.3). Its
    /// own switch: commands do not bring it. Off when absent, so a page
    /// agent saved before the switch has no MCP tools until it is turned on.
    #[serde(default)]
    pub mcp: bool,
    /// Skills (`skill`, X.34): load the skills this agent may use in its
    /// workspace (Plugins: Shared ones, and the ones Assigned to it). Its
    /// own switch. With file or command access the node's skill folders
    /// too; without, only the skills stored on the server (step B, text),
    /// and the tool only in a turn that has one to load. On when absent,
    /// so an agent saved before the switch keeps what it had.
    #[serde(default = "yes")]
    pub skills: bool,
    /// Hand sub-tasks to the agents available as sub-agents.
    pub delegate: bool,
    /// Start sub-agents that run in the background (`delegate` with
    /// `background: true`, ADOPTION_PLAN X.24): its own switch, off when
    /// absent; it does something only with `delegate` on.
    #[serde(default)]
    pub background_subagents: bool,
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
            (self.recall, &[RECALL_TOOL][..]),
            (self.web, WEB_TOOLS),
            (self.read_files, READ_TOOLS),
            (self.edit_files, EDIT_TOOLS),
            (self.commands, COMMAND_TOOLS),
            (self.mcp, &[harness_core::agent::MCP_ALL][..]),
            (
                self.edit_files || self.commands,
                &[PUBLISH_TOOL, TODO_TOOL][..],
            ),
            (self.skills, &[SKILL_TOOL][..]),
        ];
        let mut tools: Vec<String> = groups
            .iter()
            .filter(|(on, _)| *on)
            .flat_map(|(_, tools)| tools.iter().map(|t| t.to_string()))
            .collect();
        if self.board {
            tools.push(BOARD_TOOL.into());
        }
        // The shell only where the node will run it read-only: the rest
        // changes nothing (the shared rule, as the node reads it). Such an
        // agent stays read-only: the board's list, not its changes.
        let read_only = self.read_only_commands && !changes_things(&tools);
        if read_only {
            tools.extend(COMMAND_TOOLS.iter().map(|t| t.to_string()));
        } else if self.board {
            tools.push(BOARD_EDIT_TOOL.into());
        }
        tools
    }
}

/// Whether an agent with exactly `tools` changes things by the shared rule
/// (`harness_core::agent::AgentSpec::changes_things`, its shell aside).
fn changes_things(tools: &[String]) -> bool {
    tools
        .iter()
        .any(|t| t == harness_core::agent::MCP_ALL || harness_core::agent::tool_changes(t, true))
}

/// An agent as listed in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
    /// The summary under its name (one or two sentences).
    pub description: String,
    /// More `@handles` for it, beside its id.
    pub aliases: Vec<String>,
    /// A short label the UI groups agents by.
    pub category: String,
    pub builtin: bool,
    pub origin: Origin,
    /// The server's, or made in the workspace it is listed for.
    pub scope: AgentScope,
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
    let mut fm: Frontmatter =
        serde_saphyr::from_str(yaml).map_err(|e| AgentError::Frontmatter {
            id: id.into(),
            msg: e.to_string(),
        })?;
    if builtin
        && RECALL_BUILTINS.contains(&id)
        && let ToolSelection::List(tools) = &mut fm.tools
        && !tools.iter().any(|t| t == RECALL_TOOL)
    {
        tools.push(RECALL_TOOL.into());
    }
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
        aliases: vec![],
        category: None,
        tools: ToolSelection::List(features.tools()),
        model: None,
        effort: None,
        ui: UiLevel::Rich,
        delegates: if features.delegate { delegates } else { vec![] },
        max_subagents: None,
        max_subagent_tokens: None,
        background_subagents: features.background_subagents && features.delegate,
        memory: features.memory,
        project_memory: features.project_memory,
        personalisation: features.personalisation,
        available_as,
    };
    let mut agent = build(id, fm, prompt, Origin::Custom)?;
    // Kept as chosen: with no sub-agent to call it has no `delegate` tool,
    // but the switch stays on.
    agent.features.delegate = features.delegate;
    agent.features.background_subagents = features.background_subagents;
    // Kept as chosen too: with a switch on that changes things it adds no
    // shell, but turning that one off again brings it back.
    agent.features.read_only_commands = features.read_only_commands;
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
    let (aliases, category) = labels(id, &fm.aliases, fm.category.as_deref())?;
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
    if !fm.personalisation {
        h.update(b"no-personalisation\0");
    }
    // Only when on, for the same reason.
    if fm.background_subagents {
        h.update(b"background-subagents\0");
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
            background: fm.background_subagents,
        }
    });
    let any = |tools: &[&str]| tools.iter().any(|t| fm.tools.allows(t));
    // A shell the node runs read-only (PLAN.md T70): shown as its own
    // switch, not as "Run commands".
    let read_only_shell = fm.tools.allows(harness_core::agent::SHELL)
        && match &fm.tools {
            ToolSelection::List(l) => !changes_things(l),
            ToolSelection::All(_) => false,
        };
    let features = Features {
        memory: fm.memory,
        project_memory: fm.project_memory,
        personalisation: fm.personalisation,
        recall: fm.tools.allows(RECALL_TOOL),
        board: fm.tools.allows(BOARD_TOOL),
        web: any(WEB_TOOLS),
        read_files: any(READ_TOOLS),
        edit_files: any(EDIT_TOOLS),
        commands: any(COMMAND_TOOLS) && !read_only_shell,
        read_only_commands: read_only_shell,
        mcp: mcp_listed(&fm.tools),
        skills: fm.tools.allows(SKILL_TOOL),
        delegate: !fm.delegates.is_empty(),
        background_subagents: fm.background_subagents && !fm.delegates.is_empty(),
        as_agent: fm.available_as.contains(&Use::Agent),
        as_subagent: fm.available_as.contains(&Use::Subagent),
    };
    Ok(Agent {
        id: id.into(),
        name: fm.name.clone(),
        description: fm.description,
        aliases,
        category,
        builtin: origin == Origin::Builtin,
        origin,
        scope: AgentScope::Server,
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
            vision: vec![],
            context_windows: Default::default(),
            effort: fm.effort,
            ui: fm.ui,
            no_personalisation: !fm.personalisation,
            prompt_hash,
            deny_tools: vec![],
            deny_skills: vec![],
            deny_mcp_servers: vec![],
            stored_skills: vec![],
            connector_tools: vec![],
            delegates: vec![],
            caps,
        },
    })
}

/// Checked aliases (valid ids, not `id`, no repeats, at most
/// [`MAX_ALIASES`]) and the category (trimmed, printable, at most
/// [`MAX_CATEGORY`] characters; none or blank is [`DEFAULT_CATEGORY`]).
pub fn labels(
    id: &str,
    aliases: &[String],
    category: Option<&str>,
) -> Result<(Vec<String>, String), AgentError> {
    let bad = |msg: String| AgentError::Frontmatter { id: id.into(), msg };
    if aliases.len() > MAX_ALIASES {
        return Err(bad(format!("at most {MAX_ALIASES} aliases")));
    }
    let mut seen = HashSet::new();
    for a in aliases {
        if !valid_id(a) || a == id || !seen.insert(a.as_str()) {
            return Err(bad(format!(
                "bad alias {a:?}: aliases are 1-32 characters (a-z, 0-9, -), each once, not the id"
            )));
        }
    }
    let category = category.map(str::trim).unwrap_or_default();
    if category.chars().count() > MAX_CATEGORY || category.chars().any(char::is_control) {
        return Err(bad(format!(
            "the category must be at most {MAX_CATEGORY} printable characters"
        )));
    }
    let category = if category.is_empty() {
        DEFAULT_CATEGORY
    } else {
        category
    };
    Ok((aliases.to_vec(), category.to_string()))
}

/// Settle the one namespace of ids and aliases over `agents`, in order: an
/// alias that is another agent's id, or that an earlier agent claimed, is
/// dropped from the later one and reported. Then every `delegates` names
/// ids (an alias there becomes its agent's id; itself and repeats go).
pub(crate) fn settle(agents: &mut [Agent]) -> Vec<AgentError> {
    let names: HashMap<String, String> = agents
        .iter()
        .map(|a| (a.id.clone(), a.name.clone()))
        .collect();
    let mut owner: HashMap<String, String> = HashMap::new();
    let mut errors = Vec::new();
    for a in agents.iter_mut() {
        let (id, name) = (a.id.clone(), a.name.clone());
        a.aliases.retain(|alias| {
            let by = if names.contains_key(alias) {
                Some((alias.clone(), "id"))
            } else {
                owner.get(alias).map(|o| (o.clone(), "alias"))
            };
            match by {
                Some((by, how)) => {
                    errors.push(AgentError::AliasTaken(Box::new(AliasClash {
                        id: id.clone(),
                        name: name.clone(),
                        alias: alias.clone(),
                        by_name: names.get(&by).cloned().unwrap_or_default(),
                        by,
                        how,
                    })));
                    false
                }
                None => {
                    owner.insert(alias.clone(), id.clone());
                    true
                }
            }
        });
    }
    for a in agents.iter_mut() {
        let mut seen = HashSet::new();
        let delegates = std::mem::take(&mut a.delegates);
        a.delegates = delegates
            .into_iter()
            .map(|d| owner.get(&d).cloned().unwrap_or(d))
            .filter(|d| *d != a.id && seen.insert(d.clone()))
            .collect();
    }
    errors
}

/// Whether a tool list asks for MCP tools: `all`, `mcp`, or one by name
/// (what [`AgentSpec::allows_mcp`] reads before a turn's `deny_tools`).
fn mcp_listed(tools: &ToolSelection) -> bool {
    match tools {
        ToolSelection::All(_) => true,
        ToolSelection::List(l) => l.iter().any(|t| {
            t == harness_core::agent::MCP_ALL || t.starts_with(harness_core::agent::MCP_PREFIX)
        }),
    }
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
        errors.extend(settle(&mut agents));
        (Self::from_agents(agents, DEFAULT_AGENT), errors)
    }

    /// The default is `default_id` when that agent may run a chat, else the
    /// first one that may: a sub-agent-only agent is never the default.
    /// Ids and aliases are settled here too (see [`AgentSet::load`] for
    /// what is dropped; this reports nothing).
    pub fn from_agents(mut agents: Vec<Agent>, default_id: &str) -> Self {
        let _ = settle(&mut agents);
        let usable = |a: &&Agent| a.features.as_agent;
        let default_id = agents
            .iter()
            .filter(usable)
            .find(|a| a.id == default_id)
            .or_else(|| agents.iter().find(usable))
            .map(|a| a.id.clone())
            .unwrap_or_default();
        Self { agents, default_id }
    }

    pub fn list(&self) -> &[Agent] {
        &self.agents
    }

    /// An agent by its id (what sessions, events and settings store).
    pub fn get(&self, id: &str) -> Option<&Agent> {
        self.agents.iter().find(|a| a.id == id)
    }

    /// An agent by its id or one of its aliases (what people type: an
    /// `@mention`, a prompt's `agent_id`).
    pub fn resolve(&self, handle: &str) -> Option<&Agent> {
        self.get(handle).or_else(|| {
            self.agents
                .iter()
                .find(|a| a.aliases.iter().any(|x| x == handle))
        })
    }

    /// Whether `handle` is any agent's id or alias.
    pub fn taken(&self, handle: &str) -> bool {
        self.resolve(handle).is_some()
    }

    pub fn default_agent(&self) -> Option<&Agent> {
        self.get(&self.default_id)
    }

    pub fn default_id(&self) -> &str {
        &self.default_id
    }
}
