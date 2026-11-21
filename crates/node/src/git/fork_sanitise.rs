//! Stage 1 of a fork's own copy: a sanitised copy of the parent's `.git`,
//! made by our own code, with no git process (PLAN.md T92, docs/fork.md).
//!
//! The parent's `.git` may have been written by an agent, and git follows
//! every link it finds there (`refs/heads/<x>`, `objects/pack/*.pack`,
//! `objects/pack` itself, a loose object, `config`), so a link to another
//! repository on the disk would carry that repository's history or its
//! `origin` URL into the fork. So **git never opens the parent**, and this
//! walk never follows a link in it:
//!
//! - every entry is opened with `openat` from the descriptor of the folder
//!   that holds it, `O_NOFOLLOW | O_CLOEXEC` (`O_DIRECTORY` for folders),
//!   first as an `O_PATH` descriptor to learn its kind with `fstat` on that
//!   descriptor (a FIFO, a socket or a device is never opened for reading,
//!   so nothing waits), then for a regular file as a readable descriptor
//!   that is checked again with `fstat` to be the same regular file;
//! - no path-based `stat`, `open` or `read_link` is made on anything in the
//!   parent below its `.git` (that one is opened from the repository's
//!   folder with `O_NOFOLLOW` too);
//! - what is copied is `HEAD`, `refs/`, `packed-refs`, `shallow` and
//!   `objects/`, as new files made with `O_EXCL` in our staging folder
//!   (bytes copied, never linked, so nothing is shared with the parent),
//!   and nothing else: no `config`, hooks, `info/`, `worktrees/`,
//!   `modules/`, `index`, `logs/` or `grafts`;
//! - anything that is not a regular file or a folder under what is copied,
//!   a link above all, ends the whole copy in a refusal that names its
//!   path: skipping it would give a fork with a ref or a pack missing;
//! - the parent's `config` and `info/exclude` are read as data (a regular
//!   file, capped, bytes) and returned, never copied as they are;
//! - bytes, entries, depth and time are bounded, and the walk stops at the
//!   bound.
//!
//! The order is `HEAD`, `refs/`, `packed-refs`, then `objects/`: every
//! object a copied ref names existed when the ref was read, so a parent that
//! keeps working cannot make the copy point at objects it lacks. (Refs are
//! read loose first, packed second, as git itself reads them: a ref a
//! `pack-refs` moves in between is found in `packed-refs`.) A parent being
//! repacked at that moment may still give a copy whose objects are missing:
//! the clone of stage 2 then fails cleanly and the person can try again.

use std::ffi::{OsStr, OsString};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::ffi::OsStringExt;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use nix::errno::Errno;
use nix::fcntl::{OFlag, openat};
use nix::sys::stat::{FileStat, Mode, SFlag, fstat, mkdirat};

/// The most the parent's `config` and `info/exclude` may hold, each.
pub(crate) const DATA_MAX: u64 = 1 << 20;

/// The most entries (files and folders) the walk copies.
pub(crate) const ENTRY_MAX: u64 = 1_000_000;

/// The deepest folder nesting the walk follows (refs are a few levels).
const DEPTH_MAX: usize = 32;

/// What the walk is bounded by.
pub(crate) struct Limits {
    pub max_bytes: u64,
    pub max_entries: u64,
    pub deadline: Instant,
    /// Set when the caller stopped waiting (a cancelled copy).
    pub cancel: Arc<AtomicBool>,
}

/// What the parent's `.git` said that the copy needs, as bytes read as data.
#[derive(Debug, Default)]
pub(crate) struct Data {
    /// `config`, up to [`DATA_MAX`] bytes.
    pub config: Option<Vec<u8>>,
    /// `info/exclude`, up to [`DATA_MAX`] bytes.
    pub exclude: Option<Vec<u8>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Dir,
    File,
    Link,
    Other,
}

fn kind_of(st: &FileStat) -> Kind {
    match SFlag::from_bits_truncate(st.st_mode) & SFlag::S_IFMT {
        SFlag::S_IFDIR => Kind::Dir,
        SFlag::S_IFREG => Kind::File,
        SFlag::S_IFLNK => Kind::Link,
        _ => Kind::Other,
    }
}

/// A name for a message: lossy, control characters out, bounded.
fn shown(rel: &str) -> String {
    rel.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .take(200)
        .collect()
}

