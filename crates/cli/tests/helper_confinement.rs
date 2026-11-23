//! The confinement of the `pdf-text`, `doc-text` and `doc-edit` helpers is proved, not
//! assumed (they share `confined_helper`): after it, the helper cannot open a
//! file that exists, cannot connect to a port that listens, holds a core
//! limit of 0 and is not dumpable. The helper answers this only when built
//! with the `helper-probe` feature (a dev-dependency of this crate) and asked
//! by environment; no shipped build contains the question.
//!
//! Where the helper cannot confine itself (no Landlock, or a kernel before
//! ABI 4) these tests skip with a printed reason, but fail when `CI` is set.

use std::process::{Command, Stdio};

use harness_master::pdf;

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

/// The helper's own report of what it can still do.
fn probe(sub: &str, port: u16) -> Option<Vec<String>> {
    let out = Command::new(EXE)
        .arg(sub)
        .env_clear()
        .env("HARNESS_HELPER_PROBE", port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .unwrap();
    if out.status.code() == Some(pdf::EXIT_UNCONFINED) {
        assert!(
            std::env::var_os("CI").is_none(),
            "{sub} cannot confine itself on this CI kernel"
        );
        eprintln!("skipped: {sub} cannot confine itself here (Landlock ABI 4 needed)");
        return None;
    }
    assert_eq!(out.status.code(), Some(0), "{sub}: {:?}", out.status);
    Some(
        String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect(),
    )
}

#[test]
fn a_confined_helper_opens_no_file_reaches_no_port_and_leaves_no_core() {
    // A port that really listens: only the confinement can refuse it.
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    assert!(std::path::Path::new("/etc/passwd").exists());
    for sub in [pdf::SUBCOMMAND, pdf::DOC_SUBCOMMAND, pdf::EDIT_SUBCOMMAND] {
        let Some(report) = probe(sub, port) else {
            return;
        };
        assert_eq!(
            report,
            ["open refused", "connect refused", "core 0 0", "dumpable 0"],
            "{sub}"
        );
    }
}
