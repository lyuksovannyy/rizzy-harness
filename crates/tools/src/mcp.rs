//! The MCP client (ADOPTION_PLAN X.3, first cut; PLAN.md T47): stdio
//! servers declared in the node's own `node.toml` (`[mcp_servers.<name>]`),
//! their tools offered to the agent beside the built-in ones.
//!
//! **Only node.toml adds a server.** The table is read at the node's start;
//! nothing the master, a prompt or a repository file sends reaches it
//! (research round 2 §6.1 step 4(a): no harness studied makes the node's
//! policy the only door, and DeepSeek's settings RPC shows what that costs).
//!
//! **One process per chat, in the chat's own confinement.** A server is
//! started lazily, at the first turn of an agentic chat (never a
//! sub-agent's) that offers MCP tools, through the same
//! [`crate::sandbox::Confinement::shell`] as that chat's `bash`: its
//! execution mode, its folder (read-write, as for `bash`), its file view,
//! its network rule, its guard. A per-node process would serve every user's
//! chats from one folder and one mode. Its environment is the allow-listed
//! core list (`crate::env`), the server's own `env_vars` names (the node's
//! deny-list still wins) and its own `env` values; nothing else of the
//! node's. A contained server reads its requests from a FIFO the guard
//! hands to bubblewrap ([`crate::guard::STDIN_FIFO_ENV`]); an uncontained
//! one (a `host` chat's) from a pipe, in its own process group.
//!
//! **It never outlives its chat.** The process is owned by one task: a
//! stop order (the chat deleted, the node stopping, ten idle minutes)
//! kills it through the guard's channel (contained) or its process group
//! (uncontained), and so does dropping the chat's table. A contained server
//! also dies with the node (T.8e); an uncontained one can outlive a
//! `kill -9` of the node, as a `host` chat's `bash` can (C.11).
//!
//! **Its tools are named `mcp__<server>__<tool>`** (Claude Code, Codex,
//! DeepSeek), `[A-Za-z0-9_-]`, at most 64 characters, with a short hash
//! of the identity when cleaning or cutting changed the name (DeepSeek's
//! `publicToolName`). Built-in names never start with `mcp__`, and server
//! names may not hold `__`, so no tool can shadow another.
//!
//! **Every call is a mutating call** for the approval path: an MCP tool is
//! code the harness did not write, so `ask` asks about each one (with the
//! usual "for this turn", which covers that exact call). What a server says
//! about its tools (`readOnlyHint` and the other annotations) is never read:
//! a hint could only loosen that, and a hostile server can mislabel a
//! destructive tool (goose's SmartApprove; research round 2 §4.3).
//!
//! **Bounded everywhere.** A message from the server is at most
//! [`MAX_LINE_BYTES`] (Codex's 8 MiB stdio line cap): past it the server is
//! stopped and the call fails at once, rather than waiting for its timeout.
//! A listing is at most [`MAX_LIST_PAGES`] pages; a tool's description is
//! cut to [`MAX_DESCRIPTION_CHARS`]; a tool whose input schema is over
//! [`MAX_SCHEMA_BYTES`] is left out, and a chat carries at most
//! [`MAX_TOOLS`] MCP tools and [`MAX_TOOL_TEXT`] bytes of their names,
//! descriptions and schemas; what is left out is named in a note. A result
//! is tool output like any other: text only (images, audio and binary
//! resources are named, never dropped silently), cut to
//! [`crate::MAX_OUTPUT_CHARS`], scrubbed, and quoted to the model by the
//! loop as every tool result is.

use std::collections::{BTreeMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::{Duration, Instant};

use harness_core::policy::ToolRisk;
use harness_core::secret::Secret;
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, Implementation, PaginatedRequestParams,
};
use rmcp::service::{RoleClient, RunningService};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, ReadBuf};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::{Tool, ToolCtx, ToolOutcome};

/// Every MCP tool's name starts with this.
pub const PREFIX: &str = "mcp__";

/// The longest tool name (the providers' function-name limit; DeepSeek's
/// `MAX_PUBLIC_NAME_LENGTH`).
pub const MAX_NAME: usize = 64;

/// MCP tools one chat carries. Claude Code's documentation: tool selection
/// degrades beyond 30 to 50 loaded tools (research round 1 §2.1, deferral);
/// the built-in tools come on top.
pub const MAX_TOOLS: usize = 40;

/// Characters of one tool's description (Claude Code's upfront cap).
pub const MAX_DESCRIPTION_CHARS: usize = 4096;

/// Bytes of one tool's input schema; a larger one leaves the tool out
/// (Codex hides a plugin tool whose spec is over 8,000 bytes).
pub const MAX_SCHEMA_BYTES: usize = 8_000;

/// Bytes of every MCP tool's name, description and schema in one chat
/// (Codex's 64,000-byte budget for plugin tools).
pub const MAX_TOOL_TEXT: usize = 64_000;

/// Bytes of one message from a server (Codex's stdio line cap).
pub const MAX_LINE_BYTES: usize = 8 << 20;

/// Pages of one `tools/list`: a server that pages forever is cut here.
pub const MAX_LIST_PAGES: usize = 20;

/// Tools one server may list before the rest of its listing is ignored
/// (long before [`MAX_TOOLS`] would offer them; bounds memory).
const MAX_LISTED: usize = 1_000;

/// Times a server is started again after it failed (did not start, ended
/// by itself, a reply over the cap, a call past its timeout) in one chat;
/// then it is not started again until the node restarts. DeepSeek bounds
/// reconnects the same way (10 attempts per outage).
pub const MAX_RESTARTS: u32 = 3;

/// A server no call used for this long is stopped (it starts again at the
/// next turn that offers its tools): every chat stays in the node's memory,
/// so servers would otherwise pile up, one set per chat ever used.
pub const IDLE_STOP: Duration = Duration::from_secs(10 * 60);