fn what(k: Kind) -> &'static str {
    match k {
        Kind::Dir => "a folder",
        Kind::File => "a file",
        Kind::Link => "a link",
        Kind::Other => "a special file (a FIFO, a socket or a device)",
    }
}

fn refuse(rel: &str, k: Kind) -> String {
    format!(
        "the repository is not copied: .git/{} is {}, and only plain files and folders are \
         copied (a link could lead out of the repository)",
        shown(rel),
        what(k)
    )
}

fn dir_flags() -> OFlag {
    OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC
}

/// `name` in `dir`, as `O_PATH` (nothing is read or waited on, a link is
/// not followed): its descriptor, `fstat` and kind; `None` when absent.
fn probe(dir: BorrowedFd<'_>, name: &OsStr) -> Result<Option<(OwnedFd, FileStat, Kind)>, String> {
    let shown_name = || shown(&name.to_string_lossy());
    match openat(
        dir,
        name,
        OFlag::O_PATH | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => {
            let st = fstat(&fd).map_err(|e| format!("{}: {e}", shown_name()))?;
            let k = kind_of(&st);
            Ok(Some((fd, st, k)))
        }
        Err(Errno::ENOENT) => Ok(None),
        Err(e) => Err(format!("cannot look at .git/{}: {e}", shown_name())),
    }
}

struct Walk<'a> {
    lim: &'a Limits,
    bytes: u64,
    entries: u64,
    buf: Vec<u8>,
    /// A measuring pass: the same walk, with the same bounds, that opens
    /// nothing for reading and writes nothing; it counts the sizes `fstat`
    /// gives for what a copy would write.
    dry: bool,
}

