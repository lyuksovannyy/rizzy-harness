//! Process containment of the agent's shell (ADOPTION_PLAN T.8a, T.8e;
//! PLAN.md T9).
//!
//! Landlock decides what a command may open; it does nothing about
//! processes. A child that calls `setsid` leaves the process group the tool
//! signals, and every child outlives a `kill -9` of the node (experiment
//! B.1). Here each agent `bash` call runs inside its own PID namespace made
//! by bubblewrap, under a guard of its own: `<harness> sandbox-guard --mark
//! … -- bwrap --die-with-parent --new-session --unshare-pid --dev-bind / /
//! --proc /proc --chdir <folder> -- <harness> sandbox-exec … -- bash -c
//! <cmd>`. When the namespace's first process (bwrap's init) dies, the
//! kernel kills every process in it, whatever its session or signal mask.
//!
//! **The guard** ([`crate::guard`], T.8e) is bubblewrap's parent and holds
//! the command until it is dead: its stdin is one end of a socket pair
//! whose other end the node holds ([`ContainedChild`]; for the moment
//! between `fork` and `exec`, so does every child the node forks). The
//! channel closing, by the node to cancel or by the kernel when the node
//! dies, is the kill order, and so is the guard's parent changing (review
//! T8E-1: a child of the node waiting before `exec` keeps the channel
//! open); the guard kills and repeats until it has reaped
//! everything, and tells the node when it has, or that something is still
//! alive. bubblewrap's `--die-with-parent` names the guard, so a guard that
//! is itself killed takes the namespace with it, and [`sweep`] ends at the
//! next start what a doubly unlucky kill left.
//!
//! The file system is the one the command sees without bwrap (`/` bound as
//! it is, devices included): Landlock inside still decides what is readable
//! and writable (`docs/container-experiment.md` §8: `sandbox-exec` reaches
//! full enforcement inside bwrap). Only `/proc` is the namespace's own.
//!
//! **Which thread spawns** no longer matters. `PR_SET_PDEATHSIG` fires when
//! the *thread* that forked bwrap exits; T.8a therefore spawned every
//! command from one long-lived thread. The guard is single-threaded and
//! forks bubblewrap itself, and the node's guard does not depend on a death
//! signal at all (the channel closes when the process dies, whichever
//! thread spawned it), so a command is spawned where it is asked for
//! (`cli/tests/contain.rs` `a_command_outlives_the_thread_that_started_it`).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

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

    /// `<exe> sandbox-guard --node <pid> [--mark …] -- bwrap <args> --
    /// <exe>`: the guard, which starts bubblewrap in `cwd` (T.8e).
    ///
    /// The guard is started from `image`, a descriptor of the binary this
    /// process holds for its life, opened when the first sandbox was made
    /// (for the node, `/proc/self/exe`: its own running image), as
    /// `/proc/self/fd/<n>`, resolved by `execve` in the forked child before
    /// the close-on-exec descriptor closes: whatever is at `exe` now, a
    /// command that can write there cannot replace its guard (review
    /// T8E-5). Without it (it could not be opened), `exe`. Its `argv[0]` is
    /// `exe`, for `ps`. `sandbox-exec` inside the namespace is still started
    /// from `exe` (there neither descriptor nor `/proc/self/exe` is the
    /// node's); `sandbox::start` warns where a session can write it.
    ///
    /// With `net_off` (`[sandbox] network = "off"`, decided by the node's
    /// [`crate::sandbox::Sandbox`], never per call) the command also gets a
    /// network namespace of its own (T.8b).
    pub(crate) fn command(
        &self,
        exe: &Path,
        image: Option<std::os::fd::RawFd>,
        cwd: &Path,
        mark: Option<&Mark>,
        net_off: bool,
    ) -> std::process::Command {
        use std::os::unix::process::CommandExt;
        let mut c = match image {
            Some(fd) => std::process::Command::new(format!("/proc/self/fd/{fd}")),
            None => std::process::Command::new(exe),
        };
        c.arg0(exe).arg(crate::guard::SUBCOMMAND);
        c.arg(crate::guard::NODE)
            .arg(std::process::id().to_string());
        if let Some(m) = mark {
            m.push_args(&mut c);
        }
        c.arg("--")
            .arg(&self.bwrap)
            .args(self.args(cwd, net_off))
            .arg(exe);
        c
    }

    /// `bwrap <args> -- <program>`, without the guard (tests, and the
    /// measurement of what the guard costs).
    #[doc(hidden)]
    pub fn bwrap_command(&self, program: &Path, cwd: &Path) -> std::process::Command {
        let mut c = std::process::Command::new(&self.bwrap);
        c.args(self.args(cwd, false)).arg(program);
        c
    }

    /// The arguments up to and including `--`.
    fn args(&self, cwd: &Path, net_off: bool) -> Vec<OsString> {
        let mut a: Vec<OsString> = [
            // Through PR_SET_PDEATHSIG: bwrap dies with its guard (a
            // single-threaded process), and bwrap's init dies with bwrap.
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
        if net_off {
            // T.8b: a network namespace with only its own loopback (which
            // bwrap brings up), so no UDP or DNS leaves either, which
            // Landlock's TCP rule inside does not cover. Abstract Unix
            // sockets belong to it too: the host's are not there.
            a.push("--unshare-net".into());
        }
        a.push("--chdir".into());
        a.push(cwd.as_os_str().to_owned());
        a.push("--".into());
        a
    }
}

