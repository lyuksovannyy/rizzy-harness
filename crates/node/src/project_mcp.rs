//! A repository's own MCP servers (ADOPTION_PLAN X.36, PLAN.md T82).
//!
//! **Off unless node.toml says so** (`[mcp] project_servers = "ask"`):
//! without it nothing here exists on the runtime, no `.mcp.json` is read
//! and every `project.mcp` is refused.
//!
//! **A person approves each server, for their own chats.** A project
//! folder's `.mcp.json` (the standard shape: `{"mcpServers": {"<name>":
//! {"command", "args", "env"}}}`) is read only on a person's request (the
//! project page's list, its Reload, an Approve or a Remove: `project.mcp`)
//! and once when a chat's table is built (the chat is made, or loaded at
//! the node's start) if its owner approved something for its folder. An
//! approval is keyed by (the person's user id, the folder's real path, the
//! server's name) and holds the node's own copy of the entry, written to
//! [`crate::policy::McpPolicy::approvals_file`] in the node's data
//! directory (0600; no chat's file view or shell reaches it). It is taken
//! only if the file's entry still has the digest the person was shown.
//!
//! **What runs is the approved copy, never the file's text.** Before each
//! turn the chat's table is given its owner's approved copies for its
//! folder ([`ProjectMcp::servers_for`]), less those whose name was gone
//! from the file at the node's last read of it (removing narrows, so it
//! counts at once); a changed entry runs its approved copy until the new
//! one is approved; a new entry runs nothing; an unreadable file (a link,
//! a FIFO, a folder, over the cap, not JSON) changes nothing. Never in a
//! `host` chat. Every rule of node.toml's servers holds (PLAN.md T47): the
//! chat's own confinement, the caps, the idle stop, the approval of calls.
//!
//! **What the person sees is what runs.** An entry is approvable only if
//! every string of it (the command, each argument, each `env` name and
//! value) can be shown whole and unchanged: at most [`MAX_SHOWN_CHARS`]
//! characters, the whole entry at most [`MAX_SHOWN_ENTRY`] bytes, no
//! control character or line break of any kind, no hidden or direction
//! character, no space at either end and no run of spaces a screen would
//! fold. The list carries those strings in full (`env` values included:
//! they are part of what runs, `NODE_OPTIONS` or `LD_PRELOAD` turn a
//! harmless command into repository code), and the digest the click must
//! match is over exactly them.
//!
//! **Environment.** A project server gets less than the chat's `bash`:
//! the core allow-list only (no `env_pass`, no `env_vars`: like a
//! node.toml server without `env_vars`), and its entry's own `env` values,
//! which reach only the server's own process: the node writes them to a
//! private file the guard hands to bubblewrap as a descriptor, and
//! `sandbox-exec` sets them after it confined itself, right before the
//! `exec` (`ServerConfig::env_inside`); they are in no command line and
//! not on the guard, bubblewrap or `sandbox-exec` itself. They are
//! repository content the chat's agent can read anyway, so they join no
//! scrubber; they are never written to an event, a log or an audit line by
//! the harness, and only the asking person's answer carries them.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use harness_core::secret::Secret;
use harness_protocol::{
    ProjectMcpEntry, ProjectMcpEnv, ProjectMcpList, ProjectMcpRequest, ProjectMcpState,
    valid_user_id,
};
use harness_tools::mcp::{EnvValues, ServerConfig};
use nix::fcntl::OFlag;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::policy::NodePolicy;

/// The file read in a project's folder.
pub const FILE: &str = ".mcp.json";
/// The largest `.mcp.json` read, in bytes; a larger one is unreadable.
pub const MAX_FILE: u64 = 64 * 1024;
/// Servers one file may declare; more make it unreadable.
pub const MAX_ENTRIES: usize = 32;
/// The largest entry (its JSON), in bytes; a larger one is unsupported.
pub const MAX_ENTRY: usize = 8 * 1024;
/// The longest string of an approvable entry (its command, an argument, an
/// `env` name or value), in characters, and its strings together, in
/// bytes: so a whole entry fits a screen and is shown uncut.
pub const MAX_SHOWN_CHARS: usize = 512;
pub const MAX_SHOWN_ENTRY: usize = 4 * 1024;
/// Arguments, and environment names, one entry may have.
pub const MAX_ARGS: usize = 64;
pub const MAX_ENV: usize = 64;
/// Approvals this node keeps, all people and folders together, and per
/// person and folder.
pub const MAX_APPROVALS: usize = 1_000;
pub const MAX_PER_FOLDER: usize = MAX_ENTRIES;
/// Requests handled at once on this node; one more is refused.
pub const AT_ONCE: usize = 2;
/// How long a request may take (a folder on a stalled mount): then it is
/// answered as not read in time.
pub const WAIT: std::time::Duration = std::time::Duration::from_secs(10);
/// How long building a chat's table waits for the read of its file.
pub const LOOK_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// The refusal of every request on a node whose node.toml keeps it off.
pub const OFF: &str = "this node does not run a repository's MCP servers: its node.toml has \
     no [mcp] project_servers = \"ask\"";
/// The refusal on a node that offers no sandboxed chats.
pub const HOST_ONLY: &str = "this node runs only host chats, and a repository's MCP servers \
     never run in one: they run only in sandboxed chats";
/// What a `host` chat is told when its owner approved servers here.
pub const HOST_NOTE: &str = "The repository's approved MCP servers (.mcp.json) are not \
     started in this chat: it runs on the host, and they run only in sandboxed chats";

