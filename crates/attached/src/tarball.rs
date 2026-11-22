//! Walking a tar (plain or gzip-compressed) held in memory: entries are
//! read in order through the `tar` crate, whose reader is wrapped so that a
//! compressed tar is never inflated past [`crate::LIST_CEILING`] or the
//! deadline. A link, a device or a pipe is listed as what it is and never
//! followed or opened.

use std::io::{self, Read};
use std::time::Instant;

use crate::{Budget, Entry, EntryKind, Kind, LIST_CEILING, Metered, clean_path};

fn kind_of(t: &tar::Entry<'_, impl Read>) -> EntryKind {
    use tar::EntryType as T;
    let ty = t.header().entry_type();
    match ty {
        T::Regular | T::Continuous | T::GNUSparse => EntryKind::File,
        T::Directory => EntryKind::Dir,
        T::Symlink | T::Link => {
            let to = t
                .link_name_bytes()
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default();
            EntryKind::Link(to)
        }
        T::Char => EntryKind::Other("character device"),
        T::Block => EntryKind::Other("block device"),
        T::Fifo => EntryKind::Other("pipe"),
        _ => EntryKind::Other("special entry"),
    }
}

fn each<'a, R: Read + 'a>(
    it: impl Iterator<Item = io::Result<tar::Entry<'a, R>>>,
    max: usize,
    budget: &Budget,
    f: &mut impl FnMut(Entry, &mut dyn Read) -> bool,
) -> (usize, bool, Option<String>) {
    let mut n = 0usize;
    for item in it {
        if n >= max {
            return (n, false, None);
        }
        let mut e = match item {
            Ok(e) => e,
            Err(_) => {
                let note = if budget.late.get() {
                    format!("reading the tar stopped after {n} entries: it took too long")
                } else if budget.hit.get() {
                    format!(
                        "reading the tar stopped after {n} entries, at {} MiB of unpacked data",
                        budget.ceiling >> 20
                    )
                } else {
                    format!("the tar is damaged after {n} entries; the rest cannot be listed")
                };
                // A stop at a limit leaves entries unseen; damage ends it.
                return (n, !(budget.hit.get() || budget.late.get()), Some(note));
            }
        };
        let path = String::from_utf8_lossy(&e.path_bytes()).into_owned();
        let entry = Entry {
            index: n,
            path: clean_path(&path),
            size: Some(e.size()),
            kind: kind_of(&e),
        };
        n += 1;
        if !f(entry, &mut e) {
            return (n, false, None);
        }
    }
    (n, true, None)
}

/// Give each entry of the tar in `bytes` (and a reader of its data) to `f`
/// until it says stop or `max` entries were given. Returns `(entries seen,
/// the end was reached, a note)`.
pub(crate) fn walk(
    bytes: &[u8],
    kind: Kind,
    max: usize,
    deadline: Instant,
    mut f: impl FnMut(Entry, &mut dyn Read) -> bool,
) -> (usize, bool, Option<String>) {
    let budget = Budget::new(deadline, LIST_CEILING);
    match kind {
        Kind::TarGz => {
            let src = Metered {
                inner: flate2::read::GzDecoder::new(bytes),
                budget: &budget,
            };
            let mut ar = tar::Archive::new(src);
            match ar.entries() {
                Ok(it) => each(it, max, &budget, &mut f),
                Err(_) => (0, true, Some("the tar could not be read".into())),
            }
        }
        _ => {
            let mut ar = tar::Archive::new(io::Cursor::new(bytes));
            match ar.entries_with_seek() {
                Ok(it) => each(it, max, &budget, &mut f),
                Err(_) => (0, true, Some("the tar could not be read".into())),
            }
        }
    }
}
