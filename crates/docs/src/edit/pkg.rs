//! A package (a zip) opened for editing one part of it (PLAN.md T98). The
//! other parts are copied as their original compressed bytes: nothing is
//! unpacked that is not the part being edited or one the edit has to read,
//! so a bomb in an untouched entry costs nothing. The entry order and the
//! entries' timestamps are kept.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use harness_attached::{Package, PartError, RawEntry, clean_path_bytes};

use crate::Result;
use crate::limits::MAX_OUTPUT_BYTES;
use crate::zip::{ZipWriter, check_name};

/// Entries a package may have to be edited.
pub const MAX_ENTRIES: usize = 4_000;
/// How long reading one package may take.
const READ_TIME: Duration = Duration::from_secs(15);

/// A package opened and checked.
pub struct Source<'a> {
    pkg: Package<'a>,
    names: Vec<String>,
}

/// Changes to the package's parts.
#[derive(Default)]
pub struct Rewrite {
    /// A part's new bytes (its name must exist).
    pub replaced: HashMap<String, Vec<u8>>,
    /// Parts left out of the result.
    pub removed: HashSet<String>,
}

impl<'a> Source<'a> {
    /// Open `bytes` as a package and refuse what an edit cannot keep
    /// faithfully: not a zip, too many entries, an entry that is encrypted,
    /// a link, of an unknown method or too large for 32 bits, names that are
    /// not UTF-8, that a package cannot hold, that differ from how this
    /// reader shows them, that repeat, or that repeat but for letter case; a
    /// signed package.
    pub fn open(bytes: &'a [u8]) -> Result<Self> {
        let pkg = Package::open(bytes, Instant::now() + READ_TIME)
            .map_err(|e| format!("this is not a zip file that can be edited: {e}"))?;
        let entries = pkg.raw_entries()?;
        if entries.len() > MAX_ENTRIES {
            return Err("the file has too many parts to be edited".into());
        }
        let mut names = Vec::with_capacity(entries.len());
        let mut seen = HashSet::new();
        for e in &entries {
            let n = std::str::from_utf8(e.name)
                .map_err(|_| "a part of the file has a name that is not UTF-8".to_string())?;
            check_name(n.strip_suffix('/').unwrap_or(n))?;
            if clean_path_bytes(e.name) != n {
                return Err("a part of the file has a name with characters in it that are not shown as they are".into());
            }
            if !seen.insert(n.to_lowercase()) {
                return Err(
                    "two parts of the file have the same name (or names that differ only by letter case); readers disagree on which is meant, so it is not edited"
                        .into(),
                );
            }
            if n.to_ascii_lowercase().starts_with("_xmlsignatures/") {
                return Err("the file is digitally signed; an edit would invalidate the signature, so it is not edited".into());
            }
            names.push(n.to_string());
        }
        // A package treats `%64` in a part name as `d` (OPC): a name that
        // reads as another entry's name that way would be two parts to this
        // reader and one to Word.
        for n in &names {
            if n.contains('%') {
                let d = percent_decode(n);
                if d != *n && seen.contains(&d.to_lowercase()) {
                    return Err("a part of the file has a name that, percent-decoded, is another part's name; readers disagree on which is meant, so it is not edited".into());
                }
            }
        }
        Ok(Self { pkg, names })
    }

    pub fn has(&self, name: &str) -> bool {
        self.names.iter().any(|n| n == name)
    }

    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// A part, unpacked, at most `max` bytes.
    pub fn read(&self, name: &str, max: usize) -> Result<Vec<u8>> {
        self.pkg.read_small(name, max).map_err(|e| match e {
            PartError::Missing => format!("the file has no part {name}"),
            PartError::Encrypted => "a part of the file is encrypted".into(),
            PartError::Failed(m) => format!("{name}: {m}"),
        })
    }

    /// The package with `rw` applied: every other entry copied raw.
    pub fn rewrite(&self, rw: &Rewrite) -> Result<Vec<u8>> {
        for n in rw.replaced.keys().chain(rw.removed.iter()) {
            if !self.has(n) {
                return Err(format!("the file has no part {n}"));
            }
        }
        let entries: Vec<RawEntry<'_>> = self.pkg.raw_entries()?;
        let mut w = ZipWriter::new(MAX_OUTPUT_BYTES);
        for (e, n) in entries.iter().zip(&self.names) {
            if rw.removed.contains(n) {
                continue;
            }
            if let Some(b) = rw.replaced.get(n) {
                w.add_at(n, b, e.time, e.date)?;
            } else {
                w.put_raw(
                    n,
                    e.method,
                    e.crc,
                    e.comp as u32,
                    e.size as u32,
                    e.time,
                    e.date,
                    e.data,
                )?;
            }
        }
        w.finish()
    }
}

/// `%XX` escapes of a part name decoded (what a package's part names are
/// read as); anything that is not an escape stays.
fn percent_decode(n: &str) -> String {
    let b = n.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(h) = n.get(i + 1..i + 3)
            && let Ok(v) = u8::from_str_radix(h, 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
