//! The guard (ADOPTION_PLAN T.8e, PLAN.md T9): one short-lived process per
//! contained command, between the node and bubblewrap, that watches the
//! command until it is dead.
//!
//! `rizzy-harness sandbox-guard --node <pid> [--mark <dir> --mark-session
//! <key>] -- <bwrap> <args…>`: the node starts it (from `/proc/self/exe`,
//! its own running image) with stdin set to one end of a Unix socket pair
//! whose other end the node holds. The guard starts bubblewrap itself
//! (stdin `/dev/null`, its own process group), gives it the command's
//! stdout and stderr, keeps neither, and then waits on its children, the
//! channel, and its parent.
//!
//! **The kill order cannot be lost.** The channel closing is the fast
//! path: the kernel closes it when the node dies and nothing else holds a
//! copy; the node half-closes it to cancel and keeps reading the answer. A
//! [`TERM`] byte before the close asks for the graceful stop first: SIGTERM
//! to the command's session, [`TERM_GRACE`], then the kill. But a child of
//! the node between `fork` and `exec` holds a copy of the node's end (one
//! waiting in `chdir` into a folder that does not answer holds it for as
//! long: review T8E-1), so the guard also looks at its parent: before it
//! starts bubblewrap and at least every [`PARENT_LOOK`] after, a parent
//! other than `--node` (the kernel reparents the guard when the node
//! process dies, whoever holds descriptors) is the kill order. SIGTERM,
//! SIGINT or SIGHUP to the guard itself are kill orders too.
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

/// How often the kill is repeated while something is left: often until
/// [`CONFIRM_BOUND`], then seldom (a process stuck for hours costs two
/// wakeups a second).
const REPEAT: Duration = Duration::from_millis(20);
const REPEAT_STUCK: Duration = Duration::from_millis(500);

/// How often the guard looks at its parent while nothing else wakes it
/// (review T8E-1): the node's death is noticed within this, even when a
/// child of the node holds a copy of the channel.
pub const PARENT_LOOK: Duration = Duration::from_millis(500);

/// The node's pid, in the guard's arguments: `getppid()` other than this
/// is the kill order (review T8E-1).
pub const NODE: &str = "--node";

/// The node's link to its own running image (T.8f, [`crate::image`]), in
/// the guard's arguments where the node made one: the view binds it, and
/// only while it is this guard's own image.
pub const IMAGE: &str = "--image";

/// Who gave the kill order, when it was not the node through the channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum By {
    /// The node: the channel closed, or a stop byte.
    Node,
    /// SIGTERM, SIGINT or SIGHUP to the guard.
    Signal,
    /// The guard's parent is no longer the node (the node died while a
    /// child of it held the channel).
    Parent,
    /// bubblewrap died before its init: the guard ended what was left.
    Bwrap,
}

impl By {
    fn word(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Signal => "signal",
            Self::Parent => "parent",
            Self::Bwrap => "bwrap",
        }
    }
}

/// What the guard tells the node after a kill order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Report {
    /// Everything was reaped, this long after the order arrived.
    Ended(Duration, By),
    /// Something was still not reaped this long after the order.
    StillAlive(Duration),
}

impl Report {
    fn line(self) -> String {
        match self {
            Self::Ended(d, by) => format!("ended {} {}\n", d.as_millis(), by.word()),
            Self::StillAlive(d) => format!("alive {}\n", d.as_millis()),
        }
    }

