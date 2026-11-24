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
//! modules removed) and fails on `std::fs` and on `std::io::Write` in
//! `http.rs` and every file under `http/`. Its honest limit: it sees a
//! handler doing file IO itself, not a helper the handler calls (a sync
//! `secrets::get`, `providers::build`, the sealed-key read: see FIX_PLAN
//! 4.2); `tokio::fs` is not flagged, it is the fix.

use std::path::{Path, PathBuf};

use crate::route_walk_http::{Tok, Toks, lex, rs_files, src_dir, without_test_modules};

/// Files that may use `std::fs` or `std::io::Write` directly, each with the
/// reason. Empty today: every file operation a handler file had was moved.
/// An entry needs a reason a reviewer can check.
const ALLOWED: &[(&str, &str)] = &[];

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
        if id(i, "std") && colons(i + 1) && id(i + 3, "io") && colons(i + 4) && id(i + 6, "Write") {
            out.push((line, "std::io::Write".to_string()));
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
                if id(k, "Write") {
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
