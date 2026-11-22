# X.53 **sec** (T94)

## Adds

Attach any file (the owner, 2026-11-21: "add ability to attach any files please", and for what the model cannot read, "Put it in the chat's folder").
Text, Markdown, code, CSV, JSON and PDF are read as text and images go to the model, as before; any other file, in a coding chat, is saved by the node into `attachments/` in the chat's folder and the agent is told its path; a plain chat refuses it with the reason.
Claude.ai's code-execution uploads work the same way (the file lands in the sandbox and the model is told where); Claude Code has no upload at all.

## Safeguards and abuse tests

One event field, `user_message.uploads` (additive), one link message (`session.upload`, acked, lane-ordered) and one prompt field with their capabilities; one migration (4, `chat_uploads` on the owner's blobs).
The node writes with `O_NOFOLLOW` directory handles and `RENAME_NOREPLACE`; the master stores and forwards; the node's `[limits] upload_max_mb` (50) and `uploads_per_message` (5) decide.
Tests: `node/src/uploads_tests.rs`, `node/tests/uploads.rs`, `master/tests/it/uploads_http.rs`, `store/tests/chat_uploads.rs`, `store/tests/migrations.rs`, `core/src/event.rs` (`upload_line_tests`), `protocol/src/lib.rs`, `protocol/tests/turn_wire.rs`, `web/e2e/attach-any.cjs`.

## Invariants

The node decides; the master runs no file tool; one writer per session; chats are private to their owner; the harness process stays light (a file is never held whole)

## Podman / Node

No / Yes (`uploads` capability)

## Depends

X.21, C.2

## Size

**M**
