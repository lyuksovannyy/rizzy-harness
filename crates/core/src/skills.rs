//! Skills (ADOPTION_PLAN X.2): folders with a `SKILL.md` that the agent
//! loads by name when a task calls for it. The node finds them at the
//! start of every turn (`harness_tools::skills::discover`) and hands what
//! it found to the loop as `TurnExtra::skills`; the `skill` tool reads a
//! body from there. Set only by the node, never from the protocol.

use std::path::PathBuf;

/// Where a skill was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillOrigin {
    /// A `.agents/skills` or `.claude/skills` folder of the session's
    /// repository: the repository's text, like its `AGENTS.md`.
    Repository,
    /// A folder the node's owner named (node.toml `[skills] roots`).
    Node,
}

/// One skill, as discovery found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    /// From the front matter's `name`, else the folder's name.
    pub name: String,
    /// One line: control characters and line breaks are spaces.
    pub description: String,
    /// The skill's folder, canonical: its `SKILL.md` and every file the
    /// `skill` tool reads must lie inside it.
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
}
