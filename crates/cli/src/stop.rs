//! Stop signals for `serve` and `node run` (ADOPTION_PLAN A.19,
//! `docs/node-update.md` §7.1): SIGTERM and SIGINT alike start a stop. The
//! process refuses new work, cancels its running turns through the normal
//! cancel path, waits for them to close (at most [`STOP_WAIT`]) and exits 0,
//! or 1 when a turn is still open. A second signal during that wait exits
//! at once with 1. Then `main` gives the runtime [`RUNTIME_SHUTDOWN`] and
//! exits whatever threads are left (a tool the agent loop abandoned, stuck
//! in a read: review A19-1), holding the data directory's lock until the
//! process is gone.
//!
//! Exiting at once is safe: every contained command's guard ends its
//! command's whole process tree when its node process is gone (its channel
//! closes, its parent changes: T.8e), SQLite keeps every committed append
//! across a process exit, the data directory's lock goes only with the
//! process (A.20), and the next start closes the turns left open as
//! `interrupted` and sweeps what a guard could not end. Two limits: a
//! command in a read the kernel cannot interrupt keeps its guard until the
//! read returns, and an uncontained command (`[sandbox] contain = "off"`)
//! can outlive the node, as it can outlive a SIGKILL today (C.11).

use std::time::Duration;

use tokio::signal::unix::{Signal, SignalKind, signal};

/// What a stop allows beyond its parts below: the turn's end and its
/// checkpoint's events written, a garbage collection's hold, the kept copy
/// of the changed files (A.17).
const STOP_MARGIN: Duration = Duration::from_secs(15);

/// The longest a stop waits for its running turns to close once they are
/// cancelled, derived from what a cancelled turn still does (review
/// A19-3): its tool is abandoned at the latest at the agent loop's
/// `TOOL_CANCEL_GRACE` (a command gets SIGTERM, then its guard's kill 2 s
/// later), then the after-turn checkpoint, the turn's undo point, may take
/// its whole `SNAPSHOT_TIMEOUT`, then [`STOP_MARGIN`]. 80 s today; a
/// constant until N.9's `[drain]`.
pub const STOP_WAIT: Duration = harness_agent_native::TOOL_CANCEL_GRACE
    .saturating_add(harness_node::SNAPSHOT_TIMEOUT)
    .saturating_add(STOP_MARGIN);

const _: () = assert!(
    STOP_WAIT.as_secs()
        > harness_agent_native::TOOL_CANCEL_GRACE.as_secs()
            + harness_node::SNAPSHOT_TIMEOUT.as_secs(),
    "a stop must outwait a cancelled turn's grace and its checkpoint"
);

/// The stop's wait: [`STOP_WAIT`], or in a debug build
/// `HARNESS_STOP_WAIT_S` (the tests' seam to reach the bound in seconds).
pub fn stop_wait() -> Duration {
    #[cfg(debug_assertions)]
    if let Ok(v) = std::env::var("HARNESS_STOP_WAIT_S") {
        // Up to a day; anything else (unparsable, or so large a deadline
        // overflows: review R-10) is refused and the real wait used.
        match v.parse::<u64>() {
            Ok(s) if s <= 86_400 => return Duration::from_secs(s),
            _ => tracing::warn!(
                "HARNESS_STOP_WAIT_S={v:?} is not 0 to 86400 seconds; using {} s",
                STOP_WAIT.as_secs()
            ),
        }
    }
    STOP_WAIT
}

/// How long `main` lets the runtime's tasks and blocking threads end after
/// the work is done; the process then exits whatever is left.
pub const RUNTIME_SHUTDOWN: Duration = Duration::from_secs(2);

/// The exit status of a node its master refused (a revoked or unknown key,
/// a spent token, another protocol version): `EX_NOPERM` of sysexits(3),
/// so a service manager can leave it down (`RestartPreventExitStatus=77`).
/// `docs/node-update.md` reserves 75 and 76 for the update.
pub const EXIT_REFUSED: i32 = 77;

/// How long the last events of a stop may take to reach the master (what
/// does not replays at the next hello).
pub const FLUSH_WAIT: Duration = Duration::from_secs(2);

/// SIGTERM and SIGINT, caught from [`Signals::install`] on.
pub struct Signals {
    term: Signal,
    int: Signal,
}

impl Signals {
    /// Catch both signals from now on (their default action, ending the
    /// process, no longer applies).
    pub fn install() -> std::io::Result<Self> {
        Ok(Self {
            term: signal(SignalKind::terminate())?,
            int: signal(SignalKind::interrupt())?,
        })
    }

    /// The first stop signal's name.
    pub async fn first(&mut self) -> &'static str {
        tokio::select! {
            _ = self.term.recv() => "SIGTERM",
            _ = self.int.recv() => "SIGINT",
        }
    }

    /// From now on, the next SIGTERM or SIGINT ends the process at once
    /// with exit status 1.
    pub fn force_on_second(mut self) {
        tokio::spawn(async move {
            let name = self.first().await;
            tracing::warn!(
                "{name} again: exiting now without waiting; each command's guard ends its \
                 command, and turns still open are closed at the next start"
            );
            std::process::exit(1);
        });
    }
}

/// The log line a stop starts with.
pub fn announce(name: &str, what: &str) {
    tracing::info!(
        "{name}: stopping {what}: new work is refused, running turns are cancelled and \
         closed (at most {} s); a second signal exits at once",
        stop_wait().as_secs()
    );
}
