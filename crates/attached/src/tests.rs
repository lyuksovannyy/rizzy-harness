use std::io::Write;
use std::time::{Duration, Instant};

use super::*;

fn soon() -> Instant {
    Instant::now() + Duration::from_secs(20)
}

// -- builders ----------------------------------------------------------------

struct Ent<'a> {
    name: &'a [u8],
    /// What the entry holds (uncompressed).
    data: &'a [u8],
    deflate: bool,
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

/// A zip made by hand. `declared` overrides the uncompressed size the
/// central directory states (a lying header).
fn zip_with(ents: &[Ent<'_>], declared: Option<u64>) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (i, e) in ents.iter().enumerate() {
        let body = if e.deflate { deflate(e.data) } else { e.data.to_vec() };
        let at = out.len() as u32;
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&[20, 0, 0, 0]);
        out.extend_from_slice(&(if e.deflate { 8u16 } else { 0 }).to_le_bytes());
        out.extend_from_slice(&[0; 4]); // time, date
        out.extend_from_slice(&0u32.to_le_bytes()); // crc
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&(e.data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(e.name);
        out.extend_from_slice(&body);
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&[20, 0, 20, 0, 0, 0]);
        central.extend_from_slice(&(if e.deflate { 8u16 } else { 0 }).to_le_bytes());
        central.extend_from_slice(&[0; 4]);
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&(body.len() as u32).to_le_bytes());
        let size = declared.filter(|_| i == 0).unwrap_or(e.data.len() as u64);
        central.extend_from_slice(&(size as u32).to_le_bytes());
        central.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0; 8]); // extra, comment, disk, internal
        let dir = if e.name.ends_with(b"/") { 0x10u32 } else { 0 };
        central.extend_from_slice(&dir.to_le_bytes());
        central.extend_from_slice(&at.to_le_bytes());
        central.extend_from_slice(e.name);
    }
    end(out, central, ents.len())
}

fn end(mut out: Vec<u8>, central: Vec<u8>, n: usize) -> Vec<u8> {
    let off = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(n.min(0xFFFF) as u16).to_le_bytes());
    out.extend_from_slice(&(n.min(0xFFFF) as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn zip(ents: &[(&str, &[u8])]) -> Vec<u8> {
    let e: Vec<Ent> = ents
        .iter()
        .map(|(n, d)| Ent {
            name: n.as_bytes(),
            data: d,
            deflate: true,
        })
        .collect();
    zip_with(&e, None)
}

fn tar_with(f: impl FnOnce(&mut tar::Builder<Vec<u8>>)) -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    f(&mut b);
    b.into_inner().unwrap()
}

fn file_header(path: &str, size: u64, ty: tar::EntryType) -> tar::Header {
    let mut h = tar::Header::new_gnu();
    h.set_path(path).unwrap();
    h.set_size(size);
    h.set_entry_type(ty);
    h.set_mode(0o644);
    h.set_cksum();
    h
}

fn gz(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

fn req(entry: Option<&str>) -> ReadReq<'_> {
    ReadReq {
        entry,
        offset: None,
        limit: None,
        mode: Mode::Auto,
    }
}

fn zeros(n: usize) -> Vec<u8> {
    vec![0u8; n]
}

// -- kinds and text ------------------------------------------------------------

#[test]
fn kinds_are_told_by_the_first_bytes() {
    assert_eq!(detect(&zip(&[("a", b"x")])), Kind::Zip);
    assert_eq!(detect(&gz(b"hello")), Kind::Gz);
    let t = tar_with(|b| {
        b.append(&file_header("a.txt", 2, tar::EntryType::Regular), &b"hi"[..])
            .unwrap()
    });
    assert_eq!(detect(&t), Kind::Tar);
    assert_eq!(detect(&gz(&t)), Kind::TarGz);
    assert_eq!(detect(b"just words\n"), Kind::Plain);
    let d = describe(&zip(&[("a", b"x")]));
    assert_eq!(d.label, "ZIP archive");
    assert!(!d.text);
    let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0];
    let d = describe(&png);
    assert_eq!(d.mime.as_deref(), Some("image/png"));
    assert!(describe("héllo\n".as_bytes()).text);
}

