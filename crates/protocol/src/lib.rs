//! Master <-> node messages. PLAN.md §3. JSON, `{"type": "..."}`-tagged.
//! Changes must be additive within a major `PROTOCOL_VERSION`. Before the
//! first tagged release that is not yet held to (no compatibility code):
//! `node.plugins` (X.34) is the first message a node sends unasked that an
//! older master does not know, and such a master drops the link on it as
//! a bad frame; master and nodes are updated together until the tag.

use harness_core::agent::AgentSpec;
use harness_core::event::Profile;
use harness_core::policy::{ApprovalMode, Decision};
use harness_core::{RawEvent, SessionId};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

/// The largest WebSocket frame (and message) either end of a remote link
/// accepts. A bigger one closes the link (checked from the frame header,
/// before the payload is read).
pub const MAX_FRAME: usize = 8 * 1024 * 1024;

/// The most file bytes one `artifact.chunk` carries (the node clamps a
/// larger `len`); with four in flight a transfer stays far below the link's
/// frame limit.
pub const MAX_CHUNK: u32 = 256 * 1024;

/// The `ack.error` of a node that is stopping (SIGTERM or SIGINT, A.19) for
/// a command that would start new work: a session, a turn, a restore, a
/// staged file. Exactly this text, so the master can tell it from a refusal
/// of the command itself: it is answered as a node that is away (503, a
/// routine retries), not as a bad request. A node that sends it falsely
/// gains nothing it could not get by not answering.
pub const NODE_STOPPING: &str = "this node is stopping (its owner or its service manager stopped \
                                 it): send this again once it is back";

/// The node capability that announces the `artifact.*` messages. The master
/// sends them only to nodes whose hello listed it (an unknown message drops
/// the link).
pub const CAP_ARTIFACTS: &str = "artifacts";

/// The node capability that announces `session.prompt.images`. The master
/// sends images only to nodes whose hello listed it.
pub const CAP_IMAGES: &str = "images";

/// The node capability that announces `session.stop_background` (T.8d,
/// docs/background-commands.md). The master sends it only to nodes whose
/// hello listed it.
pub const CAP_BACKGROUND: &str = "background";

/// The node capability that announces `session.callback` (T.8d step 3).
/// The master resumes chats only on nodes whose hello listed it.
pub const CAP_CALLBACK: &str = "callback";

/// The node capability that announces `session.handoff` (X.16): the
/// master asks for hand-off notes only on nodes whose hello listed it.
pub const CAP_HANDOFF: &str = "handoff";

/// The node capability that announces `session.compact` (X.20): a person's
/// `/compact`, summarising the chat's history now.
pub const CAP_COMPACT: &str = "compact";

/// The node capability that announces `session.retry` (X.21): a person's
/// Retry of the chat's failed last turn. The master offers Retry only for
/// chats on nodes whose hello listed it.
pub const CAP_RETRY: &str = "retry";

/// The node capability that announces `session.rewind` and
/// `session.rewind_undo` (X.46): a person's rewind of the conversation to
/// an earlier message, and putting it back. The master offers them only for
/// chats on nodes whose hello listed it.
pub const CAP_REWIND: &str = "rewind";

/// The node capability that announces `session.pin` (docs/pins.md): the
/// owner's bookmark on a message. The master offers it only for chats on
/// nodes whose hello listed it; an older node is told to update.
pub const CAP_PIN: &str = "pin";

/// The node capability that announces `session.fork` (ADOPTION_PLAN C.2,
/// docs/fork.md): a person's fork of a chat at the end of a completed turn.
/// The master offers it only for chats on nodes whose hello listed it.
pub const CAP_FORK: &str = "fork";

/// The node capability that announces `session.diff` (X.12, PLAN.md T53).
/// The master asks for a turn's changes only on nodes whose hello listed it.
pub const CAP_DIFF: &str = "diff";

/// The node capability that announces `@` file mentions (PLAN.md T63):
/// `session.prompt.files` and `session.files`. The master sends neither to
/// a node whose hello did not list it.
pub const CAP_FILES: &str = "files";

/// The node capability that announces `project.create` (ADOPTION_PLAN
/// X.26, PLAN.md T77): its node.toml names a projects folder in which it
/// makes a new project's folder on request. Never the folder's path: the
/// master learns a path only from a `project.created` answer.
pub const CAP_PROJECTS: &str = "projects";

/// The longest display name a `project.create` may carry, in bytes; a node
/// refuses a longer one (the master's project names are far shorter).
pub const MAX_PROJECT_NAME: usize = 256;

/// The node capability that announces `project.clone` (ADOPTION_PLAN X.27
/// step 3, PLAN.md T86), followed by the node's clone time limit in seconds
/// (`[limits] clone_timeout_s`): `forge-clone:600`. Announced only where
/// node.toml lets the node clone at all (`[forge] clone`, a host listed in
/// `[forge] hosts`, a usable projects folder); never the hosts themselves.
/// The master waits for the answer longer than that limit.
pub const CAP_FORGE_CLONE_PREFIX: &str = "forge-clone:";

/// The clone time limit a node announces in [`CAP_FORGE_CLONE_PREFIX`], if
/// it announces one: `None` for a node that does not clone.
/// A value over [`MAX_CLONE_TIMEOUT_S`] counts as that maximum, so a node
/// cannot hold a request on the master longer than any node may be set to.
pub fn clone_timeout_of(capabilities: &[String]) -> Option<u64> {
    capabilities.iter().find_map(|c| {
        let n = c.strip_prefix(CAP_FORGE_CLONE_PREFIX)?;
        (!n.is_empty() && n.len() <= 6 && n.bytes().all(|b| b.is_ascii_digit()))
            .then(|| n.parse::<u64>().ok())
            .flatten()
            .map(|n| n.min(MAX_CLONE_TIMEOUT_S))
    })
}

/// The longest `[limits] clone_timeout_s` a node may be set to, in seconds.
pub const MAX_CLONE_TIMEOUT_S: u64 = 7200;

/// The longest `full_name` a `project.clone` may carry, in bytes.
pub const MAX_CLONE_NAME: usize = 512;
/// The longest token a `project.clone` may carry, in bytes (the master's
/// forge tokens are at most 512).
pub const MAX_CLONE_TOKEN: usize = 512;

/// A forge token on the wire (`project.clone`): serialized as its plain
/// text, since the link is the one place it must travel; `Debug` and
/// `Display` stay redacted ([`harness_core::Secret`]).
mod wire_secret {
    use harness_core::Secret;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &Option<Secret<String>>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(t) => s.serialize_some(t.expose()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<Secret<String>>, D::Error> {
        Ok(Option::<String>::deserialize(d)?.map(Secret::new))
    }
}

/// The node capability that announces `project.mcp` (ADOPTION_PLAN X.36,
/// PLAN.md T82): its node.toml says `[mcp] project_servers = "ask"`, so a
/// person may approve the MCP servers a project folder's `.mcp.json`
/// declares, one by one, for their own chats there. A node without it
/// reads no `.mcp.json` and the master sends it no `project.mcp`.
pub const CAP_PROJECT_MCP: &str = "project-mcp";

/// The longest user id a `session.create` or `project.mcp` may carry, in
/// bytes (the master's ids are 26 characters).
pub const MAX_USER_ID: usize = 128;

/// Whether `id` may name an account in a message to a node: 1 to
/// [`MAX_USER_ID`] of ASCII letters, digits, `-` and `_` (the master's ids
/// are ULIDs), so it is safe in a log line and as a key.
pub fn valid_user_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_USER_ID
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The longest string of a `project.mcp_result` entry, in characters: an
/// approvable entry's command, argument, `env` name or value (shown whole,
/// never cut), the cut text of one that cannot be approved, a reason. The
/// node keeps to it by construction and the master refuses a list that
/// does not (X.36, PLAN.md T82).
pub const MAX_SHOWN: usize = 512;

/// The first character of `s` that an approvable entry may not hold, if
/// any: an approvable string is printable ASCII (0x21 to 0x7E) and single
/// spaces only, so what a screen draws is what runs (no invisible, combining
/// or look-alike character: review of PR 150, NEW-1). Spaces at the edges
/// and runs of them are the node's and the master's other checks.
pub fn first_unshowable(s: &str) -> Option<char> {
    s.chars().find(|c| !(' '..='~').contains(c))
}

/// How a character is named in a reason: its code point, never itself.
pub fn code_point(c: char) -> String {
    format!("U+{:04X}", c as u32)
}

/// What a person asks a node about a project folder's `.mcp.json` (X.36).
/// Every one reads the file once, then answers with the list as it is
/// after the request. Nothing here names a program: an approval names a
/// server of the file and the digest of the entry the person was shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ProjectMcpRequest {
    /// The list (opening it, and Reload).
    List,
    /// Approve the file's entry `name`, if it still has `digest`.
    Approve { name: String, digest: String },
    /// Remove this person's approval of `name` (always allowed).
    Remove { name: String },
}

/// Where a server of a project's `.mcp.json` stands for one person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectMcpState {
    /// The file's entry is the one this person approved.
    Approved,
    /// Approved, but the file's entry differs now: the approved copy is
    /// what runs until the new one is approved.
    Changed,
    /// In the file, not approved: it does not run.
    New,
    /// Approved, and no longer in the file: it is not started.
    Removed,
    /// It can never be approved (`reason` says why).
    Unsupported,
    /// The node's own node.toml has a server of this name, which wins.
    Hidden,
}

/// One `env` entry of a repository's server, as the asking person is
/// shown it: the value too, which is repository content and part of what
/// runs (X.36).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMcpEnv {
    pub name: String,
    pub value: String,
}

/// One server as a person is shown it: what the entry runs, whole, as
/// text: the command, each argument, each `env` name and value. For an
/// entry that can be approved these are exactly the strings the digest
/// covers and the node runs; one that cannot be approved may be shown
/// escaped and cut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMcpEntry {
    pub name: String,
    pub state: ProjectMcpState,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<ProjectMcpEnv>,
    /// The digest of the file's entry, to approve it with (`new`,
    /// `changed`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    /// Why it is `unsupported` or `hidden`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// For `changed`: what the approved copy runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_env: Option<Vec<ProjectMcpEnv>>,
}

/// A project folder's `.mcp.json` as one person stands with it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMcpList {
    /// Why the file could not be read, when it could not (a link, not a
    /// regular file, over the cap, not the standard shape); `entries`
    /// then holds this person's approved copies only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unreadable: Option<String>,
    /// There is no `.mcp.json` in the folder.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub missing: bool,
    #[serde(default)]
    pub entries: Vec<ProjectMcpEntry>,
    /// What the node said about the request (an approval refused because
    /// the entry changed, …), in words for the person.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The most entries a [`ProjectMcpList`] may hold (the node's 32 in the
/// file and 32 gone from it), and arguments or `env` entries per entry.
pub const MAX_LISTED_ENTRIES: usize = 64;
pub const MAX_LISTED_PARTS: usize = 64;