/// Where bubblewrap is: `bwrap` on the node's `PATH`, canonical. Resolved
/// once at start, like the harness binary itself.
///
/// bubblewrap is the one program on the command path that runs outside
/// the sandbox, at every start and for every call, so only one the node's
/// own user cannot replace is taken (review N-1): the file and every folder
/// above it owned by root, none writable by group or others (a folder with
/// the sticky bit, as `/nix/store`, is accepted: nobody but root can replace
/// root's entries there), and the file executable. A `bwrap` that fails
/// that (one an agent planted in a project's `bin` ahead of `/usr/bin`) is
/// skipped and named. Elsewhere, name it in node.toml: `[sandbox] bwrap`.
pub fn find_bwrap() -> Result<PathBuf, String> {
    find_bwrap_in(&std::env::var_os("PATH").unwrap_or_default())
}

pub(crate) fn find_bwrap_in(path: &std::ffi::OsStr) -> Result<PathBuf, String> {
    let mut skipped = Vec::new();
    for dir in std::env::split_paths(path) {
        if !dir.is_absolute() {
            continue;
        }
        let p = dir.join("bwrap");
        if !p.is_file() {
            continue;
        }
        let real = match p.canonicalize() {
            Ok(r) => r,
            Err(e) => {
                skipped.push(format!("{}: {e}", p.display()));
                continue;
            }
        };
        match trusted(&real) {
            Ok(()) => return Ok(real),
            Err(why) => skipped.push(format!("{} ({why})", p.display())),
        }
    }
    if skipped.is_empty() {
        Err("bubblewrap (`bwrap`) is not installed (not found on PATH)".into())
    } else {
        Err(format!(
            "no bubblewrap on PATH that this user cannot replace; skipped: {}. Name one in \
             node.toml ([sandbox] bwrap = \"/absolute/path\") to use it anyway",
            skipped.join("; ")
        ))
    }
}

/// Whether `file` (a canonical path) and every folder above it are out of
/// reach of anyone but root (see [`find_bwrap`]). ACLs are not read.
pub(crate) fn trusted(file: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let md = std::fs::metadata(file).map_err(|e| e.to_string())?;
    if md.mode() & 0o111 == 0 {
        return Err("not executable".into());
    }
    let mut at = Some(file);
    while let Some(p) = at {
        let md = std::fs::metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
        if md.uid() != 0 {
            return Err(format!("{} is not owned by root", p.display()));
        }
        let sticky_dir = md.is_dir() && md.mode() & 0o1000 != 0;
        if md.mode() & 0o022 != 0 && !sticky_dir {
            return Err(format!("{} is writable by group or others", p.display()));
        }
        at = p.parent();
    }
    Ok(())
}

