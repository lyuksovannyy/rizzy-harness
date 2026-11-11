//! A session's folder, pinned when the session is created (R.1, PLAN.md T4).
//!
//! A folder is trusted by its path, and a path can be made to lead
//! elsewhere: a process that may write the directory holding the folder
//! renames it away and puts a symlink (or another directory) in its place,
//! and from then on every tool that resolves the path again works in the
//! other folder. So the folder is pinned once, by its canonical path and by
//! the identity of the directory there (device and inode), and every use
//! checks both first: [`FolderPin::check`] is the one place that does.
//!
//! What it cannot close: the path is resolved again by whatever uses it
//! after the check (a `chdir`, an `open`), so a swap in the window between
//! the two still works once. That window is small, and the sandbox gives
//! the agent's own shell no write access to any directory that holds a
//! session's folder; it remains open to unsandboxed processes of the same
//! OS user (PLAN.md T4).

use std::path::{Path, PathBuf};

/// The first words of every refusal: the folder is no longer the one the
/// session was created in.
pub const REPLACED: &str = "this session's folder was replaced";

/// A session folder by canonical path and directory identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderPin {
    path: PathBuf,
    dev: u64,
    ino: u64,
}

impl FolderPin {
    /// Pin the directory at `path`, which must already be canonical (what
    /// the node's policy check returned, or what the log recorded): a path
    /// that resolves anywhere else, or is not a directory, is refused.
    pub fn at(path: &Path) -> Result<Self, String> {
        let real = path
            .canonicalize()
            .map_err(|e| format!("{REPLACED} or removed ({}: {e})", path.display()))?;
        if real != path {
            return Err(format!(
                "{REPLACED}: {} now leads to {}",
                path.display(),
                real.display()
            ));
        }
        let (dev, ino) = identity(&real)?;
        Ok(Self {
            path: real,
            dev,
            ino,
        })
    }

    /// The pinned path (canonical).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The pinned directory's identity (for a sandbox grant).
    pub fn identity(&self) -> Identity {
        Identity {
            dev: self.dev,
            ino: self.ino,
        }
    }

    /// The folder's canonical path, if it is still the pinned directory:
    /// the path canonicalises to itself (no link anywhere on it leads
    /// elsewhere) and the directory there has the same device and inode.
    /// Otherwise an error that starts with [`REPLACED`]; nothing may then be
    /// read, written or granted there.
    pub fn check(&self) -> Result<PathBuf, String> {
        let again = Self::at(&self.path)?;
        if again != *self {
            return Err(format!(
                "{REPLACED}: {} is not the directory this session was created in; \
                 start a new chat in the folder",
                self.path.display()
            ));
        }
        Ok(again.path)
    }
}

/// What a directory is, independent of the path that reaches it: device
/// and inode (R.1, R1c-1). A sandbox grant carries the identity the node
/// expects, and the wrapper compares it with the descriptor it opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Identity {
    pub dev: u64,
    pub ino: u64,
}

impl Identity {
    /// The identity of what is at `path` now, not following a link at its
    /// last component. For folders the node itself made, or pinned and
    /// checked a moment ago.
    pub fn of(path: &Path) -> Result<Self, String> {
        let (dev, ino) = identity_any(path)?;
        Ok(Self { dev, ino })
    }
}

#[cfg(unix)]
fn identity_any(p: &Path) -> Result<(u64, u64), String> {
    use std::os::unix::fs::MetadataExt;
    let md = std::fs::symlink_metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok((md.dev(), md.ino()))
}

#[cfg(not(unix))]
fn identity_any(p: &Path) -> Result<(u64, u64), String> {
    std::fs::symlink_metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok((0, 0))
}

/// A folder the node made for one session only (a sub-agent's folder under
/// the node's sessions folder, which is protected like the data
/// directory): the session may work in it although it lies in a protected
/// folder, and nothing else may (R.1 follow-up, PLAN.md T32).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedFolder {
    /// The folder's top (canonical); the session's folder is it or inside.
    pub top: PathBuf,
    pub top_id: Identity,
    /// Its own git directory (`<top>/.git`), which the session's shell may
    /// write; the node never runs git with it.
    pub git: PathBuf,
    pub git_id: Identity,
    /// The user's object store that git directory borrows from, read-only
    /// to the session's shell, with the identity it had when the node
    /// accepted it for borrowing (`None`: none). A store replaced since is
    /// not granted: the child loses history, never gains another folder.
    pub objects: Option<(PathBuf, Identity)>,
}

#[cfg(unix)]
fn identity(dir: &Path) -> Result<(u64, u64), String> {
    use std::os::unix::fs::MetadataExt;
    // The path is canonical, so this is the directory itself, not a link.
    let md = std::fs::symlink_metadata(dir)
        .map_err(|e| format!("{REPLACED} or removed ({}: {e})", dir.display()))?;
    if !md.is_dir() {
        return Err(format!("{REPLACED}: {} is not a directory", dir.display()));
    }
    Ok((md.dev(), md.ino()))
}

#[cfg(not(unix))]
fn identity(dir: &Path) -> Result<(u64, u64), String> {
    if !dir.is_dir() {
        return Err(format!("{REPLACED}: {} is not a directory", dir.display()));
    }
    Ok((0, 0))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_folder_replaced_by_a_link_or_a_new_directory_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let top = d.path().canonicalize().unwrap();
        let ws = top.join("ws");
        let other = top.join("other");
        std::fs::create_dir(&ws).unwrap();
        std::fs::create_dir(&other).unwrap();
        let pin = FolderPin::at(&ws).unwrap();
        assert_eq!(pin.check().unwrap(), ws);
        // Renamed away and a link put in its place.
        std::fs::rename(&ws, top.join("ws.r")).unwrap();
        std::os::unix::fs::symlink(&other, &ws).unwrap();
        assert!(pin.check().unwrap_err().starts_with(REPLACED));
        // A new directory of the same name.
        std::fs::remove_file(&ws).unwrap();
        std::fs::create_dir(&ws).unwrap();
        assert!(pin.check().unwrap_err().starts_with(REPLACED));
        // Gone.
        std::fs::remove_dir(&ws).unwrap();
        assert!(pin.check().unwrap_err().starts_with(REPLACED));
        // A path that is not canonical is never pinned.
        std::os::unix::fs::symlink(&other, top.join("link")).unwrap();
        assert!(FolderPin::at(&top.join("link")).is_err());
        assert!(FolderPin::at(&top.join("other/../other")).is_err());
    }
}
