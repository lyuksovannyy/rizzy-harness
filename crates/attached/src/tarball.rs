//! Walking a tar (plain or gzip-compressed) held in memory, by a header
//! walker of our own: the `tar` crate buffers a long name or an extended
//! header whole, however large it says it is, and a 140 KB gzip could make it
//! hold tens of MiB (T95). Here an `L`, `K` or `x` header over
//! [`EXT_MAX`] bytes ends the listing with a sentence before one byte of it
//! is read, every name taken from the archive is cut to [`crate::PATH_RAW`]
//! bytes before it is converted, and a compressed tar is never inflated
//! past [`crate::LIST_CEILING`] or the deadline. A link, a device or a pipe
//! is listed as what it is and never followed or opened. Sparse files are
//! listed and not read.

use std::io::{self, Read};
use std::time::Instant;

use crate::{Budget, Entry, EntryKind, Kind, LIST_CEILING, Metered, PATH_RAW, clean_path_bytes};

/// The most a long name, a long link or a pax header may hold.
pub(crate) const EXT_MAX: u64 = 64 * 1024;

/// A tar number: octal text, or (high bit set) big-endian binary.
fn number(b: &[u8]) -> Option<u64> {
    if b.first().is_some_and(|f| f & 0x80 != 0) {
        let mut v: u64 = 0;
        for (i, x) in b.iter().enumerate() {
            let x = if i == 0 { x & 0x7f } else { *x };
            v = v.checked_mul(256)?.checked_add(u64::from(x))?;
        }
        return Some(v);
    }
    let t: Vec<u8> = b
        .iter()
        .copied()
        .skip_while(|c| *c == b' ')
        .take_while(|c| (b'0'..=b'7').contains(c))
        .collect();
    if t.is_empty() {
        return Some(0);
    }
    t.iter().try_fold(0u64, |v, c| {
        v.checked_mul(8)?.checked_add(u64::from(c - b'0'))
    })
}

fn cstr(b: &[u8]) -> &[u8] {
    let n = b.iter().position(|c| *c == 0).unwrap_or(b.len());
    &b[..n]
}