impl Walk<'_> {
    fn tick(&self) -> Result<(), String> {
        if self.lim.cancel.load(Ordering::Relaxed) {
            return Err("the copy was cancelled".into());
        }
        if Instant::now() >= self.lim.deadline {
            return Err(
                "copying the repository took longer than this node allows ([limits] \
                 clone_timeout_s); nothing was kept"
                    .into(),
            );
        }
        Ok(())
    }

    fn entry(&mut self) -> Result<(), String> {
        self.entries += 1;
        if self.entries > self.lim.max_entries {
            return Err(format!(
                "the repository is not copied: it has more than {} files and folders",
                self.lim.max_entries
            ));
        }
        self.tick()
    }

    fn count(&mut self, n: u64) -> Result<(), String> {
        self.bytes = self.bytes.saturating_add(n);
        if self.bytes > self.lim.max_bytes {
            return Err(format!(
                "the repository is bigger than this node copies ([limits] clone_max_mb = {} MB); \
                 nothing was kept",
                self.lim.max_bytes / (1024 * 1024)
            ));
        }
        self.tick()
    }

    /// The regular file `name` of `dir`, open for reading: never a link,
    /// never a FIFO, a device or a socket (those are refused from the
    /// `O_PATH` descriptor, before anything is opened for reading), and the
    /// file that was probed (same device and inode).
    fn open_file(
        &self,
        dir: BorrowedFd<'_>,
        name: &OsStr,
        rel: &str,
    ) -> Result<Option<std::fs::File>, String> {
        let Some((_p, st, k)) = probe(dir, name)? else {
            return Ok(None);
        };
        if k != Kind::File {
            return Err(refuse(rel, k));
        }
        let fd = openat(
            dir,
            name,
            OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| refuse(rel, Kind::Other))?;
        let now = fstat(&fd).map_err(|e| format!("{}: {e}", shown(rel)))?;
        if kind_of(&now) != Kind::File || (now.st_dev, now.st_ino) != (st.st_dev, st.st_ino) {
            return Err(refuse(rel, Kind::Other));
        }
        Ok(Some(std::fs::File::from(fd)))
    }

    /// `name` of `dir`, opened as a folder.
    fn open_dir(&self, dir: BorrowedFd<'_>, name: &OsStr, rel: &str) -> Result<OwnedFd, String> {
        let Some((_p, _st, k)) = probe(dir, name)? else {
            return Err(format!(
                "the repository is not copied: .git/{} is missing",
                shown(rel)
            ));
        };
        if k != Kind::Dir {
            return Err(refuse(rel, k));
        }
        openat(dir, name, dir_flags(), Mode::empty()).map_err(|_| refuse(rel, Kind::Link))
    }

    /// Copy the bytes of `src` into a new file `name` in `dst` (made with
    /// `O_EXCL`, so nothing is overwritten or followed), counting them.
    fn write_new(
        &mut self,
        mut src: std::fs::File,
        dst: BorrowedFd<'_>,
        name: &OsStr,
        rel: &str,
    ) -> Result<(), String> {
        use std::io::{Read, Write};
        let out = openat(
            dst,
            name,
            OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::from_bits_truncate(0o600),
        )
        .map_err(|e| format!("cannot write the copy of .git/{}: {e}", shown(rel)))?;
        let mut out = std::fs::File::from(out);
        loop {
            let n = src
                .read(&mut self.buf)
                .map_err(|e| format!("cannot read .git/{}: {e}", shown(rel)))?;
            if n == 0 {
                return Ok(());
            }
            self.count(n as u64)?;
            out.write_all(&self.buf[..n])
                .map_err(|e| format!("cannot write the copy of .git/{}: {e}", shown(rel)))?;
        }
    }

    /// One plain file of `src` into `dst`, when it is there.
    fn copy_file(
        &mut self,
        src: BorrowedFd<'_>,
        dst: Option<BorrowedFd<'_>>,
        name: &str,
    ) -> Result<bool, String> {
        self.entry()?;
        let Some(dst) = dst else {
            let Some((_p, st, k)) = probe(src, OsStr::new(name))? else {
                return Ok(false);
            };
            if k != Kind::File {
                return Err(refuse(name, k));
            }
            self.count(u64::try_from(st.st_size).unwrap_or(0))?;
            return Ok(true);
        };
        match self.open_file(src, OsStr::new(name), name)? {
            Some(f) => self
                .write_new(f, dst, OsStr::new(name), name)
                .map(|()| true),
            None => Ok(false),
        }
    }

    /// A file read as data: a regular file of at most `cap` bytes.
    fn read_data(
        &mut self,
        src: BorrowedFd<'_>,
        name: &str,
        rel: &str,
        cap: u64,
    ) -> Result<Option<Vec<u8>>, String> {
        use std::io::Read;
        let Some(f) = self.open_file(src, OsStr::new(name), rel)? else {
            return Ok(None);
        };
        let mut data = Vec::new();
        f.take(cap + 1)
            .read_to_end(&mut data)
            .map_err(|e| format!("cannot read .git/{}: {e}", shown(rel)))?;
        if data.len() as u64 > cap {
            return Err(format!(
                "the repository is not copied: .git/{} is larger than {} MiB",
                shown(rel),
                cap / (1024 * 1024)
            ));
        }
        self.count(data.len() as u64)?;
        Ok(Some(data))
    }

    /// A new folder `name` in `dst`, opened.
    fn make_dir(&self, dst: BorrowedFd<'_>, name: &OsStr, rel: &str) -> Result<OwnedFd, String> {
        let bad = |e: Errno| format!("cannot write the copy of .git/{}: {e}", shown(rel));
        mkdirat(dst, name, Mode::from_bits_truncate(0o700)).map_err(bad)?;
        openat(dst, name, dir_flags(), Mode::empty()).map_err(bad)
    }

    /// The folder `src` into `dst` (both opened), recursively; `skip` names
    /// relative paths that are left out (never followed either).
    fn copy_tree(
        &mut self,
        src: OwnedFd,
        dst: Option<OwnedFd>,
        rel: &str,
        depth: usize,
        skip: &[&str],
    ) -> Result<(), String> {
        if depth > DEPTH_MAX {
            return Err(format!(
                "the repository is not copied: .git/{} is nested deeper than {DEPTH_MAX} levels",
                shown(rel)
            ));
        }
        // A second descriptor for the listing: `Dir` takes it over.
        let bad = |e: Errno| format!("cannot list .git/{}: {e}", shown(rel));
        let lister = openat(src.as_fd(), ".", dir_flags(), Mode::empty()).map_err(bad)?;
        let mut list = nix::dir::Dir::from_fd(lister).map_err(bad)?;
        let mut names: Vec<OsString> = Vec::new();
        for e in list.iter() {
            let e = e.map_err(bad)?;
            let n = e.file_name().to_bytes();
            if n == b"." || n == b".." {
                continue;
            }
            names.push(OsString::from_vec(n.to_vec()));
            // A folder of millions of names is stopped while it is listed.
            if names.len() as u64 > self.lim.max_entries {
                return Err(format!(
                    "the repository is not copied: .git/{} holds more than {} entries",
                    shown(rel),
                    self.lim.max_entries
                ));
            }
        }
        for name in names {
            let child = format!("{rel}/{}", name.to_string_lossy());
            self.entry()?;
            if skip.contains(&child.as_str()) {
                continue;
            }
            let Some((_p, st, k)) = probe(src.as_fd(), &name)? else {
                // Gone since the listing (a loose object packed away).
                continue;
            };
            match k {
                Kind::Dir => {
                    let d = match &dst {
                        Some(dst) => Some(self.make_dir(dst.as_fd(), &name, &child)?),
                        None => None,
                    };
                    let s = openat(src.as_fd(), name.as_os_str(), dir_flags(), Mode::empty())
                        .map_err(|_| refuse(&child, Kind::Link))?;
                    self.copy_tree(s, d, &child, depth + 1, skip)?;
                }
                Kind::File => match &dst {
                    Some(dst) => {
                        if let Some(f) = self.open_file(src.as_fd(), &name, &child)? {
                            self.write_new(f, dst.as_fd(), &name, &child)?;
                        }
                    }
                    None => self.count(u64::try_from(st.st_size).unwrap_or(0))?,
                },
                other => return Err(refuse(&child, other)),
            }
        }
        Ok(())
    }
}

