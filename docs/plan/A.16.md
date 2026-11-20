# A.16 **sec**

## Fixes

B.2 (new, 2026-11-11)

## Change

**A restore is all or nothing, and a damaged checkpoint store repairs itself.**
B.2 showed that when an object a checkpoint borrowed from the user's repository is gone (history rewritten and pruned), a restore deletes the user's file and leaves the folder half-restored, and from then on every turn runs with no checkpoint.
So: before a restore writes anything it checks that every object it needs can be read, and refuses with the reason if not; a shadow index that refers to a missing object is rebuilt so later turns are checkpointed again.
Whether checkpoints keep borrowing at all is O11.

## Done when

B.2's tests assert the fixed behaviour: with a missing blob the restore is refused and the folder is byte-identical; the next turn has a checkpoint.
**M**

## Lands on

gate3, after A.15 (same files)

