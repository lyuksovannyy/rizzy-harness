//! What the node knows about the user's repository, read by our own code
//! from its files with strict rules, never by asking git (R.1 follow-up,
//! PLAN.md T19).
//!
//! A session may write anything in its repository's `.git` (PLAN.md R2):
//! `commondir`, a gitfile, `core.worktree`, `extensions.*`, alternates. Git
//! honours each of those as a redirect, so no git process of the node runs
//! with the user's `.git` as its git directory or common directory. What we
//! need is read here: whether `<top>/.git` is a plain repository directory,
//! its `HEAD` commit and branch (one symbolic ref, followed to a loose ref
//! file or `packed-refs`), its object format (from the length of that id),
//! and the paths of its object store, ignore file and index, each refused
//! when it is a link or anything else unusual. The object store is the one
//! thing the node's git uses, borrowed read-only through a shadow's
//! `objects/info/alternates`, and only when it chains to nothing else.

use std::path::{Path, PathBuf};

/// The repository whose `.git` directory is in [`UserRepo::top`].
#[derive(Debug, Clone)]
pub(crate) struct UserRepo {
    /// The folder holding `.git` (canonical).
    pub top: PathBuf,
    /// `<top>/.git/objects` (canonical), when it may be borrowed: a real
    /// directory, not a link, holding no `info/alternates` or
    /// `info/http-alternates` (git follows those chains), and `HEAD` read.
    pub objects: Option<PathBuf>,
    /// The identity of [`Self::objects`] when it was accepted (R1c-1): every
    /// sandbox grant of it carries this, so a store replaced since is never
    /// granted.
    pub objects_id: Option<harness_core::folder::Identity>,
    /// Why [`Self::objects`] is `None` (for a sub-agent's refusal).
    pub no_borrow: Option<String>,
    /// `<top>/.git/info/exclude`, when a regular file reached by no link.
    pub exclude: Option<PathBuf>,
    /// `<top>/.git/index`, the same way.
    pub index: Option<PathBuf>,
    /// The commit `HEAD` names (hex), when it could be read; `None` for an
    /// unborn branch or anything unusual.
    pub head: Option<String>,
    /// The branch `HEAD` names (`main`), or `HEAD` when detached.
    pub branch: String,
    /// `sha1` or `sha256`, from the length of `head` (`sha1` when unknown).
    pub format: &'static str,
}

/// What the walk up from a session folder found.
#[derive(Debug, Clone)]
pub(crate) enum Found {
    /// A plain repository directory.
    Repo(UserRepo),
    /// A `.git` entry that is not one: a gitfile (a linked worktree, a
    /// submodule), a link, or a directory with a `commondir`. The folder is
    /// treated as a plain one; the reason is for a sub-agent's refusal.
    Unusual(String),
    /// No `.git` entry within the bounds.
    None,
}

