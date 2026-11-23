//! A bounded reader of a zip archive held in memory (at most 10 MiB, the
//! upload limit): the central directory is read entry by entry and every
//! number in it is checked against the bytes there are; nothing is trusted
//! and nothing is allocated from a size the archive states. Only stored and
//! deflate entries are readable, never encrypted ones. Hand-written on
//! purpose (PLAN.md T95): the `zip` crate pulls in several new crates and
//! its readers are not bounded by this module's ceilings; this one uses
//! `flate2`, which is already in the tree.

use crate::{Entry, EntryKind, PATH_RAW, clean_path_bytes};

const EOCD: u32 = 0x0605_4b50;
const EOCD64: u32 = 0x0606_4b50;
const LOC64: u32 = 0x0706_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const LOCAL: u32 = 0x0403_4b50;
/// The comment after the end record is at most this long.
const MAX_COMMENT: usize = 65_535;

fn u16le(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        b.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}
fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}
fn u64le(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        b.get(at..at.checked_add(8)?)?.try_into().ok()?,
    ))
}

/// An entry as its central directory record says it is.
#[derive(Debug, Clone)]
pub(crate) struct Raw {
    pub entry: Entry,
    pub method: u16,
    pub flags: u16,
    pub comp: u64,
    pub size: u64,
    pub local: u64,
    /// The CRC-32 of the unpacked data, as the directory says.
    pub crc: u32,
    /// The entry's name exactly as the directory holds it.
    pub name: Vec<u8>,
    /// The entry's DOS time and date, as the directory says.
    pub time: u16,
    pub date: u16,
}

/// Where the central directory is: `(offset, size)`.
fn directory(b: &[u8]) -> Result<(u64, u64), String> {
    let start = b.len().saturating_sub(22 + MAX_COMMENT);
    let mut at = b
        .len()
        .checked_sub(22)
        .ok_or("it is too short to be a zip")?;
    loop {
        if u32le(b, at) == Some(EOCD) {
            break;
        }
        if at == start {
            return Err("this is not a readable zip: no end-of-archive record".into());
        }
        at -= 1;
    }
    let mut size = u64::from(u32le(b, at + 12).ok_or("corrupt end record")?);
    let mut off = u64::from(u32le(b, at + 16).ok_or("corrupt end record")?);
    let total = u16le(b, at + 10).ok_or("corrupt end record")?;
    if (total == 0xFFFF || size == 0xFFFF_FFFF || off == 0xFFFF_FFFF)
        && at >= 20
        && u32le(b, at - 20) == Some(LOC64)
    {
        let rec = usize::try_from(u64le(b, at - 12).ok_or("corrupt zip64 locator")?)
            .map_err(|_| "corrupt zip64 locator")?;
        if u32le(b, rec) != Some(EOCD64) {
            return Err("corrupt zip64 end record".into());
        }
        size = u64le(b, rec.checked_add(40).ok_or("corrupt zip64 end record")?)
            .ok_or("corrupt zip64 end record")?;
        off = u64le(b, rec.checked_add(48).ok_or("corrupt zip64 end record")?)
            .ok_or("corrupt zip64 end record")?;
    }
    let end = off.checked_add(size).ok_or("corrupt directory")?;
    if end > b.len() as u64 {
        return Err("the zip's directory lies outside the file: it is cut short or corrupt".into());
    }
    Ok((off, size))
}

