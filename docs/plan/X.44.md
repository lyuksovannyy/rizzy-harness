# X.44 **sec** (T45)

## Adds

Editing by tag and row numbers (the owner, 2026-11-20: "make editing by hash then like in oh my pi")

## Safeguards and abuse tests

**Built (pull request 187).**
A second form of `edit` beside `old_string`: `{path, tag, lines}`; `read`, `edit` and `write` print `[path#TAG]`; the table keeps the last 4 versions with the rows shown; refusals print rows; a range lands only where its rows are unchanged and still next to each other (a seeded random test against a reference, `crates/tools/src/edit_fuzz_tests.rs`); the approval card shows the rows a line edit replaces (`Tool::preview`, `approval_requested.preview`).
Code and tests in `crates/tools/src/edit_lines.rs`; the threat model is in T45. `search` results carry tags (a pull request after 187: rows a search printed whole can be edited with no `read`; at most 16 files and 1 MiB per search; the `old_string` form's refusal no longer makes the file count as read); whether this becomes the only form is the owner's to decide (a live check with three Mistral models: the medium one used it, the small ones mostly did not).
Not taken from oh-my-pi: its patch language, registers, syntax anchors, move/delete, parser recovery.

## Invariants


## Podman / Node


## Depends


## Size