/// Read up to `buf.len()` bytes; how many came.
fn fill(r: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut got = 0;
    while got < buf.len() {
        match r.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(got)
}

fn discard(r: &mut impl Read, n: u64) -> io::Result<()> {
    let copied = io::copy(&mut r.take(n), &mut io::sink())?;
    if copied < n {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    Ok(())
}

/// The bytes after `n` bytes of data up to the next 512-byte block (never a
/// subtraction: `n` is the archive's number and may be 2^64-1).
fn padding(n: u64) -> u64 {
    (512 - n % 512) % 512
}

/// `len key=value\n` records of a pax header: the keys this reader uses.
#[derive(Default)]
struct Pax {
    path: Option<Vec<u8>>,
    link: Option<Vec<u8>>,
    size: Option<u64>,
}

fn pax(data: &[u8]) -> Pax {
    let mut out = Pax::default();
    let mut at = 0usize;
    while at < data.len() {
        let rest = &data[at..];
        let Some(sp) = rest.iter().position(|c| *c == b' ') else {
            break;
        };
        let Some(len) = std::str::from_utf8(&rest[..sp])
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|l| *l > sp + 1 && *l <= rest.len())
        else {
            break;
        };
        let rec = rest[sp + 1..len]
            .strip_suffix(b"\n")
            .unwrap_or(&rest[sp + 1..len]);
        if let Some(eq) = rec.iter().position(|c| *c == b'=') {
            let (k, v) = (&rec[..eq], &rec[eq + 1..]);
            let cut = |v: &[u8]| v[..v.len().min(PATH_RAW)].to_vec();
            match k {
                b"path" => out.path = Some(cut(v)),
                b"linkpath" => out.link = Some(cut(v)),
                b"size" => out.size = std::str::from_utf8(v).ok().and_then(|s| s.parse().ok()),
                _ => {}
            }
        }
        at += len;
    }
    out
}

fn walk_in<R: Read>(
    mut r: R,
    max: usize,
    budget: &Budget,
    f: &mut impl FnMut(Entry, &mut dyn Read) -> bool,
) -> (usize, bool, Option<String>) {
    let mut n = 0usize;
    let (mut long_name, mut long_link): (Option<Vec<u8>>, Option<Vec<u8>>) = (None, None);
    let mut px = Pax::default();
    // Why the walk stopped early: a limit leaves entries unseen, damage ends it.
    let stop = |n: usize, budget: &Budget| {
        if budget.late.get() {
            (
                n,
                false,
                Some(format!(
                    "reading the tar stopped after {n} entries: it took too long"
                )),
            )
        } else if budget.hit.get() {
            (
                n,
                false,
                Some(format!(
                    "reading the tar stopped after {n} entries, at {} MiB of unpacked data",
                    budget.ceiling >> 20
                )),
            )
        } else {
            (
                n,
                true,
                Some(format!(
                    "the tar is damaged after {n} entries; the rest cannot be listed"
                )),
            )
        }
    };
    loop {
        if n >= max {
            return (n, false, None);
        }
        let mut h = [0u8; 512];
        match fill(&mut r, &mut h) {
            Ok(0) => return (n, true, None),
            Ok(512) => {}
            _ => return stop(n, budget),
        }
        if h.iter().all(|b| *b == 0) {
            return (n, true, None);
        }
        let sum: u64 = h
            .iter()
            .enumerate()
            .map(|(i, b)| {
                if (148..156).contains(&i) {
                    32
                } else {
                    u64::from(*b)
                }
            })
            .sum();
        if number(&h[148..156]) != Some(sum) {
            return stop(n, budget);
        }
        let Some(size) = number(&h[124..136]) else {
            return stop(n, budget);
        };
        let ty = h[156];
        if matches!(ty, b'L' | b'K' | b'x' | b'g') {
            if size > EXT_MAX {
                return (
                    n,
                    true,
                    Some(format!(
                        "the tar has a name or extended header of {size} bytes, over the {} bytes \
                         read; the rest of it is not listed",
                        EXT_MAX
                    )),
                );
            }
            let mut data = vec![0u8; size as usize];
            if fill(&mut r, &mut data).is_ok_and(|g| g == data.len())
                && discard(&mut r, padding(size)).is_ok()
            {
                match ty {
                    b'L' => {
                        long_name = Some(cstr(&data)[..cstr(&data).len().min(PATH_RAW)].to_vec())
                    }
                    b'K' => {
                        long_link = Some(cstr(&data)[..cstr(&data).len().min(PATH_RAW)].to_vec())
                    }
                    b'x' => px = pax(&data),
                    _ => {}
                }
                continue;
            }
            return stop(n, budget);
        }
        let ustar = &h[257..262] == b"ustar";
        let mut name = cstr(&h[0..100]).to_vec();
        let prefix = cstr(&h[345..500]);
        if ustar && !prefix.is_empty() {
            let mut p = prefix.to_vec();
            p.push(b'/');
            p.extend_from_slice(&name);
            name = p;
        }
        let name = px.path.take().or(long_name.take()).unwrap_or(name);
        let link = px
            .link
            .take()
            .or(long_link.take())
            .unwrap_or_else(|| cstr(&h[157..257]).to_vec());
        let data_len = px.size.take().unwrap_or(size);
        let path = clean_path_bytes(&name);
        let kind = match ty {
            b'0' | 0 | b'7' if name.ends_with(b"/") => EntryKind::Dir,
            b'0' | 0 | b'7' => EntryKind::File,
            b'5' => EntryKind::Dir,
            b'1' | b'2' => EntryKind::Link(clean_path_bytes(&link)),
            b'3' => EntryKind::Other("character device"),
            b'4' => EntryKind::Other("block device"),
            b'6' => EntryKind::Other("pipe"),
            b'S' => EntryKind::Other("sparse file"),
            _ => EntryKind::Other("special entry"),
        };
        let entry = Entry {
            index: n,
            path,
            size: Some(data_len),
            kind,
        };
        n += 1;
        let mut t = (&mut r).take(data_len);
        let go = f(entry, &mut t);
        let left = t.limit();
        if !go {
            return (n, false, None);
        }
        if discard(&mut r, left.saturating_add(padding(data_len))).is_err() {
            return stop(n, budget);
        }
    }
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
        Kind::TarGz => walk_in(
            Metered {
                inner: flate2::read::GzDecoder::new(bytes),
                budget: &budget,
            },
            max,
            &budget,
            &mut f,
        ),
        _ => walk_in(
            Metered {
                inner: bytes,
                budget: &budget,
            },
            max,
            &budget,
            &mut f,
        ),
    }
}
