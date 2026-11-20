//! What a result prints against what it may count as shown (PLAN.md T45).
//!
//! Every tool result goes through [`crate::ToolOutcome::ok`] or `err`, which
//! cut anything over [`crate::MAX_OUTPUT_CHARS`] to its first and last half:
//! the middle never reaches the model. A row may be recorded as shown
//! ([`crate::seen::SessionSeen::add_shown`]) only if it is in the text the
//! model receives, so:
//!
//! 1. a producer of rows prints them through a [`Budget`] and stops at the
//!    first row that does not fit (it says how many it left out, and how to
//!    get them), so the rows it printed are a prefix of what it had;
//! 2. recording takes a [`Sealed`], which only [`seal`] makes, from the
//!    finished text of the result, and only when the cap would not cut it.
//!    In a debug build (every test) text over the cap panics: a producer
//!    that forgot its budget fails the first test that prints enough rows,
//!    not silently at a user's.

use crate::MAX_OUTPUT_CHARS;

/// What one result may print, in characters: below the cap, which leaves room
/// for what the loop may add (a note after the result is a message of its
/// own, but the live-output line of `bash` is appended to the text).
pub(crate) const RESULT_BUDGET: usize = MAX_OUTPUT_CHARS - 1_000;

/// Characters still free in one result.
#[derive(Debug)]
pub(crate) struct Budget {
    left: usize,
}

impl Budget {
    pub(crate) fn new() -> Self {
        Self::of(RESULT_BUDGET)
    }

    pub(crate) fn of(left: usize) -> Self {
        Self { left }
    }

    pub(crate) fn left(&self) -> usize {
        self.left
    }

    /// Spend `s` if it fits, and say so; spend nothing if it does not.
    pub(crate) fn take(&mut self, s: &str) -> bool {
        let n = s.chars().count();
        if n > self.left {
            return false;
        }
        self.left -= n;
        true
    }

    /// Spend `s`, which is printed whatever the room (a lead, a header, a
    /// footer): the rows get what is left, which may be nothing.
    pub(crate) fn spend(&mut self, s: &str) {
        self.left = self.left.saturating_sub(s.chars().count());
    }

    pub(crate) fn spend_n(&mut self, n: usize) {
        self.left = self.left.saturating_sub(n);
    }
}

/// The text is not cut by [`crate::cap`]: the model receives all of it.
pub(crate) fn within(text: &str) -> bool {
    text.chars().count() <= MAX_OUTPUT_CHARS
}

/// Proof that the text of a result reaches the model whole; the only way to
/// record rows as shown.
#[derive(Debug)]
pub(crate) struct Sealed(());

/// Seal `text`, the finished output of a result. Text the cap would cut is
/// not sealed, so its rows are not recorded: the model is then shown them
/// again when it names them, which is slower and never wrong. In this
/// crate's own tests that case fails loudly, since it means a producer
/// printed past its budget; outside them it must not, because a server
/// built for development (`cargo run`) would crash a turn on a file with an
/// unusual shape instead of asking for one more read.
pub(crate) fn seal(text: &str) -> Option<Sealed> {
    let ok = within(text);
    #[cfg(test)]
    assert!(
        ok,
        "a result that records rows as shown is over the cap: {} characters",
        text.chars().count()
    );
    ok.then_some(Sealed(()))
}

impl Sealed {
    /// For rows the model wrote itself (`write`'s content): they are in its
    /// own message, not in a result.
    pub(crate) fn model_wrote() -> Self {
        Sealed(())
    }
}
