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
//! Unix sockets (ABI 9). Below ABI 9 Landlock lets a pathname socket
//! (`/run/user/<uid>/bus`, `/var/run/docker.sock`, an ssh-agent) be
//! connected to. Where the shell is contained, its file view (T.8c,
//! [`crate::contain`]) holds only the grants, so a socket outside them is
//! not there; a socket inside a granted folder, and every socket with
//! Landlock alone, is an open residual, see PLAN.md R2.

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// The subcommand name of the wrapper.
pub const SUBCOMMAND: &str = "sandbox-exec";

/// `sandbox-exec`, first: close every descriptor above 2 this process was
/// given (T.8f, review T8F-2). A contained `sandbox-exec` is started from a
/// descriptor of the node's image that its guard left open (and, with
/// binds by path, nothing else closes descriptors on the way), and the
/// command must hold nothing of the image, the view or the guard. The list
/// is `/proc/self/fd` (taken whole, then closed); without `/proc`, every
/// number below the soft descriptor limit (capped at 65536), so one above
/// it would stay (re-check R-3). `close_range(2)` would end that, but nix
/// 0.31.3 has no safe wrapper of it, and no start of the node reaches this
/// case (both forms of the view have a `/proc`). Nothing opened before this
/// is needed after it.
pub fn close_inherited() {
    let fds: Vec<i32> = match std::fs::read_dir("/proc/self/fd") {
        Ok(d) => d
            .filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok())
            .filter(|&n| n > 2)
            .collect(),
        Err(_) => {
            let max = nix::sys::resource::getrlimit(nix::sys::resource::Resource::RLIMIT_NOFILE)
                .map_or(1024, |(soft, _)| soft.min(1 << 16));
            (3..max as i32).collect()
        }
    };
    for fd in fds {
        // The listing's own descriptor is among them, already closed.
        let _ = nix::unistd::close(fd);
    }
}

/// The self-test's canary in `dir` (the data directory): a name of this
/// process's own. With one fixed name, a `doctor` run beside a starting
/// node removed that node's canary in the middle of its self-test, and a
/// canary that is gone reads as "could not be read", which is what the test
/// takes for a working sandbox.
fn canary_file(dir: &Path) -> PathBuf {
    dir.join(format!("sandbox-canary-{}", std::process::id()))
}

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
    /// which closes UDP and DNS too. Where the shell is not contained (a
    /// `host` session) and for the node's own helpers, only TCP: Landlock
    /// does not cover UDP, and so DNS.
    Off,
}

/// The node's sandbox settings, paths absolute (from node.toml).
#[derive(Debug, Clone, Default)]
pub struct Settings {
    pub mode: Mode,
    pub network: Network,
    pub read: Vec<PathBuf>,
    pub write: Vec<PathBuf>,
    /// `[execution] sandbox`: whether the `sandbox` mode, the agent shell's
    /// process containment (T.8a), is offered (N.10a).
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
    contain: std::sync::OnceLock<crate::contain::Contain>,
    /// Set only where an `auto` start contains without the network
    /// namespace the network-off setting wants (review T8B-1): the way up to
    /// it, which is the only way the node ever moves.
    step_up: Option<StepUp>,
    /// The `sandbox` mode's own (N.10a): a shell command without a
    /// containment is refused, never run with Landlock alone. Set by
    /// [`start`] on the sandbox it offers; its uncontained twin (the `host`
    /// mode's and the helpers', [`Sandbox::uncontained`]) never contains.
    require_contain: bool,
    /// The harness binary as it was when first opened, held open (`O_PATH`)
    /// for the process's life: the guard, the helpers' and the uncontained
    /// shell's `sandbox-exec` are started from it (`/proc/self/fd/<n>`), so
    /// a file that later replaces the one at `exe` never runs in their place
    /// (reviews T8E-5, T8F re-check R-1 and R-4). For the node itself it is
    /// `/proc/self/exe`, its own running image ([`hold_own_image`]).
    image: Option<std::os::fd::RawFd>,
    /// Why there is no `image`, where there is none.
    image_err: Option<String>,
}

type Images = std::sync::Mutex<Vec<(PathBuf, std::os::fd::OwnedFd)>>;

/// The images this process holds, by the path they are known as: each
/// descriptor is held until the process exits, so its number never names
/// another file.
fn images() -> &'static Images {
    static IMAGES: std::sync::OnceLock<Images> = std::sync::OnceLock::new();
    IMAGES.get_or_init(Default::default)
}