/// One server of a `.mcp.json` as the node runs it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Entry")
            .field("command", &self.command)
            .field("args", &self.args)
            .field("env", &self.env.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl Entry {
    /// Hex sha256 of the entry as the node keeps it (fields in order,
    /// `env` sorted): what the person was shown, bound to their click.
    pub fn digest(&self) -> String {
        use sha2::Digest;
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        hex::encode(sha2::Sha256::digest(&bytes))
    }

    /// The server as a chat's table starts it: `env` values only inside
    /// the sandbox, nothing of node.toml's `env_vars`, the default
    /// timeouts.
    pub fn config(&self) -> ServerConfig {
        let args: Vec<&str> = self.args.iter().map(String::as_str).collect();
        let mut c = ServerConfig::new(self.command.clone(), &args);
        c.env = EnvValues(
            self.env
                .iter()
                .map(|(k, v)| (k.clone(), Secret::new(v.clone())))
                .collect(),
        );
        c.env_inside = true;
        c
    }

    fn env_shown(&self) -> Vec<ProjectMcpEnv> {
        self.env
            .iter()
            .map(|(k, v)| ProjectMcpEnv {
                name: k.clone(),
                value: v.clone(),
            })
            .collect()
    }
}

/// One entry of the file: the server, or why it can never be approved,
/// and what the person is shown either way.
#[derive(Debug, Clone)]
pub struct Parsed {
    pub entry: Result<Entry, String>,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<ProjectMcpEnv>,
}

/// What a read of a folder's `.mcp.json` found.
#[derive(Debug, Clone)]
pub enum FileRead {
    Missing,
    Unreadable(String),
    /// Every entry by its name as the file has it.
    Entries(BTreeMap<String, Parsed>),
}

impl FileRead {
    /// The names in the file, when the read says (none for a missing one).
    fn names(&self) -> Option<BTreeSet<String>> {
        match self {
            FileRead::Missing => Some(BTreeSet::new()),
            FileRead::Unreadable(_) => None,
            FileRead::Entries(m) => Some(m.keys().cloned().collect()),
        }
    }
}

/// Read `folder`'s `.mcp.json`: the folder opened without following a
/// link and checked to be the path given (canonical), the file opened
/// relative to it with `O_NOFOLLOW | O_NONBLOCK` (a link, a FIFO or a
/// device is never followed or waited on), checked to be a regular file
/// on the descriptor, read through `take(MAX_FILE + 1)`.
pub fn read_file(folder: &Path) -> FileRead {
    let dir = match nix::fcntl::open(
        folder,
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    ) {
        Ok(d) => d,
        Err(e) => {
            return FileRead::Unreadable(format!(
                "the folder could not be opened: {}",
                std::io::Error::from(e)
            ));
        }
    };
    match std::fs::read_link(format!("/proc/self/fd/{}", dir.as_raw_fd())) {
        Ok(real) if real == folder => {}
        _ => {
            return FileRead::Unreadable(
                "the folder leads elsewhere than its path (a folder on its way is a link)".into(),
            );
        }
    }
    let fd = match nix::fcntl::openat(
        &dir,
        FILE,
        OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    ) {
        Ok(f) => f,
        Err(nix::errno::Errno::ENOENT) => return FileRead::Missing,
        Err(nix::errno::Errno::ELOOP) => {
            return FileRead::Unreadable("it is a link, which the node does not follow".into());
        }
        Err(e) => {
            return FileRead::Unreadable(format!(
                "it could not be opened: {}",
                std::io::Error::from(e)
            ));
        }
    };
    let f = std::fs::File::from(fd);
    match f.metadata() {
        Ok(m) if m.is_file() => {}
        Ok(_) => return FileRead::Unreadable("it is not a regular file".into()),
        Err(e) => return FileRead::Unreadable(format!("it could not be read: {e}")),
    }
    let mut bytes = Vec::new();
    if let Err(e) = f.take(MAX_FILE + 1).read_to_end(&mut bytes) {
        return FileRead::Unreadable(format!("it could not be read: {e}"));
    }
    if bytes.len() as u64 > MAX_FILE {
        return FileRead::Unreadable(format!(
            "it is larger than {} KiB, which the node does not read",
            MAX_FILE / 1024
        ));
    }
    parse(&bytes)
}

/// The standard shape, checked.
pub fn parse(bytes: &[u8]) -> FileRead {
    let v: Value = match serde_json::from_slice(bytes) {
        Ok(v) => v,
        Err(e) => return FileRead::Unreadable(format!("it is not JSON: {e}")),
    };
    let Some(servers) = v.get("mcpServers") else {
        return FileRead::Unreadable("it has no \"mcpServers\" object".into());
    };
    let Some(servers) = servers.as_object() else {
        return FileRead::Unreadable("\"mcpServers\" is not an object".into());
    };
    if servers.len() > MAX_ENTRIES {
        return FileRead::Unreadable(format!(
            "it declares {} servers; the node reads a file of at most {MAX_ENTRIES}",
            servers.len()
        ));
    }
    FileRead::Entries(
        servers
            .iter()
            .map(|(name, v)| (name.clone(), parse_entry(name, v)))
            .collect(),
    )
}

/// How a string of an entry that cannot be approved is shown: printable
/// ASCII, every other character as its `\u{…}` escape, cut with a mark,
/// so it never passes [`harness_protocol::MAX_SHOWN`]. An approvable
/// entry's strings are shown as they are (see [`unshowable`]).
fn shown(s: &str) -> String {
    escaped(s, 500, "...[cut]")
}

/// A short excerpt of `s` for a reason or a name: [`shown`]'s escapes, at
/// most 64 characters of it.
fn excerpt(s: &str) -> String {
    escaped(s, 64, "...")
}

fn escaped(s: &str, keep: usize, mark: &str) -> String {
    let out: String = s
        .chars()
        .flat_map(|c| {
            if (' '..='~').contains(&c) {
                vec![c]
            } else {
                c.escape_unicode().collect::<Vec<_>>()
            }
        })
        .collect();
    if out.chars().count() <= keep {
        out
    } else {
        let mut cut: String = out.chars().take(keep).collect();
        cut.push_str(mark);
        cut
    }
}

/// Why `s` would not be shown whole and unchanged, if it would not. An
/// approvable string is printable ASCII (0x21 to 0x7E) and single spaces
/// only (review of PR 150, NEW-1: invisible, combining and look-alike
/// characters draw the same as others), at most
/// [`MAX_SHOWN_CHARS`] characters, with no space at either end. The
/// first other character is named by its code point, never printed.
pub fn unshowable(s: &str) -> Option<String> {
    if s.chars().count() > MAX_SHOWN_CHARS {
        return Some("longer than the 512 characters the page shows whole".into());
    }
    if let Some(c) = harness_protocol::first_unshowable(s) {
        return Some(format!(
            "it holds {}: only printable ASCII and single spaces are shown exactly as they run",
            harness_protocol::code_point(c)
        ));
    }
    if s.contains("  ") {
        return Some("a run of spaces a screen would fold".into());
    }
    if s.starts_with(' ') || s.ends_with(' ') {
        return Some("a space at its start or end, which a screen would not show".into());
    }
    None
}

/// Words for the person that may name a path: one line, at most 480
/// characters (well inside [`harness_protocol::MAX_SHOWN`]), cut with a mark.
fn words(s: &str) -> String {
    let line: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if line.chars().count() <= 480 {
        line
    } else {
        let mut cut: String = line.chars().take(477).collect();
        cut.push_str("...");
        cut
    }
}

fn valid_env_name(k: &str) -> bool {
    let mut b = k.bytes();
    b.next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
        && b.all(|c| c.is_ascii_alphanumeric() || c == b'_')
        && k.len() <= 128
}

fn parse_entry(name: &str, v: &Value) -> Parsed {
    let s = |v: Option<&Value>| v.and_then(Value::as_str).unwrap_or_default().to_string();
    let command = s(v.get("command"));
    let args: Vec<String> = v
        .get("args")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|x| x.as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default();
    let entry = check_entry(name, v);
    // An approvable entry is shown exactly as it runs; any other one as
    // escaped, cut text.
    if let Ok(e) = &entry {
        return Parsed {
            command: e.command.clone(),
            args: e.args.clone(),
            env: e.env_shown(),
            entry,
        };
    }
    // At most 64 parts, the last naming how many more there were.
    const KEEP: usize = harness_protocol::MAX_LISTED_PARTS - 1;
    let mut env: Vec<ProjectMcpEnv> = v
        .get("env")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .take(KEEP)
                .map(|(k, x)| ProjectMcpEnv {
                    name: excerpt(k),
                    value: shown(x.as_str().unwrap_or_default()),
                })
                .collect()
        })
        .unwrap_or_default();
    let env_total = v
        .get("env")
        .and_then(Value::as_object)
        .map_or(0, |m| m.len());
    if env_total > KEEP {
        env.push(ProjectMcpEnv {
            name: "...".into(),
            value: format!("[{} more]", env_total - KEEP),
        });
    }
    let mut shown_args: Vec<String> = args.iter().take(KEEP).map(|a| shown(a)).collect();
    if args.len() > KEEP {
        shown_args.push(format!("...[{} more]", args.len() - KEEP));
    }
    Parsed {
        entry,
        command: shown(&command),
        args: shown_args,
        env,
    }
}

