//! The `sleep` durations tests start and then find (and kill) by command
//! line across the machine. A test names its own number (`Mark(975_601)`);
//! what it runs and what it looks for is that number behind this test
//! process's id (`sleep 1234975601` for pid 1234), so two runs of the suite
//! at once (two builders, a builder and a reviewer, CI shards) never find
//! or kill each other's commands. Tests in one process tell themselves
//! apart by their numbers, as before. A helper process a test starts
//! (`current_exe()` with `--exact`) is given the test's pid in
//! [`PID_ENV`] (see [`Mark::for_helper`]), so it runs what its parent looks for.

// Each test file uses what it needs of this.
#![allow(dead_code)]

use std::fmt;

/// Where a helper process finds the pid of the test that started it.
pub const PID_ENV: &str = "HARNESS_TEST_MARK_PID";

/// The pid the marks of this process carry: its parent test's, in a
/// helper, else its own.
fn pid() -> u64 {
    std::env::var(PID_ENV)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| u64::from(std::process::id()))
}

/// A test's own `sleep` number; shown as the duration it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark(pub u32);

impl Mark {
    /// The duration on the command line: unique to this process.
    pub fn secs(self) -> u64 {
        assert!(self.0 < 1_000_000, "a mark is below 1,000,000: {}", self.0);
        pid() * 1_000_000 + u64::from(self.0)
    }

    /// `sleep\0<secs>\0`: the command line of `sleep <secs>` in `/proc`.
    pub fn cmdline(self) -> Vec<u8> {
        format!("sleep\0{}\0", self.secs()).into_bytes()
    }

    /// The variable and value that make a helper process's marks this
    /// process's (`Command::env`).
    pub fn for_helper() -> (&'static str, String) {
        (PID_ENV, pid().to_string())
    }
}

impl fmt::Display for Mark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.secs())
    }
}
