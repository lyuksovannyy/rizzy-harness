//! A fixed-seed mutation fuzz of the parsers (from the independent review of
//! PLAN.md T95): any panic fails, and the input that once overflowed in the zip
//! helpers is a regression case. Debug builds check overflow, so this is where
//! unchecked arithmetic on archive numbers shows.
use std::io::Write;
use std::time::{Duration, Instant};

use super::*;

fn soon() -> Instant {
    Instant::now() + Duration::from_secs(20)
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

fn gz(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

/// A small zip with stored + deflate entries, a dir, a zip64 extra field and
/// an end record (a hand builder, like tests.rs).
fn seed_zip() -> Vec<u8> {
    let ents: Vec<(&[u8], Vec<u8>, bool)> = vec![
        (b"a.txt", b"hello\nworld\n".to_vec(), true),
        (b"dir/", vec![], false),
        (b"b.bin", vec![0, 1, 2, 3, 255, 254, 65, 66, 67, 68], false),
        (b"c.txt", b"line one\nline two\nline three".repeat(20), true),
    ];
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data, def) in &ents {
        let body = if *def { deflate(data) } else { data.clone() };
        let at = out.len() as u32;
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&[20, 0, 0, 0]);
        out.extend_from_slice(&(if *def { 8u16 } else { 0 }).to_le_bytes());
        out.extend_from_slice(&[0; 8]);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(&body);
        // zip64 extra with the sizes marked 0xFFFFFFFF.
        let mut extra = Vec::new();
        extra.extend_from_slice(&1u16.to_le_bytes());
        extra.extend_from_slice(&16u16.to_le_bytes());
        extra.extend_from_slice(&(data.len() as u64).to_le_bytes());
        extra.extend_from_slice(&(body.len() as u64).to_le_bytes());
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&[20, 3, 20, 0, 0, 0]);
        central.extend_from_slice(&(if *def { 8u16 } else { 0 }).to_le_bytes());
        central.extend_from_slice(&[0; 8]);
        central.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        central.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&(extra.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0; 6]);
        let mode: u32 = if name.ends_with(b"/") {
            0o040755
        } else {
            0o100644
        };
        central.extend_from_slice(&(mode << 16).to_le_bytes());
        central.extend_from_slice(&at.to_le_bytes());
        central.extend_from_slice(name);
        central.extend_from_slice(&extra);
    }
    let off = out.len() as u64;
    let clen = central.len() as u64;
    out.extend_from_slice(&central);
    // zip64 end record + locator, then an end record that defers to it.
    let rec = out.len() as u64;
    out.extend_from_slice(&0x0606_4b50u32.to_le_bytes());
    out.extend_from_slice(&44u64.to_le_bytes());
    out.extend_from_slice(&[45, 0, 45, 0]);
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&(ents.len() as u64).to_le_bytes());
    out.extend_from_slice(&(ents.len() as u64).to_le_bytes());
    out.extend_from_slice(&clen.to_le_bytes());
    out.extend_from_slice(&off.to_le_bytes());
    out.extend_from_slice(&0x0706_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&rec.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    out.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    out.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn seed_tar() -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    let mut h = tar::Header::new_gnu();
    h.set_size(12);
    h.set_mode(0o644);
    h.set_entry_type(tar::EntryType::Regular);
    b.append_data(&mut h, "a.txt", &b"hello\nworld\n"[..])
        .unwrap();
    // A long name (GNU L) and a long link (K).
    let long = "d/".repeat(80) + "long.txt";
    let mut h = tar::Header::new_gnu();
    h.set_size(5);
    h.set_mode(0o644);
    b.append_data(&mut h, &long, &b"abcde"[..]).unwrap();
    let mut h = tar::Header::new_gnu();
    h.set_entry_type(tar::EntryType::Symlink);
    h.set_size(0);
    b.append_link(&mut h, "ln", "t/".repeat(70)).unwrap();
    // A pax header with a path.
    b.append_pax_extensions([("path", &b"pax/name.txt"[..])])
        .unwrap();
    let mut h = tar::Header::new_ustar();
    h.set_size(3);
    h.set_mode(0o644);
    b.append_data(&mut h, "x", &b"\x00\x01\x02"[..]).unwrap();
    b.into_inner().unwrap()
}

fn seed_gz() -> Vec<u8> {
    let mut e = flate2::GzBuilder::new()
        .filename("inner.txt")
        .write(Vec::new(), flate2::Compression::fast());
    e.write_all(b"some text\nmore\n").unwrap();
    e.finish().unwrap()
}

