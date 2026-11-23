# X.56 **sec** (T97)

## Adds

Agents make Word and Excel files (the owner, 2026-11-22: "make tools to work with documents, like making new, editing existing with proper formatting support for documents like docx, xls"; "available in chat and agentic sessions"): `doc_create` (Markdown to `.docx`: headings 1 to 4, bold, italic, strike, code, http, https and mailto links, nested lists, tables with alignment, quotes, code blocks, a rule as a page break, A4 or Letter, landscape, a title property) and `sheet_create` (a JSON sheet spec to `.xlsx`: typed cells, a closed list of formats, ISO dates as real dates, `{"f": …}` formulas without a cached value and a full calculation on open, a frozen bold header, autofilter, widths).
One per-agent switch for the family, "Create and edit documents" (`Features::office`), off by default and on for the built-in Assistant; this row is CREATE only, and the tools that change an existing file join the same switch.
In a coding chat the file is written into the folder through the write tool's path policy, approval, checkpoint and undo (a new name when one exists, never over a file); in a plain chat the master builds it, keeps it as the owner's blob and the person downloads it from a chip on the tool's block, and the agent reads it again with `attached_file`.
No docx or xlsx crate and no `zip` crate: `harness-docs` is string templates over a fixed style set, a zip writer beside the bounded reader and `pulldown-cmark` (the one new dependency).
Claude.ai and Codex make these files by running code in a sandbox; a plain chat here has none, so the library does it, with the same Markdown a chat shows so the file and the preview agree.

## Safeguards and abuse tests

`tool_call_finished.made` (additive) and the node message pair `docs.request` and `docs.result` with capability `doc-create`; no schema change (the rows of `session_files` of T95 carry the file; the log's line marks one logged).
Tests: `docs/src/tests.rs` (every file read back with the reader of T96; injection through every text field, formulas in every spelling, names, limits, nesting), `tools/src/office.rs`, `agent-native/tests/host_forwarding.rs`, `node/tests/docs.rs`, `master/tests/it/made_http.rs`, `store/tests/session_files.rs`, `agents/tests/agents.rs`, `web/src/components/toolcard.test.tsx`, `web/e2e/doc-create.cjs`.

## Invariants

The master runs no shell, no file tool and no program the model names (a fixed built-in service; text in is bounded and one build's memory is measured, up to about 60 MiB, T97); the node decides what it writes (the folder's own path policy and approvals; the file is the chat's only when its own log names it); chats are private to their owner (the download resolves the chat as opening it does); no secret in an event, a URL or a log line (the name and path are scrubbed, lengths only in logs); nothing the model writes is trusted (every string is cleaned and escaped, formulas are checked, a string is never a formula, raw HTML and images are dropped, an attachment never rendered)

## Podman / Node

No / Yes (the folder case is node code; a node announces `doc-create` where it offers a tool)

## Depends

X.54, X.55

## Size

**L**
