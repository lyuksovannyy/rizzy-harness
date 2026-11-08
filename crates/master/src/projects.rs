//! Projects (docs/projects.md): a group of chats inside a workspace, with
//! instructions, its own memory and a task board. This is the domain side:
//! limits, validation, the built-in board templates, who may do what, and
//! the per-turn context a project chat gets. HTTP lives in `http/projects.rs`,
//! storage in `harness_store` (projects.rs).
//!
//! A project lives in one workspace (Personal included). "Which workspace"
//! and "may manage projects" sit behind the helpers below, over
//! `crate::workspaces`.

use std::collections::{HashMap, HashSet};

use harness_core::SessionId;
use harness_store::{NewColumn, ProjectLink, ProjectRow, SessionRow};
use serde::{Deserialize, Serialize};

use crate::Master;
use crate::auth::Principal;
use crate::workspaces::{self as ws, Access, Perm, WsError};

/// Icon names the web maps to `lucide-react` icons (web/src/lib/projectIcons.tsx).
pub const ICONS: &[&str] = &[
    "folder",
    "briefcase",
    "code",
    "terminal",
    "rocket",
    "book-open",
    "graduation-cap",
    "flask-conical",
    "lightbulb",
    "pen-tool",
    "palette",
    "camera",
    "music",
    "film",
    "gamepad-2",
    "heart",
    "house",
    "plane",
    "chart-line",
    "globe",
];
pub const DEFAULT_ICON: &str = "folder";
pub const DEFAULT_ACCENT: &str = "blue";

pub const MAX_PROJECTS: usize = 200;
pub const MAX_COLUMNS: usize = 12;
pub const MAX_CARDS: usize = 500;
pub const MAX_TEMPLATES: usize = 50;
pub const NAME_MAX: usize = 64;
pub const DESCRIPTION_MAX: usize = 280;
/// Instructions go into every turn of every chat in the project.
pub const INSTRUCTIONS_MAX: usize = 4000;
pub const COLUMN_NAME_MAX: usize = 40;
pub const TITLE_MAX: usize = 200;
pub const NOTES_MAX: usize = 4000;
const CWD_MAX: usize = 1024;

/// Project facts recalled per turn, and open cards shown to the model.
const RECALL_FACTS: usize = 6;
const CONTEXT_CARDS: usize = 8;
const CONTEXT_TITLE_CHARS: usize = 80;

/// A board template in the binary: its id, name and columns (name, done).
pub struct Template {
    pub id: &'static str,
    pub name: &'static str,
    pub columns: &'static [(&'static str, bool)],
}

/// The built-in templates; the first is the default.
pub const TEMPLATES: &[Template] = &[
    Template {
        id: "basic",
        name: "Basic",
        columns: &[("To do", false), ("In progress", false), ("Done", true)],
    },
    Template {
        id: "software",
        name: "Software",
        columns: &[
            ("Backlog", false),
            ("To do", false),
            ("In progress", false),
            ("Review", false),
            ("Done", true),
        ],
    },
    Template {
        id: "writing",
        name: "Writing",
        columns: &[
            ("Ideas", false),
            ("Drafting", false),
            ("Editing", false),
            ("Published", true),
        ],
    },
];

impl Template {
    pub fn new_columns(&self) -> Vec<NewColumn> {
        self.columns
            .iter()
            .map(|(name, done)| NewColumn {
                name: name.to_string(),
                done: *done,
            })
            .collect()
    }
}

pub fn builtin_template(id: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.id == id)
}

/// Per-project switches and defaults (stored as JSON in `projects.settings`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectSettings {
    /// The project's chats also recall the workspace's shared facts, once
    /// workspace memory exists (docs/projects.md, Memory).
    pub workspace_memory: bool,
    /// Defaults for agentic chats started in the project.
    pub node_id: Option<String>,
    pub cwd: Option<String>,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            workspace_memory: true,
            node_id: None,
            cwd: None,
        }
    }
}

