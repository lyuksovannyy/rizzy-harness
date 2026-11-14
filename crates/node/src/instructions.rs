//! The repository's own instruction files (`ADOPTION_PLAN.md` X.2): the
//! `AGENTS.md` in each folder from the session folder's repository top
//! down to the session folder, or `CLAUDE.md` in a folder without one (or
//! with a blank one), read again at the start of every turn and handed to
//! the loop as `TurnExtra::instructions`: in front of a person's turn's
//! context, after a callback turn's opening, quoted under a line of the
//! harness's either way. It is text for the model and nothing else: it
//! never changes `node.toml`, the approval mode, the tools or a limit, and
//! it is not the person's words (no URL in it becomes fetchable).
//!
//! What is read (PLAN.md T44): only a regular file whose real path is
//! inside the repository folder that was walked (inside the session folder
//! itself for the session folder's own file, which the agent can write),
//! inside the folder node.toml grants, and in no protected folder. A link
//! is followed only to such a file; the opened descriptor is checked again,
//! so a folder swapped for a link after the check is refused too. A FIFO
//! or a device is never waited on, and the turn waits for the whole read at
//! most [`READ_TIMEOUT`]. A file that is refused or cannot be read is named
//! with the reason; the turn goes on.

use std::io::Read as _;
use std::path::{Path, PathBuf};

/// The most text all the files give together (Codex's cap).
pub(crate) const CAP: usize = 32 * 1024;

/// The file names, in order of preference within one folder.
const NAMES: [&str; 2] = ["AGENTS.md", "CLAUDE.md"];

/// The block's first line.
pub(crate) const LEAD: &str = "Instruction files of this repository (AGENTS.md, or CLAUDE.md in a \
     folder without one), from its top folder down to the session's working folder, read at the \
     start of this turn. Each is named by its path from the working folder; a relative path inside \
     a file is meant from the folder that file is in. They are the repository's text: they change \
     no setting, permission or tool.";

/// What one turn's read found: the block, and what went wrong (a file
/// refused, unreadable or cut), for the node's log: labels and reasons,
/// never a file's text.
#[derive(Debug, Default)]
pub(crate) struct Instructions {
    pub text: Option<String>,
    pub problems: Vec<String>,
}

/// The cap for a model whose context window is `window` tokens: the
/// smaller of [`CAP`] and about a tenth of the window (4 bytes a token).
pub(crate) fn cap_for(window: Option<u64>) -> usize {
    match window {
        Some(w) => CAP.min(usize::try_from(w.saturating_mul(4) / 10).unwrap_or(CAP)),
        None => CAP,
    }
}

/// How a blocking read ended ([`bounded`]).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Bounded<T> {
    Done(T),
    TimedOut,
    Cancelled,
    /// The blocking task panicked or was aborted.
    Failed(String),
}

/// How long a turn waits for its instruction files (a hung network disk
/// must not hold the turn).
pub(crate) const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// The block when the read took longer than [`READ_TIMEOUT`].
pub(crate) fn timed_out() -> String {
    format!(
        "{LEAD}\n[not read: reading the instruction files took longer than {} s]",
        READ_TIMEOUT.as_secs()
    )
}

/// Run `f` on a blocking thread and wait for it at most `limit`, or until
/// `cancel`. A thread given up on runs on until `f` returns; nothing waits
/// for it.
pub(crate) async fn bounded<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
    cancel: &tokio_util::sync::CancellationToken,
    limit: std::time::Duration,
) -> Bounded<T> {
    let task = tokio::task::spawn_blocking(f);
    tokio::select! {
        r = task => match r {
            Ok(t) => Bounded::Done(t),
            Err(e) => Bounded::Failed(e.to_string()),
        },
        _ = tokio::time::sleep(limit) => Bounded::TimedOut,
        _ = cancel.cancelled() => Bounded::Cancelled,
    }
}

