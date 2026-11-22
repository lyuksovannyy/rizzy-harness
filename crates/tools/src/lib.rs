//! Built-in tools: bash, read, write, edit, grep, glob, todo, skill.
//!
//! Every tool is cancellable through `ToolCtx::cancel`. Path-taking tools are
//! confined to the workspace unless the node policy says otherwise.

pub mod approval;
pub mod args;
pub mod background;
mod bash;
mod attached;
mod board;
pub mod connector;
pub mod contain;
pub mod guard;
pub use bash::{
    DEFAULT_TIMEOUT_MAX_S, MAX_TIMEOUT_MAX_S, MIN_TIMEOUT_MAX_S, workdir as bash_workdir,
};
mod edit;
#[cfg(test)]
mod edit_fuzz_tests;
#[cfg(test)]
mod edit_hardening_tests;
mod edit_lines;
#[cfg(test)]
mod edit_lines_tests;
#[cfg(test)]
mod edit_preview_tests;
#[cfg(test)]
mod edit_tag_unique_tests;
#[cfg(test)]
mod edit_tests;
pub mod env;
pub mod filter;
mod fs;
pub mod graphify;
#[cfg(test)]
mod image_read_tests;
pub mod inside;
pub mod mcp;
pub mod mention;
mod outline;
pub mod outputs;
mod publish;
pub mod sandbox;
mod search;
#[cfg(test)]
mod search_tag_tests;
pub mod seen;
mod shown;
pub use shown::RESULT_BUDGET;
#[cfg(test)]
mod shown_tests;
pub mod skills;
pub mod stream;
pub mod todo;

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use harness_core::event::Source;
use harness_core::policy::ToolRisk;
use harness_core::refs::SharedRefs;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

pub use approval::{CallKey, call_key};
pub use background::{BashOutput, BashStop};
pub use bash::{
    Bash, ENDED_MS_LEAD, FOREGROUND_DESCRIPTION, HOST_DESCRIPTION, KEPT_OUTPUT_LEAD,
    STILL_ALIVE_TAIL, ShellChild, foreground_schema, read_only_note,
};
pub use attached::AttachedFileTool;
pub use board::{Board, BoardEdit, render as render_board};
pub use edit::Edit;
pub use env::ToolEnv;
pub use fs::{Read, Write, image_of};
pub use outline::Outline;
pub use outputs::{OutputStore, SessionOutputs, ToolOutput};
pub use publish::Publish;
pub use search::{Glob, Grep};
pub use stream::OutputSink;

/// Cap on what the model sees from one tool call.
pub const MAX_OUTPUT_CHARS: usize = 30_000;

/// Bytes `read` takes from one file. It streams them line by line, so this
/// bounds time, not memory; past it the model is told where it stopped.
pub const MAX_READ_BYTES: u64 = 32 << 20;

/// Files over this size are skipped by `grep` (it holds one file at a time
/// in memory), and the output says how many were. `edit` and `outline`,
/// which load the whole file too, refuse them.
pub const MAX_GREP_FILE_BYTES: u64 = 8 << 20;

/// Flags for every file a tool opens: opening a FIFO never waits for the
/// other end, and a terminal never becomes the node's controlling one.
/// Neither changes anything for a regular file.
pub(crate) fn open_flags() -> i32 {
    (nix::fcntl::OFlag::O_NONBLOCK | nix::fcntl::OFlag::O_NOCTTY).bits()
}

/// Refuse anything but a regular file (a FIFO blocks a reader forever, a
/// device never ends or has side effects, a directory or socket is no
/// text).
pub(crate) fn check_regular(path: &Path, ft: std::fs::FileType) -> Result<(), String> {
    use std::os::unix::fs::FileTypeExt;
    if ft.is_file() {
        return Ok(());
    }
    let kind = if ft.is_dir() {
        "a directory"
    } else if ft.is_fifo() {
        "a FIFO"
    } else if ft.is_socket() {
        "a socket"
    } else if ft.is_char_device() {
        "a character device"
    } else if ft.is_block_device() {
        "a block device"
    } else {
        "a special file"
    };
    Err(format!(
        "{}: not a regular file ({kind}); file tools only read and write regular files",
        path.display()
    ))
}

/// Open an existing file for reading, only if it is a regular file. The
/// type is checked before opening (a device is never opened) and again on
/// the opened handle (`fstat`), so a FIFO or device swapped in between is
/// still refused; the open itself never blocks.
pub(crate) fn open_regular(path: &Path) -> Result<std::fs::File, String> {
    use std::os::unix::fs::OpenOptionsExt;
    let md = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    check_regular(path, md.file_type())?;
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(open_flags())
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let md = f
        .metadata()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    check_regular(path, md.file_type())?;
    Ok(f)
}