/// A small regular file, as bytes: opened with `O_NOFOLLOW | O_NONBLOCK`
/// (a link at the last component, a FIFO or a device is never followed or
/// waited on), checked to be a regular file on the opened descriptor, and
/// read through `take(cap + 1)`: more than `cap` bytes, whatever the file
/// claimed when it was opened, counts as unreadable (R1b-3). The session
/// may write every file read here.
pub(crate) fn read_bounded(path: &Path, cap: u64) -> Option<Vec<u8>> {
    use nix::fcntl::OFlag;
    use std::io::Read;
    let fd = nix::fcntl::open(
        path,
        OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .ok()?;
    let f = std::fs::File::from(fd);
    if !f.metadata().ok()?.is_file() {
        return None;
    }
    let mut out = Vec::new();
    f.take(cap + 1).read_to_end(&mut out).ok()?;
    (out.len() as u64 <= cap).then_some(out)
}

/// [`read_bounded`] as text.
fn read_small(path: &Path, cap: u64) -> Option<String> {
    String::from_utf8(read_bounded(path, cap)?).ok()
}

/// The most `HEAD` and a loose ref file may hold (a ref is under 100
/// bytes; `HEAD` names one).
pub(super) const REF_MAX: u64 = 512;

/// The most `packed-refs` may hold (about 190,000 refs); a bigger one makes
/// `HEAD` unknown. A fork's copy refuses the same size (`fork_sanitise`).
pub(super) const PACKED_REFS_MAX: u64 = 16 << 20;

/// `path` exists and leads nowhere else: no link on it (its canonical form
/// is itself).
fn plain(path: &Path) -> bool {
    path.canonicalize().is_ok_and(|c| c == path)
}

/// A commit id: 40 or 64 lowercase hex digits.
fn object_id(s: &str) -> Option<String> {
    let s = s.trim();
    ((s.len() == 40 || s.len() == 64) && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| s.to_ascii_lowercase())
}

/// A branch ref name we follow: `refs/heads/` and plain components of
/// ordinary characters only.
fn branch_ref(name: &str) -> Option<&str> {
    let rest = name.strip_prefix("refs/heads/")?;
    let ok = !rest.is_empty()
        && rest.split('/').all(|c| {
            !c.is_empty()
                && c != "."
                && c != ".."
                && !c.starts_with('.')
                && !c.ends_with(".lock")
                && c.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.+@".contains(&b))
        });
    ok.then_some(rest)
}

/// `HEAD`'s commit and branch in the git directory `git` (canonical).
pub(super) fn head_of(git: &Path) -> (Option<String>, String) {
    let Some(text) = read_small(&git.join("HEAD"), REF_MAX) else {
        return (None, "HEAD".into());
    };
    let text = text.trim_end_matches('\n');
    if let Some(id) = object_id(text) {
        return (Some(id), "HEAD".into());
    }
    let Some(name) = text.strip_prefix("ref: ").map(str::trim) else {
        return (None, "HEAD".into());
    };
    let Some(branch) = branch_ref(name) else {
        return (None, "HEAD".into());
    };
    (resolve(git, name), branch.into())
}

/// The commit the ref `name` names in the git directory `git`: its loose
/// ref file (read with no link followed anywhere on the path), else its
/// line in `packed-refs` (`<id> <ref>` lines, `#` and `^` lines skipped).
fn resolve(git: &Path, name: &str) -> Option<String> {
    let loose = git.join(name);
    if plain(&loose)
        && let Some(id) = read_small(&loose, REF_MAX).and_then(|t| object_id(&t))
    {
        return Some(id);
    }
    read_small(&git.join("packed-refs"), PACKED_REFS_MAX).and_then(|t| {
        t.lines()
            .filter(|l| !l.starts_with('#') && !l.starts_with('^'))
            .find_map(|l| match l.split_once(' ') {
                Some((id, r)) if r == name => object_id(id),
                _ => None,
            })
    })
}

/// The most a `HEAD` read for [`head_at`] may hold: a branch name of a few
/// kilobytes is cut when shown, not refused (a person's own branch is a
/// name; a hostile one is bounded text).
const HEAD_AT_MAX: u64 = 8192;

/// Where a repository's `HEAD` pointed, for the note on a turn that moved
/// it (X.42): the branch name as the repository holds it (`None` when
/// detached) and the commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HeadPoint {
    pub branch: Option<String>,
    pub commit: String,
}

/// [`HeadPoint`] of the repository whose top folder is `top` (canonical),
/// from its files alone and by the rules of [`find`]: `<top>/.git` is a
/// real directory (no gitfile, no link, no `commondir`), `HEAD` is a small
/// regular file, a branch is `refs/heads/<name>` with plain path components
/// (a name may hold any other character: it is only ever shown, after
/// cleaning), the ref is read like [`head_of`] reads it. `None` for
/// anything else, including an unborn branch.
pub(crate) fn head_at(top: &Path) -> Option<HeadPoint> {
    let git = top.join(".git");
    if !std::fs::symlink_metadata(&git).ok()?.is_dir()
        || git.join("commondir").exists()
        || std::fs::symlink_metadata(git.join("gitdir")).is_ok()
        || !plain(&git)
    {
        return None;
    }
    let text = read_small(&git.join("HEAD"), HEAD_AT_MAX)?;
    let text = text.trim_end_matches('\n');
    if let Some(commit) = object_id(text) {
        return Some(HeadPoint {
            branch: None,
            commit,
        });
    }
    let name = text.strip_prefix("ref: ")?.trim();
    let branch = name.strip_prefix("refs/heads/")?;
    let ok = !branch.is_empty()
        && !name.contains('\0')
        && branch
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != "..");
    if !ok {
        return None;
    }
    Some(HeadPoint {
        branch: Some(branch.to_string()),
        commit: resolve(&git, name)?,
    })
}

