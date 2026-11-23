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

const S_NS: &str = "xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" mc:Ignorable=\"x14ac\" \
xmlns:x14ac=\"http://schemas.microsoft.com/office/spreadsheetml/2009/9/ac\"";

pub fn sheet1_xml() -> String {
    format!(
        "{DECL}<worksheet {S_NS}><dimension ref=\"A1:D8\"/><sheetViews><sheetView workbookViewId=\"0\"/></sheetViews><sheetFormatPr defaultRowHeight=\"15\" x14ac:dyDescent=\"0.25\"/>\
<cols><col min=\"1\" max=\"1\" width=\"20\" customWidth=\"1\"/><col min=\"5\" max=\"5\" width=\"9\" hidden=\"1\" customWidth=\"1\"/></cols>\
<sheetData>\
<row r=\"1\" spans=\"1:4\" x14ac:dyDescent=\"0.25\"><c r=\"A1\" s=\"1\" t=\"s\"><v>0</v></c><c r=\"B1\" s=\"1\" t=\"s\"><v>1</v></c><c r=\"C1\" s=\"1\" t=\"s\"><v>2</v></c><c r=\"D1\" s=\"1\" t=\"s\"><v>3</v></c></row>\
<row r=\"2\" spans=\"1:4\"><c r=\"A2\" t=\"s\"><v>4</v></c><c r=\"B2\"><v>100</v></c><c r=\"C2\" s=\"2\"><v>46000</v></c><c r=\"D2\"><f t=\"shared\" ref=\"D2:D4\" si=\"0\">B2*2</f><v>200</v></c></row>\
<row r=\"3\" spans=\"1:4\"><c r=\"A3\" t=\"s\"><v>5</v></c><c r=\"B3\"><v>250</v></c><c r=\"C3\" s=\"3\"><v>46001</v></c><c r=\"D3\"><f t=\"shared\" si=\"0\"/><v>500</v></c></row>\
<row r=\"4\" spans=\"1:4\"><c r=\"A4\" t=\"inlineStr\"><is><r><t>C</t></r><r><t>y</t></r></is></c><c r=\"B4\"><v>3</v></c><c r=\"D4\"><f t=\"shared\" si=\"0\"/><v>6</v></c></row>\
<row r=\"5\"><c r=\"A5\" s=\"1\" t=\"s\"><v>6</v></c><c r=\"B5\" s=\"1\"/></row>\
<row r=\"6\" hidden=\"1\"><c r=\"A6\" t=\"s\"><v>7</v></c></row>\
<row r=\"7\"><c r=\"A7\"><f t=\"array\" ref=\"A7:A8\">SUM(B2:B4)</f><v>353</v></c><c r=\"E7\"><v>1</v></c></row>\
<row r=\"8\"><c r=\"A8\"><v>353</v></c></row>\
</sheetData>\
<mergeCells count=\"1\"><mergeCell ref=\"A5:B5\"/></mergeCells>\
<conditionalFormatting sqref=\"B2:B4\"><cfRule type=\"cellIs\" dxfId=\"0\" priority=\"1\" operator=\"greaterThan\"><formula>200</formula></cfRule></conditionalFormatting>\
<dataValidations count=\"1\"><dataValidation type=\"whole\" sqref=\"B2:B4\"><formula1>0</formula1></dataValidation></dataValidations>\
<pageMargins left=\"0.7\" right=\"0.7\" top=\"0.75\" bottom=\"0.75\" header=\"0.3\" footer=\"0.3\"/>\
<tableParts count=\"1\"><tablePart r:id=\"rId1\"/></tableParts></worksheet>"
    )
}