/// Read a whole regular file of at most `cap` bytes (`edit`, `outline`).
/// A larger file, or one that grows past `cap` while it is read, is
/// refused rather than cut.
pub(crate) fn read_small(path: &Path, cap: u64) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    let f = open_regular(path)?;
    let len = f
        .metadata()
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len();
    if len > cap {
        return Err(too_big(path, len, cap));
    }
    let mut bytes = Vec::with_capacity(len as usize);
    f.take(cap + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() as u64 > cap {
        return Err(too_big(path, bytes.len() as u64, cap));
    }
    Ok(bytes)
}

pub(crate) fn too_big(path: &Path, len: u64, cap: u64) -> String {
    format!(
        "{}: too large ({len} bytes or more; the limit is {cap}); use `read` with offset/limit, \
         or bash",
        path.display()
    )
}

pub struct ToolCtx {
    pub workspace: PathBuf,
    pub cancel: CancellationToken,
    /// Live output chunks for the UI (bash only today), bounded and with a
    /// total byte budget per call.
    pub output: Option<OutputSink>,
    pub allow_outside_workspace: bool,
    /// Folders no file tool may touch in any mode, `allow_outside_workspace`
    /// included: the node's data directory (keys, the API token, every
    /// user's chats). Canonical paths, from the node's policy.
    pub protected: Arc<Vec<PathBuf>>,
    pub timeout: Duration,
    /// The longest a `bash` call may run, whatever it asks for (node.toml
    /// `[limits] bash_timeout_max_s`).
    pub bash_timeout_max: Duration,
    /// The environment of every child process a tool starts (an allow-list).
    pub env: ToolEnv,
    /// Text this conversation got from the user and from tools, never text
    /// the model wrote. Network tools only follow URLs found here, so injected
    /// instructions cannot make the model smuggle data out in a URL it builds.
    pub observed: Arc<Vec<String>>,
    /// What the web tools have done in this turn (a sub-agent's turn has its
    /// own): the loop makes one per turn and shares it by every call.
    pub budget: Arc<TurnBudget>,
    /// Token-saving methods in effect for this call.
    pub savings: harness_core::savings::Savings,
    /// Where full outputs of filtered commands are kept (`tool_output`).
    /// Bound to the session the call runs for.
    pub outputs: Option<SessionOutputs>,
    /// Citation labels for web results, shared by every call of the turn.
    pub refs: SharedRefs,
    /// Where `publish` stages files (the node session's stager); `None`
    /// where publishing is off.
    pub artifacts: Option<Arc<dyn harness_core::artifacts::ArtifactSink>>,
    /// The call being run, for tools whose events must land on its card.
    pub call_id: String,
    /// How child processes (bash, graphify) are started: through the
    /// Landlock wrapper on a sandboxed node (node.toml `[sandbox]`).
    pub sandbox: sandbox::Confinement,
    /// The session folder as pinned when the session was created (its
    /// canonical path, device and inode). When set, every path is resolved
    /// in it only after [`Self::workspace_dir`] has checked that it is still
    /// that directory (R.1, PLAN.md T4).
    pub pin: Option<harness_core::folder::FolderPin>,
    /// The node-owned folder of a sub-agent session (its own folder under
    /// the node's protected sessions folder): paths inside it are not
    /// refused as protected, for this session only.
    pub owned: Option<harness_core::folder::OwnedFolder>,
    /// The identity the agent's `git commit` carries (T88), as the node's
    /// session resolved it from the chat owner; `None` adds nothing for git.
    pub identity: Option<harness_core::identity::Identity>,
    /// This session's background commands (`bash` with `background: true`,
    /// `bash_output`, `bash_stop`), or why there are none here.
    pub background: background::Access,
    /// The skills the node found for this turn (`skill` loads from them);
    /// `None` where there are none.
    pub skills: Option<Arc<harness_core::skills::Catalogue>>,
    /// What this session's agent has seen of each file: `edit` and `write`
    /// check it, `read` and they record it (T.2). A table of its own
    /// unless the loop binds the session's.
    pub seen: seen::SessionSeen,
    /// Where `recall` sends its query: the session's link to the master
    /// (`TurnHost::recall`); `None` where there is none.
    pub recall: Option<Arc<dyn harness_core::recall::RecallSource>>,
    /// Where `attached_file` sends its requests: the session's link to the
    /// master (`TurnHost::attached`); `None` where there is none.
    pub attached: Option<Arc<dyn harness_core::attached::AttachedSource>>,
    /// Where `history` reads the session's own chat (`TurnHost::history`);
    /// `None` where there is none.
    pub history: Option<Arc<dyn harness_core::history::HistorySource>>,
    /// Where `board` and `board_edit` send their requests: the session's
    /// link to the master (`TurnHost::board`); `None` where there is none.
    pub board: Option<Arc<dyn harness_core::board::BoardSource>>,
    /// Where `skill` asks for a stored skill's body (X.34 step B): the
    /// session's link to the master (`TurnHost::stored_skills`); `None`
    /// where there is none.
    pub stored_skills: Option<Arc<dyn harness_core::skills::StoredSkillSource>>,
    /// The agent may call `read` here (its tools include it): only then
    /// does `edit`'s refusal of an unread file show the file's lines.
    pub read_offered: bool,
    /// A `read` of a file would run right now with no card and no policy
    /// refusal (the chat's mode, the node's `denied_tools`; the loop asks
    /// the host). `edit`'s early refusal of an unread file (its `refusal`,
    /// before any card) shows the file's lines only then; otherwise it
    /// gives the plain refusal and shows nothing (T45, T51).
    pub read_unasked: bool,
    /// `bash` runs in the read-only shell ([`sandbox::Grants::read_only`],
    /// T.7): set by the node's loop for a chat in plan mode, never from the
    /// call's arguments.
    pub read_only_shell: bool,
}

