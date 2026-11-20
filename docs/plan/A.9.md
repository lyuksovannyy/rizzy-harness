# A.9 **sec**

## Fixes

Exec 8; A14; missed 9

## Change

**Snapshot before a forced restore,** so the restore can be undone.
The new checkpoint phase is an in-place schema change, recorded in `PLAN.md`. `06` §6 item 3.

## Done when

Edit a file, force a restore, undo the restore: the edit is back.
**S**

## Lands on

gate3

