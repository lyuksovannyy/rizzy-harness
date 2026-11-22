//! Looking into a file a person attached, read-only and bounded (PLAN.md
//! T95, docs/attached-files.md). The bytes are at most 10 MiB and are held
//! in memory by the caller; everything here works on that slice and
//! nothing is ever written, unpacked to disk or followed.
//!
//! * [`describe`]: what the first bytes say it is.
//! * [`list`]: the entries of a zip, tar, tar.gz or gz: at most
//!   [`LIST_TOTAL`] are read and a call shows at most [`LIST_PAGE`].
//! * [`read`]: the file or one entry of it, as numbered lines when it is
//!   text, else a hex dump or the printable runs.
//!
//! Every number an archive states is checked against the bytes there are.
//! A compressed stream is never inflated past a fixed ceiling per call
//! ([`READ_CEILING`], [`LIST_CEILING`]) or past the call's deadline, however
//! little the archive says it holds; an archive inside an archive is read as
//! bytes. Entry paths are data: they come back cleaned
//! ([`clean_path`]) and are only ever compared with a path the model gives
//! back, never used as a path of any disk.

mod tarball;
mod zip;

use std::cell::Cell;
use std::io::{self, Read};
use std::time::Instant;

use harness_core::event::shown_name;
use harness_core::text::clean_line;

/// Entries one `list` call shows, at most.
pub const LIST_PAGE: usize = 500;
/// Entries one call reads in all, at most; past them the answer says "and
/// more".
pub const LIST_TOTAL: usize = 20_000;
/// The most unpacked data one `read` call inflates (offset and the
/// window together, rounded up to this).
pub const READ_CEILING: u64 = 16 << 20;
/// The most unpacked data a `list` of a compressed tar scans.
pub const LIST_CEILING: u64 = 32 << 20;
/// An entry that says it holds more than this is refused.
pub const MAX_DECLARED: u64 = 1 << 30;
/// Bytes of the start looked at to tell text from data.
pub const SNIFF: usize = 8192;
/// Bytes one hex dump shows, at most.
pub const HEX_MAX: u64 = 4096;
/// Bytes one hex dump shows when no `limit` is given.
pub const HEX_DEFAULT: u64 = 512;
/// Lines one text read shows when no `limit` is given, and at most.
pub const LINES_DEFAULT: u64 = 200;
pub const LINES_MAX: u64 = 2000;
/// Characters of one text line shown; the rest is cut.
pub const LINE_CHARS: usize = 2000;
/// Characters one text read shows in all, at most.
pub const TEXT_CAP: usize = 30_000;
/// Bytes of printable runs one `strings` call shows, at most.
pub const STRINGS_CAP: usize = 4096;
/// Bytes one `strings` call looks through when no `limit` is given, and at most.
pub const STRINGS_SCAN_DEFAULT: u64 = 256 << 10;
pub const STRINGS_SCAN_MAX: u64 = 1 << 20;
/// Characters of an entry path shown.
pub const PATH_CHARS: usize = 300;

/// What a file is, by its first bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Plain,
    Zip,
    Tar,
    TarGz,
    Gz,
}

impl Kind {
    pub fn is_archive(self) -> bool {
        self != Kind::Plain
    }
}

/// What an entry is. A link is listed, never followed; the target is shown
/// as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
    Link(String),
    Other(&'static str),
}

/// One entry of an archive, as a listing shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its position in the archive's own order (0-based).
    pub index: usize,
    /// Cleaned: one line, no hidden or control characters, at most
    /// [`PATH_CHARS`]. Data, never a path on any disk.
    pub path: String,
    /// What the archive says the entry holds.
    pub size: Option<u64>,
    pub kind: EntryKind,
}

/// One page of a listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub kind: Kind,
    pub entries: Vec<Entry>,
    /// The first entry of the page (0-based).
    pub offset: usize,
    /// Entries read in this call (at most [`LIST_TOTAL`]).
    pub scanned: usize,
    /// The end of the archive was reached: `scanned` is its whole count.
    pub complete: bool,
    pub note: Option<String>,
}

impl Listing {
    /// Whether there are entries after this page.
    pub fn more(&self) -> bool {
        !self.complete || self.offset + self.entries.len() < self.scanned
    }
}