impl ProjectSettings {
    /// The stored document; unreadable ones read as the defaults.
    pub fn read(json: &str) -> Self {
        serde_json::from_str(json).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "unreadable project settings; using defaults");
            Self::default()
        })
    }

    /// Trim the defaults; empty ones are none.
    pub fn cleaned(mut self) -> Result<Self, String> {
        let opt = |v: Option<String>, what: &str, max: usize| -> Result<Option<String>, String> {
            match v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
                Some(s) if s.chars().count() > max || s.chars().any(char::is_control) => Err(
                    format!("{what} must be one line of at most {max} characters"),
                ),
                other => Ok(other),
            }
        };
        self.node_id = opt(self.node_id, "node_id", 128)?;
        self.cwd = opt(self.cwd, "cwd", CWD_MAX)?;
        Ok(self)
    }
}

/// Trim `s` and drop control characters (newlines too unless `multiline`)
/// and the hidden set (bidi, zero-width, tags): this text reaches other
/// members' turns, like memory facts (`harness_memory::clean_text`).
pub fn clean(what: &str, s: &str, max: usize, multiline: bool) -> Result<String, String> {
    let kept: String = s
        .chars()
        .filter(|c| {
            (!c.is_control() || (multiline && *c == '\n')) && !harness_core::text::is_hidden(*c)
        })
        .collect();
    let kept = kept.trim().to_string();
    if kept.chars().count() > max {
        return Err(format!("{what} is at most {max} characters"));
    }
    Ok(kept)
}

/// Like [`clean`], and not empty.
pub fn clean_required(what: &str, s: &str, max: usize) -> Result<String, String> {
    let s = clean(what, s, max, false)?;
    if s.is_empty() {
        return Err(format!("{what} cannot be empty"));
    }
    Ok(s)
}

pub fn check_icon(icon: &str) -> Result<(), String> {
    if ICONS.contains(&icon) {
        Ok(())
    } else {
        Err(format!("icon must be one of: {}", ICONS.join(", ")))
    }
}

pub fn check_accent(accent: &str) -> Result<(), String> {
    if crate::prefs::ACCENTS.contains(&accent) {
        Ok(())
    } else {
        Err(format!(
            "accent must be one of: {}",
            crate::prefs::ACCENTS.join(", ")
        ))
    }
}

/// The memory-store owner key of a project's facts. User ids are UUIDs, so
/// it never collides with a user's, and `/api/memory` never reaches it.
pub fn memory_owner(project_id: &str) -> String {
    format!("project:{project_id}")
}

// ---- workspace seams (docs/projects.md, Permissions) ----

/// The workspace a project list, a template list or a new project applies
/// to: the one asked for (members only), else the caller's active one.
/// `None` only for the API token without one (it then lists every
/// workspace's projects).
pub async fn current_workspace(
    m: &Master,
    who: &Principal,
    requested: Option<&str>,
) -> Result<Option<Access>, WsError> {
    ws::scope(m, who, requested).await
}

/// The caller's standing in `workspace`: members see its projects, boards
/// and confirmed facts (guests too). Non-members get `NotFound`; the API
/// token acts as an owner of every existing workspace.
pub async fn member(m: &Master, who: &Principal, workspace: &str) -> Result<Access, WsError> {
    ws::access(m, who, workspace).await
}

/// "May manage projects": create, edit and delete projects, change boards
/// and saved templates, write confirmed project facts. The workspace
/// permission `settings` (owner, admin; everyone in their Personal
/// workspace) until the workspaces contract has a permission of its own for
/// this (docs/projects.md, open questions).
pub fn may_manage(a: &Access) -> bool {
    a.can(Perm::Settings)
}

/// The workspace a chat is in: its effective one, which falls back to the
/// owner's Personal workspace once they leave a team. `None` only for a
/// chat without an owner or a workspace (API token).
pub fn chat_workspace(row: &SessionRow) -> Option<&str> {
    row.workspace_id.as_deref()
}

// ---- chats in projects ----