/// Walk the central directory, giving each record to `f` until it says stop
/// or `max` records were given. Returns `(records seen, reached the end of
/// the directory, a note when it ended in a damaged record)`.
pub(crate) fn walk(
    b: &[u8],
    max: usize,
    mut f: impl FnMut(Raw) -> bool,
) -> Result<(usize, bool, Option<String>), String> {
    let (off, size) = directory(b)?;
    // Checked by `directory`: the whole of it lies in the file.
    let (mut at, end) = (off as usize, off.saturating_add(size) as usize);
    let mut n = 0usize;
    while at < end {
        if n >= max {
            return Ok((n, false, None));
        }
        let damaged = |n: usize| {
            Ok((
                n,
                true,
                Some(format!(
                    "the zip's directory is damaged after {n} entries; the rest cannot be listed"
                )),
            ))
        };
        if at.checked_add(46).is_none_or(|x| x > end) || u32le(b, at) != Some(CENTRAL) {
            return damaged(n);
        }
        let made = u16le(b, at + 4).unwrap_or(0);
        let flags = u16le(b, at + 8).unwrap_or(0);
        let method = u16le(b, at + 10).unwrap_or(0);
        let mut comp = u64::from(u32le(b, at + 20).unwrap_or(0));
        let mut size = u64::from(u32le(b, at + 24).unwrap_or(0));
        let name_len = usize::from(u16le(b, at + 28).unwrap_or(0));
        let extra_len = usize::from(u16le(b, at + 30).unwrap_or(0));
        let comment_len = usize::from(u16le(b, at + 32).unwrap_or(0));
        let crc = u32le(b, at + 16).unwrap_or(0);
        let external = u32le(b, at + 38).unwrap_or(0);
        let mut local = u64::from(u32le(b, at + 42).unwrap_or(0));
        let Some(next) = at
            .checked_add(46 + name_len + extra_len + comment_len)
            .filter(|x| *x <= end)
        else {
            return damaged(n);
        };
        let name = &b[at + 46..at + 46 + name_len];
        let extra = &b[at + 46 + name_len..at + 46 + name_len + extra_len];
        // zip64: the fields that were 0xFFFFFFFF, in this order.
        let mut e = 0usize;
        while e.checked_add(4).is_some_and(|x| x <= extra.len()) {
            let id = u16le(extra, e).unwrap_or(0);
            let len = usize::from(u16le(extra, e + 2).unwrap_or(0));
            let body = extra.get(e + 4..(e + 4).saturating_add(len)).unwrap_or(&[]);
            if id == 1 {
                let (want_size, want_comp, want_local) = (
                    size == 0xFFFF_FFFF,
                    comp == 0xFFFF_FFFF,
                    local == 0xFFFF_FFFF,
                );
                let mut p = 0;
                for (wanted, v) in [
                    (want_size, &mut size),
                    (want_comp, &mut comp),
                    (want_local, &mut local),
                ] {
                    if wanted && let Some(x) = u64le(body, p) {
                        *v = x;
                        p += 8;
                    }
                }
            }
            e += 4 + len;
        }
        let path = clean_path_bytes(name);
        let slashed = name.ends_with(b"/");
        let unix = made >> 8 == 3;
        let mode = external >> 16;
        let dir = slashed || (external & 0x10 != 0) || (unix && mode & 0o170000 == 0o040000);
        let kind = if dir {
            EntryKind::Dir
        } else if unix && mode & 0o170000 == 0o120000 {
            // The target is the entry's own data: shown as a link, never read
            // as one.
            EntryKind::Link(String::new())
        } else if unix && !matches!(mode & 0o170000, 0 | 0o100000) {
            EntryKind::Other("special file")
        } else {
            EntryKind::File
        };
        let entry = Entry {
            index: n,
            path,
            size: Some(size),
            kind,
        };
        n += 1;
        let go = f(Raw {
            name: name.to_vec(),
            time: u16le(b, at + 12).unwrap_or(0),
            date: u16le(b, at + 14).unwrap_or(0),
            entry,
            method,
            flags,
            comp,
            size,
            local,
            crc,
        });
        at = next;
        if !go {
            return Ok((n, at >= end, None));
        }
    }
    Ok((n, true, None))
}

/// The bytes of `raw`'s data, as stored (compressed), after checking that
/// the local header and the data lie in the file.
pub(crate) fn data<'a>(b: &'a [u8], raw: &Raw) -> Result<&'a [u8], String> {
    if raw.flags & 1 != 0 {
        return Err("this entry is encrypted; it cannot be read".into());
    }
    let at = usize::try_from(raw.local).map_err(|_| "corrupt entry")?;
    if u32le(b, at) != Some(LOCAL) {
        return Err("this entry's header is corrupt".into());
    }
    let cut = || "this entry's header is cut short";
    let n = usize::from(u16le(b, at.checked_add(26).ok_or_else(cut)?).ok_or_else(cut)?);
    let e = usize::from(u16le(b, at.checked_add(28).ok_or_else(cut)?).ok_or_else(cut)?);
    let name_at = at.checked_add(30).ok_or_else(cut)?;
    let start = name_at
        .checked_add(n)
        .and_then(|x| x.checked_add(e))
        .ok_or_else(cut)?;
    // The local header names the entry too: it must be the one the listing
    // showed, or the data is another record's.
    let local_name = b
        .get(name_at..name_at.saturating_add(n.min(PATH_RAW)))
        .ok_or_else(cut)?;
    if clean_path_bytes(local_name) != raw.entry.path {
        return Err(
            "this entry's own header names another file than the listing does; it is not read"
                .into(),
        );
    }
    let end = start
        .checked_add(usize::try_from(raw.comp).map_err(|_| "corrupt entry")?)
        .ok_or("corrupt entry")?;
    b.get(start..end)
        .ok_or_else(|| "this entry's data lies outside the file: the zip is cut short".into())
}
