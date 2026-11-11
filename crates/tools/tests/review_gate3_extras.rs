//! Gate 3 fix review 6 (docs/security-review/2026-11-gate3-fixes-6.md),
//! R4-1: the reviewer's proof, inverted. It passed on `9972b6d` (the swapped
//! entry's target was granted after a restart); it must fail to do so now.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_tools::ToolEnv;
use harness_tools::sandbox::{Confinement, Grants, Sandbox, Settings, check_extras_against_roots};

/// The wrapper's arguments for a command of a session in `ws`.
fn args(sb: &Arc<Sandbox>, ws: &Path) -> Vec<String> {
    let c = Confinement::Confined {
        sandbox: sb.clone(),
        tmp: None,
    }
    .command(
        OsStr::new("true"),
        &[],
        ws,
        None,
        &ToolEnv::default(),
        &[],
        Grants::default(),
    )
    .unwrap();
    c.as_std()
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

/// R4-1. A `[sandbox] write` entry is bound to where its path resolves
/// when the node starts. While the node runs, an entry replaced by a link
/// is left out (the fix of R1d-1). The link stays on disk, and at the next
/// start the entry would resolve through it: the start-up check now refuses
/// an entry below a workspace root whose path holds a link, naming the
/// link, so the swap becomes a refusal instead of a grant.
#[test]
fn review_r4_1_an_extra_swapped_before_a_restart_is_refused_at_the_restart() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path().canonicalize().unwrap();
    let code = home.join("code"); // a session's folder, the workspace root
    let cache = code.join(".toolcache"); // listed under [sandbox] write
    let ssh = home.join(".ssh");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::create_dir_all(&ssh).unwrap();
    let settings = Settings {
        write: vec![cache.clone()],
        ..Default::default()
    };
    let roots = [code.clone()];
    // What the node does at start: the checks, then the rules.
    let start = || -> Result<Arc<Sandbox>, String> {
        check_extras_against_roots(&settings, &roots)?;
        Ok(Arc::new(Sandbox::new(
            PathBuf::from("/bin/true"),
            &settings,
            None,
            &[],
        )?))
    };

    let running = start().unwrap();
    assert!(args(&running, &code).contains(&cache.display().to_string()));
    // What the session's shell may do in its own folder.
    std::fs::rename(&cache, code.join(".toolcache.r")).unwrap();
    std::os::unix::fs::symlink(&ssh, &cache).unwrap();
    // The running node leaves the entry out: R1d-1's fix holds.
    let now = args(&running, &code);
    assert!(!now.contains(&ssh.display().to_string()), "{now:?}");
    assert!(!now.contains(&cache.display().to_string()), "{now:?}");

    // The node restarts (an upgrade, a reboot): refused, naming the link.
    let e = start().expect_err("the link's target is granted after the restart");
    assert!(e.contains("[sandbox] write"), "{e}");
    assert!(e.contains(&cache.display().to_string()), "{e}");
    assert!(e.contains("link"), "{e}");
}

/// R4-1. An entry inside another `[sandbox] write` entry has a parent the
/// session's commands may change, so it could be swapped too; it is never
/// needed (the outer entry already grants it) and is refused at start,
/// naming both.
#[test]
fn an_extra_inside_a_write_extra_is_refused_at_start() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path().canonicalize().unwrap();
    let cargo = home.join(".cargo");
    std::fs::create_dir_all(cargo.join("bin")).unwrap();
    for (read, write) in [
        (vec![cargo.join("bin")], vec![cargo.clone()]),
        (vec![], vec![cargo.clone(), cargo.join("bin")]),
    ] {
        let s = Settings {
            read,
            write,
            ..Default::default()
        };
        let e = check_extras_against_roots(&s, &[]).unwrap_err();
        assert!(e.contains(&cargo.join("bin").display().to_string()), "{e}");
        assert!(
            e.contains(&format!("write entry {};", cargo.display())),
            "{e}"
        );
    }
    // Side by side is fine (the README's own list).
    let s = Settings {
        read: vec![cargo.join("bin")],
        write: vec![cargo.join("registry")],
        ..Default::default()
    };
    assert!(check_extras_against_roots(&s, &[]).is_ok());
}
