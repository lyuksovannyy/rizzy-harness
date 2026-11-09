//! Stored files on disk and how they are served (docs/media.md).
//!
//! Each blob is one file, `RZB1` + a 7-byte random nonce prefix, then the
//! plaintext in 64 KiB segments sealed with AES-256-GCM in the STREAM
//! construction: segment `i`'s nonce is the prefix, `i` big-endian and a
//! last-segment flag, and the AAD is the blob id. Reordered, dropped or
//! swapped segments and a file renamed to another id all fail to open. The
//! key is random per blob and lives only in master.db, so deleting the row
//! (with `secure_delete` and a WAL truncate) leaves the file unreadable.
//!
//! Serving never trusts a name or a declared type: the sniffed `display`
//! decides whether a file is shown as its own type, only to an `<img>`, or
//! offered as a download, and every response carries a sandboxing CSP.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use aws_lc_rs::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use harness_store::blobs::BlobRow;
use sha2::{Digest, Sha256};

/// Plaintext bytes per sealed segment.
pub const SEGMENT: usize = 64 * 1024;
const TAG: usize = 16;
const MAGIC: &[u8; 4] = b"RZB1";
const PREFIX: usize = 7;
const HEADER: u64 = (MAGIC.len() + PREFIX) as u64;

/// What every stored file is served with: nothing it contains may run,
/// load from elsewhere or reach the app.
pub const CSP: &str = "default-src 'none'; img-src 'self' data:; media-src 'self'; style-src 'unsafe-inline'; sandbox";

fn bad_data(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

/// Segments a file of `size` plaintext bytes has (an empty file has one,
/// empty and flagged last).
fn segments(size: u64) -> u64 {
    size.div_ceil(SEGMENT as u64).max(1)
}

/// The file's length on disk for `size` plaintext bytes.
pub fn sealed_len(size: u64) -> u64 {
    HEADER + size + TAG as u64 * segments(size)
}

fn nonce(prefix: &[u8; PREFIX], segment: u64, last: bool) -> io::Result<Nonce> {
    let segment = u32::try_from(segment).map_err(|_| bad_data("file too large"))?;
    let mut n = [0u8; 12];
    n[..PREFIX].copy_from_slice(prefix);
    n[PREFIX..11].copy_from_slice(&segment.to_be_bytes());
    n[11] = u8::from(last);
    Ok(Nonce::assume_unique_for_key(n))
}

fn cipher(key: &[u8]) -> io::Result<LessSafeKey> {
    let k = UnboundKey::new(&AES_256_GCM, key).map_err(|_| bad_data("bad blob key"))?;
    Ok(LessSafeKey::new(k))
}

/// A blob file's path under the store's root: sharded by the id's last two
/// characters (a uuid v7 starts with the time).
pub fn path(root: &Path, blob_id: &str) -> Option<PathBuf> {
    if blob_id.len() != 32
        || !blob_id
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return None;
    }
    Some(root.join(&blob_id[30..]).join(format!("{blob_id}.rzb")))
}

/// A file being written: hashed, measured and sealed as it streams in. It
/// lives in the store's `tmp/` until committed; dropped unfinished, it is
/// deleted.
pub struct Writer {
    blob_id: String,
    file: File,
    path: PathBuf,
    key: [u8; 32],
    cipher: LessSafeKey,
    prefix: [u8; PREFIX],
    segment: u64,
    pending: Vec<u8>,
    hasher: Sha256,
    size: u64,
    head: Vec<u8>,
    head_max: usize,
    done: bool,
}

/// A written, synced file in `tmp/`, ready to commit.
#[derive(Debug)]
pub struct Written {
    pub blob_id: String,
    pub key: Vec<u8>,
    /// Lowercase hex of the plaintext's SHA-256.
    pub sha256: String,
    pub size: u64,
    /// The plaintext's first bytes, for sniffing.
    pub head: Vec<u8>,
    pub path: PathBuf,
}