impl ProjectMcpList {
    /// Whether the page can show this list whole (X.36): at most
    /// [`MAX_LISTED_ENTRIES`] entries of at most [`MAX_LISTED_PARTS`]
    /// arguments and `env` entries; every string of every entry (name,
    /// command, arguments, `env` names and values, digest, reason, the
    /// approved copy's) printable ASCII and spaces of at most
    /// [`MAX_SHOWN`] characters ([`first_unshowable`]); `unreadable` and
    /// `note` at most [`MAX_SHOWN`] characters without a control character.
    /// The node builds every list to pass it; the master refuses one that
    /// does not.
    pub fn showable(&self) -> Result<(), String> {
        let text = |what: &str, s: &str| -> Result<(), String> {
            if s.chars().count() > MAX_SHOWN {
                return Err(format!("{what} is longer than {MAX_SHOWN} characters"));
            }
            match first_unshowable(s) {
                Some(c) => Err(format!("{what} holds {}", code_point(c))),
                None => Ok(()),
            }
        };
        let words = |what: &str, s: &Option<String>| -> Result<(), String> {
            match s {
                Some(s) if s.chars().count() > MAX_SHOWN => {
                    Err(format!("{what} is longer than {MAX_SHOWN} characters"))
                }
                Some(s) if s.chars().any(char::is_control) => {
                    Err(format!("{what} holds a control character"))
                }
                _ => Ok(()),
            }
        };
        let parts = |args: &[String], env: &[ProjectMcpEnv]| -> Result<(), String> {
            if args.len() > MAX_LISTED_PARTS || env.len() > MAX_LISTED_PARTS {
                return Err("an entry has too many arguments or env entries".into());
            }
            for a in args {
                text("an argument", a)?;
            }
            for e in env {
                text("an env name", &e.name)?;
                text("an env value", &e.value)?;
            }
            Ok(())
        };
        if self.entries.len() > MAX_LISTED_ENTRIES {
            return Err(format!("more than {MAX_LISTED_ENTRIES} entries"));
        }
        words("the file's state", &self.unreadable)?;
        words("the note", &self.note)?;
        for e in &self.entries {
            text("a name", &e.name)?;
            text("a command", &e.command)?;
            parts(&e.args, &e.env)?;
            for (what, s) in [
                ("a digest", &e.digest),
                ("a reason", &e.reason),
                ("an approved command", &e.approved_command),
            ] {
                if let Some(s) = s {
                    text(what, s)?;
                }
            }
            parts(
                e.approved_args.as_deref().unwrap_or_default(),
                e.approved_env.as_deref().unwrap_or_default(),
            )?;
        }
        Ok(())
    }
}

/// The most paths one `session.prompt` may name in `files`, and the
/// longest one, in bytes; a node reads at most its own number of them.
pub const MAX_PROMPT_FILES: usize = 64;
pub const MAX_PROMPT_FILE_BYTES: usize = 1024;

/// The most paths one `session.files_result` carries.
pub const MAX_FILES_RESULT: usize = 50;

/// The largest serialized `session.diff_result` a node sends: a quarter of
/// the link's frame, so one answer holds the node's ordered output back by
/// a few seconds at most on a slow link. The node cuts what does not fit
/// and says so (`TurnDiff::cut`).
pub const MAX_DIFF_JSON: usize = 2 * 1024 * 1024;

/// What one turn changed in its session's folder, computed by the owning
/// node from the turn's two checkpoints (X.12). A view, never an event:
/// nothing of it is logged or stored, on the node or the master.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnDiff {
    /// The changed files, in git's order (by path).
    pub files: Vec<DiffFile>,
    /// Changed files past the listed ones (not named, only counted).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub more_files: u64,
    /// Something was left out to keep the answer small: a file's lines, or
    /// whole files' lines past the total budget. Each such file says so.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cut: bool,
    /// Git repositories nested in the folder, which no checkpoint holds:
    /// their files are not compared (docs/undo-and-stash.md §2.6).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nested: Vec<String>,
    /// Files of this diff that another chat working in the folder changed
    /// during the turn (its edit tools, its own undo): an undo of the turn
    /// would put them back too, so it asks first (PLAN.md T57). Nothing
    /// about that chat.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub others_wrote: Vec<String>,
    /// Files of this diff another chat may have changed during the turn
    /// (its commands, a background command, a chat deleted since), not in
    /// `others_wrote`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub others_may: Vec<String>,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// How a file changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffStatus {
    Added,
    Deleted,
    Modified,
    /// Moved (`old_path` says from where), maybe also changed.
    Renamed,
    /// A file became a link, or the other way round.
    TypeChanged,
}

/// What the answer holds of a file's content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffContent {
    /// Line changes in `hunks` (none for a pure rename or a mode change).
    Text,
    /// Git found binary content: only the sizes are given.
    Binary,
    /// A side is larger than the node compares line by line.
    TooLarge,
    /// Left out: the answer's budget was spent on the files before it.
    Omitted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffFile {
    /// Relative to the checkpoint's folder (lossy UTF-8; hidden characters
    /// are the viewer's to mark).
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    pub status: DiffStatus,
    pub content: DiffContent,
    /// Git's modes (`100644`, `100755`, `120000` for a link); absent on the
    /// side that does not exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_size: Option<u64>,
    /// Lines added and removed in the hunks given.
    #[serde(default)]
    pub added: u32,
    #[serde(default)]
    pub removed: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hunks: Vec<DiffHunk>,
    /// Some of this file's lines, or of one long line, were cut.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cut: bool,
}

/// One hunk of a unified diff with three lines of context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    /// The text after the second `@@` (the function git found), if any.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub section: String,
    /// Each line with its mark first: ` ` context, `+` added, `-` removed,
    /// `\` "no newline at end of file" for the line before it.
    pub lines: Vec<String>,
}

/// The node capability that says it offers `remember` to its sessions
/// (registered there and not in its `denied_tools`). The tool runs nothing
/// on the node; the master files a mirrored call as a proposal only from a
/// node whose hello listed this, and tells the model about the tool only
/// in chats on such a node.
pub const CAP_REMEMBER: &str = "remember";

/// The node capability that says it offers `recall` to its sessions
/// (registered there and not in its `denied_tools`) and speaks
/// `recall.request`. The master offers the tool only in chats on such a
/// node, answers requests only from one, and sends `recall.result` only to
/// one (PLAN.md T55).
pub const CAP_RECALL: &str = "recall";

/// The node capability that says it enforces `AgentSpec::deny_skills` and
/// `AgentSpec::deny_mcp_servers` (ADOPTION_PLAN X.34, PLAN.md T80). To a
/// node without it the master withholds the whole `skill` tool or `mcp`
/// (through `deny_tools`, which every node reads) from an agent that has
/// any of them, so an Assigned item never reaches another agent there.
pub const CAP_PLUGIN_ACCESS: &str = "plugin-access";

/// The node capability that says it takes the master's stored skills
/// (X.34 step B, PLAN.md T81): it offers the `skill` tool, node.toml
/// `[skills] from_master` is on, it merges `AgentSpec::stored_skills` into
/// a turn's catalogue and speaks `skill.request`. The master sends the list
/// only to such a node (to any other it sends none), answers requests only
/// from one and sends `skill.result` only to one.
pub const CAP_STORED_SKILLS: &str = "stored-skills";

/// The node capability that says it offers a workspace's custom
/// connectors' tools to its turns (X.35 step 2, PLAN.md T85): node.toml
/// `[mcp] master_connectors` is on (the default), it offers
/// `AgentSpec::connector_tools` as MCP tools and speaks
/// `connector.request`. The master sends the list only to such a node,
/// answers requests only from one and sends `connector.result` only to one.
pub const CAP_CONNECTORS: &str = "connectors";

/// Skills one `node.plugins` names; more are counted, not named.
pub const MAX_ANNOUNCED_SKILLS: usize = 300;
/// MCP servers one `node.plugins` names.
pub const MAX_ANNOUNCED_SERVERS: usize = 64;
/// Characters of one announced skill's description.
pub const MAX_ANNOUNCED_DESCRIPTION: usize = 200;
/// Bytes of every announced name and description together.
pub const MAX_ANNOUNCED_BYTES: usize = 64 * 1024;

/// One skill a node announces: its name and a one-line description.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnouncedSkill {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// What a node has for the Plugins screen (X.34): the skills of its own
/// node.toml `[skills] roots` and the names of its `[mcp_servers]`, never
/// anything else of them (no path, command, argument or environment).
/// Repository skills are not here: they belong to a session's folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodePlugins {
    #[serde(default)]
    pub skills: Vec<AnnouncedSkill>,
    #[serde(default)]
    pub mcp_servers: Vec<String>,
    /// Items found but not named (past a cap, or refused by the checks).
    #[serde(default)]
    pub left_out: u32,
}

impl NodePlugins {
    /// This announcement as the master keeps it, whoever sent it: names
    /// that pass the node's own rules (`valid_skill_name`,
    /// `valid_mcp_server_name`: ASCII only, so no look-alike letter, hidden
    /// character or line break), each once, sorted; descriptions one clean
    /// line of at most [`MAX_ANNOUNCED_DESCRIPTION`] characters; at most
    /// [`MAX_ANNOUNCED_SKILLS`] skills, [`MAX_ANNOUNCED_SERVERS`] servers
    /// and [`MAX_ANNOUNCED_BYTES`] bytes. What is dropped is counted in
    /// `left_out`.
    pub fn bounded(self) -> NodePlugins {
        use harness_core::agent::valid_mcp_server_name;
        use harness_core::skills::valid_skill_name;
        let mut left = u64::from(self.left_out);
        let mut bytes = 0usize;
        let mut skills: Vec<AnnouncedSkill> = Vec::new();
        let mut sent = self.skills;
        sent.sort_by(|a, b| a.name.cmp(&b.name));
        for s in sent {
            let description =
                harness_core::text::clean_line(&s.description, MAX_ANNOUNCED_DESCRIPTION);
            let size = s.name.len() + description.len();
            if !valid_skill_name(&s.name)
                || skills.last().is_some_and(|l| l.name == s.name)
                || skills.len() >= MAX_ANNOUNCED_SKILLS
                || bytes + size > MAX_ANNOUNCED_BYTES
            {
                left += 1;
                continue;
            }
            bytes += size;
            skills.push(AnnouncedSkill {
                name: s.name,
                description,
            });
        }
        let mut servers: Vec<String> = Vec::new();
        let mut sent = self.mcp_servers;
        sent.sort();
        for name in sent {
            if !valid_mcp_server_name(&name)
                || servers.last() == Some(&name)
                || servers.len() >= MAX_ANNOUNCED_SERVERS
                || bytes + name.len() > MAX_ANNOUNCED_BYTES
            {
                left += 1;
                continue;
            }
            bytes += name.len();
            servers.push(name);
        }
        NodePlugins {
            skills,
            mcp_servers: servers,
            left_out: u32::try_from(left).unwrap_or(u32::MAX),
        }
    }
}

/// The node capability that says it offers the project board tools
/// (`board`, `board_edit`: registered there and not both in its
/// `denied_tools`) and speaks `board.request`. The master offers the tools
/// only in project chats on such a node, answers requests only from one,
/// and sends `board.result` only to one (ADOPTION_PLAN X.32, PLAN.md T78).
pub const CAP_BOARD: &str = "board";

