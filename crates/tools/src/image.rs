//! The node's link to its own running image (ADOPTION_PLAN T.8f; review
//! note T8C-N3).
//!
//! A contained command's view holds the node's running image at
//! [`crate::contain::VIEW_EXE`], bound from a descriptor through the path
//! that descriptor names. A binary replaced under the running node (a
//! `cargo build`, a package upgrade renames a new file over it) leaves the
//! image without a path, and binding whatever is at the path now would bind
//! what a session could have put there (review T8E-5), so T.8c refuses
//! every contained command until the node restarts.
//!
//! At start the node gives its image a second name, `<data dir>/node-image`
//! ([`LINK`]): a hard link made through the descriptor the node holds
//! (`linkat("/proc/self/fd/<n>", …, AT_SYMLINK_FOLLOW)`, which links the
//! file the descriptor holds, never the file now at the binary's path, and
//! which the kernel refuses for a file that has no name left), then checked
//! by device and inode against that descriptor. The view binds the link;
//! the guard checks it against its own `/proc/self/exe` before every
//! command and refuses on a difference ([`crate::guard`]). The data
//! directory is in no grant (`Sandbox::new` and every session's grants are
//! checked against it), so no command can replace the link.
//!
//! Where it cannot be made (the data directory on another file system than
//! the binary: `EXDEV`; a binary that is not the node user's own file with
//! `fs.protected_hardlinks` on: `EPERM`; a data directory that cannot be
//! written), nothing is left at the name and the view is T.8c's, said in
//! the `file view` line of the log and of `doctor`. The binary is never
//! copied.
//!
//! The name is replaced at every start (made under a temporary name,
//! checked, renamed over whatever is there; a folder in its place is
//! removed; nothing found there is followed). What a link left by an
//! earlier start points at never matters: the check is what decides.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use nix::errno::Errno;

/// The link's one fixed name, in the data directory.
pub const LINK: &str = "node-image";

/// What became of the link at start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageLink {
    /// Made, and checked to be the image this process runs.
    Linked(PathBuf),
    /// `doctor`'s trial: a link could be made here (it was, under another
    /// name, and removed), at this path.
    Possible(PathBuf),
    /// Not made, and why, in words for the log.
    NotMade(String),
}

/// Make `<data_dir>/node-image` a hard link to the file `image` (a
/// descriptor this process holds of its own running image) and check it.
/// On any failure nothing is left at the name.
pub fn make(image: std::os::fd::RawFd, data_dir: &Path) -> ImageLink {
    let link = data_dir.join(LINK);
    match place(image, data_dir, &link) {
        Ok(()) => ImageLink::Linked(link),
        Err(why) => {
            let _ = remove(&link);
            ImageLink::NotMade(why)
        }
    }
}

/// `doctor`'s trial of [`make`]: the same steps under a name of its own,
/// removed again, so a running node's link is never touched.
pub fn trial(image: std::os::fd::RawFd, data_dir: &Path) -> ImageLink {
    let name = data_dir.join(format!("{LINK}.doctor-{}", std::process::id()));
    let r = place(image, data_dir, &name);
    let _ = remove(&name);
    match r {
        Ok(()) => ImageLink::Possible(data_dir.join(LINK)),
        Err(why) => ImageLink::NotMade(why),
    }
}

/// The device and inode of a file, not following a link at its name.
fn id_of(m: &std::fs::Metadata) -> (u64, u64) {
    (m.dev(), m.ino())
}

fn place(image: std::os::fd::RawFd, data_dir: &Path, at: &Path) -> Result<(), String> {
    let held = format!("/proc/self/fd/{image}");
    // The file the descriptor holds (the magic link followed).
    let want = std::fs::metadata(&held).map_err(|e| format!("its own image: {e}"))?;
    if want.nlink() == 0 {
        return Err(
            "the harness binary was deleted or replaced since the node started (its file has \
             no name left to link)"
                .into(),
        );
    }
    let tmp = data_dir.join(format!("{LINK}.new-{}", std::process::id()));
    remove(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    nix::unistd::linkat(
        nix::fcntl::AT_FDCWD,
        held.as_str(),
        nix::fcntl::AT_FDCWD,
        &tmp,
        nix::fcntl::AtFlags::AT_SYMLINK_FOLLOW,
    )
    .map_err(|e| explain(e, data_dir))?;
    let r = settle(&tmp, at, id_of(&want));
    if r.is_err() {
        let _ = remove(&tmp);
    }
    r
}

/// Check the new link, rename it over whatever is at `at` (a folder there
/// is removed first), and check `at`.
fn settle(tmp: &Path, at: &Path, want: (u64, u64)) -> Result<(), String> {
    check(tmp, want)?;
    if let Err(e) = std::fs::rename(tmp, at) {
        // A folder in the way (EISDIR, ENOTEMPTY, EEXIST): not followed,
        // removed with what is inside.
        if !std::fs::symlink_metadata(at).is_ok_and(|m| m.is_dir()) {
            return Err(format!("{}: {e}", at.display()));
        }
        remove(at).map_err(|e| format!("{}: {e}", at.display()))?;
        std::fs::rename(tmp, at).map_err(|e| format!("{}: {e}", at.display()))?;
    }
    check(at, want)
}

/// `p` is a regular file (not a link to one) with the identity `want`.
fn check(p: &Path, want: (u64, u64)) -> Result<(), String> {
    let m = std::fs::symlink_metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
    if !m.file_type().is_file() || id_of(&m) != want {
        return Err(format!(
            "{} is not the image this node runs (it was replaced while the link was made)",
            p.display()
        ));
    }
    Ok(())
}

/// Remove what is at `p` without following it: a folder with what is in
/// it, anything else by its name. Nothing there is not an error.
fn remove(p: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(p) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        // `remove_dir_all` does not follow links inside.
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(p),
        Ok(_) => std::fs::remove_file(p),
    }
}