/// `startup_timeout_sec` (start, `initialize` and the listing) and
/// `tool_timeout_sec` (one call), Codex's defaults, and their ceilings.
pub const DEFAULT_STARTUP_TIMEOUT_S: u64 = 30;
pub const MAX_STARTUP_TIMEOUT_S: u64 = 300;
pub const DEFAULT_TOOL_TIMEOUT_S: u64 = 300;
pub const MAX_TOOL_TIMEOUT_S: u64 = 3600;

/// Bytes of a server's stderr kept (its last ones) for error messages.
const STDERR_KEEP: usize = 2048;

/// How long a stop waits for a server's process to be gone.
pub const STOP_WAIT: Duration = Duration::from_secs(6);

// ---- configuration (node.toml) ----

/// One `[mcp_servers.<name>]` table of node.toml. The field names are
/// Codex's (`config.toml` `[mcp_servers.<name>]`), so a definition can be
/// copied over; only stdio servers exist in this cut.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    /// The program, found on the chat's `PATH` like a `bash` command.
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Values given to the server, and to nothing else (tokens). They are
    /// added to the node's scrubber, so they never reach an event.
    #[serde(default)]
    pub env: EnvValues,
    /// Names (or globs, as in `env_pass`) passed from the node's own
    /// environment on top of the core list; provider keys, names from
    /// `.env` and `HARNESS_*` never are.
    #[serde(default)]
    pub env_vars: Vec<String>,
    #[serde(default = "on")]
    pub enabled: bool,
    /// Only these of the server's tools (exact names), when set.
    #[serde(default)]
    pub enabled_tools: Option<Vec<String>>,
    /// Never these (exact names).
    #[serde(default)]
    pub disabled_tools: Vec<String>,
    #[serde(default = "default_startup")]
    pub startup_timeout_sec: u64,
    #[serde(default = "default_tool")]
    pub tool_timeout_sec: u64,
}

fn on() -> bool {
    true
}
fn default_startup() -> u64 {
    DEFAULT_STARTUP_TIMEOUT_S
}
fn default_tool() -> u64 {
    DEFAULT_TOOL_TIMEOUT_S
}

impl std::fmt::Debug for ServerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerConfig")
            .field("command", &self.command)
            .field("args", &self.args)
            .field("env", &self.env.0.keys().collect::<Vec<_>>())
            .field("env_vars", &self.env_vars)
            .field("enabled", &self.enabled)
            .field("enabled_tools", &self.enabled_tools)
            .field("disabled_tools", &self.disabled_tools)
            .field("startup_timeout_sec", &self.startup_timeout_sec)
            .field("tool_timeout_sec", &self.tool_timeout_sec)
            .finish()
    }
}

impl ServerConfig {
    /// A server running `command args…` with the defaults (tests).
    pub fn new(command: impl Into<String>, args: &[&str]) -> Self {
        Self {
            command: command.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            env: EnvValues::default(),
            env_vars: Vec::new(),
            enabled: true,
            enabled_tools: None,
            disabled_tools: Vec::new(),
            startup_timeout_sec: DEFAULT_STARTUP_TIMEOUT_S,
            tool_timeout_sec: DEFAULT_TOOL_TIMEOUT_S,
        }
    }

    fn startup_timeout(&self) -> Duration {
        Duration::from_secs(self.startup_timeout_sec)
    }

    fn tool_timeout(&self) -> Duration {
        Duration::from_secs(self.tool_timeout_sec)
    }

    fn offers(&self, tool: &str) -> bool {
        self.enabled_tools
            .as_ref()
            .is_none_or(|l| l.iter().any(|t| t == tool))
            && !self.disabled_tools.iter().any(|t| t == tool)
    }
}

/// A server's `env` values, each a [`Secret`]: redacted in `Debug` and
/// when the policy is serialized.
#[derive(Clone, Default, PartialEq)]
pub struct EnvValues(pub BTreeMap<String, Secret<String>>);

impl<'de> Deserialize<'de> for EnvValues {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = BTreeMap::<String, String>::deserialize(d)?;
        Ok(Self(
            m.into_iter().map(|(k, v)| (k, Secret::new(v))).collect(),
        ))
    }
}

impl Serialize for EnvValues {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let m: BTreeMap<&str, &str> = self.0.keys().map(|k| (k.as_str(), "<redacted>")).collect();
        m.serialize(s)
    }
}

/// Check the `[mcp_servers]` tables serde cannot: names, commands,
/// environment names, timeouts.
pub fn validate(servers: &BTreeMap<String, ServerConfig>) -> Result<(), String> {
    for (name, s) in servers {
        let at = format!("[mcp_servers.{name}]");
        if !valid_server_name(name) {
            return Err(format!(
                "{at}: a server name is 1 to 32 of a-z, 0-9, `-` and `_`, starts with a letter or \
                 digit and holds no `__` (it becomes part of tool names: mcp__{name}__<tool>)"
            ));
        }
        if s.command.trim().is_empty() {
            return Err(format!("{at}: command is empty"));
        }
        if s.command.contains('\0') || s.args.iter().any(|a| a.contains('\0')) {
            return Err(format!("{at}: command and args may not hold a NUL byte"));
        }
        for (k, v) in &s.env.0 {
            if k.is_empty()
                || k.contains(['=', '\0'])
                || k.chars().any(char::is_whitespace)
                || v.expose().contains('\0')
            {
                return Err(format!("{at}: env name {k:?} is not a variable name"));
            }
            if crate::env::DENY_PREFIXES.iter().any(|p| k.starts_with(p)) {
                return Err(format!(
                    "{at}: env {k:?}: HARNESS_* names are the harness's own settings"
                ));
            }
        }
        crate::env::compile_pass(&s.env_vars).map_err(|e| format!("{at}: env_vars: {e}"))?;
        if !(1..=MAX_STARTUP_TIMEOUT_S).contains(&s.startup_timeout_sec) {
            return Err(format!(
                "{at}: startup_timeout_sec = {}: must be between 1 and {MAX_STARTUP_TIMEOUT_S}",
                s.startup_timeout_sec
            ));
        }
        if !(1..=MAX_TOOL_TIMEOUT_S).contains(&s.tool_timeout_sec) {
            return Err(format!(
                "{at}: tool_timeout_sec = {}: must be between 1 and {MAX_TOOL_TIMEOUT_S}",
                s.tool_timeout_sec
            ));
        }
    }
    Ok(())
}

