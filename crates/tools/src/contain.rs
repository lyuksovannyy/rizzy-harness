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
//! whose other end only the node holds ([`ContainedChild`]). The channel
//! closing, by the node to cancel or by the kernel when the node dies, is
//! the kill order; the guard kills and repeats until it has reaped
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

    /// `<exe> sandbox-guard [--mark …] -- bwrap <args> -- <exe>`: the
    /// guard, which starts bubblewrap in `cwd` (T.8e). `exe` is the harness
    /// binary, resolved once at start, as for `sandbox-exec`.
    pub(crate) fn command(
        &self,
        exe: &Path,
        cwd: &Path,
        mark: Option<&Mark>,
    ) -> std::process::Command {
        let mut c = std::process::Command::new(exe);
        c.arg(crate::guard::SUBCOMMAND);
        if let Some(m) = mark {
            m.push_args(&mut c);
        }
        c.arg("--").arg(&self.bwrap).args(self.args(cwd)).arg(exe);
        c
    }

    /// `bwrap <args> -- <program>`, without the guard (tests, and the
    /// measurement of what the guard costs).
    #[doc(hidden)]
    pub fn bwrap_command(&self, program: &Path, cwd: &Path) -> std::process::Command {
        let mut c = std::process::Command::new(&self.bwrap);
        c.args(self.args(cwd)).arg(program);
        c
    }

    /// The arguments up to and including `--`.
    fn args(&self, cwd: &Path) -> Vec<OsString> {
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
        use crate::guard::{CONFIRM_BOUND, Report, TERM, TERM_GRACE};
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
        let bound = TERM_GRACE + CONFIRM_BOUND + std::time::Duration::from_secs(1);
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
            Ok(Some(Report::Ended(d))) => {
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
                    Ok(Ok(s)) if s.signal().is_none() => Stopped::Ended(started.elapsed()),
                    Ok(Ok(s)) => {
                        Stopped::Unconfirmed(format!("its guard was killed without a report ({s})"))
                    }
                    _ => Stopped::Unconfirmed("its guard ended without a report".into()),
                }
            }
            Err(_) => Stopped::Unconfirmed(format!(
                "its guard did not answer within {} s",
                bound.as_secs()
            )),
        }
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

/// A namespace [`sweep`] found and killed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Swept {
    pub pid: i32,
    pub session: String,
    /// Gone within 2 s of the kill.
    pub ended: bool,
}

/// One `/proc` entry, as much as the sweep needs.
struct Proc {
    pid: i32,
    ppid: i32,
    uid: u32,
    argv: Vec<Vec<u8>>,
}

fn read_proc(pid: i32) -> Option<Proc> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let mut f = stat.rsplit(')').next()?.split_whitespace();
    let state = f.next()?;
    if matches!(state, "Z" | "X" | "x") {
        return None;
    }
    let ppid = f.next()?.parse().ok()?;
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
        argv,
    })
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
}

/// The marked processes of this user that nothing alive holds: carrying
/// `root`'s marker, not a guard, and whose parent does not carry it (a
/// live guard, bubblewrap above its init, the init above `sandbox-exec`).
/// What is left is bubblewrap whose guard died, or an init whose
/// bubblewrap died, both orphans.
fn orphans(root: &Path) -> Vec<Proc> {
    let root = root.as_os_str().as_encoded_bytes();
    let uid = nix::unistd::getuid().as_raw();
    let me = std::process::id() as i32;
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    dir.filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
        .filter(|p| *p != me)
        .filter_map(read_proc)
        .filter(|p| p.uid == uid && p.marked(root) && !p.is_guard())
        .filter(|p| !read_proc(p.ppid).is_some_and(|parent| parent.marked(root)))
        .collect()
}

/// At the node's start, before it serves (T.8e's second line): end every
/// namespace a dead node left behind, found by the marker naming this
/// node's temp root `root` (inside its data directory) among this user's
/// own processes in `/proc`. A process without the marker, with another
/// node's, or held by a live guard (one is its parent) is never touched; a
/// process that carries this node's marker of its own accord is killed
/// with them (only this user's processes can, and the marker names this
/// node). Each is stopped and checked again before the kill, so a number
/// reused in between is left alone; its init (bubblewrap's child) is
/// killed first, then it.
pub fn sweep(root: &Path) -> Vec<Swept> {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    let rb = root.as_os_str().as_encoded_bytes().to_vec();
    let mut killed = Vec::new();
    for p in orphans(root) {
        let pid = Pid::from_raw(p.pid);
        if kill(pid, Signal::SIGSTOP).is_err() {
            continue;
        }
        // Stopped, it cannot exit or exec; check it is still the one found.
        match read_proc(p.pid) {
            Some(now) if now.uid == p.uid && now.argv == p.argv && now.marked(&rb) => {}
            _ => {
                let _ = kill(pid, Signal::SIGCONT);
                continue;
            }
        }
        let children = std::fs::read_to_string(format!("/proc/{0}/task/{0}/children", p.pid))
            .unwrap_or_default();
        for c in children
            .split_whitespace()
            .filter_map(|c| c.parse::<i32>().ok())
        {
            let _ = kill(Pid::from_raw(c), Signal::SIGKILL);
        }
        let _ = kill(pid, Signal::SIGKILL);
        let session = p
            .value(crate::guard::MARK_SESSION)
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .unwrap_or_default();
        killed.push((p, session));
    }
    // Not this process's children: their reaper reaps them. Wait for that.
    let by = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let gone = |p: &Proc| read_proc(p.pid).is_none_or(|now| now.argv != p.argv);
    while std::time::Instant::now() < by && !killed.iter().all(|(p, _)| gone(p)) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    killed
        .into_iter()
        .map(|(p, session)| Swept {
            ended: gone(&p),
            pid: p.pid,
            session,
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
        let a = full.args(Path::new("/w"));
        assert!(a.windows(2).any(|w| w[0] == "--proc" && w[1] == "/proc"));
        assert!(a.iter().any(|x| x == "--die-with-parent"));
        assert!(a.iter().any(|x| x == "--new-session"));
        assert_eq!(a.last().unwrap(), "--");
        let shared = Contain::new("/usr/bin/bwrap".into(), Form::SharedProc);
        assert!(!shared.args(Path::new("/w")).iter().any(|x| x == "--proc"));
    }
}