/// The most raw bytes of a name taken from an archive that are looked at:
/// what [`PATH_CHARS`] characters can need, with room for characters that
/// cleaning drops. Everything past it is cut before it is converted or
/// copied, so a name of 30 MiB costs as much as one of 2 KiB.
pub const PATH_RAW: usize = PATH_CHARS * 4 + 1024;

/// An entry path as shown: one clean line of at most [`PATH_CHARS`]
/// characters, cut with `…`. Cleaning twice gives the same string, so a
/// path the model copies from a listing finds its entry. Text longer than
/// [`PATH_RAW`] bytes is cut first.
pub fn clean_path(s: &str) -> String {
    let mut end = s.len().min(PATH_RAW);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    let c = clean_line(&s[..end], usize::MAX >> 4);
    if c.chars().count() > PATH_CHARS {
        let mut t: String = c.chars().take(PATH_CHARS - 1).collect();
        t.push('\u{2026}');
        t
    } else {
        c
    }
}

/// [`clean_path`] of raw archive bytes: cut to [`PATH_RAW`] bytes before the
/// lossy conversion, so nothing hostile is ever copied whole.
pub fn clean_path_bytes(b: &[u8]) -> String {
    clean_path(&String::from_utf8_lossy(&b[..b.len().min(PATH_RAW)]))
}

/// What `info` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Described {
    pub kind: Kind,
    /// A short name for the type ("ZIP archive", "PNG image", "text").
    pub label: String,
    /// The media type the first bytes say, when they say one.
    pub mime: Option<String>,
    pub text: bool,
}

/// Whether `sample` (the start of a file) reads as text: UTF-8 with no NUL
/// (a sequence cut at the end of the sample is let through).
pub fn is_text(sample: &[u8]) -> bool {
    let sample = sample.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(sample);
    if sample.contains(&0) {
        return false;
    }
    match std::str::from_utf8(sample) {
        Ok(_) => true,
        Err(e) => e.error_len().is_none() && e.valid_up_to() + 4 > sample.len(),
    }
}

/// The kind of `bytes`: a zip by its signature, a gzip by its magic (a tar
/// inside it is looked for in its first block), a tar by `ustar`.
pub fn detect(bytes: &[u8]) -> Kind {
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        return Kind::Zip;
    }
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut head = [0u8; 512];
        let mut d = flate2::read::GzDecoder::new(bytes);
        let mut got = 0;
        while got < head.len() {
            match d.read(&mut head[got..]) {
                Ok(0) | Err(_) => break,
                Ok(n) => got += n,
            }
        }
        return if got == 512 && &head[257..262] == b"ustar" {
            Kind::TarGz
        } else {
            Kind::Gz
        };
    }
    if bytes.len() >= 512 && &bytes[257..262] == b"ustar" {
        return Kind::Tar;
    }
    Kind::Plain
}

pub fn describe(bytes: &[u8]) -> Described {
    let kind = detect(bytes);
    let head = &bytes[..bytes.len().min(SNIFF)];
    let text = kind == Kind::Plain && is_text(head);
    let known = infer::get(head);
    let label = match kind {
        Kind::Zip => "ZIP archive".to_string(),
        Kind::Tar => "tar archive".to_string(),
        Kind::TarGz => "gzip-compressed tar archive".to_string(),
        Kind::Gz => "gzip-compressed data".to_string(),
        Kind::Plain if text => "text (UTF-8)".to_string(),
        Kind::Plain => match known {
            Some(k) => format!("{} ({})", k.extension(), k.mime_type()),
            None => "binary data".to_string(),
        },
    };
    Described {
        kind,
        label,
        mime: known.map(|k| k.mime_type().to_string()),
        text,
    }
}

// ---------------------------------------------------------------------------
// Bounds on a stream

/// Counts what a stream gives and refuses more than its ceiling or past
/// the deadline.
pub(crate) struct Budget {
    deadline: Instant,
    ceiling: u64,
    produced: Cell<u64>,
    hit: Cell<bool>,
    late: Cell<bool>,
}

impl Budget {
    fn new(deadline: Instant, ceiling: u64) -> Self {
        Self {
            deadline,
            ceiling,
            produced: Cell::new(0),
            hit: Cell::new(false),
            late: Cell::new(false),
        }
    }
}

#[derive(Debug)]
struct Stopped;
impl std::fmt::Display for Stopped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("stopped at the limit")
    }
}
impl std::error::Error for Stopped {}