/// What `<top>/.git` (a real directory) says.
fn read(top: &Path) -> Result<UserRepo, String> {
    let git = top.join(".git");
    if git.join("commondir").exists() || std::fs::symlink_metadata(git.join("gitdir")).is_ok() {
        return Err(format!(
            "{} names another git directory (commondir); it is not used as a repository",
            git.display()
        ));
    }
    if !std::fs::symlink_metadata(git.join("HEAD")).is_ok_and(|m| m.is_file()) {
        return Err(format!("{} has no HEAD file", git.display()));
    }
    let (head, branch) = head_of(&git);
    let format = match head.as_deref().map(str::len) {
        Some(64) => "sha256",
        _ => "sha1",
    };
    let objects = git.join("objects");
    let no_borrow = if head.is_none() {
        Some("its HEAD names no commit the node could read".to_string())
    } else if !std::fs::symlink_metadata(&objects).is_ok_and(|m| m.is_dir()) || !plain(&objects) {
        Some("its object store (.git/objects) is a link or missing".to_string())
    } else if ["info/alternates", "info/http-alternates"]
        .iter()
        .any(|f| std::fs::symlink_metadata(objects.join(f)).is_ok())
    {
        Some("its object store borrows from another one (objects/info/alternates)".to_string())
    } else {
        None
    };
    let objects_id = if no_borrow.is_none() {
        harness_core::folder::Identity::of(&objects).ok()
    } else {
        None
    };
    let no_borrow = match (no_borrow, objects_id) {
        (None, None) => Some("its object store could not be read".to_string()),
        (n, _) => n,
    };
    let file = |p: PathBuf| {
        (plain(&p) && std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_file())).then_some(p)
    };
    Ok(UserRepo {
        top: top.to_path_buf(),
        objects: no_borrow.is_none().then_some(objects),
        objects_id,
        no_borrow,
        exclude: file(git.join("info/exclude")),
        index: file(git.join("index")),
        head,
        branch,
        format,
    })
}

