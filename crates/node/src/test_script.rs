//! Tests only: an executable script written without this process ever
//! holding a write descriptor of it.
//!
//! A test that writes a script with `std::fs::write` and then has the code
//! under test start it can fail with "Text file busy": between the write's
//! `open` and `close`, another test's thread may fork a child, which
//! inherits the write descriptor until its own `exec`; the kernel refuses
//! to start a file that is open for writing anywhere. Seen in CI twice
//! (`projects::tests`, `git::child_tests`). The code under test starts the
//! script, so the start cannot be retried from the test; `copy_exe` in the
//! cli tests is the same answer for a copied program.

use std::path::Path;

/// Write `body` to `path` and make it executable, through a child `sh`.
pub(crate) fn script(path: &Path, body: &str) {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    let mut sh = Command::new("sh")
        .args(["-c", r#"cat > "$1" && chmod 755 "$1""#, "sh"])
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()
        .expect("sh");
    sh.stdin
        .take()
        .expect("stdin")
        .write_all(body.as_bytes())
        .expect("script body");
    assert!(sh.wait().expect("sh").success(), "{}", path.display());
}