    pub fn parse(line: &str) -> Option<Self> {
        let mut w = line.split_whitespace();
        let what = w.next()?;
        let d = Duration::from_millis(w.next()?.parse().ok()?);
        match what {
            "ended" => {
                let by = match w.next() {
                    Some("signal") => By::Signal,
                    Some("parent") => By::Parent,
                    Some("bwrap") => By::Bwrap,
                    _ => By::Node,
                };
                Some(Self::Ended(d, by))
            }
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
            "{SUBCOMMAND}: usage: {SUBCOMMAND} [--node PID] [--image LINK] [--mark DIR \
             --mark-session KEY] -- PROGRAM ARGS…"
        );
        return 125;
    };
    let Some(program) = args.get(at + 1) else {
        eprintln!("{SUBCOMMAND}: no program");
        return 125;
    };
    let node = args[..at]
        .windows(2)
        .find(|w| w[0] == NODE)
        .and_then(|w| w[1].to_str()?.parse::<i32>().ok());
    let image = args[..at]
        .windows(2)
        .find(|w| w[0] == IMAGE)
        .map(|w| std::path::PathBuf::from(&w[1]));
    match run(program, &args[at + 2..], node, image.as_deref()) {
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

fn run(
    program: &OsStr,
    args: &[OsString],
    node: Option<i32>,
    image: Option<&std::path::Path>,
) -> Result<i32, String> {
    // A node that died before this ran: start nothing.
    if let Some(n) = node
        && nix::unistd::getppid().as_raw() != n
    {
        return Err(format!("its node {n} is gone; nothing started"));
    }
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
    // Made before the spawn (it needs no blocked signal), so nothing after
    // the spawn can fail before the loop but the block itself (N-6).
    let mut mask = SigSet::empty();
    for s in [
        Signal::SIGCHLD,
        Signal::SIGTERM,
        Signal::SIGINT,
        Signal::SIGHUP,
    ] {
        mask.add(s);
    }
    let sfd = SignalFd::with_flags(&mask, SfdFlags::SFD_NONBLOCK | SfdFlags::SFD_CLOEXEC)
        .map_err(|e| format!("signalfd: {e}"))?;
    // The file view's descriptors (T.8c): opened here, inherited by
    // bubblewrap alone (this process has one thread and starts nothing
    // else), closed here once it has started.
    let (args, view_fds) = open_view(args, image)?;
    let child = std::process::Command::new(program)
        .args(&args)
        .stdin(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", program.to_string_lossy()))?;
    drop(view_fds);
    let bwrap = child.id() as i32;
    // Reaped below by `waitpid`, never through `Child`.
    drop(child);
    // From here a child's end and a stop signal wait in the `signalfd`. One
    // that came before is not lost: the loop's first `waitpid` sees a child
    // that already ended, and a SIGTERM before this ended the guard (its
    // default action: the window of review T8E-2, which the node closes by
    // sweeping when it sees its guard die of a signal). If the block fails,
    // the command is not left running unwatched: it is killed (N-6).
    let blocked = mask.thread_block().is_ok();
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
    Ok(watch(bwrap, ctl, &sfd, node, !blocked))
}

/// The file view's descriptors (T.8c, [`crate::contain`]): in bubblewrap's
/// options (up to its first `--`), each [`crate::contain::FD_TOKEN`]
/// `<dev>:<ino>` after `--bind-fd` or `--ro-bind-fd` becomes a descriptor
/// of the destination's path (the view binds every grant at its own path),
/// opened `O_PATH` without following a link at its last component, whose
/// device and inode must be the ones the node checked: a folder swapped
/// since refuses the command; one swapped after this open is caught by
/// bubblewrap, which compares what it mounted with the descriptor. The
/// [`crate::contain::IMAGE_TOKEN`] becomes a descriptor of this process's
/// own running image (the node's: it started this guard from it): opened
/// on `image`, the node's link to it (T.8f, [`crate::image`]), where the
/// node made one, and only while that link is this image (device and inode
/// against `/proc/self/exe`; else the command is refused, as for a grant);
/// without one, on `/proc/self/exe`, refused once the image has no path
/// left. Not
/// close-on-exec, so bubblewrap inherits them; bubblewrap closes those it
/// binds with `--bind-fd` before the command starts. Any other `@rizzy`
/// word, or one in another place, refuses the command.
fn open_view(
    args: &[OsString],
    image: Option<&std::path::Path>,
) -> Result<(Vec<OsString>, Vec<std::os::fd::OwnedFd>), String> {
    use crate::contain::{FD_TOKEN, IMAGE_TOKEN};
    use nix::fcntl::OFlag;
    use std::os::fd::AsRawFd;
    let mut out = args.to_vec();
    let mut fds = Vec::new();
    let end = args.iter().position(|a| a == "--").unwrap_or(args.len());
    for i in 0..end {
        let Some(word) = args[i].to_str().filter(|w| w.starts_with("@rizzy")) else {
            continue;
        };
        let opt = i.checked_sub(1).and_then(|j| args[j].to_str());
        let by_fd = matches!(opt, Some("--bind-fd" | "--ro-bind-fd"));
        let fd = if let (Some(link), true) = (
            image,
            word == IMAGE_TOKEN && (by_fd || opt == Some("--ro-bind")),
        ) {
            own_image_link(link)?
        } else if word == IMAGE_TOKEN && (by_fd || opt == Some("--ro-bind")) {
            let fd = nix::fcntl::open(
                "/proc/self/exe",
                OFlag::O_PATH,
                nix::sys::stat::Mode::empty(),
            )
            .map_err(|e| format!("its own image (/proc/self/exe): {e}"))?;
            // bubblewrap binds a descriptor through the path it names, which
            // a deleted file no longer has (an upgrade that replaced the
            // binary under a running node). Never the file now at the path:
            // that is what a session could have put there (T8E-5). This node
            // keeps no link to its image (T.8f): it could not make one.
            let at = std::fs::read_link(format!("/proc/self/fd/{}", fd.as_raw_fd()))
                .map_err(|e| format!("its own image: {e}"))?;
            if at.as_os_str().as_encoded_bytes().ends_with(b" (deleted)") {
                return Err(format!(
                    "the harness binary this node runs was deleted or replaced since the node \
                     started ({}); restart the node (it could not keep its image by a link in its \
                     data directory: the file view line of its log, or doctor, says why). The \
                     command was not started",
                    at.display()
                ));
            }
            fd
        } else if let (Some(id), true) = (word.strip_prefix(FD_TOKEN), by_fd) {
            let bad = || format!("a bad view argument {word:?}");
            let (dev, ino) = id.split_once(':').ok_or_else(bad)?;
            let (dev, ino): (u64, u64) = (
                dev.parse().map_err(|_| bad())?,
                ino.parse().map_err(|_| bad())?,
            );
            let dest = args
                .get(i + 1)
                .map(std::path::Path::new)
                .filter(|p| p.is_absolute())
                .ok_or_else(bad)?;
            let fd = nix::fcntl::open(
                dest,
                OFlag::O_PATH | OFlag::O_NOFOLLOW,
                nix::sys::stat::Mode::empty(),
            )
            .map_err(|e| {
                format!(
                    "the folder granted at {} was replaced or removed ({e}); the command was \
                     not started",
                    dest.display()
                )
            })?;
            let st = nix::sys::stat::fstat(&fd).map_err(|e| format!("{}: {e}", dest.display()))?;
            if st.st_dev as u64 != dev || st.st_ino as u64 != ino {
                return Err(format!(
                    "the folder granted at {} was replaced (it is not the one the node checked); \
                     the command was not started",
                    dest.display()
                ));
            }
            fd
        } else {
            return Err(format!("a bad view argument {word:?}"));
        };
        out[i] = if by_fd {
            fd.as_raw_fd().to_string().into()
        } else {
            format!("/proc/self/fd/{}", fd.as_raw_fd()).into()
        };
        fds.push(fd);
    }
    Ok((out, fds))
}

/// The node's link to its image (T.8f), opened without following a link
/// at its name, if it is this process's own running image (the node's).
/// The data directory it is in is in no grant; a difference here is a
/// second node started on the same data directory (task A.20) or the link
/// removed, and the command is refused rather than bound to another file.
fn own_image_link(link: &std::path::Path) -> Result<std::os::fd::OwnedFd, String> {
    use nix::fcntl::OFlag;
    use std::os::unix::fs::MetadataExt;
    let refuse = |why: String| {
        format!(
            "the node's link to its own image ({}) {why}; restart the node. The command was not \
             started",
            link.display()
        )
    };
    let own = std::fs::metadata("/proc/self/exe")
        .map_err(|e| format!("its own image (/proc/self/exe): {e}"))?;
    let fd = nix::fcntl::open(
        link,
        OFlag::O_PATH | OFlag::O_NOFOLLOW,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(|e| {
        refuse(format!(
            "cannot be opened ({e}): it was removed since the node started"
        ))
    })?;
    let st = nix::sys::stat::fstat(&fd).map_err(|e| refuse(e.to_string()))?;
    let regular = nix::sys::stat::SFlag::from_bits_truncate(st.st_mode)
        & nix::sys::stat::SFlag::S_IFMT
        == nix::sys::stat::SFlag::S_IFREG;
    if !regular || st.st_dev as u64 != own.dev() || st.st_ino as u64 != own.ino() {
        return Err(refuse(
            "is not the image this node runs: it was replaced since the node started (a second \
             node started on this data directory does that)"
                .into(),
        ));
    }
    Ok(fd)
}

/// SIGKILL what is left: the namespace through bubblewrap while it is
/// unreaped (so its number is still its own), then each of the guard's own
/// children (an init reparented here). Only pids that are this process's
/// unreaped children, or that bubblewrap names as its own, are signalled.
fn kill_all(bwrap: Option<i32>) {
    if let Some(b) = bwrap {
        crate::contain::kill_namespace(b);
    }
    for pid in own_children() {
        let _ = kill(Pid::from_raw(pid), Signal::SIGKILL);
    }
}

/// This process's children: from the `children` file, or, on a kernel
/// without it, every `/proc` entry whose parent is this process. Each is
/// unreaped, so its number is still its own.
fn own_children() -> Vec<i32> {
    let me = std::process::id() as i32;
    if let Ok(list) = std::fs::read_to_string(format!("/proc/{me}/task/{me}/children")) {
        return list
            .split_whitespace()
            .filter_map(|p| p.parse().ok())
            .collect();
    }
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    dir.filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
        .filter(|pid| {
            std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
                s.rsplit(')')
                    .next()
                    .and_then(|f| f.split_whitespace().nth(1))
                    .and_then(|p| p.parse::<i32>().ok())
                    == Some(me)
            })
        })
        .collect()
}

fn watch(
    bwrap: i32,
    mut ctl: UnixStream,
    sfd: &SignalFd,
    node: Option<i32>,
    kill_now: bool,
) -> i32 {
    let mut phase = Phase::Running;
    let mut ordered: Option<Instant> = None;
    let mut by = By::Node;
    if kill_now {
        phase = Phase::Killing(Instant::now(), false);
        ordered = Some(Instant::now());
        by = By::Signal;
    }
    let mut bwrap_unreaped = true;
    let mut bwrap_signaled = false;
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
                    bwrap_signaled = true;
                    status = 128 + sig as i32;
                }
                Ok(WaitStatus::StillAlive) => break,
                Ok(_) | Err(Errno::EINTR) => {}
                Err(_) => {
                    // ECHILD (or a kernel that cannot answer): done.
                    // After a kill order, say so (to a node that is gone,
                    // or stopped listening, this fails, harmlessly).
                    if let Some(at) = ordered {
                        let _ = ctl.write_all(Report::Ended(at.elapsed(), by).line().as_bytes());
                    }
                    return status;
                }
            }
        }
        let now = Instant::now();
        // A child left after bubblewrap: its init. bubblewrap exits with
        // the status its init sent it, so on an ordinary end the init may
        // still be exiting for a moment; after a bubblewrap killed by a
        // signal, it is an init no one else will end. Either way it is
        // killed and waited for; only the second is an order to report.
        if !bwrap_unreaped && !matches!(phase, Phase::Killing(..)) {
            phase = Phase::Killing(now, false);
            if ordered.is_none() && bwrap_signaled {
                ordered = Some(now);
                by = By::Bwrap;
            }
        }
        // The node is gone though the channel is open (a child of the node
        // holds a copy of it: review T8E-1): the kernel reparented this
        // process, whoever holds descriptors.
        if let (Some(n), Phase::Running) = (node, phase)
            && nix::unistd::getppid().as_raw() != n
        {
            phase = Phase::Killing(now, false);
            ordered = Some(now);
            by = By::Parent;
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
            Phase::Running if node.is_some() => {
                PollTimeout::try_from(PARENT_LOOK).unwrap_or(PollTimeout::MAX)
            }
            Phase::Running => PollTimeout::NONE,
            Phase::Grace(at) => {
                let left = TERM_GRACE.saturating_sub(now.duration_since(at));
                PollTimeout::try_from(left.max(Duration::from_millis(1)))
                    .unwrap_or(PollTimeout::MAX)
            }
            Phase::Killing(_, reported) => {
                PollTimeout::try_from(if reported { REPEAT_STUCK } else { REPEAT })
                    .unwrap_or(PollTimeout::MAX)
            }
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
                    if ordered.is_none() {
                        ordered = Some(Instant::now());
                        by = By::Signal;
                    }
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
            Report::Ended(Duration::from_millis(12), By::Node),
            Report::Ended(Duration::from_millis(3), By::Signal),
            Report::Ended(Duration::from_millis(3), By::Parent),
            Report::Ended(Duration::from_millis(3), By::Bwrap),
            Report::StillAlive(Duration::from_millis(2001)),
        ] {
            assert_eq!(Report::parse(&r.line()), Some(r));
        }
        assert_eq!(Report::parse("ended"), None);
        assert_eq!(Report::parse("gone 3"), None);
    }
}
