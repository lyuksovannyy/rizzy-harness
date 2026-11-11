//! Writing and deleting a restore's files relative to directories opened
//! without following links (R.1 follow-up, PLAN.md T33).
//!
//! The folder a restore (an undo, or a sub-agent's result) writes into can
//! be changed by a process the session left running while the node writes.
//! A path checked one moment can lead through a link the next, so nothing
//! here resolves a path again: the folder's top is opened once
//! (`O_DIRECTORY | O_NOFOLLOW`; its parent is in no session's grant, so it
//! cannot be renamed or replaced), and every further step is `openat` of
//! one component with `O_NOFOLLOW`, then `unlinkat`, `mkdirat`,
//! `symlinkat`, `renameat` in the directory so opened. A link met where a
//! directory is expected is never followed: for a delete the path is
//! already gone; for a write the link is removed and a directory made, as
//! `git checkout-index -f` does. A file is written to a temporary name in
//! its directory and renamed over the old one.

use std::ffi::OsStr;
use std::os::fd::OwnedFd;
use std::path::{Component, Path};

use nix::errno::Errno;
use nix::fcntl::{AtFlags, OFlag};
use nix::sys::stat::{Mode, SFlag};
use nix::unistd::UnlinkatFlags;

fn err(what: &str, p: &Path, e: Errno) -> String {
    format!("{what} {}: {}", p.display(), std::io::Error::from(e))
}

fn dir_flags() -> OFlag {
    OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC
}

/// The process's umask, read without changing it.
fn umask() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("Umask:"))
                .and_then(|v| u32::from_str_radix(v.trim(), 8).ok())
        })
        .unwrap_or(0o022)
}

/// What is at `name` in `dir`, not following a link.
fn kind(dir: &OwnedFd, name: &OsStr) -> Result<Option<SFlag>, Errno> {
    match nix::sys::stat::fstatat(dir, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(st) => Ok(Some(SFlag::from_bits_truncate(st.st_mode) & SFlag::S_IFMT)),
        Err(Errno::ENOENT) => Ok(None),
        Err(e) => Err(e),
    }
}

/// The folder a restore writes into, opened once.
pub(super) struct Top {
    fd: OwnedFd,
}

impl Top {
    pub(super) fn open(root: &Path) -> Result<Self, String> {
        let fd = nix::fcntl::open(root, dir_flags(), Mode::empty())
            .map_err(|e| err("cannot open the folder", root, e))?;
        Ok(Self { fd })
    }

