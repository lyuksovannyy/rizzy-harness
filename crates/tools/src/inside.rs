//! Reading a file the harness itself puts in front of the model (the
//! repository's instruction files, skills): only a regular file whose real
//! path is inside a given folder and in no protected folder, opened without
//! following a link at its end or waiting on a FIFO, and checked again on
//! the opened descriptor (PLAN.md T44, T46).

use std::io::Read as _;
use std::path::{Path, PathBuf};

/// Why a file is not read.
pub const OUTSIDE: &str = "it leads outside the folder";

/// At most `cap + 1` bytes of the regular file `path` leads to, when its
/// real path is inside `bound` and in no `protected` folder (one that holds
/// `bound` itself does not count: a sub-agent's own folder). The real path
/// is opened without following a link at its end or waiting on a FIFO, and
/// the descriptor is checked again, so a folder swapped for a link after
/// the first check is refused.
pub fn read_inside(
    path: &Path,
    bound: &Path,
    protected: &[PathBuf],
    cap: usize,
) -> Result<Vec<u8>, String> {
    use nix::fcntl::OFlag;
    use std::os::unix::fs::OpenOptionsExt;
    let inside = |p: &Path| is_inside(p, bound, protected);
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
    if !inside(&real_path(&f, &real).map_err(unreadable)?) {
        return Err(OUTSIDE.into());
    }
    let mut out = Vec::new();
    f.take(cap as u64 + 1)
        .read_to_end(&mut out)
        .map_err(unreadable)?;
    Ok(out)
}

/// `p` (canonical) is inside `bound` and in no `protected` folder that
/// does not hold `bound` itself.
pub fn is_inside(p: &Path, bound: &Path, protected: &[PathBuf]) -> bool {
    p.starts_with(bound)
        && !protected
            .iter()
            .any(|x| p.starts_with(x) && !bound.starts_with(x))
}

/// The path an open file has now.
#[cfg(target_os = "linux")]
fn real_path(f: &std::fs::File, _path: &Path) -> std::io::Result<PathBuf> {
    use std::os::fd::AsRawFd;
    std::fs::read_link(format!("/proc/self/fd/{}", f.as_raw_fd()))
}

#[cfg(not(target_os = "linux"))]
fn real_path(f: &std::fs::File, path: &Path) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let (a, b) = (f.metadata()?, std::fs::symlink_metadata(path)?);
    if a.dev() != b.dev() || a.ino() != b.ino() {
        return Err(std::io::Error::other(
            "the file changed while it was opened",
        ));
    }
    path.canonicalize()
}
