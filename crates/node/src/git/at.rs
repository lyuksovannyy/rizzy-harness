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
use std::os::fd::{AsRawFd, OwnedFd};
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

    /// Write `content` at `rel` with git's `mode` (`100644`, `100755` or
    /// `120000` for a link whose target is `content`), replacing whatever
    /// is there (a directory is removed with everything in it, as checkout
    /// does; the plan has refused or named what that loses).
    pub(super) fn write(&self, rel: &Path, mode: &str, content: &[u8]) -> Result<(), String> {
        let (dir, name) = self
            .parent(rel, true)?
            .ok_or_else(|| format!("{}: cannot be written", rel.display()))?;
        if kind(&dir, name).map_err(|e| err("cannot write", rel, e))? == Some(SFlag::S_IFDIR) {
            remove_tree(&dir, name).map_err(|e| err("cannot replace the folder", rel, e))?;
        }
        let tmp = format!(".harness-restore-{}.tmp", harness_core::new_id());
        if mode == "120000" {
            use std::os::unix::ffi::OsStrExt;
            let target = OsStr::from_bytes(content);
            nix::unistd::symlinkat(target, &dir, tmp.as_str())
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
            let written = std::io::Write::write_all(&mut f, content).and_then(|_| f.sync_data());
            if let Err(e) = written {
                let _ = nix::unistd::unlinkat(&dir, tmp.as_str(), UnlinkatFlags::NoRemoveDir);
                return Err(format!("cannot write {}: {e}", rel.display()));
            }
        }
        nix::fcntl::renameat(&dir, tmp.as_str(), &dir, name).map_err(|e| {
            let _ = nix::unistd::unlinkat(&dir, tmp.as_str(), UnlinkatFlags::NoRemoveDir);
            err("cannot write", rel, e)
        })
    }
}

/// Remove the directory `name` in `dir` and everything in it, never
/// following a link (entries are listed through the opened directory).
fn remove_tree(dir: &OwnedFd, name: &OsStr) -> Result<(), Errno> {
    let sub = nix::fcntl::openat(dir, name, dir_flags(), Mode::empty())?;
    let listed = std::fs::read_dir(format!("/proc/self/fd/{}", sub.as_raw_fd()))
        .map_err(|e| Errno::from_raw(e.raw_os_error().unwrap_or(libc_eio())))?;
    for e in listed {
        let e = e.map_err(|e| Errno::from_raw(e.raw_os_error().unwrap_or(libc_eio())))?;
        let n = e.file_name();
        match kind(&sub, &n)? {
            Some(SFlag::S_IFDIR) => remove_tree(&sub, &n)?,
            Some(_) => nix::unistd::unlinkat(&sub, n.as_os_str(), UnlinkatFlags::NoRemoveDir)?,
            None => {}
        }
    }
    nix::unistd::unlinkat(dir, name, UnlinkatFlags::RemoveDir)
}

fn libc_eio() -> i32 {
    Errno::EIO as i32
}
