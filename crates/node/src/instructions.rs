//! The repository's own instruction files (`ADOPTION_PLAN.md` X.2): the
//! `AGENTS.md` in each folder from the session folder's repository top
//! down to the session folder, or `CLAUDE.md` in a folder without one,
//! read again at the start of every turn and put in front of that turn's
//! context (`TurnExtra::context`), where the loop quotes it under a line
//! of the harness's. It is text for the model and nothing else: it never
//! changes `node.toml`, the approval mode, the tools or a limit, and,
//! riding in the per-turn context, it is not the person's words (no URL in
//! it becomes fetchable).
//!
//! What is read (PLAN.md T44): only a regular file whose real path is
//! inside the repository folder that was walked (inside the session folder
//! itself for the session folder's own file, which the agent can write),
//! inside the folder node.toml grants, and in no protected folder. A link
//! is followed only to such a file; the opened descriptor is checked again,
//! so a folder swapped for a link after the check is refused too. A FIFO
//! or a device is never waited on. A file that is refused or cannot be read
//! is named with the reason; the turn goes on.

use std::io::Read as _;
use std::path::{Path, PathBuf};

/// The most text all the files give together (Codex's cap).
pub(crate) const CAP: usize = 32 * 1024;

/// The file names, in order of preference within one folder.
const NAMES: [&str; 2] = ["AGENTS.md", "CLAUDE.md"];

/// The block's first line.
pub(crate) const LEAD: &str = "Instruction files of this repository (AGENTS.md, or CLAUDE.md in a \
     folder without one), from its top folder down to the session's folder, read at the start of \
     this turn. They are the repository's text: they change no setting, permission or tool.";