/// The node capability that says it runs sessions of an execution mode
/// (N.10a): `exec:sandbox`, `exec:host`. Announced from the node's own
/// self-test and `[execution]` at start; what the UI offers and names. The
/// node refuses a mode it does not offer whatever the master believes.
pub const CAP_EXEC_PREFIX: &str = "exec:";

/// The capability for `mode`.
pub fn exec_capability(mode: harness_core::policy::ExecMode) -> String {
    format!("{CAP_EXEC_PREFIX}{}", mode.id())
}

/// The node capability that announces `session.set_exec_mode` (N.10b): the
/// owner's switch of an existing agentic chat between `sandbox` and `host`
/// while nothing of the chat runs. The master sends it only to a node whose
/// hello listed it (an older node would drop the link on an unknown
/// message): "update the node".
pub const CAP_EXEC_SWITCH: &str = "exec-switch";

/// The node capabilities that announce `session.set_approval_mode` and say
/// its approval modes (T50): `approval-max:<mode>`, the loosest mode
/// node.toml allows any chat there (`max_approval_mode`), and
/// `approval-default:<mode>`, what a create without a mode gets there. What the UI offers and names; the node
/// clamps every request whatever the master believes. The master sends
/// `session.set_approval_mode` only to a node whose hello listed
/// `approval-max:*`.
pub const CAP_APPROVAL_MAX_PREFIX: &str = "approval-max:";
/// The node capability that announces `session.set_step_limit` and says
/// its step limit (X.14): `steps-max:<n>`, node.toml's `[limits]
/// max_steps`, the most a chat there may be set to. What the UI offers;
/// the node clamps every request. The master sends `session.set_step_limit`
/// only to a node whose hello listed it.
pub const CAP_STEPS_MAX_PREFIX: &str = "steps-max:";

/// How a node's refusal of a `session.set_step_limit` sent as a lowering
/// that would raise the chat's limit begins. The master answers it as a
/// refusal to send again (its copy of the log was behind).
pub const STEPS_NOT_A_LOWERING: &str =
    "this change was sent as a lowering but would raise the chat's step limit";
pub const CAP_APPROVAL_DEFAULT_PREFIX: &str = "approval-default:";

/// How a node's refusal of a `session.set_approval_mode` sent as a lowering
/// that would raise the mode begins. The master answers it as a refusal to
/// send again (its copy of the log was behind; on the next try it sees the
/// mode the node has).
pub const MODE_NOT_A_LOWERING: &str =
    "this change was sent as a lowering but would raise the chat's approval mode";

/// The node capability that announces it takes `session.prompt.peers`: the
/// agents a message called after the first, which it asks one after another
/// (ADOPTION_PLAN X.37). The master sends the field only to a node that
/// listed it (an older node would ignore it and answer with the first agent
/// alone) and refuses the message with a sentence otherwise.
pub const CAP_PEERS: &str = "peers";

/// The most agents after the first that one prompt may carry
/// ([`harness_core::mention::MAX_CALLED`] less the first). The node refuses
/// more.
pub const MAX_PEERS: usize = harness_core::mention::MAX_CALLED - 1;

/// The node capability that announces the person's answers beyond allow
/// and deny: a note with a denial, `allow_chat` and `session.revoke_allow`.
/// The master sends `allow_chat` and `session.revoke_allow` only to a node
/// whose hello listed it (an unknown decision or message drops the link).
pub const CAP_ANSWERS: &str = "answers";

/// The gaps of the modes a node offers, for the UI to say in the chat
/// (reviews T8B-3, N.10a L-d, L-e). The node's state at its hello: one the
/// node closes later (the network namespace taken by a later probe) is
/// still announced until its next hello.
///
/// With `[sandbox] network = "off"`: the sandbox mode's commands have no
/// network namespace (bubblewrap could not make one at start), so UDP and
/// DNS still go out; Landlock stops TCP.
pub const GAP_SANDBOX_NETNS: &str = "gap:sandbox:netns";
/// With the network off: a `host` chat never has a namespace; Landlock
/// stops TCP, UDP and DNS go out.
pub const GAP_HOST_NETNS: &str = "gap:host:netns";
/// With the network off: a `host` chat where nothing applies it (`[sandbox]
/// mode = "off"`, or Landlock not enforcing its TCP rule): everything goes
/// out.
pub const GAP_HOST_NET_OPEN: &str = "gap:host:net-open";
/// Below Landlock ABI 9: a sandbox command can reach a service of the
/// node's user over a socket inside a granted folder and have it start a
/// process outside the containment.
pub const GAP_SANDBOX_SOCKETS: &str = "gap:sandbox:sockets";
/// The shared-`/proc` form: every process ends with the command, but
/// `/proc` lists this machine's processes.
pub const GAP_SANDBOX_PROC: &str = "gap:sandbox:proc";
/// Every gap a node may announce.
pub const GAPS: [&str; 5] = [
    GAP_SANDBOX_NETNS,
    GAP_HOST_NETNS,
    GAP_HOST_NET_OPEN,
    GAP_SANDBOX_SOCKETS,
    GAP_SANDBOX_PROC,
];

/// How a node's refusal of a `session.callback` begins when there is
/// nothing to resume: every named end was already delivered, covered by a
/// later turn or refused before, or the chat is busy. The master drops
/// those quietly (a second send of the same callback, a race with the
/// person's message); every other refusal is the owner's to see.
pub const CALLBACK_NOTHING: &str = "nothing to resume";

/// A node's refusal of a `session.callback` while a turn of the chat runs
/// (a [`CALLBACK_NOTHING`] for now): the master looks again later. The ends
/// it named are then covered by that turn, or, beyond what one callback may
/// name ([`MAX_CALLBACK_IDS`]), still wait and are asked for once it ended.
pub const CALLBACK_BUSY: &str = "nothing to resume: a turn is already running";

/// How a node's refusal of a `session.callback` on its own rules begins
/// (its budget, `callback_turns = 0`, a chat that takes none). The node
/// recorded `callback_refused` in the chat's log, which is where the owner
/// sees it; the master adds nothing of its own.
pub const CALLBACK_REFUSED: &str = "the agent was not resumed";

/// A `session.handoff` the node did not start because no note is due
/// (nothing changed since the last one, a turn ran since the master read
/// the log, a chat that takes none): the master drops it quietly.
pub const HANDOFF_NOTHING: &str = "no hand-off note due";
/// A `session.handoff` refused because a turn or another note runs: the
/// master asks again at its next look.
pub const HANDOFF_BUSY: &str = "no hand-off note now: the chat is busy";

/// A `session.compact` refused because a turn runs: `/compact` works between
/// turns (a running turn shortens its own context when it must, X.15).
pub const COMPACT_TURN_RUNNING: &str = "A turn is running: /compact works between turns. Stop \
     it or wait for it to end, then try again (a running turn shortens its own context when it \
     must).";
/// A `session.compact` refused because a message of the person's waits to
/// be read (sent during the last turn): it is read first.
pub const COMPACT_PENDING: &str = "A message of yours is waiting to be read: send /compact \
     again after its answer (or send a message to have it read).";
/// A `session.compact` refused because another one runs in the chat.
pub const COMPACT_BUSY: &str = "This chat is already being summarised.";
/// A `session.set_agent` refused while the chat works: a turn of any kind
/// runs (also one being stopped, a callback turn, a retry) or a `/compact`.
/// The master answers 409 with it.
pub const AGENT_WORKING: &str = "The agent is working: stop it or wait for it to finish, then \
     pick another agent.";
/// A prompt or callback refused while the chat is being summarised.
pub const COMPACTING: &str = "This chat is being summarised: send your message when that is \
     done.";

/// How a node's refusal of a `session.retry` begins when the turn may not
/// be retried (not the chat's last, ended otherwise, followed by something,
/// a turn the person did not start): the master answers 409 with it.
pub const RETRY_REFUSED: &str = "This turn can't be retried";

/// How a node's refusal of a `session.rewind` or `session.rewind_undo`
/// begins (X.46): the master answers 409 with it.
pub const REWIND_REFUSED: &str = "This chat can't be rewound there";

/// How a node's refusal of a `session.fork` begins (C.2): the master
/// answers 409 with it.
pub const FORK_REFUSED: &str = "This chat can't be forked there";

/// The most background ids one `session.callback` may name.
pub const MAX_CALLBACK_IDS: usize = 64;

/// Who asks a node to stop a session's background commands. The node
/// records the end with this reason; nothing here can start or widen
/// anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopBackgroundBy {
    /// The person (step 2: the chat's Stop per command, Stop all).
    User,
    /// The master's revocation sweep: the owner lost access.
    AccessRevoked,
}

/// At most this many images go with one message...
pub const MAX_IMAGES: usize = 4;
/// ...and at most this many bytes of them together (before base64).
pub const MAX_IMAGE_BYTES: u64 = harness_core::image::MAX_BYTES;
/// The largest serialized `session.prompt` the master sends: the link's
/// frame is 8 MiB, and images, attachments, the agent spec and the context
/// together could pass it otherwise.
pub const MAX_PROMPT_JSON: usize = 7 * 1024 * 1024;

/// One image sent with a prompt (`session.prompt.images`). The node keeps
/// the bytes for the chat and records the rest in `user_message.images`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatImage {
    pub media_id: String,
    /// Hex sha256 of the bytes (the node checks it).
    pub sha256: String,
    pub mime: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// The bytes, base64.
    pub data: String,
}