/// The marker entry in [`ToolCtx::observed`] after which the entries are
/// text of pages `web_fetch` returned: a URL they list is vouched, but only
/// by a page, and the web tools count what is fetched on that word alone
/// (review of PR 179, H1). Before it: the person's words and search results.
pub const PAGE_MARK: &str = "\u{1}pages\u{1}";

/// `observed` in two parts: what the person and a search gave, and what
/// fetched pages said.
pub fn split_observed(observed: &[String]) -> (&[String], &[String]) {
    match observed.iter().position(|t| t == PAGE_MARK) {
        Some(i) => (&observed[..i], &observed[i + 1..]),
        None => (observed, &[]),
    }
}

/// `strong` and `pages` as one `observed` list ([`split_observed`] undoes it).
pub fn join_observed(mut strong: Vec<String>, pages: Vec<String>) -> Vec<String> {
    if !pages.is_empty() {
        strong.push(PAGE_MARK.to_string());
        strong.extend(pages);
    }
    strong
}

/// Counts of what the web tools did in one turn, for the per-turn caps.
#[derive(Debug, Default)]
pub struct TurnBudget {
    /// `web_fetch` calls whose URL only a fetched page vouched for.
    pub page_fetches: std::sync::atomic::AtomicUsize,
    /// `web_search` calls.
    pub searches: std::sync::atomic::AtomicUsize,
}

impl TurnBudget {
    /// Take one from `counter` if it is below `cap`; says whether it did.
    pub fn take(counter: &std::sync::atomic::AtomicUsize, cap: usize) -> bool {
        use std::sync::atomic::Ordering::SeqCst;
        let mut n = counter.load(SeqCst);
        loop {
            if n >= cap {
                return false;
            }
            match counter.compare_exchange(n, n + 1, SeqCst, SeqCst) {
                Ok(_) => return true,
                Err(now) => n = now,
            }
        }
    }
}

