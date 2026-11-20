# T.9

## Adds

**Built (2026-11-20, branch `feat/changed-files-warning`):** `RECOMMENDED_MAX_FILES_PER_TURN` in `policy.rs`.
The node writes the two chat notes (end of turn from the `after` checkpoint's list, after an undo from the files it wrote), as `error` events with `notice: true`, which the chat draws muted and not red.
Tests in `crates/node/tests/many_files.rs`.
Soft limit of 100 changed files per turn: warn at the end of the turn and on its undo; never block

## Safeguards and abuse tests, written first

One named constant (100); the count is `changed(before, after)` (nested repositories and ignored files not counted; a sub-agent's changes count toward the turn that applied them); the warning is the existing chat note and the undo plan's count, no new event.
Tests: 100 files warn nobody; 101 warn at the end of the turn and on its undo, and both complete normally; nested repositories are not counted. `docs/undo-and-stash.md` §2.6.

## Invariants

One writer

## Podman / Node

No / No

## Depends

C.13

## Size

**S**

