//! Stop signals for `serve` and `node run` (ADOPTION_PLAN A.19,
//! `docs/node-update.md` §7.1): SIGTERM and SIGINT alike start a stop. The
//! process refuses new work, cancels its running turns through the normal
//! cancel path, waits for them to close (at most [`STOP_WAIT`]) and exits 0.
//! A second signal during that wait exits at once with 1.
//!
//! Exiting at once is safe: every contained command's guard ends its
//! command's whole process tree when its node process is gone (its channel
//! closes, its parent changes: T.8e), SQLite keeps every committed append
//! across a process exit, the data directory's lock is released by the
//! kernel (A.20), and the next start closes the turns left open as
//! `interrupted` and sweeps what a guard could not end. An uncontained
//! command (`[sandbox] contain = "off"`) can outlive it, as it can outlive
//! a SIGKILL today (C.11).

use std::time::Duration;

use tokio::signal::unix::{Signal, SignalKind, signal};

/// The longest a stop waits for its running turns to close once they are
/// cancelled: `hard_stop_seconds`' default in `docs/node-update.md` §3 (a
/// cancelled command gets SIGTERM, then its guard's kill 2 s later; a tool
/// still running 5 s after a cancel is abandoned; the after-turn checkpoint
/// took up to 15.5 s on a slow disk). A constant until N.9's `[drain]`.
pub const STOP_WAIT: Duration = Duration::from_secs(60);

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
        STOP_WAIT.as_secs()
    );
}
