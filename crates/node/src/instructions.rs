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
    let _ = (cwd, granted, protected);
    None
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
        put(&top.join("crates/foo/deeper/AGENTS.md"), "below the session");
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
        let text: usize = got.lines().filter(|l| l.starts_with('x')).map(|l| l.len()).sum();
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