fn valid_server_name(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        && name
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric())
        && !name.contains("__")
}

/// The model-facing name of `server`'s tool `tool`: `mcp__<server>__<tool>`
/// as it is when that is clean and short enough, else cleaned, cut and
/// given 12 hex characters of `sha256(server \0 tool)` (DeepSeek's rule),
/// so two tools never share a name.
pub fn public_name(server: &str, tool: &str) -> String {
    use sha2::Digest;
    const HASH: usize = 12;
    let joined = format!("{PREFIX}{server}__{tool}");
    let clean: String = joined
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if clean == joined && clean.len() <= MAX_NAME {
        return clean;
    }
    let mut h = sha2::Sha256::new();
    h.update(server.as_bytes());
    h.update([0u8]);
    h.update(tool.as_bytes());
    let hex: String = h.finalize()[..HASH / 2]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let keep: String = clean.chars().take(MAX_NAME - HASH - 1).collect();
    format!("{keep}_{hex}")
}

// ---- a chat's servers ----

/// One chat's MCP servers, made by the node for an agentic chat that is not
/// a sub-agent's (from node.toml alone) and handed to its turns
/// (`TurnHost::mcp`). Dropping it stops every server.
pub struct SessionMcp {
    slots: Vec<Arc<Slot>>,
    /// Set by [`Self::close`]: nothing starts any more (the chat is being
    /// deleted, the node is stopping).
    closed: Arc<AtomicBool>,
    /// Notes already shown in this chat (each is shown once).
    shown: Mutex<HashSet<String>>,
}

/// What a turn gets: the tools to offer, and notes for the person and the
/// model (servers that did not start, tools left out). Notes repeat only
/// when they change.
pub struct Loaded {
    pub tools: Vec<Arc<dyn Tool>>,
    pub notes: Vec<String>,
}

impl SessionMcp {
    /// The enabled servers of `servers`; `None` when there is none.
    pub fn new(servers: &BTreeMap<String, ServerConfig>) -> Option<Arc<Self>> {
        let slots: Vec<Arc<Slot>> = servers
            .iter()
            .filter(|(_, c)| c.enabled)
            .map(|(name, cfg)| {
                Arc::new(Slot {
                    name: name.clone(),
                    cfg: cfg.clone(),
                    state: tokio::sync::Mutex::new(State::Idle),
                    live: Mutex::new(None),
                    fails: AtomicU32::new(0),
                    last_used: Mutex::new(Instant::now()),
                    busy: AtomicUsize::new(0),
                })
            })
            .collect();
        (!slots.is_empty()).then(|| {
            Arc::new(Self {
                slots,
                closed: Arc::new(AtomicBool::new(false)),
                shown: Mutex::new(HashSet::new()),
            })
        })
    }

    /// Start the servers that are not running (all at once, each within
    /// its `startup_timeout_sec`), in the confinement `ctx` gives (this
    /// chat's), and return the tools this turn may offer.
    pub async fn load(&self, ctx: &ToolCtx) -> Loaded {
        let mut notes = Vec::new();
        if self.closed.load(Ordering::SeqCst) {
            return Loaded {
                tools: Vec::new(),
                notes,
            };
        }
        let started =
            futures::future::join_all(self.slots.iter().map(|s| s.ensure(ctx, &self.closed))).await;
        let mut tools: Vec<Arc<dyn Tool>> = Vec::new();
        let mut text = 0usize;
        for (slot, res) in self.slots.iter().zip(started) {
            let listed = match res {
                Ok(l) => l,
                Err(e) => {
                    notes.push(format!("MCP server `{}` is not available: {e}", slot.name));
                    continue;
                }
            };
            let mut cut = Vec::new();
            for l in listed.tools {
                let size = l.name.len() + l.description.len() + l.schema_len;
                if tools.len() >= MAX_TOOLS || text + size > MAX_TOOL_TEXT {
                    cut.push(l.raw.clone());
                    continue;
                }
                text += size;
                tools.push(Arc::new(McpTool {
                    slot: slot.clone(),
                    closed: self.closed.clone(),
                    name: l.name,
                    raw: l.raw,
                    description: l.description,
                    schema: l.schema,
                }));
            }
            notes.extend(listed.notes);
            if !cut.is_empty() {
                notes.push(format!(
                    "{} tool(s) of MCP server `{}` are not offered in this chat ({}): a chat \
                     carries at most {MAX_TOOLS} MCP tools and {MAX_TOOL_TEXT} bytes of their \
                     descriptions and schemas. The node's admin can choose them with \
                     `enabled_tools` in node.toml",
                    cut.len(),
                    slot.name,
                    shorten(&cut.join(", "), 300)
                ));
            }
        }
        let notes = {
            let mut shown = self.shown.lock().unwrap_or_else(|p| p.into_inner());
            notes
                .into_iter()
                .filter(|n| shown.insert(n.clone()))
                .collect()
        };
        Loaded { tools, notes }
    }

