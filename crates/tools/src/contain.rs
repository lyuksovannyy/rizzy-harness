//! Process containment of the agent's shell (ADOPTION_PLAN T.8a, PLAN.md T9).
//!
//! Landlock decides what a command may open; it does nothing about
//! processes. A child that calls `setsid` leaves the process group the tool
//! signals, and every child outlives a `kill -9` of the node (experiment
//! B.1). Here each agent `bash` call runs inside its own PID namespace made
//! by bubblewrap: `bwrap --die-with-parent --new-session --unshare-pid
//! --dev-bind / / --proc /proc --chdir <folder> -- <harness> sandbox-exec …
//! -- bash -c <cmd>`. When the namespace's first process (bwrap's init) dies,
//! the kernel kills every process in it, whatever its session or signal
//! mask. It dies when the command ends, when the node kills bwrap, and,
//! through `--die-with-parent` (`PR_SET_PDEATHSIG`), when the node dies.
//!
//! The file system is the one the command sees without bwrap (`/` bound as
//! it is, devices included): Landlock inside still decides what is readable
//! and writable (`docs/container-experiment.md` §8: `sandbox-exec` reaches
//! full enforcement inside bwrap). Only `/proc` is the namespace's own.
//!
//! **Which thread spawns.** `PR_SET_PDEATHSIG` fires when the *thread* that
//! forked bwrap exits, not the process. A command spawned from a tokio
//! blocking-pool thread, or any thread that later ends, would be killed for
//! no reason. Every contained command is therefore spawned by one thread
//! that lives as long as the process ([`spawn`]).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, mpsc};

use serde::{Deserialize, Serialize};

/// `[sandbox] contain` in node.toml.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContainMode {
    /// Contain the shell where bubblewrap works; elsewhere run it as
    /// before (Landlock alone) and say so in the log and in `doctor`.
    #[default]
    Auto,
    /// Refuse agentic sessions unless the shell can be contained.
    Required,
    /// Never contain (an explicit, logged choice).
    Off,
}

/// How a command is contained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// A PID namespace with its own `/proc`: the command sees only its
    /// own processes.
    Full,
    /// A PID namespace with the outer `/proc` (where a fresh `/proc`
    /// cannot be mounted, as inside a default rootless container): every
    /// process still dies with the namespace, but `/proc` lists the outer
    /// namespace's processes, under numbers that are not the command's own
    /// (`$$` is 2, `/proc/2` is another process).
    SharedProc,
}

/// A working bubblewrap and the form the probe found.
#[derive(Debug, Clone)]
pub struct Contain {
    bwrap: PathBuf,
    form: Form,
}

impl Contain {
    pub fn new(bwrap: PathBuf, form: Form) -> Self {
        Self { bwrap, form }
    }

    pub fn form(&self) -> Form {
        self.form
    }

    pub fn bwrap(&self) -> &Path {
        &self.bwrap
    }

    /// `bwrap <args> -- <program>`, started in `cwd`.
    pub(crate) fn command(&self, program: &Path, cwd: &Path) -> std::process::Command {
        let mut c = std::process::Command::new(&self.bwrap);
        c.args(self.args(cwd)).arg(program);
        c
    }

    /// The arguments up to and including `--`.
    fn args(&self, cwd: &Path) -> Vec<OsString> {
        let mut a: Vec<OsString> = [
            // Through PR_SET_PDEATHSIG: bwrap dies with the thread that
            // started it, and bwrap's init dies with bwrap.
            "--die-with-parent",
            // bwrap's init calls setsid: the command has no controlling
            // terminal (no TIOCSTI into the node's), and its session is the
            // process group the node sends the graceful SIGTERM to.
            "--new-session",
            "--unshare-pid",
            // The file system as it is; Landlock inside decides.
            "--dev-bind",
            "/",
            "/",
        ]
        .iter()
        .map(OsString::from)
        .collect();
        if self.form == Form::Full {
            a.extend(["--proc", "/proc"].map(OsString::from));
        }
        a.push("--chdir".into());
        a.push(cwd.as_os_str().to_owned());
        a.push("--".into());
        a
    }
}