fn check_entry(name: &str, v: &Value) -> Result<Entry, String> {
    if !harness_core::agent::valid_mcp_server_name(name) {
        return Err(
            "not supported: a server name is 1 to 32 of a-z, 0-9, `-` and `_`, starts \
             and ends with a letter or digit and holds no `__` (it becomes part of tool names)"
                .into(),
        );
    }
    let Some(obj) = v.as_object() else {
        return Err("not supported: the entry is not an object".into());
    };
    if serde_json::to_vec(v).map_or(usize::MAX, |b| b.len()) > MAX_ENTRY {
        return Err(format!(
            "not supported: the entry is larger than {} KiB",
            MAX_ENTRY / 1024
        ));
    }
    let kind = obj.get("type").and_then(Value::as_str);
    if obj.contains_key("url")
        || matches!(
            kind,
            Some("http" | "sse" | "streamable-http" | "streamableHttp" | "ws")
        )
    {
        return Err(
            "not supported: remote (a server reached over the network); only programs \
             the node starts are"
                .into(),
        );
    }
    match obj.get("type") {
        None => {}
        Some(Value::String(t)) if t == "stdio" => {}
        Some(_) => return Err("not supported: a \"type\" other than \"stdio\"".into()),
    }
    for k in obj.keys() {
        if !matches!(k.as_str(), "type" | "command" | "args" | "env") {
            return Err(format!(
                "not supported: it uses \"{}\", which the node does not read (only command, \
                 args and env)",
                excerpt(k)
            ));
        }
    }
    let Some(Value::String(command)) = obj.get("command") else {
        return Err("not supported: it has no \"command\" string".into());
    };
    let mut args = Vec::new();
    match obj.get("args") {
        None => {}
        Some(Value::Array(a)) if a.len() <= MAX_ARGS => {
            for x in a {
                let Value::String(x) = x else {
                    return Err("not supported: an argument that is not a string".into());
                };
                args.push(x.clone());
            }
        }
        Some(Value::Array(_)) => {
            return Err(format!("not supported: more than {MAX_ARGS} arguments"));
        }
        Some(_) => return Err("not supported: \"args\" is not a list".into()),
    }
    let mut env = BTreeMap::new();
    match obj.get("env") {
        None => {}
        Some(Value::Object(m)) if m.len() <= MAX_ENV => {
            for (k, x) in m {
                let Value::String(x) = x else {
                    return Err("not supported: an \"env\" value that is not a string".into());
                };
                if !valid_env_name(k) {
                    return Err(format!(
                        "not supported: the env name \"{}\" is not a variable name",
                        excerpt(k)
                    ));
                }
                if harness_tools::env::DENY_PREFIXES
                    .iter()
                    .any(|p| k.starts_with(p))
                {
                    return Err(format!(
                        "not supported: the env name {k:?} is the harness's own (HARNESS_*)"
                    ));
                }
                env.insert(k.clone(), x.clone());
            }
        }
        Some(Value::Object(_)) => {
            return Err(format!("not supported: more than {MAX_ENV} env names"));
        }
        Some(_) => return Err("not supported: \"env\" is not an object".into()),
    }
    let all = std::iter::once(command.as_str())
        .chain(args.iter().map(String::as_str))
        .chain(env.values().map(String::as_str));
    for t in all.clone() {
        if t.contains("${") {
            return Err(
                "not supported: uses a variable (`${...}`), which the node never expands \
                 from its own environment"
                    .into(),
            );
        }
    }
    // What is shown is what runs: every string whole and unchanged.
    let mut bytes = 0usize;
    let parts = std::iter::once(("the command", command.as_str()))
        .chain(args.iter().map(|a| ("an argument", a.as_str())))
        .chain(env.keys().map(|k| ("an env name", k.as_str())))
        .chain(env.values().map(|x| ("an env value", x.as_str())));
    for (what, t) in parts {
        if let Some(why) = unshowable(t) {
            return Err(format!(
                "not supported: {what} cannot be shown whole and unchanged ({why})"
            ));
        }
        bytes += t.len();
    }
    if bytes > MAX_SHOWN_ENTRY {
        return Err(format!(
            "not supported: the entry is larger than the {} KiB the page shows whole",
            MAX_SHOWN_ENTRY / 1024
        ));
    }
    if command.is_empty() {
        return Err("not supported: the command is empty".into());
    }
    if args.iter().any(String::is_empty) {
        return Err("not supported: an empty argument, which a screen would not show".into());
    }
    Ok(Entry {
        command: command.clone(),
        args,
        env,
    })
}

/// (user id, the folder's real path, the server's name).
type Key = (String, String, String);

#[derive(Serialize, Deserialize)]
struct Saved {
    approvals: Vec<SavedApproval>,
}

#[derive(Serialize, Deserialize)]
struct SavedApproval {
    user: String,
    folder: String,
    name: String,
    entry: Entry,
}