pub(crate) struct Metered<'a, R> {
    pub inner: R,
    pub budget: &'a Budget,
}

impl<R: Read> Read for Metered<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let b = self.budget;
        if Instant::now() > b.deadline {
            b.late.set(true);
            return Err(io::Error::other(Stopped));
        }
        let room = b.ceiling.saturating_sub(b.produced.get());
        if room == 0 {
            // Exactly at the ceiling is fine if the stream ends there.
            let mut one = [0u8; 1];
            return match self.inner.read(&mut one) {
                Ok(0) => Ok(0),
                Ok(_) | Err(_) => {
                    b.hit.set(true);
                    Err(io::Error::other(Stopped))
                }
            };
        }
        let want = buf.len().min(usize::try_from(room).unwrap_or(usize::MAX));
        let n = self.inner.read(&mut buf[..want])?;
        b.produced.set(b.produced.get() + n as u64);
        Ok(n)
    }
}

fn stopped_note(b: &Budget, what: &str) -> Option<String> {
    if b.late.get() {
        Some(format!("{what} took too long and was stopped"))
    } else if b.hit.get() {
        Some(format!(
            "{what} stopped at {} MiB of unpacked data",
            b.ceiling >> 20
        ))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Listing

/// Entries `offset..offset + limit` of `bytes` (a page of at most
/// [`LIST_PAGE`]); at most [`LIST_TOTAL`] entries are read in all.
pub fn list(
    bytes: &[u8],
    offset: usize,
    limit: usize,
    deadline: Instant,
) -> Result<Listing, String> {
    let kind = detect(bytes);
    let limit = limit.clamp(1, LIST_PAGE);
    let mut entries: Vec<Entry> = Vec::new();
    let mut each = |e: Entry| {
        if e.index >= offset && e.index < offset + limit {
            entries.push(e);
        }
        true
    };
    let (scanned, complete, note) = match kind {
        Kind::Plain => return Err("this file is not an archive (zip, tar, tar.gz or gz)".into()),
        Kind::Zip => zip::walk(bytes, LIST_TOTAL, |r| each(r.entry))?,
        Kind::Tar | Kind::TarGz => tarball::walk(bytes, kind, LIST_TOTAL, deadline, |e, _| each(e)),
        Kind::Gz => {
            let (e, note) = gz_entry(bytes)?;
            each(e);
            (1, true, note)
        }
    };
    Ok(Listing {
        kind,
        entries,
        offset,
        scanned,
        complete,
        note,
    })
}

/// The one entry of a gzip file: the name its header gives (or none), and
/// the size its trailer says (the last four bytes, modulo 2^32: only a
/// hint, shown as such).
fn gz_entry(bytes: &[u8]) -> Result<(Entry, Option<String>), String> {
    let mut d = flate2::read::GzDecoder::new(bytes);
    let mut one = [0u8; 1];
    // Reading the first byte parses the header.
    let _ = d.read(&mut one);
    let name = d
        .header()
        .and_then(|h| h.filename())
        .map(clean_path_bytes)
        .unwrap_or_default();
    let declared = bytes
        .len()
        .checked_sub(4)
        .and_then(|at| bytes.get(at..))
        .and_then(|t| t.try_into().ok())
        .map(u32::from_le_bytes)
        .map(u64::from);
    Ok((
        Entry {
            index: 0,
            path: name,
            size: declared,
            kind: EntryKind::File,
        },
        Some(
            "a gzip file holds one stream; its size is what the file's last four bytes say and may be wrong"
                .into(),
        ),
    ))
}

// ---------------------------------------------------------------------------
// Reading

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Text as numbered lines; anything else as a hex dump.
    #[default]
    Auto,
    Hex,
    Strings,
}

#[derive(Debug, Clone)]
pub struct ReadReq<'a> {
    /// The listed path of an entry; none: the file itself, as bytes.
    pub entry: Option<&'a str>,
    /// Text: the first line (1-based, default 1). Hex and strings: the first byte (0-based).
    pub offset: Option<u64>,
    /// Text: lines. Hex: bytes. Strings: bytes to look through.
    pub limit: Option<u64>,
    pub mode: Mode,
}

