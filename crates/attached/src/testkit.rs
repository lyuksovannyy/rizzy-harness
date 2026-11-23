//! Builders for the tests of every crate that reads or sends an office
//! document: a zip made by hand, and small Word, Excel, PowerPoint and
//! OpenDocument files built in code (no fixture over a few KB is checked
//! in). Not used outside tests.

use std::io::Write;

/// One entry of a zip to build.
pub struct Ent<'a> {
    pub name: &'a [u8],
    /// What the entry holds (uncompressed).
    pub data: &'a [u8],
    pub deflate: bool,
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
    e.write_all(data).expect("in memory");
    e.finish().expect("in memory")
}

/// A zip made by hand. `declared` overrides the uncompressed size the
/// central directory states for the first entry (a lying header);
/// `encrypted` sets the encryption flag on every entry.
pub fn zip_with(ents: &[Ent<'_>], declared: Option<u64>, encrypted: bool) -> Vec<u8> {
    let flags: u16 = if encrypted { 1 } else { 0 };
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (i, e) in ents.iter().enumerate() {
        let body = if e.deflate {
            deflate(e.data)
        } else {
            e.data.to_vec()
        };
        let mut crc = flate2::Crc::new();
        crc.update(e.data);
        let crc = crc.sum();
        let at = out.len() as u32;
        let method: u16 = if e.deflate { 8 } else { 0 };
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&[0; 4]); // time, date
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&(e.data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(e.name);
        out.extend_from_slice(&body);
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&[20, 0, 20, 0]);
        central.extend_from_slice(&flags.to_le_bytes());
        central.extend_from_slice(&method.to_le_bytes());
        central.extend_from_slice(&[0; 4]);
        central.extend_from_slice(&crc.to_le_bytes());
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
    let off = out.len() as u32;
    let n = ents.len().min(0xFFFF) as u16;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// A zip of deflated entries, in this order.
pub fn zip(ents: &[(&str, &[u8])]) -> Vec<u8> {
    let e: Vec<Ent> = ents
        .iter()
        .map(|(n, d)| Ent {
            name: n.as_bytes(),
            data: d,
            deflate: true,
        })
        .collect();
    zip_with(&e, None, false)
}

/// `s` as XML text.
pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

const CT: &str = r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/></Types>"#;
const NS_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const NS_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

/// A paragraph of one run.
pub fn p(text: &str) -> String {
    format!("<w:p><w:r><w:t>{}</w:t></w:r></w:p>", esc(text))
}

/// A paragraph in a style.
pub fn styled(style: &str, text: &str) -> String {
    format!(
        "<w:p><w:pPr><w:pStyle w:val=\"{style}\"/></w:pPr><w:r><w:t>{}</w:t></w:r></w:p>",
        esc(text)
    )
}

/// A list item at `level`.
pub fn item(level: u8, text: &str) -> String {
    format!(
        "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"{level}\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>{}</w:t></w:r></w:p>",
        esc(text)
    )
}

/// A table of rows of cells of text.
pub fn table(rows: &[&[&str]]) -> String {
    let mut s = String::from("<w:tbl>");
    for r in rows {
        s.push_str("<w:tr>");
        for c in *r {
            s.push_str(&format!("<w:tc>{}</w:tc>", p(c)));
        }
        s.push_str("</w:tr>");
    }
    s.push_str("</w:tbl>");
    s
}

/// A Word file whose body is `body` (the XML inside `w:body`).
pub fn docx(body: &str) -> Vec<u8> {
    docx_with(body, &[])
}

/// A Word file with more parts (`styles.xml`, comments, ...).
pub fn docx_with(body: &str, extra: &[(&str, &[u8])]) -> Vec<u8> {
    let doc = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"{NS_W}\"><w:body>{body}</w:body></w:document>"
    );
    let mut e: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", CT.as_bytes()),
        ("word/document.xml", doc.as_bytes()),
    ];
    e.extend_from_slice(extra);
    zip(&e)
}