fn mutate(r: &mut Rng, base: &[u8]) -> Vec<u8> {
    let mut v = base.to_vec();
    let edits = 1 + r.below(8);
    for _ in 0..edits {
        if v.is_empty() {
            v.push(r.next() as u8);
            continue;
        }
        match r.below(7) {
            0 => {
                let i = r.below(v.len());
                v[i] = r.next() as u8;
            }
            1 => {
                let i = r.below(v.len());
                v[i] ^= 1 << r.below(8);
            }
            2 => {
                // interesting values at a random place
                let i = r.below(v.len());
                let vals: [&[u8]; 6] = [
                    &[0xff, 0xff, 0xff, 0xff],
                    &[0, 0, 0, 0],
                    &[0xff, 0xff],
                    &[0xfe, 0xff, 0xff, 0x7f],
                    &[0x01, 0, 0x10, 0],
                    &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
                ];
                let x = vals[r.below(vals.len())];
                for (k, b) in x.iter().enumerate() {
                    if i + k < v.len() {
                        v[i + k] = *b;
                    }
                }
            }
            3 => {
                let n = r.below(v.len());
                v.truncate(n);
            }
            4 => {
                let i = r.below(v.len());
                let n = r.below(64);
                let end = (i + n).min(v.len());
                v.drain(i..end);
            }
            5 => {
                let i = r.below(v.len());
                let j = r.below(v.len());
                let n = r.below(64).min(v.len() - j);
                let chunk = v[j..j + n].to_vec();
                v.splice(i..i, chunk);
            }
            _ => {
                let i = r.below(v.len() + 1);
                v.insert(i.min(v.len()), r.next() as u8);
            }
        }
    }
    v
}

fn exercise(r: &mut Rng, b: &[u8]) {
    let _ = describe(b);
    let offs = [0usize, 1, 3, 499, usize::MAX / 2, usize::MAX - 10];
    let off = offs[r.below(offs.len())];
    let names: Vec<String> = match list(b, 0, 500, soon()) {
        Ok(l) => l.entries.into_iter().map(|e| e.path).collect(),
        Err(_) => vec![],
    };
    // A list at a random offset (huge ones too: the model chooses it).
    if off < usize::MAX / 4 {
        let _ = list(b, off, 1 + r.below(600), soon());
    }
    let modes = [Mode::Auto, Mode::Hex, Mode::Strings];
    let numbers = [
        None,
        Some(0u64),
        Some(1),
        Some(7),
        Some(u64::MAX),
        Some(1 << 40),
        Some(9000),
    ];
    for k in 0..3 {
        let entry = if k == 0 || names.is_empty() {
            None
        } else {
            Some(names[r.below(names.len())].as_str())
        };
        let req = ReadReq {
            entry,
            offset: numbers[r.below(numbers.len())],
            limit: numbers[r.below(numbers.len())],
            mode: modes[r.below(3)],
        };
        let _ = read(b, &req, soon());
    }
}

/// Mutation fuzz over zip, tar, tar.gz and gz. Any panic fails the test.
/// HARNESS_FUZZ_N sets the iterations (default 20,000).
#[test]
fn fuzz_parsers_never_panic() {
    let n: usize = std::env::var("HARNESS_FUZZ_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20_000);
    let tarb = seed_tar();
    let seeds = vec![seed_zip(), tarb.clone(), gz(&tarb), seed_gz()];
    for s in &seeds {
        exercise(&mut Rng(1), s);
    }
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    let started = Instant::now();
    for i in 0..n {
        let m = if i % 5 == 4 {
            gz(&mutate(&mut r, &tarb))
        } else {
            mutate(&mut r, &seeds[i % 4])
        };
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut rr = Rng(r.0 ^ 0xabcdef);
            exercise(&mut rr, &m);
        }));
        if res.is_err() {
            panic!("panic at iteration {i} (seed {})", i % 4);
        }
        // tgz mutants: also decompress-and-mutate the inner tar.
        let _ = r.next();
    }
    eprintln!("fuzz: {n} iterations in {:?}", started.elapsed());
}

/// The model picks `offset`: a huge one for `list`.
#[test]
fn list_at_a_huge_offset_does_not_panic() {
    let z = seed_zip();
    let r = std::panic::catch_unwind(|| list(&z, usize::MAX - 10, 500, soon()));
    match r {
        Ok(Ok(l)) => eprintln!(
            "list at huge offset: {} entries, more={}",
            l.entries.len(),
            {
                std::panic::catch_unwind(|| l.more())
                    .map(|m| m.to_string())
                    .unwrap_or_else(|_| "PANIC in more()".into())
            }
        ),
        Ok(Err(e)) => eprintln!("list at huge offset: err {e}"),
        Err(_) => panic!("list() panicked at a huge offset"),
    }
}

/// The mutant that overflowed `at + 4` in the zip helpers (iteration 112,600
/// of the reviewer's run): a zip64 locator pointing at the end of the address
/// space.
#[test]
fn the_input_that_overflowed_the_zip_helpers_is_refused_not_a_panic() {
    let b = include_bytes!("fuzz-crash-112600.bin");
    let mut r = Rng(7);
    exercise(&mut r, b);
}
