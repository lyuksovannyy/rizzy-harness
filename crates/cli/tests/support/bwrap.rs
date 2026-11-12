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

/// `sb` with the full containment, as the node sets it up, or `sb` back (and
/// a printed reason) where bubblewrap cannot contain a command here.
/// (Both sides carry the sandbox back; a test helper, called once.)
#[allow(clippy::result_large_err)]
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

/// bubblewrap's part of a guard's command line (`argv`, from the program
/// on), made runnable without the guard: the file view's descriptor words,
/// which only the guard can resolve (T.8c), become binds by path, and the
/// image the harness binary `exe` by its path. For the tests that start
/// bubblewrap themselves (a namespace left behind, the guard's own cost);
/// the view itself is tested through the guard (`mount_view.rs`).
#[allow(dead_code)]
pub fn by_path(argv: &[std::ffi::OsString], exe: &str) -> Vec<std::ffi::OsString> {
    use harness_tools::contain::{FD_TOKEN, IMAGE_TOKEN};
    let mut out = argv.to_vec();
    let end = argv.iter().position(|a| a == "--").unwrap_or(argv.len());
    for i in 1..end {
        let Some(word) = argv[i].to_str() else {
            continue;
        };
        let opt = argv[i - 1].to_str().unwrap_or("");
        if word == IMAGE_TOKEN {
            out[i - 1] = "--ro-bind".into();
            out[i] = exe.into();
        } else if word.starts_with(FD_TOKEN) {
            out[i - 1] = if opt == "--bind-fd" {
                "--bind"
            } else {
                "--ro-bind"
            }
            .into();
            out[i] = argv[i + 1].clone();
        }
    }
    out
}