    /// Order every server to stop, and nothing to start again; not waited
    /// for ([`Self::wait_gone`]).
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        for s in &self.slots {
            s.kill();
        }
    }

    /// Wait until every server's process is gone, at most `bound`. True
    /// when they are.
    pub async fn wait_gone(&self, bound: Duration) -> bool {
        let by = tokio::time::Instant::now() + bound;
        for s in &self.slots {
            let rx = s
                .live
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
                .map(|l| l.exited.clone());
            if let Some(mut rx) = rx
                && tokio::time::timeout_at(by, rx.wait_for(Option::is_some))
                    .await
                    .is_err()
            {
                return false;
            }
        }
        true
    }

    /// Stop the servers no call used for `idle` (they start again when a
    /// turn needs them; this does not count as a failure).
    pub fn stop_idle(&self, idle: Duration) {
        for s in &self.slots {
            let quiet = s.busy.load(Ordering::SeqCst) == 0
                && s.last_used
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .elapsed()
                    >= idle;
            if quiet {
                s.kill();
            }
        }
    }

    /// How many server processes are alive.
    pub fn running(&self) -> usize {
        self.slots.iter().filter(|s| s.alive()).count()
    }
}

impl Drop for SessionMcp {
    fn drop(&mut self) {
        self.close();
    }
}

struct Slot {
    name: String,
    cfg: ServerConfig,
    state: tokio::sync::Mutex<State>,
    /// The running process's stop order and end, for stops that must not
    /// wait for `state` (held across a start or a call).
    live: Mutex<Option<Live>>,
    fails: AtomicU32,
    last_used: Mutex<Instant>,
    busy: AtomicUsize,
}

#[derive(Clone)]
struct Live {
    kill: CancellationToken,
    exited: watch::Receiver<Option<Exit>>,
}

#[derive(Debug, Clone)]
struct Exit {
    /// Stopped by the node (idle, a close, after a timeout): not counted.
    ordered: bool,
    text: String,
}

enum State {
    Idle,
    Up(Box<Up>),
    /// Not started again in this chat.
    Failed(String),
}

struct Up {
    service: RunningService<RoleClient, rmcp::model::ClientConfig>,
    listed: Listing,
    live: Live,
    stderr: Arc<Mutex<Tail>>,
    oversize: Arc<AtomicBool>,
}

#[derive(Clone)]
struct Listing {
    tools: Vec<Listed>,
    notes: Vec<String>,
}

#[derive(Clone)]
struct Listed {
    name: String,
    raw: String,
    description: String,
    schema: Value,
    schema_len: usize,
}

