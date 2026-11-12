//! The guard (ADOPTION_PLAN T.8e, PLAN.md T9): one short-lived process per
//! contained command, between the node and bubblewrap, that watches the
//! command until it is dead.
//!
//! `rizzy-harness sandbox-guard [--mark <dir> --mark-session <key>] --
//! <bwrap> <args…>`: the node starts it with stdin set to one end of a Unix
//! socket pair whose other end only the node holds. The guard starts
//! bubblewrap itself (stdin `/dev/null`, its own process group), gives it
//! the command's stdout and stderr, keeps neither, and then waits on two
//! things: its children, and the channel.
//!
//! **The channel closing is the kill order that cannot be lost.** The
//! kernel closes it when the node dies in any way (`kill -9` included); the
//! node half-closes it to cancel and keeps reading the answer. A [`TERM`]
//! byte before the close asks for the graceful stop first: SIGTERM to the
//! command's session, [`TERM_GRACE`], then the kill. SIGTERM, SIGINT or
//! SIGHUP to the guard itself are kill orders too.
//!
//! **"Nothing is left" is proved, not assumed.** The guard is a child
//! subreaper (`PR_SET_CHILD_SUBREAPER`), so when bubblewrap dies before its
//! init, that init is reparented to the guard, not to the user's manager.
//! The guard kills (T.8a's [`crate::contain::kill_namespace`] while
//! bubblewrap is unreaped, then SIGKILL to each of its own children) and
//! repeats until `waitpid` says it has no child left (ECHILD). The init of
//! a PID namespace is reaped only after every process in the namespace is
//! gone (the kernel's `zap_pid_ns_processes`), so ECHILD means the whole
//! namespace has been released. Only then does it exit, after writing
//! `ended <ms>` on the channel; if that takes longer than
//! [`CONFIRM_BOUND`] after the kill (a process stuck in I/O the kernel will
//! not interrupt), it writes `alive <ms>` once and keeps going.
//!
//! **One thread.** bubblewrap's `--die-with-parent` (`PR_SET_PDEATHSIG`)
//! names the thread that forked it; the guard has only its main thread, so
//! bubblewrap dies exactly when the guard does: the second line for a guard
//! that was itself killed. Signals reach the loop through a `signalfd`,
//! blocked only after bubblewrap started with an empty mask (the mask is
//! inherited across `exec`; a test checks the command has none blocked).

use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::Stdio;
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use nix::sys::signal::{SigSet, Signal, kill};
use nix::sys::signalfd::{SfdFlags, SignalFd};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::Pid;

/// The guard's subcommand of the harness binary.
pub const SUBCOMMAND: &str = "sandbox-guard";

/// The marker in the guard's and in `sandbox-exec`'s arguments (so in
/// bubblewrap's and its init's command lines): the node's temp root inside
/// its data directory, and the session. A restarted node finds what a dead
/// one left by it ([`crate::contain::sweep`]).
pub const MARK: &str = "--mark";
pub const MARK_SESSION: &str = "--mark-session";

/// The byte that asks for the graceful stop before the close.
pub const TERM: u8 = b'T';

/// SIGTERM, then this long before the kill (as before T.8e).
pub const TERM_GRACE: Duration = Duration::from_secs(2);

/// How long after the kill the guard waits before it reports that
/// something is still alive. With [`TERM_GRACE`] it stays inside the agent
/// loop's 5 s grace for a cancelled tool (`TOOL_CANCEL_GRACE`), so the
/// report reaches the tool result.
pub const CONFIRM_BOUND: Duration = Duration::from_secs(2);

/// How often the kill is repeated while something is left.
const REPEAT: Duration = Duration::from_millis(20);

/// What the guard tells the node after a kill order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Report {
    /// Everything was reaped, this long after the order arrived.
    Ended(Duration),
    /// Something was still not reaped this long after the order.
    StillAlive(Duration),
}

impl Report {
    fn line(self) -> String {
        match self {
            Self::Ended(d) => format!("ended {}\n", d.as_millis()),
            Self::StillAlive(d) => format!("alive {}\n", d.as_millis()),
        }
    }

    pub fn parse(line: &str) -> Option<Self> {
        let (what, ms) = line.trim().split_once(' ')?;
        let d = Duration::from_millis(ms.parse().ok()?);
        match what {
            "ended" => Some(Self::Ended(d)),
            "alive" => Some(Self::StillAlive(d)),
            _ => None,
        }
    }
}