impl ToolCtx {
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        Self {
            workspace: workspace.into(),
            cancel: CancellationToken::new(),
            output: None,
            allow_outside_workspace: false,
            protected: Arc::default(),
            timeout: background::DEFAULT_TIMEOUT,
            bash_timeout_max: Duration::from_secs(bash::DEFAULT_TIMEOUT_MAX_S),
            env: ToolEnv::default(),
            observed: Arc::default(),
            budget: Arc::default(),
            savings: Default::default(),
            outputs: None,
            refs: Arc::default(),
            artifacts: None,
            call_id: String::new(),
            sandbox: Default::default(),
            pin: None,
            owned: None,
            identity: None,
            background: Default::default(),
            skills: None,
            seen: Default::default(),
            recall: None,
            attached: None,
            history: None,
            board: None,
            stored_skills: None,
            read_offered: true,
            read_unasked: true,
            read_only_shell: false,
        }
    }

    /// The session folder's real path, checked against [`Self::pin`] when
    /// one is set: a folder replaced since the session was created (renamed
    /// away, a link or another directory in its place) is refused, so
    /// nothing is read, written or started there. Every tool goes through
    /// this before it touches the folder.
    pub fn workspace_dir(&self) -> Result<PathBuf, String> {
        match &self.pin {
            Some(pin) => pin.check(),
            None => self
                .workspace
                .canonicalize()
                .map_err(|e| format!("workspace: {e}")),
        }
    }

    /// The citation label for a URL (see [`harness_core::refs::RefBook`]).
    pub fn label(&self, url: &str) -> String {
        self.refs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .label(url)
    }

    /// Resolve a user/model-supplied path against the workspace and enforce
    /// confinement. Works for paths that do not exist yet.
    ///
    /// The path is walked one component at a time with `lstat`. Inside the
    /// workspace a symlink is followed only when its target exists and is
    /// inside the workspace too; a dangling link is refused wherever it is
    /// in the path, because whatever creates its target later (a `write`, a
    /// `mkdir -p`) would land wherever the link points. The result is the
    /// real path: existing part canonical, the part that does not exist yet
    /// appended.
    ///
    /// In every mode, `allow_outside_workspace` included, a path whose real
    /// location is inside a [`Self::protected`] folder is refused.
    pub fn resolve(&self, p: &str) -> Result<PathBuf, String> {
        // First of all: the folder is still the session's own (pin).
        let ws = self.workspace_dir()?;
        if self.allow_outside_workspace {
            let n = self.normalized(&ws, p);
            // Nothing else is refused here, but where the path really leads
            // (links followed, dangling ones by their target) must not be
            // the data directory.
            self.refuse_protected(&n, p)?;
            if !self.protected.is_empty() {
                self.refuse_protected(&walk(&n, None, p)?, p)?;
            }
            return Ok(n);
        }
        // Relative paths start at the real workspace, so the walk is always
        // over an absolute path whatever `workspace` looks like.
        let raw = Path::new(p);
        let normalized = normalize(&if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            ws.join(raw)
        });
        let real = walk(&normalized, Some(&ws), p)?;
        self.refuse_protected(&real, p)?;
        if real.starts_with(&ws) {
            Ok(real)
        } else {
            Err(format!(
                "path {} is outside the session workspace {}",
                p,
                ws.display()
            ))
        }
    }

    /// [`Self::resolve`] for a path about to be written: also refuses any
    /// path inside a `.git` directory, in every mode. The repository's git
    /// config and hooks run commands (`core.fsmonitor`, `pre-commit`) the
    /// next time anyone runs git there, outside every approval.
    ///
    /// The caller must still open the file without following a symlink at
    /// the last component ([`open_nofollow`]): the link could be swapped in
    /// after this check.
    pub fn resolve_for_write(&self, p: &str) -> Result<PathBuf, String> {
        let path = self.resolve(p)?;
        // In outside mode `resolve` does not look at links; find where the
        // path really goes so a link to some `.git` is no way around this.
        let real = if self.allow_outside_workspace {
            walk(&path, None, p).unwrap_or_else(|_| path.clone())
        } else {
            path.clone()
        };
        let ws = self.workspace_dir()?;
        // Only components below the workspace count when it is inside, so
        // a workspace that itself lives under some `.git` still works.
        let checked = real.strip_prefix(&ws).unwrap_or(&real);
        let in_git = checked.components().any(|c| match c {
            Component::Normal(n) => n.to_str().is_some_and(|n| n.eq_ignore_ascii_case(".git")),
            _ => false,
        });
        if in_git {
            return Err(format!(
                "refusing to write {p}: it is inside a .git directory. The repository's git \
                 configuration and hooks can run commands, so they are read-only to the agent; \
                 ask the user to make this change"
            ));
        }
        Ok(path)
    }

    /// The path the file tools' record and lock are keyed on: the real
    /// path. In the workspace [`Self::resolve`] already gives it; in
    /// outside mode it does not follow links, so it is walked here, and two
    /// spellings of one file share one record and one lock.
    pub(crate) fn seen_key(&self, path: &Path) -> PathBuf {
        if self.allow_outside_workspace {
            walk(path, None, "").unwrap_or_else(|_| path.to_path_buf())
        } else {
            path.to_path_buf()
        }
    }

    /// A real path as the model addresses it, for the `[path#TAG]` header:
    /// relative to the session folder when the file is inside it (the string
    /// it passes as `path`, and fewer tokens in every read), else as it is.
    /// Display only: the table of what was seen stays keyed by the real path.
    pub(crate) fn model_path(&self, path: &Path) -> PathBuf {
        let inside = |base: &Path| {
            path.strip_prefix(base)
                .ok()
                .filter(|r| !r.as_os_str().is_empty())
        };
        inside(&self.workspace)
            .map(Path::to_path_buf)
            .or_else(|| {
                let base = self.workspace.canonicalize().ok()?;
                inside(&base).map(Path::to_path_buf)
            })
            .unwrap_or_else(|| path.to_path_buf())
    }

    /// Refuse `real` (a real or normalized path) inside a protected folder.
    fn refuse_protected(&self, real: &Path, shown: &str) -> Result<(), String> {
        // A sub-agent's own folder lies in a protected folder by design;
        // only that folder is let through, for that session.
        if let Some(o) = &self.owned
            && real.starts_with(&o.top)
        {
            return Ok(());
        }
        match self.protected.iter().find(|d| real.starts_with(d)) {
            Some(d) => Err(format!(
                "path {shown} is inside the harness's own data directory {} (keys, tokens, \
                 every user's chats); file tools never read or change it",
                d.display()
            )),
            None => Ok(()),
        }
    }

    fn normalized(&self, ws: &Path, p: &str) -> PathBuf {
        let raw = Path::new(p);
        let joined = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            ws.join(raw)
        };
        normalize(&joined)
    }
}