/// Walk up from `cwd` (canonical), not above `ceiling`, to the nearest
/// folder that holds a `.git` entry, and read it.
pub(crate) fn find(cwd: &Path, ceiling: Option<&Path>) -> Found {
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        if let Some(c) = ceiling
            && !d.starts_with(c)
        {
            break;
        }
        let git = d.join(".git");
        if let Ok(md) = std::fs::symlink_metadata(&git) {
            if !md.is_dir() {
                return Found::Unusual(format!(
                    "{} is a {} (a linked worktree or a submodule), not a repository \
                         directory",
                    git.display(),
                    if md.file_type().is_symlink() {
                        "link"
                    } else {
                        "file"
                    }
                ));
            }
            return match read(d) {
                Ok(r) => Found::Repo(r),
                Err(why) => Found::Unusual(why),
            };
        }
        dir = d.parent();
    }
    Found::None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R1b-3: every file read here is bounded, never followed through a
    /// link at its last component, and never waited on.
    #[test]
    fn files_the_session_may_write_are_read_bounded_and_without_following() {
        let d = tempfile::tempdir().unwrap();
        let top = d.path().canonicalize().unwrap();
        let git = top.join(".git");
        std::fs::create_dir_all(git.join("objects")).unwrap();
        std::fs::create_dir_all(git.join("refs/heads")).unwrap();
        let id = "a".repeat(40);
        // A packed `HEAD` ref within the cap is read...
        std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(git.join("packed-refs"), format!("{id} refs/heads/main\n")).unwrap();
        assert_eq!(head_of(&git).0.as_deref(), Some(id.as_str()));
        // ...one past it makes HEAD unknown.
        let mut big = "# pad\n".repeat((PACKED_REFS_MAX as usize) / 6 + 1);
        big.push_str(&format!("{id} refs/heads/main\n"));
        std::fs::write(git.join("packed-refs"), big).unwrap();
        assert_eq!(head_of(&git).0, None);
        std::fs::remove_file(git.join("packed-refs")).unwrap();
        // A loose ref that is a link, or a FIFO, is not read (and the FIFO
        // is not waited on).
        let elsewhere = top.join("elsewhere");
        std::fs::write(&elsewhere, format!("{id}\n")).unwrap();
        std::os::unix::fs::symlink(&elsewhere, git.join("refs/heads/main")).unwrap();
        assert_eq!(head_of(&git).0, None);
        std::fs::remove_file(git.join("refs/heads/main")).unwrap();
        nix::unistd::mkfifo(&git.join("refs/heads/main"), nix::sys::stat::Mode::S_IRWXU).unwrap();
        assert_eq!(head_of(&git).0, None);
        // An oversized HEAD is unknown too.
        std::fs::write(git.join("HEAD"), "x".repeat(REF_MAX as usize + 1)).unwrap();
        assert_eq!(head_of(&git).0, None);
    }

    /// X.42: HEAD for the turn's note is read from files, by the same rules:
    /// no link, no gitfile, no `commondir`, no way out of `refs/heads`, a
    /// bounded `HEAD`.
    #[test]
    fn head_at_reads_files_and_refuses_redirects() {
        use std::os::unix::fs::symlink;
        let d = tempfile::tempdir().unwrap();
        let top = d.path().canonicalize().unwrap();
        let git = top.join(".git");
        std::fs::create_dir_all(git.join("refs/heads")).unwrap();
        let (a, b) = ("a".repeat(40), "b".repeat(64));
        let put = |name: &str, text: &str| std::fs::write(git.join(name), text).unwrap();
        // A branch, loose; a branch only in packed-refs; detached.
        put("HEAD", "ref: refs/heads/main\n");
        put("refs/heads/main", &format!("{a}\n"));
        let at = head_at(&top).unwrap();
        assert_eq!(
            (at.branch.as_deref(), at.commit.as_str()),
            (Some("main"), a.as_str())
        );
        put("HEAD", "ref: refs/heads/feat/x\n");
        put("packed-refs", &format!("# pack\n{b} refs/heads/feat/x\n"));
        assert_eq!(head_at(&top).unwrap().commit, b);
        put("HEAD", &format!("{a}\n"));
        let at = head_at(&top).unwrap();
        assert_eq!((at.branch, at.commit), (None, a.clone()));
        // Unborn, a ref outside refs/heads, a way out of it, NUL, no commit.
        for head in [
            "ref: refs/heads/unborn\n",
            "ref: refs/remotes/origin/main\n",
            "ref: refs/heads/../../HEAD\n",
            "ref: refs/heads/a//b\n",
            "ref: refs/heads/\n",
            "ref: refs/heads/x\0y\n",
            "garbage\n",
        ] {
            put("HEAD", head);
            assert_eq!(head_at(&top), None, "{head:?}");
        }
        // A ref file that is a link is not followed (the packed line is).
        put("HEAD", "ref: refs/heads/l\n");
        put("packed-refs", "");
        std::fs::write(top.join("target"), format!("{a}\n")).unwrap();
        symlink(top.join("target"), git.join("refs/heads/l")).unwrap();
        assert_eq!(head_at(&top), None);
        // A linked directory on the ref's path is not followed either.
        let outside = d.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("x"), format!("{a}\n")).unwrap();
        symlink(&outside, git.join("refs/heads/feat")).unwrap();
        put("HEAD", "ref: refs/heads/feat/x\n");
        assert_eq!(head_at(&top), None);
        std::fs::remove_file(git.join("refs/heads/feat")).unwrap();
        // An oversized HEAD is unknown.
        put("HEAD", &"x".repeat(HEAD_AT_MAX as usize + 1));
        assert_eq!(head_at(&top), None);
        // A commondir, a gitfile, a linked `.git`.
        put("HEAD", &format!("{a}\n"));
        put("commondir", "..\n");
        assert_eq!(head_at(&top), None);
        std::fs::remove_file(git.join("commondir")).unwrap();
        assert!(head_at(&top).is_some());
        let linked = d.path().join("linked");
        std::fs::create_dir(&linked).unwrap();
        let linked = linked.canonicalize().unwrap();
        symlink(&git, linked.join(".git")).unwrap();
        assert_eq!(head_at(&linked), None);
        let filed = d.path().join("filed");
        std::fs::create_dir(&filed).unwrap();
        let filed = filed.canonicalize().unwrap();
        std::fs::write(filed.join(".git"), format!("gitdir: {}\n", git.display())).unwrap();
        assert_eq!(head_at(&filed), None);
    }
}