/// The script the probe runs inside the namespace: `$$` is 2 only in a new
/// PID namespace (bwrap's init is 1, the shell 2); with `own_proc`,
/// `/proc/self` must be that same 2, so `/proc` is the namespace's own;
/// with `own_net`, `/proc/net/dev` (the reader's network namespace, in
/// either form of `/proc`) must list `lo` and nothing else.
pub(crate) fn probe_script(own_proc: bool, own_net: bool) -> String {
    let mut s = String::from("cat \"$1\" >/dev/null || exit 2; [ \"$$\" = 2 ] || exit 4;");
    if own_proc {
        s.push_str(" read -r p _ </proc/self/stat; [ \"$p\" = 2 ] || exit 5;");
    }
    if own_net {
        // Past the two header lines, one line per interface.
        s.push_str(
            " n=0; { read -r _; read -r _; while IFS=': ' read -r i _; do \
             [ \"$i\" = lo ] || exit 6; n=$((n+1)); done; } </proc/net/dev; \
             [ \"$n\" = 1 ] || exit 6;",
        );
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

/// SIGKILL a contained command's whole namespace now, on every kill path
/// (review T8A-2). Killing bubblewrap alone ends the namespace only through
/// the init's own `PR_SET_PDEATHSIG`, which bubblewrap's init sets late,
/// after its `setsid` has taken it out of the group: a kill in that moment
/// left the namespace running. So: stop bubblewrap's group (it can no longer
/// fork its init or reap it, so the number read next stays that init's),
/// SIGKILL the init (from the parent namespace that ends the namespace
/// outright), then SIGKILL the group. Without the `children` file there is
/// no init to name, and the kill falls back to the group alone, as before.
pub fn kill_namespace(bwrap_pid: i32) {
    kill_namespace_with(bwrap_pid, sandbox_init)
}

pub(crate) fn kill_namespace_with(bwrap_pid: i32, init_of: impl Fn(i32) -> Option<i32>) {
    use nix::sys::signal::{Signal, kill, killpg};
    use nix::unistd::Pid;
    let group = Pid::from_raw(bwrap_pid);
    let _ = killpg(group, Signal::SIGSTOP);
    if let Some(init) = init_of(bwrap_pid) {
        let _ = kill(Pid::from_raw(init), Signal::SIGKILL);
    }
    let _ = killpg(group, Signal::SIGKILL);
}

/// The graceful stop: SIGTERM to the command's group, the session of the
/// namespace's init (bwrap does not forward signals; the init ignores it).
/// Without the init's number, SIGTERM goes to bubblewrap, which ends the
/// namespace at once, without the grace (bubblewrap dies, its init by its
/// death signal, or by the kill that follows the grace period).
pub(crate) fn term_namespace_with(bwrap_pid: i32, init_of: impl Fn(i32) -> Option<i32>) {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;
    let target = init_of(bwrap_pid).unwrap_or(bwrap_pid);
    let _ = killpg(Pid::from_raw(target), Signal::SIGTERM);
}

/// The marker a contained command carries in its guard's and in
/// bubblewrap's command lines (T.8e): the node's temp root (inside its data
/// directory) and the session. [`sweep`] looks for it.
#[derive(Debug, Clone)]
pub struct Mark {
    pub root: PathBuf,
    pub session: OsString,
}

impl Mark {
    pub(crate) fn push_args(&self, c: &mut std::process::Command) {
        c.arg(crate::guard::MARK)
            .arg(&self.root)
            .arg(crate::guard::MARK_SESSION)
            .arg(&self.session);
    }
}

/// How long [`ContainedChild::stop`] waits for its guard's answer.
/// The grace, the guard's bound, and 300 ms for a guard that is slow to
/// answer: with [`STUCK_DRAIN`] it fits inside the agent loop's grace for a
/// cancelled tool (`cli/tests/guard.rs` checks it against the real
/// `TOOL_CANCEL_GRACE`), so its result is the one recorded (review T8E-3).
pub const STOP_BOUND: std::time::Duration = crate::guard::TERM_GRACE
    .saturating_add(crate::guard::CONFIRM_BOUND)
    .saturating_add(std::time::Duration::from_millis(300));

/// How long the sweep after an unconfirmed stop waits for what it killed
/// (review T8E-2), inside the same budget.
pub const SWEEP_IN_STOP: std::time::Duration = std::time::Duration::from_millis(300);

/// How long the bash tool waits for the output pipes after a stop that did
/// not confirm the end (a process still alive holds them).
pub const STUCK_DRAIN: std::time::Duration = std::time::Duration::from_millis(100);

/// How a stop of a contained command ended, as its guard reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stopped {
    /// Everything the command started was reaped, this long after the
    /// order.
    Ended(std::time::Duration),
    /// Something was still not dead this long after the order (stuck in
    /// I/O the kernel will not interrupt); the guard keeps killing it.
    StillAlive(std::time::Duration),
    /// The guard gave no report (it was killed, or did not answer in
    /// time): the kill was ordered, its end is not known.
    Unconfirmed(String),
}

/// A spawned contained command: its guard (the child), and the node's end
/// of the guard's control channel (T.8e). There is no other way to hold
/// one. Wherever it is dropped (an answer nobody reads, a call dropped
/// mid-run, an early return, a panic) the channel closes, which is the
/// guard's kill order; a `kill -9` of the node closes it the same way. The
/// guard is never signalled from here: a SIGKILL to it would leave the
/// namespace to bubblewrap's death signals, the slower second line.
#[derive(Debug)]
pub struct ContainedChild {
    child: tokio::process::Child,
    ctl: Option<tokio::net::UnixStream>,
}

impl ContainedChild {
    /// Stop the command and wait for its guard's answer: with `graceful`,
    /// SIGTERM to the command first and the guard's 2 s grace. Returns when
    /// the guard reported the end (and has exited), or that something is
    /// still alive [`crate::guard::CONFIRM_BOUND`] after the kill, or after
    /// a bound of its own when the guard does not answer at all.
    pub async fn stop(&mut self, graceful: bool) -> Stopped {
        use crate::guard::{Report, TERM};
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let Some(mut ctl) = self.ctl.take() else {
            return Stopped::Unconfirmed("the stop was already ordered".into());
        };
        let started = std::time::Instant::now();
        if graceful {
            let _ = ctl.write_all(&[TERM]).await;
        }
        // The half-close is the order; the read half stays for the answer.
        let _ = ctl.shutdown().await;
        let bound = STOP_BOUND;
        let answer = tokio::time::timeout(bound, async move {
            let mut lines = tokio::io::BufReader::new(ctl).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                if let Some(r) = Report::parse(&l) {
                    return Some(r);
                }
            }
            None
        })
        .await;
        match answer {
            Ok(Some(Report::Ended(d, _))) => {
                // It exits right after the report; reap it.
                let _ = tokio::time::timeout(std::time::Duration::from_secs(1), self.child.wait())
                    .await;
                Stopped::Ended(d)
            }
            Ok(Some(Report::StillAlive(d))) => Stopped::StillAlive(d),
            Ok(None) => {
                use std::os::unix::process::ExitStatusExt;
                let status =
                    tokio::time::timeout(std::time::Duration::from_secs(1), self.child.wait())
                        .await;
                match status {
                    // A guard that exits on its own has reaped everything
                    // first (it ended with the command, just before the
                    // order): that is the confirmation.
                    // 125 is the guard's own failure, not that proof (N-6).
                    Ok(Ok(s)) if s.signal().is_none() && s.code() != Some(125) => {
                        Stopped::Ended(started.elapsed())
                    }
                    Ok(Ok(s)) => {
                        Stopped::Unconfirmed(format!("its guard was killed without a report ({s})"))
                    }
                    _ => Stopped::Unconfirmed("its guard ended without a report".into()),
                }
            }
            Err(_) => Stopped::Unconfirmed(format!(
                "its guard did not answer within {:.1} s",
                bound.as_secs_f64()
            )),
        }
    }

    /// After the guard exited on its own (the command's end, as the node
    /// saw it): the report it left on the channel, if it was given a kill
    /// order the node did not send (a signal to it, bubblewrap dying
    /// first). `None` for an ordinary end.
    pub async fn exit_report(&mut self) -> Option<crate::guard::Report> {
        use tokio::io::AsyncBufReadExt;
        let ctl = self.ctl.take()?;
        let mut lines = tokio::io::BufReader::new(ctl).lines();
        let line = tokio::time::timeout(std::time::Duration::from_millis(50), lines.next_line())
            .await
            .ok()?
            .ok()??;
        crate::guard::Report::parse(&line)
    }

    /// Wait for it and collect its output, like `Child::wait_with_output`,
    /// keeping the channel open (so not ordering a kill) meanwhile.
    pub async fn wait_with_output(mut self) -> std::io::Result<std::process::Output> {
        async fn read(p: Option<impl tokio::io::AsyncRead + Unpin>) -> Vec<u8> {
            use tokio::io::AsyncReadExt;
            let mut buf = Vec::new();
            if let Some(mut p) = p {
                let _ = p.read_to_end(&mut buf).await;
            }
            buf
        }
        let (out, err) = (self.child.stdout.take(), self.child.stderr.take());
        let (status, stdout, stderr) = tokio::join!(self.child.wait(), read(out), read(err));
        Ok(std::process::Output {
            status: status?,
            stdout,
            stderr,
        })
    }
}

