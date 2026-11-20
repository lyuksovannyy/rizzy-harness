# A.17 **sec**

## Fixes

B.2, B.3; C.13 (new, 2026-11-12)

## Change

**Undo does not depend on the user's repository keeping old content.**
At the end of each turn, copy the old versions of the files the turn changed (and the trees on their paths) into the shadow repository's own store as a kept pack, and keep them with the turn's checkpoint; `prune` deletes the pack with the checkpoint; no gc runs for it.
Objects already gone (an agent that pruned inside its own turn) are named in the chat as files that turn can no longer undo.
Same for a pre-restore checkpoint. `docs/undo-and-stash.md` §2.4.

## Done when

After a turn, history is rewritten and pruned in the user's repository and the undo of that turn still restores every file; the copy holds only the files the turn changed; nothing is written into the user's repository.
**S to M**

## Lands on

`main`