impl ChatImage {
    /// What the message records: everything but the bytes.
    pub fn image_ref(&self) -> harness_core::event::ImageRef {
        harness_core::event::ImageRef {
            media_id: self.media_id.clone(),
            sha256: self.sha256.clone(),
            mime: self.mime.clone(),
            name: self.name.clone(),
            width: self.width,
            height: self.height,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionHead {
    pub session_id: SessionId,
    pub head_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NodeToMaster {
    #[serde(rename = "node.hello")]
    Hello {
        protocol_version: u32,
        node_id: String,
        hostname: String,
        version: String,
        #[serde(default)]
        capabilities: Vec<String>,
        #[serde(default)]
        sessions: Vec<SessionHead>,
    },
    #[serde(rename = "session.event")]
    Event { event: RawEvent },
    /// A replay batch answering `resume`. `head_seq` is the node's head at the
    /// time the batch was read; more batches follow while `seq < head_seq`.
    #[serde(rename = "session.snapshot")]
    Snapshot {
        session_id: SessionId,
        head_seq: u64,
        events: Vec<RawEvent>,
    },
    /// The node deleted a session (on request or because its incognito TTL
    /// ran out); the master drops its mirror copy.
    #[serde(rename = "session.deleted")]
    Deleted { session_id: SessionId },
    #[serde(rename = "ack")]
    Ack {
        req_id: String,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// A `recall` call of a session this node runs (PLAN.md T55): words to
    /// look for and nothing else. The master decides whose chats and which
    /// memory from `session_id` alone and answers with one `recall.result`.
    #[serde(rename = "recall.request")]
    Recall {
        req_id: String,
        session_id: SessionId,
        query: harness_core::recall::RecallQuery,
    },
    /// A `board` or `board_edit` call of a session this node runs (PLAN.md
    /// T78): what to do and nothing else. The master finds the project,
    /// the person and whether they may change the board from `session_id`
    /// alone and answers with one `board.result`.
    #[serde(rename = "board.request")]
    Board {
        req_id: String,
        session_id: SessionId,
        request: harness_core::board::BoardRequest,
    },
    /// The agent of a session this node runs loads a stored skill (PLAN.md
    /// T81): its name and nothing else. The master decides from
    /// `session_id` alone (its workspace, its open turn's agent, the
    /// Plugins rule) and answers with one `skill.result`.
    #[serde(rename = "skill.request")]
    Skill {
        req_id: String,
        session_id: SessionId,
        name: String,
    },
    /// A call of a custom connector's tool by a session this node runs
    /// (X.35 step 2, PLAN.md T85): the public tool name and its arguments,
    /// nothing else. The master decides from `session_id` alone whose
    /// account (the chat owner's), which connector (the chat's workspace's,
    /// by the name in `tool`) and whether the open turn's agent may call
    /// it, makes the call and answers with one `connector.result`.
    #[serde(rename = "connector.request")]
    Connector {
        req_id: String,
        session_id: SessionId,
        tool: String,
        #[serde(default)]
        arguments: serde_json::Value,
    },
    /// The call `req_id` is no longer waited for (its turn stopped): the
    /// master drops it if it still runs. Nothing is answered.
    #[serde(rename = "connector.cancel")]
    ConnectorCancel { req_id: String },
    /// Bytes of a staged file, answering `artifact.fetch` (pull only: a
    /// node never sends one unasked). `error: "busy"` asks for a retry.
    #[serde(rename = "artifact.chunk")]
    ArtifactChunk {
        req_id: String,
        stage_id: String,
        offset: u64,
        /// Base64.
        #[serde(default)]
        data: String,
        eof: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// The answer to `session.diff` (pull only: never sent unasked): the
    /// turn's changes, or why there are none to show, in words for the
    /// person. At most `MAX_DIFF_JSON` serialized.
    #[serde(rename = "session.diff_result")]
    DiffResult {
        req_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diff: Option<Box<TurnDiff>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// The answer to `session.files` (pull only): at most
    /// [`MAX_FILES_RESULT`] paths relative to the chat's folder (a folder
    /// ends with `/`), or why there are none.
    #[serde(rename = "session.files_result")]
    FilesResult {
        req_id: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        paths: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// What the node has for the Plugins screen (X.34), sent unasked after
    /// `node.hello` once the node has read it. The master keeps the first
    /// one of each link, bounded ([`NodePlugins::bounded`]), and ignores a
    /// second on the same link; it names nothing a node may run.
    #[serde(rename = "node.plugins")]
    Plugins { plugins: NodePlugins },
    /// The answer to `project.create` (pull only: never sent unasked): the
    /// absolute path of the folder the node made, or why it made none, in
    /// words for the person.
    #[serde(rename = "project.created")]
    ProjectCreated {
        req_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// The answer to `project.mcp` (pull only: never sent unasked): the
    /// list after the request for the asking person, or why there is none,
    /// in words for that person. Each entry's command, arguments and `env`
    /// names and values: repository content, shown so the person sees
    /// what runs; the master passes it to the asker alone.
    #[serde(rename = "project.mcp_result")]
    ProjectMcpResult {
        req_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        list: Option<Box<ProjectMcpList>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

/// The body of `session.prompt`. Every field is named by every sender: there is no `Default`,
/// so a new field is a compile error where the master builds the message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptMsg {
    pub req_id: String,
    pub session_id: SessionId,
    pub text: String,
    /// The agent that runs this turn (the session's agent, or an
    /// @mention override). Resolved by the master; the node does not
    /// store agent definitions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentSpec>,
    /// The person chose `agent` for this message (an explicit agent or
    /// an @mention), not the chat's own. A message that would wait for
    /// a running turn of another agent is refused (X.5, review ST-5).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub agent_named: bool,
    /// Documents with their extracted text (extracted on the master).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<harness_core::event::Attachment>,
    /// Parallel answers from these models (chat turns, no tools).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compare: Vec<String>,
    /// Token-saving methods for this turn (absent = all off).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub savings: Option<harness_core::savings::Savings>,
    /// The master's context for this message (the person's settings,
    /// memory, the project's brief and board), placed after the history
    /// so the cached prefix does not change. The loop records what it
    /// sent of it in the turn's `context_sent` and sends a standing part
    /// again only when it changed (X.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Box<harness_core::backend::MasterContext>>,
    /// An answer to a choices or form block; `text` is then the user's
    /// note (may be empty). The node checks it against its own log.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<harness_core::event::FormReply>,
    /// Images for this message; only to nodes with `CAP_IMAGES`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ChatImage>,
    /// Paths of the chat's folder the person mentioned with `@path`
    /// (PLAN.md T63; only to nodes with [`CAP_FILES`]). Names only: the
    /// node reads each under its own `read` rules and records what it
    /// found in the message; nothing here is file content.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// The account that owns the chat (its user id; none for the API
    /// token's), set by the master from the chat's row in
    /// `send_command` whoever builds the command: the node starts only
    /// the repository MCP servers this account approved (X.36, PLAN.md
    /// T82), and refuses a turn whose `owner` is not
    /// [`valid_user_id`]. Kept in the node's memory, never logged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// The chat owner's login, display name and the e-mail they set for
    /// commits (T88), set by the master beside `owner` and only for the
    /// node that runs the chat: the agent's `git commit` carries it. The
    /// node checks every field again and falls back to a no-reply
    /// address built from the login; none for a chat nobody owns. Kept
    /// in the node's memory; never logged, never in an event (its
    /// `Debug` hides the address).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<harness_core::identity::CommitIdentity>,
    /// The agents this message called after the first, in the order
    /// they answer (X.37; only to nodes with [`CAP_PEERS`], at most
    /// [`MAX_PEERS`]): each is resolved by the master as `agent` is,
    /// with no connector tool, memory tool or master context (their
    /// turns are no send of the master's). The node keeps them in
    /// memory only and starts a turn for each, one after another,
    /// after the turn before it ended; `user_message.triggers` records
    /// who was called. The first agent is `agent` and answers this
    /// message's own turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub peers: Vec<AgentSpec>,
    /// The agents that run only as sub-agents this message mentions
    /// (tags, X.37; only to nodes with [`CAP_PEERS`], at most
    /// `harness_core::mention::MAX_RESOLVED`): the node records them in
    /// `user_message.tags` and names the ones each answering agent has
    /// among its own delegates in that turn's opening. Never a call.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

/// The body of `session.callback`. Every field is named by every sender: there is no `Default`,
/// so a new field is a compile error where the master builds the message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallbackMsg {
    pub req_id: String,
    pub session_id: SessionId,
    pub bg_ids: Vec<String>,
    /// The chat's agent, resolved by the master as for a prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub savings: Option<harness_core::savings::Savings>,
    /// The standing parts of the master's context for the chat's agent
    /// and owner (`MasterContext::unattended`, X.1 review M1): the
    /// turn's request extends the last one when they did not change.
    /// Absent: the turn reads no memory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Box<harness_core::backend::MasterContext>>,
    /// The account that owns the chat (its user id; none for the API
    /// token's), set by the master from the chat's row in
    /// `send_command` whoever builds the command: the node starts only
    /// the repository MCP servers this account approved (X.36, PLAN.md
    /// T82), and refuses a turn whose `owner` is not
    /// [`valid_user_id`]. Kept in the node's memory, never logged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// The chat owner's login, display name and the e-mail they set for
    /// commits (T88), set by the master beside `owner` and only for the
    /// node that runs the chat: the agent's `git commit` carries it. The
    /// node checks every field again and falls back to a no-reply
    /// address built from the login; none for a chat nobody owns. Kept
    /// in the node's memory; never logged, never in an event (its
    /// `Debug` hides the address).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<harness_core::identity::CommitIdentity>,
}

/// The body of `session.retry`. Every field is named by every sender: there is no `Default`,
/// so a new field is a compile error where the master builds the message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetryMsg {
    pub req_id: String,
    pub session_id: SessionId,
    pub turn_id: String,
    /// The chat's agent, resolved by the master as for a prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub savings: Option<harness_core::savings::Savings>,
    /// The master's context for the turn, as for a prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Box<harness_core::backend::MasterContext>>,
    /// The retried message named its agent (a leading @mention), as
    /// `session.prompt`'s: the retry is then no switch of the chat's.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub agent_named: bool,
    /// The account that owns the chat (its user id; none for the API
    /// token's), set by the master from the chat's row in
    /// `send_command` whoever builds the command: the node starts only
    /// the repository MCP servers this account approved (X.36, PLAN.md
    /// T82), and refuses a turn whose `owner` is not
    /// [`valid_user_id`]. Kept in the node's memory, never logged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// The chat owner's login, display name and the e-mail they set for
    /// commits (T88), set by the master beside `owner` and only for the
    /// node that runs the chat: the agent's `git commit` carries it. The
    /// node checks every field again and falls back to a no-reply
    /// address built from the login; none for a chat nobody owns. Kept
    /// in the node's memory; never logged, never in an event (its
    /// `Debug` hides the address).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<harness_core::identity::CommitIdentity>,
}

/// Test constructors: every field but the ones named is at its empty value
/// (`None`, no items, `false`). Only for tests (`test-support` feature of
/// this crate, enabled by the dev-dependencies of the crates whose tests
/// build these messages): a sender in production code names every field.
#[cfg(any(test, feature = "test-support"))]
impl PromptMsg {
    /// A prompt with `req_id` `"p"`.
    pub fn test(session_id: SessionId, text: impl Into<String>) -> Self {
        PromptMsg {
            req_id: "p".into(),
            session_id,
            text: text.into(),
            agent: None,
            agent_named: false,
            attachments: vec![],
            compare: vec![],
            savings: None,
            context: None,
            reply: None,
            images: vec![],
            files: vec![],
            owner: None,
            identity: None,
            peers: vec![],
            tags: vec![],
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl CallbackMsg {
    /// A callback with `req_id` `"r"`.
    pub fn test(session_id: SessionId, bg_ids: Vec<String>) -> Self {
        CallbackMsg {
            req_id: "r".into(),
            session_id,
            bg_ids,
            agent: None,
            savings: None,
            context: None,
            owner: None,
            identity: None,
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl RetryMsg {
    /// A retry with `req_id` `"r"`.
    pub fn test(session_id: SessionId, turn_id: impl Into<String>) -> Self {
        RetryMsg {
            req_id: "r".into(),
            session_id,
            turn_id: turn_id.into(),
            agent: None,
            savings: None,
            context: None,
            agent_named: false,
            owner: None,
            identity: None,
        }
    }
}

// A command is built, sent once and dropped; none is kept in bulk, so the
// size of its largest variant (`session.prompt`, with its spec and, since
// X.36, its owner) costs nothing worth boxing every literal for.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MasterToNode {
    #[serde(rename = "master.welcome")]
    Welcome { protocol_version: u32 },
    #[serde(rename = "session.create")]
    Create {
        req_id: String,
        session_id: SessionId,
        title: String,
        cwd: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// A request only; the node clamps it to its own policy.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        approval_mode: Option<ApprovalMode>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<Profile>,
        /// The session's initial agent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent: Option<AgentSpec>,
        /// Temporary chat: excluded from search and memory, purged after a TTL.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        incognito: bool,
        /// This chat never gets a callback turn (a routine's run chat). The
        /// node records it in `session_created` and holds to it for good;
        /// no later command can clear it.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        no_callbacks: bool,
        /// How the session's commands run (N.10a). A request only: the node
        /// refuses a mode it does not offer, whatever the master sends, and
        /// without one takes `sandbox` or refuses (never `host` unless asked
        /// for by name). The master always sends one. Recorded in
        /// `session_created`; no later command carries or changes it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exec_mode: Option<harness_core::policy::ExecMode>,
    },
    #[serde(rename = "session.prompt")]
    Prompt(PromptMsg),
    /// Resume an idle chat because background commands ended (a callback
    /// turn, T.8d step 3; only to nodes with `CAP_CALLBACK`). There is no
    /// person's text, attachment, image, reply or context: the node opens
    /// the turn with the harness's own frame and the ended commands' notes
    /// from its log. A request only: the node starts it only if every id is
    /// an undelivered end in its log with no turn after it, and its own
    /// `[limits]` allow it (`callback_turns`, `callback_max_steps`).
    #[serde(rename = "session.callback")]
    Callback(CallbackMsg),
    /// Run the chat's failed last turn again (a person's Retry, X.21; only
    /// to nodes with [`CAP_RETRY`]). Sent only for the chat's owner. No
    /// text, attachment, image or file: the message is the one the failed
    /// turn answered, in the node's log. A request only: the node starts it
    /// only if `turn_id` is its log's last turn, a person's, ended `error`,
    /// `interrupted` or `stopped` with nothing after it, and no turn or
    /// `/compact` runs ([`RETRY_REFUSED`], `TURN_RUNNING`, [`COMPACTING`]).
    #[serde(rename = "session.retry")]
    Retry(RetryMsg),
    /// Take the chat back to an earlier message of the person's (a rewind,
    /// X.46, docs/rewind.md; only to nodes with [`CAP_REWIND`]). Sent only
    /// for the chat's owner, on their click: nothing a model, a tool or a
    /// routine does sends one. A request only: the node writes
    /// `conversation_rewound` only if `message_seq` is a message
    /// `harness_core::rewind::check` allows in its own log, and no turn,
    /// `/compact` or background work of the chat runs ([`REWIND_REFUSED`],
    /// `TURN_RUNNING`, [`COMPACTING`]). With `files` it first puts the
    /// folder back to before the rewound turns, as `session.restore` with
    /// `rewind` does (`force` as there); if that is refused nothing is
    /// written. Every field is named by every sender.
    #[serde(rename = "session.rewind")]
    Rewind {
        req_id: String,
        session_id: SessionId,
        message_seq: u64,
        files: bool,
        force: bool,
    },
    /// Put back what the rewind at `rewind_seq` took out (only to nodes with
    /// [`CAP_REWIND`], for the chat's owner): the node writes
    /// `conversation_rewind_undone` only if `harness_core::rewind::check_undo`
    /// allows it in its own log (no message, turn or summary after the
    /// rewind). Files are never touched.
    #[serde(rename = "session.rewind_undo")]
    RewindUndo {
        req_id: String,
        session_id: SessionId,
        rewind_seq: u64,
    },
    /// Bookmark a message of the chat, or take the bookmark off (docs/pins.md;
    /// only to nodes with [`CAP_PIN`]). Sent only for the chat's owner, on
    /// their click: nothing a model, a tool or a routine does sends one. A
    /// request only: the node writes `message_pinned` only if
    /// `harness_core::pins::check` allows it in its own log. Unlike a
    /// rewind it is also accepted while a turn runs: it changes nothing the
    /// model reads. Every field is named by every sender.
    #[serde(rename = "session.pin")]
    Pin {
        req_id: String,
        session_id: SessionId,
        message_seq: u64,
        pinned: bool,
    },
    /// Fork a chat (ADOPTION_PLAN C.2, docs/fork.md; only to nodes with
    /// [`CAP_FORK`]): a new chat `session_id` (minted by the master, like a
    /// create's) on the same node, whose log is `parent`'s events up to the
    /// end of the turn `turn_id`, at the same seqs, then its own. Sent only
    /// for the parent's owner, on their click: nothing a model, a tool or a
    /// routine does sends one. A request only: the node forks only a turn
    /// `harness_core::fork::check` allows in its own log, into a folder its
    /// own policy allows as at a create ([`FORK_REFUSED`]); the new chat's
    /// events are written in one transaction and the ack follows the last
    /// of them on the ordered link. `title` is the new chat's title (the
    /// parent's, as the master shows it now, and a suffix).
    #[serde(rename = "session.fork")]
    Fork {
        req_id: String,
        session_id: SessionId,
        parent: SessionId,
        turn_id: String,
        title: String,
    },
    /// Write the chat's hand-off note now if one is due (X.16,
    /// docs/handoff-notes.md; only to nodes with [`CAP_HANDOFF`]). A
    /// request only: the node writes it only for a chat of its own that
    /// takes notes (agentic, not a sub-agent's, not a routine's), whose log
    /// says one is due (`harness_core::handoff::due`), with no turn
    /// running; it acks once the writing started and records the note (or
    /// that no model wrote one) in the chat's log. A turn that starts
    /// meanwhile cancels it.
    #[serde(rename = "session.handoff")]
    Handoff {
        req_id: String,
        session_id: SessionId,
        /// The small model to write it (prefixed); `None`: the chat's own.
        /// Tried first; the chat's own model is the fallback.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        small_model: Option<String>,
        /// Context windows the master knows (tokens), by model: the note's
        /// input is sized to the writing model's.
        #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
        windows: std::collections::BTreeMap<String, u64>,
        /// Those models that take a reasoning effort: the note asks them
        /// for the lowest, the others for none.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        low_effort: Vec<String>,
    },
    /// Summarise the chat's history now (a person's `/compact`, X.20, PLAN.md
    /// T64; only to nodes with [`CAP_COMPACT`]). Sent only for the chat's
    /// owner. The node refuses it while a turn runs ([`COMPACT_TURN_RUNNING`])
    /// or for a chat too short to summarise (no event), else acks once it
    /// started and records `history_summarized` (`manual`) or
    /// `compact_failed` in the chat's log.
    #[serde(rename = "session.compact")]
    Compact {
        req_id: String,
        session_id: SessionId,
        /// The person's words on what to keep in focus; the node scrubs
        /// them and cuts them to `COMPACT_FOCUS_MAX_CHARS`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        focus: Option<String>,
        /// Context windows the master knows (tokens), by model.
        #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
        windows: std::collections::BTreeMap<String, u64>,
    },
    /// The user picked one of several parallel answers.
    #[serde(rename = "session.select_variant")]
    SelectVariant {
        req_id: String,
        session_id: SessionId,
        group: String,
        message_id: String,
    },
    /// The person picked another agent for the chat. The node writes
    /// nothing (the master keeps the pick in the chat's row and sends the
    /// agent with the next message, whose turn records `agent_changed`);
    /// it refuses the change while the chat works ([`AGENT_WORKING`]).
    #[serde(rename = "session.set_agent")]
    SetAgent {
        req_id: String,
        session_id: SessionId,
        agent: AgentSpec,
    },
    /// Change the chat's approval mode (T50). A request only: the node
    /// clamps it by its own `max_approval_mode` (nothing else), refuses it for a sub-agent's or a routine's chat, and records
    /// the result as `approval_mode_changed`. Only to nodes whose hello
    /// listed `approval-max:*`.
    #[serde(rename = "session.set_approval_mode")]
    SetApprovalMode {
        req_id: String,
        session_id: SessionId,
        mode: ApprovalMode,
        /// The master judged it a lowering from its copy of the log and so
        /// skipped the owner's level check: the node refuses it if it would
        /// in fact raise the chat's mode (`MODE_NOT_A_LOWERING`; the copy
        /// was behind), and records nothing.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        lowering: bool,
    },
    /// Switch the chat's execution mode (N.10b). A request only: the node
    /// decides again (it offers the mode, the chat is a person's agentic
    /// chat and not a sub-agent's or a routine's, and nothing of the chat
    /// runs: a turn, a `/compact`, a background command or sub-agent, an
    /// approval) and records `exec_mode_changed`. Only to nodes whose hello
    /// listed [`CAP_EXEC_SWITCH`].
    #[serde(rename = "session.set_exec_mode")]
    SetExecMode {
        req_id: String,
        session_id: SessionId,
        mode: harness_core::policy::ExecMode,
    },
    /// Change the chat's step limit (X.14). A request only: the node
    /// clamps it to `1..=` its own `[limits] max_steps`, refuses it for a
    /// sub-agent's or a routine's chat, and records the result as
    /// `step_limit_changed`. Only to nodes whose hello listed `steps-max:*`.
    #[serde(rename = "session.set_step_limit")]
    SetStepLimit {
        req_id: String,
        session_id: SessionId,
        max_steps: i64,
        /// The master judged it a lowering from its copy of the log and so
        /// skipped the owner's level check: the node refuses it if it would
        /// in fact raise the chat's limit (`STEPS_NOT_A_LOWERING`; the copy
        /// was behind), and records nothing.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        lowering: bool,
    },
    #[serde(rename = "session.cancel")]
    Cancel {
        req_id: String,
        session_id: SessionId,
    },
    /// Stop one background command of a session (`bg_id`), or all of them.
    /// A turn's cancel does not stop them; this does (only to nodes with
    /// `CAP_BACKGROUND`).
    #[serde(rename = "session.stop_background")]
    StopBackground {
        req_id: String,
        session_id: SessionId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bg_id: Option<String>,
        by: StopBackgroundBy,
    },
    /// Undo the file changes of one finished turn (git checkpoint).
    #[serde(rename = "session.restore")]
    Restore {
        req_id: String,
        session_id: SessionId,
        turn_id: String,
        /// Overwrite files edited after the turn too.
        #[serde(default)]
        force: bool,
        /// Rewind: also undo every later turn (back to before this one).
        #[serde(default)]
        rewind: bool,
    },
    /// Delete a session and its log (cancels a running turn first).
    #[serde(rename = "session.delete")]
    Delete {
        req_id: String,
        session_id: SessionId,
    },
    #[serde(rename = "approval.respond")]
    ApprovalRespond {
        req_id: String,
        session_id: SessionId,
        approval_id: String,
        decision: Decision,
        /// The person's note with a denial; the node drops it with an
        /// allow and cuts it to `MAX_DENY_NOTE`. Only to nodes with
        /// `CAP_ANSWERS` (an older one ignores the field).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// Remove a call the person allowed for the rest of a chat
    /// (`chat_allow_removed`). Only to nodes with `CAP_ANSWERS`.
    #[serde(rename = "session.revoke_allow")]
    RevokeAllow {
        req_id: String,
        session_id: SessionId,
        grant_id: String,
    },
    #[serde(rename = "resume")]
    Resume {
        session_id: SessionId,
        last_seq: u64,
    },
    /// Read part of a staged file. Answered by one `artifact.chunk`, never
    /// by an ack (it is not in `req_id()`), and handled outside the node's
    /// command loop so a transfer never holds up a prompt.
    #[serde(rename = "artifact.fetch")]
    ArtifactFetch {
        req_id: String,
        session_id: SessionId,
        stage_id: String,
        offset: u64,
        /// At most `MAX_CHUNK`; the node clamps it.
        len: u32,
    },
    /// The master has the file (or gave up): the node deletes the stage.
    #[serde(rename = "artifact.release")]
    ArtifactRelease {
        req_id: String,
        session_id: SessionId,
        stage_id: String,
    },
    /// "Save as artifact": stage `path` of the session's folder. Acked once
    /// the node checked the path; the copy ends in `artifact_staged` or
    /// `artifact_stage_failed`.
    #[serde(rename = "artifact.stage")]
    ArtifactStage {
        req_id: String,
        session_id: SessionId,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// The answer to a `recall.request`: hits, or why there are none. Only
    /// to nodes whose hello listed `CAP_RECALL`; never acked.
    #[serde(rename = "recall.result")]
    RecallResult {
        req_id: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        hits: Vec<harness_core::recall::RecallHit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// The answer to a `board.request`: the board or what was changed, or
    /// why not. Only to nodes whose hello listed `CAP_BOARD`; never acked.
    #[serde(rename = "board.result")]
    BoardResult {
        req_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        answer: Option<harness_core::board::BoardAnswer>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// The answer to a `skill.request`: the stored skill's body (text), or
    /// why not. Only to nodes whose hello listed [`CAP_STORED_SKILLS`];
    /// never acked.
    #[serde(rename = "skill.result")]
    SkillResult {
        req_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// The answer to a `connector.request`: the tool's result as text
    /// (the server's own error marked `is_error`), or why the call was not
    /// made or failed. Only to nodes whose hello listed [`CAP_CONNECTORS`];
    /// never acked.
    #[serde(rename = "connector.result")]
    ConnectorResult {
        req_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        answer: Option<harness_core::connectors::ConnectorAnswer>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// What one turn changed (X.12): read from the turn's checkpoints,
    /// answered by one `session.diff_result`, never by an ack, handled
    /// outside the node's command loop, and nothing is logged. `rewind`:
    /// this turn and every later one (before this turn against the latest
    /// state, what a rewind would undo). Only to nodes with `CAP_DIFF`.
    #[serde(rename = "session.diff")]
    Diff {
        req_id: String,
        session_id: SessionId,
        turn_id: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        rewind: bool,
    },
    /// Paths of the chat's folder for the composer's `@` completion
    /// (PLAN.md T63): answered by one `session.files_result`, never by an
    /// ack, handled outside the node's command loop, nothing logged. Only
    /// to nodes with [`CAP_FILES`].
    #[serde(rename = "session.files")]
    Files {
        req_id: String,
        session_id: SessionId,
        query: String,
    },
    /// Make a new project's folder (ADOPTION_PLAN X.26, PLAN.md T77): a
    /// display name and nothing else. The node makes the folder's name
    /// from it (`<slug>-<4 random characters>`) inside the projects folder
    /// its node.toml names, makes it a git repository with one empty
    /// commit, and answers with one `project.created`, never an ack. No
    /// field names a place: a path sent beside it is an unknown field and
    /// has no effect. Only to nodes with [`CAP_PROJECTS`].
    #[serde(rename = "project.create")]
    ProjectCreate { req_id: String, name: String },
    /// Make a new project's folder by cloning a forge repository into it
    /// (ADOPTION_PLAN X.27 step 3, PLAN.md T86): the display name (the
    /// folder's name is made from it as for `project.create`), the forge
    /// host and the repository's `owner/name` (GitLab: `group/…/name`).
    /// The node clones only from a host its node.toml lists (`[forge]
    /// hosts`), builds the URL itself (`https://<host>/<full_name>.git`),
    /// checks both again by rules of its own, and answers with one
    /// `project.created`, never an ack. `username` and `token` only for a
    /// private repository: the token is handed to that one git process
    /// through a pipe and never written anywhere on the node. No field
    /// names a URL, a scheme or a place. Only to nodes with
    /// [`CAP_FORGE_CLONE_PREFIX`].
    #[serde(rename = "project.clone")]
    ProjectClone {
        req_id: String,
        name: String,
        host: String,
        full_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        username: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", with = "wire_secret")]
        token: Option<harness_core::Secret<String>>,
    },
    /// A person's request about a project folder's `.mcp.json` (X.36,
    /// PLAN.md T82): whose request it is, the project's folder (the node
    /// checks it as a chat's folder, `check_cwd`) and what they ask.
    /// Answered by one `project.mcp_result`, never an ack, outside the
    /// command loop; no session. The node refuses it unless its node.toml
    /// says `[mcp] project_servers = "ask"`. Only to nodes with
    /// [`CAP_PROJECT_MCP`], and only on a person's HTTP request: nothing a
    /// turn, a tool or a model does sends one.
    #[serde(rename = "project.mcp")]
    ProjectMcp {
        req_id: String,
        user_id: String,
        folder: String,
        request: ProjectMcpRequest,
    },
}

impl MasterToNode {
    pub fn req_id(&self) -> Option<&str> {
        match self {
            MasterToNode::Create { req_id, .. }
            | MasterToNode::Fork { req_id, .. }
            | MasterToNode::Prompt(PromptMsg { req_id, .. })
            | MasterToNode::Callback(CallbackMsg { req_id, .. })
            | MasterToNode::Retry(RetryMsg { req_id, .. })
            | MasterToNode::Rewind { req_id, .. }
            | MasterToNode::RewindUndo { req_id, .. }
            | MasterToNode::Pin { req_id, .. }
            | MasterToNode::Handoff { req_id, .. }
            | MasterToNode::Compact { req_id, .. }
            | MasterToNode::SetAgent { req_id, .. }
            | MasterToNode::SetApprovalMode { req_id, .. }
            | MasterToNode::SetStepLimit { req_id, .. }
            | MasterToNode::SetExecMode { req_id, .. }
            | MasterToNode::Cancel { req_id, .. }
            | MasterToNode::StopBackground { req_id, .. }
            | MasterToNode::Delete { req_id, .. }
            | MasterToNode::Restore { req_id, .. }
            | MasterToNode::SelectVariant { req_id, .. }
            | MasterToNode::ApprovalRespond { req_id, .. }
            | MasterToNode::RevokeAllow { req_id, .. }
            | MasterToNode::ArtifactRelease { req_id, .. }
            | MasterToNode::ArtifactStage { req_id, .. } => Some(req_id),
            // A fetch is answered by its chunk, never acked.
            MasterToNode::Welcome { .. }
            | MasterToNode::Resume { .. }
            | MasterToNode::ArtifactFetch { .. }
            | MasterToNode::RecallResult { .. }
            | MasterToNode::BoardResult { .. }
            | MasterToNode::SkillResult { .. }
            | MasterToNode::ConnectorResult { .. }
            | MasterToNode::Diff { .. }
            | MasterToNode::Files { .. }
            | MasterToNode::ProjectCreate { .. }
            | MasterToNode::ProjectClone { .. }
            | MasterToNode::ProjectMcp { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// X.34 (PLAN.md T80): whatever a node sends, the master keeps names
    /// by the node's own rules, each once, one clean description line, and
    /// at most the caps, counting the rest; the message names no path,
    /// command or environment.
    #[test]
    fn announced_plugins_are_bounded_and_clean() {
        let skill = |n: &str, d: &str| AnnouncedSkill {
            name: n.into(),
            description: d.into(),
        };
        let sent = NodePlugins {
            skills: vec![
                skill("deploy", "Ship\nit \u{202e}now\u{200b}"),
                skill("deploy", "again"),
                skill("d\u{0435}ploy", "Cyrillic e: a look-alike"),
                skill("bad name", "space"),
                skill("x\u{200b}", "hidden"),
                skill(&"a".repeat(65), "long"),
                skill("lint", &"é".repeat(500)),
            ],
            mcp_servers: vec![
                "github".into(),
                "GitHub".into(),
                "a__b".into(),
                "github".into(),
                "-x".into(),
            ],
            left_out: 3,
        };
        let got = sent.bounded();
        let names: Vec<&str> = got.skills.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["deploy", "lint"]);
        assert_eq!(got.skills[0].description, "Ship it now");
        assert_eq!(
            got.skills[1].description.chars().count(),
            MAX_ANNOUNCED_DESCRIPTION
        );
        assert_eq!(got.mcp_servers, ["github"]);
        assert_eq!(got.left_out, 3 + 5 + 4);
        // Past the count cap: named up to it, the rest counted.
        let many = NodePlugins {
            skills: (0..100_000).map(|i| skill(&format!("s{i}"), "d")).collect(),
            mcp_servers: (0..1000).map(|i| format!("m{i}")).collect(),
            left_out: u32::MAX,
        }
        .bounded();
        assert_eq!(many.skills.len(), MAX_ANNOUNCED_SKILLS);
        assert_eq!(many.mcp_servers.len(), MAX_ANNOUNCED_SERVERS);
        assert_eq!(many.left_out, u32::MAX, "saturates");
        // Past the byte cap.
        let big = NodePlugins {
            skills: (0..MAX_ANNOUNCED_SKILLS)
                .map(|i| skill(&format!("s{i:03}"), &"é".repeat(MAX_ANNOUNCED_DESCRIPTION)))
                .collect(),
            ..Default::default()
        }
        .bounded();
        let bytes: usize = big
            .skills
            .iter()
            .map(|s| s.name.len() + s.description.len())
            .sum();
        assert!(bytes <= MAX_ANNOUNCED_BYTES, "{bytes}");
        assert!(big.left_out > 0);
        let wire = serde_json::to_value(NodeToMaster::Plugins {
            plugins: NodePlugins {
                skills: vec![skill("a", "b")],
                mcp_servers: vec!["m".into()],
                left_out: 0,
            },
        })
        .unwrap();
        assert_eq!(
            wire,
            serde_json::json!({"type": "node.plugins", "plugins": {
                "skills": [{"name": "a", "description": "b"}], "mcp_servers": ["m"], "left_out": 0
            }})
        );
    }

    /// `project.create` carries a display name only (T77): its wire shape,
    /// never acked, and a place sent beside the name (`path`, `dir`, `cwd`)
    /// is dropped by the parse, so it cannot reach the node's handler.
    #[test]
    fn project_create_carries_a_name_only() {
        let m = MasterToNode::ProjectCreate {
            req_id: "r".into(),
            name: "Project Name Example".into(),
        };
        assert_eq!(
            serde_json::to_value(&m).unwrap(),
            json!({"type": "project.create", "req_id": "r", "name": "Project Name Example"})
        );
        assert_eq!(m.req_id(), None, "answered by project.created, never acked");
        let hostile = json!({"type": "project.create", "req_id": "r", "name": "x",
                             "path": "/etc", "dir": "/", "cwd": "/root"});
        assert_eq!(
            serde_json::from_value::<MasterToNode>(hostile).unwrap(),
            MasterToNode::ProjectCreate {
                req_id: "r".into(),
                name: "x".into()
            }
        );
        let a = NodeToMaster::ProjectCreated {
            req_id: "r".into(),
            path: Some("/home/me/rizzy-projects/x-ab1c".into()),
            error: None,
        };
        assert_eq!(
            serde_json::to_value(&a).unwrap(),
            json!({"type": "project.created", "req_id": "r",
                   "path": "/home/me/rizzy-projects/x-ab1c"})
        );
    }

    /// `project.clone` (T86): its wire shape, never acked; the token is on
    /// the wire only when sent, and never in `Debug`; a URL, a scheme or a
    /// path sent beside it is dropped by the parse.
    #[test]
    fn project_clone_carries_names_and_a_redacted_token() {
        const CANARY: &str = "ghp_CANARYwire000000000000000000000001";
        let m = MasterToNode::ProjectClone {
            req_id: "r".into(),
            name: "Harness".into(),
            host: "github.com".into(),
            full_name: "ada/harness".into(),
            username: Some("ada".into()),
            token: Some(harness_core::Secret::new(CANARY.into())),
        };
        assert_eq!(m.req_id(), None, "answered by project.created, never acked");
        assert!(!format!("{m:?}").contains(CANARY), "{m:?}");
        assert_eq!(
            serde_json::to_value(&m).unwrap(),
            json!({"type": "project.clone", "req_id": "r", "name": "Harness",
                   "host": "github.com", "full_name": "ada/harness",
                   "username": "ada", "token": CANARY})
        );
        let back: MasterToNode = serde_json::from_value(serde_json::to_value(&m).unwrap()).unwrap();
        assert_eq!(back, m);
        let public = MasterToNode::ProjectClone {
            req_id: "r".into(),
            name: "x".into(),
            host: "gitlab.com".into(),
            full_name: "g/s/p".into(),
            username: None,
            token: None,
        };
        let v = serde_json::to_value(&public).unwrap();
        assert!(
            v.get("token").is_none() && v.get("username").is_none(),
            "{v}"
        );
        let hostile = json!({"type": "project.clone", "req_id": "r", "name": "x",
                             "host": "github.com", "full_name": "a/b",
                             "url": "file:///etc", "scheme": "ext", "path": "/root"});
        assert!(matches!(
            serde_json::from_value::<MasterToNode>(hostile).unwrap(),
            MasterToNode::ProjectClone { token: None, .. }
        ));
        assert_eq!(
            clone_timeout_of(&["x".into(), "forge-clone:600".into()]),
            Some(600)
        );
        for bad in [
            "forge-clone:",
            "forge-clone:-1",
            "forge-clone:1e3",
            "forge-clone:9999999",
        ] {
            assert_eq!(clone_timeout_of(&[bad.into()]), None, "{bad}");
        }
        // Review of PR 163, L2: a node cannot make the master wait longer
        // than the most any node may be set to.
        assert_eq!(clone_timeout_of(&["forge-clone:999999".into()]), Some(7200));
        assert_eq!(clone_timeout_of(&["forge-clone:7201".into()]), Some(7200));
        assert_eq!(clone_timeout_of(&["forge-clone:7200".into()]), Some(7200));
    }

    /// `recall.request` carries words only: a field naming a user, a
    /// workspace or a chat does not parse, so a node cannot widen the scope
    /// the master takes from the session (PLAN.md T55).
    #[test]
    fn recall_messages_carry_words_only() {
        let sid: SessionId = "0193f2a0-0000-7000-8000-000000000001".parse().unwrap();
        let r = NodeToMaster::Recall {
            req_id: "r".into(),
            session_id: sid,
            query: harness_core::recall::RecallQuery {
                query: "plan".into(),
                scope: harness_core::recall::RecallIn::Chats,
                limit: Some(3),
            },
        };
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"type": "recall.request", "req_id": "r", "session_id": sid,
                   "query": {"query": "plan", "in": "chats", "limit": 3}})
        );
        for extra in ["user_id", "owner", "workspace_id", "session_id", "chats"] {
            let v = json!({"type": "recall.request", "req_id": "r", "session_id": sid,
                           "query": {"query": "plan", extra: "x"}});
            assert!(
                serde_json::from_value::<NodeToMaster>(v).is_err(),
                "{extra}"
            );
        }
        let a = MasterToNode::RecallResult {
            req_id: "r".into(),
            hits: vec![],
            error: Some("no".into()),
        };
        assert_eq!(
            serde_json::to_value(&a).unwrap(),
            json!({"type": "recall.result", "req_id": "r", "error": "no"})
        );
        assert_eq!(a.req_id(), None, "never acked");
    }

    /// `board.request` carries what to do only: a field naming a project,
    /// a workspace, a user or a chat does not parse, so a node cannot point
    /// the master at another board (PLAN.md T78).
    #[test]
    fn board_messages_name_no_place() {
        use harness_core::board::{BoardAnswer, BoardRequest};
        let sid: SessionId = "0193f2a0-0000-7000-8000-000000000001".parse().unwrap();
        let r = NodeToMaster::Board {
            req_id: "r".into(),
            session_id: sid,
            request: BoardRequest::Note {
                card_id: "c".into(),
                text: "done".into(),
            },
        };
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"type": "board.request", "req_id": "r", "session_id": sid,
                   "request": {"action": "note", "card_id": "c", "text": "done"}})
        );
        for extra in ["project_id", "workspace_id", "user_id", "owner"] {
            let v = json!({"type": "board.request", "req_id": "r", "session_id": sid,
                           "request": {"action": "list", extra: "x"}});
            assert!(
                serde_json::from_value::<NodeToMaster>(v).is_err(),
                "{extra}"
            );
        }
        let a = MasterToNode::BoardResult {
            req_id: "r".into(),
            answer: Some(BoardAnswer::Done {
                card_id: "c".into(),
                said: "noted".into(),
            }),
            error: None,
        };
        assert_eq!(
            serde_json::to_value(&a).unwrap(),
            json!({"type": "board.result", "req_id": "r",
                   "answer": {"kind": "done", "card_id": "c", "said": "noted"}})
        );
        assert_eq!(a.req_id(), None, "never acked");
    }

    /// `skill.request` carries a name only: the master reads the workspace,
    /// the agent and the person from the session (PLAN.md T81). A field
    /// naming any of them is not read, and the answer is text or a reason.
    #[test]
    fn project_mcp_messages_name_no_program() {
        // X.36: an approval names a server and a digest, never a command;
        // a command, an argument or an environment sent beside it is not
        // kept.
        let v = json!({"type": "project.mcp", "req_id": "r", "user_id": "01A", "folder": "/p",
                       "request": {"op": "approve", "name": "s", "digest": "d",
                                   "command": "sh", "args": ["-c", "x"], "env": {"A": "1"}}});
        assert_eq!(
            serde_json::from_value::<MasterToNode>(v).unwrap(),
            MasterToNode::ProjectMcp {
                req_id: "r".into(),
                user_id: "01A".into(),
                folder: "/p".into(),
                request: ProjectMcpRequest::Approve {
                    name: "s".into(),
                    digest: "d".into()
                },
            }
        );
        assert!(
            serde_json::from_value::<MasterToNode>(
                json!({"type": "project.mcp", "req_id": "r", "user_id": "u", "folder": "/p",
                       "request": {"op": "install", "command": "sh"}})
            )
            .is_err()
        );
        // An entry shows its env values: part of what runs.
        let e = ProjectMcpEntry {
            name: "s".into(),
            state: ProjectMcpState::New,
            command: "npx".into(),
            args: vec![],
            env: vec![ProjectMcpEnv {
                name: "NODE_OPTIONS".into(),
                value: "--require ./x.js".into(),
            }],
            digest: None,
            reason: None,
            approved_command: None,
            approved_args: None,
            approved_env: None,
        };
        let text = serde_json::to_string(&e).unwrap();
        assert!(text.contains("--require ./x.js"), "{text}");
        for (id, ok) in [
            ("01JD8Z", true),
            ("a-b_c", true),
            ("", false),
            ("a b", false),
            ("a\nb", false),
            ("é", false),
        ] {
            assert_eq!(valid_user_id(id), ok, "{id:?}");
        }
        assert!(!valid_user_id(&"a".repeat(MAX_USER_ID + 1)));
    }

    #[test]
    fn skill_messages_name_no_place() {
        let sid: SessionId = "0193f2a0-0000-7000-8000-000000000001".parse().unwrap();
        let r = NodeToMaster::Skill {
            req_id: "r".into(),
            session_id: sid,
            name: "release-notes".into(),
        };
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"type": "skill.request", "req_id": "r", "session_id": sid,
                   "name": "release-notes"})
        );
        let v = json!({"type": "skill.request", "req_id": "r", "session_id": sid,
                       "name": "x", "workspace_id": "w", "agent_id": "a", "user_id": "u"});
        assert_eq!(
            serde_json::from_value::<NodeToMaster>(v).unwrap(),
            NodeToMaster::Skill {
                req_id: "r".into(),
                session_id: sid,
                name: "x".into()
            },
            "nothing else is kept"
        );
        let a = MasterToNode::SkillResult {
            req_id: "r".into(),
            body: Some("Do it.".into()),
            error: None,
        };
        assert_eq!(
            serde_json::to_value(&a).unwrap(),
            json!({"type": "skill.result", "req_id": "r", "body": "Do it."})
        );
        assert_eq!(a.req_id(), None, "never acked");
    }

    #[test]
    fn connector_messages_name_no_place() {
        let sid: SessionId = "0193f2a0-0000-7000-8000-000000000001".parse().unwrap();
        let r = NodeToMaster::Connector {
            req_id: "r".into(),
            session_id: sid,
            tool: "mcp__linear__create_issue".into(),
            arguments: json!({"title": "x"}),
        };
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"type": "connector.request", "req_id": "r", "session_id": sid,
                   "tool": "mcp__linear__create_issue", "arguments": {"title": "x"}})
        );
        let v = json!({"type": "connector.request", "req_id": "r", "session_id": sid,
                       "tool": "mcp__linear__x", "arguments": {}, "workspace_id": "w",
                       "user_id": "u", "url": "https://evil.example/", "token": "t"});
        assert_eq!(
            serde_json::from_value::<NodeToMaster>(v).unwrap(),
            NodeToMaster::Connector {
                req_id: "r".into(),
                session_id: sid,
                tool: "mcp__linear__x".into(),
                arguments: json!({}),
            },
            "nothing else is kept"
        );
        let a = MasterToNode::ConnectorResult {
            req_id: "r".into(),
            answer: Some(harness_core::connectors::ConnectorAnswer {
                text: "done".into(),
                is_error: false,
            }),
            error: None,
        };
        assert_eq!(
            serde_json::to_value(&a).unwrap(),
            json!({"type": "connector.result", "req_id": "r", "answer": {"text": "done"}})
        );
        assert_eq!(a.req_id(), None, "never acked");
        assert_eq!(
            serde_json::to_value(NodeToMaster::ConnectorCancel { req_id: "r".into() }).unwrap(),
            json!({"type": "connector.cancel", "req_id": "r"})
        );
    }

    #[test]
    fn wire_shapes_are_stable() {
        let sid: SessionId = "0193f2a0-0000-7000-8000-000000000001".parse().unwrap();
        let m = MasterToNode::Resume {
            session_id: sid,
            last_seq: 7,
        };
        assert_eq!(
            serde_json::to_value(&m).unwrap(),
            json!({"type": "resume", "session_id": "0193f2a0-0000-7000-8000-000000000001", "last_seq": 7})
        );
        let a = NodeToMaster::Ack {
            req_id: "r".into(),
            ok: true,
            error: None,
        };
        assert_eq!(
            serde_json::to_value(&a).unwrap(),
            json!({"type": "ack", "req_id": "r", "ok": true})
        );
        let d = MasterToNode::ApprovalRespond {
            req_id: "r".into(),
            session_id: sid,
            approval_id: "a".into(),
            decision: Decision::AllowSession,
            note: None,
        };
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["type"], "approval.respond");
        assert_eq!(v["decision"], "allow_session");
        assert_eq!(serde_json::from_value::<MasterToNode>(v).unwrap(), d);
        let m = MasterToNode::SetApprovalMode {
            lowering: false,
            req_id: "r".into(),
            session_id: sid,
            mode: ApprovalMode::AcceptEdits,
        };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(
            v,
            json!({"type": "session.set_approval_mode", "req_id": "r",
                   "session_id": "0193f2a0-0000-7000-8000-000000000001", "mode": "accept_edits"})
        );
        assert_eq!(m.req_id(), Some("r"));
        assert_eq!(serde_json::from_value::<MasterToNode>(v).unwrap(), m);
        let m = MasterToNode::SetExecMode {
            req_id: "r".into(),
            session_id: sid,
            mode: harness_core::policy::ExecMode::Host,
        };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(
            v,
            json!({"type": "session.set_exec_mode", "req_id": "r",
                   "session_id": "0193f2a0-0000-7000-8000-000000000001", "mode": "host"})
        );
        assert_eq!(m.req_id(), Some("r"));
        assert_eq!(serde_json::from_value::<MasterToNode>(v).unwrap(), m);
        let m = MasterToNode::SetStepLimit {
            req_id: "r".into(),
            session_id: sid,
            max_steps: 30,
            lowering: false,
        };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["type"], "session.set_step_limit");
        assert_eq!(v["max_steps"], 30);
        assert_eq!(serde_json::from_value::<MasterToNode>(v).unwrap(), m);
    }

    #[test]
    fn artifact_messages_wire_shapes() {
        let sid: SessionId = "0193f2a0-0000-7000-8000-000000000001".parse().unwrap();
        let fetch = MasterToNode::ArtifactFetch {
            req_id: "r1".into(),
            session_id: sid,
            stage_id: "s".into(),
            offset: 262_144,
            len: MAX_CHUNK,
        };
        assert_eq!(
            serde_json::to_value(&fetch).unwrap(),
            json!({"type": "artifact.fetch", "req_id": "r1",
                   "session_id": "0193f2a0-0000-7000-8000-000000000001",
                   "stage_id": "s", "offset": 262_144, "len": 262_144})
        );
        // Answered by a chunk, never by an automatic ack.
        assert_eq!(fetch.req_id(), None);
        let release = MasterToNode::ArtifactRelease {
            req_id: "r2".into(),
            session_id: sid,
            stage_id: "s".into(),
        };
        assert_eq!(release.req_id(), Some("r2"));
        assert_eq!(
            serde_json::to_value(&release).unwrap()["type"],
            "artifact.release"
        );
        let stage = MasterToNode::ArtifactStage {
            req_id: "r3".into(),
            session_id: sid,
            path: "target/release/app".into(),
            name: None,
        };
        assert_eq!(stage.req_id(), Some("r3"));
        let v = serde_json::to_value(&stage).unwrap();
        assert_eq!(v["type"], "artifact.stage");
        assert!(v.get("name").is_none(), "{v}");
        assert_eq!(serde_json::from_value::<MasterToNode>(v).unwrap(), stage);
        let chunk = NodeToMaster::ArtifactChunk {
            req_id: "r1".into(),
            stage_id: "s".into(),
            offset: 0,
            data: "aGk=".into(),
            eof: true,
            error: None,
        };
        assert_eq!(
            serde_json::to_value(&chunk).unwrap(),
            json!({"type": "artifact.chunk", "req_id": "r1", "stage_id": "s",
                   "offset": 0, "data": "aGk=", "eof": true})
        );
        let busy: NodeToMaster = serde_json::from_value(json!({"type": "artifact.chunk",
            "req_id": "r", "stage_id": "s", "offset": 5, "eof": false, "error": "busy"}))
        .unwrap();
        assert!(
            matches!(busy, NodeToMaster::ArtifactChunk { data, error: Some(e), .. }
            if data.is_empty() && e == "busy")
        );
    }

    /// X.12: `session.diff` is answered by its result, never acked; the
    /// result carries the diff or the node's words.
    #[test]
    fn diff_messages_wire_shapes() {
        let sid: SessionId = "0193f2a0-0000-7000-8000-000000000001".parse().unwrap();
        let ask = MasterToNode::Diff {
            req_id: "r".into(),
            session_id: sid,
            turn_id: "t1".into(),
            rewind: false,
        };
        assert_eq!(ask.req_id(), None);
        let v = serde_json::to_value(&ask).unwrap();
        assert_eq!(
            v,
            json!({"type": "session.diff", "req_id": "r",
                   "session_id": "0193f2a0-0000-7000-8000-000000000001", "turn_id": "t1"})
        );
        assert_eq!(serde_json::from_value::<MasterToNode>(v).unwrap(), ask);
        let diff = TurnDiff {
            files: vec![DiffFile {
                path: "a.txt".into(),
                old_path: None,
                status: DiffStatus::Modified,
                content: DiffContent::Text,
                old_mode: Some("100644".into()),
                new_mode: Some("100644".into()),
                old_size: Some(4),
                new_size: Some(8),
                added: 1,
                removed: 0,
                hunks: vec![DiffHunk {
                    old_start: 1,
                    old_lines: 1,
                    new_start: 1,
                    new_lines: 2,
                    section: String::new(),
                    lines: vec![" one".into(), "+two".into()],
                }],
                cut: false,
            }],
            ..Default::default()
        };
        let ans = NodeToMaster::DiffResult {
            req_id: "r".into(),
            diff: Some(Box::new(diff)),
            error: None,
        };
        let v = serde_json::to_value(&ans).unwrap();
        assert_eq!(v["type"], "session.diff_result");
        assert_eq!(v["diff"]["files"][0]["status"], "modified");
        assert_eq!(v["diff"]["files"][0]["hunks"][0]["lines"][1], "+two");
        assert!(v["diff"].get("cut").is_none(), "{v}");
        assert_eq!(serde_json::from_value::<NodeToMaster>(v).unwrap(), ans);
    }

    #[test]
    fn prompt_images_wire_shape() {
        let sid: SessionId = "0193f2a0-0000-7000-8000-000000000001".parse().unwrap();
        let img = ChatImage {
            media_id: "m".into(),
            sha256: "ab".repeat(32),
            mime: "image/png".into(),
            name: "a.png".into(),
            width: 2,
            height: 1,
            data: "aGk=".into(),
        };
        let p = MasterToNode::Prompt(PromptMsg {
            req_id: "r".into(),
            images: vec![img.clone()],
            ..PromptMsg::test(sid, "look")
        });
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(
            v["images"],
            json!([{"media_id": "m", "sha256": "ab".repeat(32), "mime": "image/png",
                    "name": "a.png", "width": 2, "height": 1, "data": "aGk="}])
        );
        assert_eq!(serde_json::from_value::<MasterToNode>(v).unwrap(), p);
        let r = img.image_ref();
        assert_eq!((r.media_id.as_str(), r.width, r.height), ("m", 2, 1));
    }

    #[test]
    fn prompt_reply_roundtrips_and_is_omitted_when_absent() {
        let sid: SessionId = "0193f2a0-0000-7000-8000-000000000001".parse().unwrap();
        let prompt = |reply| {
            MasterToNode::Prompt(PromptMsg {
                req_id: "r".into(),
                reply,
                ..PromptMsg::test(sid, String::new())
            })
        };
        let v = serde_json::to_value(prompt(None)).unwrap();
        assert!(v.get("reply").is_none(), "{v}");
        assert!(v.get("images").is_none(), "{v}");
        let p = prompt(Some(harness_core::event::FormReply {
            message_id: "m1".into(),
            block_id: "db".into(),
            values: json!({"selected": ["pg"]}).as_object().unwrap().clone(),
        }));
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(
            v["reply"],
            json!({"message_id": "m1", "block_id": "db", "values": {"selected": ["pg"]}})
        );
        assert_eq!(serde_json::from_value::<MasterToNode>(v).unwrap(), p);
    }

    /// T.8d step 3: `session.callback` carries ids and the agent, nothing a
    /// person writes; `session.create` says a chat takes no callbacks only
    /// when it is so.
    #[test]
    fn callback_and_no_callbacks_wire_shapes() {
        // A busy chat's refusal is one that finds nothing to resume now.
        assert!(CALLBACK_BUSY.starts_with(CALLBACK_NOTHING));
        let sid: SessionId = "0193f2a0-0000-7000-8000-000000000001".parse().unwrap();
        let c = MasterToNode::Callback(CallbackMsg::test(sid, vec!["bg-1".into(), "bg-2".into()]));
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(
            v,
            json!({"type": "session.callback", "req_id": "r",
                   "session_id": "0193f2a0-0000-7000-8000-000000000001", "bg_ids": ["bg-1", "bg-2"]})
        );
        assert_eq!(c.req_id(), Some("r"));
        assert_eq!(serde_json::from_value::<MasterToNode>(v).unwrap(), c);
        // Text, attachments or context are not fields of it: ignored.
        let extra: MasterToNode = serde_json::from_value(json!({"type": "session.callback",
            "req_id": "r", "session_id": sid, "bg_ids": ["bg-1"], "text": "do it"}))
        .unwrap();
        assert!(matches!(extra, MasterToNode::Callback(_)));
        let create = |no_callbacks| MasterToNode::Create {
            exec_mode: None,
            req_id: "r".into(),
            session_id: sid,
            title: "t".into(),
            cwd: "/w".into(),
            model: None,
            approval_mode: None,
            profile: None,
            agent: None,
            incognito: false,
            no_callbacks,
        };
        assert!(
            serde_json::to_value(create(false))
                .unwrap()
                .get("no_callbacks")
                .is_none()
        );
        assert_eq!(
            serde_json::to_value(create(true)).unwrap()["no_callbacks"],
            true
        );
    }
}