/// An Excel file: sheets of rows of cells. A cell that parses as a number is
/// stored as one; any other is a shared string.
pub fn xlsx(sheets: &[(&str, &[&[&str]])]) -> Vec<u8> {
    let mut shared: Vec<String> = Vec::new();
    let mut parts: Vec<(String, String)> = Vec::new();
    for (i, (_, rows)) in sheets.iter().enumerate() {
        let mut data = String::new();
        for (r, row) in rows.iter().enumerate() {
            data.push_str(&format!("<row r=\"{}\">", r + 1));
            for (c, cell) in row.iter().enumerate() {
                if cell.is_empty() {
                    continue;
                }
                let at = format!("{}{}", (b'A' + c as u8) as char, r + 1);
                if cell.parse::<f64>().is_ok() {
                    data.push_str(&format!("<c r=\"{at}\"><v>{cell}</v></c>"));
                } else {
                    shared.push((*cell).to_string());
                    data.push_str(&format!(
                        "<c r=\"{at}\" t=\"s\"><v>{}</v></c>",
                        shared.len() - 1
                    ));
                }
            }
            data.push_str("</row>");
        }
        parts.push((
            format!("xl/worksheets/sheet{}.xml", i + 1),
            sheet_xml(&data),
        ));
    }
    xlsx_raw(
        sheets.iter().map(|(n, _)| *n).collect(),
        &shared.iter().map(String::as_str).collect::<Vec<_>>(),
        parts,
    )
}

/// The XML of a worksheet whose `sheetData` holds `rows`.
pub fn sheet_xml(rows: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>{rows}</sheetData></worksheet>"
    )
}

/// An Excel file from sheet names, shared strings and the XML of each sheet
/// (`xl/worksheets/sheetN.xml`).
pub fn xlsx_raw(names: Vec<&str>, shared: &[&str], sheets: Vec<(String, String)>) -> Vec<u8> {
    let mut wb = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"{NS_R}\"><sheets>"
    );
    let mut rels = format!("<?xml version=\"1.0\"?><Relationships xmlns=\"{NS_REL}\">");
    for (i, n) in names.iter().enumerate() {
        wb.push_str(&format!(
            "<sheet name=\"{}\" sheetId=\"{}\" r:id=\"rId{}\"/>",
            esc(n),
            i + 1,
            i + 1
        ));
        rels.push_str(&format!(
            "<Relationship Id=\"rId{}\" Type=\"{NS_R}/worksheet\" Target=\"worksheets/sheet{}.xml\"/>",
            i + 1,
            i + 1
        ));
    }
    wb.push_str("</sheets></workbook>");
    rels.push_str(&format!(
        "<Relationship Id=\"rIdS\" Type=\"{NS_R}/sharedStrings\" Target=\"sharedStrings.xml\"/></Relationships>"
    ));
    let mut sst = String::from(
        "<?xml version=\"1.0\"?><sst xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">",
    );
    for s in shared {
        sst.push_str(&format!("<si><t>{}</t></si>", esc(s)));
    }
    sst.push_str("</sst>");
    let mut e: Vec<(String, Vec<u8>)> = vec![
        ("[Content_Types].xml".into(), CT.as_bytes().to_vec()),
        ("xl/workbook.xml".into(), wb.into_bytes()),
        ("xl/_rels/workbook.xml.rels".into(), rels.into_bytes()),
        ("xl/sharedStrings.xml".into(), sst.into_bytes()),
    ];
    for (n, x) in sheets {
        e.push((n, x.into_bytes()));
    }
    let refs: Vec<(&str, &[u8])> = e.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
    zip(&refs)
}

