# X.60 **sec** (T100)

## Adds

A person changes the text of a Word or Excel file in the preview panel and saves a copy (the owner, 2026-11-23: "can we view/edit (no need for full suite) the documents in web app?", then "do both please"; this is the edit half, and a coding chat is the basic chat with more abilities, so it is one control with the same words in both kinds of chat).
A click on a paragraph, heading or list item of a Word file, or on a cell of the grid of an Excel file (an empty cell inside the shown grid too), makes it a text field with the text it has now (a formula cell: the formula with its `=`).
Enter or leaving the field keeps the change, Escape drops it, the focus goes back to the place; the places are buttons, reached by Tab, with a visible focus ring and an accessible name ("Paragraph 3", "Cell B7").
A changed place is marked, the panel says "N changes" and offers **Discard** and **Save a copy**; nothing is sent until Save, and leaving the panel or the sheet with changes asks once.
Save sends all the changes of the shown sheet in one request (changes are per sheet; there is no change set across sheets).
The original is never changed.
A chat without a folder gets the copy as a held attachment in the composer (as an upload is, under the same quotas), so it goes with the next message; a coding chat gets it written by the node beside the original as `<name> (edited).<ext>` (then `(edited) (2)` and so on; never over a file), with an `@` mention of it put in the message (`@"path"` when the path holds a space, which the composer now reads back).
Both say the same sentence: a copy named X was saved and added to the message; a coding chat's panel then shows the copy the node wrote, a plain chat's shows the changed text read-only (a held copy cannot be previewed before the message is sent).
If any change cannot be applied nothing is saved; the panel keeps the changes and says which place and why, in sentences of ours.
Not in this row, on purpose: formatting controls; adding or removing paragraphs, rows, columns or sheets; cells of a Word table; text the preview cut for size; calculation (a typed formula is stored and its value is not shown until Excel or the agent opens the file).
What changing a paragraph does: `set_text` keeps the first run's formatting and drops the other runs, so a paragraph of several formattings says so before Save.
What a typed cell means is decided once, in Rust (`harness_docs::edit::page::cell_value`): empty clears; `=` and a formula that passes the edit tools' formula check is a formula; a number by a strict rule (a sign, digits with a `.`, an exponent; no separators, spaces, hex or leading zeros; at most 15 digits; finite) is a number; anything else is text.
The engine part is one request of the confined `doc-edit` helper, `PageEdit { sheet, changes }`, turned into the existing `set_text` and `set` operations, so every check of theirs applies (stale tag, limits, formula check, hidden, merged, array and shared-formula cells, the package check of the output); at most 200 changes of at most 20,000 characters.
A chat without a folder: `POST /api/sessions/{id}/files/{file_id}/edit` (`crates/master/src/http/doc_page_edit.rs`).
A coding chat: `POST /api/sessions/{id}/folder-edit` (`folder_page_edit.rs`) and one typed request over the link, `session.doc_page_edit`, answered by one `session.doc_page_edit_result`; the capability `doc-page-edit` is announced only where the helper can start and node.toml lets the agent's own `doc_edit` or `sheet_edit` run.
Claude Desktop and Codex edit a document by asking the agent; none lets a person change a paragraph or a cell in the panel and keep the original; this does.
A place whose cleaned preview text differs from the file's (invisible, direction or private-use characters, an object, line breaks or runs of spaces in a cell) is not offered and the helper refuses it (`not_editable`); the request carries the kind the file's name says and the helper refuses a file of the other kind; a node whose approval ceiling is plan refuses and does not announce the capability; a copy's stem is cut before ` (edited)` is added.
Closing the panel, changing the sheet, pressing Preview on another file and moving to another chat or page each ask once, in the app's own dialog ("Keep editing" has the first focus and is Escape; "Discard changes" goes on), while changes are unsaved (`web/src/lib/unsavedGuard.ts`: the open panel puts the address back inside `popstate`, which comes before the router's `hashchange`, so the router reads the old address and stays (a listener of ours on `hashchange` cannot run first: same target, registration order), and Discard puts the new address back and replays `hashchange`; `gate` in `lib/docPreview.ts` asks for another file; the same file's Preview and a Save ask nothing); closing or reloading the tab keeps the browser's own `beforeunload` warning, which cannot show our sentence; Keep editing on a link, the sidebar or New chat leaves the chat's address as one more history entry, and on a back or forward press it overwrites the entry the person moved to; a crash loses the changes; a retyped date or percentage becomes text.
Out of scope: a rich-text field, a save across sheets, a download of a folder file, previewing a held copy, a stale-cell check for a workbook (a cell has no hash: a cell the agent changed since the preview is changed to what the person typed, in the copy only).

## Safeguards and abuse tests

See [T100](../threats/T100.md).

## Podman / Node

No / Yes (the node reads, runs the helper and writes the copy; the master only relays, or holds the copy of a chat without a folder)

## Depends

X.59, X.58, X.57, T98, T99

## Size

**L**