/// Where bubblewrap is: `bwrap` on the node's `PATH`, canonical. Resolved
/// once at start, like the harness binary itself.
pub fn find_bwrap() -> Result<PathBuf, String> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        if !dir.is_absolute() {
            continue;
        }
        let p = dir.join("bwrap");
        if p.is_file() {
            return p
                .canonicalize()
                .map_err(|e| format!("{}: {e}", p.display()));
        }
    }
    Err("bubblewrap (`bwrap`) is not installed (not found on PATH)".into())
}

/// The script the probe runs inside the namespace: `$$` is 2 only in a new
/// PID namespace (bwrap's init is 1, the shell 2); with `own_proc`,
/// `/proc/self` must be that same 2, so `/proc` is the namespace's own.
pub(crate) fn probe_script(own_proc: bool) -> String {
    let mut s = String::from("cat \"$1\" >/dev/null || exit 2; [ \"$$\" = 2 ] || exit 4;");
    if own_proc {
        s.push_str(" read -r p _ </proc/self/stat; [ \"$p\" = 2 ] || exit 5;");
    }
    s
}

/// The host pid of the namespace's init, bwrap's only child (it is also
/// the session and process group of the command, `--new-session`). Read
/// from `/proc/<bwrap>/task/<bwrap>/children`; `None` when it is gone or
/// the kernel has no such file. Used only for the graceful SIGTERM: the
/// kill itself never depends on it. The init cannot be reaped by anyone
/// but bwrap, which is the caller's unreaped child, so the number names
/// that init until bwrap reaps it and exits.
pub(crate) fn sandbox_init(bwrap_pid: i32) -> Option<i32> {
    std::fs::read_to_string(format!("/proc/{bwrap_pid}/task/{bwrap_pid}/children"))
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

type Reply = tokio::sync::oneshot::Sender<std::io::Result<tokio::process::Child>>;

struct Job {
    cmd: tokio::process::Command,
    handle: tokio::runtime::Handle,
    reply: Reply,
}

static SPAWNER: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();

/// Spawn `cmd` from the process's one spawner thread, which never exits
/// before the process does, so `--die-with-parent` fires only when the node
/// dies (see the module docs). The child is registered with the caller's
/// tokio runtime.
pub(crate) async fn spawn(cmd: tokio::process::Command) -> std::io::Result<tokio::process::Child> {
    let tx = SPAWNER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("rizzy-contain-spawner".into())
            .spawn(move || {
                // Ends only when every sender is gone, which the static
                // never is: this thread lives as long as the process.
                while let Ok(mut job) = rx.recv() {
                    let r = {
                        let _rt = job.handle.enter();
                        job.cmd.spawn()
                    };
                    let _ = job.reply.send(r);
                }
            })
            .expect("start the containment spawner thread");
        Mutex::new(tx)
    });
    let (reply, rx) = tokio::sync::oneshot::channel();
    let job = Job {
        cmd,
        handle: tokio::runtime::Handle::current(),
        reply,
    };
    let sent = tx
        .lock()
        .map_err(|_| std::io::Error::other("the containment spawner is gone"))?
        .send(job);
    if sent.is_err() {
        return Err(std::io::Error::other("the containment spawner is gone"));
    }
    rx.await
        .map_err(|_| std::io::Error::other("the containment spawner is gone"))?
}

/// For tests of the spawner: spawn `cmd` the way a contained `bash` call
/// does.
#[doc(hidden)]
pub async fn spawn_for_test(
    cmd: tokio::process::Command,
) -> std::io::Result<tokio::process::Child> {
    spawn(cmd).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_parse_kebab_case() {
        let m: ContainMode = serde_json::from_str("\"required\"").unwrap();
        assert_eq!(m, ContainMode::Required);
        assert_eq!(ContainMode::default(), ContainMode::Auto);
    }

    #[test]
    fn the_shared_proc_form_mounts_no_proc() {
        let full = Contain::new("/usr/bin/bwrap".into(), Form::Full);
        let a = full.args(Path::new("/w"));
        assert!(a.windows(2).any(|w| w[0] == "--proc" && w[1] == "/proc"));
        assert!(a.iter().any(|x| x == "--die-with-parent"));
        assert!(a.iter().any(|x| x == "--new-session"));
        assert_eq!(a.last().unwrap(), "--");
        let shared = Contain::new("/usr/bin/bwrap".into(), Form::SharedProc);
        assert!(!shared.args(Path::new("/w")).iter().any(|x| x == "--proc"));
    }
}
