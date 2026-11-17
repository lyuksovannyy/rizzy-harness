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

/// Whether this machine's bubblewrap can bind the way `binds` asks: binds
/// by descriptor need 0.10.0 or newer (`harness_tools::contain::binds_of`).
/// A test that forces [`harness_tools::contain::Binds::Fd`] skips, with a
/// printed reason, only where `bwrap --version` names a version below
/// 0.10: the node never asks that bubblewrap for them (it binds by path,
/// which the same tests check with `Binds::Path`). A version that cannot
/// be read fails the test under `HARNESS_REQUIRE_BWRAP=1` instead of
/// skipping it; with 0.10 or newer this returns true, and the callers'
/// own checks fail a forced-descriptor case whose probe fails (the full
/// form under `HARNESS_REQUIRE_BWRAP=1`), so on CI the case runs or the
/// test is red.
#[allow(dead_code)]
pub fn can_bind(bwrap: &Path, binds: harness_tools::contain::Binds) -> bool {
    use harness_tools::contain::{Binds, binds_of};
    let (have, version) = binds_of(bwrap);
    if binds != Binds::Fd || have == Binds::Fd {
        return true;
    }
    // `bubblewrap 0.9.0`: the first word that starts with a digit.
    let parsed = version.as_deref().and_then(|v| {
        let num = v
            .split_whitespace()
            .find(|w| w.starts_with(|c: char| c.is_ascii_digit()))?;
        let mut it = num.split('.');
        let major: u32 = it.next()?.parse().ok()?;
        let minor: u32 = it
            .next()?
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .ok()?;
        Some((major, minor))
    });
    match parsed {
        Some(v) if v < (0, 10) => {
            eprintln!(
                "SKIPPED (binds by descriptor): {} is {version:?}; --bind-fd needs bubblewrap 0.10.0",
                bwrap.display()
            );
            false
        }
        _ => {
            assert!(
                !required(),
                "HARNESS_REQUIRE_BWRAP=1 but the version of {} cannot be read ({version:?}): \
                 the binds by descriptor would go untested",
                bwrap.display()
            );
            eprintln!(
                "SKIPPED (binds by descriptor): the version of {} cannot be read ({version:?})",
                bwrap.display()
            );
            false
        }
    }
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
/// program, the node's image through a descriptor (T.8f), the harness
/// binary `exe`, bound read-only at its own path for this. For the tests
/// that start bubblewrap themselves (a namespace left behind, the guard's
/// own cost); the view itself is tested through the guard (`mount_view.rs`).
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
        if word.starts_with(FD_TOKEN) {
            out[i - 1] = if opt == "--bind-fd" {
                "--bind"
            } else {
                "--ro-bind"
            }
            .into();
            out[i] = argv[i + 1].clone();
        }
    }
    if out.get(end + 1).is_some_and(|p| p == IMAGE_TOKEN) {
        out[end + 1] = exe.into();
        // Before the root is made read-only.
        let ro = out
            .windows(2)
            .position(|w| w[0] == "--remount-ro")
            .unwrap_or(end);
        out.splice(ro..ro, ["--ro-bind".into(), exe.into(), exe.into()]);
    }
    out
}