impl std::ops::Deref for ContainedChild {
    type Target = tokio::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.child
    }
}

impl std::ops::DerefMut for ContainedChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.child
    }
}

/// Spawn `cmd` (a guard's command line, [`Contain::command`]) with the
/// guard's control channel as its stdin. The node keeps the only other
/// end: both ends are close-on-exec, so no other child inherits it, and the
/// copy in `cmd` is closed when `cmd` is dropped here. Never killed on
/// drop: the closing channel is the kill.
pub(crate) fn spawn(mut cmd: tokio::process::Command) -> std::io::Result<ContainedChild> {
    let (ours, theirs) = std::os::unix::net::UnixStream::pair()?;
    ours.set_nonblocking(true)?;
    let ours = tokio::net::UnixStream::from_std(ours)?;
    cmd.stdin(std::process::Stdio::from(std::os::fd::OwnedFd::from(
        theirs,
    )))
    .kill_on_drop(false);
    let child = cmd.spawn();
    drop(cmd);
    Ok(ContainedChild {
        child: child?,
        ctl: Some(ours),
    })
}

/// For tests: spawn `cmd` the way a contained `bash` call does.
#[doc(hidden)]
pub async fn spawn_for_test(cmd: tokio::process::Command) -> std::io::Result<ContainedChild> {
    spawn(cmd)
}

