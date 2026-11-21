# C.2

## Decision

Fork by copy with lineage and an explicit head

## What the note settles

First event `SessionForked { parent, root, parent_node_id, through_seq, cut, lossy }`, then inherited `history_imported` batches, then a seed-ended marker. `through_seq` is the highest contiguous mirrored `seq`; default cut the last completed turn; an open turn is closed.
The child starts at `ask`, no grants, no allowance, zero counters. `session_forks` gains the hash at `through_seq`.
Same-node forks may seed files from the cut's checkpoint.
Explicit head only if B.6 says so. `05` §6.2; `06` §6 items 1, 2, 8.

## What was built (part 1, X.48)

The same-node fork, built differently from the note's lossy seed: the child's log is the parent's events `2..=T` copied whole at the same `seq` (not `history_imported` batches), so every `seq` inside a payload stays right, the model's history for the fork is the parent's byte for byte, and no second code path reads it.
The first event is the parent's `session_created` with `forked {parent, through_seq}`; `session_forked {parent, through_seq, turn_id}` is the divider; the note's `root`, `parent_node_id`, `cut` and `lossy` are not needed (same node, whole copy).
A turn that is not finished is not a cut (it is refused, never closed); background commands at the cut are closed by `background_ended` with reason `forked`.
The child starts with no grants, and the approval mode is the parent's at the cut clamped by the node; usage starts at zero (a copy is skipped by the folds and the store).
`session_forks (session_id, parent_id, through_seq, created_ms)` is migration 3, with no hash column and no foreign key; deleting a parent deletes no fork.
Files are not seeded in part 1: both chats share the folder.
A cross-node fork stays with N.5 and the note's lossy seed.

## What was built (part 2: the fork's own folder)

The note said "same-node forks may seed files from the cut's checkpoint". Built as a clone of a sanitised copy of the parent's history files (the harness's own no-follow walk, then `git clone --no-local --no-checkout --template=` in the jail; git never opens the parent), in the node's projects folder, with the working files written from the cut's checkpoint and the index set to the branch's commit, so the parent's uncommitted work shows as uncommitted in the fork.
Not a `git worktree add`: it would run inside a repository an agent may have written (config, hooks), and the node is not sandboxed (PLAN.md T92 has the quoted documentation and the minimum git version).
The person chooses where the files live: `session.fork` carries `checkout: "own" | "shared"`; `session_created.forked.own_folder` records it; the node refuses an own copy with a sentence where it cannot and "the same folder" stays possible.
A fork's title is numbered per original chat: "X (fork)", "X (fork 2)".
Design: `docs/fork.md`.

## Tests it commits to

Lineage first; no unmirrored tail; non-owner gets 404; the child asks for what the parent had allowed; no fork during a running turn.