impl Writer {
    /// Start `blob_id`'s file in `tmp_dir` (0600, never over another),
    /// keeping its first `head_max` bytes for sniffing.
    pub fn create(tmp_dir: &Path, blob_id: &str, head_max: usize) -> io::Result<Self> {
        let path = tmp_dir.join(format!("{blob_id}.part"));
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts.open(&path)?;
        let mut key = [0u8; 32];
        let mut prefix = [0u8; PREFIX];
        getrandom::fill(&mut key).map_err(|e| io::Error::other(e.to_string()))?;
        getrandom::fill(&mut prefix).map_err(|e| io::Error::other(e.to_string()))?;
        let mut w = Self {
            blob_id: blob_id.to_string(),
            cipher: cipher(&key)?,
            file: {
                file.write_all(MAGIC)?;
                file.write_all(&prefix)?;
                file
            },
            path,
            key,
            prefix,
            segment: 0,
            pending: Vec::with_capacity(SEGMENT + TAG),
            hasher: Sha256::new(),
            size: 0,
            head: Vec::new(),
            head_max,
            done: false,
        };
        w.head.reserve(head_max.min(64 * 1024));
        Ok(w)
    }

    /// Bytes written so far.
    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn write(&mut self, mut data: &[u8]) -> io::Result<()> {
        self.hasher.update(data);
        self.size += data.len() as u64;
        if self.head.len() < self.head_max {
            let take = (self.head_max - self.head.len()).min(data.len());
            self.head.extend_from_slice(&data[..take]);
        }
        while !data.is_empty() {
            // A full segment is sealed only once more data shows it is not
            // the last one.
            if self.pending.len() == SEGMENT {
                self.seal(false)?;
            }
            let take = (SEGMENT - self.pending.len()).min(data.len());
            self.pending.extend_from_slice(&data[..take]);
            data = &data[take..];
        }
        Ok(())
    }

    fn seal(&mut self, last: bool) -> io::Result<()> {
        let n = nonce(&self.prefix, self.segment, last)?;
        self.cipher
            .seal_in_place_append_tag(n, Aad::from(self.blob_id.as_bytes()), &mut self.pending)
            .map_err(|_| io::Error::other("sealing failed"))?;
        self.file.write_all(&self.pending)?;
        self.pending.clear();
        self.segment += 1;
        Ok(())
    }

