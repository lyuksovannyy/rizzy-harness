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
    /// Deny TCP connect and bind (Landlock ABI 4), and run the agent's
    /// contained shell in a network namespace of its own (bubblewrap, T.8b),
    /// which closes UDP and DNS too. Where the shell is not contained (the
    /// Landlock-only fallback, `contain = "off"`) and for the node's own
    /// helpers, only TCP: Landlock does not cover UDP, and so DNS.
    Off,
}

/// The node's sandbox settings, paths absolute (from node.toml).
#[derive(Debug, Clone, Default)]
pub struct Settings {
    pub mode: Mode,
    pub network: Network,
    pub read: Vec<PathBuf>,
    pub write: Vec<PathBuf>,
    /// `[sandbox] contain`: the agent shell's process containment (T.8a).
    pub contain: crate::contain::ContainMode,
    /// Where bubblewrap is (`[sandbox] bwrap`, absolute, taken as given);
    /// `None`: the first `bwrap` on `PATH` the node's user cannot replace,
    /// looked up at start.
    pub bwrap: Option<PathBuf>,
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
    /// The agent shell's containment, when the node has one (set once at
    /// start, from node.toml and the probe; nothing per session or per
    /// call can change it).
    contain: Option<crate::contain::Contain>,
    /// The harness binary as it was when first opened, held open for the
    /// process's life: the guard is started from it (`/proc/self/fd/<n>`),
    /// so a command
    /// that later replaces the file at `exe` cannot replace its guard
    /// (review T8E-5). For the node itself it is `/proc/self/exe`, its own
    /// running image.
    image: Option<std::os::fd::RawFd>,
}

