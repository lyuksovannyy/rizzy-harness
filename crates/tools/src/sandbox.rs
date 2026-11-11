//! The agent's shell in a Landlock sandbox (FIX_PLAN 2.7, PLAN.md R2).
//!
//! Tool child processes (bash, graphify) are not started directly: the node
//! starts its own binary as `rizzy-harness sandbox-exec --rw <dir>… --ro
//! <dir>… -- <cmd>`, which applies a Landlock ruleset to itself and then
//! `exec`s the command. No `unsafe`, no `pre_exec`, no extra program on the
//! node; Landlock domains are inherited across `exec` and `fork` and can only
//! be narrowed, so nothing the command runs later (a nested `sandbox-exec`
//! included) gets more than its parent had.
//!
//! What a command may touch: read-write its session folder and a per-session
//! temp directory (`TMPDIR`); read-only the system folders in
//! [`SYSTEM_READ`] and node.toml's `[sandbox] read`; read-write node.toml's
//! `[sandbox] write`; the device files in [`DEVICES`]. Nothing else: `~/.ssh`,
//! `~/.aws`, the data directory and other users' folders cannot be opened.
//!
//! **Enforced** means: the kernel applied the filesystem rights of Landlock
//! ABI 3 (read, write, execute, create, remove, rename/link across folders,
//! truncate) in full, `no_new_privs` is set, and, with `network = "off"`, the
//! TCP connect and bind rights of ABI 4. In `mode = "required"` anything less
//! is an error (`sandbox-exec` exits [`EXIT_UNCONFINED`] without running the
//! command). Newer rights are added best-effort on top: device ioctls (ABI 5),
//! abstract Unix socket and signal scoping (ABI 6), connecting to pathname
//! Unix sockets (ABI 9). Below ABI 9 a pathname socket (`/run/user/<uid>/bus`,
//! `/var/run/docker.sock`, an ssh-agent) can still be connected to: an open
//! residual, see PLAN.md R2.

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// The subcommand name of the wrapper.
pub const SUBCOMMAND: &str = "sandbox-exec";

/// Exit code of `sandbox-exec` when it cannot confine the command as asked
/// (the command did not run).
pub const EXIT_UNCONFINED: i32 = 125;

/// First word of the line `sandbox-exec --status` prints on stderr.
pub const STATUS_PREFIX: &str = "rizzy-harness sandbox status:";

/// Read-only for every sandboxed command (those that exist). `/proc` is
/// needed by ordinary tools (`/proc/self`, `/dev/fd`); other processes'
/// `environ`, `fd`, `mem` and `cwd` stay closed because Landlock denies
/// ptrace-class access to any process outside the domain.
pub const SYSTEM_READ: &[&str] = &[
    "/usr", "/bin", "/sbin", "/lib", "/lib32", "/lib64", "/libx32", "/etc", "/opt", "/proc",
];

/// Read-write device files. Not `/dev/tty`: the harness's controlling
/// terminal, when it has one, is the user's shell; reading it steals their
/// keystrokes and `TIOCSTI` types into it.
pub const DEVICES: &[&str] = &[
    "/dev/null",
    "/dev/zero",
    "/dev/full",
    "/dev/random",
    "/dev/urandom",
];

/// `[sandbox] mode` in node.toml.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Refuse agentic sessions unless the sandbox is enforced.
    #[default]
    Required,
    /// Use whatever the kernel enforces; warn about the rest.
    BestEffort,
    /// No sandbox (an explicit, logged choice).
    Off,
}

/// `[sandbox] network` in node.toml (FIX_PLAN Q6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Network {
    #[default]
    On,
    /// Deny TCP connect and bind (Landlock ABI 4). UDP, and so DNS, is not
    /// covered by Landlock.
    Off,
}

/// The node's sandbox settings, paths absolute (from node.toml).
#[derive(Debug, Clone, Default)]
pub struct Settings {
    pub mode: Mode,
    pub network: Network,
    pub read: Vec<PathBuf>,
    pub write: Vec<PathBuf>,
}

/// A working sandbox: the wrapper binary and the node-wide rules.
#[derive(Debug)]
pub struct Sandbox {
    exe: PathBuf,
    /// The system folders ([`SYSTEM_READ`]) that exist.
    read: Vec<PathBuf>,
    /// node.toml's `[sandbox] read` and `write`.
    extra_read: Vec<Extra>,
    extra_write: Vec<Extra>,
    network: Network,
    best_effort: bool,
    /// Per-session temp directories live under it (inside the data dir).
    tmp_root: Option<PathBuf>,
    protected: Vec<PathBuf>,
}

/// Lexically normalized absolute path (`.` and `..` folded).
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

/// Where `p` really is: canonical if it exists, else normalized.
fn real(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| normalize(p))
}

/// The protected folder a grant on `p` would expose: `p` equals one, is
/// inside one, or contains one (Landlock has no deny rules, so a grant on
/// `$HOME` would open the data directory inside it).
fn exposes<'a>(p: &Path, protected: &'a [PathBuf]) -> Option<&'a PathBuf> {
    let p = real(p);
    protected.iter().find(|d| {
        let now = real(d);
        [d.as_path(), now.as_path()]
            .iter()
            .any(|d| p.starts_with(d) || d.starts_with(&p))
    })
}