/// Every chat's stored link, by chat.
pub async fn links(
    m: &Master,
) -> Result<HashMap<SessionId, ProjectLink>, harness_store::StoreError> {
    let all = m.inner.mirror.call(|s| s.project_links(None)).await?;
    Ok(all.into_iter().map(|l| (l.session_id, l)).collect())
}

/// A chat's effective project: its link, while the project is in the chat's
/// workspace (a chat that moved workspace keeps an inert link).
pub fn effective<'a>(
    links: &'a HashMap<SessionId, ProjectLink>,
    row: &SessionRow,
) -> Option<&'a str> {
    let l = links.get(&row.session_id)?;
    (chat_workspace(row) == Some(l.workspace_id.as_str())).then_some(l.project_id.as_str())
}

/// The project a chat is in, if any. A sub-agent's session counts as part
/// of its parent's project.
pub async fn project_of(m: &Master, row: &SessionRow) -> Option<ProjectRow> {
    let mut row = row.clone();
    if let Some(parent) = row
        .parent_id
        .as_deref()
        .and_then(|p| p.parse::<SessionId>().ok())
    {
        row = m
            .inner
            .mirror
            .call(move |s| s.session(parent))
            .await
            .ok()??;
    }
    let sid = row.session_id;
    let link = m
        .inner
        .mirror
        .call(move |s| s.session_project(sid))
        .await
        .ok()??;
    if chat_workspace(&row) != Some(link.workspace_id.as_str()) {
        return None;
    }
    let pid = link.project_id;
    m.inner.mirror.call(move |s| s.project(&pid)).await.ok()?
}

/// What a turn may write to memory: personal facts (the agent's and the
/// member's switches), project facts (the chat is in a project and the
/// agent's project memory is on). `Master::prompt` decides it when the turn
/// starts; `remember` calls are checked against it when they are filed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnMemory {
    pub personal: bool,
    pub project: bool,
}

/// Withhold `remember` from an agent whose turn may write no memory, and
/// from each of its sub-agents whose own switches leave it none.
pub fn offer_remember(
    agents: &harness_agents::AgentSet,
    spec: &mut harness_core::agent::AgentSpec,
    t: TurnMemory,
) {
    if !t.personal && !t.project {
        crate::deny_remember(spec);
        return;
    }
    for d in &mut spec.delegates {
        let f = agents.get(&d.agent_id).map(|a| a.features);
        let some = (t.personal && f.is_none_or(|f| f.memory))
            || (t.project && f.is_none_or(|f| f.project_memory));
        if !some {
            d.deny_tools.push("remember".into());
        }
    }
}

/// What the turn behind a `remember` call from `row` may write: the one its
/// chat (a sub-agent's: its parent's) recorded, narrowed by the calling
/// agent's own switches where it is not the turn's (a sub-agent), or where
/// nothing was recorded (a turn from before a restart).
pub async fn turn_memory(m: &Master, row: &SessionRow) -> TurnMemory {
    let turn = row
        .parent_id
        .as_deref()
        .and_then(|p| p.parse::<SessionId>().ok())
        .unwrap_or(row.session_id);
    let recorded = crate::lock(&m.inner.turn_memory).get(&turn).copied();
    let mut t = match recorded {
        Some(t) => t,
        None => TurnMemory {
            personal: ws::uses_memory(m, row).await,
            project: true,
        },
    };
    if row.parent_id.is_some() || recorded.is_none() {
        let r = m.resolver_for(row.workspace_id.as_deref()).await;
        let agents = r.agents();
        let agent = match row.agent_id.as_deref() {
            Some(id) => agents.get(id),
            None => agents.default_agent(),
        };
        if let Some(f) = agent.map(|a| a.features) {
            t.personal &= f.memory;
            t.project &= f.project_memory;
        }
    }
    t
}

/// Where a `remember { scope: "project" }` from this chat goes: the
/// project's memory, when the chat is in a project. It waits there for the
/// chat's owner, a member of the project's workspace (the chat's effective
/// workspace says so). `None` files it as a personal proposal instead.
pub async fn proposal_owner(m: &Master, row: &SessionRow) -> Option<String> {
    let p = project_of(m, row).await?;
    Some(memory_owner(&p.project_id))
}