impl Slot {
    fn kill(&self) {
        if let Some(l) = self.live.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            l.kill.cancel();
        }
    }

    fn alive(&self) -> bool {
        self.live
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .is_some_and(|l| l.exited.borrow().is_none())
    }

    fn touch(&self) {
        *self.last_used.lock().unwrap_or_else(|p| p.into_inner()) = Instant::now();
    }

    /// Count a failure; the reason a caller gives the model.
    fn failed(&self, why: &str) -> String {
        let n = self.fails.fetch_add(1, Ordering::SeqCst) + 1;
        if n > MAX_RESTARTS {
            format!(
                "{why}. It failed {n} times in this chat and is not started again until the \
                 node restarts"
            )
        } else {
            format!(
                "{why}. It is started again at its next use (restart {n} of the {MAX_RESTARTS} \
                 this chat allows)"
            )
        }
    }

    /// Running, or started now: its listing.
    async fn ensure(&self, ctx: &ToolCtx, closed: &AtomicBool) -> Result<Listing, String> {
        let mut st = self.state.lock().await;
        let ended = match &*st {
            State::Up(up) => {
                let exit = up.live.exited.borrow().clone();
                match exit {
                    None => {
                        self.touch();
                        return Ok(up.listed.clone());
                    }
                    Some(exit) => Some((up.why_ended(&exit), exit.ordered)),
                }
            }
            _ => None,
        };
        if let Some((why, ordered)) = ended {
            {
                {
                    *st = State::Idle;
                    if !ordered {
                        let msg = self.failed(&why);
                        if self.fails.load(Ordering::SeqCst) > MAX_RESTARTS {
                            *st = State::Failed(msg.clone());
                            return Err(msg);
                        }
                    }
                }
            }
        }
        if let State::Failed(why) = &*st {
            return Err(why.clone());
        }
        if closed.load(Ordering::SeqCst) {
            return Err("the chat's servers were stopped".into());
        }
        let started = tokio::select! {
            r = tokio::time::timeout(self.cfg.startup_timeout(), start(&self.name, &self.cfg, ctx)) => r,
            _ = ctx.cancel.cancelled() => return Err("the turn was stopped".into()),
        };
        match started {
            Ok(Ok(up)) => {
                *self.live.lock().unwrap_or_else(|p| p.into_inner()) = Some(up.live.clone());
                let listed = up.listed.clone();
                *st = State::Up(Box::new(up));
                self.touch();
                Ok(listed)
            }
            Ok(Err(e)) => {
                let msg = self.failed(&format!("it did not start: {e}"));
                if self.fails.load(Ordering::SeqCst) > MAX_RESTARTS {
                    *st = State::Failed(msg.clone());
                }
                Err(msg)
            }
            // The half-started process went with the dropped future.
            Err(_) => {
                let msg = self.failed(&format!(
                    "it did not answer within {} s (startup_timeout_sec in node.toml)",
                    self.cfg.startup_timeout_sec
                ));
                if self.fails.load(Ordering::SeqCst) > MAX_RESTARTS {
                    *st = State::Failed(msg.clone());
                }
                Err(msg)
            }
        }
    }

    /// Stop the running process after a failure a call saw (`msg`, from
    /// [`Self::failed`]) and forget it; past the restarts, for good.
    async fn take_down(&self, msg: &str) {
        let mut st = self.state.lock().await;
        if let State::Up(up) = &*st {
            up.live.kill.cancel();
        }
        *st = if self.fails.load(Ordering::SeqCst) > MAX_RESTARTS {
            State::Failed(msg.to_string())
        } else {
            State::Idle
        };
    }

    async fn call(
        &self,
        ctx: &ToolCtx,
        closed: &AtomicBool,
        raw: &str,
        input: Value,
    ) -> ToolOutcome {
        if let Err(e) = self.ensure(ctx, closed).await {
            return ToolOutcome::err(format!("MCP server `{}` is not available: {e}", self.name));
        }
        let (peer, mut exited, stderr, oversize) = {
            let st = self.state.lock().await;
            let State::Up(up) = &*st else {
                return ToolOutcome::err(format!("MCP server `{}` stopped", self.name));
            };
            (
                up.service.peer().clone(),
                up.live.exited.clone(),
                up.stderr.clone(),
                up.oversize.clone(),
            )
        };
        let Value::Object(args) = input else {
            return ToolOutcome::err("tool arguments must be a JSON object");
        };
        let mut params = CallToolRequestParams::new(raw.to_string());
        params.arguments = Some(args);
        self.busy.fetch_add(1, Ordering::SeqCst);
        let _busy = Busy(&self.busy);
        self.touch();
        let timeout = self.cfg.tool_timeout();
        // Dropping the call's future sends `notifications/cancelled`.
        let r = tokio::select! {
            r = tokio::time::timeout(timeout, detached(async move { peer.call_tool(params).await })) => r,
            exit = async { exited.wait_for(Option::is_some).await.ok().and_then(|e| e.clone()) } => {
                // Stopped by the harness (the chat closed, the node
                // stopping): not the server's failure.
                if exit.as_ref().is_some_and(|e| e.ordered) {
                    self.take_down("").await;
                    return ToolOutcome::err(format!(
                        "MCP server `{}` was stopped by the harness during the call",
                        self.name
                    ));
                }
                let why = self.ended_text(&stderr, &oversize, exit);
                let msg = self.failed(&why);
                self.take_down(&msg).await;
                return ToolOutcome::err(format!("MCP server `{}`: {msg}", self.name));
            }
            _ = ctx.cancel.cancelled() => return ToolOutcome::err("[cancelled]"),
        };
        self.touch();
        match r {
            Ok(Ok(result)) => render(
                &self.name,
                serde_json::to_value(&result).unwrap_or_default(),
            ),
            Ok(Err(rmcp::ServiceError::McpError(e))) => ToolOutcome::err(format!(
                "MCP server `{}` refused the call: {}",
                self.name, e.message
            )),
            Ok(Err(e)) => {
                // The output closing usually comes a moment before the
                // process's end: give that a moment, for its exit and stderr.
                let _ = tokio::time::timeout(
                    Duration::from_millis(500),
                    exited.wait_for(Option::is_some),
                )
                .await;
                let why = if oversize.load(Ordering::SeqCst) {
                    over_cap()
                } else {
                    let exit = exited.borrow().clone();
                    match exit {
                        Some(_) => self.ended_text(&stderr, &oversize, exit),
                        None => format!("the call failed ({e})"),
                    }
                };
                let msg = self.failed(&why);
                self.take_down(&msg).await;
                ToolOutcome::err(format!("MCP server `{}`: {msg}", self.name))
            }
            Err(_) => {
                let msg = self.failed(&format!(
                    "no answer within {} s (tool_timeout_sec in node.toml); the call was \
                     cancelled and the server stopped",
                    self.cfg.tool_timeout_sec
                ));
                self.take_down(&msg).await;
                ToolOutcome::err(format!("MCP server `{}`: {msg}", self.name))
            }
        }
    }

    fn ended_text(
        &self,
        stderr: &Mutex<Tail>,
        oversize: &AtomicBool,
        exit: Option<Exit>,
    ) -> String {
        if oversize.load(Ordering::SeqCst) {
            return over_cap();
        }
        let tail = stderr.lock().unwrap_or_else(|p| p.into_inner()).text();
        let mut s = format!(
            "the server ended ({})",
            exit.map_or_else(|| "its output closed".to_string(), |e| e.text)
        );
        if !tail.is_empty() {
            s.push_str(&format!("; the end of its error output: {tail}"));
        }
        s
    }
}

fn over_cap() -> String {
    format!(
        "the server sent a message larger than {} MiB, which the harness does not read; it was \
         stopped",
        MAX_LINE_BYTES >> 20
    )
}

impl Up {
    fn why_ended(&self, exit: &Exit) -> String {
        if self.oversize.load(Ordering::SeqCst) {
            return over_cap();
        }
        let tail = self.stderr.lock().unwrap_or_else(|p| p.into_inner()).text();
        if tail.is_empty() {
            format!("it ended ({})", exit.text)
        } else {
            format!(
                "it ended ({}); the end of its error output: {tail}",
                exit.text
            )
        }
    }
}

struct Busy<'a>(&'a AtomicUsize);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The end of a server's stderr.
#[derive(Default)]
struct Tail(Vec<u8>);

impl Tail {
    fn push(&mut self, b: &[u8]) {
        self.0.extend_from_slice(b);
        if self.0.len() > STDERR_KEEP {
            let cut = self.0.len() - STDERR_KEEP;
            self.0.drain(..cut);
        }
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0).trim().to_string()
    }
}

fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

// ---- starting a server ----