/// What a request did, for the node's audit line.
pub(crate) struct Outcome {
    pub list: Result<ProjectMcpList, String>,
    /// `("approve" | "remove", name)` when an approval was taken or removed.
    pub changed: Option<(&'static str, String)>,
}

/// The node's approvals and what it last read of each folder's file.
pub(crate) struct ProjectMcp {
    file: PathBuf,
    approvals: Mutex<BTreeMap<Key, Entry>>,
    /// The names each folder's file held at the node's last read of it
    /// (a person's request, a chat's table being built); unknown until
    /// then, and unchanged by an unreadable file.
    seen: Mutex<HashMap<PathBuf, BTreeSet<String>>>,
    pub(crate) busy: std::sync::Arc<tokio::sync::Semaphore>,
    /// Why the approvals file could not be read at start, if it could not:
    /// then nothing is approved or removed (nothing would be written over
    /// what it held) until it is fixed and the node restarted.
    store_error: Option<String>,
}

fn folder_key(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

impl ProjectMcp {
    /// The store at `file`, as written before. A file that cannot be used
    /// (not a regular file, not the node user's own with mode 0600, not
    /// what the node writes) fails closed and loud: none of it is taken,
    /// the log says why, and approve and remove are refused naming the
    /// file, so nothing is written over it.
    pub(crate) fn open(file: PathBuf) -> Self {
        let mut approvals = BTreeMap::new();
        let mut store_error = None;
        match read_store(&file) {
            Ok(Some(saved)) => {
                for a in saved.approvals.into_iter().take(MAX_APPROVALS) {
                    approvals.insert((a.user, a.folder, a.name), a.entry);
                }
            }
            Ok(None) => {}
            Err(e) => {
                tracing::error!(
                    file = %file.display(),
                    error = %e,
                    "project MCP approvals cannot be used; none are taken, and none are approved or removed until the file is fixed and the node restarted"
                );
                store_error = Some(format!(
                    "the node's approvals file {} cannot be used ({e}): nothing is approved or \
                     removed until it is fixed or moved away and the node restarted",
                    file.display()
                ));
            }
        }
        Self {
            file,
            approvals: Mutex::new(approvals),
            seen: Mutex::new(HashMap::new()),
            busy: std::sync::Arc::new(tokio::sync::Semaphore::new(AT_ONCE)),
            store_error,
        }
    }

    fn approved_in(&self, user: &str, folder: &str) -> BTreeMap<String, Entry> {
        self.approvals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .filter(|((u, f, _), _)| u == user && f == folder)
            .map(|((_, _, n), e)| (n.clone(), e.clone()))
            .collect()
    }

    /// Whether `user` approved anything for `folder`.
    pub(crate) fn has_approvals(&self, user: &str, folder: &Path) -> bool {
        let f = folder_key(folder);
        self.approvals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .keys()
            .any(|(u, ff, _)| u == user && *ff == f)
    }

    fn note_read(&self, folder: &Path, read: &FileRead) {
        if let Some(names) = read.names() {
            self.seen
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(folder.to_path_buf(), names);
        }
    }

    /// Read `folder`'s file now, for the names it holds (a chat's table
    /// being built). Blocking.
    pub(crate) fn look(&self, folder: &Path) {
        let read = read_file(folder);
        self.note_read(folder, &read);
    }

    /// The servers a chat of `owner` in `folder` starts from now on, and
    /// what to tell it: `owner`'s approved copies for that folder, less a
    /// name the file did not hold at the node's last read of it, less a
    /// name node.toml has (node.toml wins); none in a `host` chat. Nothing
    /// is read here.
    pub(crate) fn servers_for(
        &self,
        policy: &NodePolicy,
        owner: Option<&str>,
        folder: &Path,
        exec_mode: Option<harness_core::policy::ExecMode>,
    ) -> (BTreeMap<String, ServerConfig>, Vec<String>) {
        let Some(owner) = owner else {
            return Default::default();
        };
        let approved = self.approved_in(owner, &folder_key(folder));
        if approved.is_empty() {
            return Default::default();
        }
        if exec_mode != Some(harness_core::policy::ExecMode::Sandbox) {
            return (BTreeMap::new(), vec![HOST_NOTE.to_string()]);
        }
        let present = self
            .seen
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(folder)
            .cloned();
        let mut notes = Vec::new();
        let mut out = BTreeMap::new();
        for (name, e) in approved {
            if present.as_ref().is_some_and(|p| !p.contains(&name)) {
                continue;
            }
            if policy.mcp_servers.contains_key(&name) {
                notes.push(format!(
                    "The repository's approved MCP server `{name}` is not started: the node's \
                     own node.toml has a server of that name, which is the one used"
                ));
                continue;
            }
            out.insert(name, e.config());
        }
        (out, notes)
    }

    /// A person's request (`project.mcp`), handled whole: blocking. The
    /// list it answers with always passes [`ProjectMcpList::showable`]:
    /// the note and the file's state (which can name a long path) are cut
    /// to a bound, so one long path never makes the master refuse the
    /// person's list.
    pub(crate) fn handle(
        &self,
        policy: &NodePolicy,
        user: &str,
        folder: &str,
        req: &ProjectMcpRequest,
    ) -> Outcome {
        let mut out = self.handle_whole(policy, user, folder, req);
        if let Ok(l) = &mut out.list {
            l.note = l.note.take().map(|n| words(&n));
            l.unreadable = l.unreadable.take().map(|n| words(&n));
        }
        out
    }

    fn handle_whole(
        &self,
        policy: &NodePolicy,
        user: &str,
        folder: &str,
        req: &ProjectMcpRequest,
    ) -> Outcome {
        let refuse = |e: String| Outcome {
            list: Err(e),
            changed: None,
        };
        if !valid_user_id(user) {
            return refuse("the request names no valid account".into());
        }
        if policy
            .exec
            .check_session(Some(harness_core::policy::ExecMode::Sandbox))
            .is_err()
        {
            return refuse(HOST_ONLY.into());
        }
        let real = policy.check_cwd(folder);
        if let ProjectMcpRequest::Remove { name } = req {
            // Always allowed: by the real path, or by the path as stored
            // when the folder cannot be resolved any more.
            let key = real
                .as_deref()
                .map_or_else(|_| folder.to_string(), folder_key);
            let (removed, note) = match self.remove(user, &key, name) {
                Ok(true) => (true, None),
                Ok(false) => (false, Some(format!("you had no approval of {name} here"))),
                Err(e) => (false, Some(e)),
            };
            let changed = removed.then(|| ("remove", name.clone()));
            return match real {
                Ok(p) => {
                    let read = read_file(&p);
                    self.note_read(&p, &read);
                    Outcome {
                        list: Ok(self.list(policy, user, &p, &read, note)),
                        changed,
                    }
                }
                Err(e) => Outcome {
                    list: Ok(ProjectMcpList {
                        unreadable: Some(e),
                        note: Some(
                            note.unwrap_or_else(|| format!("removed your approval of {name}")),
                        ),
                        ..Default::default()
                    }),
                    changed,
                },
            };
        }
        let real = match real {
            Ok(p) => p,
            Err(e) => return refuse(e),
        };
        let read = read_file(&real);
        self.note_read(&real, &read);
        let mut note = None;
        let mut changed = None;
        if let ProjectMcpRequest::Approve { name, digest } = req {
            match self.approve(policy, user, &real, &read, name, digest) {
                Ok(()) => changed = Some(("approve", name.clone())),
                Err(e) => note = Some(e),
            }
        }
        Outcome {
            list: Ok(self.list(policy, user, &real, &read, note)),
            changed,
        }
    }

    fn approve(
        &self,
        policy: &NodePolicy,
        user: &str,
        folder: &Path,
        read: &FileRead,
        name: &str,
        digest: &str,
    ) -> Result<(), String> {
        const RELOAD: &str = "changed: the file's entry is not the one you were shown; reload \
             and look again";
        if let Some(e) = &self.store_error {
            return Err(format!("not approved: {e}"));
        }
        let FileRead::Entries(entries) = read else {
            return Err(match read {
                FileRead::Unreadable(why) => format!("not approved: the file {why}"),
                _ => "not approved: there is no .mcp.json in the folder".into(),
            });
        };
        let Some(parsed) = entries.get(name) else {
            return Err(format!(
                "not approved: the file has no server \"{}\"",
                excerpt(name)
            ));
        };
        let entry = match &parsed.entry {
            Ok(e) => e,
            Err(why) => return Err(format!("not approved: {why}")),
        };
        if policy.mcp_servers.contains_key(name) {
            return Err("not approved: the node's own node.toml has a server of that name".into());
        }
        if entry.digest() != digest {
            return Err(RELOAD.into());
        }
        let f = folder_key(folder);
        let key = (user.to_string(), f.clone(), name.to_string());
        let mut all = self.approvals.lock().unwrap_or_else(|p| p.into_inner());
        if !all.contains_key(&key) {
            let here = all
                .keys()
                .filter(|(u, ff, _)| u == user && *ff == f)
                .count();
            if here >= MAX_PER_FOLDER || all.len() >= MAX_APPROVALS {
                return Err(format!(
                    "not approved: this node keeps at most {MAX_PER_FOLDER} approvals per person \
                     and folder and {MAX_APPROVALS} in all"
                ));
            }
        }
        let before = all.insert(key.clone(), entry.clone());
        if let Err(e) = write_store(&self.file, &all) {
            match before {
                Some(b) => all.insert(key, b),
                None => all.remove(&key),
            };
            return Err(format!("not approved: the node could not save it: {e}"));
        }
        Ok(())
    }

    /// Remove an approval: `Ok(false)` when there was none; an `Err` (the
    /// store cannot be used, or the write failed) leaves it in place.
    fn remove(&self, user: &str, folder: &str, name: &str) -> Result<bool, String> {
        let mut all = self.approvals.lock().unwrap_or_else(|p| p.into_inner());
        let key = (user.to_string(), folder.to_string(), name.to_string());
        if let Some(e) = &self.store_error {
            return Err(format!("not removed: {e}"));
        }
        if !all.contains_key(&key) {
            return Ok(false);
        }
        let old = all.remove(&key);
        if let Err(e) = write_store(&self.file, &all) {
            if let Some(old) = old {
                all.insert(key, old);
            }
            return Err(format!(
                "not removed: the node could not save the change to {}: {e}; the approval \
                 stays",
                self.file.display()
            ));
        }
        Ok(true)
    }

    fn list(
        &self,
        policy: &NodePolicy,
        user: &str,
        folder: &Path,
        read: &FileRead,
        note: Option<String>,
    ) -> ProjectMcpList {
        let approved = self.approved_in(user, &folder_key(folder));
        let mut out = ProjectMcpList {
            note,
            ..Default::default()
        };
        let shown_entry = |name: &str, state, e: &Entry| ProjectMcpEntry {
            name: name.to_string(),
            state,
            command: e.command.clone(),
            args: e.args.clone(),
            env: e.env_shown(),
            digest: None,
            reason: None,
            approved_command: None,
            approved_args: None,
            approved_env: None,
        };
        match read {
            FileRead::Unreadable(why) => {
                out.unreadable = Some(why.clone());
                for (name, e) in &approved {
                    out.entries
                        .push(shown_entry(name, ProjectMcpState::Approved, e));
                }
                return out;
            }
            FileRead::Missing => out.missing = true,
            FileRead::Entries(entries) => {
                for (name, p) in entries {
                    let mut x = ProjectMcpEntry {
                        name: excerpt(name),
                        state: ProjectMcpState::New,
                        command: p.command.clone(),
                        args: p.args.clone(),
                        env: p.env.clone(),
                        digest: None,
                        reason: None,
                        approved_command: None,
                        approved_args: None,
                        approved_env: None,
                    };
                    match &p.entry {
                        _ if harness_core::agent::valid_mcp_server_name(name)
                            && policy.mcp_servers.contains_key(name) =>
                        {
                            x.state = ProjectMcpState::Hidden;
                            x.reason = Some(
                                "hidden by the node's own server of this name (node.toml), \
                                 which is the one used"
                                    .into(),
                            );
                        }
                        Err(why) => {
                            x.state = ProjectMcpState::Unsupported;
                            x.reason = Some(why.clone());
                        }
                        Ok(e) => match approved.get(name) {
                            None => x.digest = Some(e.digest()),
                            Some(a) if a == e => x.state = ProjectMcpState::Approved,
                            Some(a) => {
                                x.state = ProjectMcpState::Changed;
                                x.digest = Some(e.digest());
                                x.approved_command = Some(a.command.clone());
                                x.approved_args = Some(a.args.clone());
                                x.approved_env = Some(a.env_shown());
                            }
                        },
                    }
                    // An approved entry the file now spoils (a variable, a
                    // field): its approved copy still runs.
                    if x.state == ProjectMcpState::Unsupported
                        && let Some(a) = approved.get(name)
                    {
                        x.state = ProjectMcpState::Changed;
                        x.approved_command = Some(a.command.clone());
                        x.approved_args = Some(a.args.clone());
                        x.approved_env = Some(a.env_shown());
                    }
                    out.entries.push(x);
                }
            }
        }
        let present = read.names().unwrap_or_default();
        for (name, e) in &approved {
            if !present.contains(name) {
                out.entries
                    .push(shown_entry(name, ProjectMcpState::Removed, e));
            }
        }
        out
    }
}

fn read_store(file: &Path) -> Result<Option<Saved>, String> {
    let f = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags((OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC).bits())
        .open(file)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let m = f.metadata().map_err(|e| e.to_string())?;
    if !m.is_file() {
        return Err("it is not a regular file".into());
    }
    if m.uid() != nix::unistd::geteuid().as_raw() || m.mode() & 0o077 != 0 {
        return Err("it is not the node user's own file with mode 0600".into());
    }
    let mut bytes = Vec::new();
    f.take(64 << 20)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| e.to_string())
}

/// Write the approvals: a new 0600 file beside it, synced, renamed over.
fn write_store(file: &Path, all: &BTreeMap<Key, Entry>) -> Result<(), String> {
    use std::io::Write;
    let saved = Saved {
        approvals: all
            .iter()
            .map(|((user, folder, name), entry)| SavedApproval {
                user: user.clone(),
                folder: folder.clone(),
                name: name.clone(),
                entry: entry.clone(),
            })
            .collect(),
    };
    let bytes = serde_json::to_vec(&saved).map_err(|e| e.to_string())?;
    let dir = file.parent().ok_or("the approvals file has no folder")?;
    let tmp = dir.join(format!(
        ".{}.{}",
        file.file_name().unwrap_or_default().to_string_lossy(),
        harness_core::new_id()
    ));
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags((OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC).bits())
        .open(&tmp)
        .map_err(|e| format!("{}: {e}", tmp.display()))?;
    let done = f
        .write_all(&bytes)
        .and_then(|()| f.sync_all())
        .and_then(|()| std::fs::rename(&tmp, file));
    if let Err(e) = done {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(json: &str) -> BTreeMap<String, Parsed> {
        match parse(json.as_bytes()) {
            FileRead::Entries(m) => m,
            other => panic!("{other:?}"),
        }
    }

    fn why(json: &str, name: &str) -> String {
        entries(json)[name].entry.clone().unwrap_err()
    }

    #[test]
    fn the_standard_shape_is_read_and_everything_else_is_named() {
        let m = entries(
            r#"{"mcpServers": {"fs": {"command": "npx", "args": ["-y", "srv"], "env": {"TOKEN": "t"}},
                "s": {"type": "stdio", "command": "x"}}}"#,
        );
        let fs = m["fs"].entry.clone().unwrap();
        assert_eq!(fs.command, "npx");
        assert_eq!(fs.args, ["-y", "srv"]);
        assert_eq!(
            m["fs"].env,
            [ProjectMcpEnv {
                name: "TOKEN".into(),
                value: "t".into()
            }]
        );
        assert!(m["s"].entry.is_ok());
        assert!(
            why(
                r#"{"mcpServers": {"r": {"type": "http", "url": "https://x"}}}"#,
                "r"
            )
            .contains("remote")
        );
        assert!(why(r#"{"mcpServers": {"r": {"url": "https://x"}}}"#, "r").contains("remote"));
        for v in [
            r#"{"mcpServers": {"v": {"command": "${HOME}/x"}}}"#,
            r#"{"mcpServers": {"v": {"command": "x", "args": ["${ANY}"]}}}"#,
            r#"{"mcpServers": {"v": {"command": "x", "env": {"A": "${HOME}"}}}}"#,
        ] {
            assert!(why(v, "v").contains("uses a variable"), "{v}");
        }
        assert!(why(r#"{"mcpServers": {"Up": {"command": "x"}}}"#, "Up").contains("server name"));
        assert!(
            why(r#"{"mcpServers": {"a__b": {"command": "x"}}}"#, "a__b").contains("server name")
        );
        assert!(
            why(
                r#"{"mcpServers": {"a": {"command": "x", "cwd": "/"}}}"#,
                "a"
            )
            .contains("\"cwd\"")
        );
        assert!(why(r#"{"mcpServers": {"a": {"command": "x", "env": {"HARNESS_GUARD_STDIN_FIFO": "/f"}}}}"#, "a").contains("HARNESS_"));
        assert!(
            why(
                r#"{"mcpServers": {"a": {"command": "x", "env": {"A=B": "1"}}}}"#,
                "a"
            )
            .contains("variable name")
        );
        assert!(why(r#"{"mcpServers": {"a": {"command": "x\u0000"}}}"#, "a").contains("U+0000"));
        assert!(
            why(
                r#"{"mcpServers": {"a": {"command": "x", "args": ["\u202e"]}}}"#,
                "a"
            )
            .contains("U+202E")
        );
        let many = format!(
            r#"{{"mcpServers": {{"a": {{"command": "x", "args": [{}]}}}}}}"#,
            vec!["\"a\""; MAX_ARGS + 1].join(",")
        );
        assert!(why(&many, "a").contains("arguments"));
        let big = format!(
            r#"{{"mcpServers": {{"a": {{"command": "x", "args": ["{}"]}}}}}}"#,
            "a".repeat(MAX_ENTRY)
        );
        assert!(why(&big, "a").contains("larger"));
        // Unreadable shapes.
        for bad in ["[]", "{}", r#"{"mcpServers": []}"#, "not json"] {
            assert!(
                matches!(parse(bad.as_bytes()), FileRead::Unreadable(_)),
                "{bad}"
            );
        }
        let flood: String = (0..10_000)
            .map(|i| format!(r#""s{i}": {{"command": "x"}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let FileRead::Unreadable(w) = parse(format!(r#"{{"mcpServers": {{{flood}}}}}"#).as_bytes())
        else {
            panic!("10,000 entries were read")
        };
        assert!(w.contains("at most 32"), "{w}");
    }

    #[test]
    fn a_digest_binds_every_part_of_the_entry() {
        let e = Entry {
            command: "x".into(),
            args: vec!["a".into()],
            env: [("K".to_string(), "v".to_string())].into(),
        };
        let mut other = e.clone();
        other.env.insert("K".into(), "w".into());
        assert_ne!(e.digest(), other.digest());
        let mut other = e.clone();
        other.args.push("b".into());
        assert_ne!(e.digest(), other.digest());
        assert_eq!(e.digest(), e.clone().digest());
        assert!(!format!("{e:?}").contains("\"v\""));
        let c = e.config();
        assert!(c.env_inside && c.env_vars.is_empty());
    }

    #[test]
    fn links_fifos_folders_and_huge_files_are_unreadable() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        assert!(matches!(read_file(&root), FileRead::Missing));
        std::fs::write(root.join("real.json"), r#"{"mcpServers": {}}"#).unwrap();
        std::os::unix::fs::symlink(root.join("real.json"), root.join(FILE)).unwrap();
        let FileRead::Unreadable(w) = read_file(&root) else {
            panic!()
        };
        assert!(w.contains("link"), "{w}");
        std::fs::remove_file(root.join(FILE)).unwrap();
        nix::unistd::mkfifo(&root.join(FILE), nix::sys::stat::Mode::S_IRWXU).unwrap();
        let FileRead::Unreadable(w) = read_file(&root) else {
            panic!()
        };
        assert!(w.contains("regular"), "{w}");
        std::fs::remove_file(root.join(FILE)).unwrap();
        std::fs::create_dir(root.join(FILE)).unwrap();
        assert!(matches!(read_file(&root), FileRead::Unreadable(_)));
        std::fs::remove_dir(root.join(FILE)).unwrap();
        std::fs::write(root.join(FILE), vec![b' '; 10 << 20]).unwrap();
        let FileRead::Unreadable(w) = read_file(&root) else {
            panic!()
        };
        assert!(w.contains("larger"), "{w}");
        // The folder itself through a link: not read.
        std::fs::remove_file(root.join(FILE)).unwrap();
        std::fs::create_dir(root.join("inner")).unwrap();
        std::fs::write(root.join("inner").join(FILE), r#"{"mcpServers": {}}"#).unwrap();
        std::os::unix::fs::symlink(root.join("inner"), root.join("via")).unwrap();
        assert!(matches!(
            read_file(&root.join("via")),
            FileRead::Unreadable(_)
        ));
    }

    #[test]
    fn the_store_is_private_and_survives_a_restart() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("project-mcp.json");
        let mut all = BTreeMap::new();
        all.insert(
            ("u".to_string(), "/f".to_string(), "s".to_string()),
            Entry {
                command: "x".into(),
                args: vec![],
                env: [("K".to_string(), "secret-value-1".to_string())].into(),
            },
        );
        write_store(&file, &all).unwrap();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let pm = ProjectMcp::open(file.clone());
        assert!(pm.has_approvals("u", Path::new("/f")));
        assert!(!pm.has_approvals("v", Path::new("/f")));
        // A loosened file is not taken.
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let pm = ProjectMcp::open(file);
        assert!(!pm.has_approvals("u", Path::new("/f")));
    }

    /// A node with `[mcp] project_servers = "ask"` rooted at `root`.
    fn policy(root: &Path) -> NodePolicy {
        NodePolicy {
            workspace_roots: vec![root.to_path_buf()],
            ..Default::default()
        }
    }

    fn entry_json(command: &str, args: &[&str], env: &[(&str, &str)]) -> String {
        let env: serde_json::Map<String, Value> = env
            .iter()
            .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
            .collect();
        serde_json::json!({"mcpServers": {"s": {"command": command, "args": args, "env": env}}})
            .to_string()
    }

    /// Review of PR 150 (LOW): an approvals file the node cannot use fails
    /// closed and loud: approve and remove are refused naming the file,
    /// and nothing is written over it.
    #[test]
    fn an_unusable_store_refuses_approve_and_remove_and_is_not_overwritten() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        let ws = root.join("ws");
        std::fs::create_dir(&ws).unwrap();
        std::fs::write(ws.join(FILE), entry_json("python3", &["srv.py"], &[])).unwrap();
        let file = root.join("project-mcp.json");
        let mut all = BTreeMap::new();
        all.insert(
            ("u".to_string(), folder_key(&ws), "old".to_string()),
            Entry {
                command: "x".into(),
                args: vec![],
                env: BTreeMap::new(),
            },
        );
        write_store(&file, &all).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let before = std::fs::read(&file).unwrap();
        let pm = ProjectMcp::open(file.clone());
        let p = policy(&root);
        let folder = ws.display().to_string();
        let FileRead::Entries(m) = read_file(&ws) else {
            panic!()
        };
        let digest = m["s"].entry.clone().unwrap().digest();
        let o = pm.handle(
            &p,
            "u",
            &folder,
            &ProjectMcpRequest::Approve {
                name: "s".into(),
                digest,
            },
        );
        let note = o.list.unwrap().note.unwrap();
        assert!(
            note.contains("cannot be used") && note.contains("project-mcp.json"),
            "{note}"
        );
        assert!(o.changed.is_none());
        let o = pm.handle(
            &p,
            "u",
            &folder,
            &ProjectMcpRequest::Remove { name: "old".into() },
        );
        assert!(o.changed.is_none());
        let note = o.list.unwrap().note.unwrap();
        assert!(
            note.contains("not removed") && note.contains("project-mcp.json"),
            "{note}"
        );
        assert_eq!(
            std::fs::read(&file).unwrap(),
            before,
            "the store was written over"
        );
    }

    /// Review of PR 150 (LOW): a remove whose write fails is reported, and
    /// the approval stays (in memory and on disk alike).
    #[test]
    fn a_remove_that_cannot_be_saved_keeps_the_approval() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        let (ws, data) = (root.join("ws"), root.join("data"));
        std::fs::create_dir(&ws).unwrap();
        std::fs::create_dir(&data).unwrap();
        std::fs::write(ws.join(FILE), entry_json("python3", &["srv.py"], &[])).unwrap();
        let pm = ProjectMcp::open(data.join("project-mcp.json"));
        let p = policy(&root);
        let folder = ws.display().to_string();
        let l = pm
            .handle(&p, "u", &folder, &ProjectMcpRequest::List)
            .list
            .unwrap();
        let digest = l.entries[0].digest.clone().unwrap();
        let o = pm.handle(
            &p,
            "u",
            &folder,
            &ProjectMcpRequest::Approve {
                name: "s".into(),
                digest,
            },
        );
        assert!(o.changed.is_some());
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o500)).unwrap();
        let o = pm.handle(
            &p,
            "u",
            &folder,
            &ProjectMcpRequest::Remove { name: "s".into() },
        );
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700)).unwrap();
        let note = o.list.unwrap().note.unwrap();
        assert!(note.contains("not removed"), "{note}");
        assert!(o.changed.is_none());
        assert!(
            pm.has_approvals("u", &ws),
            "the approval went from memory alone"
        );
    }

    /// Review of PR 150 (HIGH 2): what the person is shown is what is
    /// hashed and run, or the entry cannot be approved. The reviewer's
    /// entry (a command line padded so its tail falls past what the page
    /// showed) is unsupported; every class of string a screen would not
    /// show whole and unchanged is too; an entry at the limits is shown
    /// whole and its digest is that of the strings shown.
    #[test]
    fn only_what_is_shown_whole_can_be_approved() {
        let padded = format!(
            "echo starting a safe server{}; touch /tmp/pwned; exec python3 x",
            " ".repeat(3000)
        );
        let parse_one = |json: String| match parse(json.as_bytes()) {
            FileRead::Entries(m) => m["s"].clone(),
            other => panic!("{other:?}"),
        };
        let p = parse_one(entry_json("sh", &["-c", &padded], &[]));
        assert!(p.entry.is_err(), "the padded entry can be approved");
        assert!(
            p.args[1].ends_with("...[cut]"),
            "a cut string is not marked"
        );
        let mut cases: Vec<(String, String)> = vec![
            ("longer".into(), "a".repeat(MAX_SHOWN_CHARS + 1)),
            ("run of spaces".into(), "a  b".to_string()),
            ("start or end".into(), " a".to_string()),
            ("start or end".into(), "a ".to_string()),
        ];
        // Every character outside printable ASCII is refused, named by its
        // code point: line breaks, controls, hidden and direction
        // characters, and the invisible and look-alike ones of the review
        // of PR 150 (NEW-1).
        for c in [
            '\n',
            '\r',
            '\u{0B}',
            '\u{0C}',
            '\u{85}',
            '\u{2028}',
            '\u{2029}',
            '\t',
            '\u{0}',
            '\u{7F}',
            '\u{202E}',
            '\u{200B}',
            '\u{AD}',
            '\u{34F}',
            '\u{115F}',
            '\u{1160}',
            '\u{3164}',
            '\u{FFA0}',
            '\u{2800}',
            '\u{180E}',
            '\u{180B}',
            '\u{FE00}',
            '\u{FE0F}',
            '\u{17B4}',
            '\u{FFF9}',
            '\u{1BCA0}',
            '\u{A0}',
            '\u{3000}',
            '\u{301}',
            '\u{440}',
            '\u{443}',
            '\u{E9}',
        ] {
            cases.push((
                harness_protocol::code_point(c),
                format!("--require ./saf{c}e.js"),
            ));
        }
        cases.push(("U+0301".into(), format!("safe{}.js", "\u{301}".repeat(20))));
        cases.push(("U+0440".into(), "\u{440}\u{443}thon3".into()));
        for (what, bad) in &cases {
            for json in [
                entry_json(bad, &[], &[]),
                entry_json("x", &[bad], &[]),
                entry_json("x", &[], &[("K", bad)]),
            ] {
                let p = parse_one(json);
                let why = p.entry.unwrap_err();
                assert!(why.contains(what.as_str()), "{bad:?}: {why}");
                // Named, never printed: the reason and the shown strings
                // are printable ASCII.
                for t in std::iter::once(&why)
                    .chain(std::iter::once(&p.command))
                    .chain(p.args.iter())
                    .chain(p.env.iter().map(|e| &e.value))
                {
                    assert_eq!(harness_protocol::first_unshowable(t), None, "{t:?}");
                }
            }
        }
        assert!(
            parse_one(entry_json("x", &[""], &[]))
                .entry
                .unwrap_err()
                .contains("empty")
        );
        // At the limits: 512-character strings, about 4 KiB in all.
        let long = "b".repeat(MAX_SHOWN_CHARS);
        let args: Vec<&str> = (0..6).map(|_| long.as_str()).collect();
        let p = parse_one(entry_json("python3", &args, &[("NODE_OPTIONS", &long)]));
        let e = p.entry.clone().unwrap();
        assert_eq!(p.command, e.command);
        assert_eq!(p.args, e.args);
        assert_eq!(p.env, e.env_shown());
        let shown = Entry {
            command: p.command.clone(),
            args: p.args.clone(),
            env: p
                .env
                .iter()
                .map(|x| (x.name.clone(), x.value.clone()))
                .collect(),
        };
        assert_eq!(shown.digest(), e.digest());
        assert_eq!(shown.config().args, e.args, "what runs is what is shown");
        // One more argument passes the whole-entry limit.
        let args: Vec<&str> = (0..8).map(|_| long.as_str()).collect();
        let why = parse_one(entry_json("python3", &args, &[]))
            .entry
            .unwrap_err();
        assert!(why.contains("4 KiB"), "{why}");
    }

    /// Review of PR 150 (NEW-2): one bad entry never blanks the list. The
    /// worst reason of every refusal, and every shown string of an entry
    /// that cannot be approved, stay within what the master accepts, so a
    /// list with them and with an approved entry passes `showable` whole.
    #[test]
    fn the_worst_entry_never_blanks_the_list() {
        let huge = "k".repeat(10_000);
        let wide = "\u{1BCA0}".repeat(3_000);
        let mut servers = serde_json::Map::new();
        let mut add = |name: &str, v: Value| {
            servers.insert(name.to_string(), v);
        };
        add(
            "good",
            serde_json::json!({"command": "python3", "args": ["srv.py"]}),
        );
        add(&huge, serde_json::json!({"command": "x"}));
        add(&wide, serde_json::json!({"command": "x"}));
        // Keys that pass the 8 KiB entry cap, so the reason that names
        // them is the one built (re-check NEW-2: 499 characters).
        for (n, key) in [
            ("unknown-499", "k".repeat(499)),
            ("unknown-1500", "k".repeat(1500)),
            ("unknown-wide", "\u{1BCA0}".repeat(1500)),
        ] {
            let mut v = serde_json::json!({"command": "x"});
            v[key.as_str()] = serde_json::json!(1);
            add(n, v);
            add(
                &format!("{n}-env"),
                serde_json::json!({"command": "x", "env": {key.as_str(): "v"}}),
            );
        }
        let mut unknown = serde_json::json!({"command": "x"});
        unknown[huge.as_str()] = serde_json::json!(1);
        add("unknown-long", unknown);
        let mut unknown = serde_json::json!({"command": "x"});
        unknown[wide.as_str()] = serde_json::json!(1);
        add("unknown-wide", unknown);
        add("type", serde_json::json!({"type": huge, "command": "x"}));
        add("remote", serde_json::json!({"url": huge}));
        add(
            "env-name",
            serde_json::json!({"command": "x", "env": {huge.as_str(): "v"}}),
        );
        add(
            "env-wide",
            serde_json::json!({"command": "x", "env": {wide.as_str(): "v"}}),
        );
        add(
            "harness",
            serde_json::json!({"command": "x", "env": {"HARNESS_X": "v"}}),
        );
        add(
            "env-value",
            serde_json::json!({"command": "x", "env": {"A": wide}}),
        );
        add(
            "var",
            serde_json::json!({"command": format!("${{{huge}}}")}),
        );
        add(
            "cmd-wide",
            serde_json::json!({"command": wide, "args": [wide, huge]}),
        );
        add(
            "not-string",
            serde_json::json!({"command": 1, "args": [1, 2]}),
        );
        add(
            "args",
            serde_json::json!({"command": "x", "args": vec!["a"; MAX_ARGS + 1]}),
        );
        add(
            "big",
            serde_json::json!({"command": "x", "args": [ "b".repeat(9_000) ]}),
        );
        let json = serde_json::json!({"mcpServers": servers}).to_string();
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        let ws = root.join("ws");
        std::fs::create_dir(&ws).unwrap();
        std::fs::write(ws.join(FILE), &json).unwrap();
        // A file this big is unreadable as a whole; the bounds per entry
        // are what is tested, so the cap is checked through `parse`.
        let FileRead::Entries(m) = parse(json.as_bytes()) else {
            panic!()
        };
        assert!(m.len() > 10);
        let pm = ProjectMcp::open(root.join("project-mcp.json"));
        let p = policy(&root);
        let list = pm.list(
            &p,
            "u",
            &ws,
            &FileRead::Entries(m),
            Some(format!("x{}", "/a".repeat(4000))),
        );
        let reasons: Vec<&str> = list
            .entries
            .iter()
            .filter_map(|e| e.reason.as_deref())
            .collect();
        assert!(reasons.len() >= 13, "{reasons:?}");
        let mut l = list.clone();
        l.note = l.note.map(|n| words(&n));
        assert_eq!(l.showable(), Ok(()), "{l:#?}");
        assert!(
            l.entries
                .iter()
                .any(|e| e.name == "good" && e.digest.is_some())
        );
    }
}
