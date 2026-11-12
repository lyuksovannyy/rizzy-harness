//! Shared by the sandbox tests: the shell's containment (bubblewrap, T.8a)
//! where it works. Where it does not, a test that needs it skips with a
//! printed reason, but with `HARNESS_REQUIRE_BWRAP=1` (CI) it fails: a
//! green run there must mean the containment tests ran.

use std::path::Path;

use harness_tools::contain::{Contain, Form};
use harness_tools::sandbox::Sandbox;

/// Whether a missing bubblewrap must fail the test run.
pub fn required() -> bool {
    std::env::var_os("HARNESS_REQUIRE_BWRAP").is_some_and(|v| v == "1")
}

/// `sb` with the full containment, as the node sets it up, or `None` (and
/// a printed reason) where bubblewrap cannot contain a command here.
pub fn contained(sb: Sandbox, scratch: &Path) -> Result<Sandbox, Sandbox> {
    let why = match harness_tools::contain::find_bwrap() {
        Ok(bwrap) => {
            let c = Contain::new(bwrap, Form::Full);
            match sb.probe_containment(&c, scratch) {
                Ok(()) => return Ok(sb.with_containment(c)),
                Err(e) => e,
            }
        }
        Err(e) => e,
    };
    assert!(
        !required(),
        "HARNESS_REQUIRE_BWRAP=1 but bubblewrap cannot contain a command here: {why}"
    );
    eprintln!("SKIPPED (containment): bubblewrap does not work here: {why}");
    Err(sb)
}