/// Start `cfg` in `ctx`'s confinement, `initialize` it and list its tools.
async fn start(name: &str, cfg: &ServerConfig, ctx: &ToolCtx) -> Result<Up, String> {
    let env = ctx.env.with_pass(&cfg.env_vars)?;
    let args: Vec<&OsStr> = cfg.args.iter().map(OsStr::new).collect();
    let (mut cmd, contained) = ctx.sandbox.shell(
        OsStr::new(&cfg.command),
        &args,
        &ctx.workspace,
        ctx.pin.as_ref(),
        &env,
        &ctx.protected,
        crate::sandbox::Grants {
            workspace_write: true,
            owned: ctx.owned.clone(),
            ..Default::default()
        },
    )?;
    for (k, v) in &cfg.env.0 {
        cmd.env(k, v.expose());
    }
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    // A contained server's stdin is a FIFO the guard opens (its own stdin
    // is its control channel); the folder holding it is the node's own.
    let mut fifo_dir = None;
    let writer: Box<dyn tokio::io::AsyncWrite + Send + Unpin> = if contained {
        let root = ctx
            .sandbox
            .private_root()
            .ok_or("the sandbox has no private folder for the server's input")?;
        let dir = FifoDir::new(root)?;
        let path = dir.0.join("in");
        nix::unistd::mkfifo(
            &path,
            nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
        )
        .map_err(|e| format!("mkfifo {}: {e}", path.display()))?;
        // Read-write: the open never waits for the guard, and never fails
        // because the guard has not opened it yet.
        let tx = tokio::net::unix::pipe::OpenOptions::new()
            .read_write(true)
            .open_sender(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        cmd.env(crate::guard::STDIN_FIFO_ENV, &path);
        fifo_dir = Some(dir);
        Box::new(tx)
    } else {
        cmd.stdin(std::process::Stdio::piped()).process_group(0);
        Box::new(tokio::io::sink())
    };
    let program = cmd.as_std().get_program().to_owned();
    let mut child = crate::sandbox::Confinement::spawn(cmd, contained)
        .await
        .map_err(|e| {
            crate::sandbox::image_spawn_error(&program, &e)
                .unwrap_or_else(|| format!("cannot start {}: {e}", cfg.command))
        })?;
    let writer = match child.stdin.take() {
        Some(stdin) => Box::new(stdin) as Box<dyn tokio::io::AsyncWrite + Send + Unpin>,
        None => writer,
    };
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr_pipe = child.stderr.take();
    let stderr = Arc::new(Mutex::new(Tail::default()));
    if let Some(mut e) = stderr_pipe {
        let tail = stderr.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            while let Ok(n) = e.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                tail.lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push(&buf[..n]);
            }
        });
    }
    let (live, exited_tx) = {
        let (tx, rx) = watch::channel(None);
        (
            Live {
                kill: CancellationToken::new(),
                exited: rx,
            },
            tx,
        )
    };
    // The one owner of the process from here.
    tokio::spawn(own(child, contained, live.kill.clone(), exited_tx));
    // If anything below fails, the process goes with `guard`.
    let guard = live.kill.clone().drop_guard();
    let oversize = Arc::new(AtomicBool::new(false));
    let reader = Capped {
        inner: stdout,
        since_newline: 0,
        over: oversize.clone(),
    };
    let info = rmcp::model::ClientConfig::new(
        ClientCapabilities::default(),
        Implementation::new(harness_core::APP_NAME, env!("CARGO_PKG_VERSION")),
    );
    // Through the trait: its future is declared `Send`, which the free
    // function's is not proven to be.
    use rmcp::service::ClientServiceExt as _;
    let service = info
        .serve_with_lifecycle(
            (reader, writer),
            rmcp::service::ClientLifecycleMode::Initialize,
        )
        .await
        .map_err(|e| {
            if oversize.load(Ordering::SeqCst) {
                over_cap()
            } else {
                let tail = stderr.lock().unwrap_or_else(|p| p.into_inner()).text();
                if tail.is_empty() {
                    format!("initialize failed: {e}")
                } else {
                    format!("initialize failed: {e}; the end of its error output: {tail}")
                }
            }
        })?;
    drop(fifo_dir);
    let listed = list(name, cfg, service.peer().clone()).await?;
    let _ = guard.disarm();
    Ok(Up {
        service,
        listed,
        live,
        stderr,
        oversize,
    })
}

/// The owner task: waits for the process to end or for the stop order.
async fn own(
    mut child: crate::bash::ShellChild,
    contained: bool,
    kill: CancellationToken,
    exited: watch::Sender<Option<Exit>>,
) {
    let pgid = child.id().map(|p| p as i32).filter(|_| !contained);
    let mut group = crate::bash::KillGroupOnDrop(pgid);
    let exit = tokio::select! {
        s = child.wait() => {
            // Whatever it left in its group goes with it.
            drop(group);
            Exit {
                ordered: false,
                text: match s {
                    Ok(s) => s.to_string(),
                    Err(e) => e.to_string(),
                },
            }
        }
        _ = kill.cancelled() => {
            match &mut child {
                crate::bash::ShellChild::Contained(c) => {
                    c.stop(false).await;
                }
                crate::bash::ShellChild::Plain(c) => {
                    if let Some(g) = group.0.take() {
                        crate::bash::signal_group(g, nix::sys::signal::Signal::SIGKILL);
                    }
                    let _ = c.start_kill();
                    let _ = tokio::time::timeout(Duration::from_secs(2), c.wait()).await;
                }
            }
            Exit {
                ordered: true,
                text: "stopped by the harness".into(),
            }
        }
    };
    let _ = exited.send(Some(exit));
}

/// A node-private folder for one FIFO, removed when dropped.
struct FifoDir(PathBuf);

impl FifoDir {
    fn new(root: &Path) -> Result<Self, String> {
        use std::os::unix::fs::DirBuilderExt;
        let mut b = [0u8; 8];
        getrandom::fill(&mut b).map_err(|e| e.to_string())?;
        let name: String = b.iter().map(|x| format!("{x:02x}")).collect();
        let dir = root.join(format!(".mcp-{name}"));
        std::fs::DirBuilder::new()
            .recursive(false)
            .mode(0o700)
            .create(&dir)
            .or_else(|e| match e.kind() {
                std::io::ErrorKind::NotFound => {
                    std::fs::DirBuilder::new()
                        .recursive(true)
                        .mode(0o700)
                        .create(root)?;
                    std::fs::DirBuilder::new().mode(0o700).create(&dir)
                }
                _ => Err(e),
            })
            .map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(Self(dir))
    }
}