/// Refuse a node.toml `[sandbox] read`/`write` entry that equals or holds
/// a `workspace_roots` entry (Gate 3 fix review 5, R1d-1): every command
/// would get every other chat's folder. An entry *inside* a root is not
/// refused (with the default root, the home folder, every toolchain entry
/// the README lists is inside it); it is bound to its identity instead
/// ([`Extra::grant`]).
pub fn check_extras_against_roots(settings: &Settings, roots: &[PathBuf]) -> Result<(), String> {
    for (key, list, what) in [
        ("read", &settings.read, "read"),
        ("write", &settings.write, "read and change"),
    ] {
        for p in list {
            let rp = real(p);
            if let Some(r) = roots
                .iter()
                .find(|r| r.starts_with(&rp) || real(r).starts_with(&rp))
            {
                return Err(format!(
                    "node.toml [sandbox] {key}: {} holds {}, a workspace_roots entry, so every \
                     chat's commands could {what} every other chat's folder; list a folder that \
                     does not contain a workspace root",
                    p.display(),
                    r.display()
                ));
            }
        }
    }
    Ok(())
}

/// A node.toml `[sandbox] read`/`write` entry, as it resolved when the node
/// started (R1d-1).
#[derive(Debug)]
struct Extra {
    /// Its real path at start (canonical, or normalized if it did not exist).
    real: PathBuf,
    label: &'static str,
}

impl Extra {
    fn new(p: &Path, key: &str) -> Self {
        Self {
            real: real(p),
            label: if key == "write" {
                "a node.toml [sandbox] write entry"
            } else {
                "a node.toml [sandbox] read entry"
            },
        }
    }

    /// Its grant for the next command: only while it still resolves to
    /// where it did at start (a link put in its place, or in place of a
    /// folder above it, resolves elsewhere and gets nothing), bound to what
    /// is there now, so the wrapper refuses a swap after this look. A file
    /// rewritten by renaming a new one over it keeps its grant; an entry
    /// missing now gets nothing until it is back.
    fn grant(&self) -> Option<Grant> {
        let now = self.real.canonicalize().ok()?;
        if now != self.real {
            return None;
        }
        let id = harness_core::folder::Identity::of(&now).ok()?;
        Some(Grant::identified(&now, id, self.label))
    }
}

impl Sandbox {
    /// `exe` is the harness binary (resolved once at start); `protected` the
    /// folders no grant may expose. Refuses a `read`/`write` entry or a
    /// system folder that would expose a protected one, and relative entries.
    pub fn new(
        exe: PathBuf,
        settings: &Settings,
        tmp_root: Option<PathBuf>,
        protected: &[PathBuf],
    ) -> Result<Self, String> {
        // Every grant is passed to the wrapper, which runs in the session
        // folder: a relative path there would name a folder inside it (and
        // a link planted there could lead anywhere, R.1 S-4).
        for p in tmp_root.iter().chain(protected) {
            if !p.is_absolute() {
                return Err(format!(
                    "the sandbox needs absolute paths for the harness's own folders; {} is \
                     relative (start the harness with an absolute --data-dir)",
                    p.display()
                ));
            }
        }
        let read: Vec<PathBuf> = SYSTEM_READ
            .iter()
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .collect();
        for (key, list) in [("read", &settings.read), ("write", &settings.write)] {
            for p in list {
                if !p.is_absolute() {
                    return Err(format!(
                        "node.toml [sandbox] {key}: {} is not an absolute path",
                        p.display()
                    ));
                }
                if let Some(d) = exposes(p, protected) {
                    return Err(format!(
                        "node.toml [sandbox] {key}: {} would make {} (the harness's own data \
                         directory, or a protected_dirs entry) visible to the agent; list a \
                         folder that neither contains it nor is inside it",
                        p.display(),
                        d.display()
                    ));
                }
            }
        }
        for p in &read {
            if let Some(d) = exposes(p, protected) {
                return Err(format!(
                    "the harness's data directory {} is inside {}, which the sandbox grants \
                     read-only to every command; move the data directory (--data-dir)",
                    d.display(),
                    p.display()
                ));
            }
        }
        Ok(Self {
            exe,
            read,
            extra_read: settings
                .read
                .iter()
                .map(|p| Extra::new(p, "read"))
                .collect(),
            extra_write: settings
                .write
                .iter()
                .map(|p| Extra::new(p, "write"))
                .collect(),
            network: settings.network,
            best_effort: settings.mode == Mode::BestEffort,
            tmp_root,
            protected: protected.to_vec(),
        })
    }

    pub fn network(&self) -> Network {
        self.network
    }

    /// The wrapper's command line up to and including `--`.
    fn wrapper(&self, rw: &[Grant], ro: &[Grant], status: bool) -> std::process::Command {
        let mut c = std::process::Command::new(&self.exe);
        c.arg(SUBCOMMAND);
        if self.best_effort {
            c.arg("--best-effort");
        }
        if self.network == Network::Off {
            c.arg("--no-network");
        }
        if status {
            c.arg("--status");
        }
        // The system folders: not session-writable. node.toml's extras may
        // lie where a session can write (inside a workspace root, inside
        // another `write` entry): each by its identity (R1d-1).
        let extra_rw: Vec<Grant> = self.extra_write.iter().filter_map(Extra::grant).collect();
        let extra_ro: Vec<Grant> = self
            .read
            .iter()
            .map(|p| Grant::system(p))
            .chain(self.extra_read.iter().filter_map(Extra::grant))
            .collect();
        for g in rw.iter().chain(&extra_rw).chain(&devices()) {
            g.push_args(&mut c, "--rw");
        }
        for g in extra_ro.iter().chain(ro) {
            g.push_args(&mut c, "--ro");
        }
        c.arg("--");
        c
    }

