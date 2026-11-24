//! FIX_PLAN 4.2, blocking work: a handler file does no file IO of its own.
//!
//! A handler runs on an async worker; a read, a write or a delete there
//! stalls every other request on that thread when the disk is slow (this
//! project's own disk is a USB disk). File work belongs behind the store
//! (`Store::call` is `spawn_blocking`), the blob store, `tokio::fs`, or a
//! helper that moves it onto the blocking pool (`secrets::set_off_thread`,
//! `ProviderRuntime::persist`).
//!
//! The test reads the source the way the route walk does (the lexer of
//! `route_walk_http`: comments dropped, strings kept apart, `#[cfg(test)]`
//! modules removed) and fails on `std::fs`, the `std::io` read and write
//! traits, `std::process`, `tempfile`, and the `Path` methods that ask the
//! disk (`exists`, `metadata`, `read_dir`, `canonicalize`, `is_file`,
//! `is_dir`, ...) in `http.rs` and every file under `http/`. Nothing is
//! flagged today. What it still cannot see: an alias (`use std as s;`), a
//! helper the handler calls (`secrets::get`, `providers::resolve_key`, the
//! sealed-key read, `Prefs`; FIX_PLAN 4.2 lists the ones that remain), a
//! method named like a disk one on another type (a hit there goes in the
//! allow-list with its reason), and any blocking that is not file or
//! process work (a `std::thread::sleep`, a CPU loop). `tokio::fs` is not
//! flagged: it is the fix.

use std::path::{Path, PathBuf};

use crate::route_walk_http::{Tok, Toks, lex, rs_files, src_dir, without_test_modules};

/// Files that may use `std::fs` or `std::io::Write` directly, each with the
/// reason. Empty today: every file operation a handler file had was moved.
/// An entry needs a reason a reviewer can check.
const ALLOWED: &[(&str, &str)] = &[];

/// `Path` / `PathBuf` methods that hit the disk. `Metadata::is_file` and
/// `is_dir` do not, and share their names: a hit there is looked at by
/// hand (the allow-list).
const DISK_METHODS: &[&str] = &[
    "exists",
    "try_exists",
    "metadata",
    "symlink_metadata",
    "read_dir",
    "canonicalize",
    "read_link",
    "is_file",
    "is_dir",
    "is_symlink",
];

/// The `std::io` traits that read or write a file or a pipe.
const IO_TRAITS: &[&str] = &["Write", "Read", "BufRead", "Seek"];

/// `(line, what)` for every direct use in these tokens.
fn uses(t: &Toks) -> Vec<(usize, String)> {
    let id = |i: usize, s: &str| matches!(t.get(i), Some((Tok::Id(x), _)) if x == s);
    let colons = |i: usize| {
        matches!(t.get(i), Some((Tok::P(':'), _))) && matches!(t.get(i + 1), Some((Tok::P(':'), _)))
    };
    let mut out = Vec::new();
    for i in 0..t.len() {
        let line = t[i].1;
        // A path: `std::fs`, `std::io::Write`.
        if id(i, "std") && colons(i + 1) && id(i + 3, "fs") {
            out.push((line, "std::fs".to_string()));
        }
        if id(i, "std")
            && colons(i + 1)
            && id(i + 3, "io")
            && colons(i + 4)
            && IO_TRAITS.iter().any(|x| id(i + 6, x))
        {
            out.push((line, "std::io::Read/Write".to_string()));
        }
        // A `Path` method that asks the disk.
        if matches!(t.get(i), Some((Tok::P('.'), _)))
            && DISK_METHODS.iter().any(|m| id(i + 1, m))
            && matches!(t.get(i + 2), Some((Tok::P('('), _)))
        {
            out.push((line, "a Path method that reads the disk".to_string()));
        }
        // A process: `std::process::Command` blocks in `output()`/`status()`.
        if id(i, "std") && colons(i + 1) && id(i + 3, "process") {
            out.push((line, "std::process".to_string()));
        }
        if id(i, "tempfile") && colons(i + 1) {
            out.push((line, "tempfile".to_string()));
        }
        // An import: `use std::fs;`, `use std::{fs, ..}`, `use std::io::{self, Write}`.
        if id(i, "use") && id(i + 1, "std") {
            let end = (i..t.len())
                .find(|k| matches!(t[*k].0, Tok::P(';')))
                .unwrap_or(t.len());
            for k in i + 2..end {
                if id(k, "fs") {
                    out.push((line, "use std::..fs".to_string()));
                }
                if IO_TRAITS.iter().any(|x| id(k, x)) || id(k, "process") {
                    out.push((line, "use std::..Write".to_string()));
                }
            }
        }
    }
    out.sort();
    out.dedup_by_key(|(line, _)| *line);
    out
}

