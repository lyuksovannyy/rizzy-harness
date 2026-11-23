//! Node-local policy (`node.toml`). Authoritative: nothing the master sends
//! can loosen it (PLAN.md T5).

use std::path::{Component, Path, PathBuf};

use harness_core::policy::ApprovalMode;
use serde::{Deserialize, Serialize};

/// Checkpoints kept per working folder unless `node.toml` says otherwise.
/// It was 10, which the readiness trial of 2026-11-19 outgrew in about a
/// dozen short turns (a turn could then be neither undone nor shown); what
/// 100 costs is in `docs/checkpoint-cost.md`.
pub const DEFAULT_CHECKPOINTS_KEEP: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NodePolicy {
    /// The loosest approval mode any session on this node may use
    /// (`ask_all`, `ask`, `accept_edits`, `auto`); [`DEFAULT_MAX_APPROVAL`]
    /// when node.toml does not say.
    pub max_approval_mode: ApprovalMode,
    /// Session working directories must be inside one of these.
    pub workspace_roots: Vec<PathBuf>,
    /// Tools that are always refused.
    pub denied_tools: Vec<String>,
    /// "Allow this exact call for this chat" (PLAN.md T51): off, no card
    /// offers it and grants already in a chat's log are not honoured, so
    /// `max_approval_mode = "ask"` makes every change and command ask.
    pub chat_grants: bool,
    /// Allow file tools to touch paths outside the session workspace.
    pub allow_outside_workspace: bool,
    /// File checkpoints kept per working folder for undo/rewind (0 = off;
    /// default [`DEFAULT_CHECKPOINTS_KEEP`]). A turn is one, and so is the
    /// saving an undo makes of the folder.
    pub checkpoints_keep: usize,
    /// Let agents query a graphify knowledge graph (runs the third-party
    /// `graphify` program here; install it with `rizzy-harness graphify install`).
    pub graphify: bool,
    /// Publishing files as artifacts (`[artifacts]`).
    pub artifacts: ArtifactsPolicy,
    /// Variables passed to tool child processes on top of the core list
    /// (`PATH`, `HOME`, `LANG`, ...): names or globs such as `"CARGO_*"`.
    /// Provider keys, `.env` names and `HARNESS_*` are never passed.
    pub env_pass: Vec<String>,
    /// Resource limits (`[limits]`).
    pub limits: LimitsPolicy,
    /// The shell sandbox (`[sandbox]`, FIX_PLAN 2.7).
    pub sandbox: SandboxPolicy,
    /// The execution modes this node may offer (`[execution]`, N.10a): its
    /// ceiling. What it offers is decided at start, from this and its own
    /// self-test ([`Self::exec`]).
    pub execution: ExecutionPolicy,
    /// Skills (`[skills]`, ADOPTION_PLAN X.2). Left out of node.toml (or
    /// no node.toml): [`SkillsPolicy::node_toml`], the owner's home roots.
    #[serde(default = "SkillsPolicy::node_toml")]
    pub skills: SkillsPolicy,
    /// The node operator's instruction file (`[instructions]`, X.2).
    pub instructions: InstructionsPolicy,
    /// MCP servers (`[mcp_servers.<name>]`, ADOPTION_PLAN X.3): stdio
    /// programs each agentic chat may start, in its own confinement, and
    /// whose tools its agent is offered. Only this file adds one.
    pub mcp_servers: std::collections::BTreeMap<String, harness_tools::mcp::ServerConfig>,
    /// A repository's own MCP servers (`[mcp]`, ADOPTION_PLAN X.36,
    /// PLAN.md T82): off by default.
    pub mcp: McpPolicy,
    /// Where the node makes new projects' folders on request (`[projects]`,
    /// ADOPTION_PLAN X.26, PLAN.md T77).
    pub projects: ProjectsPolicy,
    /// Cloning a forge repository into a new project's folder (`[forge]`,
    /// ADOPTION_PLAN X.27 step 3, PLAN.md T86): the hosts this node clones
    /// from. Nothing the master sends adds one.
    pub forge: ForgePolicy,
    /// The web tools this node's agentic chats have (`[web]`, ADOPTION_PLAN
    /// T.3, PLAN.md T89). A remote node: this file is the only source, and no
    /// table (or a key left out of it) means off. The server's own node: the
    /// server's `[web]` decides what exists, and a table here can only take
    /// away (`fetch = false`, `search = "none"`; a key left out counts as
    /// off) and never switches on what the server has off. Nothing the
    /// master sends switches either on.
    pub web: Option<WebPolicy>,
    /// Folders no session may include and no file tool may touch, in every
    /// mode: the runtime's own data directory (keys, the API token,
    /// databases). Set by the runtime with [`NodePolicy::protect`], never
    /// read from node.toml. Canonical paths.
    #[serde(skip)]
    pub protected_dirs: Vec<PathBuf>,
    /// The execution modes this node offers, set by the runtime at start
    /// from its self-test and [`Self::execution`] (N.10a): every agentic
    /// create chooses inside it, and every turn, callback, restore and
    /// delegation checks the session's recorded mode against it. Never
    /// read from node.toml.
    #[serde(skip)]
    pub exec: crate::exec::ExecOffers,
    /// Where the runtime keeps per-session temp folders (removed with the
    /// session). Never read from node.toml.
    #[serde(skip)]
    pub sandbox_tmp: Option<PathBuf>,
}

/// `[mcp]` in node.toml: a repository's own MCP servers (ADOPTION_PLAN
/// X.36, PLAN.md T82) and a workspace's custom connectors (X.35 step 2,
/// T85). Nothing the master sends can change it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct McpPolicy {
    /// `true` (default): a workspace's custom connectors, which the master
    /// speaks to with the chat owner's own account, are offered to this
    /// node's turns as MCP tools, and a call is filed with the master. A
    /// call runs nothing on this node and its token never reaches it; only
    /// the result's text enters the model's context. `false`: the master's
    /// list is ignored, nothing is asked, and the node does not announce
    /// the capability (so the master sends no list and answers no call).
    pub master_connectors: bool,
    /// `"off"` (default): no `.mcp.json` is ever read and every request
    /// about one is refused. `"ask"`: a person may approve, one by one and
    /// for their own chats only, the servers a project folder's
    /// `.mcp.json` declares; the node keeps its own copy of each approved
    /// entry, and that copy is what runs, in the chat's sandbox.
    pub project_servers: ProjectServers,
    /// Where the approved copies are kept (in the node's data directory,
    /// 0600). Set by the runtime, never read from node.toml; without it
    /// `"ask"` works as `"off"`.
    #[serde(skip)]
    pub approvals_file: Option<PathBuf>,
}

impl Default for McpPolicy {
    fn default() -> Self {
        Self {
            master_connectors: true,
            project_servers: ProjectServers::default(),
            approvals_file: None,
        }
    }
}

/// `[mcp] project_servers`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectServers {
    #[default]
    Off,
    Ask,
}

/// `[sandbox]` in node.toml: what the agent's shell (and graphify) may
/// touch. Nothing the master sends can change it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SandboxPolicy {
    /// `"required"` (default: refuse agentic sessions where Landlock does
    /// not work), `"best-effort"` or `"off"`.
    pub mode: harness_tools::sandbox::Mode,
    /// `"on"` (default) or `"off"`: deny TCP connect and bind.
    pub network: harness_tools::sandbox::Network,
    /// More read-only folders (toolchains: `~/.cargo`, `~/.rustup`, ...).
    /// `~/` is the node user's home.
    pub read: Vec<PathBuf>,
    /// More read-write folders (caches: `~/.cache`, `~/.npm`, ...).
    pub write: Vec<PathBuf>,
    /// Where bubblewrap is, absolute; taken as given. Without it, the first
    /// `bwrap` on `PATH` that this user cannot replace (owned by root).
    pub bwrap: Option<PathBuf>,
    /// The read-only shell's `/tmp` per command, in MiB (memory: a tmpfs of
    /// the command's own, host memory per running read-only command;
    /// 64 to 65536). Absent: 256 (PLAN.md T70).
    pub read_only_tmp_mib: Option<u64>,
    /// Whether the read-only shell (plan mode, read-only agents) reaches
    /// the network. Off by default (PLAN.md T70): their commands run
    /// unasked and may run the repository's own code. On, it follows
    /// `network` (it never widens it).
    pub read_only_network: bool,
}

/// `[skills]` in node.toml (ADOPTION_PLAN X.2, PLAN.md T46): where the
/// node looks for skill folders. `denied_tools = ["skill"]` turns skills
/// off altogether. Nothing the master sends can change it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SkillsPolicy {
    /// Read the skills of the session's repository (`.agents/skills` and
    /// `.claude/skills` from its top down to the session folder).
    pub repository: bool,
    /// The node owner's skill folders, absolute or under `~/` (the node
    /// user's home). In node.toml by default `~/.agents/skills` and
    /// `~/.claude/skills` ([`Self::HOME_ROOTS`]), where other harnesses keep
    /// personal skills. Every session on this node can read the skills in
    /// them.
    #[serde(default = "SkillsPolicy::home_roots")]
    pub roots: Vec<PathBuf>,
    /// Take the skills the server's and the workspace's admins stored on
    /// the master (X.34 step B, PLAN.md T81): their names and descriptions
    /// come with each turn, a body when the agent loads one. Text only, of
    /// the kind the instructions and memory the master sends already are;
    /// this node's own skill of a name wins. `false`: the node ignores the
    /// list and never asks. On by default.
    #[serde(default = "yes")]
    pub from_master: bool,
}

fn yes() -> bool {
    true
}

/// Built in code: no root. A policy a program makes for itself reads
/// nothing of the home folder unless it says so, as a policy built in code
/// reads no instruction file until it is located
/// ([`InstructionsPolicy::located`]); every test rig builds its policy this
/// way, so a developer's own skills never change what a test's agent is
/// offered. What a node reads by default is node.toml's default,
/// [`Self::node_toml`]: [`NodePolicy::load`] and the master's chat runtime
/// use it.
impl Default for SkillsPolicy {
    fn default() -> Self {
        Self {
            repository: true,
            roots: Vec::new(),
            from_master: true,
        }
    }
}

impl SkillsPolicy {
    /// node.toml's default `roots`.
    pub const HOME_ROOTS: [&'static str; 2] = ["~/.agents/skills", "~/.claude/skills"];

    /// [`Self::HOME_ROOTS`] as paths (serde's default for `roots`).
    pub fn home_roots() -> Vec<PathBuf> {
        Self::HOME_ROOTS.iter().map(PathBuf::from).collect()
    }

    /// `[skills]` as node.toml has it when it says nothing: the repository's
    /// skills and the owner's home roots.
    pub fn node_toml() -> Self {
        Self {
            repository: true,
            roots: Self::home_roots(),
            from_master: true,
        }
    }

    /// [`Self::roots`] with `~/` expanded; an entry that is neither
    /// absolute nor under `~/` (or `~/` without a home) is an error.
    pub fn roots(&self) -> Result<Vec<PathBuf>, String> {
        self.roots
            .iter()
            .map(|p| expand_home(p, "[skills] roots"))
            .collect()
    }
}

/// `p` with a leading `~/` made the node user's home; an error for a path
/// that is neither absolute nor under `~/`, or `~/` without a home.
fn expand_home(p: &Path, what: &str) -> Result<PathBuf, String> {
    let p = match (p.strip_prefix("~"), dirs::home_dir()) {
        (Ok(rest), Some(h)) => h.join(rest),
        (Ok(_), None) => {
            return Err(format!(
                "{what}: {} uses ~ but there is no home directory",
                p.display()
            ));
        }
        (Err(_), _) => p.to_path_buf(),
    };
    if p.is_absolute() {
        Ok(p)
    } else {
        Err(format!(
            "{what}: {} must be absolute (or start with ~/)",
            p.display()
        ))
    }
}

/// `[instructions]` in node.toml (ADOPTION_PLAN X.2, PLAN.md T62): the
/// node operator's own instruction file, what `~/.claude/CLAUDE.md` and
/// `~/.codex/AGENTS.md` are to those harnesses. Read at the start of every
/// agentic turn on this node, in front of the repository's files, for
/// every chat on the node, whoever owns it. Only this file sets where it
/// is; nothing the master sends names, widens or changes it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InstructionsPolicy {
    /// Where the file is, absolute or under `~/`. By default `AGENTS.md`
    /// in the node's data directory, which no agent's tool can write. A
    /// file that does not exist adds nothing.
    pub user_file: Option<PathBuf>,
    /// The file read: [`Self::user_file`], or the default in the data
    /// directory the runtime gave [`Self::locate`]. `None` (a policy built
    /// in code and never located): no file. Never read from node.toml.
    #[serde(skip)]
    pub located: Option<PathBuf>,
}