    /// The wrapper for one of the node's own helpers (its git for
    /// checkpoints, R.1 R1b-2), up to and including `--`: exactly `rw` and
    /// `ro` on top of the system folders ([`SYSTEM_READ`]) and the device
    /// files, and none of node.toml's `[sandbox] read`/`write` (those are
    /// for the agent's toolchains). Nothing here checks the grants against
    /// the protected folders: the caller passes only its own folders, each
    /// with the identity it expects (R1c-1).
    pub fn helper(&self, rw: &[Grant], ro: &[Grant]) -> std::process::Command {
        let mut c = std::process::Command::new(&self.exe);
        c.arg(SUBCOMMAND);
        if self.best_effort {
            c.arg("--best-effort");
        }
        if self.network == Network::Off {
            c.arg("--no-network");
        }
        for g in rw.iter().chain(&devices()) {
            g.push_args(&mut c, "--rw");
        }
        let system: Vec<Grant> = SYSTEM_READ
            .iter()
            .map(Path::new)
            .filter(|p| p.exists())
            .map(Grant::system)
            .collect();
        for g in system.iter().chain(ro) {
            g.push_args(&mut c, "--ro");
        }
        c.arg("--");
        c
    }

    /// Whether this sandbox only does what the kernel enforces
    /// (`best-effort`): on a kernel without Landlock it confines nothing.
    pub fn is_best_effort(&self) -> bool {
        self.best_effort
    }

    /// Run a fixed check through the wrapper: a file in a granted folder
    /// must be readable, `canary` (a file this writes inside `canary_dir`,
    /// the data directory) must not. Returns the wrapper's status line.
    pub fn probe(&self, scratch: &Path, canary_dir: &Path) -> Result<String, String> {
        std::fs::create_dir_all(scratch).map_err(|e| format!("{}: {e}", scratch.display()))?;
        let ok = scratch.join("granted");
        std::fs::write(&ok, "ok").map_err(|e| format!("{}: {e}", ok.display()))?;
        let canary = canary_dir.join("sandbox-canary");
        std::fs::write(&canary, "the sandbox must not read this\n")
            .map_err(|e| format!("{}: {e}", canary.display()))?;
        let mut c = self.wrapper(&[Grant::made(scratch, "the self-test folder")?], &[], true);
        c.args([
            "sh",
            "-c",
            "cat \"$1\" >/dev/null || exit 2; if cat \"$2\" >/dev/null 2>&1; then exit 3; fi",
            "sh",
        ])
        .arg(&ok)
        .arg(&canary)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .current_dir(scratch)
        .stdin(std::process::Stdio::null());
        let out = c.output();
        let _ = std::fs::remove_file(&canary);
        let _ = std::fs::remove_dir_all(scratch);
        let out =
            out.map_err(|e| format!("cannot start {} {SUBCOMMAND}: {e}", self.exe.display()))?;
        let stderr = String::from_utf8_lossy(&out.stderr);
        let status = stderr
            .lines()
            .find_map(|l| l.strip_prefix(STATUS_PREFIX))
            .map(|s| s.trim().to_string());
        match (out.status.code(), status) {
            (Some(0), Some(s)) => Ok(s),
            (Some(3), _) => Err(format!(
                "the sandbox self-test could read {} (Landlock not enforced)",
                canary.display()
            )),
            (Some(2), _) => Err(format!(
                "the sandbox self-test could not read a granted file: {}",
                stderr.trim()
            )),
            (code, _) => Err(format!(
                "the sandbox self-test failed (exit {code:?}): {}",
                stderr.trim()
            )),
        }
    }
}

/// How a tool's child processes are started.
#[derive(Debug, Clone, Default)]
pub enum Confinement {
    /// Directly (sandbox `off`, and unit tests).
    #[default]
    Unconfined,
    /// Through `sandbox-exec`; `tmp` is the session's temp directory.
    Confined {
        sandbox: Arc<Sandbox>,
        tmp: Option<PathBuf>,
    },
    /// Not at all: the sandbox is required and does not work here.
    Refused(Arc<str>),
}

/// What a command gets on top of the node-wide rules.
#[derive(Debug, Default)]
pub struct Grants {
    /// Read-write on the session folder (bash); otherwise read-only.
    pub workspace_write: bool,
    /// Harness-owned folders inside the data directory (graphify's output):
    /// read-write, exempt from the protected-folder check.
    pub owned_write: Vec<PathBuf>,
    /// More read-only folders (checked against the protected ones).
    pub read: Vec<PathBuf>,
    /// A sub-agent's node-owned folder (see `harness_core::folder::
    /// OwnedFolder`), when the session is one.
    pub owned: Option<harness_core::folder::OwnedFolder>,
}

impl Confinement {
    /// This confinement for one session: its temp directory is
    /// `<tmp_root>/<key>`.
    pub fn for_session(&self, key: &str) -> Self {
        match self {
            Self::Confined { sandbox, .. } => Self::Confined {
                sandbox: sandbox.clone(),
                tmp: sandbox.tmp_root.as_ref().map(|r| r.join(key)),
            },
            other => other.clone(),
        }
    }

