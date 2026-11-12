//! `<data>/lock`: one process per data directory (ADOPTION_PLAN A.20).
//!
//! `serve`, `node run` and `node join` hold an exclusive lock on this file
//! (`flock` through `File::try_lock`) from right after the data directory
//! is resolved until the process ends, before anything else touches the
//! directory: the start-up sweep, the emptied temp root, the removed
//! sub-agents' folder and the closing of interrupted turns all assume no
//! other process uses it. The kernel drops the lock when the process dies
//! in any way (`kill -9` included), so a crash leaves nothing stale.
//!
//! The file holds `<pid> <role>`, written after the lock is taken, for the
//! refusal message and `doctor` only: never trusted for anything (a pid
//! from a dead holder is just a wrong hint). The file itself is never
//! removed: removing a lock file races (a process that opened the old file
//! would hold a lock on a name nobody else opens any more).

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read, Seek, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, bail};

/// The lock file's name in the data directory.
pub const NAME: &str = "lock";

/// A held lock: dropped (and released) when the process ends.
pub struct DirLock {
    _file: File,
}

/// What a lock attempt came to.
#[derive(Debug)]
enum Taken {
    Held,
    /// The file system has no file locks: the process runs unprotected.
    Unsupported(std::io::Error),
}

/// How often and how long a start retries a lock that is held: `doctor`
/// holds a shared lock for the moment it checks, and a start in that
/// moment must not be refused for it.
const RETRIES: u32 = 10;
const RETRY_EVERY: Duration = Duration::from_millis(20);

/// Take the lock for `role` (`serve`, `node run`, `node join`) or end with
/// the one-line refusal naming the directory and the holder.
pub fn take(dir: &Path, role: &str) -> anyhow::Result<DirLock> {
    take_with(dir, role, |f| f.try_lock())
}

fn take_with(
    dir: &Path,
    role: &str,
    mut try_lock: impl FnMut(&File) -> Result<(), TryLockError>,
) -> anyhow::Result<DirLock> {
    let path = dir.join(NAME);
    // Not truncated before the lock is ours (the holder's hint is in it);
    // never through a link (the directory is this user's own and 0700, so
    // only this user could plant one, but there is no reason to follow it).
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(&path)
        .with_context(|| format!("opening the data directory's lock {}", path.display()))?;
    let mut tries = 0;
    let taken = loop {
        match try_lock(&file) {
            Ok(()) => break Taken::Held,
            Err(TryLockError::WouldBlock) if tries < RETRIES => {
                tries += 1;
                std::thread::sleep(RETRY_EVERY);
            }
            Err(TryLockError::WouldBlock) => {
                let mut text = String::new();
                let _ = file.read_to_string(&mut text);
                bail!(
                    "{} is in use by {}: one data directory serves one process; stop that \
                     one first, or give this one its own --data-dir",
                    dir.display(),
                    holder(&text)
                );
            }
            Err(TryLockError::Error(e)) if unsupported(&e) => break Taken::Unsupported(e),
            Err(TryLockError::Error(e)) => {
                return Err(e).with_context(|| format!("locking {}", path.display()));
            }
        }
    };
    if let Taken::Unsupported(e) = &taken {
        tracing::warn!(
            "{}: this file system does not support file locks ({e}); starting without the \
             lock, so nothing stops a second process on this data directory: start only one",
            path.display()
        );
    }
    // The hint for a second start and `doctor`; a failure here changes
    // nothing about the lock.
    let _ = file
        .set_len(0)
        .and_then(|()| file.rewind())
        .and_then(|()| writeln!(file, "{} {}", std::process::id(), role.replace(' ', "-")));
    Ok(DirLock { _file: file })
}

/// `ENOLCK`, `EOPNOTSUPP`, `ENOSYS`: the file system has no locks (some
/// network and FUSE file systems), as opposed to a lock someone holds.
fn unsupported(e: &std::io::Error) -> bool {
    use nix::errno::Errno;
    e.kind() == std::io::ErrorKind::Unsupported
        || [Errno::ENOLCK, Errno::EOPNOTSUPP, Errno::ENOSYS]
            .iter()
            .any(|n| e.raw_os_error() == Some(*n as i32))
}