    /// Seal the last segment and sync the file.
    pub fn finish(mut self) -> io::Result<Written> {
        self.seal(true)?;
        self.file.sync_all()?;
        self.done = true;
        Ok(Written {
            blob_id: std::mem::take(&mut self.blob_id),
            key: self.key.to_vec(),
            sha256: hex::encode(std::mem::take(&mut self.hasher).finalize()),
            size: self.size,
            head: std::mem::take(&mut self.head),
            path: std::mem::take(&mut self.path),
        })
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        if !self.done {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// An open blob file, decrypting segments on demand.
pub struct Reader {
    file: File,
    cipher: LessSafeKey,
    prefix: [u8; PREFIX],
    blob_id: String,
    size: u64,
    buf: Vec<u8>,
}

impl Reader {
    /// Open `blob_id`'s file, which must be exactly as long as `size`
    /// plaintext bytes make it (a cut file fails here).
    pub fn open(path: &Path, blob_id: &str, key: &[u8], size: u64) -> io::Result<Self> {
        let mut file = File::open(path)?;
        if file.metadata()?.len() != sealed_len(size) {
            return Err(bad_data("stored file has the wrong length"));
        }
        let mut header = [0u8; HEADER as usize];
        file.read_exact(&mut header)?;
        if &header[..MAGIC.len()] != MAGIC {
            return Err(bad_data("not a stored file"));
        }
        let mut prefix = [0u8; PREFIX];
        prefix.copy_from_slice(&header[MAGIC.len()..]);
        Ok(Self {
            file,
            cipher: cipher(key)?,
            prefix,
            blob_id: blob_id.to_string(),
            size,
            buf: Vec::with_capacity(SEGMENT + TAG),
        })
    }

    /// The plaintext of segment `i`.
    pub fn segment(&mut self, i: u64) -> io::Result<&[u8]> {
        let count = segments(self.size);
        if i >= count {
            return Err(bad_data("no such segment"));
        }
        let start = i * SEGMENT as u64;
        let len = (self.size - start).min(SEGMENT as u64) as usize;
        self.file
            .seek(SeekFrom::Start(HEADER + i * (SEGMENT + TAG) as u64))?;
        self.buf.resize(len + TAG, 0);
        self.file.read_exact(&mut self.buf)?;
        let n = nonce(&self.prefix, i, i + 1 == count)?;
        let plain = self
            .cipher
            .open_in_place(n, Aad::from(self.blob_id.as_bytes()), &mut self.buf)
            .map_err(|_| bad_data("stored file failed its integrity check"))?;
        Ok(plain)
    }

    /// Plaintext bytes `start..=end`, handed to `out` a segment at a time;
    /// only the segments the range covers are decrypted.
    pub fn range(
        &mut self,
        start: u64,
        end: u64,
        mut out: impl FnMut(&[u8]) -> io::Result<()>,
    ) -> io::Result<()> {
        if self.size == 0 || end < start || end >= self.size {
            return Ok(());
        }
        let seg = SEGMENT as u64;
        for i in start / seg..=end / seg {
            let base = i * seg;
            let plain = self.segment(i)?;
            let from = start.saturating_sub(base) as usize;
            let to = ((end - base) as usize + 1).min(plain.len());
            out(&plain[from..to])?;
        }
        Ok(())
    }
}

/// A whole small blob's plaintext (an image for a prompt; never a large
/// file, which is streamed).
pub fn read_all(path: &Path, blob_id: &str, key: &[u8], size: u64) -> io::Result<Vec<u8>> {
    let mut r = Reader::open(path, blob_id, key, size)?;
    let mut out = Vec::with_capacity(size as usize);
    if size > 0 {
        r.range(0, size - 1, |b| {
            out.extend_from_slice(b);
            Ok(())
        })?;
    }
    Ok(out)
}

/// What a `Range` header asks for, against a file of `size` bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    /// No range, or several (served whole, 200).
    Full,
    /// Bytes `start..=end` (206).
    Part(u64, u64),
    /// Malformed or outside the file (416).
    Bad,
}

/// Parse one `bytes=` range: `a-b`, `a-` or `-n`.
pub fn parse_range(header: Option<&str>, size: u64) -> Range {
    let Some(h) = header else {
        return Range::Full;
    };
    let Some(spec) = h.trim().strip_prefix("bytes=") else {
        return Range::Bad;
    };
    if spec.contains(',') {
        return Range::Full;
    }
    let Some((a, b)) = spec.split_once('-') else {
        return Range::Bad;
    };
    let num = |s: &str| {
        let s = s.trim();
        (!s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
            .then(|| s.parse::<u64>().ok())
            .flatten()
    };
    let (a, b) = (a.trim(), b.trim());
    match (a.is_empty(), b.is_empty()) {
        (true, true) => Range::Bad,
        (true, false) => match num(b) {
            Some(n) if n > 0 && size > 0 => Range::Part(size.saturating_sub(n), size - 1),
            _ => Range::Bad,
        },
        (false, open) => {
            let Some(start) = num(a) else {
                return Range::Bad;
            };
            let end = if open { Some(u64::MAX) } else { num(b) };
            match end {
                Some(end) if start < size && end >= start => Range::Part(start, end.min(size - 1)),
                _ => Range::Bad,
            }
        }
    }
}

/// A download name: cleaned to one line without quotes, separators or
/// controls, and given the format's extension when its own last one is not
/// among the format's (`x.mp3.html` stored as mp3 → `x.mp3.html.mp3`).
pub fn download_name(name: &str, format: &str) -> String {
    let line = harness_core::text::clean_line(name, 120);
    let mut n: String = line
        .chars()
        .filter(|c| !matches!(c, '"' | '\'' | '/' | '\\' | ';'))
        .collect();
    n = n.trim().trim_start_matches('.').to_string();
    if n.is_empty() {
        n = "file".into();
    }
    let exts = crate::sniff::extensions(format);
    let lower = n.to_lowercase();
    if let Some(first) = exts.first()
        && !exts.iter().any(|e| lower.ends_with(&format!(".{e}")))
    {
        n = format!("{n}.{first}");
    }
    n
}

/// `attachment` or `inline`, with an ASCII fallback name and the full name
/// percent-encoded (RFC 6266).
fn disposition(kind: &str, name: &str) -> HeaderValue {
    let ascii: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut enc = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            enc.push(b as char);
        } else {
            enc.push_str(&format!("%{b:02X}"));
        }
    }
    HeaderValue::from_str(&format!(
        "{kind}; filename=\"{ascii}\"; filename*=UTF-8''{enc}"
    ))
    .unwrap_or_else(|_| HeaderValue::from_static("attachment"))
}

/// Does `If-None-Match` name `etag` (or `*`)?
fn not_modified(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(|t| t.trim().trim_start_matches("W/"))
        .any(|t| t == "*" || t == etag)
}

/// The headers every stored-file response carries.
fn base_headers(h: &mut HeaderMap, etag: &str) {
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CSP),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("same-origin"),
    );
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-cache"),
    );
    if let Ok(v) = HeaderValue::from_str(etag) {
        h.insert(header::ETAG, v);
    }
}