/// What a read gave: `text` is the lines (each `n<TAB>line`, joined by
/// newlines), the dump or the runs, raw: the caller quotes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Out {
    pub mode: &'static str,
    pub text: String,
    /// The offset this started at (a line number for text, else a byte).
    pub from: u64,
    /// Lines (text) or bytes (hex, strings) shown.
    pub shown: u64,
    /// Where to go on, when there is more.
    pub next: Option<u64>,
    /// The size of what was read, when known (the file's, or what the entry says).
    pub size: Option<u64>,
    /// Unpacked bytes this call produced from a compressed stream.
    pub inflated: u64,
    pub note: Option<String>,
}

/// Where the bytes of what is read come from.
enum Src<'a> {
    Raw(&'a [u8]),
    Stream(&'a mut dyn Read),
}

/// Read `req` from `bytes`.
pub fn read(bytes: &[u8], req: &ReadReq<'_>, deadline: Instant) -> Result<Out, String> {
    let kind = detect(bytes);
    let Some(entry) = req.entry else {
        return consume(Src::Raw(bytes), Some(bytes.len() as u64), req, 0, None);
    };
    let want = clean_path(entry);
    // `#<n>`: the entry the listing numbered n (two entries can share a path).
    let index = entry
        .trim()
        .strip_prefix('#')
        .and_then(|n| n.parse::<usize>().ok());
    let is = |e: &Entry| index.map_or(e.path == want, |i| e.index == i);
    // What a sentence may say of it: model text, made one quoted-safe line.
    let shown = shown_name(entry);
    match kind {
        Kind::Plain => Err("this file is not an archive: read it without `entry`".into()),
        Kind::Zip => {
            let mut found: Option<zip::Raw> = None;
            zip::walk(bytes, LIST_TOTAL, |r| {
                if is(&r.entry) {
                    found = Some(r);
                    return false;
                }
                true
            })?;
            let raw = found.ok_or_else(|| not_found(&shown))?;
            read_zip_entry(bytes, &raw, req, deadline)
        }
        Kind::Gz => {
            let (e, _) = gz_entry(bytes)?;
            if !is(&e) {
                return Err(not_found(&shown));
            }
            let b = Budget::new(deadline, READ_CEILING);
            let mut r = Metered {
                inner: flate2::read::GzDecoder::new(bytes),
                budget: &b,
            };
            let out = consume(Src::Stream(&mut r), None, req, 0, Some(&b));
            finish(out, &b)
        }
        Kind::Tar | Kind::TarGz => {
            let mut result: Option<Result<Out, String>> = None;
            let (_, _, note) = tarball::walk(bytes, kind, LIST_TOTAL, deadline, |e, data| {
                if !is(&e) {
                    return true;
                }
                result = Some(match &e.kind {
                    EntryKind::File => {
                        let size = e.size;
                        if size.is_some_and(|s| s > MAX_DECLARED) {
                            Err(too_big(size.unwrap_or(0)))
                        } else {
                            let b = Budget::new(deadline, READ_CEILING);
                            let mut r = Metered {
                                inner: data,
                                budget: &b,
                            };
                            let out = consume(Src::Stream(&mut r), size, req, 0, Some(&b));
                            finish(out, &b)
                        }
                    }
                    EntryKind::Dir => Err(format!("\"{shown}\" is a folder: list the archive")),
                    // The target is in the listing, quoted; not here.
                    EntryKind::Link(_) => {
                        Err(format!("\"{shown}\" is a link: links are never followed"))
                    }
                    EntryKind::Other(what) => Err(format!(
                        "\"{shown}\" is a {what}: only regular files can be read"
                    )),
                });
                false
            });
            result.unwrap_or_else(|| {
                Err(match note {
                    Some(n) => format!("{} ({n})", not_found(&shown)),
                    None => not_found(&shown),
                })
            })
        }
    }
}

fn not_found(path: &str) -> String {
    format!(
        "no entry \"{path}\" in this archive: use a path from `list`, or `#` and the number it shows"
    )
}

fn too_big(size: u64) -> String {
    format!(
        "this entry says it holds {size} bytes, more than the {} bytes that are ever read from one; it is not read",
        MAX_DECLARED
    )
}

fn read_zip_entry(
    bytes: &[u8],
    raw: &zip::Raw,
    req: &ReadReq<'_>,
    deadline: Instant,
) -> Result<Out, String> {
    let shown = shown_name(&raw.entry.path);
    match &raw.entry.kind {
        EntryKind::Dir => return Err(format!("\"{shown}\" is a folder: list the archive")),
        EntryKind::Link(_) => {
            return Err(format!("\"{shown}\" is a link: links are never followed"));
        }
        EntryKind::Other(what) => {
            return Err(format!(
                "\"{shown}\" is a {what}: only regular files can be read"
            ));
        }
        EntryKind::File => {}
    }
    if raw.size > MAX_DECLARED {
        return Err(too_big(raw.size));
    }
    let data = zip::data(bytes, raw)?;
    match raw.method {
        0 => {
            if raw.comp != raw.size {
                return Err("this entry's sizes disagree; it is not read".into());
            }
            let mut r = CrcRead::new(data);
            let out = consume(Src::Stream(&mut r), Some(raw.size), req, 0, None);
            r.checked(out, raw.crc)
        }
        8 => {
            // Deflate cannot give more than about 1032 bytes for one; an
            // entry that says it does has a header that lies.
            if raw.size > raw.comp.saturating_mul(1100).saturating_add(1024) {
                return Err(
                    "this entry says it unpacks to far more than its data can hold; it is not read"
                        .into(),
                );
            }
            let b = Budget::new(deadline, READ_CEILING);
            let mut r = Metered {
                inner: flate2::read::DeflateDecoder::new(data),
                budget: &b,
            };
            let mut c = CrcRead::new(&mut r);
            let out = consume(Src::Stream(&mut c), Some(raw.size), req, 0, Some(&b));
            let out = c.checked(out, raw.crc);
            finish(out, &b)
        }
        m => Err(format!(
            "this entry uses compression method {m}, which is not supported (only stored and deflate)"
        )),
    }
}

/// A reader that sums the CRC-32 of what passes and notes whether the end
/// was reached: only a read from the first byte to the last can be checked.
struct CrcRead<R> {
    inner: R,
    crc: flate2::Crc,
    eof: bool,
}

impl<R: Read> CrcRead<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            crc: flate2::Crc::new(),
            eof: false,
        }
    }

    /// `out`, with a note when the whole entry went through and its checksum
    /// is not the one the directory states (not a refusal: the bytes are the
    /// person's, and may only be damaged).
    fn checked(&self, out: Result<Out, String>, want: u32) -> Result<Out, String> {
        let mut o = out?;
        if self.eof && self.crc.sum() != want {
            let n = "the entry's checksum does not match: it may be damaged";
            o.note = Some(match o.note.take() {
                Some(x) => format!("{x}; {n}"),
                None => n.into(),
            });
        }
        Ok(o)
    }
}