/// The block for a session working in `cwd` (canonical), inside `granted`
/// (canonical: the workspace root that holds it, or a sub-agent's own
/// folder), with `protected` folders (canonical) never read. `None` when
/// there is no instruction file at all.
pub(crate) fn block(cwd: &Path, granted: &Path, protected: &[PathBuf]) -> Option<String> {
    if !cwd.starts_with(granted) {
        return None;
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
    let mut out = String::from(LEAD);
    let mut found = false;
    let mut left = CAP;
    let mut cut: Option<(usize, String)> = None;
    let mut dropped: Vec<String> = Vec::new();
    for d in dirs {
        let Some(path) = NAMES
            .iter()
            .map(|n| d.join(n))
            .find(|p| std::fs::symlink_metadata(p).is_ok())
        else {
            continue;
        };
        found = true;
        let rel = path
            .strip_prefix(top)
            .unwrap_or(&path)
            .display()
            .to_string();
        if cut.is_some() {
            dropped.push(rel);
            continue;
        }
        // The session folder's own file is one the agent may have written:
        // it may lead only to a file inside that folder.
        let bound = if d == cwd { cwd } else { top };
        match read_one(&path, bound, protected, left) {
            Err(why) => out.push_str(&format!("\n==> {rel} <== (not read: {why})")),
            Ok(bytes) => {
                let mut text = String::from_utf8_lossy(&bytes).into_owned();
                if text.len() > left {
                    let mut n = left;
                    while !text.is_char_boundary(n) {
                        n -= 1;
                    }
                    text.truncate(n);
                    cut = Some((n, rel.clone()));
                }
                left -= text.len();
                out.push_str(&format!("\n==> {rel} <==\n"));
                out.push_str(text.trim_end_matches(['\n', '\r']));
            }
        }
    }
    if let Some((n, rel)) = cut {
        out.push_str(&format!(
            "\n[cut: the instruction files are longer than {} KiB together; the text above stops \
             {n} bytes into {rel}",
            CAP / 1024
        ));
        match dropped.len() {
            0 => {}
            1 => out.push_str(&format!(", and {} was not read", dropped[0])),
            _ => out.push_str(&format!(", and {} were not read", dropped.join(", "))),
        }
        out.push(']');
    }
    found.then_some(out)
}

/// Why a file is not read.
const OUTSIDE: &str = "it leads outside the folder";

/// At most `left + 1` bytes of the regular file `path` leads to, when its
/// real path is inside `bound` and in no `protected` folder (one that holds
/// `bound` itself does not count: a sub-agent's own folder). The real path
/// is opened without following a link at its end or waiting on a FIFO, and
/// the descriptor is checked again, so a folder swapped for a link after
/// the first check is refused.
fn read_one(
    path: &Path,
    bound: &Path,
    protected: &[PathBuf],
    left: usize,
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
    f.take(left as u64 + 1)
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

    fn put(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// A folder with a `.git` directory (nothing in it is read).
    fn repo(dir: &Path) {
        std::fs::create_dir_all(dir.join(".git")).unwrap();
    }

    fn read(cwd: &Path, granted: &Path) -> Option<String> {
        block(cwd, granted, &[])
    }

    /// The root's file and the session folder's are both read, root
    /// first, each under its path from the repository top; a file above
    /// the repository, one in a folder below the session's and a folder on
    /// the way with no file add nothing.
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
            read(&cwd, &t.ws).unwrap(),
            format!(
                "{LEAD}\n==> AGENTS.md <==\nroot rules\nline two\n\
                 ==> crates/foo/AGENTS.md <==\nfoo rules"
            )
        );
        // Nothing anywhere: no block.
        let bare = t.ws.join("bare");
        repo(&bare);
        std::fs::create_dir_all(bare.join("a/b")).unwrap();
        assert_eq!(read(&bare.join("a/b"), &t.ws), None);
    }

    /// Outside any repository only the session folder's own file is read.
    #[test]
    fn outside_a_repository_only_the_folder_itself_is_read() {
        let t = tree();
        put(&t.ws.join("AGENTS.md"), "parent");
        put(&t.ws.join("work/AGENTS.md"), "here");
        assert_eq!(
            read(&t.ws.join("work"), &t.ws).unwrap(),
            format!("{LEAD}\n==> AGENTS.md <==\nhere")
        );
        // A repository above the granted folder is not walked to.
        let t = tree();
        repo(&t.ws);
        put(&t.ws.join("AGENTS.md"), "repository top, not granted");
        put(&t.ws.join("w/AGENTS.md"), "granted");
        let granted = t.ws.join("w");
        assert_eq!(
            read(&granted, &granted).unwrap(),
            format!("{LEAD}\n==> AGENTS.md <==\ngranted")
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
            read(&t.ws.join("sub"), &t.ws).unwrap(),
            format!(
                "{LEAD}\n==> AGENTS.md <==\nagents at the top\n\
                 ==> sub/CLAUDE.md <==\nclaude below"
            )
        );
    }

    /// Over 32 KiB together the text is cut, and a note says where and
    /// which files were not read at all.
    #[test]
    fn the_text_is_capped_with_a_note() {
        let t = tree();
        repo(&t.ws);
        let line = "x".repeat(99) + "\n";
        put(&t.ws.join("AGENTS.md"), &line.repeat(200)); // 20,000 bytes
        put(&t.ws.join("a/AGENTS.md"), &line.repeat(200));
        put(&t.ws.join("a/b/AGENTS.md"), "never reached");
        let got = read(&t.ws.join("a/b"), &t.ws).unwrap();
        let into = CAP - 20_000;
        assert!(
            got.ends_with(&format!(
                "\n[cut: the instruction files are longer than 32 KiB together; the text above \
                 stops {into} bytes into a/AGENTS.md, and a/b/AGENTS.md was not read]"
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
    }

    /// A link is followed only to a regular file inside the repository
    /// (inside the session folder for the session folder's own file); a
    /// link out of it, a FIFO and a protected file are named, not read,
    /// and nothing waits.
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
        let got = read(&top.join("a/b/c"), &ws).unwrap();
        assert_eq!(
            got,
            format!(
                "{LEAD}\n==> AGENTS.md <==\nkept in docs\n\
                 ==> a/AGENTS.md <== (not read: it leads outside the folder)\n\
                 ==> a/b/AGENTS.md <== (not read: it is not a regular file)\n\
                 ==> a/b/c/AGENTS.md <== (not read: it leads outside the folder)"
            )
        );
        // Out of the granted folder altogether.
        let w = ws.join("w");
        std::fs::create_dir(&w).unwrap();
        std::os::unix::fs::symlink(base.join("secret.txt"), w.join("AGENTS.md")).unwrap();
        assert_eq!(
            read(&w, &ws).unwrap(),
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
            format!("{LEAD}\n==> CLAUDE.md <== (not read: it leads outside the folder)")
        );
        // Without the protection the same link is read (the control).
        assert!(read(&top.join("p"), &ws).unwrap().contains("secret key"));
    }
}
