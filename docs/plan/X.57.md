# X.57 **sec** (T98)

## Adds

Agents edit existing Word and Excel files with their formatting kept (the owner, 2026-11-22: "editing existing with proper formatting support for documents like docx, xls"): `doc_view`, `doc_edit` and `sheet_edit`, in the same per-agent switch as the creating tools ("Create and edit documents", `Features::office`).
`doc_view` shows a Word file as tagged paragraphs and cells (`p12#a3f`, `t2r3c1#9bd`: an ordinal and a short hash of the visible text, so a stale edit is refused) with styles and list markers, or the rows of one sheet with A1 addresses, values, `=formula (not calculated)`, merged ranges, tables, shared and array formulas and named ranges; hidden content is left out; long files are paged.
`doc_edit` has `replace_text` (text split across runs is found; the first run's formatting stays), `set_text`, `insert_after` and `insert_before` (Markdown paragraphs, headings by the file's own style names, bold, italic, strike, code, list items that continue the anchor's list) and `delete`; `sheet_edit` has `set` (a cell keeps its style; text as inline strings; ISO dates only into date-formatted cells in the workbook's date system; formulas through the deny-list of T97), `append_rows` (optionally with the row above's cell styles) and `clear`.
All edits of a call apply or none, with a reason per edit; the result is a NEW file (`save_as` or `name (edited).ext`), never the input.
One part is patched by byte ranges and every other entry is copied as its stored bytes, so what the editor did not touch is the original's.
The file is parsed only in the confined `rizzy-harness doc-edit` helper (Landlock, memory cap, no core, supervised), started by the master for a plain chat and by the node for a folder chat.
Out of scope: tracked-changes output, comments, images, charts, number formats, adding sheets, row and column insertion, pptx, odt and ods, old `.doc` and `.xls`, recalculation, new lists, new hyperlinks.
Claude and Codex edit by running python-docx and openpyxl in a sandbox, which re-serialises the file through a model of it and drops what the model lacks (charts, content controls, custom XML); an editing MCP server does structured operations but not in the harness's chats; this patches the stored XML, which keeps unknown content as it was, and refuses what it cannot keep.

## Safeguards and abuse tests

See [T98](../threats/T98.md).
Round trips over files shaped like Word's and Excel's (split runs with revision ids, spelling marks, bookmarks, fields, links, tracked changes, content controls, alternate content, section breaks; shared strings, shared formulas, merges, a table part, a calculation chain, the 1904 date system) assert that every byte outside the edited elements is unchanged and every other entry is byte for byte the original; the helper's tests run the real binary against a bomb, a `DOCTYPE`, a non-UTF-8 part and a part at its ceiling.

## Podman / Node

No / Yes (a node announces `doc-create` where it offers any of the five tools; the folder case is node code and runs the helper of the node's own binary)

## Depends

X.55, X.56

## Size

**XL**
