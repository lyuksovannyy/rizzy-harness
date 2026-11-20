# C.2

## Decision

Fork by copy with lineage and an explicit head

## What the note settles

First event `SessionForked { parent, root, parent_node_id, through_seq, cut, lossy }`, then inherited `history_imported` batches, then a seed-ended marker. `through_seq` is the highest contiguous mirrored `seq`; default cut the last completed turn; an open turn is closed.
The child starts at `ask`, no grants, no allowance, zero counters. `session_forks` gains the hash at `through_seq`.
Same-node forks may seed files from the cut's checkpoint.
Explicit head only if B.6 says so. `05` §6.2; `06` §6 items 1, 2, 8.

## Tests it commits to

Lineage first; no unmirrored tail; non-owner gets 404; the child asks for what the parent had allowed; no fork during a running turn.