/// [`read`] with the full cap, its text only.
#[cfg(test)]
pub(crate) fn block(cwd: &Path, granted: &Path, protected: &[PathBuf]) -> Option<String> {
    read(cwd, granted, protected, CAP).text
}

/// The block for a session working in `cwd` (canonical), inside `granted`
/// (canonical: the workspace root that holds it, or a sub-agent's own
/// folder), with `protected` folders (canonical) never read, at most `cap`
/// bytes of the files' text ([`cap_for`]). `text` is `None` when there is
/// no instruction file at all.
pub(crate) fn read(cwd: &Path, granted: &Path, protected: &[PathBuf], cap: usize) -> Instructions {
    let mut found = Instructions::default();
    if !cwd.starts_with(granted) {
        return found;
    }
    // The repository's top: the nearest folder holding a `.git` entry (a
    // directory, or a gitfile of a worktree or submodule), never above the
    // granted folder; the session folder itself when there is none.
    let top = cwd
        .ancestors()
        .take_while(|d| d.starts_with(granted))
        .find(|d| std::fs::symlink_metadata(d.join(".git")).is_ok())
        .unwrap_or(cwd);
    let mut dirs: Vec<&Path> = cwd.ancestors().take_while(|d| d.starts_with(top)).collect();
    dirs.reverse();
    let depth = cwd.components().count();
    let mut out = String::from(LEAD);
    let mut any = false;
    let mut left = cap;
    let mut cut: Option<(usize, String)> = None;
    let mut dropped: Vec<String> = Vec::new();
    for d in dirs {
        // The session folder's own file is one the agent may have written:
        // it may lead only to a file inside that folder.
        let bound = if d == cwd { cwd } else { top };
        let Some((name, result)) = pick(d, bound, protected, cap) else {
            continue;
        };
        any = true;
        // From the working folder: what the model's own `read` opens.
        let label = format!("{}{name}", "../".repeat(depth - d.components().count()));
        if cut.is_some() {
            dropped.push(label);
            continue;
        }
        match result {
            Err(why) => {
                out.push_str(&format!("\n==> {label} <== (not read: {why})"));
                found.problems.push(format!("{label}: {why}"));
            }
            Ok(bytes) => {
                let mut text = String::from_utf8_lossy(&bytes).into_owned();
                if text.len() > left {
                    let mut n = left;
                    while !text.is_char_boundary(n) {
                        n -= 1;
                    }
                    text.truncate(n);
                    cut = Some((n, label.clone()));
                }
                left -= text.len();
                out.push_str(&format!("\n==> {label} <==\n"));
                out.push_str(text.trim_end_matches(['\n', '\r']));
            }
        }
    }
    if let Some((n, label)) = cut {
        let limit = if cap < CAP {
            format!("{cap} bytes together (about a tenth of the model's context window)")
        } else {
            format!("{} KiB together", CAP / 1024)
        };
        let mut note = format!(
            "[cut: the instruction files are longer than {limit}; the text above stops {n} bytes \
             into {label}"
        );
        match dropped.len() {
            0 => {}
            1 => note.push_str(&format!(", and {} was not read", dropped[0])),
            _ => note.push_str(&format!(", and {} were not read", dropped.join(", "))),
        }
        note.push(']');
        out.push('\n');
        out.push_str(&note);
        found.problems.push(note);
    }
    if any {
        found.text = Some(out);
    }
    found
}

/// The file `d` gives: its `AGENTS.md`, else its `CLAUDE.md`, each read
/// ([`read_one`]); one that is empty or only whitespace counts as absent.
/// A refused `AGENTS.md` is still the folder's file (its `CLAUDE.md` does
/// not stand in for it).
fn pick(
    d: &Path,
    bound: &Path,
    protected: &[PathBuf],
    cap: usize,
) -> Option<(&'static str, Result<Vec<u8>, String>)> {
    for name in NAMES {
        let path = d.join(name);
        if std::fs::symlink_metadata(&path).is_err() {
            continue;
        }
        let result = read_one(&path, bound, protected, cap);
        if matches!(&result, Ok(b) if b.iter().all(u8::is_ascii_whitespace)) {
            continue;
        }
        return Some((name, result));
    }
    None
}