/// One stored file to send.
pub struct Serve<'a> {
    pub blob: &'a BlobRow,
    pub path: PathBuf,
    pub key: Vec<u8>,
    /// The name it is offered under (cleaned here).
    pub name: &'a str,
    /// A showing endpoint (`/content`, `/preview`) rather than a download.
    pub inline: bool,
}

/// Send a stored file with the T17 rules: its sniffed type only when it may
/// be shown here (`display = img` only to `Sec-Fetch-Dest: image`, with
/// `Vary` and a variant ETag), else `application/octet-stream` as an
/// attachment; a single `Range` → 206; `If-None-Match` → 304. The caller
/// checked access first.
pub async fn serve(headers: &HeaderMap, s: Serve<'_>) -> Response {
    let b = s.blob;
    let to_img = headers
        .get("sec-fetch-dest")
        .is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(b"image"));
    let (ctype, attach, variant) = match (s.inline, b.display.as_str()) {
        (true, "inline") => (b.mime.as_str(), false, ""),
        (true, "img") if to_img => (b.mime.as_str(), false, "-img"),
        _ => ("application/octet-stream", true, ""),
    };
    let etag = format!("\"{}{variant}\"", b.sha256);
    let mut h = HeaderMap::new();
    base_headers(&mut h, &etag);
    if s.inline && b.display == "img" {
        h.insert(header::VARY, HeaderValue::from_static("Sec-Fetch-Dest"));
    }
    if not_modified(headers, &etag) {
        return (StatusCode::NOT_MODIFIED, h).into_response();
    }
    let name = download_name(s.name, &b.format);
    h.insert(
        header::CONTENT_DISPOSITION,
        disposition(if attach { "attachment" } else { "inline" }, &name),
    );
    if attach && let Ok(v) = HeaderValue::from_str(&b.sha256) {
        h.insert("x-content-sha256", v);
    }
    h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    let range = parse_range(
        headers.get(header::RANGE).and_then(|v| v.to_str().ok()),
        b.size,
    );
    let (status, start, end) = match range {
        Range::Bad => {
            if let Ok(v) = HeaderValue::from_str(&format!("bytes */{}", b.size)) {
                h.insert(header::CONTENT_RANGE, v);
            }
            return (StatusCode::RANGE_NOT_SATISFIABLE, h).into_response();
        }
        Range::Part(a, z) => {
            if let Ok(v) = HeaderValue::from_str(&format!("bytes {a}-{z}/{}", b.size)) {
                h.insert(header::CONTENT_RANGE, v);
            }
            (StatusCode::PARTIAL_CONTENT, a, z)
        }
        Range::Full => (StatusCode::OK, 0, b.size.saturating_sub(1)),
    };
    let mut reader = match Reader::open(&s.path, &b.blob_id, &s.key, b.size) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(blob = %b.blob_id, error = %e, "cannot open a stored file");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({ "error": "the stored file is missing or damaged" })),
            )
                .into_response();
        }
    };
    let len = if b.size == 0 { 0 } else { end - start + 1 };
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    if let Ok(v) = HeaderValue::from_str(ctype) {
        h.insert(header::CONTENT_TYPE, v);
    }
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    // Decrypt off the runtime, a few segments ahead of the socket; a
    // client that goes away stops it.
    let (tx, rx) = tokio::sync::mpsc::channel::<io::Result<Bytes>>(4);
    let blob_id = b.blob_id.clone();
    tokio::task::spawn_blocking(move || {
        let res = reader.range(start, end, |chunk| {
            tx.blocking_send(Ok(Bytes::copy_from_slice(chunk)))
                .map_err(|_| io::Error::other("client went away"))
        });
        if let Err(e) = res
            && e.kind() == io::ErrorKind::InvalidData
        {
            tracing::warn!(blob = %blob_id, error = %e, "stored file failed while sending");
            let _ = tx.blocking_send(Err(e));
        }
    });
    let stream = futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    });
    (status, h, Body::from_stream(stream)).into_response()
}

