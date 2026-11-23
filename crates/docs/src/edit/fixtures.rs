//! Files shaped like what Word and Excel write, built in code for the tests:
//! split runs with revision ids, spelling marks, bookmarks, fields, links,
//! tracked changes, content controls, alternate content, section breaks;
//! shared strings, shared formulas, merges, a table part, a calculation
//! chain, the 1904 date system.

use crate::zip::ZipWriter;

pub const W_NS: &str = "xmlns:wpc=\"http://schemas.microsoft.com/office/word/2010/wordprocessingCanvas\" \
xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\" \
xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
mc:Ignorable=\"w14\"";

pub const DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n";

pub fn document_xml() -> String {
    let body = concat!(
        // 1 heading with a bookmark
        "<w:p w:rsidR=\"00A10001\" w:rsidRDefault=\"00A10001\"><w:pPr><w:pStyle w:val=\"berschrift1\"/></w:pPr><w:bookmarkStart w:id=\"0\" w:name=\"_Toc100\"/><w:r><w:t>Quarterly report</w:t></w:r><w:bookmarkEnd w:id=\"0\"/></w:p>",
        // 2 split runs, spelling marks, rsids
        "<w:p w:rsidR=\"00B10002\" w:rsidRDefault=\"00B10002\"><w:r w:rsidRPr=\"00C10003\"><w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\">The qu</w:t></w:r><w:proofErr w:type=\"spellStart\"/><w:r><w:rPr><w:i/></w:rPr><w:t>ick bro</w:t></w:r><w:proofErr w:type=\"spellEnd\"/><w:r><w:t xml:space=\"preserve\">wn fox jumps</w:t></w:r></w:p>",
        // 3 hyperlink
        "<w:p><w:r><w:t xml:space=\"preserve\">Visit </w:t></w:r><w:hyperlink r:id=\"rId5\" w:history=\"1\"><w:r><w:rPr><w:rStyle w:val=\"Hyperlink\"/></w:rPr><w:t>our site</w:t></w:r></w:hyperlink><w:r><w:t xml:space=\"preserve\"> today</w:t></w:r></w:p>",
        // 4 complex field
        "<w:p><w:r><w:t xml:space=\"preserve\">Page </w:t></w:r><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText xml:space=\"preserve\"> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>3</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r><w:r><w:t xml:space=\"preserve\"> of the report</w:t></w:r></w:p>",
        // 5 tracked changes
        "<w:p><w:r><w:t xml:space=\"preserve\">Keep </w:t></w:r><w:ins w:id=\"7\" w:author=\"Ann\" w:date=\"2026-01-01T00:00:00Z\"><w:r><w:t>added</w:t></w:r></w:ins><w:del w:id=\"8\" w:author=\"Ann\" w:date=\"2026-01-01T00:00:00Z\"><w:r><w:delText>removed</w:delText></w:r></w:del><w:r><w:t xml:space=\"preserve\"> end</w:t></w:r></w:p>",
        // 6 hidden text
        "<w:p><w:r><w:t xml:space=\"preserve\">Visible </w:t></w:r><w:r><w:rPr><w:vanish/></w:rPr><w:t>secret</w:t></w:r></w:p>",
        // 7 alternate content with a text box
        "<w:p><w:r><w:t xml:space=\"preserve\">Picture: </w:t></w:r><w:r><mc:AlternateContent><mc:Choice Requires=\"wps\"><w:drawing><wp:inline><wp:docPr id=\"1\" name=\"Box\"/><w:txbxContent><w:p><w:r><w:t>inside the box</w:t></w:r></w:p></w:txbxContent></wp:inline></w:drawing></mc:Choice><mc:Fallback><w:pict/></mc:Fallback></mc:AlternateContent></w:r><w:r><w:t xml:space=\"preserve\"> after</w:t></w:r></w:p>",
        // 8, 9 list items
        "<w:p><w:pPr><w:pStyle w:val=\"ListParagraph\"/><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>first item</w:t></w:r></w:p>",
        "<w:p><w:pPr><w:pStyle w:val=\"ListParagraph\"/><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>second item</w:t></w:r></w:p>",
        // 10 a section break in a paragraph
        "<w:p><w:pPr><w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/></w:sectPr></w:pPr><w:r><w:t>End of section one</w:t></w:r></w:p>",
        // table 1: two rows, two columns, a cell with two paragraphs
        "<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/></w:tblPr><w:tblGrid><w:gridCol w:w=\"4000\"/><w:gridCol w:w=\"4000\"/></w:tblGrid>",
        "<w:tr><w:tc><w:tcPr><w:tcW w:w=\"4000\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>Name</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Amount</w:t></w:r></w:p></w:tc></w:tr>",
        "<w:tr><w:tc><w:p><w:r><w:t>Alice</w:t></w:r></w:p><w:p><w:r><w:t>(lead)</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>1,200</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
        // a content control around a paragraph
        "<w:sdt><w:sdtPr><w:alias w:val=\"Toc\"/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>Contents</w:t></w:r></w:p></w:sdtContent></w:sdt>",
        // last paragraphs
        "<w:p><w:r><w:t>The end.</w:t></w:r></w:p>",
        "<w:p/>",
        "<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1440\"/></w:sectPr>"
    );
    format!("{DECL}<w:document {W_NS}><w:body>{body}</w:body></w:document>")
}

pub fn styles_xml() -> String {
    format!(
        "{DECL}<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Standard\"><w:name w:val=\"Normal\"/></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"berschrift1\"><w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Standard\"/></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"berschrift2\"><w:name w:val=\"heading 2\"/></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"ListParagraph\"><w:name w:val=\"List Paragraph\"/></w:style>\
<w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/></w:style>\
</w:styles>"
    )
}

pub fn docx_with(document: &str, extra: &[(&str, &str)]) -> Vec<u8> {
    let mut w = ZipWriter::new(8 << 20);
    w.add("[Content_Types].xml", format!("{DECL}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/></Types>").as_bytes()).unwrap();
    w.add("_rels/.rels", format!("{DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>").as_bytes()).unwrap();
    w.add("word/document.xml", document.as_bytes()).unwrap();
    w.add("word/_rels/document.xml.rels", format!("{DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/><Relationship Id=\"rId5\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"https://example.com/\" TargetMode=\"External\"/></Relationships>").as_bytes()).unwrap();
    w.add("word/styles.xml", styles_xml().as_bytes()).unwrap();
    for (n, c) in extra {
        w.add(n, c.as_bytes()).unwrap();
    }
    w.finish().unwrap()
}

pub fn docx_real() -> Vec<u8> {
    docx_with(&document_xml(), &[])
}