fn handler_files() -> Vec<PathBuf> {
    let src = src_dir();
    let mut files = vec![src.join("http.rs")];
    rs_files(&src.join("http"), &mut files);
    files.sort();
    files
}

fn rel(p: &Path) -> String {
    p.strip_prefix(src_dir()).unwrap().display().to_string()
}

#[test]
fn the_scan_sees_what_it_must_and_nothing_else() {
    let n = |src: &str| uses(&without_test_modules(lex(src)));
    assert_eq!(n("fn f() { let _ = std::fs::remove_file(p); }").len(), 1);
    assert_eq!(n("use std::fs;").len(), 1);
    assert_eq!(n("use std::{fs, path::Path};").len(), 1);
    assert_eq!(n("use std::io::Write;").len(), 1);
    assert_eq!(n("use std::io::{self, Write};").len(), 1);
    assert_eq!(n("fn f(w: &mut dyn std::io::Write) {}").len(), 1);
    // Not file IO of the handler's own.
    assert!(n("fn f() { tokio::fs::remove_file(p).await; }").is_empty());
    assert!(n("use tokio::fs;").is_empty());
    assert!(n("fn f(e: std::io::Error) -> std::io::Result<()> { Ok(()) }").is_empty());
    // Paths that ask the disk, processes, temp files, the read traits.
    for src in [
        "fn f(p: &Path) -> bool { p.exists() }",
        "fn f(p: PathBuf) { let _ = p.metadata(); }",
        "fn f(p: &Path) { for _ in p.read_dir().unwrap() {} }",
        "fn f(p: &Path) { let _ = p.canonicalize(); }",
        "fn f(p: &Path) -> bool { p.is_file() || p.is_dir() }",
        "fn f() { std::process::Command::new(\"x\"); }",
        "use std::process::Command;",
        "fn f() { let d = tempfile::tempdir(); }",
        "use std::io::Read;",
        "use std::io::{BufRead, Seek};",
        "fn f(r: &mut dyn std::io::Read) {}",
    ] {
        assert_eq!(n(src).len(), 1, "{src}");
    }
    assert!(n("fn f(p: &Path) { let x = p.join(\"a\").display(); }").is_empty());
    // Permission work is file work too.
    assert_eq!(n("use std::os::unix::fs::PermissionsExt;").len(), 1);
    assert!(n("// std::fs::read\nfn f() { let s = \"std::fs::read\"; }").is_empty());
    assert!(n("#[cfg(test)] mod tests { fn t() { std::fs::write(p, b); } }").is_empty());
}

#[test]
fn handler_files_do_no_file_io_of_their_own() {
    let files = handler_files();
    assert!(files.len() > 50, "{}", files.len());
    let mut problems = Vec::new();
    for f in &files {
        let src = std::fs::read_to_string(f).unwrap();
        let found = uses(&without_test_modules(lex(&src)));
        let name = rel(f);
        if ALLOWED.iter().any(|(a, _)| *a == name) || found.is_empty() {
            continue;
        }
        for (line, what) in found {
            problems.push(format!("{name}:{line}: {what}"));
        }
    }
    assert!(
        problems.is_empty(),
        "blocking file work in a handler file (use the store, tokio::fs or a spawn_blocking helper; \
         or add the file to ALLOWED with a reason):\n{}",
        problems.join("\n")
    );
    // An allow-list entry that no longer matches anything is dead weight.
    for (name, why) in ALLOWED {
        assert!(!why.is_empty(), "{name} needs a reason");
        assert!(src_dir().join(name).exists(), "{name} is gone: drop it");
    }
}