/// The holder in words, from the file's hint: `serve (pid 1234)`, or
/// `another process` when the hint cannot be read. Only a number and a
/// known role are printed, never the file's text.
fn holder(text: &str) -> String {
    let mut it = text.split_whitespace();
    let pid = it.next().and_then(|p| p.parse::<u32>().ok());
    let role = match it.next() {
        Some("serve") => "serve",
        Some("node-run") => "node run",
        Some("node-join") => "node join",
        _ => "another process",
    };
    match pid {
        Some(pid) => format!("{role} (pid {pid})"),
        None => role.to_string(),
    }
}

/// What `doctor` found, in words.
pub enum Status {
    Free(String),
    Held(String),
    Unsupported(String),
}

/// For `doctor`: whether a process holds the lock now, and who, without
/// taking it for longer than the check (a shared lock, released at once;
/// a start in that moment retries) and without making the file.
pub fn status(dir: &Path) -> Result<Status, String> {
    let path: PathBuf = dir.join(NAME);
    let mut file = match OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(&path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Status::Free(
                "free (no server or node has run on this directory yet)".into(),
            ));
        }
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    match file.try_lock_shared() {
        Ok(()) => {
            let _ = file.unlock();
            Ok(Status::Free(
                "free (no server or node runs on this directory now)".into(),
            ))
        }
        Err(TryLockError::WouldBlock) => {
            let mut text = String::new();
            let _ = file.read_to_string(&mut text);
            Ok(Status::Held(format!(
                "held by {}: a second server or node on this directory is refused",
                holder(&text)
            )))
        }
        Err(TryLockError::Error(e)) if unsupported(&e) => Ok(Status::Unsupported(format!(
            "{}: this file system does not support file locks ({e}): nothing stops a second \
             process on this data directory",
            path.display()
        ))),
        Err(TryLockError::Error(e)) => Err(format!("{}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file system without locks (`ENOLCK`, as NFS without a lock
    /// manager answers): the start goes on, with the warning, and the hint
    /// is written; any other error refuses.
    #[test]
    fn a_file_system_without_locks_starts_with_the_warning() {
        let dir = tempfile::tempdir().unwrap();
        for errno in [nix::errno::Errno::ENOLCK, nix::errno::Errno::EOPNOTSUPP] {
            let lock = take_with(dir.path(), "serve", |_| {
                Err(TryLockError::Error(std::io::Error::from_raw_os_error(
                    errno as i32,
                )))
            });
            assert!(lock.is_ok(), "{errno}: {:?}", lock.err());
            let text = std::fs::read_to_string(dir.path().join(NAME)).unwrap();
            assert_eq!(text, format!("{} serve\n", std::process::id()));
        }
        let e = take_with(dir.path(), "serve", |_| {
            Err(TryLockError::Error(std::io::Error::from_raw_os_error(
                nix::errno::Errno::EIO as i32,
            )))
        });
        assert!(e.is_err());
    }

    /// Held by another open file (as by another process): refused with the
    /// directory and the holder's hint; the hint is left as it was.
    #[test]
    fn a_held_lock_is_refused_naming_the_holder() {
        let dir = tempfile::tempdir().unwrap();
        let first = take(dir.path(), "node run").unwrap();
        let e = take(dir.path(), "serve").err().unwrap().to_string();
        assert!(e.contains(&dir.path().display().to_string()), "{e}");
        assert!(
            e.contains(&format!("node run (pid {})", std::process::id())),
            "{e}"
        );
        assert!(
            matches!(status(dir.path()), Ok(Status::Held(m)) if m.starts_with("held by node run"))
        );
        drop(first);
        assert!(matches!(status(dir.path()), Ok(Status::Free(_))));
        let _again = take(dir.path(), "serve").unwrap();
    }

    /// The hint is only ever a number and a known role.
    #[test]
    fn the_hint_prints_nothing_of_the_files_text() {
        assert_eq!(holder("12 serve\n"), "serve (pid 12)");
        assert_eq!(holder("x\x1b[2J evil"), "another process");
        assert_eq!(holder("12 \x1b[2J"), "another process (pid 12)");
        assert_eq!(holder(""), "another process");
    }

    /// A link at the lock's name is not followed.
    #[test]
    fn a_link_at_the_locks_name_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("elsewhere");
        std::fs::write(&target, "x").unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join(NAME)).unwrap();
        assert!(take(dir.path(), "serve").is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "x");
    }
}