impl InstructionsPolicy {
    /// The file name the default is, in the node's data directory.
    pub const DEFAULT_NAME: &'static str = "AGENTS.md";

    /// [`Self::user_file`] with `~/` expanded. A `.` or `..` component is
    /// refused: the path is resolved once at start and protected as such,
    /// and `..` after a link names another folder than it seems to.
    pub fn user_file(&self) -> Result<Option<PathBuf>, String> {
        if let Some(p) = &self.user_file
            && p.components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(format!(
                "[instructions] user_file: {} has a `.` or `..` in it; name the file by its plain \
                 path",
                p.display()
            ));
        }
        self.user_file
            .as_deref()
            .map(|p| expand_home(p, "[instructions] user_file"))
            .transpose()
    }

    /// What is wrong with how the file at `path` (or its folder) is held,
    /// for the node's log and `doctor`: a file the node will not read
    /// ([`file_refusal`]: another owner, writable by others), a file with
    /// several names, a folder another user can write. Empty when nothing
    /// is, or nothing is there.
    pub fn warnings(path: &Path) -> Vec<String> {
        use std::os::unix::fs::MetadataExt;
        let me = nix::unistd::geteuid().as_raw();
        let mut out = Vec::new();
        if let Ok(m) = std::fs::symlink_metadata(path) {
            if let Some(why) = file_refusal(m.uid(), m.mode(), me) {
                out.push(format!("{}: {why}; it is not read", path.display()));
            }
            if m.is_file() && m.nlink() > 1 {
                out.push(format!(
                    "{}: it has {} names (hard links); a change through another name changes \
                     every chat's instructions",
                    path.display(),
                    m.nlink()
                ));
            }
        }
        if let Some(dir) = path.parent()
            && let Ok(m) = std::fs::metadata(dir)
        {
            let others_write = m.mode() & 0o022 != 0;
            if (m.uid() != me && m.uid() != 0) || others_write {
                out.push(format!(
                    "{}: another user can create or replace files in this folder, and so the \
                     node's instruction file in it",
                    dir.display()
                ));
            }
        }
        out
    }
}

/// Why a file owned by `uid` with permission bits `mode` is not read as the
/// operator's instructions on a node running as `me`: only the node's own
/// user or root may own it (another account would steer every chat on the
/// node, as AUDIT L7 refuses another user's data directory), and nobody
/// else may write it: group- or other-writable is refused for every owner,
/// root included. A group is not checked for being the node user's own:
/// `chmod go-w` costs the operator nothing, telling groups apart is a guess.
pub fn file_refusal(uid: u32, mode: u32, me: u32) -> Option<String> {
    if uid != me && uid != 0 {
        return Some(format!(
            "it belongs to another user (uid {uid}), not to the node's user or root"
        ));
    }
    (mode & 0o022 != 0).then(|| {
        "its group or other users can write it, and so set the instructions of every chat on \
         this node (chmod go-w)"
            .to_string()
    })
}

impl NodePolicy {
    /// Find the node operator's instruction file and protect it (X.2,
    /// PLAN.md T62): node.toml's `[instructions] user_file`, else
    /// `AGENTS.md` in `data_dir` (the node's data directory, already
    /// protected). Whatever its place, it joins [`Self::protected_dirs`],
    /// so no file tool, mention, listing or sandbox grant reaches it.
    /// A `user_file` an agent could write or the shell could read anyway is
    /// refused, with the reason: inside a `workspace_roots` entry (every
    /// chat's folder lies in one), inside a `[sandbox]` read or write
    /// folder, or inside a folder the sandbox grants every command
    /// (`/etc`, `/usr`, `/opt`...). Call after [`Self::protect`] of the
    /// data directory and before the sandbox starts.
    pub fn locate_instructions(&mut self, data_dir: &Path) -> Result<(), String> {
        let data_dir = std::path::absolute(data_dir).unwrap_or_else(|_| data_dir.to_path_buf());
        let file = match self.instructions.user_file()? {
            None => resolve_file(&data_dir.join(InstructionsPolicy::DEFAULT_NAME)),
            Some(p) => {
                let real = resolve_file(&p);
                let in_data = real.starts_with(resolve_existing(&data_dir));
                let why = |what: String| {
                    Err(format!(
                        "[instructions] user_file = {}: it lies inside {what}, so an agent could \
                         change or read it, and a text one chat wrote would become the \
                         instructions of every chat on this node. Put it in a folder of its own \
                         outside them (for example /srv/rizzy-harness/AGENTS.md), or leave \
                         user_file out to use AGENTS.md in the node's data directory",
                        p.display()
                    ))
                };
                if !in_data {
                    for r in &self.workspace_roots {
                        let r = expand_home(r, "workspace_roots").unwrap_or_else(|_| r.clone());
                        if real.starts_with(resolve_existing(&r)) {
                            return why(format!("the workspace root {}", r.display()));
                        }
                    }
                    for (key, list) in
                        [("read", &self.sandbox.read), ("write", &self.sandbox.write)]
                    {
                        for r in list {
                            let r = expand_home(r, "[sandbox]").unwrap_or_else(|_| r.clone());
                            if real.starts_with(resolve_existing(&r)) {
                                return why(format!("the [sandbox] {key} folder {}", r.display()));
                            }
                        }
                    }
                    for r in harness_tools::sandbox::SYSTEM_READ {
                        if real.starts_with(r) || real.starts_with(resolve_existing(Path::new(r))) {
                            return why(format!("{r}, which the sandbox lets every command read"));
                        }
                    }
                }
                real
            }
        };
        self.protect(&file);
        self.instructions.located = Some(file);
        Ok(())
    }
}

/// Where the file `p` lies: the deepest existing ancestor of its folder
/// canonical (links and `..` resolved), the components below it and the
/// file's own name appended as they are (a link at the name itself is
/// not followed; the read refuses it). The one
/// resolver of the operator's file (review of PR 120): the start-up
/// refusal checks it, `protect` stores it, `located` is it, and every read
/// resolves again and compares (`instructions::read_user`).
pub fn resolve_file(p: &Path) -> PathBuf {
    match (p.parent(), p.file_name()) {
        (Some(d), Some(n)) => resolve_existing(d).join(n),
        _ => resolve_existing(p),
    }
}

/// [`resolve_file`]'s folders: `p`'s deepest existing ancestor canonical,
/// the rest appended.
pub fn resolve_existing(p: &Path) -> PathBuf {
    let p = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let mut rest: Vec<&std::ffi::OsStr> = Vec::new();
    for a in p.ancestors() {
        if let Ok(real) = a.canonicalize() {
            return rest.iter().rev().fold(real, |acc, c| acc.join(c));
        }
        if let Some(n) = a.file_name() {
            rest.push(n);
        }
    }
    p
}

/// `[projects]` in node.toml (ADOPTION_PLAN X.26, PLAN.md T77): the one
/// folder in which this node makes a new project's folder when the master
/// asks (`project.create`). The master sends a display name only; the node
/// makes the folder's name and never a path the master chose. Only this
/// file sets the folder.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectsPolicy {
    /// Absolute or under `~/`. Left out: [`Self::DEFAULT_DIR`]; `""` turns
    /// making project folders off on this node. It must lie inside a
    /// `workspace_roots` entry (so a session can then run in what is made
    /// there, under the one rule every session folder follows), must not
    /// be, hold or lie inside the node's data directory, its parent folder
    /// must exist, and it must be on the same file system as the data
    /// directory (a new project is built there and moved in). The folder
    /// itself is made on first use with the node user's ordinary
    /// permissions.
    pub dir: Option<PathBuf>,
    /// The folder as checked at start ([`NodePolicy::locate_projects`]):
    /// its parent canonical, its own name appended. `None`: this node makes
    /// no project folders. Never read from node.toml.
    #[serde(skip)]
    pub located: Option<PathBuf>,
    /// What the folder's parent was at start (device and inode), and the
    /// folder's own when it existed then: every creation compares the
    /// descriptors it opens with them. Never read from node.toml.
    #[serde(skip)]
    pub parent_id: Option<harness_core::folder::Identity>,
    #[serde(skip)]
    pub dir_id: Option<harness_core::folder::Identity>,
    /// `<data>/projects-staging`, where a new project's repository is built
    /// before it is moved in (protected: no session ever has it in view).
    /// Emptied at start. Never read from node.toml.
    #[serde(skip)]
    pub staging: Option<PathBuf>,
}

impl ProjectsPolicy {
    /// The folder when node.toml does not name one.
    pub const DEFAULT_DIR: &'static str = "~/rizzy-projects";
    /// The staging folder's name in the node's data directory.
    pub const STAGING: &'static str = "projects-staging";

    /// The folder node.toml asks for, `~/` expanded: `Ok(None)` when it is
    /// turned off (`dir = ""`), the default when left out. A `.` or `..`
    /// component is refused, as for `[instructions] user_file`: the folder
    /// is resolved once at start and judged as such.
    pub fn configured(&self) -> Result<Option<PathBuf>, String> {
        let p = match &self.dir {
            Some(p) if p.as_os_str().is_empty() => return Ok(None),
            Some(p) => p.clone(),
            // The default needs a home folder; a service user without one
            // makes no project folders unless node.toml names one.
            None if dirs::home_dir().is_none() => return Ok(None),
            None => PathBuf::from(Self::DEFAULT_DIR),
        };
        if p.components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(format!(
                "[projects] dir: {} has a `.` or `..` in it; name the folder by its plain path",
                p.display()
            ));
        }
        expand_home(&p, "[projects] dir").map(Some)
    }
}

/// What [`NodePolicy::locate_projects`] found: the folder, the identities
/// it recorded, the staging folder.
struct Located {
    dir: PathBuf,
    parent_id: harness_core::folder::Identity,
    dir_id: Option<harness_core::folder::Identity>,
    staging: PathBuf,
}

impl NodePolicy {
    /// Decide where this node makes new projects' folders (T77) and set
    /// [`ProjectsPolicy::located`], the identities and the staging folder.
    /// Call after [`Self::protect`] of `data_dir`, the node's data
    /// directory. The folder must lie inside a `workspace_roots` entry, must
    /// not be, hold or lie inside a protected folder (the data directory,
    /// the sub-agents' folders, the operator's instruction file), its parent
    /// must exist and it must not be a link, and it must be on the data
    /// directory's file system. One named in node.toml that breaks a rule is
    /// an error (the node does not start); the default that breaks one only
    /// turns the feature off, with the reason returned for the log, so a
    /// node whose roots leave out the home folder starts as before. The
    /// staging folder `<data>/projects-staging` is made (0700), emptied (a
    /// project half-made when the node was killed goes) and protected.
    pub fn locate_projects(&mut self, data_dir: &Path) -> Result<Option<String>, String> {
        self.projects.located = None;
        self.projects.parent_id = None;
        self.projects.dir_id = None;
        self.projects.staging = None;
        let explicit = self.projects.dir.is_some();
        let Some(dir) = self.projects.configured()? else {
            return Ok(None);
        };
        match self.locate_at(&dir, data_dir) {
            Ok(l) => {
                self.protect(&l.staging);
                self.projects.located = Some(l.dir);
                self.projects.parent_id = Some(l.parent_id);
                self.projects.dir_id = l.dir_id;
                self.projects.staging = Some(l.staging);
                Ok(None)
            }
            Err(why) if explicit => Err(format!("[projects] dir = {}: {why}", dir.display())),
            Err(why) => Ok(Some(format!(
                "new project folders are off: the default {} {why}; set [projects] dir in \
                 node.toml to a folder inside workspace_roots, or dir = \"\" to say so",
                ProjectsPolicy::DEFAULT_DIR
            ))),
        }
    }