#[test]
fn text_is_valid_utf8_without_nul() {
    assert!(is_text("héllo wörld\n".as_bytes()));
    assert!(is_text(b"\xEF\xBB\xBFwith a bom"));
    assert!(!is_text(b"a\0b"));
    assert!(!is_text(b"\xff\xfe\x00a"));
    assert!(!is_text(&[0x80, 0x81, 0x82, 0x83, 0x84]));
    // A sample cut in the middle of a character is still text.
    let s = "é".as_bytes();
    assert!(is_text(&[b'a', s[0]]));
    assert!(is_text(b""));
}

// -- zip ------------------------------------------------------------------------

#[test]
fn a_zip_is_listed_and_a_text_entry_read_as_numbered_lines() {
    let z = zip(&[
        ("hello.txt", b"one\ntwo\nthree\n"),
        ("data/", b""),
        ("data/blob.bin", &[0u8, 1, 2, 3, 250, 251]),
    ]);
    let l = list(&z, 0, 500, soon()).unwrap();
    assert_eq!(l.kind, Kind::Zip);
    assert!(l.complete && !l.more());
    let paths: Vec<_> = l.entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, ["hello.txt", "data/", "data/blob.bin"]);
    assert_eq!(l.entries[1].kind, EntryKind::Dir);
    assert_eq!(l.entries[0].size, Some(14));

    let o = read(&z, &req(Some("hello.txt")), soon()).unwrap();
    assert_eq!(o.mode, "text");
    assert_eq!(o.text, "1\tone\n2\ttwo\n3\tthree");
    assert_eq!(o.next, None);
    assert_eq!(o.size, Some(14));

    let mut r = req(Some("hello.txt"));
    r.offset = Some(2);
    r.limit = Some(1);
    let o = read(&z, &r, soon()).unwrap();
    assert_eq!(o.text, "2\ttwo");
    assert_eq!(o.next, Some(3));

    // A binary entry is a hex dump with its printable side.
    let o = read(&z, &req(Some("data/blob.bin")), soon()).unwrap();
    assert_eq!(o.mode, "hex");
    assert!(o.text.starts_with("00000000  00 01 02 03 fa fb"), "{}", o.text);
    assert!(o.text.ends_with("|......|"), "{}", o.text);

    assert!(read(&z, &req(Some("nope")), soon()).unwrap_err().contains("no entry"));
    assert!(read(&z, &req(Some("data/")), soon()).unwrap_err().contains("folder"));
}

#[test]
fn a_stored_entry_is_read_from_its_bytes() {
    let z = zip_with(
        &[Ent {
            name: b"s.txt",
            data: b"stored\ntext\n",
            deflate: false,
        }],
        None,
    );
    let o = read(&z, &req(Some("s.txt")), soon()).unwrap();
    assert_eq!(o.text, "1\tstored\n2\ttext");
    assert_eq!(o.inflated, 0);
}

#[test]
fn hostile_entry_names_are_cleaned_and_still_find_their_entry() {
    let long = format!("{}f", "d/".repeat(2047));
    let names: Vec<(String, &[u8])> = vec![
        ("../../etc/passwd".into(), b"a"),
        ("/etc/shadow".into(), b"b"),
        ("nul\0byte.txt".into(), b"c"),
        ("ctl\x1b[31m\r\nred.txt".into(), b"d"),
        ("rtl\u{202E}gnp.exe".into(), b"e"),
        (long.clone(), b"f"),
        ("quote\"[harness] x.txt".into(), b"g"),
    ];
    let ents: Vec<Ent> = names
        .iter()
        .map(|(n, d)| Ent {
            name: n.as_bytes(),
            data: d,
            deflate: true,
        })
        .collect();
    let z = zip_with(&ents, None);
    let l = list(&z, 0, 500, soon()).unwrap();
    assert_eq!(l.entries.len(), 7);
    for e in &l.entries {
        assert!(
            !e.path.chars().any(|c| c.is_control() || c == '\u{202E}'),
            "{:?}",
            e.path
        );
        assert!(e.path.chars().count() <= PATH_CHARS, "{}", e.path.len());
        // Each is found again by the path the listing shows.
        let o = read(&z, &req(Some(&e.path)), soon()).unwrap();
        assert!(o.text.starts_with("1\t"), "{o:?}");
    }
    assert_eq!(l.entries[0].path, "../../etc/passwd");
    assert_eq!(l.entries[2].path, "nul byte.txt");
    assert_eq!(l.entries[4].path, "rtlgnp.exe");
    assert!(l.entries[5].path.ends_with('\u{2026}'));
    // Cleaning is stable: the cleaned path cleans to itself.
    for e in &l.entries {
        assert_eq!(clean_path(&e.path), e.path);
    }
}