fn explain(e: Errno, data_dir: &Path) -> String {
    let d = data_dir.display();
    match e {
        Errno::EXDEV => format!(
            "the data directory {d} is on another file system than the harness binary (a hard \
             link cannot cross file systems)"
        ),
        Errno::EPERM => "the harness binary is not this user's own file and \
                         fs.protected_hardlinks is on (an installed binary)"
            .into(),
        Errno::EACCES | Errno::EROFS => format!("the data directory {d} cannot be written ({e})"),
        Errno::ENOENT => {
            "the harness binary was deleted or replaced since the node started (ENOENT)".into()
        }
        e => format!("linking the harness binary into {d}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;

    fn held(p: &Path) -> std::fs::File {
        std::fs::File::open(p).unwrap()
    }

    #[test]
    fn links_the_held_file_and_replaces_what_is_there() {
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("bin");
        std::fs::write(&bin, "image").unwrap();
        let f = held(&bin);
        let other = d.path().join("other");
        std::fs::write(&other, "other").unwrap();
        // A symlink to another file: replaced, the target untouched.
        std::os::unix::fs::symlink(&other, d.path().join(LINK)).unwrap();
        assert_eq!(
            make(f.as_raw_fd(), d.path()),
            ImageLink::Linked(d.path().join(LINK))
        );
        let l = std::fs::symlink_metadata(d.path().join(LINK)).unwrap();
        assert!(l.file_type().is_file());
        assert_eq!(l.ino(), std::fs::metadata(&bin).unwrap().ino());
        assert_eq!(std::fs::read_to_string(&other).unwrap(), "other");
        // A folder in its place, with something inside.
        std::fs::remove_file(d.path().join(LINK)).unwrap();
        std::fs::create_dir_all(d.path().join(LINK).join("in")).unwrap();
        assert!(matches!(
            make(f.as_raw_fd(), d.path()),
            ImageLink::Linked(_)
        ));
        assert!(
            std::fs::symlink_metadata(d.path().join(LINK))
                .unwrap()
                .is_file()
        );
        // Nothing but the link is left.
        let names: Vec<_> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 3, "{names:?}");
    }

    /// Replaced by a rename after it was opened: the kernel refuses to link
    /// a file with no name left, and the file now at the path is not used.
    #[test]
    fn a_replaced_file_is_not_linked_and_an_old_link_goes() {
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("bin");
        std::fs::write(&bin, "image").unwrap();
        let f = held(&bin);
        std::fs::write(d.path().join(LINK), "an old build").unwrap();
        std::fs::write(d.path().join("next"), "next").unwrap();
        std::fs::rename(d.path().join("next"), &bin).unwrap();
        let ImageLink::NotMade(why) = make(f.as_raw_fd(), d.path()) else {
            panic!("linked")
        };
        assert!(why.contains("replaced"), "{why}");
        assert!(std::fs::symlink_metadata(d.path().join(LINK)).is_err());
    }

    /// The identity check itself: a link to another file than the one held
    /// (what a link by path would make after a replacement) is refused and
    /// removed.
    #[test]
    fn a_link_to_another_file_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("bin");
        std::fs::write(&bin, "image").unwrap();
        let want = id_of(&std::fs::metadata(&bin).unwrap());
        let tmp = d.path().join("tmp");
        std::fs::write(&tmp, "another").unwrap();
        let at = d.path().join(LINK);
        let e = settle(&tmp, &at, want).unwrap_err();
        assert!(e.contains("not the image"), "{e}");
        assert!(std::fs::symlink_metadata(&at).is_err());
    }

    #[test]
    fn the_trial_leaves_the_name_alone() {
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("bin");
        std::fs::write(&bin, "image").unwrap();
        let f = held(&bin);
        std::fs::write(d.path().join(LINK), "a running node's").unwrap();
        assert_eq!(
            trial(f.as_raw_fd(), d.path()),
            ImageLink::Possible(d.path().join(LINK))
        );
        assert_eq!(
            std::fs::read_to_string(d.path().join(LINK)).unwrap(),
            "a running node's"
        );
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 2);
    }
}