/// Every project's id, before a deletion that may cascade to projects
/// (a workspace, an account); see [`forget_gone`].
pub async fn ids(m: &Master) -> HashSet<String> {
    match m.inner.mirror.call(|s| s.projects(None)).await {
        Ok(rows) => rows.into_iter().map(|r| r.project_id).collect(),
        Err(e) => {
            tracing::warn!(error = %e, "could not list projects");
            HashSet::new()
        }
    }
}

/// After such a deletion: forget the facts of every project it took (the
/// memory store is not in master.db, so no foreign key reaches it), and
/// tell the clients.
pub async fn forget_gone(m: &Master, before: HashSet<String>) {
    let now = ids(m).await;
    let gone: Vec<&String> = before.iter().filter(|p| !now.contains(*p)).collect();
    if gone.is_empty() {
        return;
    }
    for p in gone {
        delete_memory(m, p).await;
    }
    let _ = m.inner.live.send(crate::Live::ProjectsChanged);
    let _ = m.inner.live.send(crate::Live::MemoryChanged);
}

/// Forget every fact of a deleted project.
pub async fn delete_memory(m: &Master, project_id: &str) {
    let Some(mem) = m.memory() else { return };
    let owner = memory_owner(project_id);
    match mem.list(Some(&owner)).await {
        Ok(facts) => {
            for f in facts {
                if let Err(e) = mem.delete(&f.id).await {
                    tracing::warn!(error = %e, "could not delete a project fact");
                }
            }
        }
        Err(e) => tracing::warn!(error = %e, "could not list a deleted project's facts"),
    }
}

// ---- per-turn context ----

fn cut(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
    t.push('…');
    t
}

/// The open cards at most `CONTEXT_CARDS` titles, the columns nearest to
/// done first (done columns left out): "Board — In progress: a; b. To do: c
/// (+2 more)."
fn board_line(
    cols: &[harness_store::ColumnRow],
    cards: &[harness_store::CardRow],
) -> Option<String> {
    let mut parts = Vec::new();
    let (mut shown, mut more) = (0usize, 0usize);
    for c in cols.iter().rev().filter(|c| !c.done) {
        let titles: Vec<String> = cards
            .iter()
            .filter(|k| k.column_id == c.column_id)
            .map(|k| cut(&k.title, CONTEXT_TITLE_CHARS))
            .collect();
        if titles.is_empty() {
            continue;
        }
        let room = CONTEXT_CARDS - shown;
        more += titles.len().saturating_sub(room);
        let take: Vec<String> = titles.into_iter().take(room).collect();
        shown += take.len();
        if !take.is_empty() {
            parts.push(format!("{}: {}", c.name, take.join("; ")));
        }
    }
    if parts.is_empty() {
        return None;
    }
    let tail = if more > 0 {
        format!(" (+{more} more)")
    } else {
        String::new()
    };
    Some(format!("Board — {}{tail}.", parts.join(". ")))
}

