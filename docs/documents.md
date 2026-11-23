# Documents the agent makes (X.56, T97)

Design note for `doc_create` and `sheet_create`; the threat model is [T97](threats/T97.md) and the plan row [X.56](plan/X.56.md).
The editing tools that follow build on the same pieces.

## What was compared

- **Claude (Desktop, Code) and Codex** make these files by running code (python-docx, openpyxl, docx-js) in a sandbox and then show the file.
  That is the most flexible way and the least predictable: it needs an interpreter and a sandbox, and the results are as good as the code the model wrote that turn.
  A plain chat here has no sandbox, and the harness process must stay light, so the file is made by a library instead: the model writes Markdown, which it writes more reliably than any document JSON and which the chat already shows, so the preview and the file agree.
- **MCP office servers** take structured operations (add a paragraph, set a cell).
  That is the right shape for editing (a later row), and the wrong one for making a whole file: many calls for one document.
- **Rust crates**: `docx-rs` pulls an old `zip` and a serialisation stack for a job that is string templates; `rust_xlsxwriter` is the best xlsx writer around and brings `zip` in; `zip` had ten releases and seven advisories in a fortnight.
  The create path is five parts of XML of which one varies, so it is written by hand, with a zip writer in the same crate (the bounded reader of X.54 is in `harness-attached`); the single new dependency is `pulldown-cmark` (Markdown parsing, no HTML renderer).

## The pieces (`crates/docs`)

| Module | What |
|---|---|
| `zip` | `ZipWriter`: stored or deflate, CRC-32 and sizes, UTF-8 names, a fixed timestamp, names checked, the size counted as parts are added |
| `xml` | `clean` (what XML 1.0 forbids, bidirectional overrides), `text`, `attr`, `[Content_Types].xml`, `.rels`, core properties |
| `docx` | Markdown to the parts of a Word file; the walk is iterative; property blocks in schema order; one numbering per ordered list |
| `xlsx` | the sheet spec to a workbook: typed cells, a closed list of formats, ISO dates, inline strings, a frozen header, a filter, widths |
| `formula` | the deny-list and the tokeniser it reads with |
| `name` | the file name |
| `notes` | the sentences the model is told |
| `limits` | every limit, one place |

## Where the file goes

- A chat with a folder: the node's tool writes it through the write tool's path policy, approval and undo record, under a name that is free.
- A chat without a folder: the node's tool files a `docs.request` (content only); the master's service builds the file, keeps it as the owner's blob and answers with its id; the node's `tool_call_finished.made` names it, which is what makes it the chat's; the owner downloads it from `GET /api/sessions/{id}/made/{file_id}`.

## What is not here

Editing, images, headers and footers, charts, comments, `.odt`, `.ods`, `.pptx` creation and a rendering check (nothing here can show how Word draws the file; the tests read every file back with our own reader only).