/// The node, once at start: hold its own running image (`/proc/self/exe`,
/// whatever is at `exe` now) as the image of `exe`, so every [`Sandbox`]
/// of `exe` starts its guards, helpers and wrappers from it. `O_PATH`:
/// execute permission is enough, for this and for `execve` through
/// `/proc/self/fd/<n>` (a binary of mode 711 or 311: re-check R-1). This
/// replaces a second `current_exe()` comparison in [`open_image`], which a
/// binary replaced between the two calls could have answered with the new
/// file (the first review's "not proven").
pub fn hold_own_image(exe: &Path) -> Result<(), String> {
    let mut v = images().lock().map_err(|_| "a poisoned lock".to_string())?;
    if v.iter().any(|(p, _)| p == exe) {
        return Ok(());
    }
    let fd = nix::fcntl::open(
        "/proc/self/exe",
        nix::fcntl::OFlag::O_PATH | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(|e| format!("/proc/self/exe: {e}"))?;
    v.push((exe.to_owned(), fd));
    Ok(())
}

/// The harness binary to start guards, helpers and wrappers from: the one
/// held for `exe` ([`hold_own_image`], the node), else `exe` as it is when
/// first asked for (a test, whose own image is the test binary), opened
/// `O_PATH`, once per path.
fn open_image(exe: &Path) -> Result<std::os::fd::RawFd, String> {
    use std::os::fd::AsRawFd;
    let mut v = images().lock().map_err(|_| "a poisoned lock".to_string())?;
    if let Some((_, f)) = v.iter().find(|(p, _)| p == exe) {
        return Ok(f.as_raw_fd());
    }
    let fd = nix::fcntl::open(
        exe,
        nix::fcntl::OFlag::O_PATH | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(|e| format!("{}: {e}", exe.display()))?;
    let raw = fd.as_raw_fd();
    v.push((exe.to_owned(), fd));
    Ok(raw)
}

/// What to tell when `program` could not be started: where it is the
/// node's own image (a held descriptor, or the path it is known as), that
/// the node's own binary could not be executed, which, why, and what to do
/// (re-check R-2: before, the agent was told "failed to spawn bash").
pub fn image_spawn_error(program: &OsStr, e: &std::io::Error) -> Option<String> {
    use std::os::fd::AsRawFd;
    let v = images().lock().ok()?;
    let fd: Option<i32> = program
        .to_str()
        .and_then(|p| p.strip_prefix("/proc/self/fd/"))
        .and_then(|n| n.parse().ok());
    let (path, _) = v
        .iter()
        .find(|(p, f)| Some(f.as_raw_fd()) == fd || p.as_os_str() == program)?;
    let p = path.display();
    Some(match e.raw_os_error() {
        Some(nix::libc::EACCES | nix::libc::EPERM) => format!(
            "the node's own binary {p} could not be executed ({e}): restore its mode (chmod \
             755 {p}); if the file at that path is no longer the one the node runs (it was \
             rebuilt or replaced), restart the node instead. Nothing was started"
        ),
        _ => format!("the node's own binary {p} could not be started ({e}). Nothing was started"),
    })
}

/// [`image_spawn_error`], or `<program>: <error>`.
pub fn explain_spawn(program: &OsStr, e: &std::io::Error) -> String {
    image_spawn_error(program, e).unwrap_or_else(|| format!("{}: {e}", program.to_string_lossy()))
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
#[derive(Debug, Clone)]
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
        let image = open_image(&exe);
        Ok(Self {
            image: image.as_ref().ok().copied(),
            image_err: image.err(),
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
            contain: std::sync::OnceLock::new(),
            step_up: None,
            require_contain: false,
        })
    }

    /// The same rules (the same resolved `[sandbox]` entries, the same held
    /// image), without a containment and unable to gain one: the `host`
    /// mode's shell and the node's helpers (N.10a). Made before the
    /// containment is probed, so it never shares the probe's outcome.
    fn uncontained(&self) -> Self {
        Self {
            exe: self.exe.clone(),
            read: self.read.clone(),
            extra_read: self.extra_read.clone(),
            extra_write: self.extra_write.clone(),
            network: self.network,
            best_effort: self.best_effort,
            tmp_root: self.tmp_root.clone(),
            protected: self.protected.clone(),
            contain: std::sync::OnceLock::new(),
            step_up: None,
            require_contain: false,
            image: self.image,
            image_err: self.image_err.clone(),
        }
    }

    /// This sandbox with the agent's shell contained by `c` (bubblewrap).
    pub fn with_containment(self, c: crate::contain::Contain) -> Self {
        let _ = self.contain.set(c);
        self
    }

    /// The agent shell's containment, if any (now: an `auto` node that
    /// started without one can gain one later, [`StepUp`]).
    pub fn containment(&self) -> Option<&crate::contain::Contain> {
        self.contain.get()
    }

    /// Whether a contained command with the network off gets its network
    /// namespace now: the containment's own setting, or the gap left later
    /// (review T8B-1).
    pub fn network_namespace(&self, c: &crate::contain::Contain) -> bool {
        c.network_namespace() || self.step_up.as_ref().is_some_and(StepUp::net_taken)
    }

    /// Whether this node is as far up as it can go: contained, and with the
    /// network off in a network namespace of its own.
    fn fully_up(&self) -> bool {
        self.contain
            .get()
            .is_some_and(|c| self.network == Network::On || self.network_namespace(c))
    }

    /// When a shell command is started on a node contained in the
    /// network-off gap and the last probe is at least the interval old: run the start's probes again on a thread of their own, bounded
    /// ([`LATER_PROBE_BOUND`]), one at a time. The command that noticed does
    /// not wait (re-check R-2): it starts in the mode the node is in, and
    /// the commands started after a probe passed get the step up. Only the
    /// node's own start sets this up, and nothing ever moves the node down.
    fn step_up_if_due(self: &Arc<Self>) {
        use std::sync::atomic::Ordering::SeqCst;
        let Some(st) = &self.step_up else {
            return;
        };
        if self.fully_up() || !st.due() {
            return;
        }
        if st
            .running
            .compare_exchange(false, true, SeqCst, SeqCst)
            .is_err()
        {
            return;
        }
        let me = self.clone();
        let spawned = std::thread::Builder::new()
            .name("sandbox-step-up".into())
            .spawn(move || {
                me.step_up_now();
                if let Some(st) = &me.step_up {
                    st.running.store(false, SeqCst);
                }
            });
        if spawned.is_err() {
            st.running.store(false, SeqCst);
        }
    }

    /// One later probe ([`Self::step_up_if_due`]): a node contained in the
    /// network-off gap tries the namespace again. A node that could not
    /// contain at start does not try again: it does not offer the `sandbox`
    /// mode until it is restarted (N.10a), so nothing would use it.
    fn step_up_now(&self) {
        use crate::contain::Contain;
        let Some(st) = &self.step_up else {
            return;
        };
        let scratch = |what: &str| st.scratch.join(format!("probe-{what}-{}", st.now_ms()));
        let Some(c) = self.contain.get() else {
            return;
        };
        let with = Contain::new(c.bwrap().to_path_buf(), c.form());
        if self
            .probe_containment_within(&with, &scratch("net"), Some(LATER_PROBE_BOUND))
            .is_ok()
        {
            st.net.store(true, std::sync::atomic::Ordering::SeqCst);
            network_log(
                "shell containment: bubblewrap makes a network namespace now; with \
                 [sandbox] network = \"off\" commands started from now on run in one of \
                 their own (no UDP or DNS out; a command already running stays as it \
                 started)",
            );
        }
    }

    pub fn network(&self) -> Network {
        self.network
    }

    /// What a contained command's file view holds (T.8c), in words a person
    /// can check against node.toml: empty where the shell is not contained.
    pub fn view_line(&self) -> String {
        use crate::contain::{Binds, Source};
        let Some(c) = self.contain.get() else {
            return String::new();
        };
        let extra_rw: Vec<Grant> = self.extra_write.iter().filter_map(Extra::grant).collect();
        let extra_ro: Vec<Grant> = self
            .read
            .iter()
            .map(|p| Grant::system(p))
            .chain(self.extra_read.iter().filter_map(Extra::grant))
            .collect();
        let mounts = view(&extra_rw, &extra_ro);
        let list = |pick: &dyn Fn(&crate::contain::Mount) -> Option<String>| {
            let v: Vec<String> = mounts.iter().filter_map(pick).collect();
            if v.is_empty() {
                "none".to_string()
            } else {
                v.join(", ")
            }
        };
        let system =
            list(&|m| (m.source == Source::Host && !m.write).then(|| m.path.display().to_string()));
        let links = list(&|m| match &m.source {
            Source::Symlink(t) => Some(format!("{} -> {}", m.path.display(), t.display())),
            _ => None,
        });
        let read = list(&|m| {
            (matches!(m.source, Source::Checked(_)) && !m.write)
                .then(|| m.path.display().to_string())
        });
        let write = list(&|m| m.write.then(|| m.path.display().to_string()));
        let version = crate::contain::binds_of(c.bwrap())
            .1
            .unwrap_or_else(|| c.bwrap().display().to_string());
        let how = match c.binds() {
            Binds::Fd => format!(
                "{version}: each grant bound from a descriptor the guard opened and checked \
                 against what the node checked"
            ),
            Binds::Path => format!(
                "{version}, older than 0.10.0: each grant bound by its path and checked by \
                 identity inside by sandbox-exec, which refuses the command on a difference"
            ),
        };
        let proc = match c.form() {
            crate::contain::Form::Full => "/proc of its own",
            crate::contain::Form::SharedProc => "this machine's /proc",
        };
        format!(
            "a contained command sees only: read-only {system}; links {links}; [sandbox] read \
             {read}; read-write [sandbox] write {write}; its session's folder (read-write) and \
             temp folder; /dev of its own; {proc}; an empty /tmp. Nothing else of this machine \
             is there ({how}). Its sandbox-exec is the node's own running image, started \
             through a descriptor (no path of the harness binary is in the view): a rebuild or \
             upgrade under the running node changes nothing for contained commands until the \
             node is restarted"
        )
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
        probe: Option<&Probe>,
    ) -> std::process::Command {
        let mark = self.tmp_root.as_ref().map(|root| crate::contain::Mark {
            root: root.clone(),
            session: session.to_owned(),
        });
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
        let all_rw: Vec<Grant> = rw.iter().chain(&extra_rw).cloned().collect();
        let all_ro: Vec<Grant> = extra_ro.iter().chain(ro).cloned().collect();
        let mut c = match contain {
            // The file view (T.8c) holds exactly what Landlock grants below.
            Some((b, cwd)) => b.command(
                &self.exe,
                self.image,
                cwd,
                mark.as_ref(),
                self.network == Network::Off && self.network_namespace(b),
                &view(&all_rw, &all_ro),
            ),
            None => self.own_program(),
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
        if let Some(p) = probe {
            p.push_args(&mut c);
        }
        for g in all_rw.iter().chain(&devices()) {
            g.push_args(&mut c, "--rw");
        }
        for g in &all_ro {
            g.push_args(&mut c, "--ro");
        }
        c.arg("--");
        c
    }

    /// The harness binary to run `sandbox-exec` from outside a containment
    /// (the helpers, an uncontained shell): the held image
    /// (`/proc/self/fd/<n>`, resolved by `execve` in the forked child before
    /// the close-on-exec descriptor closes), as the guard is started
    /// (re-check R-4); `argv[0]` is the path, for `ps`. Without a held
    /// image (said at start), the path.
    fn own_program(&self) -> std::process::Command {
        use std::os::unix::process::CommandExt;
        match self.image {
            Some(fd) => {
                let mut c = std::process::Command::new(format!("/proc/self/fd/{fd}"));
                c.arg0(&self.exe);
                c
            }
            None => std::process::Command::new(&self.exe),
        }
    }

    /// The wrapper for one of the node's own helpers (its git for
    /// checkpoints, R.1 R1b-2), up to and including `--`: exactly `rw` and
    /// `ro` on top of the system folders ([`SYSTEM_READ`]) and the device
    /// files, and none of node.toml's `[sandbox] read`/`write` (those are
    /// for the agent's toolchains). Nothing here checks the grants against
    /// the protected folders: the caller passes only its own folders, each
    /// with the identity it expects (R1c-1).
    pub fn helper(&self, rw: &[Grant], ro: &[Grant]) -> std::process::Command {
        let mut c = self.own_program();
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
        let canary = canary_file(canary_dir);
        std::fs::write(&canary, "the sandbox must not read this\n")
            .map_err(|e| format!("{}: {e}", canary.display()))?;
        // The checks are the wrapper's own (review R-3): no program found
        // by name.
        let probe = Probe {
            read: vec![ok.clone()],
            unreadable: vec![canary.clone()],
            ..Default::default()
        };
        let mut c = self.wrapper(
            &[Grant::made(scratch, "the self-test folder")?],
            &[],
            true,
            None,
            OsStr::new("probe"),
            Some(&probe),
        );
        c.env_clear()
            .current_dir(scratch)
            .stdin(std::process::Stdio::null());
        let out = c.output();
        let _ = std::fs::remove_file(&canary);
        let _ = std::fs::remove_dir_all(scratch);
        let out = out.map_err(|e| {
            format!(
                "cannot start {SUBCOMMAND}: {}",
                explain_spawn(c.get_program(), &e)
            )
        })?;
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
        self.probe_containment_within(contain, scratch, None)
    }

    /// The same, killed and counted as failed after `bound` (the later
    /// probes, re-check R-2: on the deadline the node's end of the guard's
    /// channel is closed, which is the guard's kill order).
    fn probe_containment_within(
        &self,
        contain: &crate::contain::Contain,
        scratch: &Path,
        bound: Option<std::time::Duration>,
    ) -> Result<(), String> {
        std::fs::create_dir_all(scratch).map_err(|e| format!("{}: {e}", scratch.display()))?;
        let ok = scratch.join("granted");
        std::fs::write(&ok, "ok").map_err(|e| format!("{}: {e}", ok.display()))?;
        // Beside the granted folder, in the node's temp root, granted to
        // nothing: the view must not hold it (T.8c).
        let mut outside = scratch.as_os_str().to_owned();
        outside.push(".outside");
        let outside = PathBuf::from(outside);
        std::fs::write(&outside, "not in the view")
            .map_err(|e| format!("{}: {e}", outside.display()))?;
        let full = contain.form() == crate::contain::Form::Full;
        let net = self.network == Network::Off && self.network_namespace(contain);
        // The wrapper's own checks, after it confined itself in the view it
        // will use (no program found by name: review R-3).
        let probe = Probe {
            read: vec![ok.clone()],
            pid_ns: true,
            own_proc: full,
            // The node's own namespace, which the command's must not be.
            net_not: if net {
                Some(
                    std::fs::read_link("/proc/self/ns/net")
                        .map_err(|e| format!("/proc/self/ns/net: {e}"))?
                        .into_os_string(),
                )
            } else {
                None
            },
            absent: vec![outside.clone()],
            ..Default::default()
        };
        let mut c = self.wrapper(
            &[Grant::made(scratch, "the self-test folder")?],
            &[],
            false,
            Some((contain, scratch)),
            OsStr::new("probe"),
            Some(&probe),
        );
        // Through the guard, as every command: its channel stays open (no
        // kill order) until the probe has ended.
        let (keep, theirs) = std::os::unix::net::UnixStream::pair()
            .map_err(|e| format!("the guard's channel: {e}"))?;
        c.env_clear()
            .current_dir(scratch)
            .stdin(std::process::Stdio::from(std::os::fd::OwnedFd::from(
                theirs,
            )));
        c.stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        let out = run_bounded(c, keep, bound);
        let _ = std::fs::remove_dir_all(scratch);
        let _ = std::fs::remove_file(&outside);
        let (status, stderr) =
            out.map_err(|e| format!("cannot start {}: {e}", contain.bwrap().display()))?;
        let stderr = String::from_utf8_lossy(&stderr);
        match status.code() {
            Some(0) => Ok(()),
            Some(2) => Err(format!(
                "the command could not read a granted file in the view: {}",
                stderr.trim()
            )),
            Some(7) => Err(
                "a file outside the grants was there for the command: the file view did not \
                 hold"
                    .into(),
            ),
            Some(4) => Err("the command was not in a PID namespace of its own".into()),
            Some(5) => Err("/proc is not the namespace's own".into()),
            Some(6) => Err(
                "the probe's own check failed, not bubblewrap: the command's \
                 /proc/self/ns/net was the node's, or could not be read"
                    .into(),
            ),
            code => Err(format!(
                "{} failed (exit {code:?}): {}",
                contain.bwrap().display(),
                stderr.trim()
            )),
        }
    }
}

/// What a `sandbox` session's shell command is told where its sandbox has
/// no containment (never the case on a node that offers the mode; the
/// check stands in case it ever is).
pub const NO_CONTAINMENT: &str = "this chat runs in the sandbox mode and the node has no \
     working bubblewrap containment for it: nothing was started";

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
    /// Read-only folders a session might be able to write or re-point (the
    /// node owner's skill roots): bound by their identity now, as a
    /// node.toml `[sandbox] read` entry is, so a swap after this look
    /// refuses the command (checked against the protected ones too).
    pub read_identified: Vec<(PathBuf, &'static str)>,
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

    /// The node's temp root (inside its data directory, which no file view
    /// holds): only `<tmp_root>/<session>` folders are ever granted, so a
    /// folder made beside them is out of every contained command's reach
    /// (an MCP server's request FIFO, [`crate::mcp`]).
    pub(crate) fn private_root(&self) -> Option<&Path> {
        match self {
            Self::Confined { sandbox, .. } => sandbox.tmp_root.as_deref(),
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
                checked.extend(grants.read_identified.iter().map(|(p, _)| p.clone()));
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
                for (p, label) in &grants.read_identified {
                    ro.push(Grant::identified(
                        p,
                        harness_core::folder::Identity::of(p)?,
                        label,
                    ));
                }
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
                if shell {
                    // Beside the command, never in its way (re-check R-1, R-2).
                    sandbox.step_up_if_due();
                }
                let contain = sandbox.contain.get().filter(|_| shell);
                // The `sandbox` mode never falls back to Landlock alone
                // (N.10a): its node offers it only with a containment.
                if shell && contain.is_none() && sandbox.require_contain {
                    return Err(NO_CONTAINMENT.into());
                }
                contained = contain.is_some();
                let session = tmp
                    .as_ref()
                    .and_then(|t| t.file_name())
                    .unwrap_or(OsStr::new("-"));
                let mut c = sandbox.wrapper(
                    &rw,
                    &ro,
                    false,
                    contain.map(|b| (b, ws.as_path())),
                    session,
                    None,
                );
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
    /// Contained with a named gap.
    Warn,
    /// The `sandbox` mode is not offered (N.10a): the containment does not
    /// work, `[execution] sandbox` is `"off"`, or `"strict"` and a gap.
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
        "; and with the network on, any service of this user on an abstract socket (X11 and \
         others: Landlock below ABI 6; the network off closes it)"
    } else {
        ""
    };
    (
        format!(
            "{what}) WITH A GAP: on this kernel (Landlock ABI {abi}, below 9) Landlock does not \
             cover Unix sockets, so a command can still start a process outside the \
             containment, which then outlives it and runs without the sandbox, by asking a \
             service of this user over a socket it can reach: the file view (T.8c) keeps every \
             socket outside the grants out of reach (the session bus at /run/user/<uid>/bus, a \
             container runtime's socket), but not one inside a granted folder (the session's \
             own, a [sandbox] read or write entry){abstract_too}. Closed by Linux 7.1 or newer \
             (Landlock ABI 9); README, \"The shell sandbox\""
        ),
        ContainLevel::Warn,
    )
}

/// The outcome of [`start`].
#[derive(Debug)]
pub struct Started {
    /// The `sandbox` mode's (N.10a): `Confined`, its shell contained, where
    /// this node offers that mode; else `Refused`, saying why not (the
    /// probe failed, `[execution] sandbox = "off"`, or `"strict"` and a gap).
    pub confinement: Confinement,
    /// The same rules without the containment, for the `host` mode's shell
    /// and the node's own helpers (its checkpoint git, graphify):
    /// `Confined` (Landlock), `Unconfined` with `[sandbox] mode = "off"`,
    /// `Refused` where the Landlock sandbox is required and does not work.
    pub uncontained: Confinement,
    /// Set when no agentic session may run here in any mode (`[sandbox]
    /// mode = "required"` and Landlock not enforced); the text names the
    /// node.toml setting. Whether `sandbox` is offered is [`Self::confinement`].
    pub refusal: Option<String>,
    /// One line for the log and `doctor`.
    pub summary: String,
    /// One line on the shell's process containment (bubblewrap), for the
    /// log and `doctor`.
    pub contain_summary: String,
    /// How `doctor` and the log mark that line.
    pub contain_level: ContainLevel,
    /// One line on what a contained command's file view holds (T.8c), for
    /// the log and `doctor`; empty where the shell is not contained.
    pub view_summary: String,
    /// Where per-session temp directories live.
    pub tmp_root: Option<PathBuf>,
    /// The Landlock ABI the start's probe found enforced; 0 where it is not
    /// (`[sandbox] mode = "off"`, or best-effort on a kernel without it).
    pub landlock_abi: u32,
}

/// Set up the node's sandbox once at start: resolve the wrapper binary
/// (`current_exe`, canonical; the node's helpers and an uncontained shell
/// pick up a binary replaced by an upgrade at the same path; a contained
/// command's guard and `sandbox-exec` are the node's own running image,
/// started through descriptors of it, until the node restarts: T.8f), build
/// the rules and run the self-test; then find bubblewrap and probe the
/// shell's containment (T.8a). `Err` is a configuration error (a
/// `[sandbox]` entry that would expose the data directory or a protected
/// folder, or that holds a workspace root; `[execution] sandbox =
/// "strict"` with `mode = "off"`).
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
        if settings.contain == ContainMode::Strict {
            return Err(format!(
                "{}: [execution] sandbox = \"strict\" needs the shell sandbox; it is \
                 [sandbox] mode = \"off\" (the containment wraps the sandbox's wrapper)",
                node_toml.display()
            ));
        }
        let why = format!(
            "the sandbox mode is not offered: [sandbox] mode = \"off\" in {} (the containment \
             wraps the sandbox's wrapper)",
            node_toml.display()
        );
        return Ok(Started {
            confinement: Confinement::Refused(why.clone().into()),
            uncontained: Confinement::Unconfined,
            refusal: None,
            summary: format!(
                "OFF ([sandbox] mode = \"off\" in {}): a host session's shell runs with every \
                 right of this OS user",
                node_toml.display()
            ),
            contain_summary: format!("NOT OFFERED: {why}"),
            contain_level: ContainLevel::Refused,
            view_summary: String::new(),
            tmp_root: None,
            landlock_abi: 0,
        });
    }
    check_extras_against_roots(settings, workspace_roots)?;
    let exe = std::env::current_exe()
        .and_then(|e| e.canonicalize())
        .map_err(|e| format!("cannot find the harness binary for {SUBCOMMAND}: {e}"));
    match exe {
        Ok(exe) => {
            // Its own running image, once, before anything is started from
            // it (re-check R-1).
            let held = hold_own_image(&exe);
            let mut s = start_with_exe(exe.clone(), settings, data_dir, protected, node_toml)?;
            check_image(&mut s, held.err(), settings, node_toml);
            check_binary_reach(&mut s, &exe, settings, workspace_roots, node_toml);
            Ok(s)
        }
        Err(e) => Ok(not_enforced(settings, e, node_toml, None, None)),
    }
}

/// The `sandbox` mode is not offered (N.10a), for `msg`: the rest (the
/// `host` mode, the helpers) is as it was.
fn refuse_sandbox(s: &mut Started, msg: String) {
    s.contain_level = ContainLevel::Refused;
    s.contain_summary = format!("NOT OFFERED: {msg}");
    s.confinement = Confinement::Refused(msg.into());
}

impl Started {
    /// Why this node does not offer the `sandbox` mode, or `None` when it
    /// does.
    pub fn sandbox_refusal(&self) -> Option<&str> {
        match &self.confinement {
            Confinement::Refused(why) => Some(why),
            _ => None,
        }
    }

    /// Whether the `sandbox` mode, offered with `[sandbox] network = "off"`,
    /// runs its commands without a network namespace of their own (the
    /// gap of T.8b; review T8B-3: the chat says so). As at start: a later
    /// probe may close it ([`StepUp`]).
    /// The form the sandbox mode contains in, where it is offered.
    pub fn sandbox_form(&self) -> Option<crate::contain::Form> {
        match &self.confinement {
            Confinement::Confined { sandbox, .. } => sandbox.containment().map(|c| c.form()),
            _ => None,
        }
    }

    /// Whether `[sandbox] network = "off"` stops a `host` chat's TCP: its
    /// confinement is Landlock and Landlock enforces the TCP rule (ABI 4).
    pub fn host_tcp_stopped(&self) -> bool {
        matches!(self.uncontained, Confinement::Confined { .. }) && self.landlock_abi >= 4
    }

    pub fn sandbox_netns_gap(&self) -> bool {
        match &self.confinement {
            Confinement::Confined { sandbox, .. } => {
                sandbox.network == Network::Off
                    && sandbox
                        .containment()
                        .is_some_and(|c| !sandbox.network_namespace(c))
            }
            _ => false,
        }
    }
}

/// Re-check R-1: a node that holds no descriptor of its own image (or
/// could not hold `/proc/self/exe`, `held`) starts its guards, helpers and
/// wrappers from the binary's path, so a file put there runs in their
/// place. Said in the log and `doctor` (the containment line); `[execution]
/// sandbox = "strict"` does not offer the sandbox mode.
fn check_image(s: &mut Started, held: Option<String>, settings: &Settings, node_toml: &Path) {
    // The twin holds the same image: what it says holds for both modes.
    let Confinement::Confined { sandbox, .. } = &s.uncontained else {
        return;
    };
    let why = match (&held, &sandbox.image_err) {
        (_, Some(e)) => format!(
            "the node holds no descriptor of its own binary ({e}): its guards, sandbox-exec and \
             helpers are started from the path {}, so a file put at that path runs in their \
             place",
            sandbox.exe.display()
        ),
        (Some(e), None) => format!(
            "the node could not hold its own running image ({e}): it holds the file that was at \
             {} when it started, which may not be the build it runs",
            sandbox.exe.display()
        ),
        (None, None) => return,
    };
    if settings.contain == crate::contain::ContainMode::Strict {
        let msg = format!(
            "the sandbox mode is not offered: [execution] sandbox = \"strict\" in {}, and {why}",
            node_toml.display()
        );
        refuse_sandbox(s, msg);
    } else {
        s.contain_summary.push_str(&format!("; WARNING: {why}"));
        s.contain_level = ContainLevel::Warn;
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

/// Review T8E-5: where a session can write the binary's path, it can put a
/// file there. Nothing the node starts runs it: the guard, `sandbox-exec`
/// and the helpers are the node's own image, through descriptors (T.8e,
/// T.8f, re-check R-4). What is left is the mode: a session that keeps a
/// name of the image can change it and stop them all, and the node's
/// restart (review T8F-5). Warned in the log and `doctor`; `[execution]
/// sandbox = "strict"` does not offer the sandbox mode.
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
    // Nothing the node starts runs a file at this path (its guards,
    // sandbox-exec and helpers are the node's running image, through
    // descriptors), but the image's mode is the file's.
    let why = format!(
        "a session can replace the harness binary {} (it is inside {}, a workspace root or \
         [sandbox] write entry): no file put at that path runs in the node's place (its guards, \
         sandbox-exec and helpers are started from the node's own running image, through \
         descriptors), but a command can keep a name of the file the node runs there, and a \
         session that can write there can then stop every contained command and \
         every helper of every session, and the node's restart, by changing that file's mode \
         (chmod 000). Install the binary where no session can write",
        exe.display(),
        dir.display()
    );
    if settings.contain == crate::contain::ContainMode::Strict {
        let msg = format!(
            "the sandbox mode is not offered: [execution] sandbox = \"strict\" in {}, and {why}",
            node_toml.display()
        );
        refuse_sandbox(s, msg);
    } else {
        s.contain_summary.push_str(&format!("; WARNING: {why}"));
        s.contain_level = ContainLevel::Warn;
    }
}

/// [`start_with_exe`] with the interval after which a node in the
/// network-off gap probes the namespace again given (tests; the node uses
/// [`STEP_UP_INTERVAL`]).
#[doc(hidden)]
pub fn start_with_exe_retrying(
    exe: PathBuf,
    settings: &Settings,
    data_dir: &Path,
    protected: &[PathBuf],
    node_toml: &Path,
    retry: std::time::Duration,
) -> Result<Started, String> {
    start_inner(exe, settings, data_dir, protected, node_toml, retry)
}

/// The way up of an `auto` node contained in the network-off gap (review
/// T8B-1): to the network namespace. When the last probe was, whether one
/// is running, and whether the namespace was taken later. One per node, in
/// the `sandbox` mode's `Sandbox`; only its start makes one.
#[derive(Debug)]
struct StepUp {
    retry: std::time::Duration,
    since: std::time::Instant,
    /// Milliseconds after `since` of the last probe.
    last_ms: std::sync::atomic::AtomicU64,
    /// A probe is running (one at a time).
    running: std::sync::atomic::AtomicBool,
    /// The network namespace was taken after the start. Set once, never
    /// cleared.
    net: std::sync::atomic::AtomicBool,
    /// Where the probes make their folders (the node's temp root).
    scratch: PathBuf,
}

impl StepUp {
    fn new(retry: std::time::Duration, scratch: PathBuf) -> Self {
        Self {
            retry,
            since: std::time::Instant::now(),
            last_ms: 0.into(),
            running: false.into(),
            net: false.into(),
            scratch,
        }
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.since.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn net_taken(&self) -> bool {
        self.net.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Whether a probe is due; claims it, so only one caller probes.
    fn due(&self) -> bool {
        use std::sync::atomic::Ordering::SeqCst;
        let now = self.now_ms();
        let last = self.last_ms.load(SeqCst);
        let retry = u64::try_from(self.retry.as_millis()).unwrap_or(u64::MAX);
        now.saturating_sub(last) >= retry
            && self
                .last_ms
                .compare_exchange(last, now, SeqCst, SeqCst)
                .is_ok()
    }
}

/// How often the start probes bubblewrap before it decides the sandbox
/// mode is not offered, and the pause between two tries (review L-b): at
/// most two pauses, so a start that fails for good costs about 2 s more.
pub const START_PROBE_TRIES: u32 = 3;
pub const START_PROBE_PAUSE: std::time::Duration = std::time::Duration::from_secs(1);

/// How long a later probe ([`StepUp`]) may take before it is killed and
/// counted as failed (re-check R-2). The start's own probes have no bound:
/// a cold start of the binary on a slow disk can take longer.
pub const LATER_PROBE_BOUND: std::time::Duration = std::time::Duration::from_secs(2);

/// Run a probe (`c`, a guard's command line, whose stdin is the other end
/// of `keep`): its exit status and stderr. With `bound`, past it the
/// node's end `keep` is closed (the guard's kill order) and the probe
/// counts as failed; a guard that then does not exit within a second is
/// left to a thread that reaps it, and the probe's output is not read.
fn run_bounded(
    mut c: std::process::Command,
    keep: std::os::unix::net::UnixStream,
    bound: Option<std::time::Duration>,
) -> std::io::Result<(std::process::ExitStatus, Vec<u8>)> {
    use std::io::Read;
    let Some(bound) = bound else {
        let out = c.output();
        drop(keep);
        return out.map(|o| (o.status, o.stderr));
    };
    let mut child = c.spawn()?;
    drop(c);
    let wait = |child: &mut std::process::Child, until: std::time::Instant| loop {
        match child.try_wait() {
            Ok(Some(s)) => return Some(s),
            Ok(None) if std::time::Instant::now() < until => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            _ => return None,
        }
    };
    let started = std::time::Instant::now();
    if let Some(status) = wait(&mut child, started + bound) {
        drop(keep);
        let mut err = Vec::new();
        if let Some(mut e) = child.stderr.take() {
            let _ = e.read_to_end(&mut err);
        }
        return Ok((status, err));
    }
    drop(keep);
    let ended = wait(
        &mut child,
        std::time::Instant::now() + std::time::Duration::from_secs(1),
    );
    if ended.is_none() {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        format!(
            "the probe did not answer within {:.1} s and was killed",
            bound.as_secs_f64()
        ),
    ))
}

/// Where the node's one line on leaving the gap goes: the node sets its
/// logger ([`set_network_log`]); otherwise stderr.
static NETWORK_LOG: std::sync::OnceLock<fn(&str)> = std::sync::OnceLock::new();

/// Set where [`Sandbox`] logs its change from the network-off gap to the
/// namespace (the node: its log). Once per process.
pub fn set_network_log(f: fn(&str)) {
    let _ = NETWORK_LOG.set(f);
}

fn network_log(line: &str) {
    match NETWORK_LOG.get() {
        Some(f) => f(line),
        None => eprintln!("{line}"),
    }
}

/// How long an `auto` node below where it could be (Landlock alone after a
/// failed start, or the network-off gap) waits between its later probes
/// (review T8B-1, re-check R-1).
pub const STEP_UP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

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
    start_inner(
        exe,
        settings,
        data_dir,
        protected,
        node_toml,
        STEP_UP_INTERVAL,
    )
}

fn start_inner(
    exe: PathBuf,
    settings: &Settings,
    data_dir: &Path,
    protected: &[PathBuf],
    node_toml: &Path,
    retry: std::time::Duration,
) -> Result<Started, String> {
    let tmp_root = data_dir.join("sandbox-tmp");
    let mut sandbox = Sandbox::new(exe, settings, Some(tmp_root.clone()), protected)?;
    // The `host` mode's and the helpers' (N.10a): never contained.
    let plain = Arc::new(sandbox.uncontained());
    // The `sandbox` mode's: never Landlock alone.
    sandbox.require_contain = true;
    let scratch = tmp_root.join(format!("probe-{}", std::process::id()));
    let canary_dir = protected.first().map_or(data_dir, |p| p.as_path());
    let landlock = sandbox.probe(&scratch, canary_dir);
    if let (Err(e), Mode::Required) = (&landlock, settings.mode) {
        return Ok(not_enforced(
            settings,
            e.clone(),
            node_toml,
            Some(Arc::new(sandbox)),
            Some(plain),
        ));
    }
    let abi = landlock.as_deref().map_or(0, effective_abi);
    let (sandbox, contain) = containment(
        sandbox,
        settings,
        abi,
        &tmp_root.join(format!("probe-contain-{}", std::process::id())),
        node_toml,
        retry,
    );
    let sandbox = Arc::new(sandbox);
    let mut started = match landlock {
        Ok(status) if settings.mode == Mode::Required => Started {
            confinement: Confinement::Confined {
                sandbox: sandbox.clone(),
                tmp: None,
            },
            uncontained: Confinement::Confined {
                sandbox: plain.clone(),
                tmp: None,
            },
            refusal: None,
            summary: format!("enforced ({status})"),
            contain_summary: String::new(),
            contain_level: ContainLevel::Ok,
            view_summary: String::new(),
            tmp_root: Some(tmp_root),
            landlock_abi: 0,
        },
        Ok(status) => Started {
            confinement: Confinement::Confined {
                sandbox: sandbox.clone(),
                tmp: None,
            },
            uncontained: Confinement::Confined {
                sandbox: plain.clone(),
                tmp: None,
            },
            refusal: None,
            summary: format!("best-effort ({status}): only what the kernel enforces"),
            contain_summary: String::new(),
            contain_level: ContainLevel::Ok,
            view_summary: String::new(),
            tmp_root: Some(tmp_root),
            landlock_abi: 0,
        },
        Err(e) => not_enforced(
            settings,
            e,
            node_toml,
            Some(sandbox.clone()),
            Some(plain.clone()),
        ),
    };
    match contain {
        Contained::Yes(s, level) => {
            started.contain_summary = s;
            started.contain_level = level;
            let net_ns = sandbox.containment().is_some_and(|c| c.network_namespace());
            if settings.network == Network::Off && net_ns {
                started.contain_summary.push_str(
                    "; network off: a network namespace of its own per command (only its own \
                     loopback: no TCP, UDP or DNS out",
                );
                // Review N-1: a resolver on a named socket is outside it.
                started.contain_summary.push_str(if abi >= ABI_PATHNAME_SOCKETS {
                    ")"
                } else {
                    ", but below Landlock ABI 9 a resolver that answers over a named Unix socket \
                     can still look names up for a command)"
                });
            }
            path_write_warning(settings, &mut started);
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
            // Review L-c: `strict` promises no named gap; these two are
            // gaps the line names, so `strict` does not offer the mode.
            if settings.contain == crate::contain::ContainMode::Strict {
                let path =
                    path_write_entries(settings, &std::env::var_os("PATH").unwrap_or_default());
                let why = match (risky.is_empty(), path.is_empty()) {
                    (false, _) => Some(format!(
                        "a [sandbox] write entry ({}) holds other programs' sockets (/tmp or \
                         /run/user/<uid>)",
                        risky
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                    (true, false) => Some(format!(
                        "a [sandbox] write entry holds a folder on this node's PATH ({})",
                        path.iter()
                            .map(|(_, d)| d.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                    (true, true) => None,
                };
                if let Some(why) = why {
                    let msg = format!(
                        "the sandbox mode is not offered: [execution] sandbox = \"strict\" in {}, \
                         and {why}: a command could reach past the containment there. Set \
                         sandbox = \"auto\" to offer it with that gap named",
                        node_toml.display()
                    );
                    refuse_sandbox(&mut started, msg);
                }
            }
        }
        Contained::No(msg) => {
            // Not offered (N.10a): never a quiet fall back to Landlock
            // alone. A host session's shell is uncontained by its mode.
            refuse_sandbox(&mut started, msg);
            if settings.network == Network::Off {
                started
                    .contain_summary
                    .push_str(&format!("; {NETWORK_OFF_UNCONTAINED}"));
            }
            path_write_warning(settings, &mut started);
        }
        Contained::Refuse(msg) => refuse_sandbox(&mut started, msg),
    }
    started.view_summary = sandbox.view_line();
    started.landlock_abi = abi;
    Ok(started)
}

/// The folders on this node's `PATH` that lie inside a `[sandbox] write`
/// entry: a command can put a program there that the node's own helpers,
/// or a later command, find by name (T.8c; review R-3). Compared by real
/// path.
pub fn path_write_entries(settings: &Settings, path: &OsStr) -> Vec<(PathBuf, PathBuf)> {
    let mut out = Vec::new();
    for dir in std::env::split_paths(path).filter(|d| d.is_absolute()) {
        let rd = real(&dir);
        if let Some(w) = settings
            .write
            .iter()
            .find(|w| rd.starts_with(real(w)) || dir.starts_with(w))
        {
            out.push((w.clone(), dir));
        }
    }
    out
}

/// Adds the [`path_write_entries`] warning, for this process's `PATH`, to
/// the containment line.
fn path_write_warning(settings: &Settings, started: &mut Started) {
    let found = path_write_entries(settings, &std::env::var_os("PATH").unwrap_or_default());
    if found.is_empty() {
        return;
    }
    let list: Vec<String> = found
        .iter()
        .map(|(w, d)| format!("{} holds {}", w.display(), d.display()))
        .collect();
    started.contain_summary.push_str(&format!(
        "; WARNING: a [sandbox] write entry holds a folder on this node's PATH ({}): a command \
         can put a program there that the node itself (its git, its helpers) or a later command \
         finds by name. Take that folder off the node's PATH, or list it under [sandbox] read",
        list.join("; ")
    ));
    if started.contain_level == ContainLevel::Ok {
        started.contain_level = ContainLevel::Warn;
    }
}

/// A working containment of `form` under `mode` on a kernel with
/// Landlock ABI `abi`: its line, or why the sandbox mode is not offered.
/// `strict` means the guarantee holds; below ABI 9 it does not (T8A-1), so
/// `strict` does not offer the mode there, and `auto` offers it and warns.
fn decide(
    form: crate::contain::Form,
    abi: u32,
    bwrap: &Path,
    mode: crate::contain::ContainMode,
    node_toml: &Path,
) -> Result<(String, ContainLevel), String> {
    let (line, level) = contained_line(form, abi, bwrap);
    // Review L-c: the shared-/proc form is a named gap at every ABI (its
    // `/proc` lists this machine's processes), so `strict` refuses it too.
    let gap = level != ContainLevel::Ok || form == crate::contain::Form::SharedProc;
    if gap && mode == crate::contain::ContainMode::Strict {
        return Err(format!(
            "the sandbox mode is not offered: [execution] sandbox = \"strict\" in {} promises \
             that nothing a command starts outlives it, and here it does not hold: {line}. Set \
             sandbox = \"auto\" to offer it with that gap",
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
/// is off (T.8b, `[execution] sandbox = "auto"`): a gap, as below Landlock
/// ABI 9.
fn network_gap_line(why: &str, abi: u32, retry: std::time::Duration) -> String {
    let abstract_sockets = if abi >= ABI_ABSTRACT_SOCKETS {
        "abstract Unix sockets outside are closed by Landlock (ABI 6 or newer)"
    } else {
        "abstract Unix sockets outside (X11 and others) can be reached (Landlock below ABI 6)"
    };
    format!(
        "; network off WITH A GAP: the probe with --unshare-net failed at start, twice, though \
         the one without it passed ({why}), so commands run contained without a network \
         namespace: TCP is stopped by Landlock, but UDP and DNS still go out, and \
         {abstract_sockets}. The node probes again beside a shell command at most every {} s, \
         and commands started after a probe passed get the namespace (logged). [execution] \
         sandbox = \"strict\" does not offer the sandbox mode with this gap",
        retry.as_secs()
    )
}

/// What the containment line adds where the sandbox mode is not offered
/// and the network is off (T.8b): a `host` session's shell is never
/// contained, and Landlock alone covers only TCP.
pub const NETWORK_OFF_UNCONTAINED: &str = "WARNING: [sandbox] network = \"off\" without the \
     containment (a host session's commands) stops TCP only: UDP and DNS still go out (Landlock \
     does not cover them; the network namespace needs bubblewrap)";

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
    retry: std::time::Duration,
) -> (Sandbox, Contained) {
    use crate::contain::{Contain, ContainMode, Form};
    if settings.contain == ContainMode::Off {
        return (
            sandbox,
            Contained::No(format!(
                "the sandbox mode is not offered: [execution] sandbox = \"off\" in {}",
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
    // The temp root, where the start's own probes and the later ones go.
    let temp_root = scratch.parent().unwrap_or(scratch).to_path_buf();
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
            // The start is the only moment a node decides whether it offers
            // the sandbox mode (N.10a): a probe that fails for a moment (a
            // full process table, no memory) is tried again, a few times,
            // a pause apart (review L-b), each failure logged.
            let mut tries = 0;
            let (found, net_err) = loop {
                tries += 1;
                let mut found = probe(true);
                // T.8b: bubblewrap works but not with `--unshare-net`. Tried
                // once more first (review T8B-1: a failure of the moment is
                // not a failure of the namespace). Then `strict` does not
                // offer the mode; `auto` still contains (more than Landlock
                // alone), with the gap named, and probes the namespace again
                // later ([`StepUp`]). Decided by the node's own probes,
                // never per call.
                let mut net_err = None;
                if settings.network == Network::Off
                    && let Err(e) = &found
                    && let Ok(without) = probe(false)
                {
                    match probe(true) {
                        Ok(again) => found = Ok(again),
                        Err(again) => {
                            net_err = Some(format!("{e}; once more: {again}"));
                            found = Ok(without);
                        }
                    }
                }
                match &found {
                    Err(e) if tries < START_PROBE_TRIES => {
                        network_log(&format!(
                            "shell containment: the start's probe {tries} of \
                             {START_PROBE_TRIES} failed ({e}); trying again in {} s",
                            START_PROBE_PAUSE.as_secs()
                        ));
                        std::thread::sleep(START_PROBE_PAUSE);
                    }
                    _ => break (found, net_err),
                }
            };
            let found = found.map_err(|e| {
                if tries > 1 {
                    format!("{tries} tries at start, the last: {e}")
                } else {
                    e
                }
            });
            match (found, net_err) {
                (Ok(_), Some(e)) if settings.contain == ContainMode::Strict => {
                    return (
                        sandbox,
                        Contained::Refuse(format!(
                            "the sandbox mode is not offered: [sandbox] network = \"off\" and \
                             [execution] sandbox = \"strict\" in {}, and the probe with \
                             --unshare-net failed, twice, though the one without it passed \
                             ({e}); a command without the network namespace would send UDP and \
                             DNS out. Restart the node to probe again, or set sandbox = \"auto\" \
                             to offer it with that gap until a later probe passes",
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
                            let mut sandbox = sandbox.with_containment(c);
                            if let Some(e) = net_err {
                                line.push_str(&network_gap_line(&e, abi, retry));
                                level = ContainLevel::Warn;
                                sandbox.step_up = Some(StepUp::new(retry, temp_root));
                            }
                            (sandbox, Contained::Yes(line, level))
                        }
                        Err(msg) => (sandbox, Contained::Refuse(msg)),
                    };
                }
                (Err(e), _) => e,
            }
        }
    };
    let advice = "install bubblewrap and allow unprivileged user namespaces (README, \
                  \"The shell sandbox\"), then restart the node: it probes only at start (up to three tries)";
    let setting = match settings.contain {
        ContainMode::Strict => "strict",
        _ => "auto",
    };
    (
        sandbox,
        Contained::No(format!(
            "the sandbox mode is not offered ([execution] sandbox = \"{setting}\" in {}): the \
             shell cannot be contained here ({why}); {advice}",
            node_toml.display()
        )),
    )
}

fn not_enforced(
    settings: &Settings,
    why: String,
    node_toml: &Path,
    sandbox: Option<Arc<Sandbox>>,
    plain: Option<Arc<Sandbox>>,
) -> Started {
    let tmp_root = sandbox.as_ref().and_then(|s| s.tmp_root.clone());
    match (settings.mode, sandbox, plain) {
        (Mode::BestEffort, Some(sandbox), Some(plain)) => Started {
            confinement: Confinement::Confined { sandbox, tmp: None },
            uncontained: Confinement::Confined {
                sandbox: plain,
                tmp: None,
            },
            refusal: None,
            summary: format!(
                "NOT ENFORCED, running anyway ([sandbox] mode = \"best-effort\"): {why}"
            ),
            contain_summary: String::new(),
            contain_level: ContainLevel::Ok,
            view_summary: String::new(),
            tmp_root,
            landlock_abi: 0,
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
                uncontained: Confinement::Refused(msg.clone().into()),
                refusal: Some(msg.clone()),
                summary: format!("REFUSING agentic sessions: {msg}"),
                contain_summary: "not probed: the sandbox does not work".into(),
                contain_level: ContainLevel::Refused,
                view_summary: String::new(),
                tmp_root,
                landlock_abi: 0,
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

/// The file view of a contained command (T.8c): the same grants Landlock
/// gets, as mounts at their own paths. Left out: the device files (the view
/// has a private `/dev`), `/proc` (the containment's form decides it), and
/// a path without an identity that does not exist now (Landlock grants
/// nothing there either). A system folder that is a link to inside another
/// one (`/bin -> usr/bin`) becomes the same link; any other is bound with
/// its link followed, as Landlock's rule follows it. A path granted twice
/// is bound once, by its identity if either grant has one. A mount is
/// writable when it, or a folder above it in the view, is granted
/// read-write: Landlock adds the rights of every rule above a path, so a
/// read-only grant inside a read-write one is writable there too. Sorted so
/// that a folder is bound before what is inside it.
pub(crate) fn view(rw: &[Grant], ro: &[Grant]) -> Vec<crate::contain::Mount> {
    use crate::contain::{Mount, Source};
    let granted: Vec<&Path> = rw.iter().chain(ro).map(|g| g.path.as_path()).collect();
    let mut out: Vec<Mount> = Vec::new();
    for (g, write) in rw
        .iter()
        .map(|g| (g, true))
        .chain(ro.iter().map(|g| (g, false)))
    {
        if g.path.starts_with("/dev") || g.path == Path::new("/proc") {
            continue;
        }
        let source = match g.expect {
            Expect::Identity(id) => Source::Checked(id),
            Expect::NotSessionWritable => match std::fs::symlink_metadata(&g.path) {
                Err(_) => continue,
                Ok(md) if md.file_type().is_symlink() => {
                    let inside = g.path.canonicalize().ok().is_some_and(|t| {
                        granted
                            .iter()
                            .any(|o| *o != g.path.as_path() && t.starts_with(o))
                    });
                    match std::fs::read_link(&g.path) {
                        Ok(target) if inside => Source::Symlink(target),
                        _ => Source::Host,
                    }
                }
                Ok(_) => Source::Host,
            },
        };
        match out.iter_mut().find(|m| m.path == g.path) {
            Some(m) => {
                m.write |= write;
                if matches!(source, Source::Checked(_)) {
                    m.source = source;
                }
            }
            None => out.push(Mount {
                path: g.path.clone(),
                write,
                source,
            }),
        }
    }
    let writable: Vec<PathBuf> = rw.iter().map(|g| g.path.clone()).collect();
    for m in &mut out {
        if !matches!(m.source, Source::Symlink(_)) && writable.iter().any(|w| m.path.starts_with(w))
        {
            m.write = true;
        }
    }
    out.sort_by_key(|m| m.path.components().count());
    out
}

/// What `sandbox-exec --probe-…` checks after it confined itself, in place
/// of running a command (the start's self-tests; review R-3: no program is
/// found by name on `PATH`). Each failure has its own exit code.
#[derive(Debug, Default, Clone)]
pub struct Probe {
    /// Must be readable (exit 2).
    pub read: Vec<PathBuf>,
    /// Must not be readable (exit 3).
    pub unreadable: Vec<PathBuf>,
    /// This process must be pid 2 of a PID namespace of its own: bwrap's
    /// init is 1 (exit 4).
    pub pid_ns: bool,
    /// `/proc/self` must be that 2: `/proc` is the namespace's own (exit 5).
    pub own_proc: bool,
    /// `/proc/self/ns/net` must be readable and another than this, the
    /// node's: by identity, not by the interface list (review T8B-2; exit
    /// 6).
    pub net_not: Option<OsString>,
    /// Must not exist at all, not merely be unreadable: the file view (exit
    /// 7).
    pub absent: Vec<PathBuf>,
}

impl Probe {
    fn push_args(&self, c: &mut std::process::Command) {
        for p in &self.read {
            c.arg("--probe-read").arg(p);
        }
        for p in &self.unreadable {
            c.arg("--probe-unreadable").arg(p);
        }
        if self.pid_ns {
            c.arg("--probe-pid-ns");
        }
        if self.own_proc {
            c.arg("--probe-own-proc");
        }
        if let Some(n) = &self.net_not {
            c.arg("--probe-net-not").arg(n);
        }
        for p in &self.absent {
            c.arg("--probe-absent").arg(p);
        }
    }

    /// Whether anything is asked (an empty probe is no probe).
    pub fn is_empty(&self) -> bool {
        self.read.is_empty()
            && self.unreadable.is_empty()
            && !self.pid_ns
            && !self.own_proc
            && self.net_not.is_none()
            && self.absent.is_empty()
    }

    /// The checks, in this process: 0 when all hold, else the first
    /// failure's exit code.
    pub fn run(&self) -> i32 {
        if self.read.iter().any(|p| std::fs::read(p).is_err()) {
            return 2;
        }
        if self.unreadable.iter().any(|p| std::fs::read(p).is_ok()) {
            return 3;
        }
        if self.pid_ns && std::process::id() != 2 {
            return 4;
        }
        if self.own_proc && std::fs::read_link("/proc/self").ok() != Some(PathBuf::from("2")) {
            return 5;
        }
        if let Some(node) = &self.net_not {
            let ours = std::fs::read_link("/proc/self/ns/net").ok();
            if !ours.is_some_and(|n| !n.as_os_str().is_empty() && n.as_os_str() != node) {
                return 6;
            }
        }
        let gone = |p: &PathBuf| {
            std::fs::symlink_metadata(p).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        };
        if !self.absent.iter().all(gone) {
            return 7;
        }
        0
    }
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
    /// The start's self-test, run in place of a command (no command then).
    pub probe: Probe,
    pub command: Vec<OsString>,
}

/// `sandbox-exec`: confine this process, then `exec` the command (or run
/// the probe and return its result). Returns only on failure or after the
/// probe, with the exit code to use. Must run before any other thread
/// exists (Landlock restricts the calling thread; `exec` then drops the
/// others anyway).
pub fn exec(req: ExecRequest) -> i32 {
    use std::os::unix::process::CommandExt;
    let probe = !req.probe.is_empty();
    if probe && !req.command.is_empty() {
        eprintln!("{SUBCOMMAND}: a probe runs no command");
        return 2;
    }
    let command = req.command.split_first();
    if command.is_none() && !probe {
        eprintln!("{SUBCOMMAND}: no command given");
        return 2;
    }
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
    let Some((program, args)) = command else {
        return req.probe.run();
    };
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

    /// Two processes probing in one data directory (a node starting and a
    /// `doctor` beside it) must not share a canary file.
    #[test]
    fn the_canary_is_this_processes_own() {
        let dir = Path::new("/data");
        let name = canary_file(dir);
        assert_eq!(name.parent(), Some(dir));
        assert_eq!(
            name.file_name().and_then(|n| n.to_str()),
            Some(format!("sandbox-canary-{}", std::process::id()).as_str())
        );
    }

    /// Re-check R-1: a node with no descriptor of its own binary says so,
    /// with the reason, on its containment line (`auto`), and `strict`
    /// does not offer the sandbox mode (the node itself is not refused);
    /// with one it says nothing.
    #[test]
    fn a_node_without_a_descriptor_of_its_image_says_so() {
        let d = tempfile::tempdir().unwrap();
        let missing = d.path().join("no-such-binary");
        for (contain, refused) in [
            (crate::contain::ContainMode::Auto, false),
            (crate::contain::ContainMode::Strict, true),
        ] {
            let settings = Settings {
                contain,
                ..Default::default()
            };
            let sb = Sandbox::new(missing.clone(), &settings, None, &[]).unwrap();
            assert!(sb.image.is_none());
            let plain = Arc::new(sb.uncontained());
            let mut s = not_enforced(
                &Settings {
                    mode: Mode::BestEffort,
                    ..settings.clone()
                },
                "test".into(),
                Path::new("node.toml"),
                Some(Arc::new(sb)),
                Some(plain),
            );
            check_image(&mut s, None, &settings, Path::new("node.toml"));
            assert!(
                s.contain_summary
                    .contains("no descriptor of its own binary")
                    && s.contain_summary.contains("No such file"),
                "{}",
                s.contain_summary
            );
            assert!(s.refusal.is_none(), "{contain:?}");
            assert_eq!(s.sandbox_refusal().is_some(), refused, "{contain:?}");
            assert!(
                matches!(s.uncontained, Confinement::Confined { .. }),
                "{contain:?}"
            );
        }
        let exe = PathBuf::from("/bin/true");
        let sb = Sandbox::new(exe, &Settings::default(), None, &[]).unwrap();
        assert!(sb.image.is_some());
        let plain = Arc::new(sb.uncontained());
        let mut s = not_enforced(
            &Settings {
                mode: Mode::BestEffort,
                ..Default::default()
            },
            "test".into(),
            Path::new("node.toml"),
            Some(Arc::new(sb)),
            Some(plain),
        );
        let before = s.contain_summary.clone();
        check_image(&mut s, None, &Settings::default(), Path::new("node.toml"));
        assert_eq!(s.contain_summary, before);
    }

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
        sb.wrapper(&[], &[], false, None, OsStr::new("-"), None)
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

    /// N.10a: where bubblewrap is missing the node does not offer the
    /// sandbox mode, `auto` and `strict` alike, and says why: the sandbox
    /// mode's shell is refused, never run with Landlock alone (before, `auto`
    /// ran it uncontained and only warned). The node itself is not refused:
    /// the uncontained twin (the `host` mode's, the helpers') still builds
    /// its command, uncontained. Simulated with a bubblewrap path that does
    /// not exist (tests cannot change `PATH`).
    #[test]
    fn without_bubblewrap_the_sandbox_mode_is_not_offered() {
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
        for (contain, word) in [(ContainMode::Auto, "auto"), (ContainMode::Strict, "strict")] {
            let s = start(&settings(contain), &data, prot, &[], &toml).unwrap();
            assert!(s.refusal.is_none(), "{contain:?}: {:?}", s.refusal);
            let why = s
                .sandbox_refusal()
                .expect("no sandbox mode without bubblewrap");
            assert!(
                why.contains(&format!("[execution] sandbox = \"{word}\"")),
                "{why}"
            );
            assert!(why.contains("bubblewrap not found"), "{why}");
            assert!(why.contains("install bubblewrap"), "{why}");
            assert_eq!(s.contain_level, ContainLevel::Refused);
            assert!(
                s.contain_summary.contains("NOT OFFERED"),
                "{}",
                s.contain_summary
            );
            assert_eq!(shell(&s.confinement).unwrap_err(), why);
            let (_, contained) = shell(&s.uncontained).unwrap();
            assert!(!contained);
        }

        // `off` with the sandbox off: the mode is not offered; `strict`
        // with it is a configuration error.
        let off = |contain| Settings {
            mode: Mode::Off,
            contain,
            ..Default::default()
        };
        let s = start(&off(ContainMode::Auto), &data, prot, &[], &toml).unwrap();
        assert!(s.sandbox_refusal().is_some());
        assert!(matches!(s.uncontained, Confinement::Unconfined));
        let e = start(&off(ContainMode::Strict), &data, prot, &[], &toml).unwrap_err();
        assert!(e.contains("[execution] sandbox = \"strict\""), "{e}");
    }

    /// A sandbox made for the `sandbox` mode refuses a shell command when
    /// it has no containment, rather than run it with Landlock alone; its
    /// uncontained twin never contains, whatever the original holds.
    #[test]
    fn the_sandbox_modes_shell_is_never_landlock_alone() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().canonicalize().unwrap();
        let mut sb = Sandbox::new("/bin/true".into(), &Settings::default(), None, &[]).unwrap();
        let twin = sb.uncontained();
        sb.require_contain = true;
        let build = |sb: Sandbox| {
            Confinement::Confined {
                sandbox: Arc::new(sb),
                tmp: None,
            }
            .shell(
                OsStr::new("bash"),
                &[],
                &ws,
                None,
                &Default::default(),
                &[],
                Grants::default(),
            )
        };
        assert_eq!(build(sb).unwrap_err(), NO_CONTAINMENT);
        let contained = Sandbox::new("/bin/true".into(), &Settings::default(), None, &[])
            .unwrap()
            .with_containment(crate::contain::Contain::new(
                "/usr/bin/bwrap".into(),
                crate::contain::Form::Full,
            ));
        let twin_of_contained = contained.uncontained();
        assert!(twin_of_contained.containment().is_none());
        assert!(!build(twin).unwrap().1);
        assert!(!build(twin_of_contained).unwrap().1);
    }

    /// T8A-1: the line depends on the effective Landlock ABI. From ABI 9
    /// the claim is made in full; below, it is a warning that names the
    /// session bus (and below ABI 6 abstract sockets), and `strict` does
    /// not offer the sandbox mode. The shared-`/proc` form says so in every
    /// case.
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
            let e = decide(Form::Full, abi, b, ContainMode::Strict, toml).unwrap_err();
            assert!(
                e.contains("[execution] sandbox = \"strict\"") && e.contains("auto"),
                "{e}"
            );
            assert!(decide(Form::Full, abi, b, ContainMode::Auto, toml).is_ok());
        }
        let (line, level) = contained_line(Form::SharedProc, 9, b);
        assert_eq!(level, ContainLevel::Ok);
        assert!(line.contains("without a /proc of its own"), "{line}");
        let (_, level) = contained_line(Form::SharedProc, 4, b);
        assert_eq!(level, ContainLevel::Warn);
        assert!(decide(Form::Full, 9, b, ContainMode::Strict, toml).is_ok());
        // Review L-c: the shared-/proc form is a gap `strict` names, at
        // every ABI.
        let e = decide(Form::SharedProc, 9, b, ContainMode::Strict, toml).unwrap_err();
        assert!(e.contains("without a /proc of its own"), "{e}");
        assert!(decide(Form::SharedProc, 9, b, ContainMode::Auto, toml).is_ok());
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
                if let Err(e) = decide(form, abi, b, ContainMode::Strict, toml) {
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

    /// T.8c: the view is the grants, at their own paths. A read-only grant
    /// inside a read-write one is writable (Landlock adds the rights of the
    /// rules above a path); the devices and `/proc` are left to the
    /// containment; a path granted twice is bound once, by its identity; a
    /// system link into another system folder is a link; a folder comes
    /// before what is inside it.
    #[test]
    fn the_view_is_the_grants() {
        use crate::contain::Source;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let ws = root.join("ws");
        let inner = ws.join("vendor");
        std::fs::create_dir_all(&inner).unwrap();
        let id = |p: &Path| harness_core::folder::Identity::of(p).unwrap();
        let rw = vec![
            Grant::identified(&ws, id(&ws), "the session's folder"),
            Grant::system(Path::new("/dev/null")),
        ];
        let ro = vec![
            Grant::system(Path::new("/usr")),
            Grant::system(Path::new("/bin")),
            Grant::system(Path::new("/proc")),
            Grant::system(&root.join("missing")),
            Grant::identified(&inner, id(&inner), "a node.toml [sandbox] read entry"),
            Grant::system(&ws),
        ];
        let v = view(&rw, &ro);
        let at = |p: &Path| v.iter().position(|m| m.path == p);
        assert!(at(Path::new("/dev/null")).is_none() && at(Path::new("/proc")).is_none());
        assert!(at(&root.join("missing")).is_none());
        let w = &v[at(&ws).unwrap()];
        assert!(w.write && w.source == Source::Checked(id(&ws)), "{w:?}");
        assert_eq!(v.iter().filter(|m| m.path == ws).count(), 1);
        let i = &v[at(&inner).unwrap()];
        assert!(i.write, "a read grant inside a write grant: {i:?}");
        assert!(at(&ws).unwrap() < at(&inner).unwrap());
        let usr = &v[at(Path::new("/usr")).unwrap()];
        assert!(!usr.write && usr.source == Source::Host);
        if std::fs::symlink_metadata("/bin").is_ok_and(|m| m.file_type().is_symlink())
            && Path::new("/bin")
                .canonicalize()
                .is_ok_and(|t| t.starts_with("/usr"))
        {
            let b = &v[at(Path::new("/bin")).unwrap()];
            assert_eq!(
                b.source,
                Source::Symlink(std::fs::read_link("/bin").unwrap())
            );
        }
    }

    /// The probe's checks, in this process (outside any namespace).
    #[test]
    fn the_probe_checks_what_it_is_asked() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f");
        std::fs::write(&file, "x").unwrap();
        let gone = dir.path().join("gone");
        assert!(Probe::default().is_empty());
        let p = |probe: Probe| probe.run();
        assert_eq!(
            p(Probe {
                read: vec![file.clone()],
                absent: vec![gone.clone()],
                ..Default::default()
            }),
            0
        );
        assert_eq!(
            p(Probe {
                read: vec![gone.clone()],
                ..Default::default()
            }),
            2
        );
        assert_eq!(
            p(Probe {
                unreadable: vec![file.clone()],
                ..Default::default()
            }),
            3
        );
        // Not pid 2 here.
        assert_eq!(
            p(Probe {
                pid_ns: true,
                ..Default::default()
            }),
            4
        );
        let ours = std::fs::read_link("/proc/self/ns/net").unwrap();
        assert_eq!(
            p(Probe {
                net_not: Some(ours.into_os_string()),
                ..Default::default()
            }),
            6
        );
        assert_eq!(
            p(Probe {
                absent: vec![file],
                ..Default::default()
            }),
            7
        );
    }

    /// T.8c, review R-3: a `PATH` folder inside a `write` entry is named,
    /// by real path; one outside is not.
    #[test]
    fn a_path_folder_inside_a_write_entry_is_named() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let cargo = root.join(".cargo");
        std::fs::create_dir_all(cargo.join("bin")).unwrap();
        let s = Settings {
            write: vec![cargo.clone()],
            ..Default::default()
        };
        let path = std::env::join_paths([cargo.join("bin"), PathBuf::from("/usr/bin")]).unwrap();
        assert_eq!(
            path_write_entries(&s, &path),
            vec![(cargo.clone(), cargo.join("bin"))]
        );
        assert!(path_write_entries(&s, OsStr::new("/usr/bin:relative")).is_empty());
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
