//! The named parts of a zip held in memory, read as bounded streams: what an
//! office document (Word, Excel, PowerPoint, OpenDocument) is made of
//! (PLAN.md T96). The same checks as [`crate::read`] on a zip entry: the
//! directory is walked entry by entry, every number in it is checked against
//! the bytes there are, nothing is allocated from a size the archive states,
//! an entry that says it unpacks to far more than its data can hold is
//! refused, and everything unpacked from one package together, whatever its
//! parts say, stops at [`PACKAGE_CEILING`] or at the deadline.
//!
//! Part names are only ever compared with names the caller gives; they are
//! never a path of any disk.

use std::io::{self, Read};
use std::time::Instant;

use crate::zip::{self, Raw};
use crate::{Budget, EntryKind, LIST_TOTAL, MAX_DECLARED, Metered, stopped_note};

/// Unpacked bytes one package may give in all, across its parts. A real
/// document's XML is a few MiB; a sheet of tens of thousands of rows is
/// a few MiB; past this a file is a bomb, not a document.
pub const PACKAGE_CEILING: u64 = 64 << 20;

/// Why a part cannot be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartError {
    /// The package has no part of that name (as a file).
    Missing,
    /// The entry is encrypted.
    Encrypted,
    /// Anything else, as a sentence (no text of the archive in it).
    Failed(String),
}

/// A zip opened for reading its parts.
pub struct Package<'a> {
    bytes: &'a [u8],
    parts: Vec<Raw>,
    budget: Budget,
}

impl<'a> Package<'a> {
    /// Read the directory of `bytes`: at most [`LIST_TOTAL`] entries.
    pub fn open(bytes: &'a [u8], deadline: Instant) -> Result<Self, String> {
        let mut parts = Vec::new();
        let (_, complete, note) = zip::walk(bytes, LIST_TOTAL, |r| {
            parts.push(r);
            true
        })?;
        if !complete {
            return Err("the zip has too many entries to be a document".into());
        }
        if let Some(n) = note {
            return Err(n);
        }
        Ok(Self {
            bytes,
            parts,
            budget: Budget::new(deadline, PACKAGE_CEILING),
        })
    }

    fn find(&self, name: &str) -> Option<&Raw> {
        self.parts
            .iter()
            .find(|r| matches!(r.entry.kind, EntryKind::File) && r.entry.path == name)
    }

    /// Whether a file of that (cleaned) name is in the package.
    pub fn has(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    /// The part `name` as a stream of its unpacked bytes. Reading past the
    /// package's ceiling, or past the deadline, fails the read;
    /// [`Self::stopped`] then says why.
    pub fn read(&self, name: &str) -> Result<Box<dyn Read + '_>, PartError> {
        let raw = self.find(name).ok_or(PartError::Missing)?;
        if raw.flags & 1 != 0 {
            return Err(PartError::Encrypted);
        }
        if raw.size > MAX_DECLARED {
            return Err(PartError::Failed(
                "a part says it holds more than 1 GiB".into(),
            ));
        }
        let data = zip::data(self.bytes, raw).map_err(PartError::Failed)?;
        match raw.method {
            0 => {
                if raw.comp != raw.size {
                    return Err(PartError::Failed("a part's sizes disagree".into()));
                }
                Ok(Box::new(Metered {
                    inner: data,
                    budget: &self.budget,
                }))
            }
            8 => {
                // Deflate cannot give more than about 1032 bytes for one.
                if raw.size > raw.comp.saturating_mul(1100).saturating_add(1024) {
                    return Err(PartError::Failed(
                        "a part says it unpacks to far more than its data can hold".into(),
                    ));
                }
                Ok(Box::new(Metered {
                    inner: flate2::read::DeflateDecoder::new(data),
                    budget: &self.budget,
                }))
            }
            _ => Err(PartError::Failed(
                "a part uses a compression method that is not supported".into(),
            )),
        }
    }

    /// The whole part, when it unpacks to at most `max` bytes.
    pub fn read_small(&self, name: &str, max: usize) -> Result<Vec<u8>, PartError> {
        let mut r = self.read(name)?;
        let mut out = Vec::new();
        r.by_ref()
            .take(max as u64 + 1)
            .read_to_end(&mut out)
            .map_err(|e| self.failed(&e))?;
        if out.len() > max {
            return Err(PartError::Failed("a part is larger than it may be".into()));
        }
        Ok(out)
    }

    /// A sentence when a read was stopped by the ceiling or the deadline.
    pub fn stopped(&self) -> Option<String> {
        stopped_note(&self.budget, "reading the document")
    }

    /// An I/O failure of a part's stream as a [`PartError`].
    pub fn failed(&self, e: &io::Error) -> PartError {
        PartError::Failed(self.stopped().unwrap_or_else(|| {
            if e.kind() == io::ErrorKind::InvalidData || e.kind() == io::ErrorKind::UnexpectedEof {
                "a part is damaged".into()
            } else {
                "a part could not be read".into()
            }
        }))
    }

    /// Unpacked bytes given so far.
    pub fn inflated(&self) -> u64 {
        self.budget.produced.get()
    }
}