/// The guard's `main`: its exit code is bubblewrap's (bubblewrap's is the
/// command's), 128 + N when bubblewrap was killed by signal N, 125 when it
/// could not start.
pub fn main() -> i32 {
    let args: Vec<OsString> = std::env::args_os().skip(2).collect();
    let Some(at) = args.iter().position(|a| a == "--") else {
        eprintln!(
            "{SUBCOMMAND}: usage: {SUBCOMMAND} [--mark DIR --mark-session KEY] -- PROGRAM ARGS…"
        );
        return 125;
    };
    let Some(program) = args.get(at + 1) else {
        eprintln!("{SUBCOMMAND}: no program");
        return 125;
    };
    match run(program, &args[at + 2..]) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{SUBCOMMAND}: {e}");
            125
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Phase {
    Running,
    /// SIGTERM sent at this instant.
    Grace(Instant),
    /// Killing since this instant; whether "alive" was reported.
    Killing(Instant, bool),
}

fn run(program: &OsStr, args: &[OsString]) -> Result<i32, String> {
    let ctl: UnixStream = std::io::stdin()
        .as_fd()
        .try_clone_to_owned()
        .map(UnixStream::from)
        .map_err(|e| format!("the control channel: {e}"))?;
    // bubblewrap, and so the command, inherits the signal mask: it starts
    // empty, whatever the node's was (`std::process::Command` does not
    // clear it, a test found), and the guard blocks its own signals only
    // after the spawn.
    SigSet::empty()
        .thread_set_mask()
        .map_err(|e| format!("sigprocmask: {e}"))?;
    nix::sys::prctl::set_child_subreaper(true).map_err(|e| format!("subreaper: {e}"))?;
    let child = std::process::Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", program.to_string_lossy()))?;
    let bwrap = child.id() as i32;
    // Reaped below by `waitpid`, never through `Child`.
    drop(child);
    // From here a child's end and a stop signal wait in the `signalfd`. One
    // that came before is not lost: the loop's first `waitpid` sees a child
    // that already ended, and a SIGTERM before this ended the guard (its
    // default action; bubblewrap's death signal is the second line).
    let mut mask = SigSet::empty();
    for s in [
        Signal::SIGCHLD,
        Signal::SIGTERM,
        Signal::SIGINT,
        Signal::SIGHUP,
    ] {
        mask.add(s);
    }
    mask.thread_block()
        .map_err(|e| format!("sigprocmask: {e}"))?;
    let sfd = SignalFd::with_flags(&mask, SfdFlags::SFD_NONBLOCK | SfdFlags::SFD_CLOEXEC)
        .map_err(|e| format!("signalfd: {e}"))?;
    // The command's stdout and stderr are bubblewrap's now: the guard keeps
    // no copy (the node sees their end when the command's last holder
    // goes), and its stdin is the duplicate in `ctl`.
    if let Ok(null) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/null")
    {
        let _ = nix::unistd::dup2_stdin(&null);
        let _ = nix::unistd::dup2_stdout(&null);
        let _ = nix::unistd::dup2_stderr(&null);
    }
    Ok(watch(bwrap, ctl, &sfd))
}

/// SIGKILL what is left: the namespace through bubblewrap while it is
/// unreaped (so its number is still its own), then each of the guard's own
/// children (an init reparented here). Only pids that are this process's
/// unreaped children, or that bubblewrap names as its own, are signalled.
fn kill_all(bwrap: Option<i32>) {
    if let Some(b) = bwrap {
        crate::contain::kill_namespace(b);
    }
    let me = std::process::id();
    for pid in std::fs::read_to_string(format!("/proc/{me}/task/{me}/children"))
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|p| p.parse::<i32>().ok())
    {
        let _ = kill(Pid::from_raw(pid), Signal::SIGKILL);
    }
}