    /// `program args…` in `workspace`, with exactly the allow-listed
    /// environment `env`, confined as this says. With `pin` (the session's
    /// folder as pinned at its creation), a folder that is no longer that
    /// directory is refused before anything is granted or started; the
    /// command then starts in the checked real path.
    #[allow(clippy::too_many_arguments)]
    pub fn command(
        &self,
        program: &OsStr,
        args: &[&OsStr],
        workspace: &Path,
        pin: Option<&harness_core::folder::FolderPin>,
        env: &crate::ToolEnv,
        protected: &[PathBuf],
        grants: Grants,
    ) -> Result<tokio::process::Command, String> {
        if let Self::Refused(why) = self {
            return Err(why.to_string());
        }
        let ws = match pin {
            Some(p) => p.check()?,
            None => workspace
                .canonicalize()
                .map_err(|e| format!("workspace {}: {e}", workspace.display()))?,
        };
        let std_cmd = match self {
            Self::Refused(why) => return Err(why.to_string()),
            Self::Unconfined => {
                let mut c = std::process::Command::new(program);
                env.apply_std(&mut c);
                c
            }
            Self::Confined { sandbox, tmp } => {
                let all: Vec<PathBuf> =
                    sandbox.protected.iter().chain(protected).cloned().collect();
                // Every grant carries the identity the node expects (R1c-1):
                // the session's folder its pin's, the node's own folders
                // theirs, the system and node.toml folders the marker that
                // no session can write them.
                let ws_id = match pin {
                    Some(p) => p.identity(),
                    None => harness_core::folder::Identity::of(&ws)?,
                };
                let mut rw: Vec<Grant> = grants
                    .owned_write
                    .iter()
                    .map(|p| Grant::made(p, "the tool's output folder"))
                    .collect::<Result<_, _>>()?;
                let mut ro: Vec<Grant> = Vec::new();
                // A sub-agent's own folder lies in the node's protected
                // sessions folder by design: it alone is exempt from the
                // check, and only for the session the node made it for (the
                // node passes it; nothing is judged from the path or from a
                // file the session can write).
                let owned = grants.owned.as_ref().filter(|o| ws.starts_with(&o.top));
                let mut checked = Vec::new();
                if owned.is_none() {
                    checked.push(ws.clone());
                }
                checked.extend(grants.read.iter().cloned());
                let ws_grant = Grant::identified(&ws, ws_id, "the session's folder");
                if grants.workspace_write {
                    rw.push(ws_grant);
                } else {
                    ro.push(ws_grant);
                }
                // The child's own git directory is in its folder; when the
                // child works in a sub-folder, `.git` is granted too (its git
                // writes the index and objects there) and the rest of the
                // folder read-only. The user's object store, which that git
                // directory borrows for history, is read-only, and granted
                // only while it is still the store the node accepted: one
                // replaced since (a link put in its place by a process of the
                // parent's) is left out, so the child loses history and gains
                // nothing; the wrapper checks the identity again on the
                // descriptor it opens. Nothing of the user's `.git` is
                // writable (R.1, PLAN.md T32).
                if let Some(o) = owned {
                    if ws != o.top {
                        ro.push(Grant::identified(
                            &o.top,
                            o.top_id,
                            "the sub-agent's folder",
                        ));
                        if grants.workspace_write {
                            rw.push(Grant::identified(
                                &o.git,
                                o.git_id,
                                "the sub-agent's git directory",
                            ));
                        }
                    }
                    if let Some((objects, id)) = &o.objects
                        && harness_core::folder::Identity::of(objects).ok() == Some(*id)
                    {
                        checked.push(objects.clone());
                        ro.push(Grant::identified(
                            objects,
                            *id,
                            "the repository's object store",
                        ));
                    }
                }
                ro.extend(grants.read.iter().map(|p| Grant::system(p)));
                for p in &checked {
                    if let Some(d) = exposes(p, &all) {
                        return Err(format!(
                            "refusing to run: {} would expose the harness's data directory {}",
                            p.display(),
                            d.display()
                        ));
                    }
                }
                if let Some(t) = tmp {
                    make_private_dir(t)?;
                    rw.push(Grant::made(t, "the session's temp folder")?);
                }
                let mut c = sandbox.wrapper(&rw, &ro, false);
                c.arg(program);
                env.apply_std(&mut c);
                if let Some(t) = tmp {
                    c.env("TMPDIR", t);
                }
                c
            }
        };
        let mut c = tokio::process::Command::from(std_cmd);
        c.args(args).current_dir(&ws);
        Ok(c)
    }
}

/// Create `dir` (and parents) with mode 0700.
fn make_private_dir(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("{}: {e}", dir.display()))
}

/// The outcome of [`start`].
#[derive(Debug)]
pub struct Started {
    pub confinement: Confinement,
    /// Set when agentic sessions must be refused (mode `required`, sandbox
    /// not enforced); the text names the node.toml setting.
    pub refusal: Option<String>,
    /// One line for the log and `doctor`.
    pub summary: String,
    /// Where per-session temp directories live.
    pub tmp_root: Option<PathBuf>,
}