impl<R: Read> Read for CrcRead<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n == 0 && !buf.is_empty() {
            self.eof = true;
        }
        self.crc.update(&buf[..n]);
        Ok(n)
    }
}

fn finish(out: Result<Out, String>, b: &Budget) -> Result<Out, String> {
    match out {
        Ok(mut o) => {
            o.inflated = b.produced.get();
            Ok(o)
        }
        Err(e) => Err(stopped_note(b, "reading")
            .map(|n| {
                format!(
                    "{n}: a call reaches at most {} MiB into a compressed entry; ask for an earlier part",
                    b.ceiling >> 20
                )
            })
            .unwrap_or(e)),
    }
}

fn read_up_to(r: &mut dyn Read, buf: &mut [u8]) -> io::Result<usize> {
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

/// Read the window `req` asks for from `src`.
fn consume(
    src: Src<'_>,
    size: Option<u64>,
    req: &ReadReq<'_>,
    inflated: u64,
    budget: Option<&Budget>,
) -> Result<Out, String> {
    let io_err = |e: io::Error| match budget.and_then(|b| stopped_note(b, "reading")) {
        // `finish` words it.
        Some(n) => n,
        None => format!("could not read the data: {e}"),
    };
    let mut head = vec![0u8; SNIFF];
    // The first bytes, whatever the source: they decide text or data.
    let (sample, mut rest): (Vec<u8>, Src<'_>) = match src {
        Src::Raw(b) => (b[..b.len().min(SNIFF)].to_vec(), Src::Raw(b)),
        Src::Stream(r) => {
            let n = read_up_to(r, &mut head).map_err(io_err)?;
            head.truncate(n);
            (head.clone(), Src::Stream(r))
        }
    };
    let text = matches!(req.mode, Mode::Auto) && is_text(&sample);
    let mode = if text {
        "text"
    } else if matches!(req.mode, Mode::Strings) {
        "strings"
    } else {
        "hex"
    };
    // A reader over the whole data again: the sample, then the rest.
    let mut full: Box<dyn Read + '_> = match &mut rest {
        Src::Raw(b) => Box::new(*b),
        Src::Stream(r) => Box::new(io::Cursor::new(sample.clone()).chain(&mut **r)),
    };
    let mut out = match mode {
        "text" => read_text(&mut *full, req).map_err(io_err)?,
        "strings" => read_strings(&mut *full, req).map_err(io_err)?,
        _ => read_hex(&mut *full, req).map_err(io_err)?,
    };
    out.size = size;
    out.inflated = inflated;
    Ok(out)
}