// ---- the sweep: what a dead node and a dead guard left ----

/// What [`sweep`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Left {
    /// A namespace nothing alive holds (bubblewrap whose guard died, or an
    /// init whose bubblewrap died): killed.
    Namespace,
    /// A live guard of this node's data directory whose parent is no longer
    /// the node named in its own `--node` (that node died while a child of
    /// it held the channel, review T8E-1): told to kill (SIGTERM, its kill
    /// order). A guard of a node that is alive (another process on the same
    /// data directory) is never signalled (re-check R-1).
    Guard,
    /// A guard of this data directory without a `--node` this can read:
    /// left alone, named.
    GuardUnknown,
}

/// One thing [`sweep`] found and acted on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Swept {
    pub pid: i32,
    /// As it came from the process's own arguments, which a command can
    /// choose: print it escaped (`{:?}`), never as it is (re-check R-2).
    pub session: String,
    pub what: Left,
    /// Gone within 2 s: the process and every child it was killed with have
    /// no `/proc` entry, or are zombies, or their number now belongs to a
    /// process started later. A killed init whose command line is empty
    /// (released, waiting for a process the kernel cannot end yet) is not
    /// gone (review T8E-4).
    pub ended: bool,
}

/// One `/proc` entry, as much as the sweep needs.
#[derive(Clone)]
struct Proc {
    pid: i32,
    ppid: i32,
    uid: u32,
    /// Start time in clock ticks since boot: with the pid, it names one
    /// process for good.
    start: u64,
    argv: Vec<Vec<u8>>,
}