    fn locate_at(&self, dir: &Path, data_dir: &Path) -> Result<Located, String> {
        use harness_core::folder::Identity;
        use std::os::unix::fs::{DirBuilderExt, MetadataExt};
        let (Some(parent), Some(name)) = (dir.parent(), dir.file_name()) else {
            return Err("names no folder".into());
        };
        let parent = parent.canonicalize().map_err(|e| {
            format!(
                "its parent folder {} cannot be used ({e}); make it first",
                parent.display()
            )
        })?;
        let real = parent.join(name);
        let dir_id = match std::fs::symlink_metadata(&real) {
            Ok(m) if m.is_dir() => Some(Identity {
                dev: m.dev(),
                ino: m.ino(),
            }),
            Ok(_) => return Err("is a link or a file, not a folder".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.to_string()),
        };
        if let Some(why) = self.projects_refusal(&real) {
            return Err(why);
        }
        let parent_id = Identity::of(&parent)?;
        let data = std::path::absolute(data_dir).map_err(|e| e.to_string())?;
        let staging = data.join(ProjectsPolicy::STAGING);
        match std::fs::symlink_metadata(&staging) {
            Ok(m) if m.is_dir() => std::fs::remove_dir_all(&staging),
            Ok(_) => std::fs::remove_file(&staging),
            Err(_) => Ok(()),
        }
        .map_err(|e| format!("cannot empty {}: {e}", staging.display()))?;
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&staging)
            .map_err(|e| format!("cannot make {}: {e}", staging.display()))?;
        let fs_of = |p: &Path| std::fs::metadata(p).map(|m| m.dev()).ok();
        let here = if dir_id.is_some() { &real } else { &parent };
        if fs_of(&staging) != fs_of(here) {
            return Err(format!(
                "is not on the same file system as the node's data directory {}; a new \
                 project is built there and moved in, which needs one file system",
                data.display()
            ));
        }
        // The move in never replaces: the file system must offer that
        // (some network file systems do not).
        let (a, b) = (staging.join("probe-a"), staging.join("probe-b"));
        std::fs::create_dir(&a).map_err(|e| format!("cannot write {}: {e}", staging.display()))?;
        let moved = nix::fcntl::renameat2(
            nix::fcntl::AT_FDCWD,
            &a,
            nix::fcntl::AT_FDCWD,
            &b,
            nix::fcntl::RenameFlags::RENAME_NOREPLACE,
        );
        let _ = std::fs::remove_dir(&a);
        let _ = std::fs::remove_dir(&b);
        if let Err(e) = moved {
            return Err(format!(
                "lies on a file system that cannot move a folder without replacing what is \
                 there ({e}); a new project is moved in that way"
            ));
        }
        Ok(Located {
            dir: real,
            parent_id,
            dir_id,
            staging,
        })
    }

    /// Why `dir` (resolved) cannot hold new projects' folders, or `None`.
    pub(crate) fn projects_refusal(&self, dir: &Path) -> Option<String> {
        for d in &self.protected_dirs {
            let now = d.canonicalize().unwrap_or_else(|_| d.clone());
            for d in [d, &now] {
                if dir.starts_with(d) || d.starts_with(dir) {
                    return Some(format!(
                        "is, holds or lies inside the harness's own data {}; choose a folder \
                         outside it",
                        d.display()
                    ));
                }
            }
        }
        let inside = self
            .workspace_roots
            .iter()
            .filter_map(|r| r.canonicalize().ok())
            .any(|r| dir.starts_with(&r));
        (!inside).then(|| {
            "lies outside the folders this node allows (workspace_roots in node.toml), so no \
             chat could run in a folder made there"
                .to_string()
        })
    }
}

/// `[execution]` in node.toml (N.10a; README "Execution modes"): which
/// execution modes this node may offer, its ceiling. Nothing the master
/// sends can widen it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExecutionPolicy {
    /// The `sandbox` mode (each command in bubblewrap): `"auto"` (default:
    /// offered where the containment probe passes at start, its gaps
    /// named), `"strict"` (offered only where it holds in full) or `"off"`.
    pub sandbox: harness_tools::contain::ContainMode,
    /// The `host` mode: commands as this node's user without bubblewrap.
    /// Off by default; the node's admin turns it on.
    pub host: bool,
}

impl SandboxPolicy {
    /// The settings with `~/` expanded; relative entries are an error.
    /// `contain` is `[execution] sandbox`.
    pub fn settings(
        &self,
        contain: harness_tools::contain::ContainMode,
    ) -> Result<harness_tools::sandbox::Settings, String> {
        let home = dirs::home_dir();
        let expand = |key: &str, list: &[PathBuf]| -> Result<Vec<PathBuf>, String> {
            list.iter()
                .map(|p| {
                    let p = match (p.strip_prefix("~"), &home) {
                        (Ok(rest), Some(h)) => h.join(rest),
                        (Ok(_), None) => {
                            return Err(format!(
                                "[sandbox] {key}: {} uses ~ but there is no home directory",
                                p.display()
                            ));
                        }
                        (Err(_), _) => p.clone(),
                    };
                    if p.is_absolute() {
                        Ok(p)
                    } else {
                        Err(format!(
                            "[sandbox] {key}: {} must be absolute (or start with ~/)",
                            p.display()
                        ))
                    }
                })
                .collect()
        };
        let read_only_tmp = match self.read_only_tmp_mib {
            None => 0,
            Some(m @ 64..=65536) => m * 1024 * 1024,
            Some(m) => {
                return Err(format!(
                    "[sandbox] read_only_tmp_mib = {m}: must be 64 to 65536 (MiB)"
                ));
            }
        };
        Ok(harness_tools::sandbox::Settings {
            read_only_tmp,
            read_only_network: self.read_only_network,
            mode: self.mode,
            network: self.network,
            read: expand("read", &self.read)?,
            write: expand("write", &self.write)?,
            contain,
            bwrap: match &self.bwrap {
                Some(p) if !p.is_absolute() => {
                    return Err(format!(
                        "[sandbox] bwrap: {} must be an absolute path",
                        p.display()
                    ));
                }
                p => p.clone(),
            },
        })
    }
}

/// `[artifacts]` in node.toml: the node's own switch and limits for files
/// published from it (docs/artifacts.md). Nothing the master sends can
/// loosen them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ArtifactsPolicy {
    /// Off: no `publish` tool, no "Save as artifact", no capability.
    pub enabled: bool,
    /// One published file or folder (uncompressed), in MB.
    pub max_mb: u64,
    /// Every staged file together; the oldest go first beyond it.
    pub stage_max_mb: u64,
    /// A stage the master never fetched is deleted after this long.
    pub stage_ttl_hours: u64,
    /// More names never published (globs: `*`, `?`, `**`), on top of the
    /// built-in secret-like names.
    pub deny_globs: Vec<String>,
}

impl Default for ArtifactsPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            max_mb: 200,
            stage_max_mb: 2048,
            stage_ttl_hours: 24,
            deny_globs: Vec::new(),
        }
    }
}

/// `[web]` in node.toml (ADOPTION_PLAN T.3, PLAN.md T89): the same keys as
/// the server's `[web]` in config.toml, with the opposite default: a node
/// has no web tools until its own file says so. A key is named, never
/// written here (`brave_key_env`, or `brave_key_secret` in the node's own
/// 0600 `secrets.toml`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WebPolicy {
    /// `web_search`'s engine: `none` (default), `searxng` or `brave`.
    pub search: harness_core::policy::SearchKind,
    /// The SearXNG address, for `search = "searxng"`.
    pub searxng_url: String,
    /// The environment variable holding the Brave key.
    pub brave_key_env: Option<String>,
    /// The Brave key's name in this node's `secrets.toml`.
    pub brave_key_secret: Option<String>,
    /// `web_fetch`: public addresses only, URLs from the person, a search
    /// or a page it fetched (never a file the agent read).
    pub fetch: bool,
}

impl WebPolicy {
    /// What serde cannot check: an engine without its address or key.
    pub fn validate(&self) -> Result<(), String> {
        use harness_core::policy::SearchKind;
        match self.search {
            SearchKind::Searxng if self.searxng_url.is_empty() => {
                Err("[web] search = \"searxng\" needs searxng_url".into())
            }
            SearchKind::Brave
                if self.brave_key_env.is_none() && self.brave_key_secret.is_none() =>
            {
                Err("[web] search = \"brave\" needs brave_key_env or brave_key_secret".into())
            }
            _ => Ok(()),
        }
    }
}

/// `[forge]` in node.toml (ADOPTION_PLAN X.27 step 3, PLAN.md T86): which
/// forge hosts this node clones a new project's repository from. The node
/// builds the clone's URL itself, `https://<entry>/<owner>/<name>.git`, from
/// an entry here and a repository name it checks by its own rules; nothing
/// the master sends names a scheme, a port, a user or a path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ForgePolicy {
    /// `false`: this node clones nothing (it does not announce
    /// `forge-clone`, and refuses `project.clone`).
    pub clone: bool,
    /// Host names (lower-case DNS names or IPv4 addresses), each optionally
    /// with `:<port>`; exactly these, compared as written.
    pub hosts: Vec<String>,
}

/// The most `[forge] hosts` entries.
pub const MAX_FORGE_HOSTS: usize = 64;

impl Default for ForgePolicy {
    fn default() -> Self {
        Self {
            clone: true,
            hosts: vec!["github.com".into(), "gitlab.com".into()],
        }
    }
}

impl ForgePolicy {
    /// Whether this node clones from anywhere.
    pub fn clones(&self) -> bool {
        self.clone && !self.hosts.is_empty()
    }

    /// Whether `host` is one of the entries, exactly.
    pub fn allows(&self, host: &str) -> bool {
        self.clone && self.hosts.iter().any(|h| h == host)
    }

    fn validate(&self) -> Result<(), String> {
        if self.hosts.len() > MAX_FORGE_HOSTS {
            return Err(format!("[forge] hosts: at most {MAX_FORGE_HOSTS} entries"));
        }
        for (i, h) in self.hosts.iter().enumerate() {
            if !valid_forge_host(h) {
                return Err(format!(
                    "[forge] hosts {h:?}: must be a lower-case host name or IPv4 address, \
                     optionally with :<port>; no scheme, user, path or dot at the end"
                ));
            }
            if self.hosts[..i].contains(h) {
                return Err(format!("[forge] hosts {h:?}: listed twice"));
            }
        }
        Ok(())
    }
}