fn skip(r: &mut dyn Read, mut n: u64) -> io::Result<bool> {
    let mut buf = [0u8; 16 * 1024];
    while n > 0 {
        let want = buf.len().min(usize::try_from(n).unwrap_or(usize::MAX));
        let got = r.read(&mut buf[..want])?;
        if got == 0 {
            return Ok(false);
        }
        n -= got as u64;
    }
    Ok(true)
}

fn read_hex(r: &mut dyn Read, req: &ReadReq<'_>) -> io::Result<Out> {
    let from = req.offset.unwrap_or(0);
    let limit = req.limit.unwrap_or(HEX_DEFAULT).clamp(1, HEX_MAX);
    if !skip(r, from)? {
        return Ok(Out {
            mode: "hex",
            text: String::new(),
            from,
            shown: 0,
            next: None,
            size: None,
            inflated: 0,
            note: Some("the offset is past the end of the data".into()),
        });
    }
    let mut buf = vec![0u8; usize::try_from(limit).unwrap_or(0)];
    let n = read_up_to(r, &mut buf)?;
    buf.truncate(n);
    // One more byte tells whether there is anything after the window.
    let mut peek = [0u8; 1];
    let more = n as u64 == limit && r.read(&mut peek)? > 0;
    let mut text = String::new();
    for (i, row) in buf.chunks(16).enumerate() {
        if i > 0 {
            text.push('\n');
        }
        text.push_str(&format!("{:08x} ", from + (i * 16) as u64));
        for j in 0..16 {
            if j == 8 {
                text.push(' ');
            }
            match row.get(j) {
                Some(b) => text.push_str(&format!(" {b:02x}")),
                None => text.push_str("   "),
            }
        }
        text.push_str("  |");
        text.extend(row.iter().map(|b| {
            if (0x20..0x7f).contains(b) {
                *b as char
            } else {
                '.'
            }
        }));
        text.push('|');
    }
    Ok(Out {
        mode: "hex",
        text,
        from,
        shown: n as u64,
        next: more.then_some(from + n as u64),
        size: None,
        inflated: 0,
        note: None,
    })
}

fn read_strings(r: &mut dyn Read, req: &ReadReq<'_>) -> io::Result<Out> {
    let from = req.offset.unwrap_or(0);
    let scan = req
        .limit
        .unwrap_or(STRINGS_SCAN_DEFAULT)
        .clamp(1, STRINGS_SCAN_MAX);
    if !skip(r, from)? {
        return Ok(Out {
            mode: "strings",
            text: String::new(),
            from,
            shown: 0,
            next: None,
            size: None,
            inflated: 0,
            note: Some("the offset is past the end of the data".into()),
        });
    }
    let mut text = String::new();
    let mut run = String::new();
    let mut run_at = from;
    let mut at = from;
    let mut end = from;
    let mut full = false;
    let mut eof = false;
    let mut buf = [0u8; 16 * 1024];
    let mut left = scan;
    let flush = |run: &mut String, run_at: u64, text: &mut String| -> bool {
        // A run of at least four characters is a string.
        let ok = run.chars().count() >= 4;
        let mut full = false;
        if ok {
            let line = format!("{run_at:08x}  {run}");
            if text.len() + line.len() + 1 > STRINGS_CAP {
                full = true;
            } else {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&line);
            }
        }
        run.clear();
        full
    };
    'scan: while left > 0 {
        let want = buf.len().min(usize::try_from(left).unwrap_or(usize::MAX));
        let n = r.read(&mut buf[..want])?;
        if n == 0 {
            eof = true;
            break;
        }
        left -= n as u64;
        for &b in &buf[..n] {
            let printable = (0x20..0x7f).contains(&b) || b == b'\t';
            if printable {
                if run.is_empty() {
                    run_at = at;
                }
                run.push(b as char);
                // A very long run is cut into pieces.
                if run.len() >= 200 {
                    if flush(&mut run, run_at, &mut text) {
                        full = true;
                        end = run_at;
                        break 'scan;
                    }
                    end = at + 1;
                }
            } else if !run.is_empty() {
                if flush(&mut run, run_at, &mut text) {
                    full = true;
                    end = run_at;
                    break 'scan;
                }
                end = at;
            } else {
                end = at;
            }
            at += 1;
        }
    }
    if !full && !run.is_empty() && flush(&mut run, run_at, &mut text) {
        full = true;
        end = run_at;
    }
    let next = if full {
        Some(end)
    } else if eof {
        None
    } else {
        let mut peek = [0u8; 1];
        (r.read(&mut peek)? > 0).then_some(at)
    };
    let shown = text.lines().count() as u64;
    Ok(Out {
        mode: "strings",
        text,
        from,
        shown,
        next,
        size: None,
        inflated: 0,
        note: None,
    })
}