/// A PowerPoint file: each slide a list of text lines and, if given, notes.
pub fn pptx(slides: &[(&[&str], Option<&str>)]) -> Vec<u8> {
    let mut pres = format!(
        "<?xml version=\"1.0\"?><p:presentation xmlns:p=\"{NS_P}\" xmlns:r=\"{NS_R}\"><p:sldIdLst>"
    );
    let mut rels = format!("<?xml version=\"1.0\"?><Relationships xmlns=\"{NS_REL}\">");
    let mut e: Vec<(String, Vec<u8>)> =
        vec![("[Content_Types].xml".into(), CT.as_bytes().to_vec())];
    for (i, (lines, notes)) in slides.iter().enumerate() {
        let n = i + 1;
        pres.push_str(&format!("<p:sldId id=\"{}\" r:id=\"rId{n}\"/>", 255 + n));
        rels.push_str(&format!(
            "<Relationship Id=\"rId{n}\" Type=\"{NS_R}/slide\" Target=\"slides/slide{n}.xml\"/>"
        ));
        let mut shapes = String::new();
        for l in *lines {
            shapes.push_str(&format!(
                "<p:sp><p:nvSpPr><p:nvPr/></p:nvSpPr><p:txBody><a:p><a:r><a:t>{}</a:t></a:r></a:p></p:txBody></p:sp>",
                esc(l)
            ));
        }
        e.push((
            format!("ppt/slides/slide{n}.xml"),
            format!(
                "<?xml version=\"1.0\"?><p:sld xmlns:p=\"{NS_P}\" xmlns:a=\"{NS_A}\"><p:cSld><p:spTree>{shapes}</p:spTree></p:cSld></p:sld>"
            )
            .into_bytes(),
        ));
        if let Some(t) = notes {
            e.push((
                format!("ppt/slides/_rels/slide{n}.xml.rels"),
                format!(
                    "<?xml version=\"1.0\"?><Relationships xmlns=\"{NS_REL}\"><Relationship Id=\"rId1\" Type=\"{NS_R}/notesSlide\" Target=\"../notesSlides/notesSlide{n}.xml\"/></Relationships>"
                )
                .into_bytes(),
            ));
            e.push((
                format!("ppt/notesSlides/notesSlide{n}.xml"),
                format!(
                    "<?xml version=\"1.0\"?><p:notes xmlns:p=\"{NS_P}\" xmlns:a=\"{NS_A}\"><p:cSld><p:spTree>\
                     <p:sp><p:nvSpPr><p:nvPr><p:ph type=\"sldImg\"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>IMAGE</a:t></a:r></a:p></p:txBody></p:sp>\
                     <p:sp><p:nvSpPr><p:nvPr><p:ph type=\"body\" idx=\"1\"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>{}</a:t></a:r></a:p></p:txBody></p:sp>\
                     <p:sp><p:nvSpPr><p:nvPr><p:ph type=\"sldNum\"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:fld type=\"slidenum\"><a:t>{n}</a:t></a:fld></a:p></p:txBody></p:sp>\
                     </p:spTree></p:cSld></p:notes>",
                    esc(t)
                )
                .into_bytes(),
            ));
        }
    }
    pres.push_str("</p:sldIdLst></p:presentation>");
    rels.push_str("</Relationships>");
    e.push(("ppt/presentation.xml".into(), pres.into_bytes()));
    e.push(("ppt/_rels/presentation.xml.rels".into(), rels.into_bytes()));
    let refs: Vec<(&str, &[u8])> = e.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
    zip(&refs)
}

/// An OpenDocument file of `kind` (`text`, `spreadsheet`, `presentation`)
/// whose `office:body` holds `body`. `mimetype` is the first entry, stored.
pub fn odf(kind: &str, body: &str) -> Vec<u8> {
    odf_with(kind, body, &[])
}

/// [`odf`] with more entries (a manifest, say).
pub fn odf_with(kind: &str, body: &str, extra: &[(&str, &[u8])]) -> Vec<u8> {
    let mime = format!("application/vnd.oasis.opendocument.{kind}");
    let content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-content \
         xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
         xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
         xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" \
         xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" \
         xmlns:presentation=\"urn:oasis:names:tc:opendocument:xmlns:presentation:1.0\">\
         <office:body>{body}</office:body></office:document-content>"
    );
    let mut e = vec![
        Ent {
            name: b"mimetype",
            data: mime.as_bytes(),
            deflate: false,
        },
        Ent {
            name: b"content.xml",
            data: content.as_bytes(),
            deflate: true,
        },
    ];
    for (n, d) in extra {
        e.push(Ent {
            name: n.as_bytes(),
            data: d,
            deflate: true,
        });
    }
    zip_with(&e, None, false)
}