fn read_proc(pid: i32) -> Option<Proc> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let f: Vec<&str> = stat.rsplit(')').next()?.split_whitespace().collect();
    if matches!(*f.first()?, "Z" | "X" | "x") {
        return None;
    }
    let ppid = f.get(1)?.parse().ok()?;
    let start = f.get(19)?.parse().ok()?;
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let uid = status
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    let argv = std::fs::read(format!("/proc/{pid}/cmdline"))
        .ok()?
        .split(|b| *b == 0)
        .map(<[u8]>::to_vec)
        .collect();
    Some(Proc {
        pid,
        ppid,
        uid,
        start,
        argv,
    })
}

/// Whether the process `pid` that started at `start` is gone: no entry, a
/// zombie, or the number reused by a later process.
fn gone(pid: i32, start: u64) -> bool {
    read_proc(pid).is_none_or(|now| now.start != start)
}

impl Proc {
    /// The value after `key` in its arguments.
    fn value(&self, key: &str) -> Option<&[u8]> {
        self.argv
            .windows(2)
            .find(|w| w[0] == key.as_bytes())
            .map(|w| w[1].as_slice())
    }

    fn marked(&self, root: &[u8]) -> bool {
        self.value(crate::guard::MARK) == Some(root)
    }

    fn is_guard(&self) -> bool {
        self.argv.get(1).map(Vec::as_slice) == Some(crate::guard::SUBCOMMAND.as_bytes())
    }

    fn session(&self) -> String {
        self.value(crate::guard::MARK_SESSION)
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .unwrap_or_default()
    }
}

/// This user's processes carrying `root`'s marker (and, with `session`,
/// that session's), this process excluded.
fn marked(root: &[u8], session: Option<&str>) -> Vec<Proc> {
    let uid = nix::unistd::getuid().as_raw();
    let me = std::process::id() as i32;
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    dir.filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
        .filter(|p| *p != me)
        .filter_map(read_proc)
        .filter(|p| p.uid == uid && p.marked(root))
        .filter(|p| session.is_none_or(|s| p.session() == s))
        .collect()
}

/// What one sweep killed or told, and what to watch until it is gone.
struct Acted {
    pid: i32,
    session: String,
    what: Left,
    /// The process and the children killed with it, by pid and start.
    watch: Vec<(i32, u64)>,
}

/// Stop `p`, check it is still the one found, SIGKILL its children (its
/// init first: T8A-2), then it. `None` when it is no longer that process.
fn kill_orphan(p: &Proc, root: &[u8]) -> Option<Acted> {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    let pid = Pid::from_raw(p.pid);
    kill(pid, Signal::SIGSTOP).ok()?;
    match read_proc(p.pid) {
        Some(now) if now.uid == p.uid && now.start == p.start && now.marked(root) => {}
        _ => {
            let _ = kill(pid, Signal::SIGCONT);
            return None;
        }
    }
    let mut watch = vec![(p.pid, p.start)];
    let children =
        std::fs::read_to_string(format!("/proc/{0}/task/{0}/children", p.pid)).unwrap_or_default();
    for c in children
        .split_whitespace()
        .filter_map(|c| c.parse::<i32>().ok())
    {
        if let Some(cp) = read_proc(c) {
            watch.push((c, cp.start));
        }
        let _ = kill(Pid::from_raw(c), Signal::SIGKILL);
    }
    let _ = kill(pid, Signal::SIGKILL);
    Some(Acted {
        pid: p.pid,
        session: p.session(),
        what: Left::Namespace,
        watch,
    })
}

/// The orphans among `procs`: not a guard, and whose parent does not carry
/// the marker (a live guard, bubblewrap above its init, the init above
/// `sandbox-exec`).
fn orphans<'a>(procs: &'a [Proc], root: &'a [u8]) -> impl Iterator<Item = &'a Proc> {
    procs
        .iter()
        .filter(|p| !p.is_guard())
        .filter(move |p| !read_proc(p.ppid).is_some_and(|parent| parent.marked(root)))
}