/// A zeros file of 10 MiB packs to about 50 KB. Reading it as hex or
/// text produces a window, never the file.
#[test]
fn a_ten_mib_file_of_zeros_is_not_inflated_beyond_what_is_asked() {
    let z = zip(&[("zeros.bin", &zeros(10 << 20))]);
    assert!(z.len() < 60_000, "{}", z.len());
    let o = read(&z, &req(Some("zeros.bin")), soon()).unwrap();
    assert_eq!(o.mode, "hex");
    assert_eq!(o.shown, HEX_DEFAULT);
    assert!(o.inflated <= 2 * SNIFF as u64, "{}", o.inflated);
    assert_eq!(o.next, Some(HEX_DEFAULT));
    let mut r = req(Some("zeros.bin"));
    r.mode = Mode::Strings;
    r.limit = Some(1 << 20);
    let o = read(&z, &r, soon()).unwrap();
    assert!(o.inflated <= SNIFF as u64 + (1 << 20) + 1, "{}", o.inflated);
    assert!(o.text.is_empty());
    // A text file of the same size: a window of lines, not the file.
    let big = "line of text\n".repeat((10 << 20) / 13);
    let z = zip(&[("big.txt", big.as_bytes())]);
    let o = read(&z, &req(Some("big.txt")), soon()).unwrap();
    assert_eq!(o.shown, LINES_DEFAULT);
    assert!(o.inflated <= 128 * 1024, "{}", o.inflated);
    assert!(o.text.len() <= TEXT_CAP);
}

#[test]
fn a_lying_header_or_a_huge_declared_size_is_refused() {
    // Says 4 GiB, holds 10 bytes of data: no deflate stream gives that.
    let z = zip_with(
        &[Ent {
            name: b"liar.bin",
            data: b"0123456789",
            deflate: true,
        }],
        Some(4 << 30 >> 2),
    );
    let e = read(&z, &req(Some("liar.bin")), soon()).unwrap_err();
    assert!(e.contains("far more than its data can hold"), "{e}");
    // Stored, but the two sizes disagree.
    let mut z = zip_with(
        &[Ent {
            name: b"s",
            data: b"abcdef",
            deflate: false,
        }],
        Some(3),
    );
    let e = read(&z, &req(Some("s")), soon()).unwrap_err();
    assert!(e.contains("sizes disagree"), "{e}");
    // A real 2 GiB claim in the zip64 sense: past what one entry may be.
    let big = zip_with(
        &[Ent {
            name: b"z",
            data: &zeros(4 << 20),
            deflate: true,
        }],
        Some((MAX_DECLARED + 1).min(u32::MAX as u64)),
    );
    // (4 MiB packs to ~4 KiB: 2^30+1 is within the 1100:1 allowance only for
    // bigger data, so it is the plain refusal that answers.)
    let e = read(&big, &req(Some("z")), soon()).unwrap_err();
    assert!(e.contains("not read"), "{e}");
    z.truncate(10);
}

/// A gzip that would inflate to far more than the ceiling: a read that has
/// to go past it says so, and the stream is never inflated past it.
#[test]
fn a_stream_is_never_inflated_past_the_ceiling() {
    let b = Budget::new(soon(), 1 << 20);
    let data = gz(&zeros(64 << 20));
    let mut m = Metered {
        inner: flate2::read::GzDecoder::new(&data[..]),
        budget: &b,
    };
    let mut sink = Vec::new();
    let r = std::io::copy(&mut m, &mut sink);
    assert!(r.is_err());
    assert!(b.hit.get());
    assert_eq!(b.produced.get(), 1 << 20);
    assert_eq!(sink.len(), 1 << 20);
    // Exactly the ceiling and then the end is not a refusal.
    let b = Budget::new(soon(), 1 << 20);
    let data = gz(&zeros(1 << 20));
    let mut m = Metered {
        inner: flate2::read::GzDecoder::new(&data[..]),
        budget: &b,
    };
    let mut sink = Vec::new();
    std::io::copy(&mut m, &mut sink).unwrap();
    assert!(!b.hit.get());
}