/// The harness binary to start guards from, as a descriptor this process
/// holds until it exits (so its number can never name another file): the
/// running image when `exe` is this process's own binary (the node), else
/// `exe` as it is when first asked for (a test, whose own image is the
/// test binary). Opened once per path.
fn open_image(exe: &Path) -> Option<std::os::fd::RawFd> {
    use std::os::fd::AsRawFd;
    type Images = std::sync::Mutex<Vec<(PathBuf, std::fs::File)>>;
    static IMAGES: std::sync::OnceLock<Images> = std::sync::OnceLock::new();
    let mut images = IMAGES.get_or_init(Default::default).lock().ok()?;
    if let Some((_, f)) = images.iter().find(|(p, _)| p == exe) {
        return Some(f.as_raw_fd());
    }
    let own = std::env::current_exe()
        .and_then(|e| e.canonicalize())
        .ok()
        .is_some_and(|me| exe.canonicalize().is_ok_and(|e| e == me));
    let path = if own {
        Path::new("/proc/self/exe")
    } else {
        exe
    };
    let f = std::fs::File::open(path).ok()?;
    let fd = f.as_raw_fd();
    images.push((exe.to_owned(), f));
    Some(fd)
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

/// The start-up checks of node.toml's `[sandbox] read`/`write` entries
/// against the workspace roots and each other (Gate 3 fix reviews 5 and 6,
/// R1d-1 and R4-1). Refused, naming the key and the path:
///
/// * an entry that equals or holds a `workspace_roots` entry: every command
///   would get every other chat's folder;
/// * an entry inside another `write` entry: its parent is writable to every
///   command, so a command could replace it by a link; it is never needed,
///   the outer entry already grants it;
/// * an entry below a workspace root whose path holds a symbolic link
///   anywhere below the root (each component `lstat`ed): a chat there may
///   have put that link in place of a real folder while the node was down,
///   and the entry would be granted through it. List the real path instead.
///
/// An entry inside a root is not refused otherwise (with the default root,
/// the home folder, every toolchain entry the README lists is inside it);
/// while the node runs each entry is granted only while it still resolves
/// where it did at start ([`Extra::grant`]).
pub fn check_extras_against_roots(settings: &Settings, roots: &[PathBuf]) -> Result<(), String> {
    let entries: Vec<(&str, &PathBuf)> = settings
        .read
        .iter()
        .map(|p| ("read", p))
        .chain(settings.write.iter().map(|p| ("write", p)))
        .collect();
    for (key, p) in &entries {
        let what = if *key == "write" {
            "read and change"
        } else {
            "read"
        };
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
        for w in &settings.write {
            let rw = real(w);
            if rp != rw && rp.starts_with(&rw) {
                return Err(format!(
                    "node.toml [sandbox] {key}: {} is inside the [sandbox] write entry {}; every \
                     command may change that folder, so it could replace this entry by a link; \
                     remove it (the write entry already grants it)",
                    p.display(),
                    w.display()
                ));
            }
        }
        let lexical = normalize(p);
        for r in roots {
            let below = [normalize(r), real(r)]
                .into_iter()
                .find_map(|base| Some((lexical.strip_prefix(&base).ok()?.to_path_buf(), base)));
            let Some((rest, base)) = below else { continue };
            let mut at = base;
            for c in rest.components() {
                at.push(c);
                match std::fs::symlink_metadata(&at) {
                    Ok(md) if md.file_type().is_symlink() => {
                        return Err(format!(
                            "node.toml [sandbox] {key}: {} goes through the symbolic link {} \
                             inside the workspace root {}; a chat there could have put that link \
                             in place of a real folder; list the real path instead",
                            p.display(),
                            at.display(),
                            r.display()
                        ));
                    }
                    Ok(_) => {}
                    // Not there yet: nothing below it can be a link either.
                    Err(_) => break,
                }
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
            image: open_image(&exe),
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
            contain: None,
        })
    }

    /// This sandbox with the agent's shell contained by `c` (bubblewrap).
    pub fn with_containment(mut self, c: crate::contain::Contain) -> Self {
        self.contain = Some(c);
        self
    }

    /// The agent shell's containment, if any.
    pub fn containment(&self) -> Option<&crate::contain::Contain> {
        self.contain.as_ref()
    }

    pub fn network(&self) -> Network {
        self.network
    }

    /// The wrapper's command line up to and including `--`; with
    /// `contain`, inside bubblewrap started in the given folder by its
    /// guard (T.8e), both marked with this node's temp root and `session`.
    fn wrapper(
        &self,
        rw: &[Grant],
        ro: &[Grant],
        status: bool,
        contain: Option<(&crate::contain::Contain, &Path)>,
        session: &OsStr,
    ) -> std::process::Command {
        let mark = self.tmp_root.as_ref().map(|root| crate::contain::Mark {
            root: root.clone(),
            session: session.to_owned(),
        });
        let mut c = match contain {
            Some((b, cwd)) => b.command(
                &self.exe,
                self.image,
                cwd,
                mark.as_ref(),
                self.network == Network::Off && b.network_namespace(),
            ),
            None => std::process::Command::new(&self.exe),
        };
        c.arg(SUBCOMMAND);
        // In bubblewrap's command line (and its init's) too, where a
        // restarted node's sweep looks for it.
        if let (Some(_), Some(m)) = (contain, &mark) {
            m.push_args(&mut c);
        }
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
        let mut c = self.wrapper(
            &[Grant::made(scratch, "the self-test folder")?],
            &[],
            true,
            None,
            OsStr::new("probe"),
        );
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

    /// Run a fixed check through bubblewrap and the wrapper, as a shell
    /// command runs: the command must be in a PID namespace of its own
    /// and, for [`crate::contain::Form::Full`], see its own `/proc`; with
    /// the network off, in a network namespace with only its own `lo`
    /// (T.8b), so a bubblewrap that cannot make one is not used.
    pub fn probe_containment(
        &self,
        contain: &crate::contain::Contain,
        scratch: &Path,
    ) -> Result<(), String> {
        std::fs::create_dir_all(scratch).map_err(|e| format!("{}: {e}", scratch.display()))?;
        let ok = scratch.join("granted");
        std::fs::write(&ok, "ok").map_err(|e| format!("{}: {e}", ok.display()))?;
        let full = contain.form() == crate::contain::Form::Full;
        let mut c = self.wrapper(
            &[Grant::made(scratch, "the self-test folder")?],
            &[],
            false,
            Some((contain, scratch)),
            OsStr::new("probe"),
        );
        // Through the guard, as every command: its channel stays open (no
        // kill order) until the probe has ended.
        let (keep, theirs) = std::os::unix::net::UnixStream::pair()
            .map_err(|e| format!("the guard's channel: {e}"))?;
        let net = self.network == Network::Off && contain.network_namespace();
        c.args(["sh", "-c", &crate::contain::probe_script(full, net), "sh"])
            .arg(&ok)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .current_dir(scratch)
            .stdin(std::process::Stdio::from(std::os::fd::OwnedFd::from(
                theirs,
            )));
        let out = c.output();
        drop(keep);
        let _ = std::fs::remove_dir_all(scratch);
        let out = out.map_err(|e| format!("cannot start {}: {e}", contain.bwrap().display()))?;
        let stderr = String::from_utf8_lossy(&out.stderr);
        match out.status.code() {
            Some(0) => Ok(()),
            Some(4) => Err("the command was not in a PID namespace of its own".into()),
            Some(5) => Err("/proc is not the namespace's own".into()),
            Some(6) => Err("the command was not in a network namespace of its own".into()),
            code => Err(format!(
                "{} failed (exit {code:?}): {}",
                contain.bwrap().display(),
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
    /// The marker of this confinement's contained commands (T.8e): the
    /// node's temp root and the session; `None` when there is none.
    pub fn mark(&self) -> Option<crate::contain::Mark> {
        match self {
            Self::Confined { sandbox, tmp } => Some(crate::contain::Mark {
                root: sandbox.tmp_root.clone()?,
                session: tmp
                    .as_ref()
                    .and_then(|t| t.file_name())
                    .unwrap_or(OsStr::new("-"))
                    .to_owned(),
            }),
            _ => None,
        }
    }

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
        self.build(program, args, workspace, pin, env, protected, grants, false)
            .map(|(c, _)| c)
    }

    /// The agent's shell: as [`Self::command`], inside the node's process
    /// containment when it has one (T.8a). Returns whether it is contained;
    /// a contained command must be started with [`Self::spawn`]. Whether
    /// there is a containment is the node's alone: nothing a call or a
    /// session passes reaches this decision.
    #[allow(clippy::too_many_arguments)]
    pub fn shell(
        &self,
        program: &OsStr,
        args: &[&OsStr],
        workspace: &Path,
        pin: Option<&harness_core::folder::FolderPin>,
        env: &crate::ToolEnv,
        protected: &[PathBuf],
        grants: Grants,
    ) -> Result<(tokio::process::Command, bool), String> {
        self.build(program, args, workspace, pin, env, protected, grants, true)
    }

    /// Start a command from [`Self::shell`]: a contained one with its
    /// guard's control channel (`crate::contain::spawn`, T.8e).
    pub async fn spawn(
        mut cmd: tokio::process::Command,
        contained: bool,
    ) -> std::io::Result<crate::bash::ShellChild> {
        if contained {
            crate::contain::spawn(cmd).map(crate::bash::ShellChild::Contained)
        } else {
            cmd.spawn().map(crate::bash::ShellChild::Plain)
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        &self,
        program: &OsStr,
        args: &[&OsStr],
        workspace: &Path,
        pin: Option<&harness_core::folder::FolderPin>,
        env: &crate::ToolEnv,
        protected: &[PathBuf],
        grants: Grants,
        shell: bool,
    ) -> Result<(tokio::process::Command, bool), String> {
        if let Self::Refused(why) = self {
            return Err(why.to_string());
        }
        let ws = match pin {
            Some(p) => p.check()?,
            None => workspace
                .canonicalize()
                .map_err(|e| format!("workspace {}: {e}", workspace.display()))?,
        };
        let mut contained = false;
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
                let contain = sandbox.contain.as_ref().filter(|_| shell);
                contained = contain.is_some();
                let session = tmp
                    .as_ref()
                    .and_then(|t| t.file_name())
                    .unwrap_or(OsStr::new("-"));
                let mut c =
                    sandbox.wrapper(&rw, &ro, false, contain.map(|b| (b, ws.as_path())), session);
                c.arg(program);
                env.apply_std(&mut c);
                if let Some(t) = tmp {
                    c.env("TMPDIR", t);
                }
                c
            }
        };
        let mut c = tokio::process::Command::from(std_cmd);
        // A contained command's guard starts in `/`: bubblewrap enters the
        // folder (`--chdir`), so a folder that hangs (a stalled network
        // mount) stalls the guard, never the node's spawn (review N-2).
        c.args(args)
            .current_dir(if contained { Path::new("/") } else { &ws });
        Ok((c, contained))
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

/// How the containment line is marked in `doctor` and the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainLevel {
    /// Contained, and the claim holds in full.
    Ok,
    /// Contained with a named gap, or not contained (`auto`, `off`).
    Warn,
    /// Agentic sessions are refused for it (`contain = "required"`).
    Refused,
}

/// The effective Landlock ABI in the wrapper's status line
/// (`effective_abi: V9` gives 9), 0 when there is none.
pub fn effective_abi(status: &str) -> u32 {
    status
        .split("effective_abi: V")
        .nth(1)
        .unwrap_or("0")
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

/// The Landlock ABI from which a command can no longer reach a service of
/// the node user over a pathname Unix socket (the session bus), and the one
/// from which abstract sockets are closed (review T8A-1).
pub const ABI_PATHNAME_SOCKETS: u32 = 9;
pub const ABI_ABSTRACT_SOCKETS: u32 = 6;

/// What the containment line says for a working containment of `form` on
/// a kernel with Landlock ABI `abi` (review T8A-1). Below ABI 9 it is a
/// warning that names the hole: bubblewrap ends every process in its
/// namespace, but a command can ask a service outside it to start one.
pub fn contained_line(
    form: crate::contain::Form,
    abi: u32,
    bwrap: &Path,
) -> (String, ContainLevel) {
    let what = match form {
        crate::contain::Form::Full => format!(
            "contained ({}: a PID namespace and /proc of its own per command",
            bwrap.display()
        ),
        crate::contain::Form::SharedProc => format!(
            "contained without a /proc of its own ({}: a PID namespace per command, but a \
             fresh /proc could not be mounted, so /proc lists this machine's processes",
            bwrap.display()
        ),
    };
    if abi >= ABI_PATHNAME_SOCKETS {
        return (
            format!("{what}; nothing a command starts outlives it)"),
            ContainLevel::Ok,
        );
    }
    let abstract_too = if abi < ABI_ABSTRACT_SOCKETS {
        ", or over an abstract socket (X11 and others)"
    } else {
        ""
    };
    (
        format!(
            "{what}) WITH A GAP: on this kernel (Landlock ABI {abi}, below 9) a command can \
             still start a process outside the containment, which then outlives it and runs \
             without the sandbox, by asking a service of this user over a Unix socket (the \
             session bus at /run/user/<uid>/bus, a container runtime's socket){abstract_too}. \
             Closed by Linux 7.1 or newer (Landlock ABI 9), or by the planned file view (T.8c); \
             README, \"The shell sandbox\""
        ),
        ContainLevel::Warn,
    )
}

/// The outcome of [`start`].
#[derive(Debug)]
pub struct Started {
    pub confinement: Confinement,
    /// Set when agentic sessions must be refused (mode `required`, sandbox
    /// not enforced; or `contain = "required"` and no containment); the
    /// text names the node.toml setting.
    pub refusal: Option<String>,
    /// One line for the log and `doctor`.
    pub summary: String,
    /// One line on the shell's process containment (bubblewrap), for the
    /// log and `doctor`.
    pub contain_summary: String,
    /// How `doctor` and the log mark that line.
    pub contain_level: ContainLevel,
    /// Where per-session temp directories live.
    pub tmp_root: Option<PathBuf>,
}

/// Set up the node's sandbox once at start: resolve the wrapper binary
/// (`current_exe`, canonical; a binary replaced by an upgrade is picked up
/// at the same path, a deleted one makes every command fail closed), build
/// the rules and run the self-test; then find bubblewrap and probe the
/// shell's containment (T.8a). `Err` is a configuration error (a
/// `[sandbox]` entry that would expose the data directory or a protected
/// folder, or that holds a workspace root; `contain = "required"` with
/// `mode = "off"`).
pub fn start(
    settings: &Settings,
    data_dir: &Path,
    protected: &[PathBuf],
    workspace_roots: &[PathBuf],
    node_toml: &Path,
) -> Result<Started, String> {
    use crate::contain::ContainMode;
    if !data_dir.is_absolute() {
        return Err(format!(
            "the data directory {} is a relative path; the sandbox needs it absolute",
            data_dir.display()
        ));
    }
    if settings.mode == Mode::Off {
        if settings.contain == ContainMode::Required {
            return Err(format!(
                "{}: [sandbox] contain = \"required\" needs the sandbox; it is \
                 mode = \"off\" (the containment wraps the sandbox's wrapper)",
                node_toml.display()
            ));
        }
        return Ok(Started {
            confinement: Confinement::Unconfined,
            refusal: None,
            summary: format!(
                "OFF ([sandbox] mode = \"off\" in {}): the agent's shell runs with every right \
                 of this OS user",
                node_toml.display()
            ),
            contain_summary: "OFF ([sandbox] mode = \"off\"): processes a command leaves \
                              behind can outlive it"
                .into(),
            contain_level: ContainLevel::Warn,
            tmp_root: None,
        });
    }
    check_extras_against_roots(settings, workspace_roots)?;
    let exe = std::env::current_exe()
        .and_then(|e| e.canonicalize())
        .map_err(|e| format!("cannot find the harness binary for {SUBCOMMAND}: {e}"));
    match exe {
        Ok(exe) => {
            let mut s = start_with_exe(exe.clone(), settings, data_dir, protected, node_toml)?;
            check_binary_reach(&mut s, &exe, settings, workspace_roots, node_toml);
            Ok(s)
        }
        Err(e) => Ok(not_enforced(settings, e, node_toml, None)),
    }
}

/// The workspace root or `[sandbox] write` entry that holds `exe`, if any:
/// where a session can replace the harness binary (review T8E-5).
pub fn binary_reach(exe: &Path, settings: &Settings, roots: &[PathBuf]) -> Option<PathBuf> {
    let exe = real(exe);
    roots
        .iter()
        .chain(&settings.write)
        .find(|d| exe.starts_with(real(d)))
        .cloned()
}

/// Review T8E-5: a contained command's guard is the node's own image
/// (`/proc/self/exe`), but `sandbox-exec` inside the namespace is started
/// from the binary's path. Where a session can write that path, one command
/// can replace it and the next runs the agent's program in place of the
/// wrapper, without Landlock (still inside the PID namespace and under its
/// guard). Warned in the log and `doctor`; `contain = "required"` refuses.
/// Closed by T.8c's file view (the binary bound read-only from the node's
/// own image).
fn check_binary_reach(
    s: &mut Started,
    exe: &Path,
    settings: &Settings,
    roots: &[PathBuf],
    node_toml: &Path,
) {
    let contained = matches!(&s.confinement, Confinement::Confined { sandbox, .. }
        if sandbox.containment().is_some());
    let Some(dir) = binary_reach(exe, settings, roots).filter(|_| contained) else {
        return;
    };
    let why = format!(
        "a session can replace the harness binary {} (it is inside {}, a workspace root or \
         [sandbox] write entry): the next command's sandbox-exec would then be the agent's \
         program, without the file sandbox (its guard is the node's own running image and is \
         not affected). Install the binary where no session can write",
        exe.display(),
        dir.display()
    );
    if settings.contain == crate::contain::ContainMode::Required {
        let msg = format!(
            "this node refuses agentic sessions: [sandbox] contain = \"required\" in {}, and {why}",
            node_toml.display()
        );
        s.contain_level = ContainLevel::Refused;
        s.contain_summary = format!("REFUSING agentic sessions: {msg}");
        s.confinement = Confinement::Refused(msg.clone().into());
        s.refusal = Some(msg);
    } else {
        s.contain_summary.push_str(&format!("; WARNING: {why}"));
        s.contain_level = ContainLevel::Warn;
    }
}

/// [`start_with_exe`] with the interval after which a node in the
/// network-off gap probes the namespace again given (tests; the node uses
/// [`NET_RETRY_INTERVAL`]).
#[doc(hidden)]
pub fn start_with_exe_retrying(
    exe: PathBuf,
    settings: &Settings,
    data_dir: &Path,
    protected: &[PathBuf],
    node_toml: &Path,
    _retry: std::time::Duration,
) -> Result<Started, String> {
    start_with_exe(exe, settings, data_dir, protected, node_toml)
}

/// How long a node in the network-off gap waits between probes of the
/// namespace (review T8B-1).
pub const NET_RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

/// [`start`] with the wrapper binary given (`start` passes its own; a test
/// passes the harness binary, since its own executable is the test).
/// The caller has checked the `[sandbox]` entries against the roots.
pub fn start_with_exe(
    exe: PathBuf,
    settings: &Settings,
    data_dir: &Path,
    protected: &[PathBuf],
    node_toml: &Path,
) -> Result<Started, String> {
    let tmp_root = data_dir.join("sandbox-tmp");
    let sandbox = Sandbox::new(exe, settings, Some(tmp_root.clone()), protected)?;
    let scratch = tmp_root.join(format!("probe-{}", std::process::id()));
    let canary_dir = protected.first().map_or(data_dir, |p| p.as_path());
    let landlock = sandbox.probe(&scratch, canary_dir);
    if let (Err(e), Mode::Required) = (&landlock, settings.mode) {
        return Ok(not_enforced(
            settings,
            e.clone(),
            node_toml,
            Some(Arc::new(sandbox)),
        ));
    }
    let abi = landlock.as_deref().map_or(0, effective_abi);
    let (sandbox, contain) = containment(
        sandbox,
        settings,
        abi,
        &tmp_root.join(format!("probe-contain-{}", std::process::id())),
        node_toml,
    );
    let sandbox = Arc::new(sandbox);
    let mut started = match landlock {
        Ok(status) if settings.mode == Mode::Required => Started {
            confinement: Confinement::Confined {
                sandbox: sandbox.clone(),
                tmp: None,
            },
            refusal: None,
            summary: format!("enforced ({status})"),
            contain_summary: String::new(),
            contain_level: ContainLevel::Ok,
            tmp_root: Some(tmp_root),
        },
        Ok(status) => Started {
            confinement: Confinement::Confined {
                sandbox: sandbox.clone(),
                tmp: None,
            },
            refusal: None,
            summary: format!("best-effort ({status}): only what the kernel enforces"),
            contain_summary: String::new(),
            contain_level: ContainLevel::Ok,
            tmp_root: Some(tmp_root),
        },
        Err(e) => not_enforced(settings, e, node_toml, Some(sandbox.clone())),
    };
    match contain {
        Contained::Yes(s, level) => {
            started.contain_summary = s;
            started.contain_level = level;
            let net_ns = sandbox.containment().is_some_and(|c| c.network_namespace());
            if settings.network == Network::Off && net_ns {
                started.contain_summary.push_str(
                    "; network off: a network namespace of its own per command (only its own \
                     loopback: no TCP, UDP or DNS out)",
                );
            }
            // Review R-3: a read-write grant carries Landlock's socket right,
            // so a `write` entry that holds the usual socket folders hands
            // the command other programs' sockets on every kernel.
            let risky = socket_write_entries(settings);
            if !risky.is_empty() {
                let list: Vec<String> = risky.iter().map(|p| p.display().to_string()).collect();
                started.contain_summary.push_str(&format!(
                    "; WARNING: the [sandbox] write entry {} holds sockets of other programs \
                     (/tmp: tmux, X11; /run/user/<uid>: the session bus), through which a \
                     command can start a process outside the containment",
                    list.join(", ")
                ));
                started.contain_level = ContainLevel::Warn;
            }
        }
        Contained::No(s) => {
            started.contain_summary = s;
            started.contain_level = ContainLevel::Warn;
            if settings.network == Network::Off {
                started
                    .contain_summary
                    .push_str(&format!("; {NETWORK_OFF_UNCONTAINED}"));
            }
        }
        Contained::Refuse(msg) => {
            started.contain_level = ContainLevel::Refused;
            started.contain_summary = format!("REFUSING agentic sessions: {msg}");
            started.confinement = Confinement::Refused(msg.clone().into());
            started.refusal = Some(msg);
        }
    }
    Ok(started)
}

/// A working containment of `form` under `mode` on a kernel with
/// Landlock ABI `abi`: its line, or the refusal. `required` means the
/// guarantee holds; below ABI 9 it does not (T8A-1), so `required` refuses
/// there, and `auto` contains anyway and warns.
fn decide(
    form: crate::contain::Form,
    abi: u32,
    bwrap: &Path,
    mode: crate::contain::ContainMode,
    node_toml: &Path,
) -> Result<(String, ContainLevel), String> {
    let (line, level) = contained_line(form, abi, bwrap);
    if level != ContainLevel::Ok && mode == crate::contain::ContainMode::Required {
        return Err(format!(
            "this node refuses agentic sessions: [sandbox] contain = \"required\" in {} \
             promises that nothing a command starts outlives it, and here it does not hold: \
             {line}. Set contain = \"auto\" to run contained with that gap",
            node_toml.display()
        ));
    }
    Ok((line, level))
}

/// `[sandbox] write` entries that are, or hold, `/tmp` or this user's
/// `/run/user/<uid>`, where other programs keep their sockets (review R-3).
/// Only those two: a socket elsewhere in a write entry is not looked for.
fn socket_write_entries(settings: &Settings) -> Vec<PathBuf> {
    let uid = nix::unistd::getuid();
    let risky = [
        PathBuf::from("/tmp"),
        PathBuf::from(format!("/run/user/{uid}")),
    ];
    settings
        .write
        .iter()
        .filter(|w| {
            let w = real(w);
            risky
                .iter()
                .any(|r| r.starts_with(&w) || real(r).starts_with(&w))
        })
        .cloned()
        .collect()
}

/// What the containment line adds where the shell is contained but
/// bubblewrap could not make a network namespace (`why`) and the network
/// is off (T.8b, `contain = "auto"`): a gap, as below Landlock ABI 9.
fn network_gap_line(why: &str, abi: u32) -> String {
    let abstract_sockets = if abi >= ABI_ABSTRACT_SOCKETS {
        "abstract Unix sockets outside are closed by Landlock (ABI 6 or newer)"
    } else {
        "abstract Unix sockets outside (X11 and others) can be reached (Landlock below ABI 6)"
    };
    format!(
        "; network off WITH A GAP: bubblewrap cannot make a network namespace here ({why}), so \
         commands run contained without one: TCP is stopped by Landlock, but UDP and DNS still go \
         out, and {abstract_sockets}. contain = \"required\" refuses this"
    )
}

/// What the containment line adds where the shell is not contained and the
/// network is off (T.8b): Landlock alone covers only TCP.
const NETWORK_OFF_UNCONTAINED: &str = "WARNING: [sandbox] network = \"off\" without the \
     containment stops TCP only: UDP and DNS still go out (Landlock does not cover them; the \
     network namespace needs bubblewrap)";

enum Contained {
    Yes(String, ContainLevel),
    No(String),
    Refuse(String),
}

/// Find bubblewrap and probe its two forms, the full one first (T.8a).
fn containment(
    sandbox: Sandbox,
    settings: &Settings,
    abi: u32,
    scratch: &Path,
    node_toml: &Path,
) -> (Sandbox, Contained) {
    use crate::contain::{Contain, ContainMode, Form};
    if settings.contain == ContainMode::Off {
        return (
            sandbox,
            Contained::No(format!(
                "OFF ([sandbox] contain = \"off\" in {}): processes a command leaves behind \
                 can outlive it, and it sees every process of this machine in /proc",
                node_toml.display()
            )),
        );
    }
    // node.toml's `[sandbox] bwrap` is the owner's choice and taken as it
    // is; otherwise the first `bwrap` on PATH this user cannot replace.
    let found = match &settings.bwrap {
        Some(p) if p.is_absolute() && p.is_file() => p
            .canonicalize()
            .map_err(|e| format!("{}: {e}", p.display())),
        Some(p) => Err(format!("bubblewrap not found at {}", p.display())),
        None => crate::contain::find_bwrap(),
    };
    let why = match found {
        Err(e) => e,
        Ok(bwrap) => {
            let probe = |net_ns: bool| {
                let mk = |form| {
                    let c = Contain::new(bwrap.clone(), form);
                    if net_ns {
                        c
                    } else {
                        c.without_network_namespace()
                    }
                };
                let (full, shared) = (mk(Form::Full), mk(Form::SharedProc));
                match sandbox.probe_containment(&full, scratch) {
                    Ok(()) => Ok((full, None)),
                    Err(full_err) => match sandbox.probe_containment(&shared, scratch) {
                        Ok(()) => Ok((shared, Some(full_err))),
                        Err(e) => Err(format!("{full_err}; without a new /proc: {e}")),
                    },
                }
            };
            let mut found = probe(true);
            // T.8b: bubblewrap works but cannot make a network namespace.
            // `required` refuses; `auto` still contains (more than Landlock
            // alone), with the gap named. Decided here, once, at start.
            let mut net_err = None;
            if settings.network == Network::Off
                && let Err(e) = &found
                && let Ok(without) = probe(false)
            {
                net_err = Some(e.clone());
                found = Ok(without);
            }
            match (found, net_err) {
                (Ok(_), Some(e)) if settings.contain == ContainMode::Required => {
                    return (
                        sandbox,
                        Contained::Refuse(format!(
                            "this node refuses agentic sessions: [sandbox] network = \"off\" and \
                             contain = \"required\" in {}, and bubblewrap cannot make a network \
                             namespace here ({e}), so UDP and DNS would go out. Set contain = \
                             \"auto\" to run contained with that gap",
                            node_toml.display()
                        )),
                    );
                }
                (Ok((c, proc_err)), net_err) => {
                    return match decide(c.form(), abi, &bwrap, settings.contain, node_toml) {
                        Ok((mut line, mut level)) => {
                            // Why the fresh /proc failed (the shared form).
                            if let Some(e) = proc_err {
                                line.push_str(&format!("; a fresh /proc failed: {e}"));
                            }
                            if let Some(e) = net_err {
                                line.push_str(&network_gap_line(&e, abi));
                                level = ContainLevel::Warn;
                            }
                            (sandbox.with_containment(c), Contained::Yes(line, level))
                        }
                        Err(msg) => (sandbox, Contained::Refuse(msg)),
                    };
                }
                (Err(e), _) => e,
            }
        }
    };
    let advice = "install bubblewrap and allow unprivileged user namespaces (README, \
                  \"The shell sandbox\")";
    match settings.contain {
        ContainMode::Required => (
            sandbox,
            Contained::Refuse(format!(
                "this node refuses agentic sessions: [sandbox] contain = \"required\" in {} and \
                 the shell cannot be contained here ({why}); {advice}, or set contain = \"auto\"",
                node_toml.display()
            )),
        ),
        _ => (
            sandbox,
            Contained::No(format!(
                "NOT CONTAINED, running with Landlock alone ([sandbox] contain = \"auto\"): \
                 {why}. Processes a command leaves behind can outlive it; {advice}"
            )),
        ),
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
            contain_summary: String::new(),
            contain_level: ContainLevel::Ok,
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
                contain_summary: "not probed: the sandbox does not work".into(),
                contain_level: ContainLevel::Warn,
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
        sb.wrapper(&[], &[], false, None, OsStr::new("-"))
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

    /// `contain = "required"` where bubblewrap is missing: agentic sessions
    /// are refused, naming the setting, and no shell command is built.
    /// `auto` runs as before (Landlock alone) and says so. Simulated with a
    /// bubblewrap path that does not exist (tests cannot change `PATH`).
    #[test]
    fn contain_required_without_bubblewrap_refuses_and_auto_falls_back() {
        use crate::contain::ContainMode;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let data = root.join("data");
        let ws = root.join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        let missing = data.join("no-such/bwrap");
        let settings = |contain| Settings {
            // `best-effort`: the test binary is no sandbox wrapper, so the
            // Landlock probe fails; that path still probes the containment.
            mode: Mode::BestEffort,
            contain,
            bwrap: Some(missing.clone()),
            ..Default::default()
        };
        let toml = data.join("node.toml");
        let prot = std::slice::from_ref(&data);
        let s = start(&settings(ContainMode::Required), &data, prot, &[], &toml).unwrap();
        let why = s.refusal.expect("required without bubblewrap must refuse");
        assert!(why.contains("contain = \"required\""), "{why}");
        assert!(why.contains("bubblewrap not found"), "{why}");
        let shell = |c: &Confinement| {
            c.shell(
                OsStr::new("bash"),
                &[],
                &ws,
                None,
                &Default::default(),
                &[],
                Grants::default(),
            )
        };
        assert_eq!(shell(&s.confinement).unwrap_err(), why);

        let s = start(&settings(ContainMode::Auto), &data, prot, &[], &toml).unwrap();
        assert!(s.refusal.is_none(), "{:?}", s.refusal);
        assert_eq!(s.contain_level, ContainLevel::Warn);
        assert!(
            s.contain_summary.contains("NOT CONTAINED"),
            "{}",
            s.contain_summary
        );
        let (_, contained) = shell(&s.confinement).unwrap();
        assert!(!contained);

        // `off` with the sandbox off is fine; `required` with it is a
        // configuration error.
        let off = |contain| Settings {
            mode: Mode::Off,
            contain,
            ..Default::default()
        };
        assert!(start(&off(ContainMode::Auto), &data, prot, &[], &toml).is_ok());
        let e = start(&off(ContainMode::Required), &data, prot, &[], &toml).unwrap_err();
        assert!(e.contains("contain = \"required\""), "{e}");
    }

    /// T8A-1: the line depends on the effective Landlock ABI. From ABI 9
    /// the claim is made in full; below, it is a warning that names the
    /// session bus (and below ABI 6 abstract sockets), and `required`
    /// refuses. The shared-`/proc` form says so in every case.
    #[test]
    fn the_containment_line_depends_on_the_landlock_abi() {
        use crate::contain::{ContainMode, Form};
        let b = Path::new("/usr/bin/bwrap");
        let toml = Path::new("/n/node.toml");
        let (line, level) = contained_line(Form::Full, 9, b);
        assert_eq!(level, ContainLevel::Ok);
        assert!(
            line.contains("nothing a command starts outlives it"),
            "{line}"
        );
        assert!(!line.contains("GAP"), "{line}");
        for abi in [0, 4, 6, 8] {
            let (line, level) = contained_line(Form::Full, abi, b);
            assert_eq!(level, ContainLevel::Warn, "{abi}");
            assert!(
                line.contains("WITH A GAP") && line.contains("session bus"),
                "{line}"
            );
            assert!(
                !line.contains("nothing a command starts outlives it"),
                "{line}"
            );
            assert_eq!(line.contains("abstract socket"), abi < 6, "{abi}: {line}");
            let e = decide(Form::Full, abi, b, ContainMode::Required, toml).unwrap_err();
            assert!(
                e.contains("contain = \"required\"") && e.contains("auto"),
                "{e}"
            );
            assert!(decide(Form::Full, abi, b, ContainMode::Auto, toml).is_ok());
        }
        let (line, level) = contained_line(Form::SharedProc, 9, b);
        assert_eq!(level, ContainLevel::Ok);
        assert!(line.contains("without a /proc of its own"), "{line}");
        let (_, level) = contained_line(Form::SharedProc, 4, b);
        assert_eq!(level, ContainLevel::Warn);
        assert!(decide(Form::Full, 9, b, ContainMode::Required, toml).is_ok());
        assert_eq!(
            effective_abi("landlock Available { effective_abi: V9, kernel_abi: Some(10) }"),
            9
        );
        assert_eq!(effective_abi("landlock unavailable"), 0);
        // Review R-2: a lost line continuation left runs of spaces; every
        // line, in every form and range, is single-spaced.
        for form in [Form::Full, Form::SharedProc] {
            for abi in [0, 4, 6, 8, 9, 10] {
                let (line, _) = contained_line(form, abi, b);
                assert!(!line.contains("  "), "{form:?} {abi}: {line}");
                if let Err(e) = decide(form, abi, b, ContainMode::Required, toml) {
                    assert!(!e.contains("  "), "{form:?} {abi}: {e}");
                }
            }
        }
    }

    /// Review R-3: a `write` entry that is or holds `/tmp` or this user's
    /// runtime folder makes `doctor` warn; other entries do not.
    #[test]
    fn a_write_entry_with_other_programs_sockets_is_named() {
        let uid = nix::unistd::getuid();
        let run = PathBuf::from(format!("/run/user/{uid}"));
        let s = Settings {
            write: vec![
                "/tmp".into(),
                "/run/user".into(),
                run.join("gcr"),
                "/home/someone/.cache".into(),
            ],
            ..Default::default()
        };
        assert_eq!(
            socket_write_entries(&s),
            vec![PathBuf::from("/tmp"), PathBuf::from("/run/user")]
        );
        assert!(socket_write_entries(&Settings::default()).is_empty());
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
