# X.58 **sec** (T99)

## Adds

A person looks at a Word or Excel file in the page, without opening another program (the owner, 2026-11-23: "can we view/edit(no need for full suite) the documents in web app?"; this is the view half).
In a chat without a folder, a file the agent made (`MadeChip`) and a .docx or .xlsx the owner attached (the sent message's chip) have a **Preview** control that opens the file in the right side panel beside the conversation, a lazy chunk with the file's name, a Download link, the line "Preview: text and structure, not the page layout", headings, paragraphs (bold, italic, strike as direct formatting), list items and tables for a document, and sheet tabs with a grid (column letters, row numbers, a formula cell marked with its formula) for a workbook, "more is not shown" when cut, and loading and error states with a retry.
The reading is one more request of the confined `doc-edit` helper (`Request::Preview`), through the code of `doc_view` (`docx::index`, `para::scan`, `xlsx::index`, `xlsx::scan_row`), so what the view leaves out is left out here; it returns structure, not HTML, bounded by constants in `limits.rs` (2,000 blocks, 5,000 table cells, 500 rows, 50 columns, about 200 KB of JSON).
The route is `GET /api/sessions/{id}/files/{file_id}/preview?sheet=` (`crates/master/src/http/doc_preview.rs`): the checks of the download, one helper at a time per account, a plain sentence for every failure.
ChatGPT shows a made spreadsheet as a table and a document as a preview; Claude Desktop opens artifacts in a side panel and leaves office files to download; a browser library (docx-preview, mammoth, SheetJS) would parse the untrusted zip inside the signed-in page and build HTML from it, so this does not use one.
Out of scope: editing from the page, files in a coding chat's folder (the node owns them: a typed request over the link), pictures, charts, page layout, comments.
A block or a cell can later carry its edit tag (an optional field) for edit by click; none is sent now.

## Safeguards and abuse tests

See [T99](../threats/T99.md).

## Podman / Node

No / No (the master reads it for a chat without a folder)

## Depends

X.57

## Size

**M**