/// Wait up to `within` for everything in `acted` to be gone (none of it is this
/// process's child: its reaper reaps it), and say which is.
fn settle(acted: Vec<Acted>, within: std::time::Duration) -> Vec<(Acted, bool)> {
    let done = |a: &Acted| a.watch.iter().all(|(p, s)| gone(*p, *s));
    let by = std::time::Instant::now() + within;
    while std::time::Instant::now() < by && !acted.iter().all(done) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    acted
        .into_iter()
        .map(|a| {
            let d = done(&a);
            (a, d)
        })
        .collect()
}

/// At the node's start, before it serves (T.8e's second line): end every
/// namespace a dead node left behind, found by the marker naming this
/// node's temp root `root` (inside its data directory) among this user's
/// own processes in `/proc`, and tell every guard of this data directory
/// whose node is dead (its parent is not the pid of its own `--node`) to
/// kill (SIGTERM). A process without the marker, with another node's, or
/// held by a live guard is never touched; a process that carries this
/// node's marker of its own accord is killed with them (only this user's
/// processes can, and the marker names this node). Each orphan is stopped
/// and checked again (same start time) before the kill, its init
/// (bubblewrap's child) is killed first, then it. What is not gone 2 s
/// later is reported not ended; it was sent SIGKILL and ends when the
/// kernel releases it. Nothing is kept for a later start (re-check R-2).
pub fn sweep(root: &Path) -> Vec<Swept> {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    let rb = root.as_os_str().as_encoded_bytes().to_vec();
    let mut acted: Vec<Acted> = Vec::new();
    let procs = marked(&rb, None);
    for p in orphans(&procs, &rb) {
        if acted.iter().any(|a| a.pid == p.pid) {
            continue;
        }
        if let Some(a) = kill_orphan(p, &rb) {
            acted.push(a);
        }
    }
    for g in procs.iter().filter(|p| p.is_guard()) {
        let node = g
            .value(crate::guard::NODE)
            .and_then(|v| std::str::from_utf8(v).ok()?.parse::<i32>().ok());
        let Some(node) = node else {
            acted.push(Acted {
                pid: g.pid,
                session: g.session(),
                what: Left::GuardUnknown,
                watch: Vec::new(),
            });
            continue;
        };
        // Its node is alive (the parent is still that pid): not ours.
        if g.ppid == node {
            continue;
        }
        if kill(Pid::from_raw(g.pid), Signal::SIGTERM).is_ok() {
            acted.push(Acted {
                pid: g.pid,
                session: g.session(),
                what: Left::Guard,
                watch: vec![(g.pid, g.start)],
            });
        }
    }
    let settled = settle(acted, std::time::Duration::from_secs(2));
    settled
        .into_iter()
        .map(|(a, ended)| Swept {
            pid: a.pid,
            session: a.session,
            what: a.what,
            ended,
        })
        .collect()
}