/// Set up the node's sandbox once at start: resolve the wrapper binary
/// (`current_exe`, canonical; a binary replaced by an upgrade is picked up
/// at the same path, a deleted one makes every command fail closed), build
/// the rules and run the self-test. `Err` is a configuration error (a
/// `[sandbox]` entry that would expose the data directory or a protected
/// folder, or that holds a workspace root).
pub fn start(
    settings: &Settings,
    data_dir: &Path,
    protected: &[PathBuf],
    workspace_roots: &[PathBuf],
    node_toml: &Path,
) -> Result<Started, String> {
    if !data_dir.is_absolute() {
        return Err(format!(
            "the data directory {} is a relative path; the sandbox needs it absolute",
            data_dir.display()
        ));
    }
    let tmp_root = data_dir.join("sandbox-tmp");
    if settings.mode == Mode::Off {
        return Ok(Started {
            confinement: Confinement::Unconfined,
            refusal: None,
            summary: format!(
                "OFF ([sandbox] mode = \"off\" in {}): the agent's shell runs with every right \
                 of this OS user",
                node_toml.display()
            ),
            tmp_root: None,
        });
    }
    check_extras_against_roots(settings, workspace_roots)?;
    let exe = std::env::current_exe()
        .and_then(|e| e.canonicalize())
        .map_err(|e| format!("cannot find the harness binary for {SUBCOMMAND}: {e}"));
    let sandbox = match exe {
        Ok(exe) => Sandbox::new(exe, settings, Some(tmp_root.clone()), protected)?,
        Err(e) => return Ok(not_enforced(settings, e, node_toml, None)),
    };
    let scratch = tmp_root.join(format!("probe-{}", std::process::id()));
    let canary_dir = protected.first().map_or(data_dir, |p| p.as_path());
    let sandbox = Arc::new(sandbox);
    match sandbox.probe(&scratch, canary_dir) {
        Ok(status) if settings.mode == Mode::Required => Ok(Started {
            confinement: Confinement::Confined { sandbox, tmp: None },
            refusal: None,
            summary: format!("enforced ({status})"),
            tmp_root: Some(tmp_root),
        }),
        Ok(status) => Ok(Started {
            confinement: Confinement::Confined { sandbox, tmp: None },
            refusal: None,
            summary: format!("best-effort ({status}): only what the kernel enforces"),
            tmp_root: Some(tmp_root),
        }),
        Err(e) => Ok(not_enforced(settings, e, node_toml, Some(sandbox))),
    }
}

fn not_enforced(
    settings: &Settings,
    why: String,
    node_toml: &Path,
    sandbox: Option<Arc<Sandbox>>,
) -> Started {
    let tmp_root = sandbox.as_ref().and_then(|s| s.tmp_root.clone());
    match (settings.mode, sandbox) {
        (Mode::BestEffort, Some(sandbox)) => Started {
            confinement: Confinement::Confined { sandbox, tmp: None },
            refusal: None,
            summary: format!(
                "NOT ENFORCED, running anyway ([sandbox] mode = \"best-effort\"): {why}"
            ),
            tmp_root,
        },
        _ => {
            let network = if settings.network == Network::Off {
                " (`network = \"off\"` needs Landlock ABI 4, Linux 6.7)"
            } else {
                ""
            };
            let msg = format!(
                "this node refuses agentic sessions: the shell sandbox does not work here \
                 ({why}){network}. It needs Linux 6.2 or newer with Landlock enabled (`landlock` \
                 in /sys/kernel/security/lsm). To run anyway, set `mode = \"best-effort\"` or \
                 `mode = \"off\"` under [sandbox] in {}",
                node_toml.display()
            );
            Started {
                confinement: Confinement::Refused(msg.clone().into()),
                refusal: Some(msg.clone()),
                summary: format!("REFUSING agentic sessions: {msg}"),
                tmp_root,
            }
        }
    }
}

/// Remove every per-session temp directory (at node start, before any
/// session runs).
pub fn reset_tmp(root: &Path) -> Result<(), String> {
    match std::fs::remove_dir_all(root) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{}: {e}", root.display())),
    }
    make_private_dir(root)
}

/// What a grant expects to find at its path (R.1, R1c-1). Every grant
/// names one, so a new grant cannot forget the question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    /// The directory or file with this identity, taken when the node pinned
    /// or made it (a session's folder, a sub-agent's folder and its `.git`,
    /// the borrowed object store as accepted, a shadow, a temp folder, a
    /// node.toml `[sandbox] read`/`write` entry as it is now, R1d-1). The
    /// wrapper opens the path once without following a link at its last
    /// component, compares the opened descriptor's device and inode, and
    /// builds the rule from that same descriptor: whoever re-points the path
    /// gets the whole command refused, never a grant on the new target.
    Identity(harness_core::folder::Identity),
    /// A path no session can write (the system folders, the device files,
    /// graphify's install): opened as it is, links followed (`/lib64` is a
    /// link on most systems).
    NotSessionWritable,
}

/// One folder or file the wrapper is asked to grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub path: PathBuf,
    pub expect: Expect,
    /// What it is, for the refusal ("the folder granted as <label> was
    /// replaced").
    pub label: String,
}