/// Walk `path` (absolute, already normalized) one component at a time with
/// `lstat`, following symlinks, and return the real path. With `ws`, a
/// symlink met inside `ws` must resolve to an existing target inside `ws`.
/// Without it, nothing is refused and a dangling link ends the walk (the
/// rest is appended as it is).
fn walk(path: &Path, ws: Option<&Path>, shown: &str) -> Result<PathBuf, String> {
    walk_links(path, ws, shown, 0)
}

/// Dangling links followed by their target, one after another (outside
/// mode), before the walk gives up on a loop.
const MAX_LINK_HOPS: usize = 40;

fn walk_links(path: &Path, ws: Option<&Path>, shown: &str, hops: usize) -> Result<PathBuf, String> {
    let mut real = PathBuf::new();
    let mut missing = false;
    for c in path.components() {
        let name = match c {
            Component::Normal(n) => n,
            other => {
                real.push(other.as_os_str());
                continue;
            }
        };
        if missing {
            real.push(name);
            continue;
        }
        let candidate = real.join(name);
        match std::fs::symlink_metadata(&candidate) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                missing = true;
                real = candidate;
            }
            Err(e) => return Err(format!("{}: {e}", candidate.display())),
            Ok(md) if md.file_type().is_symlink() => {
                let inside = ws.is_some_and(|ws| real.starts_with(ws));
                match candidate.canonicalize() {
                    Ok(target) => {
                        if inside
                            && let Some(ws) = ws
                            && !target.starts_with(ws)
                        {
                            return Err(format!(
                                "path {shown}: {} is a symlink to {}, outside the session \
                                 workspace {}; refusing to follow it",
                                candidate.display(),
                                target.display(),
                                ws.display()
                            ));
                        }
                        real = target;
                    }
                    Err(e) if ws.is_some() => {
                        return Err(format!(
                            "path {shown}: {} is a symlink whose target cannot be resolved \
                             ({e}); refusing to follow a dangling symlink",
                            candidate.display()
                        ));
                    }
                    // A dangling link counts as where it points: whatever
                    // creates its target later lands there, so that is the
                    // path to judge (the data-dir and `.git` rules).
                    Err(_) => {
                        if hops >= MAX_LINK_HOPS {
                            return Err(format!("path {shown}: too many levels of symbolic links"));
                        }
                        let target = std::fs::read_link(&candidate)
                            .map_err(|e| format!("{}: {e}", candidate.display()))?;
                        real = walk_links(&normalize(&real.join(target)), None, shown, hops + 1)?;
                        missing = true;
                    }
                }
            }
            Ok(_) => real = candidate,
        }
    }
    Ok(real)
}

/// Open `path` without following a symlink at its last component (`write`
/// and `edit`): a link put there after [`ToolCtx::resolve_for_write`]
/// checked the path fails with a clear error instead of being followed.
///
/// Only regular files: an existing FIFO or device is refused before it is
/// opened, the open never blocks (`O_NONBLOCK`), and the opened handle is
/// checked again (`fstat`) for one swapped in meanwhile.
pub(crate) async fn open_nofollow(
    path: &Path,
    opts: &mut tokio::fs::OpenOptions,
) -> Result<tokio::fs::File, String> {
    if let Ok(md) = tokio::fs::symlink_metadata(path).await
        && !md.file_type().is_symlink()
    {
        check_regular(path, md.file_type())?;
    }
    opts.custom_flags(nix::fcntl::OFlag::O_NOFOLLOW.bits() | open_flags());
    let f = opts.open(path).await.map_err(|e| {
        if e.raw_os_error() == Some(nix::errno::Errno::ELOOP as i32) {
            format!("{}: is a symlink; refusing to follow it", path.display())
        } else {
            format!("{}: {e}", path.display())
        }
    })?;
    let md = f
        .metadata()
        .await
        .map_err(|e| format!("{}: {e}", path.display()))?;
    check_regular(path, md.file_type())?;
    Ok(f)
}

fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolOutcome {
    pub ok: bool,
    pub output: String,
    pub exit_code: Option<i32>,
    pub truncated: bool,
    /// Pages behind the `[S<n>]` labels in `output` (web tools only).
    pub sources: Vec<Source>,
    /// A `bash` call that left a background command running: the loop
    /// records `background_started` before this call's result.
    pub background: Option<background::Started>,
    /// A `todo` call's accepted list: the loop records `todo_updated`
    /// before this call's result.
    pub todo: Option<Vec<harness_core::todo::TodoItem>>,
    /// The file a `read`, `write` or `edit` left the agent knowing: the
    /// loop records it with the call's result (`tool_call_finished.seen`).
    pub seen: Option<harness_core::event::FileSeen>,
    /// The person (or the approval path) denied the call: it did not run.
    /// Set by the loop where it handles the denial, never read from the
    /// text (a command could print the same words). The repeated-call
    /// guard's second rule does not count it.
    pub denied: bool,
    /// The image a `read` of an image file found (X.23): the loop decides
    /// whether the model gets it (its model takes images, the caps), keeps
    /// the bytes with the chat and records only a reference. Never part of
    /// `output`.
    pub image: Option<FileImage>,
    /// A line of the harness's own about this call, from a fixed text the
    /// tool owns (never from the command's output or the model): the loop
    /// sends it to the model after the result as a note, unescaped, and
    /// records it as the call's `note`. Not part of `output`, whose
    /// `[harness]` lines the model reads as forged.
    pub note: Option<String>,
}

/// An image file as `read` found it: its bytes as read (at most
/// [`harness_core::image::MAX_BYTES`]), checked by their magic number and
/// header, never decoded.
#[derive(Clone, PartialEq, Eq)]
pub struct FileImage {
    pub mime: &'static str,
    /// Hex SHA-256 of `bytes`.
    pub sha256: String,
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for FileImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileImage")
            .field("mime", &self.mime)
            .field("sha256", &self.sha256)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

impl ToolOutcome {
    pub fn ok(output: impl Into<String>) -> Self {
        let (output, truncated) = cap(output.into());
        Self {
            ok: true,
            output,
            truncated,
            ..Default::default()
        }
    }

    pub fn err(output: impl Into<String>) -> Self {
        let (output, truncated) = cap(output.into());
        Self {
            output,
            truncated,
            ..Default::default()
        }
    }
}

/// Keep head and tail if over the cap.
pub fn cap(s: String) -> (String, bool) {
    if s.chars().count() <= MAX_OUTPUT_CHARS {
        return (s, false);
    }
    let half = MAX_OUTPUT_CHARS / 2;
    let head: String = s.chars().take(half).collect();
    let tail: String = {
        let v: Vec<char> = s.chars().rev().take(half).collect();
        v.into_iter().rev().collect()
    };
    (
        format!("{head}\n\n[... output truncated ...]\n\n{tail}"),
        true,
    )
}

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    /// Built-in names are fixed; an MCP tool's (`mcp__<server>__<tool>`,
    /// [`mcp`]) comes from its server's list.
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    /// JSON Schema of the input object.
    fn schema(&self) -> Value;
    /// The schema shown to the model on this node: the same, with this
    /// node's `bash` timeout ceiling in the text that states one.
    fn schema_for(&self, _bash_timeout_max: Duration) -> Value {
        self.schema()
    }
    fn risk(&self) -> ToolRisk;
    /// The call's arguments with other harnesses' names for them read as
    /// this tool's own (`file_text` for `write`'s `content`), applied once
    /// by the loop before the schema check, the card, the approval key and
    /// every policy check, so all of them see the tool's own names only.
    /// The schema offered to the model does not list the aliases. The
    /// default changes nothing.
    fn normalize_args(&self, _input: &mut Value) -> Result<(), String> {
        Ok(())
    }
    /// Whether calls of this tool may run at the same time as other such
    /// calls of the same step (the loop's parallel batch, PLAN.md "Event
    /// order of a parallel step"). Only a [`ToolRisk::ReadOnly`] tool's
    /// answer counts, and only a tool that starts no process, writes
    /// nothing, streams no live output and shares no numbering with other
    /// calls (web citation labels) may say yes. `false`, the default, keeps
    /// a tool added later in order until it says so (an allow-list).
    fn parallel_safe(&self) -> bool {
        false
    }
    /// Offered to the model only when this returns true (tools that exist
    /// for a token-saving method are off with it).
    fn enabled(&self, _savings: &harness_core::savings::Savings) -> bool {
        true
    }
    /// The refusal `run` would give `input` whatever anyone answers, read
    /// from the call and the node's configuration in `ctx` alone (a path or
    /// folder out of reach): asked before the approval card, so no card asks
    /// about a call that cannot happen (PLAN.md T51). `None`, the default,
    /// is no opinion. It only ever refuses earlier: `run` checks again, as
    /// the file system can change while a card waits.
    fn refusal(&self, _ctx: &ToolCtx, _input: &Value) -> Option<String> {
        None
    }
    /// Text for the PERSON deciding on this call's approval card, computed
    /// from the call and the files as they are now (the rows a line edit
    /// replaces). Never sent to the model, never part of the approval key,
    /// and it may be file content: shown as plain text only. Bounded by the
    /// tool. Changes and records nothing. `None`, the default, is none; a
    /// call `refusal` refuses gets none either (no card is shown).
    fn preview(&self, _ctx: &ToolCtx, _input: &Value) -> Option<String> {
        None
    }
    async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome;