/// Why a file is not read.
const OUTSIDE: &str = "it leads outside the folder";

/// At most `cap + 1` bytes of the regular file `path` leads to, when its
/// real path is inside `bound` and in no `protected` folder (one that holds
/// `bound` itself does not count: a sub-agent's own folder). The real path
/// is opened without following a link at its end or waiting on a FIFO, and
/// the descriptor is checked again, so a folder swapped for a link after
/// the first check is refused.
pub(crate) fn read_one(
    path: &Path,
    bound: &Path,
    protected: &[PathBuf],
    cap: usize,
) -> Result<Vec<u8>, String> {
    use nix::fcntl::OFlag;
    use std::os::unix::fs::OpenOptionsExt;
    let inside = |p: &Path| {
        p.starts_with(bound)
            && !protected
                .iter()
                .any(|x| p.starts_with(x) && !bound.starts_with(x))
    };
    let unreadable = |e: std::io::Error| format!("it could not be read: {e}");
    let real = path.canonicalize().map_err(unreadable)?;
    if !inside(&real) {
        return Err(OUTSIDE.into());
    }
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags((OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC).bits())
        .open(&real)
        .map_err(|e| match e.raw_os_error() {
            Some(code) if code == nix::libc::ELOOP => OUTSIDE.to_string(),
            _ => unreadable(e),
        })?;
    if !f.metadata().map_err(unreadable)?.is_file() {
        return Err("it is not a regular file".into());
    }
    if !inside(&crate::artifacts::real_path(&f, &real).map_err(unreadable)?) {
        return Err(OUTSIDE.into());
    }
    let mut out = Vec::new();
    f.take(cap as u64 + 1)
        .read_to_end(&mut out)
        .map_err(unreadable)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tree {
        _d: tempfile::TempDir,
        /// The granted folder (a workspace root).
        ws: PathBuf,
    }

    fn tree() -> Tree {
        let d = tempfile::tempdir().unwrap();
        let ws = d.path().canonicalize().unwrap().join("ws");
        std::fs::create_dir(&ws).unwrap();
        Tree { _d: d, ws }
    }

    fn put(path: &Path, text: impl AsRef<[u8]>) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// A folder with a `.git` directory (nothing in it is read).
    fn repo(dir: &Path) {
        std::fs::create_dir_all(dir.join(".git")).unwrap();
    }

    fn read_in(cwd: &Path, granted: &Path) -> Option<String> {
        block(cwd, granted, &[])
    }

    /// The root's file and the session folder's are both read, root
    /// first, each under its path from the session's working folder (what
    /// the model's own `read` opens); a file above the repository, one in a
    /// folder below the session's and a folder on the way with no file add
    /// nothing.
    #[test]
    fn reads_from_the_repository_top_down_to_the_session_folder() {
        let t = tree();
        let top = t.ws.join("repo");
        repo(&top);
        put(&t.ws.join("AGENTS.md"), "above the repository");
        put(&top.join("AGENTS.md"), "root rules\nline two\n");
        put(&top.join("crates/foo/AGENTS.md"), "foo rules");
        put(
            &top.join("crates/foo/deeper/AGENTS.md"),
            "below the session",
        );
        let cwd = top.join("crates/foo");
        assert_eq!(
            read_in(&cwd, &t.ws).unwrap(),
            format!(
                "{LEAD}\n==> ../../AGENTS.md <==\nroot rules\nline two\n\
                 ==> AGENTS.md <==\nfoo rules"
            )
        );
        assert!(
            LEAD.contains("relative path inside a file is meant from the folder that file is in"),
            "{LEAD}"
        );
        // Nothing anywhere: no block.
        let bare = t.ws.join("bare");
        repo(&bare);
        std::fs::create_dir_all(bare.join("a/b")).unwrap();
        assert_eq!(read_in(&bare.join("a/b"), &t.ws), None);
    }

    /// Outside any repository only the session folder's own file is read.
    #[test]
    fn outside_a_repository_only_the_folder_itself_is_read() {
        let t = tree();
        put(&t.ws.join("AGENTS.md"), "parent");
        put(&t.ws.join("work/AGENTS.md"), "here");
        assert_eq!(
            read_in(&t.ws.join("work"), &t.ws).unwrap(),
            format!("{LEAD}\n==> AGENTS.md <==\nhere")
        );
        // A repository above the granted folder is not walked to.
        let t = tree();
        repo(&t.ws);
        put(&t.ws.join("AGENTS.md"), "repository top, not granted");
        put(&t.ws.join("w/AGENTS.md"), "granted");
        let granted = t.ws.join("w");
        assert_eq!(
            read_in(&granted, &granted).unwrap(),
            format!("{LEAD}\n==> AGENTS.md <==\ngranted")
        );
    }

    /// A `.git` file (a linked worktree, a submodule) marks the top too.
    #[test]
    fn a_git_file_marks_the_top() {
        let t = tree();
        repo(&t.ws);
        put(&t.ws.join("AGENTS.md"), "the outer repository");
        let wt = t.ws.join("wt");
        put(&wt.join(".git"), "gitdir: /elsewhere/.git/worktrees/wt\n");
        put(&wt.join("AGENTS.md"), "the worktree's");
        put(&wt.join("x/AGENTS.md"), "x's");
        assert_eq!(
            read_in(&wt.join("x"), &t.ws).unwrap(),
            format!("{LEAD}\n==> ../AGENTS.md <==\nthe worktree's\n==> AGENTS.md <==\nx's")
        );
    }

    /// `CLAUDE.md` stands in only where a folder has no `AGENTS.md`.
    #[test]
    fn claude_md_is_read_only_where_there_is_no_agents_md() {
        let t = tree();
        repo(&t.ws);
        put(&t.ws.join("AGENTS.md"), "agents at the top");
        put(&t.ws.join("CLAUDE.md"), "claude at the top");
        put(&t.ws.join("sub/CLAUDE.md"), "claude below");
        assert_eq!(
            read_in(&t.ws.join("sub"), &t.ws).unwrap(),
            format!(
                "{LEAD}\n==> ../AGENTS.md <==\nagents at the top\n\
                 ==> CLAUDE.md <==\nclaude below"
            )
        );
    }

    /// An `AGENTS.md` that is empty or only whitespace counts as absent:
    /// the folder's `CLAUDE.md` is read, and a folder with nothing else
    /// adds nothing. Bytes that are not UTF-8 reach the model as U+FFFD.
    #[test]
    fn a_blank_agents_md_is_absent_and_other_bytes_are_kept() {
        let t = tree();
        repo(&t.ws);
        put(&t.ws.join("AGENTS.md"), " \n\t\r\n");
        put(&t.ws.join("CLAUDE.md"), "claude is used");
        put(&t.ws.join("e/AGENTS.md"), "");
        assert_eq!(
            read_in(&t.ws.join("e"), &t.ws).unwrap(),
            format!("{LEAD}\n==> ../CLAUDE.md <==\nclaude is used")
        );
        put(&t.ws.join("u/AGENTS.md"), b"ok \xff\xfe end\n");
        assert_eq!(
            read_in(&t.ws.join("u"), &t.ws).unwrap(),
            format!(
                "{LEAD}\n==> ../CLAUDE.md <==\nclaude is used\n\
                 ==> AGENTS.md <==\nok \u{FFFD}\u{FFFD} end"
            )
        );
        // Only blank files: no block.
        let b = tree();
        repo(&b.ws);
        put(&b.ws.join("AGENTS.md"), "\n\n");
        assert_eq!(read_in(&b.ws, &b.ws), None);
    }

    /// Over 32 KiB together the text is cut, and a note says where and
    /// which files were not read at all; the log is told.
    #[test]
    fn the_text_is_capped_with_a_note() {
        let t = tree();
        repo(&t.ws);
        let line = "x".repeat(99) + "\n";
        put(&t.ws.join("AGENTS.md"), line.repeat(200)); // 20,000 bytes
        put(&t.ws.join("a/AGENTS.md"), line.repeat(200));
        put(&t.ws.join("a/b/AGENTS.md"), "never reached");
        let r = read(&t.ws.join("a/b"), &t.ws, &[], CAP);
        let got = r.text.unwrap();
        let into = CAP - 20_000;
        assert!(
            got.ends_with(&format!(
                "\n[cut: the instruction files are longer than 32 KiB together; the text above \
                 stops {into} bytes into ../AGENTS.md, and AGENTS.md was not read]"
            )),
            "{}",
            &got[got.len() - 300..]
        );
        assert!(!got.contains("never reached"));
        let text: usize = got
            .lines()
            .filter(|l| l.starts_with('x'))
            .map(|l| l.len())
            .sum();
        assert!(text <= CAP, "{text}");
        assert!(got.len() < CAP + 1024, "{}", got.len());
        assert_eq!(r.problems.len(), 1, "{:?}", r.problems);
        assert!(r.problems[0].contains("cut"), "{:?}", r.problems);
    }

    /// With a small context window the cap is about a tenth of it, and
    /// the note says why.
    #[test]
    fn a_small_window_lowers_the_cap() {
        assert_eq!(cap_for(None), CAP);
        assert_eq!(cap_for(Some(1_000_000)), CAP);
        assert_eq!(cap_for(Some(8192)), 3276);
        let t = tree();
        put(&t.ws.join("AGENTS.md"), "y".repeat(5000));
        let got = read(&t.ws, &t.ws, &[], cap_for(Some(8192))).text.unwrap();
        assert!(
            got.ends_with(
                "\n[cut: the instruction files are longer than 3276 bytes together (about a \
                 tenth of the model's context window); the text above stops 3276 bytes into \
                 AGENTS.md]"
            ),
            "{}",
            &got[got.len() - 300..]
        );
    }

    /// A link is followed only to a regular file inside the repository
    /// (inside the session folder for the session folder's own file); a
    /// link out of it, a FIFO and a protected file are named, not read,
    /// and nothing waits; the log is told of each.
    #[test]
    fn links_out_of_the_folder_are_not_followed() {
        let d = tempfile::tempdir().unwrap();
        let base = d.path().canonicalize().unwrap();
        let ws = base.join("ws");
        let top = ws.join("repo");
        repo(&top);
        put(&base.join("secret.txt"), "outside the granted folder");
        put(&ws.join("other.txt"), "outside the repository");
        put(&top.join("docs/rules.md"), "kept in docs");
        put(&top.join("up.txt"), "above the session folder");
        // The top's file: a link to a file in the repository is read.
        std::os::unix::fs::symlink("docs/rules.md", top.join("AGENTS.md")).unwrap();
        // A folder on the way: a link out of the repository is not.
        std::fs::create_dir_all(top.join("a/b/c")).unwrap();
        std::os::unix::fs::symlink(ws.join("other.txt"), top.join("a/AGENTS.md")).unwrap();
        // Its CLAUDE.md does not stand in for the refused AGENTS.md.
        put(&top.join("a/CLAUDE.md"), "not a stand-in");
        // A FIFO is never waited on.
        nix::unistd::mkfifo(&top.join("a/b/AGENTS.md"), nix::sys::stat::Mode::S_IRWXU).unwrap();
        // The session folder's own file: not even to the repository above.
        std::os::unix::fs::symlink("../../../up.txt", top.join("a/b/c/AGENTS.md")).unwrap();
        let r = read(&top.join("a/b/c"), &ws, &[], CAP);
        assert_eq!(
            r.text.unwrap(),
            format!(
                "{LEAD}\n==> ../../../AGENTS.md <==\nkept in docs\n\
                 ==> ../../AGENTS.md <== (not read: it leads outside the folder)\n\
                 ==> ../AGENTS.md <== (not read: it is not a regular file)\n\
                 ==> AGENTS.md <== (not read: it leads outside the folder)"
            )
        );
        assert_eq!(r.problems.len(), 3, "{:?}", r.problems);
        // Out of the granted folder altogether.
        let w = ws.join("w");
        std::fs::create_dir(&w).unwrap();
        std::os::unix::fs::symlink(base.join("secret.txt"), w.join("AGENTS.md")).unwrap();
        assert_eq!(
            read_in(&w, &ws).unwrap(),
            format!("{LEAD}\n==> AGENTS.md <== (not read: it leads outside the folder)")
        );
        // Into a protected folder inside the repository (the node's data
        // directory under a repository at `$HOME`).
        let data = top.join("data");
        put(&data.join("key"), "secret key");
        std::fs::create_dir_all(top.join("p")).unwrap();
        std::fs::remove_file(top.join("AGENTS.md")).unwrap();
        std::os::unix::fs::symlink("data/key", top.join("CLAUDE.md")).unwrap();
        let got = block(&top.join("p"), &ws, std::slice::from_ref(&data)).unwrap();
        assert_eq!(
            got,
            format!("{LEAD}\n==> ../CLAUDE.md <== (not read: it leads outside the folder)")
        );
        // Without the protection the same link is read (the control).
        assert!(read_in(&top.join("p"), &ws).unwrap().contains("secret key"));
    }

    /// A writing sub-agent's own folder lies inside the node's protected
    /// sessions folder: its files are read there (its folder is its bound),
    /// and nothing outside it.
    #[test]
    fn a_sub_agents_own_folder_is_read_inside_the_protected_folder() {
        let d = tempfile::tempdir().unwrap();
        let data = d.path().canonicalize().unwrap().join("data");
        let files = data.join("sessions/child/files");
        repo(&files);
        put(&data.join("sessions/child/AGENTS.md"), "beside, not inside");
        put(&files.join("AGENTS.md"), "child rules");
        put(&files.join("sub/AGENTS.md"), "sub rules");
        for protected in [
            vec![data.clone()],
            vec![files.parent().unwrap().to_path_buf()],
        ] {
            assert_eq!(
                block(&files.join("sub"), &files, &protected).unwrap(),
                format!("{LEAD}\n==> ../AGENTS.md <==\nchild rules\n==> AGENTS.md <==\nsub rules")
            );
        }
    }

    /// The read is waited for at most its limit, and not past a cancel.
    #[tokio::test]
    async fn a_slow_read_times_out_and_a_cancel_stops_the_wait() {
        let cancel = tokio_util::sync::CancellationToken::new();
        let slow = || std::thread::sleep(std::time::Duration::from_millis(962));
        let at = std::time::Instant::now();
        assert_eq!(
            bounded(slow, &cancel, std::time::Duration::from_millis(50)).await,
            Bounded::TimedOut
        );
        assert!(at.elapsed() < std::time::Duration::from_millis(900));
        let gone = tokio_util::sync::CancellationToken::new();
        gone.cancel();
        assert_eq!(
            bounded(slow, &gone, std::time::Duration::from_secs(60)).await,
            Bounded::Cancelled
        );
        assert_eq!(
            bounded(|| 7, &cancel, std::time::Duration::from_secs(60)).await,
            Bounded::Done(7)
        );
    }
}