fn read_text(r: &mut dyn Read, req: &ReadReq<'_>) -> io::Result<Out> {
    let first = req.offset.unwrap_or(1).max(1);
    let max_lines = req.limit.unwrap_or(LINES_DEFAULT).clamp(1, LINES_MAX);
    let last = first.saturating_add(max_lines); // exclusive
    let mut lines: Vec<String> = Vec::new();
    let mut chars = 0usize;
    let mut lineno = 1u64;
    let mut cur: Vec<u8> = Vec::new();
    let mut over = false;
    let mut next: Option<u64> = None;
    let mut done = false;
    let mut buf = [0u8; 64 * 1024];
    let keep = LINE_CHARS * 4;
    let mut first_chunk = true;
    // A finished line: kept when it is in the window; false when the window
    // is full (the caller stops after noting where to go on).
    let finish_line = |cur: &mut Vec<u8>,
                       over: &mut bool,
                       lineno: u64,
                       lines: &mut Vec<String>,
                       chars: &mut usize|
     -> bool {
        if lineno >= first && lineno < last {
            let mut s = String::from_utf8_lossy(cur).into_owned();
            if s.ends_with('\r') {
                s.pop();
            }
            let cut = s.chars().count() > LINE_CHARS || *over;
            let mut s: String = s.chars().take(LINE_CHARS).collect();
            if cut {
                s.push_str(" [line cut]");
            }
            let row = format!("{lineno}\t{s}");
            if *chars + row.len() + 1 > TEXT_CAP && !lines.is_empty() {
                cur.clear();
                *over = false;
                return false;
            }
            *chars += row.len() + 1;
            lines.push(row);
        }
        cur.clear();
        *over = false;
        true
    };
    'outer: loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let mut chunk = &buf[..n];
        if first_chunk {
            first_chunk = false;
            chunk = chunk.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(chunk);
        }
        for &b in chunk {
            if done {
                // Something after the last line shown: there is more.
                next = Some(lineno);
                break 'outer;
            }
            if b == b'\n' {
                if !finish_line(&mut cur, &mut over, lineno, &mut lines, &mut chars) {
                    next = Some(lineno);
                    break 'outer;
                }
                lineno += 1;
                if lineno >= last {
                    done = true;
                }
            } else if lineno >= first && lineno < last {
                if cur.len() < keep {
                    cur.push(b);
                } else {
                    over = true;
                }
            }
        }
    }
    if next.is_none() && !cur.is_empty() && lineno >= first && lineno < last {
        // A last line with no newline.
        if !finish_line(&mut cur, &mut over, lineno, &mut lines, &mut chars) {
            next = Some(lineno);
        }
    }
    let shown = lines.len() as u64;
    let note = (shown == 0).then(|| "there are no lines at that offset".to_string());
    Ok(Out {
        mode: "text",
        text: lines.join("\n"),
        from: first,
        shown,
        next,
        size: None,
        inflated: 0,
        note,
    })
}

#[cfg(test)]
mod fuzz;
#[cfg(test)]
mod tests;