    /// Where the tool runs, when its name does not say it (MCP tools: a
    /// node server and its source, or a workspace's connector). Part of
    /// every approval key of its calls ([`approval::keyed`]). `None`, the
    /// default: the name says it.
    fn approval_origin(&self) -> Option<String> {
        None
    }
}

/// [`Tool::refusal`] of a tool whose `run` starts by resolving its `path`
/// argument with [`ToolCtx::resolve`] (or, for a write,
/// [`ToolCtx::resolve_for_write`]): that same check's error.
pub(crate) fn path_refusal(ctx: &ToolCtx, input: &Value, write: bool) -> Option<String> {
    let p = input.get("path")?.as_str()?;
    if write {
        ctx.resolve_for_write(p).err()
    } else {
        ctx.resolve(p).err()
    }
}

pub type ToolSet = Vec<Arc<dyn Tool>>;

pub fn builtin() -> ToolSet {
    vec![
        Arc::new(Bash),
        Arc::new(BashOutput),
        Arc::new(BashStop),
        Arc::new(Read),
        Arc::new(Write),
        Arc::new(Edit),
        Arc::new(Grep),
        Arc::new(Glob),
        Arc::new(ToolOutput),
        Arc::new(Outline),
        Arc::new(Publish),
        Arc::new(todo::Todo),
        Arc::new(skills::SkillTool),
    ]
}