/// The files that make git read another object store; the copy is refused
/// when one holds anything (an empty one says nothing).
const ALTERNATES: [&str; 2] = ["objects/info/alternates", "objects/info/http-alternates"];

/// Copy the parent's repository `<root>/.git` into the existing folder
/// `into` (see the module comment); returns the parent's `config` and
/// `info/exclude` as data.
pub(crate) fn copy(root: &Path, into: &Path, lim: &Limits) -> Result<Data, String> {
    walk(root, Some(into), lim).map(|(data, _)| data)
}

/// The bytes a [`copy`] of `<root>/.git` would write, from a first pass of
/// the same walk (the same descriptors, `O_NOFOLLOW`, bounds and
/// cancellation) that opens no file for reading and writes nothing: the
/// room check before a copy. A link, a special file or a bound ends it in
/// the refusal the copy would give.
pub(crate) fn measure(root: &Path, lim: &Limits) -> Result<u64, String> {
    walk(root, None, lim).map(|(_, bytes)| bytes)
}

fn walk(root: &Path, into: Option<&Path>, lim: &Limits) -> Result<(Data, u64), String> {
    let mut w = Walk {
        lim,
        bytes: 0,
        entries: 0,
        buf: if into.is_some() {
            vec![0; 256 * 1024]
        } else {
            Vec::new()
        },
        dry: into.is_none(),
    };
    let top = nix::fcntl::open(root, dir_flags(), Mode::empty())
        .map_err(|e| format!("cannot open the repository's folder: {e}"))?;
    let git = match openat(top.as_fd(), ".git", dir_flags(), Mode::empty()) {
        Ok(fd) => fd,
        Err(Errno::ELOOP) => return Err("the repository is not copied: .git is a link".into()),
        Err(Errno::ENOTDIR)
            if matches!(
                probe(top.as_fd(), OsStr::new(".git")),
                Ok(Some((_, _, Kind::Link)))
            ) =>
        {
            return Err("the repository is not copied: .git is a link".into());
        }
        Err(Errno::ENOTDIR) => {
            return Err(
                "the repository is not copied: .git is a file (a linked worktree or a submodule), \
                 not a repository folder"
                    .into(),
            );
        }
        Err(Errno::ENOENT) => return Err("the folder is not a git repository".into()),
        Err(e) => return Err(format!("cannot open .git: {e}")),
    };
    let dst = match into {
        Some(into) => Some(
            nix::fcntl::open(into, dir_flags(), Mode::empty())
                .map_err(|e| format!("cannot open the copy's folder: {e}"))?,
        ),
        None => None,
    };
    let dst_fd = dst.as_ref().map(|d| d.as_fd());
    // Another git directory by name: not a repository of its own.
    for n in ["commondir", "gitdir"] {
        if probe(git.as_fd(), OsStr::new(n))?.is_some() {
            return Err(format!(
                "the repository is not copied: .git/{n} names another git directory"
            ));
        }
    }
    // What is read as data first: cheap, and a special file there ends
    // the copy before the heavy work.
    let (mut config, mut exclude) = (None, None);
    if !w.dry {
        config = w.read_data(git.as_fd(), "config", "config", DATA_MAX)?;
        exclude = match probe(git.as_fd(), OsStr::new("info"))? {
            Some((_, _, Kind::Dir)) => {
                let info = w.open_dir(git.as_fd(), OsStr::new("info"), "info")?;
                w.read_data(info.as_fd(), "exclude", "info/exclude", DATA_MAX)?
            }
            // Not copied and not read: whatever `info` is, nothing of it is used.
            _ => None,
        };
    }
    if !w.copy_file(git.as_fd(), dst_fd, "HEAD")? {
        return Err("the repository is not copied: it has no HEAD file".into());
    }
    // Refs, loose then packed.
    let refs_src = w.open_dir(git.as_fd(), OsStr::new("refs"), "refs")?;
    let refs_dst = match dst_fd {
        Some(d) => Some(w.make_dir(d, OsStr::new("refs"), "refs")?),
        None => None,
    };
    w.copy_tree(refs_src, refs_dst, "refs", 1, &[])?;
    w.copy_file(git.as_fd(), dst_fd, "packed-refs")?;
    w.copy_file(git.as_fd(), dst_fd, "shallow")?;
    // The objects, after every ref that names them.
    let obj_src = w.open_dir(git.as_fd(), OsStr::new("objects"), "objects")?;
    let info = match probe(obj_src.as_fd(), OsStr::new("info"))? {
        None => None,
        Some((_, _, Kind::Dir)) => {
            Some(w.open_dir(obj_src.as_fd(), OsStr::new("info"), "objects/info")?)
        }
        Some((_, _, k)) => return Err(refuse("objects/info", k)),
    };
    if let Some(info) = &info {
        for alt in ALTERNATES {
            let name = alt.rsplit('/').next().unwrap_or(alt);
            if let Some(f) = w.open_file(info.as_fd(), OsStr::new(name), alt)?
                && f.metadata().map_err(|e| format!("{alt}: {e}"))?.len() > 0
            {
                return Err(format!(
                    "the repository is not copied: its object store borrows from another one \
                     (.git/{alt})"
                ));
            }
        }
    }
    let obj_dst = match dst_fd {
        Some(d) => Some(w.make_dir(d, OsStr::new("objects"), "objects")?),
        None => None,
    };
    w.copy_tree(obj_src, obj_dst, "objects", 1, &ALTERNATES)?;
    Ok((Data { config, exclude }, w.bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(max_bytes: u64, max_entries: u64) -> Limits {
        Limits {
            max_bytes,
            max_entries,
            deadline: Instant::now() + std::time::Duration::from_secs(30),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    fn repo(top: &Path) -> std::path::PathBuf {
        let git = top.join("src/.git");
        std::fs::create_dir_all(git.join("refs/heads")).unwrap();
        std::fs::create_dir_all(git.join("objects/ab")).unwrap();
        std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(git.join("refs/heads/main"), "a".repeat(40)).unwrap();
        std::fs::write(git.join("objects/ab/cdef"), "object bytes").unwrap();
        top.join("src")
    }

    #[test]
    fn what_is_copied_is_the_five_things_and_the_rest_is_left_out() {
        let d = tempfile::tempdir().unwrap();
        let top = d.path().canonicalize().unwrap();
        let root = repo(&top);
        let git = root.join(".git");
        for f in [
            "hooks/pre-commit",
            "info/exclude",
            "logs/HEAD",
            "modules/x/y",
            "grafts-x",
        ] {
            std::fs::create_dir_all(git.join(f).parent().unwrap()).unwrap();
            std::fs::write(git.join(f), "x\n").unwrap();
        }
        std::fs::write(git.join("config"), "[core]\n\tbare = false\n").unwrap();
        std::fs::write(git.join("index"), "i").unwrap();
        std::fs::write(git.join("shallow"), "s\n").unwrap();
        std::fs::write(git.join("packed-refs"), "# pack\n").unwrap();
        let into = top.join("into");
        std::fs::create_dir(&into).unwrap();
        let data = copy(&root, &into, &limits(1 << 20, 1000)).unwrap();
        assert_eq!(
            data.config.as_deref(),
            Some(&b"[core]\n\tbare = false\n"[..])
        );
        assert_eq!(data.exclude.as_deref(), Some(&b"x\n"[..]));
        let mut seen: Vec<String> = walkdir::WalkDir::new(&into)
            .min_depth(1)
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
            .map(|e| e.path().strip_prefix(&into).unwrap().display().to_string())
            .collect();
        seen.sort();
        assert_eq!(
            seen,
            [
                "HEAD",
                "objects/ab/cdef",
                "packed-refs",
                "refs/heads/main",
                "shallow"
            ]
        );
    }

    /// The first pass counts what the copy writes, and refuses what it refuses.
    #[test]
    fn measure_counts_what_copy_writes_and_refuses_what_it_refuses() {
        let d = tempfile::tempdir().unwrap();
        let top = d.path().canonicalize().unwrap();
        let root = repo(&top);
        std::fs::write(root.join(".git/objects/ab/big"), vec![7u8; 4096]).unwrap();
        std::fs::write(root.join(".git/config"), "x".repeat(1000)).unwrap();
        let into = top.join("into");
        std::fs::create_dir(&into).unwrap();
        let n = measure(&root, &limits(1 << 20, 1000)).unwrap();
        copy(&root, &into, &limits(1 << 20, 1000)).unwrap();
        let written: u64 = walkdir::WalkDir::new(&into)
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
            .map(|e| e.metadata().unwrap().len())
            .sum();
        assert_eq!(n, written);
        std::os::unix::fs::symlink(top.join("elsewhere"), root.join(".git/refs/heads/x")).unwrap();
        let e = measure(&root, &limits(1 << 20, 1000)).unwrap_err();
        assert!(e.contains("refs/heads/x") && e.contains("a link"), "{e}");
    }

    #[test]
    fn a_link_anywhere_in_what_is_copied_is_refused_by_name() {
        for (at, name) in [
            ("refs/heads/x", "refs/heads/x"),
            ("objects/ab/zz", "objects/ab/zz"),
            ("HEAD2", ""),
        ] {
            let d = tempfile::tempdir().unwrap();
            let top = d.path().canonicalize().unwrap();
            let root = repo(&top);
            let git = root.join(".git");
            std::os::unix::fs::symlink(top.join("elsewhere"), git.join(at)).unwrap();
            let into = top.join("into");
            std::fs::create_dir(&into).unwrap();
            let r = copy(&root, &into, &limits(1 << 20, 1000));
            if name.is_empty() {
                assert!(
                    r.is_ok(),
                    "{at}: a link outside what is copied is not looked at"
                );
            } else {
                let e = r.unwrap_err();
                assert!(e.contains(name) && e.contains("a link"), "{at}: {e}");
            }
        }
    }

    #[test]
    fn the_bounds_stop_the_walk() {
        let d = tempfile::tempdir().unwrap();
        let top = d.path().canonicalize().unwrap();
        let root = repo(&top);
        std::fs::write(root.join(".git/objects/ab/big"), vec![7u8; 4096]).unwrap();
        let into = top.join("into");
        std::fs::create_dir(&into).unwrap();
        let e = copy(&root, &into, &limits(1000, 1000)).unwrap_err();
        assert!(e.contains("clone_max_mb"), "{e}");
        let into = top.join("into2");
        std::fs::create_dir(&into).unwrap();
        let e = copy(&root, &into, &limits(1 << 20, 3)).unwrap_err();
        assert!(e.contains("more than 3"), "{e}");
        let into = top.join("into3");
        std::fs::create_dir(&into).unwrap();
        let mut l = limits(1 << 20, 1000);
        l.deadline = Instant::now();
        let e = copy(&root, &into, &l).unwrap_err();
        assert!(e.contains("took longer"), "{e}");
        let into = top.join("into4");
        std::fs::create_dir(&into).unwrap();
        let l = limits(1 << 20, 1000);
        l.cancel.store(true, Ordering::Relaxed);
        assert!(copy(&root, &into, &l).unwrap_err().contains("cancelled"));
    }

    #[test]
    fn nesting_beyond_the_bound_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let top = d.path().canonicalize().unwrap();
        let root = repo(&top);
        let mut p = root.join(".git/refs/heads");
        for i in 0..40 {
            p = p.join(format!("d{i}"));
        }
        std::fs::create_dir_all(&p).unwrap();
        let into = top.join("into");
        std::fs::create_dir(&into).unwrap();
        let e = copy(&root, &into, &limits(1 << 20, 1000)).unwrap_err();
        assert!(e.contains("nested deeper"), "{e}");
    }
}
