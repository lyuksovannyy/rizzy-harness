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

/// A small regular file reached by no link (`path` is canonical by
/// construction: every component was checked), as text.
fn read_small(path: &Path, cap: u64) -> Option<String> {
    let md = std::fs::symlink_metadata(path).ok()?;
    if !md.is_file() || md.len() > cap {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

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
    let Some(text) = read_small(&git.join("HEAD"), 4096) else {
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
        && let Some(id) = read_small(&loose, 4096).and_then(|t| object_id(&t))
    {
        return (Some(id), branch.into());
    }
    // `packed-refs`: `<id> <ref>` lines (`#` and `^` lines skipped).
    let id = read_small(&git.join("packed-refs"), 64 << 20).and_then(|t| {
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
    let file = |p: PathBuf| {
        (plain(&p) && std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_file())).then_some(p)
    };
    Ok(UserRepo {
        top: top.to_path_buf(),
        objects: no_borrow.is_none().then_some(objects),
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