pub fn xlsx_with(sheet1: &str, workbook_extra_attrs: &str, extra: &[(&str, &str)]) -> Vec<u8> {
    let mut w = ZipWriter::new(8 << 20);
    let types = format!(
        "{DECL}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"bin\" ContentType=\"application/vnd.ms-office.vbaProject\"/>\
<Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>\
<Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>\
<Override PartName=\"/xl/worksheets/sheet2.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>\
<Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>\
<Override PartName=\"/xl/sharedStrings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml\"/>\
<Override PartName=\"/xl/tables/table1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml\"/>\
<Override PartName=\"/xl/calcChain.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml\"/></Types>"
    );
    w.add("[Content_Types].xml", types.as_bytes()).unwrap();
    w.add("_rels/.rels", format!("{DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>").as_bytes()).unwrap();
    w.add("xl/workbook.xml", format!("{DECL}<workbook {S_NS}><fileVersion appName=\"xl\"/><workbookPr{workbook_extra_attrs}/><bookViews><workbookView/></bookViews><sheets><sheet name=\"Data\" sheetId=\"1\" r:id=\"rId1\"/><sheet name=\"Old\" sheetId=\"2\" state=\"hidden\" r:id=\"rId2\"/></sheets><definedNames><definedName name=\"Total\">Data!$D$2:$D$4</definedName></definedNames><calcPr calcId=\"191029\"/></workbook>").as_bytes()).unwrap();
    w.add("xl/_rels/workbook.xml.rels", format!("{DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet2.xml\"/><Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/><Relationship Id=\"rId4\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings\" Target=\"sharedStrings.xml\"/><Relationship Id=\"rId5\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain\" Target=\"calcChain.xml\"/></Relationships>").as_bytes()).unwrap();
    w.add("xl/worksheets/sheet1.xml", sheet1.as_bytes())
        .unwrap();
    w.add("xl/worksheets/_rels/sheet1.xml.rels", format!("{DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/table\" Target=\"../tables/table1.xml\"/></Relationships>").as_bytes()).unwrap();
    w.add("xl/worksheets/sheet2.xml", format!("{DECL}<worksheet {S_NS}><sheetData><row r=\"1\"><c r=\"A1\"><v>9</v></c></row></sheetData></worksheet>").as_bytes()).unwrap();
    w.add("xl/tables/table1.xml", format!("{DECL}<table {S_NS} id=\"1\" name=\"Sales\" displayName=\"Sales\" ref=\"A1:D4\"><tableColumns count=\"1\"><tableColumn id=\"1\" name=\"Name\"/></tableColumns></table>").as_bytes()).unwrap();
    w.add("xl/styles.xml", format!("{DECL}<styleSheet {S_NS}><numFmts count=\"1\"><numFmt numFmtId=\"164\" formatCode=\"yyyy\\-mm\\-dd\"/></numFmts><cellXfs count=\"4\"><xf numFmtId=\"0\"/><xf numFmtId=\"0\" fontId=\"1\"/><xf numFmtId=\"14\"/><xf numFmtId=\"164\"/></cellXfs></styleSheet>").as_bytes()).unwrap();
    w.add("xl/sharedStrings.xml", format!("{DECL}<sst {S_NS} count=\"8\" uniqueCount=\"8\"><si><t>Name</t></si><si><t>Amount</t></si><si><t>Date</t></si><si><t>Total</t></si><si><t>Alice</t></si><si><t>Bob</t></si><si><r><t>Mer</t></r><r><t>ged</t></r><rPh sb=\"0\" eb=\"1\"><t>x</t></rPh></si><si><t>secret</t></si></sst>").as_bytes()).unwrap();
    w.add("xl/calcChain.xml", format!("{DECL}<calcChain {S_NS}><c r=\"D2\" i=\"1\"/><c r=\"D3\"/><c r=\"D4\"/><c r=\"A7\"/></calcChain>").as_bytes()).unwrap();
    for (n, c) in extra {
        w.add(n, c.as_bytes()).unwrap();
    }
    w.finish().unwrap()
}

pub fn xlsx_real() -> Vec<u8> {
    xlsx_with(&sheet1_xml(), "", &[])
}
