# X.55 **sec** (T96)

## Adds

Word, Excel, PowerPoint and OpenDocument files count as documents (the owner, 2026-11-22): their text reaches the model as a PDF's does, in every chat, and the file is kept as well (a coding chat's folder, or `attached_file` in a plain chat whose agent has the switch) so a later row can create and edit such files.
The master starts its own binary as `rizzy-harness doc-text`, the PDF helper's twin (same confinement, memory cap, timeout, slots and output cap), which reads the zip with the bounded reader of X.54 and one XML pull parser (`quick-xml`) that expands no entity and opens nothing.
Output is plain text for a model: paragraphs, `# ` headings, `- ` list items, tables as ` | ` rows, per sheet `## Sheet: <name>` with tab-joined rows, per slide `## Slide n` and its notes.
A document is told by its parts, never by its name; encrypted and old binary files stay what they were; a document that cannot be read is a plain file with one sentence.
Claude.ai and Claude Code read these files by running code in a sandbox; a plain chat here has none, so the text is extracted out of process instead and arrives with the message.

## Safeguards and abuse tests

No event, link or schema change: the text is `user_message.attachments` as a PDF's is and the file is `uploads` or `attached` as X.53 and X.54 left them; the pairing is an id derived from the text's.
Tests: `master/src/doc/tests.rs` (formats, bombs, depth, DTD and entities, shared strings, rows, hostile text), `master/src/pdf.rs`, `master/src/attachments.rs`, `master/tests/it/doc_http.rs`, `master/tests/logs/doc_http.rs`, `cli/tests/doc_text.rs` (the real helper), `store/tests/chat_uploads.rs`, `web/src/components/composerfiles.test.tsx`, `conversation.test.tsx`, `web/e2e/doc-text.cjs`.

## Invariants

The master runs no shell, no file tool and no program the model names (a fixed subcommand of its own binary, bytes on stdin); the node decides what it saves; no secret in an event, a URL or a log line (nothing about the file is logged); the harness process stays light (the parser runs only in the helper); nothing the model writes is trusted (every string from the file is cleaned and quoted)

## Podman / Node

No / No

## Depends

X.53, X.54, FIX_PLAN 3.4

## Size

**M**