/// Send a thumbnail (sniffed as WebP, JPEG or PNG when it was stored) with
/// the same headers as its file.
pub fn serve_thumb(headers: &HeaderMap, mime: &str, bytes: Vec<u8>) -> Response {
    let etag = format!("\"t-{}\"", hex::encode(Sha256::digest(&bytes)));
    let mut h = HeaderMap::new();
    base_headers(&mut h, &etag);
    if not_modified(headers, &etag) {
        return (StatusCode::NOT_MODIFIED, h).into_response();
    }
    let ctype = match crate::sniff::thumb_mime(&bytes) {
        Some(m) if m == mime => m,
        _ => "application/octet-stream",
    };
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(ctype));
    (StatusCode::OK, h, bytes).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic bytes that differ per position.
    fn data(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 31 + i / 251) as u8).collect()
    }

    fn write(dir: &Path, id: &str, bytes: &[u8], chunk: usize) -> Written {
        let mut w = Writer::create(dir, id, 64).unwrap();
        for c in bytes.chunks(chunk.max(1)) {
            w.write(c).unwrap();
        }
        if bytes.is_empty() {
            w.write(&[]).unwrap();
        }
        w.finish().unwrap()
    }

    fn read_back(w: &Written) -> Vec<u8> {
        read_all(&w.path, &w.blob_id, &w.key, w.size).unwrap()
    }

    const ID: &str = "0193f1c2a4b57d6e8f9012345678abcd";

    #[test]
    fn stream_round_trips_at_the_edges() {
        let dir = tempfile::tempdir().unwrap();
        for (i, n) in [
            0,
            1,
            SEGMENT - 1,
            SEGMENT,
            SEGMENT + 1,
            3 * SEGMENT,
            5_000_003,
        ]
        .into_iter()
        .enumerate()
        {
            let id = format!("{}{i:02}", &ID[..30]);
            let bytes = data(n);
            let w = write(dir.path(), &id, &bytes, 7919);
            assert_eq!(w.size, n as u64);
            assert_eq!(w.sha256, hex::encode(Sha256::digest(&bytes)));
            assert_eq!(
                std::fs::metadata(&w.path).unwrap().len(),
                sealed_len(n as u64)
            );
            assert_eq!(w.head, bytes[..n.min(64)]);
            assert_eq!(read_back(&w), bytes, "size {n}");
        }
    }

    #[test]
    fn ciphertext_hides_the_plaintext() {
        let dir = tempfile::tempdir().unwrap();
        let marker = b"PLAINTEXT-MARKER-0123456789";
        let bytes = marker.repeat(10_000);
        let w = write(dir.path(), ID, &bytes, 4096);
        let disk = std::fs::read(&w.path).unwrap();
        assert!(!disk.windows(marker.len()).any(|x| x == marker));
    }

    #[test]
    fn truncation_swaps_and_a_wrong_id_fail() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = data(3 * SEGMENT + 10);
        let w = write(dir.path(), ID, &bytes, SEGMENT);
        let disk = std::fs::read(&w.path).unwrap();
        let fail = |content: &[u8], id: &str, size: u64| {
            let p = dir.path().join("bad.rzb");
            std::fs::write(&p, content).unwrap();
            let mut out = Vec::new();
            match Reader::open(&p, id, &w.key, size) {
                Err(_) => true,
                Ok(mut r) => r
                    .range(0, size - 1, |c| {
                        out.extend_from_slice(c);
                        Ok(())
                    })
                    .is_err(),
            }
        };
        // Cut by one segment, the size told to match: the new last segment
        // was not sealed as last.
        let seg = SEGMENT + TAG;
        let cut = &disk[..disk.len() - (10 + TAG)];
        assert!(fail(cut, ID, 3 * SEGMENT as u64));
        // Cut without telling: the length check refuses it.
        assert!(fail(&disk[..disk.len() - 1], ID, w.size));
        // Two segments swapped.
        let mut swapped = disk.clone();
        let h = HEADER as usize;
        let (a, b) = (h..h + seg, h + seg..h + 2 * seg);
        let first = swapped[a.clone()].to_vec();
        let second = swapped[b.clone()].to_vec();
        swapped[a].copy_from_slice(&second);
        swapped[b].copy_from_slice(&first);
        assert!(fail(&swapped, ID, w.size));
        // A flipped byte.
        let mut flipped = disk.clone();
        flipped[h + 5] ^= 1;
        assert!(fail(&flipped, ID, w.size));
        // The right bytes under another blob's id (AAD).
        assert!(fail(&disk, "ffffffffffffffffffffffffffffffff", w.size));
        // And the original still reads.
        assert_eq!(read_back(&w), bytes);
    }

    #[test]
    fn random_ranges_decrypt_only_what_they_cover() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = data(4 * SEGMENT + 123);
        let w = write(dir.path(), ID, &bytes, 1000);
        let mut r = Reader::open(&w.path, &w.blob_id, &w.key, w.size).unwrap();
        let mut seed = 0x1234_5678_9ABC_DEF0u64;
        for _ in 0..200 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let a = seed % w.size;
            let z = a + (seed >> 20) % (w.size - a);
            let mut out = Vec::new();
            r.range(a, z, |c| {
                out.extend_from_slice(c);
                Ok(())
            })
            .unwrap();
            assert_eq!(out, bytes[a as usize..=z as usize], "{a}-{z}");
        }
    }

    #[test]
    fn an_unfinished_writer_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = Writer::create(dir.path(), ID, 16).unwrap();
        w.write(b"half").unwrap();
        drop(w);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn range_header_parsing() {
        use Range::*;
        let p = |h: &str, size| parse_range(Some(h), size);
        assert_eq!(parse_range(None, 10), Full);
        assert_eq!(p("bytes=0-4", 10), Part(0, 4));
        assert_eq!(p("bytes=5-", 10), Part(5, 9));
        assert_eq!(p("bytes=-3", 10), Part(7, 9));
        assert_eq!(p("bytes=-30", 10), Part(0, 9));
        assert_eq!(p("bytes=8-100", 10), Part(8, 9));
        assert_eq!(p(" bytes= 2 - 3 ", 10), Part(2, 3));
        assert_eq!(p("bytes=0-1,4-5", 10), Full);
        for bad in [
            "bytes=10-",
            "bytes=5-4",
            "bytes=-0",
            "bytes=-",
            "bytes=a-b",
            "bytes=+1-2",
            "items=0-1",
            "bytes=0",
            "bytes=1-2-3",
        ] {
            assert_eq!(p(bad, 10), Bad, "{bad}");
        }
        assert_eq!(p("bytes=0-0", 0), Bad);
        assert_eq!(p("bytes=-1", 0), Bad);
    }

    #[test]
    fn download_names_are_clean_and_typed() {
        assert_eq!(download_name("photo.jpg", "jpeg"), "photo.jpg");
        assert_eq!(download_name("photo.JPEG", "jpeg"), "photo.JPEG");
        assert_eq!(download_name("x.mp3.html", "mp3"), "x.mp3.html.mp3");
        assert_eq!(download_name("notes", "pdf"), "notes.pdf");
        assert_eq!(download_name("app.tar.gz", "tar.gz"), "app.tar.gz");
        assert_eq!(download_name("app.tgz", "tar.gz"), "app.tgz");
        assert_eq!(download_name("app", "tar.gz"), "app.tar.gz");
        assert_eq!(download_name("blob", "bin"), "blob");
        assert_eq!(
            download_name("../evil\"name\r\nX-Hdr: 1/\\.png", "png"),
            "evilname X-Hdr: 1.png"
        );
        assert_eq!(download_name("\u{202e}gnp.exe", "exe"), "gnp.exe");
        assert_eq!(download_name("", "png"), "file.png");
        let d = disposition("attachment", "Ünïcødé ☃.png");
        let s = d.to_str().unwrap();
        assert!(
            s.starts_with("attachment; filename=\"_n_c_d_ _.png\""),
            "{s}"
        );
        assert!(
            s.contains("filename*=UTF-8''%C3%9Cn%C3%AFc%C3%B8d%C3%A9%20%E2%98%83.png"),
            "{s}"
        );
    }

    #[test]
    fn paths_only_for_ids() {
        let root = Path::new("/data/blobs");
        assert_eq!(
            path(root, ID).unwrap(),
            Path::new("/data/blobs/cd/0193f1c2a4b57d6e8f9012345678abcd.rzb")
        );
        assert!(path(root, "../../etc/passwd").is_none());
        assert!(path(root, &ID.to_uppercase()).is_none());
        assert!(path(root, &ID[1..]).is_none());
    }
}
