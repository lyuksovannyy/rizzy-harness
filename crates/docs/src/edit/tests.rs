//! Tests of the edit engine: the raw copy of a package, the byte-faithful
//! patcher, and (in the modules that follow) the tools.

use std::io::Write;

use harness_attached::Package;

use super::pkg::{Rewrite, Source};
use super::scan::{NS_W, Patches, Scan};
use crate::zip::ZipWriter;

pub(crate) fn crc(b: &[u8]) -> u32 {
    let mut c = flate2::Crc::new();
    c.update(b);
    c.sum()
}

/// A zip with the entries exactly as given: `(name bytes, flags, method,
/// stored data, crc, unpacked size)`. For files a well-behaved writer never
/// makes (repeated names, encrypted entries, odd timestamps).
pub(crate) fn raw_zip(items: &[(&[u8], u16, u16, &[u8], u32, u32)], stamp: (u16, u16)) -> Vec<u8> {
    let mut o = Vec::new();
    let mut central = Vec::new();
    for (name, flags, method, data, crc, size) in items {
        let off = o.len() as u32;
        let le16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
        let le32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
        le32(&mut o, 0x0403_4b50);
        le16(&mut o, 20);
        le16(&mut o, *flags);
        le16(&mut o, *method);
        le16(&mut o, stamp.0);
        le16(&mut o, stamp.1);
        le32(&mut o, *crc);
        le32(&mut o, data.len() as u32);
        le32(&mut o, *size);
        le16(&mut o, name.len() as u16);
        le16(&mut o, 0);
        o.extend_from_slice(name);
        o.extend_from_slice(data);
        le32(&mut central, 0x0201_4b50);
        le16(&mut central, 20);
        le16(&mut central, 20);
        le16(&mut central, *flags);
        le16(&mut central, *method);
        le16(&mut central, stamp.0);
        le16(&mut central, stamp.1);
        le32(&mut central, *crc);
        le32(&mut central, data.len() as u32);
        le32(&mut central, *size);
        le16(&mut central, name.len() as u16);
        le16(&mut central, 0);
        le16(&mut central, 0);
        le16(&mut central, 0);
        le16(&mut central, 0);
        le32(&mut central, 0);
        le32(&mut central, off);
        central.extend_from_slice(name);
    }
    let start = o.len() as u32;
    o.extend_from_slice(&central);
    let cd = central.len() as u32;
    o.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    o.extend_from_slice(&[0, 0, 0, 0]);
    o.extend_from_slice(&(items.len() as u16).to_le_bytes());
    o.extend_from_slice(&(items.len() as u16).to_le_bytes());
    o.extend_from_slice(&cd.to_le_bytes());
    o.extend_from_slice(&start.to_le_bytes());
    o.extend_from_slice(&[0, 0]);
    o
}

pub(crate) fn stored<'a>(
    name: &'a str,
    data: &'a [u8],
) -> (&'a [u8], u16, u16, &'a [u8], u32, u32) {
    (name.as_bytes(), 0, 0, data, crc(data), data.len() as u32)
}

fn deflate(b: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    e.write_all(b).unwrap();
    e.finish().unwrap()
}

/// The entries of a package as `(name, method, crc, comp, size, time, date, data)`.
type Row = (String, u16, u32, u64, u64, u16, u16, Vec<u8>);
fn rows(bytes: &[u8]) -> Vec<Row> {
    let p = Package::open(
        bytes,
        std::time::Instant::now() + std::time::Duration::from_secs(5),
    )
    .unwrap();
    p.raw_entries()
        .unwrap()
        .iter()
        .map(|e| {
            (
                String::from_utf8(e.name.to_vec()).unwrap(),
                e.method,
                e.crc,
                e.comp,
                e.size,
                e.time,
                e.date,
                e.data.to_vec(),
            )
        })
        .collect()
}

/// Every entry the edit did not change comes out as its original compressed
/// bytes with its original CRC, sizes, method and timestamp, in its original
/// place; a zip bomb among them is copied, not inflated.
#[test]
fn untouched_entries_are_copied_as_their_original_bytes() {
    let bomb = deflate(&vec![0u8; 40 << 20]);
    assert!(bomb.len() < 100_000);
    let bytes = raw_zip(
        &[
            stored("[Content_Types].xml", b"<Types/>"),
            (b"word/bomb.bin", 0, 8, &bomb, 0xDEAD_BEEF, 40 << 20),
            stored("word/document.xml", b"<w:document/>"),
            stored("docProps/app.xml", b"<Properties/>"),
        ],
        (0x1234, 0x5678),
    );
    let src = Source::open(&bytes).unwrap();
    let mut rw = Rewrite::default();
    rw.replaced.insert(
        "word/document.xml".into(),
        b"<w:document>new</w:document>".to_vec(),
    );
    let t = std::time::Instant::now();
    let out = src.rewrite(&rw).unwrap();
    assert!(t.elapsed().as_secs() < 5, "nothing was inflated");
    let (a, b) = (rows(&bytes), rows(&out));
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(&b) {
        assert_eq!(x.0, y.0, "order kept");
        assert_eq!((x.5, x.6), (y.5, y.6), "timestamp kept: {}", x.0);
        if x.0 == "word/document.xml" {
            assert_eq!(y.7, b"<w:document>new</w:document>");
        } else {
            assert_eq!(x, y, "{} is byte for byte the original", x.0);
        }
    }
    // The bomb's lie about its CRC is the file's own; it is copied, not judged.
    assert_eq!(b[1].2, 0xDEAD_BEEF);
}