#[test]
fn a_gz_that_never_ends_stops_at_the_ceiling_with_a_sentence() {
    // 300 MiB of lines: a text read far in has to inflate past the ceiling.
    let line = "x\n".repeat(1 << 20);
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    for _ in 0..150 {
        e.write_all(line.as_bytes()).unwrap();
    }
    let data = e.finish().unwrap();
    assert!(data.len() < 3 << 20, "{}", data.len());
    let (entry, _) = gz_entry(&data).unwrap();
    let mut r = req(Some(&entry.path));
    r.offset = Some(200_000_000);
    let started = Instant::now();
    let err = read(&data, &r, soon()).unwrap_err();
    assert!(err.contains("at most 16 MiB"), "{err}");
    assert!(started.elapsed() < Duration::from_secs(10));
    // The first lines are there.
    let o = read(&data, &req(Some(&entry.path)), soon()).unwrap();
    assert_eq!(o.shown, LINES_DEFAULT);
    assert!(o.inflated <= 128 * 1024);
    // An overdue call is stopped, with its own sentence.
    let late = Instant::now() - Duration::from_secs(1);
    let err = read(&data, &r, late).unwrap_err();
    assert!(err.contains("too long"), "{err}");
}

#[test]
fn overlapping_entries_cost_a_listing_nothing() {
    // 42.zip style: thousands of entries share one local record whose
    // packed data is a few KiB and unpacks to 10 MiB.
    let packed = deflate(&zeros(10 << 20));
    let one = zip(&[("a", &zeros(10 << 20))]);
    let local_len = 31 + packed.len();
    let mut out = one[..local_len].to_vec();
    let mut central = Vec::new();
    for i in 0..5000u32 {
        let name = format!("f{i}");
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&[20, 0, 20, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        central.extend_from_slice(&(packed.len() as u32).to_le_bytes());
        central.extend_from_slice(&(10u32 << 20).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0; 8]);
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    out = end(out, central, 5000);
    let started = Instant::now();
    let l = list(&out, 0, 500, soon()).unwrap();
    assert_eq!(l.entries.len(), 500);
    assert_eq!(l.scanned, 5000);
    assert!(started.elapsed() < Duration::from_secs(2));
    // Reading one of them produces a window.
    let o = read(&out, &req(Some("f4999")), soon()).unwrap();
    assert!(o.inflated <= 2 * SNIFF as u64, "{}", o.inflated);
}

/// An archive whose directory has more entries than may be read: at most
/// 20,000 are, the answer says there are more, and a page near the end
/// shows what was read.
#[test]
fn a_huge_directory_is_read_only_so_far() {
    let n = 220_000usize;
    let mut out = Vec::new();
    let mut central = Vec::with_capacity(n * 48);
    // One empty stored local record for all of them.
    out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
    out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out.extend_from_slice(&[0; 12]);
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.push(b'x');
    for i in 0..n {
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        central.extend_from_slice(&[0; 8]);
        central.extend_from_slice(&1u16.to_le_bytes());
        central.extend_from_slice(&[0; 8]);
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.push(b'a' + (i % 26) as u8);
    }
    let out = end(out, central, n);
    assert!(out.len() < 11 << 20, "{}", out.len());
    let started = Instant::now();
    let l = list(&out, 0, 500, soon()).unwrap();
    assert_eq!(l.entries.len(), LIST_PAGE);
    assert_eq!(l.scanned, LIST_TOTAL);
    assert!(!l.complete && l.more());
    assert!(started.elapsed() < Duration::from_secs(5));
    let l = list(&out, 19_900, 500, soon()).unwrap();
    assert_eq!(l.entries.len(), 100);
    assert!(l.more(), "the archive has entries past the ones read");
    let l = list(&out, 25_000, 500, soon()).unwrap();
    assert!(l.entries.is_empty() && l.more());
}

#[test]
fn truncated_and_corrupt_archives_are_refused_with_a_sentence() {
    let z = zip(&[("a.txt", b"hello hello hello"), ("b.txt", b"second")]);
    for cut in [0, 5, 21, z.len() / 2, z.len() - 1] {
        match list(&z[..cut], 0, 10, soon()) {
            Err(e) => assert!(!e.is_empty()),
            Ok(l) => assert!(l.note.is_some() || l.entries.len() < 2, "{cut}"),
        }
    }
    // A directory pointing outside the file.
    let mut bad = z.clone();
    let n = bad.len();
    bad[n - 6..n - 2].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(list(&bad, 0, 10, soon()).is_err());
    // Entry data that lies outside the file.
    let mut cut_data = z.clone();
    let at = cut_data.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
    cut_data[at + 42..at + 46].copy_from_slice(&0x00ff_ffffu32.to_le_bytes());
    assert!(read(&cut_data, &req(Some("a.txt")), soon()).is_err());
    // An unsupported method and an encrypted entry.
    let mut m = z.clone();
    let at = m.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
    m[at + 10] = 14;
    let e = read(&m, &req(Some("a.txt")), soon()).unwrap_err();
    assert!(e.contains("method 14"), "{e}");
    let mut enc = z.clone();
    enc[at + 8] = 1;
    let e = read(&enc, &req(Some("a.txt")), soon()).unwrap_err();
    assert!(e.contains("encrypted"), "{e}");
    // Not an archive at all.
    assert!(list(b"plain words", 0, 10, soon()).is_err());
    // Garbage that starts like a zip.
    assert!(list(b"PK\x03\x04garbage", 0, 10, soon()).is_err());
}

// -- tar and gz -------------------------------------------------------------------

fn demo_tar() -> Vec<u8> {
    tar_with(|b| {
        b.append(&file_header("src/", 0, tar::EntryType::Directory), &b""[..])
            .unwrap();
        b.append(
            &file_header("src/main.rs", 13, tar::EntryType::Regular),
            &b"fn main() {}\n"[..],
        )
        .unwrap();
        let mut h = tar::Header::new_gnu();
        h.set_path("link").unwrap();
        h.set_entry_type(tar::EntryType::Symlink);
        h.set_link_name("/etc/passwd").unwrap();
        h.set_size(0);
        h.set_cksum();
        b.append(&h, &b""[..]).unwrap();
        let mut h = tar::Header::new_gnu();
        h.set_path("hard").unwrap();
        h.set_entry_type(tar::EntryType::Link);
        h.set_link_name("src/main.rs").unwrap();
        h.set_size(0);
        h.set_cksum();
        b.append(&h, &b""[..]).unwrap();
        let mut h = tar::Header::new_gnu();
        h.set_path("dev/null").unwrap();
        h.set_entry_type(tar::EntryType::Char);
        h.set_size(0);
        h.set_cksum();
        b.append(&h, &b""[..]).unwrap();
        let mut h = tar::Header::new_gnu();
        h.set_path("dev/pipe").unwrap();
        h.set_entry_type(tar::EntryType::Fifo);
        h.set_size(0);
        h.set_cksum();
        b.append(&h, &b""[..]).unwrap();
    })
}

#[test]
fn a_tar_lists_links_and_devices_as_what_they_are_and_never_follows_them() {
    for data in [demo_tar(), gz(&demo_tar())] {
        let l = list(&data, 0, 500, soon()).unwrap();
        let kinds: Vec<_> = l.entries.iter().map(|e| (&e.path[..], e.kind.clone())).collect();
        assert_eq!(
            kinds,
            vec![
                ("src/", EntryKind::Dir),
                ("src/main.rs", EntryKind::File),
                ("link", EntryKind::Link("/etc/passwd".into())),
                ("hard", EntryKind::Link("src/main.rs".into())),
                ("dev/null", EntryKind::Other("character device")),
                ("dev/pipe", EntryKind::Other("pipe")),
            ]
        );
        let o = read(&data, &req(Some("src/main.rs")), soon()).unwrap();
        assert_eq!(o.text, "1\tfn main() {}");
        let e = read(&data, &req(Some("link")), soon()).unwrap_err();
        assert!(e.contains("never followed") && e.contains("/etc/passwd"), "{e}");
        assert!(read(&data, &req(Some("hard")), soon()).unwrap_err().contains("never followed"));
        assert!(read(&data, &req(Some("dev/null")), soon()).unwrap_err().contains("character device"));
        assert!(read(&data, &req(Some("src/")), soon()).unwrap_err().contains("folder"));
    }
}

#[test]
fn a_tar_with_hostile_names_and_a_truncated_tar_are_handled() {
    let t = tar_with(|b| {
        for n in ["../../x", "/abs/path", "a\u{202E}b"] {
            let mut h = tar::Header::new_gnu();
            h.as_gnu_mut().unwrap().name[..n.len()].copy_from_slice(n.as_bytes());
            h.set_size(1);
            h.set_entry_type(tar::EntryType::Regular);
            h.set_mode(0o644);
            h.set_cksum();
            b.append(&h, &b"x"[..]).unwrap();
        }
    });
    let l = list(&t, 0, 500, soon()).unwrap();
    let p: Vec<_> = l.entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(p, ["../../x", "/abs/path", "ab"]);
    let cut = &demo_tar()[..1200];
    let l = list(cut, 0, 500, soon()).unwrap();
    assert!(l.note.is_some() || !l.entries.is_empty());
    let gcut = gz(&demo_tar());
    let gcut = &gcut[..gcut.len() / 2];
    // A truncated gzip is a damaged one, said so.
    let l = list(gcut, 0, 500, soon());
    assert!(l.is_err() || l.unwrap().note.is_some());
}

#[test]
fn a_tar_gz_of_zeros_is_scanned_only_to_the_ceiling() {
    // A tar header for a 100 MiB file whose data is 100 MiB of zeros, in a
    // gzip of a few hundred KB: a read far in stops at the ceiling.
    let mut t = Vec::new();
    let mut h = tar::Header::new_gnu();
    h.set_path("big").unwrap();
    h.set_size(100 << 20);
    h.set_entry_type(tar::EntryType::Regular);
    h.set_cksum();
    t.extend_from_slice(h.as_bytes());
    t.extend_from_slice(&zeros(100 << 20));
    let data = gz(&t);
    let started = Instant::now();
    let l = list(&data, 0, 500, soon()).unwrap();
    assert_eq!(l.entries.len(), 1);
    assert_eq!(l.entries[0].size, Some(100 << 20));
    assert!(started.elapsed() < Duration::from_secs(10));
    let e = read(&data, &req(Some("big")), soon()).unwrap();
    assert_eq!(e.mode, "hex");
    assert_eq!(e.size, Some(100 << 20));
    assert!(e.inflated <= READ_CEILING);
    let mut r = req(Some("big"));
    r.offset = Some(64 << 20);
    assert!(read(&data, &r, soon()).unwrap_err().contains("16 MiB"));
    // An entry that says 4 GiB is not read at all.
    let mut t = Vec::new();
    let mut h = tar::Header::new_gnu();
    h.set_path("huge").unwrap();
    h.set_size(4 << 30);
    h.set_entry_type(tar::EntryType::Regular);
    h.set_cksum();
    t.extend_from_slice(h.as_bytes());
    t.extend_from_slice(&zeros(1 << 20));
    let data = gz(&t);
    let e = read(&data, &req(Some("huge")), soon()).unwrap_err();
    assert!(e.contains("not read"), "{e}");
}

#[test]
fn a_gz_is_one_entry() {
    let g = gz(b"alpha\nbeta\n");
    let l = list(&g, 0, 10, soon()).unwrap();
    assert_eq!(l.kind, Kind::Gz);
    assert_eq!(l.entries.len(), 1);
    assert_eq!(l.entries[0].size, Some(11));
    assert!(l.complete);
    let o = read(&g, &req(Some(&l.entries[0].path)), soon()).unwrap();
    assert_eq!(o.text, "1\talpha\n2\tbeta");
    // With a name in its header.
    let mut e = flate2::GzBuilder::new()
        .filename("notes.txt")
        .write(Vec::new(), flate2::Compression::fast());
    e.write_all(b"x\n").unwrap();
    let named = e.finish().unwrap();
    let l = list(&named, 0, 10, soon()).unwrap();
    assert_eq!(l.entries[0].path, "notes.txt");
}

#[test]
fn an_archive_inside_an_archive_is_read_as_bytes() {
    let inner = zip(&[("deep.txt", b"deep text")]);
    let outer = zip(&[("inner.zip", &inner)]);
    let o = read(&outer, &req(Some("inner.zip")), soon()).unwrap();
    assert_eq!(o.mode, "hex");
    assert!(o.text.contains("|PK"), "{}", o.text);
    // There is no entry path through it.
    assert!(read(&outer, &req(Some("inner.zip/deep.txt")), soon()).is_err());
}

// -- reading windows ------------------------------------------------------------------

#[test]
fn text_reads_page_by_lines_with_a_bounded_window() {
    let body: String = (1..=500).map(|i| format!("row {i}\n")).collect();
    let mut r = req(None);
    r.limit = Some(3);
    let o = read(body.as_bytes(), &r, soon()).unwrap();
    assert_eq!(o.text, "1\trow 1\n2\trow 2\n3\trow 3");
    assert_eq!(o.next, Some(4));
    r.offset = Some(499);
    r.limit = Some(50);
    let o = read(body.as_bytes(), &r, soon()).unwrap();
    assert_eq!(o.text, "499\trow 499\n500\trow 500");
    assert_eq!(o.next, None);
    r.offset = Some(900);
    let o = read(body.as_bytes(), &r, soon()).unwrap();
    assert!(o.text.is_empty() && o.note.is_some());
    // CRLF, a BOM, a last line without a newline.
    let o = read(b"\xEF\xBB\xBFa\r\nb\r\nc", &req(None), soon()).unwrap();
    assert_eq!(o.text, "1\ta\n2\tb\n3\tc");
    // A very long line is cut, and the total stays under the cap.
    let long = "y".repeat(100_000);
    let o = read(long.as_bytes(), &req(None), soon()).unwrap();
    assert!(o.text.ends_with("[line cut]"));
    assert!(o.text.len() < LINE_CHARS + 40);
    let wide = format!("{}\n", "z".repeat(1900)).repeat(100);
    let o = read(wide.as_bytes(), &req(None), soon()).unwrap();
    assert!(o.text.len() <= TEXT_CAP, "{}", o.text.len());
    assert!(o.next.is_some(), "the cap stopped the read before the limit");
}

#[test]
fn hex_and_strings_are_bounded_windows() {
    let data: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8 | 0x80).collect();
    let mut r = req(None);
    r.limit = Some(100_000);
    let o = read(&data, &r, soon()).unwrap();
    assert_eq!(o.mode, "hex");
    assert_eq!(o.shown, HEX_MAX);
    assert_eq!(o.next, Some(HEX_MAX));
    r.offset = Some(9990);
    let o = read(&data, &r, soon()).unwrap();
    assert_eq!(o.shown, 10);
    assert_eq!(o.next, None);
    r.offset = Some(20_000);
    let o = read(&data, &r, soon()).unwrap();
    assert!(o.note.is_some());
    // strings
    let mut blob = vec![0u8; 100];
    blob.extend_from_slice(b"hello world");
    blob.extend_from_slice(&[0, 1, 2]);
    blob.extend_from_slice(b"ab");
    blob.extend_from_slice(&[0xff]);
    blob.extend_from_slice(b"api_key=sk-123456");
    let mut r = req(None);
    r.mode = Mode::Strings;
    let o = read(&blob, &r, soon()).unwrap();
    assert_eq!(o.mode, "strings");
    assert_eq!(o.text, "00000064  hello world\n00000075  api_key=sk-123456");
    // The strings output is capped.
    let many: Vec<u8> = (0..5000).flat_map(|i| format!("string number {i}\0").into_bytes()).collect();
    let o = read(&many, &r, soon()).unwrap();
    assert!(o.text.len() <= STRINGS_CAP);
    assert!(o.next.is_some());
}

#[test]
fn an_entry_path_is_needed_only_for_an_archive() {
    assert!(read(b"text\n", &req(Some("x")), soon()).unwrap_err().contains("not an archive"));
    // An archive read without an entry is read as bytes.
    let z = zip(&[("a", b"x")]);
    assert_eq!(read(&z, &req(None), soon()).unwrap().mode, "hex");
}
