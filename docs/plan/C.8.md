# C.8

## Decision

The `.git` residual

## What the note settles

Landlock only adds rights and a right covers the tree, so `.git` cannot be read-only in a read-write folder.
Host sessions: a named residual in `PLAN.md` with its real impact (our git ignores hooks and config; a planted hook fires when the user runs git).
Containers: evaluate read-only binds over `repo.git/hooks` and `repo.git/config` in N.4. `03` §6.3 item 1.

## Tests it commits to

If chosen, a container session cannot write `repo.git/hooks/post-checkout`.

