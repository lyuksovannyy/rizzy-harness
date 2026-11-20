# A.22 **sec**

## Fixes

First live test with real models (2026-11-15): two chats in one folder;
B edited a file inside A's turn and A's undo put it back silently

## Change

**An undo never silently reverts another chat's work** (PLAN.md T57).
Before writing, the node reads the other chats' logs in the overlapping folder from the undone span's start on: `edit`/`write` records and their undos are certain; other tool calls name the files their turn changed (by its checkpoints), background commands name every file.
Refused like the changed-since check, naming files only; "Undo anyway" stays and tells the other chat which of its files were put back.
Rejected: undoing only the files this turn's own records name (a turn's shell commands have no record, so it would leave the turn half undone), and one writer per folder (blocks two chats in one repository, the ordinary case).

## Done when

**Done (branch `fix/undo-other-chats`):** `crates/node/src/others.rs`; tests `crates/node/tests/undo_other_chats.rs` (the live scenario, a shell change, rewind, restart, privacy, no false refusal for reads or work outside the span; after the review: a deleted chat's work through its write tombstone, the undoing chat's own edit chain explaining a file, a broken chain still refused, the window starting at `turn_started`).

