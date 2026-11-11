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
const REF_MAX: u64 = 512;

/// The most `packed-refs` may hold; a bigger one makes `HEAD` unknown.
const PACKED_REFS_MAX: u64 = 1 << 20;

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
fn head_of(git: &Path) -> (Option<String>, String) {
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
    let loose = git.join(name);
    if plain(&loose)
        && let Some(id) = read_small(&loose, REF_MAX).and_then(|t| object_id(&t))
    {
        return (Some(id), branch.into());
    }
    // `packed-refs`: `<id> <ref>` lines (`#` and `^` lines skipped).
    let id = read_small(&git.join("packed-refs"), PACKED_REFS_MAX).and_then(|t| {
        t.lines()
            .filter(|l| !l.starts_with('#') && !l.starts_with('^'))
            .find_map(|l| match l.split_once(' ') {
                Some((id, r)) if r == name => object_id(id),
                _ => None,
            })
    });
    (id, branch.into())
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
}