    /// The directory holding `rel` and its last component. `create`: make
    /// missing directories (replacing a file or a link in the way, as
    /// checkout does); otherwise `None` when the path is not there as one.
    fn parent<'a>(
        &self,
        rel: &'a Path,
        create: bool,
    ) -> Result<Option<(OwnedFd, &'a OsStr)>, String> {
        let comps: Vec<&OsStr> = rel
            .components()
            .map(|c| match c {
                Component::Normal(n) => Ok(n),
                _ => Err(format!("{}: not a plain path", rel.display())),
            })
            .collect::<Result<_, _>>()?;
        let Some((last, dirs)) = comps.split_last() else {
            return Err(format!("{}: empty path", rel.display()));
        };
        let mut cur = nix::unistd::dup(&self.fd).map_err(|e| err("dup", rel, e))?;
        for d in dirs {
            match nix::fcntl::openat(&cur, *d, dir_flags(), Mode::empty()) {
                Ok(fd) => cur = fd,
                Err(Errno::ENOENT) if create => {
                    nix::sys::stat::mkdirat(&cur, *d, Mode::from_bits_truncate(0o777))
                        .or_else(|e| if e == Errno::EEXIST { Ok(()) } else { Err(e) })
                        .map_err(|e| err("cannot make the folder for", rel, e))?;
                    cur = nix::fcntl::openat(&cur, *d, dir_flags(), Mode::empty())
                        .map_err(|e| err("cannot open the folder for", rel, e))?;
                }
                // A file or a link where a directory is wanted.
                Err(Errno::ENOTDIR | Errno::ELOOP) if create => {
                    nix::unistd::unlinkat(&cur, *d, UnlinkatFlags::NoRemoveDir)
                        .map_err(|e| err("cannot replace", rel, e))?;
                    nix::sys::stat::mkdirat(&cur, *d, Mode::from_bits_truncate(0o777))
                        .map_err(|e| err("cannot make the folder for", rel, e))?;
                    cur = nix::fcntl::openat(&cur, *d, dir_flags(), Mode::empty())
                        .map_err(|e| err("cannot open the folder for", rel, e))?;
                }
                Err(Errno::ENOENT | Errno::ENOTDIR | Errno::ELOOP) => return Ok(None),
                Err(e) => return Err(err("cannot open the folder for", rel, e)),
            }
        }
        Ok(Some((cur, last)))
    }

    /// Delete the file or link at `rel`. Already gone (missing, a parent no
    /// longer a directory or a link) is fine; a directory there now is
    /// someone else's and left alone.
    pub(super) fn delete(&self, rel: &Path) -> Result<(), String> {
        let Some((dir, name)) = self.parent(rel, false)? else {
            return Ok(());
        };
        match kind(&dir, name).map_err(|e| err("could not delete", rel, e))? {
            None => Ok(()),
            Some(SFlag::S_IFDIR) => Ok(()),
            Some(_) => match nix::unistd::unlinkat(&dir, name, UnlinkatFlags::NoRemoveDir) {
                Ok(()) | Err(Errno::ENOENT) => Ok(()),
                Err(e) => Err(err("could not delete", rel, e)),
            },
        }
    }

    /// Write `size` bytes read from `content` at `rel` with git's `mode`
    /// (`100644`, `100755`, or `120000` for a link whose target is the
    /// content), replacing whatever is there (a directory is removed with
    /// everything in it, as checkout does; the plan has refused or named
    /// what that loses). The bytes are copied through a small buffer, never
    /// held whole (R1b-1); exactly `size` bytes are read, or the write
    /// fails and nothing replaces the old file.
    pub(super) fn write_from(
        &self,
        rel: &Path,
        mode: &str,
        size: u64,
        content: &mut dyn std::io::Read,
    ) -> Result<(), String> {
        let (dir, name) = self
            .parent(rel, true)?
            .ok_or_else(|| format!("{}: cannot be written", rel.display()))?;
        if kind(&dir, name).map_err(|e| err("cannot write", rel, e))? == Some(SFlag::S_IFDIR) {
            remove_tree(&dir, name)
                .map_err(|e| format!("cannot replace the folder {}: {e}", rel.display()))?;
        }
        let tmp = format!(".harness-restore-{}.tmp", harness_core::new_id());
        let short = |n: u64| format!("cannot write {}: {n} of {size} bytes read", rel.display());
        if mode == "120000" {
            use std::os::unix::ffi::OsStrExt;
            if size > LINK_MAX {
                return Err(format!("{}: a link target of {size} bytes", rel.display()));
            }
            let mut target = Vec::with_capacity(size as usize);
            let n = std::io::Read::read_to_end(
                &mut std::io::Read::take(&mut *content, size),
                &mut target,
            )
            .map_err(|e| format!("cannot write {}: {e}", rel.display()))?;
            if n as u64 != size {
                return Err(short(n as u64));
            }
            nix::unistd::symlinkat(OsStr::from_bytes(&target), &dir, tmp.as_str())
                .map_err(|e| err("cannot write", rel, e))?;
        } else {
            let perm = if mode == "100755" { 0o777 } else { 0o666 } & !umask();
            let fd = nix::fcntl::openat(
                &dir,
                tmp.as_str(),
                OFlag::O_WRONLY
                    | OFlag::O_CREAT
                    | OFlag::O_EXCL
                    | OFlag::O_NOFOLLOW
                    | OFlag::O_CLOEXEC,
                Mode::from_bits_truncate(perm),
            )
            .map_err(|e| err("cannot write", rel, e))?;
            let mut f = std::fs::File::from(fd);
            let written = std::io::copy(&mut std::io::Read::take(&mut *content, size), &mut f)
                .and_then(|n| {
                    f.sync_data()?;
                    Ok(n)
                });
            let failed = match written {
                Ok(n) if n == size => None,
                Ok(n) => Some(short(n)),
                Err(e) => Some(format!("cannot write {}: {e}", rel.display())),
            };
            if let Some(e) = failed {
                let _ = nix::unistd::unlinkat(&dir, tmp.as_str(), UnlinkatFlags::NoRemoveDir);
                return Err(e);
            }
        }
        nix::fcntl::renameat(&dir, tmp.as_str(), &dir, name).map_err(|e| {
            let _ = nix::unistd::unlinkat(&dir, tmp.as_str(), UnlinkatFlags::NoRemoveDir);
            err("cannot write", rel, e)
        })
    }

    /// [`Self::write_from`] with the content in memory (tests).
    #[cfg(test)]
    pub(super) fn write(&self, rel: &Path, mode: &str, content: &[u8]) -> Result<(), String> {
        self.write_from(rel, mode, content.len() as u64, &mut &content[..])
    }
}

