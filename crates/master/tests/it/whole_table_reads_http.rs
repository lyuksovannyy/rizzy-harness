//! FIX_PLAN 4.5 slice 4: no request path reads every chat.
//!
//! `Store::sessions()` reads every chat of every account (52 ms and the
//! store lock at 50,000 chats, `docs/store-readers.md` §2), and
//! `Store::sessions_of` every chat of one account, every workspace;
//! `Store::project_links(None)` every account's project links. A
//! request asks the store for what it shows instead: a page
//! (`session_pages`) or a scoped set (`session_sets`). This test reads the
//! server's source (the master, the node runtime it embeds, the binary)
//! the way the route walk does (comments and strings dropped, `#[cfg(test)]`
//! modules removed) and fails on any call of one of them, as a method or by
//! path (`Store::sessions(&s)`, `.call(Store::sessions)`), beyond the exact
//! number `ALLOWED` gives a file.
//!
//! What it cannot see: a call through another name (an alias, `use … as`;
//! a helper that calls `sessions()` for its caller is flagged where it
//! calls it, which is the point); the store crate itself (its `sessions()`
//! is the one being fenced; `backup.rs` calls it from the offline
//! `restore`); and other unscoped reads it does not list (`routine_marks`,
//! one statement per id, is one: review F4 of slice 4). Three one-line
//! misses (review R2-4): `m.mirror.call(<Store>::sessions)` (the token
//! before `::` is `>`); a lower-case alias (`use harness_store::Store as
//! st; st::sessions(s)`); and a whole-table read under a scoped name
//! (`s.sessions_owned_by(&every_user_id, None)`). It is a tripwire, not a
//! proof.

use std::path::{Path, PathBuf};

use crate::route_walk_http::{Tok, Toks, lex, rs_files, src_dir, without_test_modules};

/// The store methods that read every chat (of everyone, or of one account),
/// and every chat's project link (`project_links(None)`; one chat's link is
/// `session_project`, a set's `project_links_of`).
const WHOLE: &[&str] = &["sessions", "sessions_of", "project_links"];

/// Calls that stay: the file (relative to `crates/`), exactly how many
/// calls it may hold, and the reason a reviewer can check. A second call in
/// the same file fails the test as any new one does.
const ALLOWED: &[(&str, usize, &str)] = &[(
    "node/src/lib.rs",
    1,
    "start-up only: a node runtime reads the chats of its own node.db once, before it serves",
)];

/// `(line, method)` of every call of one of `WHOLE`: as a method
/// (`s.sessions()`) or by path (`Store::sessions(&s)`, or the method passed
/// as a function, `.call(Store::sessions)`: `X::sessions` where `X` is a
/// type, by its upper-case first letter).
fn calls(t: &Toks) -> Vec<(usize, &'static str)> {
    let p = |i: usize, c: char| matches!(t.get(i), Some((Tok::P(x), _)) if *x == c);
    let mut out = Vec::new();
    for i in 0..t.len() {
        let method = p(i, '.');
        // A path through a type (`Store::`, `Self::`): a module's own
        // function (`recall::sessions(…)`) or a module (`super::sessions::X`)
        // of the same name is not the store's method.
        let path = i > 1
            && p(i - 1, ':')
            && p(i, ':')
            && matches!(t.get(i - 2), Some((Tok::Id(x), _)) if x.starts_with(char::is_uppercase));
        if !method && !path {
            continue;
        }
        for m in WHOLE {
            // A path needs no parentheses: `.call(Store::sessions)` passes
            // the method itself.
            if matches!(t.get(i + 1), Some((Tok::Id(x), _)) if x == m) && (path || p(i + 2, '(')) {
                out.push((t[i].1, *m));
            }
        }
    }
    out
}

fn crates_dir() -> PathBuf {
    src_dir().join("../..").canonicalize().unwrap()
}

fn server_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for c in ["master/src", "node/src", "cli/src"] {
        rs_files(&crates_dir().join(c), &mut files);
    }
    files.sort();
    files
}

fn rel(p: &Path) -> String {
    p.strip_prefix(crates_dir()).unwrap().display().to_string()
}

#[test]
fn the_scan_sees_what_it_must_and_nothing_else() {
    let n = |src: &str| calls(&without_test_modules(lex(src)));
    assert_eq!(n("fn f(s: &Store) { s.sessions().unwrap(); }").len(), 1);
    assert_eq!(n("fn f(s: &Store) { s.sessions_of(Some(u)); }").len(), 1);
    assert_eq!(
        n("async fn f(m: &M) { m.mirror.call(|s| s.sessions()).await; }").len(),
        1
    );
    assert_eq!(
        n("fn f(s: &Store) { s\n  .sessions()\n  .unwrap(); }").len(),
        1
    );
    assert_eq!(n("fn f(s: &Store) { s.project_links(None); }").len(), 1);
    // By path.
    assert_eq!(
        n("fn f(s: &Store) { Store::sessions(s).unwrap(); }").len(),
        1
    );
    assert_eq!(
        n("fn f(s: &Store) { harness_store::Store::sessions_of(s, None); }").len(),
        1
    );
    assert_eq!(
        n("async fn f(m: &M) { m.mirror.call(Store::sessions).await; }").len(),
        1
    );
    // Scoped reads, a free function of the same name, comments, strings,
    // test modules.
    for src in [
        "fn f(s: &Store) { s.sessions_page(&q); s.sessions_on(n); s.sessions_owned_by(&u, None); }",
        "fn f(s: &Store) { s.project_links_of(o, &ids); s.session_project(sid); }",
        "fn f(base: &Path) { sessions_of(base); }",
        "// s.sessions()\nfn f() { let x = \"s.sessions()\"; }",
        "#[cfg(test)] mod tests { fn t(s: &Store) { s.sessions(); } }",
        "fn f(r: &Row) { let x = r.sessions; }",
        "use super::sessions::CreateBody;",
        "async fn f(m: &M) { crate::recall::sessions(m, n, s).await; }",
    ] {
        assert!(n(src).is_empty(), "{src}");
    }
}

#[test]
fn no_request_path_reads_every_chat() {
    let files = server_files();
    assert!(files.len() > 100, "{}", files.len());
    let mut problems = Vec::new();
    let mut used: Vec<(String, usize)> = Vec::new();
    for f in &files {
        let src = std::fs::read_to_string(f).unwrap();
        let found = calls(&without_test_modules(lex(&src)));
        let name = rel(f);
        if found.is_empty() {
            continue;
        }
        if let Some((_, max, _)) = ALLOWED.iter().find(|(a, _, _)| *a == name) {
            used.push((name.clone(), found.len()));
            if found.len() <= *max {
                continue;
            }
        }
        for (line, m) in found {
            problems.push(format!("{name}:{line}: .{m}()"));
        }
    }
    assert!(
        problems.is_empty(),
        "a whole-table chat read outside start-up (ask the store for the page or the set the \
         request shows: session_pages, session_sets; or add the file to ALLOWED with a count and \
         a reason):\n{}",
        problems.join("\n")
    );
    // An allow-list entry must match exactly: fewer calls than allowed is
    // dead weight that a new call could hide behind.
    for (name, max, why) in ALLOWED {
        assert!(!why.is_empty(), "{name} needs a reason");
        let n = used.iter().find(|(u, _)| u == name).map_or(0, |(_, n)| *n);
        assert_eq!(
            n, *max,
            "{name} has {n} such calls, ALLOWED says {max}: fix the count"
        );
    }
}