/// A `[forge] hosts` entry, and the only shape of `project.clone`'s `host`:
/// a lower-case DNS name of letters, digits and inner hyphens (labels of 1
/// to 63, at most 253 in all, at least one letter in the last label) or an
/// IPv4 address, optionally `:<port>` (1 to 65535, no leading zero).
pub fn valid_forge_host(h: &str) -> bool {
    let (name, port) = match h.split_once(':') {
        Some((n, p)) => (n, Some(p)),
        None => (h, None),
    };
    if let Some(p) = port
        && !(!p.is_empty()
            && p.len() <= 5
            && !p.starts_with('0')
            && p.bytes().all(|b| b.is_ascii_digit())
            && p.parse::<u32>().is_ok_and(|n| (1..=65535).contains(&n)))
    {
        return false;
    }
    if name.parse::<std::net::Ipv4Addr>().is_ok() {
        return name.split('.').all(|o| o == "0" || !o.starts_with('0'));
    }
    let labels: Vec<&str> = name.split('.').collect();
    (1..=253).contains(&name.len())
        && labels.iter().all(|l| {
            (1..=63).contains(&l.len())
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
        && labels
            .last()
            .is_some_and(|l| l.bytes().any(|b| b.is_ascii_lowercase()))
}

/// `[limits]` in node.toml.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LimitsPolicy {
    /// Bytes one tool call may stream live (stored as events and sent to
    /// the master); the rest is read and dropped after one marker.
    pub tool_stream_bytes: usize,
    /// Model calls one turn may make. Each sub-agent's turn has the same cap
    /// of its own; the caps do not add up into one budget.
    pub max_steps: usize,
    /// The longest a `bash` call may run, in seconds, whatever the model
    /// asks for.
    pub bash_timeout_max_s: u64,
    /// Seconds a model call may wait for the provider's first answer
    /// (headers, then the first event) before it fails and the retry (at
    /// most once) and fallback rules take over.
    pub model_first_byte_timeout_s: u64,
    /// Seconds the stream may then go without an event.
    pub model_idle_timeout_s: u64,
    /// Sub-agents one turn may start (`delegate` calls run). The master's
    /// agent spec may ask for fewer, never more (Gate 3 review P-2).
    pub max_subagents: u32,
    /// How deep delegation may nest: 1 lets a turn's agent delegate and
    /// its sub-agents not; 0 turns delegation off on this node.
    pub max_delegation_depth: u32,
    /// Where a sub-agent that can change files works (`"shared"`, the
    /// default: in the chat's folder; `"own"`: every one gets a copy of
    /// its own, merged back when it ends, and none starts where the folder
    /// is not a git repository). The node's word: a `delegate` call's
    /// `folder` asks for one of them and `"own"` here wins over it
    /// (PLAN.md T32, the owner's decision of 2026-11-21).
    pub sub_agent_folder: harness_core::subagent::SubFolder,
    /// Background commands one chat may run at once (`bash` with
    /// `background: true`, T.8d); 0 turns them off on this node.
    pub background_per_session: usize,
    /// Background commands this node runs at once, all chats together.
    pub background_per_node: usize,
    /// KiB of output kept in memory per background command (its first and
    /// last part); never written to disk.
    pub background_output_kb: usize,
    /// Sub-agents one chat may run in the background at once (`delegate`
    /// with `background: true`, ADOPTION_PLAN X.24); 0 turns them off on
    /// this node. Counted apart from background commands. The master's
    /// agent spec can only turn them off for an agent, never raise this.
    pub background_agents_per_chat: usize,
    /// The longest a background sub-agent may run, in seconds: at this
    /// wall-clock limit it is stopped and reports what it has.
    pub background_agent_timeout_s: u64,
    /// Callback turns a chat may run in a row without a person's message
    /// (T.8d step 3): turns the harness starts because background commands
    /// ended while the chat was idle. Only a person's message refills it;
    /// 0 turns callback turns off on this node (ends are still recorded
    /// and reach the agent at the person's next message).
    pub callback_turns: usize,
    /// Model calls one callback turn may make; the cap is the lower of this
    /// and `max_steps`.
    pub callback_max_steps: usize,
    /// Hand-off notes (X.16) this node writes per chat per hour: model
    /// calls the master asks for while nobody watches. 0 turns them off on
    /// this node (it does not announce `handoff` and refuses the command).
    /// The bound also stops a master that asks and cancels in a loop: a
    /// cancelled note leaves no event, so the chat stays due.
    pub handoff_notes_per_hour: usize,
    /// New project folders this node makes per hour (`project.create`,
    /// T77, and `project.clone`, T86), all requests together; 0 turns
    /// making them off.
    pub projects_per_hour: usize,
    /// The longest one clone may run, in seconds (`project.clone`, T86):
    /// git is then ended and what it fetched removed.
    pub clone_timeout_s: u64,
    /// The most one clone may put on disk, in MB (its fetched objects and
    /// its checkout together), checked while git runs and once after.
    pub clone_max_mb: u64,
    /// The largest file this node saves into a chat's folder when the
    /// person attaches one the model cannot read as text (T94), in MiB; 0
    /// turns saving uploaded files off on this node. The master follows
    /// the number this node announces.
    pub upload_max_mb: u64,
    /// The most such files one message may bring; 0 turns them off.
    pub uploads_per_message: usize,
}

/// `[limits] upload_max_mb` by default, and the most it may say.
pub const DEFAULT_UPLOAD_MAX_MB: u64 = harness_protocol::DEFAULT_UPLOAD_MAX_MB;
pub const MAX_UPLOAD_MAX_MB: u64 = harness_protocol::MAX_UPLOAD_MAX_MB;
/// `[limits] uploads_per_message` by default, and the most it may say.
pub const DEFAULT_UPLOADS_PER_MESSAGE: usize = harness_protocol::DEFAULT_UPLOADS_PER_MESSAGE;
pub const MAX_UPLOADS_PER_MESSAGE: usize = harness_protocol::MAX_UPLOADS_PER_MESSAGE;

/// `[limits] clone_timeout_s` by default, and its range.
pub const DEFAULT_CLONE_TIMEOUT_S: u64 = 600;
pub const MIN_CLONE_TIMEOUT_S: u64 = 10;
pub const MAX_CLONE_TIMEOUT_S: u64 = harness_protocol::MAX_CLONE_TIMEOUT_S;
/// `[limits] clone_max_mb` by default, and its range.
pub const DEFAULT_CLONE_MAX_MB: u64 = 2048;
pub const MAX_CLONE_MAX_MB: u64 = 1024 * 1024;

/// `max_approval_mode` when node.toml does not say: `auto`, so the owner's
/// default for new chats (`DEFAULT_CHAT_APPROVAL`, 2026-11-14) holds on a
/// fresh node. The node's owner lowers it in node.toml (README "Approval
/// modes"); the node enforces it whatever the master sends.
pub const DEFAULT_MAX_APPROVAL: ApprovalMode = ApprovalMode::Auto;

/// `[limits] callback_turns` by default, and the most it may say.
pub const DEFAULT_CALLBACK_TURNS: usize = 3;
pub const MAX_CALLBACK_TURNS: usize = 20;
/// `[limits] callback_max_steps` by default.
pub const DEFAULT_CALLBACK_MAX_STEPS: usize = 20;
/// `[limits] handoff_notes_per_hour` by default (a note per stretch of
/// work is one; the rest is room for one cancelled by a turn and asked
/// again), and the most it may say.
pub const DEFAULT_HANDOFF_PER_HOUR: usize = 4;
pub const MAX_HANDOFF_PER_HOUR: usize = 60;
/// `[limits] projects_per_hour` by default, and the most it may say.
pub const DEFAULT_PROJECTS_PER_HOUR: usize = 20;
pub const MAX_PROJECTS_PER_HOUR: usize = 200;

/// `[limits] background_agent_timeout_s` by default, and its range: at
/// most an hour, as a background command (the master's running window,
/// `callbacks::RUNNING_WINDOW_MS`, assumes it).
pub const DEFAULT_AGENT_TIMEOUT_S: u64 = 1800;
pub const MIN_AGENT_TIMEOUT_S: u64 = 60;
pub const MAX_AGENT_TIMEOUT_S: u64 = 3600;

/// The most `[limits] max_subagents` and `max_delegation_depth` may say.
pub const MAX_SUBAGENTS: u32 = 32;
pub const MAX_DELEGATION_DEPTH: u32 = 4;

impl Default for LimitsPolicy {
    fn default() -> Self {
        Self {
            tool_stream_bytes: harness_tools::stream::DEFAULT_STREAM_BYTES,
            max_steps: harness_core::policy::DEFAULT_MAX_STEPS,
            bash_timeout_max_s: harness_tools::DEFAULT_TIMEOUT_MAX_S,
            model_first_byte_timeout_s: harness_core::policy::DEFAULT_MODEL_FIRST_BYTE_S,
            model_idle_timeout_s: harness_core::policy::DEFAULT_MODEL_IDLE_S,
            // Today's values: the default caps, and one level (the master
            // resolves a sub-agent's own delegates to none).
            max_subagents: harness_core::agent::DelegateCaps::default().max_subagents,
            max_delegation_depth: 1,
            sub_agent_folder: harness_core::subagent::SubFolder::Shared,
            background_per_session: harness_tools::background::DEFAULT_PER_SESSION,
            background_per_node: harness_tools::background::DEFAULT_PER_NODE,
            background_output_kb: harness_tools::background::DEFAULT_OUTPUT_KB,
            background_agents_per_chat: harness_tools::background::DEFAULT_AGENTS_PER_SESSION,
            background_agent_timeout_s: DEFAULT_AGENT_TIMEOUT_S,
            callback_turns: DEFAULT_CALLBACK_TURNS,
            callback_max_steps: DEFAULT_CALLBACK_MAX_STEPS,
            handoff_notes_per_hour: DEFAULT_HANDOFF_PER_HOUR,
            projects_per_hour: DEFAULT_PROJECTS_PER_HOUR,
            clone_timeout_s: DEFAULT_CLONE_TIMEOUT_S,
            clone_max_mb: DEFAULT_CLONE_MAX_MB,
            upload_max_mb: DEFAULT_UPLOAD_MAX_MB,
            uploads_per_message: DEFAULT_UPLOADS_PER_MESSAGE,
        }
    }
}

impl LimitsPolicy {
    /// The background table's bounds.
    pub fn background(&self) -> harness_tools::background::Limits {
        harness_tools::background::Limits {
            per_session: self.background_per_session,
            per_node: self.background_per_node,
            output_kb: self.background_output_kb,
            agents_per_session: self.background_agents_per_chat,
        }
    }

    /// A background sub-agent's wall-clock limit.
    pub fn agent_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.background_agent_timeout_s)
    }

    /// A callback turn's step cap: `callback_max_steps`, never above
    /// `max_steps`.
    pub fn callback_steps(&self) -> usize {
        self.callback_max_steps.min(self.max_steps)
    }

    /// The master's agent spec within this node's delegation bounds: its
    /// per-turn sub-agent cap no higher than `max_subagents`, at every
    /// level, and no delegates below `max_delegation_depth`. Deny lists
    /// over the node's own caps ([`MAX_DENIED_NAMES`], [`MAX_DENIED_NAME`])
    /// withhold the whole `skill` tool or every MCP tool from that spec
    /// (X.34: fails closed, nothing is cut).
    pub fn bound_delegation(
        &self,
        mut agent: harness_core::agent::AgentSpec,
    ) -> harness_core::agent::AgentSpec {
        self.bound_at(&mut agent, 0);
        agent
    }

    fn bound_at(&self, agent: &mut harness_core::agent::AgentSpec, depth: u32) {
        if depth >= self.max_delegation_depth || self.max_subagents == 0 {
            agent.delegates.clear();
        }
        let mut caps = agent.caps.unwrap_or_default();
        caps.max_subagents = caps.max_subagents.min(self.max_subagents);
        // Background sub-agents (X.24): off on this node at 0, and never
        // below the top: a sub-agent starts none.
        caps.background &= depth == 0 && self.background_agents_per_chat > 0;
        agent.caps = Some(caps);
        let over = |l: &[String]| {
            l.len() > MAX_DENIED_NAMES || l.iter().any(|n| n.len() > MAX_DENIED_NAME)
        };
        for (list_over, tool) in [
            (over(&agent.deny_skills), harness_tools::skills::NAME),
            (over(&agent.deny_mcp_servers), harness_core::agent::MCP_ALL),
        ] {
            if list_over && !agent.deny_tools.iter().any(|t| t == tool) {
                agent.deny_tools.push(tool.into());
            }
        }
        for d in &mut agent.delegates {
            self.bound_at(d, depth + 1);
        }
    }
}

/// Names one spec's `deny_skills` or `deny_mcp_servers` may hold on this
/// node (X.34; the master sends at most 200 Assigned items a workspace).
pub const MAX_DENIED_NAMES: usize = 1000;
/// Bytes of one name in those lists (a skill's is at most 64, a server's
/// 32).
pub const MAX_DENIED_NAME: usize = 128;

