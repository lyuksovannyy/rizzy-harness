# X.54 **sec** (T95)

## Adds

Every chat works with attached files (the owner, 2026-11-22): images, text and PDFs as before, and any other file (a zip, a binary) is readable, never changeable, in a plain chat through a tool the agent calls on demand.
`attached_file` has three read-only operations: `info` (name, size, type from the first bytes, sha256), `list` (a zip, tar, tar.gz or gz: paths, sizes and kinds, paged) and `read` (the file or one entry as numbered lines, a hex dump or printable runs).
The master keeps the file as the owner's blob and answers from a built-in service; nothing is written to any folder.
It is a per-agent switch, "Read attached files", on for the Assistant.
Claude.ai's code-execution uploads give the model a sandbox to open the file in; Claude Code has no upload; here a plain chat has no sandbox, so the agent gets a bounded reader instead.

## Safeguards and abuse tests

One event field, `user_message.attached` (additive; it is not `files`, which holds @-mentions), two link messages (`attached.request`, `attached.result`), one prompt field (`session.prompt.attached`) with the `attached-file` capability, one migration (5, `session_files` on the owner's blobs) and one crate (`harness-attached`).
Scope from the session's own log, the master runs no tool, work bounded in size, time and concurrency.
Tests: `attached/src/tests.rs`, `core/src/attached.rs`, `core/src/event.rs` (`attached_line_tests`), `tools/src/attached.rs`, `node/tests/attached.rs`, `master/tests/it/attached_http.rs`, `store/tests/session_files.rs`, `store/tests/migrations.rs`, `agents/tests/agents.rs`, `protocol/src/lib.rs`, `web/e2e/file-view.cjs`.

## Invariants

The master runs no shell, no file tool and no program the model names (a built-in service, scope from the session); the node decides; chats are private to their owner; no secret in an event; the harness process stays light (nothing held between calls)

## Podman / Node

No / Yes (`attached-file` capability)

## Depends

X.53, X.13

## Size

**L**