impl Grant {
    pub fn system(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            expect: Expect::NotSessionWritable,
            label: "system".into(),
        }
    }

    pub fn identified(path: &Path, id: harness_core::folder::Identity, label: &str) -> Self {
        Self {
            path: path.to_path_buf(),
            expect: Expect::Identity(id),
            label: label.into(),
        }
    }

    /// A folder the node made itself (no session can write where it lies):
    /// its identity now.
    pub fn made(path: &Path, label: &str) -> Result<Self, String> {
        Ok(Self::identified(
            path,
            harness_core::folder::Identity::of(path)?,
            label,
        ))
    }

    /// `--rw|--ro <expect> <path>`: `<expect>` is `system`, or
    /// `<dev>:<ino>:<label>`.
    fn push_args(&self, c: &mut std::process::Command, flag: &str) {
        let spec = match self.expect {
            Expect::NotSessionWritable => "system".to_string(),
            Expect::Identity(id) => format!("{}:{}:{}", id.dev, id.ino, self.label),
        };
        c.arg(flag).arg(spec).arg(&self.path);
    }

    /// The grants of `sandbox-exec`'s arguments: pairs of `<expect> <path>`.
    pub fn parse_pairs(pairs: &[OsString]) -> Result<Vec<Self>, String> {
        if !pairs.len().is_multiple_of(2) {
            return Err("a grant is `<expect> <path>`".into());
        }
        pairs
            .chunks(2)
            .map(|c| {
                let spec = c[0].to_str().ok_or("a grant's identity is not text")?;
                let path = PathBuf::from(&c[1]);
                if spec == "system" {
                    return Ok(Self::system(&path));
                }
                let mut it = spec.splitn(3, ':');
                let (Some(dev), Some(ino), label) = (it.next(), it.next(), it.next()) else {
                    return Err(format!("bad grant identity {spec:?}"));
                };
                let dev = dev
                    .parse()
                    .map_err(|_| format!("bad grant identity {spec:?}"))?;
                let ino = ino
                    .parse()
                    .map_err(|_| format!("bad grant identity {spec:?}"))?;
                Ok(Self::identified(
                    &path,
                    harness_core::folder::Identity { dev, ino },
                    label.unwrap_or("a folder"),
                ))
            })
            .collect()
    }
}

/// The device files, as grants.
fn devices() -> Vec<Grant> {
    DEVICES
        .iter()
        .map(|d| Grant::system(Path::new(d)))
        .collect()
}

/// The arguments of `sandbox-exec`.
#[derive(Debug, Default)]
pub struct ExecRequest {
    pub rw: Vec<Grant>,
    pub ro: Vec<Grant>,
    pub no_network: bool,
    pub best_effort: bool,
    /// Print a [`STATUS_PREFIX`] line on stderr before running the command.
    pub status: bool,
    pub command: Vec<OsString>,
}

/// `sandbox-exec`: confine this process, then `exec` the command. Returns
/// only on failure, with the exit code to use. Must run before any other
/// thread exists (Landlock restricts the calling thread; `exec` then drops
/// the others anyway).
pub fn exec(req: ExecRequest) -> i32 {
    use std::os::unix::process::CommandExt;
    let Some((program, args)) = req.command.split_first() else {
        eprintln!("{SUBCOMMAND}: no command given");
        return 2;
    };
    match imp::restrict(&req) {
        Ok(status) => {
            if req.status {
                eprintln!("{STATUS_PREFIX} {status}");
            }
        }
        Err(e) => {
            eprintln!("{SUBCOMMAND}: cannot enforce the sandbox, not running the command: {e}");
            return EXIT_UNCONFINED;
        }
    }
    let err = std::process::Command::new(program).args(args).exec();
    eprintln!("{SUBCOMMAND}: {}: {err}", Path::new(program).display());
    if err.kind() == std::io::ErrorKind::NotFound {
        127
    } else {
        126
    }
}

/// Confine the calling thread (and what it later starts) by `req`'s rules
/// without running a command: for a helper that confines itself after
/// start-up (`pdf-text`, FIX_PLAN 3.4). Empty `ro`/`rw` means no filesystem
/// access at all. Call it while the process has one thread.
pub fn restrict_self(req: &ExecRequest) -> Result<String, String> {
    imp::restrict(req)
}

/// Open a grant's path for its Landlock rule, once (`O_PATH`): `None` when
/// nothing is there (a grant that does not exist gives nothing). A grant
/// with an expected identity is opened without following a link at its
/// last component, and the opened descriptor must have that device and
/// inode, else the whole command is refused, naming the grant (R1c-1).
/// Returns the descriptor and whether it is a directory.
pub(crate) fn open_grant(g: &Grant) -> Result<Option<(std::os::fd::OwnedFd, bool)>, String> {
    use nix::fcntl::OFlag;
    use nix::sys::stat::SFlag;
    let mut flags = OFlag::O_PATH | OFlag::O_CLOEXEC;
    if matches!(g.expect, Expect::Identity(_)) {
        flags |= OFlag::O_NOFOLLOW;
    }
    let fd = match nix::fcntl::open(&g.path, flags, nix::sys::stat::Mode::empty()) {
        Ok(fd) => fd,
        Err(nix::errno::Errno::ENOENT) if g.expect == Expect::NotSessionWritable => {
            return Ok(None);
        }
        Err(e) => {
            return Err(match g.expect {
                Expect::Identity(_) => format!(
                    "the folder granted as {} was replaced or removed ({}: {})",
                    g.label,
                    g.path.display(),
                    std::io::Error::from(e)
                ),
                Expect::NotSessionWritable => {
                    format!("{}: {}", g.path.display(), std::io::Error::from(e))
                }
            });
        }
    };
    let st = nix::sys::stat::fstat(&fd).map_err(|e| format!("{}: {e}", g.path.display()))?;
    if let Expect::Identity(id) = g.expect
        && (st.st_dev as u64 != id.dev || st.st_ino as u64 != id.ino)
    {
        return Err(format!(
            "the folder granted as {} was replaced ({} is not the one the node checked)",
            g.label,
            g.path.display()
        ));
    }
    let is_dir = SFlag::from_bits_truncate(st.st_mode) & SFlag::S_IFMT == SFlag::S_IFDIR;
    Ok(Some((fd, is_dir)))
}