impl Default for NodePolicy {
    fn default() -> Self {
        Self {
            max_approval_mode: DEFAULT_MAX_APPROVAL,
            workspace_roots: dirs::home_dir().into_iter().collect(),
            denied_tools: Vec::new(),
            chat_grants: true,
            allow_outside_workspace: false,
            checkpoints_keep: DEFAULT_CHECKPOINTS_KEEP,
            graphify: false,
            artifacts: ArtifactsPolicy::default(),
            env_pass: Vec::new(),
            limits: LimitsPolicy::default(),
            sandbox: SandboxPolicy::default(),
            execution: ExecutionPolicy::default(),
            skills: SkillsPolicy::default(),
            instructions: InstructionsPolicy::default(),
            mcp_servers: Default::default(),
            mcp: McpPolicy::default(),
            projects: ProjectsPolicy::default(),
            forge: ForgePolicy::default(),
            web: None,
            protected_dirs: Vec::new(),
            exec: Default::default(),
            sandbox_tmp: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("cannot read {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("invalid {0}: {1}")]
    Parse(PathBuf, toml::de::Error),
    #[error("invalid {0}: {1}")]
    Invalid(PathBuf, String),
}

impl NodePolicy {
    /// Whether `denied_tools` refuses `tool` outright.
    /// `mcp` stands for every MCP tool and `mcp__<server>` for one
    /// server's (X.3, review N2); any other entry is an exact name.
    ///
    /// `write` also stands for the tools that create Word and Excel files
    /// in a folder ([`harness_core::docs::TOOLS`], T97).
    pub fn denies(&self, tool: &str) -> bool {
        let prefix = harness_tools::mcp::PREFIX;
        self.denied_tools.iter().any(|t| {
            t == tool
                || (t == "write" && harness_core::docs::WRITE_TOOLS.contains(&tool))
                || (tool.starts_with(prefix)
                    && (t == harness_core::agent::MCP_ALL
                        || (t.starts_with(prefix)
                            && !t[prefix.len()..].contains("__")
                            && tool
                                .strip_prefix(t.as_str())
                                .is_some_and(|rest| rest.starts_with("__")))))
        })
    }

    /// What an empty node.toml means: [`Self::default`] with node.toml's
    /// skill roots ([`SkillsPolicy::node_toml`]). The policy of a node with
    /// no node.toml, and of the master's chat runtime.
    pub fn node_toml_default() -> Self {
        Self {
            skills: SkillsPolicy::node_toml(),
            ..Self::default()
        }
    }

    /// Load `node.toml`; a missing file means [`Self::node_toml_default`].
    pub fn load(path: &Path) -> Result<Self, PolicyError> {
        let p: Self = match std::fs::read_to_string(path) {
            Ok(s) => toml::from_str(&s).map_err(|e| PolicyError::Parse(path.into(), e))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::node_toml_default(),
            Err(e) => return Err(PolicyError::Io(path.into(), e)),
        };
        p.validate()
            .map_err(|e| PolicyError::Invalid(path.into(), e))?;
        Ok(p)
    }

    /// Values serde cannot check: `env_pass` globs, `[limits]` ranges.
    pub fn validate(&self) -> Result<(), String> {
        harness_tools::env::compile_pass(&self.env_pass)?;
        self.skills.roots()?;
        self.instructions.user_file()?;
        self.projects.configured()?;
        if let Some(w) = &self.web {
            w.validate()?;
        }
        let n = self.limits.tool_stream_bytes;
        let (min, max) = (
            harness_tools::stream::MIN_STREAM_BYTES,
            harness_tools::stream::MAX_STREAM_BYTES,
        );
        if !(min..=max).contains(&n) {
            return Err(format!(
                "[limits] tool_stream_bytes = {n}: must be between {min} and {max}"
            ));
        }
        let n = self.limits.max_steps;
        let (min, max) = (
            harness_core::policy::MIN_MAX_STEPS,
            harness_core::policy::MAX_MAX_STEPS,
        );
        if !(min..=max).contains(&n) {
            return Err(format!(
                "[limits] max_steps = {n}: must be between {min} and {max}"
            ));
        }
        let n = self.limits.bash_timeout_max_s;
        let (min, max) = (
            harness_tools::MIN_TIMEOUT_MAX_S,
            harness_tools::MAX_TIMEOUT_MAX_S,
        );
        if !(min..=max).contains(&n) {
            return Err(format!(
                "[limits] bash_timeout_max_s = {n}: must be between {min} and {max}"
            ));
        }
        for (key, n) in [
            (
                "model_first_byte_timeout_s",
                self.limits.model_first_byte_timeout_s,
            ),
            ("model_idle_timeout_s", self.limits.model_idle_timeout_s),
        ] {
            let (min, max) = (
                harness_core::policy::MIN_MODEL_IDLE_S,
                harness_core::policy::MAX_MODEL_IDLE_S,
            );
            if !(min..=max).contains(&n) {
                return Err(format!(
                    "[limits] {key} = {n}: must be between {min} and {max}"
                ));
            }
        }
        let n = self.limits.max_subagents;
        if n > MAX_SUBAGENTS {
            return Err(format!(
                "[limits] max_subagents = {n}: must be between 0 and {MAX_SUBAGENTS}"
            ));
        }
        let n = self.limits.max_delegation_depth;
        if n > MAX_DELEGATION_DEPTH {
            return Err(format!(
                "[limits] max_delegation_depth = {n}: must be between 0 and {MAX_DELEGATION_DEPTH}"
            ));
        }
        use harness_tools::background as bg;
        for (key, n, min, max) in [
            (
                "background_per_session",
                self.limits.background_per_session,
                0,
                bg::MAX_PER_SESSION,
            ),
            (
                "background_per_node",
                self.limits.background_per_node,
                0,
                bg::MAX_PER_NODE,
            ),
            (
                "background_output_kb",
                self.limits.background_output_kb,
                bg::MIN_OUTPUT_KB,
                bg::MAX_OUTPUT_KB,
            ),
            (
                "background_agents_per_chat",
                self.limits.background_agents_per_chat,
                0,
                bg::MAX_AGENTS_PER_SESSION,
            ),
            (
                "callback_turns",
                self.limits.callback_turns,
                0,
                MAX_CALLBACK_TURNS,
            ),
            (
                "handoff_notes_per_hour",
                self.limits.handoff_notes_per_hour,
                0,
                MAX_HANDOFF_PER_HOUR,
            ),
            (
                "projects_per_hour",
                self.limits.projects_per_hour,
                0,
                MAX_PROJECTS_PER_HOUR,
            ),
            (
                "uploads_per_message",
                self.limits.uploads_per_message,
                0,
                MAX_UPLOADS_PER_MESSAGE,
            ),
            (
                "callback_max_steps",
                self.limits.callback_max_steps,
                harness_core::policy::MIN_MAX_STEPS,
                harness_core::policy::MAX_MAX_STEPS,
            ),
        ] {
            if !(min..=max).contains(&n) {
                return Err(format!(
                    "[limits] {key} = {n}: must be between {min} and {max}"
                ));
            }
        }
        for (key, n, min, max) in [
            (
                "clone_timeout_s",
                self.limits.clone_timeout_s,
                MIN_CLONE_TIMEOUT_S,
                MAX_CLONE_TIMEOUT_S,
            ),
            (
                "clone_max_mb",
                self.limits.clone_max_mb,
                1,
                MAX_CLONE_MAX_MB,
            ),
            (
                "upload_max_mb",
                self.limits.upload_max_mb,
                0,
                MAX_UPLOAD_MAX_MB,
            ),
        ] {
            if !(min..=max).contains(&n) {
                return Err(format!(
                    "[limits] {key} = {n}: must be between {min} and {max}"
                ));
            }
        }
        self.forge.validate()?;
        let n = self.limits.background_agent_timeout_s;
        if !(MIN_AGENT_TIMEOUT_S..=MAX_AGENT_TIMEOUT_S).contains(&n) {
            return Err(format!(
                "[limits] background_agent_timeout_s = {n}: must be between \
                 {MIN_AGENT_TIMEOUT_S} and {MAX_AGENT_TIMEOUT_S}"
            ));
        }
        self.sandbox_settings()?;
        harness_tools::mcp::validate(&self.mcp_servers)?;
        Ok(())
    }

    /// `[sandbox]` with `[execution] sandbox`, for the sandbox's start.
    pub fn sandbox_settings(&self) -> Result<harness_tools::sandbox::Settings, String> {
        self.sandbox.settings(self.execution.sandbox)
    }

    /// Whether `mode` may run here now (see [`Self::exec`]).
    pub fn check_exec(&self, mode: Option<harness_core::policy::ExecMode>) -> Result<(), String> {
        self.exec.check_session(mode)
    }

    /// The environment tool child processes get here: the core list, plus
    /// `env_pass`, minus `deny` (names that hold secrets).
    pub fn tool_env(
        &self,
        deny: impl IntoIterator<Item = String>,
    ) -> Result<harness_tools::ToolEnv, String> {
        harness_tools::ToolEnv::new(&self.env_pass, deny)
    }

    /// Validate and canonicalize a requested session cwd.
    pub fn check_cwd(&self, cwd: &str) -> Result<PathBuf, String> {
        let p = Path::new(cwd)
            .canonicalize()
            .map_err(|e| format!("cwd {cwd}: {e}"))?;
        if !p.is_dir() {
            return Err(format!("cwd {cwd} is not a directory"));
        }
        self.check_protected(&p)?;
        self.check_roots(&p)?;
        Ok(p)
    }

    /// The workspace root (canonical) that holds `dir` (already canonical),
    /// or a refusal naming `workspace_roots`.
    pub fn root_of(&self, dir: &Path) -> Result<PathBuf, String> {
        self.workspace_roots
            .iter()
            .filter_map(|root| root.canonicalize().ok())
            .filter(|r| dir.starts_with(r))
            // The innermost root: the walk for a repository stops there.
            .max_by_key(|r| r.components().count())
            .ok_or_else(|| {
                format!(
                    "cwd {} is outside the folders this node allows (workspace_roots in \
                     node.toml)",
                    dir.display()
                )
            })
    }

    fn check_roots(&self, dir: &Path) -> Result<(), String> {
        self.root_of(dir).map(|_| ())
    }

    /// What is checked again before every turn, restore, delegation and
    /// publish of an agentic session (`cwd` is its folder as the pin
    /// checked it): the folder does not include a protected one, and it is
    /// still inside `workspace_roots` (node.toml may have been narrowed
    /// since the session was made). The session's mode is checked beside it
    /// ([`Self::check_exec`]).
    pub fn check_session(&self, cwd: &Path) -> Result<(), String> {
        self.check_protected(cwd)?;
        let real = cwd
            .canonicalize()
            .map_err(|e| format!("cwd {}: {e}", cwd.display()))?;
        self.check_roots(&real)
    }

    /// Add `dir` to [`Self::protected_dirs`] (canonicalized; a folder that
    /// does not exist yet is kept as given, made absolute).
    pub fn protect(&mut self, dir: &Path) {
        let d = dir
            .canonicalize()
            .unwrap_or_else(|_| std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf()));
        if !self.protected_dirs.contains(&d) {
            self.protected_dirs.push(d);
        }
    }

    /// Refuse a session folder that is a protected folder, inside one, or
    /// contains one (an ancestor: `$HOME` holds `~/.local/share/...`). The
    /// folder is canonicalized here, so a link or `..` is judged by where it
    /// leads; one that cannot be resolved is refused.
    pub fn check_protected(&self, cwd: &Path) -> Result<(), String> {
        if self.protected_dirs.is_empty() {
            return Ok(());
        }
        let p = cwd
            .canonicalize()
            .map_err(|e| format!("cwd {}: {e}", cwd.display()))?;
        for d in &self.protected_dirs {
            // Compare with where it is now, too: it may have been replaced
            // by a link since start.
            let now = d.canonicalize().unwrap_or_else(|_| d.clone());
            for d in [d, &now] {
                if p.starts_with(d) {
                    return Err(format!(
                        "cwd {} is inside the harness's own data directory {}; choose a \
                         project folder outside it",
                        p.display(),
                        d.display()
                    ));
                }
                if d.starts_with(&p) {
                    return Err(format!(
                        "cwd {} cannot be used: choose a project folder; this one contains the \
                         harness's own data directory {}",
                        p.display(),
                        d.display()
                    ));
                }
            }
        }
        Ok(())
    }

    /// The mode a session gets for `requested` (a create's, a change's, a
    /// load's): [`harness_core::policy::DEFAULT_CHAT_APPROVAL`] without
    /// one, never above [`Self::max_approval_mode`]. Nothing else limits it:
    /// not the chat's kind, not its execution mode (the owner, 2026-11-14).
    pub fn effective_mode(&self, requested: Option<ApprovalMode>) -> ApprovalMode {
        ApprovalMode::clamp(
            requested.unwrap_or(harness_core::policy::DEFAULT_CHAT_APPROVAL),
            self.max_approval_mode,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `checkpoints_keep` is 100 unless the file says otherwise (trial
    /// 2026-11-19, item 6: 10 was gone after about a dozen short turns).
    #[test]
    fn a_node_keeps_the_latest_100_checkpoints_by_default() {
        let keep = |t: &str| toml::from_str::<NodePolicy>(t).unwrap().checkpoints_keep;
        assert_eq!(NodePolicy::default().checkpoints_keep, 100);
        assert_eq!(keep(""), 100);
        assert_eq!(keep("checkpoints_keep = 7"), 7);
        assert_eq!(keep("checkpoints_keep = 0"), 0);
    }

    /// `[sandbox] read_only_tmp_mib` (PLAN.md T70): absent is the default,
    /// a value in range is taken in MiB, one outside is refused at load.
    #[test]
    fn the_read_only_shells_tmp_size_is_the_nodes() {
        let size = |toml_text: &str| {
            toml::from_str::<NodePolicy>(toml_text)
                .unwrap()
                .sandbox_settings()
                .map(|s| s.read_only_tmp)
        };
        assert_eq!(size(""), Ok(0), "0: the default");
        assert_eq!(
            size("[sandbox]\nread_only_tmp_mib = 2048\n"),
            Ok(2048 * 1024 * 1024)
        );
        for bad in [0, 63, 65537] {
            let e = size(&format!("[sandbox]\nread_only_tmp_mib = {bad}\n")).unwrap_err();
            assert!(e.contains("read_only_tmp_mib"), "{e}");
        }
    }

    /// X.2, T62: the operator's file is `AGENTS.md` in the data directory
    /// unless node.toml names one (absolute or under `~/`); a relative
    /// path is refused at load; a policy never located reads no file.
    #[test]
    fn the_operators_instruction_file_defaults_to_the_data_directory() {
        let mut p = NodePolicy {
            workspace_roots: vec!["/home/u".into()],
            ..Default::default()
        };
        assert_eq!(p.instructions.located, None);
        p.locate_instructions(Path::new("/srv/harness")).unwrap();
        assert_eq!(
            p.instructions.located.as_deref(),
            Some(Path::new("/srv/harness/AGENTS.md"))
        );
        // Protected wherever it lies.
        assert!(
            p.protected_dirs
                .contains(&PathBuf::from("/srv/harness/AGENTS.md"))
        );
        let mut p: NodePolicy = toml::from_str(
            "workspace_roots = [\"/home/u\"]\n[instructions]\nuser_file = \"/srv/ops/rules.md\"\n",
        )
        .unwrap();
        p.validate().unwrap();
        p.locate_instructions(Path::new("/srv/harness")).unwrap();
        assert_eq!(
            p.instructions.located.as_deref(),
            Some(Path::new("/srv/ops/rules.md"))
        );
        assert!(
            p.protected_dirs
                .contains(&PathBuf::from("/srv/ops/rules.md"))
        );
        let bad: NodePolicy = toml::from_str("[instructions]\nuser_file = \"rules.md\"\n").unwrap();
        let e = bad.validate().unwrap_err();
        assert!(
            e.contains("[instructions] user_file: rules.md must be absolute"),
            "{e}"
        );
        assert!(toml::from_str::<NodePolicy>("[instructions]\nlocated = \"/x\"\n").is_err());
    }

    /// Review H1: a `user_file` an agent could write (inside a workspace
    /// root, a `[sandbox]` folder) or every command reads (`/etc`) is
    /// refused at start with the reason; one in the data directory is not.
    #[test]
    fn an_operators_file_agents_can_reach_is_refused() {
        let at = |file: &str| {
            let mut p: NodePolicy = toml::from_str(&format!(
                "workspace_roots = [\"/home/u\"]\n[sandbox]\nwrite = [\"/var/cache/x\"]\n\
                 read = [\"/opt2/tools\"]\n[instructions]\nuser_file = \"{file}\"\n"
            ))
            .unwrap();
            p.locate_instructions(Path::new("/home/u/.local/share/h"))
        };
        for (file, why) in [
            ("/home/u/.codex/AGENTS.md", "the workspace root /home/u"),
            ("/home/u/proj/OPS.md", "the workspace root /home/u"),
            (
                "/var/cache/x/a.md",
                "the [sandbox] write folder /var/cache/x",
            ),
            ("/opt2/tools/a.md", "the [sandbox] read folder /opt2/tools"),
            (
                "/etc/rizzy/AGENTS.md",
                "/etc, which the sandbox lets every command read",
            ),
        ] {
            let e = at(file).unwrap_err();
            assert!(e.contains(why), "{file}: {e}");
            assert!(e.contains("every chat on this node"), "{e}");
        }
        at("/home/u/.local/share/h/OPS.md").unwrap();
        at("/srv/ops/AGENTS.md").unwrap();
    }

    /// Review of PR 120 (H1, x1-x3): the path is resolved before it is
    /// checked and protected: a missing folder behind a link into a
    /// workspace root is refused, a missing file behind a linked folder is
    /// protected at its real place, and `.`/`..` are refused at load.
    #[test]
    fn the_operators_file_is_resolved_before_it_is_checked() {
        let d = tempfile::tempdir().unwrap();
        let b = d.path().canonicalize().unwrap();
        for f in ["ws", "out/real", "data"] {
            std::fs::create_dir_all(b.join(f)).unwrap();
        }
        std::os::unix::fs::symlink(b.join("ws"), b.join("out/into-ws")).unwrap();
        std::os::unix::fs::symlink(b.join("out/real"), b.join("out/link")).unwrap();
        let at = |file: &Path| {
            let mut p = NodePolicy {
                workspace_roots: vec![b.join("ws")],
                ..Default::default()
            };
            p.instructions.user_file = Some(file.to_path_buf());
            p.validate()?;
            p.locate_instructions(&b.join("data"))?;
            Ok::<_, String>(p)
        };
        let e = at(&b.join("out/into-ws/sub/AGENTS.md")).unwrap_err();
        assert!(e.contains("inside the workspace root"), "{e}");
        let p = at(&b.join("out/link/AGENTS.md")).unwrap();
        let real = b.join("out/real/AGENTS.md");
        assert_eq!(p.instructions.located.as_deref(), Some(real.as_path()));
        assert!(p.protected_dirs.contains(&real));
        for dots in ["out/real/../real/AGENTS.md", "out/link/../real/AGENTS.md"] {
            let e = at(&b.join(dots)).unwrap_err();
            assert!(e.contains("has a `.` or `..` in it"), "{e}");
        }
    }

    /// Only the node's user and root may own the operator's file.
    #[test]
    fn a_foreign_owner_is_refused() {
        assert_eq!(file_refusal(1000, 0o644, 1000), None);
        assert_eq!(file_refusal(0, 0o644, 1000), None);
        assert!(
            file_refusal(1001, 0o644, 1000)
                .unwrap()
                .contains("another user (uid 1001)")
        );
        // Writable by others: refused for every owner, root included.
        for (uid, mode) in [(1000, 0o664), (1000, 0o646), (0, 0o666), (0, 0o620)] {
            assert!(
                file_refusal(uid, mode, 1000)
                    .unwrap()
                    .contains("chmod go-w"),
                "{uid} {mode:o}"
            );
        }
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("AGENTS.md");
        std::fs::write(&f, "x").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            InstructionsPolicy::warnings(&f).is_empty(),
            "{:?}",
            InstructionsPolicy::warnings(&f)
        );
        std::fs::hard_link(&f, d.path().join("other")).unwrap();
        let w = InstructionsPolicy::warnings(&f);
        assert!(w.len() == 1 && w[0].contains("2 names"), "{w:?}");
        std::fs::remove_file(d.path().join("other")).unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o666)).unwrap();
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        let w = InstructionsPolicy::warnings(&f);
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(w[0].contains("it is not read"), "{w:?}");
    }

    /// `denied_tools = ["write"]` also denies the two tools that write Word
    /// and Excel files into a folder (PLAN.md T97): an operator who refuses
    /// writes has not agreed to them; either can still be denied alone.
    #[test]
    fn denying_write_denies_the_document_tools() {
        let p = |d: &[&str]| NodePolicy {
            denied_tools: d.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        for t in ["doc_create", "sheet_create"] {
            assert!(p(&["write"]).denies(t), "{t}");
            assert!(p(&[t]).denies(t), "{t}");
            assert!(!p(&["edit"]).denies(t), "{t}");
            assert!(!p(&[]).denies(t), "{t}");
        }
        assert!(!p(&["doc_create"]).denies("sheet_create"));
        assert!(!p(&["doc_create"]).denies("write"));
        assert!(!p(&["write"]).denies("attached_file"));
    }

    /// Review N2: `mcp` denies every MCP tool, `mcp__<server>` one
    /// server's; anything else stays an exact name.
    #[test]
    fn denied_tools_can_name_every_mcp_tool_or_one_servers() {
        let p = |d: &[&str]| NodePolicy {
            denied_tools: d.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        assert!(p(&["mcp"]).denies("mcp__github__create_issue"));
        assert!(!p(&["mcp"]).denies("bash"));
        let gh = p(&["mcp__github"]);
        assert!(gh.denies("mcp__github__create_issue"));
        assert!(!gh.denies("mcp__git__status"));
        assert!(!gh.denies("mcp__githubx__a"));
        let one = p(&["mcp__github__drop"]);
        assert!(one.denies("mcp__github__drop") && !one.denies("mcp__github__list"));
        assert!(p(&["bash"]).denies("bash") && !p(&["bash"]).denies("mcp__bash__x"));
    }

    /// Review of PR 147 (L5): deny lists over the node's caps withhold the
    /// whole tool from that spec, a sub-agent's too; within them nothing
    /// changes; nothing is cut.
    #[test]
    fn oversized_deny_lists_withhold_the_whole_tool() {
        let l = LimitsPolicy::default();
        let base: harness_core::agent::AgentSpec = serde_json::from_value(serde_json::json!({
            "agent_id": "a", "name": "A", "system_prompt": "", "tools": "all", "prompt_hash": "h"
        }))
        .unwrap();
        let many: Vec<String> = (0..=MAX_DENIED_NAMES).map(|i| format!("s{i}")).collect();
        let mut top = base.clone();
        top.deny_skills = many.clone();
        let mut child = base.clone();
        child.agent_id = "c".into();
        child.deny_mcp_servers = vec!["x".repeat(MAX_DENIED_NAME + 1)];
        top.delegates = vec![child];
        let got = l.bound_delegation(top);
        assert!(!got.allows_tool("skill") && got.allows_mcp());
        assert_eq!(got.deny_skills.len(), many.len(), "nothing cut");
        let c = &got.delegates[0];
        assert!(!c.allows_mcp() && c.allows_tool("skill"));
        let mut fine = base.clone();
        fine.deny_skills = (0..MAX_DENIED_NAMES).map(|i| format!("s{i}")).collect();
        fine.deny_mcp_servers = vec!["x".repeat(MAX_DENIED_NAME)];
        let got = l.bound_delegation(fine);
        assert!(got.allows_tool("skill") && got.allows_mcp() && got.deny_tools.is_empty());
    }

    #[test]
    fn mcp_servers_are_read_from_node_toml_and_checked() {
        let p: NodePolicy = toml::from_str(
            r#"
[mcp_servers.github]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
env = { GITHUB_PERSONAL_ACCESS_TOKEN = "ghp-test-123456" }
env_vars = ["SSL_CERT_FILE"]
disabled_tools = ["delete_repository"]
tool_timeout_sec = 120
"#,
        )
        .unwrap();
        p.validate().unwrap();
        let g = &p.mcp_servers["github"];
        assert_eq!(g.command, "npx");
        assert_eq!(
            g.startup_timeout_sec,
            harness_tools::mcp::DEFAULT_STARTUP_TIMEOUT_S
        );
        assert_eq!(g.tool_timeout_sec, 120);
        assert!(!format!("{p:?}").contains("ghp-test"));
        for bad in [
            "[mcp_servers.Bad]\ncommand = \"x\"\n",
            "[mcp_servers.a__b]\ncommand = \"x\"\n",
            "[mcp_servers.a]\ncommand = \"\"\n",
            "[mcp_servers.a]\ncommand = \"x\"\nenv = { HARNESS_GUARD_STDIN_FIFO = \"/tmp/f\" }\n",
            "[mcp_servers.a]\ncommand = \"x\"\nstartup_timeout_sec = 0\n",
        ] {
            let p: NodePolicy = toml::from_str(bad).unwrap();
            assert!(p.validate().is_err(), "{bad}");
        }
        // Only stdio servers: a URL is not a field.
        assert!(toml::from_str::<NodePolicy>("[mcp_servers.a]\nurl = \"https://x\"\n").is_err());
    }

    /// Gate 3 review P-2: node.toml bounds what the master's spec asks for
    /// delegation, at every level; it never raises it.
    #[test]
    fn delegation_is_bounded_by_the_node_at_every_level() {
        use harness_core::agent::{AgentSpec, DelegateCaps};
        let spec = |id: &str, delegates: Vec<AgentSpec>, caps: Option<DelegateCaps>| AgentSpec {
            agent_id: id.into(),
            name: id.into(),
            system_prompt: String::new(),
            tools: harness_core::agent::ToolSelection::List(vec![]),
            model: None,
            fallbacks: vec![],
            unavailable: vec![],
            no_effort: vec![],
            vision: vec![],
            context_windows: Default::default(),
            effort: None,
            ui: Default::default(),
            no_personalisation: false,
            prompt_hash: "h".into(),
            deny_tools: vec![],
            deny_skills: vec![],
            deny_mcp_servers: vec![],
            stored_skills: vec![],
            connector_tools: vec![],
            delegates,
            caps,
        };
        let caps = |n| {
            Some(DelegateCaps {
                background: false,
                max_subagents: n,
                max_tokens: 1,
            })
        };
        // A spec nested three deep, asking for 10 per turn.
        let deep = spec(
            "a",
            vec![spec("b", vec![spec("c", vec![], caps(10))], caps(10))],
            caps(10),
        );
        let l = LimitsPolicy::default();
        assert_eq!((l.max_subagents, l.max_delegation_depth), (4, 1));
        let got = l.bound_delegation(deep.clone());
        assert_eq!(got.caps.unwrap().max_subagents, 4);
        assert_eq!(got.delegates.len(), 1);
        assert!(got.delegates[0].delegates.is_empty(), "one level only");
        // Fewer than the node allows stays as asked.
        assert_eq!(
            l.bound_delegation(spec("a", vec![], caps(2)))
                .caps
                .unwrap()
                .max_subagents,
            2
        );
        let two = LimitsPolicy {
            max_delegation_depth: 2,
            ..LimitsPolicy::default()
        };
        let got = two.bound_delegation(deep.clone());
        assert_eq!(got.delegates[0].delegates.len(), 1);
        assert!(got.delegates[0].delegates[0].delegates.is_empty());
        assert_eq!(got.delegates[0].caps.unwrap().max_subagents, 4);
        for off in [
            LimitsPolicy {
                max_delegation_depth: 0,
                ..LimitsPolicy::default()
            },
            LimitsPolicy {
                max_subagents: 0,
                ..LimitsPolicy::default()
            },
        ] {
            assert!(off.bound_delegation(deep.clone()).delegates.is_empty());
        }
        let mut p = NodePolicy::default();
        p.limits.max_subagents = MAX_SUBAGENTS + 1;
        assert!(p.validate().unwrap_err().contains("max_subagents"));
        p.limits.max_subagents = 0;
        p.limits.max_delegation_depth = MAX_DELEGATION_DEPTH + 1;
        assert!(p.validate().unwrap_err().contains("max_delegation_depth"));
    }

    /// A policy built in code reads no skill folder of the home (every test
    /// rig builds one: a developer's `~/.agents/skills` changed what tests
    /// offered the agent, 2026-11-15); node.toml's default still names the
    /// owner's two home roots, whether `[skills]` is missing, says nothing of
    /// `roots`, or there is no node.toml at all.
    #[test]
    fn code_built_policies_have_no_home_skill_roots_and_node_toml_keeps_them() {
        assert!(NodePolicy::default().skills.roots.is_empty());
        assert!(NodePolicy::default().skills.repository);
        let home = vec![
            PathBuf::from("~/.agents/skills"),
            PathBuf::from("~/.claude/skills"),
        ];
        let empty: NodePolicy = toml::from_str("").unwrap();
        assert_eq!(empty.skills.roots, home);
        assert!(empty.skills.repository);
        let partial: NodePolicy = toml::from_str("[skills]\nrepository = false\n").unwrap();
        assert_eq!(partial.skills.roots, home);
        assert!(!partial.skills.repository);
        let none: NodePolicy = toml::from_str("[skills]\nroots = []\n").unwrap();
        assert!(none.skills.roots.is_empty());
        let dir = tempfile::tempdir().unwrap();
        let missing = NodePolicy::load(&dir.path().join("node.toml")).unwrap();
        assert_eq!(missing.skills.roots, home);
        assert_eq!(NodePolicy::node_toml_default().skills, empty.skills);
    }

    /// T.8d: the background bounds are node.toml's, with the owner's
    /// defaults (8 a chat, 64 a node, 256 KiB each; on since step 2 gave
    /// the person the strip and its stop), and checked; 0 turns it off.
    #[test]
    fn background_limits_default_and_range() {
        let l = NodePolicy::default().limits.background();
        assert_eq!((l.per_session, l.per_node, l.output_kb), (8, 64, 256));
        let p: NodePolicy = toml::from_str(
            "[limits]\nbackground_per_session = 0\nbackground_per_node = 3\nbackground_output_kb = 16\n",
        )
        .unwrap();
        assert!(p.validate().is_ok());
        assert_eq!(p.limits.background().per_session, 0);
        for bad in [
            "background_per_session = 33",
            "background_per_node = 257",
            "background_output_kb = 15",
            "background_output_kb = 4097",
        ] {
            let p: NodePolicy = toml::from_str(&format!("[limits]\n{bad}\n")).unwrap();
            let e = p.validate().unwrap_err();
            assert!(e.contains(bad.split(' ').next().unwrap()), "{bad}: {e}");
        }
    }

    /// `[limits] sub_agent_folder` (the owner, 2026-11-21): `shared` unless
    /// the node says `own`; any other word is refused when node.toml is
    /// read, never taken as one of the two.
    #[test]
    fn sub_agent_folder_defaults_to_shared_and_takes_two_words() {
        use harness_core::subagent::SubFolder;
        assert_eq!(
            NodePolicy::default().limits.sub_agent_folder,
            SubFolder::Shared
        );
        let own: NodePolicy = toml::from_str("[limits]\nsub_agent_folder = \"own\"\n").unwrap();
        assert_eq!(own.limits.sub_agent_folder, SubFolder::Own);
        assert!(own.validate().is_ok());
        for bad in ["\"both\"", "\"Own\"", "true", "\"\""] {
            let e = toml::from_str::<NodePolicy>(&format!("[limits]\nsub_agent_folder = {bad}\n"))
                .unwrap_err()
                .to_string();
            assert!(
                e.contains("sub_agent_folder") || e.contains("variant"),
                "{bad}: {e}"
            );
        }
    }

    /// X.24: background sub-agents are node.toml's, 4 a chat and 30 min
    /// each by default, checked; 0 turns them off, and the agent spec's
    /// switch is cleared then, and below the top always (depth one): the
    /// master's spec cannot turn them on where the node says no.
    #[test]
    fn background_agents_limits_and_the_specs_switch() {
        let d = NodePolicy::default().limits;
        assert_eq!(
            (d.background_agents_per_chat, d.background_agent_timeout_s),
            (4, 1800)
        );
        assert_eq!(d.background().agents_per_session, 4);
        for bad in [
            "background_agents_per_chat = 17",
            "background_agent_timeout_s = 59",
            "background_agent_timeout_s = 3601",
        ] {
            let p: NodePolicy = toml::from_str(&format!("[limits]\n{bad}\n")).unwrap();
            let e = p.validate().unwrap_err();
            assert!(e.contains(bad.split(' ').next().unwrap()), "{bad}: {e}");
        }
        let caps = Some(harness_core::agent::DelegateCaps {
            background: true,
            ..Default::default()
        });
        let mut child = harness_core::agent::AgentSpec {
            agent_id: "c".into(),
            name: "c".into(),
            system_prompt: "p".into(),
            tools: harness_core::agent::ToolSelection::List(vec![]),
            model: None,
            fallbacks: vec![],
            unavailable: vec![],
            no_effort: vec![],
            vision: vec![],
            context_windows: Default::default(),
            effort: None,
            ui: Default::default(),
            no_personalisation: false,
            prompt_hash: "h".into(),
            deny_tools: vec![],
            deny_skills: vec![],
            deny_mcp_servers: vec![],
            stored_skills: vec![],
            connector_tools: vec![],
            delegates: vec![],
            caps,
        };
        let mut top = child.clone();
        child.caps = caps;
        top.delegates = vec![child];
        let mut limits = LimitsPolicy {
            max_delegation_depth: 2,
            ..Default::default()
        };
        let b = limits.bound_delegation(top.clone());
        assert!(b.caps.unwrap().background, "the top keeps its switch");
        assert!(
            !b.delegates[0].caps.unwrap().background,
            "never below the top"
        );
        limits.background_agents_per_chat = 0;
        let b = limits.bound_delegation(top);
        assert!(!b.caps.unwrap().background, "0 turns it off");
    }

    /// T.8d step 3: the callback bounds are node.toml's, 3 turns of 20
    /// steps by default, checked; 0 turns callbacks off; a callback turn's
    /// cap is never above `max_steps`.
    #[test]
    fn callback_limits_default_and_range() {
        let d = NodePolicy::default().limits;
        assert_eq!((d.callback_turns, d.callback_max_steps), (3, 20));
        assert_eq!(d.callback_steps(), 20);
        let p: NodePolicy = toml::from_str(
            "[limits]\ncallback_turns = 0\ncallback_max_steps = 70\nmax_steps = 30\n",
        )
        .unwrap();
        assert!(p.validate().is_ok());
        assert_eq!(p.limits.callback_turns, 0);
        assert_eq!(p.limits.callback_steps(), 30, "never above max_steps");
        for bad in [
            "callback_turns = 21",
            "callback_max_steps = 0",
            "callback_max_steps = 10001",
        ] {
            let p: NodePolicy = toml::from_str(&format!("[limits]\n{bad}\n")).unwrap();
            let e = p.validate().unwrap_err();
            assert!(e.contains(bad.split(' ').next().unwrap()), "{bad}: {e}");
        }
    }

    /// The node's ceiling is enforced whatever the master asks; a fresh
    /// node allows `auto` and a new chat starts in it (the owner,
    /// 2026-11-14).
    #[test]
    fn master_cannot_loosen() {
        let p = NodePolicy::default();
        assert_eq!(p.max_approval_mode, ApprovalMode::Auto);
        assert_eq!(p.effective_mode(None), ApprovalMode::Auto);
        let strict = NodePolicy {
            max_approval_mode: ApprovalMode::Ask,
            ..Default::default()
        };
        assert_eq!(
            strict.effective_mode(Some(ApprovalMode::Auto)),
            ApprovalMode::Ask
        );
        assert_eq!(strict.effective_mode(None), ApprovalMode::Ask);
        assert_eq!(
            strict.effective_mode(Some(ApprovalMode::AskAll)),
            ApprovalMode::AskAll
        );
    }

    #[test]
    fn cwd_confinement() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("proj")).unwrap();
        let p = NodePolicy {
            workspace_roots: vec![root.path().into()],
            ..Default::default()
        };
        assert!(
            p.check_cwd(root.path().join("proj").to_str().unwrap())
                .is_ok()
        );
        assert!(p.check_cwd(other.path().to_str().unwrap()).is_err());
        assert!(p.check_cwd("/definitely/missing").is_err());
    }

    /// The data directory holds the API token, the session keys and every
    /// user's chats: no session may include it (AUDIT S1).
    #[test]
    fn cwd_never_includes_the_data_dir() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path().canonicalize().unwrap();
        let local = home.join(".local");
        let data = local.join("share/rizzy-harness");
        std::fs::create_dir_all(data.join("keys")).unwrap();
        std::fs::create_dir_all(home.join("projects/x")).unwrap();
        let mut p = NodePolicy {
            workspace_roots: vec![home.clone()],
            ..Default::default()
        };
        p.protect(&data);
        let s = |p: &Path| p.to_str().unwrap().to_string();
        for bad in [&home, &local, &data, &data.join("keys")] {
            let e = p.check_cwd(&s(bad)).unwrap_err();
            assert!(e.contains("choose a project folder"), "{e}");
            assert!(e.contains(&s(&data)), "{e}");
        }
        assert!(p.check_cwd(&s(&home.join("projects/x"))).is_ok());
        assert!(p.check_cwd(&s(&home.join("projects"))).is_ok());
        // A link that leads into it is judged by where it leads.
        std::os::unix::fs::symlink(&data, home.join("projects/data")).unwrap();
        assert!(p.check_cwd(&s(&home.join("projects/data"))).is_err());
        std::os::unix::fs::symlink(&home, home.join("projects/up")).unwrap();
        assert!(p.check_cwd(&s(&home.join("projects/up"))).is_err());
        // A `..` path is no way around it either.
        assert!(p.check_cwd(&s(&home.join("projects/x/../.."))).is_err());
        // Nothing in node.toml can set or clear the list.
        assert!(toml::from_str::<NodePolicy>("protected_dirs = []\n").is_err());
    }

    #[test]
    fn sandbox_settings_parse_and_validate() {
        use harness_tools::sandbox::{Mode, Network};
        let d = NodePolicy::default();
        assert_eq!(d.sandbox.mode, Mode::Required, "fail closed by default");
        assert_eq!(d.sandbox.network, Network::On, "Q6: on by default");
        let p: NodePolicy = toml::from_str(
            "[sandbox]\nmode = \"best-effort\"\nnetwork = \"off\"\n\
             read = [\"~/.cargo\", \"/opt/x\"]\nwrite = [\"~/.cache\"]\n",
        )
        .unwrap();
        assert_eq!(p.sandbox.mode, Mode::BestEffort);
        assert_eq!(p.sandbox.network, Network::Off);
        let s = p.sandbox_settings().unwrap();
        let home = dirs::home_dir().unwrap();
        assert_eq!(s.read, [home.join(".cargo"), PathBuf::from("/opt/x")]);
        assert_eq!(s.write, [home.join(".cache")]);
        for bad in [
            "[sandbox]\nmode = \"maybe\"\n",
            "[sandbox]\nnetwork = \"udp\"\n",
            "[sandbox]\nreads = []\n",
            "sandbox = \"off\"\n",
            // Moved to `[execution] sandbox` (N.10a); no old spelling.
            "[sandbox]\ncontain = \"auto\"\n",
            "[execution]\nsandbox = \"required\"\n",
            "[execution]\nhost = \"yes\"\n",
            "[execution]\ncontainers = true\n",
        ] {
            assert!(toml::from_str::<NodePolicy>(bad).is_err(), "{bad}");
        }
        let p: NodePolicy = toml::from_str("[sandbox]\nread = [\"relative/dir\"]\n").unwrap();
        assert!(p.validate().unwrap_err().contains("absolute"));
        // The runtime-only fields cannot come from node.toml.
        assert!(toml::from_str::<NodePolicy>("exec = \"x\"\n").is_err());
        assert!(toml::from_str::<NodePolicy>("sandbox_tmp = \"/x\"\n").is_err());
    }

    /// `[execution]` (N.10a): `host` off and `sandbox = "auto"` by default;
    /// both can be set; `[execution] sandbox` reaches the sandbox's start.
    #[test]
    fn the_execution_table_is_the_ceiling_and_host_is_off_by_default() {
        use harness_tools::contain::ContainMode;
        let d = NodePolicy::default();
        assert!(!d.execution.host, "the host mode is off unless turned on");
        assert_eq!(d.execution.sandbox, ContainMode::Auto);
        assert!(!d.exec.offers(harness_core::policy::ExecMode::Host));
        let p: NodePolicy =
            toml::from_str("[execution]\nsandbox = \"strict\"\nhost = true\n").unwrap();
        assert!(p.execution.host);
        assert_eq!(p.sandbox_settings().unwrap().contain, ContainMode::Strict);
        let p: NodePolicy = toml::from_str("[execution]\nsandbox = \"off\"\n").unwrap();
        assert_eq!(p.sandbox_settings().unwrap().contain, ContainMode::Off);
    }

    /// A node that offers no mode refuses every agentic session's work
    /// (N.10a): what `ExecOffers` says, through the policy.
    #[test]
    fn a_node_offering_nothing_refuses_every_session() {
        use harness_core::policy::ExecMode;
        let mut p = NodePolicy::default();
        assert!(p.check_exec(Some(ExecMode::Sandbox)).is_ok());
        p.exec = crate::exec::ExecOffers::new(
            Err("set [sandbox] mode in node.toml".into()),
            Err("set [sandbox] mode in node.toml".into()),
            vec![],
        );
        let e = p.check_exec(Some(ExecMode::Sandbox)).unwrap_err();
        assert!(e.contains("[sandbox] mode"), "{e}");
        assert!(p.exec.choose(None).unwrap_err().contains("[sandbox] mode"));
    }

    #[test]
    fn parses_toml_and_rejects_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("node.toml");
        std::fs::write(
            &f,
            "max_approval_mode = \"auto\"\ndenied_tools = [\"bash\"]\n",
        )
        .unwrap();
        let p = NodePolicy::load(&f).unwrap();
        assert_eq!(p.max_approval_mode, ApprovalMode::Auto);
        assert_eq!(p.denied_tools, vec!["bash".to_string()]);
        assert!(p.denies("bash") && !p.denies("bas") && !p.denies("read"));
        std::fs::write(&f, "max_aproval_mode = \"auto\"\n").unwrap();
        assert!(NodePolicy::load(&f).is_err());
        std::fs::write(&f, "[artifacts]\nmax_mb = 5\ndeny_globs = [\"*.sqlite\"]\n").unwrap();
        let p = NodePolicy::load(&f).unwrap();
        assert!(p.artifacts.enabled, "unset keys keep their defaults");
        assert_eq!(p.artifacts.max_mb, 5);
        assert_eq!(p.artifacts.stage_ttl_hours, 24);
        std::fs::write(&f, "[artifacts]\nmax_megabytes = 5\n").unwrap();
        assert!(NodePolicy::load(&f).is_err());
        assert!(NodePolicy::load(&dir.path().join("missing.toml")).is_ok());
    }