fn watch(bwrap: i32, mut ctl: UnixStream, sfd: &SignalFd) -> i32 {
    let mut phase = Phase::Running;
    let mut ordered: Option<Instant> = None;
    let mut bwrap_unreaped = true;
    let mut status = 125;
    let mut ctl_open = true;
    loop {
        // Reap everything that has ended; ECHILD is the proof that nothing
        // is left.
        loop {
            match waitpid(None, Some(WaitPidFlag::WNOHANG)) {
                Ok(WaitStatus::Exited(pid, code)) if pid.as_raw() == bwrap => {
                    bwrap_unreaped = false;
                    status = code;
                }
                Ok(WaitStatus::Signaled(pid, sig, _)) if pid.as_raw() == bwrap => {
                    bwrap_unreaped = false;
                    status = 128 + sig as i32;
                }
                Ok(WaitStatus::StillAlive) => break,
                Ok(_) | Err(Errno::EINTR) => {}
                Err(_) => {
                    // ECHILD (or a kernel that cannot answer): done.
                    // After a kill order, say so (to a node that is gone,
                    // or stopped listening, this fails, harmlessly).
                    if let Some(at) = ordered {
                        let _ = ctl.write_all(Report::Ended(at.elapsed()).line().as_bytes());
                    }
                    return status;
                }
            }
        }
        let now = Instant::now();
        // bubblewrap reaps its init before it exits; a child left after
        // bubblewrap is an init whose bubblewrap died first: no one else
        // will end it.
        if !bwrap_unreaped && !matches!(phase, Phase::Killing(..)) {
            phase = Phase::Killing(now, false);
            ordered.get_or_insert(now);
        }
        if let Phase::Grace(at) = phase
            && now.duration_since(at) >= TERM_GRACE
        {
            phase = Phase::Killing(now, false);
        }
        if let Phase::Killing(at, reported) = phase {
            kill_all(bwrap_unreaped.then_some(bwrap));
            if !reported && now.duration_since(at) >= CONFIRM_BOUND {
                let since = ordered.map_or(Duration::ZERO, |o| o.elapsed());
                let _ = ctl.write_all(Report::StillAlive(since).line().as_bytes());
                phase = Phase::Killing(at, true);
            }
        }
        let timeout = match phase {
            Phase::Running => PollTimeout::NONE,
            Phase::Grace(at) => {
                let left = TERM_GRACE.saturating_sub(now.duration_since(at));
                PollTimeout::try_from(left.max(Duration::from_millis(1)))
                    .unwrap_or(PollTimeout::MAX)
            }
            Phase::Killing(..) => PollTimeout::try_from(REPEAT).unwrap_or(PollTimeout::MAX),
        };
        let (sig_ready, ctl_ready) = {
            let mut fds = vec![PollFd::new(sfd.as_fd(), PollFlags::POLLIN)];
            if ctl_open {
                fds.push(PollFd::new(ctl.as_fd(), PollFlags::POLLIN));
            }
            match poll(&mut fds, timeout) {
                Ok(_) => {}
                Err(Errno::EINTR) => continue,
                // Should not happen; keep the loop alive and the kill going.
                Err(_) => std::thread::sleep(REPEAT),
            }
            let ready = |f: &PollFd| f.revents().is_some_and(|r| !r.is_empty());
            (ready(&fds[0]), fds.get(1).is_some_and(ready))
        };
        if ctl_ready {
            let mut buf = [0u8; 64];
            match ctl.read(&mut buf) {
                Ok(0) | Err(_) => {
                    // The kill order: the node closed its end or died.
                    ctl_open = false;
                    if matches!(phase, Phase::Running) {
                        phase = Phase::Killing(Instant::now(), false);
                        ordered.get_or_insert_with(Instant::now);
                    }
                }
                Ok(n) => {
                    if buf[..n].contains(&TERM) && matches!(phase, Phase::Running) {
                        ordered.get_or_insert_with(Instant::now);
                        if bwrap_unreaped {
                            crate::contain::term_namespace_with(
                                bwrap,
                                crate::contain::sandbox_init,
                            );
                        }
                        phase = Phase::Grace(Instant::now());
                    }
                }
            }
        }
        if sig_ready {
            while let Ok(Some(si)) = sfd.read_signal() {
                let s = si.ssi_signo as i32;
                if (s == Signal::SIGTERM as i32
                    || s == Signal::SIGINT as i32
                    || s == Signal::SIGHUP as i32)
                    && !matches!(phase, Phase::Killing(..))
                {
                    phase = Phase::Killing(Instant::now(), false);
                    ordered.get_or_insert_with(Instant::now);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_round_trip() {
        for r in [
            Report::Ended(Duration::from_millis(12)),
            Report::StillAlive(Duration::from_millis(2001)),
        ] {
            assert_eq!(Report::parse(&r.line()), Some(r));
        }
        assert_eq!(Report::parse("ended"), None);
        assert_eq!(Report::parse("gone 3"), None);
    }

    /// The bounds fit inside the agent loop's grace for a cancelled tool
    /// (5 s, `harness_agent_native::TOOL_CANCEL_GRACE`, checked end to end
    /// in `cli/tests/guard.rs`), with a second to spare.
    #[test]
    fn the_stop_fits_the_loops_grace() {
        assert!(TERM_GRACE + CONFIRM_BOUND + Duration::from_secs(1) <= Duration::from_secs(5));
    }
}