impl Drop for FifoDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The server's stdout, cut at [`MAX_LINE_BYTES`] without a newline: past
/// it the read fails (the client then ends) and `over` says why.
struct Capped<R> {
    inner: R,
    since_newline: usize,
    over: Arc<AtomicBool>,
}

impl<R: AsyncRead + Unpin> AsyncRead for Capped<R> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.over.load(Ordering::SeqCst) {
            return Poll::Ready(Err(std::io::Error::other("message over the cap")));
        }
        let before = buf.filled().len();
        let r = std::pin::Pin::new(&mut self.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &r {
            let mut since = self.since_newline;
            for &b in &buf.filled()[before..] {
                since = if b == b'\n' { 0 } else { since + 1 };
                if since > MAX_LINE_BYTES {
                    self.over.store(true, Ordering::SeqCst);
                    return Poll::Ready(Err(std::io::Error::other("message over the cap")));
                }
            }
            self.since_newline = since;
        }
        r
    }
}

/// `tools/list`, every page up to [`MAX_LIST_PAGES`], bounded per tool.
async fn list(
    server: &str,
    cfg: &ServerConfig,
    peer: rmcp::Peer<RoleClient>,
) -> Result<Listing, String> {
    let mut raw: Vec<Value> = Vec::new();
    let mut cursor: Option<String> = None;
    let mut notes = Vec::new();
    for page in 0.. {
        if page == MAX_LIST_PAGES {
            notes.push(format!(
                "MCP server `{server}` lists its tools in more than {MAX_LIST_PAGES} pages; only \
                 the first {MAX_LIST_PAGES} are read"
            ));
            break;
        }
        let p = peer.clone();
        let params = PaginatedRequestParams::default().with_cursor(cursor);
        let r = detached(async move { p.list_tools(Some(params)).await })
            .await
            .map_err(|e| format!("tools/list failed: {e}"))?;
        raw.extend(r.tools.iter().filter_map(|t| serde_json::to_value(t).ok()));
        cursor = r.next_cursor;
        if cursor.is_none() || raw.len() >= MAX_LISTED {
            break;
        }
    }
    let mut tools: Vec<Listed> = Vec::new();
    let mut seen = HashSet::new();
    let mut big = Vec::new();
    for t in raw.into_iter().take(MAX_LISTED) {
        let Some(raw_name) = t.get("name").and_then(Value::as_str).map(str::to_string) else {
            continue;
        };
        if !cfg.offers(&raw_name) || !seen.insert(raw_name.clone()) {
            continue;
        }
        let schema = t.get("inputSchema").cloned().unwrap_or(Value::Null);
        let schema_len = serde_json::to_string(&schema).map_or(usize::MAX, |s| s.len());
        let is_object = schema.get("type").and_then(Value::as_str) == Some("object");
        if schema_len > MAX_SCHEMA_BYTES || !is_object {
            big.push(raw_name);
            continue;
        }
        let description = t
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        tools.push(Listed {
            name: public_name(server, &raw_name),
            description: format!(
                "(MCP server `{server}`) {}",
                shorten(&description, MAX_DESCRIPTION_CHARS)
            ),
            raw: raw_name,
            schema,
            schema_len,
        });
    }
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    if !big.is_empty() {
        notes.push(format!(
            "{} tool(s) of MCP server `{server}` are not offered: their input schema is over \
             {MAX_SCHEMA_BYTES} bytes or not a JSON object schema ({})",
            big.len(),
            shorten(&big.join(", "), 300)
        ));
    }
    Ok(Listing { tools, notes })
}

/// Run one request of the client in its own task, aborted (so the request
/// is dropped, which sends `notifications/cancelled`) when this future is.
/// The client's request futures borrow its peer in a way the compiler
/// cannot prove `Send` for every lifetime inside a `Tool` future
/// (rust-lang/rust#100013); a task of its own owns everything.
async fn detached<T: Send + 'static>(
    f: impl std::future::Future<Output = Result<T, rmcp::ServiceError>> + Send + 'static,
) -> Result<T, rmcp::ServiceError> {
    own_task(f)
        .await
        .unwrap_or(Err(rmcp::ServiceError::TransportClosed))
}

/// `f` in a task of its own, aborted when this future is dropped; `None`
/// when the task panicked.
async fn own_task<T: Send + 'static>(
    f: impl std::future::Future<Output = T> + Send + 'static,
) -> Option<T> {
    struct Abort(tokio::task::AbortHandle);
    impl Drop for Abort {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let task = tokio::spawn(f);
    let _abort = Abort(task.abort_handle());
    task.await.ok()
}

/// A call's result as tool output: its text, with every other kind of
/// content named.
fn render(server: &str, result: Value) -> ToolOutcome {
    let mut out = String::new();
    let budget = crate::MAX_OUTPUT_CHARS * 2;
    let mut push = |s: &str| {
        if out.len() < budget {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(s);
        }
    };
    let kb = |data: &Value| data.as_str().map_or(0, |d| d.len() * 3 / 4 / 1024);
    let content = result
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for c in &content {
        let mime = c
            .get("mimeType")
            .and_then(Value::as_str)
            .unwrap_or("unknown type");
        match c.get("type").and_then(Value::as_str).unwrap_or("") {
            "text" => push(c.get("text").and_then(Value::as_str).unwrap_or("")),
            kind @ ("image" | "audio") => push(&format!(
                "[{kind} from MCP server `{server}` ({mime}, about {} KiB): not shown; this \
                 harness passes only text from MCP tools]",
                kb(c.get("data").unwrap_or(&Value::Null))
            )),
            "resource" => {
                let r = c.get("resource").cloned().unwrap_or_default();
                let uri = r.get("uri").and_then(Value::as_str).unwrap_or("");
                match r.get("text").and_then(Value::as_str) {
                    Some(t) => push(&format!("[resource {uri}]\n{t}")),
                    None => push(&format!(
                        "[binary resource {uri} ({}, about {} KiB): not shown; this harness \
                         passes only text from MCP tools]",
                        r.get("mimeType")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown type"),
                        kb(r.get("blob").unwrap_or(&Value::Null))
                    )),
                }
            }
            "resource_link" => push(&format!(
                "[resource link: {} {}]",
                c.get("name").and_then(Value::as_str).unwrap_or(""),
                c.get("uri").and_then(Value::as_str).unwrap_or("")
            )),
            other => push(&format!(
                "[content of type `{other}` from MCP server `{server}`: not shown]"
            )),
        }
    }
    if content.is_empty()
        && let Some(s) = result.get("structuredContent").filter(|v| !v.is_null())
    {
        push(&serde_json::to_string_pretty(s).unwrap_or_default());
    }
    if out.is_empty() {
        out.push_str("(no output)");
    }
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        ToolOutcome::err(out)
    } else {
        ToolOutcome::ok(out)
    }
}