    #[test]
    fn env_pass_and_limits_are_validated_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("node.toml");
        let d = NodePolicy::load(&f).unwrap();
        assert!(d.env_pass.is_empty(), "nothing extra passes by default");
        assert_eq!(d.limits.tool_stream_bytes, 1 << 20);
        std::fs::write(
            &f,
            "env_pass = [\"CARGO_*\", \"NVM_DIR\"]\n[limits]\ntool_stream_bytes = 65536\n",
        )
        .unwrap();
        let p = NodePolicy::load(&f).unwrap();
        assert_eq!(p.env_pass, ["CARGO_*", "NVM_DIR"]);
        assert_eq!(p.limits.tool_stream_bytes, 65536);
        for bad in [
            "env_pass = [\"CARGO_[\"]\n",
            "env_pass = [\"A=B\"]\n",
            "env_pass = [\"\"]\n",
            "[limits]\ntool_stream_bytes = 10\n",
            "[limits]\nmax_steps = 0\n",
            "[limits]\nmax_steps = 10001\n",
            "[limits]\nbash_timeout_max_s = 0\n",
            "[limits]\nbash_timeout_max_s = 3601\n",
            "[limits]\ntool_bytes = 65536\n",
        ] {
            std::fs::write(&f, bad).unwrap();
            let e = NodePolicy::load(&f).unwrap_err().to_string();
            assert!(e.contains("node.toml"), "{bad}: {e}");
        }
        std::fs::write(&f, "env_pass = [\"CARGO_[\"]\n").unwrap();
        let e = NodePolicy::load(&f).unwrap_err().to_string();
        assert!(e.contains("env_pass") && e.contains("CARGO_["), "{e}");
    }

    /// The model stream's timeouts: 600 s for the first answer and 300 s
    /// between events unless node.toml says, and a value outside the range
    /// stops the node by name.
    #[test]
    fn the_model_stream_timeouts_default_and_are_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("node.toml");
        let d = NodePolicy::load(&f).unwrap().limits;
        assert_eq!(
            (d.model_first_byte_timeout_s, d.model_idle_timeout_s),
            (600, 300)
        );
        std::fs::write(
            &f,
            "[limits]\nmodel_idle_timeout_s = 900\nmodel_first_byte_timeout_s = 60\n",
        )
        .unwrap();
        let p = NodePolicy::load(&f).unwrap().limits;
        assert_eq!(
            (p.model_first_byte_timeout_s, p.model_idle_timeout_s),
            (60, 900)
        );
        for bad in [
            "model_idle_timeout_s = 9",
            "model_idle_timeout_s = 3601",
            "model_first_byte_timeout_s = 9",
            "model_first_byte_timeout_s = 3601",
        ] {
            std::fs::write(&f, format!("[limits]\n{bad}\n")).unwrap();
            let e = NodePolicy::load(&f).unwrap_err().to_string();
            let key = bad.split(' ').next().unwrap();
            assert!(e.contains(key) && e.contains("10 and 3600"), "{bad}: {e}");
        }
    }

    #[test]
    fn loop_limits_default_to_todays_values_and_stop_the_node_out_of_range() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("node.toml");
        let d = NodePolicy::load(&f).unwrap();
        // The owner's 1,000 (2026-11-15).
        assert_eq!(d.limits.max_steps, 1000);
        assert_eq!(d.limits.bash_timeout_max_s, 3600);
        std::fs::write(&f, "[limits]\nmax_steps = 2\nbash_timeout_max_s = 1\n").unwrap();
        let p = NodePolicy::load(&f).unwrap();
        assert_eq!((p.limits.max_steps, p.limits.bash_timeout_max_s), (2, 1));
        std::fs::write(
            &f,
            "[limits]\nmax_steps = 10000\nbash_timeout_max_s = 3600\n",
        )
        .unwrap();
        assert!(NodePolicy::load(&f).is_ok(), "the range ends are valid");
        for (bad, key, range) in [
            ("max_steps = 0", "max_steps", "1 and 10000"),
            ("max_steps = 10001", "max_steps", "1 and 10000"),
            ("bash_timeout_max_s = 0", "bash_timeout_max_s", "1 and 3600"),
            (
                "bash_timeout_max_s = 3601",
                "bash_timeout_max_s",
                "1 and 3600",
            ),
        ] {
            std::fs::write(&f, format!("[limits]\n{bad}\n")).unwrap();
            let e = NodePolicy::load(&f).unwrap_err().to_string();
            assert!(e.contains(key) && e.contains(range), "{bad}: {e}");
        }
    }
}