pub(crate) fn str_arg<'a>(input: &'a Value, key: &str) -> Result<&'a str, String> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing string argument `{key}`"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_confines_to_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx::new(dir.path());
        assert!(ctx.resolve("a/b.txt").is_ok());
        assert!(ctx.resolve("../escape").is_err());
        assert!(ctx.resolve("/etc/passwd").is_err());
        let inside = dir.path().join("x");
        assert!(ctx.resolve(inside.to_str().unwrap()).is_ok());
    }

    #[test]
    fn resolve_blocks_symlink_escape() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
        let ctx = ToolCtx::new(dir.path());
        assert!(ctx.resolve("link/file").is_err());
    }

    #[test]
    fn resolve_refuses_dangling_links_anywhere_in_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path().join("nope"), dir.path().join("dl")).unwrap();
        let ctx = ToolCtx::new(dir.path());
        let last = ctx.resolve("dl").unwrap_err();
        assert!(last.contains("symlink"), "{last}");
        assert!(ctx.resolve("dl/x/y").is_err());
        // A dangling link that would point inside is refused too: its
        // target cannot be checked until it exists.
        std::os::unix::fs::symlink(dir.path().join("later"), dir.path().join("in")).unwrap();
        assert!(ctx.resolve("in").is_err());
    }

    #[test]
    fn resolve_follows_links_that_stay_inside() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("real")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("real"), dir.path().join("l")).unwrap();
        let ctx = ToolCtx::new(dir.path());
        let r = ctx.resolve("l/new.txt").unwrap();
        assert_eq!(r, dir.path().canonicalize().unwrap().join("real/new.txt"));
    }

    /// A home with a project and the harness's data directory beside it
    /// (AUDIT S1): `(home, project, data)`, all canonical.
    fn home_with_data() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let home = tempfile::tempdir().unwrap();
        let h = home.path().canonicalize().unwrap();
        let data = h.join(".local/share/rizzy-harness");
        std::fs::create_dir_all(data.join("keys")).unwrap();
        std::fs::write(data.join("api-token"), "token-value-123456").unwrap();
        std::fs::write(data.join("keys/jwt.key"), "jwt-key-value-123456").unwrap();
        let project = h.join("projects/x");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("main.rs"), "fn main() {}\n").unwrap();
        (home, project, data)
    }

    fn protected_ctx(project: &Path, data: &Path, outside: bool) -> ToolCtx {
        ToolCtx {
            allow_outside_workspace: outside,
            protected: Arc::new(vec![data.to_path_buf()]),
            ..ToolCtx::new(project)
        }
    }

    fn run(tool: &dyn Tool, ctx: &ToolCtx, input: Value) -> ToolOutcome {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(tool.run(ctx, input))
    }

    #[test]
    fn file_tools_never_touch_the_data_dir() {
        let (_home, project, data) = home_with_data();
        let token = data.join("api-token");
        for outside in [false, true] {
            let ctx = protected_ctx(&project, &data, outside);
            for p in [
                "../../.local/share/rizzy-harness/api-token".to_string(),
                token.display().to_string(),
                data.join("keys/jwt.key").display().to_string(),
                data.join("new-file").display().to_string(),
                data.display().to_string(),
            ] {
                let e = ctx.resolve(&p).unwrap_err();
                assert!(e.contains("data directory"), "outside={outside} {p}: {e}");
                let r = run(&Read, &ctx, serde_json::json!({ "path": p }));
                assert!(!r.ok && !r.output.contains("token-value"), "{}", r.output);
                let w = run(
                    &Write,
                    &ctx,
                    serde_json::json!({ "path": p, "content": "x" }),
                );
                assert!(!w.ok, "outside={outside} {p}: {}", w.output);
                let e = run(
                    &Edit,
                    &ctx,
                    serde_json::json!({ "path": p, "old_string": "token", "new_string": "x" }),
                );
                assert!(!e.ok, "outside={outside} {p}: {}", e.output);
                let g = run(
                    &Grep,
                    &ctx,
                    serde_json::json!({ "pattern": "value", "path": p }),
                );
                assert!(!g.output.contains("value-123456"), "{}", g.output);
            }
            assert_eq!(
                std::fs::read_to_string(&token).unwrap(),
                "token-value-123456"
            );
            assert!(!data.join("new-file").exists());
            // Files of the project itself still work.
            let r = run(&Read, &ctx, serde_json::json!({ "path": "main.rs" }));
            assert!(r.ok, "{}", r.output);
        }
    }

    #[test]
    fn a_link_into_the_data_dir_is_refused() {
        let (_home, project, data) = home_with_data();
        std::os::unix::fs::symlink(&data, project.join("d")).unwrap();
        std::os::unix::fs::symlink(data.join("api-token"), project.join("t")).unwrap();
        // A dangling link whose target would be created inside it.
        std::os::unix::fs::symlink(data.join("later"), project.join("later")).unwrap();
        for outside in [false, true] {
            let ctx = protected_ctx(&project, &data, outside);
            for p in ["d/api-token", "t", "later/x"] {
                assert!(ctx.resolve(p).is_err(), "outside={outside} {p}");
                let r = run(&Read, &ctx, serde_json::json!({ "path": p }));
                assert!(!r.ok && !r.output.contains("token-value"), "{}", r.output);
            }
            let w = run(
                &Write,
                &ctx,
                serde_json::json!({ "path": "later/x", "content": "x" }),
            );
            assert!(!w.ok, "{}", w.output);
            assert!(!data.join("later").exists());
        }
    }

    #[test]
    fn searches_from_above_skip_the_data_dir() {
        let (home, project, data) = home_with_data();
        let ctx = protected_ctx(&project, &data, true);
        let root = home.path().display().to_string();
        let g = run(
            &Grep,
            &ctx,
            serde_json::json!({ "pattern": "value|main", "path": root }),
        );
        assert!(g.output.contains("main.rs"), "{}", g.output);
        assert!(!g.output.contains("value-123456"), "{}", g.output);
        let l = run(
            &Glob,
            &ctx,
            serde_json::json!({ "pattern": "**/*", "path": root }),
        );
        assert!(l.output.contains("main.rs"), "{}", l.output);
        assert!(!l.output.contains("api-token"), "{}", l.output);
        assert!(!l.output.contains("jwt.key"), "{}", l.output);
    }

    #[test]
    fn outcomes_carry_no_sources_unless_a_tool_adds_them() {
        assert!(ToolOutcome::default().sources.is_empty());
        assert!(ToolOutcome::ok("x").sources.is_empty());
        assert!(!ToolOutcome::err("x").ok);
    }

    #[test]
    fn calls_of_one_turn_share_labels() {
        let a = ToolCtx::new("/");
        let b = ToolCtx {
            refs: a.refs.clone(),
            ..ToolCtx::new("/")
        };
        assert_eq!(a.label("https://a.example/"), "S1");
        assert_eq!(b.label("https://b.example/"), "S2");
        assert_eq!(b.label("https://a.example/"), "S1");
        assert_eq!(ToolCtx::new("/").label("https://b.example/"), "S1");
    }

    #[test]
    fn cap_keeps_head_and_tail() {
        let s = "a".repeat(MAX_OUTPUT_CHARS) + &"b".repeat(100);
        let (out, t) = cap(s);
        assert!(t);
        assert!(out.starts_with('a') && out.ends_with('b'));
    }
}
