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
use std::path::{Component, Path, PathBuf};

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

/// Whether a failed `fchmod` of the temporary file is let go. The file was
/// created with `perm` less the umask, never wider than intended, so where
/// the filesystem does not take a mode change (some FUSE mounts: not
/// supported, not implemented, not permitted) the restore goes on with the
/// mode the open gave. Any other error fails the write, as before. (On
/// Linux `ENOTSUP` is `EOPNOTSUPP`.)
fn fchmod_is_ignorable(e: Errno) -> bool {
    matches!(e, Errno::EOPNOTSUPP | Errno::ENOSYS | Errno::EPERM)
}

/// The permission bits a file written at `name` in `dir` gets. A regular
/// file there now keeps its bits (a `.env` at 0600 stays 0600; git records
/// nothing but the executable bit, so the snapshot cannot say more), with
/// the executable bits taken from the snapshot: when `exec`, those who can read
/// the old file plus those who could already execute it; cleared otherwise. Set-user-id, set-group-id
/// and sticky are dropped. Nothing there (the turn deleted it), a link, a
/// folder or anything else gives nothing to keep: the old default, `0666`
/// or `0777` less the umask. Read with `fstatat` on the directory
/// descriptor, never following a link. If it cannot be read, the owner's
/// bits of that default only: never wider than what it would have been.
fn restored_perm(dir: &OwnedFd, name: &OsStr, exec: bool) -> u32 {
    let default = if exec { 0o777 } else { 0o666 } & !umask();
    match nix::sys::stat::fstatat(dir, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(st) if SFlag::from_bits_truncate(st.st_mode) & SFlag::S_IFMT == SFlag::S_IFREG => {
            let old = st.st_mode & 0o777;
            (old & 0o666)
                | if exec {
                    ((old & 0o444) >> 2) | (old & 0o111)
                } else {
                    0
                }
        }
        Ok(_) | Err(Errno::ENOENT) => default,
        Err(_) => default & 0o700,
    }
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

    /// What a write (`write`) or a delete of `rel` would remove that is not
    /// a folder, walked as [`Self::write_from`] and [`Self::delete`] walk
    /// (no link followed): the file or link at `rel`, and for a write also
    /// a file or link where a folder on the way to it goes (the first one;
    /// nothing beyond it is reached). A folder at `rel` is not named here.
    pub(super) fn in_the_way(&self, rel: &Path, write: bool) -> Result<Vec<PathBuf>, String> {
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
        let mut at = PathBuf::new();
        for d in dirs {
            at.push(d);
            match nix::fcntl::openat(&cur, *d, dir_flags(), Mode::empty()) {
                Ok(fd) => cur = fd,
                Err(Errno::ENOENT) => return Ok(Vec::new()),
                // A file or a link where a folder goes: a write replaces it,
                // a delete finds the path already gone.
                Err(Errno::ENOTDIR | Errno::ELOOP) => {
                    return Ok(if write { vec![at] } else { Vec::new() });
                }
                Err(e) => return Err(err("cannot look at", rel, e)),
            }
        }
        match kind(&cur, last).map_err(|e| err("cannot look at", rel, e))? {
            None | Some(SFlag::S_IFDIR) => Ok(Vec::new()),
            Some(_) => Ok(vec![rel.to_path_buf()]),
        }
    }

    /// Whether `rel` is reached without passing a link or a file, and is
    /// then a named pipe, a socket or a device (nothing git can stage).
    /// Nothing is opened at `rel` itself; only `fstatat` without following.
    pub(super) fn is_special(&self, rel: &Path) -> Result<bool, String> {
        let Some((dir, name)) = self.parent(rel, false)? else {
            return Ok(false);
        };
        Ok(!matches!(
            kind(&dir, name).map_err(|e| err("cannot look at", rel, e))?,
            None | Some(SFlag::S_IFREG | SFlag::S_IFLNK | SFlag::S_IFDIR)
        ))
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

    /// Remove the directory at `rel` if it is empty. Gone, not a directory
    /// (a link or a file there now) or not empty: left as it is.
    pub(super) fn remove_empty_dir(&self, rel: &Path) -> Result<(), String> {
        let Some((dir, name)) = self.parent(rel, false)? else {
            return Ok(());
        };
        match nix::unistd::unlinkat(&dir, name, UnlinkatFlags::RemoveDir) {
            Ok(()) | Err(Errno::ENOENT | Errno::ENOTDIR | Errno::ENOTEMPTY | Errno::EEXIST) => {
                Ok(())
            }
            Err(e) => Err(err("could not remove the folder", rel, e)),
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
            let perm = restored_perm(&dir, name, mode == "100755");
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
            // The mode as chosen, not as the umask leaves it: what was kept
            // from the old file stays whatever the umask is. On the open
            // descriptor, before any content is in it.
            if let Err(e) = nix::sys::stat::fchmod(&fd, Mode::from_bits_truncate(perm))
                && !fchmod_is_ignorable(e)
            {
                let _ = nix::unistd::unlinkat(&dir, tmp.as_str(), UnlinkatFlags::NoRemoveDir);
                return Err(err("cannot write", rel, e));
            }
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

    fn mode_of(p: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::symlink_metadata(p).unwrap().permissions().mode() & 0o7777
    }

    fn put(p: &Path, text: &str, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(p, text).unwrap();
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    /// A failed `fchmod` is let go only where the filesystem does not take a
    /// mode change; everything else still fails the write.
    #[test]
    fn only_a_filesystem_that_takes_no_mode_change_lets_fchmod_fail() {
        for e in [
            Errno::EOPNOTSUPP,
            Errno::ENOTSUP,
            Errno::ENOSYS,
            Errno::EPERM,
        ] {
            assert!(fchmod_is_ignorable(e), "{e}");
        }
        for e in [
            Errno::EACCES,
            Errno::EIO,
            Errno::EROFS,
            Errno::EBADF,
            Errno::ENOSPC,
            Errno::EINVAL,
        ] {
            assert!(!fchmod_is_ignorable(e), "{e}");
        }
    }

    /// Task-5 notes of the trial, 2026-11-19: a restore that replaces a file
    /// keeps the file's permission bits (a `.env` at 0600 stays 0600); the
    /// snapshot's mode only says whether it is executable.
    #[test]
    fn a_replaced_file_keeps_its_permission_bits() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        let top = Top::open(&root).unwrap();
        for (old, git_mode, want) in [
            (0o600, "100644", 0o600),
            (0o640, "100644", 0o640),
            (0o664, "100644", 0o664),
            (0o444, "100644", 0o444),
            // The executable bits follow the snapshot, both ways, and only
            // for those who can already read the file.
            (0o644, "100755", 0o755),
            (0o600, "100755", 0o700),
            (0o640, "100755", 0o750),
            (0o755, "100644", 0o644),
            (0o700, "100644", 0o600),
            (0o750, "100644", 0o640),
            // Execute bits the old file had are kept when the snapshot is
            // executable, even for one that could not read it; cleared when
            // it is not.
            (0o100, "100755", 0o100),
            (0o710, "100755", 0o710),
            (0o100, "100644", 0o000),
            // The sticky bit is not carried over: only this code decides it
            // (set-user-id is not shown here: the kernel strips it when an
            // unprivileged user writes the file, so a mutant that kept it
            // would pass).
            (0o1644, "100644", 0o644),
            (0o1755, "100755", 0o755),
        ] {
            let f = root.join("f");
            put(&f, "turn\n", old);
            top.write(Path::new("f"), git_mode, b"back\n").unwrap();
            if want & 0o400 != 0 {
                assert_eq!(std::fs::read_to_string(&f).unwrap(), "back\n");
            }
            assert_eq!(mode_of(&f), want, "{old:o} + {git_mode}: {:o}", mode_of(&f));
            std::fs::remove_file(&f).unwrap();
        }
    }

    /// Nothing at the path (the turn deleted the file): today's mode, from
    /// the snapshot and the umask. A link or a folder there gives nothing to
    /// keep either.
    #[test]
    fn nothing_to_keep_gives_the_default_mode() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        let top = Top::open(&root).unwrap();
        let um = umask();
        top.write(Path::new("a"), "100644", b"x").unwrap();
        top.write(Path::new("b"), "100755", b"x").unwrap();
        assert_eq!(mode_of(&root.join("a")), 0o666 & !um);
        assert_eq!(mode_of(&root.join("b")), 0o777 & !um);
        // A link whose target is 0600: the new file does not take the
        // target's mode (the link is not followed and is replaced).
        put(&root.join("t"), "t", 0o600);
        std::os::unix::fs::symlink("t", root.join("l")).unwrap();
        top.write(Path::new("l"), "100644", b"x").unwrap();
        assert_eq!(mode_of(&root.join("l")), 0o666 & !um);
        assert_eq!(mode_of(&root.join("t")), 0o600);
        // A folder, replaced.
        std::fs::create_dir(root.join("dd")).unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(root.join("dd"), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        top.write(Path::new("dd"), "100644", b"x").unwrap();
        assert_eq!(mode_of(&root.join("dd")), 0o666 & !um);
    }

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