/// While the node runs (review T8E-2): end what a guard of `mark`'s session
/// left when it died before it could (a signal in its first millisecond),
/// or did not answer. Only orphans: a namespace under a live guard (another
/// command of the same session) is never touched, and no guard is
/// signalled. Blocks up to `within` when what it killed does not end.
pub fn sweep_session(mark: &Mark, within: std::time::Duration) -> Vec<Swept> {
    let rb = mark.root.as_os_str().as_encoded_bytes().to_vec();
    let session = mark.session.to_string_lossy().into_owned();
    let procs = marked(&rb, Some(&session));
    let acted: Vec<Acted> = orphans(&procs, &rb)
        .filter_map(|p| kill_orphan(p, &rb))
        .collect();
    settle(acted, within)
        .into_iter()
        .map(|(a, ended)| Swept {
            pid: a.pid,
            session: a.session,
            what: a.what,
            ended,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live processes whose command line is exactly `sleep <secs>`.
    fn sleeps(secs: u32) -> Vec<i32> {
        let want = format!("sleep\0{secs}\0");
        std::fs::read_dir("/proc")
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
            .filter(|pid| {
                std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|c| c == want.as_bytes())
                    && std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
                        !s.rsplit(')')
                            .next()
                            .unwrap_or("")
                            .trim_start()
                            .starts_with('Z')
                    })
            })
            .collect()
    }

    /// N-4: without the init's number (a kernel without the `children`
    /// file), the cancel's SIGTERM and the kill go to bubblewrap; the
    /// namespace still ends, detached child included, only without the
    /// grace. Skips without bubblewrap unless `HARNESS_REQUIRE_BWRAP=1`.
    #[test]
    fn without_the_children_file_the_stop_still_ends_the_namespace() {
        let Ok(bwrap) = find_bwrap() else {
            assert!(
                std::env::var_os("HARNESS_REQUIRE_BWRAP").is_none(),
                "no bubblewrap"
            );
            eprintln!("SKIPPED: no bubblewrap");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        for (base, term) in [(974_301, true), (974_303, false)] {
            let c = Contain::new(bwrap.clone(), Form::Full);
            let mut cmd = c.bwrap_command(Path::new("sh"), dir.path());
            cmd.args(["-c", &format!("setsid sleep {base} & sleep {}", base + 1)]);
            std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
            let mut child = cmd.spawn().unwrap();
            let by = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while sleeps(base).is_empty() || sleeps(base + 1).is_empty() {
                assert!(std::time::Instant::now() < by, "never started");
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let pid = child.id() as i32;
            if term {
                term_namespace_with(pid, |_| None);
            } else {
                kill_namespace_with(pid, |_| None);
            }
            let _ = child.wait();
            let by = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while !(sleeps(base).is_empty() && sleeps(base + 1).is_empty()) {
                assert!(
                    std::time::Instant::now() < by,
                    "left after the stop (term {term})"
                );
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
    }

    /// N-1: a `bwrap` the node's user could replace is skipped, and the
    /// next one on `PATH` is taken; one owned by root is.
    #[test]
    fn a_bwrap_this_user_can_replace_is_not_taken() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().canonicalize().unwrap().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let planted = bin.join("bwrap");
        std::fs::write(&planted, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&planted, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(trusted(&planted).is_err());
        let only = find_bwrap_in(bin.as_os_str()).unwrap_err();
        assert!(
            only.contains("skipped") && only.contains("[sandbox] bwrap"),
            "{only}"
        );
        if Path::new("/usr/bin/bwrap").is_file() {
            let mut path = std::ffi::OsString::from(&bin);
            path.push(":/usr/bin");
            let found = find_bwrap_in(&path).unwrap();
            assert_eq!(found, Path::new("/usr/bin/bwrap").canonicalize().unwrap());
        }
    }

    #[test]
    fn settings_parse_kebab_case() {
        let m: ContainMode = serde_json::from_str("\"required\"").unwrap();
        assert_eq!(m, ContainMode::Required);
        assert_eq!(ContainMode::default(), ContainMode::Auto);
    }

    #[test]
    fn the_shared_proc_form_mounts_no_proc() {
        let full = Contain::new("/usr/bin/bwrap".into(), Form::Full);
        let a = full.args(Path::new("/w"), false);
        assert!(a.windows(2).any(|w| w[0] == "--proc" && w[1] == "/proc"));
        assert!(a.iter().any(|x| x == "--die-with-parent"));
        assert!(a.iter().any(|x| x == "--new-session"));
        assert_eq!(a.last().unwrap(), "--");
        let shared = Contain::new("/usr/bin/bwrap".into(), Form::SharedProc);
        assert!(
            !shared
                .args(Path::new("/w"), false)
                .iter()
                .any(|x| x == "--proc")
        );
    }

    /// T.8b: the network namespace exactly when the node's network is off,
    /// in both forms, before the `--` that ends bubblewrap's options.
    #[test]
    fn the_network_off_adds_a_network_namespace() {
        for form in [Form::Full, Form::SharedProc] {
            let c = Contain::new("/usr/bin/bwrap".into(), form);
            assert!(
                !c.args(Path::new("/w"), false)
                    .iter()
                    .any(|x| x == "--unshare-net")
            );
            let off = c.args(Path::new("/w"), true);
            let at = off.iter().position(|x| x == "--unshare-net").unwrap();
            assert!(at < off.len() - 1 && off.last().unwrap() == "--");
        }
        assert!(probe_script(true, true).contains("/proc/net/dev"));
        assert!(!probe_script(true, false).contains("/proc/net/dev"));
    }
}