/// The longest link target a restore writes (Linux's `PATH_MAX`).
const LINK_MAX: u64 = 4096;

/// How deep [`remove_tree`] goes (R1b-4): one descriptor per level is open
/// at a time; a folder nested deeper is left in place and the write fails
/// with the reason, never a recursion without bound.
pub(super) const REMOVE_DEPTH: usize = 128;

/// The names in the directory `fd`, listed through a duplicate of that
/// descriptor (closed when the listing ends), so nothing is resolved by
/// path.
fn names(fd: &OwnedFd) -> Result<Vec<std::ffi::OsString>, String> {
    use std::os::unix::ffi::OsStrExt;
    let dup = nix::unistd::dup(fd).map_err(|e| e.to_string())?;
    let mut d = nix::dir::Dir::from_fd(dup).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for e in d.iter() {
        let e = e.map_err(|e| e.to_string())?;
        let n = e.file_name().to_bytes();
        if n != b"." && n != b".." {
            out.push(OsStr::from_bytes(n).to_os_string());
        }
    }
    Ok(out)
}

/// Remove the directory `name` in `dir` and everything in it, never
/// following a link, iteratively: a stack of one open directory per level
/// (at most [`REMOVE_DEPTH`]), each listed again after a sub-folder in it
/// was removed.
fn remove_tree(dir: &OwnedFd, name: &OsStr) -> Result<(), String> {
    let e = |what: &str, x: Errno| format!("{what}: {}", std::io::Error::from(x));
    let first =
        nix::fcntl::openat(dir, name, dir_flags(), Mode::empty()).map_err(|x| e("open", x))?;
    // (the directory, its name in the level above)
    let mut stack: Vec<(OwnedFd, std::ffi::OsString)> = vec![(first, name.to_os_string())];
    while let Some((cur, _)) = stack.last() {
        let mut sub = None;
        for n in names(cur)? {
            match kind(cur, &n).map_err(|x| e("stat", x))? {
                Some(SFlag::S_IFDIR) => {
                    sub = Some(n);
                    break;
                }
                Some(_) => nix::unistd::unlinkat(cur, n.as_os_str(), UnlinkatFlags::NoRemoveDir)
                    .map_err(|x| e("unlink", x))?,
                None => {}
            }
        }
        match sub {
            Some(n) => {
                if stack.len() >= REMOVE_DEPTH {
                    return Err(format!(
                        "it holds folders nested more than {REMOVE_DEPTH} deep; they were left \
                         in place"
                    ));
                }
                let fd = nix::fcntl::openat(cur, n.as_os_str(), dir_flags(), Mode::empty())
                    .map_err(|x| e("open", x))?;
                stack.push((fd, n));
            }
            None => {
                let (fd, n) = stack.pop().expect("a level");
                drop(fd);
                let parent = stack.last().map_or(dir, |(p, _)| p);
                nix::unistd::unlinkat(parent, n.as_os_str(), UnlinkatFlags::RemoveDir)
                    .map_err(|x| e("remove", x))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R1b-4: a folder nested past the limit is left in place and the write
    /// says why; one within it is removed; no recursion without bound.
    #[test]
    fn a_deeply_nested_folder_is_removed_iteratively_up_to_a_limit() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        let top = Top::open(&root).unwrap();
        for (depth, ok) in [(REMOVE_DEPTH - 1, true), (REMOVE_DEPTH + 5, false)] {
            let mut p = root.join("d");
            for i in 0..depth {
                p.push(format!("{i}"));
            }
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join("f"), "x").unwrap();
            let r = top.write(Path::new("d"), "100644", b"file\n");
            assert_eq!(r.is_ok(), ok, "{depth}: {r:?}");
            if ok {
                assert_eq!(std::fs::read_to_string(root.join("d")).unwrap(), "file\n");
                std::fs::remove_file(root.join("d")).unwrap();
            } else {
                assert!(r.unwrap_err().contains("nested more than"));
                assert!(root.join("d").is_dir());
                std::fs::remove_dir_all(root.join("d")).unwrap();
            }
        }
    }
}