#[test]
fn a_part_is_removed_and_the_rest_keeps_its_order() {
    let bytes = raw_zip(
        &[
            stored("a.xml", b"<a/>"),
            stored("b.xml", b"<b/>"),
            stored("c.xml", b"<c/>"),
        ],
        (0, 0),
    );
    let src = Source::open(&bytes).unwrap();
    let mut rw = Rewrite::default();
    rw.removed.insert("b.xml".into());
    let names: Vec<String> = rows(&src.rewrite(&rw).unwrap())
        .into_iter()
        .map(|r| r.0)
        .collect();
    assert_eq!(names, ["a.xml", "c.xml"]);
    let mut rw = Rewrite::default();
    rw.removed.insert("nope.xml".into());
    assert!(src.rewrite(&rw).is_err(), "a part that is not there");
}

#[test]
fn a_package_an_edit_cannot_keep_faithfully_is_refused() {
    let ok = stored("word/document.xml", b"<d/>");
    // Names: repeated, repeated by letter case, not UTF-8, hidden characters,
    // a slash up, signatures.
    let dup = raw_zip(&[ok, stored("word/document.xml", b"<e/>")], (0, 0));
    assert!(Source::open(&dup).err().unwrap().contains("same name"));
    let case = raw_zip(&[ok, stored("Word/Document.xml", b"<e/>")], (0, 0));
    assert!(Source::open(&case).err().unwrap().contains("same name"));
    let bad = raw_zip(&[(b"word/\xff.xml", 0, 0, b"x", crc(b"x"), 1)], (0, 0));
    assert!(Source::open(&bad).err().unwrap().contains("UTF-8"));
    let up = raw_zip(&[stored("../x.xml", b"x")], (0, 0));
    assert!(Source::open(&up).is_err());
    let ctl = raw_zip(&[stored("word/a\u{202e}.xml", b"x")], (0, 0));
    assert!(Source::open(&ctl).is_err());
    let signed = raw_zip(&[ok, stored("_xmlsignatures/sig1.xml", b"<s/>")], (0, 0));
    assert!(Source::open(&signed).err().unwrap().contains("signed"));
    // Encrypted, an unknown method, a link.
    let enc = raw_zip(&[(b"a.xml", 1, 0, b"xx", 0, 2)], (0, 0));
    assert!(Source::open(&enc).err().unwrap().contains("encrypted"));
    let method = raw_zip(&[(b"a.xml", 0, 12, b"xx", 0, 2)], (0, 0));
    assert!(Source::open(&method).err().unwrap().contains("compression"));
    // Not a zip at all.
    assert!(Source::open(b"hello").is_err());
}

#[test]
fn the_writer_refuses_a_copy_it_cannot_keep() {
    let mut w = ZipWriter::new(1 << 20);
    w.put_raw("a.xml", 0, crc(b"xx"), 2, 2, 0, 0, b"xx")
        .unwrap();
    assert!(
        w.put_raw("A.xml", 0, 0, 2, 2, 0, 0, b"xx").is_err(),
        "case twin"
    );
    assert!(
        w.put_raw("b.xml", 9, 0, 2, 2, 0, 0, b"xx").is_err(),
        "method"
    );
    assert!(
        w.put_raw("c.xml", 0, 0, 3, 2, 0, 0, b"xx").is_err(),
        "length"
    );
    let mut small = ZipWriter::new(100);
    assert!(
        small
            .put_raw("a.xml", 0, 0, 200, 200, 0, 0, &[0; 200])
            .is_err()
    );
}

fn scan_all(src: &[u8]) -> Result<usize, String> {
    let mut s = Scan::new(src, NS_W, 1000)?;
    let mut n = 0;
    while s.next()?.is_some() {
        n += 1;
    }
    Ok(n)
}

