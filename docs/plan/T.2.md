# T.2 **sec**

## Adds

`edit` staleness guard (C.9)

## Safeguards and abuse tests, written first

**Done (branch `feat/edit-quality`): `crates/tools/src/seen.rs` (the per-session table, the node-wide per-path lock), `crates/tools/src/edit.rs` (guard, ladder, batch, result), `crates/tools/src/fs.rs` (`read` records, `write` checks, the read window's character budget); `tool_call_finished.seen` (`FileSeen`: real path, SHA-256) is what the loop logs and `SeenStore::sync` rebuilds from at every turn;
PLAN.md T45.
Tests: `crates/tools/src/edit_tests.rs` (18 of them failed on the old code), `seen.rs`, `crates/agent-native/src/edit_guard_tests.rs` (restart, summary, deleted session).
C.9 decided here, from research 08 §6.1 and §6.2: `edit`, and `write` over an existing file, need a read in this session (any part) and an unchanged hash, Claude Code's rule (a "whole read" rule was dropped in review: past the read budget or with one long line it could never be met, F1, F2; a summary changes nothing, F7); a stale file is edited only when every replaced span lies outside the changed lines (common prefix and suffix against the snapshot, widened to whole lines), else refused with those lines as they are now (numbered, at most 40: the bounded diff); with no snapshot any change refuses with "read it again"; own writes re-record; batches match the original, disjoint, all or none; ladder exact (on an LF view), trailing whitespace on whole lines, then a one-for-one typographic fold on whole lines, a pasted `read` gutter stripped (from every line of `new_string` that has one), the typographic tier keeps the file's own characters where the model did not change them, the tier in the result; misses name all match lines, an already-made edit, or the nearest lines (OpenCode's vote).
Differs from C.9: no `dev:ino:size:mtime_ns:ctime_ns` token (the hash is read anyway, since `edit` loads the whole file; mtime adds nothing a hash misses), no ranges shown, snapshots in memory (512 KiB a file, 8 MiB per node, 4,096 entries per node, LRU; dropped with a deleted session) not on disk, the hash in `tool_call_finished` rather than in the `read` event alone (edits and writes need it too).
Not done: `cat`/`sed -n` in `bash` counting as a read (Claude Code does; no safe way without parsing command text), a third-miss escalation (Hermes), the web card shows `edits` with the generic fields.**
Was: `edit` without a read refused; stale with a changed span refused with a diff; identical span applied; `write` over an unread file refused; restart rebuilds; batch against the original; tier reported.

## Invariants

One writer; light process

## Podman / Node

No / No

## Depends

A.8, C.9, PR 7

## Size

**M**