/// What a turn in a chat of project `p` gets after the history (never in
/// the system prompt; PLAN.md "Token savings"): the project's name and
/// instructions, its facts recalled by `text` (when `allowed.project`), and
/// the open cards. `remember` says whether the turn may propose project
/// facts (it has the tool); `allowed.personal` whether personal ones too.
pub async fn turn_context(
    m: &Master,
    p: &ProjectRow,
    text: &str,
    allowed: TurnMemory,
    remember: bool,
) -> String {
    let mut s = format!("Project \"{}\"", p.name);
    if !p.description.is_empty() {
        s.push_str(" — ");
        s.push_str(&p.description);
    }
    s.push_str(
        "\nThis chat is part of the project. Its instructions, memory and board are written by \
         the people working on it.",
    );
    if !p.instructions.is_empty() {
        s.push_str("\n\nProject instructions:\n");
        s.push_str(&p.instructions);
    }
    if let Some(mem) = m
        .memory()
        .filter(|_| allowed.project && !text.trim().is_empty())
    {
        match mem
            .recall(Some(&memory_owner(&p.project_id)), text, RECALL_FACTS)
            .await
        {
            Ok(facts) if !facts.is_empty() => {
                s.push_str("\n\nWhat this project has established (confirmed; may be outdated):");
                for f in facts {
                    s.push_str("\n- ");
                    s.push_str(&f.text);
                }
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "project memory recall failed"),
        }
    }
    let pid = p.project_id.clone();
    if let Ok((cols, cards)) = m.inner.mirror.call(move |s| s.board(&pid)).await
        && let Some(line) = board_line(&cols, &cards)
    {
        s.push_str("\n\n");
        s.push_str(&line);
    }
    if remember {
        s.push_str(
            "\n\nTo keep a lasting decision or fact about this project, call `remember` with \
             scope \"project\".",
        );
        if !allowed.personal {
            s.push_str(
                " The user keeps their personal memory out of this workspace: use no other scope.",
            );
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_store::{CardRow, ColumnRow};

    fn col(id: &str, name: &str, done: bool) -> ColumnRow {
        ColumnRow {
            column_id: id.into(),
            name: name.into(),
            position: 0,
            done,
        }
    }

    fn card(col: &str, title: &str) -> CardRow {
        CardRow {
            card_id: title.into(),
            column_id: col.into(),
            title: title.into(),
            notes: String::new(),
            position: 0,
            session_id: None,
            created_by: None,
            created_ms: 0,
            updated_ms: 0,
        }
    }

    #[test]
    fn board_line_skips_done_and_caps() {
        let cols = [
            col("a", "To do", false),
            col("b", "In progress", false),
            col("c", "Done", true),
        ];
        let mut cards = vec![card("b", "wire it"), card("c", "shipped")];
        assert_eq!(
            board_line(&cols, &cards).unwrap(),
            "Board — In progress: wire it."
        );
        for i in 0..9 {
            cards.push(card("a", &format!("t{i}")));
        }
        let line = board_line(&cols, &cards).unwrap();
        assert!(
            line.starts_with("Board — In progress: wire it. To do: t0; t1"),
            "{line}"
        );
        assert!(line.ends_with("t6 (+2 more)."), "{line}");
        assert!(!line.contains("shipped"));
        assert_eq!(board_line(&cols, &[card("c", "x")]), None);
    }

    #[test]
    fn cleaning_drops_hidden_text() {
        assert_eq!(
            clean("name", " A\u{202E}b\u{200B}\nc ", 10, false).unwrap(),
            "Abc"
        );
        assert_eq!(clean("notes", "a\nb\u{7}", 10, true).unwrap(), "a\nb");
        assert!(clean("name", "abcdef", 5, false).is_err());
        assert!(clean_required("name", " \u{200B} ", 5).is_err());
    }

    #[test]
    fn settings_read_leniently_and_clean() {
        assert_eq!(
            ProjectSettings::read("not json"),
            ProjectSettings::default()
        );
        let s = ProjectSettings {
            workspace_memory: false,
            node_id: Some("  ".into()),
            cwd: Some(" /src/app ".into()),
        }
        .cleaned()
        .unwrap();
        assert_eq!(s.node_id, None);
        assert_eq!(s.cwd.as_deref(), Some("/src/app"));
        assert!(
            ProjectSettings {
                cwd: Some("a\nb".into()),
                ..Default::default()
            }
            .cleaned()
            .is_err()
        );
    }

    #[test]
    fn templates_and_names_are_valid() {
        assert_eq!(TEMPLATES[0].id, "basic");
        for t in TEMPLATES {
            assert!(t.columns.len() <= MAX_COLUMNS);
            assert!(
                t.columns.iter().any(|(_, done)| *done),
                "{} has a done column",
                t.id
            );
        }
        assert!(check_icon(DEFAULT_ICON).is_ok());
        assert!(check_icon("skull").is_err());
        assert!(check_accent(DEFAULT_ACCENT).is_ok());
        assert!(check_accent("red").is_err());
        assert_eq!(memory_owner("p1"), "project:p1");
    }
}
