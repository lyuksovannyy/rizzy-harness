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
    read: Vec<PathBuf>,
    write: Vec<PathBuf>,
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
        let mut read: Vec<PathBuf> = SYSTEM_READ
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
                        "node.toml [sandbox] {key}: {} would make the harness's own data \
                         directory {} visible to the agent; list a folder that neither contains \
                         it nor is inside it",
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
        read.extend(settings.read.iter().cloned());
        Ok(Self {
            exe,
            read,
            write: settings.write.clone(),
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
    fn wrapper(&self, rw: &[PathBuf], ro: &[PathBuf], status: bool) -> std::process::Command {
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
        for p in rw.iter().chain(&self.write) {
            c.arg("--rw").arg(p);
        }
        for d in DEVICES {
            c.arg("--rw").arg(d);
        }
        for p in self.read.iter().chain(ro) {
            c.arg("--ro").arg(p);
        }
        c.arg("--");
        c
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
        let mut c = self.wrapper(&[scratch.to_path_buf()], &[], true);
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
}

/// Where a sub-agent worktree keeps its git metadata in the main
/// repository: `<repo>/.git/harness-worktrees/<id>/…` gives the main
/// `<repo>/.git` and the worktree's own admin folder
/// `<repo>/.git/worktrees/<id>` (the node checks, when it makes the
/// worktree, that git named it so). Judged from the path alone, never from
/// a `.git` file's contents (repository content could name any folder
/// there).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeGit {
    /// The main repository's git directory.
    pub git: PathBuf,
    /// This worktree's folder in it (`HEAD`, `index`, its reflog).
    pub admin: PathBuf,
    /// The worktree's top folder (`<git>/harness-worktrees/<id>`).
    pub top: PathBuf,
}

/// The [`WorktreeGit`] of `cwd`, when it is inside a sub-agent worktree.
pub fn worktree_git(cwd: &Path) -> Option<WorktreeGit> {
    let mut p = cwd;
    while let Some(parent) = p.parent() {
        if parent.file_name() == Some(OsStr::new("harness-worktrees"))
            && let Some(git) = parent.parent()
            && git.file_name() == Some(OsStr::new(".git"))
            && let Some(id) = p.file_name()
        {
            return Some(WorktreeGit {
                git: git.to_path_buf(),
                admin: git.join("worktrees").join(id),
                top: p.to_path_buf(),
            });
        }
        p = parent;
    }
    None
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
                let mut rw = grants.owned_write.clone();
                let mut ro = Vec::new();
                let mut checked = vec![ws.clone()];
                checked.extend(grants.read.iter().cloned());
                if grants.workspace_write {
                    rw.push(ws.clone());
                    // A sub-agent's worktree keeps its git metadata in the
                    // main repository's `.git`. Its own git (`status`, `add`,
                    // `commit`) needs to write only the object store and the
                    // worktree's own admin folder; the rest of `.git` (the
                    // config, hooks, `info`, refs, `packed-refs`, and the
                    // folder that holds the worktree) is read-only, so the
                    // child can neither configure commands for anyone's git
                    // nor move its own folder away (R.1, PLAN.md T32).
                    // Landlock only adds rights per subtree: the read-only
                    // rule covers all of `.git`, the two read-write rules
                    // add writing beneath `objects` and `worktrees/<id>`.
                    if let Some(wt) = worktree_git(&ws) {
                        checked.push(wt.git.clone());
                        rw.push(wt.git.join("objects"));
                        rw.push(wt.admin);
                        ro.push(wt.git);
                    }
                } else {
                    ro.push(ws.clone());
                }
                ro.extend(grants.read.iter().cloned());
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
                    rw.push(t.clone());
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
/// `[sandbox]` entry that would expose the data directory).
pub fn start(
    settings: &Settings,
    data_dir: &Path,
    protected: &[PathBuf],
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

/// The arguments of `sandbox-exec`.
#[derive(Debug, Default)]
pub struct ExecRequest {
    pub rw: Vec<PathBuf>,
    pub ro: Vec<PathBuf>,
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

#[cfg(target_os = "linux")]
mod imp {
    use super::ExecRequest;
    use landlock::{
        ABI, Access, AccessFs, AccessNet, BitFlags, CompatLevel, Compatible, PathBeneath, PathFd,
        Ruleset, RulesetAttr, RulesetCreatedAttr, RulesetStatus, Scope,
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
        for (paths, access) in [(&req.ro, read), (&req.rw, all)] {
            for p in paths {
                let meta = match std::fs::metadata(p) {
                    Ok(m) => m,
                    // Nothing to grant; without a rule it stays denied.
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(err) => return Err(format!("{}: {err}", p.display())),
                };
                let access: BitFlags<AccessFs> = if meta.is_dir() {
                    access
                } else {
                    access & AccessFs::from_file(NEWEST)
                };
                let fd = PathFd::new(p).map_err(|e| e.to_string())?;
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

    #[test]
    fn a_data_dir_under_a_system_folder_is_refused() {
        let s = Settings::default();
        let e =
            Sandbox::new("/bin/true".into(), &s, None, &["/opt/rizzy/data".into()]).unwrap_err();
        assert!(e.contains("/opt"), "{e}");
    }

    #[test]
    fn worktree_git_comes_from_the_path_only() {
        let wt = WorktreeGit {
            git: PathBuf::from("/r/.git"),
            admin: PathBuf::from("/r/.git/worktrees/abc"),
            top: PathBuf::from("/r/.git/harness-worktrees/abc"),
        };
        assert_eq!(
            worktree_git(Path::new("/r/.git/harness-worktrees/abc/sub")),
            Some(wt.clone())
        );
        assert_eq!(
            worktree_git(Path::new("/r/.git/harness-worktrees/abc")),
            Some(wt)
        );
        assert_eq!(worktree_git(Path::new("/r/.git/harness-worktrees")), None);
        assert_eq!(worktree_git(Path::new("/r/harness-worktrees/abc")), None);
        assert_eq!(worktree_git(Path::new("/r/src")), None);
    }

    /// S-4: the wrapper runs in the session folder, so a relative data
    /// directory would make the temp root (and the protected folder) a path
    /// inside it. Refused in every mode, before anything runs.
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
