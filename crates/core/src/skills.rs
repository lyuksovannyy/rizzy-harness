//! Skills (ADOPTION_PLAN X.2): folders with a `SKILL.md` that the agent
//! loads by name when a task calls for it. The node finds them at the
//! start of every turn (`harness_tools::skills::discover`) and hands what
//! it found to the loop as `TurnExtra::skills`; the `skill` tool reads a
//! body from there. The catalogue is the node's to build: what the
//! protocol brings is the master's stored skills (below), text the node
//! merges under its own.
//!
//! **Stored skills** (X.34 step B, PLAN.md T81) are text kept on the master:
//! a name, a description and a body that the server's or a workspace's
//! admins wrote. The master sends their names and descriptions with each
//! turn (`AgentSpec::stored_skills`), the node merges them into its
//! catalogue under its own skills ([`SkillOrigin::Stored`]), and a body is
//! asked for only when the agent loads one ([`StoredSkillSource`]). Nothing
//! of one names a file or a program: it is text only.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Where a skill was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillOrigin {
    /// A `.agents/skills` or `.claude/skills` folder of the session's
    /// repository: the repository's text, like its `AGENTS.md`.
    Repository,
    /// A folder the node's owner named (node.toml `[skills] roots`).
    Node,
    /// Text kept on the master (X.34 step B): no folder, no file.
    Stored(StoredScope),
}

/// Who wrote a stored skill: the server's admins (shown as Built-in,
/// offered in every workspace) or one workspace's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredScope {
    Server,
    Workspace,
}

/// A stored skill as the master lists it with a turn: its name and its
/// one-line description, never its body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredSkillEntry {
    pub name: String,
    pub description: String,
    pub scope: StoredScope,
}

/// Stored skills one spec may list; the node takes none from a longer
/// list (it fails closed, nothing is cut).
pub const MAX_STORED_LISTED: usize = 150;
/// A stored skill's description as a turn's list carries it: one line of
/// at most this many characters (the store keeps the whole of it).
pub const WIRE_DESCRIPTION: usize = 200;
/// A stored skill's description, at most (characters; the format's limit).
pub const STORED_DESCRIPTION_MAX: usize = 1024;

/// A stored skill's name, the Agent Skills format's rule: 1 to 64 of
/// `a-z`, `0-9` and `-`, not starting or ending with `-`, no `--`. Every
/// such name is also a [`valid_skill_name`].
pub fn valid_stored_skill_name(n: &str) -> bool {
    let b = n.as_bytes();
    !b.is_empty()
        && b.len() <= SKILL_NAME_MAX
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
        && b[0] != b'-'
        && b[b.len() - 1] != b'-'
        && !n.contains("--")
}

/// Where a session's loads of a stored skill go: the node's link to the
/// master, bound to the session the call runs for. The request names the
/// skill and nothing else; the master decides from the session alone
/// whether this turn's agent may have it.
#[async_trait::async_trait]
pub trait StoredSkillSource: Send + Sync {
    async fn load(&self, name: &str) -> Result<String, String>;
}

/// One skill, as discovery found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    /// From the front matter's `name`, else the folder's name.
    pub name: String,
    /// One line: control characters and line breaks are spaces.
    pub description: String,
    /// The skill's folder, canonical: its `SKILL.md` and every file the
    /// `skill` tool reads must lie inside it. Empty for a stored skill,
    /// which has none.
    pub dir: PathBuf,
    pub origin: SkillOrigin,
    /// `disable-model-invocation: true`: not listed to the model, loaded
    /// only in the turn the person names it (`/name`).
    pub manual: bool,
}

/// What one turn's discovery found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalogue {
    /// Sorted by name; one skill per name.
    pub skills: Vec<Skill>,
    /// The skill the person's message names with `/name` in this turn.
    pub invoked: Option<String>,
    /// Skill folders that hold a `SKILL.md` that could not be used (refused
    /// or unreadable), by folder name: named to the model, the reasons in
    /// the node's log.
    pub refused: Vec<String>,
    /// Canonical folders of the node owner's skills (each `[skills] roots`
    /// entry that exists, and a skill folder that is a link out of it): the
    /// agent's shell may read them, never write (`bash` grants them
    /// read-only to sessions whose agent has the `skill` tool).
    pub read: Vec<PathBuf>,
    /// Skills found here that this turn's agent may not use in its
    /// workspace (`AgentSpec::deny_skills`, X.34), by name: left out of
    /// `skills` and of `read`, so the tool and `/name` can say why rather
    /// than "no such skill".
    pub withheld: Vec<String>,
    /// Skills whose front matter says `user-invocable: false` (X.65), by
    /// name: left out of the composer's `/` menu and nothing else; the model
    /// still sees them and `/name` still asks for them.
    pub not_invocable: Vec<String>,
    /// Stored skills left out this turn because one of the node's own has
    /// the name (the node's wins), by name: for the node's log only.
    pub shadowed: Vec<String>,
    /// Folder names of `refused` that lie in the node owner's own roots: a
    /// stored skill never stands in for one of them (the node's name wins,
    /// review of PR 148 L2).
    pub refused_node: Vec<String>,
}

/// A skill's name: 1 to 64 ASCII letters, digits, `.`, `_` or `-`,
/// starting with a letter or digit (the Agent Skills format's limit). The
/// one rule for a found skill and an announced one (X.34), so a name never
/// holds a look-alike letter, a hidden character or a line break.
pub const SKILL_NAME_MAX: usize = 64;

pub fn valid_skill_name(n: &str) -> bool {
    let b = n.as_bytes();
    !b.is_empty()
        && b.len() <= SKILL_NAME_MAX
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

impl Catalogue {
    /// The skill called `name` that this turn may load: a listed one, or
    /// the one the person invoked.
    pub fn loadable(&self, name: &str) -> Option<&Skill> {
        self.skills
            .iter()
            .find(|s| s.name == name)
            .filter(|s| !s.manual || self.invoked.as_deref() == Some(name))
    }

    /// Whether `name` is a skill this turn's agent may not use here.
    pub fn is_withheld(&self, name: &str) -> bool {
        self.withheld.iter().any(|w| w == name)
    }
}
