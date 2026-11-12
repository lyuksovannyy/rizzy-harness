//! The node's link to its own running image (ADOPTION_PLAN T.8f; review
//! note T8C-N3).
//!
//! Stub: the API the tests are written against. Not built yet.

use std::path::{Path, PathBuf};

/// The link's one fixed name, in the data directory.
pub const LINK: &str = "node-image";

/// What became of the link at start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageLink {
    /// Made, and checked to be the image this process runs.
    Linked(PathBuf),
    /// `doctor`'s trial: a link could be made here (it was, under another
    /// name, and removed).
    Possible(PathBuf),
    /// Not made, and why, in words for the log.
    NotMade(String),
}

/// Make `<data_dir>/node-image` a hard link to the file `image` (a
/// descriptor this process holds of its own running image).
pub fn make(_image: std::os::fd::RawFd, _data_dir: &Path) -> ImageLink {
    ImageLink::NotMade("not built".into())
}

/// `doctor`'s trial of [`make`].
pub fn trial(_image: std::os::fd::RawFd, _data_dir: &Path) -> ImageLink {
    ImageLink::NotMade("not built".into())
}