// ---- the tool ----

/// One MCP tool of a chat, as offered to the model.
pub struct McpTool {
    slot: Arc<Slot>,
    closed: Arc<AtomicBool>,
    name: String,
    raw: String,
    description: String,
    schema: Value,
}

impl McpTool {
    /// The server's own name of this tool.
    pub fn server_tool(&self) -> (&str, &str) {
        (&self.slot.name, &self.raw)
    }
}

#[async_trait::async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn schema(&self) -> Value {
        self.schema.clone()
    }

    /// Always: whatever the server says about the tool.
    fn risk(&self) -> ToolRisk {
        ToolRisk::Mutating
    }

    async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome {
        // A tool of a closed table (the chat deleted, the node stopping)
        // starts nothing: `ensure` checks `closed`.
        self.slot.call(ctx, &self.closed, &self.raw, input).await
    }
}

/// Start `cfg` once in `ctx`, list its tools and stop it (`doctor`): the
/// number of tools it lists and how many a chat would get.
pub async fn probe(name: &str, cfg: &ServerConfig, ctx: &ToolCtx) -> Result<String, String> {
    let mut servers = BTreeMap::new();
    servers.insert(name.to_string(), cfg.clone());
    let set = SessionMcp::new(&servers).ok_or("disabled (enabled = false)")?;
    let loaded = set.load(ctx).await;
    set.close();
    set.wait_gone(STOP_WAIT).await;
    let (tools, notes) = (loaded.tools.len(), loaded.notes);
    if tools == 0
        && let Some(n) = notes.first()
    {
        return Err(n.clone());
    }
    let mut s = format!("starts; {tools} tool(s) offered");
    for n in notes {
        s.push_str("; ");
        s.push_str(&n);
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_names_are_prefixed_clean_and_unique() {
        assert_eq!(
            public_name("github", "create_issue"),
            "mcp__github__create_issue"
        );
        let a = public_name("fs", "read.file");
        let b = public_name("fs", "read_file");
        assert_ne!(a, b);
        assert!(a.starts_with("mcp__fs__read_file_"), "{a}");
        assert!(
            a.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        );
        let long = public_name("s", &"x".repeat(200));
        assert_eq!(long.len(), MAX_NAME);
        assert_ne!(long, public_name("s", &"x".repeat(201)));
    }

    #[test]
    fn server_names_and_settings_are_checked() {
        let ok = ServerConfig::new("npx", &["-y", "x"]);
        for bad in ["", "Up", "a__b", "-a", "a b", &"a".repeat(33)] {
            let mut m = BTreeMap::new();
            m.insert(bad.to_string(), ok.clone());
            assert!(validate(&m).is_err(), "{bad:?}");
        }
        let mut m = BTreeMap::new();
        m.insert("git-hub_2".to_string(), ok.clone());
        assert!(validate(&m).is_ok());
        let mut harness = ok.clone();
        harness.env.0.insert(
            crate::guard::STDIN_FIFO_ENV.into(),
            Secret::new("/x".into()),
        );
        m.insert("a".into(), harness);
        assert!(validate(&m).unwrap_err().contains("HARNESS_"));
        let mut slow = ok.clone();
        slow.tool_timeout_sec = 0;
        let mut m = BTreeMap::new();
        m.insert("a".into(), slow);
        assert!(validate(&m).is_err());
    }

    #[test]
    fn env_values_never_show() {
        let c: ServerConfig = serde_json::from_value(
            serde_json::json!({"command": "x", "env": {"TOKEN": "tok-123456789"}}),
        )
        .unwrap();
        assert_eq!(c.env.0["TOKEN"].expose(), "tok-123456789");
        assert!(!format!("{c:?}").contains("tok-123"));
        assert!(!serde_json::to_string(&c).unwrap().contains("tok-123"));
    }

    #[test]
    fn results_name_what_they_do_not_show() {
        let r = render(
            "s",
            serde_json::json!({"content": [
                {"type": "text", "text": "hello"},
                {"type": "image", "data": "aGVsbG8=", "mimeType": "image/png"},
                {"type": "resource", "resource": {"uri": "file:///b", "blob": "AAAA", "mimeType": "application/zip"}},
                {"type": "widget"}
            ]}),
        );
        assert!(r.ok);
        assert!(r.output.contains("hello"));
        assert!(
            r.output.contains("image from MCP server `s` (image/png"),
            "{}",
            r.output
        );
        assert!(
            r.output.contains("binary resource file:///b"),
            "{}",
            r.output
        );
        assert!(r.output.contains("`widget`"), "{}", r.output);
        let e = render(
            "s",
            serde_json::json!({"content": [{"type": "text", "text": "no"}], "isError": true}),
        );
        assert!(!e.ok);
        let huge = render(
            "s",
            serde_json::json!({"content": [{"type": "text", "text": "z".repeat(5_000_000)}]}),
        );
        assert!(huge.truncated && huge.output.chars().count() < crate::MAX_OUTPUT_CHARS + 100);
    }
}