#[cfg(target_os = "linux")]
mod imp {
    use super::ExecRequest;
    use landlock::{
        ABI, Access, AccessFs, AccessNet, BitFlags, CompatLevel, Compatible, PathBeneath, Ruleset,
        RulesetAttr, RulesetCreatedAttr, RulesetStatus, Scope,
    };

    /// Must be enforced in full (see the module docs).
    const REQUIRED: ABI = ABI::V3;
    /// The newest ABI this code knows; its extra rights are best-effort.
    const NEWEST: ABI = ABI::V9;

    pub fn restrict(req: &ExecRequest) -> Result<String, String> {
        let hard = if req.best_effort {
            CompatLevel::BestEffort
        } else {
            CompatLevel::HardRequirement
        };
        let e = |e: landlock::RulesetError| e.to_string();
        let mut rs = Ruleset::default()
            .set_compatibility(hard)
            .handle_access(AccessFs::from_all(REQUIRED))
            .map_err(e)?
            .set_compatibility(CompatLevel::BestEffort)
            .handle_access(AccessFs::from_all(NEWEST))
            .map_err(e)?;
        if req.no_network {
            rs = rs
                .set_compatibility(hard)
                .handle_access(AccessNet::from_all(ABI::V4))
                .map_err(e)?;
        }
        let mut created = rs
            .set_compatibility(CompatLevel::BestEffort)
            .scope(Scope::from_all(ABI::V6))
            .map_err(e)?
            .create()
            .map_err(e)?;
        let read = AccessFs::from_read(NEWEST);
        let all = AccessFs::from_all(NEWEST);
        for (grants, access) in [(&req.ro, read), (&req.rw, all)] {
            for g in grants {
                // Opened once; the identity is checked on this descriptor
                // and the rule is built from it, so the path is never
                // resolved twice (R1c-1).
                let Some((fd, is_dir)) = super::open_grant(g)? else {
                    // Nothing to grant; without a rule it stays denied.
                    continue;
                };
                let access: BitFlags<AccessFs> = if is_dir {
                    access
                } else {
                    access & AccessFs::from_file(NEWEST)
                };
                created = created.add_rule(PathBeneath::new(fd, access)).map_err(e)?;
            }
        }
        let status = created.restrict_self().map_err(e)?;
        if !req.best_effort {
            if status.ruleset == RulesetStatus::NotEnforced {
                return Err("the kernel enforced none of the rules".into());
            }
            if !status.no_new_privs {
                return Err("no_new_privs could not be set".into());
            }
        }
        Ok(format!(
            "landlock {:?}, ruleset {:?}, no_new_privs {}, network {}",
            status.landlock,
            status.ruleset,
            status.no_new_privs,
            if req.no_network { "off" } else { "on" }
        ))
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::ExecRequest;

    pub fn restrict(req: &ExecRequest) -> Result<String, String> {
        if req.best_effort {
            Ok("landlock unavailable (not Linux), nothing enforced".into())
        } else {
            Err("Landlock exists only on Linux".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grants_that_expose_the_data_dir_are_refused() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path().canonicalize().unwrap();
        let data = home.join(".local/share/rizzy-harness");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(home.join(".cargo")).unwrap();
        let protected = vec![data.clone()];
        let exe = PathBuf::from("/bin/true");
        for bad in [
            home.clone(),
            home.join(".local"),
            data.clone(),
            data.join("keys"),
            home.join(".cargo/../.local"),
            PathBuf::from("/"),
        ] {
            for write in [false, true] {
                let s = Settings {
                    read: if write { vec![] } else { vec![bad.clone()] },
                    write: if write { vec![bad.clone()] } else { vec![] },
                    ..Default::default()
                };
                let e = Sandbox::new(exe.clone(), &s, None, &protected).unwrap_err();
                assert!(e.contains("data directory"), "{e}");
                assert!(e.contains(if write { "write" } else { "read" }), "{e}");
            }
        }
        // A link to it is judged by where it leads.
        std::os::unix::fs::symlink(&data, home.join("link")).unwrap();
        let s = Settings {
            read: vec![home.join("link")],
            ..Default::default()
        };
        assert!(Sandbox::new(exe.clone(), &s, None, &protected).is_err());
        let s = Settings {
            read: vec![home.join(".cargo")],
            write: vec![home.join(".cache")],
            ..Default::default()
        };
        assert!(Sandbox::new(exe.clone(), &s, None, &protected).is_ok());
        let s = Settings {
            read: vec!["relative".into()],
            ..Default::default()
        };
        assert!(Sandbox::new(exe, &s, None, &protected).is_err());
    }

    /// The wrapper's arguments for a command with no grants of its own.
    fn wrapper_args(sb: &Sandbox) -> Vec<String> {
        sb.wrapper(&[], &[], false)
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    /// The `<expect>` the wrapper is given for `path`, if it is granted.
    fn expect_of(args: &[String], path: &Path) -> Option<String> {
        let p = path.to_string_lossy();
        args.iter()
            .position(|a| *a == p)
            .map(|i| args[i - 1].clone())
    }

    /// Gate 3 fix review 5, R1d-1: a node.toml extra was granted as a
    /// system path, opened following links. An extra inside a folder a
    /// session can write (its own folder, another `write` entry) could be
    /// replaced by a link, and the next command was granted the link's
    /// target. Now an extra is granted only while it resolves to where it
    /// did at start, bound to what is there now.
    #[test]
    fn an_extra_replaced_by_a_link_is_not_granted() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path().canonicalize().unwrap();
        let registry = home.join("code/.cargo/registry");
        let ssh = home.join(".ssh");
        let gitconfig = home.join("code/.gitconfig");
        std::fs::create_dir_all(&registry).unwrap();
        std::fs::create_dir_all(&ssh).unwrap();
        std::fs::write(&gitconfig, "[user]\n").unwrap();
        let s = Settings {
            read: vec![gitconfig.clone()],
            write: vec![registry.clone()],
            ..Default::default()
        };
        let sb = Sandbox::new("/bin/true".into(), &s, None, &[]).unwrap();
        let args = wrapper_args(&sb);
        for p in [&registry, &gitconfig] {
            let e = expect_of(&args, p).unwrap_or_else(|| panic!("{} not granted", p.display()));
            assert_ne!(e, "system", "{} is granted following links", p.display());
        }

        // A file rewritten the usual way (a new file renamed over it) is
        // still the same entry: granted, with its new identity.
        std::fs::write(home.join("code/.gitconfig.lock"), "[user]\nname = x\n").unwrap();
        std::fs::rename(home.join("code/.gitconfig.lock"), &gitconfig).unwrap();
        let id = harness_core::folder::Identity::of(&gitconfig).unwrap();
        assert_eq!(
            expect_of(&wrapper_args(&sb), &gitconfig).unwrap(),
            format!("{}:{}:a node.toml [sandbox] read entry", id.dev, id.ino)
        );

        // A process of a session in `code` swaps the entry for a link.
        std::fs::rename(&registry, home.join("code/.cargo/registry.old")).unwrap();
        std::os::unix::fs::symlink(&ssh, &registry).unwrap();
        // ...or a folder above it.
        std::fs::rename(&gitconfig, home.join("gitconfig.old")).unwrap();
        std::os::unix::fs::symlink(ssh.join("id_ed25519"), &gitconfig).unwrap();
        let args = wrapper_args(&sb);
        for p in [&registry, &gitconfig, &ssh, &ssh.join("id_ed25519")] {
            assert_eq!(expect_of(&args, p), None, "{} is granted", p.display());
        }
    }

    #[test]
    fn an_extra_that_holds_a_workspace_root_is_refused() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path().canonicalize().unwrap();
        let code = home.join("code");
        std::fs::create_dir_all(code.join("vendor")).unwrap();
        let roots = [code.clone()];
        for (key, bad) in [
            ("read", code.clone()),
            ("write", home.clone()),
            ("read", code.join("vendor/..")),
            ("write", PathBuf::from("/")),
        ] {
            let s = Settings {
                read: if key == "read" {
                    vec![bad.clone()]
                } else {
                    vec![]
                },
                write: if key == "write" {
                    vec![bad.clone()]
                } else {
                    vec![]
                },
                ..Default::default()
            };
            let e = check_extras_against_roots(&s, &roots).unwrap_err();
            assert!(e.contains(&format!("[sandbox] {key}")), "{e}");
            assert!(e.contains(&bad.display().to_string()), "{e}");
            assert!(e.contains("workspace_roots"), "{e}");
        }
        // Inside a root is not refused: it is bound to its identity instead
        // (`an_extra_replaced_by_a_link_is_not_granted`).
        let s = Settings {
            read: vec![code.join("vendor")],
            write: vec![home.join(".cache")],
            ..Default::default()
        };
        assert!(check_extras_against_roots(&s, &roots).is_ok());
    }

    #[test]
    fn a_data_dir_under_a_system_folder_is_refused() {
        let s = Settings::default();
        let e =
            Sandbox::new("/bin/true".into(), &s, None, &["/opt/rizzy/data".into()]).unwrap_err();
        assert!(e.contains("/opt"), "{e}");
    }

    #[test]
    fn a_relative_data_dir_is_refused_in_every_mode() {
        for mode in [Mode::Required, Mode::BestEffort, Mode::Off] {
            let s = Settings {
                mode,
                ..Default::default()
            };
            let e = start(
                &s,
                Path::new("data"),
                &[PathBuf::from("/abs/data")],
                &[],
                Path::new("/abs/node.toml"),
            )
            .unwrap_err();
            assert!(e.contains("relative"), "{mode:?}: {e}");
        }
        for (tmp, protected) in [
            (
                Some(PathBuf::from("data/sandbox-tmp")),
                PathBuf::from("/abs/data"),
            ),
            (
                Some(PathBuf::from("/abs/data/sandbox-tmp")),
                PathBuf::from("data"),
            ),
        ] {
            let e = Sandbox::new("/bin/true".into(), &Settings::default(), tmp, &[protected])
                .unwrap_err();
            assert!(e.contains("relative"), "{e}");
        }
    }

    #[test]
    fn refused_never_runs() {
        let c = Confinement::Refused("no sandbox here".into());
        let e = c
            .command(
                OsStr::new("true"),
                &[],
                Path::new("/"),
                None,
                &Default::default(),
                &[],
                Grants::default(),
            )
            .unwrap_err();
        assert_eq!(e, "no sandbox here");
        assert!(matches!(c.for_session("x"), Confinement::Refused(_)));
    }

    #[test]
    fn settings_parse_kebab_case() {
        #[derive(Deserialize)]
        struct T {
            mode: Mode,
            network: Network,
        }
        let t: T = toml_like(r#"{"mode":"best-effort","network":"off"}"#);
        assert_eq!(t.mode, Mode::BestEffort);
        assert_eq!(t.network, Network::Off);
    }

    fn toml_like<T: for<'de> Deserialize<'de>>(s: &str) -> T {
        serde_json::from_str(s).unwrap()
    }
}