/// No patches: the same bytes, whatever the XML looks like. One patch: only
/// its range differs.
#[test]
fn the_patcher_changes_only_its_ranges() {
    let xml = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n\
        <w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:mc=\"x\" mc:Ignorable=\"w14\" >\n\
        <!-- a comment --><w:body ><w:p  w:rsidR=\"00A1\"  w:rsidP='00B2' /><w:p><w:r><w:t xml:space=\"preserve\">a &amp; b &#160;<![CDATA[<c>]]></w:t></w:r></w:p></w:body ></w:document >\n";
    let src = xml.as_bytes();
    let p = Patches::new();
    assert_eq!(p.apply(src, 1 << 20).unwrap(), src);
    let at = xml.find("a &amp; b").unwrap();
    let mut p = Patches::new();
    p.replace(at, at + 9, b"X".to_vec());
    let out = String::from_utf8(p.apply(src, 1 << 20).unwrap()).unwrap();
    assert_eq!(out, xml.replacen("a &amp; b", "X", 1));
    // Inserts keep the order they were made in, before a range at the same place.
    let mut p = Patches::new();
    p.replace(at, at + 1, b"[2]".to_vec());
    p.replace(at, at, b"[1a]".to_vec());
    p.replace(at, at, b"[1b]".to_vec());
    let out = String::from_utf8(p.apply(src, 1 << 20).unwrap()).unwrap();
    assert!(out.contains("[1a][1b][2] &amp; b"), "{out}");
    // Overlapping ranges and a range past the end are refused; so is growth
    // past the ceiling, before the bytes are made.
    let mut p = Patches::new();
    p.replace(at, at + 5, vec![]);
    p.replace(at + 2, at + 6, vec![]);
    assert!(p.apply(src, 1 << 20).is_err());
    let mut p = Patches::new();
    p.replace(0, src.len() + 1, vec![]);
    assert!(p.apply(src, 1 << 20).is_err());
    let mut p = Patches::new();
    p.replace(at, at, vec![b'x'; 100]);
    assert!(p.apply(src, src.len() + 99).is_err());
    let mut p = Patches::new();
    p.replace(at, at, vec![b'x'; 100]);
    assert!(p.apply(src, src.len() + 100).is_ok());
    // The scan reads this part.
    assert!(scan_all(src).unwrap() > 10);
}

#[test]
fn the_scan_refuses_what_it_cannot_keep() {
    let w = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"";
    // DOCTYPE, an entity declared by the file.
    let t = format!(
        "<!DOCTYPE d [<!ENTITY x SYSTEM \"file:///etc/passwd\">]><w:document {w}>&x;</w:document>"
    );
    assert!(scan_all(t.as_bytes()).unwrap_err().contains("DOCTYPE"));
    // Not UTF-8: the bytes, or the declaration.
    assert!(
        scan_all(b"<w:document \xff/>")
            .unwrap_err()
            .contains("UTF-8")
    );
    let t = format!("<?xml version=\"1.0\" encoding=\"UTF-16\"?><w:document {w}/>");
    assert!(scan_all(t.as_bytes()).unwrap_err().contains("UTF-8"));
    // Another namespace; the main prefix rebound below the root.
    assert!(scan_all(b"<w:document xmlns:w=\"urn:other\"/>").is_err());
    let t = format!("<w:document {w}><w:p xmlns:w=\"urn:other\"/></w:document>");
    assert!(scan_all(t.as_bytes()).unwrap_err().contains("rebinds"));
    // Malformed: unclosed, mismatched, a doubled attribute is caught where attributes are read.
    assert!(scan_all(format!("<w:document {w}><w:p></w:document>").as_bytes()).is_err());
    // Too deep, too many events.
    let deep = format!(
        "<w:document {w}>{}{}</w:document>",
        "<w:p>".repeat(100),
        "</w:p>".repeat(100)
    );
    assert!(scan_all(deep.as_bytes()).unwrap_err().contains("deeply"));
    let many = format!("<w:document {w}>{}</w:document>", "<w:p/>".repeat(2000));
    assert!(scan_all(many.as_bytes()).unwrap_err().contains("too many"));
}

/// Every entry but the ones named comes out as the input's bytes.
pub(crate) fn assert_others_identical(before: &[u8], after: &[u8], changed: &[&str]) {
    let (a, b) = (rows(before), rows(after));
    for x in &a {
        if changed.contains(&x.0.as_str()) {
            continue;
        }
        let y = b
            .iter()
            .find(|y| y.0 == x.0)
            .unwrap_or_else(|| panic!("{} is gone", x.0));
        assert_eq!(x, y, "{} changed", x.0);
    }
    let names = |r: &[Row]| r.iter().map(|x| x.0.clone()).collect::<Vec<_>>();
    assert_eq!(names(&a), names(&b), "the order of the parts");
}
